//! Painter-style widgets: label + value over a thin slider with a round
//! handle, gradient sliders, color bars, channel chips, resource slots and
//! cards, section titles, icon tabs and the searchable resource picker.

use super::icons::{self, Icon};
use crate::style;
use eframe::egui::{self, Color32, Id, Pos2, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};

pub const LABEL: Color32 = Color32::from_gray(200);
pub const VALUE: Color32 = Color32::from_gray(225);
pub const DIM: Color32 = Color32::from_gray(120);

fn font(size: f32) -> egui::FontId {
    egui::FontId::proportional(size)
}

/// A number being typed: text, pass it was last drawn in, bits of the value
/// when the edit started (a field drawn for another layer at the same place
/// shows another value: it must not inherit the edit).
#[derive(Clone, Default)]
struct EditBuf(String, u64, u32);

/// Label and value on one line, a thin track below with a hollow handle.
pub struct Slider<'a> {
    label: &'a str,
    value: &'a mut f32,
    min: f32,
    max: f32,
    decimals: usize,
    percent: bool,
    log: bool,
    indent: f32,
    enabled: bool,
    suffix: &'a str,
    /// Typed values may go beyond the track range, up to these limits.
    soft: Option<(f32, f32)>,
}

impl<'a> Slider<'a> {
    pub fn new(label: &'a str, value: &'a mut f32, range: std::ops::RangeInclusive<f32>) -> Self {
        Slider { label, value, min: *range.start(), max: *range.end(), decimals: 2, percent: false, log: false, indent: 0.0, enabled: true, suffix: "", soft: None }
    }
    pub fn decimals(mut self, d: usize) -> Self {
        self.decimals = d;
        self
    }
    /// Shows 0..1 values as 0..100.
    pub fn percent(mut self) -> Self {
        self.percent = true;
        self.decimals = 0;
        self
    }
    pub fn log(mut self) -> Self {
        self.log = true;
        self
    }
    /// Typed values can go up to `lo..=hi` (the track keeps its range).
    pub fn soft(mut self, lo: f32, hi: f32) -> Self {
        self.soft = Some((lo, hi));
        self
    }
    pub fn enabled(mut self, e: bool) -> Self {
        self.enabled = e;
        self
    }

    fn to_t(&self, v: f32) -> f32 {
        let (a, b) = (self.min, self.max);
        if self.log && a > 0.0 {
            ((v.max(a)).ln() - a.ln()) / (b.ln() - a.ln())
        } else {
            (v - a) / (b - a)
        }
        .clamp(0.0, 1.0)
    }

    fn from_t(&self, t: f32) -> f32 {
        let (a, b) = (self.min, self.max);
        let t = t.clamp(0.0, 1.0);
        if self.log && a > 0.0 {
            (a.ln() + t * (b.ln() - a.ln())).exp()
        } else {
            a + t * (b - a)
        }
    }

    /// Returns true when the value changed.
    pub fn show(self, ui: &mut Ui) -> bool {
        let w = ui.available_width();
        let (rect, base) = ui.allocate_exact_size(Vec2::new(w, 34.0), Sense::hover());
        let rect = Rect::from_min_max(rect.min + Vec2::new(self.indent, 0.0), rect.max);
        // The allocation's auto id is unique: labels repeat ("Scale" in every
        // procedural, child UIs share their parent's id), and a clashing id
        // left all but one of the sliders dead to the mouse.
        let id = base.id.with("sp slider");
        let col = |c: Color32| if self.enabled { c } else { c.linear_multiply(0.45) };
        let p = ui.painter().clone();
        p.text(rect.min + Vec2::new(0.0, 1.0), egui::Align2::LEFT_TOP, self.label, font(12.0), col(LABEL));

        // Value: a click types a number.
        let vrect = Rect::from_min_max(Pos2::new(rect.max.x - 64.0, rect.min.y), Pos2::new(rect.max.x - 6.0, rect.min.y + 16.0));
        let mut changed = false;
        let mut typed = false;
        let mut value = *self.value;
        let edit = NumberEdit { decimals: self.decimals, percent: self.percent, suffix: self.suffix, enabled: self.enabled, color: col(VALUE) };
        if let Some(v) = edit.show(ui, id, vrect, *self.value) {
            value = v;
            changed = true;
            typed = true;
        }

        // Track.
        let y = rect.min.y + 25.0;
        let (x0, x1) = (rect.min.x + 6.0, rect.max.x - 8.0);
        let track = Rect::from_min_max(Pos2::new(x0 - 6.0, y - 8.0), Pos2::new(x1 + 6.0, y + 8.0));
        let tresp = ui.interact(track, id.with("track"), if self.enabled { Sense::click_and_drag() } else { Sense::hover() });
        let (mods, dx) = ui.input(|i| (i.modifiers, i.pointer.delta().x));
        if let Some(pos) = tresp.interact_pointer_pos() {
            if mods.shift && tresp.dragged() {
                // Shift: fine adjustment, a tenth of the pointer motion.
                value = self.from_t(self.to_t(*self.value) + dx / (x1 - x0) * 0.1);
                changed = true;
            } else if !mods.shift && (tresp.is_pointer_button_down_on() || tresp.dragged() || tresp.clicked()) {
                // Jump on press, not on release.
                let mut t = (pos.x - x0) / (x1 - x0);
                if mods.ctrl {
                    // Ctrl snaps to tenths of the track (round values on log tracks).
                    t = (t * 10.0).round() / 10.0;
                }
                let mut v = self.from_t(t);
                if mods.ctrl && self.log {
                    v = round_sig(v, 2);
                }
                value = v;
                changed = true;
            }
        }
        // Arrow keys over the slider: one percent steps (Shift: ten).
        if self.enabled && (tresp.hovered() || ui.rect_contains_pointer(vrect)) && !ui.ctx().wants_keyboard_input() {
            let (up, down) = ui.input_mut(|i| {
                let m = i.modifiers;
                let up = i.consume_key(m, egui::Key::ArrowRight) || i.consume_key(m, egui::Key::ArrowUp);
                let down = i.consume_key(m, egui::Key::ArrowLeft) || i.consume_key(m, egui::Key::ArrowDown);
                (up, down)
            });
            if up || down {
                let step = if mods.shift { 0.1 } else { 0.01 };
                let t = self.to_t(*self.value) + if up { step } else { -step };
                value = self.from_t(t);
                changed = true;
            }
        }
        if changed {
            let (lo, hi) = match self.soft {
                Some((lo, hi)) if typed => (lo, hi),
                _ => (self.min, self.max),
            };
            value = value.clamp(lo, hi);
            if self.decimals == 0 && !self.percent {
                value = value.round();
            }
            *self.value = value;
        }
        let t = self.to_t(*self.value);
        let hx = x0 + t * (x1 - x0);
        p.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], Stroke::new(1.0f32, col(Color32::from_gray(95))));
        p.line_segment([Pos2::new(x0, y), Pos2::new(hx, y)], Stroke::new(2.0f32, col(Color32::from_gray(205))));
        let hot = tresp.hovered() || tresp.dragged();
        p.circle_filled(Pos2::new(hx, y), if hot { 6.5 } else { 5.5 }, style::PANEL);
        p.circle_stroke(Pos2::new(hx, y), if hot { 6.5 } else { 5.5 }, Stroke::new(1.5f32, col(if hot { Color32::WHITE } else { Color32::from_gray(200) })));
        changed
    }
}

/// Horizontal gradient bar with a handle (uniform grayscale channel values).
pub fn gradient_slider(ui: &mut Ui, id: impl std::hash::Hash, value: &mut f32, min: f32, max: f32, from: Color32, to: Color32) -> bool {
    let w = ui.available_width();
    let (row, base) = ui.allocate_exact_size(Vec2::new(w, 20.0), Sense::hover());
    let _ = id;
    // Bar on the left, the typed value on the right (like Painter).
    let vrect = Rect::from_min_max(Pos2::new(row.max.x - 52.0, row.min.y + 2.0), Pos2::new(row.max.x - 4.0, row.max.y - 2.0));
    let rect = Rect::from_min_max(row.min, Pos2::new(vrect.min.x - 6.0, row.max.y));
    let resp = ui.interact(rect, base.id.with("bar"), Sense::click_and_drag());
    let mut changed = false;
    let edit = NumberEdit { decimals: 3, percent: false, suffix: "", enabled: true, color: VALUE };
    if let Some(v) = edit.show(ui, base.id.with("number"), vrect, *value) {
        *value = v.clamp(min, max);
        changed = true;
    }
    let bar = Rect::from_center_size(rect.center(), Vec2::new(rect.width() - 12.0, 7.0));
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(bar.left_top(), from);
    mesh.colored_vertex(bar.right_top(), to);
    mesh.colored_vertex(bar.right_bottom(), to);
    mesh.colored_vertex(bar.left_bottom(), from);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    let p = ui.painter();
    p.add(egui::Shape::mesh(mesh));
    p.line_segment([Pos2::new(bar.center().x, bar.min.y), Pos2::new(bar.center().x, bar.max.y)], Stroke::new(1.0f32, Color32::from_gray(90)));
    let (shift, dx) = ui.input(|i| (i.modifiers.shift, i.pointer.delta().x));
    if let Some(pos) = resp.interact_pointer_pos() {
        if shift && resp.dragged() {
            *value = (*value + dx / bar.width() * 0.1 * (max - min)).clamp(min, max);
            changed = true;
        } else if !shift && (resp.is_pointer_button_down_on() || resp.dragged() || resp.clicked()) {
            let t = ((pos.x - bar.min.x) / bar.width()).clamp(0.0, 1.0);
            *value = min + t * (max - min);
            changed = true;
        }
    }
    let t = ((*value - min) / (max - min)).clamp(0.0, 1.0);
    let c = Pos2::new(bar.min.x + t * bar.width(), bar.center().y);
    let hot = resp.hovered() || resp.dragged();
    let shade = (t * 255.0) as u8;
    p.circle_filled(c, 6.0, Color32::from_gray(shade));
    p.circle_stroke(c, 6.0, Stroke::new(1.5f32, if hot { Color32::WHITE } else { Color32::from_gray(170) }));
    let _ = resp;
    changed
}

fn round_sig(v: f32, digits: i32) -> f32 {
    if v == 0.0 {
        return 0.0;
    }
    let m = 10f32.powi(digits - 1 - v.abs().log10().floor() as i32);
    (v * m).round() / m
}

/// Number shown right-aligned in a rect; a click turns it into a text field
/// with the whole number selected. Enter or a click elsewhere applies,
/// Escape cancels. An edit is dropped (not applied) when its field stops
/// being drawn or now edits another value.
struct NumberEdit<'a> {
    decimals: usize,
    percent: bool,
    suffix: &'a str,
    enabled: bool,
    color: Color32,
}

impl NumberEdit<'_> {
    fn show(&self, ui: &mut Ui, id: Id, vrect: Rect, value: f32) -> Option<f32> {
        let owner = value.to_bits();
        let pass = ui.ctx().cumulative_pass_nr();
        let vresp = ui.interact(vrect, id.with("value"), if self.enabled { Sense::click() } else { Sense::hover() });
        let shown = if self.percent { value * 100.0 } else { value };
        let live = ui.data(|d| d.get_temp::<EditBuf>(id)).filter(|e| e.1 + 1 >= pass && e.2 == owner);
        let Some(EditBuf(mut buf, _, _)) = live else {
            ui.painter().text(Pos2::new(vrect.max.x, vrect.min.y + 1.0), egui::Align2::RIGHT_TOP, format!("{:.*}{}", self.decimals, shown, self.suffix), font(12.0), self.color);
            if vresp.clicked() {
                ui.data_mut(|d| {
                    d.insert_temp(id, EditBuf(format!("{:.*}", self.decimals, shown), pass, owner));
                    d.insert_temp(id.with("fresh"), true);
                });
            }
            if self.enabled {
                vresp.on_hover_cursor(egui::CursorIcon::Text);
            }
            return None;
        };
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(vrect));
        let mut out = egui::TextEdit::singleline(&mut buf).id(id.with("edit")).desired_width(vrect.width()).horizontal_align(egui::Align::RIGHT).show(&mut child);
        let r = out.response.clone();
        let fresh = ui.data(|d| d.get_temp::<bool>(id.with("fresh")).is_some());
        if fresh && !r.has_focus() {
            // First frame of the edit: focus and select the whole number.
            r.request_focus();
            out.state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(buf.chars().count()))));
            out.state.store(ui.ctx(), r.id);
            ui.data_mut(|d| d.remove::<bool>(id.with("fresh")));
        }
        let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
        let done = esc || r.lost_focus() || (enter && r.has_focus()) || (!fresh && !r.has_focus());
        if !done {
            ui.data_mut(|d| d.insert_temp(id, EditBuf(buf, pass, owner)));
            return None;
        }
        ui.data_mut(|d| d.remove::<EditBuf>(id));
        if esc || !(r.lost_focus() || enter) {
            return None;
        }
        let v = buf.trim().trim_end_matches(self.suffix).trim().replace(',', ".").parse::<f32>().ok()?;
        Some(if self.percent { v / 100.0 } else { v })
    }
}

fn lin_to_c32(c: [f32; 3]) -> Color32 {
    egui::Rgba::from_rgb(c[0].max(0.0), c[1].max(0.0), c[2].max(0.0)).into()
}

fn c32_to_lin(c: Color32) -> [f32; 3] {
    let r = egui::Rgba::from(c);
    [r.r(), r.g(), r.b()]
}

/// HSV of gamma (sRGB) values in 0..1.
fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    let h = if d <= 1e-6 {
        0.0
    } else if mx == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if mx == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let s = if mx <= 1e-6 { 0.0 } else { d / mx };
    (h, s, mx)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h = h.rem_euclid(1.0) * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (r + m, g + m, b + m)
}

fn gamma01(c: Color32) -> (f32, f32, f32) {
    (c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0)
}

fn from_gamma01(r: f32, g: f32, b: f32) -> Color32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    Color32::from_rgb(q(r), q(g), q(b))
}

/// The open color picker: owner widget id, window title, previous color.
#[derive(Clone)]
struct OpenPicker {
    owner: Id,
    title: String,
    previous: [f32; 3],
    hue: f32,
    /// Pass in which the owner bar last drew the picker.
    drawn: u64,
}

/// Start of every frame: a picker or an armed eyedropper whose color bar was
/// not drawn last frame (selection changed, panel hidden) goes away instead
/// of coming back later on another field; the "picker hovered" flag is reset.
pub fn begin_frame(ctx: &egui::Context) {
    let pass = ctx.cumulative_pass_nr();
    let key = Id::new("color picker open");
    ctx.data_mut(|d| {
        d.insert_temp(Id::new("color picker hovered"), false);
        if d.get_temp::<OpenPicker>(key).is_some_and(|o| o.drawn + 1 < pass) {
            d.remove::<OpenPicker>(key);
        }
        if let Some(owner) = d.get_temp::<Id>(eyedropper_id()) {
            if d.get_temp::<u64>(owner.with("drawn")).map_or(true, |f| f + 1 < pass) {
                d.remove::<Id>(eyedropper_id());
            }
        }
        // The viewport disarms it when it samples: popups ask about the frame start.
        let armed = d.get_temp::<Id>(eyedropper_id()).is_some();
        d.insert_temp(eyedropper_armed_at_start_id(), armed);
    });
}

/// Memory key: an eyedropper was armed when this frame started.
pub fn eyedropper_armed_at_start_id() -> Id {
    Id::new("eyedropper armed at frame start")
}

/// Memory key of the eyedropper armed by a picker (its owner id).
pub fn eyedropper_id() -> Id {
    Id::new("eyedropper armed")
}

/// Memory key where the viewport puts the sampled color: (owner, color).
pub fn eyedropper_result_id() -> Id {
    Id::new("eyedropper result")
}

/// A color picker window is open.
pub fn color_picker_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<OpenPicker>(Id::new("color picker open"))).is_some()
}

/// Closes the open color picker; returns true if one was open (Escape).
pub fn close_color_picker(ctx: &egui::Context) -> bool {
    let open = ctx.data(|d| d.get_temp::<OpenPicker>(Id::new("color picker open"))).is_some();
    ctx.data_mut(|d| {
        d.remove::<OpenPicker>(Id::new("color picker open"));
        d.remove::<Id>(eyedropper_id());
    });
    open
}

/// Full width color swatch with an eyedropper; click opens the color
/// picker window. `c` is linear RGB.
pub fn color_bar(ui: &mut Ui, id: impl std::hash::Hash, c: &mut [f32; 3]) -> bool {
    color_bar_named(ui, id, "Color", c)
}

pub fn color_bar_named(ui: &mut Ui, id: impl std::hash::Hash, title: &str, c: &mut [f32; 3]) -> bool {
    let owner = ui.id().with(("color bar", id));
    let mut changed = false;
    // A color sampled by the eyedropper for this bar.
    if let Some((o, col)) = ui.data(|d| d.get_temp::<(Id, [f32; 3])>(eyedropper_result_id())) {
        if o == owner {
            *c = col;
            changed = true;
            ui.data_mut(|d| d.remove::<(Id, [f32; 3])>(eyedropper_result_id()));
        }
    }
    let w = ui.available_width();
    let (row, _) = ui.allocate_exact_size(Vec2::new(w, 20.0), Sense::hover());
    let bar = Rect::from_min_max(row.min, Pos2::new(row.max.x - 24.0, row.max.y));
    let resp = ui.interact(bar, owner.with("bar"), Sense::click()).on_hover_text("Click to edit the color");
    let p = ui.painter();
    p.rect_filled(bar, 2.0, style::FIELD);
    p.rect_filled(bar.shrink(2.0), 1.0, lin_to_c32(*c));
    if resp.hovered() {
        p.rect_stroke(bar, 2.0, Stroke::new(1.0f32, Color32::from_gray(150)));
    }
    let er = Rect::from_center_size(Pos2::new(row.max.x - 10.0, row.center().y), Vec2::splat(18.0));
    let pass = ui.ctx().cumulative_pass_nr();
    ui.data_mut(|d| d.insert_temp(owner.with("drawn"), pass));
    let armed = ui.data(|d| d.get_temp::<Id>(eyedropper_id())) == Some(owner);
    let eresp = ui.interact(er, owner.with("eyedrop"), Sense::click()).on_hover_text("Eyedropper: click the mesh (its color) or anywhere on the screen");
    // Only the visible part (a scrolled-away bar marks nothing).
    super::mark(ui.ctx(), format!("colorbar:{title}"), bar.intersect(ui.clip_rect()));
    super::mark(ui.ctx(), format!("eyedrop:{title}"), er.intersect(ui.clip_rect()));
    if armed {
        // Live color under the cursor behind the armed eyedropper.
        if let Some(live) = ui.data(|d| d.get_temp::<[f32; 3]>(Id::new("eyedropper live"))) {
            ui.painter().rect_filled(er, 2.0, lin_to_c32(live));
            ui.painter().rect_stroke(er, 2.0, Stroke::new(1.5f32, style::ACCENT));
        }
    }
    icons::paint(ui.painter(), er.shrink(2.0), Icon::Picker, if armed { Color32::WHITE } else if eresp.hovered() { Color32::WHITE } else { Color32::from_gray(160) });
    if eresp.clicked() {
        ui.data_mut(|d| if armed { d.remove::<Id>(eyedropper_id()) } else { d.insert_temp(eyedropper_id(), owner) });
    }
    let key = Id::new("color picker open");
    if resp.clicked() {
        let (r, g, b) = gamma01(lin_to_c32(*c));
        let hue = rgb_to_hsv(r, g, b).0;
        ui.data_mut(|d| d.insert_temp(key, OpenPicker { owner, title: title.to_string(), previous: *c, hue, drawn: pass }));
    }
    let Some(mut open) = ui.data(|d| d.get_temp::<OpenPicker>(key)).filter(|o| o.owner == owner) else { return changed };
    open.drawn = pass;
    let mut keep = true;
    let win = egui::Window::new(RichText::new(format!("Color for {}", open.title)).size(12.0).color(Color32::from_gray(200)))
        .id(owner.with("picker window"))
        .open(&mut keep)
        .collapsible(false)
        .resizable(false)
        .default_pos(bar.left_bottom() + Vec2::new(60.0, 6.0))
        // Above popups such as the viewport quick properties it opens from.
        .order(egui::Order::Foreground)
        .frame(egui::Frame::window(ui.style()).fill(style::PANEL))
        .show(ui.ctx(), |ui| {
            ui.set_width(250.0);
            ui.label(RichText::new("sRGB").size(11.5).color(LABEL));
            let (gr, gg, gb) = gamma01(lin_to_c32(*c));
            let (mut h, s, v) = rgb_to_hsv(gr, gg, gb);
            if s < 1e-4 || v < 1e-4 {
                h = open.hue;
            }
            // Saturation / value square and a vertical hue bar.
            let (area, _) = ui.allocate_exact_size(Vec2::new(250.0, 200.0), Sense::hover());
            let sq = Rect::from_min_size(area.min, Vec2::new(220.0, 200.0));
            let hb = Rect::from_min_max(Pos2::new(sq.max.x + 12.0, sq.min.y), Pos2::new(area.max.x, sq.max.y));
            let mut mesh = egui::Mesh::default();
            let n = 16;
            for j in 0..=n {
                for i in 0..=n {
                    let (fs, fv) = (i as f32 / n as f32, 1.0 - j as f32 / n as f32);
                    let (r, g, b) = hsv_to_rgb(h, fs, fv);
                    mesh.colored_vertex(Pos2::new(sq.min.x + fs * sq.width(), sq.min.y + (1.0 - fv) * sq.height()), from_gamma01(r, g, b));
                }
            }
            for j in 0..n {
                for i in 0..n {
                    let a = (j * (n + 1) + i) as u32;
                    let b2 = a + n as u32 + 1;
                    mesh.add_triangle(a, a + 1, b2 + 1);
                    mesh.add_triangle(a, b2 + 1, b2);
                }
            }
            let mut hm = egui::Mesh::default();
            for k in 0..=12 {
                let t = k as f32 / 12.0;
                let (r, g, b) = hsv_to_rgb(1.0 - t, 1.0, 1.0);
                let y = hb.min.y + t * hb.height();
                hm.colored_vertex(Pos2::new(hb.min.x, y), from_gamma01(r, g, b));
                hm.colored_vertex(Pos2::new(hb.max.x, y), from_gamma01(r, g, b));
                if k > 0 {
                    let i = (k * 2) as u32;
                    hm.add_triangle(i - 2, i - 1, i + 1);
                    hm.add_triangle(i - 2, i + 1, i);
                }
            }
            let p = ui.painter();
            p.add(egui::Shape::mesh(mesh));
            p.add(egui::Shape::mesh(hm));
            let sresp = ui.interact(sq, owner.with("sv"), Sense::click_and_drag());
            let hresp = ui.interact(hb, owner.with("hue"), Sense::click_and_drag());
            let mut new_hsv = None;
            if let Some(q) = sresp.interact_pointer_pos() {
                if sresp.dragged() || sresp.clicked() {
                    let fs = ((q.x - sq.min.x) / sq.width()).clamp(0.0, 1.0);
                    let fv = 1.0 - ((q.y - sq.min.y) / sq.height()).clamp(0.0, 1.0);
                    new_hsv = Some((h, fs, fv));
                }
            }
            if let Some(q) = hresp.interact_pointer_pos() {
                if hresp.dragged() || hresp.clicked() {
                    let nh = 1.0 - ((q.y - hb.min.y) / hb.height()).clamp(0.0, 1.0);
                    open.hue = nh;
                    new_hsv = Some((nh, s, v));
                }
            }
            let p = ui.painter();
            let cur = Pos2::new(sq.min.x + s * sq.width(), sq.min.y + (1.0 - v) * sq.height());
            p.circle_stroke(cur, 5.0, Stroke::new(1.5f32, Color32::WHITE));
            p.circle_stroke(cur, 6.5, Stroke::new(1.0f32, Color32::BLACK));
            let hy = hb.min.y + (1.0 - h) * hb.height();
            p.circle_filled(Pos2::new(hb.center().x, hy), 6.0, Color32::from_gray(230));
            ui.add_space(6.0);
            // Current / previous swatches, hex and eyedropper.
            ui.horizontal(|ui| {
                let (sw, _) = ui.allocate_exact_size(Vec2::new(70.0, 24.0), Sense::hover());
                ui.painter().rect_filled(Rect::from_min_max(sw.min, Pos2::new(sw.center().x, sw.max.y)), 0.0, lin_to_c32(*c));
                let prev = Rect::from_min_max(Pos2::new(sw.center().x, sw.min.y), sw.max);
                ui.painter().rect_filled(prev, 0.0, lin_to_c32(open.previous));
                if ui.interact(prev, owner.with("prev"), Sense::click()).on_hover_text("Previous color: click to restore").clicked() {
                    *c = open.previous;
                    changed = true;
                }
                ui.label(RichText::new("#").color(LABEL));
                let col = lin_to_c32(*c);
                let hex_id = owner.with("hex");
                let mut hex = ui.data(|d| d.get_temp::<String>(hex_id)).unwrap_or_else(|| format!("{:02X}{:02X}{:02X}", col.r(), col.g(), col.b()));
                let te = ui.add(egui::TextEdit::singleline(&mut hex).desired_width(70.0).char_limit(7));
                if te.has_focus() {
                    ui.data_mut(|d| d.insert_temp(hex_id, hex.clone()));
                } else {
                    ui.data_mut(|d| d.remove::<String>(hex_id));
                }
                // A click on the square that also ends the hex edit wins.
                if new_hsv.is_none() && (te.lost_focus() || (te.changed() && hex.trim_start_matches('#').len() == 6)) {
                    let hs = hex.trim_start_matches('#');
                    if let (Ok(r), Ok(g), Ok(b)) = (u8::from_str_radix(&hs.get(0..2).unwrap_or("x"), 16), u8::from_str_radix(&hs.get(2..4).unwrap_or("x"), 16), u8::from_str_radix(&hs.get(4..6).unwrap_or("x"), 16)) {
                        *c = c32_to_lin(Color32::from_rgb(r, g, b));
                        changed = true;
                    }
                }
                let armed = ui.data(|d| d.get_temp::<Id>(eyedropper_id())) == Some(owner);
                if icons::button(ui, Icon::Picker, armed, 22.0, "Eyedropper: click the mesh to pick a color").clicked() {
                    ui.data_mut(|d| if armed { d.remove::<Id>(eyedropper_id()) } else { d.insert_temp(eyedropper_id(), owner) });
                }
            });
            ui.add_space(4.0);
            let mode_id = owner.with("mode");
            let mut rgb_mode = ui.data(|d| d.get_temp::<bool>(mode_id)).unwrap_or(false);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut rgb_mode, false, "HSV");
                ui.selectable_value(&mut rgb_mode, true, "RGB");
            });
            ui.data_mut(|d| d.insert_temp(mode_id, rgb_mode));
            let row = |ui: &mut Ui, name: &str, v: &mut f32| -> bool {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(name).color(LABEL));
                    ui.spacing_mut().slider_width = 150.0;
                    ui.add(egui::Slider::new(v, 0.0..=1.0).show_value(false)).changed() | ui.add(egui::DragValue::new(v).speed(0.002).range(0.0..=1.0).max_decimals(3)).changed()
                })
                .inner
            };
            if rgb_mode {
                let (mut r, mut g, mut b) = gamma01(lin_to_c32(*c));
                let ch = row(ui, "R", &mut r) | row(ui, "G", &mut g) | row(ui, "B", &mut b);
                if ch {
                    *c = c32_to_lin(from_gamma01(r, g, b));
                    changed = true;
                }
            } else {
                let (mut hh, mut ss, mut vv) = (h, s, v);
                let ch = row(ui, "H", &mut hh) | row(ui, "S", &mut ss) | row(ui, "V", &mut vv);
                if ch {
                    open.hue = hh;
                    new_hsv = Some((hh, ss, vv));
                }
            }
            if let Some((hh, ss, vv)) = new_hsv {
                let (r, g, b) = hsv_to_rgb(hh, ss, vv);
                *c = c32_to_lin(from_gamma01(r, g, b));
                changed = true;
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Working color space").font(style::bold(11.5)));
            ui.label(RichText::new(format!("R {:.4}   G {:.4}   B {:.4}", c[0], c[1], c[2])).size(11.0).color(Color32::from_gray(170)));
            ui.add_space(4.0);
            ui.label(RichText::new("Swatches").font(style::bold(11.5)));
            let sw_id = Id::new("color swatches");
            let mut swatches = ui.data(|d| d.get_temp::<Vec<[f32; 3]>>(sw_id)).unwrap_or_default();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                let mut remove = None;
                for (i, s) in swatches.iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                    ui.painter().rect_filled(r, 2.0, lin_to_c32(*s));
                    let resp = resp.on_hover_text("Click to use, right click to delete");
                    if resp.clicked() {
                        *c = *s;
                        changed = true;
                    }
                    if resp.secondary_clicked() {
                        remove = Some(i);
                    }
                }
                if let Some(i) = remove {
                    swatches.remove(i);
                }
                if icons::button(ui, Icon::Plus, false, 18.0, "Add the current color").clicked() && swatches.last() != Some(c) {
                    swatches.push(*c);
                }
            });
            ui.data_mut(|d| d.insert_temp(sw_id, swatches));
        });
    // A click outside the picker (and its color bar) closes it, like a
    // popup; not the click an armed eyedropper uses to pick.
    let picker_rect = win.as_ref().map(|w| w.response.rect);
    let outside = ui.input(|i| {
        i.pointer.any_pressed() && i.pointer.press_origin().is_some_and(|p| !picker_rect.is_some_and(|r| r.contains(p)) && !row.contains(p))
    });
    let picking = ui.data(|d| d.get_temp::<bool>(eyedropper_armed_at_start_id())).unwrap_or(false);
    if outside && !picking {
        keep = false;
    }
    // Lets the viewport quick menu ignore clicks made inside the picker.
    let over = win.is_some_and(|w| w.response.rect.contains(ui.ctx().pointer_interact_pos().unwrap_or(Pos2::ZERO)));
    ui.data_mut(|d| d.insert_temp(Id::new("color picker hovered"), over));
    if keep {
        ui.data_mut(|d| d.insert_temp(key, open));
    } else {
        ui.data_mut(|d| d.remove::<OpenPicker>(key));
    }
    changed
}

/// Bordered toggle chip ("color", "rough"...).
pub fn chip(ui: &mut Ui, text: &str, on: bool, tip: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), style::bold(11.5), Color32::WHITE);
    let size = Vec2::new(galley.size().x + 14.0, 20.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let c = if on { Color32::from_gray(228) } else if resp.hovered() { Color32::from_gray(160) } else { Color32::from_gray(105) };
    let p = ui.painter();
    p.rect_stroke(rect.shrink(0.5), 3.0, Stroke::new(1.3f32, c));
    p.text(rect.center(), egui::Align2::CENTER_CENTER, text, style::bold(11.5), c);
    resp.on_hover_text(tip)
}

/// A resource slot box: bold title, subtitle, optional thumbnail. Click to
/// choose; the caller handles drops via `drop_hover` / `drop_take`.
pub fn slot_box(ui: &mut Ui, title: &str, subtitle: &str, thumb: Option<egui::TextureId>, drop_ok: bool) -> Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 40.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, if resp.hovered() { Color32::from_gray(52) } else { style::FIELD });
    p.rect_stroke(rect, 3.0, Stroke::new(1.0f32, if drop_ok { style::ACCENT } else { Color32::from_gray(64) }));
    if drop_ok {
        p.rect_stroke(rect.shrink(1.0), 3.0, Stroke::new(1.0f32, style::ACCENT));
    }
    match thumb {
        Some(t) => {
            let tr = Rect::from_min_size(rect.min + Vec2::new(4.0, 4.0), Vec2::splat(32.0));
            p.image(t, tr, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            p.text(Pos2::new(tr.max.x + 8.0, rect.min.y + 12.0), egui::Align2::LEFT_CENTER, title, style::bold(12.0), Color32::from_gray(230));
            p.text(Pos2::new(tr.max.x + 8.0, rect.min.y + 28.0), egui::Align2::LEFT_CENTER, subtitle, font(11.0), Color32::from_gray(170));
        }
        None => {
            p.text(Pos2::new(rect.center().x, rect.min.y + 13.0), egui::Align2::CENTER_CENTER, title, style::bold(12.0), Color32::from_gray(230));
            p.text(Pos2::new(rect.center().x, rect.min.y + 28.0), egui::Align2::CENTER_CENTER, subtitle, font(11.0), Color32::from_gray(170));
        }
    }
    resp
}

/// Card with a thumbnail, bold name, subtitle and a clear cross.
/// Returns (card response, cleared).
pub fn resource_card(ui: &mut Ui, thumb: Option<egui::TextureId>, name: &str, subtitle: &str, clearable: bool, drop_ok: bool) -> (Response, bool) {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 40.0), Sense::click());
    let p = ui.painter().clone();
    p.rect_filled(rect, 3.0, if resp.hovered() { Color32::from_gray(52) } else { style::FIELD });
    p.rect_stroke(rect, 3.0, Stroke::new(1.0f32, if drop_ok { style::ACCENT } else { Color32::from_gray(58) }));
    let tr = Rect::from_min_size(rect.min + Vec2::new(4.0, 4.0), Vec2::splat(32.0));
    p.rect_filled(tr, 1.0, Color32::BLACK);
    if let Some(t) = thumb {
        p.image(t, tr, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    p.text(Pos2::new(tr.max.x + 8.0, rect.min.y + 12.0), egui::Align2::LEFT_CENTER, name, style::bold(12.0), Color32::from_gray(235));
    p.text(Pos2::new(tr.max.x + 8.0, rect.min.y + 28.0), egui::Align2::LEFT_CENTER, subtitle, font(11.0), Color32::from_gray(170));
    let mut cleared = false;
    if clearable {
        let cr = Rect::from_center_size(Pos2::new(rect.max.x - 14.0, rect.center().y), Vec2::splat(16.0));
        let c = ui.interact(cr, resp.id.with("card clear"), Sense::click()).on_hover_text("Reset");
        icons::paint(&p, cr.shrink(3.0), Icon::Close, if c.hovered() { Color32::WHITE } else { Color32::from_gray(150) });
        cleared = c.clicked();
    }
    (resp, cleared)
}

/// Uppercase bold section title with a separator above it.
pub fn section_title(ui: &mut Ui, title: &str, first: bool) -> Rect {
    if !first {
        ui.add_space(8.0);
        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
        ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0f32, Color32::from_gray(64)));
    }
    ui.add_space(8.0);
    let r = ui.label(RichText::new(title.to_uppercase()).font(style::bold(12.0)).color(Color32::from_gray(235))).rect;
    ui.add_space(4.0);
    r
}

/// Collapsible bold sub section ("Parameters", "Attributes").
pub fn sub_section(ui: &mut Ui, id: impl std::hash::Hash, title: &str, default_open: bool, body: impl FnOnce(&mut Ui)) {
    let id = ui.id().with(("sub", id));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, default_open)
        .show_header(ui, |ui| {
            ui.label(RichText::new(title).font(style::bold(12.0)).color(Color32::from_gray(225)));
        })
        .body(|ui| body(ui));
}

/// Label column of the property rows: about 40 % of the panel.
pub fn label_width(ui: &Ui) -> f32 {
    (ui.available_width() * 0.42).clamp(48.0, 110.0)
}

/// "Label    [dropdown]" row.
pub fn combo_row<T: Copy + PartialEq>(ui: &mut Ui, label: &str, id: impl std::hash::Hash, value: &mut T, all: &[T], name: fn(T) -> &'static str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized([label_width(ui), 22.0], egui::Label::new(RichText::new(label).size(12.0).color(LABEL)).truncate());
        // ComboBox adds its padding and arrow to `width`: keep it inside the row.
        let w = (ui.available_width() - 2.0 * ui.spacing().button_padding.x - ui.spacing().icon_width - 8.0).max(16.0);
        egui::ComboBox::from_id_salt(id).width(w).wrap_mode(egui::TextWrapMode::Truncate).selected_text(name(*value)).show_ui(ui, |ui| {
            for v in all {
                changed |= ui.selectable_value(value, *v, name(*v)).changed();
            }
        });
    });
    changed
}

/// "Label    [Enabled / Disabled]" toggle row.
pub fn toggle_row(ui: &mut Ui, label: &str, value: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized([label_width(ui), 22.0], egui::Label::new(RichText::new(label).size(12.0).color(LABEL)).truncate());
        let text = if *value { "Enabled" } else { "Disabled" };
        let b = egui::Button::new(RichText::new(text).size(12.0)).wrap_mode(egui::TextWrapMode::Truncate).min_size(Vec2::new((ui.available_width() - 2.0).max(16.0), 22.0));
        if ui.add(b).clicked() {
            *value = !*value;
            changed = true;
        }
    });
    changed
}

/// Row of icon tabs (top of the Properties panel). Returns the clicked tab.
pub fn icon_tabs(ui: &mut Ui, tabs: &[(Icon, &str)], current: usize) -> Option<usize> {
    let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
    ui.painter().rect_filled(bar, 0.0, style::HEADER);
    let mut clicked = None;
    for (i, (icon, tip)) in tabs.iter().enumerate() {
        let r = Rect::from_min_size(bar.min + Vec2::new(i as f32 * 34.0, 0.0), Vec2::new(34.0, 30.0));
        let resp = ui.interact(r, ui.id().with(("icon tab", i)), Sense::click()).on_hover_text(*tip);
        if i == current {
            ui.painter().rect_filled(r, 0.0, style::PANEL);
            ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(2.0f32, style::ACCENT));
        }
        let c = if i == current || resp.hovered() { Color32::WHITE } else { Color32::from_gray(160) };
        icons::paint(ui.painter(), r.shrink(8.0), *icon, c);
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

/// One entry of the resource picker.
pub struct PickItem {
    pub name: String,
    pub thumb: Option<egui::TextureId>,
}

/// Opens / shows the searchable resource picker popup under `anchor`.
/// Returns the index of the clicked item. Closes on click outside, on pick
/// and on Escape (handled globally).
pub fn picker_popup(ui: &Ui, id: Id, anchor: &Response, title: &str, items: &[PickItem]) -> Option<usize> {
    let mut picked = None;
    popup_below(ui, id, anchor, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_width(300.0);
        ui.horizontal(|ui| {
            let r = ui.label(RichText::new(title.to_uppercase()).font(style::bold(11.5)).color(Color32::WHITE)).rect;
            ui.painter().line_segment([r.left_bottom() + Vec2::new(0.0, 3.0), r.right_bottom() + Vec2::new(0.0, 3.0)], Stroke::new(2.0f32, style::ACCENT));
        });
        ui.add_space(4.0);
        let fid = id.with("filter");
        let mut filter = ui.data(|d| d.get_temp::<String>(fid)).unwrap_or_default();
        let fr = ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Search").desired_width(f32::INFINITY));
        if ui.memory(|m| m.focused().is_none()) {
            fr.request_focus();
        }
        ui.data_mut(|d| d.insert_temp(fid, filter.clone()));
        let f = filter.to_lowercase();
        ui.add_space(4.0);
        egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
            let cols = 5;
            let size = 52.0;
            let list: Vec<(usize, &PickItem)> = items.iter().enumerate().filter(|(_, it)| f.is_empty() || it.name.to_lowercase().contains(&f)).collect();
            for chunk in list.chunks(cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (i, it) in chunk {
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(size, size + 14.0), Sense::click());
                        let p = ui.painter();
                        if resp.hovered() {
                            p.rect_filled(r, 2.0, Color32::from_gray(60));
                        }
                        let tr = Rect::from_min_size(r.min + Vec2::new(4.0, 2.0), Vec2::splat(size - 8.0));
                        match it.thumb {
                            Some(t) => {
                                p.image(t, tr, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                            }
                            None => {
                                p.rect_filled(tr, 2.0, Color32::from_gray(45));
                            }
                        }
                        let mut n = it.name.clone();
                        if n.chars().count() > 9 {
                            n = n.chars().take(8).collect::<String>() + "...";
                        }
                        p.text(Pos2::new(r.center().x, r.max.y - 6.0), egui::Align2::CENTER_CENTER, n, font(10.0), Color32::from_gray(200));
                        if resp.on_hover_text(&it.name).clicked() {
                            picked = Some(*i);
                        }
                    }
                });
            }
        });
    });
    if picked.is_some() {
        ui.memory_mut(|m| m.close_popup());
    }
    picked
}

/// `egui::popup::popup_below_widget` that stays whole on screen: near the
/// right edge it opens leftward from the anchor (the layer rows' blend,
/// opacity and mask menus sit there), near the bottom above it. egui's own
/// version squeezes it against the edge instead.
pub fn popup_below<R>(ui: &Ui, id: Id, anchor: &Response, close: egui::PopupCloseBehavior, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    if !ui.memory(|m| m.is_popup_open(id)) {
        return None;
    }
    let screen = ui.ctx().screen_rect();
    let frame = egui::Frame::popup(ui.style());
    let inner_width = anchor.rect.width() - frame.total_margin().sum().x;
    // Last frame's size (first frame: a typical menu).
    let size = ui.ctx().memory(|m| m.area_rect(id)).map_or(Vec2::new(200.0, 160.0), |r| r.size());
    let leftward = anchor.rect.left() + size.x.max(inner_width) > screen.right() - 2.0;
    let above = anchor.rect.bottom() + size.y > screen.bottom() - 2.0 && anchor.rect.top() - size.y > screen.top();
    let (pos, pivot) = match (leftward, above) {
        (false, false) => (anchor.rect.left_bottom(), egui::Align2::LEFT_TOP),
        (true, false) => (anchor.rect.right_bottom(), egui::Align2::RIGHT_TOP),
        (false, true) => (anchor.rect.left_top(), egui::Align2::LEFT_BOTTOM),
        (true, true) => (anchor.rect.right_top(), egui::Align2::RIGHT_BOTTOM),
    };
    let response = egui::Area::new(id)
        .kind(egui::UiKind::Popup)
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .pivot(pivot)
        .default_width(inner_width)
        .show(ui.ctx(), |ui| {
            frame
                .show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        ui.set_min_width(inner_width);
                        add(ui)
                    })
                    .inner
                })
                .inner
        });
    let should_close = match close {
        egui::PopupCloseBehavior::CloseOnClick => anchor.clicked_elsewhere(),
        egui::PopupCloseBehavior::CloseOnClickOutside => anchor.clicked_elsewhere() && response.response.clicked_elsewhere(),
        egui::PopupCloseBehavior::IgnoreClicks => false,
    };
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) || should_close {
        ui.memory_mut(|m| m.close_popup());
    }
    Some(response.inner)
}

/// A submenu unfolded in place (click its title) inside a context menu or
/// popup. egui's flyout submenus have no room next to menus at the right
/// edge of the screen (the layer stack) and open over their parent, so a
/// click there lands on the wrong entry.
pub fn sub_menu<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let id = ui.id().with(("sub menu", label));
    // Folded again whenever its menu was closed (not drawn last frame).
    let pass = ui.ctx().cumulative_pass_nr();
    let open = ui.data(|d| d.get_temp::<(bool, u64)>(id)).is_some_and(|(o, at)| o && at + 1 >= pass);
    let resp = ui.add(egui::Button::new(label).shortcut_text("   "));
    let r = Rect::from_center_size(Pos2::new(resp.rect.max.x - 10.0, resp.rect.center().y), Vec2::splat(10.0));
    icons::paint(ui.painter(), r, if open != resp.clicked() { Icon::Chevron } else { Icon::Arrow }, Color32::from_gray(190));
    let open = open != resp.clicked();
    ui.data_mut(|d| d.insert_temp(id, (open, pass)));
    if !open {
        return None;
    }
    Some(ui.indent(id, add).inner)
}
