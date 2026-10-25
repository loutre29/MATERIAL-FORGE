//! Window chrome: the icon tool strip, the right dock icon column, the
//! context toolbar above the viewport, per-view display menus and the
//! status bar.

use super::icons::{self, Icon};
use super::*;
use super::widgets;
use eframe::egui::{RichText, Sense, Ui};


impl RetroApp {
    /// The options of the active tool (or the projection of a selected
    /// fill), in the floating bar above the tool dock.
    pub(super) fn tool_options(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.x = 4.0;
        // A compact Painter slider (label and value over the track).
        // On a narrow window the options that no longer fit are left out
        // (half a slider under the view icons is neither readable nor usable).
        // The options bar wraps onto a second row when the canvas is narrow.
        let field = |ui: &mut Ui, w: f32, s: widgets::Slider| -> bool {
            ui.allocate_ui_with_layout(egui::vec2(w, 28.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(w);
                s.show(ui)
            })
            .inner
        };
        let sep = |ui: &mut Ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(9.0, 26.0), Sense::hover());
            ui.painter().line_segment([r.center_top(), r.center_bottom()], egui::Stroke::new(1.0f32, crate::style::warm(70)));
        };
        // A selected fill layer (or a mask's image fill) gets its projection
        // tools, like the real app.
        if let Some(doc) = &mut self.doc {
            let s = &mut doc.sets[doc.active];
            {
                if let Some(pr) = super::uvgizmo::sel_projection_mut(s) {
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
                    dirty |= field(ui, 170.0, widgets::Slider::new("Tiling", &mut pr.scale, 0.05..=256.0).log().soft(0.001, 100000.0));
                    if pr.mode == crate::document::ProjMode::Uv {
                        dirty |= field(ui, 160.0, widgets::Slider::new("Rotation", &mut pr.rotation, -180.0..=180.0).decimals(0));
                    }
                    sep(ui);
                    // Mirror U / V: two matching toggles.
                    for (i, n) in ["Mirror U", "Mirror V"].into_iter().enumerate() {
                        if ui.add(egui::Button::new(RichText::new(n).size(11.5)).selected(pr.mirror[i])).clicked() {
                            pr.mirror[i] = !pr.mirror[i];
                            dirty = true;
                        }
                    }
                    sep(ui);
                    if pr.mode == crate::document::ProjMode::Uv {
                        let _ = pr;
                        if dirty {
                            s.dirty = true;
                            doc.modified = true;
                        }
                        self.uv_gizmo_toolbar(ui);
                        return;
                    }
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
            Tool::Paint | Tool::Erase | Tool::Clone | Tool::Smudge | Tool::SelBrush | Tool::Projection => {
                if self.tool == Tool::Projection {
                    let name = self.stencil.resource.and_then(|id| self.doc.as_ref()?.resources.get(&id)).map(|r| r.name.clone());
                    let text = name.unwrap_or_else(|| "No texture".into());
                    if ui.add(egui::Button::new(RichText::new(format!("Texture: {text}")).size(11.5))).on_hover_text("Choose it in Properties > Stencil").clicked() {
                        self.props_tab = 2;
                    }
                    if ui.add(egui::Button::new(RichText::new("Fit to view").size(11.5))).on_hover_text("T + left / middle / right drag rotates, moves, scales").clicked() {
                        self.stencil_fit();
                    }
                    field(ui, 160.0, widgets::Slider::new("Opacity", &mut self.stencil.opacity, 0.0..=1.0).percent());
                    sep(ui);
                }
                if self.tool == Tool::SelBrush {
                    for (add, n) in [(true, "Add"), (false, "Subtract")] {
                        if ui.add(egui::Button::new(RichText::new(n).size(11.5)).selected(self.sel_brush_add == add)).on_hover_text("X swaps").clicked() {
                            self.sel_brush_add = add;
                        }
                    }
                    sep(ui);
                }
                let b = &mut self.brush;
                field(ui, 170.0, widgets::Slider::new("Size", &mut b.size, 1.0..=1000.0).log().decimals(0));
                if icons::button(ui, Icon::Pen, b.pressure_size, 24.0, "Pen pressure drives the size").clicked() {
                    b.pressure_size = !b.pressure_size;
                }
                field(ui, 170.0, widgets::Slider::new("Flow", &mut b.flow, 0.01..=1.0).percent());
                if icons::button(ui, Icon::Pen, b.pressure_flow, 24.0, "Pen pressure drives the flow").clicked() {
                    b.pressure_flow = !b.pressure_flow;
                }
                field(ui, 160.0, widgets::Slider::new("Hardness", &mut b.hardness, 0.0..=1.0).percent());
                field(ui, 160.0, widgets::Slider::new("Spacing", &mut b.spacing, 0.02..=2.0).percent());
                if self.tool == Tool::Smudge {
                    field(ui, 160.0, widgets::Slider::new("Strength", &mut self.smudge_strength, 0.05..=2.0).percent());
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
                if field(ui, 160.0, widgets::Slider::new("Distance", &mut dist, 1.0..=200.0).decimals(0).enabled(lazy_on)) && lazy_on {
                    b.lazy = dist;
                }
                sep(ui);
                let sym_on = b.symmetry.iter().any(|s| *s);
                let sresp = icons::button(ui, Icon::Symmetry, sym_on, 24.0, "Symmetry (Y)");
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
                if self.tool == Tool::PolygonFill {
                    for (sel, n, tip) in [(false, "Fill", "Fill the faces with the brush material"), (true, "Select", "Make the faces a selection (paint, fill or erase inside it)")] {
                        if ui.add(egui::Button::new(RichText::new(n).size(11.5)).selected(self.poly_to_selection == sel)).on_hover_text(tip).clicked() {
                            self.poly_to_selection = sel;
                        }
                    }
                    sep(ui);
                }
                for m in super::view3d::PolySelect::ALL {
                    if icons::button(ui, m.icon(), self.poly_select == m, 22.0, m.name()).clicked() {
                        self.poly_select = m;
                    }
                }
                sep(ui);
                // Fill modes apply to the rectangle (loops fill quads).
                let marquee = self.poly_select.is_area();
                ui.add_enabled_ui(marquee, |ui| {
                    for m in PolyMode::ALL {
                        if icons::button(ui, m.icon(), self.poly_mode == m && marquee, 22.0, m.name()).clicked() {
                            self.poly_mode = m;
                        }
                    }
                });
                if marquee {
                    sep(ui);
                    ui.label(RichText::new("Hit-test").size(11.5).color(widgets::LABEL));
                    egui::ComboBox::from_id_salt("poly hit")
                        .selected_text(self.poly_hit.name())
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            for h in super::view3d::PolyHit::ALL {
                                ui.selectable_value(&mut self.poly_hit, h, h.name());
                            }
                        });
                    ui.checkbox(&mut self.poly_visible_only, "Visible only");
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
            Tool::Path => {
                let sel = self.selected_path().map(|p| (p.id, p.closed, p.points.len()));
                for (pen, label, tip) in [(false, "Curve", "New points: smooth, their handles follow the neighbors"), (true, "Pen", "New points: click for a corner, click and drag for a smooth point with handles")] {
                    if ui.add(egui::Button::new(RichText::new(label).size(11.5)).selected(self.path.pen == pen)).on_hover_text(tip).clicked() {
                        self.path.pen = pen;
                    }
                }
                sep(ui);
                // Brushes made for paths: stitches, seams, hardware.
                let presets: Vec<crate::brush::BrushPreset> =
                    crate::brush::builtin_brushes().into_iter().filter(|b| b.folder == "Stitches" || b.folder == "Hardware").collect();
                let shown = if presets.iter().any(|b| b.name == self.brush_preset) { self.brush_preset.clone() } else { "Choose a stitch...".to_string() };
                let mut pick = None;
                egui::ComboBox::from_id_salt("path brush").width(150.0).selected_text(RichText::new(shown).size(11.5)).show_ui(ui, |ui| {
                    for (i, b) in presets.iter().enumerate() {
                        if ui.selectable_label(b.name == self.brush_preset, &b.name).clicked() {
                            pick = Some(i);
                        }
                    }
                });
                if let Some(i) = pick {
                    self.apply_brush_preset(&presets[i]);
                }
                sep(ui);
                let closed = sel.is_some_and(|s| s.1);
                if ui
                    .add_enabled(sel.is_some_and(|s| s.2 > 2), egui::Button::new(RichText::new("Closed").size(11.5)).selected(closed))
                    .on_hover_text("The selected path loops back to its first point")
                    .clicked()
                {
                    if let Some((id, ..)) = sel {
                        self.edit_path(id, |p| p.closed = !closed);
                    }
                }
                if ui.add_enabled(sel.is_some(), egui::Button::new(RichText::new("End path").size(11.5))).on_hover_text("The next click starts a new path (Escape)").clicked() {
                    self.path_deselect();
                }
                if ui.add_enabled(sel.is_some(), egui::Button::new(RichText::new("Delete path").size(11.5))).clicked() {
                    if let Some((id, ..)) = sel {
                        self.delete_path(id);
                    }
                }
                let on_layer = self.active_paths().is_some();
                if ui
                    .add_enabled(on_layer, egui::Button::new(RichText::new("New path layer").size(11.5)))
                    .on_hover_text("Paths with another brush or material go on their own layer")
                    .clicked()
                {
                    self.new_path_layer();
                }
                sep(ui);
                let b = &mut self.brush;
                field(ui, 170.0, widgets::Slider::new("Size", &mut b.size, 1.0..=1000.0).log().decimals(0));
                field(ui, 160.0, widgets::Slider::new("Flow", &mut b.flow, 0.01..=1.0).percent());
                field(ui, 160.0, widgets::Slider::new("Spacing", &mut b.spacing, 0.02..=4.0).percent());
                sep(ui);
                ui.label(
                    RichText::new(if !on_layer {
                        "Click the mesh to start a path (a stitch, seam or rivet brush follows it)"
                    } else {
                        "Brush and material apply to every path of this layer, live"
                    })
                    .size(11.5)
                    .color(widgets::DIM),
                );
            }
            Tool::Gradient => {
                for (k, n) in ["Linear", "Radial", "Reflected"].into_iter().enumerate() {
                    if ui.add(egui::Button::new(RichText::new(n).size(11.5)).selected(self.gradient_kind == k as u8)).clicked() {
                        self.gradient_kind = k as u8;
                    }
                }
                sep(ui);
                field(ui, 170.0, widgets::Slider::new("Opacity", &mut self.brush.flow, 0.01..=1.0).percent());
                sep(ui);
                ui.checkbox(&mut self.gradient_through, "Through").on_hover_text("3D view: every texel takes the gradient, hidden or facing away too");
                let mask = self.doc.as_ref().map(|d| d.sets[d.active].target.is_mask()).unwrap_or(false);
                super::gradient::preview(ui, if mask { &self.grad_mask } else { &self.grad_color }, mask, egui::vec2(90.0, 20.0));
                sep(ui);
                ui.label(
                    RichText::new(if self.has_selection() { "Drag a line, inside the selection" } else { "Drag a line over the mesh" })
                        .size(11.5)
                        .color(widgets::DIM),
                );
            }
            Tool::Wand | Tool::Bucket => {
                use crate::engine::select::SelOp;
                if self.tool == Tool::Wand {
                    for (op, icon) in [(SelOp::Replace, Icon::SelNew), (SelOp::Add, Icon::SelAdd), (SelOp::Subtract, Icon::SelSub), (SelOp::Intersect, Icon::SelInter)] {
                        if icons::button(ui, icon, self.sel_op == op, 22.0, op.name()).clicked() {
                            self.sel_op = op;
                        }
                    }
                    sep(ui);
                }
                field(ui, 200.0, widgets::Slider::new("Tolerance", &mut self.wand_tolerance, 0.0..=255.0).decimals(0));
                ui.checkbox(&mut self.wand_contiguous, "Contiguous").on_hover_text("Only the area connected to the click (across UV seams); off: every similar texel");
                ui.checkbox(&mut self.wand_all_layers, "All layers").on_hover_text("Compare the visible result; off: the selected paint layer only");
                sep(ui);
                let has = self.has_selection();
                if self.tool == Tool::Wand {
                    if ui.add_enabled(has, egui::Button::new("Deselect")).on_hover_text("Ctrl + D").clicked() {
                        self.deselect();
                    }
                    if ui.add_enabled(has, egui::Button::new("Invert")).on_hover_text("Ctrl + Shift + I").clicked() {
                        self.invert_selection();
                    }
                } else {
                    ui.label(RichText::new(if has { "Fills inside the selection" } else { "Uses the brush material" }).size(11.5).color(widgets::DIM));
                }
            }
            Tool::Select(shape) => {
                use super::selection::SelShape;
                use crate::engine::select::SelOp;
                for (sh, icon, tip) in [
                    (SelShape::Rect, Icon::SelMarquee, "Rectangle select (M)"),
                    (SelShape::Ellipse, Icon::SelEllipse, "Ellipse select (Shift + M)"),
                    (SelShape::Lasso, Icon::SelLasso, "Lasso (L)"),
                    (SelShape::PolyLasso, Icon::SelPolyLasso, "Polygonal lasso (Shift + L): click points, close on the first one or double click"),
                ] {
                    if icons::button(ui, icon, shape == sh, 22.0, tip).clicked() {
                        self.tool = Tool::Select(sh);
                        self.sel_drag = None;
                    }
                }
                sep(ui);
                for (op, icon) in [(SelOp::Replace, Icon::SelNew), (SelOp::Add, Icon::SelAdd), (SelOp::Subtract, Icon::SelSub), (SelOp::Intersect, Icon::SelInter)] {
                    if icons::button(ui, icon, self.sel_op == op, 22.0, op.name()).clicked() {
                        self.sel_op = op;
                    }
                }
                sep(ui);
                ui.checkbox(&mut self.sel_through, "Through").on_hover_text("3D view: also select surfaces hidden behind others or facing away");
                sep(ui);
                let has = self.has_selection();
                if ui.button("All").on_hover_text("Select all (Ctrl + A)").clicked() {
                    self.select_all();
                }
                if ui.add_enabled(has, egui::Button::new("Deselect")).on_hover_text("Ctrl + D").clicked() {
                    self.deselect();
                }
                if ui.add_enabled(has, egui::Button::new("Invert")).on_hover_text("Ctrl + Shift + I").clicked() {
                    self.invert_selection();
                }
                sep(ui);
                if ui.add_enabled(has, egui::Button::new("Fill")).on_hover_text("Fill the selection with the brush material (Shift + Backspace)").clicked() {
                    self.fill_selection(false);
                }
                if ui.add_enabled(has, egui::Button::new("Clear")).on_hover_text("Erase the selection on the layer (Delete)").clicked() {
                    self.fill_selection(true);
                }
            }
        }
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
