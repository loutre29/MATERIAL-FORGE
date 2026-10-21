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
            })
            .collect(),
        shadows: display == 0.0,
        wireframe: display == 0.0,
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

fn screenshot(engine: &Engine, t: &Tex, path: &Path) -> Result<()> {
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
    let sm = library::smart_materials().into_iter().find(|s| s.name == "Steel painted worn").context("smart material")?;
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
    let view = View { view_proj: vpm, inv_view_proj: vpm.inverse(), eye: cam.eye(), size: vp_size, fov_y: cam.fov_y, ppp: 1.0, ortho_wpp: None };
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
        engine.uvview.render(&engine.device, &mut enc, &doc.sets[0].gpu.final_base, 0, 560.0, [20.0, 20.0], Some((&doc.gpu_mesh, 0)), 0.4);
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
        };
        for _ in 0..96 {
            let mut enc = engine.device.create_command_encoder(&Default::default());
            engine.pathtracer.render(&engine.device, &mut enc, u, &slots, &engine.env.view, engine.viewport.hdr());
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

    // Shelf thumbnail.
    let mut pv = crate::preview::Previewer::new(&engine);
    let thumb = pv.render(&engine, &library::smart_materials()[1].tree, &doc2.resources);
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

    opacity_check(&mut engine, out)?;
    projection_check(&mut engine, out)?;
    normal_filter_check(&mut engine, out)?;
    visibility_check(&mut engine, out)?;
    tiling_check(&mut engine, out)?;

    println!("SELFTEST OK in {:.1}s, outputs in {}", t0.elapsed().as_secs_f32(), out.display());
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
        for sm in library::smart_materials().iter().take(6) {
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
    for sm in library::smart_materials().iter().take(3) {
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
