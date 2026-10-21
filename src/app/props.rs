//! The Properties panel, laid out like the real application: a preview
//! header and icon tabs for the paint tools (BRUSH, ALPHA, STENCIL,
//! MATERIAL), FILL + MATERIAL for fill layers, and the filter / mask /
//! effect editors. Resource slots open a searchable picker and accept
//! assets dropped from the Assets panel.

use super::icons::{self, Icon};
use super::panels::{channels_table, effect_menu, filter_ui, proc_params, projection_ui, Cmd, TexSlot, CHANNEL_SHORT};
use super::shelf::{Category, Item};
use super::widgets::{self, PickItem, Slider};
use super::*;
use crate::brush::{AlphaKind, Alignment};
use crate::document::*;
use eframe::egui::{Color32, Pos2, RichText, Ui};
use std::collections::HashMap;

/// Thumbnails available to the panel this frame.
pub(super) struct Thumbs {
    pub res: HashMap<u64, egui::TextureId>,
    pub assets: HashMap<String, egui::TextureId>,
}

impl Thumbs {
    fn item(&self, it: &Item) -> Option<egui::TextureId> {
        match it {
            Item::ProjectTexture(id, _) => self.res.get(id).copied(),
            _ => self.assets.get(&it.key()).copied().filter(|t| *t != egui::TextureId::default()),
        }
    }
    fn pick_items(&self, items: &[Item]) -> Vec<PickItem> {
        items.iter().map(|it| PickItem { name: it.name(), thumb: self.item(it) }).collect()
    }
}

/// Asset lists offered by the slot pickers.
pub(super) struct Lists {
    alphas: Vec<Item>,
    textures: Vec<Item>,
    procedurals: Vec<Item>,
    materials: Vec<Item>,
}

/// What a fill channel slot was set to.
enum SlotPick {
    Uniform,
    Item(Item),
}

enum Mode {
    /// Paint tool settings (brush, alpha, stencil, material).
    Paint,
    /// Polygon fill, geometry mask, picker.
    Tool,
    /// The selected layer, mask or effect.
    Selection,
}

fn accepts_texture(it: &Item) -> bool {
    matches!(it, Item::Texture(_) | Item::ProjectTexture(..))
}

fn accepts_fill_source(it: &Item) -> bool {
    matches!(it, Item::Texture(_) | Item::ProjectTexture(..) | Item::Procedural(_) | Item::Material(..))
}

fn accepts_alpha(it: &Item) -> bool {
    matches!(it, Item::Alpha(_) | Item::AlphaFile(_) | Item::ProjectTexture(..) | Item::Texture(_))
}

/// Drop handling for a slot rect: (hovering with an accepted asset, dropped asset).
fn slot_drop(ui: &Ui, rect: Rect, accept: &dyn Fn(&Item) -> bool) -> (bool, Option<Item>) {
    let hovering = super::drop_hover::<Item>(ui, rect).is_some_and(|p| accept(&p));
    let dropped = if hovering { super::drop_take::<Item>(ui, rect).map(|i| (*i).clone()) } else { None };
    (hovering, dropped)
}

/// Rect the next 40 px slot will occupy (drops are tested before drawing so
/// the slot can show the drop highlight).
fn next_slot_rect(ui: &Ui) -> Rect {
    let probe = ui.available_rect_before_wrap();
    Rect::from_min_size(probe.min, egui::vec2(probe.width(), 40.0))
}

/// "Or" divider between the material slot and the channel slots.
fn or_divider(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), egui::Sense::hover());
    let p = ui.painter();
    let c = Color32::from_gray(70);
    p.line_segment([Pos2::new(r.min.x, r.center().y), Pos2::new(r.center().x - 14.0, r.center().y)], egui::Stroke::new(1.0f32, c));
    p.line_segment([Pos2::new(r.center().x + 14.0, r.center().y), Pos2::new(r.max.x, r.center().y)], egui::Stroke::new(1.0f32, c));
    p.text(r.center(), egui::Align2::CENTER_CENTER, "Or", egui::FontId::proportional(11.5), Color32::from_gray(170));
}

/// A section title shortened to leave room for `icons` title-line icons.
fn fit_title(ui: &Ui, title: &str, icons: usize) -> String {
    let room = ui.available_width() - icons as f32 * 24.0 - 12.0;
    let max = (room / 7.5).max(4.0) as usize;
    if title.chars().count() <= max {
        title.to_string()
    } else {
        title.chars().take(max.saturating_sub(3)).collect::<String>() + "..."
    }
}

fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(widgets::DIM));
}

/// Angle knob followed by a slider (brush angle).
fn angle_row(ui: &mut Ui, angle: &mut f32) {
    ui.horizontal(|ui| {
        let (r, resp) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::click_and_drag());
        let c = r.center();
        let p = ui.painter();
        p.circle_filled(c, 14.0, Color32::from_gray(110));
        p.circle_filled(c - egui::vec2(3.0, 3.0), 8.0, Color32::from_gray(150));
        p.circle_stroke(c, 14.0, egui::Stroke::new(1.5f32, if resp.hovered() { Color32::WHITE } else { Color32::from_gray(210) }));
        let a = angle.to_radians();
        let tip = c + egui::vec2(a.cos(), -a.sin()) * 13.0;
        p.line_segment([c, tip], egui::Stroke::new(1.5f32, Color32::WHITE));
        p.circle_filled(tip, 2.5, Color32::WHITE);
        if let Some(pos) = resp.interact_pointer_pos() {
            if resp.dragged() || resp.clicked() {
                let d = pos - c;
                *angle = (-d.y).atan2(d.x).to_degrees().round();
            }
        }
        ui.vertical(|ui| {
            Slider::new("Angle", angle, -180.0..=180.0).decimals(0).show(ui);
        });
    });
}

impl RetroApp {
    pub(super) fn properties_ui(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        if self.doc.is_none() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.label(RichText::new("No properties to show").size(11.5).color(widgets::DIM));
            });
            return;
        }
        let mut cmds = Vec::new();
        let (thumbs, lists) = self.slot_assets(ctx);
        self.properties_body(ui, ctx, &mut cmds, &thumbs, &lists);
        for c in cmds {
            self.run_cmd(c);
        }
    }

    /// Dock tab title: "Properties - Paint" and so on.
    pub(super) fn properties_title_short(&self) -> String {
        self.properties_title()
    }

    /// Everything under the Properties title (also shown by the viewport's
    /// right click quick menu).
    fn properties_body(&mut self, ui: &mut Ui, ctx: &egui::Context, cmds: &mut Vec<Cmd>, thumbs: &Thumbs, lists: &Lists) {
        match self.properties_mode() {
            Mode::Paint => self.paint_properties(ui, ctx, cmds, thumbs, lists),
            Mode::Tool => self.scroll_body(ui, |app, ui| app.tool_only_properties(ui)),
            Mode::Selection => {
                let fill = self.doc.as_ref().is_some_and(|d| {
                    let s = &d.sets[d.active];
                    s.target == Target::Content && matches!(s.selected_layer().map(|l| &l.desc.kind), Some(LayerKind::Fill(_)))
                });
                let mut jump = None;
                if fill {
                    jump = widgets::icon_tabs(ui, &[(Icon::FillTab, "Fill"), (Icon::Material, "Material")], self.props_tab.min(1));
                    if let Some(t) = jump {
                        self.props_tab = t;
                    }
                }
                self.scroll_body(ui, |app, ui| app.selection_properties(ui, cmds, thumbs, lists, jump));
            }
        }
    }

    fn scroll_body(&mut self, ui: &mut Ui, body: impl FnOnce(&mut Self, &mut Ui)) {
        egui::ScrollArea::vertical().id_salt("props").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::none().inner_margin(egui::Margin { left: 10.0, right: 12.0, top: 0.0, bottom: 12.0 }).show(ui, |ui| {
                // Never ask for more width than the panel has (the panel would grow).
                ui.set_max_width(ui.available_width());
                body(self, ui)
            });
        });
    }

    fn properties_mode(&self) -> Mode {
        let Some(doc) = &self.doc else { return Mode::Selection };
        let s = &doc.sets[doc.active];
        // Fill, filter and folder layers (and non-paint effects) always show
        // their own properties, whatever the tool.
        let own = match (s.selected_layer(), s.target) {
            (Some(l), Target::Content) => !matches!(l.desc.kind, LayerKind::Paint),
            (Some(l), Target::MaskEffect(e)) => !l.mask_paint.contains_key(&e),
            (Some(_), Target::ContentEffect(_)) => true,
            _ => false,
        };
        if own {
            return Mode::Selection;
        }
        if !self.tool.is_brush() {
            return Mode::Tool;
        }
        match (s.selected_layer().map(|l| &l.desc.kind), s.target) {
            (_, Target::MaskEffect(e)) => {
                if s.selected_layer().is_some_and(|l| l.mask_paint.contains_key(&e)) {
                    Mode::Paint
                } else {
                    Mode::Selection
                }
            }
            (Some(_), Target::Mask) => Mode::Paint,
            (Some(LayerKind::Paint), Target::Content) | (None, _) => Mode::Paint,
            _ => Mode::Selection,
        }
    }

    fn properties_title(&self) -> String {
        let Some(doc) = &self.doc else { return "Properties".into() };
        let s = &doc.sets[doc.active];
        let tool = match self.tool {
            Tool::Paint if self.stencil.enabled => "Projection",
            Tool::Paint => "Paint",
            Tool::Erase => "Eraser",
            Tool::Clone => "Clone",
            Tool::Smudge => "Smudge",
            Tool::PolygonFill => "Polygon fill",
            Tool::Picker => "Color picker",
            Tool::Hide => "Geometry mask",
        };
        match self.properties_mode() {
            Mode::Paint | Mode::Tool => format!("Properties - {tool}"),
            Mode::Selection => match (s.selected_layer(), s.target) {
                (Some(_), Target::Mask) => "Properties - Mask".into(),
                (Some(l), Target::MaskEffect(e)) => format!(
                    "Properties - {}",
                    l.desc.mask.as_ref().and_then(|m| m.effects.iter().find(|x| x.id == e)).map(|x| x.kind.label()).unwrap_or_else(|| "Effect".into())
                ),
                (Some(_), Target::ContentEffect(_)) => "Properties - Filter".into(),
                (Some(l), Target::Content) => format!(
                    "Properties - {}",
                    match &l.desc.kind {
                        LayerKind::Paint => "Paint layer",
                        LayerKind::Fill(_) => "Fill",
                        LayerKind::Filter(_) => "Filter",
                        LayerKind::Folder => "Folder",
                    }
                ),
                _ => format!("Properties - {tool}"),
            },
        }
    }

    fn slot_assets(&mut self, ctx: &egui::Context) -> (Thumbs, Lists) {
        let mut res = HashMap::new();
        let mut project_tex = Vec::new();
        let mut project_alpha = Vec::new();
        if let Some(doc) = &self.doc {
            let mut v: Vec<&Resource> = doc.resources.values().collect();
            v.sort_by_key(|r| r.id);
            for r in v {
                res.insert(r.id, self.shelf.texture_for_resource(ctx, r));
                let it = Item::ProjectTexture(r.id, r.name.clone());
                if r.kind == ResKind::Alpha {
                    project_alpha.push(it);
                } else {
                    project_tex.push(it);
                }
            }
        }
        let mut alphas = self.shelf.items_of(Category::Alphas);
        alphas.extend(project_alpha);
        let mut textures = project_tex;
        textures.extend(self.shelf.items_of(Category::Textures));
        let procedurals = self.shelf.items_of(Category::Procedurals);
        let materials = self.shelf.items_of(Category::Materials);
        for it in alphas.iter().chain(&procedurals).chain(&materials).chain(textures.iter()) {
            if !matches!(it, Item::ProjectTexture(..)) {
                self.shelf.want(it.clone());
            }
        }
        (Thumbs { res, assets: self.shelf.thumb_map() }, Lists { alphas, textures, procedurals, materials })
    }

    // ------------------------------------------------------------ paint tools

    fn paint_properties(&mut self, ui: &mut Ui, ctx: &egui::Context, cmds: &mut Vec<Cmd>, th: &Thumbs, lists: &Lists) {
        // Preview header: stroke on the left, brush material on the right.
        let w = ui.available_width();
        let h = (w * 0.42).clamp(80.0, 150.0);
        let (r, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        ui.painter().rect_filled(r, 0.0, Color32::from_gray(20));
        let half = Rect::from_min_max(r.min, Pos2::new(r.center().x, r.max.y));
        if let Some(t) = self.stroke_preview_texture(ctx) {
            ui.painter().image(t, half.shrink(4.0), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        let ball = Rect::from_center_size(Pos2::new(r.center().x + r.width() * 0.25, r.center().y), egui::vec2(h - 6.0, h - 6.0));
        if let Some(t) = self.brush_ball.as_ref().map(|b| b.2) {
            ui.painter().image(t, ball, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        let tabs = [(Icon::Brush, "Brush"), (Icon::Alpha, "Alpha"), (Icon::Stencil, "Stencil"), (Icon::Material, "Material")];
        let jump = widgets::icon_tabs(ui, &tabs, self.props_tab.min(3));
        if let Some(t) = jump {
            self.props_tab = t;
        }
        self.scroll_body(ui, |app, ui| {
            let header = app.mask_target_header(ui, cmds);
            let r = widgets::section_title(ui, "Brush", !header);
            if jump == Some(0) {
                ui.scroll_to_rect(r, Some(egui::Align::TOP));
            }
            app.brush_section(ui, r, th);
            let r = widgets::section_title(ui, "Alpha", false);
            if jump == Some(1) {
                ui.scroll_to_rect(r, Some(egui::Align::TOP));
            }
            app.alpha_section(ui, cmds, th, lists);
            let r = widgets::section_title(ui, "Stencil", false);
            if jump == Some(2) {
                ui.scroll_to_rect(r, Some(egui::Align::TOP));
            }
            app.stencil_section(ui, cmds, th, lists);
            let r = widgets::section_title(ui, "Material", false);
            if jump == Some(3) {
                ui.scroll_to_rect(r, Some(egui::Align::TOP));
            }
            app.brush_material_section(ui, th, lists);
        });
    }

    fn brush_section(&mut self, ui: &mut Ui, title: Rect, th: &Thumbs) {
        // Preset menu on the section title line.
        let mr = Rect::from_center_size(Pos2::new(ui.max_rect().max.x - 10.0, title.center().y), egui::vec2(20.0, 20.0));
        let mresp = ui.interact(mr, ui.id().with("brush presets"), egui::Sense::click()).on_hover_text("Brush presets");
        icons::paint(ui.painter(), mr.shrink(2.0), Icon::Menu, if mresp.hovered() { Color32::WHITE } else { Color32::from_gray(170) });
        let pid = ui.id().with("brush preset popup");
        if mresp.clicked() {
            ui.memory_mut(|m| m.toggle_popup(pid));
        }
        let mut presets = Vec::new();
        if ui.memory(|m| m.is_popup_open(pid)) {
            presets = crate::brush::builtin_brushes();
            presets.extend(crate::library::user_brushes());
        }
        let items: Vec<PickItem> = presets
            .iter()
            .map(|b| PickItem { name: b.name.clone(), thumb: th.assets.get(&format!("brush:{}", b.name)).copied().filter(|t| *t != egui::TextureId::default()) })
            .collect();
        if let Some(i) = widgets::picker_popup(ui, pid, &mresp, "Brush presets", &items) {
            // A preset is a brush tip: size, symmetry, lazy mouse and pen
            // settings stay as the user set them.
            let keep = self.brush.clone();
            self.brush = presets[i].settings.clone();
            self.brush.size = keep.size;
            self.brush.symmetry = keep.symmetry;
            self.brush.lazy = keep.lazy;
            self.brush.pressure_size = keep.pressure_size;
            self.brush.pressure_flow = keep.pressure_flow;
            self.status = format!("Brush: {}", presets[i].name);
        }
        let b = &mut self.brush;
        let pen = |ui: &mut Ui, on: &mut bool, tip: &str| {
            ui.add_space(8.0);
            if icons::button(ui, Icon::Pen, *on, 22.0, tip).clicked() {
                *on = !*on;
            }
        };
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 44.0);
                Slider::new("Size", &mut b.size, 1.0..=1000.0).log().decimals(0).show(ui);
            });
            pen(ui, &mut b.pressure_size, "Pen pressure drives the size");
        });
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 44.0);
                Slider::new("Flow", &mut b.flow, 0.01..=1.0).percent().show(ui);
            });
            pen(ui, &mut b.pressure_flow, "Pen pressure drives the flow");
        });
        Slider::new("Spacing", &mut b.spacing, 0.02..=2.0).percent().show(ui);
        angle_row(ui, &mut b.angle);
        widgets::toggle_row(ui, "Follow path", &mut b.follow_path);
        Slider::new("Size jitter", &mut b.size_jitter, 0.0..=1.0).percent().show(ui);
        Slider::new("Flow jitter", &mut b.flow_jitter, 0.0..=1.0).percent().show(ui);
        Slider::new("Angle jitter", &mut b.angle_jitter, 0.0..=180.0).decimals(0).show(ui);
        Slider::new("Position jitter", &mut b.position_jitter, 0.0..=2.0).percent().show(ui);
        widgets::combo_row(ui, "Alignment", "align2", &mut b.alignment, &Alignment::ALL, Alignment::name);
        widgets::toggle_row(ui, "Backface culling", &mut b.angle_fade);
        ui.add_space(2.0);
        if ui.add(egui::Button::new("Save as brush preset").min_size(egui::vec2(ui.available_width(), 22.0))).clicked() {
            let preset = crate::brush::BrushPreset { name: format!("Brush {}", super::panels::chrono_like_stamp()), settings: self.brush.clone() };
            match crate::library::save_brush(&preset) {
                Ok(p) => {
                    self.shelf.refresh();
                    self.status = format!("Brush saved: {}", p.display());
                }
                Err(e) => self.status = format!("Could not save brush: {e:#}"),
            }
        }
    }

    fn alpha_section(&mut self, ui: &mut Ui, cmds: &mut Vec<Cmd>, th: &Thumbs, lists: &Lists) {
        let (thumb, name, sub) = match &self.brush.alpha {
            AlphaSource::Builtin(k) => (th.item(&Item::Alpha(*k)), k.name().to_string(), "Shape"),
            AlphaSource::Resource(id) => (
                th.res.get(id).copied(),
                self.doc.as_ref().and_then(|d| d.resources.get(id)).map(|r| r.name.clone()).unwrap_or_default(),
                "Image",
            ),
        };
        let (hovering, dropped) = slot_drop(ui, next_slot_rect(ui), &accepts_alpha);
        let (resp, cleared) = widgets::resource_card(ui, thumb, &name, sub, self.brush.alpha != AlphaSource::Builtin(AlphaKind::Round), hovering);
        super::mark(ui.ctx(), "slot:Alpha", resp.rect);
        if cleared {
            self.brush.alpha = AlphaSource::Builtin(AlphaKind::Round);
        }
        let pid = ui.id().with("alpha picker");
        if resp.clicked() && !cleared {
            ui.memory_mut(|m| m.toggle_popup(pid));
        }
        let picked = widgets::picker_popup(ui, pid, &resp, "Alphas", &th.pick_items(&lists.alphas)).map(|i| lists.alphas[i].clone());
        for it in dropped.into_iter().chain(picked) {
            match it {
                Item::Alpha(k) => self.brush.alpha = AlphaSource::Builtin(k),
                Item::ProjectTexture(id, _) => self.brush.alpha = AlphaSource::Resource(id),
                Item::AlphaFile(p) | Item::Texture(p) => cmds.push(Cmd::AssignFile(p, TexSlot::Alpha)),
                _ => {}
            }
        }
        widgets::sub_section(ui, "alpha params", "Parameters", true, |ui| {
            Slider::new("Hardness", &mut self.brush.hardness, 0.0..=1.0).percent().show(ui);
        });
    }

    fn stencil_section(&mut self, ui: &mut Ui, cmds: &mut Vec<Cmd>, th: &Thumbs, lists: &Lists) {
        let name = self.stencil.resource.and_then(|id| self.doc.as_ref()?.resources.get(&id)).map(|r| r.name.clone());
        let st = &mut self.stencil;
        let (hovering, dropped) = slot_drop(ui, next_slot_rect(ui), &accepts_texture);
        let (resp, cleared) = match &name {
            Some(n) => widgets::resource_card(ui, st.resource.and_then(|id| th.res.get(&id).copied()), n, "Stencil", true, hovering),
            None => (widgets::slot_box(ui, "Stencil", "No resource selected", None, hovering), false),
        };
        super::mark(ui.ctx(), "slot:Stencil", resp.rect);
        if cleared {
            st.resource = None;
            st.enabled = false;
        }
        let pid = ui.id().with("stencil picker");
        if resp.clicked() && !cleared {
            ui.memory_mut(|m| m.toggle_popup(pid));
        }
        let picked = widgets::picker_popup(ui, pid, &resp, "Textures", &th.pick_items(&lists.textures)).map(|i| lists.textures[i].clone());
        for it in dropped.into_iter().chain(picked) {
            match it {
                Item::ProjectTexture(id, _) => {
                    st.resource = Some(id);
                    st.enabled = true;
                }
                Item::Texture(p) => cmds.push(Cmd::AssignFile(p, TexSlot::Stencil)),
                _ => {}
            }
        }
        if st.resource.is_some() {
            widgets::toggle_row(ui, "Projection", &mut st.enabled);
            Slider::new("Size", &mut st.size, 10.0..=4000.0).log().decimals(0).show(ui);
            Slider::new("Rotation", &mut st.rotation, -180.0..=180.0).decimals(0).show(ui);
            Slider::new("Overlay opacity", &mut st.opacity, 0.0..=1.0).percent().show(ui);
            widgets::toggle_row(ui, "Color", &mut st.drive_color);
            widgets::toggle_row(ui, "Height", &mut st.drive_height);
            if st.drive_height {
                Slider::new("Height strength", &mut st.height_strength, 0.0..=2.0).show(ui);
            }
            hint(ui, "S + left drag moves, S + right drag scales and rotates.");
        }
    }

    fn brush_material_section(&mut self, ui: &mut Ui, th: &Thumbs, lists: &Lists) {
        let target_is_mask = self.doc.as_ref().map(|d| d.sets[d.active].target.is_mask()).unwrap_or(false);
        let erase = self.tool == Tool::Erase;
        let m = &mut self.mat;
        if target_is_mask {
            ui.label(RichText::new("Grayscale").size(12.0).color(widgets::LABEL));
            widgets::gradient_slider(ui, "mask value", &mut m.mask_value, 0.0, 1.0, Color32::BLACK, Color32::WHITE);
            if ui.add(egui::Button::new("Swap black / white (X)").min_size(egui::vec2(ui.available_width(), 22.0))).clicked() {
                m.mask_value = 1.0 - m.mask_value;
            }
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for c in 0..NCH {
                if widgets::chip(ui, CHANNEL_SHORT[c], m.channels[c], CHANNELS[c]).clicked() {
                    m.channels[c] = !m.channels[c];
                }
            }
        });
        ui.add_space(4.0);
        if erase {
            // Like Painter: the eraser only picks the channels it removes.
            hint(ui, "The eraser removes every enabled channel.");
            return;
        }
        let (hovering, dropped) = slot_drop(ui, next_slot_rect(ui), &|it| matches!(it, Item::Material(..)));
        let (resp, cleared) = match &m.material {
            Some((n, _)) => widgets::resource_card(ui, th.assets.get(&format!("mat:{n}")).copied(), n, "Material", true, hovering),
            None => (widgets::slot_box(ui, "Material mode", "No resource selected", None, hovering), false),
        };
        super::mark(ui.ctx(), "slot:Material mode", resp.rect);
        if cleared {
            m.material = None;
        }
        let pid = ui.id().with("brush mat picker");
        if resp.clicked() && !cleared {
            ui.memory_mut(|mm| mm.toggle_popup(pid));
        }
        let picked = widgets::picker_popup(ui, pid, &resp, "Materials", &th.pick_items(&lists.materials)).map(|i| lists.materials[i].clone());
        if let Some(Item::Material(n, f)) = dropped.or(picked) {
            m.material = Some((n, f));
        }
        if m.material.is_some() {
            return;
        }
        or_divider(ui);
        for c in 0..NCH {
            if !m.channels[c] {
                continue;
            }
            ui.label(RichText::new(CHANNELS[c]).size(12.0).color(widgets::LABEL));
            match c {
                CH_COLOR => {
                    widgets::color_bar(ui, "brush color", &mut m.color);
                }
                CH_ROUGH => {
                    widgets::gradient_slider(ui, "brush rough", &mut m.roughness, 0.0, 1.0, Color32::BLACK, Color32::WHITE);
                }
                CH_METAL => {
                    widgets::gradient_slider(ui, "brush metal", &mut m.metallic, 0.0, 1.0, Color32::BLACK, Color32::WHITE);
                }
                CH_HEIGHT => {
                    widgets::gradient_slider(ui, "brush height", &mut m.height, -1.0, 1.0, Color32::BLACK, Color32::WHITE);
                }
                CH_EMISSIVE => {
                    widgets::color_bar(ui, "brush emissive", &mut m.emissive);
                }
                CH_OPACITY => {
                    widgets::gradient_slider(ui, "brush opacity", &mut m.opacity, 0.0, 1.0, Color32::BLACK, Color32::WHITE);
                }
                _ => {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), egui::Sense::hover());
                    ui.painter().rect_filled(r.shrink(2.0), 1.0, Color32::from_rgb(128, 128, 255));
                }
            }
            ui.add_space(6.0);
        }
        if erase {
            hint(ui, "The eraser removes every enabled channel.");
        }
    }

    /// Stroke preview image (alpha stamped along a curve), cached.
    fn stroke_preview_texture(&mut self, ctx: &egui::Context) -> Option<egui::TextureId> {
        let b = &self.brush;
        let key = format!("{:?}|{:.2}|{:.2}|{:.0}", b.alpha, b.hardness, b.spacing, b.angle);
        if let Some((k, h)) = &self.stroke_preview {
            if *k == key {
                return Some(h.id());
            }
        }
        let alpha: image::GrayImage = match &b.alpha {
            AlphaSource::Builtin(k) => image::DynamicImage::ImageRgba8(crate::brush::alpha_image(*k)).to_luma8(),
            AlphaSource::Resource(id) => match self.doc.as_ref().and_then(|d| d.resources.get(id)) {
                Some(r) => image::DynamicImage::ImageRgba8((*r.image).clone()).to_luma8(),
                None => image::DynamicImage::ImageRgba8(crate::brush::alpha_image(AlphaKind::Round)).to_luma8(),
            },
        };
        let (w, h) = (176usize, 128usize);
        let mut acc = vec![0.0f32; w * h];
        let rad = 15.0f32;
        let step = (b.spacing * rad * 2.0).max(1.0);
        let curve = |t: f32| -> (f32, f32) {
            let x = 18.0 + t * (w as f32 - 36.0);
            let y = h as f32 * 0.5 - (t * std::f32::consts::PI * 2.4).sin() * h as f32 * 0.28 + (t - 0.5) * 10.0;
            (x, y)
        };
        let (aw, ah) = (alpha.width() as f32, alpha.height() as f32);
        let (sa, ca) = b.angle.to_radians().sin_cos();
        let mut t = 0.0f32;
        let mut prev = curve(0.0);
        let mut travelled = step;
        while t <= 1.0 {
            let c = curve(t);
            travelled += ((c.0 - prev.0).powi(2) + (c.1 - prev.1).powi(2)).sqrt();
            prev = c;
            t += 0.002;
            if travelled < step {
                continue;
            }
            travelled = 0.0;
            let (x0, x1) = ((c.0 - rad).floor().max(0.0) as usize, ((c.0 + rad).ceil() as usize).min(w - 1));
            let (y0, y1) = ((c.1 - rad).floor().max(0.0) as usize, ((c.1 + rad).ceil() as usize).min(h - 1));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (dx, dy) = ((x as f32 - c.0) / rad, (y as f32 - c.1) / rad);
                    let (u, v) = (dx * ca + dy * sa, -dx * sa + dy * ca);
                    if u.abs() > 1.0 || v.abs() > 1.0 {
                        continue;
                    }
                    let px = (((u * 0.5 + 0.5) * aw) as u32).min(alpha.width() - 1);
                    let py = (((v * 0.5 + 0.5) * ah) as u32).min(alpha.height() - 1);
                    let a = alpha.get_pixel(px, py).0[0] as f32 / 255.0;
                    let r = (u * u + v * v).sqrt();
                    let edge = ((1.0 - r) / (1.0 - b.hardness + 0.05)).clamp(0.0, 1.0);
                    let s = a * edge * 0.6;
                    let o = &mut acc[y * w + x];
                    *o += (1.0 - *o) * s;
                }
            }
        }
        let rgba: Vec<u8> = acc
            .iter()
            .flat_map(|a| {
                let g = (a.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
                [g, g, g, 255]
            })
            .collect();
        let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
        let handle = ctx.load_texture("stroke preview", img, egui::TextureOptions::LINEAR);
        let id = handle.id();
        self.stroke_preview = Some((key, handle));
        Some(id)
    }

    /// Material ball of the brush material, re-rendered when it changes.
    pub(super) fn update_brush_ball(&mut self) {
        use std::hash::{Hash, Hasher};
        let fill = match &self.mat.material {
            Some((_, f)) => f.clone(),
            None => {
                let m = &self.mat;
                let mut f = FillDesc::material(m.color, m.roughness, m.metallic);
                f.channels[CH_EMISSIVE].enabled = m.channels[CH_EMISSIVE];
                f.channels[CH_EMISSIVE].value = m.emissive;
                f
            }
        };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{fill:?}").hash(&mut h);
        let key = h.finish();
        if self.brush_ball.as_ref().is_some_and(|b| b.0 == key) || self.brush_ball_at.elapsed().as_secs_f32() < 0.1 {
            return;
        }
        self.brush_ball_at = Instant::now();
        let tree = LayerTree { desc: LayerDesc::new(0, String::new(), LayerKind::Fill(Box::new(fill))), children: Vec::new() };
        let empty = HashMap::new();
        let resources = self.doc.as_ref().map(|d| &d.resources).unwrap_or(&empty);
        let tex = self.shelf.preview(&self.engine, &tree, resources);
        let mut r = self.renderer.write();
        match &mut self.brush_ball {
            Some(b) => {
                r.update_egui_texture_from_wgpu_texture(&self.engine.device, &tex.view, wgpu::FilterMode::Linear, b.2);
                b.0 = key;
                b.1 = tex;
            }
            None => {
                let id = r.register_native_texture(&self.engine.device, &tex.view, wgpu::FilterMode::Linear);
                self.brush_ball = Some((key, tex, id));
            }
        }
    }

    fn tool_only_properties(&mut self, ui: &mut Ui) {
        let title = self.properties_title().replace("Properties - ", "");
        widgets::section_title(ui, &title, true);
        match self.tool {
            Tool::PolygonFill | Tool::Hide => {
                widgets::combo_row(ui, "Selection mode", "polymode", &mut self.poly_mode, &PolyMode::ALL, PolyMode::name);
                hint(
                    ui,
                    if self.tool == Tool::Hide {
                        "Click hides a part, Ctrl + click keeps only it."
                    } else {
                        "Click fills the selected layer (or its mask). The eraser unfills."
                    },
                );
            }
            _ => hint(ui, "Click the mesh to pick its base color, roughness, metallic and height into the brush."),
        }
    }

    // ------------------------------------------------------------ selection

    fn selection_properties(&mut self, ui: &mut Ui, cmds: &mut Vec<Cmd>, th: &Thumbs, lists: &Lists, jump: Option<usize>) {
        let doc = self.doc.as_mut().unwrap();
        let Document { sets, resources, active, .. } = doc;
        let s = &mut sets[*active];
        let target = s.target;
        let Some(lid) = s.selected else {
            widgets::section_title(ui, "Layer", true);
            hint(ui, "No layer selected.");
            return;
        };
        let ext_on = s.settings.ext_channels;
        let Some(l) = find_mut(&mut s.layers, lid) else { return };
        match target {
            Target::Content => {
                let id = l.desc.id;
                match &mut l.desc.kind {
                    LayerKind::Fill(f) => {
                        let r = widgets::section_title(ui, "Fill", true);
                        if jump == Some(0) {
                            ui.scroll_to_rect(r, Some(egui::Align::TOP));
                        }
                        projection_ui(ui, ("fillproj", id), &mut f.projection);
                        let r = widgets::section_title(ui, "Material", false);
                        if jump == Some(1) {
                            ui.scroll_to_rect(r, Some(egui::Align::TOP));
                        }
                        fill_material_ui(ui, id, f, resources, th, lists, cmds);
                        ext_channels_ui(ui, id, f, ext_on, th, lists);
                    }
                    LayerKind::Filter(f) => {
                        widgets::section_title(ui, "Filter", true);
                        filter_ui(ui, id, f);
                    }
                    LayerKind::Folder => {
                        widgets::section_title(ui, "Folder", true);
                        hint(ui, "Children composite in place; the folder mask and opacity apply to the result.");
                    }
                    LayerKind::Paint => {
                        widgets::section_title(ui, "Paint layer", true);
                        hint(ui, "Select the Paint tool (1) to paint on this layer.");
                    }
                }
                widgets::section_title(ui, "Blending", false);
                channels_table(ui, &mut l.desc);
                if ext_on.iter().any(|e| *e) {
                    Slider::new("Optional channels opacity", &mut l.desc.ext_opacity, 0.0..=1.0).percent().show(ui);
                }
            }
            Target::Mask => {
                widgets::section_title(ui, "Mask", true);
                if let Some(m) = &mut l.desc.mask {
                    widgets::toggle_row(ui, "Mask", &mut m.enabled);
                    ui.horizontal(|ui| {
                        ui.add_sized([96.0, 22.0], egui::Label::new(RichText::new("Background").size(12.0).color(widgets::LABEL)));
                        ui.selectable_value(&mut m.base, 1.0, "White");
                        ui.selectable_value(&mut m.base, 0.0, "Black");
                    });
                    ui.add_space(4.0);
                    ui.menu_button("Add effect", |ui| effect_menu(ui, cmds));
                    hint(ui, "Effects stack bottom to top. Drop a smart mask on the layer row to replace the mask.");
                }
            }
            Target::ContentEffect(eid) => {
                let Some(e) = l.desc.effects.iter_mut().find(|e| e.id == eid) else { return };
                let title = fit_title(ui, e.filter.kind.name(), 3);
                let r = widgets::section_title(ui, &title, true);
                let mut x = ui.max_rect().max.x - 12.0;
                for (icon, tip, cmd) in [
                    (Icon::Trash, "Delete effect", Cmd::RemoveContentEffect(eid)),
                    (Icon::Chevron, "Move down", Cmd::MoveContentEffect(eid, -1)),
                    (Icon::ChevronUp, "Move up", Cmd::MoveContentEffect(eid, 1)),
                ] {
                    let ir = Rect::from_center_size(Pos2::new(x, r.center().y), egui::vec2(20.0, 20.0));
                    let resp = ui.interact(ir, ui.id().with(("cfxtool", tip)), egui::Sense::click()).on_hover_text(tip);
                    icons::paint(ui.painter(), ir.shrink(3.0), icon, if resp.hovered() { Color32::WHITE } else { Color32::from_gray(160) });
                    if resp.clicked() {
                        cmds.push(cmd);
                    }
                    x -= 24.0;
                }
                Slider::new("Opacity", &mut e.opacity, 0.0..=1.0).percent().show(ui);
                filter_ui(ui, eid, &mut e.filter);
            }
            Target::MaskEffect(eid) => {
                let Some(m) = &mut l.desc.mask else { return };
                let Some(e) = m.effects.iter_mut().find(|e| e.id == eid) else { return };
                let title = fit_title(ui, &e.kind.label(), 3);
                let r = widgets::section_title(ui, &title, true);
                // Reorder / delete icons on the title line.
                let mut x = ui.max_rect().max.x - 12.0;
                for (icon, tip, cmd) in [
                    (Icon::Trash, "Delete effect", Cmd::RemoveEffect(eid)),
                    (Icon::Chevron, "Move down", Cmd::MoveEffect(eid, -1)),
                    (Icon::ChevronUp, "Move up", Cmd::MoveEffect(eid, 1)),
                ] {
                    let ir = Rect::from_center_size(Pos2::new(x, r.center().y), egui::vec2(20.0, 20.0));
                    let resp = ui.interact(ir, ui.id().with(("efftool", tip)), egui::Sense::click()).on_hover_text(tip);
                    icons::paint(ui.painter(), ir.shrink(3.0), icon, if resp.hovered() { Color32::WHITE } else { Color32::from_gray(160) });
                    if resp.clicked() {
                        cmds.push(cmd);
                    }
                    x -= 24.0;
                }
                effect_props(ui, lid, e, resources, th, lists, cmds);
            }
        }
    }
}

/// Fill channel slot with picker and drop support. Returns what was chosen.
#[allow(clippy::too_many_arguments)]
fn channel_slot(
    ui: &mut Ui,
    id: impl std::hash::Hash + Copy,
    title: &str,
    subtitle: &str,
    thumb: Option<egui::TextureId>,
    accept: &dyn Fn(&Item) -> bool,
    th: &Thumbs,
    lists: &[&[Item]],
) -> Option<SlotPick> {
    let (hovering, dropped) = slot_drop(ui, next_slot_rect(ui), accept);
    let resp = widgets::slot_box(ui, title, subtitle, thumb, hovering);
    super::mark(ui.ctx(), format!("slot:{title}"), resp.rect);
    let pid = ui.id().with(("slot picker", id));
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(pid));
    }
    let mut all: Vec<Item> = Vec::new();
    for l in lists {
        all.extend(l.iter().cloned());
    }
    let mut items = vec![PickItem { name: "Uniform".into(), thumb: None }];
    items.extend(th.pick_items(&all));
    let picked = widgets::picker_popup(ui, pid, &resp, "Resources", &items);
    if let Some(it) = dropped {
        return Some(SlotPick::Item(it));
    }
    match picked {
        Some(0) => Some(SlotPick::Uniform),
        Some(i) => Some(SlotPick::Item(all[i - 1].clone())),
        None => None,
    }
}

/// The texture set's optional channels (specular level, transmission) on a
/// fill: a chip each, then a slot with a uniform value or a procedural.
fn ext_channels_ui(ui: &mut Ui, id: u64, f: &mut FillDesc, ext_on: [bool; NEXT], th: &Thumbs, lists: &Lists) {
    if !ext_on.iter().any(|e| *e) {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for i in 0..NEXT {
            if ext_on[i] && widgets::chip(ui, EXT_SHORT[i], f.ext[i].enabled, EXT_NAMES[i]).clicked() {
                f.ext[i].enabled = !f.ext[i].enabled;
            }
        }
    });
    ui.add_space(4.0);
    for i in 0..NEXT {
        if !ext_on[i] || !f.ext[i].enabled {
            continue;
        }
        let ch = &mut f.ext[i];
        let (thumb, subtitle) = match ch.source {
            SourceKind::Procedural => (th.item(&Item::Procedural(ch.procedural.kind)), ch.procedural.kind.name().to_string()),
            _ => (None, "uniform color".to_string()),
        };
        let pick = channel_slot(ui, (id, "ext", i), EXT_NAMES[i], &subtitle, thumb, &|it| matches!(it, Item::Procedural(_)), th, &[&lists.procedurals]);
        match pick {
            Some(SlotPick::Uniform) => ch.source = SourceKind::Uniform,
            Some(SlotPick::Item(Item::Procedural(k))) => {
                ch.source = SourceKind::Procedural;
                ch.procedural = ProcParams::of(k);
                ch.value = [0.0; 3];
                ch.value_b = [1.0; 3];
            }
            _ => {}
        }
        ui.add_space(2.0);
        if ch.source == SourceKind::Procedural {
            let (mut a, mut b) = (ch.value[0], ch.value_b[0]);
            Slider::new("Range min", &mut a, 0.0..=1.0).show(ui);
            Slider::new("Range max", &mut b, 0.0..=1.0).show(ui);
            ch.value = [a; 3];
            ch.value_b = [b; 3];
            proc_params(ui, ("extproc", id, i), &mut ch.procedural);
        } else {
            let mut v = ch.value[0];
            if widgets::gradient_slider(ui, ("ext value", id, i), &mut v, 0.0, 1.0, Color32::BLACK, Color32::WHITE) {
                ch.value = [v; 3];
            }
        }
        ui.add_space(6.0);
    }
}

/// Fill layer material: chips, material mode slot, then one slot per enabled
/// channel with its value editor.
fn fill_material_ui(ui: &mut Ui, id: u64, f: &mut FillDesc, resources: &HashMap<u64, Resource>, th: &Thumbs, lists: &Lists, cmds: &mut Vec<Cmd>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for c in 0..NCH {
            if widgets::chip(ui, CHANNEL_SHORT[c], f.channels[c].enabled, CHANNELS[c]).clicked() {
                f.channels[c].enabled = !f.channels[c].enabled;
            }
        }
    });
    ui.add_space(4.0);
    let mut replace: Option<FillDesc> = None;
    {
        let (hovering, dropped) = slot_drop(ui, next_slot_rect(ui), &|it| matches!(it, Item::Material(..)));
        let (resp, cleared) = match &f.material {
            Some(n) => widgets::resource_card(ui, th.assets.get(&format!("mat:{n}")).copied(), n, "Material", true, hovering),
            None => (widgets::slot_box(ui, "Material mode", "No resource selected", None, hovering), false),
        };
        super::mark(ui.ctx(), "slot:Material mode", resp.rect);
        if cleared {
            // Keeps the channels, forgets where they came from.
            f.material = None;
        }
        let pid = ui.id().with(("fill mat picker", id));
        if resp.clicked() && !cleared {
            ui.memory_mut(|m| m.toggle_popup(pid));
        }
        let picked = widgets::picker_popup(ui, pid, &resp, "Materials", &th.pick_items(&lists.materials)).map(|i| lists.materials[i].clone());
        if let Some(Item::Material(n, mut fd)) = dropped.or(picked) {
            fd.material = Some(n);
            replace = Some(fd);
        }
    }
    or_divider(ui);
    for c in 0..NCH {
        let ch = &mut f.channels[c];
        if !ch.enabled {
            continue;
        }
        let (thumb, subtitle) = match ch.source {
            SourceKind::Uniform => (None, "uniform color".to_string()),
            SourceKind::Texture => (
                ch.texture.and_then(|i| th.res.get(&i).copied()),
                ch.texture.and_then(|i| resources.get(&i)).map(|r| r.name.clone()).unwrap_or("missing texture".into()),
            ),
            SourceKind::Procedural => (th.item(&Item::Procedural(ch.procedural.kind)), ch.procedural.kind.name().to_string()),
        };
        let pick = channel_slot(ui, (id, c), CHANNELS[c], &subtitle, thumb, &accepts_fill_source, th, &[&lists.textures, &lists.procedurals]);
        match pick {
            Some(SlotPick::Uniform) => ch.source = SourceKind::Uniform,
            Some(SlotPick::Item(Item::ProjectTexture(rid, _))) => cmds.push(Cmd::AssignResource(rid, TexSlot::FillChannel(id, c))),
            Some(SlotPick::Item(Item::Texture(p))) => cmds.push(Cmd::AssignFile(p, TexSlot::FillChannel(id, c))),
            Some(SlotPick::Item(Item::Procedural(k))) => {
                ch.source = SourceKind::Procedural;
                ch.procedural = ProcParams::of(k);
                if is_color_channel(c) {
                    ch.value = [0.05; 3];
                    ch.value_b = [0.9; 3];
                } else {
                    ch.value = [0.0; 3];
                    ch.value_b = [1.0; 3];
                }
            }
            Some(SlotPick::Item(Item::Material(n, mut fd))) => {
                fd.material = Some(n);
                replace = Some(fd);
            }
            _ => {}
        }
        ui.add_space(2.0);
        match ch.source {
            SourceKind::Uniform => {
                if is_color_channel(c) {
                    widgets::color_bar(ui, ("fill color", id, c), &mut ch.value);
                } else if c == CH_NORMAL {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), egui::Sense::hover());
                    ui.painter().rect_filled(r.shrink(2.0), 1.0, Color32::from_rgb(128, 128, 255));
                } else {
                    let (lo, hi) = if c == CH_HEIGHT { (-1.0, 1.0) } else { (0.0, 1.0) };
                    let mut v = ch.value[0];
                    if widgets::gradient_slider(ui, ("fill value", id, c), &mut v, lo, hi, Color32::BLACK, Color32::WHITE) {
                        ch.value = [v; 3];
                    }
                }
            }
            SourceKind::Texture => {
                if is_color_channel(c) {
                    ui.label(RichText::new("Tint").size(12.0).color(widgets::LABEL));
                    widgets::color_bar(ui, ("fill tint", id, c), &mut ch.value);
                } else if c == CH_NORMAL {
                    Slider::new("Strength", &mut ch.value_b[0], 0.0..=2.0).show(ui);
                    widgets::toggle_row(ui, "DirectX", &mut ch.flip_green);
                } else {
                    Slider::new("Range min", &mut ch.value[0], -1.0..=1.0).show(ui);
                    Slider::new("Range max", &mut ch.value_b[0], -1.0..=2.0).show(ui);
                }
            }
            SourceKind::Procedural => {
                if is_color_channel(c) {
                    ui.label(RichText::new("Colors").size(12.0).color(widgets::LABEL));
                    widgets::color_bar(ui, ("proc a", id, c), &mut ch.value);
                    widgets::color_bar(ui, ("proc b", id, c), &mut ch.value_b);
                } else if c != CH_NORMAL {
                    let (mut a, mut b) = (ch.value[0], ch.value_b[0]);
                    Slider::new("Range min", &mut a, -1.0..=1.0).show(ui);
                    Slider::new("Range max", &mut b, -1.0..=2.0).show(ui);
                    ch.value = [a; 3];
                    ch.value_b = [b; 3];
                }
                proc_params(ui, ("proc", id, c), &mut ch.procedural);
            }
        }
        ui.add_space(6.0);
    }
    if let Some(fd) = replace {
        let proj = f.projection.clone();
        *f = fd;
        if f.projection.mode == ProjMode::Uv && proj.mode != ProjMode::Uv {
            f.projection = proj;
        }
    }
}

fn effect_props(ui: &mut Ui, lid: u64, e: &mut MaskEffect, resources: &HashMap<u64, Resource>, th: &Thumbs, lists: &Lists, cmds: &mut Vec<Cmd>) {
    let eid = e.id;
    let blends = !matches!(e.kind, MaskEffectKind::Levels { .. } | MaskEffectKind::Invert | MaskEffectKind::Threshold { .. } | MaskEffectKind::Blur { .. });
    if blends {
        widgets::combo_row(ui, "Blending", ("eb", eid), &mut e.blend, MaskBlend::ALL, MaskBlend::name);
    }
    Slider::new("Opacity", &mut e.opacity, 0.0..=1.0).percent().show(ui);
    match &mut e.kind {
        MaskEffectKind::Paint => {
            hint(ui, "Paint on the mesh to edit this effect.");
            ui.horizontal(|ui| {
                if ui.button("Fill white").clicked() {
                    cmds.push(Cmd::ClearMaskPaint(eid, 1.0));
                }
                if ui.button("Clear").clicked() {
                    cmds.push(Cmd::ClearMaskPaint(eid, 0.0));
                }
            });
        }
        MaskEffectKind::Fill { value, source, texture, procedural, projection, range } => {
            let (thumb, subtitle) = match source {
                SourceKind::Uniform => (None, format!("uniform {:.2}", value)),
                SourceKind::Texture => (
                    texture.and_then(|i| th.res.get(&i).copied()),
                    texture.and_then(|i| resources.get(&i)).map(|r| r.name.clone()).unwrap_or("missing".into()),
                ),
                SourceKind::Procedural => (th.item(&Item::Procedural(procedural.kind)), procedural.kind.name().to_string()),
            };
            let accept = |it: &Item| matches!(it, Item::Texture(_) | Item::ProjectTexture(..) | Item::Procedural(_));
            match channel_slot(ui, ("mfill", eid), "Grayscale", &subtitle, thumb, &accept, th, &[&lists.procedurals, &lists.textures]) {
                Some(SlotPick::Uniform) => *source = SourceKind::Uniform,
                Some(SlotPick::Item(Item::ProjectTexture(rid, _))) => cmds.push(Cmd::AssignResource(rid, TexSlot::MaskFill(lid, eid))),
                Some(SlotPick::Item(Item::Texture(p))) => cmds.push(Cmd::AssignFile(p, TexSlot::MaskFill(lid, eid))),
                Some(SlotPick::Item(Item::Procedural(k))) => {
                    *source = SourceKind::Procedural;
                    *procedural = ProcParams::of(k);
                    if projection.mode == ProjMode::Uv {
                        *projection = Projection { mode: ProjMode::Object3D, scale: 6.0, ..Default::default() };
                    }
                }
                _ => {}
            }
            match source {
                SourceKind::Uniform => {
                    widgets::gradient_slider(ui, ("mval", eid), value, 0.0, 1.0, Color32::BLACK, Color32::WHITE);
                }
                SourceKind::Procedural => proc_params(ui, ("mp", eid), procedural),
                SourceKind::Texture => {}
            }
            if *source != SourceKind::Uniform {
                Slider::new("Range min", &mut range[0], -1.0..=1.0).show(ui);
                Slider::new("Range max", &mut range[1], -1.0..=2.0).show(ui);
                projection_ui(ui, ("mproj", eid), projection);
            }
        }
        MaskEffectKind::Generator(g) => {
            widgets::combo_row(ui, "Generator", ("gk", eid), &mut g.kind, Generator::ALL, Generator::name);
            Slider::new("Level", &mut g.level, -0.5..=1.5).show(ui);
            Slider::new("Contrast", &mut g.contrast, 0.2..=40.0).log().show(ui);
            Slider::new("Noise breakup", &mut g.noise, 0.0..=1.5).show(ui);
            if g.noise > 0.0 {
                Slider::new("Noise scale", &mut g.noise_scale, 0.5..=100.0).log().show(ui);
                Slider::new("Seed", &mut g.seed, 0.0..=100.0).decimals(0).show(ui);
            }
            if matches!(g.kind, Generator::Edges | Generator::EdgeWear | Generator::Cavity | Generator::Dirt) {
                Slider::new("Painted height", &mut g.height_reactive, 0.0..=4.0).show(ui);
            }
            if g.kind == Generator::PositionGradient {
                ui.horizontal(|ui| {
                    ui.add_sized([96.0, 22.0], egui::Label::new(RichText::new("Axis").size(12.0).color(widgets::LABEL)));
                    ui.selectable_value(&mut g.axis, 0, "X");
                    ui.selectable_value(&mut g.axis, 1, "Y");
                    ui.selectable_value(&mut g.axis, 2, "Z");
                });
            }
            if g.kind == Generator::Light {
                Slider::new("Direction X", &mut g.dir[0], -1.0..=1.0).show(ui);
                Slider::new("Direction Y", &mut g.dir[1], -1.0..=1.0).show(ui);
                Slider::new("Direction Z", &mut g.dir[2], -1.0..=1.0).show(ui);
            }
            widgets::toggle_row(ui, "Invert", &mut g.invert);
        }
        MaskEffectKind::Levels { in_lo, in_hi, gamma, out_lo, out_hi } => {
            Slider::new("Input black", in_lo, 0.0..=1.0).show(ui);
            Slider::new("Input white", in_hi, 0.0..=1.0).show(ui);
            Slider::new("Gamma", gamma, 0.1..=5.0).show(ui);
            Slider::new("Output black", out_lo, 0.0..=1.0).show(ui);
            Slider::new("Output white", out_hi, 0.0..=1.0).show(ui);
        }
        MaskEffectKind::Blur { radius } => {
            Slider::new("Radius (texels)", radius, 0.5..=64.0).show(ui);
        }
        MaskEffectKind::Invert => {}
        MaskEffectKind::Threshold { value } => {
            Slider::new("Threshold", value, 0.0..=1.0).show(ui);
        }
        MaskEffectKind::ColorSelect { colors, tolerance } => {
            Slider::new("Tolerance", tolerance, 0.0..=0.5).show(ui);
            let mut remove = None;
            for (i, c) in colors.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 34.0);
                        widgets::color_bar(ui, ("idcol", eid, i), c);
                    });
                    if icons::button(ui, Icon::Close, false, 20.0, "Remove").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                colors.remove(i);
            }
            ui.horizontal(|ui| {
                if colors.len() < 8 && ui.button("Add color").clicked() {
                    colors.push([1.0, 0.0, 0.0]);
                }
                if ui.button("Pick from mesh").clicked() {
                    cmds.push(Cmd::IdPick(eid));
                }
            });
            hint(ui, "Uses the baked ID map.");
        }
    }
}

impl RetroApp {
    /// Plain right click in the viewport: the main brush settings at the
    /// cursor. Click outside or Escape closes it.
    pub(super) fn quick_properties(&mut self, ctx: &egui::Context) {
        let Some(pos) = self.quick_props else { return };
        let mut cmds = Vec::new();
        let (thumbs, lists) = self.slot_assets(ctx);
        // Keep the popup on screen.
        let screen = ctx.screen_rect();
        let size = egui::vec2(300.0, (screen.height() * 0.7).min(640.0));
        let at = Pos2::new(pos.x.min(screen.max.x - size.x - 8.0), pos.y.min(screen.max.y - size.y - 8.0));
        let area = egui::Area::new(egui::Id::new("quick properties")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(crate::style::PANEL).stroke(egui::Stroke::new(1.0f32, crate::style::ACCENT)).inner_margin(egui::Margin::same(0.0)).show(ui, |ui| {
                ui.set_width(size.x);
                // Long property lists scroll inside the popup instead of
                // running off the bottom of the window.
                egui::ScrollArea::vertical().id_salt("quick scroll").max_height(size.y).auto_shrink([false, true]).show(ui, |ui| {
                    ui.push_id("quick", |ui| self.properties_body(ui, ctx, &mut cmds, &thumbs, &lists));
                });
            });
        });
        for c in cmds {
            self.run_cmd(c);
        }
        let r = area.response.rect;
        let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !r.contains(p)));
        // Clicks inside a popup (color picker, resource picker) opened from here do not close it.
        // Neither does the click of an eyedropper armed from here (it samples the mesh).
        let eyedropper = ctx.data(|d| d.get_temp::<bool>(widgets::eyedropper_armed_at_start_id())).unwrap_or(false);
        if outside && !eyedropper && !ctx.memory(|m| m.any_popup_open()) && !ctx.data(|d| d.get_temp::<bool>(egui::Id::new("color picker hovered")).unwrap_or(false)) {
            self.quick_props = None;
        }
    }
}

impl RetroApp {
    /// Painting on a mask: the mask's own settings (or the selected paint
    /// effect's) sit above the brush, so they stay reachable with a brush
    /// tool. Returns true when something was drawn.
    fn mask_target_header(&mut self, ui: &mut Ui, cmds: &mut Vec<Cmd>) -> bool {
        let Some(doc) = self.doc.as_mut() else { return false };
        let s = &mut doc.sets[doc.active];
        let target = s.target;
        if !target.is_mask() {
            return false;
        }
        let Some(l) = s.selected_layer_mut() else { return false };
        let Some(m) = &mut l.desc.mask else { return false };
        let lw = widgets::label_width(ui);
        match target {
            Target::Mask => {
                widgets::section_title(ui, "Mask", true);
                widgets::toggle_row(ui, "Mask", &mut m.enabled);
                ui.horizontal(|ui| {
                    ui.add_sized([lw, 22.0], egui::Label::new(RichText::new("Background").size(12.0).color(widgets::LABEL)));
                    ui.selectable_value(&mut m.base, 1.0, "White");
                    ui.selectable_value(&mut m.base, 0.0, "Black");
                });
                hint(ui, "Painting adds a paint effect to the mask.");
                true
            }
            Target::MaskEffect(eid) => {
                let Some(e) = m.effects.iter_mut().find(|e| e.id == eid) else { return false };
                widgets::section_title(ui, "Paint effect", true);
                widgets::combo_row(ui, "Blending", ("peb", eid), &mut e.blend, MaskBlend::ALL, MaskBlend::name);
                Slider::new("Opacity", &mut e.opacity, 0.0..=1.0).percent().show(ui);
                ui.horizontal(|ui| {
                    if ui.button("Fill white").clicked() {
                        cmds.push(Cmd::ClearMaskPaint(eid, 1.0));
                    }
                    if ui.button("Clear").clicked() {
                        cmds.push(Cmd::ClearMaskPaint(eid, 0.0));
                    }
                });
                true
            }
            _ => false,
        }
    }
}
