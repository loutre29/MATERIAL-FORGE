//! Selections: one r8 texture per texture set (UV space, 0 = not selected,
//! 1 = selected, soft in between). Shapes drawn in the 3D view (screen
//! space, projected through the camera) or the 2D view (UV space) are
//! rasterized into a coverage mask on the CPU, then merged into the
//! selection by `shaders/select.wgsl`. Paint passes multiply their weight
//! by the selection (`PaintPipelines::selection`).

use super::texture::Tex;
use super::viewport::vertex_layout;
use super::GpuMesh;
use wgpu::util::DeviceExt;

pub const SEL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
pub const SEL_USAGE: wgpu::TextureUsages = wgpu::TextureUsages::RENDER_ATTACHMENT
    .union(wgpu::TextureUsages::TEXTURE_BINDING)
    .union(wgpu::TextureUsages::COPY_SRC)
    .union(wgpu::TextureUsages::COPY_DST);

/// How a shape changes the selection (Photoshop's modes).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum SelOp {
    /// The shape becomes the selection.
    #[default]
    Replace,
    /// Shift: the shape is added.
    Add,
    /// Alt: the shape is removed.
    Subtract,
    /// Shift + Alt: only what is in both stays.
    Intersect,
}

impl SelOp {
    pub fn name(self) -> &'static str {
        match self {
            SelOp::Replace => "New selection",
            SelOp::Add => "Add to selection (Shift)",
            SelOp::Subtract => "Subtract from selection (Alt)",
            SelOp::Intersect => "Intersect with selection (Shift + Alt)",
        }
    }
}

/// Marching ants overlay (`shaders/ants.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct AntsUniform {
    /// x: dash phase (px), y: 2D view (1)
    pub k: [f32; 4],
    /// 2D view: uv -> ndc (as `uvview::ViewUniform`)
    pub xform: [f32; 4],
    pub offs: [f32; 4],
}

/// The selection shown with marching ants.
pub struct SelOverlay<'a> {
    pub submesh: usize,
    pub selection: &'a Tex,
    /// The set's nearest-covered texel map.
    pub nearest: &'a Tex,
    pub phase: f32,
}

pub fn ants_module(device: &wgpu::Device) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("ants.wgsl"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/ants.wgsl").into()),
    })
}

pub fn ants_bgl(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let frag = wgpu::ShaderStages::FRAGMENT;
    let tex = |binding, sample_type| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: frag,
        ty: wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("ants bgl"),
        entries: &[
            tex(0, wgpu::TextureSampleType::Float { filterable: false }),
            tex(1, wgpu::TextureSampleType::Uint),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::VERTEX | frag,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            },
        ],
    })
}

pub fn ants_bg(device: &wgpu::Device, bgl: &wgpu::BindGroupLayout, o: &SelOverlay, u: AntsUniform) -> wgpu::BindGroup {
    let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ants uniform"),
        contents: bytemuck::bytes_of(&u),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ants bg"),
        layout: bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&o.selection.view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&o.nearest.view) },
            wgpu::BindGroupEntry { binding: 2, resource: ub.as_entire_binding() },
        ],
    })
}

pub const SPACE_SCREEN: f32 = 0.0;
pub const SPACE_UV: f32 = 1.0;
pub const SPACE_ALL: f32 = 2.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SelectUniform {
    pub view_proj: [[f32; 4]; 4],
    /// width, height (physical px), near, far
    pub viewport: [f32; 4],
    /// space, visible only, orthographic, unused
    pub params: [f32; 4],
    /// mask uv = uv * xy + zw (SPACE_UV)
    pub uvx: [f32; 4],
    pub eye: [f32; 4],
}

pub struct SelectPipelines {
    bgl: wgpu::BindGroupLayout,
    /// Add (max), subtract, intersect.
    pipelines: [wgpu::RenderPipeline; 3],
    sampler: wgpu::Sampler,
    white: Tex,
}

/// A new, empty (nothing selected) selection texture.
pub fn new_selection(device: &wgpu::Device, queue: &wgpu::Queue, res: u32) -> Tex {
    let t = Tex::new(device, "selection", res, res, SEL_FORMAT, SEL_USAGE);
    t.write_bytes(queue, &vec![0u8; (res * res) as usize]);
    t
}

/// An r8 coverage mask from CPU bytes (row major, `w` x `h`).
pub fn shape_texture(device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32, data: &[u8]) -> Tex {
    let t = Tex::new(device, "selection shape", w, h, SEL_FORMAT, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
    t.write_bytes(queue, data);
    t
}

/// Coverage of a closed polygon (even-odd) over a `w` x `h` pixel grid,
/// 4 sub-rows per pixel with exact horizontal coverage, as bytes.
pub fn rasterize_polygon(points: &[[f32; 2]], w: u32, h: u32) -> Vec<u8> {
    const SUB: usize = 4;
    let (wu, hu) = (w as usize, h as usize);
    let mut acc = vec![0f32; wu];
    let mut out = vec![0u8; wu * hu];
    if points.len() < 3 {
        return out;
    }
    let ymin = points.iter().map(|p| p[1]).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
    let ymax = (points.iter().map(|p| p[1]).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(hu);
    let mut xs: Vec<f32> = Vec::new();
    for y in ymin..ymax {
        acc.iter_mut().for_each(|a| *a = 0.0);
        let mut touched = false;
        for s in 0..SUB {
            let sy = y as f32 + (s as f32 + 0.5) / SUB as f32;
            xs.clear();
            for i in 0..points.len() {
                let a = points[i];
                let b = points[(i + 1) % points.len()];
                if (a[1] <= sy) != (b[1] <= sy) {
                    xs.push(a[0] + (sy - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                }
            }
            xs.sort_by(|a, b| a.total_cmp(b));
            for pair in xs.chunks_exact(2) {
                let (x0, x1) = (pair[0].clamp(0.0, w as f32), pair[1].clamp(0.0, w as f32));
                if x1 <= x0 {
                    continue;
                }
                touched = true;
                let (i0, i1) = (x0 as usize, (x1.ceil() as usize).min(wu));
                for (i, a) in acc.iter_mut().enumerate().take(i1).skip(i0) {
                    let c = (x1.min(i as f32 + 1.0) - x0.max(i as f32)).max(0.0);
                    *a += c / SUB as f32;
                }
            }
        }
        if touched {
            for (o, a) in out[y * wu..(y + 1) * wu].iter_mut().zip(&acc) {
                *o = (a.min(1.0) * 255.0 + 0.5) as u8;
            }
        }
    }
    out
}

/// Polygon of an ellipse inscribed in the rect `a`..`b` (any corner order).
pub fn ellipse_points(a: [f32; 2], b: [f32; 2]) -> Vec<[f32; 2]> {
    let c = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let r = [(b[0] - a[0]).abs() * 0.5, (b[1] - a[1]).abs() * 0.5];
    let n = 160;
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            [c[0] + r[0] * t.cos(), c[1] + r[1] * t.sin()]
        })
        .collect()
}

pub fn rect_points(a: [f32; 2], b: [f32; 2]) -> Vec<[f32; 2]> {
    vec![a, [b[0], a[1]], b, [a[0], b[1]]]
}

impl SelectPipelines {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("select.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/select.wgsl").into()),
        });
        let frag = wgpu::ShaderStages::FRAGMENT;
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("select bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: frag,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: true,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 3, visibility: frag, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("select layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        use wgpu::BlendFactor as B;
        let comp = |src, dst, op| wgpu::BlendComponent { src_factor: src, dst_factor: dst, operation: op };
        let mk = |c: wgpu::BlendComponent| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("select"),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: "vs_uv", compilation_options: Default::default(), buffers: &[vertex_layout()] },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_select",
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: SEL_FORMAT,
                        blend: Some(wgpu::BlendState { color: c, alpha: c }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let pipelines = [
            mk(comp(B::One, B::One, wgpu::BlendOperation::Max)),
            mk(comp(B::Zero, B::OneMinusSrc, wgpu::BlendOperation::Add)),
            mk(comp(B::Zero, B::Src, wgpu::BlendOperation::Add)),
        ];
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("select sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = shape_texture(device, queue, 1, 1, &[255]);
        Self { bgl, pipelines, sampler, white }
    }

    /// Merges `shape` into `target` (a selection texture). `shape` None:
    /// every rasterized texel counts as covered (select all / faces with
    /// `subset`).
    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        &self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        mesh: &GpuMesh,
        submesh: usize,
        subset: Option<(&wgpu::Buffer, u32)>,
        target: &Tex,
        shape: Option<&Tex>,
        scene_depth: &Tex,
        u: SelectUniform,
        op: SelOp,
    ) {
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("select uniform"),
            contents: bytemuck::bytes_of(&u),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let shape = shape.unwrap_or(&self.white);
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("select bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&scene_depth.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&shape.view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        let load = if op == SelOp::Replace { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) } else { wgpu::LoadOp::Load };
        let pipeline = match op {
            SelOp::Replace | SelOp::Add => &self.pipelines[0],
            SelOp::Subtract => &self.pipelines[1],
            SelOp::Intersect => &self.pipelines[2],
        };
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("select"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
                resolve_target: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_pipeline(pipeline);
        rp.set_bind_group(0, &bg, &[]);
        rp.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
        match subset {
            Some((buf, count)) => {
                rp.set_index_buffer(buf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..count, 0, 0..1);
            }
            None => {
                let (b, n) = mesh.submeshes[submesh].draw_indices();
                rp.set_index_buffer(b.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..n, 0, 0..1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_coverage() {
        let m = rasterize_polygon(&rect_points([2.0, 2.0], [6.5, 6.0]), 10, 10);
        assert_eq!(m[3 * 10 + 3], 255);
        assert_eq!(m[3 * 10 + 6], 128);
        assert_eq!(m[3 * 10 + 7], 0);
        assert_eq!(m[1 * 10 + 3], 0);
    }
}
