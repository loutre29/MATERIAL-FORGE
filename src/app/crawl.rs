//! `material_forge --uicrawl <dir>`: clicks every interactive widget of the
//! real UI with synthetic mouse events, in several app states (selections,
//! tools, tabs, flyouts, dialogs), and opens every popup, menu and context
//! menu it finds to click their entries too. Reports:
//! - STOLEN: the click reached another widget than the one under test;
//! - UNREACHABLE: egui's hit test never picks the widget (covered);
//! - NO EFFECT: nothing on screen or in the document changed;
//! - CLASH: egui id clash warnings;
//! - PANIC, INVARIANT (selection pointing at nothing, stuck strokes),
//!   UNDO (undoing the click's history entries does not restore the stack).
//!
//! Writes `crawl_report.txt` and a screenshot per state with the findings
//! outlined. Runs with its own data folder: the user's library, layout and
//! autosaves are never touched.

use super::*;
use crate::document::{find, FillDesc, FilterDesc, FilterKind, GeneratorDesc, LayerKind, MaskEffectKind};
use eframe::egui::{epaint, Event, Id, LayerId, Order, PointerButton, Pos2, RawInput};
use panels::Cmd;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

static LAST_PANIC: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[derive(Clone, Debug)]
struct W {
    id: Id,
    layer: LayerId,
    rect: Rect,
    label: String,
}

impl W {
    fn key(&self) -> (Id, String) {
        (self.id, self.label.clone())
    }

    fn describe(&self) -> String {
        let r = self.rect;
        format!("\"{}\" [{:.0},{:.0} {:.0}x{:.0}] {:?}", self.label, r.min.x, r.min.y, r.width(), r.height(), self.layer.order)
    }
}

#[derive(Clone, Debug)]
struct Finding {
    kind: &'static str,
    state: String,
    what: String,
    rect: Rect,
}

struct Driver {
    ctx: egui::Context,
    size: egui::Vec2,
    time: f64,
    frames: usize,
    shapes: Vec<epaint::ClippedShape>,
    texts: Vec<(Rect, String)>,
    /// Clip rect of each text (its panel or window).
    text_clips: Vec<Rect>,
    clashes: Vec<String>,
}

/// Texts of a shape: the part of each that its clip rect lets show.
fn shape_texts(s: &epaint::Shape, clip: Rect, out: &mut Vec<(Rect, String)>) {
    match s {
        epaint::Shape::Vec(v) => v.iter().for_each(|x| shape_texts(x, clip, out)),
        epaint::Shape::Text(t) => {
            let text = t.galley.text().trim().to_string();
            let r = t.galley.rect.translate(t.pos.to_vec2()).intersect(clip);
            if !text.is_empty() && r.is_positive() {
                out.push((r, text));
            }
        }
        _ => {}
    }
}

fn hash_shape(s: &epaint::Shape, h: &mut std::collections::hash_map::DefaultHasher) {
    let q = |p: Pos2| ((p.x * 2.0).round() as i32, (p.y * 2.0).round() as i32);
    match s {
        epaint::Shape::Noop => {}
        epaint::Shape::Vec(v) => v.iter().for_each(|x| hash_shape(x, h)),
        epaint::Shape::Text(t) => {
            t.galley.text().hash(h);
            q(t.pos).hash(h);
            t.fallback_color.hash(h);
            t.override_text_color.hash(h);
        }
        epaint::Shape::Mesh(m) => {
            m.texture_id.hash(h);
            m.vertices.len().hash(h);
            let r = s.visual_bounding_rect();
            (q(r.min), q(r.max)).hash(h);
        }
        other => format!("{other:?}").hash(h),
    }
}

impl Driver {
    fn frame(&mut self, app: &mut RetroApp, events: Vec<Event>) -> Result<(), String> {
        self.time += 1.0 / 60.0;
        let mut raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
            events,
            time: Some(self.time),
            ..Default::default()
        };
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            app.filter_input(&self.ctx, &mut raw);
            self.ctx.run(raw, |c| app.ui_frame(c))
        }));
        self.frames += 1;
        let out = match r {
            Ok(out) => out,
            Err(_) => return Err(LAST_PANIC.lock().unwrap().take().unwrap_or_else(|| "panic".into())),
        };
        {
            let mut rd = app.renderer.write();
            for (id, delta) in &out.textures_delta.set {
                rd.update_texture(&app.engine.device, &app.engine.queue, *id, delta);
            }
            for id in &out.textures_delta.free {
                rd.free_texture(id);
            }
        }
        self.texts.clear();
        self.text_clips.clear();
        self.clashes.clear();
        for cs in &out.shapes {
            let n = self.texts.len();
            shape_texts(&cs.shape, cs.clip_rect, &mut self.texts);
            self.text_clips.extend(std::iter::repeat(cs.clip_rect).take(self.texts.len() - n));
        }
        for (_, t) in &self.texts {
            if t.starts_with('🔥') {
                self.clashes.push(t.clone());
            }
        }
        self.shapes = out.shapes;
        Ok(())
    }

    fn idle(&mut self, app: &mut RetroApp, n: usize) -> Result<(), String> {
        for _ in 0..n {
            self.frame(app, Vec::new())?;
        }
        Ok(())
    }

    fn key(&mut self, app: &mut RetroApp, k: egui::Key) -> Result<(), String> {
        let ev = |pressed| Event::Key { key: k, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.frame(app, vec![ev(true)])?;
        self.frame(app, vec![ev(false)])
    }

    fn digest(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for cs in &self.shapes {
            hash_shape(&cs.shape, &mut h);
        }
        h.finish()
    }

    fn label(&self, r: Rect) -> String {
        let mut v: Vec<&(Rect, String)> = self.texts.iter().filter(|(t, _)| r.contains(t.center())).collect();
        v.sort_by(|a, b| (a.0.min.y as i32, a.0.min.x as i32).cmp(&(b.0.min.y as i32, b.0.min.x as i32)));
        let s: Vec<&str> = v.iter().map(|(_, t)| t.as_str()).collect();
        let mut s = s.join(" ").replace('\n', " ");
        if s.chars().count() > 48 {
            s = s.chars().take(48).collect::<String>() + "...";
        }
        if s.is_empty() {
            "<icon>".into()
        } else {
            s
        }
    }

    /// Clickable, enabled widgets of the last frame.
    fn widgets(&self) -> Vec<W> {
        let screen = Rect::from_min_size(Pos2::ZERO, self.size);
        let raw: Vec<(Id, LayerId, Rect)> = self.ctx.viewport(|v| {
            let mut out = Vec::new();
            for (layer, ws) in v.prev_pass.widgets.layers() {
                if matches!(layer.order, Order::Tooltip | Order::Debug) {
                    continue;
                }
                for w in ws {
                    let r = w.interact_rect.intersect(screen);
                    if w.sense.click && w.enabled && r.width() >= 2.0 && r.height() >= 2.0 {
                        out.push((w.id, *layer, r));
                    }
                }
            }
            out
        });
        raw.into_iter().map(|(id, layer, rect)| W { id, layer, rect, label: self.label(rect) }).collect()
    }

    fn layers(&self) -> HashSet<LayerId> {
        self.ctx.viewport(|v| v.prev_pass.widgets.layer_ids().collect())
    }

    /// A point of `w` that egui's hit test gives to `w`.
    fn reach(&mut self, app: &mut RetroApp, w: &W) -> Result<Option<Pos2>, String> {
        let r = w.rect.shrink(1.0);
        let fr = [0.5, 0.25, 0.75, 0.1, 0.9];
        let mut pts = Vec::new();
        for fy in fr {
            for fx in fr {
                pts.push(Pos2::new(r.min.x + r.width() * fx, r.min.y + r.height() * fy));
            }
        }
        for p in pts.into_iter().take(12) {
            self.frame(app, vec![Event::PointerMoved(p)])?;
            self.frame(app, Vec::new())?;
            if self.ctx.viewport(|v| v.hits.click.map(|x| x.id)) == Some(w.id) {
                return Ok(Some(p));
            }
        }
        Ok(None)
    }

    /// Press and release at `p`; the id egui reports as clicked.
    fn click(&mut self, app: &mut RetroApp, p: Pos2, button: PointerButton) -> Result<Option<Id>, String> {
        let b = |pressed| Event::PointerButton { pos: p, button, pressed, modifiers: Modifiers::NONE };
        self.frame(app, vec![b(true)])?;
        self.frame(app, vec![b(false)])?;
        let id = self.ctx.viewport(|v| v.interact_widgets.clicked);
        Ok(id)
    }

    fn screenshot(&self, app: &RetroApp, path: &Path, marks: &[(Rect, [u8; 3])]) -> anyhow::Result<()> {
        let (w, h) = (self.size.x as u32, self.size.y as u32);
        let dev = &app.engine.device;
        let queue = &app.engine.queue;
        let tex = crate::engine::Tex::new(
            dev,
            "crawl shot",
            w,
            h,
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let jobs = self.ctx.tessellate(self.shapes.clone(), 1.0);
        let sd = egui_wgpu::ScreenDescriptor { size_in_pixels: [w, h], pixels_per_point: 1.0 };
        let mut enc = dev.create_command_encoder(&Default::default());
        let mut rd = app.renderer.write();
        let cmds = rd.update_buffers(dev, queue, &mut enc, &jobs, &sd);
        {
            let rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("crawl shot"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &tex.view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            let mut rp = rp.forget_lifetime();
            rd.render(&mut rp, &jobs, &sd);
        }
        queue.submit(cmds.into_iter().chain([enc.finish()]));
        drop(rd);
        let mut px = tex.read_bytes(dev, queue);
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2);
            p[3] = 255;
        }
        let mut img = image::RgbaImage::from_raw(w, h, px).ok_or_else(|| anyhow::anyhow!("shot size"))?;
        for (r, c) in marks {
            let (x0, y0) = (r.min.x.max(0.0) as u32, r.min.y.max(0.0) as u32);
            let (x1, y1) = ((r.max.x as u32).min(w - 1), (r.max.y as u32).min(h - 1));
            for x in x0..=x1 {
                for y in [y0, y1] {
                    img.put_pixel(x, y, image::Rgba([c[0], c[1], c[2], 255]));
                }
            }
            for y in y0..=y1 {
                for x in [x0, x1] {
                    img.put_pixel(x, y, image::Rgba([c[0], c[1], c[2], 255]));
                }
            }
        }
        img.save(path)?;
        Ok(())
    }
}

/// What the crawler puts back after every click.
struct Baseline {
    tool: Tool,
    poly_mode: PolyMode,
    brush: crate::brush::BrushSettings,
    mat: crate::brush::BrushMaterial,
    stencil: Stencil,
    view: ViewSettings,
    camera: OrbitCamera,
    flags: [bool; 5],
    export: bool,
    bake: bool,
    new: bool,
    dock: Option<dock::Panels>,
    shelf: (shelf::Category, String, bool, shelf::Source, Option<String>),
    props_tab: usize,
    row_channel: usize,
    undo_len: usize,
    active: usize,
    /// (fingerprint, view hash, state) per set.
    sets: Vec<(u64, u64, crate::document::SetState)>,
    digest: u64,
    /// egui's widget memory (scroll offsets, open headers, popups' inner state).
    ui_data: egui::util::IdTypeMap,
}

impl Baseline {
    fn capture(app: &RetroApp, digest: u64) -> Self {
        let doc = app.doc.as_ref().expect("document");
        Baseline {
            tool: app.tool,
            poly_mode: app.poly_mode,
            brush: app.brush.clone(),
            mat: app.mat.clone(),
            stencil: app.stencil.clone(),
            view: app.view.clone(),
            camera: app.camera,
            flags: [app.show_view_settings, app.show_set_settings, app.show_prefs, app.show_log, app.show_history],
            export: app.export_dialog.is_some(),
            bake: app.bake_dialog.is_some(),
            new: app.new_dialog.is_some(),
            dock: app.dock.clone(),
            shelf: app.shelf.ui_state(),
            props_tab: app.props_tab,
            row_channel: app.row_channel,
            undo_len: app.history.undo.len(),
            active: doc.active,
            sets: doc.sets.iter().map(|s| (s.fingerprint_hash(), s.view_hash(), s.state())).collect(),
            digest,
            ui_data: egui::util::IdTypeMap::default(),
        }
    }

    fn doc_matches(&self, app: &RetroApp) -> bool {
        app.doc.as_ref().is_some_and(|d| d.active == self.active && d.sets.len() == self.sets.len() && d.sets.iter().zip(&self.sets).all(|(s, b)| s.fingerprint_hash() == b.0))
    }

    /// Puts the UI and document back. Returns false when the document is
    /// gone or was replaced (the caller reloads the state).
    fn restore(&self, app: &mut RetroApp) -> bool {
        if let Some(job) = app.bake_job.take() {
            job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(h) = job.handle {
                let _ = h.join();
            }
        }
        app.tool = self.tool;
        app.prev_tool = None;
        app.poly_mode = self.poly_mode;
        app.brush = self.brush.clone();
        app.mat = self.mat.clone();
        app.stencil = self.stencil.clone();
        app.view = self.view.clone();
        app.camera = self.camera;
        [app.show_view_settings, app.show_set_settings, app.show_prefs, app.show_log, app.show_history] = self.flags;
        if !self.export {
            app.export_dialog = None;
        } else if app.export_dialog.is_none() {
            app.open_export();
        }
        if !self.bake {
            app.bake_dialog = None;
        } else if app.bake_dialog.is_none() {
            let n = app.doc.as_ref().map(|d| d.sets.len()).unwrap_or(0);
            app.bake_dialog = Some(BakeDialog { settings: Default::default(), high: String::new(), sets: vec![true; n] });
        }
        if !self.new {
            app.new_dialog = None;
        } else if app.new_dialog.is_none() {
            app.new_dialog = Some(NewProjectDialog { path: String::new(), res: 1024, error: None, auto_uv: Default::default() });
        }
        app.dock = self.dock.clone();
        app.shelf.set_ui_state(self.shelf.clone());
        app.props_tab = self.props_tab;
        app.row_channel = self.row_channel;
        app.quick_props = None;
        app.id_pick = None;
        app.close_prompt = false;
        app.ui_hidden = false;
        app.isolate_saved = None;
        app.stroke = None;
        app.pending.clear();
        app.line_end = None;
        app.toast = None;
        app.last_edit = Instant::now() - std::time::Duration::from_secs(10);
        app.layers_focus = false;
        if app.doc.is_none() {
            return false;
        }
        while app.history.undo.len() > self.undo_len {
            app.undo();
        }
        while app.history.undo.len() < self.undo_len && !app.history.redo.is_empty() {
            app.redo();
        }
        app.history.redo.clear();
        let Some(doc) = app.doc.as_mut() else { return false };
        if doc.sets.len() != self.sets.len() {
            return false;
        }
        doc.active = self.active;
        for (s, (h, v, st)) in doc.sets.iter_mut().zip(&self.sets) {
            if s.fingerprint_hash() != *h || s.view_hash() != *v {
                s.restore(st.clone());
            }
        }
        if doc.any_hidden() {
            doc.reset_hidden();
            doc.refresh_visibility(&app.engine);
        }
        app.tracked = None;
        true
    }
}

/// A named app state the crawl runs in.
struct Scenario {
    name: &'static str,
    setup: fn(&mut RetroApp),
}

fn select_first(app: &mut RetroApp, pred: impl Fn(&crate::document::Layer) -> bool, target: impl Fn(&crate::document::Layer) -> Target) {
    let pick = {
        let doc = app.doc.as_ref().unwrap();
        let mut out = None;
        crate::document::visit(&doc.sets[doc.active].layers, &mut |l| {
            if out.is_none() && pred(l) {
                out = Some((l.desc.id, target(l)));
            }
        });
        out
    };
    if let Some((id, t)) = pick {
        app.run_cmd(Cmd::Select(id, t));
    }
}

fn is_fill(l: &crate::document::Layer) -> bool {
    matches!(l.desc.kind, LayerKind::Fill(_))
}

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario { name: "paint layer", setup: |app| select_first(app, |l| matches!(l.desc.kind, LayerKind::Paint), |_| Target::Content) },
        Scenario { name: "fill layer", setup: |app| select_first(app, is_fill, |_| Target::Content) },
        Scenario { name: "mask", setup: |app| select_first(app, |l| l.desc.mask.is_some(), |_| Target::Mask) },
        Scenario {
            name: "mask effect",
            setup: |app| select_first(app, |l| l.desc.mask.as_ref().is_some_and(|m| !m.effects.is_empty()), |l| Target::MaskEffect(l.desc.mask.as_ref().unwrap().effects[0].id)),
        },
        Scenario { name: "filter layer", setup: |app| select_first(app, |l| matches!(l.desc.kind, LayerKind::Filter(_)), |_| Target::Content) },
        Scenario { name: "folder", setup: |app| select_first(app, |l| l.is_folder(), |_| Target::Content) },
        Scenario { name: "content effect", setup: |app| select_first(app, |l| !l.desc.effects.is_empty(), |l| Target::ContentEffect(l.desc.effects[0].id)) },
        Scenario { name: "tool eraser", setup: |app| app.tool = Tool::Erase },
        Scenario { name: "tool projection (dock)", setup: |app| app.tool = Tool::Projection },
        Scenario { name: "auto uv window", setup: |app| app.open_uv_dialog() },
        Scenario {
            name: "tool path",
            setup: |app| {
                app.tool = Tool::Path;
                // A path layer with a selected path (its handles show).
                let r = app.vp_rect;
                for (fx, fy) in [(0.42, 0.52), (0.5, 0.44), (0.58, 0.52)] {
                    app.path_press(glam::Vec2::new(r.width() * fx, r.height() * fy), false, false);
                    app.path_release();
                }
                app.path.pen = true;
            },
        },
        Scenario { name: "tool polygon fill", setup: |app| app.tool = Tool::PolygonFill },
        Scenario { name: "tool smudge", setup: |app| app.tool = Tool::Smudge },
        Scenario { name: "tool clone", setup: |app| app.tool = Tool::Clone },
        Scenario { name: "tool picker", setup: |app| app.tool = Tool::Picker },
        Scenario { name: "tool geometry mask", setup: |app| app.tool = Tool::Hide },
        Scenario {
            name: "tool rectangle select",
            setup: |app| {
                app.tool = Tool::Select(super::selection::SelShape::Rect);
                app.select_all();
            },
        },
        Scenario {
            name: "tool magic wand",
            setup: |app| {
                app.tool = Tool::Wand;
                app.select_uv_rect(glam::Vec2::new(0.2, 0.2), glam::Vec2::new(0.7, 0.6));
            },
        },
        Scenario { name: "tool paint bucket", setup: |app| app.tool = Tool::Bucket },
        Scenario { name: "tool selection brush", setup: |app| app.tool = Tool::SelBrush },
        Scenario {
            name: "select by mesh map window",
            setup: |app| {
                let k = super::selection::MapSel::Cavities;
                app.sel_by_map = Some((k, k.default_threshold()));
            },
        },
        Scenario { name: "tool gradient", setup: |app| app.tool = Tool::Gradient },
        Scenario {
            name: "select modify window",
            setup: |app| {
                app.select_uv_rect(glam::Vec2::new(0.2, 0.2), glam::Vec2::new(0.7, 0.6));
                app.sel_modify = Some((crate::flood::Modify::Feather, 8.0));
            },
        },
        Scenario {
            name: "tool polygonal lasso",
            setup: |app| {
                app.view.layout = Layout::Both;
                app.tool = Tool::Select(super::selection::SelShape::PolyLasso);
                app.select_uv_rect(glam::Vec2::new(0.1, 0.1), glam::Vec2::new(0.6, 0.5));
            },
        },
        Scenario { name: "texture set settings", setup: |app| app.focus_tab(dock::Tab::SetSettings) },
        Scenario { name: "display settings", setup: |app| app.show_view_settings = true },
        Scenario { name: "history", setup: |app| app.show_history = true },
        Scenario { name: "log", setup: |app| app.show_log = true },
        Scenario { name: "export dialog", setup: |app| app.open_export() },
        Scenario {
            name: "bake dialog",
            setup: |app| {
                let n = app.doc.as_ref().map(|d| d.sets.len()).unwrap_or(0);
                app.bake_dialog = Some(BakeDialog { settings: Default::default(), high: String::new(), sets: vec![true; n] });
            },
        },
        Scenario { name: "new project dialog", setup: |app| app.new_dialog = Some(NewProjectDialog { path: String::new(), res: 1024, error: None, auto_uv: Default::default() }) },
        Scenario { name: "2D and 3D views", setup: |app| app.view.layout = Layout::Both },
        Scenario {
            name: "2D manipulator turned view",
            setup: |app| {
                app.view.layout = Layout::Only2D;
                app.view.uv_rot = 25f32.to_radians();
                if let Some(doc) = &mut app.doc {
                    let s = &mut doc.sets[doc.active];
                    let mut fill = None;
                    crate::document::visit(&s.layers, &mut |l| {
                        if fill.is_none() && matches!(&l.desc.kind, LayerKind::Fill(f) if f.projection.mode == crate::document::ProjMode::Uv) {
                            fill = Some(l.desc.id);
                        }
                    });
                    s.selected = fill;
                    s.target = crate::document::Target::Content;
                }
            },
        },
        Scenario {
            name: "library filtered",
            setup: |app| app.shelf.set_ui_state((shelf::Category::Materials, "rust".into(), false, shelf::Source::Starter, None)),
        },
        Scenario { name: "assets list view", setup: |app| app.shelf.set_ui_state((shelf::Category::Materials, String::new(), true, shelf::Source::All, None)) },
    ]
}

/// The crawl project: a paint layer, a fill with a mask and effects, a
/// folder with a child, a filter layer and a paint layer with a content effect.
fn build_project(app: &mut RetroApp, path: &Path) -> anyhow::Result<()> {
    let doc = app.doc.as_mut().ok_or_else(|| anyhow::anyhow!("no document"))?;
    let e = &app.engine;
    doc.add_layer(e, 0, LayerKind::Fill(Box::new(crate::library::material("Rust"))));
    doc.add_mask(e, 0, false);
    doc.add_mask_effect(e, 0, MaskEffectKind::Generator(GeneratorDesc::of(crate::document::Generator::Dirt)));
    doc.add_mask_effect(e, 0, MaskEffectKind::levels());
    let folder = doc.add_layer(e, 0, LayerKind::Folder);
    doc.sets[0].selected = Some(folder);
    doc.add_layer(e, 0, LayerKind::Fill(Box::new(FillDesc::material([0.2, 0.4, 0.8], 0.4, 0.0))));
    doc.sets[0].selected = Some(folder);
    if let Some(l) = crate::document::find_mut(&mut doc.sets[0].layers, folder) {
        l.desc.expanded = true;
    }
    doc.add_layer(e, 0, LayerKind::Filter(FilterDesc::of(FilterKind::Hsl)));
    doc.add_layer(e, 0, LayerKind::Paint);
    doc.add_content_effect(0, FilterDesc::of(FilterKind::Blur));
    crate::project::save(&app.engine, doc, &app.camera, path)?;
    Ok(())
}

/// Holds two or more other clickable widgets of its layer (popup areas,
/// windows, rows): its own empty parts are what a click reaches.
/// Text alignment in the clickable boxes of a frame (buttons, fields,
/// combos, rows): a single-line text must sit vertically centered (2.5 px)
/// and inside its box; texts of one box on one row must share a center.
fn alignment_findings(d: &Driver, state: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let ws = d.widgets();
    for w in &ws {
        let r = w.rect;
        if r.height() < 14.0 || r.height() > 44.0 || r.width() < 16.0 {
            continue;
        }
        // Boxes holding other boxes (rows of buttons) are judged by those.
        if ws.iter().any(|o| o.id != w.id && r.contains_rect(o.rect) && o.rect.area() < r.area() * 0.9) {
            continue;
        }
        // Only what shows on the box's own layer (not a window above it).
        // A text clipped to another panel or window (one under this box's
        // window) is not this box's.
        let area = d.ctx.memory(|m| m.area_rect(w.layer.id)).unwrap_or(Rect::EVERYTHING).expand(1.0);
        let texts: Vec<&(Rect, String)> = d
            .texts
            .iter()
            .zip(&d.text_clips)
            .filter(|((t, _), clip)| r.contains(t.center()) && t.height() <= r.height() + 1.0 && t.height() < 26.0 && area.contains_rect(**clip))
            .filter(|((t, _), _)| d.ctx.layer_id_at(t.center()) == Some(w.layer))
            .map(|(t, _)| t)
            .collect();
        // Lines stacked in the box (title and subtitle) are centered as a block.
        let stacked = texts.iter().any(|(a, _)| texts.iter().any(|(b, _)| (a.center().y - b.center().y).abs() >= a.height().min(b.height()) * 0.6));
        if stacked {
            let (top, bottom) = texts.iter().fold((f32::MAX, f32::MIN), |(t, b), (r2, _)| (t.min(r2.min.y), b.max(r2.max.y)));
            let dy = (top + bottom) * 0.5 - r.center().y;
            if dy.abs() > 2.5 {
                out.push(Finding { kind: "ALIGN", state: state.into(), what: format!("the text block is {dy:+.1} px off center in {}", w.describe()), rect: r });
            }
        }
        for (t, label) in &texts {
            let dy = t.center().y - r.center().y;
            if dy.abs() > 2.5 && !stacked {
                out.push(Finding { kind: "ALIGN", state: state.into(), what: format!("\"{label}\" is {dy:+.1} px off center in {}", w.describe()), rect: *t });
            }
            let over = (r.min.x - t.min.x).max(t.max.x - r.max.x);
            if over > 1.5 {
                out.push(Finding { kind: "ALIGN", state: state.into(), what: format!("\"{label}\" spills {over:.1} px out of {}", w.describe()), rect: *t });
            }
        }
        for (i, (a, la)) in texts.iter().enumerate() {
            for (b, lb) in texts.iter().skip(i + 1) {
                let same_row = (a.center().y - b.center().y).abs() < a.height().min(b.height()) * 0.6;
                let dy = (a.center().y - b.center().y).abs();
                if same_row && dy > 1.5 {
                    out.push(Finding { kind: "ALIGN", state: state.into(), what: format!("\"{la}\" and \"{lb}\" are {dy:.1} px apart vertically in {}", w.describe()), rect: a.union(*b) });
                }
            }
        }
    }
    out
}

fn is_container(all: &[W], w: &W) -> bool {
    all.iter().filter(|o| o.id != w.id && o.layer == w.layer && w.rect.expand(1.0).contains_rect(o.rect)).count() >= 2
}

/// The default UI: every state starts from it.
fn reset_ui(app: &mut RetroApp, ctx: &egui::Context) {
    app.tool = Tool::Paint;
    app.prev_tool = None;
    app.stencil = Stencil::default();
    app.view = ViewSettings::default();
    app.show_view_settings = false;
    app.show_set_settings = false;
    app.show_prefs = false;
    app.show_log = false;
    app.show_history = false;
    app.export_dialog = None;
    app.bake_dialog = None;
    app.new_dialog = None;
    app.dock = None;
    app.shelf.set_ui_state((shelf::Category::Materials, String::new(), false, shelf::Source::All, None));
    app.props_tab = 0;
    app.row_channel = 0;
    app.quick_props = None;
    app.sel_drag = None;
    app.sel_op = Default::default();
    app.sel_through = false;
    app.sel_modify = None;
    app.sel_by_map = None;
    app.poly_to_selection = false;
    if let Some(d) = &mut app.doc {
        d.sets.iter_mut().for_each(|s| s.selection = None);
    }
    widgets::close_color_picker(ctx);
    ctx.memory_mut(|m| m.close_popup());
}

fn invariants(app: &RetroApp) -> Vec<String> {
    let mut out = Vec::new();
    if app.stroke.is_some() {
        out.push("a stroke is still active after the mouse was released".into());
    }
    let Some(doc) = &app.doc else { return out };
    if doc.active >= doc.sets.len() {
        out.push(format!("active set {} of {}", doc.active, doc.sets.len()));
        return out;
    }
    for (i, s) in doc.sets.iter().enumerate() {
        let mut ids = HashSet::new();
        crate::document::visit(&s.layers, &mut |l| {
            if !ids.insert(l.desc.id) {
                out.push(format!("set {i}: duplicate layer id {}", l.desc.id));
            }
        });
        for e in &s.extra {
            if find(&s.layers, *e).is_none() {
                out.push(format!("set {i}: multi-selected layer {e} does not exist"));
            }
        }
        match s.selected.map(|id| (id, find(&s.layers, id))) {
            Some((id, None)) => out.push(format!("set {i}: selected layer {id} does not exist")),
            Some((_, Some(l))) => {
                let ok = match s.target {
                    Target::Content => true,
                    Target::Mask => l.desc.mask.is_some(),
                    Target::MaskEffect(e) => l.desc.mask.as_ref().is_some_and(|m| m.effects.iter().any(|x| x.id == e)),
                    Target::ContentEffect(e) => l.desc.effects.iter().any(|x| x.id == e),
                };
                if !ok {
                    out.push(format!("set {i}: target {:?} does not exist on layer '{}'", s.target, l.desc.name));
                }
            }
            None => {
                if s.target != Target::Content {
                    out.push(format!("set {i}: no selected layer but target {:?}", s.target));
                }
            }
        }
    }
    out
}

/// Clicks an id that must not be clicked headless (quits, file pickers are
/// guarded already).
fn skip(label: &str) -> bool {
    let l = label.to_lowercase();
    l.contains("quit") || l.contains("exit")
}

pub fn ui_crawl(out: &Path) -> anyhow::Result<usize> {
    std::fs::create_dir_all(out)?;
    let out = std::fs::canonicalize(out)?;
    let data = out.join("data");
    std::fs::create_dir_all(&data)?;
    std::env::set_var("MF_DATA_DIR", &data);
    std::env::set_current_dir(&out)?;
    HEADLESS.store(true, std::sync::atomic::Ordering::Relaxed);
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("{info}");
        *LAST_PANIC.lock().unwrap() = Some(msg);
    }));
    let only: Option<String> = std::env::var("MF_CRAWL_ONLY").ok();
    let limit: usize = std::env::var("MF_CRAWL_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
    let t0 = Instant::now();
    let size = std::env::var("MF_UITEST_SIZE")
        .ok()
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some(egui::vec2(w.parse().ok()?, h.parse().ok()?))))
        .unwrap_or(egui::vec2(1600.0, 950.0));

    let obj = out.join("crawl.obj");
    std::fs::write(&obj, crate::selftest::test_obj())?;
    let project = out.join("crawl.sprp");
    let fresh = |with: Option<PathBuf>| -> anyhow::Result<(RetroApp, Driver)> {
        let engine = Engine::headless()?;
        let renderer = egui_wgpu::Renderer::new(&engine.device, wgpu::TextureFormat::Bgra8Unorm, None, 1, false);
        let ctx = egui::Context::default();
        ctx.options_mut(|o| o.warn_on_id_clash = true);
        ctx.style_mut(|s| s.interaction.tooltip_delay = 1.0e6);
        let mut app = RetroApp::build(engine, Arc::new(egui::mutex::RwLock::new(renderer)), &ctx, with);
        app.persist_layout = false;
    app.lock_cursor = false;
        app.autosave_minutes = 0.0;
        let d = Driver { ctx, size, time: 0.0, frames: 0, shapes: Vec::new(), texts: Vec::new(), text_clips: Vec::new(), clashes: Vec::new() };
        Ok((app, d))
    };
    {
        let (mut app, mut d) = fresh(Some(obj.clone()))?;
        d.idle(&mut app, 3).map_err(anyhow::Error::msg)?;
        build_project(&mut app, &project)?;
    }
    let (mut app, mut d) = fresh(None)?;

    let mut findings: Vec<Finding> = Vec::new();
    let mut align_seen: HashSet<String> = HashSet::new();
    let mut seen: HashSet<(Id, String)> = HashSet::new();
    let mut seen_clash: HashSet<String> = HashSet::new();
    let mut tested = 0usize;
    let mut tested_children = 0usize;

    // Loads the project and applies the state; returns its baseline.
    let enter = |app: &mut RetroApp, d: &mut Driver, sc: &Scenario| -> Result<Baseline, String> {
        app.open_path(&project);
        reset_ui(app, &d.ctx);
        d.idle(app, 2)?;
        app.history.clear();
        (sc.setup)(app);
        d.frame(app, vec![Event::PointerGone])?;
        d.idle(app, 4)?;
        app.last_edit = Instant::now() - std::time::Duration::from_secs(10);
        d.idle(app, 2)?;
        app.history.redo.clear();
        let undo_len = app.history.undo.len();
        let mut b = Baseline::capture(app, d.digest());
        b.undo_len = undo_len;
        b.ui_data = d.ctx.memory(|m| m.data.clone());
        Ok(b)
    };

    for (si, sc) in scenarios().iter().enumerate() {
        if only.as_deref().is_some_and(|o| !sc.name.contains(o)) {
            continue;
        }
        let st = Instant::now();
        let mut base = match enter(&mut app, &mut d, sc) {
            Ok(b) => b,
            Err(p) => {
                findings.push(Finding { kind: "PANIC", state: sc.name.into(), what: format!("entering the state: {p}"), rect: Rect::NOTHING });
                (app, d) = fresh(None)?;
                continue;
            }
        };
        // The reference frame must be stable or every click looks effective.
        d.idle(&mut app, 2).map_err(anyhow::Error::msg)?;
        let stable = d.digest() == base.digest;
        base.digest = d.digest();
        if !stable {
            println!("  warning: '{}' does not render the same frame twice", sc.name);
        }
        let shot_marks_start = findings.len();
        // The same box is reported once per run.
        for f in alignment_findings(&d, sc.name) {
            if align_seen.insert(f.what.clone()) {
                findings.push(f);
            }
        }
        let mut queue: Vec<W> = d.widgets().into_iter().filter(|w| !seen.contains(&w.key())).collect();
        let total = queue.len();
        let mut n_state = 0;
        // (parent, point, child) to test after the state's own widgets.
        let mut children: Vec<(W, Pos2, PointerButton, W)> = Vec::new();
        let mut reload = false;
        while let Some(w0) = queue.pop() {
            if tested >= limit {
                break;
            }
            if !seen.insert(w0.key()) || skip(&w0.label) {
                continue;
            }
            // Positions can move after a reset: look the widget up again.
            let Some(w) = d.widgets().into_iter().find(|x| x.id == w0.id) else { continue };
            tested += 1;
            n_state += 1;
            let res = (|| -> Result<(), String> {
                let Some(p) = d.reach(&mut app, &w)? else {
                    let covered = d.ctx.viewport(|v| v.hits.click.map(|x| x.id));
                    let cover = covered.and_then(|id| d.widgets().into_iter().find(|x| x.id == id));
                    // Under a window or popup, or a container of other widgets: expected.
                    let center_layer = d.ctx.layer_id_at(w.rect.center());
                    let expected = cover.as_ref().is_some_and(|c| c.layer != w.layer) || center_layer.is_some_and(|l| l != w.layer) || is_container(&d.widgets(), &w);
                    if !expected {
                        let by = cover.map(|x| x.describe()).unwrap_or("nothing".into());
                        findings.push(Finding { kind: "UNREACHABLE", state: sc.name.into(), what: format!("{} (covered by {by})", w.describe()), rect: w.rect });
                    }
                    return Ok(());
                };
                let container = is_container(&d.widgets(), &w);
                let before = d.digest();
                let layers_before = d.layers();
                let doc_before = base.doc_matches(&app);
                for button in [PointerButton::Primary, PointerButton::Secondary] {
                    let clicked = d.click(&mut app, p, button)?;
                    d.idle(&mut app, 4)?;
                    if button == PointerButton::Primary {
                        match clicked {
                            Some(id) if id != w.id => {
                                let by = d.widgets().into_iter().find(|x| x.id == id).map(|x| x.describe()).unwrap_or(format!("{id:?}"));
                                findings.push(Finding { kind: "STOLEN", state: sc.name.into(), what: format!("{} -> click went to {by}", w.describe()), rect: w.rect });
                            }
                            _ => {}
                        }
                        let changed = d.digest() != before || !base.doc_matches(&app) || !doc_before;
                        let thin = w.rect.width().min(w.rect.height()) < 7.0;
                        if !changed && !container && !thin && (w.layer.order != Order::Background || w.rect.area() < 60_000.0) {
                            findings.push(Finding { kind: "NO EFFECT", state: sc.name.into(), what: w.describe(), rect: w.rect });
                        }
                    }
                    for c in std::mem::take(&mut d.clashes) {
                        if seen_clash.insert(c.clone()) {
                            findings.push(Finding { kind: "CLASH", state: sc.name.into(), what: format!("{c} (after clicking {})", w.describe()), rect: w.rect });
                        }
                    }
                    for inv in invariants(&app) {
                        findings.push(Finding { kind: "INVARIANT", state: sc.name.into(), what: format!("{inv} (after {button:?} click on {})", w.describe()), rect: w.rect });
                    }
                    // Popups, menus and windows the click opened.
                    let new_layers: Vec<LayerId> = d.layers().difference(&layers_before).copied().filter(|l| matches!(l.order, Order::Foreground | Order::Middle)).collect();
                    if !new_layers.is_empty() {
                        for c in d.widgets().into_iter().filter(|c| new_layers.contains(&c.layer)) {
                            if !seen.contains(&c.key()) && children.len() < 4000 {
                                children.push((w.clone(), p, button, c));
                            }
                        }
                    }
                    // Undo must restore what the click changed.
                    let added = app.history.undo.len().saturating_sub(base.undo_len);
                    if added > 0 {
                        for _ in 0..added {
                            app.undo();
                        }
                        d.idle(&mut app, 1)?;
                        if !base.doc_matches(&app) && app.doc.is_some() {
                            findings.push(Finding {
                                kind: "UNDO",
                                state: sc.name.into(),
                                what: format!("undoing {added} entr(ies) after {button:?} click on {} did not restore the layer stack", w.describe()),
                                rect: w.rect,
                            });
                        }
                    }
                    d.frame(&mut app, vec![Event::PointerGone])?;
                    d.key(&mut app, egui::Key::Escape)?;
                    widgets::close_color_picker(&d.ctx);
                    d.ctx.memory_mut(|m| {
                        m.close_popup();
                        if let Some(f) = m.focused() {
                            m.surrender_focus(f);
                        }
                    });
                    if !base.restore(&mut app) {
                        reload = true;
                        return Ok(());
                    }
                    d.ctx.memory_mut(|m| m.data = base.ui_data.clone());
                    d.time += 2.0;
                    d.idle(&mut app, 3)?;
                    // Back over the widget for the second button.
                    d.frame(&mut app, vec![Event::PointerMoved(p)])?;
                    d.idle(&mut app, 1)?;
                }
                Ok(())
            })();
            if let Err(p) = res {
                findings.push(Finding { kind: "PANIC", state: sc.name.into(), what: format!("{p} (clicking {})", w.describe()), rect: w.rect });
                (app, d) = fresh(None)?;
                reload = true;
            }
            if reload {
                reload = false;
                base = match enter(&mut app, &mut d, sc) {
                    Ok(b) => b,
                    Err(_) => break,
                };
            }
        }
        // Popup entries: open the parent again, then click the entry.
        let mut n_children = 0;
        for (parent, p, button, child) in children {
            if tested >= limit {
                break;
            }
            if !seen.insert(child.key()) || skip(&child.label) {
                continue;
            }
            tested += 1;
            tested_children += 1;
            n_children += 1;
            let res = (|| -> Result<(), String> {
                // The parent may have moved since: find it again.
                let p = match d.widgets().into_iter().find(|x| x.id == parent.id) {
                    Some(w) => d.reach(&mut app, &w)?.unwrap_or(p),
                    None => p,
                };
                d.frame(&mut app, vec![Event::PointerMoved(p)])?;
                d.idle(&mut app, 1)?;
                d.click(&mut app, p, button)?;
                d.idle(&mut app, 4)?;
                let Some(c) = d.widgets().into_iter().find(|x| x.id == child.id) else {
                    let want = std::env::var("MF_CRAWL_DEBUG").ok().is_some_and(|w| parent.label.contains(&w));
                    let n = findings.iter().filter(|f| f.kind == "FLAKY" && f.what.contains(&std::env::var("MF_CRAWL_DEBUG").unwrap_or_default())).count();
                    if want && n < 3 {
                        let _ = d.screenshot(&app, &out.join(format!("flaky_{n}.png")), &[(child.rect, [255, 0, 255]), (parent.rect, [0, 255, 0])]);
                    }
                    findings.push(Finding {
                        kind: "FLAKY",
                        state: sc.name.into(),
                        what: format!("{} did not appear again after {button:?} click on {}", child.describe(), parent.describe()),
                        rect: child.rect,
                    });
                    return Ok(());
                };
                let container = is_container(&d.widgets(), &c);
                match d.reach(&mut app, &c)? {
                    None if container => {}
                    None => findings.push(Finding { kind: "UNREACHABLE", state: sc.name.into(), what: format!("{} (in popup of {})", c.describe(), parent.describe()), rect: c.rect }),
                    Some(q) => {
                        let before = d.digest();
                        let clicked = d.click(&mut app, q, PointerButton::Primary)?;
                        d.idle(&mut app, 4)?;
                        if let Some(id) = clicked.filter(|id| *id != c.id) {
                            let by = d.widgets().into_iter().find(|x| x.id == id).map(|x| x.describe()).unwrap_or(format!("{id:?}"));
                            findings.push(Finding { kind: "STOLEN", state: sc.name.into(), what: format!("{} (in popup of {}) -> {by}", c.describe(), parent.describe()), rect: c.rect });
                        }
                        if d.digest() == before && base.doc_matches(&app) && !container {
                            findings.push(Finding { kind: "NO EFFECT", state: sc.name.into(), what: format!("{} (in popup of {})", c.describe(), parent.describe()), rect: c.rect });
                        }
                        for cl in std::mem::take(&mut d.clashes) {
                            if seen_clash.insert(cl.clone()) {
                                findings.push(Finding { kind: "CLASH", state: sc.name.into(), what: format!("{cl} (after clicking {})", c.describe()), rect: c.rect });
                            }
                        }
                        for inv in invariants(&app) {
                            findings.push(Finding { kind: "INVARIANT", state: sc.name.into(), what: format!("{inv} (after clicking {} in popup of {})", c.describe(), parent.describe()), rect: c.rect });
                        }
                        let added = app.history.undo.len().saturating_sub(base.undo_len);
                        if added > 0 {
                            for _ in 0..added {
                                app.undo();
                            }
                            d.idle(&mut app, 1)?;
                            if !base.doc_matches(&app) && app.doc.is_some() {
                                findings.push(Finding {
                                    kind: "UNDO",
                                    state: sc.name.into(),
                                    what: format!("undoing {added} entr(ies) after {} (popup of {}) did not restore the layer stack", c.describe(), parent.describe()),
                                    rect: c.rect,
                                });
                            }
                        }
                    }
                }
                d.frame(&mut app, vec![Event::PointerGone])?;
                d.key(&mut app, egui::Key::Escape)?;
                widgets::close_color_picker(&d.ctx);
                d.ctx.memory_mut(|m| {
                    m.close_popup();
                    if let Some(f) = m.focused() {
                        m.surrender_focus(f);
                    }
                });
                if !base.restore(&mut app) {
                    reload = true;
                    return Ok(());
                }
                d.ctx.memory_mut(|m| m.data = base.ui_data.clone());
                d.time += 2.0;
                d.idle(&mut app, 3)?;
                Ok(())
            })();
            if let Err(p) = res {
                findings.push(Finding { kind: "PANIC", state: sc.name.into(), what: format!("{p} (clicking {} in popup of {})", child.describe(), parent.describe()), rect: child.rect });
                (app, d) = fresh(None)?;
                reload = true;
            }
            if reload {
                reload = false;
                base = match enter(&mut app, &mut d, sc) {
                    Ok(b) => b,
                    Err(_) => break,
                };
            }
        }
        // Screenshot of the state with its findings outlined.
        let _ = enter(&mut app, &mut d, sc);
        let marks: Vec<(Rect, [u8; 3])> = findings[shot_marks_start..]
            .iter()
            .filter(|f| f.rect.is_positive())
            .map(|f| {
                let c = match f.kind {
                    "STOLEN" => [255, 40, 40],
                    "UNREACHABLE" => [255, 160, 0],
                    "NO EFFECT" => [60, 160, 255],
                    _ => [255, 0, 255],
                };
                (f.rect, c)
            })
            .collect();
        let shot = out.join(format!("crawl_{si:02}_{}.png", sc.name.replace(' ', "_")));
        if let Err(e) = d.screenshot(&app, &shot, &marks) {
            println!("  screenshot failed: {e:#}");
        }
        println!(
            "[{}] {} widgets ({} new), {} popup entries, {} findings, {:.1}s",
            sc.name,
            n_state,
            total,
            n_children,
            findings.len() - shot_marks_start,
            st.elapsed().as_secs_f32()
        );
    }

    // Report.
    let mut by_kind: HashMap<&str, usize> = HashMap::new();
    for f in &findings {
        *by_kind.entry(f.kind).or_default() += 1;
    }
    let mut text = String::new();
    text += &format!(
        "UI crawl: {} widgets clicked ({} popup entries), {} frames, {:.0}s\n",
        tested,
        tested_children,
        d.frames,
        t0.elapsed().as_secs_f32()
    );
    for k in ["PANIC", "INVARIANT", "UNDO", "CLASH", "STOLEN", "UNREACHABLE", "FLAKY", "NO EFFECT", "ALIGN"] {
        text += &format!("{k}: {}\n", by_kind.get(k).copied().unwrap_or(0));
    }
    for k in ["PANIC", "INVARIANT", "UNDO", "CLASH", "STOLEN", "UNREACHABLE", "FLAKY", "NO EFFECT", "ALIGN"] {
        let v: Vec<&Finding> = findings.iter().filter(|f| f.kind == k).collect();
        if v.is_empty() {
            continue;
        }
        text += &format!("\n== {k}\n");
        for f in v {
            text += &format!("[{}] {}\n", f.state, f.what);
        }
    }
    std::fs::write(out.join("crawl_report.txt"), &text)?;
    println!("{}", text.lines().take(10).collect::<Vec<_>>().join("\n"));
    let serious = findings.iter().filter(|f| matches!(f.kind, "PANIC" | "INVARIANT" | "UNDO" | "CLASH" | "STOLEN")).count();
    println!("UICRAWL done: {serious} serious findings, report in {}", out.join("crawl_report.txt").display());
    Ok(serious)
}
