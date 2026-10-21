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
}

pub fn builtin_brushes() -> Vec<BrushPreset> {
    let b = |name: &str, f: &dyn Fn(&mut BrushSettings)| {
        let mut s = BrushSettings::default();
        f(&mut s);
        BrushPreset { name: name.into(), settings: s }
    };
    vec![
        b("Basic hard", &|s| {
            s.hardness = 0.95;
            s.flow = 1.0;
        }),
        b("Basic soft", &|s| {
            s.hardness = 0.0;
            s.flow = 0.5;
        }),
        b("Airbrush", &|s| {
            s.hardness = 0.0;
            s.flow = 0.08;
            s.size = 60.0;
        }),
        b("Square", &|s| {
            s.alpha = AlphaSource::Builtin(AlphaKind::Square);
            s.flow = 1.0;
        }),
        b("Chalk", &|s| {
            s.alpha = AlphaSource::Builtin(AlphaKind::Chalk);
            s.angle_jitter = 180.0;
            s.flow = 0.7;
        }),
        b("Spray", &|s| {
            s.alpha = AlphaSource::Builtin(AlphaKind::Spray);
            s.angle_jitter = 180.0;
            s.spacing = 0.3;
            s.flow = 0.8;
        }),
        b("Splatter", &|s| {
            s.alpha = AlphaSource::Builtin(AlphaKind::Splatter);
            s.angle_jitter = 180.0;
            s.size_jitter = 0.5;
            s.position_jitter = 0.6;
            s.spacing = 0.6;
            s.flow = 1.0;
        }),
        b("Grunge dabber", &|s| {
            s.alpha = AlphaSource::Builtin(AlphaKind::Grunge);
            s.angle_jitter = 180.0;
            s.spacing = 0.5;
            s.flow = 0.8;
        }),
        b("Scratch", &|s| {
            s.alpha = AlphaSource::Builtin(AlphaKind::Line);
            s.follow_path = true;
            s.spacing = 0.05;
            s.flow = 0.9;
            s.size = 18.0;
        }),
        b("Star stamp", &|s| {
            s.alpha = AlphaSource::Builtin(AlphaKind::Star);
            s.spacing = 1.2;
            s.flow = 1.0;
        }),
        b("Clouds", &|s| {
            s.alpha = AlphaSource::Builtin(AlphaKind::Cloud);
            s.angle_jitter = 180.0;
            s.size_jitter = 0.3;
            s.flow = 0.35;
            s.spacing = 0.25;
        }),
        b("Ring", &|s| {
            s.alpha = AlphaSource::Builtin(AlphaKind::Ring);
            s.spacing = 1.0;
            s.flow = 1.0;
        }),
    ]
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
}

impl Stroke {
    pub fn new(space: Space, set: usize, seed: u64) -> Self {
        Stroke { space, last: None, carry: 0.0, lazy_anchor: None, rng: seed | 1, set, moved: Vec2::ZERO, pressure: 1.0 }
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
        let mut out = Vec::new();
        for (p, dir) in pts {
            self.stamp_at(p, dir, brush, view, doc, to_uv, uv_per_point, &mut out);
        }
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
                    ext: [angle, 0.0, 0.0, 0.0],
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
