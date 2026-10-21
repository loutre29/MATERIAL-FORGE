//! `sp_retro --uitest <dir>`: drives the real UI headless through egui with
//! synthetic input. Every window, tool, display mode, layout, layer kind,
//! mask effect and shelf item is exercised, strokes are painted with the
//! mouse, history is unwound and the project is saved and reopened. Any panic
//! or GPU validation error fails the run.

use super::*;
use crate::document::{FillDesc, FilterDesc, FilterKind, GeneratorDesc, LayerKind, MaskEffectKind, ProcKind};
use eframe::egui::{Event, PointerButton, Pos2, RawInput};
use panels::Cmd;
use std::path::Path;

struct Driver {
    ctx: egui::Context,
    size: egui::Vec2,
    frames: usize,
}

impl Driver {
    fn frame(&mut self, app: &mut RetroApp, events: Vec<Event>, modifiers: Modifiers) {
        let mut raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
            events,
            modifiers,
            ..Default::default()
        };
        app.filter_input(&self.ctx, &mut raw);
        let _ = self.ctx.run(raw, |c| app.ui_frame(c));
        self.frames += 1;
    }

    fn idle(&mut self, app: &mut RetroApp, n: usize) {
        for _ in 0..n {
            self.frame(app, Vec::new(), Modifiers::NONE);
        }
    }

    /// Press, drag along `path`, release (left button).
    fn drag(&mut self, app: &mut RetroApp, path: &[Pos2], modifiers: Modifiers) {
        let Some(&first) = path.first() else { return };
        self.frame(app, vec![Event::PointerMoved(first)], modifiers);
        self.frame(
            app,
            vec![Event::PointerButton { pos: first, button: PointerButton::Primary, pressed: true, modifiers }],
            modifiers,
        );
        for p in &path[1..] {
            self.frame(app, vec![Event::PointerMoved(*p)], modifiers);
        }
        let last = *path.last().unwrap();
        self.frame(
            app,
            vec![Event::PointerButton { pos: last, button: PointerButton::Primary, pressed: false, modifiers }],
            modifiers,
        );
    }

    fn click(&mut self, app: &mut RetroApp, p: Pos2, modifiers: Modifiers) {
        self.drag(app, &[p], modifiers);
    }
}

fn line(a: Pos2, b: Pos2, n: usize) -> Vec<Pos2> {
    (0..=n).map(|k| a + (b - a) * (k as f32 / n as f32)).collect()
}

pub fn ui_smoke(out: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(out)?;
    // Never the user's library, layout or autosaves; no native dialogs.
    std::env::set_var("MF_DATA_DIR", out.join("data"));
    HEADLESS.store(true, std::sync::atomic::Ordering::Relaxed);
    let t0 = Instant::now();
    slider_check()?;
    let engine = Engine::headless()?;
    let renderer = egui_wgpu::Renderer::new(&engine.device, wgpu::TextureFormat::Bgra8Unorm, None, 1, false);
    let ctx = egui::Context::default();
    let obj = out.join("ui.obj");
    std::fs::write(&obj, crate::selftest::test_obj())?;
    let mut app = RetroApp::build(engine, Arc::new(egui::mutex::RwLock::new(renderer)), &ctx, Some(obj));
    app.persist_layout = false;
    let size = std::env::var("MF_UITEST_SIZE")
        .ok()
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some(egui::vec2(w.parse().ok()?, h.parse().ok()?))))
        .unwrap_or(egui::vec2(1600.0, 950.0));
    let mut d = Driver { ctx, size, frames: 0 };
    d.idle(&mut app, 3);
    anyhow::ensure!(app.doc.is_some(), "project was not created: {}", app.status);
    println!("project created ({} frames, {:.1}s)", d.frames, t0.elapsed().as_secs_f32());


    // Every window.
    app.show_view_settings = true;
    app.show_set_settings = true;
    app.show_history = true;
    app.show_log = true;
    app.show_help = true;
    app.open_export();
    let sets = app.doc.as_ref().unwrap().sets.len();
    app.bake_dialog = Some(BakeDialog { settings: Default::default(), high: String::new(), sets: vec![true; sets] });
    app.new_dialog = Some(NewProjectDialog { path: String::new(), res: 1024, error: None });
    d.idle(&mut app, 3);
    app.new_dialog = None;
    app.bake_dialog = None;
    app.export_dialog = None;
    app.show_help = false;

    // Every layer kind and mask effect.
    let mut cmds = vec![
        Cmd::AddLayer(LayerKind::Paint),
        Cmd::AddLayer(LayerKind::Fill(Box::new(FillDesc::default()))),
        Cmd::AddLayer(LayerKind::Folder),
    ];
    for k in FilterKind::ALL {
        cmds.push(Cmd::AddLayer(LayerKind::Filter(FilterDesc::of(*k))));
    }
    cmds.push(Cmd::AddLayer(LayerKind::Fill(Box::new(FillDesc::default()))));
    cmds.push(Cmd::AddMask(false));
    for g in crate::document::Generator::ALL {
        cmds.push(Cmd::AddEffect(MaskEffectKind::Generator(GeneratorDesc::of(*g))));
    }
    for p in [ProcKind::Grunge, ProcKind::Scratches, ProcKind::Bricks] {
        cmds.push(Cmd::AddEffect(MaskEffectKind::fill_procedural(p)));
    }
    cmds.extend([
        Cmd::AddEffect(MaskEffectKind::levels()),
        Cmd::AddEffect(MaskEffectKind::Blur { radius: 3.0 }),
        Cmd::AddEffect(MaskEffectKind::Invert),
        Cmd::AddEffect(MaskEffectKind::Threshold { value: 0.5 }),
        Cmd::AddEffect(MaskEffectKind::ColorSelect { colors: vec![[1.0, 0.0, 0.0]], tolerance: 0.1 }),
        Cmd::AddEffect(MaskEffectKind::Paint),
        Cmd::Duplicate,
        Cmd::Group,
        Cmd::Ungroup,
        Cmd::Move(1),
        Cmd::Move(-1),
    ]);
    for c in cmds {
        app.run_cmd(c);
        d.idle(&mut app, 1);
    }
    if let Some(m) = crate::library::smart_masks().into_iter().next() {
        app.run_cmd(Cmd::ApplySmartMask(m.mask));
        d.idle(&mut app, 1);
    }

    // Shelf: one item of every kind (environment files may be absent).
    let items = app.shelf.all_items();
    let mut seen = std::collections::HashSet::new();
    for it in items {
        let kind = std::mem::discriminant(&it);
        if seen.insert(kind) {
            app.apply_item(it);
            d.idle(&mut app, 1);
        }
    }
    for c in 0..8 {
        app.shelf.set_category(c);
        d.idle(&mut app, 2);
    }

    // Select every layer, its mask and every mask effect (property editors).
    let mut targets = Vec::new();
    crate::document::visit(&app.doc.as_ref().unwrap().sets[0].layers, &mut |l| {
        targets.push((l.desc.id, Target::Content));
        if let Some(m) = &l.desc.mask {
            targets.push((l.desc.id, Target::Mask));
            for e in &m.effects {
                targets.push((l.desc.id, Target::MaskEffect(e.id)));
            }
        }
    });
    let ntargets = targets.len();
    for (id, t) in targets {
        app.run_cmd(Cmd::Select(id, t));
        d.idle(&mut app, 1);
    }
    println!("{ntargets} layer / mask / effect selections drawn");

    // Displays and layouts.
    for disp in Display::ALL {
        app.view.display = disp;
        d.idle(&mut app, 1);
    }
    app.view.display = Display::Material;
    for l in [Layout::Both, Layout::Only2D, Layout::Only3D] {
        app.view.layout = l;
        d.idle(&mut app, 2);
    }
    app.camera.ortho = true;
    d.idle(&mut app, 1);
    app.camera.ortho = false;

    // Painting with every tool through mouse events on a fresh top paint layer.
    // Floating windows would sit on top of the strokes.
    app.show_view_settings = false;
    app.show_history = false;
    app.show_log = false;
    app.run_cmd(Cmd::AddLayer(LayerKind::Paint));
    d.idle(&mut app, 2);
    let r = app.vp_rect;
    // Screen positions of the box and the ball centers.
    let project = |app: &RetroApp, w: glam::Vec3| -> Pos2 {
        let vp = &app.engine.viewport;
        let c = app.camera.view_proj(vp.width as f32 / vp.height as f32) * w.extend(1.0);
        let n = c.truncate() / c.w;
        Pos2::new(r.min.x + (n.x * 0.5 + 0.5) * r.width(), r.min.y + (0.5 - n.y * 0.5) * r.height())
    };
    let (a, b) = (project(&app, glam::Vec3::new(-2.5, 0.0, 1.0)), project(&app, glam::Vec3::new(1.5, 0.0, 1.2)));
    let undo_before = app.history.version;
    for tool in [Tool::Paint, Tool::Erase, Tool::Clone, Tool::Smudge, Tool::PolygonFill, Tool::Picker, Tool::Hide] {
        app.tool = tool;
        d.idle(&mut app, 1);
        if tool == Tool::Clone {
            // V over the mesh sets the clone source.
            d.frame(&mut app, vec![Event::PointerMoved(a)], Modifiers::NONE);
            d.frame(&mut app, vec![Event::Key { key: egui::Key::V, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], Modifiers::NONE);
            d.frame(&mut app, vec![Event::Key { key: egui::Key::V, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }], Modifiers::NONE);
            anyhow::ensure!(app.clone_src.is_some(), "V did not set the clone source: {}", app.status);
        }
        let before = app.history.version;
        d.drag(&mut app, &line(a, b, 24), Modifiers::NONE);
        d.idle(&mut app, 1);
        println!("  {tool:?}: +{} history, status: {}", app.history.version - before, app.status);
    }
    if let Some(doc) = &mut app.doc {
        doc.reset_hidden();
        doc.refresh_visibility(&app.engine);
    }
    // Line tool, symmetry, stencil-less camera alignment, material brush.
    app.tool = Tool::Paint;
    app.brush.symmetry = [true, false, false];
    app.mat.material = Some(("Rust".into(), crate::library::material("Rust")));
    d.drag(&mut app, &line(a, r.center(), 10), Modifiers::NONE);
    d.click(&mut app, b, Modifiers::SHIFT);
    app.brush.alignment = crate::brush::Alignment::Camera;
    d.drag(&mut app, &line(a + egui::vec2(0.0, 60.0), b + egui::vec2(0.0, 60.0), 12), Modifiers::NONE);
    app.brush.alignment = crate::brush::Alignment::Wrap;
    // 2D view painting.
    app.view.layout = Layout::Only2D;
    d.idle(&mut app, 2);
    let u = app.uv_rect;
    d.drag(&mut app, &line(u.center() - egui::vec2(100.0, 0.0), u.center() + egui::vec2(100.0, 0.0), 12), Modifiers::NONE);
    println!("  extras done, status: {}", app.status);
    app.view.layout = Layout::Only3D;
    d.idle(&mut app, 2);
    let strokes = app.history.version - undo_before;
    println!("painting: {strokes} history entries from mouse strokes");
    anyhow::ensure!(strokes >= 4, "mouse strokes did not paint (status: {})", app.status);

    // Drag and drop from the Assets panel: material onto the layer stack and
    // onto the mesh, project texture onto a fill channel slot.
    let count = |app: &RetroApp| {
        let mut n = 0;
        crate::document::visit(&app.doc.as_ref().unwrap().sets[app.doc.as_ref().unwrap().active].layers, &mut |_| n += 1);
        n
    };
    let drop_on = |d: &mut Driver, app: &mut RetroApp, target: Pos2| -> anyhow::Result<()> {
        let src = marked(&d.ctx, "asset0").ok_or_else(|| anyhow::anyhow!("no asset tile drawn"))?.center();
        let m = Modifiers::NONE;
        d.frame(app, vec![Event::PointerMoved(src)], m);
        d.frame(app, vec![Event::PointerButton { pos: src, button: PointerButton::Primary, pressed: true, modifiers: m }], m);
        for p in line(src, target, 16).into_iter().skip(1) {
            d.frame(app, vec![Event::PointerMoved(p)], m);
        }
        d.frame(app, vec![Event::PointerButton { pos: target, button: PointerButton::Primary, pressed: false, modifiers: m }], m);
        d.idle(app, 2);
        Ok(())
    };
    app.focus_tab(dock::Tab::Layers);
    app.shelf.show_category(shelf::Category::Materials);
    d.idle(&mut app, 3);
    let n0 = count(&app);
    let target = marked(&d.ctx, "layer_top").ok_or_else(|| anyhow::anyhow!("top layer row not drawn"))?.center();
    drop_on(&mut d, &mut app, target)?;
    anyhow::ensure!(count(&app) == n0 + 1, "material dropped on the layer stack did not add a layer");
    let target = app.vp_rect.center();
    drop_on(&mut d, &mut app, target)?;
    anyhow::ensure!(count(&app) == n0 + 2, "material dropped on the viewport did not add a layer");
    let png = out.join("dnd.png");
    image::RgbaImage::from_fn(64, 64, |x, y| image::Rgba([(x * 4) as u8, (y * 4) as u8, 128, 255])).save(&png)?;
    app.open_path(&png);
    d.idle(&mut app, 2);
    anyhow::ensure!(app.import_dialog.as_ref().is_some_and(|d| !d.to_library), "dropping an image did not ask where to import it");
    let ok = marked(&d.ctx, "import:ok").ok_or_else(|| anyhow::anyhow!("import dialog not drawn"))?.center();
    d.click(&mut app, ok, Modifiers::NONE);
    d.idle(&mut app, 1);
    anyhow::ensure!(app.import_dialog.is_none() && app.doc.as_ref().is_some_and(|d| !d.resources.is_empty()), "the import dialog did not import into the project: {}", app.status);
    app.run_cmd(Cmd::AddLayer(LayerKind::Fill(Box::default())));
    app.tool = Tool::Picker;
    d.idle(&mut app, 3);
    let target = marked(&d.ctx, "slot:Base color")
        .ok_or_else(|| {
            let doc = app.doc.as_ref().unwrap();
            let s = &doc.sets[doc.active];
            anyhow::anyhow!(
                "base color slot not drawn (selected {:?} target {:?}, material slot {:?}, status {})",
                s.selected_layer().map(|l| l.desc.name.clone()),
                s.target,
                marked(&d.ctx, "slot:Material mode"),
                app.status
            )
        })?
        .center();
    drop_on(&mut d, &mut app, target)?;
    let assigned = {
        let s = &app.doc.as_ref().unwrap().sets[app.doc.as_ref().unwrap().active];
        matches!(s.selected_layer().map(|l| &l.desc.kind), Some(LayerKind::Fill(f)) if f.channels[0].source == crate::document::SourceKind::Texture)
    };
    anyhow::ensure!(assigned, "texture dropped on the base color slot was not assigned");
    app.tool = Tool::Paint;
    println!("drag and drop: layer stack, viewport and fill slot OK");

    // Keyboard: the Painter key map, Escape order, layer copy / paste.
    let key = |d: &mut Driver, app: &mut RetroApp, k: egui::Key, m: Modifiers| {
        d.frame(app, vec![Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m }], m);
        d.frame(app, vec![Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m }], m);
    };
    for (k, t, stencil) in [
        (egui::Key::Num2, Tool::Erase, false),
        (egui::Key::Num3, Tool::Paint, true),
        (egui::Key::Num4, Tool::PolygonFill, false),
        (egui::Key::Num5, Tool::Smudge, false),
        (egui::Key::Num6, Tool::Clone, false),
        (egui::Key::Num1, Tool::Paint, false),
    ] {
        key(&mut d, &mut app, k, Modifiers::NONE);
        anyhow::ensure!(app.tool == t && app.stencil.enabled == stencil, "key {k:?} selected {:?}", app.tool);
    }
    key(&mut d, &mut app, egui::Key::P, Modifiers::NONE);
    anyhow::ensure!(app.tool == Tool::Picker && app.prev_tool == Some(Tool::Paint), "P is not a temporary picker");
    key(&mut d, &mut app, egui::Key::Num1, Modifiers::NONE);
    key(&mut d, &mut app, egui::Key::C, Modifiers::NONE);
    anyhow::ensure!(app.view.display == Display::BaseColor, "C did not solo the first channel");
    key(&mut d, &mut app, egui::Key::B, Modifiers::NONE);
    anyhow::ensure!(app.view.display == Display::MapAo, "B did not show the first mesh map");
    key(&mut d, &mut app, egui::Key::M, Modifiers::NONE);
    anyhow::ensure!(app.view.display == Display::Material, "M did not restore the material view");
    let lazy = app.brush.lazy;
    key(&mut d, &mut app, egui::Key::D, Modifiers::NONE);
    anyhow::ensure!((app.brush.lazy > 0.0) != (lazy > 0.0), "D did not toggle lazy mouse");
    key(&mut d, &mut app, egui::Key::D, Modifiers::NONE);
    key(&mut d, &mut app, egui::Key::Tab, Modifiers::NONE);
    anyhow::ensure!(app.ui_hidden, "Tab did not hide the UI");
    key(&mut d, &mut app, egui::Key::Tab, Modifiers::NONE);
    let n0 = count(&app);
    // Layer shortcuts act when the layer stack has the focus.
    app.layers_focus = true;
    key(&mut d, &mut app, egui::Key::C, Modifiers::COMMAND);
    key(&mut d, &mut app, egui::Key::V, Modifiers::COMMAND);
    anyhow::ensure!(count(&app) == n0 + 1, "Ctrl+C / Ctrl+V did not paste a layer");
    key(&mut d, &mut app, egui::Key::Q, Modifiers::ALT);
    anyhow::ensure!(app.isolate_saved.is_some(), "Alt+Q did not isolate the texture set");
    key(&mut d, &mut app, egui::Key::Q, Modifiers::ALT);
    // Right click in the viewport opens the quick properties; Escape closes
    // it, then the flyouts one by one.
    let c = app.vp_rect.center();
    d.frame(&mut app, vec![Event::PointerMoved(c)], Modifiers::NONE);
    d.frame(&mut app, vec![Event::PointerButton { pos: c, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE }], Modifiers::NONE);
    d.frame(&mut app, vec![Event::PointerButton { pos: c, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE }], Modifiers::NONE);
    d.idle(&mut app, 1);
    anyhow::ensure!(app.quick_props.is_some(), "right click did not open the quick properties");
    app.show_history = true;
    d.idle(&mut app, 1);
    key(&mut d, &mut app, egui::Key::Escape, Modifiers::NONE);
    anyhow::ensure!(app.quick_props.is_none() && app.show_history, "Escape did not close the quick properties first");
    key(&mut d, &mut app, egui::Key::Escape, Modifiers::NONE);
    anyhow::ensure!(!app.show_history, "Escape did not close the history flyout");
    println!("keyboard: tool keys, C / B / M, D, Tab, Ctrl+C / V, Alt+Q, quick properties and Escape OK");

    // Content effects: a filter on a fill layer's content.
    {
        let fill = {
            let doc = app.doc.as_ref().unwrap();
            let mut f = None;
            crate::document::visit(&doc.sets[doc.active].layers, &mut |l| {
                if f.is_none() && matches!(l.desc.kind, LayerKind::Fill(_)) {
                    f = Some(l.desc.id);
                }
            });
            f
        };
        if let Some(id) = fill {
            app.run_cmd(Cmd::Select(id, Target::Content));
            app.run_cmd(Cmd::AddContentEffect(FilterKind::Blur));
            app.run_cmd(Cmd::AddContentEffect(FilterKind::Hsl));
            d.idle(&mut app, 3);
            let n = {
                let doc = app.doc.as_ref().unwrap();
                crate::document::find(&doc.sets[doc.active].layers, id).map(|l| l.desc.effects.len()).unwrap_or(0)
            };
            anyhow::ensure!(n == 2, "content effects were not added ({n})");
            let target = app.doc.as_ref().map(|d| d.sets[d.active].target);
            anyhow::ensure!(matches!(target, Some(Target::ContentEffect(_))), "content effect not selected");
            let key = |d: &mut Driver, app: &mut RetroApp, k: egui::Key, m: Modifiers| {
                d.frame(app, vec![Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m }], m);
                d.frame(app, vec![Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m }], m);
            };
            app.layers_focus = true;
            key(&mut d, &mut app, egui::Key::Delete, Modifiers::NONE);
            d.idle(&mut app, 2);
            let n = {
                let doc = app.doc.as_ref().unwrap();
                crate::document::find(&doc.sets[doc.active].layers, id).map(|l| l.desc.effects.len()).unwrap_or(0)
            };
            anyhow::ensure!(n == 1, "Delete did not remove the selected content effect ({n})");
            // Every filter kind runs as a content effect.
            for k in FilterKind::ALL {
                app.run_cmd(Cmd::Select(id, Target::Content));
                app.run_cmd(Cmd::AddContentEffect(*k));
                d.idle(&mut app, 1);
                let target = app.doc.as_ref().map(|d| d.sets[d.active].target);
                if let Some(Target::ContentEffect(e)) = target {
                    app.run_cmd(Cmd::RemoveContentEffect(e));
                }
            }
            println!("content effects: add, select, delete OK");
            // Optional channels on the set: the fill shows their chips and slots.
            if let Some(doc) = &mut app.doc {
                let a = doc.active;
                doc.sets[a].settings.ext_channels = [true; crate::document::NEXT];
            }
            app.run_cmd(Cmd::Select(id, Target::Content));
            if let Some(doc) = &mut app.doc {
                let a = doc.active;
                if let Some(LayerKind::Fill(f)) = crate::document::find_mut(&mut doc.sets[a].layers, id).map(|l| &mut l.desc.kind) {
                    f.ext[0].enabled = true;
                    f.ext[1].enabled = true;
                    f.ext[1].value = [0.7; 3];
                }
                doc.sets[a].dirty = true;
            }
            app.focus_tab(dock::Tab::SetSettings);
            d.idle(&mut app, 3);
            app.focus_tab(dock::Tab::Layers);
            d.idle(&mut app, 2);
            println!("optional channels: set settings and fill editors drawn");
        }
    }

    // Mask right click menu commands: copy, paste into another layer, invert.
    {
        let (src, dst) = {
            let doc = app.doc.as_ref().unwrap();
            let s = &doc.sets[doc.active];
            let mut with = None;
            let mut without = None;
            crate::document::visit(&s.layers, &mut |l| {
                if l.desc.mask.is_some() && with.is_none() {
                    with = Some(l.desc.id);
                } else if l.desc.mask.is_none() && without.is_none() && !l.is_folder() {
                    without = Some(l.desc.id);
                }
            });
            (with, without)
        };
        if let (Some(src), Some(dst)) = (src, dst) {
            app.run_cmd(Cmd::CopyMask(src));
            app.run_cmd(Cmd::PasteMask(dst));
            d.idle(&mut app, 2);
            let doc = app.doc.as_ref().unwrap();
            let l = crate::document::find(&doc.sets[doc.active].layers, dst).unwrap();
            anyhow::ensure!(l.desc.mask.is_some(), "Paste into mask did not add the mask");
            let base = l.desc.mask.as_ref().unwrap().base;
            app.run_cmd(Cmd::InvertMaskBase(dst));
            d.idle(&mut app, 1);
            let doc = app.doc.as_ref().unwrap();
            let l = crate::document::find(&doc.sets[doc.active].layers, dst).unwrap();
            anyhow::ensure!((l.desc.mask.as_ref().unwrap().base - (1.0 - base)).abs() < 1e-6, "Invert mask background failed");
            println!("mask menu: copy, paste into mask, invert background OK");
        }
    }

    interaction_checks(&mut d, &mut app)?;

    // Path-traced render for a few frames.
    app.view.path_trace = true;
    d.idle(&mut app, 4);
    anyhow::ensure!(app.engine.pathtracer.frame > 0, "path tracer did not run");
    app.view.path_trace = false;

    // Unwind and replay the whole history.
    let n = app.history.undo.len();
    for _ in 0..n {
        app.undo();
        d.idle(&mut app, 1);
    }
    for _ in 0..n {
        app.redo();
        d.idle(&mut app, 1);
    }
    println!("history: {n} entries undone and redone");

    // Save, reopen, change resolution.
    let proj = out.join("ui.sprp");
    {
        let doc = app.doc.as_ref().unwrap();
        crate::project::save(&app.engine, doc, &app.camera, &proj)?;
    }
    app.open_path(&proj);
    d.idle(&mut app, 2);
    anyhow::ensure!(app.doc.as_ref().is_some_and(|d| d.path.is_some()), "reopen failed: {}", app.status);
    app.pending_res = Some(512);
    d.idle(&mut app, 2);

    println!("UITEST OK: {} frames in {:.1}s", d.frames, t0.elapsed().as_secs_f32());
    Ok(())
}

/// `sp_retro --uibench <mesh>`: frame time per UI / GPU section, headless,
/// for idle frames, hovering the viewport, painting and dragging a slider.
pub fn ui_bench(mesh: &Path) -> anyhow::Result<()> {
    let t_start = Instant::now();
    let engine = Engine::headless()?;
    println!("engine: {:.0} ms", t_start.elapsed().as_secs_f32() * 1000.0);
    let renderer = egui_wgpu::Renderer::new(&engine.device, wgpu::TextureFormat::Bgra8Unorm, None, 1, false);
    let ctx = egui::Context::default();
    let mut app = RetroApp::build(engine, Arc::new(egui::mutex::RwLock::new(renderer)), &ctx, Some(mesh.to_path_buf()));
    app.persist_layout = false;
    // SP_BENCH_SIZE=WxH overrides the window size.
    let size = std::env::var("SP_BENCH_SIZE")
        .ok()
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some(egui::vec2(w.parse().ok()?, h.parse().ok()?))))
        .unwrap_or(egui::vec2(1600.0, 950.0));
    println!("app built: {:.0} ms", t_start.elapsed().as_secs_f32() * 1000.0);
    let mut d = Driver { ctx, size, frames: 0 };
    d.idle(&mut app, 4);
    app.engine.device.poll(wgpu::Maintain::Wait);
    println!("first frames: {:.0} ms", t_start.elapsed().as_secs_f32() * 1000.0);
    anyhow::ensure!(app.doc.is_some(), "mesh did not load: {}", app.status);
    // SP_BENCH_RES=4096 sets the texture set resolution.
    if let Some(res) = std::env::var("SP_BENCH_RES").ok().and_then(|v| v.parse().ok()) {
        let t = Instant::now();
        if let Some(doc) = &mut app.doc {
            for set in 0..doc.sets.len() {
                doc.change_resolution(&app.engine, set, res);
            }
        }
        app.engine.device.poll(wgpu::Maintain::Wait);
        println!("resolution change of every set: {:.0} ms", t.elapsed().as_secs_f32() * 1000.0);
        app.history.clear();
        app.engine.compositor.trim();
        d.idle(&mut app, 3);
        let doc = app.doc.as_ref().unwrap();
        println!("{} texture sets at {res}, VRAM ~{} MB", doc.sets.len(), doc.vram_bytes() / (1024 * 1024));
    }
    // A few layers so the stack is not trivial.
    for k in ["Rust", "Gold", "Iron raw"] {
        let f = crate::library::material(k);
        app.run_cmd(Cmd::AddLayer(LayerKind::Fill(Box::new(f))));
    }
    app.run_cmd(Cmd::AddLayer(LayerKind::Paint));
    d.idle(&mut app, 30);
    let run = |d: &mut Driver, app: &mut RetroApp, name: &str, frames: Vec<Vec<Event>>| {
        app.profile = Some(Default::default());
        let t0 = Instant::now();
        let n = frames.len();
        for ev in frames {
            let t = Instant::now();
            d.frame(app, ev, Modifiers::NONE);
            app.engine.device.poll(wgpu::Maintain::Wait);
            app.prof("FRAME (incl. GPU)", t);
        }
        let total = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("== {name}: {total:.2} ms per frame");
        let mut v: Vec<(&str, f64)> = app.profile.take().unwrap().into_iter().map(|(k, (ms, c))| (k, ms / c.max(1) as f64)).collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        for (k, ms) in v.iter().take(12) {
            println!("   {ms:7.3} ms  {k}");
        }
    };
    let r = app.vp_rect;
    let c = r.center();
    run(&mut d, &mut app, "idle", (0..120).map(|_| Vec::new()).collect());
    run(&mut d, &mut app, "hover viewport", (0..120).map(|i| vec![Event::PointerMoved(c + egui::vec2((i as f32 * 0.3).sin() * 80.0, (i as f32 * 0.2).cos() * 60.0))]).collect());
    // Alt + LMB orbit: the camera moves every frame.
    let alt = Modifiers::ALT;
    let mut orbit = vec![vec![Event::PointerMoved(c)], vec![Event::PointerButton { pos: c, button: PointerButton::Primary, pressed: true, modifiers: alt }]];
    for i in 0..120 {
        orbit.push(vec![Event::PointerMoved(c + egui::vec2(i as f32 * 3.0, (i as f32 * 0.1).sin() * 30.0))]);
    }
    orbit.push(vec![Event::PointerButton { pos: c, button: PointerButton::Primary, pressed: false, modifiers: alt }]);
    app.profile = Some(Default::default());
    let t0 = Instant::now();
    let n = orbit.len();
    for ev in orbit {
        let t = Instant::now();
        d.frame(&mut app, ev, alt);
        app.engine.device.poll(wgpu::Maintain::Wait);
        app.prof("FRAME (incl. GPU)", t);
    }
    println!("== orbit: {:.2} ms per frame", t0.elapsed().as_secs_f64() * 1000.0 / n as f64);
    let mut v: Vec<(&str, f64)> = app.profile.take().unwrap().into_iter().map(|(k, (ms, c))| (k, ms / c.max(1) as f64)).collect();
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    for (k, ms) in v.iter().take(10) {
        println!("   {ms:7.3} ms  {k}");
    }
    let mut stroke = vec![vec![Event::PointerMoved(c)], vec![Event::PointerButton { pos: c, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]];
    for i in 0..120 {
        stroke.push(vec![Event::PointerMoved(c + egui::vec2(-120.0 + i as f32 * 2.0, (i as f32 * 0.1).sin() * 40.0))]);
    }
    let end = c + egui::vec2(120.0, 0.0);
    stroke.push(vec![Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    let rev0 = app.doc.as_ref().map(|d| d.sets[d.active].layers.last().map(|l| l.rev).unwrap_or(0)).unwrap_or(0);
    run(&mut d, &mut app, "paint stroke", stroke);
    let rev1 = app.doc.as_ref().map(|d| d.sets[d.active].layers.last().map(|l| l.rev).unwrap_or(0)).unwrap_or(0);
    println!("   painted: {}  scratch {:?}", rev0 != rev1, app.engine.compositor.stats());
    // Drag the first Painter slider of the Properties panel (brush size).
    let props_x = 36.0 + 200.0 + 40.0;
    let y = 290.0;
    let mut drag = vec![vec![Event::PointerMoved(Pos2::new(props_x, y))], vec![Event::PointerButton { pos: Pos2::new(props_x, y), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]];
    for i in 0..120 {
        drag.push(vec![Event::PointerMoved(Pos2::new(props_x + (i % 60) as f32 * 2.0, y))]);
    }
    drag.push(vec![Event::PointerButton { pos: Pos2::new(props_x, y), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    run(&mut d, &mut app, "drag in properties", drag);
    // Select the fill layer and drag its roughness: recomposites every frame.
    let fill = {
        let doc = app.doc.as_ref().unwrap();
        let s = &doc.sets[doc.active];
        s.layers.iter().find(|l| matches!(l.desc.kind, LayerKind::Fill(_))).map(|l| l.desc.id)
    };
    if let Some(id) = fill {
        app.run_cmd(Cmd::Select(id, Target::Content));
        d.idle(&mut app, 3);
        app.profile = Some(Default::default());
        let t0 = Instant::now();
        for i in 0..120 {
            if let Some(doc) = &mut app.doc {
                let s = &mut doc.sets[doc.active];
                if let Some(l) = crate::document::find_mut(&mut s.layers, id) {
                    l.desc.opacity[1] = 0.5 + 0.5 * (i as f32 * 0.1).sin();
                }
                s.dirty = true;
            }
            let t = Instant::now();
            d.frame(&mut app, Vec::new(), Modifiers::NONE);
            app.engine.device.poll(wgpu::Maintain::Wait);
            app.prof("FRAME (incl. GPU)", t);
        }
        println!("== edit fill (recomposite): {:.2} ms per frame", t0.elapsed().as_secs_f64() * 1000.0 / 120.0);
        let mut v: Vec<(&str, f64)> = app.profile.take().unwrap().into_iter().map(|(k, (ms, c))| (k, ms / c.max(1) as f64)).collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        for (k, ms) in v.iter().take(12) {
            println!("   {ms:7.3} ms  {k}");
        }
    }
    Ok(())
}

/// Two Painter sliders with the same label in one panel (every procedural
/// has a "Scale"): each one drags, and a click on its number types a value.
fn slider_check() -> anyhow::Result<()> {
    let ctx = egui::Context::default();
    let mut vals = [1.0f32, 1.0f32];
    let mut rects = [Rect::NOTHING; 2];
    let run = |events: Vec<Event>, vals: &mut [f32; 2], rects: &mut [Rect; 2]| {
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0))), events, ..Default::default() };
        let _ = ctx.run(raw, |c| {
            egui::CentralPanel::default().show(c, |ui| {
                for i in 0..2 {
                    let (top, w) = (ui.cursor().min, ui.available_width());
                    super::widgets::Slider::new("Scale", &mut vals[i], 0.0..=10.0).show(ui);
                    rects[i] = Rect::from_min_max(top, Pos2::new(top.x + w, ui.cursor().min.y));
                }
            });
        });
    };
    run(Vec::new(), &mut vals, &mut rects);
    run(Vec::new(), &mut vals, &mut rects);
    let press = |p: Pos2, down: bool| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: down, modifiers: Modifiers::NONE };
    // Drag the first track.
    let r = rects[0];
    let (a, b) = (Pos2::new(r.min.x + 40.0, r.min.y + 25.0), Pos2::new(r.min.x + 200.0, r.min.y + 25.0));
    run(vec![Event::PointerMoved(a)], &mut vals, &mut rects);
    run(vec![press(a, true)], &mut vals, &mut rects);
    run(vec![Event::PointerMoved(b)], &mut vals, &mut rects);
    run(vec![press(b, false)], &mut vals, &mut rects);
    anyhow::ensure!((vals[0] - 1.0).abs() > 0.5 && vals[1] == 1.0, "dragging the first \"Scale\" slider gave {vals:?}");
    // Click the second number, type 7, Enter.
    let r = rects[1];
    let v = Pos2::new(r.max.x - 30.0, r.min.y + 8.0);
    run(vec![Event::PointerMoved(v)], &mut vals, &mut rects);
    run(vec![press(v, true)], &mut vals, &mut rects);
    run(vec![press(v, false)], &mut vals, &mut rects);
    run(Vec::new(), &mut vals, &mut rects);
    run(vec![Event::Text("7".into())], &mut vals, &mut rects);
    let enter = Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    run(vec![enter], &mut vals, &mut rects);
    run(Vec::new(), &mut vals, &mut rects);
    anyhow::ensure!((vals[1] - 7.0).abs() < 1e-4, "typing into the second \"Scale\" slider gave {vals:?}");
    println!("sliders: same-label sliders drag and type independently ({vals:?})");
    Ok(())
}

/// Clicks that used to land somewhere else: each check drives the mouse and
/// asserts what must (not) happen.
fn interaction_checks(d: &mut Driver, app: &mut RetroApp) -> anyhow::Result<()> {
    let key = |d: &mut Driver, app: &mut RetroApp, k: egui::Key| {
        d.frame(app, vec![Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], Modifiers::NONE);
        d.frame(app, vec![Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }], Modifiers::NONE);
    };
    let press = |d: &mut Driver, app: &mut RetroApp, p: Pos2| {
        d.frame(app, vec![Event::PointerMoved(p)], Modifiers::NONE);
        d.frame(app, vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }], Modifiers::NONE);
    };
    let release = |d: &mut Driver, app: &mut RetroApp, p: Pos2| {
        d.frame(app, vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }], Modifiers::NONE);
        d.idle(app, 2);
    };
    app.view.layout = Layout::Only3D;
    app.tool = Tool::Paint;
    app.stencil.enabled = false;
    app.show_view_settings = false;
    app.show_history = false;
    app.show_log = false;
    app.run_cmd(Cmd::AddLayer(LayerKind::Paint));
    d.idle(app, 3);

    // A press on the 3D view's display menu belongs to the menu, not to the brush.
    let menu = marked(&d.ctx, "display3d").ok_or_else(|| anyhow::anyhow!("display menu not drawn"))?.center();
    press(d, app, menu);
    anyhow::ensure!(!app.vp_press_ok, "a press on the display menu reached the 3D view");
    release(d, app, menu);
    key(d, app, egui::Key::Escape);
    let c = app.vp_rect.center();
    press(d, app, c);
    anyhow::ensure!(app.vp_press_ok, "a press on the 3D view was refused");
    release(d, app, c);

    // The click that closes a menu does not paint.
    let file = marked(&d.ctx, "menu:File").ok_or_else(|| anyhow::anyhow!("File menu not drawn"))?.center();
    d.click(app, file, Modifiers::NONE);
    d.idle(app, 1);
    anyhow::ensure!(menu_open(&d.ctx), "the File menu did not open");
    let before = app.history.version;
    d.click(app, c, Modifiers::NONE);
    d.idle(app, 2);
    anyhow::ensure!(app.history.version == before, "the click closing the File menu painted");
    anyhow::ensure!(!menu_open(&d.ctx), "the File menu stayed open");

    // Escape closes an open menu before the display settings flyout.
    app.show_view_settings = true;
    d.idle(app, 1);
    d.click(app, file, Modifiers::NONE);
    d.idle(app, 1);
    key(d, app, egui::Key::Escape);
    d.idle(app, 1);
    anyhow::ensure!(app.show_view_settings && !menu_open(&d.ctx), "Escape closed the flyout instead of the menu");
    key(d, app, egui::Key::Escape);
    anyhow::ensure!(!app.show_view_settings, "Escape did not close the flyout");

    // Selecting a layer is not an undo step and keeps Redo.
    d.drag(app, &line(c - egui::vec2(30.0, 0.0), c + egui::vec2(30.0, 0.0), 6), Modifiers::NONE);
    d.idle(app, 1);
    app.undo();
    d.idle(app, 1);
    let (undo_n, redo_n) = (app.history.undo.len(), app.history.redo.len());
    anyhow::ensure!(redo_n > 0, "nothing to redo after an undo");
    let row = marked(&d.ctx, "layer_top").ok_or_else(|| anyhow::anyhow!("top layer row not drawn"))?;
    let name = Pos2::new(row.min.x + 100.0, row.center().y);
    let second = row.translate(egui::vec2(0.0, row.height() + 1.0));
    d.click(app, Pos2::new(second.min.x + 100.0, second.center().y), Modifiers::NONE);
    d.click(app, name, Modifiers::NONE);
    d.idle(app, 2);
    anyhow::ensure!(
        app.history.undo.len() == undo_n && app.history.redo.len() == redo_n,
        "clicking layer rows changed the history ({} / {} entries, was {undo_n} / {redo_n})",
        app.history.undo.len(),
        app.history.redo.len()
    );

    // Double click renames; a click elsewhere ends the rename. (Wait first:
    // right after the selection click it would count as a triple click.)
    d.idle(app, 40);
    d.click(app, name, Modifiers::NONE);
    d.click(app, name, Modifiers::NONE);
    d.idle(app, 2);
    let rid = egui::Id::new("layer rename");
    anyhow::ensure!(d.ctx.data(|m| m.get_temp::<(u64, String)>(rid)).is_some(), "double click did not start the rename");
    let empty = marked(&d.ctx, "layers_drop").ok_or_else(|| anyhow::anyhow!("layer stack empty area not drawn"))?.center();
    d.click(app, empty, Modifiers::NONE);
    d.idle(app, 2);
    anyhow::ensure!(d.ctx.data(|m| m.get_temp::<(u64, String)>(rid)).is_none(), "the rename field stayed open after a click elsewhere");
    anyhow::ensure!(!d.ctx.wants_keyboard_input(), "the rename field kept the keyboard");

    // The right strip icon brings the Texture Set Settings tab to the front,
    // a second click closes it, a third shows it again.
    app.dock = None;
    d.idle(app, 2);
    let icon = marked(&d.ctx, "dockbar:setsettings").ok_or_else(|| anyhow::anyhow!("set settings icon not drawn"))?.center();
    anyhow::ensure!(app.tab_open(dock::Tab::SetSettings) && !app.tab_visible(dock::Tab::SetSettings), "default layout: settings tab should be behind Layers");
    d.click(app, icon, Modifiers::NONE);
    d.idle(app, 1);
    anyhow::ensure!(app.tab_visible(dock::Tab::SetSettings), "the icon did not bring the hidden tab to the front");
    d.click(app, icon, Modifiers::NONE);
    d.idle(app, 1);
    anyhow::ensure!(!app.tab_open(dock::Tab::SetSettings), "a second click did not close the tab");
    d.click(app, icon, Modifiers::NONE);
    d.idle(app, 1);
    anyhow::ensure!(app.tab_visible(dock::Tab::SetSettings), "a third click did not show the tab again");
    app.dock = None;
    app.view.layout = Layout::Both;
    d.idle(app, 2);
    println!("interactions: display menu, menu dismiss, Escape order, selection history, rename focus, dock icons OK");
    multi_select_check(d, app)?;
    gizmo_check(d, app)?;
    color_field_check(d, app)?;
    eraser_clone_split_check(d, app)?;
    Ok(())
}

/// The eraser keeps a set opaque (it used to switch Auto alpha to blending
/// and allocate emissive / normal / opacity); V shows the clone source; the
/// 2D / 3D divider drags.
fn eraser_clone_split_check(d: &mut Driver, app: &mut RetroApp) -> anyhow::Result<()> {
    app.view.layout = Layout::Only3D;
    app.run_cmd(Cmd::AddLayer(LayerKind::Paint));
    app.tool = Tool::Paint;
    app.mat.channels = [true, true, true, false, false, false, false];
    d.idle(app, 2);
    let c = app.vp_rect.center();
    d.drag(app, &line(c - egui::vec2(60.0, 0.0), c + egui::vec2(60.0, 0.0), 8), Modifiers::NONE);
    app.tool = Tool::Erase;
    d.idle(app, 1);
    d.drag(app, &line(c - egui::vec2(40.0, 10.0), c + egui::vec2(40.0, 10.0), 8), Modifiers::NONE);
    d.idle(app, 2);
    {
        let doc = app.doc.as_ref().unwrap();
        let s = &doc.sets[doc.active];
        let l = s.selected_layer().ok_or_else(|| anyhow::anyhow!("no layer"))?;
        anyhow::ensure!(l.paint.as_ref().is_some_and(|p| p.emis.is_none()), "the eraser allocated emissive / normal / opacity textures");
        anyhow::ensure!(s.shader_alpha() == 0.0 || !matches!(s.settings.alpha_mode, crate::document::AlphaMode::Auto), "erasing switched the set to alpha blending");
    }
    // Clone source marker.
    app.tool = Tool::Clone;
    d.frame(app, vec![Event::PointerMoved(c)], Modifiers::NONE);
    d.frame(app, vec![Event::Key { key: egui::Key::V, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], Modifiers::NONE);
    d.frame(app, vec![Event::Key { key: egui::Key::V, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }], Modifiers::NONE);
    anyhow::ensure!(app.clone_world.and_then(|w| app.project_vp(w)).is_some(), "V did not place a visible clone source");
    // Divider between the 2D and 3D views.
    app.tool = Tool::Paint;
    app.view.layout = Layout::Both;
    d.idle(app, 2);
    let g = marked(&d.ctx, "view split").ok_or_else(|| anyhow::anyhow!("view divider not drawn"))?.center();
    let before = app.view.split;
    d.drag(app, &line(g, g + egui::vec2(120.0, 0.0), 6), Modifiers::NONE);
    anyhow::ensure!(app.view.split > before + 0.03, "dragging the divider did not resize the views ({before} -> {})", app.view.split);
    app.view.split = 0.5;
    println!("eraser keeps the set opaque, clone source shown, view divider drags");
    Ok(())
}

/// Color fields: the picker opens on a click and closes on a click
/// outside; the eyedropper picks the mesh color without painting.
fn color_field_check(d: &mut Driver, app: &mut RetroApp) -> anyhow::Result<()> {
    // The selected fill (from the manipulator check) shows its base color bar.
    app.view.layout = Layout::Only3D;
    app.tool = Tool::Paint;
    d.idle(app, 3);
    // On short windows the bar is further down the Properties panel: scroll.
    for _ in 0..30 {
        let r = marked(&d.ctx, "colorbar:Color").unwrap_or(Rect::NOTHING);
        let e = marked(&d.ctx, "eyedrop:Color").unwrap_or(Rect::NOTHING);
        if r.height() >= 18.0 && e.height() >= 17.0 {
            break;
        }
        let at = Pos2::new(if r.is_positive() { r.center().x } else { 400.0 }, d.size.y * 0.6);
        d.frame(app, vec![Event::PointerMoved(at)], Modifiers::NONE);
        d.frame(app, vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, -120.0), modifiers: Modifiers::NONE }], Modifiers::NONE);
        d.idle(app, 2);
    }
    let bar = marked(&d.ctx, "colorbar:Color").ok_or_else(|| anyhow::anyhow!("no color bar drawn"))?;
    d.click(app, bar.center(), Modifiers::NONE);
    d.idle(app, 2);
    anyhow::ensure!(widgets::color_picker_open(&d.ctx), "clicking the color bar did not open the picker");
    let outside = app.vp_rect.right_top() + egui::vec2(-40.0, 80.0);
    d.click(app, outside, Modifiers::NONE);
    d.idle(app, 2);
    anyhow::ensure!(!widgets::color_picker_open(&d.ctx), "a click outside did not close the color picker");
    // Eyedropper on the mesh: picks its base color, paints nothing.
    let drop = marked(&d.ctx, "eyedrop:Color").ok_or_else(|| anyhow::anyhow!("no eyedropper drawn"))?.center();
    d.click(app, drop, Modifiers::NONE);
    d.idle(app, 1);
    anyhow::ensure!(d.ctx.data(|m| m.get_temp::<egui::Id>(widgets::eyedropper_id())).is_some(), "the eyedropper did not arm");
    let c = app.vp_rect.center();
    d.frame(app, vec![Event::PointerMoved(c)], Modifiers::NONE);
    d.idle(app, 1);
    anyhow::ensure!(app.eyedrop_live.is_some(), "no live color under the cursor while armed");
    let before = app.history.undo.len();
    d.click(app, c, Modifiers::NONE);
    d.idle(app, 2);
    anyhow::ensure!(d.ctx.data(|m| m.get_temp::<egui::Id>(widgets::eyedropper_id())).is_none(), "the pick did not disarm the eyedropper");
    anyhow::ensure!(app.stroke.is_none(), "the eyedropper click painted");
    anyhow::ensure!(app.history.undo.len() <= before + 1, "the eyedropper click added a stroke");
    app.view.layout = Layout::Both;
    println!("color fields: picker closes on an outside click, eyedropper picks the mesh color");
    Ok(())
}

/// Ctrl + click two layers, drag them onto a folder row: both go inside.
fn multi_select_check(d: &mut Driver, app: &mut RetroApp) -> anyhow::Result<()> {
    // A fresh stack: [paint A, paint B, folder] bottom to top.
    let (a, b, folder) = {
        let doc = app.doc.as_mut().unwrap();
        let set = doc.active;
        doc.sets[set].layers.clear();
        doc.sets[set].selected = None;
        let a = doc.add_layer(&app.engine, set, LayerKind::Paint);
        let b = doc.add_layer(&app.engine, set, LayerKind::Paint);
        doc.sets[set].selected = None;
        let folder = doc.add_layer(&app.engine, set, LayerKind::Folder);
        (a, b, folder)
    };
    app.history.clear();
    app.dock = None;
    d.idle(app, 3);
    let top = marked(&d.ctx, "layer_top").ok_or_else(|| anyhow::anyhow!("top layer row not drawn"))?;
    let h = top.height() + 1.0;
    let row = |k: f32| Pos2::new(top.min.x + 110.0, top.center().y + k * h);
    // Rows top to bottom: folder, B, A.
    d.click(app, row(1.0), Modifiers::NONE);
    d.idle(app, 30);
    d.click(app, row(2.0), Modifiers::COMMAND);
    d.idle(app, 2);
    let sel = app.doc.as_ref().map(|doc| doc.sets[doc.active].selection()).unwrap_or_default();
    anyhow::ensure!(sel.len() == 2 && sel.contains(&a) && sel.contains(&b), "Ctrl + click did not select both layers ({sel:?})");
    // Drag B (with A) onto the middle of the folder row.
    d.drag(app, &line(row(1.0), row(0.0), 12), Modifiers::NONE);
    d.idle(app, 3);
    let doc = app.doc.as_ref().unwrap();
    let s = &doc.sets[doc.active];
    let inside = crate::document::find(&s.layers, folder).map(|f| f.children.iter().map(|c| c.desc.id).collect::<Vec<_>>()).unwrap_or_default();
    anyhow::ensure!(inside == vec![a, b], "dragging the selection onto the folder gave children {inside:?} (stack {} roots)", s.layers.len());
    // Ctrl + G on a multi-selection makes one folder of both; Delete removes both.
    app.undo();
    d.idle(app, 2);
    app.run_cmd(Cmd::Select(a, Target::Content));
    app.run_cmd(Cmd::SelectToggle(b));
    app.run_cmd(Cmd::Group);
    d.idle(app, 1);
    let doc = app.doc.as_ref().unwrap();
    let s = &doc.sets[doc.active];
    anyhow::ensure!(s.layers.len() == 2 && s.layers.iter().any(|l| l.children.len() == 2), "Ctrl + G did not group the selection");
    app.undo();
    app.run_cmd(Cmd::Select(a, Target::Content));
    app.run_cmd(Cmd::SelectToggle(b));
    app.run_cmd(Cmd::Delete);
    let doc = app.doc.as_ref().unwrap();
    anyhow::ensure!(doc.sets[doc.active].layers.len() == 1, "Delete did not remove both selected layers");
    app.undo();
    d.idle(app, 2);
    println!("multi-selection: Ctrl + click, drag into a folder, group, delete OK");
    Ok(())
}

/// A tri-planar fill shows the manipulator; dragging its X arrow moves
/// the projection.
fn gizmo_check(d: &mut Driver, app: &mut RetroApp) -> anyhow::Result<()> {
    let mut f = FillDesc::default();
    f.projection.mode = crate::document::ProjMode::Triplanar;
    app.run_cmd(Cmd::AddLayer(LayerKind::Fill(Box::new(f))));
    app.view.layout = Layout::Only3D;
    app.gizmo_mode = gizmo::GizmoMode::Move;
    d.idle(app, 3);
    let (pivot, axis) = {
        let doc = app.doc.as_ref().unwrap();
        let s = &doc.sets[doc.active];
        let Some(LayerKind::Fill(f)) = s.selected_layer().map(|l| &l.desc.kind) else { anyhow::bail!("the new fill is not selected") };
        let (p, q, _) = f.projection.frame(&s.world);
        (p, q * glam::Vec3::X)
    };
    let r = app.vp_rect;
    let vp = app.camera.view_proj(r.width() / r.height());
    let to_screen = |w: glam::Vec3| {
        let c = vp * w.extend(1.0);
        let n = c.truncate() / c.w;
        Pos2::new(r.min.x + (n.x * 0.5 + 0.5) * r.width(), r.min.y + (0.5 - n.y * 0.5) * r.height())
    };
    let len = (app.camera.eye() - pivot).length() * 0.16;
    let (ps, tip) = (to_screen(pivot), to_screen(pivot + axis * len));
    let grab = ps + (tip - ps) * 0.6;
    // Edits less than 0.8 s apart merge into one undo step; this test is faster than a user.
    app.last_edit = Instant::now() - std::time::Duration::from_secs(5);
    let before = app.history.version;
    d.drag(app, &line(grab, grab + (tip - ps) * 0.8, 10), Modifiers::NONE);
    d.idle(app, 2);
    let doc = app.doc.as_ref().unwrap();
    let s = &doc.sets[doc.active];
    let Some(LayerKind::Fill(f)) = s.selected_layer().map(|l| &l.desc.kind) else { anyhow::bail!("fill lost") };
    let moved = glam::Vec3::from(f.projection.position);
    anyhow::ensure!(moved.dot(axis) > len * 0.3, "dragging the X arrow moved the projection by {moved:?}");
    anyhow::ensure!(app.stroke.is_none(), "the gizmo drag started a stroke");
    anyhow::ensure!(app.history.version > before, "the gizmo drag is not in the history (version {} -> {}, last {:?})", before, app.history.version, app.history.undo.last().map(|e| e.label().to_string()));
    app.view.layout = Layout::Both;
    println!("projection manipulator: arrow drag moved the projection by {:.3}", moved.length());
    Ok(())
}
