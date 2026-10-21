//! Environment lighting: an equirectangular HDR with a CPU-built mip chain
//! (used for rough reflections) and 9 SH coefficients for diffuse light.

use anyhow::{Context, Result};
use glam::Vec3;
use half::f16;
use std::f32::consts::PI;
use std::path::Path;

pub struct Environment {
    /// Kept alive for the view.
    #[allow(dead_code)]
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub mip_count: u32,
    /// Diffuse radiance coefficients (already convolved and divided by pi).
    pub sh: [[f32; 4]; 9],
    pub name: String,
}

/// Direction for equirect coordinates, matching `equirect_uv` in viewport.wgsl.
fn dir_from_uv(u: f32, v: f32) -> Vec3 {
    let phi = (u - 0.5) * 2.0 * PI;
    let theta = v * PI;
    Vec3::new(theta.sin() * phi.sin(), theta.cos(), -theta.sin() * phi.cos())
}

/// Built-in neutral studio: soft gradient dome with three soft boxes.
pub fn studio_pixels(w: u32, h: u32) -> Vec<[f32; 4]> {
    struct Light {
        dir: Vec3,
        half: (f32, f32),
        power: f32,
    }
    let lights = [
        Light { dir: Vec3::new(-0.55, 0.55, 0.62).normalize(), half: (0.28, 0.18), power: 9.0 },
        Light { dir: Vec3::new(0.75, 0.3, -0.58).normalize(), half: (0.1, 0.3), power: 6.0 },
        Light { dir: Vec3::new(0.05, 1.0, 0.05).normalize(), half: (0.35, 0.35), power: 2.5 },
    ];
    let mut out = vec![[0.0f32; 4]; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let d = dir_from_uv((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
            let horizon = Vec3::new(0.42, 0.42, 0.44);
            let zenith = Vec3::new(0.2, 0.22, 0.26);
            let ground = Vec3::new(0.08, 0.075, 0.07);
            let mut c = if d.y >= 0.0 {
                horizon.lerp(zenith, smooth(0.0, 0.8, d.y))
            } else {
                (horizon * 0.5).lerp(ground, smooth(0.0, 0.25, -d.y))
            };
            for l in &lights {
                let z = d.dot(l.dir);
                if z <= 0.0 {
                    continue;
                }
                let right = l.dir.cross(Vec3::Y).normalize_or(Vec3::X);
                let up = right.cross(l.dir);
                let lx = d.dot(right) / z;
                let ly = d.dot(up) / z;
                let fx = 1.0 - smooth(l.half.0 * 0.8, l.half.0, lx.abs());
                let fy = 1.0 - smooth(l.half.1 * 0.8, l.half.1, ly.abs());
                c += Vec3::splat(l.power * fx * fy);
            }
            out[(y * w + x) as usize] = [c.x, c.y, c.z, 1.0];
        }
    }
    out
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn load_hdr(path: &Path) -> Result<(u32, u32, Vec<[f32; 4]>)> {
    let img = image::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut img = img.into_rgba32f();
    if img.width() > 4096 {
        img = image::imageops::resize(&img, 4096, 2048, image::imageops::FilterType::Triangle);
    }
    let (w, h) = img.dimensions();
    let px = img.pixels().map(|p| [p.0[0].max(0.0), p.0[1].max(0.0), p.0[2].max(0.0), 1.0]).collect();
    Ok((w, h, px))
}

fn downsample(w: u32, h: u32, src: &[[f32; 4]]) -> (u32, u32, Vec<[f32; 4]>) {
    let nw = (w / 2).max(1);
    let nh = (h / 2).max(1);
    let mut out = vec![[0.0; 4]; (nw * nh) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0.0f32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (x * 2 + dx).min(w - 1);
                let sy = (y * 2 + dy).min(h - 1);
                let p = src[(sy * w + sx) as usize];
                for c in 0..4 {
                    acc[c] += p[c] * 0.25;
                }
            }
            out[(y * nw + x) as usize] = acc;
        }
    }
    (nw, nh, out)
}

fn sh_basis(d: Vec3) -> [f32; 9] {
    let (x, y, z) = (d.x, d.y, d.z);
    [
        0.282095,
        0.488603 * y,
        0.488603 * z,
        0.488603 * x,
        1.092548 * x * y,
        1.092548 * y * z,
        0.315392 * (3.0 * z * z - 1.0),
        1.092548 * x * z,
        0.546274 * (x * x - y * y),
    ]
}

fn project_sh(w: u32, h: u32, px: &[[f32; 4]]) -> [[f32; 4]; 9] {
    let mut coeffs = [[0.0f32; 4]; 9];
    let dtheta = PI / h as f32;
    let dphi = 2.0 * PI / w as f32;
    for y in 0..h {
        let v = (y as f32 + 0.5) / h as f32;
        let weight = (v * PI).sin() * dtheta * dphi;
        for x in 0..w {
            let d = dir_from_uv((x as f32 + 0.5) / w as f32, v);
            let b = sh_basis(d);
            let p = px[(y * w + x) as usize];
            for i in 0..9 {
                for c in 0..3 {
                    coeffs[i][c] += p[c] * b[i] * weight;
                }
            }
        }
    }
    // Lambert convolution (A0 = pi, A1 = 2pi/3, A2 = pi/4), then divide by pi.
    let band = [1.0, 2.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0, 0.25, 0.25, 0.25, 0.25, 0.25];
    for i in 0..9 {
        for c in 0..3 {
            coeffs[i][c] *= band[i];
        }
    }
    coeffs
}

impl Environment {
    pub fn studio(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let (w, h) = (1024, 512);
        Self::build(device, queue, w, h, studio_pixels(w, h), "Studio (built-in)".into())
    }

    pub fn build(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        w: u32,
        h: u32,
        pixels: Vec<[f32; 4]>,
        name: String,
    ) -> Self {
        let mut levels = vec![(w, h, pixels)];
        while levels.last().map(|l| l.0 > 1 || l.1 > 1).unwrap_or(false) {
            let (lw, lh, lp) = levels.last().unwrap();
            levels.push(downsample(*lw, *lh, lp));
        }
        let sh_level = levels.iter().find(|l| l.0 <= 256).unwrap_or(levels.last().unwrap());
        let sh = project_sh(sh_level.0, sh_level.1, &sh_level.2);

        let mip_count = levels.len() as u32;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("environment"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: mip_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (mip, (lw, lh, lp)) in levels.iter().enumerate() {
            let mut bytes = Vec::with_capacity(lp.len() * 8);
            for p in lp {
                for c in p {
                    bytes.extend_from_slice(&f16::from_f32(c.min(60000.0)).to_le_bytes());
                }
            }
            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &texture,
                    mip_level: mip as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &bytes,
                wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(lw * 8), rows_per_image: Some(*lh) },
                wgpu::Extent3d { width: *lw, height: *lh, depth_or_array_layers: 1 },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut sh4 = [[0.0f32; 4]; 9];
        for i in 0..9 {
            sh4[i] = [sh[i][0], sh[i][1], sh[i][2], 0.0];
        }
        Environment { texture, view, mip_count, sh: sh4, name }
    }
}
