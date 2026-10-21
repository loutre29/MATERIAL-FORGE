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
    ("S + LMB / MMB / RMB", "Rotate / move / scale the stencil"),
    ("1 2 3 4 5 6", "Paint, eraser, projection, polygon fill, smudge, clone"),
    ("P", "Color picker (temporary, returns to the previous tool)"),
    ("V", "Clone tool: set the source under the cursor"),
    ("[ / ]", "Brush size"),
    ("D", "Lazy mouse on / off"),
    ("X", "Swap the mask grayscale (black / white)"),
    ("C / Shift + C", "Next / previous channel"),
    ("B / Shift + B", "Next / previous mesh map"),
    ("M", "Material view"),
    ("F", "Frame the mesh"),
    ("F1 / F2 / F3 / F4", "3D + 2D / 3D only / 2D only / swap the views"),
    ("F5 / F6", "Perspective / orthographic camera"),
    ("F8 / F9 / F10", "Baking / painting / rendering"),
    ("Tab", "Hide the panels"),
    ("Alt + Q", "Isolate the active texture set"),
    ("Escape", "Close the popup, quick properties, dialog or flyout"),
    ("Ctrl + Z / Ctrl + Y", "Undo / redo"),
    ("Ctrl + C / Ctrl + V", "Copy / paste the selected layer"),
    ("Ctrl + D / Ctrl + G / Del", "Duplicate / group / delete"),
    ("Alt + Up / Down", "Move the selected effect (or layer) up / down"),
    ("Alt + click mask", "Show the mask in the viewport (Shift + click toggles it)"),
    ("Ctrl + S / Ctrl + Shift + S", "Save / save as"),
    ("Ctrl + Shift + E / B", "Export textures / bake mesh maps"),
];

/// A dock opened from the right icon strip: floats next to the strip with
/// an accent border; Escape or the strip icon closes it.
fn flyout<'a>(ctx: &egui::Context, title: &str, open: &'a mut bool) -> egui::Window<'a> {
    egui::Window::new(RichText::new(title.to_uppercase()).font(crate::style::bold(11.5)))
        .id(egui::Id::new(("flyout", title)))
        .open(open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-40.0, 58.0))
        // Taller than the window: scroll inside instead of sliding over the menu bar.
        .vscroll(true)
        .max_height(ctx.screen_rect().height() - 58.0 - 40.0)
        .frame(egui::Frame::window(&ctx.style()).fill(crate::style::PANEL).stroke(egui::Stroke::new(1.0f32, crate::style::ACCENT)))
}

impl RetroApp {
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        self.import_window(ctx);
        self.new_project_window(ctx);
        self.export_window(ctx);
        self.bake_window(ctx);
        self.view_settings_window(ctx);
        self.history_window(ctx);
        if self.show_help {
            egui::Window::new("Shortcuts").open(&mut self.show_help).resizable(false).collapsible(false).show(ctx, |ui| {
                egui::Grid::new("help").num_columns(2).striped(true).show(ui, |ui| {
                    for (k, v) in HELP {
                        ui.strong(*k);
                        ui.label(*v);
                        ui.end_row();
                    }
                });
            });
        }
        if self.show_log {
            flyout(ctx, "Log", &mut self.show_log).show(ctx, |ui| {
                ui.set_width(520.0);
                egui::ScrollArea::vertical().stick_to_bottom(true).max_height(320.0).show(ui, |ui| {
                    for l in &self.log {
                        ui.label(l);
                    }
                });
            });
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
                ui.weak("Each material of the mesh becomes a texture set. UVs are required.");
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
            egui::Window::new("Export textures").open(&mut open).default_width(640.0).vscroll(true).max_height(ctx.screen_rect().height() - 80.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Output folder");
                    ui.add(egui::TextEdit::singleline(&mut d.dir).desired_width(380.0));
                    if ui.button("Browse...").clicked() {
                        if let Some(p) = file_dialog().and_then(|f| f.pick_folder()) {
                            d.dir = p.display().to_string();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Preset");
                    egui::ComboBox::from_id_salt("preset").selected_text(d.presets[d.preset].name.clone()).width(260.0).show_ui(ui, |ui| {
                        for (i, p) in d.presets.iter().enumerate() {
                            ui.selectable_value(&mut d.preset, i, &p.name);
                        }
                    });
                    if ui.button("Duplicate as new").clicked() {
                        let mut p = d.presets[d.preset].clone();
                        p.name = d.new_name.clone();
                        p.builtin = false;
                        d.presets.push(p);
                        d.preset = d.presets.len() - 1;
                    }
                    ui.add(egui::TextEdit::singleline(&mut d.new_name).desired_width(120.0));
                    // User presets can be deleted (two clicks: the file goes too).
                    let cid = egui::Id::new("export preset delete");
                    let armed = ui.data(|m| m.get_temp::<usize>(cid)) == Some(d.preset);
                    let user = !d.presets[d.preset].builtin;
                    let label = if armed { "Confirm delete" } else { "Delete preset" };
                    let tip = if user { "Deletes this preset from your library" } else { "Built-in presets cannot be deleted" };
                    if ui.add_enabled(user, egui::Button::new(label)).on_hover_text(tip).on_disabled_hover_text(tip).clicked() {
                        if armed {
                            let name = d.presets[d.preset].name.clone();
                            match crate::export::delete_user_preset(&crate::library::sub_dir("export"), &name) {
                                Ok(()) => {
                                    d.presets.remove(d.preset);
                                    d.preset = d.preset.min(d.presets.len() - 1);
                                    deleted = Some(format!("Export preset deleted: {name}"));
                                }
                                Err(e) => deleted = Some(format!("Could not delete the preset: {e:#}")),
                            }
                            ui.data_mut(|m| m.remove::<usize>(cid));
                        } else {
                            ui.data_mut(|m| m.insert_temp(cid, d.preset));
                        }
                    }
                });
                let preset = &mut d.presets[d.preset];
                ui.group(|ui| {
                    ui.label(RichText::new("Output maps").strong());
                    let editable = !preset.builtin;
                    if !editable {
                        ui.weak("Built-in preset (duplicate it to edit the maps).");
                    }
                    ui.add_enabled_ui(editable, |ui| {
                        let mut remove = None;
                        egui::Grid::new("maps").num_columns(6).striped(true).show(ui, |ui| {
                            ui.strong("On");
                            ui.strong("Suffix");
                            ui.strong("R / Gray");
                            ui.strong("G");
                            ui.strong("B");
                            ui.strong("A");
                            ui.end_row();
                            for (mi, m) in preset.maps.iter_mut().enumerate() {
                                ui.checkbox(&mut m.enabled, "");
                                ui.add(egui::TextEdit::singleline(&mut m.suffix).desired_width(140.0));
                                for c in 0..4 {
                                    let cur = m.channels.get(c).copied();
                                    let text = cur.map(|s| s.name()).unwrap_or("-");
                                    let mut sel = cur;
                                    egui::ComboBox::from_id_salt(("ch", mi, c)).selected_text(text).width(100.0).show_ui(ui, |ui| {
                                        if c > 0 {
                                            ui.selectable_value(&mut sel, None, "-");
                                        }
                                        for s in Src::ALL {
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
                                if ui.small_button("x").clicked() {
                                    remove = Some(mi);
                                }
                                ui.end_row();
                            }
                        });
                        if let Some(i) = remove {
                            preset.maps.remove(i);
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Add map").clicked() {
                                preset.maps.push(OutputMap { suffix: "Map".into(), channels: vec![Src::Roughness], enabled: true });
                            }
                            save_preset = ui.button("Save preset to library").clicked();
                        });
                    });
                });
                egui::Grid::new("export opts").num_columns(2).show(ui, |ui| {
                    ui.label("Format");
                    egui::ComboBox::from_id_salt("fmt").selected_text(d.opts.format.name()).show_ui(ui, |ui| {
                        for f in FileFormat::ALL {
                            ui.selectable_value(&mut d.opts.format, f, f.name());
                        }
                    });
                    ui.end_row();
                    ui.label("Size");
                    egui::ComboBox::from_id_salt("size").selected_text(match d.opts.size_shift {
                        -2 => "Quarter",
                        -1 => "Half",
                        1 => "Double",
                        _ => "Document resolution",
                    }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.opts.size_shift, -2, "Quarter");
                        ui.selectable_value(&mut d.opts.size_shift, -1, "Half");
                        ui.selectable_value(&mut d.opts.size_shift, 0, "Document resolution");
                        ui.selectable_value(&mut d.opts.size_shift, 1, "Double");
                    });
                    ui.end_row();
                    ui.label("Padding");
                    let mut px = match d.opts.padding {
                        Padding::Dilate(p) => p,
                        _ => 8,
                    };
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut d.opts.padding, Padding::Infinite, "Infinite");
                        if ui.selectable_label(matches!(d.opts.padding, Padding::Dilate(_)), "Dilation").clicked() {
                            d.opts.padding = Padding::Dilate(px);
                        }
                        ui.selectable_value(&mut d.opts.padding, Padding::None, "None (transparent)");
                        if let Padding::Dilate(_) = d.opts.padding {
                            if ui.add(egui::DragValue::new(&mut px).range(0..=256).suffix(" px")).changed() {
                                d.opts.padding = Padding::Dilate(px);
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("File names");
                    ui.add(egui::TextEdit::singleline(&mut d.opts.pattern).desired_width(260.0)).on_hover_text("$mesh, $set and $map are replaced");
                    ui.end_row();
                });
                ui.label("Texture sets");
                ui.horizontal_wrapped(|ui| {
                    for (i, s) in doc.sets.iter().enumerate() {
                        ui.checkbox(&mut d.sets[i], format!("{} ({}px)", s.name, s.resolution()));
                    }
                });
                if ui.button(RichText::new("Export").strong()).clicked() {
                    run = true;
                }
            });
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
            egui::Window::new("Bake mesh maps").open(&mut open).collapsible(false).resizable(false).vscroll(true).max_height(ctx.screen_rect().height() - 80.0).show(ctx, |ui| {
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
                    ui.add(egui::Slider::new(&mut st.ao_samples, 4..=256).logarithmic(true));
                    ui.end_row();
                    ui.label("AO max distance");
                    ui.add(egui::Slider::new(&mut st.ao_distance, 0.005..=1.0).logarithmic(true));
                    ui.end_row();
                    ui.label("Thickness samples");
                    ui.add(egui::Slider::new(&mut st.thickness_samples, 4..=128).logarithmic(true));
                    ui.end_row();
                    ui.label("Thickness distance");
                    ui.add(egui::Slider::new(&mut st.thickness_distance, 0.005..=1.0).logarithmic(true));
                    ui.end_row();
                    ui.label("Cage distance");
                    ui.add(egui::Slider::new(&mut st.cage_distance, 0.001..=0.2).logarithmic(true));
                    ui.end_row();
                    ui.label("Sharp edge width");
                    ui.add(egui::Slider::new(&mut st.edge_width, 0.0005..=0.05).logarithmic(true));
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
                if ui.button(RichText::new("Bake").strong()).clicked() {
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
        flyout(ctx, "Display settings", &mut open).show(ctx, |ui| {
            let v = &mut self.view;
            ui.label(RichText::new("Environment").strong());
            ui.label(format!("Current: {}", self.engine.env.name));
            ui.horizontal(|ui| {
                load_hdr = ui.button("Load HDRI (.hdr / .exr)...").clicked();
                studio = ui.button("Built-in studio").clicked();
            });
            egui::Grid::new("env grid").num_columns(2).show(ui, |ui| {
                ui.label("Exposure (EV)");
                ui.add(egui::Slider::new(&mut v.exposure, -6.0..=6.0));
                ui.end_row();
                ui.label("Rotation");
                ui.add(egui::Slider::new(&mut v.env_rotation, -180.0..=180.0).suffix(" deg"));
                ui.end_row();
                ui.label("Background blur");
                ui.add(egui::Slider::new(&mut v.bg_blur, 0.0..=8.0));
                ui.end_row();
                ui.label("Background intensity");
                ui.add(egui::Slider::new(&mut v.bg_intensity, 0.0..=2.0));
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
                ui.add(egui::Slider::new(&mut v.shadow_strength, 0.0..=1.0));
                ui.end_row();
                ui.label("Key light intensity");
                ui.add(egui::Slider::new(&mut v.light_intensity, 0.0..=5.0));
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
                ui.add(egui::Slider::new(&mut v.bloom, 0.0..=0.5));
                ui.end_row();
                ui.label("Bloom threshold");
                ui.add(egui::Slider::new(&mut v.bloom_threshold, 0.0..=8.0));
                ui.end_row();
                ui.label("Vignette");
                ui.add(egui::Slider::new(&mut v.vignette, 0.0..=1.0));
                ui.end_row();
                ui.label("Saturation");
                ui.add(egui::Slider::new(&mut v.saturation, 0.0..=2.0));
                ui.end_row();
                ui.label("Contrast");
                ui.add(egui::Slider::new(&mut v.contrast, 0.5..=2.0));
                ui.end_row();
                ui.label("Field of view");
                let mut fov = self.camera.fov_y.to_degrees();
                if ui.add(egui::Slider::new(&mut fov, 10.0..=100.0).suffix(" deg")).changed() {
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
                ui.label("UV wireframe (2D)");
                ui.add(egui::Slider::new(&mut v.uv_wire, 0.0..=1.0));
                ui.end_row();
            });
            ui.separator();
            ui.label(RichText::new("Project").strong());
            egui::Grid::new("proj grid").num_columns(2).show(ui, |ui| {
                ui.label("Autosave every (min, 0 = off)");
                ui.add(egui::Slider::new(&mut self.autosave_minutes, 0.0..=60.0));
                ui.end_row();
            });
            ui.separator();
            ui.label(RichText::new("Path-traced render (F10)").strong());
            egui::Grid::new("pt grid").num_columns(2).show(ui, |ui| {
                let pt = &mut self.engine.pathtracer;
                ui.label("Samples per pixel");
                ui.add(egui::Slider::new(&mut pt.max_samples, 16..=8192).logarithmic(true));
                ui.end_row();
                ui.label("Bounces");
                ui.add(egui::Slider::new(&mut pt.bounces, 1..=12));
                ui.end_row();
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
        flyout(ctx, "History", &mut open).show(ctx, |ui| {
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
