//! `sp_retro --selftest <out_dir> [mesh]`: runs the full pipeline headless
//! and checks the results: import, bake, a layer tree with a smart material,
//! fills, filters, mask stacks, brush strokes (wrap, camera, UV), polygon
//! fill, history, every export preset and format, project save / reload,
//! resolution change and shelf thumbnails.

use crate::bake::{self, BakeSettings};
use crate::brush::{BrushSettings, Space, Stroke, View};
use crate::camera::OrbitCamera;
use crate::document::*;
use crate::engine::paint::{BrushUniform, PaintPass, Stamp, MODE_CAMERA, MODE_FULL, MODE_UV, MODE_WRAP};
use crate::engine::texture::f16_bytes_to_f32;
use crate::engine::viewport::{CameraUniform, DrawSet, Frame, PostUniform, SetUniform};
use crate::engine::{Engine, Tex};
use crate::export::{self, ExportOptions, FileFormat, Padding};
use crate::history::{Entry, History};
use crate::{library, mesh, project};
use anyhow::{ensure, Context, Result};
use glam::Vec2;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;
use std::time::Instant;
use wgpu::util::DeviceExt;

/// A hard-edged cube (material "Box") and a UV sphere (material "Ball").
pub fn test_obj() -> String {
    let mut s = String::from("# selftest\no BoxObject\n");
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ];
    let mut v = 0;
    let mut vt = 0;
    s.push_str("usemtl Box\n");
    for (fi, (n, u, w)) in faces.iter().enumerate() {
        let (cx, cy) = ((fi % 3) as f32 / 3.0, (fi / 3) as f32 / 2.0);
        let _ = writeln!(s, "vn {} {} {}", n[0], n[1], n[2]);
        for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = [n[0] + u[0] * a + w[0] * b - 2.5, n[1] + u[1] * a + w[1] * b, n[2] + u[2] * a + w[2] * b];
            let _ = writeln!(s, "v {} {} {}", p[0], p[1], p[2]);
            let tu = cx + (a * 0.5 + 0.5) * (1.0 / 3.0 - 0.02) + 0.01;
            let tv = 1.0 - (cy + (1.0 - (b * 0.5 + 0.5)) * (0.5 - 0.02) + 0.01);
            let _ = writeln!(s, "vt {tu} {tv}");
        }
        let n_idx = fi + 1;
        let _ = writeln!(
            s,
            "f {}/{}/{n_idx} {}/{}/{n_idx} {}/{}/{n_idx} {}/{}/{n_idx}",
            v + 1,
            vt + 1,
            v + 2,
            vt + 2,
            v + 3,
            vt + 3,
            v + 4,
            vt + 4
        );
        v += 4;
        vt += 4;
    }
    s.push_str("o BallObject\nusemtl Ball\n");
    let (seg, rings) = (48, 24);
    let base = v;
    for r in 0..=rings {
        let th = r as f32 / rings as f32 * std::f32::consts::PI;
        for k in 0..=seg {
            let ph = k as f32 / seg as f32 * std::f32::consts::TAU;
            let d = [th.sin() * ph.cos(), th.cos(), th.sin() * ph.sin()];
            let _ = writeln!(s, "v {} {} {}", d[0] * 1.2 + 1.5, d[1] * 1.2, d[2] * 1.2);
            let _ = writeln!(s, "vn {} {} {}", d[0], d[1], d[2]);
            let _ = writeln!(s, "vt {} {}", k as f32 / seg as f32, 1.0 - r as f32 / rings as f32);
        }
    }
    let row = seg + 1;
    for r in 0..rings {
        for k in 0..seg {
            let a = base + r * row + k + 1;
            let b = a + row;
            let ni = |i: usize| i - base + 6;
            let ti = |i: usize| i - base + 24;
            let _ = writeln!(
                s,
                "f {a}/{}/{} {}/{}/{} {}/{}/{} {b}/{}/{}",
                ti(a),
                ni(a),
                a + 1,
                ti(a + 1),
                ni(a + 1),
                b + 1,
                ti(b + 1),
                ni(b + 1),
                ti(b),
                ni(b)
            );
        }
    }
    s
}

fn frame_for<'a>(engine: &Engine, doc: &'a Document, cam: &OrbitCamera, display: f32, kind: f32, disp: &dyn Fn(&'a TextureSet) -> &'a Tex) -> Frame<'a> {
    let vp = &engine.viewport;
    let aspect = vp.width as f32 / vp.height as f32;
    let m = cam.view_proj(aspect);
    let env = &engine.env;
    Frame {
        camera: CameraUniform {
            view_proj: m.to_cols_array_2d(),
            inv_view_proj: m.inverse().to_cols_array_2d(),
            eye: cam.eye().extend(doc.mesh.diagonal()).into(),
            env: [0.0, 1.0, 4.0, (env.mip_count as f32 - 3.0).max(1.0)],
            display: [display, 0.0, if display == 0.0 { 0.7 } else { 0.0 }, 1.0],
            sh: env.sh,
            light_view_proj: glam::Mat4::IDENTITY.to_cols_array_2d(),
            light: [0.3, 1.0, 0.4, 1.0],
            wire: [0.85, 0.85, 0.9, 0.35],
        },
        post: PostUniform { a: [1.0, 0.0, 0.05, 1.5], b: [0.1, 1.0, 1.0, if display > 0.5 { 1.0 } else { 0.0 }], texel: [0.0; 4] },
        mesh: Some(&doc.gpu_mesh),
        sets: doc
            .sets
            .iter()
            .map(|s| DrawSet {
                submesh: s.submesh,
                base: &s.gpu.final_base,
                orm: &s.gpu.final_orm,
                normal: &s.gpu.final_normal,
                emissive: &s.gpu.final_emissive,
                display: disp(s),
                params: SetUniform { k: [0.0, 0.5, 1.0, kind] },
                visible: true,
                transmissive: false,
            })
            .collect(),
        shadows: display == 0.0,
        wireframe: display == 0.0,
        wire_only: None,
        selection: None,
        path_traced: false,
    }
}

fn render(engine: &Engine, doc: &Document, cam: &OrbitCamera, display: f32, kind: f32, disp: &dyn Fn(&TextureSet) -> &Tex) {
    let mut enc = engine.device.create_command_encoder(&Default::default());
    let frame = frame_for(engine, doc, cam, display, kind, disp);
    engine.viewport.render(&engine.device, &engine.queue, &mut enc, &frame);
    engine.queue.submit(Some(enc.finish()));
}

/// Pins a closure's signature so it may return a borrow of its argument.
fn pick_fn<F: Fn(&TextureSet) -> &Tex>(f: F) -> F {
    f
}

pub(crate) fn screenshot(engine: &Engine, t: &Tex, path: &Path) -> Result<()> {
    let bytes = t.read_bytes(&engine.device, &engine.queue);
    image::RgbaImage::from_raw(t.width, t.height, bytes).context("screenshot size")?.save(path)?;
    Ok(())
}

fn composite_all(engine: &Engine, doc: &mut Document) {
    let mut enc = engine.device.create_command_encoder(&Default::default());
    let res = &doc.resources;
    for s in doc.sets.iter_mut() {
        let cap = s.selected;
        s.gpu.ensure_mask_view(&engine.device);
        s.composite(engine, &mut enc, res, cap);
    }
    engine.queue.submit(Some(enc.finish()));
}

fn uniform(cam: &OrbitCamera, engine: &Engine, mode: f32, color: [f32; 3]) -> BrushUniform {
    let vp = &engine.viewport;
    let (near, far) = cam.near_far();
    BrushUniform {
        view_proj: cam.view_proj(vp.width as f32 / vp.height as f32).to_cols_array_2d(),
        eye: cam.eye().extend(1.0).into(),
        viewport: [vp.width as f32, vp.height as f32, near, far],
        color: [color[0], color[1], color[2], 1.0],
        values: [0.2, 0.0, 0.6, 1.0],
        emissive: [2.0, 1.0, 0.2, 0.0],
        normal: [0.0, 0.0, 1.0, 0.0],
        params: [0.5, 1.0, 0.0, mode],
        shape: [1.0, 0.0, 0.0, 0.0],
        stencil: [0.0; 4],
        stencil2: [1.0, 0.0, 0.0, 0.0],
        right: {
            let fwd = (glam::Vec3::from(cam.target) - cam.eye()).normalize();
            let r = fwd.cross(glam::Vec3::Y).normalize();
            [r.x, r.y, r.z, 0.0]
        },
        material: [0.0; 4],
    }
}

fn paint_targets(l: &Layer, pass: PaintPass) -> Vec<&Tex> {
    let p = l.paint.as_ref().unwrap();
    match pass {
        PaintPass::PaintA | PaintPass::EraseA => vec![&p.color, &p.vals, &p.alph],
        _ => vec![p.emis.as_deref().unwrap(), p.nrm.as_deref().unwrap(), p.opac.as_deref().unwrap()],
    }
}

fn count(engine: &Engine, t: &Tex, f: impl Fn(&[f32]) -> bool) -> usize {
    let v = f16_bytes_to_f32(&t.read_bytes(&engine.device, &engine.queue));
    let ch = (t.bytes_per_pixel() / 2) as usize;
    v.chunks_exact(ch).filter(|p| f(p)).count()
}

pub fn run(out: &Path, mesh_path: Option<&Path>) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let strict = mesh_path.is_none();
    let t0 = Instant::now();
    let mut engine = Engine::headless()?;
    println!("adapter: {}", engine.adapter_name);

    let path = match mesh_path {
        Some(p) => p.to_path_buf(),
        None => {
            let p = out.join("selftest.obj");
            std::fs::write(&p, test_obj())?;
            p
        }
    };
    let m = mesh::load(&path)?;
    println!(
        "mesh: {} verts, {} tris, sets {:?}, objects {:?} ({:.2}s)",
        m.vertices.len(),
        m.triangle_count(),
        m.submeshes.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        m.objects,
        t0.elapsed().as_secs_f32()
    );
    for n in &m.notes {
        println!("note: {n}");
    }
    let res = 1024;
    let mut doc = Document::new(&engine, m, "selftest".into(), res);

    // Bake (2x2 antialiasing, ID by object)
    let t = Instant::now();
    let tris = bake::mesh_triangles(&doc.mesh);
    let bvh = bake::build_bvh(&doc.mesh, &tris);
    let settings = BakeSettings { ao_samples: 12, thickness_samples: 6, antialias: 2, ..Default::default() };
    let (p, c) = (AtomicU32::new(0), AtomicBool::new(false));
    for i in 0..doc.sets.len() {
        let (sub, r) = (doc.sets[i].submesh, doc.sets[i].resolution());
        let gb = crate::engine::gpubake::GpuBaker::new(engine.device.clone(), engine.queue.clone(), &bvh);
        let b = bake::bake(&doc.mesh, sub, r, &bvh, None, &settings, &p, &c, Some(&gb));
        let covered = doc.sets[i].raster.tri.iter().filter(|t| **t >= 0).count().max(1);
        let ao_mean = b.ao.iter().zip(&doc.sets[i].raster.tri).filter(|(_, t)| **t >= 0).map(|(a, _)| *a).sum::<f32>() / covered as f32;
        println!("bake '{}': mean AO {:.3}, edges {}", doc.sets[i].name, ao_mean, b.curvature.iter().filter(|c| **c > 0.75).count());
        ensure!(b.ao.iter().all(|v| v.is_finite()), "AO has NaN");
        crate::app::apply_bake(&engine, &mut doc.sets[i], &b);
    }
    println!("bake time {:.2}s", t.elapsed().as_secs_f32());

    // High poly normal transfer: a rounded cube over the hard box.
    if strict {
        ensure!(bake::name_key("Bolt_HP") == "bolt" && bake::name_key("bolt_low") == "bolt", "name keys");
        let mut hp = crate::preview::rounded_cube();
        for v in hp.vertices.iter_mut() {
            v.pos[0] -= 2.5;
        }
        hp.bbox_min.x -= 2.5;
        hp.bbox_max.x -= 2.5;
        let high = bake::HighPoly::new(hp);
        let hs = BakeSettings { ao_samples: 4, thickness_samples: 2, cage_distance: 0.05, match_by_name: true, ..Default::default() };
        let r = doc.sets[0].resolution();
        let b = bake::bake(&doc.mesh, doc.sets[0].submesh, r, &bvh, Some(&high), &hs, &p, &c, None);
        let tilted = b.normal.iter().zip(&doc.sets[0].raster.tri).filter(|(n, t)| **t >= 0 && n[2] < 0.95).count();
        println!("high poly bake: {tilted} texels with tilted normals");
        let img = image::RgbImage::from_fn(r, r, |x, y| {
            let n = b.normal[(y * r + x) as usize];
            image::Rgb([((n[0] * 0.5 + 0.5) * 255.0) as u8, ((n[1] * 0.5 + 0.5) * 255.0) as u8, ((n[2] * 0.5 + 0.5) * 255.0) as u8])
        });
        img.save(out.join("08_high_poly_normal.png"))?;
        ensure!(tilted > 5000, "high poly normals were not transferred");
    }

    // Layer tree on the box: smart material folder, procedural fill with a
    // mask stack, an HSL filter on top.
    let sm = library::procedural_smart_materials().into_iter().find(|s| s.name == "Steel painted worn").context("smart material")?;
    let folder = doc.instantiate(&engine, 0, &sm.tree);
    let folder_id = folder.desc.id;
    doc.sets[0].selected = None;
    doc.insert_layer(0, folder);
    let paint_id = doc.sets[0].layers[0].desc.id;
    doc.sets[0].selected = Some(folder_id);
    // Insert after the folder at root level (collapse it so the fill lands beside it).
    find_mut(&mut doc.sets[0].layers, folder_id).unwrap().desc.expanded = false;
    let mut f = FillDesc::default();
    f.channels[CH_COLOR] = FillChannel {
        source: SourceKind::Procedural,
        procedural: ProcParams::of(ProcKind::Rust),
        value: [0.2, 0.06, 0.02],
        value_b: [0.5, 0.2, 0.05],
        ..Default::default()
    };
    f.projection = Projection { mode: ProjMode::Object3D, scale: 6.0, ..Default::default() };
    let fill_id = doc.add_layer(&engine, 0, LayerKind::Fill(Box::new(f)));
    doc.add_mask(&engine, 0, false);
    // A new mask has no effects (like Painter); the test paints into this one.
    doc.add_mask_effect(&engine, 0, MaskEffectKind::Paint);
    doc.add_mask_effect(&engine, 0, MaskEffectKind::Generator(GeneratorDesc::of(Generator::Dirt)));
    doc.add_mask_effect(&engine, 0, MaskEffectKind::levels());
    doc.add_mask_effect(&engine, 0, MaskEffectKind::Blur { radius: 2.0 });
    let mut hsl = FilterDesc::of(FilterKind::Hsl);
    hsl.p[0] = 0.05;
    let filter_id = doc.add_layer(&engine, 0, LayerKind::Filter(hsl));
    find_mut(&mut doc.sets[0].layers, filter_id).unwrap().desc.opacity[CH_COLOR] = 0.5;
    let top_id = doc.add_layer(&engine, 0, LayerKind::Paint);
    let mut depth = 0;
    visit(&doc.sets[0].layers, &mut |_| depth += 1);
    println!("set 0 layer tree: {depth} layers (root {})", doc.sets[0].layers.len());
    ensure!(depth >= 6, "layer tree too small");

    // Camera + first render to populate the depth buffer.
    let mut cam = OrbitCamera::default();
    cam.frame(doc.mesh.bbox_min, doc.mesh.bbox_max);
    engine.viewport.resize(&engine.device, 960, 600);
    composite_all(&engine, &mut doc);
    render(&engine, &doc, &cam, 0.0, 0.0, &|s| &s.gpu.final_base);
    screenshot(&engine, engine.viewport.output(), &out.join("01_stack.png"))?;

    let mut history = History::default();

    // Wrap-mode stroke through the brush engine on the box paint layer.
    let vp_size = Vec2::new(engine.viewport.width as f32, engine.viewport.height as f32);
    let vpm = cam.view_proj(vp_size.x / vp_size.y);
    let view = View { view_proj: vpm, inv_view_proj: vpm.inverse(), eye: cam.eye(), size: vp_size, fov_y: cam.fov_y, ppp: 1.0, ortho_wpp: None, uv_rot: 0.0 };
    let brush = BrushSettings { size: 40.0, flow: 0.9, spacing: 0.1, ..Default::default() };
    let mut stroke = Stroke::new(Space::Wrap, 0, 1234);
    let mut stamps: Vec<Stamp> = Vec::new();
    for k in 0..=40 {
        let p = Vec2::new(150.0 + k as f32 * 8.0, 300.0);
        stamps.extend(stroke.advance(p, &brush, &view, &doc, &|p| p, 0.0));
    }
    println!("wrap stroke: {} stamps", stamps.len());
    ensure!(!stamps.is_empty() || !strict, "wrap stroke produced no stamps (picking failed)");
    materialize_all(&engine, &mut doc);
    {
        let l = find(&doc.sets[0].layers, top_id).unwrap();
        history.push(Entry::stroke(&engine, "Paint stroke", 0, l.paint.as_ref().unwrap().all().into_iter().cloned().collect()));
        engine.paint.upload(&engine.queue, uniform(&cam, &engine, MODE_WRAP, [0.9, 0.02, 0.02]), &stamps);
        let mut enc = engine.device.create_command_encoder(&Default::default());
        let channels = [true, true, false, true, false, false, false];
        engine.paint.draw(&engine.device, &mut enc, &doc.gpu_mesh, 0, None, &paint_targets(l, PaintPass::PaintA), engine.viewport.depth(), PaintPass::PaintA, channels);
        engine.queue.submit(Some(enc.finish()));
        let painted = count(&engine, &l.paint.as_ref().unwrap().color, |p| p[3] > 0.5);
        println!("box paint layer: {painted} texels painted (wrap)");
        ensure!(!strict || painted > 500, "wrap stroke painted nothing");
        ensure!(!strict || painted < 250_000, "wrap stroke spread over far too many texels ({painted})");
    }

    // Camera-mode stroke on the ball paint layer.
    {
        let lid = doc.sets[1].layers[0].desc.id;
        let l = find(&doc.sets[1].layers, lid).unwrap();
        let stamps: Vec<Stamp> = (0..60)
            .map(|k| Stamp { pos: [520.0 + k as f32 * 6.0, 300.0, 0.0, 30.0], nrm: [0.0, 0.0, 1.0, 0.8], ext: [0.0; 4] })
            .collect();
        engine.paint.upload(&engine.queue, uniform(&cam, &engine, MODE_CAMERA, [0.05, 0.3, 0.9]), &stamps);
        let mut enc = engine.device.create_command_encoder(&Default::default());
        engine.paint.draw(&engine.device, &mut enc, &doc.gpu_mesh, 1, None, &paint_targets(l, PaintPass::PaintA), engine.viewport.depth(), PaintPass::PaintA, [true; 7]);
        engine.queue.submit(Some(enc.finish()));
        let painted = count(&engine, &l.paint.as_ref().unwrap().color, |p| p[3] > 0.5);
        println!("ball paint layer: {painted} texels painted (camera)");
        ensure!(!strict || painted > 500, "camera stroke painted nothing");
    }

    // Selections: the left half of the UV square selected (2D marquee),
    // a full fill paints only there; subtract / invert / intersect.
    {
        use crate::engine::select::{self, SelOp, SelectUniform, SPACE_ALL, SPACE_UV};
        let res = doc.sets[1].resolution();
        let sub = doc.sets[1].submesh;
        let r = res as f32;
        let dev = &engine.device;
        let uv_mask = |x0: f32, x1: f32| {
            let m = select::rasterize_polygon(&select::rect_points([x0 * r, 0.0], [x1 * r, r]), res, res);
            select::shape_texture(dev, &engine.queue, res, res, &m)
        };
        let uvu = SelectUniform { params: [SPACE_UV, 0.0, 0.0, 0.0], uvx: [1.0, 1.0, 0.0, 0.0], ..Default::default() };
        let all = SelectUniform { params: [SPACE_ALL, 0.0, 0.0, 0.0], ..Default::default() };
        let sel = select::new_selection(dev, &engine.queue, res);
        let mut enc = dev.create_command_encoder(&Default::default());
        engine.select.apply(dev, &mut enc, &doc.gpu_mesh, sub, None, &sel, Some(&uv_mask(0.0, 0.5)), engine.viewport.depth(), uvu, SelOp::Replace);
        engine.queue.submit(Some(enc.finish()));
        let raster = doc.sets[1].raster.tri.iter().map(|t| *t >= 0).collect::<Vec<bool>>();
        let raster = Raster(raster);
        // (selected texels left, selected texels right) over covered texels away from the edge.
        let halves = |bytes: &[u8], f: &dyn Fn(u8) -> bool| {
            let (mut l, mut rr, mut nl, mut nr) = (0usize, 0usize, 0usize, 0usize);
            for (i, b) in bytes.iter().enumerate() {
                if !raster.covered(i) {
                    continue;
                }
                let u = (i as u32 % res) as f32 / r;
                if u < 0.45 {
                    nl += 1;
                    l += f(*b) as usize;
                } else if u > 0.55 {
                    nr += 1;
                    rr += f(*b) as usize;
                }
            }
            (l, nl, rr, nr)
        };
        let (l, nl, rr, nr) = halves(&sel.read_bytes(dev, &engine.queue), &|b| b > 250);
        println!("selection (left half): {l} / {nl} left texels, {rr} / {nr} right texels");
        ensure!(nl > 100 && l == nl && rr == 0, "the UV marquee did not select exactly the left half");
        // Painting through the selection: only the left half takes paint.
        let mut layer = doc.new_layer(&engine, 1, LayerKind::Paint, None);
        layer.materialize(dev, res, None);
        engine.paint.selection = Some(sel.texture.create_view(&Default::default()));
        engine.paint.upload(&engine.queue, uniform(&cam, &engine, MODE_FULL, [0.9, 0.1, 0.1]), &[]);
        let mut enc = dev.create_command_encoder(&Default::default());
        engine.paint.draw(dev, &mut enc, &doc.gpu_mesh, sub, None, &paint_targets(&layer, PaintPass::PaintA), engine.viewport.depth(), PaintPass::PaintA, [true; 7]);
        engine.queue.submit(Some(enc.finish()));
        engine.paint.selection = None;
        let col = f16_bytes_to_f32(&layer.paint.as_ref().unwrap().color.read_bytes(dev, &engine.queue));
        let alpha: Vec<u8> = col.chunks_exact(4).map(|p| (p[3].clamp(0.0, 1.0) * 255.0) as u8).collect();
        let (l, nl, rr, _) = halves(&alpha, &|b| b > 250);
        println!("fill through the selection: {l} / {nl} left texels painted, {rr} right");
        ensure!(l == nl && rr == 0, "painting leaked out of the selection");
        // Subtract the left quarter, then invert: the right half plus the left quarter.
        let mut enc = dev.create_command_encoder(&Default::default());
        engine.select.apply(dev, &mut enc, &doc.gpu_mesh, sub, None, &sel, Some(&uv_mask(0.0, 0.25)), engine.viewport.depth(), uvu, SelOp::Subtract);
        let inv = select::new_selection(dev, &engine.queue, res);
        engine.select.apply(dev, &mut enc, &doc.gpu_mesh, sub, None, &inv, None, engine.viewport.depth(), all, SelOp::Replace);
        engine.select.apply(dev, &mut enc, &doc.gpu_mesh, sub, None, &inv, Some(&sel), engine.viewport.depth(), uvu, SelOp::Subtract);
        // Intersect with the left half: only the left quarter stays.
        engine.select.apply(dev, &mut enc, &doc.gpu_mesh, sub, None, &inv, Some(&uv_mask(0.0, 0.5)), engine.viewport.depth(), uvu, SelOp::Intersect);
        engine.queue.submit(Some(enc.finish()));
        let bytes = inv.read_bytes(dev, &engine.queue);
        let quarter = |lo: f32, hi: f32| {
            bytes.iter().enumerate().filter(|(i, _)| raster.covered(*i)).filter(move |(i, _)| {
                let u = (*i as u32 % res) as f32 / r;
                u > lo && u < hi
            })
        };
        let (on, n1) = (quarter(0.02, 0.23).filter(|(_, b)| **b > 250).count(), quarter(0.02, 0.23).count());
        let off = quarter(0.27, 1.0).filter(|(_, b)| **b > 5).count();
        println!("subtract / invert / intersect: {on} / {n1} texels in the left quarter, {off} elsewhere");
        ensure!(on == n1 && n1 > 50 && off == 0, "subtract / invert / intersect gave a wrong selection");
    }

    // Flood fill (magic wand / bucket): on the box, a contiguous flood of
    // one color crosses the UV seams and covers the whole surface; without
    // the seam links it stays on one island.
    {
        let s = &doc.sets[0];
        let raster = &s.raster;
        let covered = raster.tri.iter().filter(|t| **t >= 0).count();
        let seed = raster.tri.iter().position(|t| *t >= 0).unwrap();
        let t0 = Instant::now();
        let seams = crate::flood::Seams::build(&doc.mesh, s.submesh, raster);
        let built = t0.elapsed();
        let all = crate::flood::flood(raster, &seams, seed, true, &|_| true);
        let n_all = all.iter().filter(|b| **b > 0).count();
        let one = crate::flood::flood(raster, &crate::flood::Seams::none(), seed, true, &|_| true);
        let n_one = one.iter().filter(|b| **b > 0).count();
        println!(
            "flood: {} seam links ({:.0} ms), across seams {n_all} / {covered} texels, one island {n_one}",
            seams.link_count(),
            built.as_secs_f64() * 1000.0
        );
        ensure!(n_all == covered, "the flood did not cross every UV seam of the box ({n_all} / {covered})");
        ensure!(n_one * 3 < covered, "without seam links the flood should stay on one island ({n_one})");
        // Grow / shrink / feather / border of the left half, across seams.
        let res = raster.res;
        let left: Vec<u8> = (0..(res * res) as usize).map(|i| if raster.tri[i] >= 0 && (i as u32 % res) < res / 2 { 255 } else { 0 }).collect();
        let n = |v: &[u8]| v.iter().filter(|b| **b >= 128).count();
        let (n0, grow, shrink) = (
            n(&left),
            n(&crate::flood::modify(raster, &seams, &left, crate::flood::Modify::Grow, 8.0)),
            n(&crate::flood::modify(raster, &seams, &left, crate::flood::Modify::Shrink, 8.0)),
        );
        let feather = crate::flood::modify(raster, &seams, &left, crate::flood::Modify::Feather, 8.0);
        let soft = feather.iter().filter(|b| **b > 10 && **b < 245).count();
        let border = n(&crate::flood::modify(raster, &seams, &left, crate::flood::Modify::Border, 8.0));
        println!("modify: {n0} selected, grow {grow}, shrink {shrink}, feather {soft} soft texels, border {border}");
        ensure!(grow > n0 && shrink < n0 && soft > 0 && border > 0 && border < n0, "grow / shrink / feather / border gave wrong sizes");
        // Not contiguous: every similar texel, here the left half by position.
        let half = crate::flood::flood(raster, &seams, seed, false, &|i| (i as u32 % raster.res) < raster.res / 2);
        ensure!(half.iter().filter(|b| **b > 0).count() < covered, "a non contiguous flood took everything");
    }

    // Material painting: the Rust material evaluated over the ball set and
    // painted with camera stamps lower on the ball.
    {
        let res1 = doc.sets[1].resolution();
        let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        let t = |l| Tex::new(&engine.device, l, res1, res1, wgpu::TextureFormat::Rgba16Float, usage);
        engine.paint.material = Some([t("bm col"), t("bm mrh"), t("bm emi"), t("bm nrm")]);
        let mut enc = engine.device.create_command_encoder(&Default::default());
        doc.sets[1].evaluate_material(&engine, &mut enc, &doc.resources, &library::material("Rust"), engine.paint.material.as_ref().unwrap());
        engine.queue.submit(Some(enc.finish()));
        let lid = doc.sets[1].layers[0].desc.id;
        let l = find(&doc.sets[1].layers, lid).unwrap();
        let stamps: Vec<Stamp> = (0..60)
            .map(|k| Stamp { pos: [520.0 + k as f32 * 6.0, 380.0, 0.0, 25.0], nrm: [0.0, 0.0, 1.0, 1.0], ext: [0.0; 4] })
            .collect();
        let mut u = uniform(&cam, &engine, MODE_CAMERA, [1.0; 3]);
        u.material[0] = 1.0;
        engine.paint.upload(&engine.queue, u, &stamps);
        let mut enc = engine.device.create_command_encoder(&Default::default());
        engine.paint.draw(&engine.device, &mut enc, &doc.gpu_mesh, 1, None, &paint_targets(l, PaintPass::PaintA), engine.viewport.depth(), PaintPass::PaintA, [true, true, true, true, false, false, false]);
        engine.queue.submit(Some(enc.finish()));
        engine.paint.material = None;
        let px = f16_bytes_to_f32(&l.paint.as_ref().unwrap().color.read_bytes(&engine.device, &engine.queue));
        let rusty: Vec<f32> = px.chunks_exact(4).filter(|p| p[3] > 0.9).map(|p| p[0] / p[3]).filter(|r| *r > 0.1).collect();
        let (lo, hi) = rusty.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
        println!("material painting: {} texels, red channel {lo:.3}..{hi:.3}", rusty.len());
        ensure!(!strict || (rusty.len() > 500 && hi - lo > 0.05), "material painting produced no varied rust");
    }

    // UV-mode stroke into the fill layer's mask paint effect, plus a polygon
    // fill (emissive, pass B) of one UV island on the box paint layer.
    {
        materialize_all(&engine, &mut doc);
        let l = find(&doc.sets[0].layers, fill_id).unwrap();
        let eid = l.desc.mask.as_ref().unwrap().effects[0].id;
        let tex = l.mask_paint.get(&eid).unwrap().clone();
        let stamps: Vec<Stamp> = (0..30)
            .map(|k| Stamp { pos: [0.05 + k as f32 * 0.01, 0.3, 0.0, 0.04], nrm: [0.0, 0.0, 1.0, 1.0], ext: [0.0; 4] })
            .collect();
        engine.paint.upload(&engine.queue, uniform(&cam, &engine, MODE_UV, [1.0; 3]), &stamps);
        let mut enc = engine.device.create_command_encoder(&Default::default());
        engine.paint.draw(&engine.device, &mut enc, &doc.gpu_mesh, 0, None, &[&tex], engine.viewport.depth(), PaintPass::Mask, [true; 7]);
        engine.queue.submit(Some(enc.finish()));
        let painted = count(&engine, &tex, |p| p[1] > 0.5);
        println!("mask paint (UV): {painted} texels");
        ensure!(painted > 100, "UV mask stroke painted nothing");

        let labels = bake::components(&doc.mesh, 0, true);
        let island: Vec<u32> = labels.iter().enumerate().filter(|(_, l)| **l == labels[0]).map(|(i, _)| i as u32).collect();
        let idx = &doc.mesh.submeshes[0].indices;
        let subset: Vec<u32> = island.iter().flat_map(|&t| idx[t as usize * 3..t as usize * 3 + 3].iter().copied()).collect();
        println!("UV islands on the box: {}, island 0 has {} triangles", labels.iter().max().unwrap() + 1, island.len());
        let buf = engine.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&subset),
            usage: wgpu::BufferUsages::INDEX,
        });
        let lp = find_mut(&mut doc.sets[0].layers, paint_id).unwrap();
        lp.paint.as_mut().unwrap().ensure_b(&engine.device);
        let l = find(&doc.sets[0].layers, paint_id).unwrap();
        engine.paint.upload(&engine.queue, uniform(&cam, &engine, MODE_FULL, [1.0, 1.0, 1.0]), &[]);
        let mut enc = engine.device.create_command_encoder(&Default::default());
        let ch = [false, false, false, false, true, false, false];
        engine.paint.draw(&engine.device, &mut enc, &doc.gpu_mesh, 0, Some((&buf, subset.len() as u32)), &paint_targets(l, PaintPass::PaintB), engine.viewport.depth(), PaintPass::PaintB, ch);
        engine.queue.submit(Some(enc.finish()));
    }
    for s in doc.sets.iter_mut() {
        s.dirty = true;
    }
    doc.sets[0].selected = Some(fill_id);
    composite_all(&engine, &mut doc);
    render(&engine, &doc, &cam, 0.0, 0.0, &|s| &s.gpu.final_base);
    screenshot(&engine, engine.viewport.output(), &out.join("02_painted.png"))?;
    for (code, kind, name, which) in [
        (1.0, 0.0, "base", 0),
        (4.0, 0.0, "normal", 0),
        (7.0, 0.0, "emissive", 0),
        (10.0, 0.0, "curvature", 1),
        (10.0, 0.0, "ao", 2),
        (10.0, 1.0, "id", 3),
        (10.0, 0.0, "layer_mask", 4),
        (10.0, 0.0, "thickness", 5),
    ] {
        let disp = pick_fn(move |s: &TextureSet| -> &Tex {
            match which {
                1 => &s.gpu.baked_curvature,
                2 => &s.gpu.baked_ao,
                3 => &s.gpu.baked_id,
                4 => &s.gpu.mask_view,
                5 => &s.gpu.baked_thickness,
                _ => &s.gpu.final_base,
            }
        });
        render(&engine, &doc, &cam, code, kind, &disp);
        screenshot(&engine, engine.viewport.output(), &out.join(format!("03_{name}.png")))?;
    }
    {
        let mut enc = engine.device.create_command_encoder(&Default::default());
        engine.uvview.resize(&engine.device, 600, 600);
        engine.uvview.render(&engine.device, &mut enc, &doc.sets[0].gpu.final_base, 0, 560.0, [20.0, 20.0], 0.0, Some((&doc.gpu_mesh, 0)), [0.9, 0.9, 0.95, 0.4], None);
        engine.queue.submit(Some(enc.finish()));
        screenshot(&engine, &engine.uvview.out, &out.join("04_uv_view.png"))?;
    }

    // Path-traced render.
    {
        let t = Instant::now();
        engine.pathtracer.build_scene(&engine.device, &doc, 1);
        let vp = &engine.viewport;
        let m = cam.view_proj(vp.width as f32 / vp.height as f32);
        let slots: Vec<[&Tex; 4]> = doc.sets.iter().map(|s| [&s.gpu.final_base, &s.gpu.final_orm, &s.gpu.final_normal, &s.gpu.final_emissive]).collect();
        let u = crate::engine::pathtrace::PtUniform {
            inv_view_proj: m.inverse().to_cols_array_2d(),
            eye: cam.eye().extend(doc.mesh.diagonal()).into(),
            size: [0; 4],
            env: [0.0, 1.0, 0.0, 4.0],
            emi_a: [1.0; 4],
            emi_b: [1.0; 4],
            ..Default::default()
        };
        for _ in 0..96 {
            let mut enc = engine.device.create_command_encoder(&Default::default());
            engine.pathtracer.render(&engine.device, &mut enc, u, &slots, &engine.env.view, engine.viewport.hdr(), &Default::default());
            engine.queue.submit(Some(enc.finish()));
        }
        let mut frame = frame_for(&engine, &doc, &cam, 0.0, 0.0, &|s| &s.gpu.final_base);
        frame.path_traced = true;
        let mut enc = engine.device.create_command_encoder(&Default::default());
        engine.viewport.render(&engine.device, &engine.queue, &mut enc, &frame);
        engine.queue.submit(Some(enc.finish()));
        drop(frame);
        screenshot(&engine, engine.viewport.output(), &out.join("07_pathtraced.png"))?;
        let px = engine.viewport.output().read_bytes(&engine.device, &engine.queue);
        let lit = px.chunks_exact(4).filter(|p| p[0] > 30 || p[1] > 30).count();
        println!("path tracer: 96 spp in {:.2}s, lit pixels {lit}", t.elapsed().as_secs_f32());
        ensure!(lit > 1000, "path traced image is black");
    }

    let maps = export::read_maps(&engine, &doc, 0);
    let red = maps.base.chunks_exact(4).filter(|p| p[0] > 0.5 && p[1] < 0.2 && p[2] < 0.2).count();
    let metal = maps.orm.chunks_exact(4).filter(|p| p[2] > 0.5).count();
    let emissive = maps.emissive.chunks_exact(4).filter(|p| p[0] > 0.5).count();
    let bumped = maps.normal.chunks_exact(4).filter(|p| p[2] < 0.99).count();
    println!("set 0 final: red {red}, metal {metal}, emissive {emissive}, normal-bumped {bumped}");
    ensure!(maps.base.iter().chain(&maps.orm).chain(&maps.normal).all(|v| v.is_finite()), "NaN in final maps");
    ensure!(!strict || red > 300, "paint did not reach the final base color");
    ensure!(!strict || metal > 300, "smart material steel produced no metal");
    ensure!(!strict || emissive > 300, "polygon fill emissive missing");

    // History: undo the wrap stroke, check, redo.
    history.push(Entry::State { label: "test".into(), set: 0, state: doc.sets[0].state() });
    let before_delete = doc.sets[0].layers.len();
    doc.sets[0].selected = Some(filter_id);
    doc.delete_layer(0);
    ensure!(doc.sets[0].layers.len() == before_delete - 1, "delete failed");
    history.undo(&engine, &mut doc);
    ensure!(doc.sets[0].layers.len() == before_delete, "undo of delete failed");
    history.undo(&engine, &mut doc);
    composite_all(&engine, &mut doc);
    let red_undo = export::read_maps(&engine, &doc, 0).base.chunks_exact(4).filter(|p| p[0] > 0.5 && p[1] < 0.2 && p[2] < 0.2).count();
    history.redo(&engine, &mut doc);
    composite_all(&engine, &mut doc);
    let red_redo = export::read_maps(&engine, &doc, 0).base.chunks_exact(4).filter(|p| p[0] > 0.5 && p[1] < 0.2 && p[2] < 0.2).count();
    println!("history: red texels after undo {red_undo}, after redo {red_redo}");
    ensure!(!strict || red_undo < red / 2, "stroke undo did not restore the layer");
    ensure!(!strict || red_redo > red / 2, "stroke redo did not repaint");

    // Clone: snapshot the final maps and copy island 0's red band onto island 2.
    {
        let res0 = doc.sets[0].resolution();
        let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        let t = |l| Tex::new(&engine.device, l, res0, res0, wgpu::TextureFormat::Rgba16Float, usage);
        let m = [t("c col"), t("c mrh"), t("c emi"), t("c nrm")];
        let mut enc = engine.device.create_command_encoder(&Default::default());
        let g = &doc.sets[0].gpu;
        for (src, dst) in [(&g.final_base, &m[0]), (&g.final_orm, &m[1]), (&g.final_emissive, &m[2]), (&g.final_normal, &m[3])] {
            enc.copy_texture_to_texture(src.texture.as_image_copy(), dst.texture.as_image_copy(), wgpu::Extent3d { width: res0, height: res0, depth_or_array_layers: 1 });
        }
        engine.queue.submit(Some(enc.finish()));
        engine.paint.material = Some(m);
        materialize_all(&engine, &mut doc);
        let l = find(&doc.sets[0].layers, top_id).unwrap();
        let red_before = count(&engine, &l.paint.as_ref().unwrap().color, |p| p[3] > 0.5 && p[0] / p[3] > 0.5 && p[1] / p[3] < 0.2);
        let (src, dst) = ([0.16f32, 0.25f32], [0.83f32, 0.25f32]);
        let stamps: Vec<Stamp> = (0..20)
            .map(|k| Stamp { pos: [dst[0] - 0.12 + k as f32 * 0.012, dst[1], 0.0, 0.05], nrm: [0.0, 0.0, 1.0, 1.0], ext: [0.0; 4] })
            .collect();
        let mut u = uniform(&cam, &engine, MODE_UV, [1.0; 3]);
        u.material = [1.0, 1.0, (src[0] - dst[0]) * res0 as f32, (src[1] - dst[1]) * res0 as f32];
        engine.paint.upload(&engine.queue, u, &stamps);
        let mut enc = engine.device.create_command_encoder(&Default::default());
        engine.paint.draw(&engine.device, &mut enc, &doc.gpu_mesh, 0, None, &paint_targets(l, PaintPass::PaintA), engine.viewport.depth(), PaintPass::PaintA, [true, true, true, false, false, false, false]);
        engine.queue.submit(Some(enc.finish()));
        engine.paint.material = None;
        let red_after = count(&engine, &l.paint.as_ref().unwrap().color, |p| p[3] > 0.5 && p[0] / p[3] > 0.5 && p[1] / p[3] < 0.2);
        println!("clone: red texels on the top layer {red_before} -> {red_after}");
        ensure!(!strict || red_after > red_before + 1000, "clone copied nothing");
        doc.sets[0].dirty = true;
        composite_all(&engine, &mut doc);
    }

    // Height-reactive edges: a fill above the painted height band, masked by
    // an edge generator that reads the stack's height.
    {
        doc.sets[0].selected = Some(top_id);
        let mut g = GeneratorDesc::of(Generator::Edges);
        let count_edges = |engine: &Engine, doc: &mut Document, g: &GeneratorDesc| -> Result<usize> {
            let id = doc.add_layer(engine, 0, LayerKind::Fill(Box::new(FillDesc::material([1.0, 1.0, 0.0], 0.3, 1.0))));
            doc.add_mask_effect(engine, 0, MaskEffectKind::Generator(g.clone()));
            doc.sets[0].selected = Some(id);
            composite_all(engine, doc);
            let v = crate::engine::texture::f32_bytes_to_f32(&doc.sets[0].gpu.mask_view.read_bytes(&engine.device, &engine.queue));
            let n = v.iter().filter(|x| **x > 0.5).count();
            doc.delete_layer(0);
            Ok(n)
        };
        let plain = count_edges(&engine, &mut doc, &g)?;
        g.height_reactive = 4.0;
        let reactive = count_edges(&engine, &mut doc, &g)?;
        println!("height-reactive edges: {plain} -> {reactive} texels");
        ensure!(!strict || reactive > plain + 1000, "edge generator ignores painted height");
        composite_all(&engine, &mut doc);
    }

    // Geometry mask: hide the whole ball, picks pass through it and painting skips it.
    {
        let sub = doc.sets[1].submesh;
        let all: Vec<u32> = (0..(doc.mesh.submeshes[sub].indices.len() / 3) as u32).collect();
        let c = (doc.mesh.bbox_min + doc.mesh.bbox_max) * 0.5;
        let ball = glam::Vec3::new(1.5, 0.0, 0.0);
        let o = ball + glam::Vec3::new(0.0, 0.0, 10.0);
        let before = doc.pick(o, glam::Vec3::NEG_Z).map(|h| h.set);
        doc.set_hidden(&engine, sub, &all, true);
        let after = doc.pick(o, glam::Vec3::NEG_Z).map(|h| h.set);
        println!("geometry mask: pick through the ball {before:?} -> {after:?} (center {c:?})");
        ensure!(!strict || (before == Some(1) && after != Some(1)), "hidden geometry is still picked");
        materialize_all(&engine, &mut doc);
        let lid = doc.sets[1].layers[0].desc.id;
        let l = find(&doc.sets[1].layers, lid).unwrap();
        let t = l.paint.as_ref().unwrap().vals.clone();
        let n_before = count(&engine, &t, |p| p[0] > 0.0);
        engine.paint.upload(&engine.queue, uniform(&cam, &engine, MODE_FULL, [1.0; 3]), &[]);
        let mut enc = engine.device.create_command_encoder(&Default::default());
        engine.paint.draw(&engine.device, &mut enc, &doc.gpu_mesh, 1, None, &paint_targets(l, PaintPass::PaintA), engine.viewport.depth(), PaintPass::PaintA, [false, true, false, false, false, false, false]);
        engine.queue.submit(Some(enc.finish()));
        let n_after = count(&engine, &t, |p| p[0] > 0.0);
        ensure!(n_after == n_before, "hidden geometry was painted ({n_before} -> {n_after})");
        doc.reset_hidden();
        doc.refresh_visibility(&engine);
    }

    // Export: every preset in PNG, then each format with padding / size options.
    let maps = export::read_maps(&engine, &doc, 0);
    for preset in export::builtin_presets() {
        let dir = out.join(format!("export_{}", preset.name.replace([' ', '(', ')', '/'], "_")));
        let files = export::export_set(&maps, &dir, "selftest", &doc.sets[0].name, &preset, &ExportOptions::default())?;
        ensure!(files.iter().all(|f| f.exists()), "missing export file");
    }
    let preset = &export::builtin_presets()[0];
    for (i, fmt) in FileFormat::ALL.iter().enumerate() {
        let mut maps = export::read_maps(&engine, &doc, 0);
        let padding = [Padding::Infinite, Padding::Dilate(4), Padding::None][i % 3];
        maps.keep = export::padding_mask(&doc.sets[0], padding);
        let opts = ExportOptions { format: *fmt, size_shift: if i == 2 { -1 } else { 0 }, padding, ..Default::default() };
        let files = export::export_set(&maps, &out.join(format!("export_fmt_{i}")), "selftest", "Box", preset, &opts)?;
        ensure!(files.iter().all(|f| f.exists()), "missing {} file", fmt.name());
    }
    println!("exported {} presets and {} formats", export::builtin_presets().len(), FileFormat::ALL.len());

    // Project save / reload.
    let img = image::RgbaImage::from_fn(64, 64, |x, y| image::Rgba([(x * 4) as u8, (y * 4) as u8, 128, 255]));
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
    let rid = doc.add_resource(&engine, "gradient.png".into(), png, ResKind::Texture)?;
    if let Some(LayerKind::Fill(f)) = find_mut(&mut doc.sets[0].layers, fill_id).map(|l| &mut l.desc.kind) {
        f.channels[CH_ROUGH] = FillChannel { source: SourceKind::Texture, texture: Some(rid), value: [0.2; 3], value_b: [0.8; 3], ..Default::default() };
    }
    doc.sets[0].dirty = true;
    composite_all(&engine, &mut doc);
    let before = export::read_maps(&engine, &doc, 0);
    let proj = out.join("selftest.sprp");
    project::save(&engine, &doc, &cam, &proj)?;
    let size = std::fs::metadata(&proj)?.len();
    let (mut doc2, cam2) = project::load(&engine, &proj)?;
    composite_all(&engine, &mut doc2);
    let after = export::read_maps(&engine, &doc2, 0);
    let diff = before.base.iter().zip(&after.base).chain(before.orm.iter().zip(&after.orm)).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    println!("project saved ({:.1} MB), reload max diff {diff:.5}, resources {}", size as f32 / 1e6, doc2.resources.len());
    ensure!(diff < 2e-3, "reloaded project differs");
    render(&engine, &doc2, &cam2, 0.0, 0.0, &|s| &s.gpu.final_base);
    screenshot(&engine, engine.viewport.output(), &out.join("05_reloaded.png"))?;

    // Resolution change.
    doc2.change_resolution(&engine, 0, 512);
    composite_all(&engine, &mut doc2);
    let small = export::read_maps(&engine, &doc2, 0);
    ensure!(small.res == 512 && small.base.iter().all(|v| v.is_finite()), "resolution change failed");
    println!("resolution change to 512 ok");

    // Optional channels: a fill writing specular level and transmission
    // reaches the finals (export reads them from the spare .w slots).
    {
        let mut f = FillDesc::material([0.5, 0.5, 0.5], 0.5, 0.0);
        f.ext[0] = FillChannel { enabled: true, value: [0.9; 3], ..Default::default() };
        f.ext[1] = FillChannel { enabled: true, value: [0.6; 3], ..Default::default() };
        let set = 0;
        let l = doc2.new_layer(&engine, set, LayerKind::Fill(Box::new(f)), Some("ext".into()));
        doc2.insert_layer(set, l);
        composite_all(&engine, &mut doc2);
        let m = export::read_maps(&engine, &doc2, 0);
        let cov: Vec<usize> = (0..m.res as usize * m.res as usize).filter(|i| m.base[i * 4 + 3] > 0.5 && m.orm[i * 4] > 0.0).collect();
        let i = *cov.get(cov.len() / 2).unwrap_or(&0);
        let (spec, trans) = (m.emissive[i * 4 + 3], m.normal[i * 4 + 3]);
        println!("optional channels: specular level {spec:.3}, transmission {trans:.3}");
        ensure!((spec - 0.9).abs() < 0.02 && (trans - 0.6).abs() < 0.02, "optional channels did not reach the finals");
    }

    // A color image with transparency (a decal): where it is transparent
    // the layer leaves everything under it, in every channel.
    {
        let before = export::read_maps(&engine, &doc2, 0);
        let img = image::RgbaImage::from_fn(64, 64, |x, _| image::Rgba([255, 0, 0, if x < 32 { 0 } else { 255 }]));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
        let rid = doc2.add_resource(&engine, "decal.png".into(), png, ResKind::Texture)?;
        let mut f = FillDesc::material([1.0; 3], 0.9, 1.0);
        f.channels[CH_COLOR] = FillChannel { source: SourceKind::Texture, texture: Some(rid), value: [1.0; 3], ..Default::default() };
        let l = doc2.new_layer(&engine, 0, LayerKind::Fill(Box::new(f)), Some("decal".into()));
        let decal = l.desc.id;
        doc2.insert_layer(0, l);
        composite_all(&engine, &mut doc2);
        let m = export::read_maps(&engine, &doc2, 0);
        let res = m.res as usize;
        let (mut kept, mut left, mut red, mut right) = (0, 0, 0, 0);
        for y in 0..res {
            for x in 0..res {
                let i = (y * res + x) * 4;
                if before.base[i + 3] < 0.5 || x % (res / 8) == 0 {
                    continue;
                }
                if x < res / 2 - 2 {
                    left += 1;
                    let same = (0..3).all(|k| (m.base[i + k] - before.base[i + k]).abs() < 0.02 && (m.orm[i + k] - before.orm[i + k]).abs() < 0.02);
                    kept += same as usize;
                } else if x > res / 2 + 2 {
                    right += 1;
                    red += (m.base[i] > 0.8 && m.base[i + 1] < 0.1) as usize;
                }
            }
        }
        println!("image alpha: transparent half kept {kept} of {left} texels, opaque half red {red} of {right}");
        ensure!(left > 0 && right > 0 && kept * 100 >= left * 99 && red * 100 >= right * 99, "a color image's transparency does not leave the layers under it");
        doc2.delete_layers(0, &[decal]);
        composite_all(&engine, &mut doc2);
    }

    // Replace on the normal or the height hides all the relief below (a
    // height relief and normal details): the finals' normals are those of
    // the stack without the layers below.
    {
        let mut relief = FillDesc::material([0.5; 3], 0.5, 0.0);
        relief.channels[CH_HEIGHT] = FillChannel {
            enabled: true,
            source: SourceKind::Procedural,
            procedural: ProcParams::of(ProcKind::Cells),
            value: [0.0; 3],
            value_b: [1.0; 3],
            ..Default::default()
        };
        relief.channels[CH_NORMAL].enabled = true;
        relief.channels[CH_NORMAL].value = [0.4, 0.2, 0.9];
        let l = doc2.new_layer(&engine, 0, LayerKind::Fill(Box::new(relief)), Some("relief".into()));
        doc2.insert_layer(0, l);
        for (name, ch) in [("normal", CH_NORMAL), ("height", CH_HEIGHT)] {
            let mut top = FillDesc::default();
            for c in top.channels.iter_mut() {
                c.enabled = false;
            }
            top.channels[ch].enabled = true;
            let mut l = doc2.new_layer(&engine, 0, LayerKind::Fill(Box::new(top)), Some("replace".into()));
            l.desc.blend[ch] = BlendMode::Replace;
            let top_id = l.desc.id;
            doc2.insert_layer(0, l);
            composite_all(&engine, &mut doc2);
            let with = export::read_maps(&engine, &doc2, 0);
            let mut hidden = Vec::new();
            visit_mut(&mut doc2.sets[0].layers, &mut |l| {
                if l.desc.id != top_id && l.desc.visible {
                    l.desc.visible = false;
                    hidden.push(l.desc.id);
                }
            });
            doc2.sets[0].dirty = true;
            composite_all(&engine, &mut doc2);
            let alone = export::read_maps(&engine, &doc2, 0);
            visit_mut(&mut doc2.sets[0].layers, &mut |l| {
                if hidden.contains(&l.desc.id) {
                    l.desc.visible = true;
                }
            });
            let ndiff = |a: &[f32], b: &[f32]| a.chunks_exact(4).zip(b.chunks_exact(4)).map(|(a, b)| (0..3).map(|k| (a[k] - b[k]).abs()).fold(0.0f32, f32::max)).fold(0.0f32, f32::max);
            let diff = ndiff(&with.normal, &alone.normal);
            // Control: without the Replace layer the relief shows.
            visit_mut(&mut doc2.sets[0].layers, &mut |l| {
                if l.desc.id == top_id {
                    l.desc.visible = false;
                }
            });
            doc2.sets[0].dirty = true;
            composite_all(&engine, &mut doc2);
            let below = export::read_maps(&engine, &doc2, 0);
            let shows = ndiff(&below.normal, &alone.normal);
            println!("replace on the {name}: normals differ from the stack without the layers below by {diff:.4} (relief without it: {shows:.3})");
            ensure!(shows > 0.05, "the test relief does not show ({shows})");
            ensure!(diff < 0.02, "Replace on the {name} does not hide the relief below ({diff})");
            doc2.delete_layers(0, &[top_id]);
        }
        let mut rid = None;
        visit(&doc2.sets[0].layers, &mut |l| {
            if l.desc.name == "relief" {
                rid = Some(l.desc.id);
            }
        });
        if let Some(id) = rid {
            doc2.delete_layers(0, &[id]);
        }
        doc2.sets[0].dirty = true;
        composite_all(&engine, &mut doc2);
    }

    // The Flakes generator in a normal slot tilts the normals flake by flake.
    {
        let flat = export::read_maps(&engine, &doc2, 0);
        let mut f = FillDesc::material([0.8; 3], 0.3, 1.0);
        f.channels[CH_NORMAL] = FillChannel { enabled: true, source: SourceKind::Procedural, procedural: ProcParams::of(ProcKind::Flakes), ..Default::default() };
        let l = doc2.new_layer(&engine, 0, LayerKind::Fill(Box::new(f)), Some("flakes".into()));
        let fid = l.desc.id;
        doc2.insert_layer(0, l);
        composite_all(&engine, &mut doc2);
        let m = export::read_maps(&engine, &doc2, 0);
        let n = (m.normal.len() / 4).max(1);
        let tilted = m.normal.chunks_exact(4).zip(flat.normal.chunks_exact(4)).filter(|(a, b)| (0..3).any(|k| (a[k] - b[k]).abs() > 0.03)).count();
        println!("flakes normal: {:.0}% of the texels tilted", tilted as f32 * 100.0 / n as f32);
        ensure!(tilted * 4 > n, "the Flakes normal does not tilt the normals ({tilted} of {n})");
        doc2.delete_layers(0, &[fid]);
        doc2.sets[0].dirty = true;
        composite_all(&engine, &mut doc2);
    }

    // Optional channels past the first two: their own finals, allocated on
    // demand; the painted ambient occlusion darkens the applied one.
    {
        let before = export::read_maps(&engine, &doc2, 0);
        ensure!(!doc2.sets[0].gpu.ext_on(), "optional groups allocated while unused");
        for i in [2, 3, 12] {
            doc2.sets[0].settings.ext_channels[i] = true;
        }
        let mut f = FillDesc::material([0.5; 3], 0.5, 0.0);
        for c in f.channels.iter_mut() {
            c.enabled = false;
        }
        f.ext[2] = FillChannel { enabled: true, value: [0.3; 3], ..Default::default() };
        f.ext[3] = FillChannel { enabled: true, value: [0.8; 3], ..Default::default() };
        let l = doc2.new_layer(&engine, 0, LayerKind::Fill(Box::new(f)), Some("optional".into()));
        let oid = l.desc.id;
        doc2.insert_layer(0, l);
        composite_all(&engine, &mut doc2);
        let m = export::read_maps(&engine, &doc2, 0);
        ensure!(doc2.sets[0].gpu.ext_on() && m.ext[0].is_some() && m.ext[1].is_none() && m.ext[2].is_some(), "optional group finals not sized by use");
        let keep = doc2.sets[0].raster.tri.iter().position(|t| *t >= 0).unwrap_or(0) * 4;
        let x0 = m.ext[0].as_ref().unwrap();
        let x2 = m.ext[2].as_ref().unwrap();
        let ao_ratio = m.orm[keep] / before.orm[keep].max(1e-4);
        println!(
            "optional channels: ao {:.2} gloss {:.2} ior {:.2} (default {:.2}), applied ao x{:.2}",
            x0[keep], x0[keep + 1], x2[keep + 2], EXT_DEFAULTS[12], ao_ratio
        );
        ensure!((x0[keep] - 0.3).abs() < 0.01 && (x0[keep + 1] - 0.8).abs() < 0.01, "optional channel values wrong");
        ensure!((x2[keep + 2] - EXT_DEFAULTS[12]).abs() < 0.01, "optional channel default wrong");
        ensure!((ao_ratio - 0.3).abs() < 0.02, "painted AO not applied ({ao_ratio})");
        doc2.delete_layers(0, &[oid]);
        for i in [2, 3, 12] {
            doc2.sets[0].settings.ext_channels[i] = false;
        }
        doc2.sets[0].dirty = true;
        composite_all(&engine, &mut doc2);
        ensure!(!doc2.sets[0].gpu.ext_on(), "optional groups kept after removal");
    }

    // Shelf thumbnail.
    let mut pv = crate::preview::Previewer::new(&engine);
    let thumb = pv.render(&engine, &library::procedural_smart_materials()[1].tree, &doc2.resources);
    screenshot(&engine, &thumb, &out.join("06_thumbnail.png"))?;
    // The material itself must be lit, not only the background.
    {
        let px = thumb.read_bytes(&engine.device, &engine.queue);
        let w = thumb.width as usize;
        let c = &px[(w * (thumb.height as usize / 2) + w / 2) * 4..][..4];
        println!("thumbnail center pixel {c:?}");
        ensure!(c[0] as u32 + c[1] as u32 + c[2] as u32 > 30, "thumbnail material renders black");
    }
    let tb = thumb.read_bytes(&engine.device, &engine.queue);
    ensure!(tb.chunks_exact(4).any(|p| p[0] > 40), "thumbnail is empty");
    let _ = Arc::new(());
    let _ = &mut engine;

    quad_check(out)?;
    opacity_check(&mut engine, out)?;
    projection_check(&mut engine, out)?;
    normal_filter_check(&mut engine, out)?;
    curves_check(&mut engine, out)?;
    visibility_check(&mut engine, out)?;
    tiling_check(&mut engine, out)?;
    starter_assets_check(&engine, out)?;
    auto_uv_check(out)?;

    println!("SELFTEST OK in {:.1}s, outputs in {}", t0.elapsed().as_secs_f32(), out.display());
    Ok(())
}

/// Auto UV: the test scene without its UVs gets UVs in 0-1, every
/// triangle with a UV area, and OBJ export round-trips them.
fn auto_uv_check(out: &Path) -> Result<()> {
    let src = test_obj();
    // Drop the texture coordinates: "f a/t/n" becomes "f a//n".
    let mut no_uv = String::new();
    for line in src.lines() {
        if line.starts_with("vt ") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("f ") {
            let corners: Vec<String> = rest
                .split_whitespace()
                .map(|c| {
                    let p: Vec<&str> = c.split('/').collect();
                    match p.len() {
                        3 => format!("{}//{}", p[0], p[2]),
                        _ => p[0].to_string(),
                    }
                })
                .collect();
            no_uv.push_str(&format!("f {}\n", corners.join(" ")));
        } else {
            no_uv.push_str(line);
            no_uv.push('\n');
        }
    }
    let p = out.join("auto_uv_in.obj");
    std::fs::write(&p, no_uv)?;
    let t = Instant::now();
    let m = mesh::load(&p)?;
    let ms = t.elapsed().as_secs_f32() * 1000.0;
    ensure!(m.notes.iter().any(|n| n.contains("generated automatically")), "no auto UV note: {:?}", m.notes);
    ensure!(m.vertices.iter().all(|v| (0.0..=1.0).contains(&v.uv[0]) && (0.0..=1.0).contains(&v.uv[1])), "auto UVs outside 0-1");
    let mut flat = 0;
    for s in &m.submeshes {
        for t in s.indices.chunks_exact(3) {
            let (a, b, c) = (m.vertices[t[0] as usize].uv, m.vertices[t[1] as usize].uv, m.vertices[t[2] as usize].uv);
            let area = ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs();
            // Triangles without a 3D area (the sphere's poles) get none in UV either.
            let p = |k: usize| glam::Vec3::from(m.vertices[t[k] as usize].pos);
            let area3 = (p(1) - p(0)).cross(p(2) - p(0)).length();
            if area < 1e-9 && area3 > 1e-6 {
                flat += 1;
            }
        }
    }
    ensure!(flat == 0, "{flat} auto UV triangles have no area");
    let (mirrored, total) = mirrored_uvs(&m);
    ensure!(mirrored * 20 <= total, "{mirrored} of {total} auto UV triangles are mirrored (a texture reads backwards)");
    let back = out.join("auto_uv_out.obj");
    mesh::write_obj(&m, &back)?;
    let m2 = mesh::load_with(&back, mesh::AutoUv::IfMissing)?;
    ensure!(m2.triangle_count() == m.triangle_count() && m2.notes.iter().all(|n| !n.contains("generated")), "the exported OBJ lost its UVs");
    let overlap = uv_overlap(&m);
    ensure!(overlap == 0, "auto UV islands overlap ({overlap} texels covered twice)");
    // One texture set alone (a copy of the first, its UVs halved): the
    // others keep their UVs exactly.
    {
        let mut two = m.clone();
        let base = two.vertices.len() as u32;
        let copies: Vec<mesh::Vertex> = m.vertices.iter().map(|v| mesh::Vertex { uv: [v.uv[0] * 0.5, v.uv[1] * 0.5], ..*v }).collect();
        two.vertices.extend(copies);
        if !two.colors.is_empty() {
            let c = two.colors.clone();
            two.colors.extend(c);
        }
        let mut b = m.submeshes[0].clone();
        b.name = "second".into();
        b.indices.iter_mut().for_each(|i| *i += base);
        two.submeshes.push(b);
        let last = two.submeshes.len() - 1;
        let mut pick = vec![false; last + 1];
        pick[last] = true;
        let (only, _) = mesh::unwrap_sets(&two, &mesh::UvSettings::default(), &pick)?;
        let corners = |mm: &mesh::MeshData, s: usize| mm.submeshes[s].indices.iter().map(|i| mm.vertices[*i as usize].uv).collect::<Vec<_>>();
        ensure!((0..last).all(|s| corners(&only, s) == corners(&two, s)), "unwrapping one texture set changed another one's UVs");
        ensure!(corners(&only, last) != corners(&two, last), "the chosen texture set was not unwrapped");
        let used: std::collections::HashSet<u32> = only.submeshes.iter().flat_map(|s| s.indices.iter().copied()).collect();
        ensure!(used.len() == only.vertices.len(), "unused vertices left after a partial unwrap");
    }
    // A wavy grid of quads: its quads come out straightened, every row
    // and column on an axis.
    let (w, h) = (12, 8);
    let mut grid = String::new();
    for j in 0..=h {
        for i in 0..=w {
            let (x, z) = (i as f32 * 0.2, j as f32 * 0.2);
            grid.push_str(&format!("v {x} {} {z}\n", 0.3 * (i as f32 * 0.4).sin() * (j as f32 * 0.3).cos()));
        }
    }
    for j in 0..h {
        for i in 0..w {
            let a = j * (w + 1) + i + 1;
            grid.push_str(&format!("f {} {} {} {}\n", a, a + w + 1, a + w + 2, a + 1));
        }
    }
    let p = out.join("auto_uv_grid.obj");
    std::fs::write(&p, grid)?;
    let g = mesh::load(&p)?;
    let mut bent = 0;
    for s in &g.submeshes {
        for t in s.indices.chunks_exact(3) {
            // Two of the three edges (a quad's sides, not its diagonal) on an axis.
            let mut off: Vec<f32> = (0..3)
                .map(|k| {
                    let (a, b) = (g.vertices[t[k] as usize].uv, g.vertices[t[(k + 1) % 3] as usize].uv);
                    let (du, dv) = ((b[0] - a[0]).abs(), (b[1] - a[1]).abs());
                    du.min(dv) / du.hypot(dv).max(1e-12)
                })
                .collect();
            off.sort_by(f32::total_cmp);
            bent += (off[1] > 0.02) as usize;
        }
    }
    ensure!(bent == 0, "{bent} of {} triangles of a quad grid are not straightened", g.triangle_count());
    ensure!(uv_overlap(&g) == 0 && mirrored_uvs(&g).0 == 0, "the straightened grid overlaps or is mirrored");
    println!("auto UV: {} triangles unwrapped in {ms:.0} ms, no overlap, OBJ export reloads with its UVs; one texture set unwraps alone; a quad grid comes out straight", m.triangle_count());
    Ok(())
}

/// Texels of a 1024 x 1024 UV raster that two triangles of one set cover
/// (their insides, so neighbors sharing an edge don't count): 0 when no
/// islands overlap.
fn uv_overlap(m: &crate::mesh::MeshData) -> usize {
    const N: usize = 1024;
    let mut twice = 0;
    let mut id = 0u32;
    for s in &m.submeshes {
        let mut owner = vec![u32::MAX; N * N];
        for t in s.indices.chunks_exact(3) {
            id += 1;
            let p: Vec<[f32; 2]> = (0..3).map(|k| m.vertices[t[k] as usize].uv.map(|c| c * N as f32)).collect();
            let d = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
            if d.abs() < 1e-9 {
                continue;
            }
            let lo = |k: usize| (p[0][k].min(p[1][k]).min(p[2][k]).floor().max(0.0) as usize).min(N - 1);
            let hi = |k: usize| (p[0][k].max(p[1][k]).max(p[2][k]).ceil().max(0.0) as usize).min(N - 1);
            for y in lo(1)..=hi(1) {
                for x in lo(0)..=hi(0) {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w1 = ((px - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (py - p[0][1])) / d;
                    let w2 = ((p[1][0] - p[0][0]) * (py - p[0][1]) - (px - p[0][0]) * (p[1][1] - p[0][1])) / d;
                    if w1 > 1e-4 && w2 > 1e-4 && 1.0 - w1 - w2 > 1e-4 {
                        let o = &mut owner[y * N + x];
                        if *o != u32::MAX && *o != id {
                            twice += 1;
                        }
                        *o = id;
                    }
                }
            }
        }
    }
    twice
}

/// Starter brushes and smart materials: every tip file and texture material
/// they name exists; contact sheets of their thumbnails (`brushes.png`,
/// `smart_materials.png`). Skipped without the starter assets.
fn starter_assets_check(engine: &Engine, out: &Path) -> Result<()> {
    if library::starter_dir().is_none() {
        println!("starter assets: not found, skipped");
        return Ok(());
    }
    let sheet = |tiles: &[image::RgbaImage], size: u32, path: &Path| -> Result<()> {
        let cols = 8u32;
        let rows = (tiles.len() as u32).div_ceil(cols);
        let mut img = image::RgbaImage::from_pixel(cols * size, rows.max(1) * size, image::Rgba([30, 30, 30, 255]));
        for (i, t) in tiles.iter().enumerate() {
            let t = image::imageops::resize(t, size - 4, size - 4, image::imageops::FilterType::Triangle);
            image::imageops::overlay(&mut img, &t, ((i as u32 % cols) * size + 2) as i64, ((i as u32 / cols) * size + 2) as i64);
        }
        img.save(path)?;
        Ok(())
    };
    let brushes = crate::brush::builtin_brushes();
    for b in &brushes {
        if let Some(f) = &b.alpha_file {
            ensure!(library::starter_alpha(f).is_some(), "brush {}: tip {f} missing", b.name);
        }
    }
    let tiles: Vec<image::RgbaImage> = brushes.iter().map(|b| b.thumbnail(128)).collect();
    ensure!(tiles.iter().all(|t| t.pixels().any(|p| p[0] > 60)), "a brush thumbnail is empty");
    sheet(&tiles, 128, &out.join("brushes.png"))?;
    let sms = library::smart_materials();
    ensure!(sms.len() >= 18, "only {} of the 18 starter smart materials found their materials", sms.len());
    let mut pv = crate::preview::Previewer::new_box(engine);
    let mut tiles = Vec::new();
    for sm in &sms {
        let mut resources = std::collections::HashMap::new();
        for (id, p) in &sm.textures {
            let img = image::open(p).with_context(|| format!("{}: {}", sm.name, p.display()))?.to_rgba8();
            let img = image::imageops::thumbnail(&img, 256, 256);
            let gpu = crate::engine::texture::rgba8_with_mips(&engine.device, &engine.queue, "map", &img);
            resources.insert(*id, Resource { id: *id, name: String::new(), kind: ResKind::Texture, imported: false, encoded: Default::default(), gpu: Arc::new(gpu), image: Arc::new(img) });
        }
        let t = pv.render(engine, &sm.tree, &resources);
        let px = t.read_bytes(&engine.device, &engine.queue);
        tiles.push(image::RgbaImage::from_raw(t.width, t.height, px).context("thumbnail size")?);
    }
    sheet(&tiles, 192, &out.join("smart_materials.png"))?;
    println!("starter assets: {} brushes, {} smart materials", brushes.len(), sms.len());
    Ok(())
}

/// `sp_retro --bench`: compositing time of a heavy stack at 2048 and 4096.
pub fn bench() -> Result<()> {
    let engine = Engine::headless()?;
    let dir = std::env::temp_dir().join("sp_retro_bench");
    std::fs::create_dir_all(&dir)?;
    let p = dir.join("bench.obj");
    std::fs::write(&p, test_obj())?;
    for res in [2048u32, 4096] {
        let m = mesh::load(&p)?;
        let mut doc = Document::new(&engine, m, "bench".into(), res);
        for sm in library::procedural_smart_materials().iter().take(6) {
            let l = doc.instantiate(&engine, 0, &sm.tree);
            doc.insert_layer(0, l);
        }
        doc.add_layer(&engine, 0, LayerKind::Filter(FilterDesc::of(FilterKind::Blur)));
        doc.add_layer(&engine, 0, LayerKind::Paint);
        let mut n = 0;
        visit(&doc.sets[0].layers, &mut |_| n += 1);
        let mut times = Vec::new();
        for _ in 0..6 {
            let t = Instant::now();
            let mut enc = engine.device.create_command_encoder(&Default::default());
            let r = &doc.resources;
            doc.sets[0].composite(&engine, &mut enc, r, None);
            engine.queue.submit(Some(enc.finish()));
            engine.device.poll(wgpu::Maintain::Wait);
            times.push(t.elapsed().as_secs_f32() * 1000.0);
        }
        println!("res {res}: {n} layers, full composite {:?} ms", times.iter().map(|t| format!("{t:.1}")).collect::<Vec<_>>());
        // Painting the top layer: the prefix below it is cached.
        let top = doc.sets[0].layers.last().unwrap().desc.id;
        doc.sets[0].focus = Some(top);
        let mut cached = Vec::new();
        for i in 0..6 {
            if let Some(l) = find_mut(&mut doc.sets[0].layers, top) {
                l.rev = next_rev();
            }
            let t = Instant::now();
            let mut enc = engine.device.create_command_encoder(&Default::default());
            let r = &doc.resources;
            doc.sets[0].composite(&engine, &mut enc, r, None);
            engine.queue.submit(Some(enc.finish()));
            engine.device.poll(wgpu::Maintain::Wait);
            if i > 0 {
                cached.push(t.elapsed().as_secs_f32() * 1000.0);
            }
        }
        println!("res {res}: top layer, full recomposite (cached prefix) {:?} ms", cached.iter().map(|t| format!("{t:.1}")).collect::<Vec<_>>());
        // A brush stroke frame: only the 256 x 256 texels under the brush.
        let mut stroke = Vec::new();
        for i in 0..6 {
            let o = 512 + i * 64;
            doc.sets[0].dirty_region = Some([o, o, o + 256, o + 256]);
            let t = Instant::now();
            let mut enc = engine.device.create_command_encoder(&Default::default());
            let r = &doc.resources;
            doc.sets[0].composite(&engine, &mut enc, r, None);
            engine.queue.submit(Some(enc.finish()));
            engine.device.poll(wgpu::Maintain::Wait);
            stroke.push(t.elapsed().as_secs_f32() * 1000.0);
        }
        println!("res {res}: stroke frame (256 x 256 region) {:?} ms", stroke.iter().map(|t| format!("{t:.1}")).collect::<Vec<_>>());
        println!("res {res}: scratch (acc sets, masks, cached masks, MB) {:?}, set VRAM {} MB", engine.compositor.stats(), doc.vram_bytes() / (1024 * 1024));
        engine.compositor.trim();
    }
    Ok(())
}

/// Paint layers and mask paint effects start as 1x1 placeholders; the app
/// sizes them on the first stroke, the tests do it up front.
/// Covered texels of a UV raster (a copy, so the document stays free).
struct Raster(Vec<bool>);

impl Raster {
    fn covered(&self, i: usize) -> bool {
        self.0[i]
    }
}

fn materialize_all(engine: &Engine, doc: &mut Document) {
    for s in &mut doc.sets {
        let res = s.resolution();
        visit_mut(&mut s.layers, &mut |l| {
            l.materialize(&engine.device, res, None);
            let ids: Vec<u64> = l.mask_paint.keys().copied().collect();
            for id in ids {
                l.materialize(&engine.device, res, Some(id));
            }
        });
    }
}

/// The Curves filter: straight curves change nothing, raising the RGB
/// curve's middle brightens a grey, a red curve changes red alone.
fn curves_check(engine: &mut Engine, out: &Path) -> Result<()> {
    let p = out.join("curves.obj");
    std::fs::write(&p, test_obj())?;
    let mut doc = Document::new(engine, mesh::load(&p)?, "curves".into(), 64);
    doc.add_layer(engine, 0, LayerKind::Fill(Box::new(FillDesc::material([0.5, 0.5, 0.5], 0.5, 0.0))));
    let read = |engine: &Engine, doc: &mut Document| -> [f32; 3] {
        doc.sets[0].dirty = true;
        composite_all(engine, doc);
        let v = f16_bytes_to_f32(&doc.sets[0].gpu.final_base.read_bytes(&engine.device, &engine.queue));
        let i = (32 * 64 + 32) * 4;
        [v[i], v[i + 1], v[i + 2]]
    };
    let base = read(engine, &mut doc);
    let id = doc.add_layer(engine, 0, LayerKind::Filter(FilterDesc::of(FilterKind::Curves)));
    let straight = read(engine, &mut doc);
    let edit = |doc: &mut Document, c: usize, pts: Vec<[f32; 2]>| {
        if let Some(LayerKind::Filter(f)) = find_mut(&mut doc.sets[0].layers, id).map(|l| &mut l.desc.kind) {
            f.curves[c] = pts;
        }
    };
    edit(&mut doc, 0, vec![[0.0, 0.0], [0.5, 0.8], [1.0, 1.0]]);
    let bright = read(engine, &mut doc);
    edit(&mut doc, 0, vec![[0.0, 0.0], [1.0, 1.0]]);
    edit(&mut doc, 1, vec![[0.0, 0.0], [0.5, 0.2], [1.0, 1.0]]);
    let red = read(engine, &mut doc);
    println!("curves: base {base:?}, straight {straight:?}, RGB mid up {bright:?}, red down {red:?}");
    ensure!((0..3).all(|k| (straight[k] - base[k]).abs() < 0.01), "straight curves changed the color");
    ensure!((0..3).all(|k| bright[k] > base[k] + 0.1), "the RGB curve did not brighten");
    ensure!(red[0] < base[0] - 0.1 && (red[1] - base[1]).abs() < 0.01 && (red[2] - base[2]).abs() < 0.01, "the red curve must change red only");
    let e = curve_eval(&[[0.0, 0.0], [0.5, 0.8], [1.0, 1.0]], 0.5);
    ensure!((e - 0.8).abs() < 1e-4, "curve_eval misses its point ({e})");
    Ok(())
}

/// Filters reach the normal channel: Levels at its defaults keeps a tilted
/// normal, Invert flips it, Levels with a raised output floor changes it.
fn normal_filter_check(engine: &mut Engine, out: &Path) -> Result<()> {
    let p = out.join("normalfilter.obj");
    std::fs::write(&p, test_obj())?;
    let mut doc = Document::new(engine, mesh::load(&p)?, "normal filter".into(), 256);
    let mut f = FillDesc::default();
    f.channels[CH_NORMAL].enabled = true;
    f.channels[CH_NORMAL].value = [0.4, 0.2, 0.894];
    doc.add_layer(engine, 0, LayerKind::Fill(Box::new(f)));
    let read = |engine: &Engine, doc: &mut Document| -> Vec<f32> {
        doc.sets[0].dirty = true;
        composite_all(engine, doc);
        f16_bytes_to_f32(&doc.sets[0].gpu.final_normal.read_bytes(&engine.device, &engine.queue))
    };
    let diff = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
    let base = read(engine, &mut doc);
    let only_normal = |k: FilterKind| {
        let mut d = FilterDesc::of(k);
        d.channels = [false; NCH];
        d.channels[CH_NORMAL] = true;
        d
    };
    let lv = doc.add_layer(engine, 0, LayerKind::Filter(only_normal(FilterKind::Levels)));
    let identity = read(engine, &mut doc);
    if let Some(LayerKind::Filter(f)) = find_mut(&mut doc.sets[0].layers, lv).map(|l| &mut l.desc.kind) {
        f.p[3] = 0.6; // output low
    }
    let leveled = read(engine, &mut doc);
    if let Some(LayerKind::Filter(f)) = find_mut(&mut doc.sets[0].layers, lv).map(|l| &mut l.desc.kind) {
        *f = only_normal(FilterKind::Invert);
    }
    let inverted = read(engine, &mut doc);
    let (di, dl, dv) = (diff(&base, &identity), diff(&base, &leveled), diff(&base, &inverted));
    println!("normal filters: levels at defaults {di:.4}, levels output 0.6 {dl:.3}, invert {dv:.3}");
    ensure!(di < 0.01, "Levels at its defaults changed the normal ({di})");
    ensure!(dl > 0.05 && dv > 0.2, "filters do not reach the normal channel (levels {dl}, invert {dv})");
    Ok(())
}

/// A fill's 3D projection placement (the viewport manipulator): moving it
/// changes the result, moving it back restores it exactly, rotating changes it.
fn projection_check(engine: &mut Engine, out: &Path) -> Result<()> {
    let p = out.join("projection.obj");
    std::fs::write(&p, test_obj())?;
    let mut doc = Document::new(engine, mesh::load(&p)?, "projection".into(), 512);
    let mut f = FillDesc::default();
    f.channels[CH_COLOR] = FillChannel {
        source: SourceKind::Procedural,
        procedural: ProcParams::of(ProcKind::Bricks),
        value: [0.1, 0.1, 0.1],
        value_b: [0.9, 0.9, 0.9],
        ..Default::default()
    };
    f.projection = Projection { mode: ProjMode::Triplanar, scale: 4.0, ..Default::default() };
    let id = doc.add_layer(engine, 0, LayerKind::Fill(Box::new(f)));
    let read = |engine: &Engine, doc: &mut Document, edit: &dyn Fn(&mut Projection)| -> Vec<f32> {
        if let Some(LayerKind::Fill(f)) = find_mut(&mut doc.sets[0].layers, id).map(|l| &mut l.desc.kind) {
            edit(&mut f.projection);
        }
        doc.sets[0].dirty = true;
        composite_all(engine, doc);
        f16_bytes_to_f32(&doc.sets[0].gpu.final_base.read_bytes(&engine.device, &engine.queue))
    };
    let diff = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
    let base = read(engine, &mut doc, &|_| {});
    let moved = read(engine, &mut doc, &|p| p.position = [0.07, 0.0, 0.03]);
    let back = read(engine, &mut doc, &|p| p.position = [0.0; 3]);
    let turned = read(engine, &mut doc, &|p| p.orient = glam::Quat::from_rotation_y(0.6).to_array());
    let scaled = read(engine, &mut doc, &|p| {
        p.orient = [0.0, 0.0, 0.0, 1.0];
        p.axes = [2.0, 1.0, 1.0];
    });
    let (dm, db, dt, ds) = (diff(&base, &moved), diff(&base, &back), diff(&base, &turned), diff(&base, &scaled));
    println!("projection placement: move {dm:.3}, back {db:.5}, rotate {dt:.3}, scale {ds:.3}");
    ensure!(dm > 0.1 && dt > 0.1 && ds > 0.1, "moving / rotating / scaling the projection changed nothing");
    ensure!(db < 1e-4, "moving the projection back did not restore it ({db})");
    Ok(())
}

/// The opacity channel reaches the final base color alpha, from a fill and
/// from a paint stroke.
/// Meshes load as polygons: an OBJ cube made of quads, and the same cube
/// triangulated, both give 6 polygons (the second by tris-to-quads).
fn quad_check(out: &Path) -> Result<()> {
    let v = "v -1 -1 -1\nv 1 -1 -1\nv 1 1 -1\nv -1 1 -1\nv -1 -1 1\nv 1 -1 1\nv 1 1 1\nv -1 1 1\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\n";
    let quads = [[1, 2, 3, 4], [8, 7, 6, 5], [1, 5, 6, 2], [2, 6, 7, 3], [3, 7, 8, 4], [5, 1, 4, 8]];
    let mut quad_obj = v.to_string();
    let mut tri_obj = v.to_string();
    for q in quads {
        quad_obj += &format!("f {}/1 {}/2 {}/3 {}/4\n", q[3], q[2], q[1], q[0]);
        tri_obj += &format!("f {}/1 {}/2 {}/3\nf {}/1 {}/3 {}/4\n", q[3], q[2], q[1], q[3], q[1], q[0]);
    }
    let mut counts = Vec::new();
    for (name, text) in [("quads", quad_obj), ("tris", tri_obj)] {
        let path = out.join(format!("cube_{name}.obj"));
        std::fs::write(&path, text)?;
        let mesh = crate::mesh::load(&path)?;
        let s = &mesh.submeshes[0];
        let polys: std::collections::HashSet<u32> = s.tri_polys.iter().copied().collect();
        anyhow::ensure!(s.tri_polys.len() * 3 == s.indices.len(), "{name}: one polygon id per triangle");
        anyhow::ensure!(polys.len() == 6, "{name}: cube loaded as {} polygons, expected 6", polys.len());
        counts.push(polys.len());
    }
    println!("quads: cube of quads {} polygons, triangulated cube {} polygons", counts[0], counts[1]);
    Ok(())
}

fn opacity_check(engine: &mut Engine, out: &Path) -> Result<()> {
    let p = out.join("opacity.obj");
    std::fs::write(&p, test_obj())?;
    let mut doc = Document::new(engine, mesh::load(&p)?, "opacity".into(), 512);
    doc.add_layer(engine, 0, LayerKind::Fill(Box::new(FillDesc::default())));
    ensure!(doc.sets[0].shader_alpha() == 0.0, "auto alpha blends a stack without opacity");
    let mut f = FillDesc::default();
    f.channels[CH_OPACITY].enabled = true;
    f.channels[CH_OPACITY].value = [0.25; 3];
    doc.add_layer(engine, 0, LayerKind::Fill(Box::new(f)));
    ensure!(doc.sets[0].shader_alpha() == 2.0, "auto alpha does not blend a stack with opacity");
    let alpha = |engine: &Engine, doc: &mut Document| -> f32 {
        doc.sets[0].dirty = true;
        composite_all(engine, doc);
        let v = f16_bytes_to_f32(&doc.sets[0].gpu.final_base.read_bytes(&engine.device, &engine.queue));
        let s = &doc.sets[0];
        let (mut sum, mut n) = (0.0, 0);
        for (i, t) in s.raster.tri.iter().enumerate() {
            if *t >= 0 {
                sum += v[i * 4 + 3];
                n += 1;
            }
        }
        sum / n.max(1) as f32
    };
    let a_fill = alpha(engine, &mut doc);
    let top = doc.add_layer(engine, 0, LayerKind::Paint);
    materialize_all(engine, &mut doc);
    find_mut(&mut doc.sets[0].layers, top).unwrap().paint.as_mut().unwrap().ensure_b(&engine.device);
    let cam = OrbitCamera::default();
    let mut u = uniform(&cam, engine, MODE_UV, [0.8, 0.1, 0.1]);
    u.color[3] = 1.0;
    let stamps: Vec<Stamp> = (0..40)
        .flat_map(|y| (0..40).map(move |x| Stamp { pos: [x as f32 / 40.0, y as f32 / 40.0, 0.0, 0.05], nrm: [0.0, 0.0, 1.0, 1.0], ext: [0.0; 4] }))
        .collect();
    engine.paint.upload(&engine.queue, u, &stamps);
    let mut enc = engine.device.create_command_encoder(&Default::default());
    {
        let l = find(&doc.sets[0].layers, top).unwrap();
        let e = &mut *engine;
        for pass in [PaintPass::PaintA, PaintPass::PaintB] {
            e.paint.draw(&e.device, &mut enc, &doc.gpu_mesh, 0, None, &paint_targets(l, pass), e.viewport.depth(), pass, [true; 7]);
        }
    }
    engine.queue.submit(Some(enc.finish()));
    let a_paint = alpha(engine, &mut doc);
    println!("opacity: fill 0.25 gives alpha {a_fill:.3}, painted 1.0 over it gives {a_paint:.3}");
    ensure!((a_fill - 0.25).abs() < 0.02, "fill opacity does not reach the base alpha ({a_fill})");
    ensure!(a_paint > 0.9, "painted opacity does not reach the base alpha ({a_paint})");
    Ok(())
}

/// Hiding / showing the edited layer over an empty paint layer (no plan
/// entry) matches a composite without the prefix cache.
fn visibility_check(engine: &mut Engine, out: &Path) -> Result<()> {
    let p = out.join("visibility.obj");
    std::fs::write(&p, test_obj())?;
    let mut doc = Document::new(engine, mesh::load(&p)?, "visibility".into(), 512);
    doc.add_layer(engine, 0, LayerKind::Paint);
    doc.add_layer(engine, 0, LayerKind::Fill(Box::new(FillDesc::material([0.8, 0.1, 0.1], 0.5, 0.0))));
    let top = doc.add_layer(engine, 0, LayerKind::Fill(Box::new(FillDesc::material([0.1, 0.8, 0.1], 0.5, 0.0))));
    let read = |engine: &Engine, doc: &mut Document, focus: Option<u64>| -> Vec<f32> {
        doc.sets[0].focus = focus;
        doc.sets[0].dirty = true;
        composite_all(engine, doc);
        f16_bytes_to_f32(&doc.sets[0].gpu.final_base.read_bytes(&engine.device, &engine.queue))
    };
    let diff = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(p, q)| (p - q).abs()).fold(0.0, f32::max);
    let mut worst: f32 = 0.0;
    for visible in [true, false, true, false] {
        find_mut(&mut doc.sets[0].layers, top).unwrap().desc.visible = visible;
        let cached = read(engine, &mut doc, Some(top));
        let reference = read(engine, &mut doc, None);
        worst = worst.max(diff(&cached, &reference));
    }
    println!("visibility toggles over the prefix cache: max diff {worst:.5}");
    ensure!(worst < 1e-3, "toggling the edited layer's visibility shows a stale composite ({worst})");
    Ok(())
}

/// The tiled compositor (and stroke regions) against one untiled pass.
fn tiling_check(engine: &mut Engine, out: &Path) -> Result<()> {
    let p = out.join("tiling.obj");
    std::fs::write(&p, test_obj())?;
    let res = 2048;
    let mut doc = Document::new(engine, mesh::load(&p)?, "tiling".into(), res);
    for sm in library::procedural_smart_materials().iter().take(3) {
        let l = doc.instantiate(engine, 0, &sm.tree);
        doc.insert_layer(0, l);
    }
    let mut blur = FilterDesc::of(FilterKind::Blur);
    blur.p[0] = 6.0;
    doc.add_layer(engine, 0, LayerKind::Filter(blur));
    doc.add_layer(engine, 0, LayerKind::Fill(Box::new(library::material("Gold"))));
    doc.add_mask(engine, 0, false);
    doc.add_mask_effect(engine, 0, MaskEffectKind::Generator(GeneratorDesc::of(Generator::EdgeWear)));
    doc.add_mask_effect(engine, 0, MaskEffectKind::Blur { radius: 4.0 });
    let top = doc.add_layer(engine, 0, LayerKind::Paint);
    materialize_all(engine, &mut doc);
    let cam = OrbitCamera::default();
    let paint = |engine: &mut Engine, doc: &Document, stamps: &[Stamp], mode: f32| {
        let l = find(&doc.sets[0].layers, top).unwrap();
        let u = uniform(&cam, engine, mode, [0.8, 0.1, 0.1]);
        engine.paint.upload(&engine.queue, u, stamps);
        let mut enc = engine.device.create_command_encoder(&Default::default());
        let e = &mut *engine;
        e.paint.draw(&e.device, &mut enc, &doc.gpu_mesh, 0, None, &paint_targets(l, PaintPass::PaintA), e.viewport.depth(), PaintPass::PaintA, [true; 7]);
        e.queue.submit(Some(enc.finish()));
    };
    let stamps: Vec<Stamp> = (0..20)
        .map(|k| Stamp { pos: [0.2 + k as f32 * 0.02, 0.6, 0.0, 0.03], nrm: [0.0, 0.0, 1.0, 1.0], ext: [0.0; 4] })
        .collect();
    paint(engine, &doc, &stamps, MODE_UV);
    let read = |engine: &Engine, doc: &Document| -> Vec<Vec<f32>> {
        let g = &doc.sets[0].gpu;
        [&g.final_base, &g.final_orm, &g.final_normal, &g.final_emissive]
            .iter()
            .map(|t| f16_bytes_to_f32(&t.read_bytes(&engine.device, &engine.queue)))
            .collect()
    };
    let diff = |a: &[Vec<f32>], b: &[Vec<f32>]| -> f32 {
        a.iter().zip(b).flat_map(|(x, y)| x.iter().zip(y).map(|(p, q)| (p - q).abs())).fold(0.0, f32::max)
    };
    let full = |engine: &Engine, doc: &mut Document| {
        doc.sets[0].dirty = true;
        composite_all(engine, doc);
    };
    full(engine, &mut doc);
    let tiled = read(engine, &doc);
    engine.compositor.force_untiled.set(true);
    engine.compositor.trim();
    full(engine, &mut doc);
    let untiled = read(engine, &doc);
    engine.compositor.force_untiled.set(false);
    engine.compositor.trim();
    let d_tiles = diff(&tiled, &untiled);

    // Erase over the painted texels (both passes): region composite against
    // a full one, and no NaN / broken normal anywhere.
    {
        let l = find_mut(&mut doc.sets[0].layers, top).unwrap();
        l.paint.as_mut().unwrap().ensure_b(&engine.device);
    }
    full(engine, &mut doc);
    let stamps_e: Vec<Stamp> = (0..20).map(|k| Stamp { pos: [0.2 + k as f32 * 0.02, 0.6, 0.0, 0.03], nrm: [0.0, 0.0, 1.0, 1.0], ext: [0.0; 4] }).collect();
    for pass in [PaintPass::PaintA, PaintPass::PaintB] {
        let l = find(&doc.sets[0].layers, top).unwrap();
        let u = uniform(&cam, engine, MODE_UV, [0.8, 0.1, 0.1]);
        engine.paint.upload(&engine.queue, u, &stamps_e);
        let mut enc = engine.device.create_command_encoder(&Default::default());
        let e = &mut *engine;
        e.paint.draw(&e.device, &mut enc, &doc.gpu_mesh, 0, None, &paint_targets(l, pass), e.viewport.depth(), pass, [true; 7]);
        e.queue.submit(Some(enc.finish()));
    }
    full(engine, &mut doc);
    let painted = read(engine, &doc);
    let stamps_x: Vec<Stamp> = (0..20).map(|k| Stamp { pos: [0.22 + k as f32 * 0.02, 0.6, 0.0, 0.02], nrm: [0.0, 0.0, 1.0, 1.0], ext: [0.0; 4] }).collect();
    for pass in [PaintPass::EraseA, PaintPass::EraseB] {
        let l = find(&doc.sets[0].layers, top).unwrap();
        let u = uniform(&cam, engine, MODE_UV, [0.8, 0.1, 0.1]);
        engine.paint.upload(&engine.queue, u, &stamps_x);
        let mut enc = engine.device.create_command_encoder(&Default::default());
        let e = &mut *engine;
        let tp = if pass == PaintPass::EraseA { PaintPass::PaintA } else { PaintPass::PaintB };
        e.paint.draw(&e.device, &mut enc, &doc.gpu_mesh, 0, None, &paint_targets(l, tp), e.viewport.depth(), pass, [true; 7]);
        e.queue.submit(Some(enc.finish()));
    }
    let r = |v: f32| (v * res as f32) as u32;
    doc.sets[0].dirty_region = Some([r(0.15), r(0.5), r(0.7), r(0.7)]);
    composite_all(engine, &mut doc);
    let erased_region = read(engine, &doc);
    full(engine, &mut doc);
    let erased_full = read(engine, &doc);
    let d_erase = diff(&erased_region, &erased_full);
    let d_change = diff(&painted, &erased_full);
    let bad = erased_full.iter().flatten().filter(|v| !v.is_finite()).count();
    let n = &erased_full[2];
    let flipped = n.chunks_exact(4).filter(|p| p[2] < -0.01).count();
    println!("erase: region vs full {d_erase:.5}, erased change {d_change:.3}, non-finite {bad}, flipped normals {flipped}");
    for (ti, name) in ["base", "orm", "normal", "emissive"].iter().enumerate() {
        let (mut worst, mut at) = (0.0f32, 0usize);
        for (i, (x, y)) in painted[ti].iter().zip(&erased_full[ti]).enumerate() {
            if (x - y).abs() > worst {
                worst = (x - y).abs();
                at = i;
            }
        }
        let px = at / 4;
        let o = px * 4;
        println!("  {name}: max change {worst:.3} at texel ({}, {}): before {:?} after {:?}", px as u32 % res, px as u32 / res, &painted[ti][o..o + 4], &erased_full[ti][o..o + 4]);
    }
    ensure!(bad == 0 && flipped == 0, "erasing produced NaN or flipped normals ({bad}, {flipped})");
    ensure!(d_erase < 2e-3, "erase region composite differs from a full one ({d_erase})");
    ensure!(d_change > 0.05, "the erase stroke changed nothing");

    // A stroke recomposited in its rect only, against a full pass.
    full(engine, &mut doc);
    let before = read(engine, &doc);
    let stamps: Vec<Stamp> = (0..10)
        .map(|k| Stamp { pos: [0.55 + k as f32 * 0.01, 0.3, 0.0, 0.02], nrm: [0.0, 0.0, 1.0, 1.0], ext: [0.0; 4] })
        .collect();
    paint(engine, &doc, &stamps, MODE_UV);
    let r = |v: f32| (v * res as f32) as u32;
    doc.sets[0].dirty_region = Some([r(0.5), r(0.25), r(0.7), r(0.35)]);
    composite_all(engine, &mut doc);
    let region = read(engine, &doc);
    full(engine, &mut doc);
    let reference = read(engine, &doc);
    let d_region = diff(&region, &reference);
    let d_stroke = diff(&before, &reference);
    ensure!(d_stroke > 0.05, "the test stroke changed nothing ({d_stroke})");
    println!("tiling at {res}: tiled vs untiled max diff {d_tiles:.5}, stroke region vs full {d_region:.5}");
    ensure!(d_tiles < 2e-3, "tiled composite differs from the untiled one ({d_tiles})");
    ensure!(d_region < 2e-3, "region composite differs from a full one ({d_region})");
    Ok(())
}

/// `--procsheet <dir>`: every seamless procedural at its defaults, one PNG
/// each (snake_case names), to compare with the reference look.
pub fn proc_sheet(out: &Path) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let engine = Engine::headless()?;
    let mut n = 0;
    for k in ProcKind::ALL.iter().copied().filter(|k| k.is_bitmap()) {
        let pp = ProcParams::of(k);
        let Some(u) = pp.gen_u(2048) else { continue };
        let mut enc = engine.device.create_command_encoder(&Default::default());
        let tex = engine.procgen.get(&engine.device, &mut enc, &u);
        engine.queue.submit(Some(enc.finish()));
        let v = f16_bytes_to_f32(&tex.read_bytes(&engine.device, &engine.queue));
        let w = tex.width;
        let img = image::GrayImage::from_fn(w, w, |x, y| image::Luma([(v[((y * w + x) * 4) as usize].clamp(0.0, 1.0) * 255.0 + 0.5) as u8]));
        let name: String = k.name().to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
        img.save(out.join(format!("{name}.png")))?;
        n += 1;
    }
    println!("{n} procedurals written to {}", out.display());
    Ok(())
}

/// `--matsheet <dir>`: the shelf thumbnail of every texture material, one
/// contact sheet (`materials.png`, a row per folder).
/// Diagnostic: each fill layer of a project's active set, its layer stack
/// thumbnail (as the app renders it) next to the model with only that layer
/// shown (mask off), lit like the viewport. Reads the project, writes PNGs.
pub fn layer_shot(project_path: &Path, out: &Path) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let mut engine = Engine::headless()?;
    if let Some(p) = library::default_environment() {
        let (w, h, px) = crate::engine::env::load_hdr(&p)?;
        let e = crate::engine::env::Environment::build(&engine.device, &engine.queue, w, h, px, "default".into());
        engine.set_environment(e);
    }
    let (mut doc, cam) = project::load(&engine, project_path)?;
    engine.viewport.resize(&engine.device, 384, 384);
    let set = doc.active;
    let mut fills: Vec<u64> = Vec::new();
    visit(&doc.sets[set].layers, &mut |l| {
        if matches!(l.desc.kind, LayerKind::Fill(_)) {
            fills.push(l.desc.id);
        }
    });
    let mut pv_mat = crate::preview::Previewer::new_layer(&engine);
    let t = crate::preview::THUMB;
    let mut sheet = image::RgbaImage::new(t * 3 + 384, 384 * fills.len().max(1) as u32);
    for (row, id) in fills.iter().enumerate() {
        let Some(l) = find(&doc.sets[set].layers, *id) else { continue };
        let LayerKind::Fill(f) = &l.desc.kind else { continue };
        let name = l.desc.name.clone();
        // What the fill writes: channel, source, value.
        let mut what = String::new();
        for (c, ch) in f.channels.iter().enumerate().filter(|(_, c)| c.enabled) {
            what.push_str(&format!(" {}={:?}{:.2?}", CHANNELS[c], ch.source, ch.value));
        }
        let base = &doc.sets[set].settings;
        println!("{row:2} {name}:{what} | projection {:?} | set base {:.2?} {:.2?}", f.projection.mode, base.base_color, base.base_values);
        let tree = LayerTree { desc: LayerDesc::new(0, String::new(), LayerKind::Fill(f.clone())), children: Vec::new() };
        // The app's preview, the viewport's default look.
        let look = crate::preview::Look::default();
        let tex = pv_mat.render_fill(&engine, &tree, &doc.resources, look, Some(&doc.sets[set].settings));
        let thumb = image::RgbaImage::from_raw(tex.width, tex.height, tex.read_bytes(&engine.device, &engine.queue)).context("thumb")?;
        let big = image::imageops::resize(&thumb, t * 3, t * 3, image::imageops::FilterType::Triangle);
        image::imageops::overlay(&mut sheet, &big, 0, row as i64 * 384);
        // Solo: this layer (and its folders) only, without its mask.
        let mut saved: Vec<(u64, bool, Option<bool>)> = Vec::new();
        let mut keep: Vec<u64> = vec![*id];
        fn parents(layers: &[Layer], id: u64, path: &mut Vec<u64>) -> bool {
            for l in layers {
                if l.desc.id == id {
                    return true;
                }
                path.push(l.desc.id);
                if parents(&l.children, id, path) {
                    return true;
                }
                path.pop();
            }
            false
        }
        parents(&doc.sets[set].layers, *id, &mut keep);
        visit_mut(&mut doc.sets[set].layers, &mut |l| {
            saved.push((l.desc.id, l.desc.visible, l.desc.mask.as_ref().map(|m| m.enabled)));
            l.desc.visible = keep.contains(&l.desc.id);
            if l.desc.id == *id || keep.contains(&l.desc.id) {
                if let Some(m) = &mut l.desc.mask {
                    m.enabled = false;
                }
            }
        });
        doc.sets[set].dirty = true;
        composite_all(&engine, &mut doc);
        {
            let vp = &engine.viewport;
            let m = cam.view_proj(vp.width as f32 / vp.height as f32);
            let env = &engine.env;
            let rot = std::f32::consts::PI;
            // The viewport's default grading (linear tone mapping).
            let post = PostUniform { a: [1.0, 3.0, 0.0, 1.5], b: [0.0, 1.0, 1.0, 0.0], texel: [0.0; 4] };
            let frame = Frame {
                camera: CameraUniform {
                    view_proj: m.to_cols_array_2d(),
                    inv_view_proj: m.inverse().to_cols_array_2d(),
                    eye: cam.eye().extend(doc.mesh.diagonal()).into(),
                    env: [rot, 1.0, 4.0, (env.mip_count as f32 - 3.0).max(1.0)],
                    display: [0.0, 1.0, 0.0, 1.0],
                    sh: env.sh,
                    light_view_proj: glam::Mat4::IDENTITY.to_cols_array_2d(),
                    light: crate::preview::viewport_key_light(env, rot),
                    wire: [0.85, 0.85, 0.9, 0.35],
                },
                post,
                mesh: Some(&doc.gpu_mesh),
                sets: doc
                    .sets
                    .iter()
                    .map(|s| DrawSet {
                        submesh: s.submesh,
                        base: &s.gpu.final_base,
                        orm: &s.gpu.final_orm,
                        normal: &s.gpu.final_normal,
                        emissive: &s.gpu.final_emissive,
                        display: &s.gpu.final_base,
                        params: SetUniform { k: [0.0, 0.5, 1.0, 0.0] },
                        visible: true,
                        transmissive: false,
                    })
                    .collect(),
                shadows: false,
                wireframe: false,
                wire_only: None,
                selection: None,
                path_traced: false,
            };
            let mut enc = engine.device.create_command_encoder(&Default::default());
            engine.viewport.render(&engine.device, &engine.queue, &mut enc, &frame);
            engine.queue.submit(Some(enc.finish()));
        }
        let o = engine.viewport.output();
        let shot = image::RgbaImage::from_raw(o.width, o.height, o.read_bytes(&engine.device, &engine.queue)).context("shot")?;
        image::imageops::overlay(&mut sheet, &shot, (t * 3) as i64, row as i64 * 384);
        visit_mut(&mut doc.sets[set].layers, &mut |l| {
            if let Some((_, v, m)) = saved.iter().find(|s| s.0 == l.desc.id) {
                l.desc.visible = *v;
                if let (Some(mk), Some(e)) = (&mut l.desc.mask, m) {
                    mk.enabled = *e;
                }
            }
        });

    }
    sheet.save(out.join("layers.png"))?;
    println!("{} fills written to {}", fills.len(), out.join("layers.png").display());
    Ok(())
}

pub fn mat_sheet(out: &Path) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let mut engine = Engine::headless()?;
    // Lit like the app: the default environment.
    if let Some(p) = library::default_environment() {
        let (w, h, px) = crate::engine::env::load_hdr(&p)?;
        let e = crate::engine::env::Environment::build(&engine.device, &engine.queue, w, h, px, "default".into());
        engine.set_environment(e);
    }
    let mats = library::texture_materials();
    ensure!(!mats.is_empty(), "no texture materials found");
    let mut pv = crate::preview::Previewer::new_material(&engine);
    let t = crate::preview::THUMB;
    let cols = 10u32;
    let rows = (mats.len() as u32).div_ceil(cols);
    let mut sheet = image::RgbaImage::new(cols * t, rows * t);
    for (i, m) in mats.iter().enumerate() {
        let mut resources = std::collections::HashMap::new();
        let mut ids = std::collections::HashMap::new();
        for (k, (stem, _)) in library::TEX_MATERIAL_MAPS.iter().enumerate() {
            let Some(img) = m.map(stem).and_then(|p| image::open(p).ok()) else { continue };
            let img = img.resize_exact(512, 512, image::imageops::FilterType::Triangle).to_rgba8();
            let id = k as u64 + 1;
            let gpu = crate::engine::texture::rgba8_with_mips(&engine.device, &engine.queue, stem, &img);
            resources.insert(id, Resource { id, name: stem.to_string(), kind: ResKind::Texture, imported: false, encoded: Default::default(), gpu: std::sync::Arc::new(gpu), image: std::sync::Arc::new(img) });
            ids.insert(*stem, id);
        }
        let mut fd = m.fill(&ids);
        fd.projection.scale = 2.0;
        let tree = LayerTree { desc: LayerDesc::new(0, m.name.clone(), LayerKind::Fill(Box::new(fd))), children: Vec::new() };
        let tex = pv.render(&engine, &tree, &resources);
        let px = tex.read_bytes(&engine.device, &engine.queue);
        let img = image::RgbaImage::from_raw(tex.width, tex.height, px).ok_or_else(|| anyhow::anyhow!("thumbnail bytes"))?;
        image::imageops::overlay(&mut sheet, &img, (i as u32 % cols * t) as i64, (i as u32 / cols * t) as i64);
        println!("{:3} {:9} {}", i, m.category, m.name);
    }
    sheet.save(out.join("materials.png"))?;
    println!("{} materials written to {}", mats.len(), out.join("materials.png").display());
    Ok(())
}

/// `--matshot <dir> [name filter]`: large sphere renders of texture
/// materials (512 px on a 1024 set), as applied and with height off.
pub fn mat_shot(out: &Path, filter: &str) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let mut engine = Engine::headless()?;
    if filter != "calib" {
        if let Some(p) = library::default_environment() {
            let (w, h, px) = crate::engine::env::load_hdr(&p)?;
            let e = crate::engine::env::Environment::build(&engine.device, &engine.queue, w, h, px, "default".into());
            engine.set_environment(e);
        }
    }
    let mut pv = if filter == "calib" { crate::preview::Previewer::sized(&engine, 1024, 512) } else { crate::preview::Previewer::sized_material(&engine, 2048, 512) };
    let mut thumb = crate::preview::Previewer::new_material(&engine);
    if filter == "calib" {
        // Gray dielectric spheres at increasing roughness, one sheet.
        let rs = [0.1f32, 0.3, 0.5, 0.7, 0.9];
        let mut sheet = image::RgbaImage::new(512 * rs.len() as u32, 512);
        for (i, r) in rs.iter().enumerate() {
            let fd = FillDesc::material([0.5; 3], *r, 0.0);
            let tree = LayerTree { desc: LayerDesc::new(0, "calib".into(), LayerKind::Fill(Box::new(fd))), children: Vec::new() };
            let tex = pv.render(&engine, &tree, &std::collections::HashMap::new());
            let img = image::RgbaImage::from_raw(tex.width, tex.height, tex.read_bytes(&engine.device, &engine.queue)).unwrap();
            image::imageops::overlay(&mut sheet, &img, i as i64 * 512, 0);
        }
        sheet.save(out.join("calib.png"))?;
        return Ok(());
    }
    let filter = filter.to_lowercase();
    // "procs": every procedural pattern, flat, in one sheet (12 a row, in
    // `ProcKind::ALL` order, names printed).
    if filter == "procs" {
        let mut flat = crate::preview::Previewer::new_flat(&engine);
        let none = std::collections::HashMap::new();
        let ks = ProcKind::ALL;
        let mut sheet = image::RgbaImage::new(128 * 12, 128 * (ks.len() as u32).div_ceil(12));
        for (i, k) in ks.iter().enumerate() {
            let mut f = FillDesc::default();
            f.channels[CH_COLOR] = FillChannel { source: SourceKind::Procedural, procedural: ProcParams::of(*k), value: [0.03; 3], value_b: [0.9; 3], ..Default::default() };
            f.projection = Projection { mode: ProjMode::Object3D, scale: 3.0, ..Default::default() };
            let tree = LayerTree { desc: LayerDesc::new(0, k.name().into(), LayerKind::Fill(Box::new(f))), children: Vec::new() };
            let tex = flat.render(&engine, &tree, &none);
            let img = image::RgbaImage::from_raw(tex.width, tex.height, tex.read_bytes(&engine.device, &engine.queue)).unwrap();
            image::imageops::overlay(&mut sheet, &img, (i as i64 % 12) * 128, (i as i64 / 12) * 128);
            println!("{i}: {}", k.name());
        }
        sheet.save(out.join("procs.png"))?;
        return Ok(());
    }
    // "sm:<name>": smart materials without files, one strip each: the
    // layers added one by one on a sphere, then the result on the box.
    if let Some(f) = filter.strip_prefix("sm:") {
        let mut ball = crate::preview::Previewer::sized(&engine, 1024, 384);
        let mut cube = crate::preview::Previewer::sized_box(&engine, 1024, 384);
        let none = std::collections::HashMap::new();
        for sm in library::smart_materials().iter().filter(|s| s.textures.is_empty() && s.name.to_lowercase().contains(f)) {
            let n = sm.tree.children.len();
            let mut strip = image::RgbaImage::new(384 * (n as u32 + 1), 384);
            for k in 1..=n {
                let mut tree = sm.tree.clone();
                for (i, c) in tree.children.iter_mut().enumerate() {
                    c.desc.visible = i < k;
                }
                let tex = ball.render(&engine, &tree, &none);
                let img = image::RgbaImage::from_raw(tex.width, tex.height, tex.read_bytes(&engine.device, &engine.queue)).unwrap();
                image::imageops::overlay(&mut strip, &img, (k as i64 - 1) * 384, 0);
            }
            let tex = cube.render(&engine, &sm.tree, &none);
            let img = image::RgbaImage::from_raw(tex.width, tex.height, tex.read_bytes(&engine.device, &engine.queue)).unwrap();
            image::imageops::overlay(&mut strip, &img, n as i64 * 384, 0);
            let slug: String = sm.name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
            strip.save(out.join(format!("{slug}.png")))?;
            println!("{}", sm.name);
        }
        return Ok(());
    }
    for m in library::texture_materials().iter().filter(|m| m.name.to_lowercase().contains(&filter)) {
        let mut resources = std::collections::HashMap::new();
        let mut ids = std::collections::HashMap::new();
        for (k, (stem, _)) in library::TEX_MATERIAL_MAPS.iter().enumerate() {
            let Some(img) = m.map(stem).and_then(|p| image::open(p).ok()) else { continue };
            let img = img.to_rgba8();
            let id = k as u64 + 1;
            let gpu = crate::engine::texture::rgba8_with_mips(&engine.device, &engine.queue, stem, &img);
            resources.insert(id, Resource { id, name: stem.to_string(), kind: ResKind::Texture, imported: false, encoded: Default::default(), gpu: std::sync::Arc::new(gpu), image: std::sync::Arc::new(img) });
            ids.insert(*stem, id);
        }
        let slug: String = m.name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
        let variants: [(&str, fn(&mut FillDesc)); 3] = [
            ("as_is", |_| {}),
            ("no_height", |f| f.channels[CH_HEIGHT].enabled = false),
            ("no_bump", |f| {
                f.channels[CH_HEIGHT].enabled = false;
                f.channels[CH_NORMAL].enabled = false;
            }),
        ];
        for (v, edit) in variants {
            let mut fd = m.fill(&ids);
            edit(&mut fd);
            let tree = LayerTree { desc: LayerDesc::new(0, m.name.clone(), LayerKind::Fill(Box::new(fd))), children: Vec::new() };
            let tex = pv.render(&engine, &tree, &resources);
            screenshot(&engine, &tex, &out.join(format!("{slug}_{v}.png")))?;
            if v == "as_is" {
                let t = thumb.render(&engine, &tree, &resources);
                screenshot(&engine, &t, &out.join(format!("{slug}_thumb.png")))?;
            }
        }
        println!("{}", m.name);
    }
    Ok(())
}

/// `--bakeshot <mesh> <dir> [res]`: bakes every texture set with the
/// default settings and writes AO, curvature and thickness as UV-space PNGs.
/// `--floodbench <mesh> [res]`: magic wand / bucket timings per texture
/// set (seam links, color readback, contiguous flood).
pub fn flood_bench(mesh_path: &Path, res: u32) -> Result<()> {
    let engine = Engine::headless()?;
    let m = mesh::load(mesh_path)?;
    let doc = Document::new(&engine, m, "floodbench".into(), res);
    for (i, s) in doc.sets.iter().enumerate() {
        let t = Instant::now();
        let seams = crate::flood::Seams::build(&doc.mesh, s.submesh, &s.raster);
        let t_seams = t.elapsed();
        let t = Instant::now();
        let raw = crate::engine::texture::f16_bytes_to_f32(&s.gpu.final_base.read_bytes(&engine.device, &engine.queue));
        let t_read = t.elapsed();
        let Some(seed) = s.raster.tri.iter().position(|t| *t >= 0) else { continue };
        let t = Instant::now();
        let out = crate::flood::flood(&s.raster, &seams, seed, true, &|_| true);
        let t_flood = t.elapsed();
        let covered = s.raster.tri.iter().filter(|t| **t >= 0).count();
        let n = out.iter().filter(|b| **b > 0).count();
        println!(
            "set {i} '{}': {} links {:.0} ms, readback {:.0} ms ({} values), flood {n} / {covered} texels {:.0} ms",
            s.name,
            seams.link_count(),
            t_seams.as_secs_f64() * 1e3,
            t_read.as_secs_f64() * 1e3,
            raw.len(),
            t_flood.as_secs_f64() * 1e3
        );
    }
    Ok(())
}

pub fn bake_shot(mesh_path: &Path, out: &Path, res: u32) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let engine = Engine::headless()?;
    let m = mesh::load(mesh_path)?;
    let doc = Document::new(&engine, m, "bakeshot".into(), res);
    let tris = bake::mesh_triangles(&doc.mesh);
    let bvh = bake::build_bvh(&doc.mesh, &tris);
    let settings = BakeSettings::default();
    let (p, c) = (AtomicU32::new(0), AtomicBool::new(false));
    let gb = crate::engine::gpubake::GpuBaker::new(engine.device.clone(), engine.queue.clone(), &bvh);
    for (i, s) in doc.sets.iter().enumerate() {
        let t = Instant::now();
        let b = bake::bake(&doc.mesh, s.submesh, res, &bvh, None, &settings, &p, &c, Some(&gb));
        let gray = |v: &[f32]| image::GrayImage::from_fn(res, res, |x, y| image::Luma([(v[(y * res + x) as usize].clamp(0.0, 1.0) * 255.0 + 0.5) as u8]));
        gray(&b.ao).save(out.join(format!("{i}_ao.png")))?;
        gray(&b.curvature).save(out.join(format!("{i}_curvature.png")))?;
        gray(&b.thickness).save(out.join(format!("{i}_thickness.png")))?;
        println!("set {} '{}': {:.2}s", i, s.name, t.elapsed().as_secs_f32());
    }
    Ok(())
}

/// `--autouv <mesh> <dir> [smart|fast]`: unwraps the mesh (replacing its
/// UVs), prints each set's report, writes `autouv.obj` and renders a
/// checker on the new UVs from four angles (stretch and seams show).
/// Triangles whose UVs are mirrored (the texture reads backwards on them):
/// their UV winding, v up, disagrees with their facing. (mirrored, counted)
fn mirrored_uvs(m: &crate::mesh::MeshData) -> (usize, usize) {
    let (mut bad, mut all) = (0, 0);
    for s in &m.submeshes {
        for t in s.indices.chunks_exact(3) {
            let v = |k: usize| &m.vertices[t[k] as usize];
            let p = |k: usize| glam::Vec3::from(v(k).pos);
            let g = (p(1) - p(0)).cross(p(2) - p(0));
            let n = glam::Vec3::from(v(0).normal) + glam::Vec3::from(v(1).normal) + glam::Vec3::from(v(2).normal);
            let (a, b, c) = (v(0).uv, v(1).uv, v(2).uv);
            // Image-space v grows downward: counter-clockwise (v up) is negative here.
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            let facing = g.dot(n);
            if area.abs() < 1e-12 || facing.abs() < 1e-12 {
                continue;
            }
            all += 1;
            bad += ((area < 0.0) != (facing > 0.0)) as usize;
        }
    }
    (bad, all)
}

pub fn uv_check(mesh_path: &Path, out: &Path, method: &str) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let mut engine = Engine::headless()?;
    let m = mesh::load(mesh_path)?;
    let mut settings = mesh::UvSettings::default();
    if method == "fast" {
        settings.method = mesh::UvMethod::Fast;
    }
    // "free": smart without keeping islands upright.
    settings.smart.upright = method != "free";
    let t = Instant::now();
    let (m, reports) = mesh::unwrap_mesh(&m, &settings)?;
    println!("{:?}: {} triangles in {:.1} s", settings.method, m.triangle_count(), t.elapsed().as_secs_f32());
    for r in &reports {
        println!(
            "  {:<16} {:>6} tris  {:>4} islands  coverage {:>3.0} %  stretch {:.2} (worst 5 % {:.2})  seams {:.1}",
            r.name, r.triangles, r.charts, r.coverage * 100.0, r.stretch_mean, r.stretch_p95, r.seam_length
        );
    }
    let (mirrored, total) = mirrored_uvs(&m);
    println!("  mirrored triangles: {mirrored} of {total}");
    mesh::write_obj(&m, &out.join("autouv.obj"))?;
    let mut doc = Document::new(&engine, m, "autouv".into(), 1024);
    for i in 0..doc.sets.len() {
        let mut f = FillDesc::material([0.5; 3], 0.6, 0.0);
        f.channels[CH_COLOR] = FillChannel {
            source: SourceKind::Procedural,
            procedural: ProcParams::of(ProcKind::Checker),
            value: [0.06, 0.07, 0.09],
            value_b: [0.95, 0.55, 0.15],
            ..Default::default()
        };
        f.projection = Projection { mode: ProjMode::Uv, scale: 24.0, ..Default::default() };
        let l = doc.new_layer(&engine, i, LayerKind::Fill(Box::new(f)), None);
        doc.insert_layer(i, l);
    }
    engine.viewport.resize(&engine.device, 900, 900);
    composite_all(&engine, &mut doc);
    for (k, (yaw, pitch)) in [(0.0f32, 0.15f32), (std::f32::consts::PI, 0.15), (1.57, 0.1), (0.8, -0.9)].into_iter().enumerate() {
        let mut cam = OrbitCamera { yaw, pitch, ..Default::default() };
        cam.frame(doc.mesh.bbox_min, doc.mesh.bbox_max);
        let mut enc = engine.device.create_command_encoder(&Default::default());
        let mut frame = frame_for(&engine, &doc, &cam, 0.0, 0.0, &pick_fn(|s: &TextureSet| &s.gpu.final_base));
        frame.wireframe = false;
        engine.viewport.render(&engine.device, &engine.queue, &mut enc, &frame);
        engine.queue.submit(Some(enc.finish()));
        screenshot(&engine, engine.viewport.output(), &out.join(format!("checker_{k}.png")))?;
    }
    let _ = &mut engine;
    Ok(())
}

/// `--studioshot <project.sprp | mesh> <out.png>`: a packshot through the
/// studio, the project's own (or MF_STUDIO=<json> over it) from its camera
/// (MF_YAW / MF_PITCH in degrees, MF_SETUP=<n> a lighting setup), MF_SPP
/// samples (256), MF_NODENOISE: raw.
/// A plain mesh gets a grey satin material.
pub fn studio_shot(input: &Path, out: &Path) -> Result<()> {
    let mut engine = Engine::headless()?;
    let (mut doc, mut cam) = if input.extension().is_some_and(|e| e.eq_ignore_ascii_case("sprp")) {
        project::load(&engine, input)?
    } else {
        let mut doc = Document::new(&engine, mesh::load(input)?, "studio".into(), 1024);
        for i in 0..doc.sets.len() {
            let f = FillDesc::material([0.7, 0.7, 0.7], 0.35, 0.0);
            let l = doc.new_layer(&engine, i, LayerKind::Fill(Box::new(f)), None);
            doc.insert_layer(i, l);
        }
        let mut cam = OrbitCamera::default();
        cam.frame(doc.mesh.bbox_min, doc.mesh.bbox_max);
        (doc, cam)
    };
    if let Ok(p) = std::env::var("MF_STUDIO") {
        doc.studio = serde_json::from_slice(&std::fs::read(&p)?).with_context(|| format!("reading {p}"))?;
    }
    // MF_SETUP=<n>: one of the lighting setups.
    if let Some(i) = std::env::var("MF_SETUP").ok().and_then(|v| v.parse().ok()) {
        doc.studio.setup(i);
    }
    let deg = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<f32>().ok()).map(f32::to_radians);
    if let Some(y) = deg("MF_YAW") {
        cam.yaw = y;
    }
    if let Some(p) = deg("MF_PITCH") {
        cam.pitch = p;
    }
    let st = &doc.studio;
    let aspect = st.format.aspect().unwrap_or(1.0);
    let long = std::env::var("MF_LONG").ok().and_then(|v| v.parse().ok()).unwrap_or(1024u32);
    let (w, h) = if aspect >= 1.0 { (long, (long as f32 / aspect).round() as u32) } else { ((long as f32 * aspect).round() as u32, long) };
    cam.ortho = false;
    cam.fov_y = 2.0 * st.tan_half_y(aspect).atan();
    // MF_FRAME: centered like entering the studio.
    if std::env::var("MF_FRAME").is_ok() {
        let pts: Vec<glam::Vec3> = doc.mesh.vertices.iter().map(|v| glam::Vec3::from(v.pos)).collect();
        crate::studio::fit_camera(&mut cam, &pts, st.tan_half_y(aspect), aspect, crate::studio::FRAME_FILL);
    }
    engine.viewport.resize(&engine.device, w, h);
    let mut enc = engine.device.create_command_encoder(&Default::default());
    let resources = doc.resources.clone();
    for s in doc.sets.iter_mut() {
        s.composite(&engine, &mut enc, &resources, None);
    }
    engine.queue.submit(Some(enc.finish()));
    let (min, max) = (doc.mesh.bbox_min, doc.mesh.bbox_max);
    let gpu = doc.studio.gpu(cam.yaw, cam.eye(), (min + max) * 0.5, ((max - min).length() * 0.5).max(1e-4), min.y, 1.0);
    engine.pathtracer.build_scene(&engine.device, &doc, 1);
    engine.pathtracer.denoise_on = std::env::var("MF_NODENOISE").is_err();
    engine.pathtracer.bounces = 6;
    let spp: u32 = std::env::var("MF_SPP").ok().and_then(|v| v.parse().ok()).unwrap_or(256);
    engine.pathtracer.max_samples = spp;
    engine.pathtracer.reset();
    let m = cam.view_proj(w as f32 / h as f32);
    let (mut am, mut cut, mut emi) = ([0.0f32; 8], [0.5f32; 8], [0.0f32; 8]);
    for (i, s) in doc.sets.iter().take(8).enumerate() {
        am[i] = s.shader_alpha();
        cut[i] = s.settings.alpha_cutoff;
        emi[i] = s.settings.emissive_intensity;
    }
    let u = crate::engine::pathtrace::PtUniform {
        inv_view_proj: m.inverse().to_cols_array_2d(),
        eye: cam.eye().extend(doc.mesh.diagonal()).into(),
        size: [0; 4],
        env: [180f32.to_radians(), 1.0, 0.0, 0.0],
        emi_a: [emi[0], emi[1], emi[2], emi[3]],
        emi_b: [emi[4], emi[5], emi[6], emi[7]],
        amode_a: [am[0], am[1], am[2], am[3]],
        amode_b: [am[4], am[5], am[6], am[7]],
        cut_a: [cut[0], cut[1], cut[2], cut[3]],
        cut_b: [cut[4], cut[5], cut[6], cut[7]],
    };
    let slots: Vec<[&Tex; 4]> = doc.sets.iter().take(8).map(|s| [&s.gpu.final_base, &s.gpu.final_orm, &s.gpu.final_normal, &s.gpu.final_emissive]).collect();
    let t = Instant::now();
    for _ in 0..spp {
        let mut enc = engine.device.create_command_encoder(&Default::default());
        engine.pathtracer.render(&engine.device, &mut enc, u, &slots, &engine.env.view, engine.viewport.hdr(), &gpu);
        engine.queue.submit(Some(enc.finish()));
    }
    engine.device.poll(wgpu::Maintain::Wait);
    let traced = t.elapsed().as_secs_f32();
    let denoised = engine.pathtracer.denoise_on && engine.pathtracer.denoise_now(&engine.device, &engine.queue, engine.viewport.hdr());
    let mut enc = engine.device.create_command_encoder(&Default::default());
    let frame = Frame {
        camera: bytemuck::Zeroable::zeroed(),
        post: PostUniform { a: [1.0, 3.0, 0.0, 1.0], b: [0.0, 1.0, 1.0, 0.0], texel: [0.0; 4] },
        mesh: None,
        sets: Vec::new(),
        shadows: false,
        wireframe: false,
        wire_only: None,
        selection: None,
        path_traced: true,
    };
    engine.viewport.render(&engine.device, &engine.queue, &mut enc, &frame);
    engine.queue.submit(Some(enc.finish()));
    screenshot(&engine, engine.viewport.output(), out)?;
    println!("studio shot {w} x {h}, {spp} samples in {traced:.1} s{}: {}", if denoised { ", denoised" } else { "" }, out.display());
    Ok(())
}

/// `--alphashot <dir> [mesh]`: every set alpha blended at 40% opacity (and
/// once with transmission), rendered from three angles, to check blending.
pub fn alpha_shot(out: &Path, mesh_path: Option<&Path>) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let mut engine = Engine::headless()?;
    let m = match mesh_path {
        Some(p) => mesh::load(p)?,
        None => {
            let p = out.join("alpha.obj");
            std::fs::write(&p, test_obj())?;
            mesh::load(&p)?
        }
    };
    let mut doc = Document::new(&engine, m, "alpha".into(), 1024);
    let transmit = std::env::var("MF_TRANSMIT").is_ok();
    // MF_TRANSMIT=<set index> picks the glass set (default 0).
    let glass: usize = std::env::var("MF_TRANSMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    for i in 0..doc.sets.len() {
        let mut f = FillDesc::material([0.8, 0.35, 0.1], 0.3, 0.0);
        if transmit {
            // One set is clear glass over the others (opaque).
            if i == glass {
                doc.sets[i].settings.ext_channels[1] = true;
                f = FillDesc::material([0.85, 0.93, 1.0], 0.05, 0.0);
                f.ext[1].enabled = true;
                f.ext[1].value = [1.0; 3];
            }
        } else {
            doc.sets[i].settings.alpha_mode = AlphaMode::AlphaBlend;
            f.channels[CH_OPACITY].enabled = true;
            f.channels[CH_OPACITY].value = [0.4; 3];
        }
        let l = doc.new_layer(&engine, i, LayerKind::Fill(Box::new(f)), None);
        doc.insert_layer(i, l);
    }
    engine.viewport.resize(&engine.device, 1000, 800);
    let mut enc = engine.device.create_command_encoder(&Default::default());
    let resources = doc.resources.clone();
    for s in doc.sets.iter_mut() {
        s.composite(&engine, &mut enc, &resources, None);
    }
    engine.queue.submit(Some(enc.finish()));
    for (k, (yaw, pitch)) in [(0.6f32, 0.3f32), (2.4, -0.2), (4.0, 0.6)].into_iter().enumerate() {
        let mut cam = OrbitCamera { yaw, pitch, ..Default::default() };
        cam.frame(doc.mesh.bbox_min, doc.mesh.bbox_max);
        if std::env::var("MF_NOSORT").is_err() {
            let centers = crate::document::tri_centers(&doc.mesh);
            let blended: Vec<usize> = doc.sets.iter().map(|s| s.submesh).collect();
            let eye = cam.eye();
            let fwd = (glam::Vec3::from(cam.target) - eye).normalize_or(glam::Vec3::NEG_Z);
            crate::document::sort_blended(&mut doc, &engine, &centers, &blended, eye, fwd, cam.ortho);
        }
        // MF_PT=<spp>: the path tracer instead (MF_NODENOISE: raw).
        let pt_spp: Option<u32> = std::env::var("MF_PT").ok().and_then(|v| v.parse().ok());
        if let Some(spp) = pt_spp {
            engine.pathtracer.build_scene(&engine.device, &doc, k as u64 + 1);
            engine.pathtracer.denoise_on = std::env::var("MF_NODENOISE").is_err();
            engine.pathtracer.reset();
            let vp = &engine.viewport;
            let m = cam.view_proj(vp.width as f32 / vp.height as f32);
            let mut am = [0.0f32; 8];
            for (i, s) in doc.sets.iter().take(8).enumerate() {
                am[i] = s.shader_alpha();
            }
            let u = crate::engine::pathtrace::PtUniform {
                inv_view_proj: m.inverse().to_cols_array_2d(),
                eye: cam.eye().extend(doc.mesh.diagonal()).into(),
                size: [0; 4],
                env: [0.0, 1.0, 0.0, 4.0],
                emi_a: [1.0; 4],
                emi_b: [1.0; 4],
                amode_a: [am[0], am[1], am[2], am[3]],
                amode_b: [am[4], am[5], am[6], am[7]],
                cut_a: [0.5; 4],
                cut_b: [0.5; 4],
            };
            let slots: Vec<[&Tex; 4]> = doc.sets.iter().map(|s| [&s.gpu.final_base, &s.gpu.final_orm, &s.gpu.final_normal, &s.gpu.final_emissive]).collect();
            for _ in 0..spp {
                let mut enc = engine.device.create_command_encoder(&Default::default());
                engine.pathtracer.render(&engine.device, &mut enc, u, &slots, &engine.env.view, engine.viewport.hdr(), &Default::default());
                engine.queue.submit(Some(enc.finish()));
            }
            // Open Image Denoise when it is there (else the raw render stays).
            if engine.pathtracer.denoise_on {
                let ok = engine.pathtracer.denoise_now(&engine.device, &engine.queue, engine.viewport.hdr());
                println!("denoiser: {}", if ok { "Open Image Denoise" } else { "none" });
            }
        }
        let mut enc = engine.device.create_command_encoder(&Default::default());
        let mut frame = frame_for(&engine, &doc, &cam, 0.0, 0.0, &pick_fn(|s: &TextureSet| &s.gpu.final_base));
        frame.path_traced = pt_spp.is_some();
        frame.wireframe = false;
        for (d, s) in frame.sets.iter_mut().zip(&doc.sets) {
            d.params.k[0] = s.shader_alpha();
            d.transmissive = s.settings.ext_channels[1];
        }
        engine.viewport.render(&engine.device, &engine.queue, &mut enc, &frame);
        engine.queue.submit(Some(enc.finish()));
        screenshot(&engine, engine.viewport.output(), &out.join(format!("alpha_{k}.png")))?;
    }
    println!("alpha shots in {}", out.display());
    Ok(())
}

/// `--matpreviews [name filter]` (MF_PREVIEW_MISSING=1: only those without
/// one): renders the shelf preview of every
/// starter texture material into its folder (`preview.png`, 256 px): a
/// sphere displaced by the height map, path traced with the denoiser in a
/// neutral studio light, like the previews of material libraries.
pub fn material_previews(filter: &str) -> Result<()> {
    let mut engine = Engine::headless()?;
    let size = 512u32;
    engine.viewport.resize(&engine.device, size, size);
    let spp: u32 = std::env::var("MF_PREVIEW_SPP").ok().and_then(|v| v.parse().ok()).unwrap_or(384);
    let filter = filter.to_lowercase();
    // MF_PREVIEW_MISSING=1: only the materials that have no preview yet.
    let missing = std::env::var("MF_PREVIEW_MISSING").is_ok_and(|v| v == "1");
    for m in library::texture_materials().iter().filter(|m| m.name.to_lowercase().contains(&filter) && !(missing && m.preview().is_some())) {
        let t = Instant::now();
        let height = m.map("height").and_then(|p| image::open(p).ok()).map(|i| i.to_rgba8());
        // Metals keep a smooth silhouette (their height is fine relief).
        let amount = if m.metal { 0.012 } else { 0.035 };
        let mesh = crate::preview::displaced_sphere(height.as_ref(), 1.0, amount);
        let mut doc = Document::new(&engine, mesh, "preview".into(), 2048);
        doc.sets[0].layers.clear();
        let mut ids = std::collections::HashMap::new();
        for (stem, _) in library::TEX_MATERIAL_MAPS {
            let Some(path) = m.map(stem) else { continue };
            let bytes = std::fs::read(&path)?;
            ids.insert(stem, doc.add_resource(&engine, format!("{stem}.jpg"), bytes, ResKind::Texture)?);
        }
        let mut fd = m.fill(&ids);
        // The sphere's UVs already repeat the maps; the displacement is the
        // height (no second bump from it).
        fd.projection.scale = 1.0;
        fd.channels[CH_HEIGHT].enabled = false;
        let l = doc.new_layer(&engine, 0, LayerKind::Fill(Box::new(fd)), None);
        doc.insert_layer(0, l);
        let mut enc = engine.device.create_command_encoder(&Default::default());
        let resources = doc.resources.clone();
        doc.sets[0].composite(&engine, &mut enc, &resources, None);
        engine.queue.submit(Some(enc.finish()));

        let mut cam = OrbitCamera { yaw: 0.0, pitch: 0.12, fov_y: 0.5, ..Default::default() };
        cam.frame(doc.mesh.bbox_min, doc.mesh.bbox_max);
        cam.distance *= 0.6;
        engine.pathtracer.build_scene(&engine.device, &doc, t.elapsed().as_nanos() as u64);
        engine.pathtracer.denoise_on = true;
        engine.pathtracer.bounces = 6;
        engine.pathtracer.reset();
        let m4 = cam.view_proj(1.0);
        let u = crate::engine::pathtrace::PtUniform {
            inv_view_proj: m4.inverse().to_cols_array_2d(),
            eye: cam.eye().extend(doc.mesh.diagonal()).into(),
            size: [0; 4],
            // Studio light, black background (alpha from the coverage).
            env: [0.0, 1.0, 2.0, 4.0],
            emi_a: [1.0; 4],
            emi_b: [1.0; 4],
            ..Default::default()
        };
        let slots: Vec<[&Tex; 4]> = doc.sets.iter().map(|s| [&s.gpu.final_base, &s.gpu.final_orm, &s.gpu.final_normal, &s.gpu.final_emissive]).collect();
        for k in 0..spp {
            let mut enc = engine.device.create_command_encoder(&Default::default());
            engine.pathtracer.render(&engine.device, &mut enc, u, &slots, &engine.env.view, engine.viewport.hdr(), &Default::default());
            engine.queue.submit(Some(enc.finish()));
            if k % 64 == 63 {
                engine.device.poll(wgpu::Maintain::Wait);
            }
        }
        engine.pathtracer.denoise_now(&engine.device, &engine.queue, engine.viewport.hdr());
        let mut frame = frame_for(&engine, &doc, &cam, 0.0, 0.0, &|s| &s.gpu.final_base);
        frame.path_traced = true;
        frame.wireframe = false;
        frame.camera.view_proj = m4.to_cols_array_2d();
        frame.post.a = [std::env::var("MF_PREVIEW_EXPOSURE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.7), 0.0, 0.0, 1.0];
        frame.post.b = [0.0, 1.0, 1.0, 0.0];
        let mut enc = engine.device.create_command_encoder(&Default::default());
        engine.viewport.render(&engine.device, &engine.queue, &mut enc, &frame);
        engine.queue.submit(Some(enc.finish()));
        drop(frame);
        let out = engine.viewport.output();
        let mut img = image::RgbaImage::from_raw(out.width, out.height, out.read_bytes(&engine.device, &engine.queue)).context("preview size")?;
        // Transparent around the sphere: edge pixels hold the surface over
        // black, i.e. premultiplied by their coverage.
        if let Some(cov) = engine.pathtracer.coverage(&engine.device, &engine.queue) {
            for (i, p) in img.pixels_mut().enumerate() {
                let a = cov.get(i).copied().unwrap_or(1.0).clamp(0.0, 1.0);
                if a > 0.0 {
                    for c in 0..3 {
                        p.0[c] = (p.0[c] as f32 / a).min(255.0) as u8;
                    }
                }
                p.0[3] = (a * 255.0 + 0.5) as u8;
            }
        }
        let small = image::imageops::resize(&img, 256, 256, image::imageops::FilterType::Lanczos3);
        small.save(m.dir.join("preview.png"))?;
        println!("{:28} {:.1}s", m.name, t.elapsed().as_secs_f32());
    }
    Ok(())
}
