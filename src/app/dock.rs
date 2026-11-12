//! The canvas layout (docs/design/material_forge_design_system.html): the
//! views fill the window; rounded panels dock over them, Painter style
//! (by default layers top left, library bottom left, Claude top right and
//! properties under it; a header carries a panel to the other column,
//! or the library to the bottom); the tools live in one dock at the bottom
//! of the free canvas with their options right above it;
//! the view controls sit in a bar at the top center, Export top right.

use super::icons::{self, Icon};
use super::selection::{MapSel, SelShape};
use super::*;
use eframe::egui::{Color32, Pos2, RichText, Sense, Ui};

/// The panels (the names of the former dock tabs).
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Tab {
    Viewport,
    Assets,
    Properties,
    TextureSets,
    Layers,
    SetSettings,
    Muse,
    /// The packshot studio (shows while the studio is on).
    Studio,
}

impl Tab {
    pub const PANELS: [Tab; 7] = [Tab::Layers, Tab::Assets, Tab::Properties, Tab::TextureSets, Tab::SetSettings, Tab::Muse, Tab::Studio];

    pub fn name(self) -> &'static str {
        match self {
            Tab::Viewport => "Viewport",
            Tab::Assets => "Library",
            Tab::Properties => "Properties",
            Tab::TextureSets => "Texture sets",
            Tab::Layers => "Layers",
            Tab::SetSettings => "Texture set settings",
            Tab::Muse => "Claude",
            Tab::Studio => "Packshot studio",
        }
    }
}

/// Where a panel docks: the left or right column (stacked top down), or
/// the strip along the bottom between them (the library only).
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Slot {
    Left,
    Right,
    Bottom,
}

/// The docked panels, in `Panels` array order.
pub const PANEL_IDS: [&str; NP] = ["left", "library", "properties", "set settings", "muse", "studio"];
const NP: usize = 6;
const LAYERS: usize = 0;
const LIBRARY: usize = 1;
const MUSE: usize = 4;
const STUDIO: usize = 5;

/// Which panels show and where. `RetroApp::dock` None is the default
/// arrangement. Saved between sessions (`layout_v6.json`; a v5 one is
/// read with the studio panel added).
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Panels {
    /// Left panel: 0 layers, 1 texture sets, 2 history.
    pub left_tab: u8,
    pub left: bool,
    pub library: bool,
    pub properties: bool,
    /// The texture set settings panel.
    pub set_settings: bool,
    /// The Claude panel (texturing by prompt).
    pub muse: bool,
    /// Where each panel docks (layers, library, properties, set settings,
    /// Claude, studio).
    pub slots: [Slot; NP],
    /// Their order in a column, top first.
    pub order: [u8; NP],
    /// Their share of the column's height.
    pub weights: [f32; NP],
    /// Widths of the left and right columns.
    pub left_w: f32,
    pub right_w: f32,
    /// Height of the bottom strip.
    pub bottom_h: f32,
    /// Panels can neither move nor resize.
    pub locked: bool,
    /// Split view: the UV on the left, the 3D on the right (F4).
    pub swap_views: bool,
    /// Split view: where the divider sits (0..1).
    pub view_split: f32,
    /// The views shown: 0 3D, 1 split, 2 UV.
    pub view_layout: u8,
}

impl Default for Panels {
    fn default() -> Self {
        Self {
            left_tab: 0,
            left: true,
            library: true,
            properties: true,
            set_settings: false,
            muse: true,
            // Layers top left, library bottom left (set settings between
            // them when shown); Claude top right, properties under it.
            // The studio (while it is on) on top of the right column.
            slots: [Slot::Left, Slot::Left, Slot::Right, Slot::Left, Slot::Right, Slot::Right],
            order: [0, 2, 2, 1, 1, 0],
            weights: [1.0, 0.75, 0.62, 0.7, 0.38, 1.0],
            left_w: LEFT_W,
            right_w: RIGHT_W,
            bottom_h: BOTTOM_H,
            locked: false,
            swap_views: false,
            view_split: 0.5,
            view_layout: 0,
        }
    }
}

impl Panels {
    /// A panel's slot (only the library goes to the bottom).
    pub fn slot(&self, i: usize) -> Slot {
        match self.slots[i] {
            Slot::Bottom if i != LIBRARY => Slot::Right,
            s => s,
        }
    }

    /// The studio panel follows the studio (`RetroApp::canvas_ui`).
    fn shown(&self) -> [bool; NP] {
        [self.left, self.library, self.properties, self.set_settings, self.muse, false]
    }

    /// The shown panels of a column, top first.
    fn column(&self, s: Slot, shown: [bool; NP]) -> Vec<usize> {
        let mut v: Vec<usize> = (0..NP).filter(|&i| shown[i] && self.slot(i) == s).collect();
        v.sort_by_key(|&i| (self.order[i], i));
        v
    }

    /// Panel `i` docked to `s`, before panel `before` (None: last).
    pub fn docked(&self, i: usize, s: Slot, before: Option<usize>) -> Panels {
        let mut q = self.clone();
        let shown = self.shown();
        let others = self.column(s, shown).into_iter().filter(|&j| j != i).collect::<Vec<_>>();
        if self.slot(i) != s && !others.is_empty() {
            // It takes an average share of its new column.
            q.weights[i] = others.iter().map(|&j| self.weights[j].max(0.05)).sum::<f32>() / others.len() as f32;
        }
        q.slots[i] = s;
        let mut all: Vec<usize> = (0..NP).filter(|&j| j != i && q.slot(j) == s).collect();
        all.sort_by_key(|&j| (self.order[j], j));
        let at = before.and_then(|b| all.iter().position(|&j| j == b)).unwrap_or(all.len());
        all.insert(at, i);
        for (k, &j) in all.iter().enumerate() {
            q.order[j] = k as u8;
        }
        q
    }
}

fn layout_path() -> std::path::PathBuf {
    crate::library::library_dir().join("..").join("layout_v6.json")
}

/// The saved arrangement: v6, or v5 (five panels) with the studio panel
/// on top of the right column.
fn read_layout() -> Option<Panels> {
    if let Some(p) = std::fs::read_to_string(layout_path()).ok().and_then(|t| serde_json::from_str::<Panels>(&t).ok()) {
        return Some(p);
    }
    let old = crate::library::library_dir().join("..").join("layout_v5.json");
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(old).ok()?).ok()?;
    for (k, extra) in [("slots", serde_json::json!("Right")), ("order", serde_json::json!(0)), ("weights", serde_json::json!(1.0))] {
        let a = v.get_mut(k)?.as_array_mut()?;
        if a.len() == NP - 1 {
            a.push(extra);
        }
    }
    let mut p: Panels = serde_json::from_value(v).ok()?;
    // The right column's panels go one down, under the studio.
    for i in 0..STUDIO {
        if p.slot(i) == Slot::Right {
            p.order[i] = p.order[i].saturating_add(1);
        }
    }
    Some(p)
}

pub const LEFT_W: f32 = 272.0;
pub const RIGHT_W: f32 = 312.0;
pub const BOTTOM_H: f32 = 280.0;
/// Room left between stacked panels (their split grip).
const STACK_GAP: f32 = 8.0;
/// The smallest height of a stacked panel.
const MIN_PANEL_H: f32 = 120.0;
/// The Claude card's height (it does not stretch).
const MUSE_H: f32 = 150.0;

/// Where the shown panels go on `screen`.
pub(super) struct Placed {
    /// Each panel's rect (None: hidden).
    pub rects: [Option<Rect>; NP],
    /// The free canvas between them.
    pub mid: Rect,
    /// The left and right columns: their area and their panels, top first.
    pub cols: [(Rect, Vec<usize>); 2],
}

pub(super) fn place(p: &Panels, screen: Rect, shown: [bool; NP]) -> Placed {
    let g = crate::style::GAP;
    let (lw, rw) = (p.left_w.clamp(220.0, 520.0), p.right_w.clamp(260.0, 620.0));
    let left = p.column(Slot::Left, shown);
    let right = p.column(Slot::Right, shown);
    let bottom_on = shown[LIBRARY] && p.slot(LIBRARY) == Slot::Bottom;
    // The left column starts under the corner menu, which shows while the
    // layers panel (the menu's home) is hidden; the right one under the
    // Export bar.
    let ltop = screen.min.y + g + if shown[LAYERS] { 0.0 } else { 42.0 };
    let rtop = screen.min.y + g + 42.0;
    let bottom = screen.max.y - g;
    let lcol = Rect::from_min_max(Pos2::new(screen.min.x + g, ltop), Pos2::new(screen.min.x + g + lw, bottom));
    let rcol = Rect::from_min_max(Pos2::new(screen.max.x - g - rw, rtop), Pos2::new(screen.max.x - g, bottom));
    let mut rects = [None; NP];
    for (col, ids) in [(lcol, &left), (rcol, &right)] {
        // The Claude card keeps its small height; the others share the rest.
        let flex: Vec<usize> = ids.iter().copied().filter(|&i| i != MUSE).collect();
        let total: f32 = flex.iter().map(|&i| p.weights[i].max(0.05)).sum();
        let fixed = if ids.contains(&MUSE) { MUSE_H } else { 0.0 };
        let room = col.height() - STACK_GAP * ids.len().saturating_sub(1) as f32 - fixed;
        let mut y = col.min.y;
        for &i in ids.iter() {
            let y1 = if i == MUSE {
                y + MUSE_H
            } else if flex.last() == Some(&i) {
                // The last one ends at the column's bottom (minus a card under it).
                let under = ids.iter().skip_while(|&&j| j != i).skip(1).count();
                col.max.y - if under > 0 { MUSE_H + STACK_GAP } else { 0.0 }
            } else {
                (y + room * p.weights[i].max(0.05) / total).round()
            };
            rects[i] = Some(Rect::from_min_max(Pos2::new(col.min.x, y), Pos2::new(col.max.x, y1)));
            y = y1 + STACK_GAP;
        }
    }
    let mut mid_l = if left.is_empty() { screen.min.x } else { lcol.max.x };
    let mut mid_r = if right.is_empty() { screen.max.x } else { rcol.min.x };
    if mid_r - mid_l < 200.0 {
        (mid_l, mid_r) = (screen.min.x, screen.max.x);
    }
    let mut mid_b = screen.max.y;
    if bottom_on {
        let x0 = if left.is_empty() { screen.min.x + g } else { lcol.max.x + g };
        let x1 = if right.is_empty() { screen.max.x - g } else { rcol.min.x - g };
        let h = p.bottom_h.clamp(140.0, (screen.height() * 0.6).max(140.0));
        let r = Rect::from_min_max(Pos2::new(x0, bottom - h), Pos2::new(x1.max(x0 + 200.0), bottom));
        rects[LIBRARY] = Some(r);
        mid_b = r.min.y;
    }
    Placed {
        rects,
        mid: Rect::from_min_max(Pos2::new(mid_l, screen.min.y), Pos2::new(mid_r, mid_b)),
        cols: [(lcol, left), (rcol, right)],
    }
}

/// Where panel `i` docks when dropped at `pos`: the arrangement it gives
/// (None: over the canvas, it goes back where it was).
fn drop_target(p: &Panels, placed: &Placed, screen: Rect, i: usize, pos: Pos2) -> Option<Panels> {
    let g = crate::style::GAP;
    // A column catches its own width, or a band along the edge when empty.
    let band = 140.0;
    let in_left = if placed.cols[0].1.iter().any(|&j| j != i) { pos.x < placed.cols[0].0.max.x + g } else { pos.x < screen.min.x + band };
    let in_right = if placed.cols[1].1.iter().any(|&j| j != i) { pos.x > placed.cols[1].0.min.x - g } else { pos.x > screen.max.x - band };
    let slot = if in_left {
        Slot::Left
    } else if in_right {
        Slot::Right
    } else if i == LIBRARY && pos.y > screen.max.y - band.max(p.bottom_h * 0.6) {
        Slot::Bottom
    } else {
        return None;
    };
    let before = match slot {
        Slot::Bottom => None,
        s => placed.cols[if s == Slot::Left { 0 } else { 1 }].1.iter().copied().filter(|&j| j != i).find(|&j| placed.rects[j].is_some_and(|r| pos.y < r.center().y)),
    };
    Some(p.docked(i, slot, before))
}

/// While a panel's header is carried: a tag with its name under the
/// pointer, and the place it would take when let go, highlighted.
fn drop_preview(ctx: &egui::Context, p: &Panels, placed: &Placed, screen: Rect, shown: [bool; NP], i: usize, pos: Pos2) {
    const NAMES: [&str; NP] = ["Layers", "Library", "Properties", "Texture set settings", "Claude", "Packshot studio"];
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("panel drop")));
    let target = drop_target(p, placed, screen, i, pos).and_then(|q| place(&q, screen, shown).rects[i]);
    if let Some(r) = target {
        painter.rect(r, crate::style::R_PANEL, crate::style::ACCENT.gamma_multiply(0.14), egui::Stroke::new(2.0f32, crate::style::ACCENT));
        super::mark(ctx, "panel drop", r);
    }
    let g = painter.layout_no_wrap(NAMES[i].to_string(), crate::style::bold(12.0), crate::style::TEXT);
    let tag = Rect::from_min_size(pos + egui::vec2(12.0, 8.0), g.size() + egui::vec2(24.0, 14.0));
    painter.add(crate::style::shadow(3).as_shape(tag, crate::style::R_CONTROL));
    painter.rect(tag, crate::style::R_CONTROL, crate::style::PANEL, egui::Stroke::new(1.0f32, if target.is_some() { crate::style::ACCENT } else { crate::style::LINE }));
    painter.galley(tag.min + egui::vec2(12.0, 7.0), g, crate::style::TEXT);
}

/// Tool dock groups: the entries of each group (a tool, or a command).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockItem {
    Tool(Tool),
    /// Polygon fill making a selection.
    Faces,
    /// Select > By mesh map.
    ByMap,
}

impl DockItem {
    fn icon(self) -> Icon {
        match self {
            DockItem::Tool(Tool::Select(SelShape::Rect)) => Icon::SelMarquee,
            DockItem::Tool(Tool::Select(SelShape::Ellipse)) => Icon::SelEllipse,
            DockItem::Tool(Tool::Select(SelShape::Lasso)) => Icon::SelLasso,
            DockItem::Tool(Tool::Select(SelShape::PolyLasso)) => Icon::SelPolyLasso,
            DockItem::Tool(Tool::Wand) => Icon::Wand,
            DockItem::Tool(Tool::SelBrush) => Icon::SelBrush,
            DockItem::Faces => Icon::SelFaces,
            DockItem::ByMap => Icon::Generator,
            DockItem::Tool(Tool::Paint) => Icon::Brush,
            DockItem::Tool(Tool::Erase) => Icon::Eraser,
            DockItem::Tool(Tool::Smudge) => Icon::Smudge,
            DockItem::Tool(Tool::Clone) => Icon::Clone,
            DockItem::Tool(Tool::Bucket) => Icon::Bucket,
            DockItem::Tool(Tool::Gradient) => Icon::Gradient,
            DockItem::Tool(Tool::PolygonFill) => Icon::PolyFill,
            DockItem::Tool(Tool::Picker) => Icon::Picker,
            DockItem::Tool(Tool::Hide) => Icon::Hide,
            DockItem::Tool(Tool::Path) => Icon::Path,
            DockItem::Tool(Tool::Projection) => Icon::Stencil,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            DockItem::Tool(Tool::Select(s)) => s.name(),
            DockItem::Tool(Tool::Wand) => "Magic wand",
            DockItem::Tool(Tool::SelBrush) => "Selection brush",
            DockItem::Faces => "Select faces",
            DockItem::ByMap => "Select by mesh map",
            DockItem::Tool(Tool::Paint) => "Brush",
            DockItem::Tool(Tool::Erase) => "Eraser",
            DockItem::Tool(Tool::Smudge) => "Smudge",
            DockItem::Tool(Tool::Clone) => "Clone",
            DockItem::Tool(Tool::Bucket) => "Paint bucket",
            DockItem::Tool(Tool::Gradient) => "Gradient",
            DockItem::Tool(Tool::PolygonFill) => "Polygon fill",
            DockItem::Tool(Tool::Picker) => "Color picker",
            DockItem::Tool(Tool::Hide) => "Hide faces",
            DockItem::Tool(Tool::Path) => "Path",
            DockItem::Tool(Tool::Projection) => "Projection",
        }
    }

    /// Its key in the keymap ("" when it has none).
    fn key(self, km: &super::keymap::Keymap) -> String {
        use super::keymap::Action as A;
        let a = match self {
            DockItem::Tool(Tool::Select(SelShape::Rect)) => A::Marquee,
            DockItem::Tool(Tool::Select(SelShape::Ellipse)) => A::MarqueeShape,
            DockItem::Tool(Tool::Select(SelShape::Lasso)) => A::Lasso,
            DockItem::Tool(Tool::Select(SelShape::PolyLasso)) => A::LassoShape,
            DockItem::Tool(Tool::Wand) => A::Wand,
            DockItem::Tool(Tool::SelBrush) => A::SelBrush,
            DockItem::Faces | DockItem::ByMap => return String::new(),
            DockItem::Tool(Tool::Paint) => A::Brush,
            DockItem::Tool(Tool::Erase) => A::Eraser,
            DockItem::Tool(Tool::Smudge) => A::Smudge,
            DockItem::Tool(Tool::Clone) => A::Clone,
            DockItem::Tool(Tool::Bucket) => A::FillTool,
            DockItem::Tool(Tool::Gradient) => A::FillSwitch,
            DockItem::Tool(Tool::PolygonFill) => A::PolygonFill,
            DockItem::Tool(Tool::Picker) => A::Picker,
            DockItem::Tool(Tool::Hide) => A::Hide,
            DockItem::Tool(Tool::Path) => A::Path,
            DockItem::Tool(Tool::Projection) => A::Projection,
        };
        km.label(a)
    }
}

/// The tool dock: sections of slots; a slot shows one tool, or a stack of
/// tools (its last used one, the others in a flyout).
pub const DOCK: [(&str, &[&[DockItem]]); 4] = [
    (
        "Select",
        &[
            &[
                DockItem::Tool(Tool::Select(SelShape::Rect)),
                DockItem::Tool(Tool::Select(SelShape::Ellipse)),
                DockItem::Tool(Tool::Select(SelShape::Lasso)),
                DockItem::Tool(Tool::Select(SelShape::PolyLasso)),
            ],
            &[DockItem::Tool(Tool::Wand)],
            &[DockItem::Tool(Tool::SelBrush)],
            &[DockItem::Faces],
            &[DockItem::ByMap],
        ],
    ),
    (
        "Paint",
        &[
            &[DockItem::Tool(Tool::Paint)],
            &[DockItem::Tool(Tool::Projection)],
            &[DockItem::Tool(Tool::Erase)],
            &[DockItem::Tool(Tool::Path)],
            &[DockItem::Tool(Tool::Smudge)],
            &[DockItem::Tool(Tool::Clone)],
        ],
    ),
    ("Fill", &[&[DockItem::Tool(Tool::Bucket), DockItem::Tool(Tool::Gradient)], &[DockItem::Tool(Tool::PolygonFill)]]),
    ("Utilities", &[&[DockItem::Tool(Tool::Picker), DockItem::Tool(Tool::Hide)]]),
];

/// Every slot of the dock, in order (the entry it shows starts as its first).
pub fn dock_slots() -> Vec<&'static [DockItem]> {
    DOCK.iter().flat_map(|(_, slots)| slots.iter().copied()).collect()
}

/// What the user did to a panel's frame this frame.
#[derive(Default)]
struct PanelDrag {
    /// Its header is being dragged: the pointer.
    moving: Option<Pos2>,
    /// Its header was let go there.
    dropped: Option<Pos2>,
    /// Its inner edge dragged by this much (the column's width, or the
    /// bottom strip's height).
    inner: f32,
}

/// The edge of a docked panel that faces the canvas.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Edge {
    Left,
    Right,
    Top,
}

/// A rounded docked panel at `rect` (elevation 1); its content is laid
/// out in a child bounded and clipped to it. Unless `locked`, dragging the
/// top `header` points carries the panel to another dock place, and the
/// `edge` facing the canvas resizes its column (or the bottom strip).
fn dock_panel<R>(ctx: &egui::Context, id: &str, rect: Rect, header: f32, edge: Edge, locked: bool, add: impl FnOnce(&mut Ui) -> R) -> (R, PanelDrag) {
    egui::Area::new(egui::Id::new(("panel", id)))
        .fixed_pos(rect.min)
        .order(egui::Order::Middle)
        .constrain(false)
        .show(ctx, |ui| {
            // The whole panel takes the pointer (clicks never reach the views);
            // reserved first so it lies under the content (hover, wheel).
            ui.allocate_rect(rect, Sense::hover());
            let mut d = PanelDrag::default();
            let hr = Rect::from_min_size(rect.min, egui::vec2(rect.width(), header));
            if !locked {
                // The header carries the panel: under the header's own buttons.
                let h = ui.interact(hr, egui::Id::new(("panel move", id)), Sense::click_and_drag());
                let at = || ctx.input(|i| i.pointer.interact_pos().or(i.pointer.latest_pos()));
                if h.drag_stopped() {
                    d.dropped = at();
                } else if h.dragged() {
                    d.moving = at();
                    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                } else if h.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::Grab);
                }
            }
            let p = ui.painter();
            p.add(crate::style::shadow(1).as_shape(rect, crate::style::R_PANEL));
            p.rect(rect, crate::style::R_PANEL, crate::style::PANEL, egui::Stroke::new(1.0f32, crate::style::LINE));
            let inner_r = rect.shrink(1.0);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner_r).layout(egui::Layout::top_down(egui::Align::Min)));
            child.set_clip_rect(inner_r);
            let r = add(&mut child);
            super::mark(ctx, format!("panel header:{id}"), hr);
            if locked {
                return (r, d);
            }
            // The inner edge, on top of the content (a thin band inside the border).
            let (x0, y0, x1, y1) = (rect.min.x, rect.min.y, rect.max.x, rect.max.y);
            let (e, c) = (5.0, 14.0);
            let (hrect, cursor, name) = match edge {
                Edge::Left => (Rect::from_min_max(Pos2::new(x0, y0 + c), Pos2::new(x0 + e, y1 - c)), egui::CursorIcon::ResizeHorizontal, "left"),
                Edge::Right => (Rect::from_min_max(Pos2::new(x1 - e, y0 + c), Pos2::new(x1, y1 - c)), egui::CursorIcon::ResizeHorizontal, "right"),
                Edge::Top => (Rect::from_min_max(Pos2::new(x0 + c, y0), Pos2::new(x1 - c, y0 + 4.0)), egui::CursorIcon::ResizeVertical, "top"),
            };
            let resp = ui.interact(hrect, egui::Id::new(("panel resize", id, name)), Sense::drag());
            super::mark(ctx, format!("panel {name}:{id}"), hrect);
            if resp.hovered() || resp.dragged() {
                ctx.set_cursor_icon(cursor);
                let st = egui::Stroke::new(3.0f32, crate::style::ACCENT);
                let m = hrect.center();
                let seg = match edge {
                    Edge::Left => [Pos2::new(x0, m.y - 24.0), Pos2::new(x0, m.y + 24.0)],
                    Edge::Right => [Pos2::new(x1, m.y - 24.0), Pos2::new(x1, m.y + 24.0)],
                    Edge::Top => [Pos2::new(m.x - 24.0, y0), Pos2::new(m.x + 24.0, y0)],
                };
                ui.painter().line_segment(seg, st);
            }
            if resp.dragged() {
                d.inner = if edge == Edge::Top { resp.drag_delta().y } else { resp.drag_delta().x };
            }
            (r, d)
        })
        .inner
}

/// A floating pill (dock, options, view bar) anchored to the screen.
fn float_bar<R>(ctx: &egui::Context, id: &str, anchor: egui::Align2, offset: egui::Vec2, radius: f32, level: u8, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    egui::Area::new(egui::Id::new(("bar", id))).anchor(anchor, offset).order(egui::Order::Middle).show(ctx, |ui| {
        egui::Frame::none()
            .fill(crate::style::PANEL)
            .stroke(egui::Stroke::new(1.0f32, crate::style::LINE))
            .rounding(radius)
            .shadow(crate::style::shadow(level))
            .inner_margin(egui::Margin::same(6.0))
            .show(ui, |ui| ui.horizontal(|ui| add(ui)).inner)
            .inner
    })
}

/// A 42 px panel header: title and a dim subtitle; `right` adds icons.
pub(super) fn panel_header(ui: &mut Ui, title: &str, sub: &str, right: impl FnOnce(&mut Ui)) {
    panel_header_with(ui, title, sub, right, |_, _| {});
}

/// `panel_header` with something of its own between the title and the
/// buttons on the right (`middle` gets that strip; `sub` shows when empty).
pub(super) fn panel_header_with(ui: &mut Ui, title: &str, sub: &str, right: impl FnOnce(&mut Ui), middle: impl FnOnce(&mut Ui, Rect)) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 42.0), Sense::hover());
    let p = ui.painter();
    let g = p.layout_no_wrap(title.to_string(), crate::style::bold(12.5), crate::style::TEXT);
    let tw = g.size().x;
    p.galley(Pos2::new(r.min.x + 12.0, r.center().y - g.size().y * 0.5), g, crate::style::TEXT);
    if !sub.is_empty() {
        p.text(Pos2::new(r.min.x + 20.0 + tw, r.center().y), egui::Align2::LEFT_CENTER, sub, egui::FontId::proportional(11.5), crate::style::TEXT3);
    }
    p.line_segment([r.left_bottom(), r.right_bottom()], egui::Stroke::new(1.0f32, crate::style::SEPARATOR));
    let rr = Rect::from_min_max(Pos2::new(r.max.x - 120.0, r.min.y), Pos2::new(r.max.x - 8.0, r.max.y));
    let mut c = ui.new_child(egui::UiBuilder::new().max_rect(rr).layout(egui::Layout::right_to_left(egui::Align::Center)));
    right(&mut c);
    let used = c.min_rect();
    let mid = Rect::from_min_max(Pos2::new(r.min.x + 20.0 + tw, r.min.y), Pos2::new(used.min.x.min(rr.max.x) - 6.0, r.max.y));
    if mid.width() > 30.0 {
        middle(ui, mid);
    }
}

/// A segmented control (Layers | Texture sets | History...).
pub(super) fn segmented(ui: &mut Ui, id: &str, value: &mut u8, names: &[&str]) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, 30.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(r, crate::style::R_CONTROL, Color32::from_rgb(34, 34, 38));
    let n = names.len() as f32;
    let cw = (r.width() - 6.0) / n;
    for (i, name) in names.iter().enumerate() {
        let cr = Rect::from_min_size(Pos2::new(r.min.x + 3.0 + cw * i as f32, r.min.y + 3.0), egui::vec2(cw, r.height() - 6.0));
        let resp = ui.interact(cr, ui.id().with((id, i)), Sense::click());
        super::mark(ui.ctx(), format!("seg:{name}"), cr);
        let on = *value == i as u8;
        if on {
            p.rect_filled(cr, 6.0, crate::style::HOVER);
        } else if resp.hovered() {
            p.rect_filled(cr, 6.0, Color32::from_rgb(42, 42, 47));
        }
        p.text(cr.center(), egui::Align2::CENTER_CENTER, *name, egui::FontId::proportional(11.5), if on { crate::style::TEXT } else { crate::style::TEXT2 });
        if resp.clicked() {
            *value = i as u8;
        }
    }
}

impl RetroApp {
    fn panels(&mut self) -> &mut Panels {
        self.dock.get_or_insert_with(Panels::default)
    }

    /// Whether a panel shows.
    pub(super) fn tab_open(&self, tab: Tab) -> bool {
        let p = self.dock.clone().unwrap_or_default();
        match tab {
            Tab::Viewport => true,
            Tab::Assets => p.library,
            Tab::Properties => p.properties,
            Tab::Layers => p.left && p.left_tab == 0,
            Tab::TextureSets => p.left && p.left_tab == 1,
            Tab::SetSettings => p.set_settings,
            Tab::Muse => p.muse && (self.muse.peek || !super::mcp::linked()),
            Tab::Studio => self.view.studio,
        }
    }

    pub(super) fn tab_visible(&self, tab: Tab) -> bool {
        self.tab_open(tab)
    }

    /// Shows a panel (and its left tab).
    pub(super) fn focus_tab(&mut self, tab: Tab) {
        if tab == Tab::Studio {
            if !self.view.studio {
                self.toggle_studio();
            }
            return;
        }
        let p = self.panels();
        match tab {
            Tab::Viewport => {}
            Tab::Assets => p.library = true,
            Tab::Properties => p.properties = true,
            Tab::Layers => {
                p.left = true;
                p.left_tab = 0;
            }
            Tab::TextureSets => {
                p.left = true;
                p.left_tab = 1;
            }
            Tab::SetSettings => p.set_settings = true,
            Tab::Muse => p.muse = true,
            Tab::Studio => {}
        }
        if tab == Tab::Muse {
            // Asked for: it shows even while Claude is linked.
            self.muse.peek = true;
        }
    }

    /// Shows a hidden panel or hides a shown one.
    pub(super) fn toggle_tab(&mut self, tab: Tab) {
        if !self.tab_open(tab) {
            self.focus_tab(tab);
            return;
        }
        if tab == Tab::Studio {
            self.toggle_studio();
            return;
        }
        if tab == Tab::Muse {
            self.hide_muse_card();
            return;
        }
        let p = self.panels();
        match tab {
            Tab::Viewport => {}
            Tab::Assets => p.library = false,
            Tab::Properties => p.properties = false,
            Tab::Layers | Tab::TextureSets => p.left = false,
            Tab::SetSettings => p.set_settings = false,
            Tab::Muse => p.muse = false,
            Tab::Studio => {}
        }
    }

    /// The window: views, then the floating panels, bars and dock.
    pub(super) fn canvas_ui(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.show_set_settings) {
            self.panels().set_settings = true;
        }
        self.set_settings_visible = false;
        // The arrangement of the last session (once), then saved on change.
        if !self.layout_loaded {
            self.layout_loaded = true;
            if self.persist_layout && self.dock.is_none() {
                // (Layouts of before Claude are not read: the new default wins.)
                self.dock = read_layout();
                if let Some(p) = &self.dock {
                    self.view.swap_views = p.swap_views;
                    self.view.split = p.view_split.clamp(0.15, 0.85);
                    self.view.layout = match p.view_layout {
                        1 => Layout::Both,
                        2 => Layout::Only2D,
                        _ => Layout::Only3D,
                    };
                }
            }
        }
        if self.persist_layout && self.layout_saved.0.elapsed().as_secs_f32() > 0.5 {
            self.layout_saved.0 = Instant::now();
            let (swap, split) = (self.view.swap_views, self.view.split);
            let views = match self.view.layout {
                Layout::Only3D => 0,
                Layout::Both => 1,
                Layout::Only2D => 2,
            };
            let p = self.panels();
            p.swap_views = swap;
            p.view_split = split;
            p.view_layout = views;
            if let Ok(json) = serde_json::to_string(&self.dock.clone().unwrap_or_default()) {
                if json != self.layout_saved.1 {
                    let _ = std::fs::write(layout_path(), &json);
                    self.layout_saved.1 = json;
                }
            }
        }
        let screen = ctx.screen_rect();
        let p = self.dock.clone().unwrap_or_default();
        let mut shown = if !self.ui_hidden && self.doc.is_some() { p.shown() } else { [false; NP] };
        // The Claude card hides while a Claude app is linked (unless asked
        // for from the Window menu since it linked).
        let linked = super::mcp::linked();
        if linked && !self.muse.was_linked {
            self.muse.peek = false;
        }
        self.muse.was_linked = linked;
        shown[MUSE] &= !linked || self.muse.peek;
        shown[STUDIO] = self.view.studio && !self.ui_hidden && self.doc.is_some();
        let placed = place(&p, screen, shown);
        // The free canvas between the panels: bars and overlays center on it.
        self.canvas_mid = placed.mid;

        egui::CentralPanel::default().frame(egui::Frame::none().fill(crate::style::BG)).show(ctx, |ui| self.central(ui));
        if self.doc.is_none() {
            self.home_menu(ctx);
            return;
        }
        if self.ui_hidden {
            return;
        }
        if !p.left {
            // Its menu stays reachable (with the way back to the panels).
            self.home_menu(ctx);
        }
        let lock = p.locked;
        let mut drags: [PanelDrag; NP] = Default::default();
        for i in 0..NP {
            let Some(r) = placed.rects[i] else { continue };
            let edge = match p.slot(i) {
                Slot::Left => Edge::Right,
                Slot::Right => Edge::Left,
                Slot::Bottom => Edge::Top,
            };
            let id = PANEL_IDS[i];
            drags[i] = match i {
                LAYERS => dock_panel(ctx, id, r, 46.0, edge, lock, |ui| self.left_panel(ui, ctx)).1,
                LIBRARY => dock_panel(ctx, id, r, 42.0, edge, lock, |ui| self.assets_ui(ui, ctx)).1,
                2 => dock_panel(ctx, id, r, 42.0, edge, lock, |ui| self.properties_ui(ui, ctx)).1,
                MUSE => dock_panel(ctx, id, r, 42.0, edge, lock, |ui| self.muse_panel(ui)).1,
                STUDIO => dock_panel(ctx, id, r, 42.0, edge, lock, |ui| self.studio_panel(ui)).1,
                _ => dock_panel(ctx, id, r, 42.0, edge, lock, |ui| self.set_settings_panel(ui)).1,
            };
        }
        {
            // The inner edges: a column's width, the bottom strip's height.
            let max_h = (screen.height() * 0.6).max(140.0);
            let pm = self.panels();
            for (i, d) in drags.iter().enumerate() {
                if d.inner == 0.0 {
                    continue;
                }
                match p.slot(i) {
                    Slot::Left => pm.left_w = (pm.left_w.clamp(220.0, 520.0) + d.inner).clamp(220.0, 520.0),
                    Slot::Right => pm.right_w = (pm.right_w.clamp(260.0, 620.0) - d.inner).clamp(260.0, 620.0),
                    Slot::Bottom => pm.bottom_h = (pm.bottom_h.clamp(140.0, max_h) - d.inner).clamp(140.0, max_h),
                }
            }
        }
        // A header carried: where the panel would dock; let go: it docks
        // there (over the canvas it stays where it was).
        if let Some((i, pos)) = drags.iter().enumerate().find_map(|(i, d)| d.moving.map(|q| (i, q))) {
            drop_preview(ctx, &p, &placed, screen, shown, i, pos);
        }
        if let Some((i, pos)) = drags.iter().enumerate().find_map(|(i, d)| d.dropped.map(|q| (i, q))) {
            if let Some(q) = drop_target(&p, &placed, screen, i, pos) {
                let pm = self.panels();
                pm.slots = q.slots;
                pm.order = q.order;
                pm.weights = q.weights;
            }
        }
        if !lock {
            for (col, ids) in placed.cols.iter() {
                for w in ids.windows(2).filter(|w| !w.contains(&MUSE)) {
                    let (ra, rb) = (placed.rects[w[0]].unwrap(), placed.rects[w[1]].unwrap());
                    let grip = Rect::from_min_max(Pos2::new(col.min.x + 20.0, ra.max.y), Pos2::new(col.max.x - 20.0, rb.min.y));
                    self.split_grip(ctx, grip, w[0], w[1], *col, ids);
                }
            }
        }
        self.top_right_bar(ctx);
        self.view_bar(ctx);
        let dock_h = self.tool_dock(ctx);
        self.options_bar(ctx, dock_h);
        self.canvas_status(ctx);
    }

    /// The app menu in the corner, when the left panel (its usual home) is
    /// hidden or no project is open: the logo and the menu button.
    fn home_menu(&mut self, ctx: &egui::Context) {
        let logo = self.logo_id(ctx);
        let has = self.doc.is_some();
        egui::Area::new(egui::Id::new("home menu")).fixed_pos(Pos2::new(crate::style::GAP, crate::style::GAP)).order(egui::Order::Middle).show(ctx, |ui| {
            egui::Frame::none()
                .fill(crate::style::PANEL)
                .stroke(egui::Stroke::new(1.0f32, crate::style::LINE))
                .rounding(crate::style::R_PANEL)
                .shadow(crate::style::shadow(1))
                .inner_margin(egui::Margin::same(6.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), Sense::hover());
                        ui.painter().image(logo, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                        let m = ui.menu_button(RichText::new("\u{2630}").size(15.0), |ui| {
                            if has && ui.button("Show the layers panel").clicked() {
                                self.focus_tab(Tab::Layers);
                                ui.close_menu();
                            }
                            if has {
                                ui.separator();
                            }
                            self.app_menu(ui, ctx);
                        });
                        super::mark(ctx, "menu:Home", m.response.rect);
                    });
                });
        });
    }

    /// The gap between two stacked panels `a` (above) and `b` drags their
    /// split; each keeps `MIN_PANEL_H`.
    fn split_grip(&mut self, ctx: &egui::Context, r: Rect, a: usize, b: usize, col: Rect, ids: &[usize]) {
        egui::Area::new(egui::Id::new(("panel split", a, b))).fixed_pos(r.min).order(egui::Order::Middle).show(ctx, |ui| {
            let resp = ui.allocate_rect(Rect::from_min_size(r.min, r.size()), Sense::drag());
            super::mark(ctx, format!("panel split:{}", PANEL_IDS[a]), r);
            if resp.hovered() || resp.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
                ui.painter().line_segment([Pos2::new(r.center().x - 18.0, r.center().y), Pos2::new(r.center().x + 18.0, r.center().y)], egui::Stroke::new(3.0f32, crate::style::ACCENT));
            }
            if !resp.dragged() || resp.drag_delta().y == 0.0 {
                return;
            }
            let dy = resp.drag_delta().y;
            let pm = self.panels();
            let total: f32 = ids.iter().filter(|&&i| i != MUSE).map(|&i| pm.weights[i].max(0.05)).sum();
            let card = if ids.contains(&MUSE) { MUSE_H } else { 0.0 };
            let room = (col.height() - STACK_GAP * ids.len().saturating_sub(1) as f32 - card).max(1.0);
            let (wa, wb) = (pm.weights[a].max(0.05), pm.weights[b].max(0.05));
            let pair = wa + wb;
            let min = MIN_PANEL_H * total / room;
            if pair > 2.0 * min {
                let na = (wa + dy * total / room).clamp(min, pair - min);
                pm.weights[a] = na;
                pm.weights[b] = pair - na;
            }
        });
    }

    /// Left: the project (and the app menu), then layers, texture sets or history.
    fn left_panel(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            let logo = self.logo_id(ctx);
            let (r, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::hover());
            ui.painter().image(logo, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let (name, file) = match &self.doc {
                    Some(d) => (
                        d.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| d.mesh_name.clone()),
                        if d.modified {
                            "unsaved changes".to_string()
                        } else {
                            d.path.as_ref().and_then(|p| p.file_name()).map(|s| format!("{}, saved", s.to_string_lossy())).unwrap_or_else(|| "not saved yet".into())
                        },
                    ),
                    None => ("Material Forge".into(), String::new()),
                };
                ui.set_max_width((ui.available_width() - 72.0).clamp(60.0, 170.0));
                ui.add(egui::Label::new(RichText::new(name).font(crate::style::bold(12.5))).truncate());
                ui.add(egui::Label::new(RichText::new(file).size(11.0).color(crate::style::TEXT3)).truncate());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                let m = ui.menu_button(RichText::new("\u{2630}").size(15.0), |ui| self.app_menu(ui, ctx));
                super::mark(ctx, "menu:App", m.response.rect);
                // Locks the panels where they are (Window > Lock panels).
                let locked = self.dock.clone().unwrap_or_default().locked;
                let tip = if locked { "Panels locked; click to unlock" } else { "Lock the panels (no more moving or resizing)" };
                let l = icons::button(ui, if locked { Icon::Lock } else { Icon::Unlock }, locked, 26.0, tip);
                super::mark(ctx, "panel lock", l.rect);
                if l.clicked() {
                    self.panels().locked = !locked;
                }
            });
        });
        ui.add_space(8.0);
        let mut t = self.dock.clone().unwrap_or_default().left_tab;
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.allocate_ui(egui::vec2(ui.available_width() - 10.0, 30.0), |ui| segmented(ui, "left tabs", &mut t, &["Layers", "Texture sets", "History"]));
        });
        self.panels().left_tab = t;
        ui.add_space(4.0);
        match t {
            0 => self.layers_ui(ui),
            1 => self.texture_sets_ui(ui),
            _ => self.history_list(ui),
        }
    }

    /// The undo history, newest last; a click goes back (or forward) to it.
    pub(super) fn history_list(&mut self, ui: &mut Ui) {
        let mut steps: i32 = 0;
        egui::ScrollArea::vertical().id_salt("history list").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(4.0);
            let n = self.history.undo.len();
            let row = |ui: &mut Ui, on: bool, text: &str, weak: bool| -> bool {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), Sense::click());
                let r = r.shrink2(egui::vec2(8.0, 1.0));
                if on {
                    ui.painter().rect(r, 6.0, crate::style::ROW_SELECTED, egui::Stroke::new(1.0f32, crate::style::ACCENT));
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 6.0, crate::style::FIELD);
                }
                ui.painter().text(Pos2::new(r.min.x + 10.0, r.center().y), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(12.0), if weak { crate::style::TEXT3 } else { crate::style::TEXT });
                resp.clicked()
            };
            if row(ui, n == 0, "Opened", false) {
                steps = -(n as i32);
            }
            for (i, e) in self.history.undo.iter().enumerate() {
                if row(ui, i + 1 == n, e.label(), false) {
                    steps = -((n - 1 - i) as i32);
                }
            }
            for (i, e) in self.history.redo.iter().rev().enumerate() {
                if row(ui, false, e.label(), true) {
                    steps = i as i32 + 1;
                }
            }
        });
        for _ in 0..(-steps).max(0) {
            self.undo();
        }
        for _ in 0..steps.max(0) {
            self.redo();
        }
    }

    pub(super) fn logo_id(&mut self, ctx: &egui::Context) -> egui::TextureId {
        self.logo_tex
            .get_or_insert_with(|| {
                let img = crate::logo::image(256);
                let ci = egui::ColorImage::from_rgba_unmultiplied([256, 256], img.as_raw());
                ctx.load_texture("material forge logo", ci, egui::TextureOptions::LINEAR)
            })
            .id()
    }

    /// Top right: history and the Export button.
    fn top_right_bar(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::new("top right")).anchor(egui::Align2::RIGHT_TOP, egui::vec2(-crate::style::GAP, crate::style::GAP)).order(egui::Order::Middle).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let h = icons::button(ui, Icon::History, self.show_history, 30.0, "History (undo steps)");
                self.flyout_at[1] = Some(h.rect.center_bottom());
                if h.clicked() {
                    self.flip_flyout(1);
                }
                let uv = egui::Button::new(RichText::new("Auto UV").size(12.5)).rounding(crate::style::R_CONTROL).min_size(egui::vec2(80.0, 32.0));
                let r = ui.add_enabled(self.doc.is_some() && self.bake_job.is_none(), uv).on_hover_text("New UVs for the mesh: seams placed by shape, islands packed; the paint moves along");
                super::mark(ctx, "autouv", r.rect);
                if r.clicked() {
                    self.open_uv_dialog();
                }
                let busy = self.bake_job.is_some();
                let bake = egui::Button::new(RichText::new(if busy { "Baking..." } else { "Bake" }).size(12.5)).rounding(crate::style::R_CONTROL).min_size(egui::vec2(72.0, 32.0));
                let r = ui.add_enabled(!busy, bake).on_hover_text("Bake mesh maps: ambient occlusion, curvature, normal, thickness... (Ctrl + Shift + B)");
                super::mark(ctx, "bake", r.rect);
                if r.clicked() {
                    self.open_bake_dialog();
                }
                let b = egui::Button::new(RichText::new("Export").font(crate::style::bold(12.5)).color(Color32::WHITE))
                    .fill(crate::style::ACCENT)
                    .rounding(crate::style::R_CONTROL)
                    .min_size(egui::vec2(92.0, 32.0));
                let r = ui.add(b).on_hover_text("Export textures (Ctrl + Shift + E)");
                super::mark(ctx, "export", r.rect);
                if r.clicked() {
                    self.open_export();
                }
            });
        });
    }

    /// Top center: display, 3D / split / UV, camera, lighting, render.
    fn view_bar(&mut self, ctx: &egui::Context) {
        let off = egui::vec2(self.canvas_mid.center().x - ctx.screen_rect().center().x, crate::style::GAP);
        float_bar(ctx, "view", egui::Align2::CENTER_TOP, off, crate::style::R_PANEL, 1, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            // Optional channels: listed once the active set has them.
            let ext_on = self.doc.as_ref().map(|d| d.sets[d.active].settings.ext_channels).unwrap_or([false; crate::document::NEXT]);
            let combo = |ui: &mut Ui, d: &mut Display, id: &str, prefix: &str| {
                egui::ComboBox::from_id_salt(id).selected_text(format!("{prefix}{}", d.name())).width(150.0).height(520.0).show_ui(ui, |ui| {
                    for x in Display::ALL.iter() {
                        // The UV view is unlit: no "Material" there.
                        if !prefix.is_empty() && *x == Display::Material {
                            continue;
                        }
                        // The set's channels, the optional ones among them,
                        // then the mesh maps, then the mask.
                        let off = match x {
                            Display::SpecularLevel => !ext_on[0],
                            Display::Transmission => !ext_on[1],
                            Display::Ext(e) => !ext_on[*e as usize],
                            _ => false,
                        };
                        if off {
                            continue;
                        }
                        if matches!(x, Display::MapAo | Display::LayerMask) {
                            ui.separator();
                        }
                        ui.selectable_value(d, *x, x.name());
                    }
                })
            };
            if self.view.layout != Layout::Only2D {
                let r = combo(ui, &mut self.view.display, "display 3d", "");
                super::mark(ctx, "display3d", r.response.rect);
            }
            if self.view.layout != Layout::Only3D {
                if self.view.display_2d == Display::Material {
                    self.view.display_2d = Display::BaseColor;
                }
                let r = combo(ui, &mut self.view.display_2d, "display 2d", "UV: ");
                super::mark(ctx, "display2d", r.response.rect);
            }
            ui.add_space(4.0);
            let mut l = match self.view.layout {
                Layout::Only3D => 0u8,
                Layout::Both => 1,
                Layout::Only2D => 2,
            };
            ui.allocate_ui(egui::vec2(150.0, 30.0), |ui| segmented(ui, "layout", &mut l, &["3D", "Split", "UV"]));
            self.view.layout = [Layout::Only3D, Layout::Both, Layout::Only2D][l as usize];
            if self.view.layout == Layout::Both {
                let tip = if self.view.swap_views { "UV left, 3D right; click: 3D left (F4)" } else { "3D left, UV right; click: UV left, 3D right (F4)" };
                if icons::button(ui, Icon::Swap, self.view.swap_views, 28.0, tip).clicked() {
                    self.view.swap_views = !self.view.swap_views;
                }
            }
            ui.add_space(4.0);
            let cam_tip = if self.camera.ortho { "Orthographic (F6); click: perspective (F5)" } else { "Perspective (F5); click: orthographic (F6)" };
            if icons::button(ui, Icon::Camera, self.camera.ortho, 28.0, cam_tip).clicked() {
                self.camera.ortho = !self.camera.ortho;
            }
            let sun = icons::button(ui, Icon::Sun, self.show_view_settings, 28.0, "Lighting, environment and display");
            self.flyout_at[0] = Some(sun.rect.center_bottom());
            if sun.clicked() {
                self.flip_flyout(0);
            }
            let tip = if self.view.path_trace {
                format!("Rendering: {} samples, denoiser: {} (F10)", self.engine.pathtracer.frame, self.engine.pathtracer.denoiser_name())
            } else {
                format!("Render, path traced (F10); denoiser: {}", self.engine.pathtracer.denoiser_name())
            };
            if icons::button(ui, Icon::Render, self.view.path_trace, 28.0, &tip).clicked() {
                self.view.path_trace = !self.view.path_trace;
            }
            let tip = if self.view.studio { "Leave the packshot studio" } else { "Packshot studio: studio lights, background, lens and format (path traced), in its own panel" };
            let s = icons::button(ui, Icon::Studio, self.view.studio, 28.0, tip);
            if s.clicked() {
                self.toggle_studio();
            }
        });
        // Baking: progress and cancel under the bar.
        if let Some(job) = &self.bake_job {
            let f = job.progress.load(Ordering::Relaxed) as f32 / job.total.max(1) as f32;
            let cancel = job.cancel.clone();
            float_bar(ctx, "bake progress", egui::Align2::CENTER_TOP, off + egui::vec2(0.0, 56.0), crate::style::R_PANEL, 1, |ui| {
                ui.add(egui::ProgressBar::new(f).desired_width(200.0).desired_height(16.0).text(format!("Baking {:.0} %", f * 100.0)));
                if ui.small_button("Cancel").clicked() {
                    cancel.store(true, Ordering::Relaxed);
                }
            });
        }
    }

    /// How far the tool dock sits above the window's bottom: over the
    /// library when it is docked along the bottom.
    fn dock_lift(&self, ctx: &egui::Context) -> f32 {
        (ctx.screen_rect().max.y - self.canvas_mid.max.y).max(0.0)
    }

    /// The dock entry of the current tool.
    fn dock_current(&self) -> DockItem {
        match self.tool {
            Tool::PolygonFill if self.poly_to_selection => DockItem::Faces,
            t => DockItem::Tool(t),
        }
    }

    /// Activates a dock entry.
    pub(super) fn dock_pick(&mut self, item: DockItem) {
        let t = match item {
            DockItem::ByMap => {
                let k = MapSel::Edges;
                self.sel_by_map = Some((k, k.default_threshold()));
                return;
            }
            DockItem::Faces => {
                self.poly_to_selection = true;
                Tool::PolygonFill
            }
            DockItem::Tool(Tool::PolygonFill) => {
                self.poly_to_selection = false;
                Tool::PolygonFill
            }
            DockItem::Tool(t) => t,
        };
        if matches!(t, Tool::Bucket | Tool::Gradient) {
            self.last_fill_tool = t;
        }
        if let Tool::Select(s) = t {
            if matches!(s, SelShape::Rect | SelShape::Ellipse) {
                self.last_marquee = s;
            } else {
                self.last_lasso = s;
            }
        }
        if self.tool != t {
            self.sel_drag = None;
        }
        self.tool = t;
        self.prev_tool = None;
    }

    /// Bottom center: every tool, grouped in sections; stacks (marquees and
    /// lassos, bucket and gradient, utilities) show their last used tool,
    /// and a click on the active stack (or a right click) opens the others.
    /// Returns the dock's height.
    fn tool_dock(&mut self, ctx: &egui::Context) -> f32 {
        let cur = self.dock_current();
        let slots = dock_slots();
        if self.dock_last.len() != slots.len() {
            self.dock_last = slots.iter().map(|s| s[0]).collect();
        }
        for (i, sl) in slots.iter().enumerate() {
            if sl.contains(&cur) {
                self.dock_last[i] = cur;
            }
        }
        let off = egui::vec2(self.canvas_mid.center().x - ctx.screen_rect().center().x, -12.0 - self.dock_lift(ctx));
        let mut picked = None;
        let by_map_open = self.sel_by_map.is_some();
        let r = float_bar(ctx, "tool dock", egui::Align2::CENTER_BOTTOM, off, 18.0, 2, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let mut si = 0;
            for (gi, (gname, section)) in DOCK.iter().enumerate() {
                if gi > 0 {
                    let (sr, _) = ui.allocate_exact_size(egui::vec2(13.0, 38.0), Sense::hover());
                    ui.painter().line_segment([sr.center() - egui::vec2(0.0, 12.0), sr.center() + egui::vec2(0.0, 12.0)], egui::Stroke::new(1.0f32, crate::style::LINE));
                }
                for items in section.iter() {
                    let slot = si;
                    si += 1;
                    let shown = self.dock_last[slot];
                    let active = if shown == DockItem::ByMap { by_map_open } else { items.contains(&cur) };
                    let (br, resp) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), Sense::click());
                    super::mark(ctx, format!("dock:{}", shown.name()), br);
                    let p = ui.painter();
                    if active {
                        p.rect_filled(br, 12.0, crate::style::ACCENT);
                    } else if resp.hovered() {
                        p.rect_filled(br, 12.0, crate::style::HOVER);
                    }
                    let c = if active { Color32::WHITE } else if resp.hovered() { crate::style::TEXT } else { crate::style::TEXT2 };
                    icons::paint(p, br.shrink(9.0), shown.icon(), c);
                    if items.len() > 1 {
                        let k = br.right_bottom() - egui::vec2(5.0, 5.0);
                        p.add(egui::Shape::convex_polygon(vec![k, k - egui::vec2(5.0, 0.0), k - egui::vec2(0.0, 5.0)], c.gamma_multiply(0.6), egui::Stroke::NONE));
                    }
                    let key = shown.key(&self.keymap);
                    let tip = if key.is_empty() { shown.name().to_string() } else { format!("{}   {key}", shown.name()) };
                    let resp = resp.on_hover_text(if items.len() > 1 { format!("{tip}\n(click again or right click: the other {})", gname.to_lowercase()) } else { tip });
                    let pid = egui::Id::new(("dock flyout", slot));
                    if items.len() > 1 && ((resp.clicked() && active) || resp.secondary_clicked()) {
                        ui.memory_mut(|m| m.toggle_popup(pid));
                    } else if resp.clicked() {
                        picked = Some(shown);
                    }
                    if items.len() < 2 {
                        continue;
                    }
                    let keymap = self.keymap;
                    super::widgets::popup_below(ui, pid, &resp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                        ui.set_min_width(230.0);
                        for it in items.iter() {
                            let on = *it == cur;
                            let (ir, iresp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), Sense::click());
                            super::mark(ctx, format!("dockitem:{}", it.name()), ir);
                            let p = ui.painter();
                            if on {
                                p.rect_filled(ir, 8.0, crate::style::ROW_SELECTED);
                            } else if iresp.hovered() {
                                p.rect_filled(ir, 8.0, crate::style::HOVER);
                            }
                            icons::paint(p, Rect::from_center_size(Pos2::new(ir.min.x + 18.0, ir.center().y), egui::vec2(18.0, 18.0)), it.icon(), if on { crate::style::ACCENT } else { crate::style::TEXT2 });
                            p.text(Pos2::new(ir.min.x + 36.0, ir.center().y), egui::Align2::LEFT_CENTER, it.name(), egui::FontId::proportional(12.5), crate::style::TEXT);
                            let key = it.key(&keymap);
                            if !key.is_empty() {
                                p.text(Pos2::new(ir.max.x - 8.0, ir.center().y), egui::Align2::RIGHT_CENTER, key, egui::FontId::monospace(10.5), crate::style::TEXT3);
                            }
                            if iresp.clicked() {
                                picked = Some(*it);
                            }
                        }
                    });
                }
            }
        });
        if let Some(it) = picked {
            self.dock_pick(it);
        }
        self.dock_rect = r.response.rect;
        r.response.rect.height()
    }

    /// The active tool's options, in a bar right above the dock.
    fn options_bar(&mut self, ctx: &egui::Context, dock_h: f32) {
        let off = egui::vec2(self.canvas_mid.center().x - ctx.screen_rect().center().x, -12.0 - self.dock_lift(ctx) - dock_h - 8.0);
        let max_w = (self.canvas_mid.width() - 80.0).max(300.0);
        let item = self.dock_current();
        // A selected fill shows its projection here instead of the tool.
        let fill = self.doc.as_ref().is_some_and(|d| {
            let s = &d.sets[d.active];
            s.target == crate::document::Target::Content && matches!(s.selected_layer().map(|l| &l.desc.kind), Some(crate::document::LayerKind::Fill(_)))
        });
        let title = if fill { "Projection" } else { item.name() };
        self.options_rect = float_bar(ctx, "tool options", egui::Align2::CENTER_BOTTOM, off, 14.0, 1, |ui| {
            // Too wide for the canvas: a second row.
            ui.set_max_width(max_w);
            ui.horizontal_wrapped(|ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(title).font(crate::style::bold(12.0)));
                let (sr, _) = ui.allocate_exact_size(egui::vec2(9.0, 26.0), Sense::hover());
                ui.painter().line_segment([sr.center_top(), sr.center_bottom()], egui::Stroke::new(1.0f32, crate::style::LINE));
                self.tool_options(ui);
            });
        })
        .response
        .rect;
    }

    /// The last message, in the canvas corner for a few seconds.
    fn canvas_status(&mut self, ctx: &egui::Context) {
        if self.status_shown.0 != self.status {
            self.status_shown = (self.status.clone(), Instant::now());
        }
        let age = self.status_shown.1.elapsed().as_secs_f32();
        if self.status.is_empty() || age > 5.0 {
            return;
        }
        if age > 4.0 {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        let alpha = (5.0 - age).clamp(0.0, 1.0);
        let x = self.canvas_mid.min.x + 14.0;
        let y = self.canvas_mid.max.y - 18.0;
        // Left of the tool dock, never under it.
        let clip = Rect::from_min_max(Pos2::new(x, y - 30.0), Pos2::new((self.dock_rect.min.x - 12.0).max(x + 40.0), self.canvas_mid.max.y));
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("canvas status"))).with_clip_rect(clip);
        let error = self.status.starts_with("Could not") || self.status.contains("failed");
        let c = if error { Color32::from_rgb(255, 120, 110) } else { Color32::from_white_alpha(150) };
        // Windows' extended path prefix is noise to the user.
        let mut text = self.status.replace(r"\\?\", "");
        let font = egui::FontId::proportional(11.5);
        let room = clip.width() - 4.0;
        if p.layout_no_wrap(text.clone(), font.clone(), c).size().x > room {
            let mut chars: Vec<char> = text.chars().collect();
            while chars.len() > 4 && p.layout_no_wrap(chars.iter().collect::<String>() + "...", font.clone(), c).size().x > room {
                chars.truncate(chars.len() - 2);
            }
            text = chars.into_iter().collect::<String>() + "...";
        }
        p.text(Pos2::new(x, y), egui::Align2::LEFT_BOTTOM, text, font, c.gamma_multiply(alpha));
    }

    /// Texture set settings, docked (left column by default).
    fn set_settings_panel(&mut self, ui: &mut Ui) {
        self.set_settings_visible = true;
        let mut close = false;
        panel_header(ui, "Texture set settings", "", |ui| close = icons::button(ui, Icon::Close, false, 24.0, "Close").clicked());
        self.set_settings_ui(ui);
        if close {
            self.panels().set_settings = false;
        }
    }
}
