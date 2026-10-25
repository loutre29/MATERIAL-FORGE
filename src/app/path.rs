//! Path tool (P): non-destructive paths on a path layer. Points clicked on
//! the mesh (3D view) or in the UV space (2D view) define curves; the layer's
//! paint is generated from them, so a path stays editable for good: move a
//! point or a handle, change the layer's brush (stitch spacing, size,
//! angle, jitter, alpha) or material, and the layer is painted again. All
//! the paths of a layer share its brush and material (`PathStyle`): other
//! settings mean another path layer.
//!
//! - Click on the mesh: a new path (on the active path layer, or a new one),
//!   or one more point when an end of the selected path is selected.
//! - Pen mode: click and drag pulls out the handles of a smooth point;
//!   Curve mode: points get smooth handles from their neighbors.
//! - Drag a point or a handle to move it; Shift + drag a handle breaks the
//!   tangent; Shift + click a point switches it between smooth and corner.
//! - Click on a curve: inserts a point; click the first point from the last
//!   one: closes the path.
//! - Backspace / Delete removes the selected point, Escape ends the path.
//!
//! The paths only show while the tool is active. 3D paths paint in world
//! space (stamps projected on the surface along the curve), so the result
//! does not depend on the camera.

use super::*;
use crate::brush::{BrushMaterial, BrushSettings};
use crate::document::{find, LayerKind, PathDesc, PathPoint, PathStyle, PointKind};
use crate::engine::Tex;
use std::collections::HashSet;
use crate::engine::paint::{BrushUniform, Stamp, MODE_UV, MODE_WRAP};
use eframe::egui::{Color32, Pos2, Rect};
use glam::{Vec2, Vec3};

/// What the left button drags.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PathDrag {
    Point(usize),
    /// A handle of a point: its outgoing (true) or incoming end.
    Handle(usize, bool),
    /// Pen mode: the handles of a point just placed.
    Pull(usize),
}

#[derive(Default)]
pub struct PathEdit {
    /// Selected path (id, on the active path layer) and point.
    pub path: Option<u64>,
    pub point: Option<usize>,
    pub drag: Option<PathDrag>,
    /// Pen mode (click and drag pulls out handles) instead of smooth points.
    pub pen: bool,
    /// The tool's brush and material before a path layer's were shown in
    /// them; back when leaving the tool or the path layer.
    saved: Option<(BrushSettings, BrushMaterial)>,
    /// The path layer the brush properties edit: (set, layer, its style as
    /// last synced).
    bound: Option<(usize, u64, String)>,
    /// The current gesture already recorded its undo step.
    pushed: bool,
}

/// Grab distance of a point or a handle, in points.
const GRAB: f32 = 9.0;
/// Stamps of one path at most.
const MAX_PATH_STAMPS: usize = 60_000;

/// Each point's handle ends (in, out) relative to it; Auto points take
/// tangents from their neighbors.
pub fn effective_handles(p: &PathDesc) -> Vec<(Vec3, Vec3)> {
    let n = p.points.len();
    let closed = p.closed && n > 2;
    let pos = |i: usize| Vec3::from(p.points[i].pos);
    (0..n)
        .map(|i| {
            let pt = &p.points[i];
            if pt.kind != PointKind::Auto {
                return (Vec3::from(pt.h_in), Vec3::from(pt.h_out));
            }
            let c = pos(i);
            let prev = if i > 0 { Some(pos(i - 1)) } else if closed { Some(pos(n - 1)) } else { None };
            let next = if i + 1 < n { Some(pos(i + 1)) } else if closed { Some(pos(0)) } else { None };
            // 3D: the tangent stays in the surface's plane.
            let flat = |t: Vec3| {
                let nrm = Vec3::from(pt.nrm);
                if p.uv || nrm == Vec3::ZERO {
                    t
                } else {
                    t - nrm * t.dot(nrm)
                }
            };
            match (prev, next) {
                (Some(a), Some(b)) => {
                    let t = flat(b - a).normalize_or_zero();
                    (-t * c.distance(a) / 3.0, t * c.distance(b) / 3.0)
                }
                (None, Some(b)) => (Vec3::ZERO, flat(b - c) / 3.0),
                (Some(a), None) => (flat(a - c) / 3.0, Vec3::ZERO),
                _ => (Vec3::ZERO, Vec3::ZERO),
            }
        })
        .collect()
}

/// Number of curve segments.
fn segments(p: &PathDesc) -> usize {
    let n = p.points.len();
    if n < 2 {
        0
    } else if p.closed && n > 2 {
        n
    } else {
        n - 1
    }
}

/// The curve as (position, normal, segment) samples; segment `s` gets
/// `per_seg(s, length of its control polygon)` samples.
pub fn curve(p: &PathDesc, per_seg: impl Fn(usize, f32) -> usize) -> Vec<(Vec3, Vec3, usize)> {
    let n = p.points.len();
    if n == 0 {
        return Vec::new();
    }
    let pos = |i: usize| Vec3::from(p.points[i].pos);
    let nrm = |i: usize| Vec3::from(p.points[i].nrm);
    let mut out = vec![(pos(0), nrm(0), 0)];
    let h = effective_handles(p);
    for s in 0..segments(p) {
        let e = (s + 1) % n;
        let (b0, b1, b2, b3) = (pos(s), pos(s) + h[s].1, pos(e) + h[e].0, pos(e));
        let len = b0.distance(b1) + b1.distance(b2) + b2.distance(b3);
        let k = per_seg(s, len).max(1);
        for j in 1..=k {
            let t = j as f32 / k as f32;
            let u = 1.0 - t;
            let q = b0 * (u * u * u) + b1 * (3.0 * u * u * t) + b2 * (3.0 * u * t * t) + b3 * (t * t * t);
            out.push((q, nrm(s).lerp(nrm(e), t).normalize_or_zero(), s));
        }
    }
    out
}

/// The stamps a path paints with its layer's style: world stamps along the
/// surface (3D paths) or UV stamps (2D paths), always the same for the same
/// path.
pub fn path_stamps(p: &PathDesc, style: &PathStyle, doc: &crate::document::Document) -> Vec<Stamp> {
    if p.points.is_empty() {
        return Vec::new();
    }
    let b = &style.brush;
    let unit = if p.uv { style.uv_unit } else { style.unit };
    let size = (b.size * unit).max(1e-7);
    let step = (size * 2.0 * b.spacing).max(size * 0.02);
    let fine = step * 0.25;
    let mut pts = curve(p, |_, len| ((len / fine).ceil() as usize).clamp(2, 20_000));
    if !p.uv {
        // Between its points the curve leaves the surface: back onto it,
        // along the interpolated normal.
        let diag = doc.mesh.diagonal();
        let n = p.points.len();
        let chord = |s: usize| Vec3::from(p.points[s].pos).distance(Vec3::from(p.points[(s + 1) % n].pos));
        for q in pts.iter_mut() {
            let nrm = if q.1 == Vec3::ZERO { Vec3::Y } else { q.1 };
            let off = (chord(q.2) * 0.3).max(diag * 0.003);
            let near = |h: &crate::document::Hit| h.pos.distance(q.0) <= off * 1.5;
            let hit = doc.pick(q.0 + nrm * off, -nrm).filter(near).or_else(|| doc.pick(q.0 - nrm * off, nrm).filter(near));
            if let Some(h) = hit {
                *q = (h.pos, h.normal, q.2);
            }
        }
    }
    let mut rng = p.seed | 1;
    let mut rand = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 40) as f32 / (1u64 << 24) as f32
    };
    let right = Vec3::from(p.right);
    let c = (doc.mesh.bbox_min + doc.mesh.bbox_max) * 0.5;
    let axes: Vec<usize> = (0..3).filter(|&a| b.symmetry[a] && !p.uv).collect();
    let mut out = Vec::new();
    let mut stamp = |pos: Vec3, n: Vec3, dir: Vec3, out: &mut Vec<Stamp>| {
        let (r1, r2, r3, r4, r5) = (rand(), rand(), rand(), rand(), rand());
        let s = size * (1.0 + (r1 * 2.0 - 1.0) * b.size_jitter).max(0.05);
        let weight = (b.flow * (1.0 - r2 * b.flow_jitter)).clamp(0.0, 1.0);
        let mut angle = b.angle.to_radians() + (r3 * 2.0 - 1.0) * b.angle_jitter.to_radians();
        let (jx, jy) = ((r4 * 2.0 - 1.0) * b.position_jitter * size, (r5 * 2.0 - 1.0) * b.position_jitter * size);
        if p.uv {
            if b.follow_path {
                angle += dir.y.atan2(dir.x);
            }
            out.push(Stamp { pos: [pos.x + jx, pos.y + jy, 0.0, s], nrm: [0.0, 0.0, 1.0, weight], ext: [angle, 0.0, 0.0, 0.0] });
            return;
        }
        let n = n.normalize_or(Vec3::Y);
        let f = if b.follow_path { dir } else { right };
        let mut t = f - n * f.dot(n);
        if t.length_squared() < 1e-12 {
            t = n.any_orthonormal_vector();
        }
        let t = t.normalize();
        let bt = n.cross(t);
        // Jitter in the stamp's frame (y down, like the screen).
        let pos = pos + t * jx - bt * jy;
        out.push(Stamp { pos: [pos.x, pos.y, pos.z, s], nrm: [n.x, n.y, n.z, weight], ext: [angle, t.x, t.y, t.z] });
        for mask in 1u32..(1 << axes.len()) {
            let (mut q, mut m, mut tt, mut a) = (pos, n, t, angle);
            for (k, &axis) in axes.iter().enumerate() {
                if mask & (1 << k) != 0 {
                    q[axis] = 2.0 * c[axis] - q[axis];
                    m[axis] = -m[axis];
                    tt[axis] = -tt[axis];
                    a = -a;
                }
            }
            out.push(Stamp { pos: [q.x, q.y, q.z, s], nrm: [m.x, m.y, m.z, weight], ext: [a, tt.x, tt.y, tt.z] });
        }
    };
    if pts.len() == 1 {
        stamp(pts[0].0, pts[0].1, right, &mut out);
        return out;
    }
    let total: f32 = pts.windows(2).map(|w| w[0].0.distance(w[1].0)).sum();
    // A closed path does not stamp its start twice.
    let end = if p.closed && p.points.len() > 2 { total - step * 0.5 } else { total + step * 1e-3 };
    let (mut next, mut acc) = (0.0f32, 0.0f32);
    for w in pts.windows(2) {
        let ((a, na, _), (bq, nb, _)) = (w[0], w[1]);
        let len = a.distance(bq);
        if len <= 0.0 {
            continue;
        }
        let dir = (bq - a) / len;
        while next <= acc + len && next <= end && out.len() < MAX_PATH_STAMPS {
            let f = (next - acc) / len;
            stamp(a.lerp(bq, f), na.lerp(nb, f), dir, &mut out);
            next += step;
        }
        acc += len;
    }
    out
}

impl RetroApp {
    /// UV to a point of the 2D view (local), the inverse of `uv_of`.
    pub(super) fn uv_to_local(&self, uv: Vec2) -> Vec2 {
        let ppp = self.engine.uvview.width as f32 / self.uv_rect.width().max(1.0);
        self.uv_px(uv) / ppp
    }

    /// The active layer's paths (when it is a path layer).
    pub(super) fn active_paths(&self) -> Option<&Vec<PathDesc>> {
        let doc = self.doc.as_ref()?;
        doc.sets[doc.active].selected_layer()?.desc.paths.as_ref()
    }

    pub(super) fn active_paths_mut(&mut self) -> Option<&mut Vec<PathDesc>> {
        let doc = self.doc.as_mut()?;
        let set = doc.active;
        doc.sets[set].selected_layer_mut()?.desc.paths.as_mut()
    }

    /// The active path layer's brush and material.
    pub(super) fn active_style(&self) -> Option<&PathStyle> {
        let doc = self.doc.as_ref()?;
        let l = doc.sets[doc.active].selected_layer()?;
        l.desc.paths.as_ref()?;
        l.desc.path_style.as_ref()
    }

    pub(super) fn active_style_mut(&mut self) -> Option<&mut PathStyle> {
        let doc = self.doc.as_mut()?;
        let set = doc.active;
        let l = doc.sets[set].selected_layer_mut()?;
        l.desc.paths.as_ref()?;
        l.desc.path_style.as_mut()
    }

    pub(super) fn selected_path(&self) -> Option<&PathDesc> {
        let id = self.path.path?;
        self.active_paths()?.iter().find(|p| p.id == id)
    }

    pub(super) fn selected_path_mut(&mut self) -> Option<&mut PathDesc> {
        let id = self.path.path?;
        self.active_paths_mut()?.iter_mut().find(|p| p.id == id)
    }

    /// The undo step of the current gesture, once, right before its first
    /// change.
    fn path_checkpoint(&mut self) {
        if !self.path.pushed {
            self.path.pushed = true;
            self.checkpoint("Edit path");
        }
    }

    /// A path point on screen (local to its view).
    fn path_local(&self, uv: bool, q: Vec3) -> Option<Vec2> {
        if uv {
            Some(self.uv_to_local(q.truncate()))
        } else {
            self.project_vp(q)
        }
    }

    /// The surface (position, normal, camera distance) or the UV under a
    /// view point.
    fn path_surface(&self, p: Vec2, uv: bool) -> Option<(Vec3, Vec3, f32)> {
        if uv {
            let u = self.uv_of(p);
            Some((Vec3::new(u.x, u.y, 0.0), Vec3::ZERO, 0.0))
        } else {
            self.pick_at(p).map(|h| (h.pos, h.normal, h.dist))
        }
    }

    /// Where a handle dragged to view point `p` ends: in UV, or on the
    /// surface's tangent plane at point `i` (the camera's plane when that
    /// one is seen edge on).
    fn handle_at(&self, path: &PathDesc, p: Vec2, i: usize) -> Option<Vec3> {
        let pt = path.points.get(i)?;
        let base = Vec3::from(pt.pos);
        if path.uv {
            let u = self.uv_of(p);
            return Some(Vec3::new(u.x, u.y, 0.0));
        }
        let view = self.view_info();
        let (o, d) = view.ray(p * view.ppp);
        let mut n = Vec3::from(pt.nrm);
        if n == Vec3::ZERO || d.dot(n).abs() < 0.25 {
            n = -d;
        }
        let den = d.dot(n);
        if den.abs() < 1e-6 {
            return None;
        }
        let t = (base - o).dot(n) / den;
        (t > 0.0).then(|| o + d * t)
    }

    /// The paths of the active layer shown in this view.
    fn view_paths(&self, uv: bool) -> Vec<&PathDesc> {
        self.active_paths().map(|v| v.iter().filter(|p| p.uv == uv).collect()).unwrap_or_default()
    }

    /// The point under the cursor (the selected path's first).
    fn point_near(&self, p: Vec2, uv: bool) -> Option<(u64, usize)> {
        let mut best: Option<(u64, usize, f32)> = None;
        for path in self.view_paths(uv) {
            let bias = if Some(path.id) == self.path.path { 0.5 } else { 0.0 };
            for (i, q) in path.points.iter().enumerate() {
                let Some(s) = self.path_local(uv, Vec3::from(q.pos)) else { continue };
                let d = s.distance(p) - bias;
                if d < GRAB && best.is_none_or(|b| d < b.2) {
                    best = Some((path.id, i, d));
                }
            }
        }
        best.map(|(id, i, _)| (id, i))
    }

    /// The handles shown: the selected point's and its neighbors'.
    fn shown_handles(&self) -> Vec<(usize, bool, Vec3)> {
        let (Some(path), Some(sel)) = (self.selected_path(), self.path.point) else { return Vec::new() };
        let n = path.points.len();
        if n < 2 || sel >= n {
            return Vec::new();
        }
        let h = effective_handles(path);
        let closed = path.closed && n > 2;
        let mut idx = vec![sel];
        for j in [(sel + n - 1) % n, (sel + 1) % n] {
            let linked = closed || j.abs_diff(sel) == 1;
            if linked && !idx.contains(&j) {
                idx.push(j);
            }
        }
        let mut out = Vec::new();
        for i in idx {
            let base = Vec3::from(path.points[i].pos);
            // Ends of an open path have one handle.
            if (i + 1 < n || closed) && h[i].1 != Vec3::ZERO {
                out.push((i, true, base + h[i].1));
            }
            if (i > 0 || closed) && h[i].0 != Vec3::ZERO {
                out.push((i, false, base + h[i].0));
            }
        }
        out
    }

    fn handle_near(&self, p: Vec2, uv: bool) -> Option<PathDrag> {
        if self.selected_path().is_none_or(|s| s.uv != uv) {
            return None;
        }
        self.shown_handles()
            .into_iter()
            .filter_map(|(i, out, e)| self.path_local(uv, e).map(|s| (i, out, s.distance(p))))
            .filter(|x| x.2 < GRAB)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(i, out, _)| PathDrag::Handle(i, out))
    }

    /// A path's curve on screen: (point, segment) samples.
    fn screen_curve(&self, path: &PathDesc) -> Vec<(Vec2, usize)> {
        let uv = path.uv;
        let screen: Vec<Option<Vec2>> = path.points.iter().map(|q| self.path_local(uv, Vec3::from(q.pos))).collect();
        let n = path.points.len().max(1);
        let samples = |s: usize, _: f32| match (screen[s], screen[(s + 1) % n]) {
            (Some(a), Some(b)) => ((a.distance(b) / 3.0).ceil() as usize).clamp(6, 600),
            _ => 24,
        };
        curve(path, samples).into_iter().filter_map(|(q, _, s)| self.path_local(uv, q).map(|v| (v, s))).collect()
    }

    /// A segment under the cursor: (path, segment).
    fn curve_near(&self, p: Vec2, uv: bool) -> Option<(u64, usize)> {
        let mut best: Option<(u64, usize, f32)> = None;
        for path in self.view_paths(uv) {
            let c = self.screen_curve(path);
            for w in c.windows(2) {
                let (a, b) = (w[0].0, w[1].0);
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
                let d = (a + ab * t).distance(p);
                if d < GRAB * 0.7 && best.is_none_or(|x| d < x.2) {
                    best = Some((path.id, w[1].1, d));
                }
            }
        }
        best.map(|(id, s, _)| (id, s))
    }

    fn new_point(q: Vec3, n: Vec3, pen: bool) -> PathPoint {
        PathPoint { pos: q.into(), nrm: n.into(), h_in: [0.0; 3], h_out: [0.0; 3], kind: if pen { PointKind::Broken } else { PointKind::Auto } }
    }

    /// The camera's right vector (world).
    fn camera_right(&self) -> Vec3 {
        let view = self.view_info();
        let r = view.inv_view_proj * glam::Vec4::new(1.0, 0.0, 0.5, 1.0);
        let c = view.inv_view_proj * glam::Vec4::new(0.0, 0.0, 0.5, 1.0);
        (r.truncate() / r.w - c.truncate() / c.w).normalize_or(Vec3::X)
    }

    /// The active layer if it is a path layer, else a new path layer above
    /// it (its own undo step).
    fn ensure_path_layer(&mut self) -> bool {
        if self.active_paths().is_some() {
            self.path_checkpoint();
            return true;
        }
        let made = self.new_path_layer();
        self.path.pushed = true;
        made
    }

    /// A new, empty path layer above the active layer, painting with the
    /// brush and material shown now (its own undo step). Paths that need
    /// other settings go on their own layer.
    pub(super) fn new_path_layer(&mut self) -> bool {
        let Some(doc) = &self.doc else { return false };
        let set = doc.active;
        let mut count = 0;
        crate::document::visit(&doc.sets[set].layers, &mut |l| count += l.desc.paths.is_some() as usize);
        let style = PathStyle {
            brush: BrushSettings { lazy: 0.0, ..self.brush.clone() },
            material: self.mat.clone(),
            fill: self.mat.material.clone(),
            ..Default::default()
        };
        self.checkpoint("New path layer");
        let Some(doc) = &mut self.doc else { return false };
        let mut l = doc.new_layer(&self.engine, set, LayerKind::Paint, Some(format!("Path layer {}", count + 1)));
        l.desc.paths = Some(Vec::new());
        l.desc.path_style = Some(style);
        doc.insert_layer(set, l);
        self.path.path = None;
        self.path.point = None;
        self.path.drag = None;
        self.toast("New path layer: its paths share its brush and material.".into());
        true
    }

    /// Press: grabs a handle or a point, inserts a point on a curve, adds
    /// one at an end of the selected path, or starts a new path.
    pub(super) fn path_press(&mut self, p: Vec2, uv: bool, shift: bool) {
        self.path.drag = None;
        self.path.pushed = false;
        if let Some(h) = self.handle_near(p, uv) {
            if let (true, PathDrag::Handle(i, _)) = (shift, h) {
                // Shift: the two handles stop following each other.
                self.path_checkpoint();
                if let Some(path) = self.selected_path_mut() {
                    let eff = effective_handles(path);
                    let pt = &mut path.points[i];
                    (pt.h_in, pt.h_out, pt.kind) = (eff[i].0.into(), eff[i].1.into(), PointKind::Broken);
                }
            }
            self.path.drag = Some(h);
            return;
        }
        if let Some((pid, i)) = self.point_near(p, uv) {
            if shift {
                // Shift + click: smooth (from the neighbors) or a corner.
                self.path.path = Some(pid);
                self.path.point = Some(i);
                self.path_checkpoint();
                if let Some(pt) = self.selected_path_mut().and_then(|s| s.points.get_mut(i)) {
                    pt.kind = if pt.kind == PointKind::Auto { PointKind::Broken } else { PointKind::Auto };
                    (pt.h_in, pt.h_out) = ([0.0; 3], [0.0; 3]);
                }
                return;
            }
            // From one end, a click on the other closes the path.
            if let (Some(path), Some(sel)) = (self.selected_path().filter(|s| s.id == pid), self.path.point) {
                let n = path.points.len();
                if !path.closed && n > 2 && ((sel == n - 1 && i == 0) || (sel == 0 && i == n - 1)) {
                    self.path_checkpoint();
                    if let Some(path) = self.selected_path_mut() {
                        path.closed = true;
                    }
                    self.status = "Path closed.".into();
                }
            }
            self.path.path = Some(pid);
            self.path.point = Some(i);
            self.path.drag = Some(PathDrag::Point(i));
            return;
        }
        if let Some((pid, seg)) = self.curve_near(p, uv) {
            let Some((q, n, _)) = self.path_surface(p, uv) else { return };
            self.path.path = Some(pid);
            self.path_checkpoint();
            let Some(path) = self.selected_path_mut() else { return };
            let at = (seg + 1).min(path.points.len());
            path.points.insert(at, Self::new_point(q, n, false));
            self.path.point = Some(at);
            self.path.drag = Some(PathDrag::Point(at));
            return;
        }
        let Some((q, n, dist)) = self.path_surface(p, uv) else {
            self.status = "Path: click on the mesh to place a point.".into();
            return;
        };
        let pen = self.path.pen;
        // An end of the selected (open) path is selected: the path goes on.
        let extend = match (self.selected_path(), self.path.point) {
            (Some(s), Some(i)) if s.uv == uv && !s.closed && !s.points.is_empty() && (i + 1 == s.points.len() || i == 0) => Some(i == 0 && s.points.len() > 1),
            _ => None,
        };
        if let Some(front) = extend {
            self.path_checkpoint();
            let Some(path) = self.selected_path_mut() else { return };
            let at = if front { 0 } else { path.points.len() };
            path.points.insert(at, Self::new_point(q, n, pen));
            self.path.point = Some(at);
            self.path.drag = Some(if pen { PathDrag::Pull(at) } else { PathDrag::Point(at) });
            return;
        }
        // A new path, painted with its layer's brush and material.
        if !self.ensure_path_layer() {
            return;
        }
        let unit = if uv {
            self.uv_mapping().1
        } else {
            let view = self.view_info();
            view.ppp * view.world_per_px(dist)
        };
        // The layer's first path of its kind sets how big a point is.
        if let Some(s) = self.active_style_mut() {
            let u = if uv { &mut s.uv_unit } else { &mut s.unit };
            if *u <= 0.0 {
                *u = unit;
            }
        }
        let id = crate::document::next_rev();
        let count = self.active_paths().map_or(0, |v| v.len());
        let path = PathDesc {
            id,
            name: format!("Path {}", count + 1),
            uv,
            points: vec![Self::new_point(q, n, pen)],
            right: self.camera_right().into(),
            seed: id.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
            ..Default::default()
        };
        if let Some(v) = self.active_paths_mut() {
            v.push(path);
        }
        self.path.path = Some(id);
        self.path.point = Some(0);
        self.path.drag = Some(if pen { PathDrag::Pull(0) } else { PathDrag::Point(0) });
        self.status = "Path started: click to add points, Escape ends it.".into();
    }

    pub(super) fn path_drag_to(&mut self, p: Vec2) {
        let Some(drag) = self.path.drag else { return };
        let Some(path) = self.selected_path() else { return };
        let uv = path.uv;
        match drag {
            PathDrag::Point(i) => {
                let Some((q, n, _)) = self.path_surface(p, uv) else { return };
                if path.points.get(i).is_none_or(|pt| Vec3::from(pt.pos) == q) {
                    return;
                }
                self.path_checkpoint();
                if let Some(pt) = self.selected_path_mut().and_then(|s| s.points.get_mut(i)) {
                    pt.pos = q.into();
                    if !uv {
                        pt.nrm = n.into();
                    }
                }
            }
            PathDrag::Handle(i, _) | PathDrag::Pull(i) if i < path.points.len() => {
                let out = !matches!(drag, PathDrag::Handle(_, false));
                let Some(e) = self.handle_at(path, p, i) else { return };
                let v = e - Vec3::from(path.points[i].pos);
                let eff = effective_handles(path)[i];
                self.path_checkpoint();
                let Some(pt) = self.selected_path_mut().and_then(|s| s.points.get_mut(i)) else { return };
                if matches!(drag, PathDrag::Pull(_)) {
                    pt.kind = PointKind::Smooth;
                    (pt.h_out, pt.h_in) = (v.into(), (-v).into());
                    return;
                }
                if pt.kind == PointKind::Auto {
                    (pt.h_in, pt.h_out, pt.kind) = (eff.0.into(), eff.1.into(), PointKind::Smooth);
                }
                let smooth = pt.kind == PointKind::Smooth;
                let (mine, other) = if out { (&mut pt.h_out, &mut pt.h_in) } else { (&mut pt.h_in, &mut pt.h_out) };
                *mine = v.into();
                if smooth {
                    // Smooth: the other handle stays in line, at its length.
                    let len = Vec3::from(*other).length();
                    let len = if len > 0.0 { len } else { v.length() };
                    *other = (-v.normalize_or_zero() * len).into();
                }
            }
            _ => {}
        }
    }

    pub(super) fn path_release(&mut self) {
        // A pen click without a drag is a corner: a tiny accidental handle
        // would round it.
        if let Some(PathDrag::Pull(i)) = self.path.drag {
            let tiny = self.selected_path().and_then(|path| {
                let pt = path.points.get(i)?;
                let q = Vec3::from(pt.pos);
                let (a, b) = (self.path_local(path.uv, q)?, self.path_local(path.uv, q + Vec3::from(pt.h_out))?);
                Some(a.distance(b) < 3.0)
            });
            if tiny != Some(false) {
                if let Some(pt) = self.selected_path_mut().and_then(|s| s.points.get_mut(i)) {
                    (pt.h_in, pt.h_out, pt.kind) = ([0.0; 3], [0.0; 3], PointKind::Broken);
                }
            }
        }
        self.path.drag = None;
    }

    /// Removes the selected point (the path goes with its last point).
    pub(super) fn path_remove_point(&mut self) -> bool {
        let (Some(path), Some(i)) = (self.selected_path(), self.path.point) else { return false };
        if i >= path.points.len() {
            return false;
        }
        let (last, id) = (path.points.len() == 1, path.id);
        self.path.pushed = false;
        self.path_checkpoint();
        self.path.drag = None;
        if last {
            self.delete_path(id);
            self.status = "Path deleted.".into();
            return true;
        }
        if let Some(path) = self.selected_path_mut() {
            path.points.remove(i);
            if path.points.len() < 3 {
                path.closed = false;
            }
        }
        self.path.point = Some(i.saturating_sub(1));
        true
    }

    pub(super) fn delete_path(&mut self, id: u64) {
        self.path_checkpoint();
        if let Some(v) = self.active_paths_mut() {
            v.retain(|p| p.id != id);
        }
        if self.path.path == Some(id) {
            self.path.path = None;
            self.path.point = None;
        }
    }

    /// Selects a path (its last point, so a click continues it).
    pub(super) fn select_path(&mut self, id: Option<u64>) {
        self.path.path = id;
        self.path.point = id.and_then(|id| self.active_paths()?.iter().find(|p| p.id == id)).map(|p| p.points.len().saturating_sub(1));
        self.path.drag = None;
    }

    /// A one-shot change of the selected path from the panels (its own undo
    /// step).
    pub(super) fn edit_path(&mut self, id: u64, f: impl FnOnce(&mut PathDesc)) {
        self.path.pushed = false;
        self.path_checkpoint();
        if let Some(p) = self.active_paths_mut().and_then(|v| v.iter_mut().find(|p| p.id == id)) {
            f(p);
        }
    }

    /// Escape: ends the selected path (the next click starts a new one);
    /// false when none was selected.
    pub(super) fn path_deselect(&mut self) -> bool {
        if self.selected_path().is_none() {
            return false;
        }
        self.select_path(None);
        self.status = "Path ended: a click starts a new path.".into();
        true
    }

    /// A new document: nothing selected, the tool's own brush back.
    pub(super) fn path_reset(&mut self) {
        if let Some((b, m)) = self.path.saved.take() {
            self.brush = b;
            self.mat = m;
        }
        self.path = PathEdit { pen: self.path.pen, ..Default::default() };
    }

    /// Every frame: in the Path tool on a path layer, the brush properties
    /// show the layer's brush and material and edits to them go to the
    /// layer (all its paths repaint); elsewhere the tool has its own brush
    /// back.
    pub(super) fn sync_path_brush(&mut self) {
        let restore = |app: &mut Self| {
            if let Some((b, m)) = app.path.saved.take() {
                app.brush = b;
                app.mat = m;
            }
            app.path.bound = None;
        };
        if self.tool != Tool::Path {
            restore(self);
            self.path.drag = None;
            return;
        }
        if self.path.path.is_some() && self.selected_path().is_none() {
            self.path.path = None;
            self.path.point = None;
            self.path.drag = None;
        }
        let Some((set, layer)) = self.doc.as_ref().and_then(|d| Some((d.active, d.sets[d.active].selected?))) else {
            restore(self);
            return;
        };
        let key = |s: &PathStyle| serde_json::to_string(&(&s.brush, &s.material, &s.fill)).unwrap_or_default();
        let Some(skey) = self.active_style().map(key) else {
            restore(self);
            return;
        };
        let load = |app: &mut Self| {
            let Some(s) = app.active_style() else { return };
            let (b, mut m) = (s.brush.clone(), s.material.clone());
            m.material = s.fill.clone();
            if app.path.saved.is_none() {
                app.path.saved = Some((app.brush.clone(), app.mat.clone()));
            }
            app.brush = b;
            app.mat = m;
        };
        match &self.path.bound {
            Some((s, l, synced)) if *s == set && *l == layer => {
                if skey != *synced {
                    // The layer changed on its own (undo): the properties follow.
                    load(self);
                } else {
                    let brush = BrushSettings { lazy: 0.0, ..self.brush.clone() };
                    let mine = serde_json::to_string(&(&brush, &self.mat, &self.mat.material)).unwrap_or_default();
                    if mine != skey {
                        let (mat, fill) = (self.mat.clone(), self.mat.material.clone());
                        if let Some(s) = self.active_style_mut() {
                            s.brush = brush;
                            s.material = mat;
                            s.fill = fill;
                        }
                    }
                }
            }
            _ => load(self),
        }
        let now = self.active_style().map(key).unwrap_or_default();
        self.path.bound = Some((set, layer, now));
    }

    // ------------------------------------------------------------ painting

    /// Every frame: path layers whose paths (or mesh, or resolution)
    /// changed are painted again.
    pub(super) fn sync_path_layers(&mut self) {
        let Some(doc) = &mut self.doc else { return };
        for s in doc.sets.iter_mut() {
            crate::document::visit_mut(&mut s.layers, &mut |l| l.desc.migrate_path_style());
        }
        let Some(doc) = &self.doc else { return };
        let mesh_key = Arc::as_ptr(&doc.mesh) as usize as u64;
        let mut todo = Vec::new();
        let mut seen = HashSet::new();
        for (si, s) in doc.sets.iter().enumerate() {
            let res = s.resolution();
            crate::document::visit(&s.layers, &mut |l| {
                let Some(paths) = &l.desc.paths else { return };
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                serde_json::to_string(&(paths, &l.desc.path_style)).unwrap_or_default().hash(&mut h);
                (res, mesh_key, si).hash(&mut h);
                let h = h.finish();
                seen.insert(l.desc.id);
                if self.path_painted.get(&l.desc.id) != Some(&h) {
                    todo.push((si, l.desc.id, h));
                }
            });
        }
        self.path_painted.retain(|id, _| seen.contains(id));
        for (set, lid, h) in todo {
            self.paint_path_layer(set, lid);
            self.path_painted.insert(lid, h);
        }
    }

    fn path_uniform(&self, p: &PathDesc, style: &PathStyle, has_fill: bool) -> BrushUniform {
        let view = self.view_info();
        let (near, far) = self.camera.near_far();
        let (m, b) = (&style.material, &style.brush);
        BrushUniform {
            view_proj: view.view_proj.to_cols_array_2d(),
            eye: view.eye.extend(1.0).into(),
            viewport: [view.size.x, view.size.y, near, far],
            color: [m.color[0], m.color[1], m.color[2], m.opacity],
            values: [m.roughness, m.metallic, m.height, m.mask_value],
            emissive: [m.emissive[0], m.emissive[1], m.emissive[2], 0.0],
            normal: [m.normal[0], m.normal[1], m.normal[2], 0.0],
            params: [b.hardness, if b.angle_fade { 1.0 } else { 0.0 }, 0.0, if p.uv { MODE_UV } else { MODE_WRAP }],
            shape: [if b.alpha == AlphaSource::Builtin(crate::brush::AlphaKind::Round) { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
            stencil: [0.0; 4],
            stencil2: [1.0, 0.0, 0.0, 0.0],
            right: [p.right[0], p.right[1], p.right[2], 0.0],
            material: [if has_fill { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
        }
    }

    /// The grayscale tip of a brush alpha (RGBA, gray in rgb).
    pub(super) fn alpha_tip(&self, a: &AlphaSource) -> Option<image::RgbaImage> {
        match a {
            AlphaSource::Builtin(k) => Some(crate::brush::alpha_image(*k)),
            AlphaSource::Resource(id) => self.doc.as_ref().and_then(|d| d.resources.get(id)).map(|r| {
                let mut g = (*r.image).clone();
                for p in g.pixels_mut() {
                    let l = (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) * (p[3] as f32 / 255.0);
                    let v = l.round() as u8;
                    *p = image::Rgba([v, v, v, 255]);
                }
                g
            }),
        }
    }

    /// Clears a path layer's paint and paints each of its visible paths
    /// with the layer's brush and material.
    fn paint_path_layer(&mut self, set: usize, lid: u64) {
        let Some(doc) = &mut self.doc else { return };
        let res = doc.sets[set].resolution();
        let Some(layer) = crate::document::find_mut(&mut doc.sets[set].layers, lid) else { return };
        let paths: Vec<PathDesc> = layer.desc.paths.clone().unwrap_or_default().into_iter().filter(|p| p.visible && !p.points.is_empty()).collect();
        let style = layer.desc.path_style.clone().unwrap_or_default();
        let dev = &self.engine.device;
        layer.materialize(dev, res, None);
        let channels = style.material.channels;
        let needs_b = !paths.is_empty() && channels[4..7].iter().any(|c| *c);
        if !paths.is_empty() && channels[5] {
            layer.desc.opacity_painted = true;
        }
        if let Some(pt) = &mut layer.paint {
            if needs_b {
                pt.ensure_b(dev);
            }
            let mut enc = dev.create_command_encoder(&Default::default());
            for t in pt.all() {
                enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("clear path layer"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &t.view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            }
            self.engine.queue.submit(Some(enc.finish()));
        }
        layer.rev = crate::document::next_rev();
        doc.sets[set].dirty = true;
        // Path stamps use their own tip and material: the brush's come back
        // next frame.
        self.uploaded_alpha = None;
        self.engine.paint.selection = None;
        if paths.is_empty() {
            return;
        }
        // One tip and one material for the whole layer.
        if let Some(img) = self.alpha_tip(&style.brush.alpha) {
            self.engine.paint.alpha = crate::engine::paint::image_texture(&self.engine.device, &self.engine.queue, "path alpha", &img, false);
        }
        let has_fill = match &style.fill {
            Some((_, fill)) => {
                let Some(doc) = &self.doc else { return };
                let reuse = self.engine.paint.material.as_ref().is_some_and(|t| t[0].width == res);
                if !reuse {
                    let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
                    let t = |l| Tex::new(&self.engine.device, l, res, res, wgpu::TextureFormat::Rgba16Float, usage);
                    self.engine.paint.material = Some([t("bm col"), t("bm mrh"), t("bm emi"), t("bm nrm")]);
                }
                let mut enc = self.engine.device.create_command_encoder(&Default::default());
                doc.sets[set].evaluate_material(&self.engine, &mut enc, &doc.resources, fill, self.engine.paint.material.as_ref().unwrap());
                self.engine.queue.submit(Some(enc.finish()));
                true
            }
            None => false,
        };
        for p in &paths {
            let Some(doc) = &self.doc else { return };
            let stamps = path_stamps(p, &style, doc);
            if stamps.is_empty() {
                continue;
            }
            let uniform = self.path_uniform(p, &style, has_fill);
            for chunk in stamps.chunks(crate::engine::paint::MAX_STAMPS) {
                let Some(doc) = &self.doc else { return };
                let s = &doc.sets[set];
                let Some(pt) = find(&s.layers, lid).and_then(|l| l.paint.as_ref()) else { return };
                self.engine.paint.upload(&self.engine.queue, uniform, chunk);
                let mut enc = self.engine.device.create_command_encoder(&Default::default());
                for pass in passes_for(false, false, channels) {
                    let targets: Vec<&Tex> = match pass {
                        PaintPass::PaintA | PaintPass::EraseA => vec![&*pt.color, &*pt.vals, &*pt.alph],
                        _ => match (&pt.emis, &pt.nrm, &pt.opac) {
                            (Some(e), Some(n), Some(o)) => vec![&**e, &**n, &**o],
                            _ => continue,
                        },
                    };
                    self.engine.paint.draw(&self.engine.device, &mut enc, &doc.gpu_mesh, s.submesh, None, &targets, self.engine.viewport.depth(), pass, channels);
                }
                // Each chunk's stamps are uploaded before its draws run.
                self.engine.queue.submit(Some(enc.finish()));
            }
        }
        // The brush material is evaluated again before the next stroke.
        self.engine.paint.material = None;
    }

    // ------------------------------------------------------------ drawing

    /// The active layer's paths of this view, the selected one's points
    /// and handles, and where the next point goes. Only with the tool.
    pub(super) fn draw_path(&self, painter: &egui::Painter, rect: Rect, uv: bool, hover: Option<Vec2>) {
        if self.tool != Tool::Path || self.doc.is_none() {
            return;
        }
        let to = |q: Vec2| rect.min + egui::vec2(q.x, q.y);
        let sel = self.path.path;
        for path in self.view_paths(uv) {
            let mine = Some(path.id) == sel;
            let curve: Vec<Pos2> = self.screen_curve(path).into_iter().map(|(q, _)| to(q)).collect();
            if curve.len() >= 2 {
                let c = if mine { crate::style::ACCENT } else { Color32::from_gray(if path.visible { 200 } else { 110 }) };
                painter.add(egui::Shape::line(curve.clone(), egui::Stroke::new(3.0f32, Color32::from_black_alpha(120))));
                if path.visible {
                    painter.add(egui::Shape::line(curve, egui::Stroke::new(if mine { 1.6f32 } else { 1.0 }, c)));
                } else {
                    painter.extend(egui::Shape::dashed_line(&curve, egui::Stroke::new(1.0f32, c), 4.0, 4.0));
                }
            }
            for (i, q) in path.points.iter().enumerate() {
                let Some(s) = self.path_local(uv, Vec3::from(q.pos)) else { continue };
                let big = if mine { 9.0 } else { 6.0 };
                let r = Rect::from_center_size(to(s), egui::vec2(big, big));
                let fill = match (mine, self.path.point == Some(i)) {
                    (true, true) => crate::style::ACCENT,
                    (true, false) => Color32::WHITE,
                    _ => Color32::from_gray(170),
                };
                // Round: smooth from the neighbors; square: handles set by hand.
                let round = if q.kind == PointKind::Auto { big } else { 1.0 };
                painter.rect_filled(r.expand(1.5), round, Color32::from_black_alpha(160));
                painter.rect_filled(r, round, fill);
            }
        }
        let Some(path) = self.selected_path().filter(|p| p.uv == uv) else { return };
        for (i, _, e) in self.shown_handles() {
            let (Some(c), Some(h)) = (self.path_local(uv, Vec3::from(path.points[i].pos)), self.path_local(uv, e)) else { continue };
            painter.line_segment([to(c), to(h)], egui::Stroke::new(1.0f32, Color32::from_white_alpha(190)));
            painter.circle_filled(to(h), 4.5, Color32::from_black_alpha(160));
            painter.circle_filled(to(h), 3.2, Color32::WHITE);
        }
        // Rubber band: where a click adds the next point.
        if let (Some(hv), Some(i)) = (hover, self.path.point) {
            let n = path.points.len();
            let end = !path.closed && (i + 1 == n || (i == 0 && n > 1));
            if end && self.path.drag.is_none() && self.point_near(hv, uv).is_none() && self.handle_near(hv, uv).is_none() && self.curve_near(hv, uv).is_none() {
                if let Some(a) = path.points.get(i).and_then(|q| self.path_local(uv, Vec3::from(q.pos))) {
                    painter.extend(egui::Shape::dashed_line(&[to(a), to(hv)], egui::Stroke::new(1.0f32, Color32::from_white_alpha(150)), 4.0, 4.0));
                }
            }
        }
    }
}
