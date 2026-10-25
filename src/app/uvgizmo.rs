//! The UV transformation manipulator of the 2D view (Painter's): the
//! selected fill layer's UV projection as a frame over the UV layout (the
//! image's tile, so it follows the image). Its parts, all drawn along the
//! frame's own axes (they turn with it and with the view):
//! - U / V arrows from the pivot: move along that axis only;
//! - the square between them, or inside the frame: move freely (Shift: one
//!   axis); the pivot goes along;
//! - the ring around the pivot, or outside the frame: rotate around the
//!   pivot (Shift: 15 degree steps);
//! - the frame's corner and edge handles: scale (Shift: keep the aspect);
//! - the pivot: move it (the image stays).
//! A readout next to the cursor says what the drag does.

use super::*;
use crate::document::{LayerKind, MaskEffectKind, ProjMode, Projection, Target, TextureSet};
use eframe::egui::{Color32, Pos2, Sense};

/// What a manipulator drag changes, and the projection at the press.
#[derive(Clone)]
pub(super) struct UvDrag {
    kind: UvDragKind,
    lid: u64,
    start: Vec2,
    p0: Projection,
}

#[derive(Clone, Copy, PartialEq)]
enum UvDragKind {
    Move,
    /// Along one frame axis (unit vector in UV).
    MoveAxis(Vec2),
    Rotate,
    /// Handle in frame space (0, 0.5 or 1 per axis).
    Scale(Vec2),
    Pivot,
}

/// Mirror signs of a projection.
fn mirror(p: &Projection) -> Vec2 {
    Vec2::new(if p.mirror[0] { -1.0 } else { 1.0 }, if p.mirror[1] { -1.0 } else { 1.0 })
}

fn rot(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

fn stretch(p: &Projection) -> Vec2 {
    Vec2::new(p.stretch[0], p.stretch[1]).max(Vec2::splat(1e-4))
}

/// Texture coordinate of a UV point (the fill shader's `uv_xform`).
pub(super) fn tex_of_uv(p: &Projection, uv: Vec2) -> Vec2 {
    let piv = Vec2::from(p.pivot);
    rot((uv - piv) * mirror(p), p.rotation.to_radians()) * p.scale * stretch(p) + piv + Vec2::from(p.offset)
}

/// The frame is fixed on the texture: the image's tile, with the tiled
/// repeats packed around its middle (frame point q shows texture
/// coordinate (q - 0.5) * tiling + 0.5), so it moves, turns and scales
/// with the image whatever the pivot.
fn tex_of_frame(p: &Projection, q: Vec2) -> Vec2 {
    (q - 0.5) * p.scale.max(1e-4) + 0.5
}

/// UV point of a frame point `q` (the frame spans q in 0..1).
pub(super) fn uv_of_frame(p: &Projection, q: Vec2) -> Vec2 {
    let piv = Vec2::from(p.pivot);
    let d = (tex_of_frame(p, q) - piv - Vec2::from(p.offset)) / (p.scale.max(1e-4) * stretch(p));
    piv + rot(d, -p.rotation.to_radians()) * mirror(p)
}

/// Frame point of a UV point.
fn frame_of_uv(p: &Projection, uv: Vec2) -> Vec2 {
    (tex_of_uv(p, uv) - 0.5) / p.scale.max(1e-4) + 0.5
}

/// A line from `a` to `b` with an arrow head at `b`.
fn arrow(painter: &egui::Painter, a: Pos2, b: Pos2, head: f32, stroke: egui::Stroke) {
    let d = b - a;
    if d.length() < 1e-3 {
        return;
    }
    let u = d.normalized();
    let n = egui::vec2(-u.y, u.x);
    let base = b - u * head;
    painter.line_segment([a, base], stroke);
    painter.add(egui::Shape::convex_polygon(vec![b, base + n * head * 0.5, base - n * head * 0.5], stroke.color, egui::Stroke::NONE));
}

/// Frame handles: corners and edge middles.
const HANDLES: [(f32, f32); 8] = [(0.0, 0.0), (0.5, 0.0), (1.0, 0.0), (1.0, 0.5), (1.0, 1.0), (0.5, 1.0), (0.0, 1.0), (0.0, 0.5)];

/// The projection the selection edits: a selected fill layer's content, or
/// a selected Fill effect of a mask. With the selected layer's id.
pub(super) fn sel_projection(s: &TextureSet) -> Option<(u64, &Projection)> {
    let l = s.selected_layer()?;
    match s.target {
        Target::Content => match &l.desc.kind {
            LayerKind::Fill(f) => Some((l.desc.id, &f.projection)),
            _ => None,
        },
        Target::MaskEffect(eid) => match &l.desc.mask.as_ref()?.effects.iter().find(|e| e.id == eid)?.kind {
            MaskEffectKind::Fill { projection, .. } => Some((l.desc.id, projection)),
            _ => None,
        },
        _ => None,
    }
}

/// `sel_projection`, to edit.
pub(super) fn sel_projection_mut(s: &mut TextureSet) -> Option<&mut Projection> {
    let target = s.target;
    let l = s.selected_layer_mut()?;
    match target {
        Target::Content => match &mut l.desc.kind {
            LayerKind::Fill(f) => Some(&mut f.projection),
            _ => None,
        },
        Target::MaskEffect(eid) => match &mut l.desc.mask.as_mut()?.effects.iter_mut().find(|e| e.id == eid)?.kind {
            MaskEffectKind::Fill { projection, .. } => Some(projection),
            _ => None,
        },
        _ => None,
    }
}

impl RetroApp {
    /// The selected fill layer with a UV projection: its id and projection.
    pub(super) fn uv_gizmo_target(&self) -> Option<(u64, Projection)> {
        let doc = self.doc.as_ref()?;
        let s = &doc.sets[doc.active];
        let (id, p) = sel_projection(s)?;
        (p.mode == ProjMode::Uv).then(|| (id, p.clone()))
    }

    fn set_uv_projection(&mut self, lid: u64, p: Projection) {
        let Some(doc) = &mut self.doc else { return };
        let s = &mut doc.sets[doc.active];
        if s.selected != Some(lid) {
            return;
        }
        if let Some(pr) = sel_projection_mut(s) {
            *pr = p;
            s.dirty = true;
            doc.modified = true;
        }
    }

    /// Viewport-local point of a UV point in the 2D view.
    pub(super) fn uv_screen(&self, uv: Vec2) -> Pos2 {
        let ppp = self.engine.uvview.width as f32 / self.uv_rect.width().max(1.0);
        let px = self.uv_px(uv);
        self.uv_rect.min + egui::vec2(px.x / ppp, px.y / ppp)
    }

    /// Draws the manipulator and handles its drags. Returns true when it
    /// owns the left button (the 2D view then does not paint).
    pub(super) fn uv_gizmo(&mut self, ui: &mut egui::Ui, rect: egui::Rect, pointer: &egui::PointerState, mods: egui::Modifiers, ours: bool) -> bool {
        let Some((lid, p)) = self.uv_gizmo_target() else {
            self.uv_drag = None;
            return false;
        };
        if !self.uv_gizmo_show {
            return false;
        }
        let hs = [4.0f32, 5.5, 7.5][self.uv_gizmo_size.min(2) as usize];
        let k = hs / 5.5;
        let corners: Vec<Pos2> = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].iter().map(|(x, y)| self.uv_screen(uv_of_frame(&p, Vec2::new(*x, *y)))).collect();
        let handles: Vec<Pos2> = HANDLES.iter().map(|(x, y)| self.uv_screen(uv_of_frame(&p, Vec2::new(*x, *y)))).collect();
        let pivot = self.uv_screen(Vec2::from(p.pivot));
        let local = |q: Pos2| Vec2::new(q.x - rect.min.x, q.y - rect.min.y);
        // The frame's axes on screen (they turn with the projection, the
        // mirror and the view) and in UV.
        let unit = |v: egui::Vec2, or: egui::Vec2| if v.length() > 1e-3 { v.normalized() } else { or };
        let ex = unit(corners[1] - corners[0], egui::vec2(1.0, 0.0));
        let ey = unit(corners[3] - corners[0], egui::vec2(0.0, 1.0));
        let uv_axis = |a: Vec2, b: Vec2| (uv_of_frame(&p, b) - uv_of_frame(&p, a)).normalize_or(Vec2::X);
        let (ax, ay) = (uv_axis(Vec2::ZERO, Vec2::X), uv_axis(Vec2::ZERO, Vec2::Y));
        let (arrow0, arrow1) = (16.0 * k, 64.0 * k);
        let ring = 86.0 * k;
        let plane = Rect::from_center_size(pivot + (ex + ey) * 26.0 * k, egui::vec2(12.0, 12.0) * k);
        let plane_pts = |c: Pos2, s: f32| -> Vec<Pos2> { vec![c + (-ex - ey) * s, c + (ex - ey) * s, c + (ex + ey) * s, c + (-ex + ey) * s] };
        let seg_dist = |q: Pos2, a: Pos2, b: Pos2| -> f32 {
            let ab = b - a;
            let t = ((q - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
            q.distance(a + ab * t)
        };
        // What a press at q would take.
        let pick = |q: Pos2, p: &Projection, uv_of: &dyn Fn(Vec2) -> Vec2| -> UvDragKind {
            let near = |a: Pos2| a.distance(q) <= hs + 4.0;
            if near(pivot) {
                return UvDragKind::Pivot;
            }
            if seg_dist(q, pivot + ex * arrow0, pivot + ex * (arrow1 + 8.0 * k)) <= 6.0 {
                return UvDragKind::MoveAxis(ax);
            }
            if seg_dist(q, pivot + ey * arrow0, pivot + ey * (arrow1 + 8.0 * k)) <= 6.0 {
                return UvDragKind::MoveAxis(ay);
            }
            if (q - pivot - (ex + ey) * 26.0 * k).length() <= 9.0 * k {
                return UvDragKind::Move;
            }
            if let Some(i) = handles.iter().position(|h| near(*h)) {
                return UvDragKind::Scale(Vec2::new(HANDLES[i].0, HANDLES[i].1));
            }
            if (q.distance(pivot) - ring).abs() <= 6.0 {
                return UvDragKind::Rotate;
            }
            let f = frame_of_uv(p, uv_of(local(q)));
            if (0.0..=1.0).contains(&f.x) && (0.0..=1.0).contains(&f.y) {
                UvDragKind::Move
            } else {
                UvDragKind::Rotate
            }
        };

        // Press: what the manipulator takes.
        if pointer.primary_pressed() && ours && !mods.alt {
            if let Some(q) = pointer.interact_pos().filter(|q| rect.contains(*q)) {
                let kind = pick(q, &p, &|v| self.uv_of(v));
                self.uv_drag = Some(UvDrag { kind, lid, start: self.uv_of(local(q)), p0: p.clone() });
            }
        }
        if let (Some(d), Some(q)) = (self.uv_drag.clone(), pointer.interact_pos()) {
            if pointer.primary_down() && d.lid == lid {
                let cur = self.uv_of(local(q));
                let mut np = d.p0.clone();
                let piv = Vec2::from(d.p0.pivot);
                let m = mirror(&d.p0);
                match d.kind {
                    UvDragKind::Move => {
                        let mut delta = cur - d.start;
                        if mods.shift {
                            if delta.x.abs() > delta.y.abs() {
                                delta.y = 0.0;
                            } else {
                                delta.x = 0.0;
                            }
                        }
                        // Image, frame and pivot move together: rotating
                        // and scaling then turn around the moved pivot.
                        np.pivot = (piv + delta).into();
                        np.offset = (Vec2::from(d.p0.offset) - delta).into();
                    }
                    UvDragKind::MoveAxis(axis) => {
                        let delta = axis * (cur - d.start).dot(axis);
                        np.pivot = (piv + delta).into();
                        np.offset = (Vec2::from(d.p0.offset) - delta).into();
                    }
                    UvDragKind::Rotate => {
                        let a0 = (d.start - piv).to_angle();
                        let a1 = (cur - piv).to_angle();
                        // The frame follows the cursor (mirroring flips the sense).
                        let sense = m.x * m.y;
                        let mut r = d.p0.rotation - (a1 - a0).to_degrees() * sense;
                        if mods.shift {
                            r = (r / 15.0).round() * 15.0;
                        }
                        np.rotation = (r + 180.0).rem_euclid(360.0) - 180.0;
                    }
                    UvDragKind::Scale(h) => {
                        // The handle stays under the cursor: its texture point
                        // relative to the pivot keeps its value.
                        let target = (tex_of_frame(&d.p0, h) - piv - Vec2::from(d.p0.offset)) / d.p0.scale.max(1e-4);
                        let rc = rot((cur - piv) * m, d.p0.rotation.to_radians());
                        let st0 = stretch(&d.p0);
                        let mut st = st0;
                        for k in 0..2 {
                            let affects = if k == 0 { h.x != 0.5 } else { h.y != 0.5 };
                            if affects && rc[k].abs() > 1e-4 && target[k].abs() > 1e-4 {
                                st[k] = (target[k] / rc[k]).clamp(0.01, 100.0);
                            }
                        }
                        if mods.shift {
                            // Keep the aspect: the axis that moved most sets both.
                            let f = if (st.x / st0.x - 1.0).abs() >= (st.y / st0.y - 1.0).abs() { st.x / st0.x } else { st.y / st0.y };
                            st = st0 * f;
                        }
                        np.stretch = st.into();
                    }
                    UvDragKind::Pivot => {
                        // The texture stays put: the offset compensates.
                        let np_piv = cur;
                        let s = stretch(&d.p0) * d.p0.scale;
                        let o = Vec2::from(d.p0.offset) + piv - np_piv + rot((np_piv - piv) * m, d.p0.rotation.to_radians()) * s;
                        np.pivot = np_piv.into();
                        np.offset = o.into();
                    }
                }
                self.set_uv_projection(lid, np);
            }
        }
        if !pointer.primary_down() {
            self.uv_drag = None;
        }

        // What the cursor is on (or what is being dragged): it lights up.
        let hover_q = pointer.hover_pos().filter(|q| rect.contains(*q));
        let active = match &self.uv_drag {
            Some(d) => Some(d.kind),
            None => hover_q.map(|q| pick(q, &p, &|v| self.uv_of(v))),
        };
        let is = |k: UvDragKind| active == Some(k);
        let (_, p_now) = self.uv_gizmo_target().unwrap_or((lid, p.clone()));

        let painter = ui.painter_at(rect);
        let shadow = |w: f32| egui::Stroke::new(w + 2.0, Color32::from_black_alpha(130));
        let col_u = Color32::from_rgb(232, 84, 84);
        let col_v = Color32::from_rgb(98, 196, 110);
        let col_ring = Color32::from_rgb(86, 156, 240);
        let lit = |c: Color32, on: bool| if on { Color32::from_rgb(255, 236, 140) } else { c };

        // Frame.
        let moving = matches!(active, Some(UvDragKind::Move));
        let mut outline = corners.clone();
        outline.push(corners[0]);
        painter.add(egui::Shape::line(outline, shadow(1.5)));
        painter.add(egui::Shape::closed_line(corners.clone(), egui::Stroke::new(if moving { 2.5f32 } else { 1.5 }, crate::style::ACCENT)));

        // Rotation ring around the pivot, with the turn so far.
        let ring_on = is(UvDragKind::Rotate);
        painter.circle_stroke(pivot, ring, shadow(1.5));
        painter.circle_stroke(pivot, ring, egui::Stroke::new(if ring_on { 3.0f32 } else { 1.5 }, lit(col_ring, ring_on)));
        // A knob on the ring along the frame's U axis shows its angle.
        let knob = pivot + ex * ring;
        painter.circle_filled(knob, 4.5 * k, lit(col_ring, ring_on));
        painter.circle_stroke(knob, 4.5 * k, egui::Stroke::new(1.0f32, Color32::from_black_alpha(160)));
        if let Some(d) = self.uv_drag.as_ref().filter(|d| d.kind == UvDragKind::Rotate) {
            let a0 = (self.uv_screen(d.start) - pivot).angle();
            let turned = p_now.rotation - d.p0.rotation;
            let turned = (turned + 180.0).rem_euclid(360.0) - 180.0;
            let sense = -mirror(&d.p0).x * mirror(&d.p0).y;
            let a1 = a0 + (turned * sense).to_radians();
            let n = 32;
            let mut fan = vec![pivot];
            fan.extend((0..=n).map(|i| pivot + egui::Vec2::angled(a0 + (a1 - a0) * i as f32 / n as f32) * ring));
            painter.add(egui::Shape::convex_polygon(fan.clone(), col_ring.gamma_multiply(0.18), egui::Stroke::NONE));
            painter.line_segment([pivot, fan[1]], egui::Stroke::new(1.0f32, col_ring));
            painter.line_segment([pivot, *fan.last().unwrap()], egui::Stroke::new(1.5f32, lit(col_ring, true)));
        }

        // Scale handles: squares turned with the frame.
        for (i, h) in handles.iter().enumerate() {
            let hv = Vec2::new(HANDLES[i].0, HANDLES[i].1);
            let on = is(UvDragKind::Scale(hv));
            let s = hs * if on { 1.25 } else { 1.0 };
            let pts = plane_pts(*h, s);
            painter.add(egui::Shape::convex_polygon(pts.clone(), if on { lit(Color32::WHITE, true) } else { Color32::from_gray(235) }, egui::Stroke::new(1.0f32, Color32::from_black_alpha(180))));
            if on {
                // Which way it scales: out from the frame's middle.
                let mid = self.uv_screen(uv_of_frame(&p, Vec2::splat(0.5)));
                let dir = unit(*h - mid, ex);
                let (a, b) = (*h + dir * (s + 4.0), *h + dir * (s + 16.0 * k));
                arrow(&painter, a, b, 6.0 * k, egui::Stroke::new(2.0f32, lit(Color32::WHITE, true)));
                let (a, b) = (*h - dir * (s + 4.0), *h - dir * (s + 16.0 * k));
                arrow(&painter, a, b, 6.0 * k, egui::Stroke::new(2.0f32, lit(Color32::WHITE, true)));
            }
        }

        // Move arrows (U, V) and the free move square.
        for (dir, ax_uv, col, name) in [(ex, ax, col_u, "U"), (ey, ay, col_v, "V")] {
            let on = is(UvDragKind::MoveAxis(ax_uv));
            let w = if on { 3.0 } else { 2.0 };
            let (a, b) = (pivot + dir * arrow0, pivot + dir * arrow1);
            painter.line_segment([a, b], shadow(w));
            arrow(&painter, a, b + dir * 8.0 * k, 9.0 * k, egui::Stroke::new(w, lit(col, on)));
            painter.text(b + dir * 18.0 * k, egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(11.0), lit(col, on));
        }
        // Lit while hovered, or dragged from (where it was at the press).
        let plane_on = match &self.uv_drag {
            Some(d) => d.kind == UvDragKind::Move && (self.uv_screen(d.start) - (self.uv_screen(Vec2::from(d.p0.pivot)) + (ex + ey) * 26.0 * k)).length() <= 9.0 * k + 2.0,
            None => hover_q.is_some_and(|q| (q - plane.center()).length() <= 9.0 * k),
        };
        painter.add(egui::Shape::convex_polygon(
            plane_pts(plane.center(), 6.0 * k),
            if plane_on { Color32::from_rgba_unmultiplied(255, 236, 140, 200) } else { Color32::from_rgba_unmultiplied(240, 200, 90, 110) },
            egui::Stroke::new(1.0f32, Color32::from_rgb(240, 200, 90)),
        ));

        // Pivot: a ring with a cross along the frame's axes.
        let piv_on = is(UvDragKind::Pivot);
        let pc = lit(Color32::WHITE, piv_on);
        painter.circle_stroke(pivot, hs + 1.0, shadow(1.5));
        painter.circle_stroke(pivot, hs + 1.0, egui::Stroke::new(1.5f32, pc));
        painter.line_segment([pivot - ex * (hs + 4.0), pivot + ex * (hs + 4.0)], egui::Stroke::new(1.0f32, pc));
        painter.line_segment([pivot - ey * (hs + 4.0), pivot + ey * (hs + 4.0)], egui::Stroke::new(1.0f32, pc));

        // The cursor says what a drag would do; a readout what it does.
        if let Some(q) = pointer.hover_pos().filter(|q| rect.contains(*q)) {
            let icon = match active {
                Some(UvDragKind::Pivot) => egui::CursorIcon::Crosshair,
                Some(UvDragKind::Move) => egui::CursorIcon::Move,
                Some(UvDragKind::MoveAxis(_)) => {
                    let dir = if active == Some(UvDragKind::MoveAxis(ax)) { ex } else { ey };
                    if dir.x.abs() >= dir.y.abs() {
                        egui::CursorIcon::ResizeHorizontal
                    } else {
                        egui::CursorIcon::ResizeVertical
                    }
                }
                Some(UvDragKind::Scale(h)) => {
                    let mid = self.uv_screen(uv_of_frame(&p, Vec2::splat(0.5)));
                    let i = HANDLES.iter().position(|x| Vec2::new(x.0, x.1) == h).unwrap_or(0);
                    let dir = unit(handles[i] - mid, ex);
                    if dir.x.abs() > 2.0 * dir.y.abs() {
                        egui::CursorIcon::ResizeHorizontal
                    } else if dir.y.abs() > 2.0 * dir.x.abs() {
                        egui::CursorIcon::ResizeVertical
                    } else if dir.x * dir.y > 0.0 {
                        egui::CursorIcon::ResizeNwSe
                    } else {
                        egui::CursorIcon::ResizeNeSw
                    }
                }
                Some(UvDragKind::Rotate) => {
                    if self.uv_drag.is_some() {
                        egui::CursorIcon::Grabbing
                    } else {
                        egui::CursorIcon::Grab
                    }
                }
                None => egui::CursorIcon::Default,
            };
            ui.ctx().set_cursor_icon(icon);
            let readout = match (&self.uv_drag, active) {
                (Some(d), Some(kind)) => Some(match kind {
                    UvDragKind::Move | UvDragKind::MoveAxis(_) => {
                        let m = Vec2::from(p_now.pivot) - Vec2::from(d.p0.pivot);
                        format!("Move  U {:+.3}  V {:+.3}", m.x, m.y)
                    }
                    UvDragKind::Rotate => format!("Rotate {:.1}\u{00b0}", p_now.rotation),
                    UvDragKind::Scale(_) => {
                        let s0 = stretch(&d.p0);
                        let s1 = stretch(&p_now);
                        format!("Scale  U {:.0}%  V {:.0}%", s1.x / s0.x * 100.0, s1.y / s0.y * 100.0)
                    }
                    UvDragKind::Pivot => format!("Pivot  {:.3}, {:.3}", p_now.pivot[0], p_now.pivot[1]),
                }),
                (None, Some(kind)) => Some(
                    match kind {
                        UvDragKind::Move => "Move (Shift: one axis)",
                        UvDragKind::MoveAxis(_) => "Move along this axis",
                        UvDragKind::Rotate => "Rotate around the pivot (Shift: 15\u{00b0} steps)",
                        UvDragKind::Scale(_) => "Scale (Shift: keep the aspect)",
                        UvDragKind::Pivot => "Move the pivot",
                    }
                    .to_string(),
                ),
                _ => None,
            };
            if let Some(t) = readout {
                let g = painter.layout_no_wrap(t, egui::FontId::proportional(11.5), Color32::WHITE);
                let at = q + egui::vec2(18.0, 14.0);
                let r = Rect::from_min_size(at, g.size() + egui::vec2(12.0, 6.0));
                painter.rect_filled(r, 4.0, Color32::from_black_alpha(190));
                painter.galley(at + egui::vec2(6.0, 3.0), g, Color32::WHITE);
            }
        }
        let _ = Sense::hover();
        true
    }

    /// Context toolbar items of the manipulator (the 2D view is shown and a
    /// UV-projected fill is selected).
    pub(super) fn uv_gizmo_toolbar(&mut self, ui: &mut egui::Ui) {
        let Some((lid, p)) = self.uv_gizmo_target() else { return };
        if self.view.layout == Layout::Only3D {
            return;
        }
        ui.toggle_value(&mut self.uv_gizmo_show, "Manipulator").on_hover_text("Show / hide the UV transformation manipulator (2D view)");
        egui::ComboBox::from_id_salt("uv gizmo size")
            .width(70.0)
            .selected_text(["Small", "Medium", "Large"][self.uv_gizmo_size.min(2) as usize])
            .show_ui(ui, |ui| {
                for (i, n) in ["Small", "Medium", "Large"].iter().enumerate() {
                    ui.selectable_value(&mut self.uv_gizmo_size, i as u8, *n);
                }
            })
            .response
            .on_hover_text("Manipulator handles size");
        if ui.button("Reset pivot").on_hover_text("Pivot back to the middle of the transformation").clicked() {
            let mut np = p.clone();
            let mid = uv_of_frame(&p, Vec2::splat(0.5));
            let piv = Vec2::from(p.pivot);
            let s = stretch(&p) * p.scale;
            let o = Vec2::from(p.offset) + piv - mid + rot((mid - piv) * mirror(&p), p.rotation.to_radians()) * s;
            np.pivot = mid.into();
            np.offset = o.into();
            self.set_uv_projection(lid, np);
        }
        if ui.button("Reset transformation").clicked() {
            let mut np = p.clone();
            let d = Projection::default();
            (np.offset, np.rotation, np.stretch, np.pivot, np.mirror) = (d.offset, d.rotation, d.stretch, d.pivot, d.mirror);
            self.set_uv_projection(lid, np);
        }
    }
}
