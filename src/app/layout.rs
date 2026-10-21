//! Window chrome: the icon tool strip, the right dock icon column, the
//! context toolbar above the viewport, per-view display menus and the
//! status bar.

use super::icons::{self, Icon};
use super::*;
use super::widgets;
use eframe::egui::{Color32, Pos2, RichText, Sense, Ui};

pub(super) const CONTEXT_BAR_H: f32 = 40.0;

impl RetroApp {
    pub(super) fn tool_strip(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("toolstrip")
            .exact_width(36.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(crate::style::HEADER).inner_margin(egui::Margin::symmetric(3.0, 6.0)))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                let projection = self.tool == Tool::Paint && self.stencil.enabled;
                let tools = [
                    (Tool::Paint, Icon::Brush, "Paint (1)", self.tool == Tool::Paint && !projection),
                    (Tool::Erase, Icon::Eraser, "Eraser (2)", self.tool == Tool::Erase),
                    (Tool::Paint, Icon::Projection, "Projection (3): paint through a stencil", projection),
                    (Tool::PolygonFill, Icon::PolyFill, "Polygon fill (4)", self.tool == Tool::PolygonFill),
                    (Tool::Smudge, Icon::Smudge, "Smudge (5)", self.tool == Tool::Smudge),
                    (Tool::Clone, Icon::Clone, "Clone (6): V over the mesh sets the source", self.tool == Tool::Clone),
                    (Tool::Picker, Icon::Picker, "Color picker (P)", self.tool == Tool::Picker),
                    (Tool::Hide, Icon::Hide, "Geometry mask (8): click hides, Ctrl + click isolates", self.tool == Tool::Hide),
                ];
                for (i, (t, icon, tip, on)) in tools.into_iter().enumerate() {
                    if icons::button(ui, icon, on, 30.0, tip).clicked() {
                        self.tool = t;
                        // The projection entry is the paint tool with the stencil on.
                        self.stencil.enabled = i == 2;
                        self.prev_tool = None;
                    }
                }
                ui.add_space(4.0);
                let (r, _) = ui.allocate_exact_size(egui::vec2(30.0, 1.0), Sense::hover());
                ui.painter().line_segment([r.left_center(), r.right_center()], egui::Stroke::new(1.0f32, Color32::from_gray(70)));
                ui.add_space(4.0);
                let has = self.doc.is_some();
                if icons::button(ui, Icon::Bake, false, 30.0, "Bake mesh maps").clicked() && has {
                    self.open_bake_dialog();
                }
                if icons::button(ui, Icon::Export, self.export_dialog.is_some(), 30.0, "Export textures (Ctrl+Shift+E)").clicked() && has {
                    self.open_export();
                }
                let label = if self.view.path_trace { format!("Rendering: {} spp (F10)", self.engine.pathtracer.frame) } else { "Render mode (F10)".into() };
                if icons::button(ui, Icon::Render, self.view.path_trace, 30.0, &label).clicked() {
                    self.view.path_trace = !self.view.path_trace;
                }
            });
    }

    /// Far right column of icons opening the secondary panels.
    pub(super) fn dock_bar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("dockbar")
            .exact_width(32.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(crate::style::HEADER).inner_margin(egui::Margin::symmetric(3.0, 6.0)))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                if icons::button(ui, Icon::Gear, self.show_view_settings, 26.0, "Display settings").clicked() {
                    self.flip_flyout(0);
                }
                if icons::button(ui, Icon::Sun, self.view.shadows, 26.0, "Shadows").clicked() {
                    self.view.shadows = !self.view.shadows;
                }
                let on = self.tab_visible(super::dock::Tab::SetSettings);
                let b = icons::button(ui, Icon::Sphere, on, 26.0, "Texture set settings");
                super::mark(ui.ctx(), "dockbar:setsettings", b.rect);
                if b.clicked() {
                    self.toggle_tab(super::dock::Tab::SetSettings);
                }
                if icons::button(ui, Icon::History, self.show_history, 26.0, "History").clicked() {
                    self.flip_flyout(1);
                }
                if icons::button(ui, Icon::Log, self.show_log, 26.0, "Log").clicked() {
                    self.flip_flyout(2);
                }
                ui.add_space(8.0);
                for (tab, icon) in [
                    (super::dock::Tab::Assets, Icon::Grid),
                    (super::dock::Tab::Properties, Icon::Layer),
                    (super::dock::Tab::TextureSets, Icon::List),
                    (super::dock::Tab::Layers, Icon::Mask),
                ] {
                    if icons::button(ui, icon, self.tab_visible(tab), 26.0, tab.name()).clicked() {
                        self.toggle_tab(tab);
                    }
                }
            });
    }

    pub(super) fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .exact_height(22.0)
            .frame(egui::Frame::none().fill(crate::style::BG).inner_margin(egui::Margin::symmetric(8.0, 2.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if let Some(job) = &self.bake_job {
                        let f = job.progress.load(Ordering::Relaxed) as f32 / job.total.max(1) as f32;
                        ui.add(egui::ProgressBar::new(f).desired_width(160.0).desired_height(14.0).text(format!("Baking {:.0}%", f * 100.0)));
                        if ui.small_button("Cancel").clicked() {
                            job.cancel.store(true, Ordering::Relaxed);
                        }
                    }
                    let error = self.status.starts_with("Could not") || self.status.contains("failed");
                    let c = if error { Color32::from_rgb(235, 90, 80) } else { Color32::from_gray(175) };
                    ui.label(RichText::new(&self.status).size(11.0).color(c));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let dt = ctx.input(|i| i.unstable_dt).max(1e-4);
                        let mut parts = Vec::new();
                        if dt < 0.5 {
                            parts.push(format!("{:.0} fps", 1.0 / dt));
                        }
                        if let Some(doc) = &self.doc {
                            parts.push(format!("{} tris", doc.mesh.triangle_count()));
                            parts.push(format!("VRAM ~{} MB", doc.vram_bytes() / (1024 * 1024)));
                        }
                        parts.push(self.engine.adapter_name.clone());
                        ui.label(RichText::new(parts.join("  |  ")).size(11.0).color(Color32::from_gray(150)));
                    });
                });
            });
    }

    /// Tool options above the viewport (left) and view controls (right).
    pub(super) fn context_bar(&mut self, ui: &mut Ui, rect: Rect) {
        ui.painter().rect_filled(rect, 0.0, crate::style::PANEL);
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], egui::Stroke::new(1.0f32, crate::style::SEPARATOR));
        // View controls on the right first, tool options clipped to what is left.
        let inner = rect.shrink2(egui::vec2(8.0, 2.0));
        let icons_rect = Rect::from_min_max(Pos2::new(inner.max.x - 210.0, inner.min.y), inner.max);
        let mut right = ui.new_child(egui::UiBuilder::new().max_rect(icons_rect).layout(egui::Layout::right_to_left(egui::Align::Center)));
        self.view_icons(&mut right);
        let left_rect = Rect::from_min_max(inner.min, Pos2::new(icons_rect.min.x - 8.0, inner.max.y));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(left_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        child.set_clip_rect(left_rect);
        let ui = &mut child;
        ui.spacing_mut().item_spacing.x = 4.0;
        // A compact Painter slider (label and value over the track).
        // On a narrow window the options that no longer fit are left out
        // (half a slider under the view icons is neither readable nor usable).
        let field = |ui: &mut Ui, w: f32, s: widgets::Slider| -> bool {
            if ui.available_width() < w {
                return false;
            }
            ui.allocate_ui_with_layout(egui::vec2(w, 34.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(w);
                s.show(ui)
            })
            .inner
        };
        let sep = |ui: &mut Ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(9.0, 26.0), Sense::hover());
            ui.painter().line_segment([r.center_top(), r.center_bottom()], egui::Stroke::new(1.0f32, Color32::from_gray(70)));
        };
        // A selected fill layer gets its projection tools, like the real app.
        if let Some(doc) = &mut self.doc {
            let s = &mut doc.sets[doc.active];
            if s.target == crate::document::Target::Content {
                if let Some(crate::document::LayerKind::Fill(f)) = s.selected_layer_mut().map(|l| &mut l.desc.kind) {
                    let pr = &mut f.projection;
                    let mut dirty = false;
                    egui::ComboBox::from_id_salt("ctx proj")
                        .width(150.0)
                        .selected_text(pr.mode.name())
                        .show_ui(ui, |ui| {
                            for m in crate::document::ProjMode::ALL {
                                dirty |= ui.selectable_value(&mut pr.mode, *m, m.name()).changed();
                            }
                        });
                    sep(ui);
                    dirty |= field(ui, 120.0, widgets::Slider::new("Tiling", &mut pr.scale, 0.05..=256.0).log().soft(0.001, 100000.0));
                    if pr.mode == crate::document::ProjMode::Uv {
                        dirty |= field(ui, 110.0, widgets::Slider::new("Rotation", &mut pr.rotation, -180.0..=180.0).decimals(0));
                    }
                    sep(ui);
                    if icons::button(ui, Icon::Symmetry, pr.mirror[0], 24.0, "Mirror U").clicked() {
                        pr.mirror[0] = !pr.mirror[0];
                        dirty = true;
                    }
                    let flip_v = ui.add(egui::Button::new(RichText::new("V").size(11.0)).selected(pr.mirror[1]).min_size(egui::vec2(24.0, 24.0))).on_hover_text("Mirror V");
                    if flip_v.clicked() {
                        pr.mirror[1] = !pr.mirror[1];
                        dirty = true;
                    }
                    sep(ui);
                    if pr.mode != crate::document::ProjMode::Uv {
                        // Manipulator mode, like Painter's W / E / R.
                        for m in super::gizmo::GizmoMode::ALL {
                            let short = m.name().split(' ').next().unwrap_or("");
                            if ui.selectable_label(self.gizmo_mode == m, short).on_hover_text(m.name()).clicked() {
                                self.gizmo_mode = m;
                            }
                        }
                        sep(ui);
                    }
                    if ui.button("Reset transformation").clicked() {
                        let mode = pr.mode;
                        *pr = crate::document::Projection { mode, ..Default::default() };
                        dirty = true;
                    }
                    if dirty {
                        s.dirty = true;
                    }
                    return;
                }
            }
        }
        match self.tool {
            Tool::Paint | Tool::Erase | Tool::Clone | Tool::Smudge => {
                let b = &mut self.brush;
                field(ui, 120.0, widgets::Slider::new("Size", &mut b.size, 1.0..=1000.0).log().decimals(0));
                if icons::button(ui, Icon::Pen, b.pressure_size, 24.0, "Pen pressure drives the size").clicked() {
                    b.pressure_size = !b.pressure_size;
                }
                field(ui, 120.0, widgets::Slider::new("Flow", &mut b.flow, 0.01..=1.0).percent());
                if icons::button(ui, Icon::Pen, b.pressure_flow, 24.0, "Pen pressure drives the flow").clicked() {
                    b.pressure_flow = !b.pressure_flow;
                }
                field(ui, 110.0, widgets::Slider::new("Hardness", &mut b.hardness, 0.0..=1.0).percent());
                field(ui, 110.0, widgets::Slider::new("Spacing", &mut b.spacing, 0.02..=2.0).percent());
                if self.tool == Tool::Smudge {
                    field(ui, 110.0, widgets::Slider::new("Strength", &mut self.smudge_strength, 0.05..=2.0).percent());
                }
                sep(ui);
                let lazy_on = b.lazy > 0.0;
                if icons::button(ui, Icon::Lazy, lazy_on, 24.0, "Lazy mouse (D)").clicked() {
                    if lazy_on {
                        self.lazy_saved = b.lazy;
                        b.lazy = 0.0;
                    } else {
                        b.lazy = self.lazy_saved.max(10.0);
                    }
                }
                let mut dist = if b.lazy > 0.0 { b.lazy } else { self.lazy_saved };
                if field(ui, 110.0, widgets::Slider::new("Distance", &mut dist, 1.0..=200.0).decimals(0).enabled(lazy_on)) && lazy_on {
                    b.lazy = dist;
                }
                sep(ui);
                let sym_on = b.symmetry.iter().any(|s| *s);
                let sresp = icons::button(ui, Icon::Symmetry, sym_on, 24.0, "Symmetry (L)");
                let pid = ui.id().with("symmetry popup");
                if sresp.clicked() {
                    if sym_on {
                        b.symmetry = [false; 3];
                    } else {
                        b.symmetry = [true, false, false];
                    }
                }
                let cresp = icons::button(ui, Icon::Chevron, false, 16.0, "Symmetry settings");
                if cresp.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(pid));
                }
                super::widgets::popup_below(ui, pid, &cresp, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
                    ui.set_min_width(180.0);
                    ui.label(RichText::new("SYMMETRY").font(crate::style::bold(11.5)));
                    ui.horizontal(|ui| {
                        for (i, n) in ["Mirror X", "Mirror Y", "Mirror Z"].iter().enumerate() {
                            ui.toggle_value(&mut b.symmetry[i], *n);
                        }
                    });
                });
                if self.tool == Tool::Clone {
                    sep(ui);
                    ui.label(RichText::new(if self.clone_src.is_some() { "Source set (V to move it)" } else { "Press V over the mesh to set the source" }).size(11.5).color(widgets::DIM));
                }
            }
            Tool::PolygonFill | Tool::Hide => {
                ui.label(RichText::new("Mode").size(11.5).color(widgets::LABEL));
                for m in PolyMode::ALL {
                    ui.selectable_value(&mut self.poly_mode, m, m.name());
                }
                if self.tool == Tool::Hide {
                    sep(ui);
                    let has = self.doc.as_ref().is_some_and(|d| d.any_hidden());
                    if ui.add_enabled(has, egui::Button::new("Show all")).clicked() {
                        self.checkpoint_hidden("Show all geometry");
                        if let Some(doc) = &mut self.doc {
                            doc.reset_hidden();
                            doc.refresh_visibility(&self.engine);
                        }
                    }
                    if ui.add_enabled(has, egui::Button::new("Invert")).clicked() {
                        self.checkpoint_hidden("Invert hidden geometry");
                        if let Some(doc) = &mut self.doc {
                            for v in doc.hidden.iter_mut() {
                                v.iter_mut().for_each(|h| *h = !*h);
                            }
                            doc.refresh_visibility(&self.engine);
                        }
                    }
                }
            }
            Tool::Picker => {
                ui.label(RichText::new("Click the mesh to pick its material into the brush.").size(11.5).color(widgets::DIM));
            }
        }
    }

    fn view_icons(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.x = 2.0;
        let painting = !self.view.path_trace && self.bake_dialog.is_none();
        if icons::button(ui, Icon::Brush, painting, 26.0, "Painting mode (F9)").clicked() {
            self.view.path_trace = false;
            self.bake_dialog = None;
        }
        if icons::button(ui, Icon::Bake, self.bake_dialog.is_some(), 26.0, "Baking mode (F8)").clicked() {
            self.open_bake_dialog();
        }
        let render_tip = if self.view.path_trace { format!("Rendering: {} samples (F10)", self.engine.pathtracer.frame) } else { "Rendering (F10)".into() };
        if icons::button(ui, Icon::Render, self.view.path_trace, 26.0, &render_tip).clicked() {
            self.view.path_trace = !self.view.path_trace;
        }
        ui.add_space(8.0);
        // Camera projection menu.
        let cam = icons::button(ui, Icon::Camera, false, 26.0, "Camera projection");
        let cid = ui.id().with("cam menu");
        if cam.clicked() {
            ui.memory_mut(|m| m.toggle_popup(cid));
        }
        super::widgets::popup_below(ui, cid, &cam, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(170.0);
            if ui.selectable_label(!self.camera.ortho, "Perspective          F5").clicked() {
                self.camera.ortho = false;
            }
            if ui.selectable_label(self.camera.ortho, "Orthographic         F6").clicked() {
                self.camera.ortho = true;
            }
        });
        // View layout menu.
        let lay_icon = if self.view.layout == Layout::Both { Icon::Split } else { Icon::Single };
        let lay = icons::button(ui, lay_icon, false, 26.0, "Viewport layout");
        let lid = ui.id().with("layout menu");
        if lay.clicked() {
            ui.memory_mut(|m| m.toggle_popup(lid));
        }
        super::widgets::popup_below(ui, lid, &lay, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(170.0);
            for (l, n) in [(Layout::Both, "3D / 2D               F1"), (Layout::Only3D, "3D only               F2"), (Layout::Only2D, "2D only               F3")] {
                if ui.selectable_label(self.view.layout == l, n).clicked() {
                    self.view.layout = l;
                }
            }
            ui.separator();
            if ui.button("Swap 3D / 2D          F4").clicked() {
                self.view.swap_views = !self.view.swap_views;
            }
        });
    }

    /// Display channel menu in the top right corner of a view.
    pub(super) fn view_display_menu(&mut self, ui: &mut Ui, rect: Rect, is_2d: bool) {
        let r = Rect::from_min_size(Pos2::new(rect.max.x - 178.0, rect.min.y + 8.0), egui::vec2(168.0, 22.0));
        super::mark(ui.ctx(), if is_2d { "display2d" } else { "display3d" }, r);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let d = if is_2d { &mut self.view.display_2d } else { &mut self.view.display };
        egui::ComboBox::from_id_salt(if is_2d { "display 2d" } else { "display 3d" })
            .selected_text(d.name())
            .width(166.0)
            .height(520.0)
            .show_ui(&mut child, |ui| {
                for (i, x) in Display::ALL.iter().enumerate() {
                    if i == 10 || i == 17 {
                        ui.separator();
                    }
                    ui.selectable_value(d, *x, x.name());
                }
            });
    }
}

impl RetroApp {
    /// One flyout at a time (display settings 0, history 1, log 2), like
    /// the real dock strip: opening one closes the others.
    pub(super) fn flip_flyout(&mut self, which: u8) {
        let was = [self.show_view_settings, self.show_history, self.show_log][which as usize];
        self.show_view_settings = false;
        self.show_history = false;
        self.show_log = false;
        match which {
            0 => self.show_view_settings = !was,
            1 => self.show_history = !was,
            _ => self.show_log = !was,
        }
    }

    /// Geometry mask changes are undo steps of their own.
    pub(super) fn checkpoint_hidden(&mut self, label: &str) {
        let Some(doc) = &mut self.doc else { return };
        if doc.hidden.len() != doc.mesh.submeshes.len() {
            doc.reset_hidden();
        }
        let hidden = doc.hidden.clone();
        self.history.push(crate::history::Entry::Hidden { label: label.into(), hidden });
    }
}
