//! Small layer thumbnails (layer stack rows): a compute pass box-filters a
//! layer texture into a 64 x 64 RGBA8 image.

use super::texture::Tex;
use wgpu::util::DeviceExt;

pub const SIZE: u32 = 64;

const SHADER: &str = r#"
struct P { mode: vec4<u32> };
@group(0) @binding(0) var<uniform> U: P;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var dst: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let d = textureDimensions(dst);
    if g.x >= d.x || g.y >= d.y { return; }
    let sd = textureDimensions(src);
    let step = max(sd / d, vec2<u32>(1u));
    var acc = vec4<f32>(0.0);
    var n = 0.0;
    for (var y = 0u; y < 4u; y++) {
        for (var x = 0u; x < 4u; x++) {
            let p = g.xy * step + (vec2<u32>(x, y) * step) / 4u;
            acc += textureLoad(src, min(p, sd - 1u), 0);
            n += 1.0;
        }
    }
    let c = acc / n;
    // Checkerboard behind transparent content.
    let chk = select(0.16, 0.24, ((g.x / 8u) + (g.y / 8u)) % 2u == 0u);
    var o: vec3<f32>;
    if U.mode.x == 1u {
        // Grayscale value (masks).
        o = vec3<f32>(clamp(c.r, 0.0, 1.0));
    } else {
        // Premultiplied color over the checker; linear to display gamma.
        let a = clamp(c.a, 0.0, 1.0);
        o = c.rgb + vec3<f32>(chk) * (1.0 - a);
        o = pow(clamp(o, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2));
    }
    textureStore(dst, g.xy, vec4<f32>(o, 1.0));
}
"#;

pub struct Thumbnailer {
    pipeline: wgpu::ComputePipeline,
}

impl Thumbnailer {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("thumbs"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("thumbs"),
            layout: None,
            module: &module,
            entry_point: "main",
            compilation_options: Default::default(),
            cache: None,
        });
        Self { pipeline }
    }

    pub fn new_target(device: &wgpu::Device) -> Tex {
        Tex::new(
            device,
            "layer thumbnail",
            SIZE,
            SIZE,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        )
    }

    /// gray = true shows the red channel as a grayscale value.
    pub fn render(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, src: &Tex, dst: &Tex, gray: bool) {
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&[gray as u32, 0, 0, 0]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let view = src.mip_view(0);
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&dst.view) },
            ],
        });
        let mut pass = enc.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.dispatch_workgroups(SIZE / 8, SIZE / 8, 1);
    }
}
