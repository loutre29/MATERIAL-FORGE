//! Viewport manipulator of a fill's 3D projection (planar, tri-planar,
//! object 3D), like Painter's projection gizmo: move along an axis or in
//! the view plane, rotate around an axis, scale along an axis or
//! uniformly. W / E / R or the context toolbar pick the mode.

use super::*;
use crate::document::{find_mut, LayerKind, MaskEffectKind, ProjMode, Projection, WorldMap};
use eframe::egui::{Color32, Pos2};
use glam::{Quat, Vec3};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GizmoMode {
    #[default]
    Move,
    Rotate,
    Scale,
}

impl GizmoMode {
    pub const ALL: [GizmoMode; 3] = [GizmoMode::Move, GizmoMode::Rotate, GizmoMode::Scale];

    pub fn name(self) -> &'static str {
        match self {
            GizmoMode::Move => "Move (W)",
            GizmoMode::Rotate => "Rotate (E)",
            GizmoMode::Scale => "Scale (R)",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handle {
    /// Arrow (move), box-ended line (scale) or ring (rotate) of axis 0..3.
    Axis(usize),
    /// Center square: free move in the view plane, uniform scale.
    Center,
}

pub struct GizmoDrag {
    handle: Handle,
    /// Pointer angle around the pivot on the screen (rotation).
    angle: f32,
}

const AXIS_COLORS: [Color32; 3] = [Color32::from_rgb(232, 72, 72), Color32::from_rgb(96, 204, 96), Color32::from_rgb(82, 132, 244)];
const HOT: Color32 = Color32::from_rgb(255, 214, 64);

/// Screen geometry of the manipulator this frame.
struct Frame {
    pivot: Vec3,
    rot: Quat,
    /// Handle length in mesh units (constant on screen).
    len: f32,
    to_screen: Box<dyn Fn(Vec3) -> Option<Pos2>>,
}

impl Frame {
    fn axis(&self, i: usize) -> Vec3 {
        self.rot * [Vec3::X, Vec3::Y, Vec3::Z][i]
    }

    fn ring(&self, i: usize) -> Vec<Pos2> {
        let (a, b) = (self.axis((i + 1) % 3), self.axis((i + 2) % 3));
        (0..=48)
            .filter_map(|k| {
                let t = k as f32 / 48.0 * std::f32::consts::TAU;
                (self.to_screen)(self.pivot + (a * t.cos() + b * t.sin()) * self.len)
            })
            .collect()
    }
}

fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t - p).length()
}

fn poly_dist(p: Pos2, pts: &[Pos2]) -> f32 {
    pts.windows(2).map(|w| seg_dist(p, w[0], w[1])).fold(f32::MAX, f32::min)
}

impl RetroApp {
    /// The projection the manipulator edits: the selected fill layer's, or
    /// the selected mask fill effect's, when it is placed in 3D.
    fn gizmo_projection(&mut self) -> Option<(&mut Projection, WorldMap)> {
        let doc = self.doc.as_mut()?;
        let s = &mut doc.sets[doc.active];
        let world = s.world;
        let target = s.target;
        let id = s.selected?;
        let l = find_mut(&mut s.layers, id)?;
        let p = match target {
            Target::Content => match &mut l.desc.kind {
                LayerKind::Fill(f) => &mut f.projection,
                _ => return None,
            },
            Target::MaskEffect(eid) => match l.desc.mask.as_mut()?.effects.iter_mut().find(|e| e.id == eid).map(|e| &mut e.kind) {
                Some(MaskEffectKind::Fill { projection, .. }) => projection,
                _ => return None,
            },
            _ => return None,
        };
        (p.mode != ProjMode::Uv).then_some((p, world))
    }

    /// Draws the manipulator and handles its drags. Returns true while the
    /// pointer is on a handle or a handle is being dragged (no painting then).
    pub(super) fn projection_gizmo(&mut self, ui: &egui::Ui, rect: Rect, pointer: &egui::PointerState, mods: egui::Modifiers, ours: bool) -> bool {
        let camera = self.camera;
        let mode = self.gizmo_mode;
        let Some((proj, world)) = self.gizmo_projection() else {
            self.gizmo_drag = None;
            return false;
        };
        let (pivot, rot, _) = proj.frame(&world);
        let eye = camera.eye();
        let vp = camera.view_proj(rect.width() / rect.height().max(1.0));
        let to_screen = move |w: Vec3| -> Option<Pos2> {
            let c = vp * w.extend(1.0);
            if c.w <= 1e-5 {
                return None;
            }
            let n = c.truncate() / c.w;
            Some(Pos2::new(rect.min.x + (n.x * 0.5 + 0.5) * rect.width(), rect.min.y + (0.5 - n.y * 0.5) * rect.height()))
        };
        let Some(ps) = to_screen(pivot) else { return false };
        let len = (eye - pivot).length().max(1e-4) * 0.16;
        let f = Frame { pivot, rot, len, to_screen: Box::new(to_screen) };
        let tips: Vec<Option<Pos2>> = (0..3).map(|i| (f.to_screen)(pivot + f.axis(i) * len)).collect();

        // Handle under the pointer (Alt navigates, it never grabs).
        let hover = match pointer.hover_pos().filter(|p| rect.contains(*p) && !mods.alt) {
            Some(p) if self.gizmo_drag.is_none() => {
                let mut best: Option<(f32, Handle)> = None;
                let mut consider = |d: f32, h: Handle| {
                    if d < 7.0 && best.map_or(true, |(bd, _)| d < bd) {
                        best = Some((d, h));
                    }
                };
                if mode != GizmoMode::Rotate && (p - ps).abs().max_elem() < 7.0 {
                    consider(0.0, Handle::Center);
                }
                for i in 0..3 {
                    match mode {
                        GizmoMode::Rotate => consider(poly_dist(p, &f.ring(i)), Handle::Axis(i)),
                        _ => {
                            if let Some(t) = tips[i] {
                                consider(seg_dist(p, ps, t), Handle::Axis(i));
                            }
                        }
                    }
                }
                best.map(|b| b.1)
            }
            _ => None,
        };

        // Start, run and end a drag.
        if pointer.primary_pressed() && ours {
            if let (Some(h), Some(p)) = (hover, pointer.interact_pos()) {
                self.gizmo_drag = Some(GizmoDrag { handle: h, angle: (p - ps).angle() });
            }
        }
        let mut active = self.gizmo_drag.as_ref().map(|d| d.handle);
        if active.is_some() && !pointer.primary_down() {
            self.gizmo_drag = None;
            active = None;
        }
        if let (Some(h), Some(p)) = (active, pointer.interact_pos()) {
            let delta = pointer.delta();
            let prev_angle = self.gizmo_drag.as_ref().map(|d| d.angle);
            let mut new_angle = None;
            let Some((proj, _)) = self.gizmo_projection() else { return false };
            let fwd = (pivot - eye).normalize_or_zero();
            match (mode, h) {
                (GizmoMode::Move, Handle::Axis(i)) => {
                    if let Some(t) = tips[i] {
                        let ds = t - ps;
                        let amount = delta.dot(ds) / ds.length_sq().max(1.0) * len;
                        let pos = Vec3::from(proj.position) + f.axis(i) * amount;
                        proj.position = pos.to_array();
                    }
                }
                (GizmoMode::Move, Handle::Center) => {
                    // In the view plane, at the pivot's depth.
                    let right = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
                    let up = right.cross(fwd);
                    if let Some(r) = (f.to_screen)(pivot + right * len) {
                        let px = (r - ps).length().max(1.0);
                        let pos = Vec3::from(proj.position) + (right * delta.x - up * delta.y) * (len / px);
                        proj.position = pos.to_array();
                    }
                }
                (GizmoMode::Rotate, Handle::Axis(i)) => {
                    let a = (p - ps).angle();
                    let prev = prev_angle.unwrap_or(a);
                    let mut da = a - prev;
                    if da > std::f32::consts::PI {
                        da -= std::f32::consts::TAU;
                    } else if da < -std::f32::consts::PI {
                        da += std::f32::consts::TAU;
                    }
                    // Screen y points down: seen from the axis tip a positive
                    // turn is clockwise on screen.
                    let axis = f.axis(i);
                    let facing = axis.dot(eye - pivot) > 0.0;
                    let turn = if facing { -da } else { da };
                    let q = (Quat::from_axis_angle(axis, turn) * proj.rotation3()).normalize();
                    proj.orient = q.to_array();
                    new_angle = Some(a);
                }
                (GizmoMode::Scale, Handle::Axis(i)) => {
                    if let Some(t) = tips[i] {
                        let ds = t - ps;
                        let k = (1.0 + delta.dot(ds) / ds.length_sq().max(1.0)).clamp(0.5, 2.0);
                        proj.axes[i] = (proj.axes[i] * k).clamp(0.001, 1000.0);
                    }
                }
                (GizmoMode::Scale, Handle::Center) => {
                    let k = (1.0 + (delta.x - delta.y) * 0.005).clamp(0.5, 2.0);
                    for a in proj.axes.iter_mut() {
                        *a = (*a * k).clamp(0.001, 1000.0);
                    }
                }
                (GizmoMode::Rotate, Handle::Center) => {}
            }
            if let (Some(a), Some(d)) = (new_angle, &mut self.gizmo_drag) {
                d.angle = a;
            }
        }

        // Draw.
        let painter = ui.painter_at(rect);
        let lit = active.or(hover);
        let col = |h: Handle, c: Color32| if lit == Some(h) { HOT } else { c };
        let stroke = |c: Color32| egui::Stroke::new(2.0f32, c);
        let shadow = egui::Stroke::new(4.0f32, Color32::from_black_alpha(90));
        match mode {
            GizmoMode::Rotate => {
                for i in 0..3 {
                    let pts = f.ring(i);
                    painter.add(egui::Shape::line(pts.clone(), shadow));
                    painter.add(egui::Shape::line(pts, stroke(col(Handle::Axis(i), AXIS_COLORS[i]))));
                }
            }
            _ => {
                for i in 0..3 {
                    let Some(t) = tips[i] else { continue };
                    let c = col(Handle::Axis(i), AXIS_COLORS[i]);
                    painter.line_segment([ps, t], shadow);
                    painter.line_segment([ps, t], stroke(c));
                    let dir = (t - ps).normalized();
                    if mode == GizmoMode::Move {
                        let side = egui::vec2(-dir.y, dir.x) * 5.0;
                        painter.add(egui::Shape::convex_polygon(vec![t + dir * 12.0, t + side, t - side], c, egui::Stroke::NONE));
                    } else {
                        painter.rect_filled(Rect::from_center_size(t, egui::vec2(9.0, 9.0)), 1.0, c);
                    }
                }
                let c = col(Handle::Center, Color32::from_gray(235));
                painter.rect_stroke(Rect::from_center_size(ps, egui::vec2(10.0, 10.0)), 1.0, egui::Stroke::new(1.5f32, c));
            }
        }
        if hover.is_some() || active.is_some() {
            ui.ctx().set_cursor_icon(if active.is_some() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
        }
        hover.is_some() || active.is_some()
    }
}
