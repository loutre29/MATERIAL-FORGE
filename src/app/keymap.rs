//! Single-key shortcuts (tools, brush, views), editable in Edit > Preferences:
//! click a key, press the new one. Saved as `keymap.json` next to the
//! panel layout. Ctrl and Alt combinations (save, undo, selections) stay
//! fixed.

use eframe::egui::{self, Key};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    Brush,
    Eraser,
    Projection,
    Path,
    Smudge,
    Clone,
    Marquee,
    MarqueeShape,
    Lasso,
    LassoShape,
    Wand,
    SelBrush,
    FillTool,
    FillSwitch,
    PolygonFill,
    Hide,
    Picker,
    SizeDown,
    SizeUp,
    Lazy,
    Symmetry,
    SwapValue,
    Frame,
    NextChannel,
    PrevChannel,
    NextMap,
    PrevMap,
    Split,
    Only3D,
    Only2D,
    SwapViews,
    Perspective,
    Ortho,
    Baking,
    Painting,
    Rendering,
}

/// A key, with or without Shift.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chord {
    pub key: Key,
    pub shift: bool,
}

impl Chord {
    const fn k(key: Key) -> Option<Chord> {
        Some(Chord { key, shift: false })
    }
    const fn sh(key: Key) -> Option<Chord> {
        Some(Chord { key, shift: true })
    }
    pub fn label(self) -> String {
        let k = self.key.symbol_or_name();
        if self.shift {
            format!("Shift + {k}")
        } else {
            k.to_string()
        }
    }
    fn save(self) -> String {
        format!("{}{}", if self.shift { "Shift+" } else { "" }, self.key.name())
    }
    fn parse(s: &str) -> Option<Chord> {
        let (shift, k) = match s.strip_prefix("Shift+") {
            Some(k) => (true, k),
            None => (false, s),
        };
        Key::from_name(k).map(|key| Chord { key, shift })
    }
}

/// (action, group, what it does, default key)
pub const ACTIONS: &[(Action, &str, &str, Option<Chord>)] = &[
    (Action::Brush, "Tools", "Brush", Chord::k(Key::B)),
    (Action::Projection, "Tools", "Projection", Chord::sh(Key::B)),
    (Action::Eraser, "Tools", "Eraser", Chord::k(Key::E)),
    (Action::Path, "Tools", "Path", Chord::k(Key::P)),
    (Action::Smudge, "Tools", "Smudge", Chord::k(Key::R)),
    (Action::Clone, "Tools", "Clone", Chord::k(Key::S)),
    (Action::Marquee, "Tools", "Rectangle / ellipse select", Chord::k(Key::M)),
    (Action::MarqueeShape, "Tools", "Switch rectangle and ellipse", Chord::sh(Key::M)),
    (Action::Lasso, "Tools", "Lasso / polygonal lasso", Chord::k(Key::L)),
    (Action::LassoShape, "Tools", "Switch lasso and polygonal lasso", Chord::sh(Key::L)),
    (Action::Wand, "Tools", "Magic wand", Chord::k(Key::W)),
    (Action::SelBrush, "Tools", "Selection brush", Chord::k(Key::Q)),
    (Action::FillTool, "Tools", "Paint bucket or gradient (last used)", Chord::k(Key::G)),
    (Action::FillSwitch, "Tools", "Switch paint bucket and gradient", Chord::sh(Key::G)),
    (Action::PolygonFill, "Tools", "Polygon fill", Chord::k(Key::U)),
    (Action::Hide, "Tools", "Hide faces (geometry mask)", Chord::k(Key::H)),
    (Action::Picker, "Tools", "Color picker (temporary)", Chord::k(Key::I)),
    (Action::SizeDown, "Brush", "Smaller brush", Chord::k(Key::OpenBracket)),
    (Action::SizeUp, "Brush", "Larger brush", Chord::k(Key::CloseBracket)),
    (Action::Lazy, "Brush", "Lazy mouse on / off", Chord::k(Key::D)),
    (Action::Symmetry, "Brush", "Symmetry on / off", Chord::k(Key::Y)),
    (Action::SwapValue, "Brush", "Swap mask black / white (selection brush: add / subtract)", Chord::k(Key::X)),
    (Action::Frame, "View", "Frame the mesh", Chord::k(Key::F)),
    (Action::NextChannel, "View", "Next channel (material first)", Chord::k(Key::C)),
    (Action::PrevChannel, "View", "Previous channel", Chord::sh(Key::C)),
    (Action::NextMap, "View", "Next mesh map", Chord::k(Key::N)),
    (Action::PrevMap, "View", "Previous mesh map", Chord::sh(Key::N)),
    (Action::Split, "View", "3D + 2D", Chord::k(Key::F1)),
    (Action::Only3D, "View", "3D only", Chord::k(Key::F2)),
    (Action::Only2D, "View", "2D only", Chord::k(Key::F3)),
    (Action::SwapViews, "View", "Swap the views", Chord::k(Key::F4)),
    (Action::Perspective, "View", "Perspective camera", Chord::k(Key::F5)),
    (Action::Ortho, "View", "Orthographic camera", Chord::k(Key::F6)),
    (Action::Baking, "View", "Baking", Chord::k(Key::F8)),
    (Action::Painting, "View", "Painting", Chord::k(Key::F9)),
    (Action::Rendering, "View", "Rendering", Chord::k(Key::F10)),
];

fn index(a: Action) -> usize {
    ACTIONS.iter().position(|e| e.0 == a).unwrap_or(0)
}

/// The key of every action (None: unbound).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Keymap {
    keys: [Option<Chord>; ACTIONS.len()],
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap { keys: std::array::from_fn(|i| ACTIONS[i].3) }
    }
}

fn path() -> std::path::PathBuf {
    crate::library::library_dir().join("..").join("keymap.json")
}

impl Keymap {
    /// The saved keymap (missing actions keep their default).
    pub fn load() -> Self {
        let mut k = Keymap::default();
        let saved: HashMap<String, String> = std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        for (i, (a, ..)) in ACTIONS.iter().enumerate() {
            if let Some(v) = saved.get(&format!("{a:?}")) {
                k.keys[i] = Chord::parse(v);
            }
        }
        k
    }

    pub fn save(&self) {
        let m: HashMap<String, String> = ACTIONS.iter().zip(&self.keys).map(|((a, ..), c)| (format!("{a:?}"), c.map(Chord::save).unwrap_or_default())).collect();
        if let Ok(s) = serde_json::to_string_pretty(&m) {
            let p = path();
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, s);
        }
    }

    pub fn get(&self, a: Action) -> Option<Chord> {
        self.keys[index(a)]
    }

    /// Binds `a` to `c`; the action that had `c` loses it (returned).
    pub fn set(&mut self, a: Action, c: Option<Chord>) -> Option<Action> {
        let mut taken = None;
        if let Some(c) = c {
            for (i, k) in self.keys.iter_mut().enumerate() {
                if *k == Some(c) && ACTIONS[i].0 != a {
                    *k = None;
                    taken = Some(ACTIONS[i].0);
                }
            }
        }
        self.keys[index(a)] = c;
        taken
    }

    /// The action's key, for labels ("" when unbound).
    pub fn label(&self, a: Action) -> String {
        self.get(a).map(Chord::label).unwrap_or_default()
    }

    /// Pressed this frame (no Ctrl or Alt held).
    pub fn pressed(&self, i: &egui::InputState, a: Action) -> bool {
        let Some(c) = self.get(a) else { return false };
        !i.modifiers.command && !i.modifiers.alt && i.modifiers.shift == c.shift && i.key_pressed(c.key)
    }
}

/// A key pressed this frame for the editor (Shift allowed, not Ctrl / Alt;
/// Escape is the cancel).
pub fn captured(i: &egui::InputState) -> Option<Chord> {
    i.events.iter().find_map(|e| match e {
        egui::Event::Key { key, pressed: true, modifiers, repeat: false, .. } if !modifiers.command && !modifiers.alt && *key != Key::Escape => {
            Some(Chord { key: *key, shift: modifiers.shift })
        }
        _ => None,
    })
}

pub fn name(a: Action) -> &'static str {
    ACTIONS[index(a)].2
}
