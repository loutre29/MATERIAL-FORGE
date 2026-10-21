//! Resampling a texture to a new size with a bilinear render pass (texture
//! set resolution changes).

use super::texture::Tex;
use std::collections::HashMap;

const SHADER: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
struct O { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> O {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    var o: O;
    o.pos = vec4<f32>(p, 0.0, 1.0);
    o.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return o;
}
@fragment fn fs(in: O) -> @location(0) vec4<f32> {
    return textureSampleLevel(src, samp, in.uv, 0.0);
}
"#;

pub struct Blitter {
    module: wgpu::ShaderModule,
    bgl: wgpu::BindGroupLayout,
    layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    pipelines: std::cell::RefCell<HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>>,
}

impl Blitter {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self { module, bgl, layout, sampler, pipelines: Default::default() }
    }

    /// A copy of `src` at `size` x `size` with the same format and usage
    /// (plus RENDER_ATTACHMENT).
    pub fn resample(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, src: &Tex, size: u32) -> Tex {
        let dst = Tex::new(device, "resampled", size, size, src.format, src.usage | wgpu::TextureUsages::RENDER_ATTACHMENT);
        let mut pipes = self.pipelines.borrow_mut();
        let pipeline = pipes.entry(src.format).or_insert_with(|| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("blit"),
                layout: Some(&self.layout),
                vertex: wgpu::VertexState { module: &self.module, entry_point: "vs", compilation_options: Default::default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState {
                    module: &self.module,
                    entry_point: "fs",
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: src.format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        });
        let view = src.mip_view(0);
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("blit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &dst.view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_pipeline(pipeline);
        rp.set_bind_group(0, &bg, &[]);
        rp.draw(0..3, 0..1);
        drop(rp);
        dst
    }
}
