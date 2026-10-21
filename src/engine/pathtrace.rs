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

    pub fn reset(&mut self) {
        self.frame = 0;
    }

    pub fn done(&self) -> bool {
        self.frame >= self.max_samples
    }

    /// One sample per pixel into `out` (the viewport HDR target).
    /// `sets`: per set slot (base, orm, normal, emissive) final maps.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        mut u: PtUniform,
        sets: &[[&Tex; 4]],
        env: &wgpu::TextureView,
        out: &Tex,
    ) {
        let (Some(nodes), Some(tris)) = (&self.nodes, &self.tris) else { return };
        let (w, h) = (out.width, out.height);
        if !self.accum.as_ref().is_some_and(|a| a.1 == w && a.2 == h) {
            let b = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("pt accum"),
                size: (w as u64 * h as u64 * 16).max(16),
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            self.accum = Some((b, w, h));
            self.frame = 0;
        }
        u.size = [w, h, self.frame, self.bounces];
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("pt uniform"),
            contents: bytemuck::bytes_of(&u),
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
    }
}
