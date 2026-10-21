//! Menu bar, deferred panel commands and shared property widgets.

use super::*;
use crate::document::*;
use super::shelf::Item;
use eframe::egui::{self, Ui};

/// Drag payload of layer rows: the dragged layers (the whole selection when
/// the dragged row is part of it).
pub struct LayerDrag(pub Vec<u64>);

/// Deferred actions from the panels (they need the engine or a checkpoint).
pub(super) enum Cmd {
    Select(u64, Target),
    ToggleVisible(u64),
    ToggleExpand(u64),
    ToggleEffect(u64, u64),
    AddLayer(LayerKind),
    AddMask(bool),
    AddEffect(MaskEffectKind),
    ApplySmartMask(MaskDesc),
    RemoveMask,
    RemoveEffect(u64),
    MoveEffect(u64, i32),
    ClearMaskPaint(u64, f32),
    Duplicate,
    Delete,
    Group,
    Ungroup,
    IntoFolder,
    Move(i32),
    SaveSmartMaterial,
    SaveSmartMask,
    IdPick(u64),
    /// Dragged layers, row they were dropped on, where.
    DropOn(Vec<u64>, u64, crate::document::Place),
    /// Ctrl + click: adds the layer to the selection or takes it out.
    SelectToggle(u64),
    /// Shift + click: every row between the selected layer and this one.
    SelectRange(u64),
    /// Right click on a selected row: it becomes the main one, the
    /// selection stays.
    MakePrimary(u64),
    /// Assigns a project resource to a slot.
    AssignResource(u64, TexSlot),
    /// Imports an image file into the project, then assigns it.
    AssignFile(PathBuf, TexSlot),
    /// An asset dropped on the layer stack (on a row, or below the rows).
    DropItem(Option<u64>, Item),
    /// Layer row blend dropdown: (layer, channel, mode).
    SetBlend(u64, usize, BlendMode),
    /// Layer row opacity: (layer, channel, value).
    SetOpacity(u64, usize, f32),
    Rename(u64, String),
    /// Shift + click on a mask thumbnail.
    ToggleMask(u64),
    CopyLayer(u64),
    PasteLayer,
    /// Filter on the selected layer's content.
    AddContentEffect(FilterKind),
    RemoveContentEffect(u64),
    MoveContentEffect(u64, i32),
    /// (layer, effect)
    ToggleContentEffect(u64, u64),
    /// Color tag of a layer.
    SetTag(u64, u8),
    /// Layer menu "Blending options": copy the chosen channel's blend mode /
    /// opacity to every channel (mode, opacity).
    BlendToAll(u64, usize, bool, bool),
    CopyBlending(u64),
    PasteBlending(u64),
    /// Mask menu: invert the mask (an Invert effect on top) / clear its effects.
    InvertMask(u64),
    ClearMask(u64),
    /// Alt + click on a mask thumbnail: show the mask in the viewport.
    /// Alt + click on a mask: show it, or back to the material when it was
    /// the mask already shown.
    ViewMask,
    /// Swap the mask background (white / black).
    InvertMaskBase(u64),
    CopyMask(u64),
    /// Paste the copied mask (effects and painted content) into a layer.
    PasteMask(u64),
}

/// Where an imported texture goes.
#[derive(Clone, Copy)]
pub enum TexSlot {
    FillChannel(u64, usize),
    MaskFill(u64, u64),
    Stencil,
    Alpha,
}

pub(super) fn slider(ui: &mut Ui, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>) {
    super::widgets::Slider::new(label, v, range).show(ui);
}

pub(super) fn proc_params(ui: &mut Ui, id: impl std::hash::Hash + Copy, p: &mut ProcParams) {
    use super::widgets::{sub_section, toggle_row, Slider};
    sub_section(ui, (id, "pp"), "Parameters", true, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 70.0);
                Slider::new("Seed", &mut p.seed, 0.0..=100.0).decimals(0).show(ui);
            });
            if ui.button("Random").clicked() {
                let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
                p.seed = (t % 10_000) as f32 / 100.0;
            }
        });
        toggle_row(ui, "Invert", &mut p.invert);
        slider(ui, "Balance", &mut p.balance, 0.0..=1.0);
        Slider::new("Contrast", &mut p.contrast, 0.1..=10.0).log().show(ui);
        let title = format!("{} parameters", p.kind.name());
        // Painter's shelf procedurals: their own parameters.
        if p.kind.is_bitmap() {
            if let Some(d) = crate::procedurals::def_of(p.kind) {
                if !d.params.is_empty() {
                    sub_section(ui, (id, "kind params"), &title, true, |ui| {
                        for q in d.params {
                            let v = &mut p.p[q.slot];
                            let s = Slider::new(q.label, v, q.min..=q.max);
                            if q.int {
                                s.decimals(0).show(ui);
                            } else {
                                s.show(ui);
                            }
                        }
                    });
                }
            }
            if ui.add(egui::Button::new("Restore default parameters").min_size(egui::vec2(ui.available_width(), 22.0))).clicked() {
                *p = ProcParams::of(p.kind);
            }
            return;
        }
        let mut non_square = (p.aspect - 1.0).abs() > 1e-3;
        if toggle_row(ui, "Non square expansion", &mut non_square) {
            p.aspect = if non_square { 2.0 } else { 1.0 };
        }
        if non_square {
            Slider::new("Expansion", &mut p.aspect, 0.02..=50.0).log().show(ui);
        }
        sub_section(ui, (id, "kind params"), &title, true, |ui| {
            Slider::new("Scale", &mut p.scale, 0.1..=32.0).log().soft(0.01, 1000.0).show(ui);
            Slider::new("Disorder", &mut p.warp, 0.0..=2.0).show(ui);
            match p.kind {
                ProcKind::Clouds | ProcKind::Perlin | ProcKind::Ridged | ProcKind::Turbulence | ProcKind::Marble | ProcKind::Grunge => {
                    Slider::new("Detail", &mut p.detail, 1.0..=8.0).decimals(0).show(ui);
                    slider(ui, "Roughness", &mut p.roughness, 0.1..=0.9);
                }
                ProcKind::Cells | ProcKind::Hexagons | ProcKind::Dots | ProcKind::Leather | ProcKind::Cracks | ProcKind::Scratches
                | ProcKind::Fibers | ProcKind::Grain | ProcKind::Rust | ProcKind::Fabric => {
                    slider(ui, "Density", &mut p.density, 0.0..=1.0);
                    slider(ui, "Roughness", &mut p.roughness, 0.1..=0.9);
                }
                ProcKind::Wood | ProcKind::Bricks | ProcKind::Checker | ProcKind::Gradient => {
                    slider(ui, "Density", &mut p.density, 0.0..=1.0);
                }
                // The 3D noises (position based).
                _ => {
                    Slider::new("Detail", &mut p.detail, 1.0..=8.0).decimals(0).show(ui);
                    slider(ui, "Roughness", &mut p.roughness, 0.1..=0.9);
                }
            }
        });
        if ui.add(egui::Button::new("Restore default parameters").min_size(egui::vec2(ui.available_width(), 22.0))).clicked() {
            *p = ProcParams::of(p.kind);
        }
    });
}

pub(super) fn projection_ui(ui: &mut Ui, id: impl std::hash::Hash + Copy, p: &mut Projection) {
    super::widgets::combo_row(ui, "Projection", (id, "pm"), &mut p.mode, ProjMode::ALL, ProjMode::name);
    super::widgets::sub_section(ui, (id, "uvt"), "UV transformations", true, |ui| {
        super::widgets::Slider::new("Tiling", &mut p.scale, 0.05..=256.0).log().soft(0.001, 100000.0).show(ui);
        if p.mode == ProjMode::Uv {
            super::widgets::Slider::new("Rotation", &mut p.rotation, -180.0..=180.0).decimals(0).show(ui);
        }
        slider(ui, "Offset U", &mut p.offset[0], -1.0..=1.0);
        slider(ui, "Offset V", &mut p.offset[1], -1.0..=1.0);
        if p.mode == ProjMode::Triplanar {
            slider(ui, "Hardness", &mut p.sharpness, 1.0..=32.0);
        }
    });
    if p.mode != ProjMode::Uv {
        // The viewport manipulator's values (W move, E rotate, R scale).
        super::widgets::sub_section(ui, (id, "p3d"), "3D transformation", true, |ui| {
            let row = |ui: &mut Ui, label: &str, v: &mut [f32; 3], speed: f64| {
                ui.horizontal(|ui| {
                    ui.add_sized([60.0, 20.0], egui::Label::new(egui::RichText::new(label).size(12.0).color(super::widgets::LABEL)));
                    let w = ((ui.available_width() - 8.0) / 3.0).max(30.0);
                    for x in v.iter_mut() {
                        ui.add_sized([w, 20.0], egui::DragValue::new(x).speed(speed).max_decimals(3));
                    }
                });
            };
            row(ui, "Position", &mut p.position, 0.01);
            let q = p.rotation3();
            let (a, b, c) = q.to_euler(glam::EulerRot::XYZ);
            let mut deg = [a.to_degrees(), b.to_degrees(), c.to_degrees()];
            let before = deg;
            row(ui, "Rotation", &mut deg, 0.5);
            if deg != before {
                let q = glam::Quat::from_euler(glam::EulerRot::XYZ, deg[0].to_radians(), deg[1].to_radians(), deg[2].to_radians());
                p.orient = q.to_array();
            }
            row(ui, "Scale", &mut p.axes, 0.01);
            for a in p.axes.iter_mut() {
                *a = a.clamp(0.001, 1000.0);
            }
        });
    }
}

impl RetroApp {
    pub(super) fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").frame(egui::Frame::none().fill(crate::style::BG).inner_margin(egui::Margin::symmetric(6.0, 3.0))).show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                let has = self.doc.is_some();
                let file = ui.menu_button("File", |ui| {
                    if ui.add(egui::Button::new("New project...").shortcut_text("Ctrl+N")).clicked() {
                        self.new_dialog = Some(NewProjectDialog { path: String::new(), res: 2048, error: None });
                        ui.close_menu();
                    }
                    if ui.add(egui::Button::new("Open...").shortcut_text("Ctrl+O")).clicked() {
                        ui.close_menu();
                        self.open_project();
                    }
                    ui.separator();
                    if ui.add_enabled(has, egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
                        ui.close_menu();
                        self.save_project(false);
                    }
                    if ui.add_enabled(has, egui::Button::new("Save as...").shortcut_text("Ctrl+Shift+S")).clicked() {
                        ui.close_menu();
                        self.save_project(true);
                    }
                    ui.separator();
                    if ui.add_enabled(has, egui::Button::new("Reimport mesh...")).clicked() {
                        ui.close_menu();
                        self.reimport_mesh();
                    }
                    if ui.button("Import resources...").clicked() {
                        ui.close_menu();
                        self.import_resources(super::ImportKind::Texture);
                    }
                    ui.separator();
                    if ui.add_enabled(has, egui::Button::new("Export textures...").shortcut_text("Ctrl+Shift+E")).clicked() {
                        ui.close_menu();
                        self.open_export();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                super::mark(ctx, "menu:File", file.response.rect);
                ui.menu_button("Edit", |ui| {
                    if ui.add_enabled(!self.history.undo.is_empty(), egui::Button::new("Undo").shortcut_text("Ctrl+Z")).clicked() {
                        self.undo();
                        ui.close_menu();
                    }
                    if ui.add_enabled(!self.history.redo.is_empty(), egui::Button::new("Redo").shortcut_text("Ctrl+Y")).clicked() {
                        self.redo();
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.menu_button("Add layer", |ui| {
                        ui.add_enabled_ui(has, |ui| self.add_layer_menu(ui));
                    });
                });
                ui.menu_button("Mode", |ui| {
                    let busy = self.bake_job.is_some();
                    if ui.radio(!self.view.path_trace && self.bake_dialog.is_none(), "Painting (F9)").clicked() {
                        self.view.path_trace = false;
                        self.bake_dialog = None;
                        ui.close_menu();
                    }
                    if ui.add_enabled(has && !busy, egui::RadioButton::new(self.bake_dialog.is_some(), "Baking (F8)")).clicked() {
                        self.open_bake_dialog();
                        ui.close_menu();
                    }
                    if ui.radio(self.view.path_trace, "Rendering (F10)").clicked() {
                        self.view.path_trace = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("Window", |ui| {
                    ui.menu_button("Views", |ui| {
                        for tab in super::dock::Tab::PANELS {
                            let mut on = self.tab_open(tab);
                            if ui.checkbox(&mut on, tab.name()).clicked() {
                                if on {
                                    self.focus_tab(tab);
                                } else if let Some(mut d) = self.dock.take() {
                                    if let Some(found) = d.find_tab(&tab) {
                                        d.remove_tab(found);
                                    }
                                    self.dock = Some(d);
                                }
                            }
                        }
                    });
                    if ui.button("Reset UI").clicked() {
                        self.dock = None;
                        ui.close_menu();
                    }
                    if ui.add(egui::Button::new("Hide UI").shortcut_text("Tab")).clicked() {
                        self.ui_hidden = !self.ui_hidden;
                        ui.close_menu();
                    }
                    ui.separator();
                    // Flyouts: one at a time, like the icon strip.
                    for (i, on, name) in [(0u8, self.show_view_settings, "Display settings"), (1, self.show_history, "History"), (2, self.show_log, "Log")] {
                        let mut v = on;
                        if ui.checkbox(&mut v, name).clicked() {
                            self.flip_flyout(i);
                        }
                    }
                });
                ui.menu_button("Viewport", |ui| {
                    ui.radio_value(&mut self.view.layout, Layout::Both, "3D + 2D (F1)");
                    ui.radio_value(&mut self.view.layout, Layout::Only3D, "3D only (F2)");
                    ui.radio_value(&mut self.view.layout, Layout::Only2D, "2D only (F3)");
                    ui.checkbox(&mut self.view.swap_views, "Swap 2D / 3D (F4)");
                    ui.separator();
                    ui.menu_button("3D view display", |ui| {
                        for d in Display::ALL {
                            ui.radio_value(&mut self.view.display, d, d.name());
                        }
                    });
                    ui.menu_button("2D view display", |ui| {
                        for d in Display::ALL {
                            ui.radio_value(&mut self.view.display_2d, d, d.name());
                        }
                    });
                    ui.checkbox(&mut self.view.wireframe, "Wireframe");
                    ui.checkbox(&mut self.camera.ortho, "Orthographic (F6, F5 perspective)");
                    ui.menu_button("Camera view", |ui| {
                        use std::f32::consts::{FRAC_PI_2, PI};
                        for (name, yaw, pitch) in [
                            ("Front", 0.0, 0.0),
                            ("Back", PI, 0.0),
                            ("Right", FRAC_PI_2, 0.0),
                            ("Left", -FRAC_PI_2, 0.0),
                            ("Top", 0.0, FRAC_PI_2),
                            ("Bottom", 0.0, -FRAC_PI_2),
                            ("Three quarter", 0.6, 0.25),
                        ] {
                            if ui.button(name).clicked() {
                                self.camera.look_from(yaw, pitch);
                                ui.close_menu();
                            }
                        }
                    });
                    if ui.add(egui::Button::new("Frame mesh").shortcut_text("F")).clicked() {
                        if let Some(doc) = &self.doc {
                            self.camera.frame(doc.mesh.bbox_min, doc.mesh.bbox_max);
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    let mut v = self.show_view_settings;
                    if ui.checkbox(&mut v, "Display settings").clicked() {
                        self.flip_flyout(0);
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("Controls and shortcuts").clicked() {
                        self.show_help = true;
                        ui.close_menu();
                    }
                });
            });
        });
    }

    fn add_layer_menu(&mut self, ui: &mut Ui) {
        let mut cmd = None;
        if ui.button("Paint layer").clicked() {
            cmd = Some(Cmd::AddLayer(LayerKind::Paint));
        }
        if ui.button("Fill layer").clicked() {
            cmd = Some(Cmd::AddLayer(LayerKind::Fill(Box::default())));
        }
        ui.menu_button("Filter layer", |ui| {
            for k in FilterKind::ALL {
                if ui.button(k.name()).clicked() {
                    cmd = Some(Cmd::AddLayer(LayerKind::Filter(FilterDesc::of(*k))));
                    ui.close_menu();
                }
            }
        });
        if ui.button("Folder").clicked() {
            cmd = Some(Cmd::AddLayer(LayerKind::Folder));
        }
        if let Some(c) = cmd {
            self.run_cmd(c);
            ui.close_menu();
        }
    }

    /// Picks files and opens the import dialog (project or library).
    pub(super) fn import_resources(&mut self, kind: super::ImportKind) {
        let Some(files) = super::file_dialog().and_then(|f| f.add_filter("Images", crate::library::IMAGE_EXTS).pick_files()) else {
            return;
        };
        self.ask_import(files, kind);
    }

    /// Opens (or adds files to) the import dialog. Without a project the
    /// library is the only destination.
    pub(super) fn ask_import(&mut self, files: Vec<std::path::PathBuf>, kind: super::ImportKind) {
        match &mut self.import_dialog {
            Some(d) => d.files.extend(super::ImportDialog::new(files, kind, d.to_library).files),
            None => self.import_dialog = Some(super::ImportDialog::new(files, kind, self.doc.is_none() || kind != super::ImportKind::Texture)),
        }
    }

    /// Runs the import dialog's choice.
    pub(super) fn confirm_import(&mut self) {
        let Some(d) = self.import_dialog.take() else { return };
        let (mut project, mut library, mut failed) = (0, 0, 0);
        let mut env = None;
        for (f, kind) in d.files {
            if d.to_library || self.doc.is_none() {
                match crate::library::import_files(kind.folder(), std::slice::from_ref(&f)) {
                    Ok(_) => library += 1,
                    Err(e) => {
                        failed += 1;
                        self.log.push(format!("Could not copy {} to the library: {e:#}", f.display()));
                    }
                }
                continue;
            }
            let res = match kind {
                super::ImportKind::Texture => ResKind::Texture,
                super::ImportKind::Alpha => ResKind::Alpha,
                super::ImportKind::Environment => {
                    // Environments are not stored in projects: shown now.
                    env = Some(f.clone());
                    project += 1;
                    continue;
                }
            };
            let doc = self.doc.as_mut().unwrap();
            match doc.import_resource(&self.engine, &f, res) {
                Ok(_) => project += 1,
                Err(e) => {
                    failed += 1;
                    self.log.push(format!("Could not import {}: {e:#}", f.display()));
                }
            }
        }
        if let Some(p) = env {
            self.load_environment(&p);
        }
        if library > 0 {
            self.shelf.refresh();
        }
        if project > 0 {
            self.shelf.show_category(super::shelf::Category::Project);
            self.view.show_assets = true;
        }
        let mut msg = Vec::new();
        if project > 0 {
            msg.push(format!("{project} file(s) imported into the project"));
        }
        if library > 0 {
            msg.push(format!("{library} file(s) added to your library (every project)"));
        }
        if failed > 0 {
            msg.push(format!("{failed} failed (see the log)"));
        }
        self.status = msg.join(", ");
    }

    /// Points a slot at project resource `id`.
    pub(super) fn assign_resource(&mut self, id: u64, slot: TexSlot) {
        let Some(doc) = self.doc.as_mut() else { return };
        let set = doc.active;
        let s = &mut doc.sets[set];
        match slot {
            TexSlot::FillChannel(lid, c) => {
                if let Some(LayerKind::Fill(f)) = find_mut(&mut s.layers, lid).map(|l| &mut l.desc.kind) {
                    f.channels[c].texture = Some(id);
                    f.channels[c].source = SourceKind::Texture;
                    f.channels[c].enabled = true;
                    if is_color_channel(c) {
                        f.channels[c].value = [1.0; 3];
                    }
                    if c == CH_NORMAL {
                        f.channels[c].value_b = [1.0; 3];
                    }
                }
                s.dirty = true;
            }
            TexSlot::MaskFill(lid, eid) => {
                if let Some(e) =
                    find_mut(&mut s.layers, lid).and_then(|l| l.desc.mask.as_mut()).and_then(|m| m.effects.iter_mut().find(|e| e.id == eid))
                {
                    if let MaskEffectKind::Fill { texture, source, .. } = &mut e.kind {
                        *texture = Some(id);
                        *source = SourceKind::Texture;
                    }
                }
                s.dirty = true;
            }
            TexSlot::Stencil => {
                self.stencil.resource = Some(id);
                self.stencil.enabled = true;
            }
            TexSlot::Alpha => self.brush.alpha = AlphaSource::Resource(id),
        }
    }

    pub(super) fn run_cmd(&mut self, cmd: Cmd) {
        let Some(set) = self.doc.as_ref().map(|d| d.active) else { return };
        match cmd {
            Cmd::SelectToggle(id) => {
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if s.selected == Some(id) {
                    // The last selected layer stays selected.
                    if let Some(next) = s.extra.pop() {
                        s.selected = Some(next);
                    }
                } else if s.extra.contains(&id) {
                    s.extra.retain(|x| *x != id);
                } else {
                    if let Some(p) = s.selected {
                        s.extra.push(p);
                    }
                    s.selected = Some(id);
                }
                s.target = Target::Content;
            }
            Cmd::SelectRange(id) => {
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                let rows = s.visible_rows();
                match (s.selected.and_then(|a| rows.iter().position(|r| *r == a)), rows.iter().position(|r| *r == id)) {
                    (Some(a), Some(b)) => {
                        let (lo, hi) = (a.min(b), a.max(b));
                        let anchor = s.selected;
                        s.extra = rows[lo..=hi].iter().copied().filter(|r| Some(*r) != anchor).collect();
                    }
                    _ => {
                        s.selected = Some(id);
                        s.extra.clear();
                    }
                }
                s.target = Target::Content;
            }
            Cmd::MakePrimary(id) => {
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if s.selected != Some(id) {
                    s.extra.retain(|x| *x != id);
                    if let Some(p) = s.selected {
                        s.extra.push(p);
                    }
                    s.selected = Some(id);
                }
                s.target = Target::Content;
            }
            Cmd::Select(id, t) => {
                let mask_view = self.view.display == Display::LayerMask || self.view.display_2d == Display::LayerMask;
                let doc = self.doc.as_mut().unwrap();
                self.last_select = Some((doc.sets[set].selected, doc.sets[set].target));
                doc.sets[set].extra.clear();
                doc.sets[set].selected = Some(id);
                doc.sets[set].target = t;
                // Only the mask display depends on the selection.
                if mask_view {
                    doc.sets[set].dirty = true;
                }
            }
            Cmd::ToggleVisible(id) => {
                self.checkpoint("Toggle visibility");
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if let Some(l) = find_mut(&mut s.layers, id) {
                    l.desc.visible = !l.desc.visible;
                }
                s.dirty = true;
            }
            Cmd::ToggleExpand(id) => {
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if let Some(l) = find_mut(&mut s.layers, id) {
                    l.desc.expanded = !l.desc.expanded;
                }
            }
            Cmd::ToggleEffect(lid, eid) => {
                self.checkpoint("Toggle mask effect");
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if let Some(e) = find_mut(&mut s.layers, lid).and_then(|l| l.desc.mask.as_mut()).and_then(|m| m.effects.iter_mut().find(|e| e.id == eid)) {
                    e.enabled = !e.enabled;
                }
                s.dirty = true;
            }
            Cmd::AddLayer(kind) => {
                let label = format!("Add {} layer", kind.tag().to_lowercase());
                self.checkpoint(&label);
                let doc = self.doc.as_mut().unwrap();
                doc.add_layer(&self.engine, set, kind);
            }
            Cmd::AddMask(white) => {
                self.checkpoint("Add mask");
                self.doc.as_mut().unwrap().add_mask(&self.engine, set, white);
            }
            Cmd::AddEffect(kind) => {
                self.checkpoint("Add mask effect");
                self.doc.as_mut().unwrap().add_mask_effect(&self.engine, set, kind);
            }
            Cmd::ApplySmartMask(mut mask) => {
                self.checkpoint("Apply smart mask");
                let doc = self.doc.as_mut().unwrap();
                for e in &mut mask.effects {
                    e.id = doc.alloc_id();
                }
                let res = doc.sets[set].resolution();
                let s = &mut doc.sets[set];
                if let Some(l) = s.selected_layer_mut() {
                    l.mask_paint.clear();
                    for e in &mask.effects {
                        if matches!(e.kind, MaskEffectKind::Paint) {
                            l.mask_paint.insert(e.id, new_mask_paint(&self.engine.device, res));
                        }
                    }
                    l.desc.mask = Some(mask);
                    s.target = Target::Mask;
                    s.dirty = true;
                }
            }
            Cmd::RemoveMask => {
                self.checkpoint("Remove mask");
                self.doc.as_mut().unwrap().remove_mask(set);
            }
            Cmd::RemoveEffect(eid) => {
                self.checkpoint("Remove mask effect");
                self.doc.as_mut().unwrap().remove_mask_effect(set, eid);
            }
            Cmd::MoveEffect(eid, d) => {
                self.checkpoint("Reorder mask effect");
                self.doc.as_mut().unwrap().move_mask_effect(set, eid, d);
            }
            Cmd::ClearMaskPaint(eid, value) => {
                let engine = &self.engine;
                {
                    let s = &mut self.doc.as_mut().unwrap().sets[set];
                    let res = s.resolution();
                    if let Some(l) = s.selected_layer_mut() {
                        l.materialize(&engine.device, res, Some(eid));
                    }
                }
                let Some(t) = self.doc.as_ref().unwrap().sets[set].selected_layer().and_then(|l| l.mask_paint.get(&eid)).cloned() else { return };
                self.history.push(Entry::stroke(engine, "Fill mask paint", set, vec![t.clone()]));
                let mut enc = engine.device.create_command_encoder(&Default::default());
                enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("mask paint clear"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &t.view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color { r: value as f64, g: if value > 0.0 { 1.0 } else { 0.0 }, b: 0.0, a: 0.0 }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                engine.queue.submit(Some(enc.finish()));
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if let Some(l) = s.selected_layer_mut() {
                    l.rev = next_rev();
                }
                s.dirty = true;
            }
            Cmd::Duplicate => {
                let doc = self.doc.as_mut().unwrap();
                let ids = doc.sets[set].selection();
                if ids.len() > 1 {
                    self.checkpoint("Duplicate layers");
                    self.doc.as_mut().unwrap().duplicate_layers(&self.engine, set, &ids);
                } else {
                    self.checkpoint("Duplicate layer");
                    self.doc.as_mut().unwrap().duplicate_layer(&self.engine, set);
                }
            }
            Cmd::Delete => {
                let doc = self.doc.as_mut().unwrap();
                let ids = doc.sets[set].selection();
                if ids.len() > 1 {
                    self.checkpoint("Delete layers");
                    self.doc.as_mut().unwrap().delete_layers(set, &ids);
                } else {
                    self.checkpoint("Delete layer");
                    self.doc.as_mut().unwrap().delete_layer(set);
                }
            }
            Cmd::Group => {
                let doc = self.doc.as_mut().unwrap();
                let ids = doc.sets[set].selection();
                self.checkpoint("Group into folder");
                if ids.len() > 1 {
                    self.doc.as_mut().unwrap().group_layers(&self.engine, set, &ids);
                } else {
                    self.doc.as_mut().unwrap().group_selected(&self.engine, set);
                }
            }
            Cmd::Ungroup => {
                self.checkpoint("Move out of folder");
                self.doc.as_mut().unwrap().ungroup_selected(set);
            }
            Cmd::IntoFolder => {
                self.checkpoint("Move into folder");
                self.doc.as_mut().unwrap().move_into_folder_below(set);
            }
            Cmd::Move(d) => {
                self.checkpoint("Move layer");
                self.doc.as_mut().unwrap().move_layer(set, d);
            }
            Cmd::SaveSmartMaterial => {
                let doc = self.doc.as_ref().unwrap();
                let Some(l) = doc.sets[set].selected_layer() else { return };
                let name = l.desc.name.clone();
                let tree = l.tree();
                match crate::library::save_smart_material(doc, &name, &tree) {
                    Ok(p) => {
                        self.shelf.refresh();
                        self.message(format!("Smart material saved: {}", p.display()));
                    }
                    Err(e) => self.message(format!("Could not save smart material: {e:#}")),
                }
            }
            Cmd::SaveSmartMask => {
                let doc = self.doc.as_ref().unwrap();
                let Some(l) = doc.sets[set].selected_layer() else { return };
                let Some(m) = &l.desc.mask else { return };
                match crate::library::save_smart_mask(&format!("{} mask", l.desc.name), m) {
                    Ok(p) => {
                        self.shelf.refresh();
                        self.message(format!("Smart mask saved: {}", p.display()));
                    }
                    Err(e) => self.message(format!("Could not save smart mask: {e:#}")),
                }
            }
            Cmd::AssignResource(id, slot) => self.assign_resource(id, slot),
            Cmd::AssignFile(path, slot) => {
                let doc = self.doc.as_mut().unwrap();
                match doc.import_resource(&self.engine, &path, slot_kind(slot)) {
                    Ok(id) => self.assign_resource(id, slot),
                    Err(e) => self.status = format!("Could not import {}: {e:#}", path.display()),
                }
            }
            Cmd::SetBlend(id, c, m) => {
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if let Some(l) = find_mut(&mut s.layers, id) {
                    l.desc.blend[c] = m;
                }
                s.dirty = true;
            }
            Cmd::SetOpacity(id, c, v) => {
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if let Some(l) = find_mut(&mut s.layers, id) {
                    l.desc.opacity[c] = v;
                }
                s.dirty = true;
            }
            Cmd::CopyLayer(id) => {
                self.layer_clipboard = Some((set, id));
                self.status = "Layer copied (Ctrl+V pastes it above the selection).".into();
            }
            Cmd::PasteLayer => self.paste_layer(),
            Cmd::AddContentEffect(k) => {
                self.checkpoint("Add filter");
                let doc = self.doc.as_mut().unwrap();
                if doc.add_content_effect(set, FilterDesc::of(k)).is_none() {
                    self.status = "Select a paint or fill layer to add a content effect.".into();
                }
            }
            Cmd::RemoveContentEffect(id) => {
                self.checkpoint("Remove effect");
                self.doc.as_mut().unwrap().remove_content_effect(set, id);
            }
            Cmd::MoveContentEffect(id, d) => {
                self.checkpoint("Reorder effect");
                self.doc.as_mut().unwrap().move_content_effect(set, id, d);
            }
            Cmd::ToggleContentEffect(lid, id) => {
                self.checkpoint("Toggle effect");
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if let Some(e) = find_mut(&mut s.layers, lid).and_then(|l| l.desc.effects.iter_mut().find(|e| e.id == id)) {
                    e.enabled = !e.enabled;
                }
                s.dirty = true;
            }
            Cmd::SetTag(id, tag) => {
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if let Some(l) = find_mut(&mut s.layers, id) {
                    l.desc.tag = tag;
                }
            }
            Cmd::BlendToAll(id, ch, mode, opacity) => {
                self.checkpoint("Apply blending to all channels");
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if let Some(l) = find_mut(&mut s.layers, id) {
                    let (m, o) = (l.desc.blend[ch], l.desc.opacity[ch]);
                    for c in 0..NCH {
                        if mode && c != CH_NORMAL {
                            l.desc.blend[c] = m;
                        }
                        if opacity {
                            l.desc.opacity[c] = o;
                        }
                    }
                }
                s.dirty = true;
            }
            Cmd::CopyBlending(id) => {
                let s = &self.doc.as_ref().unwrap().sets[set];
                if let Some(l) = find(&s.layers, id) {
                    self.blend_clipboard = Some((l.desc.blend, l.desc.opacity));
                }
            }
            Cmd::PasteBlending(id) => {
                let Some((b, o)) = self.blend_clipboard else { return };
                self.checkpoint("Paste channel blending");
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if let Some(l) = find_mut(&mut s.layers, id) {
                    l.desc.blend = b;
                    l.desc.opacity = o;
                }
                s.dirty = true;
            }
            Cmd::InvertMask(id) => {
                self.checkpoint("Invert mask");
                let doc = self.doc.as_mut().unwrap();
                let s = &mut doc.sets[set];
                s.selected = Some(id);
                doc.add_mask_effect(&self.engine, set, MaskEffectKind::Invert);
            }
            Cmd::ClearMask(id) => {
                self.checkpoint("Clear mask");
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if let Some(l) = find_mut(&mut s.layers, id) {
                    if let Some(m) = &mut l.desc.mask {
                        m.effects.clear();
                    }
                    l.mask_paint.clear();
                }
                s.target = Target::Mask;
                s.dirty = true;
            }
            Cmd::ToggleMask(id) => {
                self.checkpoint("Toggle mask");
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if let Some(m) = find_mut(&mut s.layers, id).and_then(|l| l.desc.mask.as_mut()) {
                    m.enabled = !m.enabled;
                }
                s.dirty = true;
            }
            Cmd::InvertMaskBase(id) => {
                self.checkpoint("Invert mask background");
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if let Some(m) = find_mut(&mut s.layers, id).and_then(|l| l.desc.mask.as_mut()) {
                    m.base = 1.0 - m.base;
                }
                s.dirty = true;
            }
            Cmd::CopyMask(id) => {
                self.mask_clipboard = Some((set, id));
                self.status = "Mask copied: right click another mask (or a layer's mask slot) > Paste into mask.".into();
            }
            Cmd::PasteMask(dst) => {
                let Some((src_set, src)) = self.mask_clipboard else {
                    self.status = "Copy a mask first.".into();
                    return;
                };
                let doc = self.doc.as_ref().unwrap();
                let Some(srcl) = doc.sets.get(src_set).and_then(|s| find(&s.layers, src)) else { return };
                let Some(mut mask) = srcl.desc.mask.clone() else { return };
                let res = doc.sets[set].resolution();
                let same_res = doc.sets[src_set].resolution() == res;
                let src_paint: Vec<(u64, Arc<crate::engine::Tex>)> = srcl.mask_paint.iter().map(|(k, v)| (*k, v.clone())).collect();
                self.checkpoint("Paste into mask");
                let engine = &self.engine;
                let doc = self.doc.as_mut().unwrap();
                let mut enc = engine.device.create_command_encoder(&Default::default());
                let mut paint = std::collections::HashMap::new();
                for e in &mut mask.effects {
                    let old = e.id;
                    e.id = doc.alloc_id();
                    if matches!(e.kind, MaskEffectKind::Paint) {
                        let t = match src_paint.iter().find(|(k, _)| *k == old) {
                            Some((_, t)) if same_res => Arc::new(t.duplicate(&engine.device, &mut enc, "mask paint copy")),
                            _ => new_mask_paint(&engine.device, res),
                        };
                        paint.insert(e.id, t);
                    }
                }
                engine.queue.submit(Some(enc.finish()));
                let s = &mut doc.sets[set];
                if let Some(l) = find_mut(&mut s.layers, dst) {
                    l.desc.mask = Some(mask);
                    l.mask_paint = paint;
                    l.rev = next_rev();
                }
                s.selected = Some(dst);
                s.target = Target::Mask;
                s.dirty = true;
            }
            Cmd::ViewMask => {
                let s = &self.doc.as_ref().unwrap().sets[set];
                let same = self.last_select.is_some_and(|(sel, t)| sel == s.selected && t.is_mask());
                self.view.display = if self.view.display == Display::LayerMask && same { Display::Material } else { Display::LayerMask };
                self.doc.as_mut().unwrap().sets[set].dirty = true;
            }
            Cmd::Rename(id, name) => {
                self.checkpoint("Rename layer");
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                if let Some(l) = find_mut(&mut s.layers, id) {
                    l.desc.name = name;
                }
            }
            Cmd::DropItem(row, item) => {
                let s = &mut self.doc.as_mut().unwrap().sets[set];
                // On a row: above it (smart masks apply to it); on the empty
                // area: top of the stack.
                s.selected = row;
                s.target = Target::Content;
                self.apply_item(item);
            }
            Cmd::DropOn(ids, target, place) => {
                self.checkpoint(if ids.len() > 1 { "Move layers" } else { "Move layer" });
                self.doc.as_mut().unwrap().move_layers_onto(set, &ids, target, Some(place));
            }
            Cmd::IdPick(eid) => {
                self.id_pick = Some(eid);
                self.status = "Click the mesh to add its ID color to the selection.".into();
            }
        }
    }
}

fn slot_kind(slot: TexSlot) -> ResKind {
    match slot {
        TexSlot::Alpha => ResKind::Alpha,
        TexSlot::Stencil => ResKind::Stencil,
        _ => ResKind::Texture,
    }
}

/// Something unique enough for default preset names.
pub(super) fn chrono_like_stamp() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{}", t % 1_000_000)
}

pub(super) fn mask_menu(ui: &mut Ui, cmds: &mut Vec<Cmd>) {
    if ui.button("Add white mask").clicked() {
        cmds.push(Cmd::AddMask(true));
        ui.close_menu();
    }
    if ui.button("Add black mask").clicked() {
        cmds.push(Cmd::AddMask(false));
        ui.close_menu();
    }
    super::widgets::sub_menu(ui, "Add effect", |ui| effect_menu(ui, cmds));
    super::widgets::sub_menu(ui, "Apply smart mask", |ui| {
        for m in crate::library::smart_masks() {
            if ui.button(m.name).clicked() {
                cmds.push(Cmd::ApplySmartMask(m.mask));
                ui.close_menu();
            }
        }
        for m in crate::library::user_smart_masks() {
            if ui.button(format!("{} (user)", m.name)).clicked() {
                cmds.push(Cmd::ApplySmartMask(m.mask));
                ui.close_menu();
            }
        }
    });
    if ui.button("Remove mask").clicked() {
        cmds.push(Cmd::RemoveMask);
        ui.close_menu();
    }
    if ui.button("Save mask as smart mask").clicked() {
        cmds.push(Cmd::SaveSmartMask);
        ui.close_menu();
    }
}

pub(super) fn effect_menu(ui: &mut Ui, cmds: &mut Vec<Cmd>) {
    let mut push = |ui: &mut Ui, k: MaskEffectKind| {
        cmds.push(Cmd::AddEffect(k));
        ui.close_menu();
    };
    if ui.button("Paint").clicked() {
        push(ui, MaskEffectKind::Paint);
    }
    super::widgets::sub_menu(ui, "Generator", |ui| {
        for g in Generator::ALL {
            if ui.button(g.name()).clicked() {
                push(ui, MaskEffectKind::Generator(GeneratorDesc::of(*g)));
            }
        }
    });
    super::widgets::sub_menu(ui, "Fill (procedural)", |ui| {
        for p in ProcKind::ALL {
            if ui.button(p.name()).clicked() {
                push(ui, MaskEffectKind::fill_procedural(*p));
            }
        }
    });
    if ui.button("Fill (uniform / texture)").clicked() {
        push(
            ui,
            MaskEffectKind::Fill {
                value: 1.0,
                source: SourceKind::Uniform,
                texture: None,
                procedural: ProcParams::default(),
                projection: Projection::default(),
                range: [0.0, 1.0],
            },
        );
    }
    if ui.button("Levels").clicked() {
        push(ui, MaskEffectKind::levels());
    }
    if ui.button("Blur").clicked() {
        push(ui, MaskEffectKind::Blur { radius: 4.0 });
    }
    if ui.button("Invert").clicked() {
        push(ui, MaskEffectKind::Invert);
    }
    if ui.button("Threshold").clicked() {
        push(ui, MaskEffectKind::Threshold { value: 0.5 });
    }
    if ui.button("Color selection (ID map)").clicked() {
        push(ui, MaskEffectKind::ColorSelect { colors: Vec::new(), tolerance: 0.05 });
    }
}

pub(super) fn short_blend(b: BlendMode) -> &'static str {
    match b {
        BlendMode::Normal => "Norm",
        BlendMode::Replace => "Repl",
        BlendMode::Multiply => "Mul",
        BlendMode::Divide => "Div",
        BlendMode::InverseDivide => "IDiv",
        BlendMode::Darken => "Drk",
        BlendMode::Lighten => "Lght",
        BlendMode::LinearDodge => "Ldge",
        BlendMode::Subtract => "Sub",
        BlendMode::InverseSubtract => "ISub",
        BlendMode::Difference => "Diff",
        BlendMode::Exclusion => "Excl",
        BlendMode::SignedAddition => "AddS",
        BlendMode::Overlay => "Ovrl",
        BlendMode::Screen => "Scrn",
        BlendMode::LinearBurn => "LBrn",
        BlendMode::ColorBurn => "CBrn",
        BlendMode::ColorDodge => "CDdg",
        BlendMode::SoftLight => "SftL",
        BlendMode::HardLight => "HrdL",
        BlendMode::VividLight => "VvdL",
        BlendMode::PinLight => "PinL",
        BlendMode::Saturation => "Sat",
        BlendMode::Color => "Col",
        BlendMode::Value => "Val",
        BlendMode::Hue => "Hue",
        BlendMode::NormalCombine => "NMC",
        BlendMode::NormalDetail => "NMD",
    }
}

pub(super) fn channels_table(ui: &mut Ui, d: &mut LayerDesc) {
    // Sized from the panel width: a fixed layout grew narrow docks.
    let w = ui.available_width();
    let gap = ui.spacing().item_spacing.x;
    let label = (w * 0.34).clamp(44.0, 80.0);
    let num = 40.0;
    let combo = (w - label - num - 2.0 * gap - 2.0 * ui.spacing().button_padding.x - ui.spacing().icon_width - 4.0).max(30.0);
    for c in 0..NCH {
        ui.horizontal(|ui| {
            ui.add_sized([label, 22.0], egui::Label::new(egui::RichText::new(CHANNELS[c]).size(12.0).color(super::widgets::LABEL)).truncate());
            egui::ComboBox::from_id_salt(("blend", d.id, c)).width(combo).selected_text(short_blend(d.blend[c])).show_ui(ui, |ui| {
                for m in BlendMode::ALL {
                    ui.selectable_value(&mut d.blend[c], *m, m.name());
                }
            });
            let mut pct = d.opacity[c] * 100.0;
            if ui.add_sized([num, 20.0], egui::DragValue::new(&mut pct).range(0.0..=100.0).speed(0.5).max_decimals(0)).changed() {
                d.opacity[c] = pct / 100.0;
            }
        });
    }
}

pub(super) fn filter_ui(ui: &mut Ui, id: u64, f: &mut FilterDesc) {
    let before = f.kind;
    super::widgets::combo_row(ui, "Filter", ("fk", id), &mut f.kind, FilterKind::ALL, FilterKind::name);
    if before != f.kind {
        let ch = f.channels;
        *f = FilterDesc::of(f.kind);
        f.channels = ch;
    }
    let p = &mut f.p;
    match f.kind {
        FilterKind::Hsl => {
            slider(ui, "Hue shift", &mut p[0], -0.5..=0.5);
            slider(ui, "Saturation", &mut p[1], 0.0..=2.0);
            slider(ui, "Lightness", &mut p[2], -1.0..=1.0);
        }
        FilterKind::BrightnessContrast => {
            slider(ui, "Brightness", &mut p[0], -1.0..=1.0);
            slider(ui, "Contrast", &mut p[1], 0.0..=3.0);
        }
        FilterKind::Levels => {
            slider(ui, "Input black", &mut p[0], 0.0..=1.0);
            slider(ui, "Input white", &mut p[1], 0.0..=1.0);
            slider(ui, "Gamma", &mut p[2], 0.1..=5.0);
            slider(ui, "Output black", &mut p[3], 0.0..=1.0);
            slider(ui, "Output white", &mut p[4], 0.0..=1.0);
        }
        FilterKind::Posterize => {
            super::widgets::Slider::new("Steps", &mut p[0], 2.0..=32.0).decimals(0).show(ui);
        }
        FilterKind::Blur => slider(ui, "Radius (texels)", &mut p[0], 0.5..=64.0),
        FilterKind::Sharpen => {
            slider(ui, "Amount", &mut p[0], 0.0..=5.0);
            slider(ui, "Radius (texels)", &mut p[1], 0.5..=16.0);
        }
        FilterKind::Tint => {
            slider(ui, "Amount", &mut p[0], 0.0..=1.0);
            ui.label("Color");
            super::widgets::color_bar(ui, ("tint", id), &mut f.color);
        }
        FilterKind::GradientMap => {
            ui.label("Dark");
            super::widgets::color_bar(ui, ("gm dark", id), &mut f.color);
            ui.label("Bright");
            super::widgets::color_bar(ui, ("gm bright", id), &mut f.color2);
        }
        FilterKind::Clamp => {
            slider(ui, "Min", &mut p[0], -1.0..=1.0);
            slider(ui, "Max", &mut p[1], 0.0..=2.0);
        }
        FilterKind::HeightAdjust => {
            slider(ui, "Height scale", &mut p[0], -4.0..=4.0);
            slider(ui, "Height offset", &mut p[1], -1.0..=1.0);
        }
        FilterKind::HistogramScan => {
            slider(ui, "Position", &mut p[0], 0.0..=1.0);
            slider(ui, "Contrast", &mut p[1], 0.0..=1.0);
        }
        FilterKind::Highpass => slider(ui, "Radius (texels)", &mut p[0], 0.5..=64.0),
        FilterKind::ColorCorrect => {
            slider(ui, "Brightness", &mut p[0], -1.0..=1.0);
            slider(ui, "Contrast", &mut p[1], 0.0..=3.0);
            slider(ui, "Gamma", &mut p[2], 0.1..=4.0);
            slider(ui, "Saturation", &mut p[3], 0.0..=2.0);
            slider(ui, "Hue shift", &mut p[4], -0.5..=0.5);
        }
        FilterKind::Mirror => {
            ui.label(egui::RichText::new("Axis").size(12.0).color(super::widgets::LABEL));
            ui.horizontal_wrapped(|ui| {
                for (v, n) in [(0.0, "Horizontal"), (1.0, "Vertical"), (2.0, "Both")] {
                    if ui.selectable_label((p[0] - v).abs() < 0.5, n).clicked() {
                        p[0] = v;
                    }
                }
            });
        }
        FilterKind::Transform => {
            slider(ui, "Offset U", &mut p[0], -1.0..=1.0);
            slider(ui, "Offset V", &mut p[1], -1.0..=1.0);
            slider(ui, "Rotation", &mut p[2], -180.0..=180.0);
            slider(ui, "Scale", &mut p[3], 0.1..=8.0);
        }
        FilterKind::Invert | FilterKind::Desaturate => {}
    }
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Affected channels").size(12.0).color(super::widgets::LABEL));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for c in 0..NCH {
            if super::widgets::chip(ui, CHANNEL_SHORT[c], f.channels[c], CHANNELS[c]).clicked() {
                f.channels[c] = !f.channels[c];
            }
        }
    });
}

/// Short channel names used on chips.
pub(super) const CHANNEL_SHORT: [&str; NCH] = ["color", "rough", "metal", "height", "emiss", "op", "nrm"];
