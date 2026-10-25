//! Eyedropper of the color fields: while armed the cursor is a crosshair
//! with a live swatch of the color under it; a click picks the material's
//! base color over the 3D view, the screen pixel anywhere else, even
//! outside the window (Windows: the desktop is read with GDI). The click
//! that picks never reaches the widget under it. Escape or a right click
//! cancels. Outside the window the swatch follows the cursor in a small
//! click-through, always-on-top window of its own (Windows).

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

    #[repr(C)]
    struct WndClass {
        style: u32,
        proc_: unsafe extern "system" fn(isize, u32, usize, isize) -> isize,
        cls_extra: i32,
        wnd_extra: i32,
        instance: isize,
        icon: isize,
        cursor: isize,
        background: isize,
        menu_name: *const u16,
        class_name: *const u16,
    }

    #[repr(C)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[link(name = "user32")]
    extern "system" {
        fn RegisterClassW(c: *const WndClass) -> u16;
        fn CreateWindowExW(ex: u32, class: *const u16, title: *const u16, style: u32, x: i32, y: i32, w: i32, h: i32, parent: isize, menu: isize, inst: isize, param: isize) -> isize;
        fn DefWindowProcW(h: isize, m: u32, w: usize, l: isize) -> isize;
        fn SetWindowPos(h: isize, after: isize, x: i32, y: i32, w: i32, h2: i32, flags: u32) -> i32;
        fn ShowWindow(h: isize, cmd: i32) -> i32;
        fn SetLayeredWindowAttributes(h: isize, key: u32, alpha: u8, flags: u32) -> i32;
        fn FillRect(dc: isize, r: *const Rect, brush: isize) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> isize;
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn CreateSolidBrush(c: u32) -> isize;
        fn DeleteObject(o: isize) -> i32;
    }

    unsafe extern "system" fn swatch_proc(h: isize, m: u32, w: usize, l: isize) -> isize {
        DefWindowProcW(h, m, w, l)
    }

    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    static SWATCH: AtomicIsize = AtomicIsize::new(0);
    static SHOWN: AtomicBool = AtomicBool::new(false);

    /// The swatch window, created on first use (never activated, clicks
    /// pass through it).
    fn swatch_window() -> isize {
        let h = SWATCH.load(Ordering::Relaxed);
        if h != 0 {
            return h;
        }
        let name: Vec<u16> = "MaterialForgeEyedropper\0".encode_utf16().collect();
        // SAFETY: a plain popup window of this thread's class; its messages
        // go through DefWindowProc (the app's event loop pumps them).
        unsafe {
            let inst = GetModuleHandleW(std::ptr::null());
            let class = WndClass {
                style: 0,
                proc_: swatch_proc,
                cls_extra: 0,
                wnd_extra: 0,
                instance: inst,
                icon: 0,
                cursor: 0,
                background: 0,
                menu_name: std::ptr::null(),
                class_name: name.as_ptr(),
            };
            RegisterClassW(&class);
            // Layered + transparent: click-through; tool window: no taskbar
            // button; no-activate: the app keeps the focus.
            let ex = 0x0008_0000 | 0x20 | 0x8 | 0x80 | 0x0800_0000;
            let h = CreateWindowExW(ex, name.as_ptr(), name.as_ptr(), 0x8000_0000, 0, 0, 8, 8, 0, 0, inst, 0);
            if h != 0 {
                SetLayeredWindowAttributes(h, 0, 255, 2);
                SWATCH.store(h, Ordering::Relaxed);
            }
            h
        }
    }

    /// Shows the swatch of `rgb` (sRGB) next to the cursor, `size` pixels.
    pub fn show_swatch(rgb: [u8; 3], size: i32) {
        let mut p = Point { x: 0, y: 0 };
        // SAFETY: Win32 calls on our own window and its DC.
        unsafe {
            if GetCursorPos(&mut p) == 0 {
                return;
            }
            let h = swatch_window();
            if h == 0 {
                return;
            }
            let off = size / 2;
            SetWindowPos(h, -1, p.x + off, p.y + off, size, size, 0x10 | 0x40);
            SHOWN.store(true, Ordering::Relaxed);
            let dc = GetDC(h);
            if dc == 0 {
                return;
            }
            let fill = |r: Rect, c: u32| {
                let b = CreateSolidBrush(c);
                FillRect(dc, &r, b);
                DeleteObject(b);
            };
            fill(Rect { left: 0, top: 0, right: size, bottom: size }, 0x00c8_c8c8);
            fill(Rect { left: 1, top: 1, right: size - 1, bottom: size - 1 }, 0x0014_1414);
            fill(Rect { left: 3, top: 3, right: size - 3, bottom: size - 3 }, rgb[0] as u32 | (rgb[1] as u32) << 8 | (rgb[2] as u32) << 16);
            ReleaseDC(h, dc);
        }
    }

    pub fn hide_swatch() {
        if SHOWN.swap(false, Ordering::Relaxed) {
            // SAFETY: hides our own window.
            unsafe {
                ShowWindow(SWATCH.load(Ordering::Relaxed), 0);
            }
        }
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
    pub fn show_swatch(_rgb: [u8; 3], _size: i32) {}
    pub fn hide_swatch() {}
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
            screen::hide_swatch();
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
            screen::hide_swatch();
            return;
        }
        // Outside the window: the swatch in its own window at the cursor.
        match (inside, live) {
            (None, Some(c)) => {
                let s: Color32 = egui::Rgba::from_rgb(c[0].max(0.0), c[1].max(0.0), c[2].max(0.0)).into();
                screen::show_swatch([s.r(), s.g(), s.b()], (34.0 * ctx.pixels_per_point()).round() as i32);
            }
            _ => screen::hide_swatch(),
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
