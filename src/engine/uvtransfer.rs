//! Moves painted textures from one UV layout to another (Auto UV on a
//! project): every triangle of the new layout is drawn in texture space and
//! reads the old texture where the same triangle was before. Triangles are
//! the same, in the same order, so the transfer is exact up to resampling.
//! Each triangle is drawn twice: grown by a texel and a half first (a
//! border so filtering at island edges finds paint, not black), then exact.

use super::texture::Tex;
use std::collections::HashMap;

const SHADER: &str = r#"
struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) old_uv: vec2<f32>,
};

@vertex
fn vs(@location(0) new_uv: vec2<f32>, @location(1) old_uv: vec2<f32>) -> VOut {
    var o: VOut;
    o.pos = vec4<f32>(new_uv.x * 2.0 - 1.0, 1.0 - new_uv.y * 2.0, 0.0, 1.0);
    o.old_uv = old_uv;
    return o;
}

@group(0) @binding(0) var src: texture_2d<f32>;

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    // Nearest texel: no blending with unpainted space around old islands.
    let dims = vec2<f32>(textureDimensions(src));
    let p = clamp(vec2<i32>(floor(i.old_uv * dims)), vec2<i32>(0), vec2<i32>(dims) - vec2<i32>(1));
    return textureLoad(src, p, 0);
}
"#;

pub struct UvTransfer {
    module: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipe_layout: wgpu::PipelineLayout,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
}

/// Vertices of a transfer: (new u, new v, old u, old v) per corner.
pub type Corner = [f32; 4];

impl UvTransfer {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("uv transfer"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uv transfer"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            }],
        });
        let pipe_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("uv transfer"), bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        UvTransfer { module, layout, pipe_layout, pipelines: HashMap::new() }
    }

    fn pipeline(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) -> &wgpu::RenderPipeline {
        let (module, pipe_layout) = (&self.module, &self.pipe_layout);
        self.pipelines.entry(format).or_insert_with(|| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("uv transfer"),
                layout: Some(pipe_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: "vs",
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: 16,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                    }],
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: "fs",
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        })
    }

    /// The corners to draw for a triangle list: each triangle grown by
    /// `grow` (UV units) first, then exact. The old UVs follow the growth
    /// through the triangle's affine map.
    pub fn corners(tris: &[([[f32; 2]; 3], [[f32; 2]; 3])], grow: f32) -> Vec<Corner> {
        let mut out = Vec::with_capacity(tris.len() * 6);
        for (n, o) in tris {
            let c = [(n[0][0] + n[1][0] + n[2][0]) / 3.0, (n[0][1] + n[1][1] + n[2][1]) / 3.0];
            // Barycentric coordinates of a new-UV point in the triangle.
            let d = (n[1][0] - n[0][0]) * (n[2][1] - n[0][1]) - (n[2][0] - n[0][0]) * (n[1][1] - n[0][1]);
            for k in 0..3 {
                let v = [n[k][0] - c[0], n[k][1] - c[1]];
                let len = (v[0] * v[0] + v[1] * v[1]).sqrt().max(1e-12);
                let p = [n[k][0] + v[0] / len * grow, n[k][1] + v[1] / len * grow];
                let old = if d.abs() > 1e-20 {
                    let w1 = ((p[0] - n[0][0]) * (n[2][1] - n[0][1]) - (n[2][0] - n[0][0]) * (p[1] - n[0][1])) / d;
                    let w2 = ((n[1][0] - n[0][0]) * (p[1] - n[0][1]) - (p[0] - n[0][0]) * (n[1][1] - n[0][1])) / d;
                    let w0 = 1.0 - w1 - w2;
                    [o[0][0] * w0 + o[1][0] * w1 + o[2][0] * w2, o[0][1] * w0 + o[1][1] * w1 + o[2][1] * w2]
                } else {
                    o[k]
                };
                out.push([p[0], p[1], old[0], old[1]]);
            }
        }
        for (n, o) in tris {
            for k in 0..3 {
                out.push([n[k][0], n[k][1], o[k][0], o[k][1]]);
            }
        }
        out
    }

    /// `src` laid out on the new UVs: a new texture of the same size and
    /// format (unpainted space transparent).
    pub fn resample(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, src: &Tex, verts: &wgpu::Buffer, count: u32) -> Tex {
        let dst = Tex::new(device, "uv transfer", src.width, src.height, src.format, src.usage);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uv transfer"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src.mip_view(0)) }],
        });
        let target = dst.mip_view(0);
        let pipeline = self.pipeline(device, src.format);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("uv transfer"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.set_vertex_buffer(0, verts.slice(..));
        pass.draw(0..count, 0..1);
        drop(pass);
        dst
    }
}
