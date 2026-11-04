//! Asset thumbnails: materials, smart materials, smart masks and procedurals
//! rendered on a material ball (sphere with a raised band and two grooves,
//! so edge and cavity generators show) with its own small document.

use crate::camera::OrbitCamera;
use crate::document::{Document, Layer, LayerTree, Resource, Target};
use crate::engine::env::Environment;
use crate::engine::texture::f32_to_f16_bytes;
use crate::engine::viewport::{CameraUniform, DrawSet, Frame, PostUniform, SetUniform, Viewport};
use crate::engine::{Engine, Tex};
use crate::mesh::{finalize, MeshData, RawMesh};
use glam::Vec3;
use std::collections::HashMap;

pub const THUMB: u32 = 128;
const RES: u32 = 256;

/// The viewport's key light for an environment turned by `rot` radians:
/// the environment's brightest direction (L1 band of its spherical
/// harmonics), kept above the horizon, at the default intensity (1.2).
/// Same rule as the app's `key_light`.
pub(crate) fn viewport_key_light(env: &Environment, rot: f32) -> [f32; 4] {
    let lum = |c: &[f32; 4]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let sh = &env.sh;
    let d = Vec3::new(lum(&sh[3]), lum(&sh[1]), lum(&sh[2])).normalize_or(Vec3::Y);
    let (s, c) = (-rot).sin_cos();
    let mut dir = Vec3::new(c * d.x + s * d.z, d.y, -s * d.x + c * d.z).normalize_or(Vec3::Y);
    if dir.y < 0.2 {
        dir = Vec3::new(dir.x, 0.2, dir.z).normalize();
    }
    [dir.x, dir.y, dir.z, 1.2]
}

/// Environment rotation that puts the environment's key light in front of
/// the camera, up and to its left (a ball lit from the front, like the
/// library's previews, whatever way the viewport turns the environment).
fn front_lit_rotation(env: &Environment, cam: &OrbitCamera) -> f32 {
    let back = (cam.eye() - Vec3::from(cam.target)).normalize_or(Vec3::Z);
    let right = (-back).cross(Vec3::Y).normalize_or(Vec3::X);
    let want = (back * 0.8 - right * 0.6).with_y(0.0).normalize_or(Vec3::Z);
    let mut best = (f32::MIN, 0.0);
    for k in 0..360 {
        let rot = (k as f32).to_radians();
        let d = viewport_key_light(env, rot);
        let h = Vec3::new(d[0], 0.0, d[2]).normalize_or(Vec3::Z);
        let s = h.dot(want);
        if s > best.0 {
            best = (s, rot);
        }
    }
    best.1
}

/// Subdivided cube pushed toward a rounded box, one UV island per face.
pub fn rounded_cube() -> MeshData {
    let n = 24usize;
    let r = 0.28f32;
    let mut raw = RawMesh::default();
    let faces: [(Vec3, Vec3, Vec3); 6] = [
        (Vec3::X, Vec3::NEG_Z, Vec3::Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::NEG_Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
    ];
    let mut idx = Vec::new();
    for (fi, (nrm, u, v)) in faces.iter().enumerate() {
        let (cx, cy) = ((fi % 3) as f32 / 3.0, (fi / 3) as f32 / 2.0);
        let base = raw.positions.len() as u32;
        for j in 0..=n {
            for i in 0..=n {
                let a = i as f32 / n as f32 * 2.0 - 1.0;
                let b = j as f32 / n as f32 * 2.0 - 1.0;
                let p = *nrm + *u * a + *v * b;
                let inner = p.clamp(Vec3::splat(-1.0 + r), Vec3::splat(1.0 - r));
                let d = (p - inner).normalize_or(*nrm);
                let q = inner + d * r;
                raw.positions.push(q.into());
                raw.normals.push(d.into());
                let tu = cx + (a * 0.5 + 0.5) * (1.0 / 3.0 - 0.02) + 0.01;
                let tv = cy + (1.0 - (b * 0.5 + 0.5)) * (0.5 - 0.02) + 0.01;
                raw.uvs.push([tu, tv]);
            }
        }
        for j in 0..n as u32 {
            for i in 0..n as u32 {
                let a = base + j * (n as u32 + 1) + i;
                let b = a + n as u32 + 1;
                idx.extend_from_slice(&[a, a + 1, b + 1, a, b + 1, b]);
            }
        }
    }
    let ntri = idx.len() / 3;
    raw.groups.push(("Preview".into(), idx, vec![0; ntri]));
    finalize(raw, crate::mesh::AutoUv::IfMissing).expect("preview mesh")
}

/// Material previews: a plain sphere with lat-long UVs, like Painter's.
pub fn material_ball() -> MeshData {
    use std::f32::consts::PI;
    let (nu, nv) = (96usize, 96usize);
    let mut raw = RawMesh::default();
    for j in 0..=nv {
        let t = j as f32 / nv as f32 * PI;
        let (dr, d1) = (0.0f32, 0.0f32);
        let r = 1.0 + dr;
        for i in 0..=nu {
            let f = i as f32 / nu as f32 * 2.0 * PI;
            let er = Vec3::new(t.sin() * f.cos(), t.cos(), t.sin() * f.sin());
            let et = Vec3::new(t.cos() * f.cos(), -t.sin(), t.cos() * f.sin());
            let n = (er * r - et * d1).normalize_or(er);
            raw.positions.push((er * r).into());
            raw.normals.push(n.into());
            raw.uvs.push([i as f32 / nu as f32, j as f32 / nv as f32]);
        }
    }
    let mut idx = Vec::new();
    for j in 0..nv as u32 {
        for i in 0..nu as u32 {
            let a = j * (nu as u32 + 1) + i;
            let b = a + nu as u32 + 1;
            idx.extend_from_slice(&[a, b + 1, a + 1, a, b, b + 1]);
        }
    }
    let ntri = idx.len() / 3;
    raw.groups.push(("Preview".into(), idx, vec![0; ntri]));
    finalize(raw, crate::mesh::AutoUv::IfMissing).expect("preview mesh")
}

/// A dense sphere displaced by a height map (material previews): UVs with
/// the texel aspect kept (u runs twice the length of v), `tiles` repeats
/// top to bottom, the surface pushed out by `amount` (radius 1) times the
/// height around its mean. Normals are rebuilt from the displaced shape.
pub fn displaced_sphere(height: Option<&image::RgbaImage>, tiles: f32, amount: f32) -> MeshData {
    use std::f32::consts::PI;
    let (nu, nv) = (640usize, 320usize);
    let sample = |u: f32, v: f32| -> f32 {
        let Some(h) = height else { return 0.0 };
        let (w, hh) = (h.width() as f32, h.height() as f32);
        let (x, y) = ((u.rem_euclid(1.0) * w - 0.5).rem_euclid(w), (v.rem_euclid(1.0) * hh - 0.5).rem_euclid(hh));
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let px = |xx: f32, yy: f32| h.get_pixel(xx.rem_euclid(w) as u32, yy.rem_euclid(hh) as u32).0[0] as f32 / 255.0;
        let a = px(x0, y0) * (1.0 - fx) + px(x0 + 1.0, y0) * fx;
        let b = px(x0, y0 + 1.0) * (1.0 - fx) + px(x0 + 1.0, y0 + 1.0) * fx;
        a * (1.0 - fy) + b * fy
    };
    let mean = height.map_or(0.0, |h| h.pixels().map(|p| p.0[0] as f64).sum::<f64>() as f32 / (h.width() * h.height()) as f32 / 255.0);
    let mut raw = RawMesh::default();
    for j in 0..=nv {
        let t = j as f32 / nv as f32 * PI;
        for i in 0..=nu {
            let f = i as f32 / nu as f32 * 2.0 * PI;
            let er = Vec3::new(t.sin() * f.cos(), t.cos(), t.sin() * f.sin());
            let uv = [i as f32 / nu as f32 * 2.0 * tiles, j as f32 / nv as f32 * tiles];
            // The poles stay put (no cracks where the rows converge).
            let pole = (t.sin() * 6.0).min(1.0);
            let r = 1.0 + (sample(uv[0], uv[1]) - mean) * amount * pole;
            raw.positions.push((er * r).into());
            raw.uvs.push(uv);
        }
    }
    let mut idx = Vec::new();
    for j in 0..nv as u32 {
        for i in 0..nu as u32 {
            let a = j * (nu as u32 + 1) + i;
            let b = a + nu as u32 + 1;
            idx.extend_from_slice(&[a, b + 1, a + 1, a, b, b + 1]);
        }
    }
    let ntri = idx.len() / 3;
    raw.groups.push(("Preview".into(), idx, vec![0; ntri]));
    finalize(raw, crate::mesh::AutoUv::IfMissing).expect("preview mesh")
}

/// Flat square facing the camera (procedural texture thumbnails).
pub fn flat_square() -> MeshData {
    let mut raw = RawMesh::default();
    let n = 8usize;
    for j in 0..=n {
        for i in 0..=n {
            let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
            raw.positions.push([u * 2.0 - 1.0, 1.0 - v * 2.0, 0.0]);
            raw.normals.push([0.0, 0.0, 1.0]);
            raw.uvs.push([u, v]);
        }
    }
    let mut idx = Vec::new();
    for j in 0..n as u32 {
        for i in 0..n as u32 {
            let a = j * (n as u32 + 1) + i;
            let b = a + n as u32 + 1;
            idx.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    let ntri = idx.len() / 3;
    raw.groups.push(("Preview".into(), idx, vec![0; ntri]));
    finalize(raw, crate::mesh::AutoUv::IfMissing).expect("preview square")
}

/// The viewport's look (environment rotation in degrees, exposure in stops,
/// tone mapping, grading, key light intensity): layer and brush material
/// previews follow it so a sphere looks like the layer on the model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub env_rotation: f32,
    pub exposure: f32,
    pub tonemap: u32,
    pub saturation: f32,
    pub contrast: f32,
    pub light_intensity: f32,
}

impl Default for Look {
    /// The viewport's defaults.
    fn default() -> Self {
        Look { env_rotation: 180.0, exposure: 0.0, tonemap: 3, saturation: 1.0, contrast: 1.0, light_intensity: 1.2 }
    }
}

impl Look {
    /// Stable bits for thumbnail cache keys (the environment rotation is
    /// left out: previews turn the environment to light their front).
    pub fn key(&self) -> [u32; 6] {
        [0, self.exposure.to_bits(), self.tonemap, self.saturation.to_bits(), self.contrast.to_bits(), self.light_intensity.to_bits()]
    }
}

pub struct Previewer {
    /// Scene lit previews: the viewport look they copy.
    pub look: Look,
    doc: Document,
    viewport: Viewport,
    env: Environment,
    camera: OrbitCamera,
    /// Output size in pixels.
    size: u32,
    /// Lit by the viewport's environment (texture materials) instead of
    /// the built-in studio; name of the one bound.
    scene_env: Option<String>,
}

impl Previewer {
    pub fn new(engine: &Engine) -> Self {
        Self::with_mesh(engine, material_ball(), true, RES, THUMB)
    }

    /// A sphere preview of `size` pixels on a texture set of `res`
    /// (large hover previews, material checks).
    pub fn sized(engine: &Engine, res: u32, size: u32) -> Self {
        Self::with_mesh(engine, material_ball(), true, res, size)
    }

    /// A large texture material sphere (checks against the thumbnail).
    pub fn sized_material(engine: &Engine, res: u32, size: u32) -> Self {
        let mut p = Self::with_mesh(engine, material_ball(), true, res, size);
        p.scene_env = Some(String::new());
        p
    }

    /// Texture materials: lit by the viewport's environment, on a finer set
    /// so their tiling stays sharp, like they look once applied.
    pub fn new_material(engine: &Engine) -> Self {
        let mut p = Self::with_mesh(engine, material_ball(), true, 1024, THUMB);
        p.scene_env = Some(String::new());
        p
    }

    /// Fill layer rows: a ball filling the frame in the studio light, like
    /// the library's material thumbnails (not graded like the viewport).
    pub fn new_layer(engine: &Engine) -> Self {
        let mut p = Self::with_mesh(engine, material_ball(), true, 1024, THUMB);
        p.camera.distance *= 0.8;
        p
    }

    /// Procedural textures preview flat, seen straight on.
    pub fn new_flat(engine: &Engine) -> Self {
        let mut p = Self::with_mesh(engine, flat_square(), false, RES, THUMB);
        p.camera = OrbitCamera { yaw: 0.0, pitch: 0.0, ortho: true, ..Default::default() };
        p.camera.frame(glam::Vec3::new(-1.0, -1.0, 0.0), glam::Vec3::new(1.0, 1.0, 0.0));
        p.camera.distance *= 0.72;
        p
    }

    /// Smart materials and smart masks preview on a rounded box: its edges
    /// show wear and dirt the way the real application's previews do.
    pub fn new_box(engine: &Engine) -> Self {
        Self::with_mesh(engine, rounded_cube(), false, RES, THUMB)
    }

    /// A large rounded box (smart material checks).
    pub fn sized_box(engine: &Engine, res: u32, size: u32) -> Self {
        Self::with_mesh(engine, rounded_cube(), false, res, size)
    }

    fn with_mesh(engine: &Engine, mesh: MeshData, ball: bool, res: u32, size: u32) -> Self {
        let (min, max) = (mesh.bbox_min, mesh.bbox_max);
        let mut doc = Document::new(engine, mesh, "preview".into(), res);
        doc.sets[0].layers.clear();
        // Analytical curvature: rounded edges read as convex.
        doc.sets[0].gpu.alloc_baked(&engine.device, Some(&engine.queue));
        let s = &doc.sets[0];
        let curv: Vec<f32> = (0..s.raster.tri.len())
            .map(|i| match crate::bake::surface(&doc.mesh, 0, &s.raster, i) {
                // A sphere has no edges.
                Some(_) if ball => 0.5,
                Some(sf) => {
                    // Rounded box: the bevels read as convex edges.
                    let n = sf.normal.abs();
                    let e = 1.0 - n.x.max(n.y).max(n.z);
                    0.5 + 0.5 * (e * 5.0).clamp(0.0, 1.0)
                }
                None => 0.5,
            })
            .collect();
        s.gpu.baked_curvature.write_bytes(&engine.queue, &f32_to_f16_bytes(&curv));
        let env = Environment::studio(&engine.device, &engine.queue);
        let mut viewport = Viewport::new(&engine.device, &engine.queue, &env);
        viewport.resize(&engine.device, size, size);
        let mut camera = if ball {
            OrbitCamera { yaw: 0.4, pitch: 0.22, ..Default::default() }
        } else {
            OrbitCamera { yaw: 0.65, pitch: 0.45, ..Default::default() }
        };
        camera.frame(min, max);
        camera.distance *= if ball { 0.8 } else { 0.84 };
        Self { look: Look::default(), doc, viewport, env, camera, size, scene_env: None }
    }

    /// A fill layer's thumbnail (layer stack, brush material): this
    /// (scene lit) previewer graded like the viewport (`look`). The ball's
    /// UVs run around it once and pole to pole once: twice as many repeats
    /// around keeps UV projected maps' texels square. `set`: the texture set
    /// the layer is on, whose base values show wherever the fill writes no
    /// channel (as on the model).
    pub fn render_fill(&mut self, engine: &Engine, tree: &LayerTree, resources: &HashMap<u64, Resource>, look: Look, set: Option<&crate::document::SetSettings>) -> Tex {
        self.look = look;
        let mine = &mut self.doc.sets[0].settings;
        let from = set.cloned().unwrap_or_default();
        mine.base_color = from.base_color;
        mine.base_values = from.base_values;
        mine.height_scale = from.height_scale;
        mine.emissive_intensity = from.emissive_intensity;
        mine.ext_channels = from.ext_channels;
        let mut tree = tree.clone();
        if let crate::document::LayerKind::Fill(f) = &mut tree.desc.kind {
            if f.projection.mode == crate::document::ProjMode::Uv {
                f.projection.stretch[0] *= 2.0;
            }
        }
        self.render(engine, &tree, resources)
    }

    /// Renders `layers` (or a white fill under `mask_tree` for masks) and
    /// returns a texture ready for egui.
    pub fn render(&mut self, engine: &Engine, tree: &LayerTree, resources: &HashMap<u64, Resource>) -> Tex {
        let layer: Layer = self.doc.instantiate(engine, 0, tree);
        let set = &mut self.doc.sets[0];
        set.layers = vec![layer];
        set.selected = None;
        set.target = Target::Content;
        let mut enc = engine.device.create_command_encoder(&Default::default());
        set.composite(engine, &mut enc, resources, None);
        let scene = self.scene_env.is_some();
        if let Some(name) = &self.scene_env {
            if *name != engine.env.name {
                self.viewport.set_environment(&engine.device, &engine.env);
                self.scene_env = Some(engine.env.name.clone());
            }
        }
        let cam = &self.camera;
        let vp = cam.view_proj(1.0);
        let env = if scene { &engine.env } else { &self.env };
        // Scene lit previews: lit and graded like the viewport (its
        // environment, the key light along the environment's brightest
        // direction, exposure, tone mapping), the environment turned so that
        // light falls on the ball's front: a ball lit from behind reads as
        // a dark, wrong material.
        let look = self.look;
        let (env_rot, key) = if scene {
            let rot = front_lit_rotation(env, cam);
            let mut k = viewport_key_light(env, rot);
            k[3] = look.light_intensity;
            (rot, k)
        } else {
            (0.6, [0.3, 1.0, 0.4, 0.0])
        };
        let exposure = if scene { 2f32.powf(look.exposure) } else { 1.0 };
        let frame = Frame {
            camera: CameraUniform {
                view_proj: vp.to_cols_array_2d(),
                inv_view_proj: vp.inverse().to_cols_array_2d(),
                eye: cam.eye().extend(self.doc.mesh.diagonal()).into(),
                // The viewport's default rotation for its environment; a
                // flat background (the landscape would distract).
                env: [env_rot, exposure, 4.0, (env.mip_count as f32 - 3.0).max(1.0)],
                display: [0.0, 1.0, 0.0, 1.0],
                sh: env.sh,
                light_view_proj: glam::Mat4::IDENTITY.to_cols_array_2d(),
                light: key,
                wire: [0.85, 0.85, 0.9, 0.35],
            },
            post: if scene {
                PostUniform { a: [exposure, look.tonemap as f32, 0.0, 1.5], b: [0.0, look.saturation, look.contrast, 0.0], texel: [0.0; 4] }
            } else {
                PostUniform { a: [1.0, 0.0, 0.0, 1.0], b: [0.0, 1.0, 1.0, 0.0], texel: [0.0; 4] }
            },
            mesh: Some(&self.doc.gpu_mesh),
            sets: vec![DrawSet {
                submesh: 0,
                base: &set.gpu.final_base,
                orm: &set.gpu.final_orm,
                normal: &set.gpu.final_normal,
                emissive: &set.gpu.final_emissive,
                display: &set.gpu.final_base,
                params: SetUniform { k: [0.0, 0.5, 1.0, 0.0] },
                visible: true,
                transmissive: false,
            }],
            shadows: false,
            wireframe: false,
            wire_only: None,
            selection: None,
            path_traced: false,
        };
        self.viewport.render(&engine.device, &engine.queue, &mut enc, &frame);
        let out = Tex::new(
            &engine.device,
            "thumbnail",
            self.size,
            self.size,
            crate::engine::viewport::OUT_FORMAT,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        );
        self.viewport.output().copy_to(&mut enc, &out);
        engine.queue.submit(Some(enc.finish()));
        // Free the per-thumbnail layer textures.
        self.doc.sets[0].layers.clear();
        out
    }
}
