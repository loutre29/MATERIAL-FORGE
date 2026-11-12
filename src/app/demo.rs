//! `MF_DEMO=<script.json>`: plays a scripted session in the real window,
//! for showcase videos. The script moves a drawn cursor to widgets found by
//! their text (or an icon button's tooltip), clicks, types, drops files and
//! drags the camera, all as egui input inside the app: the computer's own
//! mouse and keyboard are never driven, and their input is ignored while
//! the demo plays. `MF_DEMO_OUT=<video.mp4>` records every frame at a
//! steady 30 frames per second (through ffmpeg), whatever the computer's
//! speed; waits for long work (Claude, a bake, a render) are recorded as a
//! time lapse.
//!
//! A script is a JSON array of steps:
//! - `{"title": "...", "sub": "...", "dur": 3}`: a title card;
//! - `{"caption": "...", "dur": 4}`: a caption at the bottom (the next step
//!   starts at once);
//! - `{"wait": 1.5}`;
//! - `{"move": TARGET}`, `{"click": TARGET}`, `{"double": TARGET}`;
//! - `{"drop": "<file>", "to": TARGET}`: a file dragged in from the left
//!   edge and dropped;
//! - `{"type": "text", "cps": 14}`, `{"key": "Enter", "mods": "ctrl"}`;
//! - `{"drag": TARGET, "by": [[dx, dy], ...], "mods": "alt", "dur": 2}`;
//! - `{"scroll": -120, "at": TARGET}`;
//! - `{"until": "muse_idle" | "doc" | "studio_idle" | "samples:N" |
//!   "text:..." , "timeout": 900, "lapse": 0.5}`: waits, one frame recorded
//!   every `lapse` seconds (0: real time);
//! - `{"end": true}`: closes the video and quits.
//!
//! A TARGET is `{"text": "Create"}` (exact), `{"contains": "..."}`,
//! `{"icon": "Packshot studio"}` (a tooltip's start), or `{"rel": [x, y]}`
//! (fractions of the window); with optional `"nth"`, `"last": true`,
//! `"region": [x0, y0, x1, y1]` (fractions), `"dx"` / `"dy"` (pixels).

use super::*;
use eframe::egui::{epaint, Color32, Event, LayerId, Modifiers, PointerButton, Pos2, Rect, Shape, Vec2, ViewportCommand};
use serde_json::Value;
use std::io::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// A demo is playing: icon buttons note where they are.
pub static ACTIVE: AtomicBool = AtomicBool::new(false);
static ICONS: Mutex<Vec<(String, Rect)>> = Mutex::new(Vec::new());

/// An icon button's place this frame (its tooltip names it).
pub fn note_icon(tip: &str, rect: Rect) {
    if ACTIVE.load(Ordering::Relaxed) {
        if let Ok(mut v) = ICONS.lock() {
            v.push((tip.to_string(), rect));
        }
    }
}

const FPS: f64 = 30.0;

pub struct Director {
    steps: Vec<Value>,
    i: usize,
    /// The demo's clock (seconds), one frame at a time.
    t: f64,
    step_t: f64,
    phase: u32,
    cursor: Pos2,
    from: Pos2,
    to: Option<Pos2>,
    path: Vec<Pos2>,
    dur: f64,
    typed: usize,
    next_key: f64,
    held: Modifiers,
    pressed: bool,
    ripple: Option<(Pos2, f64)>,
    ghost: Option<String>,
    caption: Option<(String, f64, f64)>,
    title: Option<(String, String, f64, f64)>,
    /// What the last frame showed: texts (order, rect, text) and icons.
    texts: Vec<(u8, Rect, String)>,
    icons: Vec<(String, Rect)>,
    screen: Rect,
    rng: u64,
    /// Recording: ffmpeg's input, the frame size, the time lapse.
    video: Option<(std::process::Child, std::process::ChildStdin)>,
    size: Option<[usize; 2]>,
    lapse: f64,
    last_write: Option<std::time::Instant>,
    written: u64,
    lapse_from: Option<u64>,
    log: Vec<String>,
    started: std::time::Instant,
    done: bool,
}

impl Director {
    /// The demo named by MF_DEMO, if any.
    pub fn from_env() -> Option<Box<Self>> {
        let p = std::env::var_os("MF_DEMO")?;
        let text = std::fs::read_to_string(&p).map_err(|e| eprintln!("demo: {e}")).ok()?;
        let steps: Vec<Value> = serde_json::from_str(&text).map_err(|e| eprintln!("demo script: {e}")).ok()?;
        ACTIVE.store(true, Ordering::Relaxed);
        Some(Box::new(Director {
            steps,
            i: 0,
            t: 0.0,
            step_t: 0.0,
            phase: 0,
            cursor: Pos2::new(960.0, 600.0),
            from: Pos2::ZERO,
            to: None,
            path: Vec::new(),
            dur: 0.0,
            typed: 0,
            next_key: 0.0,
            held: Modifiers::NONE,
            pressed: false,
            ripple: None,
            ghost: None,
            caption: None,
            title: None,
            texts: Vec::new(),
            icons: Vec::new(),
            screen: Rect::from_min_size(Pos2::ZERO, Vec2::new(1920.0, 1080.0)),
            rng: 0x9E37_79B9_7F4A_7C15,
            video: None,
            size: None,
            lapse: 0.0,
            last_write: None,
            written: 0,
            lapse_from: None,
            log: Vec::new(),
            started: std::time::Instant::now(),
            done: false,
        }))
    }

    fn rand(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    fn say(&mut self, s: String) {
        let line = format!("[{:7.2}s video {:6.2}s] {s}", self.started.elapsed().as_secs_f64(), self.written as f64 / FPS);
        eprintln!("demo {line}");
        self.log.push(line);
    }

    /// A target's point on screen, if it shows.
    fn find(&self, spec: &Value) -> Option<Pos2> {
        let s = self.screen;
        let off = Vec2::new(spec["dx"].as_f64().unwrap_or(0.0) as f32, spec["dy"].as_f64().unwrap_or(0.0) as f32);
        if let Some(r) = spec["rel"].as_array() {
            let f = |i: usize| r.get(i).and_then(Value::as_f64).unwrap_or(0.5) as f32;
            return Some(s.min + Vec2::new(f(0) * s.width(), f(1) * s.height()) + off);
        }
        let region = spec["region"].as_array().map(|r| {
            let f = |i: usize| r.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
            Rect::from_min_max(s.min + Vec2::new(f(0) * s.width(), f(1) * s.height()), s.min + Vec2::new(f(2) * s.width(), f(3) * s.height()))
        });
        let inside = |r: &Rect| r.intersects(s) && region.map_or(true, |g| g.contains(r.center()));
        let mut hits: Vec<(u8, Rect)> = if let Some(tip) = spec["icon"].as_str() {
            self.icons.iter().filter(|(t, r)| t.starts_with(tip) && inside(r)).map(|(_, r)| (9, *r)).collect()
        } else if let Some(t) = spec["text"].as_str() {
            self.texts.iter().filter(|(_, r, x)| x.trim() == t && inside(r)).map(|(o, r, _)| (*o, *r)).collect()
        } else if let Some(t) = spec["contains"].as_str() {
            self.texts.iter().filter(|(_, r, x)| x.contains(t) && inside(r)).map(|(o, r, _)| (*o, *r)).collect()
        } else {
            Vec::new()
        };
        // The topmost first, then top to bottom.
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.min.y.total_cmp(&b.1.min.y)).then(a.1.min.x.total_cmp(&b.1.min.x)));
        let top = hits.first()?.0;
        hits.retain(|h| h.0 == top);
        let r = if spec["last"].as_bool() == Some(true) { hits.last()?.1 } else { hits.get(spec["nth"].as_u64().unwrap_or(0) as usize)?.1 };
        Some(r.center() + off)
    }

    /// A human path from the cursor to `to`: a gentle curve.
    fn plan_move(&mut self, to: Pos2, dur: Option<f64>) {
        self.from = self.cursor;
        self.to = Some(to);
        let d = (to - self.from).length() as f64;
        self.dur = dur.unwrap_or(0.35 + d / 1700.0).max(0.12);
        // The bend: a little to the side, as a wrist turns.
        let bend = (self.rand() - 0.5) * 0.25;
        let mid = self.from + (to - self.from) * 0.5;
        let n = (to - self.from).rot90().normalized() * (d as f32 * bend as f32);
        self.path = vec![self.from, mid + n, to];
    }

    fn along(&self, k: f64) -> Pos2 {
        // Minimum jerk: slow start, fast middle, slow arrival.
        let k = k.clamp(0.0, 1.0);
        let e = (10.0 * k.powi(3) - 15.0 * k.powi(4) + 6.0 * k.powi(5)) as f32;
        let [a, b, c] = [self.path[0], self.path[1], self.path[2]];
        let ab = a + (b - a) * e;
        let bc = b + (c - b) * e;
        ab + (bc - ab) * e
    }

    fn next(&mut self) {
        self.i += 1;
        self.phase = 0;
        self.step_t = self.t;
        self.typed = 0;
    }

    fn pointer(&self, button: PointerButton, pressed: bool) -> Event {
        Event::PointerButton { pos: self.cursor, button, pressed, modifiers: self.held }
    }

    fn mods(spec: &Value) -> Modifiers {
        let m = spec["mods"].as_str().unwrap_or("");
        Modifiers { alt: m.contains("alt"), ctrl: m.contains("ctrl"), shift: m.contains("shift"), command: m.contains("ctrl"), mac_cmd: false }
    }

    /// This frame's input from the script (`app` for the waits).
    fn step(&mut self, app: &RetroApp, raw: &mut egui::RawInput) {
        let ev = &mut raw.events;
        let Some(s) = self.steps.get(self.i).cloned() else {
            return;
        };
        let el = self.t - self.step_t;
        if let Some(d) = s["wait"].as_f64() {
            if el >= d {
                self.next();
            }
        } else if let Some(t) = s["title"].as_str() {
            let d = s["dur"].as_f64().unwrap_or(3.0);
            if self.phase == 0 {
                self.title = Some((t.to_string(), s["sub"].as_str().unwrap_or("").to_string(), self.t, self.t + d));
                self.phase = 1;
            }
            if el >= d {
                self.next();
            }
        } else if let Some(t) = s["caption"].as_str() {
            let d = s["dur"].as_f64().unwrap_or(4.0);
            self.caption = Some((t.to_string(), self.t, self.t + d));
            self.next();
        } else if s.get("move").is_some() || s.get("click").is_some() || s.get("double").is_some() {
            let spec = s.get("move").or(s.get("click")).or(s.get("double")).unwrap().clone();
            let clicks = if s.get("double").is_some() { 2 } else if s.get("click").is_some() { 1 } else { 0 };
            let button = if s["button"].as_str() == Some("right") { PointerButton::Secondary } else { PointerButton::Primary };
            match self.phase {
                0 => {
                    if let Some(p) = self.find(&spec) {
                        self.plan_move(p, s["dur"].as_f64());
                        self.phase = 1;
                        self.step_t = self.t;
                    } else if el > s["timeout"].as_f64().unwrap_or(20.0) {
                        self.say(format!("target not found, skipped: {spec}"));
                        self.next();
                    }
                }
                1 => {
                    let k = el / self.dur;
                    self.cursor = self.along(k);
                    ev.push(Event::PointerMoved(self.cursor));
                    if k >= 1.0 {
                        // The target may have moved while the cursor came.
                        if let Some(p) = self.find(&spec).filter(|p| (*p - self.cursor).length() > 3.0 && (*p - self.cursor).length() < 300.0) {
                            self.cursor = p;
                            ev.push(Event::PointerMoved(p));
                        }
                        self.phase = if clicks == 0 { 9 } else { 2 };
                        self.step_t = self.t;
                    }
                }
                2..=5 => {
                    // Settle, press, hold, release (twice for a double click).
                    let gap = if self.phase == 2 { 0.12 } else { 0.07 };
                    if el >= gap {
                        let press = self.phase % 2 == 0;
                        ev.push(self.pointer(button, press));
                        if press {
                            self.ripple = Some((self.cursor, self.t));
                        }
                        self.phase += 1;
                        self.step_t = self.t;
                        if self.phase == 4 && clicks == 1 {
                            self.phase = 9;
                        }
                        if self.phase == 6 {
                            self.phase = 9;
                        }
                    }
                }
                _ => {
                    if el >= s["after"].as_f64().unwrap_or(0.3) {
                        self.next();
                    }
                }
            }
        } else if let Some(file) = s["drop"].as_str() {
            let path = std::path::PathBuf::from(file);
            match self.phase {
                0 => {
                    if let Some(p) = self.find(&s["to"]) {
                        // From the left edge of the window (the file
                        // explorer beside it), at the height of the target.
                        self.cursor = Pos2::new(self.screen.min.x + 2.0, (p.y - 120.0).max(40.0));
                        self.plan_move(p, Some(s["dur"].as_f64().unwrap_or(1.3)));
                        self.ghost = Some(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                        self.phase = 1;
                        self.step_t = self.t;
                    } else if el > 20.0 {
                        self.say(format!("drop target not found, skipped: {}", s["to"]));
                        self.next();
                    }
                }
                1 => {
                    let k = el / self.dur;
                    self.cursor = self.along(k);
                    ev.push(Event::PointerMoved(self.cursor));
                    raw.hovered_files.push(egui::HoveredFile { path: Some(path.clone()), mime: String::new() });
                    if k >= 1.0 {
                        self.phase = 2;
                        self.step_t = self.t;
                    }
                }
                2 => {
                    raw.hovered_files.push(egui::HoveredFile { path: Some(path.clone()), mime: String::new() });
                    if el >= 0.35 {
                        raw.hovered_files.clear();
                        raw.dropped_files.push(egui::DroppedFile { path: Some(path.clone()), name: String::new(), mime: String::new(), last_modified: None, bytes: None });
                        self.ghost = None;
                        self.ripple = Some((self.cursor, self.t));
                        self.phase = 3;
                        self.step_t = self.t;
                    }
                }
                _ => {
                    if el >= 0.4 {
                        self.next();
                    }
                }
            }
        } else if let Some(text) = s["type"].as_str() {
            let chars: Vec<char> = text.chars().collect();
            let cps = s["cps"].as_f64().unwrap_or(14.0);
            if self.typed == 0 && self.phase == 0 {
                self.next_key = self.t + 0.2;
                self.phase = 1;
            }
            while self.typed < chars.len() && self.t >= self.next_key {
                let c = chars[self.typed];
                ev.push(Event::Text(c.to_string()));
                self.typed += 1;
                // A human rhythm: quicker inside words, a pause after them.
                let base = 1.0 / cps;
                let pause = if c == ' ' { base * 0.8 } else if ",.".contains(c) { base * 3.0 } else { 0.0 };
                self.next_key = self.t + base * (0.55 + self.rand() * 0.9) + pause;
            }
            if self.typed >= chars.len() && self.t >= self.next_key + 0.3 {
                self.next();
            }
        } else if let Some(k) = s["key"].as_str() {
            let key = egui::Key::from_name(k);
            let m = Self::mods(&s);
            match (key, self.phase) {
                (Some(key), 0) => {
                    ev.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: m });
                    self.phase = 1;
                }
                (Some(key), 1) => {
                    ev.push(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: m });
                    self.phase = 2;
                    self.step_t = self.t;
                }
                (Some(_), _) => {
                    if el >= s["after"].as_f64().unwrap_or(0.3) {
                        self.next();
                    }
                }
                (None, _) => {
                    self.say(format!("unknown key {k}"));
                    self.next();
                }
            }
        } else if let Some(spec) = s.get("drag").cloned() {
            let m = Self::mods(&s);
            match self.phase {
                0 => {
                    if let Some(p) = self.find(&spec) {
                        self.plan_move(p, None);
                        self.phase = 1;
                        self.step_t = self.t;
                    } else if el > 20.0 {
                        self.next();
                    }
                }
                1 => {
                    let k = el / self.dur;
                    self.cursor = self.along(k);
                    ev.push(Event::PointerMoved(self.cursor));
                    if k >= 1.0 {
                        self.phase = 2;
                        self.step_t = self.t;
                    }
                }
                2 => {
                    if el >= 0.15 {
                        self.held = m;
                        raw.modifiers = m;
                        ev.push(self.pointer(PointerButton::Primary, true));
                        self.pressed = true;
                        // The offsets, joined smoothly.
                        let start = self.cursor;
                        let mut pts = vec![start];
                        let mut at = start;
                        for d in s["by"].as_array().into_iter().flatten() {
                            let d = d.as_array().cloned().unwrap_or_default();
                            at += Vec2::new(d.first().and_then(Value::as_f64).unwrap_or(0.0) as f32, d.get(1).and_then(Value::as_f64).unwrap_or(0.0) as f32);
                            pts.push(at);
                        }
                        self.path = pts;
                        self.dur = s["dur"].as_f64().unwrap_or(2.0);
                        self.phase = 3;
                        self.step_t = self.t;
                    }
                }
                3 => {
                    let k = (el / self.dur).clamp(0.0, 1.0);
                    let e = 0.5 - 0.5 * (std::f64::consts::PI * k).cos();
                    let n = self.path.len() - 1;
                    let f = e * n as f64;
                    let j = (f.floor() as usize).min(n.saturating_sub(1));
                    let (a, b) = (self.path[j], self.path[(j + 1).min(n)]);
                    self.cursor = a + (b - a) * (f - j as f64) as f32;
                    raw.modifiers = self.held;
                    ev.push(Event::PointerMoved(self.cursor));
                    if k >= 1.0 {
                        ev.push(self.pointer(PointerButton::Primary, false));
                        self.pressed = false;
                        self.phase = 4;
                        self.step_t = self.t;
                    }
                }
                _ => {
                    self.held = Modifiers::NONE;
                    if el >= 0.3 {
                        self.next();
                    }
                }
            }
        } else if let Some(dy) = s["scroll"].as_f64() {
            match self.phase {
                0 => {
                    if let Some(p) = s.get("at").and_then(|t| self.find(t)) {
                        self.plan_move(p, None);
                    } else {
                        let c = self.cursor;
                        self.plan_move(c, Some(0.01));
                    }
                    self.phase = 1;
                    self.step_t = self.t;
                }
                1 => {
                    let k = el / self.dur;
                    self.cursor = self.along(k);
                    ev.push(Event::PointerMoved(self.cursor));
                    if k >= 1.0 {
                        self.phase = 2;
                        self.step_t = self.t;
                    }
                }
                2 => {
                    // Notches over a short while, as a wheel turns.
                    let d = s["dur"].as_f64().unwrap_or(0.8);
                    let per = dy / (d * FPS);
                    ev.push(Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: Vec2::new(0.0, per as f32), modifiers: self.held });
                    if el >= d {
                        self.phase = 3;
                        self.step_t = self.t;
                    }
                }
                _ => {
                    if el >= 0.3 {
                        self.next();
                    }
                }
            }
        } else if let Some(name) = s["dump"].as_str() {
            // What shows (texts and icon tooltips), to write the script.
            let mut out: Vec<String> = self.texts.iter().map(|(o, r, t)| format!("text o{o} [{:.0},{:.0} {:.0}x{:.0}] {t:?}", r.min.x, r.min.y, r.width(), r.height())).collect();
            out.extend(self.icons.iter().map(|(t, r)| format!("icon [{:.0},{:.0} {:.0}x{:.0}] {t:?}", r.min.x, r.min.y, r.width(), r.height())));
            let dir = std::env::var_os("MF_DEMO_OUT").map(|o| std::path::PathBuf::from(o).with_file_name("")).unwrap_or_default();
            let _ = std::fs::write(dir.join(format!("{name}.txt")), out.join("\n"));
            self.next();
        } else if let Some(cond) = s["until"].as_str() {
            let lapse = s["lapse"].as_f64().unwrap_or(0.5);
            if self.phase == 0 {
                self.phase = 1;
                self.lapse = lapse;
                self.lapse_from = Some(self.written);
                self.say(format!("waiting for {cond}"));
            }
            let busy = app.muse_test_state().0 || app.bake_job.is_some();
            let met = match cond {
                "muse_idle" => !busy && app.studio_job.is_none(),
                "doc" => app.doc.is_some() && app.new_dialog.is_none(),
                "studio_idle" => app.studio_job.is_none(),
                c if c.starts_with("samples:") => c[8..].parse::<u32>().is_ok_and(|n| app.engine.pathtracer.frame >= n),
                c if c.starts_with("text:") => self.texts.iter().any(|(_, _, t)| t.contains(&c[5..])),
                _ => true,
            };
            // A short settle, so the result shows before the next step.
            if met && el > 0.5 {
                self.lapse = 0.0;
                if let Some(f) = self.lapse_from.take() {
                    let wall = self.started.elapsed().as_secs_f64();
                    self.say(format!("{cond} after a time lapse of {} frames (wall {:.0} s)", self.written - f, wall));
                }
                self.next();
            } else if self.started.elapsed().as_secs_f64() > 3600.0 || el > s["timeout"].as_f64().unwrap_or(1800.0) {
                self.say(format!("gave up waiting for {cond}"));
                self.lapse = 0.0;
                self.next();
            }
        } else if s["end"].as_bool() == Some(true) {
            self.done = true;
        } else {
            self.say(format!("unknown step {s}"));
            self.next();
        }
    }

    /// The overlays: the cursor, a click's ripple, the dragged file, the
    /// caption and the title card.
    fn paint(&self, ctx: &egui::Context) {
        let p = ctx.layer_painter(LayerId::debug());
        let t = self.t;
        let fade = |a: f64, b: f64, ramp: f64| (((t - a) / ramp).min((b - t) / ramp)).clamp(0.0, 1.0) as f32;
        if let Some((title, sub, a, b)) = &self.title {
            let k = fade(*a, *b, 0.5);
            if k > 0.0 {
                let r = self.screen;
                p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(14, 15, 18, (k * 255.0) as u8));
                let c = |v: u8| Color32::from_rgba_unmultiplied(v, v, v, (k * 255.0) as u8);
                p.text(r.center() - Vec2::new(0.0, 24.0), egui::Align2::CENTER_CENTER, title, egui::FontId::proportional(58.0), c(245));
                if !sub.is_empty() {
                    p.text(r.center() + Vec2::new(0.0, 40.0), egui::Align2::CENTER_CENTER, sub, egui::FontId::proportional(24.0), c(170));
                }
                let a8 = (k * 255.0) as u8;
                let bar = Rect::from_center_size(r.center() + Vec2::new(0.0, 8.0), Vec2::new(64.0, 3.0));
                let acc = crate::style::ACCENT;
                p.rect_filled(bar, 1.5, Color32::from_rgba_unmultiplied(acc.r(), acc.g(), acc.b(), a8));
            }
        }
        if let Some((text, a, b)) = &self.caption {
            let k = fade(*a, *b, 0.35);
            if k > 0.0 {
                let font = egui::FontId::proportional(22.0);
                let g = ctx.fonts(|f| f.layout_no_wrap(text.clone(), font.clone(), Color32::WHITE));
                let size = g.size() + Vec2::new(40.0, 22.0);
                let r = Rect::from_center_size(Pos2::new(self.screen.center().x, self.screen.max.y - 70.0), size);
                p.rect_filled(r, 12.0, Color32::from_rgba_unmultiplied(12, 13, 16, (k * 215.0) as u8));
                p.text(r.center(), egui::Align2::CENTER_CENTER, text, font, Color32::from_rgba_unmultiplied(255, 255, 255, (k * 255.0) as u8));
            }
        }
        // Fast forward: two triangles and the speed, top right.
        if self.lapse > 0.0 {
            let r = Rect::from_min_size(Pos2::new(self.screen.max.x - 118.0, self.screen.min.y + 64.0), Vec2::new(96.0, 34.0));
            p.rect_filled(r, 17.0, Color32::from_rgba_unmultiplied(12, 13, 16, 200));
            let c = r.left_center() + Vec2::new(18.0, 0.0);
            for k in 0..2 {
                let x = c.x + k as f32 * 11.0;
                p.add(Shape::convex_polygon(vec![Pos2::new(x, c.y - 7.0), Pos2::new(x + 10.0, c.y), Pos2::new(x, c.y + 7.0)], Color32::WHITE, egui::Stroke::NONE));
            }
            let speed = (self.lapse * FPS).round();
            p.text(r.left_center() + Vec2::new(46.0, 0.0), egui::Align2::LEFT_CENTER, format!("{speed:.0}x"), egui::FontId::proportional(17.0), Color32::WHITE);
        }
        if let Some((at, t0)) = self.ripple {
            let k = ((t - t0) / 0.45) as f32;
            if (0.0..1.0).contains(&k) {
                p.circle_stroke(at, 6.0 + 18.0 * k, egui::Stroke::new(2.5f32, Color32::from_rgba_unmultiplied(255, 255, 255, ((1.0 - k) * 200.0) as u8)));
            }
        }
        if let Some(name) = &self.ghost {
            let font = egui::FontId::proportional(14.0);
            let g = ctx.fonts(|f| f.layout_no_wrap(name.clone(), font.clone(), Color32::WHITE));
            let r = Rect::from_min_size(self.cursor + Vec2::new(16.0, 18.0), g.size() + Vec2::new(40.0, 14.0));
            p.rect_filled(r, 6.0, Color32::from_rgba_unmultiplied(40, 110, 200, 200));
            let icon = Rect::from_min_size(r.min + Vec2::new(8.0, 5.0), Vec2::new(12.0, r.height() - 10.0));
            p.rect_filled(icon, 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 230));
            p.text(r.min + Vec2::new(28.0, 7.0), egui::Align2::LEFT_TOP, name, font, Color32::WHITE);
        }
        // The arrow cursor.
        let s = 1.15;
        let pts: Vec<Pos2> = [(0.0, 0.0), (0.0, 17.0), (4.2, 13.2), (7.2, 19.8), (10.0, 18.6), (7.1, 12.2), (12.4, 12.2)].iter().map(|(x, y)| self.cursor + Vec2::new(x * s, y * s)).collect();
        p.add(Shape::convex_polygon(pts[..4].to_vec(), Color32::WHITE, egui::Stroke::NONE));
        p.add(Shape::convex_polygon(vec![pts[0], pts[3], pts[4], pts[5], pts[6]], Color32::WHITE, egui::Stroke::NONE));
        p.add(Shape::closed_line(pts, egui::Stroke::new(1.2f32, Color32::BLACK)));
    }

    /// Writes a frame (raw RGBA) to ffmpeg.
    fn write(&mut self, img: &egui::ColorImage) {
        let Some(out) = std::env::var_os("MF_DEMO_OUT") else { return };
        if self.video.is_none() {
            let [w, h] = img.size;
            self.size = Some(img.size);
            let child = std::process::Command::new(std::env::var("MF_FFMPEG").unwrap_or_else(|_| "ffmpeg".into()))
                .args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{w}x{h}"), "-r", "30", "-i", "-", "-c:v", "libx264", "-preset", "veryfast", "-crf", "12", "-pix_fmt", "yuv420p"])
                .arg(&out)
                .stdin(std::process::Stdio::piped())
                .spawn();
            match child {
                Ok(mut c) => {
                    let stdin = c.stdin.take().unwrap();
                    self.video = Some((c, stdin));
                }
                Err(e) => {
                    eprintln!("demo: ffmpeg: {e}");
                    return;
                }
            }
        }
        if self.size != Some(img.size) {
            return;
        }
        // In a time lapse, one frame every `lapse` seconds of the clock.
        let now = std::time::Instant::now();
        if self.lapse > 0.0 && self.last_write.is_some_and(|l| now.duration_since(l).as_secs_f64() < self.lapse) {
            return;
        }
        self.last_write = Some(now);
        if let Some((_, w)) = &mut self.video {
            let bytes: &[u8] = bytemuck::cast_slice(&img.pixels);
            if w.write_all(bytes).is_ok() {
                self.written += 1;
            }
        }
    }

    fn finish(&mut self) {
        if let Some((mut c, w)) = self.video.take() {
            drop(w);
            let _ = c.wait();
        }
        if let Some(out) = std::env::var_os("MF_DEMO_OUT") {
            let _ = std::fs::write(std::path::PathBuf::from(out).with_extension("log"), self.log.join("\n"));
        }
    }
}

/// The painted texts of this frame (before egui hands them on).
fn texts(ctx: &egui::Context) -> Vec<(u8, Rect, String)> {
    fn walk(shape: &Shape, clip: Rect, order: u8, out: &mut Vec<(u8, Rect, String)>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, clip, order, out)),
            Shape::Text(t) => {
                let r = t.galley.rect.translate(t.pos.to_vec2()).intersect(clip);
                if r.is_positive() && !t.galley.text().trim().is_empty() {
                    out.push((order, r, t.galley.text().to_string()));
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    ctx.graphics(|g| {
        for (layer, list) in g.layers() {
            let order = layer.order as u8;
            for epaint::ClippedShape { clip_rect, shape } in list.all_entries() {
                walk(shape, *clip_rect, order, &mut out);
            }
        }
    });
    out
}

impl RetroApp {
    /// Before egui: the computer's input set aside, the script's in.
    pub(super) fn demo_input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let Some(mut d) = self.demo.take() else { return };
        for e in &raw.events {
            if let Event::Screenshot { image, .. } = e {
                d.write(image);
            }
        }
        raw.events.clear();
        raw.hovered_files.clear();
        raw.dropped_files.clear();
        raw.focused = true;
        raw.modifiers = d.held;
        if let Some(r) = raw.screen_rect {
            d.screen = r;
        }
        // Nothing moves until the first frame is on screen.
        if d.written > 0 || std::env::var_os("MF_DEMO_OUT").is_none() || d.t > 2.0 {
            d.step(self, raw);
        } else if d.t == 0.0 {
            raw.events.push(Event::PointerMoved(d.cursor));
        }
        raw.time = Some(d.t);
        raw.predicted_dt = (1.0 / FPS) as f32;
        d.t += 1.0 / FPS;
        if d.done {
            d.finish();
            std::process::exit(0);
        }
        let _ = ctx;
        self.demo = Some(d);
    }

    /// After the app's frame: what shows is noted, the overlays painted,
    /// the frame captured.
    pub(super) fn demo_frame(&mut self, ctx: &egui::Context) {
        let Some(d) = self.demo.as_mut() else { return };
        d.texts = texts(ctx);
        d.icons = ICONS.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
        d.paint(ctx);
        if std::env::var_os("MF_DEMO_OUT").is_some() {
            ctx.send_viewport_cmd(ViewportCommand::Screenshot);
        }
        ctx.request_repaint();
    }
}
