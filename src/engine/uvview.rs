//! The 2D (UV space) view of the active texture set.

use super::texture::Tex;
use super::viewport::vertex_layout;
use super::GpuMesh;
use wgpu::util::DeviceExt;

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ViewUniform {
    /// uv -> ndc: x = dot(xform.xy, uv) + offs.x, y = dot(xform.zw, uv) + offs.y
    pub xform: [f32; 4],
    pub offs: [f32; 4],
    pub mode: [f32; 4],
    /// Wireframe color (linear rgb), opacity.
    pub line: [f32; 4],
}

pub struct UvView {
    pub width: u32,
    pub height: u32,
    pub out: Tex,
    bgl: wgpu::BindGroupLayout,
    quad: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    ants_bgl: wgpu::BindGroupLayout,
    ants: wgpu::RenderPipeline,
}

impl UvView {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("uvview.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/uvview.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uv view bgl"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("uv view layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let blend = wgpu::BlendState::ALPHA_BLENDING;
        let mk = |vs: &str, fs: &str, buffers: &[wgpu::VertexBufferLayout], topology| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("uv view"),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: vs, compilation_options: Default::default(), buffers },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: fs,
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: FORMAT, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let quad = mk("vs_quad", "fs_quad", &[], wgpu::PrimitiveTopology::TriangleList);
        let ants_bgl = super::select::ants_bgl(device);
        let empty = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("empty"), entries: &[] });
        let ants_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("uv ants layout"),
            bind_group_layouts: &[&empty, &ants_bgl],
            push_constant_ranges: &[],
        });
        let ants_module = super::select::ants_module(device);
        let ants = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("uv view ants"),
            layout: Some(&ants_layout),
            vertex: wgpu::VertexState { module: &ants_module, entry_point: "vs_quad", compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &ants_module,
                entry_point: "fs_ants",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: FORMAT, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let lines = mk("vs_line", "fs_line", &[vertex_layout()], wgpu::PrimitiveTopology::LineList);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("uv view sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let out = Tex::new(device, "uv view", 64, 64, FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
        Self { width: 64, height: 64, out, bgl, quad, lines, sampler, ants_bgl, ants }
    }

    pub fn resize(&mut self, device: &wgpu::Device, w: u32, h: u32) -> bool {
        let (w, h) = (w.clamp(16, 8192), h.clamp(16, 8192));
        if w == self.width && h == self.height {
            return false;
        }
        self.out = Tex::new(device, "uv view", w, h, FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
        self.width = w;
        self.height = h;
        true
    }

    /// `zoom` = pixels per UV unit, `pan` = pixel position of UV (0, 0),
    /// `rot` = view rotation (radians) around UV (0, 0).
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        texture: &Tex,
        mode: u32,
        zoom: f32,
        pan: [f32; 2],
        rot: f32,
        mesh: Option<(&GpuMesh, usize)>,
        wire: [f32; 4],
        selection: Option<&super::select::SelOverlay>,
    ) {
        let wire_alpha = wire[3];
        let (w, h) = (self.width as f32, self.height as f32);
        // px = R(rot) (uv * zoom) + pan, then pixels to ndc (y up).
        let (s, c) = rot.sin_cos();
        let u = ViewUniform {
            xform: [2.0 * zoom * c / w, -2.0 * zoom * s / w, -2.0 * zoom * s / h, -2.0 * zoom * c / h],
            offs: [2.0 * pan[0] / w - 1.0, 1.0 - 2.0 * pan[1] / h, 0.0, 0.0],
            mode: [mode as f32, wire_alpha, 0.0, 0.0],
            line: wire,
        };
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("uv view"),
            contents: bytemuck::bytes_of(&u),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        // Display through mip 0 only: the sampler is non-filtering.
        let view = texture.mip_view(0);
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uv view bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("uv view"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.out.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.012, g: 0.012, b: 0.014, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_bind_group(0, &bg, &[]);
        rp.set_pipeline(&self.quad);
        rp.draw(0..6, 0..1);
        if let Some((mesh, sub)) = mesh {
            if wire_alpha > 0.0 {
                let s = &mesh.submeshes[sub];
                rp.set_pipeline(&self.lines);
                rp.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                rp.set_index_buffer(s.edge_buffer.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..s.edge_count, 0, 0..1);
            }
        }
        if let Some(o) = selection {
            let a = super::select::AntsUniform { k: [o.phase, 1.0, 0.0, 0.0], xform: u.xform, offs: u.offs };
            let bg = super::select::ants_bg(device, &self.ants_bgl, o, a);
            let empty = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("empty"), layout: &self.ants.get_bind_group_layout(0), entries: &[] });
            rp.set_pipeline(&self.ants);
            rp.set_bind_group(0, &empty, &[]);
            rp.set_bind_group(1, &bg, &[]);
            rp.draw(0..6, 0..1);
        }
    }
}
