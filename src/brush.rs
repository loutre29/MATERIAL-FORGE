//! Brush settings, procedural alpha shapes and the stroke engine that turns
//! cursor motion into stamps (spacing, jitter, follow-path, lazy mouse,
//! symmetry) for the camera, wrap and UV projection modes.

use crate::document::{Document, NCH};
use crate::engine::paint::Stamp;
use glam::{Mat4, Vec2, Vec3, Vec4};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Alignment {
    /// Stamps wrap on the surface in world space (default).
    Wrap,
    /// Stamps projected from the camera with occlusion.
    Camera,
}

impl Alignment {
    pub const ALL: [Alignment; 2] = [Alignment::Wrap, Alignment::Camera];
    pub fn name(self) -> &'static str {
        match self {
            Alignment::Wrap => "Tangent / wrap",
            Alignment::Camera => "Camera projection",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlphaKind {
    Round,
    Square,
    SoftSquare,
    Star,
    Ring,
    Diamond,
    Splatter,
    Grunge,
    Spray,
    Line,
    Cloud,
    Chalk,
}

impl AlphaKind {
    pub const ALL: [AlphaKind; 12] = [
        AlphaKind::Round,
        AlphaKind::Square,
        AlphaKind::SoftSquare,
        AlphaKind::Star,
        AlphaKind::Ring,
        AlphaKind::Diamond,
        AlphaKind::Splatter,
        AlphaKind::Grunge,
        AlphaKind::Spray,
        AlphaKind::Line,
        AlphaKind::Cloud,
        AlphaKind::Chalk,
    ];
    pub fn name(self) -> &'static str {
        match self {
            AlphaKind::Round => "Round",
            AlphaKind::Square => "Square",
            AlphaKind::SoftSquare => "Soft square",
            AlphaKind::Star => "Star",
            AlphaKind::Ring => "Ring",
            AlphaKind::Diamond => "Diamond",
            AlphaKind::Splatter => "Splatter",
            AlphaKind::Grunge => "Grunge",
            AlphaKind::Spray => "Spray dots",
            AlphaKind::Line => "Scratch line",
            AlphaKind::Cloud => "Cloud",
            AlphaKind::Chalk => "Chalk",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AlphaSource {
    Builtin(AlphaKind),
    /// Document resource id (imported image).
    Resource(u64),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushSettings {
    /// Radius in screen points.
    pub size: f32,
    pub flow: f32,
    pub hardness: f32,
    /// Fraction of the diameter between stamps.
    pub spacing: f32,
    /// Degrees.
    pub angle: f32,
    pub follow_path: bool,
    pub size_jitter: f32,
    pub flow_jitter: f32,
    /// Degrees.
    pub angle_jitter: f32,
    /// Fraction of the radius.
    pub position_jitter: f32,
    pub alignment: Alignment,
    pub alpha: AlphaSource,
    pub angle_fade: bool,
    /// Lazy mouse distance in points (0 = off).
    pub lazy: f32,
    /// Mirror planes through the mesh center (X, Y, Z), wrap mode only.
    pub symmetry: [bool; 3],
    /// Pen pressure drives the size / the flow.
    pub pressure_size: bool,
    pub pressure_flow: bool,
}

impl Default for BrushSettings {
    fn default() -> Self {
        BrushSettings {
            size: 30.0,
            flow: 0.6,
            hardness: 0.6,
            spacing: 0.1,
            angle: 0.0,
            follow_path: false,
            size_jitter: 0.0,
            flow_jitter: 0.0,
            angle_jitter: 0.0,
            position_jitter: 0.0,
            alignment: Alignment::Wrap,
            alpha: AlphaSource::Builtin(AlphaKind::Round),
            angle_fade: true,
            lazy: 0.0,
            symmetry: [false; 3],
            pressure_size: true,
            pressure_flow: false,
        }
    }
}

/// What the brush deposits.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushMaterial {
    pub channels: [bool; NCH],
    pub color: [f32; 3],
    pub roughness: f32,
    pub metallic: f32,
    pub height: f32,
    pub emissive: [f32; 3],
    pub opacity: f32,
    pub normal: [f32; 3],
    pub mask_value: f32,
    /// Paint this material (evaluated per texel) instead of the values above.
    #[serde(skip)]
    pub material: Option<(String, crate::document::FillDesc)>,
}

impl Default for BrushMaterial {
    fn default() -> Self {
        BrushMaterial {
            channels: [true, true, true, false, false, false, false],
            color: [0.8, 0.12, 0.05],
            roughness: 0.4,
            metallic: 0.0,
            height: 0.5,
            emissive: [1.0, 0.5, 0.1],
            opacity: 1.0,
            normal: [0.0, 0.0, 1.0],
            mask_value: 1.0,
            material: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrushPreset {
    pub name: String,
    pub settings: BrushSettings,
    /// Tip from the starter alphas (path under `starter_assets/alphas`),
    /// imported into the project when the preset is applied; `settings`
    /// keep a built-in alpha for when the file is missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_file: Option<String>,
    /// Shelf folder.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
}

impl BrushPreset {
    pub fn new(name: String, settings: BrushSettings) -> Self {
        BrushPreset { name, settings, alpha_file: None, folder: String::new() }
    }

    /// Its tip as a grayscale image (the starter file when there is one).
    pub fn tip_image(&self) -> image::GrayImage {
        let file = self.alpha_file.as_deref().and_then(crate::library::starter_alpha).and_then(|p| crate::imageio::open(&p).ok());
        match file {
            Some(img) => img.to_luma8(),
            None => {
                let k = match self.settings.alpha {
                    AlphaSource::Builtin(k) => k,
                    AlphaSource::Resource(_) => AlphaKind::Round,
                };
                image::DynamicImage::ImageRgba8(alpha_image(k)).to_luma8()
            }
        }
    }

    /// Shelf thumbnail: a stroke drawn with the preset.
    pub fn thumbnail(&self, size: u32) -> image::RgbaImage {
        let round = self.alpha_file.is_none() && self.settings.alpha == AlphaSource::Builtin(AlphaKind::Round);
        stroke_image(&self.tip_image(), &self.settings, round, size, size)
    }
}

/// The starter brushes, by shelf folder. Tips come from the starter alphas.
pub fn builtin_brushes() -> Vec<BrushPreset> {
    let mut out = Vec::new();
    let mut add = |folder: &str, name: &str, file: Option<&str>, fallback: AlphaKind, f: &dyn Fn(&mut BrushSettings)| {
        let mut s = BrushSettings { alpha: AlphaSource::Builtin(fallback), ..Default::default() };
        f(&mut s);
        out.push(BrushPreset { name: name.into(), settings: s, alpha_file: file.map(|f| format!("{f}.png")), folder: folder.into() });
    };
    use AlphaKind::*;
    // Basic tips.
    add("Basic", "Basic hard", None, Round, &|s| {
        s.hardness = 0.95;
        s.flow = 1.0;
    });
    add("Basic", "Basic soft", None, Round, &|s| {
        s.hardness = 0.0;
        s.flow = 0.5;
    });
    add("Basic", "Airbrush", None, Round, &|s| {
        s.hardness = 0.0;
        s.flow = 0.08;
    });
    add("Basic", "Square", Some("Brush tips/Square Soft"), SoftSquare, &|s| s.flow = 1.0);
    // Artistic: tips that follow the stroke like a real tool.
    add("Artistic", "Pencil", Some("Brush tips/Pencil"), Chalk, &|s| {
        s.follow_path = true;
        s.spacing = 0.04;
        s.flow = 0.7;
        s.angle_jitter = 8.0;
    });
    add("Artistic", "Marker", Some("Brush tips/Marker"), SoftSquare, &|s| {
        s.spacing = 0.03;
        s.flow = 0.35;
        s.angle = 35.0;
    });
    add("Artistic", "Chalk", Some("Brush tips/Chalk"), Chalk, &|s| {
        s.angle_jitter = 180.0;
        s.spacing = 0.15;
        s.flow = 0.7;
    });
    add("Artistic", "Charcoal", Some("Brush tips/Charcoal"), Chalk, &|s| {
        s.follow_path = true;
        s.spacing = 0.06;
        s.flow = 0.5;
        s.angle_jitter = 6.0;
    });
    add("Artistic", "Dry brush", Some("Brush tips/Dry Brush"), Grunge, &|s| {
        s.follow_path = true;
        s.spacing = 0.03;
        s.flow = 0.35;
    });
    add("Artistic", "Bristles", Some("Brush tips/Bristles"), Spray, &|s| {
        s.follow_path = true;
        s.angle = 90.0;
        s.spacing = 0.02;
        s.flow = 0.45;
    });
    add("Artistic", "Sponge", Some("Brush tips/Sponge"), Spray, &|s| {
        s.angle_jitter = 180.0;
        s.size_jitter = 0.2;
        s.spacing = 0.35;
        s.flow = 0.8;
    });
    add("Artistic", "Watercolor", Some("Brush tips/Watercolor"), Cloud, &|s| {
        s.angle_jitter = 180.0;
        s.size_jitter = 0.15;
        s.spacing = 0.25;
        s.flow = 0.25;
    });
    // Grunge: dabbed, rotated and scattered.
    let dab = |s: &mut BrushSettings, spacing: f32, size_jitter: f32, flow: f32| {
        s.angle_jitter = 180.0;
        s.spacing = spacing;
        s.size_jitter = size_jitter;
        s.flow = flow;
    };
    add("Grunge", "Spray", Some("Grunge/Spray"), Spray, &|s| dab(s, 0.3, 0.0, 0.8));
    add("Grunge", "Spray dense", Some("Grunge/Spray Dense"), Spray, &|s| dab(s, 0.25, 0.0, 0.9));
    add("Grunge", "Splatter", Some("Grunge/Splatter 1"), Splatter, &|s| {
        dab(s, 0.8, 0.5, 1.0);
        s.position_jitter = 0.6;
    });
    add("Grunge", "Splatter heavy", Some("Grunge/Splatter 3"), Splatter, &|s| {
        dab(s, 1.2, 0.3, 1.0);
        s.position_jitter = 0.4;
    });
    add("Grunge", "Dust", Some("Grunge/Dust"), Spray, &|s| dab(s, 0.5, 0.4, 0.5));
    add("Grunge", "Dirt spots", Some("Grunge/Dirt Spots"), Grunge, &|s| {
        dab(s, 0.6, 0.4, 0.8);
        s.position_jitter = 0.3;
    });
    add("Grunge", "Speckles", Some("Grunge/Speckles"), Spray, &|s| dab(s, 0.4, 0.2, 0.9));
    add("Grunge", "Scratches", Some("Grunge/Scratches"), Line, &|s| dab(s, 0.8, 0.3, 0.9));
    add("Grunge", "Scratch line", Some("Brush tips/Line"), Line, &|s| {
        s.follow_path = true;
        s.spacing = 0.05;
        s.flow = 0.9;
    });
    add("Grunge", "Chipped paint", Some("Grunge/Chipped Paint"), Grunge, &|s| dab(s, 0.6, 0.4, 1.0));
    add("Grunge", "Cracks", Some("Grunge/Cracks"), Grunge, &|s| dab(s, 1.0, 0.3, 1.0));
    add("Grunge", "Rust spots", Some("Grunge/Rust Spots"), Grunge, &|s| dab(s, 0.5, 0.3, 0.8));
    add("Grunge", "Stains", Some("Grunge/Stain"), Cloud, &|s| dab(s, 0.8, 0.3, 0.6));
    add("Grunge", "Water drops", Some("Grunge/Water Drop Mark"), Ring, &|s| {
        dab(s, 1.0, 0.5, 0.8);
        s.position_jitter = 0.8;
    });
    // Leaks hang down the screen: no rotation.
    add("Grunge", "Leaks", Some("Grunge/Leaks"), Cloud, &|s| {
        s.size_jitter = 0.25;
        s.position_jitter = 0.2;
        s.spacing = 0.7;
        s.flow = 0.7;
    });
    add("Grunge", "Drips", Some("Grunge/Drips"), Cloud, &|s| {
        s.size_jitter = 0.25;
        s.spacing = 0.8;
        s.flow = 0.9;
    });
    // Stitches and seams: the tip follows the path, one element per stamp
    // (spacing = the element's length, so the line repeats evenly).
    let seam = |s: &mut BrushSettings, spacing: f32| {
        s.follow_path = true;
        s.spacing = spacing;
        s.flow = 1.0;
    };
    let st = "Stitches and hardware";
    add("Stitches", "Stitch", Some(&format!("{st}/Stitch")), Line, &|s| seam(s, 1.0));
    add("Stitches", "Stitch double", Some(&format!("{st}/Stitch Double")), Line, &|s| seam(s, 0.95));
    add("Stitches", "Stitch slanted", Some(&format!("{st}/Stitch Slanted")), Line, &|s| seam(s, 0.7));
    add("Stitches", "Cross stitch", Some(&format!("{st}/Cross Stitch")), Line, &|s| seam(s, 0.7));
    add("Stitches", "Zigzag stitch", Some(&format!("{st}/Zigzag Stitch")), Line, &|s| seam(s, 0.76));
    add("Stitches", "Blanket stitch", Some(&format!("{st}/Blanket Stitch")), Line, &|s| seam(s, 0.8));
    add("Stitches", "Dashed line", Some(&format!("{st}/Dash")), Line, &|s| seam(s, 1.0));
    add("Stitches", "Dotted line", Some(&format!("{st}/Dots Line")), Line, &|s| seam(s, 1.06));
    add("Stitches", "Seam groove", Some(&format!("{st}/Seam Groove")), Line, &|s| seam(s, 0.08));
    add("Stitches", "Seam ridge", Some(&format!("{st}/Seam Ridge")), Line, &|s| seam(s, 0.08));
    add("Stitches", "Perforation", Some(&format!("{st}/Perforation")), Round, &|s| seam(s, 0.7));
    add("Stitches", "Chain", Some(&format!("{st}/Chain Link")), Ring, &|s| seam(s, 0.62));
    add("Stitches", "Zipper", Some(&format!("{st}/Zipper Tooth")), Line, &|s| seam(s, 0.45));
    add("Stitches", "Tire tread", Some(&format!("{st}/Tread")), Line, &|s| seam(s, 0.8));
    // Hardware: spaced along the path.
    let spaced = |s: &mut BrushSettings, spacing: f32| {
        s.spacing = spacing;
        s.flow = 1.0;
    };
    add("Hardware", "Rivets", Some(&format!("{st}/Rivet")), Round, &|s| spaced(s, 2.5));
    add("Hardware", "Rivets flat", Some(&format!("{st}/Rivet Flat")), Round, &|s| spaced(s, 2.5));
    add("Hardware", "Screws cross", Some(&format!("{st}/Screw Cross")), Round, &|s| {
        spaced(s, 3.0);
        s.angle_jitter = 180.0;
    });
    add("Hardware", "Screws slotted", Some(&format!("{st}/Screw Slotted")), Round, &|s| {
        spaced(s, 3.0);
        s.angle_jitter = 180.0;
    });
    add("Hardware", "Hex bolts", Some(&format!("{st}/Bolt Hex")), Round, &|s| spaced(s, 2.5));
    // Stamps and decals.
    add("Stamps", "Hazard stripes", Some("Symbols/Hazard Stripes"), Square, &|s| seam(s, 0.9));
    add("Stamps", "Warning sign", Some("Symbols/Warning"), Star, &|s| {
        s.spacing = 2.0;
        s.flow = 1.0;
    });
    add("Stamps", "Star", Some("Shapes/Star"), Star, &|s| {
        s.spacing = 1.5;
        s.flow = 1.0;
    });
    add("Stamps", "Hearts", Some("Shapes/Heart"), Star, &|s| {
        s.spacing = 1.3;
        s.size_jitter = 0.4;
        s.angle_jitter = 25.0;
        s.position_jitter = 0.5;
        s.flow = 1.0;
    });
    out
}

/// A stroke drawn with `alpha` along an S curve, gray on black (shelf
/// thumbnails and the brush preview). `round`: the analytic round tip,
/// whose edge follows the hardness; image tips are used as they are, like
/// the paint shader does.
pub fn stroke_image(alpha: &image::GrayImage, b: &BrushSettings, round: bool, w: u32, h: u32) -> image::RgbaImage {
    let (wf, hf) = (w as f32, h as f32);
    let rad = hf.min(wf) * 0.11;
    // Sampled at about the stamp's size: no aliasing from a large tip.
    let n = ((rad * 4.0) as u32).clamp(8, alpha.width().max(8));
    let tip = image::imageops::resize(alpha, n, n, image::imageops::FilterType::Triangle);
    let step = (b.spacing * rad * 2.0).max(0.6);
    let curve = |t: f32| -> (f32, f32) { (wf * 0.13 + t * wf * 0.74, hf * 0.5 - (t * std::f32::consts::TAU).sin() * hf * 0.22) };
    let mut acc = vec![0.0f32; (w * h) as usize];
    let mut rng = 0x2545_F491_4F6C_DD1Du64;
    let mut rand = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 40) as f32 / (1u64 << 24) as f32
    };
    let mut prev = curve(0.0);
    let mut dir = {
        let n = curve(0.001);
        let d = (n.0 - prev.0, n.1 - prev.1);
        let l = (d.0 * d.0 + d.1 * d.1).sqrt().max(1e-6);
        (d.0 / l, d.1 / l)
    };
    let mut travelled = step;
    let mut t = 0.0f32;
    while t <= 1.0 {
        let c = curve(t);
        let d = (c.0 - prev.0, c.1 - prev.1);
        let l = (d.0 * d.0 + d.1 * d.1).sqrt();
        if l > 1e-6 {
            dir = (d.0 / l, d.1 / l);
        }
        travelled += l;
        prev = c;
        t += 0.0005;
        if travelled < step {
            continue;
        }
        travelled -= step;
        let (r1, r2, r3, r4, r5) = (rand(), rand(), rand(), rand(), rand());
        let size = rad * (1.0 + (r1 * 2.0 - 1.0) * b.size_jitter).max(0.05);
        // Faint flows still read in a thumbnail.
        let weight = (0.3 + 0.7 * b.flow) * (1.0 - r2 * b.flow_jitter);
        let mut angle = b.angle.to_radians() + (r3 * 2.0 - 1.0) * b.angle_jitter.to_radians();
        if b.follow_path {
            angle += dir.1.atan2(dir.0);
        }
        let (cx, cy) = (c.0 + (r4 * 2.0 - 1.0) * b.position_jitter * rad, c.1 + (r5 * 2.0 - 1.0) * b.position_jitter * rad);
        let (sa, ca) = angle.sin_cos();
        let reach = size * std::f32::consts::SQRT_2;
        let (x0, x1) = ((cx - reach).floor().max(0.0) as i64, ((cx + reach).ceil() as i64).min(w as i64 - 1));
        let (y0, y1) = ((cy - reach).floor().max(0.0) as i64, ((cy + reach).ceil() as i64).min(h as i64 - 1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = ((x as f32 + 0.5 - cx) / size, (y as f32 + 0.5 - cy) / size);
                let (u, v) = (dx * ca + dy * sa, -dx * sa + dy * ca);
                if u.abs() >= 1.0 || v.abs() >= 1.0 {
                    continue;
                }
                let a = if round {
                    let r = (u * u + v * v).sqrt();
                    1.0 - smooth(b.hardness.clamp(0.0, 0.99), 1.0, r)
                } else {
                    let px = (((u * 0.5 + 0.5) * n as f32) as u32).min(n - 1);
                    let py = (((v * 0.5 + 0.5) * n as f32) as u32).min(n - 1);
                    tip.get_pixel(px, py).0[0] as f32 / 255.0
                };
                let o = &mut acc[(y as u32 * w + x as u32) as usize];
                *o += (1.0 - *o) * a * weight;
            }
        }
    }
    image::RgbaImage::from_fn(w, h, |x, y| {
        let g = (acc[(y * w + x) as usize].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
        image::Rgba([g, g, g, 255])
    })
}

// ---------------------------------------------------------------- alpha shapes

fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263) ^ seed.wrapping_mul(2246822519);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    ((h ^ (h >> 16)) & 0xFFFFFF) as f32 / 16777216.0
}

fn vnoise(x: f32, y: f32, seed: u32) -> f32 {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (ux, uy) = (s(fx), s(fy));
    let a = hash2(ix, iy, seed);
    let b = hash2(ix + 1, iy, seed);
    let c = hash2(ix, iy + 1, seed);
    let d = hash2(ix + 1, iy + 1, seed);
    (a + (b - a) * ux) + ((c + (d - c) * ux) - (a + (b - a) * ux)) * uy
}

fn fbm(x: f32, y: f32, seed: u32, oct: u32) -> f32 {
    let (mut a, mut f, mut s, mut n) = (0.5, 1.0, 0.0, 0.0);
    for o in 0..oct {
        s += a * vnoise(x * f, y * f, seed + o);
        n += a;
        a *= 0.5;
        f *= 2.03;
    }
    s / n
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 256 x 256 grayscale alpha (stored in all channels).
pub fn alpha_image(kind: AlphaKind) -> image::RgbaImage {
    let n = 256u32;
    image::RgbaImage::from_fn(n, n, |x, y| {
        let u = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
        let v = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
        let r = (u * u + v * v).sqrt();
        let a: f32 = match kind {
            AlphaKind::Round => 1.0 - smooth(0.9, 1.0, r),
            AlphaKind::Square => 1.0 - smooth(0.92, 0.98, u.abs().max(v.abs())),
            AlphaKind::SoftSquare => 1.0 - smooth(0.3, 0.95, u.abs().max(v.abs())),
            AlphaKind::Star => {
                let ang = v.atan2(u);
                let k = 0.45 + 0.5 * ((ang * 5.0).cos() * 0.5 + 0.5).powf(3.0);
                1.0 - smooth(k - 0.04, k, r)
            }
            AlphaKind::Ring => (1.0 - smooth(0.88, 0.95, r)) * smooth(0.6, 0.67, r),
            AlphaKind::Diamond => 1.0 - smooth(0.85, 0.95, u.abs() + v.abs()),
            AlphaKind::Splatter => {
                let n1 = fbm(u * 3.0 + 10.0, v * 3.0 + 4.0, 7, 5);
                let blob = 1.0 - smooth(0.35, 0.85, r - (n1 - 0.5) * 0.9);
                let drops = {
                    let g = fbm(u * 9.0, v * 9.0, 3, 2);
                    smooth(0.72, 0.76, g) * (1.0 - smooth(0.7, 1.0, r))
                };
                blob.max(drops)
            }
            AlphaKind::Grunge => {
                let g = fbm(u * 4.0 + 3.0, v * 4.0 + 1.0, 11, 6);
                smooth(0.45, 0.62, g) * (1.0 - smooth(0.6, 1.0, r))
            }
            AlphaKind::Spray => {
                let cell = 22.0;
                let (cx, cy) = (((u + 1.0) * cell) as i32, ((v + 1.0) * cell) as i32);
                let h = hash2(cx, cy, 5);
                let fx = (u + 1.0) * cell - cx as f32 - 0.5;
                let fy = (v + 1.0) * cell - cy as f32 - 0.5;
                let dot = 1.0 - smooth(0.15, 0.3, (fx * fx + fy * fy).sqrt());
                if h > 0.55 {
                    dot * (1.0 - smooth(0.5, 1.0, r))
                } else {
                    0.0
                }
            }
            AlphaKind::Line => {
                let w = 0.12 + (fbm(u * 6.0, 0.0, 2, 3) - 0.5) * 0.1;
                (1.0 - smooth(w * 0.6, w, v.abs())) * (1.0 - smooth(0.85, 1.0, u.abs()))
            }
            AlphaKind::Cloud => {
                let g = fbm(u * 2.5 + 5.0, v * 2.5 + 9.0, 4, 6);
                (g * 1.6 - 0.3).clamp(0.0, 1.0) * (1.0 - smooth(0.4, 1.0, r))
            }
            AlphaKind::Chalk => {
                let g = fbm(u * 14.0, v * 14.0, 21, 3);
                let base = 1.0 - smooth(0.75, 1.0, r);
                base * smooth(0.35, 0.55, g)
            }
        };
        let b = (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        image::Rgba([b, b, b, 255])
    })
}

// ---------------------------------------------------------------- stroke engine

/// Camera data needed to place stamps.
pub struct View {
    pub view_proj: Mat4,
    pub inv_view_proj: Mat4,
    pub eye: Vec3,
    /// Physical pixels.
    pub size: Vec2,
    pub fov_y: f32,
    /// Physical pixels per point.
    pub ppp: f32,
    /// Orthographic camera: world units per pixel, constant with depth.
    pub ortho_wpp: Option<f32>,
    /// 2D view rotation (radians): UV stamps turn back by it, so a tip
    /// keeps its angle on screen.
    pub uv_rot: f32,
}

impl View {
    pub fn ray(&self, px: Vec2) -> (Vec3, Vec3) {
        let ndc = Vec2::new(px.x / self.size.x * 2.0 - 1.0, 1.0 - px.y / self.size.y * 2.0);
        let a = self.inv_view_proj * Vec4::new(ndc.x, ndc.y, 0.0, 1.0);
        let b = self.inv_view_proj * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
        let (a, b) = (a.truncate() / a.w, b.truncate() / b.w);
        (a, (b - a).normalize())
    }

    /// World units per physical pixel at distance `d`.
    pub fn world_per_px(&self, d: f32) -> f32 {
        if let Some(w) = self.ortho_wpp {
            return w;
        }
        2.0 * d * (self.fov_y * 0.5).tan() / self.size.y.max(1.0)
    }
}

/// Where stamps are placed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Space {
    Camera,
    Wrap,
    /// 2D view: positions in UV, `uv_per_point` converts sizes.
    Uv,
}

pub struct Stroke {
    pub space: Space,
    /// Last placed point (points, local to the view).
    last: Option<Vec2>,
    /// Distance travelled since the last stamp (points).
    carry: f32,
    /// Lazy mouse anchor.
    lazy_anchor: Option<Vec2>,
    rng: u64,
    pub set: usize,
    pub moved: Vec2,
    /// Pen pressure in [0, 1] (1 for a mouse).
    pub pressure: f32,
    /// A stamp was placed (follow-path strokes place their first one on the
    /// first move, when the direction is known).
    placed: bool,
}

impl Stroke {
    pub fn new(space: Space, set: usize, seed: u64) -> Self {
        Stroke { space, last: None, carry: 0.0, lazy_anchor: None, rng: seed | 1, set, moved: Vec2::ZERO, pressure: 1.0, placed: false }
    }

    pub fn last_point(&self) -> Option<Vec2> {
        self.last
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Applies the lazy mouse; returns the point the stroke should move to.
    pub fn lazy(&mut self, cursor: Vec2, dist: f32) -> Option<Vec2> {
        if dist <= 0.0 {
            return Some(cursor);
        }
        let a = *self.lazy_anchor.get_or_insert(cursor);
        let d = cursor - a;
        let l = d.length();
        if l <= dist {
            return if self.last.is_none() { Some(a) } else { None };
        }
        let n = a + d / l * (l - dist);
        self.lazy_anchor = Some(n);
        Some(n)
    }

    /// Moves the stroke to `to` (points, view-local) and returns new stamps.
    /// `to_uv` maps view points to UV for the 2D view (Space::Uv).
    pub fn advance(
        &mut self,
        to: Vec2,
        brush: &BrushSettings,
        view: &View,
        doc: &Document,
        to_uv: &dyn Fn(Vec2) -> Vec2,
        uv_per_point: f32,
    ) -> Vec<Stamp> {
        let mut pts = Vec::new();
        let step = (brush.size * 2.0 * brush.spacing).max(0.5);
        match self.last {
            None if brush.follow_path => {
                // The first stamp lands here once the stroke has a direction.
                self.last = Some(to);
                self.carry = step;
                return Vec::new();
            }
            None => {
                pts.push((to, Vec2::X));
            }
            Some(last) => {
                let d = to - last;
                let len = d.length();
                if len <= 1e-6 {
                    return Vec::new();
                }
                let dir = d / len;
                let mut t = step - self.carry;
                while t <= len {
                    pts.push((last + dir * t, dir));
                    t += step;
                }
                self.carry = len - (t - step);
                self.moved = dir;
            }
        }
        self.last = Some(to);
        self.placed |= !pts.is_empty();
        let mut out = Vec::new();
        for (p, dir) in pts {
            self.stamp_at(p, dir, brush, view, doc, to_uv, uv_per_point, &mut out);
        }
        out
    }

    /// End of the stroke: a follow-path click that never moved still
    /// places its one stamp.
    pub fn finish(&mut self, brush: &BrushSettings, view: &View, doc: &Document, to_uv: &dyn Fn(Vec2) -> Vec2, uv_per_point: f32) -> Vec<Stamp> {
        let Some(p) = self.last.filter(|_| !self.placed) else { return Vec::new() };
        self.placed = true;
        let mut out = Vec::new();
        self.stamp_at(p, Vec2::X, brush, view, doc, to_uv, uv_per_point, &mut out);
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn stamp_at(
        &mut self,
        p: Vec2,
        dir: Vec2,
        brush: &BrushSettings,
        view: &View,
        doc: &Document,
        to_uv: &dyn Fn(Vec2) -> Vec2,
        uv_per_point: f32,
        out: &mut Vec<Stamp>,
    ) {
        let (r1, r2, r3, r4, r5) = (self.rand(), self.rand(), self.rand(), self.rand(), self.rand());
        let p_size = if brush.pressure_size { self.pressure.clamp(0.05, 1.0) } else { 1.0 };
        let p_flow = if brush.pressure_flow { self.pressure.clamp(0.0, 1.0) } else { 1.0 };
        let size = brush.size * p_size * (1.0 + (r1 * 2.0 - 1.0) * brush.size_jitter).max(0.05);
        let weight = (brush.flow * p_flow * (1.0 - r2 * brush.flow_jitter)).clamp(0.0, 1.0);
        let mut angle = brush.angle.to_radians() + (r3 * 2.0 - 1.0) * brush.angle_jitter.to_radians();
        if brush.follow_path {
            angle += dir.y.atan2(dir.x);
        }
        let jitter = Vec2::new(r4 * 2.0 - 1.0, r5 * 2.0 - 1.0) * brush.position_jitter * brush.size;
        let p = p + jitter;
        match self.space {
            Space::Camera => {
                let px = p * view.ppp;
                out.push(Stamp { pos: [px.x, px.y, 0.0, size * view.ppp], nrm: [0.0, 0.0, 1.0, weight], ext: [angle, 0.0, 0.0, 0.0] });
            }
            Space::Uv => {
                let uv = to_uv(p);
                out.push(Stamp {
                    pos: [uv.x, uv.y, 0.0, size * uv_per_point],
                    nrm: [0.0, 0.0, 1.0, weight],
                    ext: [angle - view.uv_rot, 0.0, 0.0, 0.0],
                });
            }
            Space::Wrap => {
                let px = p * view.ppp;
                let (o, d) = view.ray(px);
                let Some(hit) = doc.pick(o, d) else { return };
                let radius = size * view.ppp * view.world_per_px(hit.dist);
                let mut push = |pos: Vec3, n: Vec3, a: f32| {
                    out.push(Stamp { pos: [pos.x, pos.y, pos.z, radius], nrm: [n.x, n.y, n.z, weight], ext: [a, 0.0, 0.0, 0.0] });
                };
                push(hit.pos, hit.normal, angle);
                let c = (doc.mesh.bbox_min + doc.mesh.bbox_max) * 0.5;
                let axes: Vec<usize> = (0..3).filter(|&a| brush.symmetry[a]).collect();
                // Every combination of the enabled mirror planes.
                for mask in 1u32..(1 << axes.len()) {
                    let mut pos = hit.pos;
                    let mut n = hit.normal;
                    let mut a = angle;
                    for (k, &axis) in axes.iter().enumerate() {
                        if mask & (1 << k) != 0 {
                            pos[axis] = 2.0 * c[axis] - pos[axis];
                            n[axis] = -n[axis];
                            a = -a;
                        }
                    }
                    push(pos, n, a);
                }
            }
        }
    }
}
