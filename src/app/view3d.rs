//! Viewport interaction: the 3D and 2D views, navigation, brush strokes,
//! polygon fill, the color picker and the stencil overlay.

use super::*;
use crate::brush::{self, Space, View};
use crate::document::find;
use crate::engine::paint::{BrushUniform, MODE_CAMERA, MODE_FULL, MODE_UV, MODE_WRAP};
use crate::engine::Tex;
use eframe::egui::{Color32, Pos2, Sense};
use wgpu::util::DeviceExt;

impl RetroApp {
    pub(super) fn central(&mut self, ui: &mut egui::Ui) {
        let all = ui.available_rect_before_wrap();
        let bar = Rect::from_min_size(all.min, egui::vec2(all.width(), super::layout::CONTEXT_BAR_H));
        let full = Rect::from_min_max(Pos2::new(all.min.x, bar.max.y), all.max);
        match self.view.layout {
            Layout::Only3D => {
                self.viewport_3d(ui, full);
                self.view_display_menu(ui, full, false);
            }
            Layout::Only2D => {
                self.viewport_2d(ui, full);
                self.view_display_menu(ui, full, true);
            }
            Layout::Both => {
                let mid = (full.min.x + full.width() * self.view.split.clamp(0.15, 0.85)).round();
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
                        self.view.split = ((p.x - full.min.x) / full.width().max(1.0)).clamp(0.15, 0.85);
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
                ui.painter().line_segment([Pos2::new(mid, full.min.y), Pos2::new(mid, full.max.y)], egui::Stroke::new(2.0f32, c));
                self.view_display_menu(ui, left, false);
                self.view_display_menu(ui, right, true);
            }
        }
        self.context_bar(ui, bar);
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
            self.new_dialog = Some(NewProjectDialog { path: String::new(), res: 2048, error: None });
        }
        if child.add(egui::Button::new(egui::RichText::new("Open...").size(13.0)).min_size(egui::vec2(125.0, 30.0))).clicked() {
            self.open_project();
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
        ui.painter().image(self.vp_tex.unwrap(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);

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
            (i.modifiers, i.pointer.clone(), i.raw_scroll_delta.y, i.key_down(Key::S) && !typing, i.key_pressed(Key::V) && !i.modifiers.any() && !typing)
        });
        // A press belongs to the view only when it starts on the view itself.
        if pointer.any_pressed() {
            self.vp_press_ok = press_is_ours(ui, resp.id, self.overlay_open);
        }
        let ours = self.vp_press_ok;
        let drag = |b: egui::PointerButton| ours && resp.dragged_by(b);
        if !pointer.any_down() {
            self.orbit_pivot = None;
        }
        let delta = pointer.delta();
        let local = |p: Pos2| Vec2::new(p.x - rect.min.x, p.y - rect.min.y);
        use egui::PointerButton::{Middle, Primary, Secondary};

        // Projection manipulator of the selected fill: its handles take the
        // left button before the brush.
        let gizmo_busy = self.projection_gizmo(ui, rect, &pointer, mods, ours);

        // Stencil: S + LMB rotates, S + MMB moves, S + RMB scales.
        if gizmo_busy {
        } else if self.stencil.enabled && s_down && resp.hovered() {
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
            let brush_gesture = mods.command && !mods.alt && !mods.shift && self.tool.is_brush();
            let navigating = mods.alt || pointer.secondary_down() || pointer.middle_down() || brush_gesture;
            // Camera: Alt + LMB orbit, Alt + MMB (or MMB, or Ctrl + Alt + LMB) pan, Alt + RMB zoom.
            // Alt + LMB orbits around the surface point under the cursor.
            if ours && resp.drag_started_by(Primary) && mods.alt && !mods.command {
                self.orbit_pivot = pointer.press_origin().and_then(|p| self.pick_at(local(p))).map(|h| h.pos);
            }
            if drag(Primary) && mods.alt && mods.command {
                self.camera.pan(delta.x * ppp, delta.y * ppp, h as f32);
            } else if drag(Primary) && mods.alt {
                match self.orbit_pivot {
                    Some(p) => self.camera.orbit_around(delta.x, delta.y, p),
                    None => self.camera.orbit(delta.x, delta.y),
                }
            }
            // Ctrl + LMB: flow (horizontal) and angle (vertical).
            if drag(Primary) && brush_gesture {
                self.brush.flow = (self.brush.flow + delta.x * 0.004).clamp(0.01, 1.0);
                self.brush.angle = ((self.brush.angle - delta.y * 0.6 + 180.0).rem_euclid(360.0) - 180.0).round();
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
                    self.brush.size = (self.brush.size * (1.0 + delta.x * 0.01)).clamp(1.0, 1000.0);
                    self.brush.hardness = (self.brush.hardness - delta.y * 0.005).clamp(0.0, 1.0);
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
                                Tool::Paint | Tool::Erase | Tool::Clone | Tool::Smudge => {
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
                                Tool::PolygonFill => self.polygon_fill(p),
                                Tool::Picker => {
                                    self.pick_color(p);
                                    // The picker is temporary: back to the previous tool.
                                    if let Some(t) = self.prev_tool.take() {
                                        self.tool = t;
                                    }
                                }
                                Tool::Hide => self.hide_at(p, mods.command),
                            }
                        }
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

        let painter = ui.painter_at(rect);
        if self.stencil.enabled {
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
            if self.doc.is_some() && !mods.alt && self.tool.is_brush() && !gizmo_busy {
                let r = self.brush.size;
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
        self.draw_axis_gizmo(&painter, rect);
        self.draw_toast(&painter, rect);
        self.draw_modifier_helper(&painter, rect, mods);
        if self.view.layout == Layout::Both {
            painter.text(rect.min + egui::vec2(8.0, 6.0), egui::Align2::LEFT_TOP, "3D", egui::FontId::proportional(12.0), Color32::from_gray(160));
        }
    }

    fn uv_fit(&mut self) {
        let (w, h) = (self.engine.uvview.width as f32, self.engine.uvview.height as f32);
        let z = w.min(h) * 0.92;
        self.view.uv_zoom = z;
        self.view.uv_pan = [(w - z) * 0.5, (h - z) * 0.5];
    }

    /// View point (points, local to the 2D view) to UV.
    fn uv_of(&self, p: Vec2) -> Vec2 {
        let ppp = self.engine.uvview.width as f32 / self.uv_rect.width().max(1.0);
        let px = p * ppp;
        Vec2::new((px.x - self.view.uv_pan[0]) / self.view.uv_zoom, (px.y - self.view.uv_pan[1]) / self.view.uv_zoom)
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
        // MMB (or Alt + MMB, Ctrl + Alt + LMB) pans, Alt + RMB zooms.
        let pan = resp.dragged_by(egui::PointerButton::Middle) || (resp.dragged_by(egui::PointerButton::Primary) && mods.alt && mods.command);
        if ours && pan {
            self.view.uv_pan[0] += delta.x;
            self.view.uv_pan[1] += delta.y;
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
        if self.doc.is_some() && self.bake_job.is_none() && self.tool.is_brush() && !armed {
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
        if let Some(p) = resp.hover_pos() {
            if self.tool.is_brush() && self.doc.is_some() {
                painter.circle_stroke(p, self.brush.size, egui::Stroke::new(1.0f32, Color32::WHITE));
            }
        }
        painter.text(rect.min + egui::vec2(8.0, 6.0), egui::Align2::LEFT_TOP, "2D (UV)", egui::FontId::proportional(12.0), Color32::from_gray(160));
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
            let img = match &self.brush.alpha {
                AlphaSource::Builtin(k) => Some(brush::alpha_image(*k)),
                AlphaSource::Resource(id) => self.doc.as_ref().and_then(|d| d.resources.get(id)).map(|r| {
                    let mut g = (*r.image).clone();
                    for p in g.pixels_mut() {
                        let l = (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) * (p[3] as f32 / 255.0);
                        let v = l.round() as u8;
                        *p = image::Rgba([v, v, v, 255]);
                    }
                    g
                }),
            };
            if let Some(img) = img {
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
        let stencil_on = self.stencil.enabled && self.stencil.resource.is_some() && mode != MODE_UV;
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
    fn prepare_brush_material(&mut self) {
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

    fn begin_stroke(&mut self, pos: Vec2, uv: bool) {
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
        self.stroke = Some(ActiveStroke { stroke: Stroke::new(space, set, seed), layer_id: lid, target, erase, channels, uv, history_version });
        self.extend_stroke(pos);
    }

    fn extend_stroke(&mut self, pos: Vec2) {
        let view = self.view_info();
        let uv_ppp = self.engine.uvview.width as f32 / self.uv_rect.width().max(1.0);
        let uv_per_point = uv_ppp / self.view.uv_zoom.max(1e-3);
        let (pan, zoom) = (self.view.uv_pan, self.view.uv_zoom.max(1e-3));
        let to_uv = move |p: Vec2| Vec2::new((p.x * uv_ppp - pan[0]) / zoom, (p.y * uv_ppp - pan[1]) / zoom);
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
        let room = crate::engine::paint::MAX_STAMPS.saturating_sub(self.pending.len());
        self.pending.extend(stamps.into_iter().take(room));
    }

    fn end_stroke(&mut self) {
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
        let stamps = std::mem::take(&mut self.pending);
        let Some(a) = &self.stroke else { return };
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

    fn pick_at(&self, p: Vec2) -> Option<crate::document::Hit> {
        let view = self.view_info();
        let (o, d) = view.ray(p * view.ppp);
        self.doc.as_ref()?.pick(o, d)
    }

    fn polygon_fill(&mut self, p: Vec2) {
        let Some(hit) = self.pick_at(p) else {
            self.status = "Polygon fill: click on the mesh.".into();
            return;
        };
        if let Some(doc) = &mut self.doc {
            doc.active = hit.set;
        }
        let (lid, target) = match self.resolve_paint_target() {
            Ok(v) => v,
            Err(e) => {
                self.toast(e);
                return;
            }
        };
        let erase = self.tool == Tool::Erase || self.mat.mask_value < 0.5 && matches!(target, PaintTarget::MaskPaint(_));
        let doc = self.doc.as_mut().unwrap();
        let set = hit.set;
        let sub = doc.sets[set].submesh;
        let tris: Vec<u32> = match self.poly_mode {
            PolyMode::Triangle => vec![hit.tri],
            PolyMode::TextureSet => (0..(doc.mesh.submeshes[sub].indices.len() / 3) as u32).collect(),
            PolyMode::Mesh | PolyMode::UvIsland => {
                let labels = crate::bake::components(&doc.mesh, sub, self.poly_mode == PolyMode::UvIsland);
                let l = labels[hit.tri as usize];
                labels.iter().enumerate().filter(|(_, x)| **x == l).map(|(i, _)| i as u32).collect()
            }
        };
        let idx = &doc.mesh.submeshes[sub].indices;
        let subset: Vec<u32> = tris.iter().flat_map(|&t| idx[t as usize * 3..t as usize * 3 + 3].iter().copied()).collect();
        let buf = self.engine.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("polygon fill"),
            contents: bytemuck::cast_slice(&subset),
            usage: wgpu::BufferUsages::INDEX,
        });
        let channels = if erase { [true; 7] } else { self.mat.channels };
        let mask = matches!(target, PaintTarget::MaskPaint(_));
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
        let entry = Entry::stroke(&self.engine, "Polygon fill", set, textures);
        if !mask && !erase {
            self.prepare_brush_material();
        }
        let uniform = self.brush_uniform(MODE_FULL);
        let doc = self.doc.as_mut().unwrap();
        let layer = find(&doc.sets[set].layers, lid).unwrap();
        self.engine.paint.upload(&self.engine.queue, uniform, &[]);
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
        self.history.push(entry);
        if let Some(l) = crate::document::find_mut(&mut doc.sets[set].layers, lid) {
            l.rev = crate::document::next_rev();
        }
        doc.sets[set].dirty = true;
        doc.modified = true;
        self.status = format!("Filled {} triangle(s)", tris.len());
    }

    fn pick_id_color(&mut self, p: Vec2, eid: u64) {
        let Some(hit) = self.pick_at(p) else { return };
        let Some(doc) = &self.doc else { return };
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

    /// Geometry mask: hides the clicked part (Ctrl: keeps only that part).
    fn hide_at(&mut self, p: Vec2, isolate: bool) {
        let Some(hit) = self.pick_at(p) else { return };
        self.checkpoint_hidden(if isolate { "Isolate geometry" } else { "Hide geometry" });
        let Some(doc) = &mut self.doc else { return };
        let sub = doc.sets[hit.set].submesh;
        let ntri = doc.mesh.submeshes[sub].indices.len() / 3;
        let tris: Vec<u32> = match self.poly_mode {
            PolyMode::Triangle => vec![hit.tri],
            PolyMode::TextureSet => (0..ntri as u32).collect(),
            PolyMode::Mesh | PolyMode::UvIsland => {
                let labels = crate::bake::components(&doc.mesh, sub, self.poly_mode == PolyMode::UvIsland);
                let l = labels[hit.tri as usize];
                labels.iter().enumerate().filter(|(_, x)| **x == l).map(|(i, _)| i as u32).collect()
            }
        };
        if isolate {
            for v in doc.hidden.iter_mut() {
                v.iter_mut().for_each(|h| *h = true);
            }
            doc.set_hidden(&self.engine, sub, &tris, false);
            self.status = format!("Isolated {} triangle(s)", tris.len());
        } else {
            doc.set_hidden(&self.engine, sub, &tris, true);
            self.status = format!("Hid {} triangle(s)", tris.len());
        }
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
    fn draw_toast(&self, painter: &egui::Painter, rect: Rect) {
        let Some((msg, at)) = &self.toast else { return };
        let age = at.elapsed().as_secs_f32();
        if age > 3.5 {
            return;
        }
        let alpha = ((3.5 - age) / 0.5).clamp(0.0, 1.0);
        let font = egui::FontId::proportional(12.5);
        let g = painter.layout(msg.clone(), font, Color32::from_white_alpha((235.0 * alpha) as u8), rect.width() - 80.0);
        let r = Rect::from_center_size(Pos2::new(rect.center().x, rect.min.y + 52.0), g.size() + egui::vec2(24.0, 14.0));
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
    fn draw_modifier_helper(&self, painter: &egui::Painter, rect: Rect, m: egui::Modifiers) {
        let lines: &[&str] = match (m.command, m.shift, m.alt) {
            (false, false, true) => &["LMB: orbit", "MMB: pan", "RMB: zoom"],
            (true, false, false) => &["LMB: flow (horizontal) / angle (vertical)", "RMB: size (horizontal) / hardness (vertical)"],
            (false, true, false) => &["LMB: straight line from the last stroke", "RMB: rotate the environment"],
            (true, true, false) => &["LMB: straight line snapped to 5 degrees"],
            (true, false, true) => &["LMB: pan", "RMB: select the texture set under the cursor"],
            _ => &[],
        };
        let font = egui::FontId::proportional(11.5);
        let mut y = rect.max.y - 14.0 - lines.len() as f32 * 18.0;
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
