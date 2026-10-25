//! The gradient tool's stops and their editor, like an image editor's: the
//! ramp over a checkerboard with a marker per stop under it. Click the ramp
//! or under it to add a stop, drag a marker to move it, drag it down out of
//! the editor to remove it. Content gradients have colored stops, mask
//! gradients grayscale ones (the value the mask takes).

use super::widgets::{self, lin_to_c32, LABEL};
use crate::style;
use eframe::egui::{self, Color32, Id, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradStop {
    /// Along the line, 0 (start) to 1 (end).
    pub pos: f32,
    /// Linear color (masks: the value in every component).
    pub color: [f32; 3],
    pub alpha: f32,
}

impl GradStop {
    pub fn new(pos: f32, color: [f32; 3], alpha: f32) -> Self {
        GradStop { pos, color, alpha }
    }
}

/// The stops sorted along the line.
pub fn sorted(stops: &[GradStop]) -> Vec<GradStop> {
    let mut v = stops.to_vec();
    v.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    v
}

/// Color and opacity at `t` (the end stops extend past them).
pub fn ramp_at(stops: &[GradStop], t: f32) -> ([f32; 3], f32) {
    let s = sorted(stops);
    let (Some(first), Some(last)) = (s.first(), s.last()) else { return ([1.0; 3], 1.0) };
    if t <= first.pos {
        return (first.color, first.alpha);
    }
    for w in s.windows(2) {
        if t <= w[1].pos {
            let f = ((t - w[0].pos) / (w[1].pos - w[0].pos).max(1e-6)).clamp(0.0, 1.0);
            let c = std::array::from_fn(|k| w[0].color[k] + (w[1].color[k] - w[0].color[k]) * f);
            return (c, w[0].alpha + (w[1].alpha - w[0].alpha) * f);
        }
    }
    (last.color, last.alpha)
}

/// The default content gradient: `color` fading out.
pub fn default_color(color: [f32; 3]) -> Vec<GradStop> {
    vec![GradStop::new(0.0, color, 1.0), GradStop::new(1.0, color, 0.0)]
}

/// The default mask gradient: white to black.
pub fn default_mask() -> Vec<GradStop> {
    vec![GradStop::new(0.0, [1.0; 3], 1.0), GradStop::new(1.0, [0.0; 3], 1.0)]
}

/// Ready-made gradients (name, stops); `brush`: the brush color.
pub fn presets(gray: bool, brush: [f32; 3]) -> Vec<(&'static str, Vec<GradStop>)> {
    let (w, b) = ([1.0; 3], [0.0; 3]);
    if gray {
        return vec![
            ("White to black", default_mask()),
            ("Black to white", vec![GradStop::new(0.0, b, 1.0), GradStop::new(1.0, w, 1.0)]),
            ("White to transparent", vec![GradStop::new(0.0, w, 1.0), GradStop::new(1.0, w, 0.0)]),
            ("Black to transparent", vec![GradStop::new(0.0, b, 1.0), GradStop::new(1.0, b, 0.0)]),
            ("Black, white, black", vec![GradStop::new(0.0, b, 1.0), GradStop::new(0.5, w, 1.0), GradStop::new(1.0, b, 1.0)]),
        ];
    }
    // Linear values of a few sRGB hues.
    let lin = |c: [f32; 3]| c.map(|v| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) });
    let spectrum = [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 0.0, 0.0]];
    vec![
        ("Brush color to transparent", default_color(brush)),
        ("Brush color to black", vec![GradStop::new(0.0, brush, 1.0), GradStop::new(1.0, b, 1.0)]),
        ("Black to white", vec![GradStop::new(0.0, b, 1.0), GradStop::new(1.0, w, 1.0)]),
        ("White to black", vec![GradStop::new(0.0, w, 1.0), GradStop::new(1.0, b, 1.0)]),
        ("Spectrum", spectrum.iter().enumerate().map(|(i, c)| GradStop::new(i as f32 / 6.0, lin(*c), 1.0)).collect()),
        ("Copper", vec![GradStop::new(0.0, lin([0.18, 0.06, 0.03]), 1.0), GradStop::new(0.5, lin([0.85, 0.45, 0.25]), 1.0), GradStop::new(1.0, lin([1.0, 0.86, 0.7]), 1.0)]),
    ]
}

fn shown(s: &GradStop, gray: bool, alpha: bool) -> Color32 {
    let a = if alpha { (s.alpha.clamp(0.0, 1.0) * 255.0).round() as u8 } else { 255 };
    let c = if gray {
        let v = (s.color[0].clamp(0.0, 1.0) * 255.0).round() as u8;
        Color32::from_gray(v)
    } else {
        lin_to_c32(s.color)
    };
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn checker(p: &egui::Painter, r: Rect) {
    p.rect_filled(r, 0.0, Color32::from_gray(150));
    let k = 6.0;
    let (nx, ny) = ((r.width() / k).ceil() as i32, (r.height() / k).ceil() as i32);
    for y in 0..ny {
        for x in 0..nx {
            if (x + y) % 2 == 0 {
                let a = r.min + Vec2::new(x as f32 * k, y as f32 * k);
                p.rect_filled(Rect::from_min_max(a, (a + Vec2::splat(k)).min(r.max)), 0.0, Color32::from_gray(100));
            }
        }
    }
}

/// The ramp in `r` (over a checkerboard).
pub fn paint_ramp(p: &egui::Painter, r: Rect, stops: &[GradStop], gray: bool) {
    checker(p, r);
    let n = 96;
    let mut mesh = egui::Mesh::default();
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let (c, a) = ramp_at(stops, t);
        let col = shown(&GradStop::new(t, c, a), gray, true);
        let x = r.left() + r.width() * t;
        mesh.colored_vertex(Pos2::new(x, r.top()), col);
        mesh.colored_vertex(Pos2::new(x, r.bottom()), col);
        if i > 0 {
            let b = (i as u32) * 2;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b, b + 1);
        }
    }
    p.add(egui::Shape::mesh(mesh));
    p.rect_stroke(r, 1.0, Stroke::new(1.0f32, Color32::from_gray(60)));
}

/// A small ramp swatch (the context toolbar); hover tells where to edit it.
pub fn preview(ui: &mut Ui, stops: &[GradStop], gray: bool, size: Vec2) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    paint_ramp(ui.painter(), r.shrink2(Vec2::new(0.0, 2.0)), stops, gray);
    resp.on_hover_text("The gradient: edit its stops in the Properties panel")
}

/// The editor: ramp and stop markers. `sel` is the selected stop. True when
/// a stop was added, moved or removed.
pub fn editor(ui: &mut Ui, id: impl std::hash::Hash, stops: &mut Vec<GradStop>, sel: &mut usize, gray: bool) -> bool {
    let id = ui.id().with(("gradient editor", id));
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 44.0), Sense::hover());
    let bar = Rect::from_min_max(Pos2::new(rect.left() + 7.0, rect.top() + 2.0), Pos2::new(rect.right() - 7.0, rect.top() + 24.0));
    let strip = Rect::from_min_max(Pos2::new(rect.left(), bar.bottom()), Pos2::new(rect.right(), rect.bottom()));
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    super::mark(ui.ctx(), "gradient editor", rect.intersect(ui.clip_rect()));
    let to_t = |x: f32| ((x - bar.left()) / bar.width().max(1.0)).clamp(0.0, 1.0);
    let marker_x = |s: &GradStop| bar.left() + s.pos * bar.width();
    let mut changed = false;
    let drag_key = id.with("dragging");
    let (pressed, released, pos) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.interact_pos()));
    if pressed && resp.hovered() {
        if let Some(p) = pos {
            // The nearest marker under the cursor, else a new stop there.
            let near = (0..stops.len())
                .filter(|&i| (marker_x(&stops[i]) - p.x).abs() <= 7.0 && p.y >= bar.bottom() - 2.0)
                .min_by(|&a, &b| (marker_x(&stops[a]) - p.x).abs().total_cmp(&(marker_x(&stops[b]) - p.x).abs()));
            let i = match near {
                Some(i) => i,
                None if (bar.contains(p) || strip.contains(p)) && stops.len() < 16 => {
                    let t = to_t(p.x);
                    let (c, a) = ramp_at(stops, t);
                    stops.push(GradStop::new(t, c, a));
                    changed = true;
                    stops.len() - 1
                }
                None => usize::MAX,
            };
            if i != usize::MAX {
                *sel = i;
                ui.data_mut(|d| d.insert_temp(drag_key, i));
            }
        }
    }
    let dragging: Option<usize> = ui.data(|d| d.get_temp(drag_key));
    if let (Some(i), Some(p)) = (dragging, pos) {
        if ui.input(|i| i.pointer.primary_down()) && i < stops.len() {
            let t = to_t(p.x);
            if (stops[i].pos - t).abs() > 1e-4 {
                stops[i].pos = t;
                changed = true;
            }
            // Pulled down out of the editor: removed (two stops stay).
            if p.y > rect.bottom() + 24.0 && stops.len() > 2 {
                stops.remove(i);
                *sel = (*sel).min(stops.len() - 1);
                ui.data_mut(|d| d.remove::<usize>(drag_key));
                changed = true;
            }
        }
    }
    if released {
        ui.data_mut(|d| d.remove::<usize>(drag_key));
    }
    *sel = (*sel).min(stops.len().saturating_sub(1));
    let p = ui.painter();
    paint_ramp(p, bar, stops, gray);
    for (i, s) in stops.iter().enumerate() {
        let x = marker_x(s);
        let top = bar.bottom() + 2.0;
        let sq = Rect::from_min_max(Pos2::new(x - 6.0, top + 5.0), Pos2::new(x + 6.0, top + 17.0));
        let edge = if i == *sel { style::ACCENT } else { Color32::from_gray(150) };
        p.add(egui::Shape::convex_polygon(vec![Pos2::new(x, top), Pos2::new(x + 6.0, top + 5.0), Pos2::new(x - 6.0, top + 5.0)], edge, Stroke::NONE));
        p.rect_filled(sq, 2.0, edge);
        let inner = sq.shrink(2.0);
        checker(p, inner);
        p.rect_filled(inner, 1.0, shown(s, gray, true));
    }
    let hover = resp.hovered() && dragging.is_none();
    resp.on_hover_text_at_pointer(if hover {
        "Click to add a stop, drag a stop to move it, drag it down out of here to remove it"
    } else {
        ""
    });
    changed
}

/// The editor with the selected stop's settings and the presets under it.
pub fn editor_section(ui: &mut Ui, id: &str, stops: &mut Vec<GradStop>, sel: &mut usize, gray: bool, brush: [f32; 3]) {
    editor(ui, id, stops, sel, gray);
    let Some(s) = stops.get_mut(*sel) else { return };
    ui.label(RichText::new(if gray { "Stop value" } else { "Stop color" }).size(12.0).color(LABEL));
    if gray {
        let mut v = s.color[0];
        if widgets::gradient_slider(ui, (id, "stop value"), &mut v, 0.0, 1.0, Color32::BLACK, Color32::WHITE) {
            s.color = [v; 3];
        }
    } else {
        widgets::color_bar_named(ui, (id, "stop color"), "Gradient stop", &mut s.color);
    }
    widgets::Slider::new("Stop opacity", &mut s.alpha, 0.0..=1.0).percent().show(ui);
    widgets::Slider::new("Location", &mut s.pos, 0.0..=1.0).percent().show(ui);
    ui.horizontal(|ui| {
        let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
        if ui.add_enabled(stops.len() > 2, egui::Button::new("Delete stop").min_size(Vec2::new(half, 22.0))).clicked() {
            stops.remove(*sel);
            *sel = (*sel).min(stops.len() - 1);
        }
        if ui.add(egui::Button::new("Reverse").min_size(Vec2::new(half, 22.0))).on_hover_text("Flip the gradient end for end").clicked() {
            for s in stops.iter_mut() {
                s.pos = 1.0 - s.pos;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.add_sized([widgets::label_width(ui), 22.0], egui::Label::new(RichText::new("Presets").size(12.0).color(LABEL)));
        let w = (ui.available_width() - 2.0 * ui.spacing().button_padding.x - ui.spacing().icon_width - 8.0).max(16.0);
        egui::ComboBox::from_id_salt(Id::new((id, "presets"))).width(w).selected_text("Choose...").show_ui(ui, |ui| {
            for (name, p) in presets(gray, brush) {
                let row = ui.horizontal(|ui| {
                    let swatch = preview(ui, &p, gray, Vec2::new(46.0, 16.0));
                    swatch.clicked() | ui.selectable_label(false, name).clicked()
                });
                if row.inner {
                    *stops = p;
                    *sel = 0;
                    ui.close_menu();
                }
            }
        });
    });
}
