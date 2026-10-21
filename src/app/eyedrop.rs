//! Eyedropper of the color fields: while armed the cursor is a crosshair
//! with a live swatch of the color under it; a click picks the material's
//! base color over the 3D view, the screen pixel anywhere else, even
//! outside the window (Windows: the desktop is read with GDI). The click
//! that picks never reaches the widget under it. Escape or a right click
//! cancels.

use super::*;
use eframe::egui::Color32;

#[cfg(windows)]
mod screen {
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetCursorPos(p: *mut Point) -> i32;
        fn GetAsyncKeyState(key: i32) -> i16;
        fn GetDC(hwnd: isize) -> isize;
        fn ReleaseDC(hwnd: isize, dc: isize) -> i32;
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn GetPixel(dc: isize, x: i32, y: i32) -> u32;
    }

    /// sRGB color of the screen pixel under the mouse cursor.
    pub fn color_under_cursor() -> Option<[u8; 3]> {
        // SAFETY: plain Win32 calls on the desktop DC, released right after.
        unsafe {
            let mut p = Point { x: 0, y: 0 };
            if GetCursorPos(&mut p) == 0 {
                return None;
            }
            let dc = GetDC(0);
            if dc == 0 {
                return None;
            }
            let c = GetPixel(dc, p.x, p.y);
            ReleaseDC(0, dc);
            (c != u32::MAX).then_some([(c & 0xff) as u8, ((c >> 8) & 0xff) as u8, ((c >> 16) & 0xff) as u8])
        }
    }

    pub fn left_button_down() -> bool {
        // SAFETY: plain Win32 call.
        unsafe { (GetAsyncKeyState(0x01) as u16 & 0x8000) != 0 }
    }
}

#[cfg(not(windows))]
mod screen {
    pub fn color_under_cursor() -> Option<[u8; 3]> {
        None
    }
    pub fn left_button_down() -> bool {
        false
    }
}

fn srgb_to_lin(c: [u8; 3]) -> [f32; 3] {
    let r = egui::Rgba::from(Color32::from_rgb(c[0], c[1], c[2]));
    [r.r(), r.g(), r.b()]
}

impl RetroApp {
    /// Raw input while armed: the left press (and its release) is kept from
    /// egui and becomes the pick; a right press cancels.
    pub(super) fn eyedrop_filter_input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let armed = ctx.data(|d| d.get_temp::<egui::Id>(widgets::eyedropper_id())).is_some();
        if !armed && !self.eyedrop_swallow {
            return;
        }
        let mut keep = Vec::with_capacity(raw.events.len());
        for e in raw.events.drain(..) {
            match e {
                egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: true, pos, .. } if armed => {
                    self.eyedrop_press = Some(pos);
                    self.eyedrop_swallow = true;
                }
                egui::Event::PointerButton { button: egui::PointerButton::Secondary, pressed: true, .. } if armed => {
                    ctx.data_mut(|d| d.remove::<egui::Id>(widgets::eyedropper_id()));
                    self.status = "Eyedropper cancelled.".into();
                    self.eyedrop_swallow = true;
                }
                egui::Event::PointerButton { pressed: false, .. } if self.eyedrop_swallow => {
                    self.eyedrop_swallow = false;
                }
                e => keep.push(e),
            }
        }
        raw.events = keep;
    }

    /// Each frame while armed: live color, the pick, the swatch at the cursor.
    pub(super) fn eyedrop_frame(&mut self, ctx: &egui::Context) {
        let Some(owner) = ctx.data(|d| d.get_temp::<egui::Id>(widgets::eyedropper_id())) else {
            self.eyedrop_live = None;
            self.eyedrop_press = None;
            return;
        };
        // Keep polling the mouse while it is outside the window.
        ctx.request_repaint();
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        let inside = ctx.input(|i| i.pointer.hover_pos());
        let pressed_at = self.eyedrop_press.take();
        let at = pressed_at.or(inside);
        let over_mesh = at.filter(|p| self.vp_rect.contains(*p) && self.doc.is_some());
        let live = match over_mesh {
            Some(p) => self.sample_color(glam::Vec2::new(p.x - self.vp_rect.min.x, p.y - self.vp_rect.min.y)),
            None => None,
        }
        .or_else(|| screen::color_under_cursor().map(srgb_to_lin));
        self.eyedrop_live = live;
        ctx.data_mut(|d| match live {
            Some(c) => d.insert_temp(egui::Id::new("eyedropper live"), c),
            None => d.remove::<[f32; 3]>(egui::Id::new("eyedropper live")),
        });
        // Outside the window egui sees no click: watch the button itself.
        let down = screen::left_button_down();
        let outside_click = inside.is_none() && down && !self.eyedrop_was_down;
        self.eyedrop_was_down = down;
        if pressed_at.is_some() || outside_click {
            if let Some(c) = live {
                ctx.data_mut(|d| {
                    d.insert_temp(widgets::eyedropper_result_id(), (owner, c));
                    d.remove::<egui::Id>(widgets::eyedropper_id());
                });
                self.status = "Color picked.".into();
            }
            return;
        }
        // Swatch next to the cursor (inside the window).
        if let (Some(p), Some(c)) = (inside, live) {
            let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("eyedropper swatch"));
            let painter = ctx.layer_painter(layer);
            let r = Rect::from_min_size(p + egui::vec2(16.0, 16.0), egui::vec2(30.0, 30.0));
            let col: Color32 = egui::Rgba::from_rgb(c[0].max(0.0), c[1].max(0.0), c[2].max(0.0)).into();
            painter.rect_filled(r.expand(2.0), 3.0, Color32::from_gray(20));
            painter.rect_filled(r, 2.0, col);
            painter.rect_stroke(r.expand(2.0), 3.0, egui::Stroke::new(1.0f32, Color32::from_gray(200)));
        }
    }
}
