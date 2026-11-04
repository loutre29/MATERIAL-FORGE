//! Progressive path-traced render mode: the mesh BVH and per-triangle
//! attributes uploaded to storage buffers, materials sampled from the texture
//! sets' final maps, one sample per pixel per frame.

use super::texture::Tex;
use crate::document::Document;
use wgpu::util::DeviceExt;

pub const MAX_SETS: usize = 8;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PtUniform {
    pub inv_view_proj: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub size: [u32; 4],
    pub env: [f32; 4],
    pub emi_a: [f32; 4],
    pub emi_b: [f32; 4],
    /// Alpha mode per set slot (0 opaque, 1 cutout, 2 blend).
    pub amode_a: [f32; 4],
    pub amode_b: [f32; 4],
    pub cut_a: [f32; 4],
    pub cut_b: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuTri {
    p: [[f32; 4]; 3],
    n: [[f32; 4]; 3],
    tng: [f32; 4],
    info: [u32; 4],
}

pub struct PathTracer {
    pipeline: wgpu::ComputePipeline,
    /// Denoiser guides (albedo, normal per pixel) and their size.
    dbufs: Option<(wgpu::Buffer, u32, u32)>,
    /// Denoise the displayed image with Open Image Denoise (the
    /// accumulation stays raw). Off, or before its first image: the noisy
    /// render shows.
    pub denoise_on: bool,
    /// Intel Open Image Denoise (`oidn.rs`): started on first use; Err when
    /// it cannot load (the render stays noisy).
    oidn: Option<Result<super::oidn::Denoiser, String>>,
    /// Identity of the current accumulation (reset bumps it): denoised
    /// images of older ones are dropped.
    accum_id: u64,
    /// A milestone was reached: read the accumulation back after submit.
    want_read: bool,
    last_submit: u32,
    /// The latest denoised image (OIDN) and the accumulation it belongs to.
    denoised: Option<(Tex, u64)>,
    /// The path tracer wrote the view this frame (the denoised image goes over it).
    wrote: bool,
    sampler: wgpu::Sampler,
    dummy: Tex,
    nodes: Option<wgpu::Buffer>,
    tris: Option<wgpu::Buffer>,
    accum: Option<(wgpu::Buffer, u32, u32)>,
    /// Samples accumulated so far.
    pub frame: u32,
    pub max_samples: u32,
    pub bounces: u32,
    /// Identity of the uploaded scene (document generation).
    pub scene: u64,
}

impl PathTracer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pathtrace.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/pathtrace.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("pathtrace"),
            layout: None,
            module: &module,
            entry_point: "main",
            compilation_options: Default::default(),
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let dummy = Tex::new(
            device,
            "pt dummy",
            1,
            1,
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        dummy.write_bytes(queue, &super::texture::f32_to_f16_bytes(&[0.5, 0.5, 0.5, 1.0]));
        Self {
            pipeline,
            dbufs: None,
            denoise_on: true,
            oidn: None,
            accum_id: 0,
            want_read: false,
            last_submit: 0,
            denoised: None,
            wrote: false,
            sampler,
            dummy,
            nodes: None,
            tris: None,
            accum: None,
            frame: 0,
            max_samples: 1024,
            bounces: 4,
            scene: u64::MAX,
        }
    }

    /// Uploads the document's BVH and triangle attributes.
    pub fn build_scene(&mut self, device: &wgpu::Device, doc: &Document, generation: u64) {
        let (nodes, ids) = doc.bvh.flatten();
        let set_of_sub: Vec<u32> = (0..doc.mesh.submeshes.len())
            .map(|sub| doc.sets.iter().position(|s| s.submesh == sub).filter(|i| *i < MAX_SETS).map(|i| i as u32).unwrap_or(255))
            .collect();
        let tris: Vec<GpuTri> = ids
            .iter()
            .map(|&id| {
                let (sub, tri) = doc.pick_map[id as usize];
                let idx = &doc.mesh.submeshes[sub as usize].indices[tri as usize * 3..tri as usize * 3 + 3];
                let v = |k: usize| &doc.mesh.vertices[idx[k] as usize];
                let mut t = GpuTri::default();
                let mut tsum = [0.0f32; 3];
                for k in 0..3 {
                    let vx = v(k);
                    t.p[k] = [vx.pos[0], vx.pos[1], vx.pos[2], vx.uv[0]];
                    t.n[k] = [vx.normal[0], vx.normal[1], vx.normal[2], vx.uv[1]];
                    for c in 0..3 {
                        tsum[c] += vx.tangent[c];
                    }
                }
                t.tng = [tsum[0], tsum[1], tsum[2], v(0).tangent[3]];
                t.info = [set_of_sub[sub as usize], 0, 0, 0];
                t
            })
            .collect();
        let buf = |label, data: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: data, usage: wgpu::BufferUsages::STORAGE })
        };
        let nodes = if nodes.is_empty() { vec![[0.0f32; 8]] } else { nodes };
        let tris = if tris.is_empty() { vec![GpuTri::default()] } else { tris };
        self.nodes = Some(buf("pt nodes", bytemuck::cast_slice(&nodes)));
        self.tris = Some(buf("pt tris", bytemuck::cast_slice(&tris)));
        self.scene = generation;
        self.frame = 0;
    }

    /// Fraction of each pixel's samples that met a surface (row major).
    pub fn coverage(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Vec<f32>> {
        let (aux, w, h) = self.dbufs.as_ref()?;
        let size = *w as u64 * *h as u64 * 32;
        let staging = device.create_buffer(&wgpu::BufferDescriptor { label: Some("coverage read"), size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(aux, 0, &staging, 0, size);
        queue.submit(Some(enc.finish()));
        let slice = staging.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::Maintain::Wait);
        let data: Vec<f32> = bytemuck::cast_slice::<u8, f32>(&slice.get_mapped_range()).to_vec();
        Some(data.chunks_exact(8).map(|c| c[7]).collect())
    }

    pub fn reset(&mut self) {
        self.frame = 0;
        self.accum_id += 1;
        self.last_submit = 0;
    }

    /// OIDN is loaded and running (started on the first call).
    fn oidn_ok(&mut self) -> bool {
        if self.oidn.is_none() {
            // MF_NO_OIDN=1: no denoiser (comparisons, tests).
            let r = if std::env::var("MF_NO_OIDN").is_ok() { Err("MF_NO_OIDN".to_string()) } else { super::oidn::Denoiser::start() };
            if let Err(e) = &r {
                log::warn!("Open Image Denoise unavailable ({e}): the render is not denoised");
            }
            self.oidn = Some(r);
        }
        matches!(&self.oidn, Some(Ok(d)) if d.alive())
    }

    /// Which denoiser the render mode uses.
    pub fn denoiser_name(&self) -> &'static str {
        match &self.oidn {
            _ if !self.denoise_on => "off",
            Some(Ok(d)) if d.alive() => "Intel Open Image Denoise",
            Some(_) => "none (Open Image Denoise not found)",
            None => "Intel Open Image Denoise",
        }
    }

    /// Open Image Denoise's state for the settings: its device, or why it
    /// is not used. Loads it on first call.
    pub fn oidn_status(&mut self) -> String {
        if !self.oidn_ok() {
            return match &self.oidn {
                Some(Err(e)) => format!("not available ({e}): the render stays noisy"),
                _ => "stopped: the render stays noisy".into(),
            };
        }
        match &self.oidn {
            Some(Ok(d)) => {
                let dev = d.device();
                if dev.is_empty() {
                    "starting...".into()
                } else {
                    format!("ready on the {dev}")
                }
            }
            _ => String::new(),
        }
    }

    /// A denoised image is on its way (keep presenting frames).
    pub fn denoise_pending(&self) -> bool {
        self.want_read || matches!(&self.oidn, Some(Ok(d)) if d.pending())
    }

    /// After the frame's commands were submitted: at a milestone, reads the
    /// accumulation and its guides back and hands them to OIDN.
    pub fn after_submit(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if !std::mem::take(&mut self.want_read) {
            return;
        }
        let (Some((accum, w, h)), Some((aux, ..))) = (&self.accum, &self.dbufs) else { return };
        let (w, h) = (*w, *h);
        let read = |buf: &wgpu::Buffer, size: u64| -> Vec<f32> {
            let staging = device.create_buffer(&wgpu::BufferDescriptor { label: Some("oidn read"), size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            let mut enc = device.create_command_encoder(&Default::default());
            enc.copy_buffer_to_buffer(buf, 0, &staging, 0, size);
            queue.submit(Some(enc.finish()));
            let slice = staging.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            device.poll(wgpu::Maintain::Wait);
            let v = bytemuck::cast_slice::<u8, f32>(&slice.get_mapped_range()).to_vec();
            v
        };
        let n = w as u64 * h as u64;
        let c = read(accum, n * 16);
        let a = read(aux, n * 32);
        let rgb = |v: &[f32], stride: usize, off: usize| -> Vec<f32> { v.chunks_exact(stride).flat_map(|p| [p[off], p[off + 1], p[off + 2]]).collect() };
        let job = super::oidn::Job {
            id: self.accum_id,
            width: w,
            height: h,
            color: rgb(&c, 4, 0),
            albedo: rgb(&a, 8, 0),
            normal: rgb(&a, 8, 4),
            final_quality: self.frame >= self.max_samples,
        };
        self.last_submit = self.frame;
        if let Some(Ok(d)) = &mut self.oidn {
            d.submit(job);
        }
    }

    /// Puts the latest denoised image (OIDN) over the view: a new one when it
    /// arrives, and again over each frame the path tracer wrote. True when
    /// the view changed.
    pub fn present(&mut self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, out: &Tex) -> bool {
        let wrote = std::mem::take(&mut self.wrote);
        if !self.denoise_on {
            return false;
        }
        let mut fresh = false;
        if let Some(Ok(d)) = &mut self.oidn {
            match d.poll() {
                Some(Ok(o)) if o.id == self.accum_id && o.width == out.width && o.height == out.height => {
                    let tex = match self.denoised.take() {
                        Some((t, _)) if t.width == o.width && t.height == o.height => t,
                        _ => Tex::new(device, "denoised", o.width, o.height, out.format, wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING),
                    };
                    // RGB f32 to RGBA f16 rows padded to the copy alignment.
                    let row = o.width as usize * 8;
                    let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
                    let mut bytes = vec![0u8; padded * o.height as usize];
                    for y in 0..o.height as usize {
                        for x in 0..o.width as usize {
                            let s = (y * o.width as usize + x) * 3;
                            let px = [o.color[s], o.color[s + 1], o.color[s + 2], 1.0f32].map(|v| half::f16::from_f32(v).to_bits());
                            let d = y * padded + x * 8;
                            bytes[d..d + 8].copy_from_slice(bytemuck::cast_slice(&px));
                        }
                    }
                    let staging = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("denoised upload"), contents: &bytes, usage: wgpu::BufferUsages::COPY_SRC });
                    enc.copy_buffer_to_texture(
                        wgpu::ImageCopyBuffer { buffer: &staging, layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(padded as u32), rows_per_image: Some(o.height) } },
                        wgpu::ImageCopyTexture { texture: &tex.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                        wgpu::Extent3d { width: o.width, height: o.height, depth_or_array_layers: 1 },
                    );
                    self.denoised = Some((tex, o.id));
                    fresh = true;
                }
                Some(Err(e)) => log::warn!("{e}"),
                _ => {}
            }
        }
        let current = self.denoised.as_ref().filter(|(t, id)| *id == self.accum_id && t.width == out.width && t.height == out.height);
        if let (Some((t, _)), true) = (current, fresh || wrote) {
            enc.copy_texture_to_texture(
                wgpu::ImageCopyTexture { texture: &t.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::ImageCopyTexture { texture: &out.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::Extent3d { width: out.width, height: out.height, depth_or_array_layers: 1 },
            );
            return true;
        }
        fresh
    }

    /// Offline renders (previews, checks): denoises the accumulation into
    /// `out` with OIDN and waits for it. False when OIDN is unavailable.
    pub fn denoise_now(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, out: &Tex) -> bool {
        if !self.oidn_ok() {
            return false;
        }
        self.want_read = true;
        self.after_submit(device, queue);
        let t = std::time::Instant::now();
        while t.elapsed().as_secs() < 120 {
            let mut enc = device.create_command_encoder(&Default::default());
            let done = self.present(device, &mut enc, out);
            queue.submit(Some(enc.finish()));
            if done {
                return true;
            }
            if !self.denoise_pending() {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    }

    pub fn done(&self) -> bool {
        self.frame >= self.max_samples
    }

    /// One sample per pixel into `out` (the viewport HDR target).
    /// `sets`: per set slot (base, orm, normal, emissive) final maps;
    /// `studio`: the packshot studio (off by default).
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        mut u: PtUniform,
        sets: &[[&Tex; 4]],
        env: &wgpu::TextureView,
        out: &Tex,
        studio: &crate::studio::StudioGpu,
    ) {
        // Open Image Denoise (started on first use).
        let oidn = self.denoise_on && self.oidn_ok();
        let (Some(nodes), Some(tris)) = (&self.nodes, &self.tris) else { return };
        let (w, h) = (out.width, out.height);
        if !self.accum.as_ref().is_some_and(|a| a.1 == w && a.2 == h) {
            let b = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("pt accum"),
                size: (w as u64 * h as u64 * 16).max(16),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            self.accum = Some((b, w, h));
            self.frame = 0;
            self.accum_id += 1;
            self.last_submit = 0;
        }
        if !self.dbufs.as_ref().is_some_and(|d| d.1 == w && d.2 == h) {
            let aux = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("pt aux"),
                size: (w as u64 * h as u64 * 32).max(16),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            self.dbufs = Some((aux, w, h));
        }
        u.size = [w, h, self.frame, self.bounces];
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("pt uniform"),
            contents: bytemuck::bytes_of(&u),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let sb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("pt studio"),
            contents: bytemuck::bytes_of(studio),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let views: Vec<wgpu::TextureView> = (0..4)
            .flat_map(|k| (0..MAX_SETS).map(move |s| (k, s)))
            .map(|(k, s)| sets.get(s).map(|t| t[k].mip_view(0)).unwrap_or_else(|| self.dummy.mip_view(0)))
            .collect();
        let out_view = out.mip_view(0);
        let mut entries = vec![
            wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: nodes.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: tris.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: self.accum.as_ref().unwrap().0.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&out_view) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(env) },
            wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(&self.sampler) },
        ];
        for (i, v) in views.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry { binding: 7 + i as u32, resource: wgpu::BindingResource::TextureView(v) });
        }
        let (aux, _, _) = self.dbufs.as_ref().unwrap();
        entries.push(wgpu::BindGroupEntry { binding: 39, resource: aux.as_entire_binding() });
        entries.push(wgpu::BindGroupEntry { binding: 40, resource: sb.as_entire_binding() });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pt bg"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut pass = enc.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
        drop(pass);
        self.frame += 1;
        self.wrote = true;
        // Open Image Denoise at milestones (and the last sample); the raw
        // render shows until its first image of this accumulation arrives.
        if oidn && self.frame != self.last_submit && (matches!(self.frame, 4 | 16 | 64 | 256 | 1024 | 4096) || self.frame >= self.max_samples) {
            self.want_read = true;
        }
    }
}
