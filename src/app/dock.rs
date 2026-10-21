//! Docking: every panel is a tab that can be dragged to another side,
//! tabbed with others, split or floated, like the real application's docks.

use super::*;
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};

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
    pub const PANELS: [Tab; 5] = [Tab::Assets, Tab::Properties, Tab::TextureSets, Tab::Layers, Tab::SetSettings];

    pub fn name(self) -> &'static str {
        match self {
            Tab::Viewport => "Viewport",
            Tab::Assets => "Assets",
            Tab::Properties => "Properties",
            Tab::TextureSets => "Texture Set List",
            Tab::Layers => "Layers",
            Tab::SetSettings => "Texture Set Settings",
        }
    }
}

fn layout_path() -> std::path::PathBuf {
    crate::library::library_dir().join("..").join("layout.json")
}

/// The arrangement saved by the last session, if it is usable.
fn saved_layout() -> Option<DockState<Tab>> {
    let text = std::fs::read_to_string(layout_path()).ok()?;
    let state: DockState<Tab> = serde_json::from_str(&text).ok()?;
    state.find_tab(&Tab::Viewport)?;
    Some(state)
}

/// The real application's default arrangement.
pub fn default_layout() -> DockState<Tab> {
    let mut state = DockState::new(vec![Tab::Viewport]);
    let surface = state.main_surface_mut();
    let [view, _assets] = surface.split_left(NodeIndex::root(), 0.15, vec![Tab::Assets]);
    let [view, _props] = surface.split_left(view, 0.22, vec![Tab::Properties]);
    let [_view, sets] = surface.split_right(view, 0.70, vec![Tab::TextureSets]);
    let [_sets, _layers] = surface.split_below(sets, 0.24, vec![Tab::Layers, Tab::SetSettings]);
    state
}

fn style(ctx: &egui::Context) -> Style {
    let mut s = Style::from_egui(&ctx.style());
    s.dock_area_padding = None;
    s.main_surface_border_stroke = egui::Stroke::NONE;
    s.separator.width = 3.0;
    s.separator.color_idle = crate::style::SEPARATOR;
    s.separator.color_hovered = crate::style::ACCENT;
    s.separator.color_dragged = crate::style::ACCENT;
    s.tab_bar.bg_fill = crate::style::HEADER;
    s.tab_bar.height = 26.0;
    s.tab_bar.hline_color = crate::style::SEPARATOR;
    s.tab.tab_body.bg_fill = crate::style::PANEL;
    s.tab.tab_body.inner_margin = egui::Margin::same(0.0);
    s.tab.tab_body.stroke = egui::Stroke::NONE;
    s.tab.active.bg_fill = crate::style::PANEL;
    s.tab.active.text_color = egui::Color32::WHITE;
    s.tab.active.outline_color = crate::style::SEPARATOR;
    s.tab.focused.bg_fill = crate::style::PANEL;
    s.tab.focused.text_color = egui::Color32::WHITE;
    s.tab.focused.outline_color = crate::style::ACCENT;
    s.tab.inactive.bg_fill = crate::style::HEADER;
    s.tab.inactive.text_color = egui::Color32::from_gray(160);
    s.tab.inactive.outline_color = crate::style::HEADER;
    s.tab.hovered.bg_fill = egui::Color32::from_gray(52);
    s.tab.hovered.text_color = egui::Color32::WHITE;
    s.tab.hline_below_active_tab_name = true;
    s.overlay.selection_color = crate::style::ACCENT.linear_multiply(0.35);
    s
}

struct Viewer<'a> {
    app: &'a mut RetroApp,
    ctx: egui::Context,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        let t = match tab {
            Tab::Properties => self.app.properties_title_short(),
            Tab::Viewport => {
                return egui::RichText::new("VIEWPORT").font(crate::style::bold(10.5)).color(egui::Color32::from_gray(140)).into();
            }
            other => other.name().to_string(),
        };
        egui::RichText::new(t.to_uppercase()).font(crate::style::bold(11.0)).into()
    }

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(("dock tab", *tab as u8))
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        let ctx = self.ctx.clone();
        // The tab body sits in a non-scrolling ScrollArea that remembers the
        // widest content it ever saw: lay the tab out in a fresh child bounded
        // to the visible rect so widgets measure the real width.
        let visible = ui.clip_rect().intersect(ui.max_rect());
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(visible).layout(egui::Layout::top_down(egui::Align::Min)));
        child.set_clip_rect(visible);
        let ui = &mut child;
        match tab {
            Tab::Viewport => self.app.central(ui),
            Tab::Assets => self.app.assets_ui(ui, &ctx),
            Tab::Properties => self.app.properties_ui(ui, &ctx),
            Tab::TextureSets => self.app.texture_sets_ui(ui),
            Tab::Layers => self.app.layers_ui(ui),
            Tab::SetSettings => {
                self.app.set_settings_visible = true;
                self.app.set_settings_ui(ui)
            }
        }
    }

    fn closeable(&mut self, tab: &mut Tab) -> bool {
        *tab != Tab::Viewport
    }

    fn allowed_in_windows(&self, tab: &mut Tab) -> bool {
        *tab != Tab::Viewport
    }

    fn clear_background(&self, tab: &Tab) -> bool {
        *tab != Tab::Viewport
    }

    fn scroll_bars(&self, _tab: &Tab) -> [bool; 2] {
        [false, false]
    }
}

impl RetroApp {
    /// All docked panels and the viewport, in the central area.
    pub(super) fn dock_ui(&mut self, ctx: &egui::Context) {
        let mut state = match self.dock.take() {
            Some(s) => s,
            None if !self.dock_loaded && self.persist_layout => saved_layout().unwrap_or_else(default_layout),
            None => default_layout(),
        };
        self.dock_loaded = true;
        if std::mem::take(&mut self.show_set_settings) {
            self.open_tab(&mut state, Tab::SetSettings);
        }
        self.set_settings_visible = false;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(crate::style::BG)).show(ctx, |ui| {
            if self.ui_hidden {
                // Tab: only the views.
                self.central(ui);
                return;
            }
            let mut viewer = Viewer { app: self, ctx: ctx.clone() };
            DockArea::new(&mut state)
                .id(egui::Id::new("main dock"))
                .style(style(ctx))
                .show_close_buttons(true)
                .tab_context_menus(true)
                .draggable_tabs(true)
                .show_inside(ui, &mut viewer);
        });
        // Remember the arrangement (checked twice a second, written on change).
        if self.persist_layout && self.dock_saved_at.elapsed().as_secs_f32() > 0.5 {
            self.dock_saved_at = Instant::now();
            if let Ok(json) = serde_json::to_string(&state) {
                if json != self.dock_saved_json {
                    let _ = std::fs::create_dir_all(layout_path().parent().unwrap_or(std::path::Path::new(".")));
                    let _ = std::fs::write(layout_path(), &json);
                    self.dock_saved_json = json;
                }
            }
        }
        self.dock = Some(state);
    }

    /// Whether a panel tab is open somewhere in the dock.
    pub(super) fn tab_open(&self, tab: Tab) -> bool {
        self.dock.as_ref().is_some_and(|d| d.find_tab(&tab).is_some())
    }

    /// Opens a panel tab (if needed) and brings it to the front.
    pub(super) fn focus_tab(&mut self, tab: Tab) {
        let mut state = self.dock.take().unwrap_or_else(default_layout);
        self.open_tab(&mut state, tab);
        self.dock = Some(state);
    }

    /// The tab exists and is the front tab of its group.
    pub(super) fn tab_visible(&self, tab: Tab) -> bool {
        let Some(d) = &self.dock else { return false };
        let Some((s, n, t)) = d.find_tab(&tab) else { return false };
        match &d[s][n] {
            egui_dock::Node::Leaf { active, .. } => *active == t,
            _ => true,
        }
    }

    /// Shows a panel tab (bringing a hidden one to the front), or closes it
    /// when it is already the one shown.
    pub(super) fn toggle_tab(&mut self, tab: Tab) {
        let visible = self.tab_visible(tab);
        let mut state = self.dock.take().unwrap_or_else(default_layout);
        match state.find_tab(&tab) {
            Some(found) if visible => {
                state.remove_tab(found);
            }
            _ => self.open_tab(&mut state, tab),
        }
        self.dock = Some(state);
    }

    fn open_tab(&mut self, state: &mut DockState<Tab>, tab: Tab) {
        if let Some(found) = state.find_tab(&tab) {
            state.set_active_tab(found);
            return;
        }
        // Next to a related panel when possible, else split off the viewport.
        let buddy = match tab {
            Tab::Layers | Tab::SetSettings => [Tab::SetSettings, Tab::Layers, Tab::TextureSets],
            Tab::TextureSets => [Tab::Layers, Tab::SetSettings, Tab::Properties],
            Tab::Assets => [Tab::Properties, Tab::Assets, Tab::Assets],
            Tab::Properties => [Tab::Assets, Tab::Properties, Tab::Properties],
            Tab::Viewport => [Tab::Viewport; 3],
        };
        for b in buddy {
            if let Some((surface, node, _)) = state.find_tab(&b) {
                state.set_focused_node_and_surface((surface, node));
                state.push_to_focused_leaf(tab);
                return;
            }
        }
        if let Some((surface, node, _)) = state.find_tab(&Tab::Viewport) {
            let left = matches!(tab, Tab::Assets | Tab::Properties);
            let tree = &mut state[surface];
            if left {
                tree.split_left(node, 0.2, vec![tab]);
            } else {
                tree.split_right(node, 0.75, vec![tab]);
            }
        }
    }
}
