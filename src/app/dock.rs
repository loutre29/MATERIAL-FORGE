//! The canvas layout (docs/design/material_forge_design_system.html): the
//! views fill the window; rounded panels float over them (project and
//! layers on the left, library top right, properties bottom right); the
//! tools live in one dock at the bottom with their options right above it;
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
}

impl Tab {
    pub const PANELS: [Tab; 5] = [Tab::Layers, Tab::Assets, Tab::Properties, Tab::TextureSets, Tab::SetSettings];

    pub fn name(self) -> &'static str {
        match self {
            Tab::Viewport => "Viewport",
            Tab::Assets => "Library",
            Tab::Properties => "Properties",
            Tab::TextureSets => "Texture sets",
            Tab::Layers => "Layers",
            Tab::SetSettings => "Texture set settings",
        }
    }
}

/// Which panels show and where. `RetroApp::dock` None is the default
/// arrangement. Saved between sessions (`layout_v3.json`).
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Panels {
    /// Left panel: 0 layers, 1 texture sets, 2 history.
    pub left_tab: u8,
    pub left: bool,
    pub library: bool,
    pub properties: bool,
    /// The texture set settings window.
    pub set_settings: bool,
    /// Library / properties split of the right column (0..1).
    pub right_split: f32,
    /// Docked widths of the left panel and of the right column.
    pub left_w: f32,
    pub right_w: f32,
    /// Floating panels (left, library, properties): their rect (x0, y0,
    /// x1, y1) once moved or resized away from their docked place.
    pub rects: [Option<[f32; 4]>; 3],
    /// Panels can neither move nor resize.
    pub locked: bool,
    /// Split view: the UV on the left, the 3D on the right (F4).
    pub swap_views: bool,
    /// Split view: where the divider sits (0..1).
    pub view_split: f32,
}

impl Default for Panels {
    fn default() -> Self {
        Self {
            left_tab: 0,
            left: true,
            library: true,
            properties: true,
            set_settings: false,
            right_split: 0.46,
            left_w: LEFT_W,
            right_w: RIGHT_W,
            rects: [None; 3],
            locked: false,
            swap_views: false,
            view_split: 0.5,
        }
    }
}

fn layout_path() -> std::path::PathBuf {
    crate::library::library_dir().join("..").join("layout_v3.json")
}

pub const LEFT_W: f32 = 272.0;
pub const RIGHT_W: f32 = 312.0;

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
    /// The panel's new rect (moved by its header, or resized by an edge or
    /// corner): it floats there.
    rect: Option<Rect>,
    /// Header double clicked: back to its docked place.
    reset: bool,
    /// A docked panel's inner edge dragged by this much (its docked width).
    inner: f32,
}

/// A rounded floating panel at `rect` (elevation 1); its content is laid
/// out in a child bounded and clipped to it. Unless `locked`, the top
/// `header` points move the panel and every edge and corner resizes it; a
/// docked panel's `inner` edge (-1 left, 1 right) changes its docked width.
fn float_panel<R>(ctx: &egui::Context, id: &str, rect: Rect, header: f32, docked: bool, inner: f32, locked: bool, add: impl FnOnce(&mut Ui) -> R) -> (R, PanelDrag) {
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
                // The header moves the panel: under the header's own buttons.
                let h = ui.interact(hr, egui::Id::new(("panel move", id)), Sense::click_and_drag());
                if h.dragged() {
                    d.rect = Some(rect.translate(h.drag_delta()));
                    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                } else if h.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::Grab);
                }
                d.reset = h.double_clicked() && !docked;
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
            // Edges and corners, on top of the content (a thin band inside the border).
            let (x0, y0, x1, y1) = (rect.min.x, rect.min.y, rect.max.x, rect.max.y);
            let e = 5.0;
            let c = 14.0;
            use egui::CursorIcon as C;
            // (handle, moves left / top / right / bottom edge, cursor, name)
            let handles: [(Rect, [bool; 4], C, &str); 8] = [
                (Rect::from_min_max(Pos2::new(x0, y0 + c), Pos2::new(x0 + e, y1 - c)), [true, false, false, false], C::ResizeHorizontal, "left"),
                (Rect::from_min_max(Pos2::new(x1 - e, y0 + c), Pos2::new(x1, y1 - c)), [false, false, true, false], C::ResizeHorizontal, "right"),
                (Rect::from_min_max(Pos2::new(x0 + c, y0), Pos2::new(x1 - c, y0 + 4.0)), [false, true, false, false], C::ResizeVertical, "top"),
                (Rect::from_min_max(Pos2::new(x0 + c, y1 - e), Pos2::new(x1 - c, y1)), [false, false, false, true], C::ResizeVertical, "bottom"),
                (Rect::from_min_size(Pos2::new(x0, y0), egui::vec2(c, c)), [true, true, false, false], C::ResizeNwSe, "top left"),
                (Rect::from_min_size(Pos2::new(x1 - c, y1 - c), egui::vec2(c, c)), [false, false, true, true], C::ResizeNwSe, "bottom right"),
                (Rect::from_min_size(Pos2::new(x1 - c, y0), egui::vec2(c, c)), [false, true, true, false], C::ResizeNeSw, "top right"),
                (Rect::from_min_size(Pos2::new(x0, y1 - c), egui::vec2(c, c)), [true, false, false, true], C::ResizeNeSw, "bottom left"),
            ];
            for (hrect, sides, cursor, name) in handles {
                let resp = ui.interact(hrect, egui::Id::new(("panel resize", id, name)), Sense::drag());
                super::mark(ctx, format!("panel {name}:{id}"), hrect);
                if resp.hovered() || resp.dragged() {
                    ctx.set_cursor_icon(cursor);
                    let st = egui::Stroke::new(3.0f32, crate::style::ACCENT);
                    let pnt = ui.painter();
                    if sides[0] && !sides[1] && !sides[3] {
                        pnt.line_segment([Pos2::new(x0, hrect.center().y - 24.0), Pos2::new(x0, hrect.center().y + 24.0)], st);
                    } else if sides[2] && !sides[1] && !sides[3] {
                        pnt.line_segment([Pos2::new(x1, hrect.center().y - 24.0), Pos2::new(x1, hrect.center().y + 24.0)], st);
                    } else if sides[1] && !sides[0] && !sides[2] {
                        pnt.line_segment([Pos2::new(hrect.center().x - 24.0, y0), Pos2::new(hrect.center().x + 24.0, y0)], st);
                    } else if sides[3] && !sides[0] && !sides[2] {
                        pnt.line_segment([Pos2::new(hrect.center().x - 24.0, y1), Pos2::new(hrect.center().x + 24.0, y1)], st);
                    } else {
                        pnt.circle_filled(hrect.center(), 4.0, crate::style::ACCENT);
                    }
                }
                if !resp.dragged() {
                    continue;
                }
                let dv = resp.drag_delta();
                let only_inner = docked && !sides[1] && !sides[3] && ((inner < 0.0 && sides[0] && !sides[2]) || (inner > 0.0 && sides[2] && !sides[0]));
                if only_inner {
                    d.inner = dv.x;
                    continue;
                }
                let mut nr = d.rect.unwrap_or(rect);
                if sides[0] {
                    nr.min.x += dv.x;
                }
                if sides[1] {
                    nr.min.y += dv.y;
                }
                if sides[2] {
                    nr.max.x += dv.x;
                }
                if sides[3] {
                    nr.max.y += dv.y;
                }
                d.rect = Some(nr);
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
        }
    }

    pub(super) fn tab_visible(&self, tab: Tab) -> bool {
        self.tab_open(tab)
    }

    /// Shows a panel (and its left tab).
    pub(super) fn focus_tab(&mut self, tab: Tab) {
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
        }
    }

    /// Shows a hidden panel or hides a shown one.
    pub(super) fn toggle_tab(&mut self, tab: Tab) {
        if !self.tab_open(tab) {
            self.focus_tab(tab);
            return;
        }
        let p = self.panels();
        match tab {
            Tab::Viewport => {}
            Tab::Assets => p.library = false,
            Tab::Properties => p.properties = false,
            Tab::Layers | Tab::TextureSets => p.left = false,
            Tab::SetSettings => p.set_settings = false,
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
                self.dock = std::fs::read_to_string(layout_path()).ok().and_then(|t| serde_json::from_str::<Panels>(&t).ok());
                if let Some(p) = &self.dock {
                    self.view.swap_views = p.swap_views;
                    self.view.split = p.view_split.clamp(0.15, 0.85);
                }
            }
        }
        if self.persist_layout && self.layout_saved.0.elapsed().as_secs_f32() > 0.5 {
            self.layout_saved.0 = Instant::now();
            let (swap, split) = (self.view.swap_views, self.view.split);
            let p = self.panels();
            p.swap_views = swap;
            p.view_split = split;
            if let Ok(json) = serde_json::to_string(&self.dock.clone().unwrap_or_default()) {
                if json != self.layout_saved.1 {
                    let _ = std::fs::write(layout_path(), &json);
                    self.layout_saved.1 = json;
                }
            }
        }
        let screen = ctx.screen_rect();
        let g = crate::style::GAP;
        let p = self.dock.clone().unwrap_or_default();
        let show = !self.ui_hidden && self.doc.is_some();
        let (lw, rw) = (p.left_w.clamp(220.0, 520.0), p.right_w.clamp(260.0, 620.0));
        // Floating rects are kept on screen and at least 220 x 160.
        let keep = |r: Rect| {
            let w = r.width().clamp(220.0, screen.width() - 2.0 * g);
            let h = r.height().clamp(160.0, screen.height() - 2.0 * g);
            let r = Rect::from_min_size(r.min, egui::vec2(w, h));
            let dx = (screen.min.x - r.min.x).max(0.0) + (screen.max.x - r.max.x).min(0.0);
            let dy = (screen.min.y - r.min.y).max(0.0) + (screen.max.y - r.max.y).min(0.0);
            r.translate(egui::vec2(dx, dy))
        };
        let float = |i: usize| p.rects[i].map(|v| keep(Rect::from_min_max(Pos2::new(v[0], v[1]), Pos2::new(v[2], v[3]))));
        let rx = screen.max.x - g - rw;
        let top = screen.min.y + g + 42.0;
        let bottom = screen.max.y - g;
        let both_docked = p.library && p.properties && p.rects[1].is_none() && p.rects[2].is_none();
        let split = if both_docked {
            top + (bottom - top) * p.right_split.clamp(0.2, 0.8)
        } else if p.library && p.rects[1].is_none() {
            bottom
        } else {
            top
        };
        let left = float(0).unwrap_or(Rect::from_min_max(screen.min + egui::vec2(g, g), Pos2::new(screen.min.x + g + lw, screen.max.y - g)));
        let lib = float(1).unwrap_or(Rect::from_min_max(Pos2::new(rx, top), Pos2::new(screen.max.x - g, if both_docked { split - 4.0 } else { bottom })));
        let props = float(2).unwrap_or(Rect::from_min_max(Pos2::new(rx, if p.library && p.rects[1].is_none() { split + 4.0 } else { top }), Pos2::new(screen.max.x - g, bottom)));
        // The free canvas between the panels: bars and overlays center on it.
        let mut mid_l = screen.min.x;
        let mut mid_r = screen.max.x;
        if show {
            for (on, r) in [(p.left, left), (p.library, lib), (p.properties, props)] {
                if !on {
                    continue;
                }
                if r.center().x < screen.center().x {
                    mid_l = mid_l.max(r.max.x);
                } else {
                    mid_r = mid_r.min(r.min.x);
                }
            }
        }
        if mid_r - mid_l < 200.0 {
            (mid_l, mid_r) = (screen.min.x, screen.max.x);
        }
        self.canvas_mid = Rect::from_min_max(Pos2::new(mid_l, screen.min.y), Pos2::new(mid_r, screen.max.y));

        egui::CentralPanel::default().frame(egui::Frame::none().fill(crate::style::BG)).show(ctx, |ui| self.central(ui));
        if self.doc.is_none() {
            self.home_menu(ctx);
            return;
        }
        if self.ui_hidden {
            return;
        }
        let lock = p.locked;
        let mut drags: [PanelDrag; 3] = Default::default();
        if p.left {
            drags[0] = float_panel(ctx, "left", left, 46.0, p.rects[0].is_none(), 1.0, lock, |ui| self.left_panel(ui, ctx)).1;
        } else {
            // Its menu stays reachable (with the way back to the panels).
            self.home_menu(ctx);
        }
        if p.library {
            drags[1] = float_panel(ctx, "library", lib, 42.0, p.rects[1].is_none(), -1.0, lock, |ui| self.assets_ui(ui, ctx)).1;
        }
        if p.properties {
            drags[2] = float_panel(ctx, "properties", props, 42.0, p.rects[2].is_none(), -1.0, lock, |ui| self.properties_ui(ui, ctx)).1;
        }
        {
            let pm = self.panels();
            for (i, d) in drags.iter().enumerate() {
                if let Some(r) = d.rect {
                    pm.rects[i] = Some([r.min.x, r.min.y, r.max.x, r.max.y]);
                }
                if d.reset {
                    pm.rects[i] = None;
                }
            }
            pm.left_w = (pm.left_w.clamp(220.0, 520.0) + drags[0].inner).clamp(220.0, 520.0);
            pm.right_w = (pm.right_w.clamp(260.0, 620.0) - drags[1].inner - drags[2].inner).clamp(260.0, 620.0);
        }
        if both_docked && !lock {
            self.right_split_grip(ctx, Rect::from_min_max(Pos2::new(rx + 20.0, split - 4.0), Pos2::new(screen.max.x - g - 20.0, split + 4.0)), top, bottom);
        }
        self.top_right_bar(ctx);
        self.view_bar(ctx);
        let dock_h = self.tool_dock(ctx);
        self.options_bar(ctx, dock_h);
        self.canvas_status(ctx);
        self.set_settings_window(ctx);
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

    /// The gap between the library and properties drags their split.
    fn right_split_grip(&mut self, ctx: &egui::Context, r: Rect, top: f32, bottom: f32) {
        egui::Area::new(egui::Id::new("right split")).fixed_pos(r.min).order(egui::Order::Middle).show(ctx, |ui| {
            let resp = ui.allocate_rect(Rect::from_min_size(r.min, r.size()), Sense::drag());
            if resp.hovered() || resp.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
                ui.painter().line_segment([Pos2::new(r.center().x - 18.0, r.center().y), Pos2::new(r.center().x + 18.0, r.center().y)], egui::Stroke::new(3.0f32, crate::style::ACCENT));
            }
            if resp.dragged() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    self.panels().right_split = ((pos.y - top) / (bottom - top).max(1.0)).clamp(0.2, 0.8);
                }
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
                ui.set_max_width(170.0);
                ui.add(egui::Label::new(RichText::new(name).font(crate::style::bold(12.5))).truncate());
                ui.add(egui::Label::new(RichText::new(file).size(11.0).color(crate::style::TEXT3)).truncate());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                let m = ui.menu_button(RichText::new("\u{2630}").size(15.0), |ui| self.app_menu(ui, ctx));
                super::mark(ctx, "menu:App", m.response.rect);
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
        let off = egui::vec2(self.canvas_mid.center().x - ctx.screen_rect().center().x, -12.0);
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
        let off = egui::vec2(self.canvas_mid.center().x - ctx.screen_rect().center().x, -12.0 - dock_h - 8.0);
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

    /// Texture set settings: a window next to the left panel.
    fn set_settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.dock.clone().unwrap_or_default().set_settings;
        if !open {
            return;
        }
        egui::Window::new(RichText::new("TEXTURE SET SETTINGS").font(crate::style::bold(11.5)))
            .id(egui::Id::new("set settings window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_pos(Pos2::new(LEFT_W + 24.0, 60.0))
            .default_size(egui::vec2(300.0, 520.0))
            .vscroll(true)
            .show(ctx, |ui| {
                self.set_settings_visible = true;
                self.set_settings_ui(ui);
            });
        self.panels().set_settings = open;
    }
}
