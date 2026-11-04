//! GPU side: meshes, per-texture-set resources and the pipelines that paint,
//! composite and display them.

pub mod blit;
pub mod composite;
pub mod env;
pub mod gpubake;
pub mod oidn;
pub mod paint;
pub mod pathtrace;
pub mod procgen;
pub mod select;
pub mod texture;
pub mod thumbs;
pub mod uvtransfer;
pub mod uvview;
pub mod viewport;

use crate::mesh::MeshData;
use std::sync::Arc;
pub use texture::Tex;
use wgpu::util::DeviceExt;

pub struct GpuSubMesh {
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
    /// Unique edges as a line list (wireframe and the UV view).
    pub edge_buffer: wgpu::Buffer,
    pub edge_count: u32,
    /// Indices of the triangles left visible by the geometry mask.
    pub visible: Option<(wgpu::Buffer, u32)>,
    /// Alpha blended sets: the visible triangles sorted back to front for
    /// the current camera (blending is order dependent).
    pub sorted: Option<(wgpu::Buffer, u32)>,
}

impl GpuSubMesh {
    /// Index buffer to draw (respects the geometry mask and, when present,
    /// the back-to-front order).
    pub fn draw_indices(&self) -> (&wgpu::Buffer, u32) {
        match (&self.sorted, &self.visible) {
            (Some((b, n)), _) | (None, Some((b, n))) => (b, *n),
            (None, None) => (&self.index_buffer, self.index_count),
        }
    }
}

pub struct GpuMesh {
    pub vertex_buffer: wgpu::Buffer,
    pub submeshes: Vec<GpuSubMesh>,
    /// Bumped when the visible (not hidden) index buffers change.
    pub vis_rev: u64,
}

impl GpuMesh {
    pub fn new(device: &wgpu::Device, mesh: &MeshData) -> Self {
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vertices"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let submeshes = mesh
            .submeshes
            .iter()
            .map(|s| {
                // Edges of each polygon: an edge seen twice inside the same
                // polygon is a triangulation diagonal and is not drawn.
                let mut edges: std::collections::HashMap<(u32, u32), (u32, u32)> = std::collections::HashMap::new();
                for (ti, t) in s.indices.chunks_exact(3).enumerate() {
                    let poly = s.tri_polys.get(ti).copied().unwrap_or(u32::MAX);
                    for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                        let e = edges.entry(if a < b { (a, b) } else { (b, a) }).or_insert((poly, 0));
                        if e.0 == poly {
                            e.1 += 1;
                        } else {
                            e.1 = 1;
                        }
                    }
                }
                let lines: Vec<u32> = edges.into_iter().filter(|(_, (_, n))| *n == 1).flat_map(|((a, b), _)| [a, b]).collect();
                let empty = [0u32, 0];
                GpuSubMesh {
                    index_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("indices"),
                        contents: bytemuck::cast_slice(if s.indices.is_empty() { &empty[..] } else { &s.indices }),
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                    index_count: s.indices.len() as u32,
                    edge_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("edges"),
                        contents: bytemuck::cast_slice(if lines.is_empty() { &empty[..] } else { &lines }),
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                    edge_count: lines.len() as u32,
                    visible: None,
                    sorted: None,
                }
            })
            .collect();
        Self { vertex_buffer, submeshes, vis_rev: 0 }
    }
}

/// Per cell of `cell` texels: bounding rect of the texels whose nearest
/// covered texel lies in that cell.
fn dilation_cells(nearest: &[[u32; 2]], res: u32, cell: u32) -> Vec<[u32; 4]> {
    use rayon::prelude::*;
    let cpr = res.div_ceil(cell);
    let empty = || vec![[u32::MAX, u32::MAX, 0u32, 0u32]; (cpr * cpr) as usize];
    let rows = 64usize;
    nearest
        .par_chunks(res as usize * rows)
        .enumerate()
        .fold(empty, |mut acc, (k, chunk)| {
            for (j, n) in chunk.iter().enumerate() {
                let i = k * rows * res as usize + j;
                let (x, y) = (i as u32 % res, i as u32 / res);
                let c = &mut acc[((n[1] / cell).min(cpr - 1) * cpr + (n[0] / cell).min(cpr - 1)) as usize];
                c[0] = c[0].min(x);
                c[1] = c[1].min(y);
                c[2] = c[2].max(x + 1);
                c[3] = c[3].max(y + 1);
            }
            acc
        })
        .reduce(empty, |mut a, b| {
            for (a, b) in a.iter_mut().zip(b) {
                *a = [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])];
            }
            a
        })
}

/// Everything one texture set owns on the GPU besides its layers.
pub struct TextureSetGpu {
    /// Unique id (compositor caches are keyed by it).
    pub uid: u64,
    pub resolution: u32,
    /// (base color rgb, opacity)
    pub final_base: Tex,
    /// (ao, roughness, metallic, height)
    pub final_orm: Tex,
    /// tangent-space normal xyz
    pub final_normal: Tex,
    pub final_emissive: Tex,
    /// The optional channel groups (`document::EXT_NAMES` from the third,
    /// four per group): full size only while the set uses one of the group.
    pub final_ext: [Tex; 3],
    pub baked_ao: Tex,
    pub baked_curvature: Tex,
    pub baked_normal: Tex,
    pub baked_position: Tex,
    pub baked_wnormal: Tex,
    pub baked_thickness: Tex,
    pub baked_id: Tex,
    pub nearest: Tex,
    /// Evaluated mask of the selected layer (mask display mode).
    pub mask_view: Tex,
    /// Side of the dilation cells (64 texels, less for tiny sets).
    pub cell: u32,
    /// Per cell: bounding rect of the texels whose nearest covered texel
    /// lies in it (the finals a tile owning that cell writes).
    pub dilation: Vec<[u32; 4]>,
}

const MASK_VIEW_USAGE: wgpu::TextureUsages = wgpu::TextureUsages::TEXTURE_BINDING
    .union(wgpu::TextureUsages::COPY_DST)
    .union(wgpu::TextureUsages::COPY_SRC);

pub const BAKED_USAGE: wgpu::TextureUsages = wgpu::TextureUsages::TEXTURE_BINDING
    .union(wgpu::TextureUsages::COPY_DST)
    .union(wgpu::TextureUsages::COPY_SRC);

impl TextureSetGpu {
    pub fn new(engine: &Engine, res: u32, nearest: &[[u32; 2]], position: &[[f32; 4]], wnormal: &[[f32; 4]]) -> Self {
        let d = &engine.device;
        let q = &engine.queue;
        let final_usage = wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC;
        let rgba = wgpu::TextureFormat::Rgba16Float;
        let r16 = wgpu::TextureFormat::R16Float;
        let fin = |l: &str| Tex::with_mips(d, l, res, res, rgba, final_usage);
        // Mesh maps that are not baked yet are constants: one texel each
        // (the shaders clamp their reads). `alloc_baked` makes them full size.
        let filled = |l: &str, f: wgpu::TextureFormat, v: &[f32]| {
            let t = Tex::new(d, l, 1, 1, f, BAKED_USAGE);
            t.write_bytes(q, &texture::f32_to_f16_bytes(v));
            t
        };
        // 16 bits per coordinate are enough up to 64k textures.
        let nearest_tex = Tex::new(d, "nearest", res, res, wgpu::TextureFormat::Rg16Uint, BAKED_USAGE);
        let packed: Vec<u16> = {
            use rayon::prelude::*;
            nearest.par_iter().flat_map_iter(|n| [n[0] as u16, n[1] as u16]).collect()
        };
        nearest_tex.write_bytes(q, bytemuck::cast_slice(&packed));
        let baked_position = Tex::new(d, "baked position", res, res, rgba, BAKED_USAGE);
        baked_position.write_bytes(q, &texture::f32_to_f16_bytes(bytemuck::cast_slice(position)));
        let baked_wnormal = Tex::new(d, "baked world normal", res, res, rgba, BAKED_USAGE);
        baked_wnormal.write_bytes(q, &texture::f32_to_f16_bytes(bytemuck::cast_slice(wnormal)));
        // Full size only while the mask display needs it (`ensure_mask_view`).
        let mask_view = Tex::new(d, "mask view", 1, 1, wgpu::TextureFormat::R32Float, MASK_VIEW_USAGE);
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let cell = 64.min(res).max(1);
        let dilation = dilation_cells(nearest, res, cell);
        Self {
            uid: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            resolution: res,
            final_base: fin("final base"),
            final_orm: fin("final orm"),
            final_normal: fin("final normal"),
            final_emissive: fin("final emissive"),
            final_ext: std::array::from_fn(|_| Tex::with_mips(d, "final optional", 1, 1, rgba, final_usage)),
            baked_ao: filled("baked ao", r16, &[1.0]),
            baked_curvature: filled("baked curvature", r16, &[0.5]),
            baked_normal: filled("baked normal", rgba, &[0.0, 0.0, 1.0, 1.0]),
            baked_position,
            baked_wnormal,
            baked_thickness: filled("baked thickness", r16, &[0.5]),
            baked_id: filled("baked id", rgba, &[0.0, 0.0, 0.0, 1.0]),
            nearest: nearest_tex,
            mask_view,
            cell,
            dilation,
        }
    }

    /// Some optional channel group is in use (its accumulators are needed).
    pub fn ext_on(&self) -> bool {
        self.final_ext.iter().any(|t| t.width > 1)
    }

    /// Sizes the optional groups' finals: full size for the groups in use,
    /// one texel otherwise. True when something changed (recomposite all).
    pub fn ensure_ext(&mut self, device: &wgpu::Device, used: [bool; 3]) -> bool {
        let usage = wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC;
        let mut changed = false;
        for (t, on) in self.final_ext.iter_mut().zip(used) {
            let size = if on { self.resolution } else { 1 };
            if t.width != size {
                *t = Tex::with_mips(device, "final optional", size, size, wgpu::TextureFormat::Rgba16Float, usage);
                changed = true;
            }
        }
        changed
    }

    /// Texels whose finals are computed from `inner` (cell aligned).
    pub fn dilation_rect(&self, inner: [u32; 4]) -> Option<[u32; 4]> {
        let cpr = self.resolution.div_ceil(self.cell);
        let mut out: Option<[u32; 4]> = None;
        for cy in inner[1] / self.cell..inner[3].div_ceil(self.cell).min(cpr) {
            for cx in inner[0] / self.cell..inner[2].div_ceil(self.cell).min(cpr) {
                let c = self.dilation[(cy * cpr + cx) as usize];
                if c[2] <= c[0] {
                    continue;
                }
                out = Some(match out {
                    Some(o) => [o[0].min(c[0]), o[1].min(c[1]), o[2].max(c[2]), o[3].max(c[3])],
                    None => c,
                });
            }
        }
        out
    }

    /// Allocates the mask display texture; true when it was just created (the
    /// set needs a composite to fill it).
    pub fn ensure_mask_view(&mut self, device: &wgpu::Device) -> bool {
        if self.mask_view.width == self.resolution {
            return false;
        }
        let res = self.resolution;
        self.mask_view = Tex::new(device, "mask view", res, res, wgpu::TextureFormat::R32Float, MASK_VIEW_USAGE);
        true
    }

    /// Sets baked mesh map `i` (the order of the set settings' MESH MAPS:
    /// normal, world normal, ID, AO, curvature, position, thickness) to the
    /// constant of an unbaked map. Position and world normal place the
    /// tri-planar projections: they are never cleared.
    pub fn clear_map(&self, q: &wgpu::Queue, i: usize) {
        let (t, v): (&Tex, &[f32]) = match i {
            0 => (&self.baked_normal, &[0.0, 0.0, 1.0, 1.0]),
            2 => (&self.baked_id, &[0.0, 0.0, 0.0, 1.0]),
            3 => (&self.baked_ao, &[1.0]),
            4 => (&self.baked_curvature, &[0.5]),
            6 => (&self.baked_thickness, &[0.5]),
            _ => return,
        };
        let n = (t.width * t.height) as usize;
        let data: Vec<f32> = (0..n).flat_map(|_| v.iter().copied()).collect();
        t.write_bytes(q, &texture::f32_to_f16_bytes(&data));
    }

    /// Gives the baked mesh maps their full size (before writing a bake).
    /// With a queue, maps that grow keep their constant value (callers that
    /// write only some of them); without, they start zeroed.
    pub fn alloc_baked(&mut self, device: &wgpu::Device, keep: Option<&wgpu::Queue>) {
        let res = self.resolution;
        let n = (res * res) as usize;
        let defaults: [&[f32]; 5] = [&[1.0], &[0.5], &[0.0, 0.0, 1.0, 1.0], &[0.5], &[0.0, 0.0, 0.0, 1.0]];
        for (t, v) in [&mut self.baked_ao, &mut self.baked_curvature, &mut self.baked_normal, &mut self.baked_thickness, &mut self.baked_id]
            .into_iter()
            .zip(defaults)
        {
            if t.width != res {
                *t = Tex::new(device, "baked map", res, res, t.format, BAKED_USAGE);
                if let Some(q) = keep {
                    let data: Vec<f32> = (0..n).flat_map(|_| v.iter().copied()).collect();
                    t.write_bytes(q, &texture::f32_to_f16_bytes(&data));
                }
            }
        }
    }

    pub fn byte_size(&self) -> u64 {
        [
            &self.final_base,
            &self.final_orm,
            &self.final_normal,
            &self.final_emissive,
            &self.final_ext[0],
            &self.final_ext[1],
            &self.final_ext[2],
            &self.baked_ao,
            &self.baked_curvature,
            &self.baked_normal,
            &self.baked_position,
            &self.baked_wnormal,
            &self.baked_thickness,
            &self.baked_id,
            &self.nearest,
            &self.mask_view,
        ]
        .iter()
        .map(|t| t.byte_size())
        .sum()
    }
}

pub struct Engine {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub adapter_name: String,
    pub env: env::Environment,
    pub viewport: viewport::Viewport,
    pub uvview: uvview::UvView,
    pub paint: paint::PaintPipelines,
    pub compositor: composite::Compositor,
    pub blitter: blit::Blitter,
    pub thumbs: thumbs::Thumbnailer,
    pub pathtracer: pathtrace::PathTracer,
    pub procgen: procgen::ProcGen,
    pub select: select::SelectPipelines,
}

/// Features this app uses when the adapter has them.
pub fn wanted_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    adapter.features() & (wgpu::Features::FLOAT32_FILTERABLE)
}

/// The adapter's full limits (the compositor binds many textures per pass).
pub fn wanted_limits(adapter: &wgpu::Adapter) -> wgpu::Limits {
    adapter.limits()
}

impl Engine {
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>, adapter_name: String) -> Self {
        let env = env::Environment::studio(&device, &queue);
        let viewport = viewport::Viewport::new(&device, &queue, &env);
        let uvview = uvview::UvView::new(&device);
        let paint = paint::PaintPipelines::new(&device, &queue);
        let compositor = composite::Compositor::new(&device, &queue);
        let blitter = blit::Blitter::new(&device);
        let thumbs = thumbs::Thumbnailer::new(&device);
        let pathtracer = pathtrace::PathTracer::new(&device, &queue);
        let procgen = procgen::ProcGen::new(&device);
        let select = select::SelectPipelines::new(&device, &queue);
        Self { device, queue, adapter_name, env, viewport, uvview, paint, compositor, blitter, thumbs, pathtracer, procgen, select }
    }

    pub fn set_environment(&mut self, env: env::Environment) {
        self.viewport.set_environment(&self.device, &env);
        self.env = env;
    }

    /// Headless device for tests and command-line tools.
    pub fn headless() -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| anyhow::anyhow!("no GPU adapter found"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("headless"),
                required_features: wanted_features(&adapter),
                required_limits: wanted_limits(&adapter),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))?;
        Ok(Self::new(Arc::new(device), Arc::new(queue), adapter.get_info().name))
    }
}
