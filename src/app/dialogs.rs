//! Floating windows: new project, export, bake, display settings, texture
//! set settings, history, log and help.

use super::*;
use crate::bake::IdSource;
use crate::export::{self, FileFormat, OutputMap, Padding, Src};
use eframe::egui::{Color32, RichText};

const HELP: &[(&str, &str)] = &[
    ("LMB", "Paint / use the tool"),
    ("Shift + LMB", "Straight line from the last stroke (Ctrl + Shift: snapped to 5 degrees)"),
    ("Alt + LMB / MMB / RMB", "Orbit / pan / zoom (wheel zooms too)"),
    ("Ctrl + LMB drag", "Brush flow (horizontal) and angle (vertical)"),
    ("Ctrl + RMB drag", "Brush size (horizontal) and hardness (vertical)"),
    ("Shift + RMB drag", "Rotate the environment"),
    ("Ctrl + Alt + RMB", "Select the texture set under the cursor"),
    ("RMB click", "Quick properties at the cursor"),
    ("T + LMB / MMB / RMB", "Rotate / move / scale the stencil"),
    ("Shift / Ctrl / Shift + Ctrl", "While selecting: add / subtract / intersect"),
    ("Ctrl + A / Ctrl + D", "Select all / deselect"),
    ("Ctrl + Shift + I / Ctrl + Shift + D", "Invert the selection / reselect"),
    ("Shift + Backspace / Delete", "Fill the selection / clear it on the layer"),
    ("Enter / Backspace", "Polygonal lasso: close / remove the last point"),
    ("1 ... 9, 0", "Brush flow 10 % ... 90 %, 100 %"),
    ("V", "Clone tool: set the source under the cursor"),
    ("Tab", "Hide the panels"),
    ("Alt + Q", "Isolate the active texture set"),
    ("Escape", "Cancel the selection being drawn, close the popup, dialog or flyout"),
    ("Ctrl + Z / Ctrl + Y", "Undo / redo"),
    ("Ctrl + C / Ctrl + V", "Copy / paste the selected layer"),
    ("Ctrl + J / Ctrl + G / Del", "Duplicate / group / delete the layer (layer stack)"),
    ("Alt + Up / Down", "Move the selected effect (or layer) up / down"),
    ("Alt + click mask", "Show the mask in the viewport (Shift + click toggles it)"),
    ("Ctrl + S / Ctrl + Shift + S", "Save / save as"),
    ("Ctrl + Shift + E / B", "Export textures / bake mesh maps"),
];

/// A panel opened from a button: hangs under it (`at`, the button's bottom
/// center) with an accent border, or from the top right corner; Escape or
/// the button closes it.
fn flyout<'a>(ctx: &egui::Context, title: &str, open: &'a mut bool, at: Option<egui::Pos2>) -> egui::Window<'a> {
    let w = egui::Window::new(RichText::new(title.to_uppercase()).font(crate::style::bold(11.5)))
        .id(egui::Id::new(("flyout", title)))
        .open(open)
        .collapsible(false)
        .resizable(false)
        // Taller than the window: scroll inside instead of sliding off it.
        .vscroll(true)
        .max_height(ctx.screen_rect().height() - 70.0 - 40.0)
        .frame(egui::Frame::window(&ctx.style()).fill(crate::style::PANEL).stroke(egui::Stroke::new(1.0f32, crate::style::ACCENT)));
    match at {
        Some(p) => w.pivot(egui::Align2::CENTER_TOP).fixed_pos(p + egui::vec2(0.0, 8.0)).constrain(true),
        None => w.anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 54.0)),
    }
}

impl RetroApp {
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        self.import_window(ctx);
        self.new_project_window(ctx);
        self.export_window(ctx);
        self.bake_window(ctx);
        self.uv_window(ctx);
        self.view_settings_window(ctx);
        self.history_window(ctx);
        self.modify_window(ctx);
        self.by_map_window(ctx);
        if self.show_prefs {
            self.preferences_window(ctx);
        }
        if self.show_log {
            flyout(ctx, "Log", &mut self.show_log, None).show(ctx, |ui| {
                ui.set_width(520.0);
                egui::ScrollArea::vertical().stick_to_bottom(true).max_height(320.0).show(ui, |ui| {
                    for l in &self.log {
                        ui.label(l);
                    }
                });
            });
        }
    }

    /// Edit > Preferences (Ctrl+K): the interface options, the editable
    /// single keys, then the fixed shortcuts.
    fn preferences_window(&mut self, ctx: &egui::Context) {
        use super::keymap::{self, Keymap, ACTIONS};
        let mut open = true;
        egui::Window::new("Preferences")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .vscroll(true)
            .max_height(ctx.screen_rect().height() - 120.0)
            .show(ctx, |ui| {
                ui.label(RichText::new("Interface").strong());
                ui.checkbox(&mut self.view.show_tool_keys, "Show the tool's shortcuts in the view (bottom left)");
                ui.separator();
                ui.label(RichText::new("Shortcuts").strong());
                ui.label(RichText::new("Click a key to change it, then press the new one (Shift allowed, Escape cancels).").size(11.5).color(crate::style::TEXT3));
                ui.add_space(4.0);
                let mut group = "";
                egui::Grid::new("keymap").num_columns(3).striped(true).show(ui, |ui| {
                    for (a, g, what, _) in ACTIONS {
                        if *g != group {
                            group = g;
                            ui.label(RichText::new(*g).strong().color(crate::style::ACCENT));
                            ui.end_row();
                        }
                        ui.label(*what);
                        let capturing = self.keymap_capture == Some(*a);
                        let text = if capturing {
                            "Press a key...".to_string()
                        } else {
                            Some(self.keymap.label(*a)).filter(|l| !l.is_empty()).unwrap_or_else(|| "none".into())
                        };
                        let b = ui.add(egui::Button::new(RichText::new(text).monospace()).selected(capturing).min_size(egui::vec2(120.0, 20.0)));
                        super::mark(ctx, format!("key:{what}"), b.rect);
                        if b.clicked() {
                            self.keymap_capture = if capturing { None } else { Some(*a) };
                        }
                        if ui.small_button("x").on_hover_text("No key for this").clicked() {
                            self.keymap.set(*a, None);
                            self.keymap.save();
                        }
                        ui.end_row();
                    }
                });
                if ui.button("Restore the default keys").clicked() {
                    self.keymap = Keymap::default();
                    self.keymap.save();
                }
                ui.separator();
                ui.label(RichText::new("Fixed shortcuts").strong());
                egui::Grid::new("help").num_columns(2).striped(true).show(ui, |ui| {
                    for (k, v) in HELP {
                        ui.strong(*k);
                        ui.label(*v);
                        ui.end_row();
                    }
                });
            });
        if let Some(a) = self.keymap_capture {
            if let Some(c) = ctx.input(keymap::captured) {
                // The key acts on nothing else this frame.
                ctx.input_mut(|i| i.consume_key(egui::Modifiers { shift: c.shift, ..Default::default() }, c.key));
                self.status = match self.keymap.set(a, Some(c)) {
                    Some(o) => format!("{}: {}. It was the key of {}, which has none now.", keymap::name(a), c.label(), keymap::name(o)),
                    None => format!("{}: {}", keymap::name(a), c.label()),
                };
                self.keymap.save();
                self.keymap_capture = None;
            }
        }
        if !open {
            self.show_prefs = false;
            self.keymap_capture = None;
        }
    }

    fn new_project_window(&mut self, ctx: &egui::Context) {
        let mut create: Option<(PathBuf, u32)> = None;
        if let Some(d) = &mut self.new_dialog {
            let mut open = true;
            egui::Window::new("New project").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Mesh");
                    ui.add(egui::TextEdit::singleline(&mut d.path).desired_width(360.0));
                    if ui.button("Browse...").clicked() {
                        if let Some(p) = file_dialog().and_then(|f| f.add_filter("Mesh", &["obj", "fbx", "gltf", "glb"]).pick_file()) {
                            d.path = p.display().to_string();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Document resolution");
                    egui::ComboBox::from_id_salt("res").selected_text(format!("{}", d.res)).show_ui(ui, |ui| {
                        for r in [256, 512, 1024, 2048, 4096] {
                            ui.selectable_value(&mut d.res, r, format!("{r}"));
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("UVs");
                    egui::ComboBox::from_id_salt("auto uv").width(260.0).selected_text(d.auto_uv.name()).show_ui(ui, |ui| {
                        for m in crate::mesh::AutoUv::CHOICES {
                            ui.selectable_value(&mut d.auto_uv, m, m.name());
                        }
                    });
                });
                ui.weak("Each material of the mesh becomes a texture set. Meshes without UVs are unwrapped automatically.");
                if let Some(e) = &d.error {
                    ui.colored_label(Color32::from_rgb(240, 90, 80), e);
                }
                if ui.add_enabled(!d.path.trim().is_empty(), egui::Button::new("Create")).clicked() {
                    create = Some((PathBuf::from(d.path.trim().trim_matches('"')), d.res));
                }
            });
            if !open {
                self.new_dialog = None;
            }
        }
        if let Some((path, res)) = create {
            if let Some(d) = &self.new_dialog {
                self.new_auto_uv = d.auto_uv;
            }
            if self.ask_discard(Discard::Create(path.clone(), res)) {
                self.new_dialog = None;
                return;
            }
            match self.create_project(path, res) {
                Ok(()) => self.new_dialog = None,
                Err(e) => {
                    if let Some(d) = &mut self.new_dialog {
                        d.error = Some(format!("{e:#}"));
                    }
                }
            }
        }
    }

    fn export_window(&mut self, ctx: &egui::Context) {
        let mut run = false;
        let mut save_preset = false;
        let Some(doc) = &self.doc else { return };
        if let Some(d) = &mut self.export_dialog {
            let mut open = true;
            let mut deleted: Option<String> = None;
            let mut cancel = false;
            egui::Window::new(RichText::new("EXPORT TEXTURES").font(crate::style::bold(11.5)))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .fixed_size(egui::vec2(560.0, 0.0))
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                let label = |ui: &mut egui::Ui, t: &str| {
                    ui.add_sized([96.0, 22.0], egui::Label::new(RichText::new(t).size(12.0).color(Color32::from_gray(170))));
                };
                // Where.
                ui.horizontal(|ui| {
                    label(ui, "Destination");
                    ui.add(egui::TextEdit::singleline(&mut d.dir).desired_width(360.0).hint_text("Folder for the textures"));
                    if ui.button("Browse...").clicked() {
                        if let Some(p) = file_dialog().and_then(|f| f.pick_folder()) {
                            d.dir = p.display().to_string().replace(r"\\?\", "");
                        }
                    }
                });
                // What: the preset (built-in ones are ready to use).
                ui.horizontal(|ui| {
                    label(ui, "Preset");
                    egui::ComboBox::from_id_salt("preset").selected_text(d.presets[d.preset].name.clone()).width(250.0).show_ui(ui, |ui| {
                        for (i, p) in d.presets.iter().enumerate() {
                            let tag = if p.builtin { "" } else { "  (custom)" };
                            ui.selectable_value(&mut d.preset, i, format!("{}{tag}", p.name));
                        }
                    });
                    if ui.button("New custom preset").on_hover_text("An editable copy of this preset").clicked() {
                        let mut p = d.presets[d.preset].clone();
                        let n = d.presets.iter().filter(|p| !p.builtin).count() + 1;
                        p.name = format!("Custom {n}");
                        p.builtin = false;
                        d.presets.push(p);
                        d.preset = d.presets.len() - 1;
                    }
                });
                ui.horizontal(|ui| {
                    label(ui, "Texture sets");
                    ui.horizontal_wrapped(|ui| {
                        for (i, s) in doc.sets.iter().enumerate() {
                            ui.checkbox(&mut d.sets[i], format!("{}  ({} px)", s.name, s.resolution()));
                        }
                    });
                });
                ui.horizontal(|ui| {
                    label(ui, "Format");
                    egui::ComboBox::from_id_salt("fmt").selected_text(d.opts.format.name()).width(120.0).show_ui(ui, |ui| {
                        for f in FileFormat::ALL {
                            ui.selectable_value(&mut d.opts.format, f, f.name());
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new("Size").size(12.0).color(Color32::from_gray(170)));
                    egui::ComboBox::from_id_salt("size")
                        .selected_text(match d.opts.size_shift {
                            -2 => "1/4",
                            -1 => "1/2",
                            1 => "x2",
                            _ => "Document",
                        })
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.opts.size_shift, -2, "1/4");
                            ui.selectable_value(&mut d.opts.size_shift, -1, "1/2");
                            ui.selectable_value(&mut d.opts.size_shift, 0, "Document");
                            ui.selectable_value(&mut d.opts.size_shift, 1, "x2");
                        });
                    ui.add_space(8.0);
                    ui.label(RichText::new("Padding").size(12.0).color(Color32::from_gray(170)));
                    let px = match d.opts.padding {
                        Padding::Dilate(p) => p,
                        _ => 8,
                    };
                    egui::ComboBox::from_id_salt("pad")
                        .selected_text(match d.opts.padding {
                            Padding::Infinite => "Infinite".to_string(),
                            Padding::Dilate(p) => format!("{p} px"),
                            Padding::None => "None".to_string(),
                        })
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.opts.padding, Padding::Infinite, "Infinite");
                            for p in [2u32, 4, 8, 16, 32, 64] {
                                ui.selectable_value(&mut d.opts.padding, Padding::Dilate(p), format!("{p} px"));
                            }
                            ui.selectable_value(&mut d.opts.padding, Padding::None, "None (transparent)");
                        });
                    let _ = px;
                });
                egui::CollapsingHeader::new(RichText::new("Advanced").size(12.0)).id_salt("export advanced").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        label(ui, "File names");
                        ui.add(egui::TextEdit::singleline(&mut d.opts.pattern).desired_width(300.0)).on_hover_text("$mesh, $set and $map are replaced");
                    });
                });
                // Custom presets only: their maps, editable.
                let preset_builtin = d.presets[d.preset].builtin;
                if !preset_builtin {
                    ui.separator();
                    ui.horizontal(|ui| {
                        label(ui, "Name");
                        ui.add(egui::TextEdit::singleline(&mut d.presets[d.preset].name).desired_width(220.0));
                        save_preset = ui.button("Save to library").clicked();
                        // Two clicks: the preset file goes too.
                        let cid = egui::Id::new("export preset delete");
                        let armed = ui.data(|m| m.get_temp::<usize>(cid)) == Some(d.preset);
                        if ui.button(if armed { "Confirm delete" } else { "Delete" }).clicked() {
                            if armed {
                                let name = d.presets[d.preset].name.clone();
                                let _ = crate::export::delete_user_preset(&crate::library::sub_dir("export"), &name);
                                d.presets.remove(d.preset);
                                d.preset = d.preset.min(d.presets.len() - 1);
                                deleted = Some(format!("Export preset deleted: {name}"));
                                ui.data_mut(|m| m.remove::<usize>(cid));
                            } else {
                                ui.data_mut(|m| m.insert_temp(cid, d.preset));
                            }
                        }
                    });
                    if !d.presets[d.preset].builtin {
                        let preset = &mut d.presets[d.preset];
                        let mut remove = None;
                        egui::ScrollArea::vertical().max_height(220.0).id_salt("export maps").show(ui, |ui| {
                            egui::Grid::new("maps").num_columns(7).striped(true).spacing([6.0, 4.0]).show(ui, |ui| {
                                for t in ["", "File suffix", "R / Gray", "G", "B", "A", ""] {
                                    ui.label(RichText::new(t).size(11.0).color(Color32::from_gray(150)));
                                }
                                ui.end_row();
                                for (mi, m) in preset.maps.iter_mut().enumerate() {
                                    ui.checkbox(&mut m.enabled, "");
                                    ui.add(egui::TextEdit::singleline(&mut m.suffix).desired_width(110.0));
                                    for c in 0..4 {
                                        let cur = m.channels.get(c).copied();
                                        let mut sel = cur;
                                        egui::ComboBox::from_id_salt(("ch", mi, c)).selected_text(cur.map(|s| s.name()).unwrap_or("-")).width(78.0).show_ui(ui, |ui| {
                                            if c > 0 {
                                                ui.selectable_value(&mut sel, None, "-");
                                            }
                                            for s in Src::all() {
                                                ui.selectable_value(&mut sel, Some(s), s.name());
                                            }
                                        });
                                        if sel != cur {
                                            match sel {
                                                Some(s) => {
                                                    while m.channels.len() <= c {
                                                        m.channels.push(Src::Zero);
                                                    }
                                                    m.channels[c] = s;
                                                }
                                                None => m.channels.truncate(c),
                                            }
                                        }
                                    }
                                    if ui.small_button("x").on_hover_text("Remove this map").clicked() {
                                        remove = Some(mi);
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                        if let Some(i) = remove {
                            preset.maps.remove(i);
                        }
                        if ui.button("Add map").clicked() {
                            preset.maps.push(OutputMap { suffix: "Map".into(), channels: vec![Src::Roughness], enabled: true });
                        }
                    }
                }
                // Summary and actions.
                ui.separator();
                let sets = d.sets.iter().filter(|s| **s).count();
                let maps = d.presets[d.preset].maps.iter().filter(|m| m.enabled && !m.channels.is_empty()).count();
                ui.horizontal(|ui| {
                    let dir = if d.dir.trim().is_empty() { "(choose a folder)".to_string() } else { d.dir.trim().to_string() };
                    ui.label(RichText::new(format!("{} files  ->  {dir}", sets * maps)).size(11.5).color(Color32::from_gray(150)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let ok = sets > 0 && maps > 0 && !d.dir.trim().is_empty();
                        if ui.add_enabled(ok, egui::Button::new(RichText::new("Export").strong()).fill(crate::style::ACCENT).min_size(egui::vec2(96.0, 28.0))).clicked() {
                            run = true;
                        }
                        if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(80.0, 28.0))).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
            if cancel {
                open = false;
            }
            if !open {
                self.export_dialog = None;
            }
            if let Some(m) = deleted {
                self.message(m);
            }
        }
        if save_preset {
            if let Some(d) = &self.export_dialog {
                let p = &d.presets[d.preset];
                match export::save_user_preset(&crate::library::sub_dir("export"), p) {
                    Ok(path) => self.message(format!("Export preset saved: {}", path.display())),
                    Err(e) => self.message(format!("Could not save the preset: {e:#}")),
                }
            }
        }
        if run {
            self.run_export();
        }
    }

    fn run_export(&mut self) {
        let (Some(d), Some(doc)) = (&self.export_dialog, &self.doc) else { return };
        let dir = PathBuf::from(d.dir.trim().trim_matches('"'));
        let preset = d.presets[d.preset].clone();
        let opts = d.opts.clone();
        let mut written = 0;
        let mut errors = Vec::new();
        // Two maps landing on one file name would overwrite each other.
        let mut seen = std::collections::HashMap::new();
        for (i, s) in doc.sets.iter().enumerate().filter(|(i, _)| d.sets[*i]) {
            for p in export::planned_paths(&dir, &doc.mesh_name, &s.name, &preset, &opts) {
                let key = p.to_string_lossy().to_lowercase();
                if let Some(other) = seen.insert(key, (i, s.name.clone())) {
                    let msg = if other.0 == i {
                        format!("two maps of \"{}\" would both be written to {}: give the outputs different suffixes", s.name, p.display())
                    } else {
                        format!("\"{}\" and \"{}\" would both be written to {}: add $set to the file name pattern or rename a texture set", other.1, s.name, p.display())
                    };
                    self.message(format!("Export cancelled: {msg}"));
                    return;
                }
            }
        }
        for (i, s) in doc.sets.iter().enumerate() {
            if !d.sets[i] {
                continue;
            }
            let mut maps = export::read_maps(&self.engine, doc, i);
            maps.keep = export::padding_mask(s, opts.padding);
            match export::export_set(&maps, &dir, &doc.mesh_name, &s.name, &preset, &opts) {
                Ok(files) => written += files.len(),
                Err(e) => errors.push(format!("{}: {e:#}", s.name)),
            }
        }
        if errors.is_empty() {
            self.message(format!("Exported {written} texture(s) to {}", dir.display()));
            self.export_dialog = None;
        } else {
            for e in errors {
                self.message(format!("Export failed: {e}"));
            }
        }
    }

    fn bake_window(&mut self, ctx: &egui::Context) {
        let mut bake: Option<(BakeSettings, Vec<bool>)> = None;
        let Some(doc) = &self.doc else { return };
        if let Some(d) = &mut self.bake_dialog {
            let mut open = true;
            egui::Window::new(RichText::new("BAKE MESH MAPS").font(crate::style::bold(11.5))).open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("High poly (optional)");
                    ui.add(egui::TextEdit::singleline(&mut d.high).desired_width(280.0));
                    if ui.button("Browse...").clicked() {
                        if let Some(p) = file_dialog().and_then(|f| f.add_filter("Mesh", &["obj", "fbx", "gltf", "glb"]).pick_file()) {
                            d.high = p.display().to_string();
                        }
                    }
                });
                let st = &mut d.settings;
                egui::Grid::new("bake grid").num_columns(2).show(ui, |ui| {
                    ui.label("Antialiasing");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut st.antialias, 1, "None");
                        ui.selectable_value(&mut st.antialias, 2, "2x2");
                        ui.selectable_value(&mut st.antialias, 4, "4x4");
                    });
                    ui.end_row();
                    ui.label("AO samples");
                    super::widgets::slide(ui, &mut st.ao_samples, 4..=256, true, "");
                    st.ao_samples = st.ao_samples.min(16384);
                    ui.end_row();
                    ui.label("AO max distance");
                    super::widgets::slide(ui, &mut st.ao_distance, 0.005..=1.0, true, "");
                    ui.end_row();
                    ui.label("Thickness samples");
                    super::widgets::slide(ui, &mut st.thickness_samples, 4..=128, true, "");
                    st.thickness_samples = st.thickness_samples.min(16384);
                    ui.end_row();
                    ui.label("Thickness distance");
                    super::widgets::slide(ui, &mut st.thickness_distance, 0.005..=1.0, true, "");
                    ui.end_row();
                    ui.label("Cage distance");
                    super::widgets::slide(ui, &mut st.cage_distance, 0.001..=0.2, true, "");
                    ui.end_row();
                    ui.label("Sharp edge width");
                    super::widgets::slide(ui, &mut st.edge_width, 0.0005..=0.05, true, "");
                    ui.end_row();
                    ui.label("Match high poly by name");
                    ui.checkbox(&mut st.match_by_name, "").on_hover_text("bolt_low takes details only from bolt_high");
                    ui.end_row();
                    ui.label("ID map source");
                    egui::ComboBox::from_id_salt("idsrc").selected_text(st.id_source.name()).show_ui(ui, |ui| {
                        for s in IdSource::ALL {
                            ui.selectable_value(&mut st.id_source, s, s.name());
                        }
                    });
                    ui.end_row();
                });
                ui.label("Texture sets");
                ui.horizontal_wrapped(|ui| {
                    for (i, s) in doc.sets.iter().enumerate() {
                        ui.checkbox(&mut d.sets[i], &s.name);
                    }
                });
                ui.weak("Distances are fractions of the mesh bounding-box diagonal.");
                ui.weak("Without a high poly the normal map stays flat; the other maps come from the mesh itself.");
                if ui.add(egui::Button::new(RichText::new("Bake").strong().color(Color32::WHITE)).fill(crate::style::ACCENT).min_size(egui::vec2(96.0, 30.0))).clicked() {
                    let mut s = st.clone();
                    let h = d.high.trim().trim_matches('"');
                    s.high_poly = if h.is_empty() { None } else { Some(PathBuf::from(h)) };
                    bake = Some((s, d.sets.clone()));
                }
            });
            if !open {
                self.bake_dialog = None;
            }
        }
        if let Some((s, sets)) = bake {
            self.bake_dialog = None;
            self.start_bake(s, sets);
        }
    }

    fn view_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_view_settings {
            return;
        }
        let mut open = true;
        let mut load_hdr = false;
        let mut studio = false;
        flyout(ctx, "Display settings", &mut open, self.flyout_at[0]).show(ctx, |ui| {
            let v = &mut self.view;
            ui.label(RichText::new("Environment").strong());
            ui.label(format!("Current: {}", self.engine.env.name));
            ui.horizontal(|ui| {
                load_hdr = ui.button("Load HDRI (.hdr / .exr)...").clicked();
                studio = ui.button("Built-in studio").clicked();
            });
            egui::Grid::new("env grid").num_columns(2).show(ui, |ui| {
                ui.label("Exposure (EV)");
                super::widgets::slide(ui, &mut v.exposure, -6.0..=6.0, false, "");
                ui.end_row();
                ui.label("Rotation");
                super::widgets::slide_dec(ui, &mut v.env_rotation, -180.0..=180.0, 0, " deg");
                ui.end_row();
                ui.label("Background blur");
                super::widgets::slide(ui, &mut v.bg_blur, 0.0..=8.0, false, "");
                ui.end_row();
                ui.label("Background intensity");
                super::widgets::slide(ui, &mut v.bg_intensity, 0.0..=2.0, false, "");
                ui.end_row();
                ui.label("Flat background");
                ui.checkbox(&mut v.flat_bg, "");
                ui.end_row();
            });
            ui.separator();
            ui.label(RichText::new("Lighting").strong());
            egui::Grid::new("light grid").num_columns(2).show(ui, |ui| {
                ui.label("Shadows");
                ui.checkbox(&mut v.shadows, "");
                ui.end_row();
                ui.label("Shadow strength");
                super::widgets::slide(ui, &mut v.shadow_strength, 0.0..=1.0, false, "");
                ui.end_row();
                ui.label("Key light intensity");
                super::widgets::slide(ui, &mut v.light_intensity, 0.0..=5.0, false, "");
                ui.end_row();
            });
            ui.separator();
            ui.label(RichText::new("Camera / post").strong());
            egui::Grid::new("post grid").num_columns(2).show(ui, |ui| {
                ui.label("Tone mapping");
                egui::ComboBox::from_id_salt("tm").selected_text(["ACES", "Filmic", "AgX-like", "Linear (clamp)"][v.tonemap as usize]).show_ui(ui, |ui| {
                    for (i, n) in ["ACES", "Filmic", "AgX-like", "Linear (clamp)"].iter().enumerate() {
                        ui.selectable_value(&mut v.tonemap, i as u32, *n);
                    }
                });
                ui.end_row();
                ui.label("Bloom");
                super::widgets::slide(ui, &mut v.bloom, 0.0..=0.5, false, "");
                ui.end_row();
                ui.label("Bloom threshold");
                super::widgets::slide(ui, &mut v.bloom_threshold, 0.0..=8.0, false, "");
                ui.end_row();
                ui.label("Vignette");
                super::widgets::slide(ui, &mut v.vignette, 0.0..=1.0, false, "");
                ui.end_row();
                ui.label("Saturation");
                super::widgets::slide(ui, &mut v.saturation, 0.0..=2.0, false, "");
                ui.end_row();
                ui.label("Contrast");
                super::widgets::slide(ui, &mut v.contrast, 0.5..=2.0, false, "");
                ui.end_row();
                ui.label("Field of view");
                let mut fov = self.camera.fov_y.to_degrees();
                if super::widgets::slide_dec(ui, &mut fov, 10.0..=100.0, 0, " deg") {
                    self.camera.fov_y = fov.to_radians();
                }
                ui.end_row();
            });
            ui.separator();
            ui.label(RichText::new("Views").strong());
            egui::Grid::new("views grid").num_columns(2).show(ui, |ui| {
                ui.label("Wireframe (3D)");
                ui.checkbox(&mut v.wireframe, "");
                ui.end_row();
                ui.label("Tool shortcuts in the view");
                ui.checkbox(&mut v.show_tool_keys, "");
                ui.end_row();
                ui.label("UV wireframe (2D)");
                super::widgets::slide(ui, &mut v.uv_wire, 0.0..=1.0, false, "");
                ui.end_row();
                ui.label("Wireframe opacity (3D)");
                super::widgets::slide(ui, &mut v.wire_opacity, 0.0..=1.0, false, "");
                ui.end_row();
                ui.label("Wireframe color");
                ui.vertical(|ui| {
                    ui.set_width(180.0);
                    super::widgets::color_bar_named(ui, "wire color", "Wireframe", &mut v.wire_color);
                });
                ui.end_row();
            });
            ui.separator();
            ui.label(RichText::new("Project").strong());
            egui::Grid::new("proj grid").num_columns(2).show(ui, |ui| {
                ui.label("Autosave every (min, 0 = off)");
                super::widgets::slide(ui, &mut self.autosave_minutes, 0.0..=60.0, false, "");
                ui.end_row();
            });
            ui.separator();
            ui.label(RichText::new("Path-traced render (F10)").strong());
            egui::Grid::new("pt grid").num_columns(2).show(ui, |ui| {
                let pt = &mut self.engine.pathtracer;
                ui.label("Samples per pixel");
                super::widgets::slide(ui, &mut pt.max_samples, 16..=8192, true, "");
                ui.end_row();
                ui.label("Bounces");
                super::widgets::slide(ui, &mut pt.bounces, 1..=12, false, "");
                pt.bounces = pt.bounces.min(64);
                ui.end_row();
                ui.label("Denoiser");
                // Intel Open Image Denoise, or the raw (noisy) render.
                let cur = pt.denoise_on as usize;
                let names = ["Off (noisy render)", "Intel Open Image Denoise"];
                let mut pick = cur;
                egui::ComboBox::from_id_salt("pt denoiser").width(220.0).selected_text(names[cur]).show_ui(ui, |ui| {
                    for (i, n) in names.iter().enumerate() {
                        ui.selectable_value(&mut pick, i, *n);
                    }
                });
                if pick != cur {
                    pt.denoise_on = pick > 0;
                    pt.reset();
                }
                ui.end_row();
                if pt.denoise_on {
                    ui.label("");
                    ui.label(RichText::new(format!("Open Image Denoise: {}", pt.oidn_status())).size(11.5).color(crate::style::TEXT3));
                    ui.end_row();
                }
            });
        });
        self.show_view_settings = open;
        if load_hdr {
            if let Some(p) = file_dialog().and_then(|f| f.add_filter("HDR image", crate::library::ENV_EXTS).pick_file()) {
                self.load_environment(&p);
            }
        }
        if studio {
            let e = crate::engine::env::Environment::studio(&self.engine.device, &self.engine.queue);
            self.engine.set_environment(e);
        }
    }

    fn history_window(&mut self, ctx: &egui::Context) {
        if !self.show_history {
            return;
        }
        let mut open = true;
        let mut steps: i32 = 0;
        flyout(ctx, "History", &mut open, self.flyout_at[1]).show(ctx, |ui| {
            ui.set_width(260.0);
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                let n = self.history.undo.len();
                if ui.selectable_label(n == 0, "<empty>").clicked() {
                    steps = -(n as i32);
                }
                for (i, e) in self.history.undo.iter().enumerate() {
                    let current = i + 1 == n;
                    if ui.selectable_label(current, e.label()).clicked() {
                        steps = -((n - 1 - i) as i32);
                    }
                }
                for (i, e) in self.history.redo.iter().rev().enumerate() {
                    if ui.selectable_label(false, RichText::new(e.label()).weak()).clicked() {
                        steps = i as i32 + 1;
                    }
                }
            });
        });
        self.show_history = open;
        for _ in 0..(-steps).max(0) {
            self.undo();
        }
        for _ in 0..steps.max(0) {
            self.redo();
        }
    }
}

impl RetroApp {
    /// Import resources: the files with their type, and where they go.
    fn import_window(&mut self, ctx: &egui::Context) {
        let has_doc = self.doc.is_some();
        let Some(d) = &mut self.import_dialog else { return };
        let mut open = true;
        let (mut ok, mut cancel) = (false, false);
        egui::Window::new("Import resources").open(&mut open).collapsible(false).resizable(false).default_width(460.0).show(ctx, |ui| {
            let mut remove = None;
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                egui::Grid::new("import files").num_columns(3).striped(true).show(ui, |ui| {
                    for (i, (f, kind)) in d.files.iter_mut().enumerate() {
                        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        ui.add(egui::Label::new(name).truncate()).on_hover_text(f.display().to_string());
                        egui::ComboBox::from_id_salt(("import kind", i)).selected_text(kind.name()).width(150.0).show_ui(ui, |ui| {
                            for k in [ImportKind::Texture, ImportKind::Alpha, ImportKind::Environment] {
                                ui.selectable_value(kind, k, k.name());
                            }
                        });
                        if icons::button(ui, icons::Icon::Close, false, 18.0, "Leave this file out").clicked() {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
            });
            if let Some(i) = remove {
                d.files.remove(i);
            }
            ui.add_space(6.0);
            ui.label(RichText::new("Import your resources to:").strong());
            ui.add_enabled_ui(has_doc, |ui| {
                ui.radio_value(&mut d.to_library, false, "This project (saved with the project file)");
            });
            ui.radio_value(&mut d.to_library, true, "Your library (available in every project)");
            if !has_doc {
                d.to_library = true;
                ui.weak("No project is open: the files go to the library.");
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let b = ui.add_enabled(!d.files.is_empty(), egui::Button::new(RichText::new("Import").strong()));
                super::mark(ui.ctx(), "import:ok", b.rect);
                ok = b.clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if ok {
            self.confirm_import();
        } else if cancel || !open {
            self.import_dialog = None;
        }
    }
}
