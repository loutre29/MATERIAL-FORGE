use super::texture::Tex;
use super::viewport::vertex_layout;
use super::GpuMesh;
use std::collections::HashMap;

pub const LAYER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const OPAC_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
pub const MASK_PAINT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;
pub const MAX_STAMPS: usize = 2048;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Stamp {
    pub pos: [f32; 4],
    pub nrm: [f32; 4],
    pub ext: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BrushUniform {
    pub view_proj: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub viewport: [f32; 4],
    pub color: [f32; 4],
    pub values: [f32; 4],
    pub emissive: [f32; 4],
    pub normal: [f32; 4],
    pub params: [f32; 4],
    pub shape: [f32; 4],
    pub stencil: [f32; 4],
    pub stencil2: [f32; 4],
    pub right: [f32; 4],
    pub material: [f32; 4],
}

pub const MODE_CAMERA: f32 = 0.0;
pub const MODE_WRAP: f32 = 1.0;
pub const MODE_UV: f32 = 2.0;
pub const MODE_FULL: f32 = 3.0;
/// Gradients (stamps[0] holds the line): through the camera, in UV space.
pub const MODE_GRADIENT: f32 = 4.0;
pub const MODE_GRADIENT_UV: f32 = 5.0;

/// Which fragment entry point and target set a paint pass uses.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PaintPass {
    /// color, values, alphas
    PaintA,
    /// emissive, normal, opacity
    PaintB,
    EraseA,
    EraseB,
    Mask,
    MaskErase,
    /// The selection brush: adds to / subtracts from the selection (r8).
    Select,
    SelectErase,
}

pub struct PaintPipelines {
    shader: wgpu::ShaderModule,
    bgl: wgpu::BindGroupLayout,
    mat_bgl: wgpu::BindGroupLayout,
    /// Brush material evaluated over the active set (col, mrh, emi, nrm).
    pub material: Option<[Tex; 4]>,
    dummy: Tex,
    layout: wgpu::PipelineLayout,
    cache: HashMap<(PaintPass, u8), wgpu::RenderPipeline>,
    uniform: wgpu::Buffer,
    stamps: wgpu::Buffer,
    sampler: wgpu::Sampler,
    pub alpha: Tex,
    pub stencil: Tex,
    /// Texel rect the next draws are limited to (the stroke region).
    pub scissor: Option<[u32; 4]>,
    /// Selection of the painted texture set (None: everything paints).
    pub selection: Option<wgpu::TextureView>,
    no_selection: Tex,
}

fn blend(src: wgpu::BlendFactor, dst: wgpu::BlendFactor) -> wgpu::BlendComponent {
    wgpu::BlendComponent { src_factor: src, dst_factor: dst, operation: wgpu::BlendOperation::Add }
}

fn tex_entry(binding: u32, multisampled: bool, depth: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: if depth {
                wgpu::TextureSampleType::Depth
            } else {
                wgpu::TextureSampleType::Float { filterable: true }
            },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled,
        },
        count: None,
    }
}

pub fn image_texture(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, img: &image::RgbaImage, srgb: bool) -> Tex {
    let t = Tex::new(
        device,
        label,
        img.width(),
        img.height(),
        if srgb { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm },
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    t.write_bytes(queue, img.as_raw());
    t
}

impl PaintPipelines {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("paint.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/paint.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("brush bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(2, true, true),
                tex_entry(3, false, false),
                tex_entry(4, false, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                tex_entry(6, false, false),
            ],
        });
        let load_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let mat_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("brush material bgl"),
            entries: &[load_entry(0), load_entry(1), load_entry(2), load_entry(3)],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("paint layout"),
            bind_group_layouts: &[&bgl, &mat_bgl],
            push_constant_ranges: &[],
        });
        let dummy = Tex::new(device, "brush material dummy", 1, 1, LAYER_FORMAT, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        dummy.write_bytes(queue, &super::texture::f32_to_f16_bytes(&[0.0; 4]));
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("brush uniform"),
            size: std::mem::size_of::<BrushUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let stamps = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stamps"),
            size: (MAX_STAMPS * std::mem::size_of::<Stamp>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("brush sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
        let alpha = image_texture(device, queue, "brush alpha", &white, false);
        let stencil = image_texture(device, queue, "stencil", &white, true);
        let no_selection = super::select::shape_texture(device, queue, 1, 1, &[255]);
        Self {
            shader,
            bgl,
            mat_bgl,
            material: None,
            dummy,
            layout,
            cache: HashMap::new(),
            uniform,
            stamps,
            sampler,
            alpha,
            stencil,
            scissor: None,
            selection: None,
            no_selection,
        }
    }

    /// channels: base color, roughness, metallic, height, emissive, opacity, normal.
    fn pipeline(&mut self, device: &wgpu::Device, pass: PaintPass, channels: [bool; 7]) -> &wgpu::RenderPipeline {
        let bits = channels.iter().enumerate().fold(0u8, |acc, (i, &c)| acc | ((c as u8) << i));
        let key = (pass, if matches!(pass, PaintPass::Mask | PaintPass::MaskErase | PaintPass::Select | PaintPass::SelectErase) { 0 } else { bits });
        if !self.cache.contains_key(&key) {
            use wgpu::BlendFactor as B;
            use wgpu::ColorWrites as W;
            let on = |b: bool, w: W| if b { w } else { W::empty() };
            let erase = matches!(pass, PaintPass::EraseA | PaintPass::EraseB | PaintPass::MaskErase);
            let premul = wgpu::BlendState {
                color: blend(B::One, B::OneMinusSrcAlpha),
                alpha: if erase { blend(B::Zero, B::OneMinusSrcAlpha) } else { blend(B::One, B::OneMinusSrcAlpha) },
            };
            let keep_alpha = blend(B::Zero, B::One);
            let target = |format, blend, write_mask| Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask });
            let targets: Vec<Option<wgpu::ColorTargetState>> = match pass {
                PaintPass::PaintA | PaintPass::EraseA => {
                    let scalar = on(channels[1], W::RED) | on(channels[2], W::GREEN) | on(channels[3], W::BLUE);
                    vec![
                        target(LAYER_FORMAT, premul, on(channels[0], W::ALL)),
                        target(
                            LAYER_FORMAT,
                            wgpu::BlendState { color: blend(B::One, B::OneMinusSrcAlpha), alpha: keep_alpha },
                            scalar,
                        ),
                        target(
                            LAYER_FORMAT,
                            wgpu::BlendState {
                                color: if erase { blend(B::Zero, B::OneMinusSrc) } else { blend(B::One, B::OneMinusSrc) },
                                alpha: if erase { blend(B::Zero, B::OneMinusSrcAlpha) } else { blend(B::One, B::OneMinusSrcAlpha) },
                            },
                            scalar | on(channels[5], W::ALPHA),
                        ),
                    ]
                }
                PaintPass::PaintB | PaintPass::EraseB => vec![
                    target(LAYER_FORMAT, premul, on(channels[4], W::ALL)),
                    target(LAYER_FORMAT, premul, on(channels[6], W::ALL)),
                    target(
                        OPAC_FORMAT,
                        wgpu::BlendState { color: blend(B::One, B::OneMinusSrcAlpha), alpha: keep_alpha },
                        on(channels[5], W::RED),
                    ),
                ],
                PaintPass::Mask | PaintPass::MaskErase => vec![target(
                    MASK_PAINT_FORMAT,
                    wgpu::BlendState { color: blend(B::One, B::OneMinusSrcAlpha), alpha: keep_alpha },
                    W::RED | W::GREEN,
                )],
                PaintPass::Select => vec![target(super::select::SEL_FORMAT, wgpu::BlendState { color: blend(B::One, B::OneMinusSrc), alpha: blend(B::One, B::OneMinusSrc) }, W::ALL)],
                PaintPass::SelectErase => vec![target(super::select::SEL_FORMAT, wgpu::BlendState { color: blend(B::Zero, B::OneMinusSrc), alpha: blend(B::Zero, B::OneMinusSrc) }, W::ALL)],
            };
            let entry = match pass {
                PaintPass::PaintA => "fs_paint_a",
                PaintPass::PaintB => "fs_paint_b",
                PaintPass::EraseA => "fs_erase_a",
                PaintPass::EraseB => "fs_erase_b",
                PaintPass::Mask => "fs_mask",
                PaintPass::MaskErase => "fs_mask_erase",
                PaintPass::Select | PaintPass::SelectErase => "fs_select",
            };
            let p = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("paint"),
                layout: Some(&self.layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: "vs_uv",
                    compilation_options: Default::default(),
                    buffers: &[vertex_layout()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: entry,
                    compilation_options: Default::default(),
                    targets: &targets,
                }),
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });
            self.cache.insert(key, p);
        }
        &self.cache[&key]
    }

    pub fn upload(&self, queue: &wgpu::Queue, mut uniform: BrushUniform, stamps: &[Stamp]) {
        let stamps = &stamps[..stamps.len().min(MAX_STAMPS)];
        uniform.params[2] = stamps.len() as f32;
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        if !stamps.is_empty() {
            queue.write_buffer(&self.stamps, 0, bytemuck::cast_slice(stamps));
        }
    }

    /// Draws the uploaded stamps into `targets` with one pass. `subset` limits
    /// the triangles (polygon fill).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        mesh: &GpuMesh,
        submesh: usize,
        subset: Option<(&wgpu::Buffer, u32)>,
        targets: &[&Tex],
        scene_depth: &Tex,
        pass: PaintPass,
        channels: [bool; 7],
    ) {
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("brush bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.stamps.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&scene_depth.view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.alpha.view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.stencil.view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(self.selection.as_ref().unwrap_or(&self.no_selection.view)),
                },
            ],
        });
        let m = |i: usize| -> &wgpu::TextureView {
            match &self.material {
                Some(t) => &t[i].view,
                None => &self.dummy.view,
            }
        };
        let mat_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("brush material bg"),
            layout: &self.mat_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(m(0)) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(m(1)) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(m(2)) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(m(3)) },
            ],
        });
        let scissor = self.scissor;
        let pipeline = self.pipeline(device, pass, channels);
        let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = targets
            .iter()
            .map(|t| {
                Some(wgpu::RenderPassColorAttachment {
                    view: &t.view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })
            })
            .collect();
        let sub = &mesh.submeshes[submesh];
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stamp"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if let (Some(r), Some(t)) = (scissor, targets.first()) {
            let (x0, y0) = (r[0].min(t.width), r[1].min(t.height));
            let (x1, y1) = (r[2].min(t.width), r[3].min(t.height));
            if x1 <= x0 || y1 <= y0 {
                return;
            }
            rp.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
        }
        rp.set_pipeline(pipeline);
        rp.set_bind_group(0, &bg, &[]);
        rp.set_bind_group(1, &mat_bg, &[]);
        rp.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
        match subset {
            Some((buf, count)) => {
                rp.set_index_buffer(buf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..count, 0, 0..1);
            }
            None => {
                let (b, n) = sub.draw_indices();
                rp.set_index_buffer(b.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..n, 0, 0..1);
            }
        }
    }
}
