//! 3D viewport: shadow map, HDR MSAA scene (environment, PBR meshes, alpha
//! blended sets, wireframe), bloom chain and the tonemapping post pass into
//! the sRGB texture shown by egui.

use super::env::Environment;
use super::texture::Tex;
use super::GpuMesh;
use crate::mesh::Vertex;
use wgpu::util::DeviceExt;

pub const OUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const MSAA: u32 = 4;
const SHADOW_SIZE: u32 = 2048;
const BLOOM_LEVELS: usize = 5;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
    pub inv_view_proj: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub env: [f32; 4],
    pub display: [f32; 4],
    pub sh: [[f32; 4]; 9],
    pub light_view_proj: [[f32; 4]; 4],
    pub light: [f32; 4],
    /// Wireframe color (linear rgb) and opacity.
    pub wire: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SetUniform {
    pub k: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PostUniform {
    pub a: [f32; 4],
    pub b: [f32; 4],
    pub texel: [f32; 4],
}

/// One texture set to draw.
pub struct DrawSet<'a> {
    pub submesh: usize,
    pub base: &'a Tex,
    pub orm: &'a Tex,
    pub normal: &'a Tex,
    pub emissive: &'a Tex,
    pub display: &'a Tex,
    pub params: SetUniform,
    pub visible: bool,
    /// The set has the transmission channel: it sees the scene behind it.
    pub transmissive: bool,
}

pub struct Frame<'a> {
    pub camera: CameraUniform,
    pub post: PostUniform,
    pub mesh: Option<&'a GpuMesh>,
    pub sets: Vec<DrawSet<'a>>,
    pub shadows: bool,
    pub wireframe: bool,
    /// Wireframe of this submesh only (polygon fill shows the active set).
    pub wire_only: Option<usize>,
    /// Marching ants around the active set's selection.
    pub selection: Option<super::select::SelOverlay<'a>>,
    /// The HDR target already holds a path-traced image: post-process only.
    pub path_traced: bool,
}

fn tv(binding: u32, t: &Tex) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(&t.view) }
}

pub fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRS: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4];
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRS,
    }
}

struct Targets {
    hdr_ms: Tex,
    hdr: Tex,
    /// Copy of the opaque scene for transmissive sets, and its depth.
    scene: Tex,
    scene_depth: Tex,
    /// Back faces of transmissive sets: normal + eye distance, own depth.
    back: Tex,
    back_depth: Tex,
    depth: Tex,
    out: Tex,
    bloom: Vec<Tex>,
}

pub struct Viewport {
    pub width: u32,
    pub height: u32,
    targets: Targets,
    shadow: Tex,
    cam_buf: wgpu::Buffer,
    cam_bgl: wgpu::BindGroupLayout,
    mat_bgl: wgpu::BindGroupLayout,
    post_bgl: wgpu::BindGroupLayout,
    cam_bg: wgpu::BindGroup,
    /// Camera uniform only: the shadow pass cannot bind the shadow map it writes.
    shadow_bg: wgpu::BindGroup,
    env_sampler: wgpu::Sampler,
    shadow_sampler: wgpu::Sampler,
    mat_sampler: wgpu::Sampler,
    post_sampler: wgpu::Sampler,
    bg_pipeline: wgpu::RenderPipeline,
    mesh_pipeline: wgpu::RenderPipeline,
    blend_pipeline: wgpu::RenderPipeline,
    /// Transmissive sets: their back faces' normals and distances, then
    /// their front faces refracting through them over a copy of the scene.
    backface_pipeline: wgpu::RenderPipeline,
    transmit_front: wgpu::RenderPipeline,
    /// MSAA depth to the R32 depth the transmission reads.
    depth_resolve: wgpu::ComputePipeline,
    scene_bgl: wgpu::BindGroupLayout,
    /// Depth of the opaque texels of alpha-blended sets, drawn before them
    /// so their surfaces keep their order.
    prepass_pipeline: wgpu::RenderPipeline,
    wire_pipeline: wgpu::RenderPipeline,
    ants_bgl: wgpu::BindGroupLayout,
    ants_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    bright_pipeline: wgpu::RenderPipeline,
    down_pipeline: wgpu::RenderPipeline,
    up_pipeline: wgpu::RenderPipeline,
    final_pipeline: wgpu::RenderPipeline,
}

/// The scene pass: MSAA color resolved into `hdr`, MSAA depth; `clear`
/// starts a frame, otherwise it continues the previous pass.
fn scene_pass<'e>(enc: &'e mut wgpu::CommandEncoder, t: &'e Targets, label: &'static str, clear: bool) -> wgpu::RenderPass<'e> {
    enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &t.hdr_ms.view,
            resolve_target: Some(&t.hdr.view),
            ops: wgpu::Operations {
                load: if clear { wgpu::LoadOp::Clear(wgpu::Color::BLACK) } else { wgpu::LoadOp::Load },
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &t.depth.view,
            depth_ops: Some(wgpu::Operations {
                load: if clear { wgpu::LoadOp::Clear(1.0) } else { wgpu::LoadOp::Load },
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    })
}

fn make_targets(device: &wgpu::Device, w: u32, h: u32) -> Targets {
    let rt = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let hdr_ms = Tex::with_samples(device, "hdr msaa", w, h, HDR_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT, MSAA);
    let hdr = Tex::new(device, "hdr", w, h, HDR_FORMAT, rt | wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST);
    let scene = Tex::new(device, "scene copy", w, h, HDR_FORMAT, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
    let scene_depth = Tex::new(device, "scene depth", w, h, wgpu::TextureFormat::R32Float, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING);
    let back = Tex::new(device, "transmission back faces", w, h, wgpu::TextureFormat::Rgba32Float, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT);
    let back_depth = Tex::new(device, "transmission back depth", w, h, DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
    let depth = Tex::with_samples(device, "viewport depth", w, h, DEPTH_FORMAT, rt, MSAA);
    let out = Tex::new(device, "viewport out", w, h, OUT_FORMAT, rt | wgpu::TextureUsages::COPY_SRC);
    let mut bloom = Vec::new();
    let (mut bw, mut bh) = (w, h);
    for _ in 0..BLOOM_LEVELS {
        bw = (bw / 2).max(1);
        bh = (bh / 2).max(1);
        bloom.push(Tex::new(device, "bloom", bw, bh, HDR_FORMAT, rt));
    }
    Targets { hdr_ms, hdr, scene, scene_depth, back, back_depth, depth, out, bloom }
}

impl Viewport {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, env: &Environment) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/viewport.wgsl").into()),
        });
        let post = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/post.wgsl").into()),
        });
        let frag = wgpu::ShaderStages::FRAGMENT;
        let tex_entry = |binding, vis| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let samp_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: frag,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let uni_entry = |binding, vis| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let cam_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera bgl"),
            entries: &[
                uni_entry(0, wgpu::ShaderStages::VERTEX | frag),
                tex_entry(1, frag),
                samp_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: frag,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                tex_entry(5, frag),
            ],
        });
        let mat_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material bgl"),
            entries: &[
                tex_entry(0, frag),
                tex_entry(1, frag),
                tex_entry(2, frag),
                tex_entry(3, frag),
                tex_entry(4, frag),
                samp_entry(5),
                uni_entry(6, frag),
            ],
        });
        let scene_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene bgl"),
            entries: &[
                tex_entry(0, frag),
                samp_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let transmit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("transmit layout"),
            bind_group_layouts: &[&cam_bgl, &mat_bgl, &scene_bgl],
            push_constant_ranges: &[],
        });
        let post_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post bgl"),
            entries: &[uni_entry(0, frag), tex_entry(1, frag), tex_entry(2, frag), samp_entry(3)],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport layout"),
            bind_group_layouts: &[&cam_bgl, &mat_bgl],
            push_constant_ranges: &[],
        });
        let cam_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("camera-only layout"),
            bind_group_layouts: &[&cam_bgl],
            push_constant_ranges: &[],
        });
        let shadow_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow bgl"),
            entries: &[uni_entry(0, wgpu::ShaderStages::VERTEX | frag)],
        });
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow layout"),
            bind_group_layouts: &[&shadow_bgl],
            push_constant_ranges: &[],
        });
        let post_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post layout"),
            bind_group_layouts: &[&post_bgl],
            push_constant_ranges: &[],
        });
        let ms = wgpu::MultisampleState { count: MSAA, mask: !0, alpha_to_coverage_enabled: false };
        let depth = |write: bool, cmp| {
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: write,
                depth_compare: cmp,
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        let alpha_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let pipe = |label: &str,
                    layout: &wgpu::PipelineLayout,
                    vs: &str,
                    fs: Option<&str>,
                    buffers: &[wgpu::VertexBufferLayout],
                    blend: Option<wgpu::BlendState>,
                    topology: wgpu::PrimitiveTopology,
                    depth_state: Option<wgpu::DepthStencilState>,
                    multisample: wgpu::MultisampleState| {
            let targets = [Some(wgpu::ColorTargetState { format: HDR_FORMAT, blend, write_mask: wgpu::ColorWrites::ALL })];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: vs, compilation_options: Default::default(), buffers },
                fragment: fs.map(|fs| wgpu::FragmentState {
                    module: &shader,
                    entry_point: fs,
                    compilation_options: Default::default(),
                    targets: &targets,
                }),
                primitive: wgpu::PrimitiveState { topology, cull_mode: None, ..Default::default() },
                depth_stencil: depth_state,
                multisample,
                multiview: None,
                cache: None,
            })
        };
        let vl = [vertex_layout()];
        let bg_pipeline = pipe(
            "background",
            &cam_layout,
            "vs_bg",
            Some("fs_bg"),
            &[],
            None,
            wgpu::PrimitiveTopology::TriangleList,
            depth(false, wgpu::CompareFunction::Always),
            ms,
        );
        let mesh_pipeline = pipe(
            "mesh",
            &layout,
            "vs_mesh",
            Some("fs_mesh"),
            &vl,
            None,
            wgpu::PrimitiveTopology::TriangleList,
            depth(true, wgpu::CompareFunction::Less),
            ms,
        );
        let prepass_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh alpha prepass"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: "vs_mesh", compilation_options: Default::default(), buffers: &vl },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_prepass",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: HDR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::empty() })],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: depth(true, wgpu::CompareFunction::Less),
            multisample: ms,
            multiview: None,
            cache: None,
        });
        let transmit = |label: &str, cull: wgpu::Face| {
            let targets = [Some(wgpu::ColorTargetState { format: HDR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&transmit_layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: "vs_mesh", compilation_options: Default::default(), buffers: &vl },
                fragment: Some(wgpu::FragmentState { module: &shader, entry_point: "fs_transmit", compilation_options: Default::default(), targets: &targets }),
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: Some(cull), ..Default::default() },
                depth_stencil: depth(true, wgpu::CompareFunction::Less),
                multisample: ms,
                multiview: None,
                cache: None,
            })
        };
        // Glass reads front-facing as counter-clockwise (the mesh winding).
        let transmit_front = transmit("transmission front faces", wgpu::Face::Back);
        let backface_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("transmission back faces"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: "vs_mesh", compilation_options: Default::default(), buffers: &vl },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_backface",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba32Float, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: Some(wgpu::Face::Front), ..Default::default() },
            depth_stencil: depth(true, wgpu::CompareFunction::Less),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let resolve_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("depth resolve"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@group(0) @binding(0) var src: texture_depth_multisampled_2d;
@group(0) @binding(1) var dst: texture_storage_2d<r32float, write>;
@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let d = textureDimensions(dst);
    if g.x >= d.x || g.y >= d.y { return; }
    // The nearest sample: thin edges keep the front surface.
    var z = 1.0;
    for (var s = 0; s < 4; s++) { z = min(z, textureLoad(src, vec2<i32>(g.xy), s)); }
    textureStore(dst, vec2<i32>(g.xy), vec4<f32>(z, 0.0, 0.0, 1.0));
}
"#
                .into(),
            ),
        });
        let depth_resolve = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("depth resolve"),
            layout: None,
            module: &resolve_module,
            entry_point: "main",
            compilation_options: Default::default(),
            cache: None,
        });
        let blend_pipeline = pipe(
            "mesh alpha blend",
            &layout,
            "vs_mesh",
            Some("fs_mesh"),
            &vl,
            Some(alpha_blend),
            wgpu::PrimitiveTopology::TriangleList,
            depth(false, wgpu::CompareFunction::LessEqual),
            ms,
        );
        let wire_pipeline = pipe(
            "wireframe",
            &cam_layout,
            "vs_wire",
            Some("fs_wire"),
            &vl,
            Some(alpha_blend),
            wgpu::PrimitiveTopology::LineList,
            depth(false, wgpu::CompareFunction::LessEqual),
            ms,
        );
        let ants_bgl = super::select::ants_bgl(device);
        let ants_module = super::select::ants_module(device);
        let ants_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ants layout"),
            bind_group_layouts: &[&cam_bgl, &ants_bgl],
            push_constant_ranges: &[],
        });
        let ants_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("selection ants"),
            layout: Some(&ants_layout),
            vertex: wgpu::VertexState { module: &ants_module, entry_point: "vs_mesh", compilation_options: Default::default(), buffers: &vl },
            fragment: Some(wgpu::FragmentState {
                module: &ants_module,
                entry_point: "fs_ants",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: HDR_FORMAT, blend: Some(alpha_blend), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: depth(false, wgpu::CompareFunction::LessEqual),
            multisample: ms,
            multiview: None,
            cache: None,
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow"),
            layout: Some(&shadow_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_shadow",
                compilation_options: Default::default(),
                buffers: &vl,
            },
            fragment: None,
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let post_pipe = |label: &str, fs: &str, format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&post_layout),
                vertex: wgpu::VertexState { module: &post, entry_point: "vs_full", compilation_options: Default::default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState {
                    module: &post,
                    entry_point: fs,
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let bright_pipeline = post_pipe("bloom bright", "fs_bright", HDR_FORMAT, None);
        let down_pipeline = post_pipe("bloom down", "fs_down", HDR_FORMAT, None);
        let up_pipeline = post_pipe("bloom up", "fs_up", HDR_FORMAT, Some(additive));
        let final_pipeline = post_pipe("post final", "fs_final", OUT_FORMAT, None);

        let linear = |u, v, mip| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: u,
                address_mode_v: v,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: mip,
                ..Default::default()
            })
        };
        let env_sampler = linear(wgpu::AddressMode::Repeat, wgpu::AddressMode::ClampToEdge, wgpu::FilterMode::Linear);
        let post_sampler = linear(wgpu::AddressMode::ClampToEdge, wgpu::AddressMode::ClampToEdge, wgpu::FilterMode::Nearest);
        let mat_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let shadow = Tex::new(
            device,
            "shadow map",
            SHADOW_SIZE,
            SHADOW_SIZE,
            DEPTH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let cam_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera"),
            contents: bytemuck::bytes_of(&<CameraUniform as bytemuck::Zeroable>::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let cam_bg = Self::make_cam_bg(device, &cam_bgl, &cam_buf, env, &env_sampler, &shadow, &shadow_sampler);
        let shadow_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow bg"),
            layout: &shadow_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: cam_buf.as_entire_binding() }],
        });
        let _ = queue;
        Viewport {
            width: 64,
            height: 64,
            targets: make_targets(device, 64, 64),
            shadow,
            cam_buf,
            cam_bgl,
            mat_bgl,
            post_bgl,
            cam_bg,
            shadow_bg,
            env_sampler,
            shadow_sampler,
            mat_sampler,
            post_sampler,
            bg_pipeline,
            mesh_pipeline,
            blend_pipeline,
            backface_pipeline,
            transmit_front,
            depth_resolve,
            scene_bgl,
            prepass_pipeline,
            wire_pipeline,
            ants_bgl,
            ants_pipeline,
            shadow_pipeline,
            bright_pipeline,
            down_pipeline,
            up_pipeline,
            final_pipeline,
        }
    }

    fn make_cam_bg(
        device: &wgpu::Device,
        bgl: &wgpu::BindGroupLayout,
        buf: &wgpu::Buffer,
        env: &Environment,
        sampler: &wgpu::Sampler,
        shadow: &Tex,
        shadow_sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera bg"),
            layout: bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&env.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&shadow.view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(shadow_sampler) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&env.spec_view) },
            ],
        })
    }

    pub fn set_environment(&mut self, device: &wgpu::Device, env: &Environment) {
        self.cam_bg = Self::make_cam_bg(
            device,
            &self.cam_bgl,
            &self.cam_buf,
            env,
            &self.env_sampler,
            &self.shadow,
            &self.shadow_sampler,
        );
    }

    /// Returns true when the targets were recreated.
    pub fn resize(&mut self, device: &wgpu::Device, w: u32, h: u32) -> bool {
        let w = w.clamp(16, 8192);
        let h = h.clamp(16, 8192);
        if w == self.width && h == self.height {
            return false;
        }
        self.targets = make_targets(device, w, h);
        self.width = w;
        self.height = h;
        true
    }

    /// The sRGB image shown by egui.
    pub fn output(&self) -> &Tex {
        &self.targets.out
    }

    /// Linear HDR scene target (path tracer output).
    pub fn hdr(&self) -> &Tex {
        &self.targets.hdr
    }

    /// Multisampled scene depth (brush occlusion).
    pub fn depth(&self) -> &Tex {
        &self.targets.depth
    }

    fn post_pass(
        &self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        uniform: PostUniform,
        src: &Tex,
        bloom: &Tex,
        dst: &Tex,
        load: bool,
    ) {
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("post"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post bg"),
            layout: &self.post_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&src.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&bloom.view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.post_sampler) },
            ],
        });
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("post"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &dst.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if load { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color::BLACK) },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_pipeline(pipeline);
        rp.set_bind_group(0, &bg, &[]);
        rp.draw(0..3, 0..1);
    }

    pub fn render(&self, device: &wgpu::Device, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, frame: &Frame) {
        queue.write_buffer(&self.cam_buf, 0, bytemuck::bytes_of(&frame.camera));
        let t = &self.targets;
        let mats: Vec<(usize, wgpu::BindGroup, bool)> = frame
            .sets
            .iter()
            .filter(|s| s.visible)
            .map(|s| {
                let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("set params"),
                    contents: bytemuck::bytes_of(&s.params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("material bg"),
                    layout: &self.mat_bgl,
                    entries: &[
                        tv(0, s.base),
                        tv(1, s.orm),
                        tv(2, s.normal),
                        tv(3, s.emissive),
                        tv(4, s.display),
                        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.mat_sampler) },
                        wgpu::BindGroupEntry { binding: 6, resource: ub.as_entire_binding() },
                    ],
                });
                (s.submesh, bg, s.params.k[0] > 1.5)
            })
            .collect();
        // Transmissive sets (alpha blended or not) draw after the opaque
        // scene, over a copy of it.
        let transmissive: Vec<bool> = frame.sets.iter().filter(|s| s.visible).map(|s| s.transmissive).collect();
        let any_transmit = transmissive.iter().any(|t| *t);

        if frame.shadows && !frame.path_traced {
            if let Some(mesh) = frame.mesh {
                let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shadow"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.shadow.view,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                rp.set_pipeline(&self.shadow_pipeline);
                rp.set_bind_group(0, &self.shadow_bg, &[]);
                rp.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                for (sub, _, _) in &mats {
                    let (b, n) = mesh.submeshes[*sub].draw_indices();
                    rp.set_index_buffer(b.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..n, 0, 0..1);
                }
            }
        }

        if !frame.path_traced {
            // Background and opaque sets.
            {
                let mut rp = scene_pass(enc, t, "viewport opaque", true);
                rp.set_bind_group(0, &self.cam_bg, &[]);
                rp.set_pipeline(&self.bg_pipeline);
                rp.draw(0..3, 0..1);
                if let Some(mesh) = frame.mesh {
                    rp.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    rp.set_pipeline(&self.mesh_pipeline);
                    for ((sub, bg, blended), tr) in mats.iter().zip(&transmissive) {
                        if *blended || *tr {
                            continue;
                        }
                        let (b, n) = mesh.submeshes[*sub].draw_indices();
                        rp.set_bind_group(1, bg, &[]);
                        rp.set_index_buffer(b.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..n, 0, 0..1);
                    }
                }
            }
            // Transmissive sets see a copy of what is drawn so far (color
            // and depth): their back faces over the opaque scene, then their
            // front faces over that.
            let snapshot = |enc: &mut wgpu::CommandEncoder| {
                enc.copy_texture_to_texture(t.hdr.texture.as_image_copy(), t.scene.texture.as_image_copy(), wgpu::Extent3d { width: t.hdr.width, height: t.hdr.height, depth_or_array_layers: 1 });
                let depth_view = t.depth.texture.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
                let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("depth resolve"),
                    layout: &self.depth_resolve.get_bind_group_layout(0),
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&depth_view) }, tv(1, &t.scene_depth)],
                });
                let mut cp = enc.begin_compute_pass(&Default::default());
                cp.set_pipeline(&self.depth_resolve);
                cp.set_bind_group(0, &bg, &[]);
                cp.dispatch_workgroups(t.hdr.width.div_ceil(8), t.hdr.height.div_ceil(8), 1);
            };
            let scene_bg = any_transmit.then(|| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("scene bg"),
                    layout: &self.scene_bgl,
                    entries: &[
                        tv(0, &t.scene),
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.post_sampler) },
                        tv(2, &t.scene_depth),
                        tv(3, &t.back),
                    ],
                })
            });
            if let (Some(_), Some(mesh)) = (&scene_bg, frame.mesh) {
                // The opaque scene (color and depth), then the back faces.
                snapshot(enc);
                let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("transmission back faces"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &t.back.view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &t.back_depth.view,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                rp.set_bind_group(0, &self.cam_bg, &[]);
                rp.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                rp.set_pipeline(&self.backface_pipeline);
                for ((sub, bg, _), tr) in mats.iter().zip(&transmissive) {
                    if *tr {
                        let (b, n) = mesh.submeshes[*sub].draw_indices();
                        rp.set_bind_group(1, bg, &[]);
                        rp.set_index_buffer(b.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..n, 0, 0..1);
                    }
                }
            }
            let mut rp = scene_pass(enc, t, "viewport transparent", false);
            rp.set_bind_group(0, &self.cam_bg, &[]);
            if let Some(mesh) = frame.mesh {
                rp.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                if let Some(sbg) = &scene_bg {
                    rp.set_pipeline(&self.transmit_front);
                    rp.set_bind_group(2, sbg, &[]);
                    for ((sub, bg, _), tr) in mats.iter().zip(&transmissive) {
                        if !*tr {
                            continue;
                        }
                        let (b, n) = mesh.submeshes[*sub].draw_indices();
                        rp.set_bind_group(1, bg, &[]);
                        rp.set_index_buffer(b.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..n, 0, 0..1);
                    }
                }
                // The depth of the blended sets' opaque texels, then the
                // blended sets over it (back to front).
                for pipeline in [&self.prepass_pipeline, &self.blend_pipeline] {
                    rp.set_pipeline(pipeline);
                    for ((sub, bg, blended), tr) in mats.iter().zip(&transmissive) {
                        if !*blended || *tr {
                            continue;
                        }
                        let (b, n) = mesh.submeshes[*sub].draw_indices();
                        rp.set_bind_group(1, bg, &[]);
                        rp.set_index_buffer(b.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..n, 0, 0..1);
                    }
                }
                if frame.wireframe || frame.wire_only.is_some() {
                    rp.set_pipeline(&self.wire_pipeline);
                    for (sub, _, _) in mats.iter().filter(|m| frame.wireframe || frame.wire_only == Some(m.0)) {
                        let s = &mesh.submeshes[*sub];
                        rp.set_index_buffer(s.edge_buffer.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..s.edge_count, 0, 0..1);
                    }
                }
                if let Some(a) = &frame.selection {
                    let u = super::select::AntsUniform { k: [a.phase, 0.0, 0.0, 0.0], xform: [0.0; 4], offs: [0.0; 4] };
                    let bg = super::select::ants_bg(device, &self.ants_bgl, a, u);
                    let (b, n) = mesh.submeshes[a.submesh].draw_indices();
                    rp.set_pipeline(&self.ants_pipeline);
                    rp.set_bind_group(1, &bg, &[]);
                    rp.set_index_buffer(b.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..n, 0, 0..1);
                }
            }
        }

        // Bloom chain, then the final tonemap.
        let raw = frame.post.b[3] > 0.5;
        let bloom_on = frame.post.a[2] > 0.0 && !raw;
        if bloom_on {
            let texel = |tx: &Tex| [1.0 / tx.width as f32, 1.0 / tx.height as f32, 0.0, 0.0];
            let mut u = frame.post;
            u.texel = texel(&t.hdr);
            self.post_pass(device, enc, &self.bright_pipeline, u, &t.hdr, &t.hdr, &t.bloom[0], false);
            for i in 1..t.bloom.len() {
                u.texel = texel(&t.bloom[i - 1]);
                self.post_pass(device, enc, &self.down_pipeline, u, &t.bloom[i - 1], &t.bloom[i - 1], &t.bloom[i], false);
            }
            for i in (1..t.bloom.len()).rev() {
                u.texel = texel(&t.bloom[i]);
                self.post_pass(device, enc, &self.up_pipeline, u, &t.bloom[i], &t.bloom[i], &t.bloom[i - 1], true);
            }
        }
        let mut u = frame.post;
        if !bloom_on {
            u.a[2] = 0.0;
        }
        self.post_pass(device, enc, &self.final_pipeline, u, &t.hdr, &t.bloom[0], &t.out, false);
    }
}
