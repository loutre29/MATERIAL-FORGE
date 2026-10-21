//! Procedural textures like Painter's: each generator (Perlin noise,
//! Clouds, Cells, grunge maps, patterns...) is rendered once into a
//! seamless grayscale texture with mips, then fills and mask fills sample
//! it like any bitmap (UV, planar or tri-planar). Textures are cached by
//! their parameters and rendered again only when those change.

use super::texture::Tex;
use bytemuck::{Pod, Zeroable};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// Side of the generated textures.
pub const PROC_RES: u32 = 1024;
/// Generated textures kept around (least recently used ones go first).
const CACHE_MAX: usize = 48;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct GenU {
    /// family, variant, resolution, unused
    pub k: [u32; 4],
    /// seed, balance, contrast, invert
    pub a: [f32; 4],
    /// the generator's own parameters
    pub p0: [f32; 4],
    pub p1: [f32; 4],
    /// hidden constants of the variant
    pub v: [f32; 4],
}

impl GenU {
    pub fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bytemuck::bytes_of(self).hash(&mut h);
        h.finish()
    }
}

pub struct ProcGen {
    gen: wgpu::ComputePipeline,
    mip: wgpu::ComputePipeline,
    cache: RefCell<HashMap<u64, (Arc<Tex>, u64)>>,
    clock: RefCell<u64>,
}

fn pipeline(device: &wgpu::Device, label: &str, code: &str, entry: &str) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(code.into()) });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: None,
        module: &module,
        entry_point: entry,
        compilation_options: Default::default(),
        cache: None,
    })
}

impl ProcGen {
    pub fn new(device: &wgpu::Device) -> Self {
        let code = format!("{}\n{}", include_str!("../shaders/common.wgsl"), include_str!("../shaders/patterns.wgsl"));
        ProcGen {
            gen: pipeline(device, "procedural generate", &code, "generate"),
            mip: pipeline(device, "procedural mip", &code, "mip"),
            cache: RefCell::new(HashMap::new()),
            clock: RefCell::new(0),
        }
    }

    /// The texture of `u`, rendered into `enc` first if it is not cached.
    pub fn get(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, u: &GenU) -> Arc<Tex> {
        let key = u.key();
        let now = {
            let mut c = self.clock.borrow_mut();
            *c += 1;
            *c
        };
        if let Some((t, used)) = self.cache.borrow_mut().get_mut(&key) {
            *used = now;
            return t.clone();
        }
        // The caller picks the size (k.z): thumbnails small, layers larger.
        let res = if u.k[2] > 0 { u.k[2] } else { PROC_RES };
        let tex = Arc::new(Tex::with_mips(
            device,
            "procedural",
            res,
            res,
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        ));
        let gu = *u;
        let ub = {
            use wgpu::util::DeviceExt;
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("procedural params"), contents: bytemuck::bytes_of(&gu), usage: wgpu::BufferUsages::UNIFORM })
        };
        let run = |enc: &mut wgpu::CommandEncoder, p: &wgpu::ComputePipeline, entries: &[wgpu::BindGroupEntry], size: u32| {
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p.get_bind_group_layout(0), entries });
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("procedural"), timestamp_writes: None });
            pass.set_pipeline(p);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(size.div_ceil(8), size.div_ceil(8), 1);
        };
        let v0 = tex.mip_view(0);
        run(enc, &self.gen, &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&v0) }], res);
        for level in 1..tex.mips {
            let (src, dst) = (tex.mip_view(level - 1), tex.mip_view(level));
            run(
                enc,
                &self.mip,
                &[wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&src) }, wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&dst) }],
                (res >> level).max(1),
            );
        }
        let mut cache = self.cache.borrow_mut();
        // Evict the least recently used textures nobody holds any more.
        while cache.len() >= CACHE_MAX {
            let old = cache.iter().filter(|(_, (t, _))| Arc::strong_count(t) == 1).min_by_key(|(_, (_, used))| *used).map(|(k, _)| *k);
            match old {
                Some(k) => {
                    cache.remove(&k);
                }
                None => break,
            }
        }
        cache.insert(key, (tex.clone(), now));
        tex
    }

    /// Generation size for a texture set of `set_res` texels.
    pub fn res_for(set_res: u32) -> u32 {
        (set_res / 2).clamp(256, PROC_RES)
    }

    /// Drops the textures no layer uses (resolution changes, project close).
    pub fn trim(&self) {
        self.cache.borrow_mut().retain(|_, (t, _)| Arc::strong_count(t) > 1);
    }
}
