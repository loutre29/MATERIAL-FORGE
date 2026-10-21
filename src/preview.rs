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
    finalize(raw).expect("preview mesh")
}

/// Profile of the material ball: radius offset at polar angle `t` and its
/// first and second derivatives.
fn ball_profile(t: f32) -> (f32, f32, f32) {
    use std::f32::consts::PI;
    let mut r = (0.0, 0.0, 0.0);
    for (c, a, w) in [(PI * 0.5, 0.045f32, 0.07f32), (PI * 0.3, -0.03, 0.045), (PI * 0.7, -0.03, 0.045)] {
        let x = t - c;
        let g = (-(x / w) * (x / w)).exp();
        r.0 += a * g;
        r.1 += a * g * (-2.0 * x / (w * w));
        r.2 += a * g * (4.0 * x * x / (w * w * w * w) - 2.0 / (w * w));
    }
    r
}

/// Material ball: surface of revolution with lat-long UVs.
pub fn material_ball() -> MeshData {
    use std::f32::consts::PI;
    let (nu, nv) = (96usize, 96usize);
    let mut raw = RawMesh::default();
    for j in 0..=nv {
        let t = j as f32 / nv as f32 * PI;
        let (dr, d1, _) = ball_profile(t);
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
    finalize(raw).expect("preview mesh")
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
    finalize(raw).expect("preview square")
}

pub struct Previewer {
    doc: Document,
    viewport: Viewport,
    env: Environment,
    camera: OrbitCamera,
}

impl Previewer {
    pub fn new(engine: &Engine) -> Self {
        Self::with_mesh(engine, material_ball(), true)
    }

    /// Procedural textures preview flat, seen straight on.
    pub fn new_flat(engine: &Engine) -> Self {
        let mut p = Self::with_mesh(engine, flat_square(), false);
        p.camera = OrbitCamera { yaw: 0.0, pitch: 0.0, ortho: true, ..Default::default() };
        p.camera.frame(glam::Vec3::new(-1.0, -1.0, 0.0), glam::Vec3::new(1.0, 1.0, 0.0));
        p.camera.distance *= 0.72;
        p
    }

    /// Smart materials and smart masks preview on a rounded box: its edges
    /// show wear and dirt the way the real application's previews do.
    pub fn new_box(engine: &Engine) -> Self {
        Self::with_mesh(engine, rounded_cube(), false)
    }

    fn with_mesh(engine: &Engine, mesh: MeshData, ball: bool) -> Self {
        let (min, max) = (mesh.bbox_min, mesh.bbox_max);
        let mut doc = Document::new(engine, mesh, "preview".into(), RES);
        doc.sets[0].layers.clear();
        // Analytical curvature: rounded edges read as convex.
        doc.sets[0].gpu.alloc_baked(&engine.device, Some(&engine.queue));
        let s = &doc.sets[0];
        let curv: Vec<f32> = (0..s.raster.tri.len())
            .map(|i| match crate::bake::surface(&doc.mesh, 0, &s.raster, i) {
                Some(sf) if ball => {
                    // Ridges (negative second derivative) read as convex.
                    let t = (sf.pos.y / sf.pos.length().max(1e-6)).clamp(-1.0, 1.0).acos();
                    0.5 + (-ball_profile(t).2 * 0.02).clamp(-0.5, 0.5)
                }
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
        viewport.resize(&engine.device, THUMB, THUMB);
        let mut camera = if ball {
            OrbitCamera { yaw: 0.4, pitch: 0.22, ..Default::default() }
        } else {
            OrbitCamera { yaw: 0.65, pitch: 0.45, ..Default::default() }
        };
        camera.frame(min, max);
        camera.distance *= if ball { 0.8 } else { 0.84 };
        Self { doc, viewport, env, camera }
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
        let cam = &self.camera;
        let vp = cam.view_proj(1.0);
        let env = &self.env;
        let frame = Frame {
            camera: CameraUniform {
                view_proj: vp.to_cols_array_2d(),
                inv_view_proj: vp.inverse().to_cols_array_2d(),
                eye: cam.eye().extend(self.doc.mesh.diagonal()).into(),
                env: [0.6, 1.0, 4.0, (env.mip_count as f32 - 3.0).max(1.0)],
                display: [0.0, 1.0, 0.0, 1.0],
                sh: env.sh,
                light_view_proj: glam::Mat4::IDENTITY.to_cols_array_2d(),
                light: [0.3, 1.0, 0.4, 0.0],
            },
            post: PostUniform { a: [1.0, 0.0, 0.0, 1.0], b: [0.0, 1.0, 1.0, 0.0], texel: [0.0; 4] },
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
            }],
            shadows: false,
            wireframe: false,
            path_traced: false,
        };
        self.viewport.render(&engine.device, &engine.queue, &mut enc, &frame);
        let out = Tex::new(
            &engine.device,
            "thumbnail",
            THUMB,
            THUMB,
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
