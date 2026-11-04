//! Viewport interaction: the 3D and 2D views, navigation, brush strokes,
//! polygon fill, the color picker and the stencil overlay.

use super::*;
use crate::brush::{Space, View};
use crate::document::find;
use crate::engine::paint::{BrushUniform, MODE_CAMERA, MODE_FULL, MODE_UV, MODE_WRAP};
use crate::engine::Tex;
use eframe::egui::{Color32, Pos2, Sense};
use std::collections::HashSet;
use wgpu::util::DeviceExt;

/// `v` turned by `a` radians (the 2D view's y axis points down, so a
/// positive angle turns clockwise on screen).
pub(super) fn turn(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// A polygon fill or hide drag. The selection builds up while the mouse
/// moves (rectangle, loops or a partial loop patch) and is applied on
/// release as one undo step, like Painter.
pub(super) struct PolyDrag {
    set: usize,
    sub: usize,
    hide: bool,
    isolate: bool,
    /// Press and current cursor (viewport local points).
    start: Vec2,
    cur: Vec2,
    moved: bool,
    select: PolySelect,
    /// Selected triangles (loops and patches; the rectangle is computed on
    /// release).
    tris: Vec<u32>,
    /// Loops: polygons already in the selection, first loop direction.
    polys: HashSet<u32>,
    loop_dir: Option<Vec3>,
    /// Lasso: the outline drawn so far (viewport local points).
    path: Vec<Vec2>,
    /// Partial loop: start quad, its exit edge once the drag direction is known.
    partial: Option<(u32, Option<usize>)>,
    primary_only: bool,
    /// Started in the 2D view: points are 2D view local, picks in UV space.
    uv: bool,
}

/// How the polygon fill tool selects (Painter's toolbar).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PolySelect {
    /// Click, or drag a rectangle.
    Marquee,
    /// Click, or draw a free-form outline.
    Lasso,
    /// Click fills a quad loop; dragging adds loops.
    Loop,
    /// Drag fills part of a loop, grown sideways into a patch.
    PartialLoop,
}

impl PolySelect {
    pub const ALL: [PolySelect; 4] = [PolySelect::Marquee, PolySelect::Lasso, PolySelect::Loop, PolySelect::PartialLoop];
    /// Selections that take the fill mode, hit-test and visibility options.
    pub fn is_area(self) -> bool {
        matches!(self, PolySelect::Marquee | PolySelect::Lasso)
    }
    pub fn name(self) -> &'static str {
        match self {
            PolySelect::Marquee => "Rectangular marquee fill",
            PolySelect::Lasso => "Lasso fill",
            PolySelect::Loop => "Polygon loop fill",
            PolySelect::PartialLoop => "Partial polygon loop fill",
        }
    }
    pub fn icon(self) -> super::icons::Icon {
        match self {
            PolySelect::Marquee => super::icons::Icon::SelMarquee,
            PolySelect::Lasso => super::icons::Icon::SelLasso,
            PolySelect::Loop => super::icons::Icon::SelLoop,
            PolySelect::PartialLoop => super::icons::Icon::SelPartialLoop,
        }
    }
}

/// Rectangle hit test.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PolyHit {
    /// Any part of the polygon inside or touching the rectangle.
    Intersection,
    /// The polygon's center inside the rectangle.
    Center,
}

impl PolyHit {
    pub const ALL: [PolyHit; 2] = [PolyHit::Intersection, PolyHit::Center];
    pub fn name(self) -> &'static str {
        match self {
            PolyHit::Intersection => "Intersection",
            PolyHit::Center => "Contained center",
        }
    }
}

impl RetroApp {
    pub(super) fn central(&mut self, ui: &mut egui::Ui) {
        // The views fill the window; the panels float over them.
        let full = ui.available_rect_before_wrap();
        match self.view.layout {
            Layout::Only3D => self.viewport_3d(ui, full),
            Layout::Only2D => self.viewport_2d(ui, full),
            Layout::Both => {
                // The split divides the visible canvas (between the panels).
                let vis = if self.canvas_mid.is_positive() && !self.ui_hidden { self.canvas_mid.intersect(full) } else { full };
                let mid = (vis.min.x + vis.width() * self.view.split.clamp(0.15, 0.85)).round();
                let mut left = Rect::from_min_max(full.min, Pos2::new(mid - 1.0, full.max.y));
                let mut right = Rect::from_min_max(Pos2::new(mid + 1.0, full.min.y), full.max);
                if self.view.swap_views {
                    std::mem::swap(&mut left, &mut right);
                }
                self.viewport_3d(ui, left);
                self.viewport_2d(ui, right);
                // The divider drags to resize the views; a double click centers it.
                let grip = Rect::from_min_max(Pos2::new(mid - 4.0, full.min.y), Pos2::new(mid + 4.0, full.max.y));
                let g = ui.interact(grip, egui::Id::new("view split"), egui::Sense::click_and_drag());
                if g.dragged() {
                    if let Some(p) = g.interact_pointer_pos() {
                        self.view.split = ((p.x - vis.min.x) / vis.width().max(1.0)).clamp(0.15, 0.85);
                    }
                }
                if g.double_clicked() {
                    self.view.split = 0.5;
                }
                if g.hovered() || g.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                super::mark(ui.ctx(), "view split", grip);
                let c = if g.hovered() || g.dragged() { crate::style::ACCENT } else { Color32::from_gray(20) };
                // Not over the floating tool dock and options bar.
                let mut cuts: Vec<(f32, f32)> = [self.options_rect, self.dock_rect]
                    .into_iter()
                    .filter(|r| r.is_positive() && !self.ui_hidden && r.x_range().contains(mid))
                    .map(|r| (r.min.y - 1.0, r.max.y + 1.0))
                    .collect();
                cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut y0 = full.min.y;
                for (a, b) in cuts.into_iter().chain(std::iter::once((full.max.y, full.max.y))) {
                    if a > y0 {
                        ui.painter().line_segment([Pos2::new(mid, y0), Pos2::new(mid, a)], egui::Stroke::new(2.0f32, c));
                    }
                    y0 = y0.max(b);
                }
            }
        }
        if self.doc.is_none() {
            self.start_screen(ui, full);
        }
    }

    /// No project: logo, name and the two ways in.
    fn start_screen(&mut self, ui: &mut egui::Ui, full: Rect) {
        let logo = self.logo_tex.get_or_insert_with(|| {
            let img = crate::logo::image(256);
            let ci = egui::ColorImage::from_rgba_unmultiplied([256, 256], img.as_raw());
            ui.ctx().load_texture("material forge logo", ci, egui::TextureOptions::LINEAR)
        })
        .id();
        let c = full.center() - egui::vec2(0.0, 60.0);
        let lr = Rect::from_center_size(c, egui::vec2(128.0, 128.0));
        ui.painter().rect_filled(full, 0.0, crate::style::BG);
        ui.painter().image(logo, lr, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        ui.painter().text(lr.center_bottom() + egui::vec2(0.0, 30.0), egui::Align2::CENTER_CENTER, "Material Forge", crate::style::bold(26.0), Color32::from_gray(235));
        ui.painter().text(
            lr.center_bottom() + egui::vec2(0.0, 58.0),
            egui::Align2::CENTER_CENTER,
            "Drop an OBJ / FBX / glTF mesh or a project here",
            egui::FontId::proportional(13.0),
            Color32::from_gray(150),
        );
        let br = Rect::from_center_size(lr.center_bottom() + egui::vec2(0.0, 100.0), egui::vec2(260.0, 30.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(br).layout(egui::Layout::left_to_right(egui::Align::Center)));
        if child.add(egui::Button::new(egui::RichText::new("New project...").size(13.0)).fill(crate::style::ACCENT).min_size(egui::vec2(125.0, 30.0))).clicked() {
            self.new_dialog = Some(NewProjectDialog { path: String::new(), res: 2048, error: None, auto_uv: Default::default() });
        }
        if child.add(egui::Button::new(egui::RichText::new("Open...").size(13.0)).min_size(egui::vec2(125.0, 30.0))).clicked() {
            self.open_project();
        }
        // Recent projects under the buttons.
        let recent = crate::library::recent_projects();
        if !recent.is_empty() {
            let top = br.center_bottom() + egui::vec2(0.0, 26.0);
            ui.painter().text(top, egui::Align2::CENTER_CENTER, "RECENT PROJECTS", crate::style::bold(10.5), Color32::from_gray(140));
            let mut y = top.y + 14.0;
            for p in recent.iter().take(6) {
                let r = Rect::from_center_size(Pos2::new(top.x, y + 12.0), egui::vec2(360.0, 24.0));
                let resp = ui.interact(r, ui.id().with(("recent", p)), egui::Sense::click()).on_hover_text(p.display().to_string());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 3.0, crate::style::warm(50));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let dir = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
                ui.painter().text(Pos2::new(r.min.x + 8.0, r.center().y), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(12.5), Color32::from_gray(225));
                let dr = Rect::from_min_max(Pos2::new(r.center().x - 10.0, r.min.y), r.max);
                let g = ui.painter().layout_no_wrap(dir, egui::FontId::proportional(11.0), Color32::from_gray(120));
                ui.painter().with_clip_rect(dr).galley(Pos2::new(r.max.x - 8.0 - g.size().x.min(dr.width() - 8.0), r.center().y - g.size().y / 2.0), g, Color32::from_gray(120));
                if resp.clicked() {
                    let p = p.clone();
                    self.open_path(&p);
                    return;
                }
                y += 26.0;
            }
        }
    }

    pub(super) fn view_info(&self) -> View {
        let vp = &self.engine.viewport;
        let aspect = vp.width as f32 / vp.height as f32;
        let m = self.camera.view_proj(aspect);
        View {
            view_proj: m,
            inv_view_proj: m.inverse(),
            eye: self.camera.eye(),
            size: Vec2::new(vp.width as f32, vp.height as f32),
            fov_y: self.camera.fov_y,
            ppp: vp.width as f32 / self.vp_rect.width().max(1.0),
            ortho_wpp: self.camera.ortho.then(|| self.camera.ortho_world_per_px(vp.height as f32)),
            uv_rot: self.view.uv_rot,
        }
    }

    fn viewport_3d(&mut self, ui: &mut egui::Ui, rect: Rect) {
        self.vp_rect = rect;
        let ppp = ui.ctx().pixels_per_point();
        let (w, h) = ((rect.width() * ppp).round() as u32, (rect.height() * ppp).round() as u32);
        let resized = self.engine.viewport.resize(&self.engine.device, w, h);
        if resized || self.vp_tex.is_none() {
            let mut r = self.renderer.write();
            let view = &self.engine.viewport.output().view;
            match self.vp_tex {
                Some(id) => r.update_egui_texture_from_wgpu_texture(&self.engine.device, view, wgpu::FilterMode::Linear, id),
                None => self.vp_tex = Some(r.register_native_texture(&self.engine.device, view, wgpu::FilterMode::Linear)),
            }
        }
        // Leave the panels' resize grips (a few points on each side) to them.
        let resp = ui.allocate_rect(rect.shrink2(egui::vec2(6.0, 0.0)), Sense::click_and_drag());
        // While Muse checks its first pass, the view as it was before: the
        // user sees one result, the final one.
        let held = self.muse_hold_tex();
        ui.painter().image(held.unwrap_or(self.vp_tex.unwrap()), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        if held.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
        self.muse_piece_labels(ui.painter(), rect);
        self.studio_overlay(ui.painter(), rect);

        // Assets dropped on the mesh go to the texture set under the cursor.
        if super::drop_hover::<super::shelf::Item>(ui, rect).is_some() {
            ui.painter().rect_stroke(rect.shrink(2.0), 0.0, egui::Stroke::new(2.0f32, crate::style::ACCENT));
        }
        if let Some(item) = super::drop_take::<super::shelf::Item>(ui, rect) {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let local = Vec2::new(pos.x - rect.min.x, pos.y - rect.min.y);
                if let Some(hit) = self.pick_at(local) {
                    if let Some(doc) = &mut self.doc {
                        doc.active = hit.set;
                    }
                }
            }
            self.apply_item((*item).clone());
        }

        // Raw wheel delta: egui's smoothing makes zoom feel floaty.
        let typing = ui.ctx().wants_keyboard_input();
        let (mods, pointer, scroll, s_down, v_pressed) = ui.input(|i| {
            (i.modifiers, i.pointer.clone(), i.raw_scroll_delta.y, i.key_down(Key::T) && !typing, i.key_pressed(Key::V) && !i.modifiers.any() && !typing)
        });
        // A press belongs to the view only when it starts on the view itself.
        if pointer.any_pressed() {
            self.vp_press_ok = press_is_ours(ui, resp.id, self.overlay_open);
        }
        let ours = self.vp_press_ok;
        let drag = |b: egui::PointerButton| ours && resp.dragged_by(b);
        if !pointer.any_down() {
            self.orbit_pivot = None;
            self.orbit_free = None;
        }
        let delta = pointer.delta();
        let local = |p: Pos2| Vec2::new(p.x - rect.min.x, p.y - rect.min.y);
        use egui::PointerButton::{Middle, Primary, Secondary};

        // Projection manipulator of the selected fill: its handles take the
        // left button before the brush.
        let gizmo_busy = self.projection_gizmo(ui, rect, &pointer, mods, ours);

        // Stencil: T + LMB rotates, T + MMB moves, T + RMB scales.
        if gizmo_busy {
        } else if self.stencil_active() && s_down && resp.hovered() {
            if drag(Primary) {
                self.stencil.rotation += delta.x * 0.3;
            }
            if drag(Middle) {
                self.stencil.center += Vec2::new(delta.x, delta.y);
            }
            if drag(Secondary) {
                self.stencil.size = (self.stencil.size * (1.0 + (delta.x - delta.y) * 0.005)).clamp(10.0, 10000.0);
            }
        } else {
            let brush_gesture = mods.command && !mods.alt && !mods.shift && self.tool.uses_brush();
            // The brush gestures read the cursor's motion while holding it.
            let gesturing = ours && brush_gesture && (resp.dragged_by(Primary) || resp.dragged_by(Secondary));
            let gdelta = if gesturing { self.gesture_delta(ui.ctx(), &pointer) } else { egui::Vec2::ZERO };
            if !gesturing {
                self.gesture = None;
            }
            let navigating = mods.alt || pointer.secondary_down() || pointer.middle_down() || brush_gesture;
            // Camera: Alt + LMB orbit, Alt + MMB (or MMB, or Ctrl + Alt + LMB) pan, Alt + RMB zoom.
            // Alt + LMB orbits around the surface point under the cursor.
            // Orbit turns around the view center (the camera target), like
            // Painter: no jump to whatever surface the press started on.
            if ours && resp.drag_started_by(Primary) && mods.alt && !mods.command {
                self.orbit_pivot = None;
            }
            if drag(Primary) && mods.alt && mods.command {
                self.camera.pan(delta.x * ppp, delta.y * ppp, h as f32);
            } else if drag(Primary) && mods.alt && mods.shift {
                // Shift: the camera snaps to the straight axis views (front,
                // sides, back, top, bottom) while the free angle follows
                // the mouse underneath.
                let q = std::f32::consts::FRAC_PI_2;
                let (fy, fp) = self.orbit_free.get_or_insert((self.camera.yaw, self.camera.pitch));
                *fy -= delta.x * 0.0105;
                *fp = (*fp + delta.y * 0.0105).clamp(-1.55, 1.55);
                self.camera.yaw = (*fy / q).round() * q;
                self.camera.pitch = ((*fp / q).round() * q).clamp(-1.55, 1.55);
            } else if drag(Primary) && mods.alt {
                self.orbit_free = None;
                match self.orbit_pivot {
                    Some(p) => self.camera.orbit_around(delta.x, delta.y, p),
                    None => self.camera.orbit(delta.x, delta.y),
                }
            }
            // Ctrl + LMB: flow (horizontal) and angle (vertical).
            if drag(Primary) && brush_gesture {
                self.brush.flow = (self.brush.flow + gdelta.x * 0.004).clamp(0.01, 1.0);
                self.brush.angle = ((self.brush.angle - gdelta.y * 0.6 + 180.0).rem_euclid(360.0) - 180.0).round();
            }
            if drag(Secondary) && ours {
                if mods.command && mods.alt {
                    // Ctrl + Alt + RMB: pick the texture set under the cursor.
                    if let Some(p) = pointer.interact_pos() {
                        if let Some(hit) = self.pick_at(local(p)) {
                            if let Some(doc) = &mut self.doc {
                                doc.active = hit.set;
                            }
                        }
                    }
                } else if mods.command {
                    self.brush.size = (self.brush.size * (1.0 + gdelta.x * 0.01)).clamp(1.0, 1000.0);
                    self.brush.hardness = (self.brush.hardness - gdelta.y * 0.005).clamp(0.0, 1.0);
                } else if mods.shift {
                    self.view.env_rotation = (self.view.env_rotation + delta.x * 0.5 + 180.0).rem_euclid(360.0) - 180.0;
                } else if mods.alt {
                    self.camera.zoom((1.0 + delta.y * 0.005).clamp(0.5, 2.0));
                }
                // Plain right button: quick properties on release, no camera move.
            }
            // Plain right click (no drag): quick properties at the cursor.
            if ours && resp.clicked_by(Secondary) && !mods.any() && self.doc.is_some() {
                self.quick_props = pointer.interact_pos();
            }
            if drag(Middle) {
                self.camera.pan(delta.x * ppp, delta.y * ppp, h as f32);
            }
            // Wheel zooms toward the point under the cursor.
            if resp.hovered() && scroll != 0.0 {
                let f = (1.0 - scroll * 0.003).clamp(0.5, 2.0);
                match pointer.hover_pos().and_then(|p| self.pick_at(local(p))) {
                    Some(hit) => self.camera.zoom_at(f, hit.pos),
                    None => self.camera.zoom(f),
                }
            }
            // V over the mesh: clone source.
            if v_pressed && resp.hovered() && self.tool == Tool::Clone {
                if let Some(p) = pointer.hover_pos() {
                    self.set_clone_source(local(p));
                }
            }

            // Eyedropper armed by a color picker: the next click samples the mesh.
            let eyedropper = ui.data(|d| d.get_temp::<egui::Id>(super::widgets::eyedropper_id()));
            if let Some(owner) = eyedropper {
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                if pointer.primary_pressed() && resp.hovered() && ours {
                    if let Some(p) = pointer.interact_pos() {
                        if let Some(c) = self.sample_color(local(p)) {
                            ui.data_mut(|d| {
                                d.insert_temp(super::widgets::eyedropper_result_id(), (owner, c));
                                d.remove::<egui::Id>(super::widgets::eyedropper_id());
                            });
                        }
                    }
                }
            }
            if self.doc.is_some() && self.bake_job.is_none() && eyedropper.is_none() {
                if pointer.primary_pressed() && resp.hovered() && !navigating && ours {
                    if let Some(p) = pointer.interact_pos() {
                        let p = local(p);
                        if let Some(eid) = self.id_pick.take() {
                            self.pick_id_color(p, eid);
                        } else {
                            match self.tool {
                                Tool::Paint | Tool::Erase | Tool::Clone | Tool::Smudge | Tool::SelBrush | Tool::Projection => {
                                    // Shift: straight line from the last stroke; Ctrl + Shift snaps it to 5 degrees.
                                    let start = mods.shift.then_some(self.line_end).flatten();
                                    let end = match start {
                                        Some(a) if mods.command => snap_line(a, p, 5.0),
                                        _ => p,
                                    };
                                    self.begin_stroke(start.unwrap_or(p), false);
                                    if start.is_some() {
                                        self.extend_stroke(end);
                                    }
                                }
                                Tool::PolygonFill => self.begin_poly_drag(p, false, false, mods.shift, false),
                                Tool::Picker => {
                                    self.pick_color(p);
                                    // The picker is temporary: back to the previous tool.
                                    if let Some(t) = self.prev_tool.take() {
                                        self.tool = t;
                                    }
                                }
                                Tool::Hide => self.begin_poly_drag(p, true, mods.command, mods.shift, false),
                                Tool::Select(_) => self.sel_press(p, false, mods),
                                Tool::Gradient => self.grad_drag = Some((false, p, p)),
                                Tool::Path => self.path_press(p, false, mods.shift),
                                Tool::Wand | Tool::Bucket => {
                                    if let Some(hit) = self.pick_at(p) {
                                        if let Some(doc) = &mut self.doc {
                                            doc.active = hit.set;
                                        }
                                        if self.tool == Tool::Wand {
                                            self.wand_click(hit.set, hit.uv, mods);
                                        } else {
                                            self.bucket_click(hit.set, hit.uv);
                                        }
                                    } else if self.tool == Tool::Wand && !mods.any() {
                                        self.deselect();
                                    }
                                }
                            }
                        }
                    }
                } else if pointer.primary_down() && self.poly_drag.as_ref().is_some_and(|d| !d.uv) {
                    if let Some(p) = pointer.interact_pos() {
                        self.poly_drag_at(local(p));
                    }
                } else if self.sel_drag.as_ref().is_some_and(|d| !d.uv) {
                    if let Some(p) = pointer.hover_pos() {
                        self.sel_move(local(p), pointer.primary_down());
                    }
                } else if pointer.primary_down() && self.path.drag.is_some() && self.selected_path().is_some_and(|p| !p.uv) {
                    if let Some(p) = pointer.interact_pos() {
                        self.path_drag_to(local(p));
                    }
                } else if pointer.primary_down() && self.grad_drag.is_some_and(|g| !g.0) {
                    if let (Some(p), Some(g)) = (pointer.interact_pos(), self.grad_drag.as_mut()) {
                        g.2 = local(p);
                    }
                } else if pointer.primary_down() && !mods.alt && self.stroke.as_ref().is_some_and(|s| !s.uv) {
                    if let Some(p) = pointer.interact_pos() {
                        self.extend_stroke(local(p));
                    }
                }
            }
        }
        if !pointer.primary_down() && self.stroke.is_some() {
            self.end_stroke();
        }
        if !pointer.primary_down() && self.poly_drag.is_some() {
            self.end_poly_drag();
        }
        if !pointer.primary_down() && self.sel_drag.as_ref().is_some_and(|d| !d.uv && !d.released) {
            self.sel_release();
        }
        if !pointer.primary_down() && self.grad_drag.is_some_and(|g| !g.0) {
            self.gradient_commit();
        }
        if !pointer.primary_down() && self.path.drag.is_some() {
            self.path_release();
        }

        let painter = ui.painter_at(rect);
        self.draw_sel_drag(&painter, rect, false);
        self.draw_grad_drag(&painter, rect, false);
        self.draw_path(&painter, rect, false, resp.hover_pos().map(local));
        if matches!(self.tool, Tool::PolygonFill | Tool::Hide) && !ui.input(|i| i.modifiers.alt || i.pointer.secondary_down() || i.pointer.middle_down()) {
            let hover = resp.hover_pos().map(local);
            self.draw_poly_overlay(&painter, rect, hover);
        }
        if self.stencil_active() {
            self.draw_stencil(ui.ctx(), &painter, rect);
        }
        // Clone: where the source is, and while painting the point sampled.
        if self.tool == Tool::Clone && self.doc.is_some() {
            let r = self.brush.size;
            let mark = |p: Pos2, strong: bool| {
                let c = if strong { Color32::from_rgb(90, 200, 255) } else { Color32::from_rgb(90, 170, 220) };
                painter.circle_stroke(p, r, egui::Stroke::new(2.5f32, Color32::from_black_alpha(120)));
                painter.circle_stroke(p, r, egui::Stroke::new(1.2f32, c));
                for d in [egui::vec2(1.0, 0.0), egui::vec2(0.0, 1.0)] {
                    painter.line_segment([p - d * 7.0, p + d * 7.0], egui::Stroke::new(1.5f32, c));
                }
            };
            match (self.clone_offset_px, resp.hover_pos()) {
                (Some(o), Some(p)) if self.stroke.is_some() => {
                    painter.line_segment([p, p + o], egui::Stroke::new(1.0f32, Color32::from_white_alpha(60)));
                    mark(p + o, true);
                }
                _ => {
                    if let Some(s) = self.clone_world.and_then(|w| self.project_vp(w)) {
                        let s = rect.min + egui::vec2(s.x, s.y);
                        if let Some(p) = resp.hover_pos() {
                            painter.line_segment([p, s], egui::Stroke::new(1.0f32, Color32::from_white_alpha(40)));
                        }
                        mark(s, false);
                        painter.text(s + egui::vec2(r + 6.0, -8.0), egui::Align2::LEFT_CENTER, "Source", egui::FontId::proportional(11.0), Color32::from_rgb(140, 200, 240));
                    }
                }
            }
        }
        if let Some(p) = resp.hover_pos() {
            if self.doc.is_some() && !mods.alt && self.tool.uses_brush() && !gizmo_busy {
                // During a Ctrl + drag gesture the circle stays at the press.
                let p = self.gesture.map(|g| g.anchor).unwrap_or(p);
                let r = self.brush.size;
                self.draw_alpha_overlay(ui.ctx(), &painter, p);
                painter.circle_stroke(p, r, egui::Stroke::new(2.5f32, Color32::from_black_alpha(140)));
                painter.circle_stroke(p, r, egui::Stroke::new(1.0f32, Color32::WHITE));
                let inner = r * self.brush.hardness;
                if inner > 2.0 && self.brush.alpha == AlphaSource::Builtin(crate::brush::AlphaKind::Round) {
                    painter.circle_stroke(p, inner, egui::Stroke::new(1.0f32, Color32::from_white_alpha(70)));
                }
                if self.brush.lazy > 0.0 {
                    if let Some(a) = &self.stroke {
                        if let Some(l) = a.stroke.last_point() {
                            painter.line_segment([rect.min + egui::vec2(l.x, l.y), p], egui::Stroke::new(1.0f32, Color32::YELLOW));
                        }
                    }
                }
            }
            if self.tool == Tool::Picker {
                painter.circle_stroke(p, 6.0, egui::Stroke::new(1.5f32, Color32::WHITE));
            }
            // Dotted guide of the straight line while Shift is held.
            if mods.shift && self.tool.is_brush() {
                if let Some(a) = self.line_end {
                    let cur = local(p);
                    let end = if mods.command { snap_line(a, cur, 5.0) } else { cur };
                    let a = rect.min + egui::vec2(a.x, a.y);
                    let b = rect.min + egui::vec2(end.x, end.y);
                    painter.extend(egui::Shape::dashed_line(&[a, b], egui::Stroke::new(1.0f32, Color32::WHITE), 4.0, 4.0));
                }
            }
        }
        // Overlays stay in the part of the view the panels leave visible.
        let vis = self.visible_part(rect);
        self.draw_axis_gizmo(&painter, vis);
        self.draw_selection_badge(&painter, vis);
        self.draw_toast(&painter, vis);
        self.draw_modifier_helper(ui.ctx(), &painter, vis, mods);
        if self.view.layout == Layout::Both {
            painter.text(vis.min + egui::vec2(12.0, 62.0), egui::Align2::LEFT_TOP, "3D", egui::FontId::proportional(12.0), Color32::from_white_alpha(110));
        }
    }

    /// Frames the mesh in the part of the 3D view the panels leave visible
    /// (the split view's half too): its bounding sphere fits there, centered.
    pub(super) fn frame_mesh(&mut self) {
        let Some(doc) = &self.doc else { return };
        let (min, max) = (doc.mesh.bbox_min, doc.mesh.bbox_max);
        let vp = self.vp_rect;
        if !vp.is_positive() {
            self.camera.frame(min, max);
            return;
        }
        let vis = self.visible_part(vp);
        let cam = &mut self.camera;
        let radius = ((max - min).length() * 0.5).max(1e-4);
        let t = (cam.fov_y * 0.5).tan();
        // Half extents of the visible part, as tangents of the view angle.
        let ty = t * vis.height() / vp.height();
        let tx = t * (vp.width() / vp.height()) * vis.width() / vp.width();
        let fit = ty.min(tx).max(1e-4);
        cam.target = ((min + max) * 0.5).into();
        cam.distance = if cam.ortho { radius / fit } else { radius / fit.atan().sin() } * 1.05;
        // Then slide the view so the mesh sits at the visible part's center.
        let o = vis.center() - vp.center();
        cam.pan(o.x, o.y, vp.height());
    }

    /// Fits the UV square in the part of the 2D view the panels leave visible.
    fn uv_fit(&mut self) {
        let w = self.engine.uvview.width as f32;
        let ppp = w / self.uv_rect.width().max(1.0);
        let vis = if self.canvas_mid.is_positive() && !self.ui_hidden { self.canvas_mid.intersect(self.uv_rect) } else { self.uv_rect };
        let vis = if vis.is_positive() { vis } else { self.uv_rect };
        let (x0, y0) = ((vis.min.x - self.uv_rect.min.x) * ppp, (vis.min.y - self.uv_rect.min.y) * ppp);
        let (vw, vh) = (vis.width() * ppp, vis.height() * ppp);
        let z = vw.min(vh) * 0.86;
        self.view.uv_zoom = z;
        self.view.uv_pan = [x0 + (vw - z) * 0.5, y0 + (vh - z) * 0.5];
    }

    /// View point (points, local to the 2D view) to UV.
    pub(super) fn uv_of(&self, p: Vec2) -> Vec2 {
        let ppp = self.engine.uvview.width as f32 / self.uv_rect.width().max(1.0);
        self.px_uv(p * ppp)
    }

    /// 2D view: UV to view pixels (of the 2D view texture): turned by the
    /// view rotation around UV (0, 0), scaled, then panned.
    pub(super) fn uv_px(&self, uv: Vec2) -> Vec2 {
        turn(uv * self.view.uv_zoom, self.view.uv_rot) + Vec2::from(self.view.uv_pan)
    }

    /// 2D view: view pixels to UV (`uv_px` the other way).
    pub(super) fn px_uv(&self, px: Vec2) -> Vec2 {
        turn(px - Vec2::from(self.view.uv_pan), -self.view.uv_rot) / self.view.uv_zoom.max(1e-6)
    }

    fn viewport_2d(&mut self, ui: &mut egui::Ui, rect: Rect) {
        self.uv_rect = rect;
        let ppp = ui.ctx().pixels_per_point();
        let (w, h) = ((rect.width() * ppp).round() as u32, (rect.height() * ppp).round() as u32);
        let resized = self.engine.uvview.resize(&self.engine.device, w, h);
        if resized || self.view.uv_zoom <= 0.0 {
            self.uv_fit();
        }
        if resized || self.uv_tex.is_none() {
            let mut r = self.renderer.write();
            let view = &self.engine.uvview.out.view;
            match self.uv_tex {
                Some(id) => r.update_egui_texture_from_wgpu_texture(&self.engine.device, view, wgpu::FilterMode::Linear, id),
                None => self.uv_tex = Some(r.register_native_texture(&self.engine.device, view, wgpu::FilterMode::Linear)),
            }
        }
        // Leave the panels' resize grips (a few points on each side) to them.
        let resp = ui.allocate_rect(rect.shrink2(egui::vec2(6.0, 0.0)), Sense::click_and_drag());
        ui.painter().image(self.uv_tex.unwrap(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        let (pointer, scroll, mods) = ui.input(|i| (i.pointer.clone(), i.raw_scroll_delta.y, i.modifiers));
        if pointer.any_pressed() {
            self.uv_press_ok = press_is_ours(ui, resp.id, self.overlay_open);
        }
        let ours = self.uv_press_ok;
        let delta = pointer.delta() * ppp;
        // MMB (or Alt + MMB, Ctrl + Alt + LMB) pans, Alt + RMB zooms, Alt +
        // LMB turns the view around the middle of its visible part, the area
        // the floating panels leave free (Shift: 15 degree steps).
        let turn_at = self.visible_part(rect).center();
        let pan = resp.dragged_by(egui::PointerButton::Middle) || (resp.dragged_by(egui::PointerButton::Primary) && mods.alt && mods.command);
        if ours && pan {
            self.view.uv_pan[0] += delta.x;
            self.view.uv_pan[1] += delta.y;
        }
        let turning = ours && resp.dragged_by(egui::PointerButton::Primary) && mods.alt && !mods.command;
        if turning {
            if let (Some(o), Some(p)) = (pointer.press_origin(), pointer.interact_pos()) {
                let c = Vec2::new((turn_at.x - rect.min.x) * ppp, (turn_at.y - rect.min.y) * ppp);
                let at = |q: Pos2| Vec2::new((q.x - rect.min.x) * ppp, (q.y - rect.min.y) * ppp) - c;
                let (a, b) = (at(o), at(p));
                if a.length() > 12.0 * ppp && b.length() > 12.0 * ppp {
                    let from = *self.uv_rot_from.get_or_insert(self.view.uv_rot);
                    let mut target = from + (b.to_angle() - a.to_angle());
                    if mods.shift {
                        let step = 15f32.to_radians();
                        target = (target / step).round() * step;
                    }
                    self.uv_turn_about(c, target - self.view.uv_rot);
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        } else {
            self.uv_rot_from = None;
        }
        if ours && resp.dragged_by(egui::PointerButton::Secondary) && mods.alt {
            if let Some(p) = pointer.press_origin() {
                let c = Vec2::new((p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp);
                let f = (1.0 - delta.y * 0.005).clamp(0.5, 2.0);
                self.view.uv_pan[0] = c.x - (c.x - self.view.uv_pan[0]) * f;
                self.view.uv_pan[1] = c.y - (c.y - self.view.uv_pan[1]) * f;
                self.view.uv_zoom *= f;
            }
        }
        if ours && resp.clicked_by(egui::PointerButton::Secondary) && !mods.any() && self.doc.is_some() {
            self.quick_props = pointer.interact_pos();
        }
        if resp.hovered() && scroll != 0.0 {
            if let Some(p) = resp.hover_pos() {
                let c = Vec2::new((p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp);
                let f = (1.0 + scroll * 0.0015).clamp(0.5, 2.0);
                self.view.uv_pan[0] = c.x - (c.x - self.view.uv_pan[0]) * f;
                self.view.uv_pan[1] = c.y - (c.y - self.view.uv_pan[1]) * f;
                self.view.uv_zoom *= f;
            }
        }
        let local = |p: Pos2| Vec2::new(p.x - rect.min.x, p.y - rect.min.y);
        let armed = ui.data(|d| d.get_temp::<egui::Id>(super::widgets::eyedropper_id())).is_some() || self.id_pick.is_some();
        // A selected UV-projected fill: the left button moves, rotates and
        // scales its projection (the manipulator), like Painter.
        let gizmo = !armed && self.uv_gizmo(ui, rect, &pointer, mods, ours);
        if self.doc.is_some() && self.bake_job.is_none() && self.tool == Tool::Gradient && !armed && !gizmo {
            if pointer.primary_pressed() && resp.hovered() && !mods.alt && ours {
                if let Some(p) = pointer.interact_pos() {
                    self.grad_drag = Some((true, local(p), local(p)));
                }
            } else if pointer.primary_down() && self.grad_drag.is_some_and(|g| g.0) {
                if let (Some(p), Some(g)) = (pointer.interact_pos(), self.grad_drag.as_mut()) {
                    g.2 = local(p);
                }
            }
            if !pointer.primary_down() && self.grad_drag.is_some_and(|g| g.0) {
                self.gradient_commit();
            }
        }
        if self.doc.is_some() && self.bake_job.is_none() && self.tool == Tool::Path && !armed && !gizmo {
            if pointer.primary_pressed() && resp.hovered() && !mods.alt && ours {
                if let Some(p) = pointer.interact_pos() {
                    self.path_press(local(p), true, mods.shift);
                }
            } else if pointer.primary_down() && self.path.drag.is_some() && self.selected_path().is_some_and(|p| p.uv) {
                if let Some(p) = pointer.interact_pos() {
                    self.path_drag_to(local(p));
                }
            }
            if !pointer.primary_down() && self.path.drag.is_some() {
                self.path_release();
            }
        }
        if self.doc.is_some() && self.bake_job.is_none() && matches!(self.tool, Tool::Wand | Tool::Bucket) && !armed && !gizmo {
            if pointer.primary_pressed() && resp.hovered() && !mods.alt && ours {
                if let Some(p) = pointer.interact_pos() {
                    let uv = self.uv_of(local(p));
                    let set = self.doc.as_ref().map_or(0, |d| d.active);
                    if (0.0..1.0).contains(&uv.x) && (0.0..1.0).contains(&uv.y) {
                        if self.tool == Tool::Wand {
                            self.wand_click(set, [uv.x, uv.y], mods);
                        } else {
                            self.bucket_click(set, [uv.x, uv.y]);
                        }
                    }
                }
            }
        }
        if self.doc.is_some() && self.bake_job.is_none() && matches!(self.tool, Tool::PolygonFill | Tool::Hide) && !armed && !gizmo {
            if pointer.primary_pressed() && resp.hovered() && !mods.alt && ours {
                if let Some(p) = pointer.interact_pos() {
                    let hide = self.tool == Tool::Hide;
                    self.begin_poly_drag(local(p), hide, hide && mods.command, mods.shift, true);
                }
            } else if pointer.primary_down() && self.poly_drag.as_ref().is_some_and(|d| d.uv) {
                if let Some(p) = pointer.interact_pos() {
                    self.poly_drag_at(local(p));
                }
            }
            if !pointer.primary_down() && self.poly_drag.as_ref().is_some_and(|d| d.uv) {
                self.end_poly_drag();
            }
        }
        if self.doc.is_some() && self.bake_job.is_none() && matches!(self.tool, Tool::Select(_)) && !armed && !gizmo {
            if pointer.primary_pressed() && resp.hovered() && !mods.alt && ours {
                if let Some(p) = pointer.interact_pos() {
                    self.sel_press(local(p), true, mods);
                }
            } else if self.sel_drag.as_ref().is_some_and(|d| d.uv) {
                if let Some(p) = pointer.hover_pos() {
                    self.sel_move(local(p), pointer.primary_down());
                }
            }
            if !pointer.primary_down() && self.sel_drag.as_ref().is_some_and(|d| d.uv && !d.released) {
                self.sel_release();
            }
        }
        if self.doc.is_some() && self.bake_job.is_none() && self.tool.is_brush() && !armed && !gizmo {
            if pointer.primary_pressed() && resp.hovered() && !mods.alt && !mods.command && ours {
                if let Some(p) = pointer.interact_pos() {
                    self.begin_stroke(local(p), true);
                }
            } else if pointer.primary_down() && self.stroke.as_ref().is_some_and(|s| s.uv) {
                if let Some(p) = pointer.interact_pos() {
                    self.extend_stroke(local(p));
                }
            }
        }
        if !pointer.primary_down() && self.stroke.is_some() {
            self.end_stroke();
        }
        let painter = ui.painter_at(rect);
        if matches!(self.tool, Tool::PolygonFill | Tool::Hide) && !mods.alt && !pointer.secondary_down() && !pointer.middle_down() {
            self.draw_poly_overlay_uv(&painter, rect, resp.hover_pos().map(local));
        }
        self.draw_sel_drag(&painter, rect, true);
        self.draw_grad_drag(&painter, rect, true);
        self.draw_path(&painter, rect, true, resp.hover_pos().map(local));
        if turning {
            // The point the view turns around, a spoke to the cursor and the
            // view's up direction.
            let c = turn_at;
            let ring = Color32::from_white_alpha(170);
            painter.circle_filled(c, 10.0, Color32::from_black_alpha(110));
            painter.circle_stroke(c, 10.0, egui::Stroke::new(1.5f32, crate::style::ACCENT));
            painter.line_segment([c - egui::vec2(16.0, 0.0), c + egui::vec2(16.0, 0.0)], egui::Stroke::new(1.0f32, ring));
            painter.line_segment([c - egui::vec2(0.0, 16.0), c + egui::vec2(0.0, 16.0)], egui::Stroke::new(1.0f32, ring));
            let up = turn(Vec2::new(0.0, -1.0), self.view.uv_rot);
            painter.arrow(c, egui::vec2(up.x, up.y) * 34.0, egui::Stroke::new(2.0f32, crate::style::ACCENT));
            if let Some(p) = pointer.interact_pos() {
                if (p - c).length() > 14.0 {
                    let d = (p - c).normalized();
                    painter.line_segment([c + d * 12.0, p], egui::Stroke::new(1.0f32, Color32::from_white_alpha(90)));
                }
            }
        }
        let vis = self.visible_part(rect);
        if self.view.layout == Layout::Only2D {
            self.draw_selection_badge(&painter, vis);
        }
        if let Some(p) = resp.hover_pos() {
            if self.tool.uses_brush() && self.doc.is_some() {
                self.draw_alpha_overlay(ui.ctx(), &painter, p);
                painter.circle_stroke(p, self.brush.size, egui::Stroke::new(1.0f32, Color32::WHITE));
            }
        }
        if self.view.layout == Layout::Both {
            painter.text(vis.min + egui::vec2(12.0, 62.0), egui::Align2::LEFT_TOP, "UV", egui::FontId::proportional(12.0), Color32::from_white_alpha(110));
        }
        // A turned view says so, with a button back to upright.
        if self.view.uv_rot.abs() > 1e-4 {
            let deg = self.view.uv_rot.to_degrees();
            let deg = (deg + 180.0).rem_euclid(360.0) - 180.0;
            let text = format!("View rotated {deg:.0}\u{00b0}");
            let g = painter.layout_no_wrap(text, egui::FontId::proportional(11.5), Color32::from_white_alpha(200));
            let at = vis.min + egui::vec2(12.0, if self.view.layout == Layout::Both { 80.0 } else { 62.0 });
            let chip = Rect::from_min_size(at, egui::vec2(g.size().x + 34.0, 22.0));
            let r = ui.interact(chip, ui.id().with("uv rot reset"), Sense::click()).on_hover_text("Turn the 2D view back upright (F also resets it)");
            super::mark(ui.ctx(), "uv rot reset", chip);
            painter.rect(chip, 11.0, Color32::from_black_alpha(if r.hovered() { 200 } else { 150 }), egui::Stroke::new(1.0f32, crate::style::ACCENT.gamma_multiply(0.7)));
            painter.galley(chip.min + egui::vec2(10.0, (22.0 - g.size().y) * 0.5), g, Color32::WHITE);
            super::icons::paint(&painter, Rect::from_center_size(Pos2::new(chip.max.x - 13.0, chip.center().y), egui::vec2(10.0, 10.0)), super::icons::Icon::Close, Color32::WHITE);
            if r.clicked() {
                let c = Vec2::new((turn_at.x - rect.min.x) * ppp, (turn_at.y - rect.min.y) * ppp);
                self.uv_turn_about(c, -self.view.uv_rot);
                self.view.uv_rot = 0.0;
            }
        }
    }

    /// Turns the 2D view by `d` radians around view pixel `c`.
    pub(super) fn uv_turn_about(&mut self, c: Vec2, d: f32) {
        let pan = c + turn(Vec2::from(self.view.uv_pan) - c, d);
        self.view.uv_pan = pan.into();
        self.view.uv_rot += d;
    }

    /// Motion of a Ctrl + drag brush gesture this frame. The cursor is
    /// hidden and sent back to the press point after each move, so it never
    /// wanders while the size, hardness, flow or angle change.
    fn gesture_delta(&mut self, ctx: &egui::Context, pointer: &egui::PointerState) -> egui::Vec2 {
        let Some(pos) = pointer.latest_pos() else { return egui::Vec2::ZERO };
        let start = pointer.press_origin().unwrap_or(pos);
        let lock = self.lock_cursor;
        let g = self.gesture.get_or_insert(super::BrushGesture { anchor: start, last: start, pending: None });
        if !lock {
            let d = pos - g.last;
            g.last = pos;
            return d;
        }
        ctx.set_cursor_icon(egui::CursorIcon::None);
        if g.pending == Some(pos) {
            // The warp back has not been seen yet: this is the old position.
            return egui::Vec2::ZERO;
        }
        let d = pos - g.last;
        if d != egui::Vec2::ZERO {
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorPosition(g.anchor));
            g.pending = Some(pos);
            g.last = g.anchor;
        }
        d
    }

    /// The stencil paints: the projection tool, or the brush's stencil
    /// option, with a texture chosen.
    pub(super) fn stencil_active(&self) -> bool {
        // Only the Projection tool projects: no other tool shows or paints it.
        self.stencil.resource.is_some() && self.tool == Tool::Projection
    }

    /// Centers the stencil in the visible part of the 3D view.
    pub(super) fn stencil_fit(&mut self) {
        let vis = self.visible_part(self.vp_rect);
        self.stencil.center = Vec2::new(vis.center().x - self.vp_rect.min.x, vis.center().y - self.vp_rect.min.y);
        self.stencil.size = (vis.height() * 0.7).max(50.0);
    }

    /// A new tool: the projection fits its stencil in view and shows the
    /// Stencil tab.
    pub(super) fn tool_changed(&mut self) {
        if self.tool == Tool::Projection {
            self.props_tab = 2;
            let vis = self.visible_part(self.vp_rect);
            let c = self.vp_rect.min + egui::vec2(self.stencil.center.x, self.stencil.center.y);
            // Never placed (still at its default spot) or out of view: centered.
            if !vis.contains(c) || self.stencil.center == Stencil::default().center {
                self.stencil_fit();
            }
            if self.stencil.resource.is_none() && self.doc.is_some() {
                self.toast("Projection: choose a texture in Properties > Stencil, or drop one from the Library.".into());
            }
        }
    }

    /// The brush's alpha under the cursor (scaled, rotated like the
    /// stamps), so image tips show what they will paint.
    pub(super) fn draw_alpha_overlay(&mut self, ctx: &egui::Context, painter: &egui::Painter, p: Pos2) {
        if self.brush.alpha == AlphaSource::Builtin(crate::brush::AlphaKind::Round) {
            return;
        }
        let Some(tex) = self.alpha_overlay_texture(ctx) else { return };
        let r = self.brush.size;
        let mut a = self.brush.angle.to_radians();
        if self.brush.follow_path {
            if let Some(d) = self.stroke.as_ref().map(|s| s.stroke.moved).filter(|d| d.length() > 0.5) {
                a += d.y.atan2(d.x);
            }
        }
        let (s, c) = a.sin_cos();
        let corner = |x: f32, y: f32| p + egui::vec2(x * c - y * s, x * s + y * c);
        let tint = Color32::from_rgba_unmultiplied(255, 255, 255, 120);
        let mut mesh = egui::Mesh::with_texture(tex);
        for (x, y, u, v) in [(-r, -r, 0.0, 0.0), (r, -r, 1.0, 0.0), (r, r, 1.0, 1.0), (-r, r, 0.0, 1.0)] {
            mesh.vertices.push(egui::epaint::Vertex { pos: corner(x, y), uv: Pos2::new(u, v), color: tint });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        painter.add(egui::Shape::mesh(mesh));
    }

    /// The current alpha as a white image with the alpha as coverage.
    fn alpha_overlay_texture(&mut self, ctx: &egui::Context) -> Option<egui::TextureId> {
        let key = format!("{:?}", self.brush.alpha);
        if let Some((k, h)) = &self.alpha_overlay {
            if *k == key {
                return Some(h.id());
            }
        }
        let img: image::RgbaImage = match &self.brush.alpha {
            AlphaSource::Builtin(k) => crate::brush::alpha_image(*k),
            AlphaSource::Resource(id) => (*self.doc.as_ref()?.resources.get(id)?.image).clone(),
        };
        let small = image::imageops::thumbnail(&img, 256, 256);
        let gray = image::DynamicImage::ImageRgba8(small).to_luma8();
        let px: Vec<u8> = gray.pixels().flat_map(|g| [255, 255, 255, g.0[0]]).collect();
        let ci = egui::ColorImage::from_rgba_unmultiplied([gray.width() as usize, gray.height() as usize], &px);
        let h = ctx.load_texture("alpha overlay", ci, egui::TextureOptions::LINEAR);
        let id = h.id();
        self.alpha_overlay = Some((key, h));
        Some(id)
    }

    fn draw_stencil(&mut self, ctx: &egui::Context, painter: &egui::Painter, rect: Rect) {
        let Some(id) = self.stencil.resource else { return };
        let Some(doc) = &self.doc else { return };
        let Some(res) = doc.resources.get(&id) else { return };
        let aspect = res.image.width() as f32 / res.image.height().max(1) as f32;
        let tex = self.shelf.texture_for_resource(ctx, res);
        let c = rect.min + egui::vec2(self.stencil.center.x, self.stencil.center.y);
        let (hw, hh) = (self.stencil.size * aspect * 0.5, self.stencil.size * 0.5);
        let a = self.stencil.rotation.to_radians();
        let (s, co) = a.sin_cos();
        let corner = |x: f32, y: f32| c + egui::vec2(x * co - y * s, x * s + y * co);
        let tint = Color32::from_white_alpha((self.stencil.opacity * 255.0) as u8);
        let mut mesh = egui::Mesh::with_texture(tex);
        let verts = [(-hw, -hh, 0.0, 0.0), (hw, -hh, 1.0, 0.0), (hw, hh, 1.0, 1.0), (-hw, hh, 0.0, 1.0)];
        for (x, y, u, v) in verts {
            mesh.vertices.push(egui::epaint::Vertex { pos: corner(x, y), uv: Pos2::new(u, v), color: tint });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        painter.add(egui::Shape::mesh(mesh));
    }

    pub(super) fn sync_brush_textures(&mut self) {
        if self.uploaded_alpha.as_ref() != Some(&self.brush.alpha) {
            if let Some(img) = self.alpha_tip(&self.brush.alpha) {
                self.engine.paint.alpha = crate::engine::paint::image_texture(&self.engine.device, &self.engine.queue, "brush alpha", &img, false);
            }
            self.uploaded_alpha = Some(self.brush.alpha.clone());
        }
        if self.uploaded_stencil != self.stencil.resource {
            if let Some(r) = self.stencil.resource.and_then(|id| self.doc.as_ref().and_then(|d| d.resources.get(&id))) {
                self.engine.paint.stencil =
                    crate::engine::paint::image_texture(&self.engine.device, &self.engine.queue, "stencil", &r.image, true);
            }
            self.uploaded_stencil = self.stencil.resource;
        }
    }

    fn brush_uniform(&self, mode: f32) -> BrushUniform {
        let view = self.view_info();
        let (near, far) = self.camera.near_far();
        let m = &self.mat;
        let b = &self.brush;
        let stencil_on = self.stencil_active() && mode != MODE_UV;
        let aspect = self
            .stencil
            .resource
            .and_then(|id| self.doc.as_ref()?.resources.get(&id))
            .map(|r| r.image.width() as f32 / r.image.height().max(1) as f32)
            .unwrap_or(1.0);
        let right = view.inv_view_proj * glam::Vec4::new(1.0, 0.0, 0.5, 1.0);
        let center = view.inv_view_proj * glam::Vec4::new(0.0, 0.0, 0.5, 1.0);
        let right = (right.truncate() / right.w - center.truncate() / center.w).normalize_or(Vec3::X);
        BrushUniform {
            view_proj: view.view_proj.to_cols_array_2d(),
            eye: view.eye.extend(1.0).into(),
            viewport: [view.size.x, view.size.y, near, far],
            color: [m.color[0], m.color[1], m.color[2], m.opacity],
            values: [m.roughness, m.metallic, m.height, m.mask_value],
            emissive: [m.emissive[0], m.emissive[1], m.emissive[2], 0.0],
            normal: [m.normal[0], m.normal[1], m.normal[2], 0.0],
            params: [b.hardness, if b.angle_fade { 1.0 } else { 0.0 }, 0.0, mode],
            shape: [
                if b.alpha == AlphaSource::Builtin(crate::brush::AlphaKind::Round) { 1.0 } else { 0.0 },
                if stencil_on { 1.0 } else { 0.0 },
                if self.stencil.drive_color { 1.0 } else { 0.0 },
                if self.stencil.drive_height { 1.0 } else { 0.0 },
            ],
            stencil: [
                self.stencil.center.x * view.ppp,
                self.stencil.center.y * view.ppp,
                self.stencil.size * view.ppp,
                self.stencil.rotation.to_radians(),
            ],
            stencil2: [aspect, self.stencil.height_strength, if self.camera.ortho { 1.0 } else { 0.0 }, 0.0],
            right: [right.x, right.y, right.z, 0.0],
            material: if matches!(self.tool, Tool::Clone | Tool::Smudge) && self.engine.paint.material.is_some() {
                [1.0, 1.0, self.sample_offset[0], self.sample_offset[1]]
            } else {
                [if self.mat.material.is_some() && self.engine.paint.material.is_some() { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0]
            },
        }
    }

    /// Viewport-local position (points) of a mesh-space point.
    pub(super) fn project_vp(&self, w: Vec3) -> Option<Vec2> {
        let r = self.vp_rect;
        let c = self.camera.view_proj(r.width() / r.height().max(1.0)) * w.extend(1.0);
        if c.w <= 1e-5 {
            return None;
        }
        let n = c.truncate() / c.w;
        Some(Vec2::new((n.x * 0.5 + 0.5) * r.width(), (0.5 - n.y * 0.5) * r.height()))
    }

    fn set_clone_source(&mut self, p: Vec2) {
        match self.pick_at(p) {
            Some(h) => {
                self.clone_src = Some((h.set, h.uv));
                self.clone_world = Some(h.pos);
                self.status = format!("Clone source set at UV ({:.3}, {:.3}). Paint to clone.", h.uv[0], h.uv[1]);
            }
            None => self.status = "Clone: press V over the mesh to set the source.".into(),
        }
    }

    /// Copies the active set's final maps into the brush material textures
    /// (clone and smudge sample them at an offset).
    fn snapshot_finals(&mut self, enc: &mut wgpu::CommandEncoder, region: Option<crate::engine::composite::Region>) {
        let Some(doc) = &self.doc else { return };
        let s = &doc.sets[doc.active];
        let res = s.resolution();
        let reuse = self.engine.paint.material.as_ref().is_some_and(|t| t[0].width == res);
        if !reuse {
            let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
            let t = |l| Tex::new(&self.engine.device, l, res, res, wgpu::TextureFormat::Rgba16Float, usage);
            self.engine.paint.material = Some([t("bm col"), t("bm mrh"), t("bm emi"), t("bm nrm")]);
        }
        let m = self.engine.paint.material.as_ref().unwrap();
        let g = &s.gpu;
        let r = region.map(|r| [r[0].min(res), r[1].min(res), r[2].min(res), r[3].min(res)]).unwrap_or([0, 0, res, res]);
        if r[2] <= r[0] || r[3] <= r[1] {
            return;
        }
        let origin = wgpu::Origin3d { x: r[0], y: r[1], z: 0 };
        for (src, dst) in [(&g.final_base, &m[0]), (&g.final_orm, &m[1]), (&g.final_emissive, &m[2]), (&g.final_normal, &m[3])] {
            enc.copy_texture_to_texture(
                wgpu::ImageCopyTexture { texture: &src.texture, mip_level: 0, origin, aspect: wgpu::TextureAspect::All },
                wgpu::ImageCopyTexture { texture: &dst.texture, mip_level: 0, origin, aspect: wgpu::TextureAspect::All },
                wgpu::Extent3d { width: r[2] - r[0], height: r[3] - r[1], depth_or_array_layers: 1 },
            );
        }
    }

    /// Evaluates the brush material over the active set before a stroke.
    pub(super) fn prepare_brush_material(&mut self) {
        let Some((_, fill)) = self.mat.material.clone() else {
            self.engine.paint.material = None;
            return;
        };
        let Some(doc) = &self.doc else { return };
        let s = &doc.sets[doc.active];
        let res = s.resolution();
        let reuse = self.engine.paint.material.as_ref().is_some_and(|t| t[0].width == res);
        if !reuse {
            let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
            let t = |l| Tex::new(&self.engine.device, l, res, res, wgpu::TextureFormat::Rgba16Float, usage);
            self.engine.paint.material = Some([t("bm col"), t("bm mrh"), t("bm emi"), t("bm nrm")]);
        }
        let mut enc = self.engine.device.create_command_encoder(&Default::default());
        s.evaluate_material(&self.engine, &mut enc, &doc.resources, &fill, self.engine.paint.material.as_ref().unwrap());
        self.engine.queue.submit(Some(enc.finish()));
    }

    pub(super) fn begin_stroke(&mut self, pos: Vec2, uv: bool) {
        if self.tool == Tool::SelBrush {
            self.begin_selection_stroke(pos, uv);
            return;
        }
        let (lid, target) = match self.resolve_paint_target() {
            Ok(v) => v,
            Err(e) => {
                self.toast(e);
                return;
            }
        };
        let erase = self.tool == Tool::Erase;
        let mask = matches!(target, PaintTarget::MaskPaint(_));
        let channels = if erase { [true; 7] } else { self.mat.channels };
        let doc = self.doc.as_mut().unwrap();
        let set = doc.active;
        let res = doc.sets[set].resolution();
        let Some(layer) = crate::document::find_mut(&mut doc.sets[set].layers, lid) else { return };
        // The eraser only clears emissive / normal / opacity a layer has:
        // it never allocates them.
        let has_b = layer.paint.as_ref().is_some_and(|p| p.emis.is_some());
        let needs_b = !mask && (channels[4] || channels[5] || channels[6]) && (!erase || has_b);
        if !erase && !mask && channels[5] {
            layer.desc.opacity_painted = true;
        }
        layer.materialize(&self.engine.device, res, match target {
            PaintTarget::MaskPaint(eid) => Some(eid),
            PaintTarget::Content => None,
        });
        if needs_b {
            if let Some(p) = &mut layer.paint {
                p.ensure_b(&self.engine.device);
            }
        }
        let textures: Vec<Arc<Tex>> = match target {
            PaintTarget::Content => layer.paint.as_ref().map(|p| p.all().into_iter().cloned().collect()).unwrap_or_default(),
            PaintTarget::MaskPaint(eid) => layer.mask_paint.get(&eid).cloned().into_iter().collect(),
        };
        if self.tool == Tool::Clone && self.clone_src.is_none() && !mask {
            self.status = "Clone: press V over the mesh to set the source first.".into();
            return;
        }
        // The clone samples the texture set it paints.
        if self.tool == Tool::Clone && self.clone_src.is_some_and(|(s, _)| s != set) {
            self.toast("Clone: the source is on another texture set. Press V over this one to set a source here.".into());
            return;
        }
        self.history.push(Entry::stroke_tiles(if erase { "Erase stroke" } else { "Paint stroke" }, set, textures));
        let history_version = self.history.version;
        if matches!(self.tool, Tool::Clone | Tool::Smudge) {
            let res = self.doc.as_ref().map(|d| d.sets[d.active].resolution() as f32).unwrap_or(1.0);
            let here = if uv { Some(self.uv_of(pos).to_array()) } else { self.pick_at(pos).map(|h| h.uv) };
            if self.tool == Tool::Clone {
                match (self.clone_src, here) {
                    (Some((_, src)), Some(dst)) => {
                        self.sample_offset = [(src[0] - dst[0]) * res, (src[1] - dst[1]) * res];
                        // On screen the sampled point keeps this offset from the brush.
                        self.clone_offset_px = match (self.clone_world, self.project_vp(self.clone_world.unwrap_or_default())) {
                            (Some(_), Some(s)) => Some(egui::vec2(s.x - pos.x, s.y - pos.y)),
                            _ => None,
                        };
                    }
                    _ => {
                        self.status = "Clone: press V over the mesh to set the source first.".into();
                        self.history.undo.pop();
                        return;
                    }
                }
            } else {
                self.smudge_prev = here;
                self.sample_offset = [0.0; 2];
            }
            let mut enc = self.engine.device.create_command_encoder(&Default::default());
            self.snapshot_finals(&mut enc, None);
            self.engine.queue.submit(Some(enc.finish()));
            self.prev_region = None;
        } else if !mask && !erase {
            self.prepare_brush_material();
        }
        let space = if uv {
            Space::Uv
        } else if self.brush.alignment == crate::brush::Alignment::Camera {
            Space::Camera
        } else {
            Space::Wrap
        };
        let seed = Instant::now().elapsed().as_nanos() as u64 ^ (pos.x.to_bits() as u64) << 7 ^ 0x9E37_79B9;
        self.stroke = Some(ActiveStroke { stroke: Stroke::new(space, set, seed), layer_id: lid, target, erase, channels, uv, selection: false, history_version });
        self.extend_stroke(pos);
    }

    /// Selection brush: the selection is copied (the old one is the undo
    /// step), then the stroke paints the copy.
    fn begin_selection_stroke(&mut self, pos: Vec2, uv: bool) {
        let Some(doc) = &self.doc else { return };
        let set = doc.active;
        let s = &doc.sets[set];
        let res = s.resolution();
        let dev = &self.engine.device;
        let copy = match &s.selection {
            Some(cur) => {
                let mut enc = dev.create_command_encoder(&Default::default());
                let t = cur.duplicate(dev, &mut enc, "selection");
                self.engine.queue.submit(Some(enc.finish()));
                t
            }
            None if self.sel_brush_add => crate::engine::select::new_selection(dev, &self.engine.queue, res),
            None => {
                self.status = "Nothing is selected.".into();
                return;
            }
        };
        self.set_selection(set, Some(Arc::new(copy)), if self.sel_brush_add { "Selection brush" } else { "Selection brush (subtract)" });
        let history_version = self.history.version;
        let space = if uv {
            Space::Uv
        } else if self.brush.alignment == crate::brush::Alignment::Camera {
            Space::Camera
        } else {
            Space::Wrap
        };
        let seed = Instant::now().elapsed().as_nanos() as u64 ^ (pos.x.to_bits() as u64) << 7 ^ 0x9E37_79B9;
        self.stroke = Some(ActiveStroke {
            stroke: Stroke::new(space, set, seed),
            layer_id: 0,
            target: PaintTarget::Content,
            erase: !self.sel_brush_add,
            channels: [true; 7],
            uv,
            selection: true,
            history_version,
        });
        self.extend_stroke(pos);
    }

    /// Stamps of a selection brush stroke, painted into the selection.
    fn flush_selection_stamps(&mut self, encoder: &mut wgpu::CommandEncoder, stamps: &[Stamp]) {
        let Some(a) = &self.stroke else { return };
        let mode = match a.stroke.space {
            Space::Camera => MODE_CAMERA,
            Space::Wrap => MODE_WRAP,
            Space::Uv => MODE_UV,
        };
        let (set, erase) = (a.stroke.set, a.erase);
        let uniform = self.brush_uniform(mode);
        // Never sample the texture being painted.
        self.engine.paint.selection = None;
        let Some(doc) = &self.doc else { return };
        let Some(s) = doc.sets.get(set) else { return };
        let Some(sel) = s.selection.clone() else { return };
        self.engine.paint.upload(&self.engine.queue, uniform, stamps);
        let pass = if erase { PaintPass::SelectErase } else { PaintPass::Select };
        self.engine.paint.draw(&self.engine.device, encoder, &doc.gpu_mesh, s.submesh, None, &[&sel], self.engine.viewport.depth(), pass, [true; 7]);
    }

    /// 2D view: view points to UV, and UV units per point.
    pub(super) fn uv_mapping(&self) -> (impl Fn(Vec2) -> Vec2, f32) {
        let uv_ppp = self.engine.uvview.width as f32 / self.uv_rect.width().max(1.0);
        let (pan, zoom, rot) = (Vec2::from(self.view.uv_pan), self.view.uv_zoom.max(1e-3), self.view.uv_rot);
        (move |p: Vec2| turn(p * uv_ppp - pan, -rot) / zoom, uv_ppp / zoom)
    }

    pub(super) fn extend_stroke(&mut self, pos: Vec2) {
        let view = self.view_info();
        let (to_uv, uv_per_point) = self.uv_mapping();
        if self.tool == Tool::Smudge {
            let is_uv = self.stroke.as_ref().is_some_and(|s| s.uv);
            let res = self.doc.as_ref().map(|d| d.sets[d.active].resolution() as f32).unwrap_or(1.0);
            let here = if is_uv { Some(self.uv_of(pos).to_array()) } else { self.pick_at(pos).map(|h| h.uv) };
            if let (Some(prev), Some(cur)) = (self.smudge_prev, here) {
                let k = self.smudge_strength;
                // Pull material from behind the motion.
                self.sample_offset = [(prev[0] - cur[0]) * res * k, (prev[1] - cur[1]) * res * k];
            }
            if here.is_some() {
                self.smudge_prev = here;
            }
        }
        let (Some(a), Some(doc)) = (self.stroke.as_mut(), self.doc.as_ref()) else { return };
        a.stroke.pressure = self.pressure;
        let Some(to) = a.stroke.lazy(pos, self.brush.lazy) else { return };
        let stamps = a.stroke.advance(to, &self.brush, &view, doc, &to_uv, uv_per_point);
        // Beyond one flush's capacity the stamps wait for the next frames.
        self.pending.extend(stamps);
    }

    pub(super) fn end_stroke(&mut self) {
        // A follow-path click places its stamp now.
        let view = self.view_info();
        let (to_uv, uv_per_point) = self.uv_mapping();
        if let (Some(a), Some(doc)) = (self.stroke.as_mut(), self.doc.as_ref()) {
            let stamps = a.stroke.finish(&self.brush, &view, doc, &to_uv, uv_per_point);
            self.pending.extend(stamps);
        }
        // Stamps not painted yet (press and release in one frame): the
        // stroke ends after the next flush.
        if !self.pending.is_empty() {
            self.stroke_end_pending = true;
            return;
        }
        self.stroke_end_pending = false;
        self.clone_offset_px = None;
        // The stroke's backup grew while painting; a stroke that painted
        // nothing leaves no undo step.
        self.history.trim();
        if let Some(a) = self.stroke.take() {
            if self.history.version == a.history_version && self.history.undo.last().is_some_and(|e| e.is_empty_stroke()) {
                self.history.undo.pop();
                self.history.version += 1;
            }
            if !a.uv {
                self.line_end = a.stroke.last_point();
            }
            if let Some(doc) = &mut self.doc {
                doc.modified = true;
            }
        }
    }

    pub(super) fn flush_stamps(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.pending.is_empty() {
            return;
        }
        let n = self.pending.len().min(crate::engine::paint::MAX_STAMPS);
        let stamps: Vec<Stamp> = self.pending.drain(..n).collect();
        let Some(a) = &self.stroke else {
            self.pending.clear();
            return;
        };
        if a.selection {
            self.flush_selection_stamps(encoder, &stamps);
            return;
        }
        let mode = match a.stroke.space {
            Space::Camera => MODE_CAMERA,
            Space::Wrap => MODE_WRAP,
            Space::Uv => MODE_UV,
        };
        let (set, lid, target, erase, channels) = (a.stroke.set, a.layer_id, a.target, a.erase, a.channels);
        let space = a.stroke.space;
        // Only the texels under the stamps are painted and recomposited.
        let view = self.view_info();
        let region = match &self.doc {
            Some(doc) => match doc.sets.get(set) {
                Some(s) => self.region_finder.region(&doc.mesh, s.submesh, s.resolution(), &view, space, &stamps),
                None => None,
            },
            None => None,
        };
        let Some(region) = region else { return };
        if self.history.version == a.history_version {
            if let Some(e) = self.history.undo.last_mut() {
                e.backup_region(&self.engine.device, encoder, region);
            }
        }
        if self.tool == Tool::Smudge {
            // The snapshot is current except where the last flush painted.
            if let Some(r) = self.prev_region {
                self.snapshot_finals(encoder, Some(r));
            }
            self.prev_region = Some(region);
        }
        let uniform = self.brush_uniform(mode);
        self.bind_selection(set);
        let Some(doc) = &mut self.doc else { return };
        let Some(s) = doc.sets.get_mut(set) else { return };
        let Some(layer) = find(&s.layers, lid) else { return };
        self.engine.paint.upload(&self.engine.queue, uniform, &stamps);
        let mask = matches!(target, PaintTarget::MaskPaint(_));
        for pass in passes_for(erase, mask, channels) {
            let targets: Vec<&Tex> = match (pass, target) {
                (_, PaintTarget::MaskPaint(eid)) => layer.mask_paint.get(&eid).map(|t| &**t).into_iter().collect(),
                (PaintPass::PaintA | PaintPass::EraseA, _) => {
                    layer.paint.as_ref().map(|p| vec![&*p.color, &*p.vals, &*p.alph]).unwrap_or_default()
                }
                _ => layer
                    .paint
                    .as_ref()
                    .and_then(|p| Some(vec![&**p.emis.as_ref()?, &**p.nrm.as_ref()?, &**p.opac.as_ref()?]))
                    .unwrap_or_default(),
            };
            if targets.is_empty() {
                continue;
            }
            self.engine.paint.scissor = Some(region);
            self.engine.paint.draw(
                &self.engine.device,
                encoder,
                &doc.gpu_mesh,
                s.submesh,
                None,
                &targets,
                self.engine.viewport.depth(),
                pass,
                channels,
            );
            self.engine.paint.scissor = None;
        }
        if let Some(l) = crate::document::find_mut(&mut s.layers, lid) {
            l.rev = crate::document::next_rev();
        }
        // Most of the texture under the brush: a full pass is as cheap.
        let res = s.resolution() as u64;
        let r = match s.dirty_region {
            Some(d) => [d[0].min(region[0]), d[1].min(region[1]), d[2].max(region[2]), d[3].max(region[3])],
            None => region,
        };
        if (r[2] - r[0]) as u64 * (r[3] - r[1]) as u64 * 10 > res * res * 6 {
            s.dirty = true;
        } else {
            s.dirty_region = Some(r);
        }
        doc.modified = true;
    }

    pub(super) fn pick_at(&self, p: Vec2) -> Option<crate::document::Hit> {
        let view = self.view_info();
        let (o, d) = view.ray(p * view.ppp);
        self.doc.as_ref()?.pick(o, d)
    }

    /// Polygon topology of submesh `sub` (cached per mesh).
    fn poly_topo(&mut self, sub: usize) -> Option<Arc<super::polysel::PolyTopo>> {
        let doc = self.doc.as_ref()?;
        let key = (Arc::as_ptr(&doc.mesh) as usize ^ (self.generation as usize).rotate_left(40), sub);
        if let Some((k, t)) = &self.poly_topo {
            if *k == key {
                return Some(t.clone());
            }
        }
        let t = Arc::new(super::polysel::PolyTopo::build(&doc.mesh, sub));
        self.poly_topo = Some((key, t.clone()));
        Some(t)
    }

    /// Triangles of the group (triangle, polygon, mesh part or UV chunk)
    /// holding each of `tris`, for the fill mode.
    fn expand_groups(&mut self, sub: usize, tris: &[u32]) -> Vec<u32> {
        match self.poly_mode {
            PolyMode::Triangle => tris.to_vec(),
            PolyMode::Polygon => {
                let Some(topo) = self.poly_topo(sub) else { return Vec::new() };
                let polys: HashSet<u32> = tris.iter().filter_map(|t| topo.tri_poly.get(*t as usize).copied()).collect();
                polys.into_iter().flat_map(|p| topo.tris[p as usize].iter().copied()).collect()
            }
            PolyMode::Mesh | PolyMode::UvIsland => {
                let Some(doc) = &self.doc else { return Vec::new() };
                let labels = crate::bake::components(&doc.mesh, sub, self.poly_mode == PolyMode::UvIsland);
                let wanted: HashSet<u32> = tris.iter().filter_map(|t| labels.get(*t as usize).copied()).collect();
                labels.iter().enumerate().filter(|(_, l)| wanted.contains(l)).map(|(i, _)| i as u32).collect()
            }
        }
    }

    /// UV to 2D view local points (`uv_of` the other way).
    pub(super) fn uv_point(&self, uv: Vec2) -> Vec2 {
        let ppp = self.engine.uvview.width as f32 / self.uv_rect.width().max(1.0);
        self.uv_px(uv) / ppp
    }

    /// The (shown) triangle of submesh `sub` under 2D view point `p`.
    fn uv_pick_tri(&self, sub: usize, p: Vec2) -> Option<u32> {
        let doc = self.doc.as_ref()?;
        let q = self.uv_of(p);
        let s = doc.mesh.submeshes.get(sub)?;
        let hidden = doc.hidden.get(sub);
        let uv = |i: u32| Vec2::from(doc.mesh.vertices[i as usize].uv);
        (0..(s.indices.len() / 3) as u32).find(|&t| {
            if hidden.and_then(|h| h.get(t as usize)).copied().unwrap_or(false) {
                return false;
            }
            let i = t as usize * 3;
            let (a, b, c) = (uv(s.indices[i]), uv(s.indices[i + 1]), uv(s.indices[i + 2]));
            let side = |p0: Vec2, p1: Vec2| (p1 - p0).perp_dot(q - p0);
            let (d0, d1, d2) = (side(a, b), side(b, c), side(c, a));
            (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
        })
    }

    /// The triangle of `set` under `p` (3D view, or 2D view when `uv`).
    fn poly_pick(&self, p: Vec2, uv: bool, set: usize) -> Option<u32> {
        if uv {
            let sub = self.doc.as_ref()?.sets.get(set)?.submesh;
            return self.uv_pick_tri(sub, p);
        }
        self.pick_at(p).filter(|h| h.set == set).map(|h| h.tri)
    }

    /// World points of polygon `poly` to 2D view points: the affine map its
    /// corners fit best (loops pick their edges by what is on screen).
    fn poly_uv_screen(&mut self, sub: usize, poly: u32) -> Box<dyn Fn(Vec3) -> Option<Vec2>> {
        let Some(topo) = self.poly_topo(sub) else { return Box::new(|_| None) };
        let Some(doc) = &self.doc else { return Box::new(|_| None) };
        let s = &doc.mesh.submeshes[sub];
        let corners: Vec<(Vec3, Vec2)> = topo.tris[poly as usize]
            .iter()
            .flat_map(|&t| (0..3).map(move |k| s.indices[t as usize * 3 + k]))
            .map(|i| (Vec3::from(doc.mesh.vertices[i as usize].pos), self.uv_point(Vec2::from(doc.mesh.vertices[i as usize].uv))))
            .collect();
        let c = corners.iter().map(|x| x.0).sum::<Vec3>() / corners.len().max(1) as f32;
        let n = corners.chunks_exact(3).map(|t| (t[1].0 - t[0].0).cross(t[2].0 - t[0].0)).sum::<Vec3>().normalize_or(Vec3::Z);
        let tx = n.any_orthonormal_vector();
        let ty = n.cross(tx);
        let local = move |w: Vec3| Vec3::new((w - c).dot(tx), (w - c).dot(ty), 1.0);
        // Least squares: view point = A (x, y, 1).
        let mut m = glam::Mat3::ZERO;
        let (mut rx, mut ry) = (Vec3::ZERO, Vec3::ZERO);
        for (w, q) in &corners {
            let v = local(*w);
            m += glam::Mat3::from_cols(v * v.x, v * v.y, v * v.z);
            rx += v * q.x;
            ry += v * q.y;
        }
        if m.determinant().abs() < 1e-12 {
            let first = corners.first().map(|x| x.1);
            return Box::new(move |_| first);
        }
        let inv = m.inverse();
        let (ax, ay) = (inv * rx, inv * ry);
        Box::new(move |w: Vec3| {
            let v = local(w);
            Some(Vec2::new(ax.dot(v), ay.dot(v)))
        })
    }

    /// Triangles of submesh `sub` the 2D view outline `shape` selects.
    fn area_tris_uv(&mut self, sub: usize, shape: &[Vec2], rect: bool) -> Vec<u32> {
        if shape.len() < 3 {
            return Vec::new();
        }
        let lo = shape.iter().fold(Vec2::splat(f32::MAX), |m, p| m.min(*p));
        let hi = shape.iter().fold(Vec2::splat(f32::MIN), |m, p| m.max(*p));
        let inside_shape = |q: Vec2| -> bool { q.x >= lo.x && q.x <= hi.x && q.y >= lo.y && q.y <= hi.y && (rect || super::polysel::point_in_polygon(q, shape)) };
        let hit_test = self.poly_hit;
        let polys = self.poly_mode == PolyMode::Polygon && hit_test == PolyHit::Center;
        let topo = if polys { self.poly_topo(sub) } else { None };
        let Some(doc) = &self.doc else { return Vec::new() };
        let s = &doc.mesh.submeshes[sub];
        let hidden = doc.hidden.get(sub);
        let pt = |i: u32| self.uv_point(Vec2::from(doc.mesh.vertices[i as usize].uv));
        let tri = |t: u32| -> [Vec2; 3] { std::array::from_fn(|k| pt(s.indices[t as usize * 3 + k])) };
        let shown = |t: u32| !hidden.and_then(|h| h.get(t as usize)).copied().unwrap_or(false);
        if let Some(topo) = topo {
            // A polygon whose center (in UV) is inside.
            return (0..topo.tris.len())
                .filter(|&p| {
                    let ts = &topo.tris[p];
                    let n = (ts.len() * 3).max(1) as f32;
                    let c = ts.iter().map(|&t| tri(t).iter().copied().sum::<Vec2>()).sum::<Vec2>() / n;
                    ts.iter().any(|&t| shown(t)) && inside_shape(c)
                })
                .flat_map(|p| topo.tris[p].iter().copied())
                .collect();
        }
        (0..(s.indices.len() / 3) as u32)
            .filter(|&t| {
                if !shown(t) {
                    return false;
                }
                let p = tri(t);
                match hit_test {
                    PolyHit::Center => inside_shape((p[0] + p[1] + p[2]) / 3.0),
                    PolyHit::Intersection if rect => super::polysel::tri_rect_overlap(p, [lo, hi]),
                    PolyHit::Intersection => super::polysel::tri_polygon_overlap(p, shape, [lo, hi]),
                }
            })
            .collect()
    }

    /// Polygon fill overlays in the 2D view: the selection being built (or
    /// its outline) and the polygon under the cursor, in UV space.
    fn draw_poly_overlay_uv(&mut self, painter: &egui::Painter, rect: Rect, hover: Option<Vec2>) {
        let fill = Color32::from_rgba_unmultiplied(255, 140, 40, 70);
        let line = egui::Stroke::new(1.0f32, Color32::from_rgb(255, 160, 70));
        let to_screen = |q: Vec2| rect.min + egui::vec2(q.x, q.y);
        let tris_mesh = |this: &Self, sub: usize, tris: &mut dyn Iterator<Item = u32>| {
            let Some(doc) = &this.doc else { return };
            let s = &doc.mesh.submeshes[sub];
            let mut mesh = egui::Mesh::default();
            for t in tris.take(60_000) {
                let base = mesh.vertices.len() as u32;
                for k in 0..3 {
                    let uv = Vec2::from(doc.mesh.vertices[s.indices[t as usize * 3 + k] as usize].uv);
                    mesh.colored_vertex(to_screen(this.uv_point(uv)), fill);
                }
                mesh.add_triangle(base, base + 1, base + 2);
            }
            painter.add(egui::Shape::mesh(mesh));
        };
        if let Some(drag) = self.poly_drag.as_ref().filter(|d| d.uv) {
            match drag.select {
                PolySelect::Lasso if drag.moved && drag.path.len() >= 2 => {
                    let pts: Vec<Pos2> = drag.path.iter().map(|q| to_screen(*q)).collect();
                    painter.add(egui::Shape::line(pts.clone(), line));
                    painter.extend(egui::Shape::dashed_line(&[*pts.last().unwrap(), pts[0]], line, 4.0, 3.0));
                }
                PolySelect::Marquee if drag.moved => {
                    let r = Rect::from_two_pos(to_screen(drag.start), to_screen(drag.cur));
                    painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(255, 140, 40, 25));
                    let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                    for w in c.windows(2) {
                        painter.extend(egui::Shape::dashed_line(&[w[0], w[1]], line, 4.0, 3.0));
                    }
                }
                PolySelect::Lasso | PolySelect::Marquee => {}
                _ => tris_mesh(self, drag.sub, &mut drag.tris.clone().into_iter()),
            }
            return;
        }
        let Some(p) = hover else { return };
        if !matches!(self.poly_mode, PolyMode::Polygon | PolyMode::Triangle) && self.poly_select.is_area() {
            return;
        }
        let Some(sub) = self.doc.as_ref().map(|d| d.sets[d.active].submesh) else { return };
        let Some(t) = self.uv_pick_tri(sub, p) else { return };
        if self.poly_mode == PolyMode::Triangle && self.poly_select.is_area() {
            tris_mesh(self, sub, &mut std::iter::once(t));
        } else if let Some(topo) = self.poly_topo(sub) {
            let ts = topo.tris[topo.tri_poly[t as usize] as usize].clone();
            tris_mesh(self, sub, &mut ts.into_iter());
        }
    }

    /// Starts a polygon fill (`hide` false) or geometry mask drag at `p`
    /// (2D view local when `uv`).
    fn begin_poly_drag(&mut self, p: Vec2, hide: bool, isolate: bool, shift: bool, uv: bool) {
        let hit = if uv { None } else { self.pick_at(p) };
        let Some(doc) = &self.doc else { return };
        // Polygon fill works on the active texture set only (its wireframe
        // is shown); the geometry mask on the set under the cursor.
        let set = match (&hit, hide) {
            (Some(h), true) => h.set,
            _ => doc.active,
        };
        let sub = doc.sets[set].submesh;
        let hit_tri = if uv { self.uv_pick_tri(sub, p) } else { hit.as_ref().filter(|h| h.set == set).map(|h| h.tri) };
        let mut drag = PolyDrag {
            set,
            sub,
            hide,
            isolate,
            start: p,
            cur: p,
            moved: false,
            select: self.poly_select,
            tris: Vec::new(),
            polys: HashSet::new(),
            loop_dir: None,
            path: vec![p],
            partial: None,
            primary_only: shift,
            uv,
        };
        match (drag.select, hit_tri) {
            (PolySelect::Loop, Some(tri)) => {
                if let Some(topo) = self.poly_topo(sub) {
                    let poly = topo.tri_poly[tri as usize];
                    let screen: Box<dyn Fn(Vec3) -> Option<Vec2>> = if uv { self.poly_uv_screen(sub, poly) } else { Box::new(|w: Vec3| self.project_vp(w)) };
                    let k = topo.nearest_edge(poly, &screen, p);
                    drag.loop_dir = Some(topo.edge_dir(poly, k));
                    for q in topo.quad_loop(poly, k) {
                        if drag.polys.insert(q) {
                            drag.tris.extend(topo.tris[q as usize].iter().copied());
                        }
                    }
                }
            }
            (PolySelect::PartialLoop, Some(tri)) => {
                if let Some(topo) = self.poly_topo(sub) {
                    let poly = topo.tri_poly[tri as usize];
                    drag.partial = Some((poly, None));
                    drag.tris = topo.tris[poly as usize].clone();
                }
            }
            _ => {}
        }
        self.poly_drag = Some(drag);
    }

    /// The cursor moved during a polygon drag.
    fn poly_drag_at(&mut self, p: Vec2) {
        let Some(drag) = &mut self.poly_drag else { return };
        drag.cur = p;
        if !drag.moved && drag.cur.distance(drag.start) > 4.0 {
            drag.moved = true;
        }
        if drag.select == PolySelect::Lasso {
            if drag.path.last().map_or(true, |q| q.distance(p) > 2.0) {
                drag.path.push(p);
            }
            return;
        }
        if !drag.moved || drag.select == PolySelect::Marquee {
            return;
        }
        let (set, sub, select, uv) = (drag.set, drag.sub, drag.select, drag.uv);
        let Some(tri) = self.poly_pick(p, uv, set) else { return };
        let Some(topo) = self.poly_topo(sub) else { return };
        let poly = topo.tri_poly[tri as usize];
        // Loops take the edge nearest the cursor in the polygon under it,
        // partial loops measure around their start.
        let screen: Box<dyn Fn(Vec3) -> Option<Vec2>> = match (uv, select, self.poly_drag.as_ref().and_then(|d| d.partial)) {
            (true, PolySelect::PartialLoop, Some((start, _))) => self.poly_uv_screen(sub, start),
            (true, ..) => self.poly_uv_screen(sub, poly),
            (false, ..) => Box::new(self.screen_fn()),
        };
        match select {
            PolySelect::Loop => {
                let drag = self.poly_drag.as_mut().unwrap();
                if drag.polys.contains(&poly) {
                    return;
                }
                let k = match drag.loop_dir {
                    Some(d) => topo.parallel_edge(poly, d),
                    None => topo.nearest_edge(poly, &screen, p),
                };
                for q in topo.quad_loop(poly, k) {
                    if drag.polys.insert(q) {
                        drag.tris.extend(topo.tris[q as usize].iter().copied());
                    }
                }
            }
            PolySelect::PartialLoop => {
                let Some((start, k)) = self.poly_drag.as_ref().unwrap().partial else { return };
                if !topo.is_quad(start) {
                    return;
                }
                let sc = screen(topo.center(start)).unwrap_or(p);
                let towards = |e: usize| {
                    let (a, b) = topo.edge(start, e);
                    let m = screen((topo.pos[a as usize] + topo.pos[b as usize]) * 0.5).unwrap_or(sc);
                    (m - sc).normalize_or_zero().dot((p - sc).normalize_or_zero())
                };
                // The strip leaves through the edge facing the drag.
                let k = k.unwrap_or_else(|| (0..4).max_by(|&i, &j| towards(i).total_cmp(&towards(j))).unwrap_or(0));
                let side = if towards((k + 1) % 4) >= towards((k + 3) % 4) { 1 } else { 3 };
                let drag = self.poly_drag.as_mut().unwrap();
                drag.partial = Some((start, Some(k)));
                if let Some(cells) = topo.patch(start, k, side, poly, drag.primary_only) {
                    drag.tris = cells.iter().flat_map(|c| topo.tris[*c as usize].iter().copied()).collect();
                }
            }
            PolySelect::Marquee | PolySelect::Lasso => {}
        }
    }

    /// World to viewport-local points, detached from `self` (`project_vp`).
    fn screen_fn(&self) -> impl Fn(Vec3) -> Option<Vec2> + Copy + Send + Sync {
        let r = self.vp_rect;
        let m = self.camera.view_proj(r.width() / r.height().max(1.0));
        move |w: Vec3| -> Option<Vec2> {
            let c = m * w.extend(1.0);
            (c.w > 1e-5).then(|| {
                let n = c.truncate() / c.w;
                Vec2::new((n.x * 0.5 + 0.5) * r.width(), (0.5 - n.y * 0.5) * r.height())
            })
        }
    }

    /// Triangles of submesh `sub` the rectangle a..b selects.
    fn marquee_tris(&mut self, set: usize, sub: usize, a: Vec2, b: Vec2) -> Vec<u32> {
        let r = [a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)];
        self.area_tris(set, sub, &r, true)
    }

    /// Triangles of submesh `sub` the outline `shape` (a closed polygon in
    /// viewport points; `rect` when it is the marquee) selects.
    fn area_tris(&mut self, set: usize, sub: usize, shape: &[Vec2], rect: bool) -> Vec<u32> {
        use rayon::prelude::*;
        let Some(doc) = &self.doc else { return Vec::new() };
        if shape.len() < 3 {
            return Vec::new();
        }
        let lo = shape.iter().fold(Vec2::splat(f32::MAX), |m, p| m.min(*p));
        let hi = shape.iter().fold(Vec2::splat(f32::MIN), |m, p| m.max(*p));
        let inside_shape = |q: Vec2| -> bool {
            if q.x < lo.x || q.x > hi.x || q.y < lo.y || q.y > hi.y {
                return false;
            }
            rect || super::polysel::point_in_polygon(q, shape)
        };
        let view = self.view_info();
        let screen = self.screen_fn();
        let mesh = doc.mesh.clone();
        let bvh = doc.bvh.clone();
        let pick_map = &doc.pick_map;
        let hidden = &doc.hidden;
        let eps = mesh.diagonal() * 1e-4;
        let visible_only = self.poly_visible_only;
        let hit_test = self.poly_hit;
        let is_hidden = |s: u32, t: u32| hidden.get(s as usize).and_then(|v| v.get(t as usize)).copied().unwrap_or(false);
        // A point is visible when nothing (shown) lies between it and the eye.
        let visible = |w: Vec3| -> bool {
            let Some(sp) = screen(w) else { return false };
            let (o, d) = view.ray(sp * view.ppp);
            let t = (w - o).dot(d) - eps;
            if t <= 0.0 {
                return true;
            }
            let filter = |id: u32| {
                let (s, t) = pick_map[id as usize];
                !is_hidden(s, t)
            };
            bvh.trace_where(o, d, t, true, Some(&filter)).is_none()
        };
        let s = &mesh.submeshes[sub];
        let _ = set;
        let picked: Vec<u32> = (0..(s.indices.len() / 3) as u32)
            .into_par_iter()
            .filter(|&t| {
                if is_hidden(sub as u32, t) {
                    return false;
                }
                let w: [Vec3; 3] = std::array::from_fn(|k| Vec3::from(mesh.vertices[s.indices[t as usize * 3 + k] as usize].pos));
                let c = (w[0] + w[1] + w[2]) / 3.0;
                let inside = match hit_test {
                    PolyHit::Center => screen(c).is_some_and(inside_shape),
                    PolyHit::Intersection => {
                        let (Some(p0), Some(p1), Some(p2)) = (screen(w[0]), screen(w[1]), screen(w[2])) else { return false };
                        if rect {
                            super::polysel::tri_rect_overlap([p0, p1, p2], [lo, hi])
                        } else {
                            super::polysel::tri_polygon_overlap([p0, p1, p2], shape, [lo, hi])
                        }
                    }
                };
                if !inside || !visible_only {
                    return inside;
                }
                // Visible: its center or a point near any corner.
                visible(c) || w.iter().any(|v| visible(*v * 0.9 + c * 0.1))
            })
            .collect();
        // Contained center of a polygon: its own center, not its triangles'.
        if hit_test == PolyHit::Center && self.poly_mode == PolyMode::Polygon {
            if let Some(topo) = self.poly_topo(sub) {
                let polys: HashSet<u32> = (0..topo.rings.len() as u32)
                    .filter(|&p| {
                        screen(topo.center(p)).is_some_and(inside_shape)
                            && topo.tris[p as usize].iter().any(|t| picked.contains(t) || !visible_only)
                    })
                    .collect();
                return polys.into_iter().flat_map(|p| topo.tris[p as usize].iter().copied()).collect();
            }
        }
        picked
    }

    /// Ends a polygon drag: fills (or hides) the selection, one undo step.
    fn end_poly_drag(&mut self) {
        let Some(drag) = self.poly_drag.take() else { return };
        let mut tris = match drag.select {
            PolySelect::Marquee if drag.moved => {
                let t = if drag.uv {
                    let (a, b) = (drag.start, drag.cur);
                    self.area_tris_uv(drag.sub, &[a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)], true)
                } else {
                    self.marquee_tris(drag.set, drag.sub, drag.start, drag.cur)
                };
                self.expand_groups(drag.sub, &t)
            }
            PolySelect::Lasso if drag.moved && drag.path.len() >= 3 => {
                let t = if drag.uv { self.area_tris_uv(drag.sub, &drag.path, false) } else { self.area_tris(drag.set, drag.sub, &drag.path, false) };
                self.expand_groups(drag.sub, &t)
            }
            // A click: the group under the cursor.
            _ if !drag.moved || drag.tris.is_empty() => match self.poly_pick(drag.start, drag.uv, drag.set) {
                Some(t) => self.expand_groups(drag.sub, &[t]),
                None => {
                    if !drag.hide {
                        self.status = "Polygon fill: click or drag over the active texture set.".into();
                    }
                    return;
                }
            },
            _ => drag.tris.clone(),
        };
        tris.sort_unstable();
        tris.dedup();
        // Hidden geometry is never filled.
        if let Some(h) = self.doc.as_ref().and_then(|d| d.hidden.get(drag.sub)) {
            tris.retain(|t| !h.get(*t as usize).copied().unwrap_or(false));
        }
        if tris.is_empty() {
            return;
        }
        if !drag.hide && self.poly_to_selection {
            let op = self.sel_op;
            self.select_tris(drag.set, drag.sub, &tris, op);
            return;
        }
        if drag.hide {
            self.checkpoint_hidden(if drag.isolate { "Isolate geometry" } else { "Hide geometry" });
            let Some(doc) = &mut self.doc else { return };
            if drag.isolate {
                for v in doc.hidden.iter_mut() {
                    v.iter_mut().for_each(|h| *h = true);
                }
            }
            doc.set_hidden(&self.engine, drag.sub, &tris, !drag.isolate);
            self.status = format!("{} {} triangles", if drag.isolate { "Isolated" } else { "Hid" }, tris.len());
            return;
        }
        let (lid, target) = match self.resolve_paint_target() {
            Ok(v) => v,
            Err(e) => {
                self.toast(e);
                return;
            }
        };
        // Black on a mask paints black (the mask pass writes the value).
        let erase = self.tool == Tool::Erase;
        let channels = if erase { [true; 7] } else { self.mat.channels };
        let mask = matches!(target, PaintTarget::MaskPaint(_));
        let set = drag.set;
        let doc = self.doc.as_mut().unwrap();
        let res = doc.sets[set].resolution();
        let Some(layer) = crate::document::find_mut(&mut doc.sets[set].layers, lid) else { return };
        // The eraser only clears emissive / normal / opacity a layer has:
        // it never allocates them.
        let has_b = layer.paint.as_ref().is_some_and(|p| p.emis.is_some());
        let needs_b = !mask && (channels[4] || channels[5] || channels[6]) && (!erase || has_b);
        if !erase && !mask && channels[5] {
            layer.desc.opacity_painted = true;
        }
        layer.materialize(&self.engine.device, res, match target {
            PaintTarget::MaskPaint(eid) => Some(eid),
            PaintTarget::Content => None,
        });
        if needs_b {
            if let Some(p) = &mut layer.paint {
                p.ensure_b(&self.engine.device);
            }
        }
        let textures: Vec<Arc<Tex>> = match target {
            PaintTarget::Content => layer.paint.as_ref().map(|p| p.all().into_iter().cloned().collect()).unwrap_or_default(),
            PaintTarget::MaskPaint(eid) => layer.mask_paint.get(&eid).cloned().into_iter().collect(),
        };
        // Undo keeps only the tiles under the filled triangles (their UV
        // bounds), not whole textures.
        let mut entry = Entry::stroke_tiles("Polygon fill", set, textures);
        let region = {
            let doc = self.doc.as_ref().unwrap();
            let s = &doc.mesh.submeshes[drag.sub];
            let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
            for &t in &tris {
                for k in 0..3 {
                    let uv = Vec2::from(doc.mesh.vertices[s.indices[t as usize * 3 + k] as usize].uv);
                    lo = lo.min(uv);
                    hi = hi.max(uv);
                }
            }
            let r = res as f32;
            if lo.x < 0.0 || lo.y < 0.0 || hi.x > 1.0 || hi.y > 1.0 {
                [0, 0, res, res]
            } else {
                // A few texels of margin for the rasterizer's coverage.
                let px = |v: f32, grow: f32| ((v * r + grow).floor().clamp(0.0, r)) as u32;
                [px(lo.x, -3.0), px(lo.y, -3.0), px(hi.x, 4.0), px(hi.y, 4.0)]
            }
        };
        let mut enc = self.engine.device.create_command_encoder(&Default::default());
        entry.backup_region(&self.engine.device, &mut enc, region);
        self.engine.queue.submit(Some(enc.finish()));
        if !mask && !erase {
            self.prepare_brush_material();
        }
        self.fill_tris(set, drag.sub, lid, target, erase, &tris);
        self.history.push(entry);
        self.status = format!("Filled {} triangles", tris.len());
    }

    /// Paints the brush material (or erases) over whole triangles.
    pub(super) fn fill_tris(&mut self, set: usize, sub: usize, lid: u64, target: PaintTarget, erase: bool, tris: &[u32]) {
        self.fill_tris_with(set, sub, lid, target, erase, tris, MODE_FULL, &[]);
    }

    /// `fill_tris` with another brush mode and its stamps (gradients).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn fill_tris_with(&mut self, set: usize, sub: usize, lid: u64, target: PaintTarget, erase: bool, tris: &[u32], mode: f32, stamps: &[Stamp]) {
        self.bind_selection(set);
        let Some(doc) = &self.doc else { return };
        let idx = &doc.mesh.submeshes[sub].indices;
        let subset: Vec<u32> = tris.iter().flat_map(|&t| idx[t as usize * 3..t as usize * 3 + 3].iter().copied()).collect();
        if subset.is_empty() {
            return;
        }
        let buf = self.engine.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("polygon fill"),
            contents: bytemuck::cast_slice(&subset),
            usage: wgpu::BufferUsages::INDEX,
        });
        let channels = if erase { [true; 7] } else { self.mat.channels };
        let mask = matches!(target, PaintTarget::MaskPaint(_));
        let uniform = self.brush_uniform(mode);
        let doc = self.doc.as_mut().unwrap();
        let Some(layer) = find(&doc.sets[set].layers, lid) else { return };
        self.engine.paint.upload(&self.engine.queue, uniform, stamps);
        let mut enc = self.engine.device.create_command_encoder(&Default::default());
        for pass in passes_for(erase, mask, channels) {
            let targets: Vec<&Tex> = match (pass, target) {
                (_, PaintTarget::MaskPaint(eid)) => layer.mask_paint.get(&eid).map(|t| &**t).into_iter().collect(),
                (PaintPass::PaintA | PaintPass::EraseA, _) => {
                    layer.paint.as_ref().map(|p| vec![&*p.color, &*p.vals, &*p.alph]).unwrap_or_default()
                }
                _ => layer
                    .paint
                    .as_ref()
                    .and_then(|p| Some(vec![&**p.emis.as_ref()?, &**p.nrm.as_ref()?, &**p.opac.as_ref()?]))
                    .unwrap_or_default(),
            };
            if targets.is_empty() {
                continue;
            }
            self.engine.paint.draw(
                &self.engine.device,
                &mut enc,
                &doc.gpu_mesh,
                sub,
                Some((&buf, subset.len() as u32)),
                &targets,
                self.engine.viewport.depth(),
                pass,
                channels,
            );
        }
        self.engine.queue.submit(Some(enc.finish()));
        if let Some(l) = crate::document::find_mut(&mut doc.sets[set].layers, lid) {
            l.rev = crate::document::next_rev();
        }
        doc.sets[set].dirty = true;
        doc.modified = true;
    }

    /// Polygon tool overlays: the selection being built (or the rectangle),
    /// and the polygon under the cursor.
    fn draw_poly_overlay(&mut self, painter: &egui::Painter, rect: Rect, hover: Option<Vec2>) {
        let fill = Color32::from_rgba_unmultiplied(255, 140, 40, 70);
        let line = egui::Stroke::new(1.5f32, Color32::from_rgb(255, 150, 50));
        let to_screen = |q: Vec2| rect.min + egui::vec2(q.x, q.y);
        if let Some(drag) = &self.poly_drag {
            if drag.uv && drag.select.is_area() {
                // Its outline is drawn in the 2D view.
            } else if drag.select == PolySelect::Lasso {
                if drag.moved && drag.path.len() >= 2 {
                    let pts: Vec<Pos2> = drag.path.iter().map(|q| to_screen(*q)).collect();
                    let st = egui::Stroke::new(1.0f32, Color32::from_rgb(255, 160, 70));
                    painter.add(egui::Shape::line(pts.clone(), st));
                    // The closing edge, dashed.
                    painter.extend(egui::Shape::dashed_line(&[*pts.last().unwrap(), pts[0]], st, 4.0, 3.0));
                }
            } else if drag.select == PolySelect::Marquee {
                if drag.moved {
                    let r = Rect::from_two_pos(to_screen(drag.start), to_screen(drag.cur));
                    painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(255, 140, 40, 25));
                    let st = egui::Stroke::new(1.0f32, Color32::from_rgb(255, 160, 70));
                    let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                    for w in c.windows(2) {
                        painter.extend(egui::Shape::dashed_line(&[w[0], w[1]], st, 4.0, 3.0));
                    }
                }
            } else if let Some(doc) = &self.doc {
                // The selection as one mesh.
                let s = &doc.mesh.submeshes[drag.sub];
                let mut mesh = egui::Mesh::default();
                for &t in drag.tris.iter().take(60_000) {
                    let pts: Vec<Pos2> = (0..3)
                        .filter_map(|k| self.project_vp(Vec3::from(doc.mesh.vertices[s.indices[t as usize * 3 + k] as usize].pos)).map(to_screen))
                        .collect();
                    if pts.len() == 3 {
                        let base = mesh.vertices.len() as u32;
                        for p in pts {
                            mesh.colored_vertex(p, fill);
                        }
                        mesh.add_triangle(base, base + 1, base + 2);
                    }
                }
                painter.add(egui::Shape::mesh(mesh));
            }
            return;
        }
        // Hover: outline of the polygon (or triangle) under the cursor.
        let Some(p) = hover else { return };
        if !matches!(self.poly_mode, PolyMode::Polygon | PolyMode::Triangle) && self.poly_select.is_area() {
            return;
        }
        let Some(hit) = self.pick_at(p) else { return };
        let sub = match &self.doc {
            Some(d) if hit.set == d.active || self.tool == Tool::Hide => d.sets[hit.set].submesh,
            _ => return,
        };
        let Some(topo) = self.poly_topo(sub) else { return };
        let ring: Vec<u32> = if self.poly_mode == PolyMode::Triangle && self.poly_select.is_area() {
            let Some(doc) = &self.doc else { return };
            let s = &doc.mesh.submeshes[sub];
            let i = hit.tri as usize * 3;
            let w: Vec<Vec3> = (0..3).map(|k| Vec3::from(doc.mesh.vertices[s.indices[i + k] as usize].pos)).collect();
            let pts: Vec<Pos2> = w.iter().filter_map(|v| self.project_vp(*v).map(to_screen)).collect();
            if pts.len() == 3 {
                painter.add(egui::Shape::convex_polygon(pts.clone(), fill, egui::Stroke::NONE));
                painter.add(egui::Shape::closed_line(pts, line));
            }
            return;
        } else {
            topo.rings[topo.tri_poly[hit.tri as usize] as usize].clone()
        };
        let pts: Vec<Pos2> = ring.iter().filter_map(|v| self.project_vp(topo.pos[*v as usize]).map(to_screen)).collect();
        if pts.len() == ring.len() && pts.len() >= 3 {
            let mut mesh = egui::Mesh::default();
            for q in &pts {
                mesh.colored_vertex(*q, fill);
            }
            for i in 1..pts.len() as u32 - 1 {
                mesh.add_triangle(0, i, i + 1);
            }
            painter.add(egui::Shape::mesh(mesh));
            painter.add(egui::Shape::closed_line(pts, line));
        }
    }

    fn pick_id_color(&mut self, p: Vec2, eid: u64) {
        // The pick stays armed until a click lands on the active set.
        let Some(hit) = self.pick_at(p) else {
            self.id_pick = Some(eid);
            return;
        };
        let Some(doc) = &self.doc else { return };
        if hit.set != doc.active {
            self.id_pick = Some(eid);
            self.toast("Pick the ID color on the selected texture set.".into());
            return;
        }
        if !doc.sets[hit.set].baked {
            self.toast("Bake the ID map first (Bake mesh maps).".into());
            return;
        }
        let s = &doc.sets[hit.set];
        let res = s.resolution() as f32;
        let (x, y) = ((hit.uv[0].rem_euclid(1.0) * res) as u32, (hit.uv[1].rem_euclid(1.0) * res) as u32);
        let c = s.gpu.baked_id.read_texel_f16(&self.engine.device, &self.engine.queue, x, y);
        self.checkpoint("Pick ID color");
        let doc = self.doc.as_mut().unwrap();
        let s = &mut doc.sets[doc.active];
        if let Some(e) = s.selected_layer_mut().and_then(|l| l.desc.mask.as_mut()).and_then(|m| m.effects.iter_mut().find(|e| e.id == eid)) {
            if let crate::document::MaskEffectKind::ColorSelect { colors, .. } = &mut e.kind {
                if colors.len() < 8 {
                    colors.push([c[0], c[1], c[2]]);
                }
            }
        }
        s.dirty = true;
        self.status = format!("Added ID color ({:.2}, {:.2}, {:.2})", c[0], c[1], c[2]);
    }

    /// Base color (linear) of the surface under `p`, for the eyedropper.
    pub(super) fn sample_color(&self, p: Vec2) -> Option<[f32; 3]> {
        let hit = self.pick_at(p)?;
        let s = &self.doc.as_ref()?.sets[hit.set];
        let res = s.resolution() as f32;
        let (x, y) = ((hit.uv[0].rem_euclid(1.0) * res) as u32, (hit.uv[1].rem_euclid(1.0) * res) as u32);
        let base = s.gpu.final_base.read_texel_f16(&self.engine.device, &self.engine.queue, x, y);
        Some([base[0], base[1], base[2]])
    }

    fn pick_color(&mut self, p: Vec2) {
        let Some(hit) = self.pick_at(p) else { return };
        let Some(doc) = &self.doc else { return };
        let s = &doc.sets[hit.set];
        let res = s.resolution() as f32;
        let (x, y) = ((hit.uv[0].rem_euclid(1.0) * res) as u32, (hit.uv[1].rem_euclid(1.0) * res) as u32);
        let base = s.gpu.final_base.read_texel_f16(&self.engine.device, &self.engine.queue, x, y);
        let orm = s.gpu.final_orm.read_texel_f16(&self.engine.device, &self.engine.queue, x, y);
        self.mat.color = [base[0], base[1], base[2]];
        self.mat.roughness = orm[1];
        self.mat.metallic = orm[2];
        self.mat.height = orm[3];
        self.status = format!(
            "Picked color ({:.3}, {:.3}, {:.3}), roughness {:.2}, metallic {:.2}",
            base[0], base[1], base[2], orm[1], orm[2]
        );
    }
}

/// End point of a line from `a` toward `b` with its angle snapped to `step` degrees.
fn snap_line(a: Vec2, b: Vec2, step: f32) -> Vec2 {
    let d = b - a;
    let len = d.length();
    if len < 1e-3 {
        return b;
    }
    let s = step.to_radians();
    let ang = (d.y.atan2(d.x) / s).round() * s;
    a + Vec2::new(ang.cos(), ang.sin()) * len
}

impl RetroApp {
    /// Warning shown over the top of the 3D view for a few seconds.
    /// The part of a view the floating panels leave visible.
    pub(super) fn visible_part(&self, rect: Rect) -> Rect {
        let mut v = if self.ui_hidden || !self.canvas_mid.is_positive() { rect } else { rect.intersect(self.canvas_mid) };
        // The bottom corners clear the tool dock and its options bar when
        // they share the view's width (the split 3D half).
        if !self.ui_hidden && self.dock_rect.is_positive() {
            let bar = self.dock_rect.with_min_y(self.dock_rect.min.y - 64.0).expand2(egui::vec2(24.0, 0.0));
            let corners = [Pos2::new(v.min.x + 60.0, v.max.y - 30.0), Pos2::new(v.max.x - 60.0, v.max.y - 30.0)];
            if corners.iter().any(|c| bar.x_range().contains(c.x)) && bar.min.y < v.max.y {
                v.max.y = v.max.y.min(bar.min.y - 4.0);
            }
        }
        if v.width() > 120.0 && v.height() > 120.0 { v } else { rect }
    }

    fn draw_toast(&self, painter: &egui::Painter, rect: Rect) {
        let Some((msg, at)) = &self.toast else { return };
        let age = at.elapsed().as_secs_f32();
        if age > 3.5 {
            return;
        }
        let alpha = ((3.5 - age) / 0.5).clamp(0.0, 1.0);
        let font = egui::FontId::proportional(12.5);
        let g = painter.layout(msg.clone(), font, Color32::from_white_alpha((235.0 * alpha) as u8), rect.width() - 80.0);
        // Under the view bar (never behind it).
        let r = Rect::from_center_size(Pos2::new(rect.center().x, rect.min.y + 84.0), g.size() + egui::vec2(24.0, 14.0));
        painter.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(30, 30, 30, (230.0 * alpha) as u8));
        painter.rect_stroke(r, 4.0, egui::Stroke::new(1.0f32, Color32::from_rgba_unmultiplied(255, 137, 34, (255.0 * alpha) as u8)));
        painter.galley(r.min + egui::vec2(12.0, 7.0), g, Color32::WHITE);
    }

    /// XYZ axes in the bottom right corner of the 3D view.
    fn draw_axis_gizmo(&self, painter: &egui::Painter, rect: Rect) {
        let v = self.camera.view();
        let o = rect.right_bottom() + egui::vec2(-34.0, -34.0);
        for (axis, c, n) in [
            (glam::Vec3::X, Color32::from_rgb(220, 70, 70), "x"),
            (glam::Vec3::Y, Color32::from_rgb(90, 200, 90), "y"),
            (glam::Vec3::Z, Color32::from_rgb(80, 130, 235), "z"),
        ] {
            let d = v.transform_vector3(axis);
            let e = o + egui::vec2(d.x, -d.y) * 18.0;
            painter.line_segment([o, e], egui::Stroke::new(1.5f32, c));
            painter.text(e + egui::vec2(d.x, -d.y) * 7.0, egui::Align2::CENTER_CENTER, n, egui::FontId::proportional(10.0), c);
        }
    }

    /// While a modifier is held, what each mouse button does (bottom left).
    /// The current tool's keys (key, what it does), for the corner of the view.
    fn tool_key_lines(&self) -> Vec<(String, &'static str)> {
        use super::keymap::Action as A;
        use super::selection::SelShape;
        let k = |a: A| self.keymap.label(a);
        let size = format!("{} / {}", k(A::SizeDown), k(A::SizeUp));
        let mut v: Vec<(String, &'static str)> = Vec::new();
        match self.tool {
            Tool::Projection => {
                v.push(("T + LMB drag".into(), "Rotate the projected image"));
                v.push(("T + MMB drag".into(), "Move it"));
                v.push(("T + RMB drag".into(), "Scale it"));
                v.push(("LMB".into(), "Paint it onto the mesh"));
                v.push((size, "Brush size"));
            }
            Tool::Paint | Tool::Erase | Tool::Smudge | Tool::Clone | Tool::SelBrush => {
                if self.tool == Tool::Clone {
                    v.push(("V".into(), "Set the source under the cursor"));
                }
                if self.tool == Tool::SelBrush {
                    v.push((k(A::SwapValue), "Add / subtract"));
                }
                v.push((size, "Brush size"));
                v.push(("Ctrl + RMB drag".into(), "Size / hardness"));
                v.push(("Ctrl + LMB drag".into(), "Flow / angle"));
                v.push(("Shift + LMB".into(), "Straight line from the last stroke"));
                v.push(("1 ... 9, 0".into(), "Flow 10 % ... 100 %"));
                v.push((k(A::Lazy), "Lazy mouse"));
                v.push((k(A::Symmetry), "Symmetry"));
            }
            Tool::Select(s) => {
                v.push(("Shift / Ctrl / both".into(), "Add / subtract / intersect"));
                if s == SelShape::PolyLasso {
                    v.push(("Enter / Backspace".into(), "Close / remove the last point"));
                }
                v.push(("Ctrl + D / Ctrl + Shift + I".into(), "Deselect / invert"));
                v.push(("Shift + Backspace / Delete".into(), "Fill / clear the selection"));
                v.push((k(if matches!(s, SelShape::Rect | SelShape::Ellipse) { A::MarqueeShape } else { A::LassoShape }), "Switch the shape"));
            }
            Tool::Wand => {
                v.push(("Click".into(), "Select similar colors"));
                v.push(("Shift / Ctrl / both".into(), "Add / subtract / intersect"));
                v.push(("Ctrl + D".into(), "Deselect"));
            }
            Tool::Bucket => {
                v.push(("Click".into(), "Fill the area of similar color"));
                v.push((k(A::FillSwitch), "Gradient"));
            }
            Tool::Gradient => {
                v.push(("LMB drag".into(), "Draw the gradient line"));
                v.push((k(A::FillSwitch), "Paint bucket"));
            }
            Tool::PolygonFill => {
                v.push(("Click".into(), "Fill under the cursor"));
                v.push(("Drag".into(), "Fill a rectangle, outline or loops"));
                v.push(("Shift + drag".into(), "Partial loop: the loop only"));
            }
            Tool::Hide => {
                v.push(("Click / drag".into(), "Hide faces"));
                v.push(("Ctrl + click / drag".into(), "Keep only these"));
            }
            Tool::Path => {
                v.push(("Click".into(), "New path, or continue the selected one from its end"));
                v.push(("Click on a curve".into(), "Insert a point"));
                v.push(("Drag".into(), "Move a point or a handle (Pen: pull out handles)"));
                v.push(("Shift + click / drag".into(), "Corner or smooth point / break the tangent"));
                v.push(("Backspace / Delete".into(), "Remove the selected point"));
                v.push(("Escape / Enter".into(), "End the path"));
            }
            Tool::Picker => v.push(("Click".into(), "Pick the color under the cursor")),
        }
        v.push(("Alt + LMB / MMB / RMB".into(), "Orbit / pan / zoom"));
        v.retain(|(key, _)| !key.trim().is_empty() && key != " / ");
        v
    }

    /// Where the bottom left helper may end: above the canvas status line
    /// (its own corner) and above the tool dock and its options bar when a
    /// box of this size would reach them.
    fn helper_bottom(&self, rect: Rect, size: egui::Vec2) -> f32 {
        let mut bottom = rect.max.y - 10.0;
        if self.canvas_mid.is_positive() && !self.ui_hidden {
            bottom = bottom.min(self.canvas_mid.max.y - 38.0);
        }
        if !self.ui_hidden {
            let bg = Rect::from_min_max(Pos2::new(rect.min.x + 6.0, bottom - size.y), Pos2::new(rect.min.x + 6.0 + size.x, bottom));
            let top = [self.options_rect, self.dock_rect].into_iter().filter(|r| r.is_positive() && r.expand(4.0).intersects(bg)).map(|r| r.min.y).fold(f32::INFINITY, f32::min);
            if top.is_finite() {
                bottom = bottom.min(top - 6.0);
            }
        }
        bottom
    }

    fn draw_modifier_helper(&mut self, ctx: &egui::Context, painter: &egui::Painter, rect: Rect, m: egui::Modifiers) {
        // Nothing held: the current tool's keys, and a box to hide them
        // (View settings brings them back).
        if !m.any() && self.view.show_tool_keys && self.doc.is_some() {
            let lines = self.tool_key_lines();
            let key_font = egui::FontId::monospace(11.0);
            let font = egui::FontId::proportional(11.5);
            let kw = lines.iter().map(|(k, _)| painter.layout_no_wrap(k.clone(), key_font.clone(), Color32::WHITE).size().x).fold(0.0f32, f32::max);
            let width = kw + 12.0 + lines.iter().map(|(_, t)| painter.layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE).size().x).fold(0.0f32, f32::max);
            let size = egui::vec2(width.max(150.0) + 12.0, lines.len() as f32 * 17.0 + 34.0);
            let bottom = self.helper_bottom(rect, size);
            let bg = Rect::from_min_max(Pos2::new(rect.min.x + 6.0, bottom - size.y), Pos2::new(rect.min.x + 6.0 + size.x, bottom));
            painter.rect_filled(bg, 4.0, Color32::from_black_alpha(110));
            let (x, mut y) = (bg.min.x + 6.0, bg.min.y + 5.0);
            for (k, t) in lines {
                painter.text(Pos2::new(x, y), egui::Align2::LEFT_TOP, k, key_font.clone(), crate::style::ACCENT);
                painter.text(Pos2::new(x + kw + 12.0, y), egui::Align2::LEFT_TOP, t, font.clone(), Color32::from_gray(215));
                y += 17.0;
            }
            painter.line_segment([Pos2::new(bg.min.x + 6.0, y + 2.0), Pos2::new(bg.max.x - 6.0, y + 2.0)], egui::Stroke::new(1.0f32, Color32::from_white_alpha(25)));
            let mut hide = false;
            egui::Area::new(egui::Id::new("tool keys hide"))
                .fixed_pos(Pos2::new(x - 2.0, y + 5.0))
                .order(egui::Order::Middle)
                .show(ctx, |ui| {
                    ui.spacing_mut().icon_width = 12.0;
                    ui.spacing_mut().interact_size.y = 16.0;
                    let r = ui.checkbox(&mut hide, egui::RichText::new("Don't show shortcuts").size(11.0).color(Color32::from_gray(190)));
                    super::mark(ui.ctx(), "hide tool keys", r.rect);
                });
            if hide {
                self.view.show_tool_keys = false;
                self.toast("Shortcuts hidden: View settings shows them again.".into());
            }
            return;
        }
        let lines: &[&str] = match (m.command, m.shift, m.alt) {
            (false, false, true) => &["LMB: orbit", "MMB: pan", "RMB: zoom"],
            (true, false, false) => &["LMB: flow (horizontal) / angle (vertical)", "RMB: size (horizontal) / hardness (vertical)"],
            (false, true, false) => &["LMB: straight line from the last stroke", "RMB: rotate the environment"],
            (true, true, false) => &["LMB: straight line snapped to 5 degrees"],
            (true, false, true) => &["LMB: pan", "RMB: select the texture set under the cursor"],
            _ => &[],
        };
        let font = egui::FontId::proportional(11.5);
        let mut y = self.helper_bottom(rect, egui::vec2(320.0, lines.len() as f32 * 18.0)) - lines.len() as f32 * 18.0;
        for l in lines {
            let g = painter.layout_no_wrap(l.to_string(), font.clone(), Color32::from_gray(230));
            let r = Rect::from_min_size(Pos2::new(rect.min.x + 12.0, y), g.size());
            painter.rect_filled(r.expand(3.0), 2.0, Color32::from_black_alpha(120));
            painter.galley(r.min, g, Color32::WHITE);
            y += 18.0;
        }
    }
}

/// The mouse press of this frame started on the view `id` itself: not on a
/// widget drawn over it (display menus, gizmos), not in another layer
/// (menus, popups, windows), and not while a popup was being dismissed.
fn press_is_ours(ui: &egui::Ui, id: egui::Id, overlay_open: bool) -> bool {
    if overlay_open {
        return false;
    }
    let ctx = ui.ctx();
    let Some(p) = ctx.input(|i| i.pointer.press_origin()) else { return false };
    if ctx.layer_id_at(p) != Some(ui.layer_id()) {
        return false;
    }
    ctx.viewport(|v| v.hits.click.map_or(true, |w| w.id == id))
}
