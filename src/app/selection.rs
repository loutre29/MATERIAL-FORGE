//! Selection tools (Photoshop's): rectangle and ellipse marquee, lasso and
//! polygonal lasso, in the 3D view (through the camera, visible surface
//! only by default) and in the 2D view (UV space). The selection belongs to
//! the texture set (`TextureSet::selection`, `engine::select`) and clips
//! every tool that paints or fills.

use super::*;
use crate::engine::select::{self, SelOp, SelectUniform, SPACE_ALL, SPACE_SCREEN, SPACE_UV};
use crate::engine::Tex;
use eframe::egui::{Color32, Pos2, Stroke};

/// Select > By mesh map: what part of the model a baked map picks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapSel {
    /// Convex edges (curvature above the threshold).
    Edges,
    /// Cavities (curvature below).
    Cavities,
    /// Occluded areas (ambient occlusion below).
    Occlusion,
    /// Thin parts (thickness below).
    Thin,
}

impl MapSel {
    pub const ALL: [MapSel; 4] = [MapSel::Edges, MapSel::Cavities, MapSel::Occlusion, MapSel::Thin];

    pub fn name(self) -> &'static str {
        match self {
            MapSel::Edges => "Edges (curvature)",
            MapSel::Cavities => "Cavities (curvature)",
            MapSel::Occlusion => "Occluded areas (AO)",
            MapSel::Thin => "Thin parts (thickness)",
        }
    }

    pub fn default_threshold(self) -> f32 {
        match self {
            MapSel::Edges => 0.58,
            MapSel::Cavities => 0.42,
            MapSel::Occlusion => 0.6,
            MapSel::Thin => 0.3,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelShape {
    Rect,
    Ellipse,
    Lasso,
    /// Click by click; double click, Enter or a click on the first point closes it.
    PolyLasso,
}

impl SelShape {
    pub fn name(self) -> &'static str {
        match self {
            SelShape::Rect => "Rectangle select",
            SelShape::Ellipse => "Ellipse select",
            SelShape::Lasso => "Lasso",
            SelShape::PolyLasso => "Polygonal lasso",
        }
    }
}

/// A shape being drawn. Points are local to the view, in points.
pub struct SelDrag {
    pub shape: SelShape,
    /// Drawn in the 2D view.
    pub uv: bool,
    pub op: SelOp,
    pub set: usize,
    pub start: Vec2,
    pub cur: Vec2,
    pub path: Vec<Vec2>,
    /// Polygonal lasso: the button went up at least once (later clicks add points).
    pub released: bool,
}

/// The operation a press makes: Shift adds, Ctrl subtracts (Alt moves the
/// camera here), both intersect; otherwise the context bar's mode.
pub fn op_for(mods: Modifiers, default: SelOp) -> SelOp {
    match (mods.shift, mods.command) {
        (true, true) => SelOp::Intersect,
        (true, false) => SelOp::Add,
        (false, true) => SelOp::Subtract,
        _ => default,
    }
}

impl RetroApp {
    pub(super) fn has_selection(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| d.sets[d.active].selection.is_some())
    }

    /// The paint passes of `set` are clipped by its selection (or the
    /// bucket's region while it fills).
    pub(super) fn bind_selection(&mut self, set: usize) {
        self.engine.paint.selection = match &self.sel_override {
            Some(t) => Some(t.texture.create_view(&Default::default())),
            None => self.doc.as_ref().and_then(|d| d.sets.get(set)).and_then(|s| s.selection.as_ref()).map(|t| t.texture.create_view(&Default::default())),
        };
    }

    /// UV seam links of `set` (built once per mesh, submesh and resolution).
    fn seams_of(&mut self, set: usize) -> Option<Arc<crate::flood::Seams>> {
        let doc = self.doc.as_ref()?;
        let s = doc.sets.get(set)?;
        let key = (Arc::as_ptr(&doc.mesh) as usize, s.submesh, s.resolution());
        if let Some((k, v)) = &self.seams {
            if *k == key {
                return Some(v.clone());
            }
        }
        let v = Arc::new(crate::flood::Seams::build(&doc.mesh, s.submesh, &s.raster));
        self.seams = Some((key, v.clone()));
        Some(v)
    }

    /// Texels of `set` similar to the one at `uv` (magic wand / bucket
    /// region): 255 in, 0 out, set sized.
    fn similar_region(&mut self, set: usize, uv: [f32; 2]) -> Option<Vec<u8>> {
        let seams = self.seams_of(set)?;
        let doc = self.doc.as_ref()?;
        let s = doc.sets.get(set)?;
        let res = s.resolution();
        let (dev, q) = (&self.engine.device, &self.engine.queue);
        // Colors in 8-bit sRGB plus alpha: the visible result or the layer.
        let layer_tex = (!self.wand_all_layers)
            .then(|| s.selected_layer().and_then(|l| l.paint.as_ref()).map(|p| p.color.clone()))
            .flatten()
            .filter(|t| t.width == res);
        let premul = layer_tex.is_some();
        let raw = match &layer_tex {
            Some(t) => crate::engine::texture::f16_bytes_to_f32(&t.read_bytes(dev, q)),
            None if !self.wand_all_layers => vec![0.0; (res * res * 4) as usize],
            None => crate::engine::texture::f16_bytes_to_f32(&s.gpu.final_base.read_bytes(dev, q)),
        };
        let colors: Vec<[u8; 4]> = raw
            .chunks_exact(4)
            .map(|p| {
                let a = if premul { p[3].clamp(0.0, 1.0) } else { 1.0 };
                let un = |v: f32| if premul && a > 1e-4 { v / a } else { v };
                [crate::flood::to_srgb8(un(p[0])), crate::flood::to_srgb8(un(p[1])), crate::flood::to_srgb8(un(p[2])), (a * 255.0 + 0.5) as u8]
            })
            .collect();
        let r = res as f32;
        let (x, y) = (((uv[0] * r).floor() as i64).clamp(0, res as i64 - 1), ((uv[1] * r).floor() as i64).clamp(0, res as i64 - 1));
        let mut seed = (y as u32 * res + x as u32) as usize;
        // A click on a gutter texel: its nearest covered one.
        if s.raster.tri[seed] < 0 {
            let mut best = None;
            for dy in -2..=2i64 {
                for dx in -2..=2i64 {
                    let (xx, yy) = (x + dx, y + dy);
                    if xx >= 0 && yy >= 0 && xx < res as i64 && yy < res as i64 {
                        let i = (yy as u32 * res + xx as u32) as usize;
                        if s.raster.tri[i] >= 0 && best.is_none() {
                            best = Some(i);
                        }
                    }
                }
            }
            seed = best?;
        }
        let c0 = colors[seed];
        let tol = self.wand_tolerance.round() as i32;
        let similar = |i: usize| (0..4).all(|k| (colors[i][k] as i32 - c0[k] as i32).abs() <= tol);
        Some(crate::flood::flood(&s.raster, &seams, seed, self.wand_contiguous, &similar))
    }

    /// Magic wand click at `uv` of `set`.
    pub(super) fn wand_click(&mut self, set: usize, uv: [f32; 2], mods: Modifiers) {
        let Some(mask) = self.similar_region(set, uv) else { return };
        let res = self.doc.as_ref().map_or(1, |d| d.sets[set].resolution());
        let shape = select::shape_texture(&self.engine.device, &self.engine.queue, res, res, &mask);
        let u = SelectUniform { params: [SPACE_UV, 0.0, 0.0, 0.0], uvx: [1.0, 1.0, 0.0, 0.0], ..Default::default() };
        let op = op_for(mods, self.sel_op);
        let label = match op {
            SelOp::Replace => "Magic wand",
            SelOp::Add => "Magic wand (add)",
            SelOp::Subtract => "Magic wand (subtract)",
            SelOp::Intersect => "Magic wand (intersect)",
        };
        self.merge_selection(set, op, Some(&shape), u, None, label);
    }

    /// Paint bucket click at `uv` of `set`: the similar region (inside the
    /// selection, when there is one) takes the brush material.
    pub(super) fn bucket_click(&mut self, set: usize, uv: [f32; 2]) {
        let Some(mask) = self.similar_region(set, uv) else { return };
        let Some(doc) = &self.doc else { return };
        let s = &doc.sets[set];
        let res = s.resolution();
        let dev = &self.engine.device;
        let shape = select::shape_texture(dev, &self.engine.queue, res, res, &mask);
        let region = select::new_selection(dev, &self.engine.queue, res);
        let u = SelectUniform { params: [SPACE_UV, 0.0, 0.0, 0.0], uvx: [1.0, 1.0, 0.0, 0.0], ..Default::default() };
        let mut enc = dev.create_command_encoder(&Default::default());
        self.engine.select.apply(dev, &mut enc, &doc.gpu_mesh, s.submesh, None, &region, Some(&shape), self.engine.viewport.depth(), u, SelOp::Replace);
        if let Some(sel) = &s.selection {
            self.engine.select.apply(dev, &mut enc, &doc.gpu_mesh, s.submesh, None, &region, Some(sel), self.engine.viewport.depth(), u, SelOp::Intersect);
        }
        self.engine.queue.submit(Some(enc.finish()));
        self.sel_override = Some(Arc::new(region));
        self.fill_masked(false, "Paint bucket");
        self.sel_override = None;
    }

    /// Press with a selection tool at `p` (view local points).
    pub(super) fn sel_press(&mut self, p: Vec2, uv: bool, mods: Modifiers) {
        let SelShape::PolyLasso = self.sel_shape() else {
            let Some(set) = self.doc.as_ref().map(|d| d.active) else { return };
            self.sel_drag = Some(SelDrag {
                shape: self.sel_shape(),
                uv,
                op: op_for(mods, self.sel_op),
                set,
                start: p,
                cur: p,
                path: vec![p],
                released: false,
            });
            return;
        };
        // Polygonal lasso: each click adds a point; a click on the first
        // point (or a double click) closes the shape.
        match &mut self.sel_drag {
            Some(d) if d.shape == SelShape::PolyLasso && d.uv == uv => {
                let close = d.path.len() >= 3 && (d.path[0] - p).length() < 7.0;
                let double = d.path.last().is_some_and(|l| (*l - p).length() < 3.0);
                if close || double {
                    self.sel_commit();
                } else {
                    d.path.push(p);
                    d.cur = p;
                }
            }
            _ => {
                let Some(set) = self.doc.as_ref().map(|d| d.active) else { return };
                self.sel_drag = Some(SelDrag {
                    shape: SelShape::PolyLasso,
                    uv,
                    op: op_for(mods, self.sel_op),
                    set,
                    start: p,
                    cur: p,
                    path: vec![p],
                    released: false,
                });
            }
        }
    }

    pub(super) fn sel_move(&mut self, p: Vec2, down: bool) {
        let Some(d) = &mut self.sel_drag else { return };
        d.cur = p;
        if d.shape == SelShape::Lasso && down && d.path.last().is_none_or(|l| (*l - p).length() >= 1.5) {
            d.path.push(p);
        }
    }

    /// The button went up: marquees and the lasso are done; the polygonal
    /// lasso keeps going.
    pub(super) fn sel_release(&mut self) {
        let Some(d) = &mut self.sel_drag else { return };
        if d.shape == SelShape::PolyLasso {
            d.released = true;
            return;
        }
        self.sel_commit();
    }

    pub(super) fn sel_cancel(&mut self) -> bool {
        self.sel_drag.take().is_some()
    }

    /// Polygonal lasso: Backspace removes the last point.
    pub(super) fn sel_back(&mut self) {
        if let Some(d) = &mut self.sel_drag {
            if d.path.len() > 1 {
                d.path.pop();
            } else {
                self.sel_drag = None;
            }
        }
    }

    /// The selection tool of the current tool (rectangle by default).
    pub(super) fn sel_shape(&self) -> SelShape {
        match self.tool {
            Tool::Select(s) => s,
            _ => SelShape::Rect,
        }
    }

    /// Merges the shape being drawn into the selection.
    pub(super) fn sel_commit(&mut self) {
        let Some(d) = self.sel_drag.take() else { return };
        let pts: Vec<Vec2> = match d.shape {
            SelShape::Rect | SelShape::Ellipse => {
                if (d.cur - d.start).abs().min_element() < 2.0 {
                    // A click without a drag deselects (like Photoshop).
                    if d.op == SelOp::Replace {
                        self.deselect();
                    }
                    return;
                }
                let (a, b) = ([d.start.x, d.start.y], [d.cur.x, d.cur.y]);
                let p = if d.shape == SelShape::Rect { select::rect_points(a, b) } else { select::ellipse_points(a, b) };
                p.into_iter().map(Vec2::from).collect()
            }
            SelShape::Lasso | SelShape::PolyLasso => {
                if d.path.len() < 3 {
                    if d.op == SelOp::Replace && d.shape == SelShape::Lasso {
                        self.deselect();
                    }
                    return;
                }
                d.path.clone()
            }
        };
        let label = match d.op {
            SelOp::Replace => d.shape.name().to_string(),
            SelOp::Add => format!("{} (add)", d.shape.name()),
            SelOp::Subtract => format!("{} (subtract)", d.shape.name()),
            SelOp::Intersect => format!("{} (intersect)", d.shape.name()),
        };
        if d.uv && self.view.uv_rot.abs() > 1e-6 {
            // A turned 2D view: the shape drawn in UV space, over the UV
            // square at the set's resolution.
            let n = self.doc.as_ref().map(|doc| doc.sets[d.set].resolution()).unwrap_or(1024).clamp(256, 4096);
            let pts: Vec<[f32; 2]> = pts
                .iter()
                .map(|p| {
                    let uv = self.uv_of(*p);
                    [uv.x * n as f32, uv.y * n as f32]
                })
                .collect();
            let mask = select::rasterize_polygon(&pts, n, n);
            let shape = select::shape_texture(&self.engine.device, &self.engine.queue, n, n, &mask);
            let u = SelectUniform { params: [SPACE_UV, 0.0, 0.0, 0.0], uvx: [1.0, 1.0, 0.0, 0.0], ..Default::default() };
            self.merge_selection(d.set, d.op, Some(&shape), u, None, &label);
        } else if d.uv {
            let ppp = self.engine.uvview.width as f32 / self.uv_rect.width().max(1.0);
            let (w, h) = (self.engine.uvview.width, self.engine.uvview.height);
            let pts: Vec<[f32; 2]> = pts.iter().map(|p| [p.x * ppp, p.y * ppp]).collect();
            let mask = select::rasterize_polygon(&pts, w, h);
            let shape = select::shape_texture(&self.engine.device, &self.engine.queue, w, h, &mask);
            let (z, pan) = (self.view.uv_zoom, self.view.uv_pan);
            let u = SelectUniform {
                params: [SPACE_UV, 0.0, 0.0, 0.0],
                uvx: [z / w as f32, z / h as f32, pan[0] / w as f32, pan[1] / h as f32],
                ..Default::default()
            };
            self.merge_selection(d.set, d.op, Some(&shape), u, None, &label);
        } else {
            let view = self.view_info();
            let (w, h) = (self.engine.viewport.width, self.engine.viewport.height);
            let pts: Vec<[f32; 2]> = pts.iter().map(|p| [p.x * view.ppp, p.y * view.ppp]).collect();
            let mask = select::rasterize_polygon(&pts, w, h);
            let shape = select::shape_texture(&self.engine.device, &self.engine.queue, w, h, &mask);
            let (near, far) = self.camera.near_far();
            let u = SelectUniform {
                view_proj: view.view_proj.to_cols_array_2d(),
                viewport: [view.size.x, view.size.y, near, far],
                params: [SPACE_SCREEN, if self.sel_through { 0.0 } else { 1.0 }, if self.camera.ortho { 1.0 } else { 0.0 }, 0.0],
                uvx: [0.0; 4],
                eye: view.eye.extend(1.0).into(),
            };
            self.merge_selection(d.set, d.op, Some(&shape), u, None, &label);
        }
    }

    /// Applies `op` with `shape` to the selection of `set` (a new texture,
    /// one undo step).
    fn merge_selection(&mut self, set: usize, op: SelOp, shape: Option<&Tex>, u: SelectUniform, subset: Option<(&wgpu::Buffer, u32)>, label: &str) {
        let Some(doc) = &self.doc else { return };
        let Some(s) = doc.sets.get(set) else { return };
        let cur = s.selection.clone();
        if cur.is_none() && matches!(op, SelOp::Subtract | SelOp::Intersect) {
            self.status = "Nothing is selected.".into();
            return;
        }
        let dev = &self.engine.device;
        let mut enc = dev.create_command_encoder(&Default::default());
        let (target, op) = match (&cur, op) {
            (Some(c), op) if op != SelOp::Replace => (c.duplicate(dev, &mut enc, "selection"), op),
            _ => (select::new_selection(dev, &self.engine.queue, s.resolution()), SelOp::Replace),
        };
        self.engine.select.apply(dev, &mut enc, &doc.gpu_mesh, s.submesh, subset, &target, shape, self.engine.viewport.depth(), u, op);
        self.engine.queue.submit(Some(enc.finish()));
        self.set_selection(set, Some(Arc::new(target)), label);
    }

    /// Replaces the selection of `set` (one undo step).
    pub(super) fn set_selection(&mut self, set: usize, sel: Option<Arc<Tex>>, label: &str) {
        let Some(doc) = &mut self.doc else { return };
        let Some(s) = doc.sets.get_mut(set) else { return };
        let old = std::mem::replace(&mut s.selection, sel);
        if old.is_none() && s.selection.is_none() {
            return;
        }
        if let Some(o) = &old {
            self.last_selection = Some((set, o.clone()));
        }
        self.history.push(Entry::Selection { label: label.to_string(), set, selection: old });
        self.status = label.to_string();
    }

    pub(super) fn select_all(&mut self) {
        let Some(set) = self.doc.as_ref().map(|d| d.active) else { return };
        let u = SelectUniform { params: [SPACE_ALL, 0.0, 0.0, 0.0], ..Default::default() };
        self.merge_selection(set, SelOp::Replace, None, u, None, "Select all");
    }

    pub(super) fn deselect(&mut self) {
        let Some(set) = self.doc.as_ref().map(|d| d.active) else { return };
        self.set_selection(set, None, "Deselect");
    }

    /// The selection that was removed last (Ctrl + Shift + D).
    pub(super) fn reselect(&mut self) {
        let Some((set, sel)) = self.last_selection.clone() else { return };
        if self.doc.as_ref().is_some_and(|d| d.sets.get(set).is_some_and(|s| s.resolution() == sel.width)) {
            self.set_selection(set, Some(sel), "Reselect");
        }
    }

    pub(super) fn invert_selection(&mut self) {
        let Some(doc) = &self.doc else { return };
        let set = doc.active;
        let Some(cur) = doc.sets[set].selection.clone() else {
            self.status = "Nothing is selected.".into();
            return;
        };
        let s = &doc.sets[set];
        let dev = &self.engine.device;
        let mut enc = dev.create_command_encoder(&Default::default());
        let target = select::new_selection(dev, &self.engine.queue, s.resolution());
        let all = SelectUniform { params: [SPACE_ALL, 0.0, 0.0, 0.0], ..Default::default() };
        self.engine.select.apply(dev, &mut enc, &doc.gpu_mesh, s.submesh, None, &target, None, self.engine.viewport.depth(), all, SelOp::Replace);
        let same = SelectUniform { params: [SPACE_UV, 0.0, 0.0, 0.0], uvx: [1.0, 1.0, 0.0, 0.0], ..Default::default() };
        self.engine.select.apply(dev, &mut enc, &doc.gpu_mesh, s.submesh, None, &target, Some(&cur), self.engine.viewport.depth(), same, SelOp::Subtract);
        self.engine.queue.submit(Some(enc.finish()));
        self.set_selection(set, Some(Arc::new(target)), "Invert selection");
    }

    /// Fills the selection of the active set with the brush material on the
    /// selected paint layer or mask (`erase`: clears it), one undo step.
    pub(super) fn fill_selection(&mut self, erase: bool) {
        if !self.has_selection() {
            self.toast("Select an area first (marquee, lasso or magic wand).".into());
            return;
        }
        self.fill_masked(erase, if erase { "Clear selection" } else { "Fill selection" });
    }

    /// Fills (or clears) the selection, or `sel_override`, on the selected
    /// paint layer or mask.
    fn fill_masked(&mut self, erase: bool, label: &str) {
        let (lid, target) = match self.resolve_paint_target() {
            Ok(v) => v,
            Err(e) => {
                self.toast(e);
                return;
            }
        };
        let channels = if erase { [true; 7] } else { self.mat.channels };
        let mask = matches!(target, PaintTarget::MaskPaint(_));
        let doc = self.doc.as_mut().unwrap();
        let set = doc.active;
        let sub = doc.sets[set].submesh;
        let res = doc.sets[set].resolution();
        let Some(layer) = crate::document::find_mut(&mut doc.sets[set].layers, lid) else { return };
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
        let entry = Entry::stroke(&self.engine, label, set, textures);
        if !mask && !erase {
            self.prepare_brush_material();
        }
        let hidden = self.doc.as_ref().and_then(|d| d.hidden.get(sub)).cloned().unwrap_or_default();
        let n = self.doc.as_ref().map_or(0, |d| d.mesh.submeshes[sub].indices.len() / 3) as u32;
        let tris: Vec<u32> = (0..n).filter(|t| !hidden.get(*t as usize).copied().unwrap_or(false)).collect();
        self.fill_tris(set, sub, lid, target, erase, &tris);
        self.history.push(entry);
        self.status = label.into();
    }

    /// Selects the UV rectangle `a`..`b` of the active set (tests, crawl).
    pub(super) fn select_uv_rect(&mut self, a: Vec2, b: Vec2) {
        let Some(doc) = &self.doc else { return };
        let set = doc.active;
        let res = doc.sets[set].resolution();
        let r = res as f32;
        let m = select::rasterize_polygon(&select::rect_points([a.x * r, a.y * r], [b.x * r, b.y * r]), res, res);
        let shape = select::shape_texture(&self.engine.device, &self.engine.queue, res, res, &m);
        let u = SelectUniform { params: [SPACE_UV, 0.0, 0.0, 0.0], uvx: [1.0, 1.0, 0.0, 0.0], ..Default::default() };
        self.merge_selection(set, SelOp::Replace, Some(&shape), u, None, "Select UV rectangle");
    }

    /// A badge in the corner of the views while something is selected (a
    /// whole texture set selected has no visible edge).
    pub(super) fn draw_selection_badge(&self, painter: &egui::Painter, rect: Rect) {
        if !self.has_selection() {
            return;
        }
        let font = egui::FontId::proportional(11.5);
        let g = painter.layout_no_wrap("Selection active  (Ctrl + D deselects)".into(), font, Color32::from_gray(225));
        let r = Rect::from_min_size(Pos2::new(rect.min.x + 10.0, rect.max.y - 32.0), g.size() + egui::vec2(18.0, 10.0));
        painter.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(24, 22, 20, 210));
        let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0f32, crate::style::ACCENT), 4.0, 3.0));
        painter.galley(r.min + egui::vec2(9.0, 5.0), g, Color32::WHITE);
    }

    /// Gradient release: the brush material from the start (full) to the
    /// end (transparent) of the dragged line, clipped by the selection.
    pub(super) fn gradient_commit(&mut self) {
        let Some((uv, a, b)) = self.grad_drag.take() else { return };
        if (b - a).length() < 3.0 {
            return;
        }
        let (lid, target) = match self.resolve_paint_target() {
            Ok(v) => v,
            Err(e) => {
                self.toast(e);
                return;
            }
        };
        let (pa, pb, mode) = if uv {
            let (ua, ub) = (self.uv_of(a), self.uv_of(b));
            ([ua.x, ua.y], [ub.x, ub.y], crate::engine::paint::MODE_GRADIENT_UV)
        } else {
            let ppp = self.view_info().ppp;
            ([a.x * ppp, a.y * ppp], [b.x * ppp, b.y * ppp], crate::engine::paint::MODE_GRADIENT)
        };
        let channels = self.mat.channels;
        let mask = matches!(target, PaintTarget::MaskPaint(_));
        let doc = self.doc.as_mut().unwrap();
        let set = doc.active;
        let sub = doc.sets[set].submesh;
        let res = doc.sets[set].resolution();
        let Some(layer) = crate::document::find_mut(&mut doc.sets[set].layers, lid) else { return };
        if !mask && channels[5] {
            layer.desc.opacity_painted = true;
        }
        layer.materialize(&self.engine.device, res, match target {
            PaintTarget::MaskPaint(eid) => Some(eid),
            PaintTarget::Content => None,
        });
        if !mask && (channels[4] || channels[5] || channels[6]) {
            if let Some(p) = &mut layer.paint {
                p.ensure_b(&self.engine.device);
            }
        }
        let textures: Vec<Arc<Tex>> = match target {
            PaintTarget::Content => layer.paint.as_ref().map(|p| p.all().into_iter().cloned().collect()).unwrap_or_default(),
            PaintTarget::MaskPaint(eid) => layer.mask_paint.get(&eid).cloned().into_iter().collect(),
        };
        let entry = Entry::stroke(&self.engine, "Gradient", set, textures);
        if !mask {
            self.prepare_brush_material();
        }
        let hidden = self.doc.as_ref().and_then(|d| d.hidden.get(sub)).cloned().unwrap_or_default();
        let n = self.doc.as_ref().map_or(0, |d| d.mesh.submeshes[sub].indices.len() / 3) as u32;
        let tris: Vec<u32> = (0..n).filter(|t| !hidden.get(*t as usize).copied().unwrap_or(false)).collect();
        // stamps[0]: the line; then the stops in order (position, color,
        // opacity). Masks take the stops' gray values.
        let stops = super::gradient::sorted(if mask { &self.grad_mask } else { &self.grad_color });
        let colors = mask || self.grad_stop_colors;
        let mut stamps = vec![crate::engine::paint::Stamp {
            pos: [pa[0], pa[1], pb[0], pb[1]],
            nrm: [0.0, 0.0, 0.0, self.brush.flow],
            ext: [self.gradient_kind as f32, stops.len() as f32, self.gradient_through as u8 as f32, colors as u8 as f32],
        }];
        stamps.extend(stops.iter().map(|s| crate::engine::paint::Stamp { pos: [s.pos, s.color[0], s.color[1], s.color[2]], nrm: [s.alpha, 0.0, 0.0, 0.0], ext: [0.0; 4] }));
        self.fill_tris_with(set, sub, lid, target, false, &tris, mode, &stamps);
        self.history.push(entry);
        self.status = "Gradient".into();
    }

    /// The gradient line being dragged.
    pub(super) fn draw_grad_drag(&self, painter: &egui::Painter, rect: Rect, uv: bool) {
        let Some((u, a, b)) = self.grad_drag else { return };
        if u != uv {
            return;
        }
        let (a, b) = (Pos2::new(rect.min.x + a.x, rect.min.y + a.y), Pos2::new(rect.min.x + b.x, rect.min.y + b.y));
        painter.line_segment([a, b], Stroke::new(3.0f32, Color32::from_black_alpha(140)));
        painter.line_segment([a, b], Stroke::new(1.2f32, Color32::WHITE));
        painter.circle_filled(a, 4.0, Color32::WHITE);
        painter.circle_stroke(b, 4.0, Stroke::new(1.2f32, Color32::WHITE));
        if self.gradient_kind == 1 {
            painter.circle_stroke(a, (b - a).length(), Stroke::new(1.0f32, Color32::from_white_alpha(90)));
        }
    }

    /// Grow / shrink / feather / border by `radius` texels, across UV seams.
    pub(super) fn modify_selection(&mut self, op: crate::flood::Modify, radius: f32) {
        let Some(doc) = &self.doc else { return };
        let set = doc.active;
        let Some(cur) = doc.sets[set].selection.clone() else {
            self.status = "Nothing is selected.".into();
            return;
        };
        let Some(seams) = self.seams_of(set) else { return };
        let doc = self.doc.as_ref().unwrap();
        let s = &doc.sets[set];
        let res = s.resolution();
        let bytes = cur.read_bytes(&self.engine.device, &self.engine.queue);
        let out = crate::flood::modify(&s.raster, &seams, &bytes, op, radius);
        let t = Tex::new(&self.engine.device, "selection", res, res, select::SEL_FORMAT, select::SEL_USAGE);
        t.write_bytes(&self.engine.queue, &out);
        self.set_selection(set, Some(Arc::new(t)), &format!("{} selection ({} px)", op.name(), radius.round()));
    }

    /// Select > Modify: the radius window.
    pub(super) fn modify_window(&mut self, ctx: &egui::Context) {
        let Some((op, mut radius)) = self.sel_modify else { return };
        let mut open = true;
        let mut apply = false;
        egui::Window::new(egui::RichText::new(format!("{} SELECTION", op.name().to_uppercase())).font(crate::style::bold(11.5)))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_min_width(280.0);
                super::widgets::Slider::new("Radius (texels)", &mut radius, 1.0..=128.0).log().decimals(0).show(ui);
                ui.label(egui::RichText::new("Measured on the surface: it continues across UV seams.").size(11.5).color(super::widgets::DIM));
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("OK").strong()).fill(crate::style::ACCENT).min_size(egui::vec2(80.0, 26.0))).clicked() {
                        apply = true;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(80.0, 26.0))).clicked() {
                        self.sel_modify = None;
                    }
                });
            });
        if self.sel_modify.is_some() {
            self.sel_modify = Some((op, radius));
        }
        if !open {
            self.sel_modify = None;
        }
        if apply {
            self.sel_modify = None;
            self.modify_selection(op, radius);
        }
    }

    /// Triangles `tris` of submesh `sub` merged into the selection of `set`
    /// (polygon fill in Select mode).
    pub(super) fn select_tris(&mut self, set: usize, sub: usize, tris: &[u32], op: SelOp) {
        let Some(doc) = &self.doc else { return };
        let idx = &doc.mesh.submeshes[sub].indices;
        let subset: Vec<u32> = tris.iter().flat_map(|&t| idx[t as usize * 3..t as usize * 3 + 3].iter().copied()).collect();
        if subset.is_empty() {
            return;
        }
        use wgpu::util::DeviceExt;
        let buf = self.engine.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("select faces"),
            contents: bytemuck::cast_slice(&subset),
            usage: wgpu::BufferUsages::INDEX,
        });
        let u = SelectUniform { params: [SPACE_ALL, 0.0, 0.0, 0.0], ..Default::default() };
        let label = match op {
            SelOp::Replace => "Select faces",
            SelOp::Add => "Select faces (add)",
            SelOp::Subtract => "Select faces (subtract)",
            SelOp::Intersect => "Select faces (intersect)",
        };
        // Faces are a full shape: subtract / intersect use the faces as the
        // covered area (the select pass writes 1 only on them).
        match op {
            SelOp::Subtract | SelOp::Intersect => {
                let res = doc.sets[set].resolution();
                let dev = &self.engine.device;
                let faces = select::new_selection(dev, &self.engine.queue, res);
                let mut enc = dev.create_command_encoder(&Default::default());
                self.engine.select.apply(dev, &mut enc, &doc.gpu_mesh, sub, Some((&buf, subset.len() as u32)), &faces, None, self.engine.viewport.depth(), u, SelOp::Replace);
                self.engine.queue.submit(Some(enc.finish()));
                let uvu = SelectUniform { params: [SPACE_UV, 0.0, 0.0, 0.0], uvx: [1.0, 1.0, 0.0, 0.0], ..Default::default() };
                self.merge_selection(set, op, Some(&faces), uvu, None, label);
            }
            _ => self.merge_selection(set, op, None, u, Some((&buf, subset.len() as u32)), label),
        }
    }

    /// The selection becomes the selected layer's mask (a paint effect
    /// holding it, over a black mask when the layer had none).
    pub(super) fn mask_from_selection(&mut self) {
        let Some(doc) = &self.doc else { return };
        let set = doc.active;
        let Some(sel) = doc.sets[set].selection.clone() else {
            self.toast("Select an area first.".into());
            return;
        };
        if doc.sets[set].selected_layer().is_none() {
            self.toast("Select a layer first.".into());
            return;
        }
        self.checkpoint("Mask from selection");
        let bytes = sel.read_bytes(&self.engine.device, &self.engine.queue);
        let doc = self.doc.as_mut().unwrap();
        let res = doc.sets[set].resolution();
        if doc.sets[set].selected_layer().is_some_and(|l| l.desc.mask.is_none()) {
            doc.add_mask(&self.engine, set, false);
        }
        let Some(eid) = doc.add_mask_effect(&self.engine, set, crate::document::MaskEffectKind::Paint) else { return };
        let Some(layer) = doc.sets[set].selected_layer_mut() else { return };
        layer.materialize(&self.engine.device, res, Some(eid));
        if let Some(t) = layer.mask_paint.get(&eid) {
            // (value * coverage, coverage): full coverage, the selection as value.
            let v: Vec<f32> = bytes.iter().flat_map(|b| [*b as f32 / 255.0, 1.0]).collect();
            t.write_bytes(&self.engine.queue, &crate::engine::texture::f32_to_f16_bytes(&v));
        }
        layer.rev = crate::document::next_rev();
        doc.sets[set].dirty = true;
        doc.modified = true;
        self.status = "Mask from selection".into();
    }

    /// A new fill layer masked by the selection (Photoshop's "layer via
    /// selection" for materials).
    pub(super) fn fill_layer_from_selection(&mut self) {
        if !self.has_selection() {
            self.toast("Select an area first.".into());
            return;
        }
        self.run_cmd(super::panels::Cmd::AddLayer(crate::document::LayerKind::Fill(Box::default())));
        self.mask_from_selection();
    }

    /// Selects where a baked mesh map passes `threshold` (soft over 0.04).
    pub(super) fn select_by_map(&mut self, kind: MapSel, threshold: f32) {
        let Some(doc) = &self.doc else { return };
        let set = doc.active;
        let s = &doc.sets[set];
        if !s.baked {
            self.toast("Bake the mesh maps first (File > Bake mesh maps).".into());
            return;
        }
        let res = s.resolution();
        let tex = match kind {
            MapSel::Edges | MapSel::Cavities => &s.gpu.baked_curvature,
            MapSel::Occlusion => &s.gpu.baked_ao,
            MapSel::Thin => &s.gpu.baked_thickness,
        };
        if tex.width != res {
            self.toast("This texture set has no baked maps yet.".into());
            return;
        }
        let v = crate::engine::texture::f16_bytes_to_f32(&tex.read_bytes(&self.engine.device, &self.engine.queue));
        let above = kind == MapSel::Edges;
        let mask: Vec<u8> = v
            .iter()
            .enumerate()
            .map(|(i, x)| {
                if s.raster.tri[i] < 0 {
                    return 0;
                }
                let d = if above { x - threshold } else { threshold - x };
                ((d / 0.04 + 0.5).clamp(0.0, 1.0) * 255.0 + 0.5) as u8
            })
            .collect();
        let shape = select::shape_texture(&self.engine.device, &self.engine.queue, res, res, &mask);
        let u = SelectUniform { params: [SPACE_UV, 0.0, 0.0, 0.0], uvx: [1.0, 1.0, 0.0, 0.0], ..Default::default() };
        let op = self.sel_op;
        self.merge_selection(set, op, Some(&shape), u, None, &format!("Select {}", kind.name().to_lowercase()));
    }

    /// Select > By mesh map: the map and threshold window.
    pub(super) fn by_map_window(&mut self, ctx: &egui::Context) {
        let Some((mut kind, mut t)) = self.sel_by_map else { return };
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(egui::RichText::new("SELECT BY MESH MAP").font(crate::style::bold(11.5)))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_min_width(300.0);
                let before = kind;
                super::widgets::combo_row(ui, "Select", "by map kind", &mut kind, &MapSel::ALL, MapSel::name);
                if kind != before {
                    t = kind.default_threshold();
                }
                super::widgets::Slider::new("Threshold", &mut t, 0.0..=1.0).show(ui);
                ui.label(
                    egui::RichText::new("Uses the baked maps of the active texture set. Shift / Ctrl modes of the context bar apply.")
                        .size(11.5)
                        .color(super::widgets::DIM),
                );
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("Select").strong()).fill(crate::style::ACCENT).min_size(egui::vec2(80.0, 26.0))).clicked() {
                        apply = true;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(80.0, 26.0))).clicked() {
                        cancel = true;
                    }
                });
            });
        self.sel_by_map = if !open || cancel || apply { None } else { Some((kind, t)) };
        if apply {
            self.select_by_map(kind, t);
        }
    }

    /// Outline of the shape being drawn (egui overlay over the view).
    pub(super) fn draw_sel_drag(&self, painter: &egui::Painter, rect: Rect, uv: bool) {
        let Some(d) = &self.sel_drag else { return };
        if d.uv != uv {
            return;
        }
        let at = |p: Vec2| Pos2::new(rect.min.x + p.x, rect.min.y + p.y);
        let pts: Vec<Pos2> = match d.shape {
            SelShape::Rect => select::rect_points([d.start.x, d.start.y], [d.cur.x, d.cur.y]).into_iter().map(|p| at(Vec2::from(p))).collect(),
            SelShape::Ellipse => select::ellipse_points([d.start.x, d.start.y], [d.cur.x, d.cur.y]).into_iter().map(|p| at(Vec2::from(p))).collect(),
            SelShape::Lasso => d.path.iter().map(|p| at(*p)).collect(),
            SelShape::PolyLasso => d.path.iter().chain(std::iter::once(&d.cur)).map(|p| at(*p)).collect(),
        };
        if pts.len() < 2 {
            return;
        }
        let closed = matches!(d.shape, SelShape::Rect | SelShape::Ellipse);
        let mut line = pts.clone();
        if closed {
            line.push(pts[0]);
        }
        // Black and white dashes: readable on any surface.
        painter.add(egui::Shape::line(line.clone(), Stroke::new(1.0f32, Color32::BLACK)));
        painter.add(egui::Shape::dashed_line(&line, Stroke::new(1.0f32, Color32::WHITE), 4.0, 4.0));
        if d.shape == SelShape::PolyLasso {
            let first = at(d.path[0]);
            let near = d.path.len() >= 3 && (d.path[0] - d.cur).length() < 7.0;
            painter.circle_stroke(first, if near { 5.0 } else { 3.0 }, Stroke::new(1.0f32, if near { crate::style::ACCENT } else { Color32::WHITE }));
        }
    }
}
