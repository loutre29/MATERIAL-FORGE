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
    app.lock_cursor = false;
    let size = std::env::var("MF_UITEST_SIZE")
        .ok()
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some(egui::vec2(w.parse().ok()?, h.parse().ok()?))))
        .unwrap_or(egui::vec2(1600.0, 950.0));
    let mut d = Driver { ctx, size, frames: 0 };
    d.idle(&mut app, 3);
    anyhow::ensure!(app.doc.is_some(), "project was not created: {}", app.status);
    println!("project created ({} frames, {:.1}s)", d.frames, t0.elapsed().as_secs_f32());
    // MF_UITEST_ONLY=path / uvgizmo: that check alone (quick iterations).
    if std::env::var("MF_UITEST_ONLY").is_ok_and(|v| v == "path") {
        path_check(&mut d, &mut app)?;
        println!("UITEST OK (path only): {} frames in {:.1}s", d.frames, t0.elapsed().as_secs_f32());
        return Ok(());
    }
    if std::env::var("MF_UITEST_ONLY").is_ok_and(|v| v == "uvgizmo") {
        uvgizmo_check(&mut d, &mut app)?;
        println!("UITEST OK (uvgizmo only): {} frames in {:.1}s", d.frames, t0.elapsed().as_secs_f32());
        return Ok(());
    }


    // Every window.
    app.show_view_settings = true;
    app.show_set_settings = true;
    app.show_history = true;
    app.show_log = true;
    app.show_prefs = true;
    app.open_export();
    let sets = app.doc.as_ref().unwrap().sets.len();
    app.bake_dialog = Some(BakeDialog { settings: Default::default(), high: String::new(), sets: vec![true; sets] });
    app.new_dialog = Some(NewProjectDialog { path: String::new(), res: 1024, error: None, auto_uv: Default::default() });
    d.idle(&mut app, 3);
    app.new_dialog = None;
    app.bake_dialog = None;
    app.export_dialog = None;
    app.show_prefs = false;
    if app.tab_open(dock::Tab::SetSettings) {
        app.toggle_tab(dock::Tab::SetSettings);
    }

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
    // The inspector's Properties panel (in front) scrolls to the slot.
    app.focus_tab(dock::Tab::Properties);
    d.idle(&mut app, 2);
    for _ in 0..40 {
        if marked(&d.ctx, "slot:Base color").is_some_and(|r| r.min.y > 0.0 && r.max.y < d.size.y - 12.0) {
            break;
        }
        let at = marked(&d.ctx, "slot:Material mode").map(|r| r.center()).unwrap_or(Pos2::new(d.size.x - 150.0, d.size.y * 0.75));
        let at = Pos2::new(at.x, at.y.clamp(40.0, d.size.y - 40.0));
        d.frame(&mut app, vec![Event::PointerMoved(at)], Modifiers::NONE);
        d.frame(&mut app, vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, -120.0), modifiers: Modifiers::NONE }], Modifiers::NONE);
        d.idle(&mut app, 2);
    }
    // The scroll is animated: let it settle before aiming.
    d.idle(&mut app, 20);
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
    use super::selection::SelShape;
    for (k, m, t) in [
        (egui::Key::E, Modifiers::NONE, Tool::Erase),
        (egui::Key::U, Modifiers::NONE, Tool::PolygonFill),
        (egui::Key::R, Modifiers::NONE, Tool::Smudge),
        (egui::Key::S, Modifiers::NONE, Tool::Clone),
        (egui::Key::H, Modifiers::NONE, Tool::Hide),
        (egui::Key::M, Modifiers::NONE, Tool::Select(SelShape::Rect)),
        (egui::Key::M, Modifiers::SHIFT, Tool::Select(SelShape::Ellipse)),
        (egui::Key::L, Modifiers::NONE, Tool::Select(SelShape::Lasso)),
        (egui::Key::L, Modifiers::SHIFT, Tool::Select(SelShape::PolyLasso)),
        (egui::Key::M, Modifiers::NONE, Tool::Select(SelShape::Ellipse)),
        (egui::Key::B, Modifiers::NONE, Tool::Paint),
    ] {
        key(&mut d, &mut app, k, m);
        anyhow::ensure!(app.tool == t, "key {k:?} selected {:?}", app.tool);
    }
    key(&mut d, &mut app, egui::Key::I, Modifiers::NONE);
    anyhow::ensure!(app.tool == Tool::Picker && app.prev_tool == Some(Tool::Paint), "I is not a temporary picker");
    key(&mut d, &mut app, egui::Key::B, Modifiers::NONE);
    key(&mut d, &mut app, egui::Key::C, Modifiers::NONE);
    anyhow::ensure!(app.view.display == Display::BaseColor, "C did not solo the first channel");
    key(&mut d, &mut app, egui::Key::N, Modifiers::NONE);
    anyhow::ensure!(app.view.display == Display::MapAo, "N did not show the first mesh map");
    key(&mut d, &mut app, egui::Key::C, Modifiers::NONE);
    anyhow::ensure!(app.view.display == Display::Material, "C did not come back to the material view");
    key(&mut d, &mut app, egui::Key::Num5, Modifiers::NONE);
    anyhow::ensure!((app.brush.flow - 0.5).abs() < 1e-4, "5 did not set the flow to 50 %");
    key(&mut d, &mut app, egui::Key::Num0, Modifiers::NONE);
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
            // The settings window floats over the view: closed again.
            app.toggle_tab(dock::Tab::SetSettings);
            app.focus_tab(dock::Tab::Layers);
            app.focus_tab(dock::Tab::Properties);
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
    app.lock_cursor = false;
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
    // Label | track | value on one line: the track starts after the label column.
    let lw = super::widgets::slider_label_width(r.width());
    let (a, b) = (Pos2::new(r.min.x + lw + 20.0, r.center().y), Pos2::new(r.min.x + lw + 160.0, r.center().y));
    run(vec![Event::PointerMoved(a)], &mut vals, &mut rects);
    run(vec![press(a, true)], &mut vals, &mut rects);
    run(vec![Event::PointerMoved(b)], &mut vals, &mut rects);
    run(vec![press(b, false)], &mut vals, &mut rects);
    anyhow::ensure!((vals[0] - 1.0).abs() > 0.5 && vals[1] == 1.0, "dragging the first \"Scale\" slider gave {vals:?}");
    // Click the second number, type 7, Enter.
    let r = rects[1];
    let v = Pos2::new(r.max.x - 26.0, r.center().y);
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
    let file = marked(&d.ctx, "menu:App").ok_or_else(|| anyhow::anyhow!("app menu not drawn"))?.center();
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

    // The gear of the layers panel opens the texture set settings window
    // (closed by default), a second click closes it, a third shows it again.
    app.dock = None;
    d.idle(app, 2);
    let icon = marked(&d.ctx, "setsettings:gear").ok_or_else(|| anyhow::anyhow!("set settings gear not drawn"))?.center();
    anyhow::ensure!(!app.tab_open(dock::Tab::SetSettings), "default layout: the settings tab opens on demand");
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
    // Panels: the edge resizes, the header moves, a double click puts it
    // back; a hidden left panel comes back from the corner menu.
    app.dock = None;
    d.idle(app, 2);
    let edge = marked(&d.ctx, "panel right:left").ok_or_else(|| anyhow::anyhow!("left panel edge not drawn"))?.center();
    d.drag(app, &line(edge, edge + egui::vec2(60.0, 0.0), 6), Modifiers::NONE);
    d.idle(app, 1);
    let lw = app.dock.clone().unwrap_or_default().left_w;
    anyhow::ensure!(lw > dock::LEFT_W + 40.0, "dragging the left panel edge did not widen it ({lw})");
    // The bottom right corner of the library: it floats and grows both ways.
    let corner = marked(&d.ctx, "panel bottom right:library").ok_or_else(|| anyhow::anyhow!("library corner not drawn"))?.center();
    d.drag(app, &line(corner, corner + egui::vec2(-30.0, 50.0), 6), Modifiers::NONE);
    d.idle(app, 1);
    let r = app.dock.clone().unwrap_or_default().rects[1].ok_or_else(|| anyhow::anyhow!("resizing the library corner did not float it"))?;
    let lib0 = marked(&d.ctx, "panel header:library").unwrap();
    anyhow::ensure!(r[3] - r[1] > 100.0 && (r[2] - r[0]) < dock::RIGHT_W, "the corner did not resize the library ({r:?}, header {lib0:?})");
    let head = marked(&d.ctx, "panel header:library").ok_or_else(|| anyhow::anyhow!("library header not drawn"))?;
    let h = Pos2::new(head.min.x + 60.0, head.center().y);
    d.drag(app, &line(h, h + egui::vec2(-80.0, 40.0), 6), Modifiers::NONE);
    d.idle(app, 1);
    let r2 = app.dock.clone().unwrap_or_default().rects[1].unwrap();
    anyhow::ensure!(r2[0] < r[0] - 60.0 && r2[1] > r[1] + 20.0, "dragging the library header did not move it ({r:?} -> {r2:?})");
    // (Wait first: right after the drag it would count as a triple click.)
    d.idle(app, 40);
    let head = marked(&d.ctx, "panel header:library").ok_or_else(|| anyhow::anyhow!("library header not drawn"))?;
    let h = Pos2::new(head.min.x + 60.0, head.center().y);
    d.frame(app, vec![Event::PointerMoved(h)], Modifiers::NONE);
    for _ in 0..2 {
        d.frame(app, vec![Event::PointerButton { pos: h, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }], Modifiers::NONE);
        d.frame(app, vec![Event::PointerButton { pos: h, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }], Modifiers::NONE);
    }
    d.idle(app, 1);
    anyhow::ensure!(app.dock.clone().unwrap_or_default().rects[1].is_none(), "a double click did not dock the library back");
    // Locked: the edge no longer resizes.
    app.dock.get_or_insert_with(Default::default).locked = true;
    d.idle(app, 2);
    let lw0 = app.dock.clone().unwrap_or_default().left_w;
    d.drag(app, &line(edge, edge + egui::vec2(60.0, 0.0), 6), Modifiers::NONE);
    d.idle(app, 1);
    anyhow::ensure!(app.dock.clone().unwrap_or_default().left_w == lw0, "a locked panel resized");
    app.dock = None;
    d.idle(app, 2);
    app.toggle_tab(dock::Tab::Layers);
    d.idle(app, 2);
    anyhow::ensure!(marked(&d.ctx, "menu:Home").is_some() && !app.tab_open(dock::Tab::Layers), "no menu in the corner while the left panel is hidden");
    app.focus_tab(dock::Tab::Layers);
    app.dock = None;
    d.idle(app, 2);
    println!("interactions: display menu, menu dismiss, Escape order, selection history, rename focus, dock icons OK");
    multi_select_check(d, app)?;
    gizmo_check(d, app)?;
    color_field_check(d, app)?;
    eraser_clone_split_check(d, app)?;
    path_check(d, app)?;
    uvgizmo_check(d, app)?;
    auto_uv_check(d, app)?;
    selection_check(d, app)?;
    Ok(())
}

fn count_layers(app: &RetroApp) -> usize {
    let mut n = 0;
    crate::document::visit(&app.doc.as_ref().unwrap().sets[app.doc.as_ref().unwrap().active].layers, &mut |_| n += 1);
    n
}

/// Selection tools: a rectangle in the 3D view clips the brush, Ctrl + D
/// / undo / invert, a polygonal lasso in the 2D view (clicks + Enter),
/// Escape cancels, Shift + Backspace fills.
fn selection_check(d: &mut Driver, app: &mut RetroApp) -> anyhow::Result<()> {
    use super::selection::SelShape;
    let key = |d: &mut Driver, app: &mut RetroApp, k: egui::Key, m: Modifiers| {
        d.frame(app, vec![Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m }], m);
        d.frame(app, vec![Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m }], m);
    };
    let label = |app: &RetroApp| app.history.undo.last().map(|e| e.label().to_string()).unwrap_or_default();
    app.view.layout = Layout::Only3D;
    app.run_cmd(Cmd::AddLayer(LayerKind::Paint));
    app.mat.channels = [true, true, true, false, false, false, false];
    app.tool = Tool::Select(SelShape::Rect);
    d.idle(app, 2);
    let c = app.vp_rect.center();
    let (a, b) = (c + egui::vec2(-160.0, -140.0), c + egui::vec2(0.0, 140.0));
    d.drag(app, &line(a, b, 8), Modifiers::NONE);
    d.idle(app, 2);
    anyhow::ensure!(app.has_selection() && label(app) == "Rectangle select", "the rectangle did not select ({})", label(app));
    // A stroke across the whole view paints only inside the selection.
    app.tool = Tool::Paint;
    d.idle(app, 1);
    d.drag(app, &line(c - egui::vec2(300.0, 0.0), c + egui::vec2(300.0, 0.0), 30), Modifiers::NONE);
    d.idle(app, 3);
    {
        let doc = app.doc.as_ref().unwrap();
        let s = &doc.sets[doc.active];
        let sel = s.selection.as_ref().unwrap().read_bytes(&app.engine.device, &app.engine.queue);
        let l = s.selected_layer().ok_or_else(|| anyhow::anyhow!("no layer"))?;
        let col = crate::engine::texture::f16_bytes_to_f32(&l.paint.as_ref().unwrap().color.read_bytes(&app.engine.device, &app.engine.queue));
        let (mut inside, mut outside) = (0usize, 0usize);
        for (i, px) in col.chunks_exact(4).enumerate() {
            if px[3] > 0.02 {
                if sel[i] > 0 {
                    inside += 1;
                } else {
                    outside += 1;
                }
            }
        }
        anyhow::ensure!(inside > 100 && outside == 0, "the brush ignored the selection ({inside} texels inside, {outside} outside)");
        println!("selection: rectangle clips the brush ({inside} texels painted inside, 0 outside)");
    }
    // Ctrl + D deselects, undo brings it back, Ctrl + Shift + I inverts.
    key(d, app, egui::Key::D, Modifiers::COMMAND);
    anyhow::ensure!(!app.has_selection(), "Ctrl + D did not deselect");
    key(d, app, egui::Key::Z, Modifiers::COMMAND);
    anyhow::ensure!(app.has_selection(), "undo did not restore the selection");
    key(d, app, egui::Key::I, Modifiers::COMMAND | Modifiers::SHIFT);
    anyhow::ensure!(label(app) == "Invert selection", "Ctrl + Shift + I did not invert ({})", label(app));
    // Polygonal lasso in the 2D view: three clicks and Enter; Escape cancels.
    app.view.layout = Layout::Both;
    // L comes back to the last lasso used; Shift + L switches.
    key(d, app, egui::Key::L, Modifiers::NONE);
    if app.tool != Tool::Select(SelShape::PolyLasso) {
        key(d, app, egui::Key::L, Modifiers::SHIFT);
    }
    anyhow::ensure!(app.tool == Tool::Select(SelShape::PolyLasso), "Shift + L did not pick the polygonal lasso ({:?})", app.tool);
    d.idle(app, 2);
    let u = app.uv_rect.intersect(app.canvas_mid).center();
    for p in [u + egui::vec2(-80.0, -60.0), u + egui::vec2(90.0, -40.0), u + egui::vec2(10.0, 90.0)] {
        d.click(app, p, Modifiers::NONE);
        d.idle(app, 1);
    }
    anyhow::ensure!(app.sel_drag.as_ref().is_some_and(|s| s.uv && s.path.len() == 3), "the polygonal lasso did not record 3 points");
    key(d, app, egui::Key::Enter, Modifiers::NONE);
    anyhow::ensure!(app.sel_drag.is_none() && label(app) == "Polygonal lasso", "Enter did not close the polygonal lasso ({})", label(app));
    d.click(app, u, Modifiers::NONE);
    d.idle(app, 1);
    anyhow::ensure!(app.sel_drag.is_some(), "a click did not start a new lasso");
    key(d, app, egui::Key::Escape, Modifiers::NONE);
    anyhow::ensure!(app.sel_drag.is_none(), "Escape did not cancel the lasso");
    // Shift + Backspace fills the selection on the layer.
    key(d, app, egui::Key::Backspace, Modifiers::SHIFT);
    anyhow::ensure!(label(app) == "Fill selection", "Shift + Backspace did not fill ({})", label(app));
    key(d, app, egui::Key::D, Modifiers::COMMAND);
    // Magic wand (W) and paint bucket (G) in the 3D view.
    app.view.layout = Layout::Only3D;
    key(d, app, egui::Key::W, Modifiers::NONE);
    anyhow::ensure!(app.tool == Tool::Wand, "W did not pick the magic wand");
    d.idle(app, 2);
    let on_mesh = {
        let r = app.vp_rect;
        let mut found = None;
        'outer: for fy in [0.5f32, 0.4, 0.6, 0.3, 0.7] {
            for fx in [0.3f32, 0.35, 0.4, 0.25, 0.45, 0.6, 0.65, 0.7] {
                let p = Pos2::new(r.min.x + r.width() * fx, r.min.y + r.height() * fy);
                if app.pick_at(Vec2::new(p.x - r.min.x, p.y - r.min.y)).is_some() {
                    found = Some(p);
                    break 'outer;
                }
            }
        }
        found.ok_or_else(|| anyhow::anyhow!("no mesh under the probe points"))?
    };
    d.click(app, on_mesh, Modifiers::NONE);
    d.idle(app, 2);
    anyhow::ensure!(app.has_selection() && label(app) == "Magic wand", "the magic wand did not select ({})", label(app));
    key(d, app, egui::Key::D, Modifiers::COMMAND);
    app.run_cmd(Cmd::AddLayer(LayerKind::Paint));
    key(d, app, egui::Key::G, Modifiers::NONE);
    anyhow::ensure!(app.tool == Tool::Bucket, "G did not pick the paint bucket");
    d.click(app, on_mesh, Modifiers::NONE);
    d.idle(app, 2);
    anyhow::ensure!(label(app) == "Paint bucket", "the paint bucket did not fill ({})", label(app));
    {
        let doc = app.doc.as_ref().unwrap();
        let s = &doc.sets[doc.active];
        let l = s.selected_layer().ok_or_else(|| anyhow::anyhow!("no layer"))?;
        let col = crate::engine::texture::f16_bytes_to_f32(&l.paint.as_ref().unwrap().color.read_bytes(&app.engine.device, &app.engine.queue));
        let painted = col.chunks_exact(4).filter(|p| p[3] > 0.5).count();
        anyhow::ensure!(painted > 1000, "the paint bucket painted {painted} texels");
        println!("selection: magic wand selects, paint bucket filled {painted} texels");
    }
    // Selection brush (Q): a stroke selects, X then a stroke subtracts.
    key(d, app, egui::Key::D, Modifiers::COMMAND);
    key(d, app, egui::Key::Q, Modifiers::NONE);
    anyhow::ensure!(app.tool == Tool::SelBrush, "Q did not pick the selection brush");
    let c = app.vp_rect.center();
    let sel_count = |app: &RetroApp| {
        let doc = app.doc.as_ref().unwrap();
        doc.sets[doc.active].selection.as_ref().map_or(0, |t| t.read_bytes(&app.engine.device, &app.engine.queue).iter().filter(|b| **b > 128).count())
    };
    d.drag(app, &line(c - egui::vec2(200.0, 0.0), c + egui::vec2(200.0, 0.0), 20), Modifiers::NONE);
    d.idle(app, 3);
    let painted = sel_count(app);
    anyhow::ensure!(painted > 100 && label(app) == "Selection brush", "the selection brush selected {painted} texels ({})", label(app));
    key(d, app, egui::Key::X, Modifiers::NONE);
    d.drag(app, &line(c - egui::vec2(60.0, 0.0), c + egui::vec2(60.0, 0.0), 8), Modifiers::NONE);
    d.idle(app, 3);
    let left = sel_count(app);
    anyhow::ensure!(left < painted && label(app) == "Selection brush (subtract)", "subtracting with the selection brush left {left} of {painted}");
    key(d, app, egui::Key::X, Modifiers::NONE);
    println!("selection: selection brush paints {painted} texels, subtracts down to {left}");
    // Select > By mesh map: a synthetic curvature map (left half convex).
    {
        let doc = app.doc.as_mut().unwrap();
        let a = doc.active;
        let s = &mut doc.sets[a];
        s.gpu.alloc_baked(&app.engine.device, Some(&app.engine.queue));
        let res = s.resolution();
        let v: Vec<f32> = (0..res * res).map(|i| if i % res < res / 2 { 0.9 } else { 0.5 }).collect();
        s.gpu.baked_curvature.write_bytes(&app.engine.queue, &crate::engine::texture::f32_to_f16_bytes(&v));
        s.baked = true;
    }
    key(d, app, egui::Key::D, Modifiers::COMMAND);
    app.select_by_map(super::selection::MapSel::Edges, 0.58);
    {
        let doc = app.doc.as_ref().unwrap();
        let s = &doc.sets[doc.active];
        let res = s.resolution();
        let b = s.selection.as_ref().ok_or_else(|| anyhow::anyhow!("by mesh map selected nothing"))?.read_bytes(&app.engine.device, &app.engine.queue);
        let wrong = b.iter().enumerate().filter(|(i, v)| s.raster.tri[*i] >= 0 && ((*i as u32 % res) < res / 2) != (**v > 128)).count();
        anyhow::ensure!(wrong == 0, "select by mesh map took {wrong} wrong texels");
        println!("selection: by mesh map (edges) selects the convex half");
    }
    // Polygon fill in Select mode: a rectangle makes a selection of faces;
    // Select > Modify grows it.
    key(d, app, egui::Key::D, Modifiers::COMMAND);
    app.tool = Tool::PolygonFill;
    app.poly_to_selection = true;
    app.poly_select = super::view3d::PolySelect::Marquee;
    d.idle(app, 1);
    let c = app.vp_rect.center();
    d.drag(app, &line(c - egui::vec2(200.0, 150.0), c + egui::vec2(50.0, 150.0), 8), Modifiers::NONE);
    d.idle(app, 2);
    anyhow::ensure!(app.has_selection() && label(app) == "Select faces", "polygon fill did not select faces ({})", label(app));
    app.poly_to_selection = false;
    app.modify_selection(crate::flood::Modify::Grow, 4.0);
    anyhow::ensure!(label(app) == "Grow selection (4 px)", "grow did not apply ({})", label(app));
    // A fill layer masked by the selection.
    let n_before = count_layers(app);
    app.fill_layer_from_selection();
    d.idle(app, 2);
    {
        let doc = app.doc.as_ref().unwrap();
        let s = &doc.sets[doc.active];
        let l = s.selected_layer().ok_or_else(|| anyhow::anyhow!("no layer"))?;
        anyhow::ensure!(count_layers(app) == n_before + 1 && matches!(l.desc.kind, LayerKind::Fill(_)), "no fill layer was added");
        let eff = l.desc.mask.as_ref().and_then(|m| m.effects.last()).ok_or_else(|| anyhow::anyhow!("the fill layer has no mask effect"))?;
        let tex = l.mask_paint.get(&eff.id).ok_or_else(|| anyhow::anyhow!("no mask paint texture"))?;
        let v = crate::engine::texture::f16_bytes_to_f32(&tex.read_bytes(&app.engine.device, &app.engine.queue));
        let on = v.chunks_exact(2).filter(|p| p[0] > 0.9).count();
        let off = v.chunks_exact(2).filter(|p| p[0] < 0.1).count();
        anyhow::ensure!(on > 100 && off > 100, "the mask does not hold the selection ({on} white, {off} black)");
        println!("selection: fill layer from the selection ({on} texels in the mask)");
    }
    key(d, app, egui::Key::D, Modifiers::COMMAND);
    // Shift + G: the gradient, a drag across the view.
    app.run_cmd(Cmd::AddLayer(LayerKind::Paint));
    key(d, app, egui::Key::G, Modifiers::NONE);
    if app.tool != Tool::Gradient {
        key(d, app, egui::Key::G, Modifiers::SHIFT);
    }
    anyhow::ensure!(app.tool == Tool::Gradient, "Shift + G did not switch to the gradient ({:?})", app.tool);
    app.brush.flow = 1.0;
    let c = app.vp_rect.center();
    d.drag(app, &line(c - egui::vec2(250.0, 0.0), c + egui::vec2(250.0, 0.0), 10), Modifiers::NONE);
    d.idle(app, 2);
    anyhow::ensure!(label(app) == "Gradient", "the gradient was not applied ({})", label(app));
    {
        let doc = app.doc.as_ref().unwrap();
        let s = &doc.sets[doc.active];
        let l = s.selected_layer().ok_or_else(|| anyhow::anyhow!("no layer"))?;
        let col = crate::engine::texture::f16_bytes_to_f32(&l.paint.as_ref().unwrap().color.read_bytes(&app.engine.device, &app.engine.queue));
        let full = col.chunks_exact(4).filter(|p| p[3] > 0.9).count();
        let partial = col.chunks_exact(4).filter(|p| p[3] > 0.1 && p[3] < 0.9).count();
        anyhow::ensure!(full > 100 && partial > 100, "the gradient has no ramp ({full} opaque, {partial} partial texels)");
        println!("selection: gradient ramps ({full} opaque, {partial} partial texels)");
    }
    app.tool = Tool::Paint;
    app.view.layout = Layout::Only3D;
    d.idle(app, 2);
    println!("selection: deselect / undo / invert, polygonal lasso in 2D, Escape, fill OK");
    // Solo (Alt + click on an eye): only that layer shows, again restores.
    {
        let vis = |app: &RetroApp| {
            let mut v = Vec::new();
            crate::document::visit(&app.doc.as_ref().unwrap().sets[app.doc.as_ref().unwrap().active].layers, &mut |l| v.push((l.desc.id, l.desc.visible)));
            v
        };
        let before = vis(app);
        let id = app.doc.as_ref().unwrap().sets[app.doc.as_ref().unwrap().active].layers[0].desc.id;
        app.run_cmd(Cmd::Solo(id));
        let solo = vis(app);
        anyhow::ensure!(solo.iter().filter(|(_, v)| *v).count() >= 1 && solo.iter().any(|(i, v)| *i == id && *v), "solo did not keep the layer");
        anyhow::ensure!(solo.iter().filter(|(i, v)| *i != id && *v).count() < before.iter().filter(|(_, v)| *v).count(), "solo hid nothing");
        app.run_cmd(Cmd::Solo(id));
        anyhow::ensure!(vis(app) == before, "a second solo did not restore the visibility");
        println!("layers: solo and restore OK");
    }
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

/// Path tool: P, clicks make a path on a new path layer that paints itself;
/// a dragged point repaints (one undo step), the brush edits the selected
/// path, Backspace removes a point, Escape ends the path, the pen pulls out
/// handles, the layer refuses hand painting; the same in the 2D view.
/// 2D manipulator: a move takes the image, its frame and the pivot along
/// (the pivot stays the frame's middle), so a rotation turns the image
/// around its own middle, wherever it was moved.
fn uvgizmo_check(d: &mut Driver, app: &mut RetroApp) -> anyhow::Result<()> {
    use super::uvgizmo::{tex_of_uv, uv_of_frame};
    let layout = app.view.layout;
    app.view.layout = Layout::Only2D;
    app.tool = Tool::Paint;
    app.run_cmd(Cmd::AddLayer(LayerKind::Fill(Box::default())));
    d.idle(app, 3);
    let (_, p0) = app.uv_gizmo_target().ok_or_else(|| anyhow::anyhow!("no UV-projected fill selected"))?;
    // Alt + left drag turns the view (a quarter turn here); the rest of the
    // test then runs on the turned view.
    // It turns around the middle of the part the panels leave visible.
    let r = app.uv_rect;
    let c = app.visible_part(r).center();
    let fixed = app.uv_of(Vec2::new(c.x - r.min.x, c.y - r.min.y));
    let arc: Vec<Pos2> = (0..=8).map(|i| c + egui::Vec2::angled(i as f32 / 8.0 * std::f32::consts::FRAC_PI_2) * 120.0).collect();
    d.drag(app, &arc, Modifiers::ALT);
    d.idle(app, 2);
    let view_deg = app.view.uv_rot.to_degrees();
    anyhow::ensure!((view_deg - 90.0).abs() < 3.0, "Alt + drag did not turn the 2D view a quarter turn ({view_deg:.1} degrees)");
    let drift = app.uv_of(Vec2::new(c.x - r.min.x, c.y - r.min.y)).distance(fixed) * app.view.uv_zoom;
    anyhow::ensure!(drift < 1.0, "the 2D view did not turn around the middle of its visible part (drift {drift:.1} px)");
    let probe = Vec2::new(0.3, 0.6);
    let back = app.uv_of(app.uv_point(probe));
    anyhow::ensure!(back.distance(probe) < 1e-4, "UV to view and back is off on a turned view ({back:?})");
    let mid = |p: &crate::document::Projection| uv_of_frame(p, Vec2::splat(0.5));
    let start = app.uv_screen(mid(&p0)) + egui::vec2(10.0, 10.0);
    d.drag(app, &line(start, start + egui::vec2(80.0, 50.0), 6), Modifiers::NONE);
    d.idle(app, 2);
    let (_, p1) = app.uv_gizmo_target().unwrap();
    let moved = mid(&p1) - mid(&p0);
    anyhow::ensure!(moved.length() > 0.01, "dragging inside the frame did not move it");
    anyhow::ensure!(mid(&p1).distance(Vec2::from(p1.pivot)) < 1e-3, "the pivot stayed behind the moved frame ({:?} vs {:?})", p1.pivot, mid(&p1));
    let probe = Vec2::new(0.3, 0.6);
    anyhow::ensure!(tex_of_uv(&p1, probe + moved).distance(tex_of_uv(&p0, probe)) < 1e-3, "the image did not move with the frame");
    // Rotate: grab outside the frame (zoomed out, so outside is in view),
    // turn around the pivot.
    let keep = app.uv_px(Vec2::from(p1.pivot));
    app.view.uv_zoom *= 0.5;
    let now = app.uv_px(Vec2::from(p1.pivot));
    for k in 0..2 {
        app.view.uv_pan[k] += keep[k] - now[k];
    }
    d.idle(app, 2);
    let piv = app.uv_screen(Vec2::from(p1.pivot));
    // Outside the frame, away from the handles.
    let out = app.uv_screen(uv_of_frame(&p1, Vec2::new(1.12, 0.3)));
    let turned = piv + egui::vec2(-(out - piv).y, (out - piv).x);
    d.drag(app, &[out, piv + ((out - piv) + (turned - piv)) * 0.5, turned], Modifiers::NONE);
    d.idle(app, 2);
    let (_, p2) = app.uv_gizmo_target().unwrap();
    anyhow::ensure!((p2.rotation - p1.rotation).abs() > 10.0, "dragging outside did not rotate ({} -> {}; from {out:?} around {piv:?} in {:?})", p1.rotation, p2.rotation, app.uv_rect);
    anyhow::ensure!(mid(&p2).distance(mid(&p1)) < 1e-3, "rotating moved the frame's middle: it does not turn around the moved image");
    // A fill in the layer's mask gets the same manipulator.
    app.run_cmd(Cmd::AddMask(false));
    app.run_cmd(Cmd::AddEffect(MaskEffectKind::Fill {
        value: 1.0,
        source: crate::document::SourceKind::Uniform,
        texture: None,
        procedural: Default::default(),
        projection: Default::default(),
        range: [0.0, 1.0],
    }));
    d.idle(app, 3);
    let (_, m0) = app.uv_gizmo_target().ok_or_else(|| anyhow::anyhow!("a mask fill gets no 2D manipulator"))?;
    let at = app.uv_screen(mid(&m0)) + egui::vec2(10.0, 10.0);
    d.drag(app, &line(at, at + egui::vec2(-60.0, 40.0), 6), Modifiers::NONE);
    d.idle(app, 2);
    let mask_proj = || {
        let doc = app.doc.as_ref()?;
        let s = &doc.sets[doc.active];
        let l = s.selected_layer()?;
        l.desc.mask.as_ref()?.effects.iter().find_map(|e| match &e.kind {
            MaskEffectKind::Fill { projection, .. } => Some(projection.clone()),
            _ => None,
        })
    };
    let m1 = mask_proj().ok_or_else(|| anyhow::anyhow!("the mask fill is gone"))?;
    anyhow::ensure!(mid(&m1).distance(mid(&m0)) > 0.01, "dragging the mask fill's frame in the 2D view did not move it");
    let (_, content) = {
        let doc = app.doc.as_ref().unwrap();
        let s = &doc.sets[doc.active];
        match &s.selected_layer().unwrap().desc.kind {
            LayerKind::Fill(f) => ((), f.projection.clone()),
            _ => anyhow::bail!("the fill layer lost its kind"),
        }
    };
    anyhow::ensure!(mid(&content).distance(mid(&p2)) < 1e-4, "moving the mask fill moved the layer's own fill");
    app.view.layout = layout;
    app.view.uv_rot = 0.0;
    app.view.uv_zoom = 0.0;
    d.idle(app, 2);
    println!("2D view turned {view_deg:.0} degrees with Alt + drag; 2D manipulator: the pivot follows a move ({:.3}), rotation turns around it, a mask fill moves too", moved.length());
    Ok(())
}

fn path_check(d: &mut Driver, app: &mut RetroApp) -> anyhow::Result<()> {
    let key = |d: &mut Driver, app: &mut RetroApp, k: egui::Key| {
        d.frame(app, vec![Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], Modifiers::NONE);
        d.frame(app, vec![Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }], Modifiers::NONE);
    };
    let painted = |app: &RetroApp| -> usize {
        let doc = app.doc.as_ref().unwrap();
        let s = &doc.sets[doc.active];
        let Some(p) = s.selected_layer().and_then(|l| l.paint.as_ref()) else { return 0 };
        let col = crate::engine::texture::f16_bytes_to_f32(&p.color.read_bytes(&app.engine.device, &app.engine.queue));
        col.chunks_exact(4).filter(|p| p[3] > 0.5).count()
    };
    app.view.layout = Layout::Only3D;
    app.run_cmd(Cmd::AddLayer(LayerKind::Paint));
    d.idle(app, 2);
    key(d, app, egui::Key::P);
    anyhow::ensure!(app.tool == Tool::Path, "P did not pick the path tool ({:?})", app.tool);
    if let Some(stitch) = crate::brush::builtin_brushes().into_iter().find(|b| b.name == "Stitch") {
        app.apply_brush_preset(&stitch);
        if crate::library::starter_dir().is_some() {
            anyhow::ensure!(matches!(app.brush.alpha, AlphaSource::Resource(_)), "the stitch preset did not load its tip");
        }
    }
    app.brush.size = 6.0;
    // A spot of the mesh with room around it (the test scene has two objects).
    let r = app.vp_rect;
    let hit = |app: &RetroApp, p: Pos2| app.pick_at(Vec2::new(p.x - r.min.x, p.y - r.min.y)).is_some();
    let mut c = None;
    'outer: for fy in [0.5f32, 0.4, 0.6] {
        for fx in [0.3f32, 0.35, 0.4, 0.25, 0.6, 0.65, 0.7] {
            let p = Pos2::new(r.min.x + r.width() * fx, r.min.y + r.height() * fy);
            if [-40.0f32, 0.0, 40.0].iter().all(|dx| [-25.0f32, 25.0].iter().all(|dy| hit(app, p + egui::vec2(*dx, *dy)))) {
                c = Some(p);
                break 'outer;
            }
        }
    }
    let c = c.ok_or_else(|| anyhow::anyhow!("no room on the mesh for the path test"))?;
    let paths = |app: &RetroApp| app.active_paths().cloned().unwrap_or_default();
    for p in [c + egui::vec2(-35.0, 10.0), c + egui::vec2(0.0, -20.0), c + egui::vec2(35.0, 10.0)] {
        d.click(app, p, Modifiers::NONE);
        d.idle(app, 1);
        anyhow::ensure!(app.pick_at(Vec2::new(p.x - app.vp_rect.min.x, p.y - app.vp_rect.min.y)).is_some(), "no mesh under the path test point ({})", app.status);
    }
    d.idle(app, 2);
    let v = paths(app);
    anyhow::ensure!(v.len() == 1 && v[0].points.len() == 3, "expected one path of 3 points: {} paths ({})", v.len(), app.status);
    let n = painted(app);
    anyhow::ensure!(n > 50, "the path painted {n} texels");
    // MF_PATH_SHOT=<png>: the layer's color (UV space), to look at the stitches.
    if let Ok(f) = std::env::var("MF_PATH_SHOT") {
        let doc = app.doc.as_ref().unwrap();
        let s = &doc.sets[doc.active];
        if let Some(p) = s.selected_layer().and_then(|l| l.paint.as_ref()) {
            let col = crate::engine::texture::f16_bytes_to_f32(&p.color.read_bytes(&app.engine.device, &app.engine.queue));
            let (w, h) = (p.color.width, p.color.height);
            let img = image::RgbaImage::from_fn(w, h, |x, y| {
                let a = col[((y * w + x) * 4 + 3) as usize].clamp(0.0, 1.0);
                let g = (a * 255.0) as u8;
                image::Rgba([g, g, g, 255])
            });
            img.save(f)?;
        }
    }
    // Non-destructive: a dragged point repaints, one undo step brings it back.
    let before = v[0].points[1].pos;
    d.drag(app, &line(c + egui::vec2(0.0, -20.0), c + egui::vec2(0.0, 10.0), 4), Modifiers::NONE);
    d.idle(app, 2);
    let v = paths(app);
    anyhow::ensure!(v[0].points.len() == 3 && v[0].points[1].pos != before, "dragging a point did not move it");
    app.undo();
    d.idle(app, 2);
    anyhow::ensure!(paths(app).first().is_some_and(|p| p.points[1].pos == before), "one undo did not bring the dragged point back");
    app.redo();
    d.idle(app, 2);
    // The brush edits the path layer: its spacing repaints the stitches.
    let n1 = painted(app);
    app.brush.spacing = (app.brush.spacing * 3.0).min(4.0);
    d.idle(app, 3);
    anyhow::ensure!(app.active_style().is_some_and(|s| s.brush.spacing == app.brush.spacing), "the brush did not edit the path layer");
    let n2 = painted(app);
    anyhow::ensure!(n2 != n1, "changing the spacing did not repaint the path ({n1} texels)");
    key(d, app, egui::Key::Backspace);
    anyhow::ensure!(paths(app)[0].points.len() == 2, "Backspace did not remove a point");
    // Escape ends the path; the next click starts another.
    key(d, app, egui::Key::Escape);
    anyhow::ensure!(app.path.path.is_none(), "Escape did not end the path");
    d.click(app, c + egui::vec2(0.0, 22.0), Modifiers::NONE);
    d.idle(app, 1);
    anyhow::ensure!(paths(app).len() == 2, "a click after Escape did not start a second path ({} paths)", paths(app).len());
    key(d, app, egui::Key::Escape);
    // Pen: click and drag pulls out handles, a click is a corner.
    app.path.pen = true;
    d.drag(app, &line(c + egui::vec2(-30.0, -22.0), c + egui::vec2(-12.0, -24.0), 4), Modifiers::NONE);
    d.click(app, c + egui::vec2(30.0, -22.0), Modifiers::NONE);
    d.idle(app, 2);
    let v = paths(app);
    let pen = v.last().unwrap();
    anyhow::ensure!(v.len() == 3 && pen.points.len() == 2, "the pen path has {} points ({} paths)", pen.points.len(), v.len());
    use crate::document::PointKind;
    anyhow::ensure!(
        pen.points[0].kind == PointKind::Smooth && pen.points[1].kind == PointKind::Broken && pen.points[1].h_out == [0.0; 3],
        "pen points: drag {:?}, click {:?}",
        pen.points[0].kind,
        pen.points[1].kind
    );
    key(d, app, egui::Key::Escape);
    app.path.pen = false;
    // A path layer is not painted by hand.
    app.tool = Tool::Paint;
    d.idle(app, 1);
    let undo = app.history.undo.len();
    d.drag(app, &line(c + egui::vec2(-20.0, 0.0), c + egui::vec2(20.0, 0.0), 6), Modifiers::NONE);
    d.idle(app, 2);
    anyhow::ensure!(app.history.undo.len() == undo, "a brush stroke painted the path layer");
    app.tool = Tool::Path;
    // 2D view.
    app.view.layout = Layout::Only2D;
    d.idle(app, 2);
    let before = painted(app);
    let u = app.uv_rect.intersect(app.canvas_mid).center();
    for p in [u + egui::vec2(-100.0, 0.0), u + egui::vec2(0.0, 60.0), u + egui::vec2(100.0, 0.0)] {
        d.click(app, p, Modifiers::NONE);
        d.idle(app, 1);
    }
    d.idle(app, 2);
    let v = paths(app);
    anyhow::ensure!(v.last().is_some_and(|p| p.uv && p.points.len() == 3), "the 2D path was not made ({} paths)", v.len());
    anyhow::ensure!(painted(app) > before + 50, "the 2D path painted nothing");
    key(d, app, egui::Key::Escape);
    // Other settings mean another path layer: the first keeps its own.
    let (first, spacing) = ({ let d = app.doc.as_ref().unwrap(); d.sets[d.active].selected }, app.brush.spacing);
    anyhow::ensure!(app.new_path_layer(), "New path layer did nothing");
    d.idle(app, 2);
    anyhow::ensure!({ let d = app.doc.as_ref().unwrap(); d.sets[d.active].selected } != first && paths(app).is_empty(), "New path layer did not select an empty path layer");
    app.brush.spacing = spacing * 0.5;
    d.idle(app, 2);
    { let d = app.doc.as_mut().unwrap(); let s = d.active; d.sets[s].selected = first; }
    d.idle(app, 2);
    anyhow::ensure!(app.brush.spacing == spacing, "the first path layer's brush changed with the new layer's ({})", app.brush.spacing);
    app.view.layout = Layout::Only3D;
    // Projection tool (Shift + B): paints the stencil without the brush's
    // stencil option.
    let tex = app.doc.as_ref().unwrap().resources.values().find(|r| r.kind == crate::document::ResKind::Texture).map(|r| r.id);
    if let Some(id) = tex {
        app.stencil.enabled = false;
        app.stencil.resource = Some(id);
        app.run_cmd(Cmd::AddLayer(LayerKind::Paint));
        app.brush = crate::brush::BrushSettings::default();
        d.frame(app, vec![Event::Key { key: egui::Key::B, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::SHIFT }], Modifiers::SHIFT);
        d.frame(app, vec![Event::Key { key: egui::Key::B, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::SHIFT }], Modifiers::SHIFT);
        d.idle(app, 2);
        anyhow::ensure!(app.tool == Tool::Projection && app.props_tab == 2, "Shift + B did not open the projection ({:?}, tab {})", app.tool, app.props_tab);
        anyhow::ensure!(app.stencil_active(), "the projection tool does not use the stencil");
        d.drag(app, &line(c - egui::vec2(30.0, 0.0), c + egui::vec2(30.0, 0.0), 8), Modifiers::NONE);
        d.idle(app, 2);
        let n = painted(app);
        anyhow::ensure!(n > 50, "the projection painted {n} texels");
        app.stencil.resource = None;
        println!("projection: Shift + B, stencil tab, {n} texels painted");
    }
    app.tool = Tool::Paint;
    d.idle(app, 2);
    println!("path: path layer painted ({n} texels), point drag + undo, brush edits the path, Backspace, Escape, pen handles, no hand painting, 2D");
    Ok(())
}

/// Auto UV on the open project: a painted spot is still painted at the
/// same place on the mesh after the UVs change; the history is cleared and
/// the bake window asks for new mesh maps.
fn auto_uv_check(d: &mut Driver, app: &mut RetroApp) -> anyhow::Result<()> {
    app.view.layout = Layout::Only3D;
    app.tool = Tool::Paint;
    app.brush = crate::brush::BrushSettings::default();
    app.brush.size = 20.0;
    app.run_cmd(Cmd::AddLayer(LayerKind::Paint));
    d.idle(app, 2);
    let r = app.vp_rect;
    let mut spot = None;
    'find: for fy in [0.5f32, 0.4, 0.6] {
        for fx in [0.3f32, 0.35, 0.4, 0.6, 0.65] {
            let p = Pos2::new(r.min.x + r.width() * fx, r.min.y + r.height() * fy);
            if app.pick_at(Vec2::new(p.x - r.min.x, p.y - r.min.y)).is_some() {
                spot = Some(p);
                break 'find;
            }
        }
    }
    let spot = spot.ok_or_else(|| anyhow::anyhow!("no mesh for the auto UV check"))?;
    d.drag(app, &line(spot - egui::vec2(4.0, 0.0), spot + egui::vec2(4.0, 0.0), 3), Modifiers::NONE);
    d.idle(app, 2);
    let local = Vec2::new(spot.x - r.min.x, spot.y - r.min.y);
    let alpha_at = |app: &RetroApp| -> anyhow::Result<(f32, [f32; 2])> {
        let hit = app.pick_at(local).ok_or_else(|| anyhow::anyhow!("the spot left the mesh"))?;
        let doc = app.doc.as_ref().unwrap();
        let s = &doc.sets[hit.set];
        let l = s.selected_layer().ok_or_else(|| anyhow::anyhow!("no layer"))?;
        let p = l.paint.as_ref().ok_or_else(|| anyhow::anyhow!("no paint"))?;
        let res = p.color.width;
        let (x, y) = (((hit.uv[0] * res as f32) as u32).min(res - 1), ((hit.uv[1] * res as f32) as u32).min(res - 1));
        Ok((p.color.read_texel_f16(&app.engine.device, &app.engine.queue, x, y)[3], hit.uv))
    };
    let (before, uv0) = alpha_at(app)?;
    anyhow::ensure!(before > 0.5, "the stroke did not paint the spot ({before})");
    // The window: open, unwrap on its thread, apply.
    app.open_uv_dialog();
    d.idle(app, 2);
    anyhow::ensure!(app.uv_dialog.is_some(), "the Auto UV window did not open");
    let mesh = app.doc.as_ref().unwrap().mesh.clone();
    let (m, reports) = crate::mesh::unwrap_mesh(&mesh, &crate::mesh::UvSettings::default())?;
    anyhow::ensure!(!reports.is_empty() && reports.iter().all(|r| r.charts > 0), "auto UV made no islands");
    app.uv_dialog = None;
    app.apply_auto_uv(m);
    d.idle(app, 3);
    let (after, uv1) = alpha_at(app)?;
    anyhow::ensure!(uv1 != uv0, "the UVs did not change ({uv0:?})");
    anyhow::ensure!(after > 0.5, "the paint did not follow the new UVs (alpha {after} at {uv1:?}, was {before} at {uv0:?})");
    anyhow::ensure!(app.history.undo.is_empty() && app.bake_dialog.is_some(), "auto UV did not clear the history / ask for a bake");
    app.bake_dialog = None;
    println!("auto UV: paint followed the new UVs ({uv0:?} -> {uv1:?}), history cleared, bake asked");
    Ok(())
}

/// Color fields: the picker opens on a click and closes on a click
/// outside; the eyedropper picks the mesh color without painting.
fn color_field_check(d: &mut Driver, app: &mut RetroApp) -> anyhow::Result<()> {
    // The selected fill (from the manipulator check) shows its base color bar.
    app.view.layout = Layout::Only3D;
    app.tool = Tool::Paint;
    app.focus_tab(dock::Tab::Properties);
    d.idle(app, 3);
    // On short windows the bar is further down the Properties panel: scroll.
    for _ in 0..30 {
        let r = marked(&d.ctx, "colorbar:Color").unwrap_or(Rect::NOTHING);
        let e = marked(&d.ctx, "eyedrop:Color").unwrap_or(Rect::NOTHING);
        // Fully inside the window (the panel clips what is below its edge).
        if r.height() >= 18.0 && e.height() >= 17.0 && r.min.y > 60.0 && r.max.y < d.size.y - 24.0 {
            break;
        }
        // Low in the Properties panel (its header and tabs are above).
        let at = Pos2::new(d.size.x - 150.0, d.size.y - 80.0);
        d.frame(app, vec![Event::PointerMoved(at)], Modifiers::NONE);
        d.frame(app, vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, -120.0), modifiers: Modifiers::NONE }], Modifiers::NONE);
        d.idle(app, 2);
    }
    // The scroll is animated: let it settle before aiming.
    d.idle(app, 20);
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
