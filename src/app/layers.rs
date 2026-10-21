//! Right dock: texture set list, then the Layers and Texture Set Settings
//! tabs. Layer rows accept dropped layers (reorder) and dropped assets
//! (materials, smart materials, smart masks, textures, procedurals).

use super::icons::{self, Icon};
use super::panels::{effect_menu, mask_menu, short_blend, Cmd};
use crate::document::BlendMode;
use super::shelf::Item;
use super::*;
use crate::document::*;
use eframe::egui::{Color32, Pos2, RichText, Sense, Ui};
use std::collections::HashMap;

const ROW_H: f32 = 50.0;
const THUMB: f32 = 36.0;

/// Thumbnails for the layer rows: (layer id, is mask) -> texture.
type RowThumbs = HashMap<(u64, bool), egui::TextureId>;

fn uv01() -> Rect {
    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))
}

fn draw_thumb(p: &egui::Painter, r: Rect, tex: Option<egui::TextureId>, swatch: Option<[f32; 3]>, icon: Option<Icon>, outline: bool) {
    p.rect_filled(r, 2.0, Color32::from_gray(26));
    match (tex, swatch) {
        (Some(t), _) => {
            p.image(t, r, uv01(), Color32::WHITE);
        }
        (None, Some(c)) => {
            let g = |v: f32| (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
            p.rect_filled(r.shrink(1.0), 2.0, Color32::from_rgb(g(c[0]), g(c[1]), g(c[2])));
        }
        _ => {}
    }
    if let Some(i) = icon {
        icons::paint(p, r.shrink(8.0), i, Color32::from_gray(190));
    }
    let stroke = if outline { egui::Stroke::new(1.5f32, crate::style::ACCENT) } else { egui::Stroke::new(1.0f32, Color32::from_gray(70)) };
    p.rect_stroke(r.expand(if outline { 1.0 } else { 0.0 }), 3.0, stroke);
}

impl RetroApp {
    fn no_project(ui: &mut Ui) {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new("No project open").size(11.5).color(Color32::from_gray(140)));
        });
    }

    pub(super) fn texture_sets_ui(&mut self, ui: &mut Ui) {
        if self.doc.is_none() {
            Self::no_project(ui);
            return;
        }
        self.texture_set_list(ui);
    }

    pub(super) fn layers_ui(&mut self, ui: &mut Ui) {
        if self.doc.is_none() {
            Self::no_project(ui);
            return;
        }
        ui.spacing_mut().slider_width = 110.0;
        let mut cmds = Vec::new();
        self.layer_stack(ui, &mut cmds);
        for c in cmds {
            self.run_cmd(c);
        }
    }

    pub(super) fn set_settings_ui(&mut self, ui: &mut Ui) {
        if self.doc.is_none() {
            Self::no_project(ui);
            return;
        }
        self.set_settings_tab(ui);
    }

    fn texture_set_list(&mut self, ui: &mut Ui) {
        let doc = self.doc.as_mut().unwrap();
        let mut modified = false;
        egui::ScrollArea::vertical().id_salt("sets").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, s) in doc.sets.iter_mut().enumerate() {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::click());
                let sel = doc.active == i;
                let p = ui.painter();
                p.rect_filled(r, 0.0, if sel { crate::style::ROW_SELECTED } else if resp.hovered() { Color32::from_gray(56) } else { crate::style::PANEL });
                p.line_segment([r.left_bottom(), r.right_bottom()], egui::Stroke::new(1.0f32, crate::style::SEPARATOR));
                if sel {
                    p.rect_stroke(r.shrink(0.5), 0.0, egui::Stroke::new(1.0f32, crate::style::ACCENT));
                }
                let eye = Rect::from_center_size(Pos2::new(r.min.x + 14.0, r.center().y), egui::vec2(16.0, 16.0));
                let eresp = ui.interact(eye, ui.id().with(("seteye", i)), Sense::click()).on_hover_text("Show in the viewport");
                icons::paint(
                    ui.painter(),
                    eye,
                    if s.settings.visible { Icon::Eye } else { Icon::EyeOff },
                    if eresp.hovered() { Color32::WHITE } else { Color32::from_gray(180) },
                );
                if eresp.clicked() {
                    s.settings.visible = !s.settings.visible;
                    modified = true;
                }
                let w = r.width();
                let p = ui.painter().with_clip_rect(r);
                p.text(
                    Pos2::new(r.min.x + 28.0, r.center().y),
                    egui::Align2::LEFT_CENTER,
                    &s.name,
                    egui::FontId::new(12.0, egui::FontFamily::Name("bold".into())),
                    if sel { Color32::WHITE } else { Color32::from_gray(205) },
                );
                let res = s.resolution();
                // Columns collapse on narrow docks: shader, then resolution.
                let res_x = if w > 300.0 { r.min.x + w * 0.46 } else { r.max.x - 8.0 };
                if w > 300.0 {
                    p.text(Pos2::new(r.max.x - 8.0, r.center().y), egui::Align2::RIGHT_CENTER, "Main shader", egui::FontId::proportional(11.5), Color32::from_gray(200));
                }
                if w > 190.0 {
                    let align = if w > 300.0 { egui::Align2::LEFT_CENTER } else { egui::Align2::RIGHT_CENTER };
                    let tr = p.text(Pos2::new(res_x, r.center().y), align, format!("{res}"), egui::FontId::proportional(11.5), Color32::from_gray(200));
                    if !s.baked {
                        // Small amber dot: mesh maps not baked yet.
                        p.circle_filled(Pos2::new(tr.min.x - 8.0, r.center().y), 2.5, Color32::from_rgb(255, 137, 34));
                    }
                }
                if resp.clicked() && !eresp.clicked() {
                    doc.active = i;
                }
                resp.context_menu(|ui| {
                    ui.set_min_width(180.0);
                    if ui.button(if s.settings.visible { "Hide texture set" } else { "Show texture set" }).clicked() {
                        s.settings.visible = !s.settings.visible;
                        modified = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.label(RichText::new("Name").size(11.0).color(Color32::from_gray(140)));
                    if ui.text_edit_singleline(&mut s.name).changed() {
                        modified = true;
                    }
                });
            }
        });
        if modified {
            doc.modified = true;
        }
    }

    fn layer_stack(&mut self, ui: &mut Ui, cmds: &mut Vec<Cmd>) {
        self.layers_rect = ui.max_rect();
        // Toolbar: channel shown in the rows, then the add / delete icons.
        ui.add_space(4.0);
        // On narrow docks the channel menu gets its own line.
        let narrow = ui.available_width() < 310.0;
        if narrow {
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                egui::ComboBox::from_id_salt("row channel")
                    .width(110.0)
                    .selected_text(CHANNELS[self.row_channel])
                    .show_ui(ui, |ui| {
                        for c in 0..NCH {
                            ui.selectable_value(&mut self.row_channel, c, CHANNELS[c]);
                        }
                    });
            });
        }
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.x = 2.0;
            if !narrow {
                egui::ComboBox::from_id_salt("row channel")
                    .width(110.0)
                    .selected_text(CHANNELS[self.row_channel])
                    .show_ui(ui, |ui| {
                        for c in 0..NCH {
                            ui.selectable_value(&mut self.row_channel, c, CHANNELS[c]);
                        }
                    });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                if icons::button(ui, Icon::Trash, false, 24.0, "Delete layer (Del)").clicked() {
                    cmds.push(Cmd::Delete);
                }
                if icons::button(ui, Icon::Folder, false, 24.0, "Add folder").clicked() {
                    cmds.push(Cmd::AddLayer(LayerKind::Folder));
                }
                // Smart material picker (inserted above the selection).
                let sm = icons::button(ui, Icon::Sphere, false, 24.0, "Add smart material");
                let smid = ui.id().with("smart material popup");
                if sm.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(smid));
                }
                let smart = self.shelf.items_of(super::shelf::Category::SmartMaterials);
                let thumbs = self.shelf.thumb_map();
                let items: Vec<super::widgets::PickItem> = smart
                    .iter()
                    .map(|it| super::widgets::PickItem { name: it.name(), thumb: thumbs.get(&it.key()).copied().filter(|t| *t != egui::TextureId::default()) })
                    .collect();
                if let Some(i) = super::widgets::picker_popup(ui, smid, &sm, "Smart materials", &items) {
                    cmds.push(Cmd::DropItem(None, smart[i].clone()));
                }
                if icons::button(ui, Icon::Fill, false, 24.0, "Add fill layer").clicked() {
                    cmds.push(Cmd::AddLayer(LayerKind::Fill(Box::default())));
                }
                if icons::button(ui, Icon::Brush, false, 24.0, "Add paint layer").clicked() {
                    cmds.push(Cmd::AddLayer(LayerKind::Paint));
                }
                let m = icons::button(ui, Icon::Mask, false, 24.0, "Add mask");
                let mid = ui.id().with("mask popup");
                if m.clicked() {
                    ui.memory_mut(|mem| mem.toggle_popup(mid));
                }
                super::widgets::popup_below(ui, mid, &m, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
                    ui.set_min_width(180.0);
                    mask_menu(ui, cmds);
                    if !cmds.is_empty() {
                        ui.memory_mut(|mem| mem.close_popup());
                    }
                });
                let f = icons::button(ui, Icon::Effect, false, 24.0, "Add effect (to the selected content or mask)");
                let fid = ui.id().with("effect popup");
                if f.clicked() {
                    ui.memory_mut(|mem| mem.toggle_popup(fid));
                }
                let target = self.doc.as_ref().map(|d| (d.sets[d.active].selected, d.sets[d.active].target));
                super::widgets::popup_below(ui, fid, &f, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
                    ui.set_min_width(200.0);
                    match target {
                        Some((Some(_), t)) => add_effect_items(ui, cmds, t.is_mask()),
                        _ => {
                            ui.label(RichText::new("Select a layer first.").size(11.5).color(Color32::from_gray(150)));
                        }
                    }
                    if !cmds.is_empty() {
                        ui.memory_mut(|mem| mem.close_popup());
                    }
                });
            });
        });
        ui.add_space(3.0);
        let doc = self.doc.as_ref().unwrap();
        let s = &doc.sets[doc.active];
        let mut thumbs: RowThumbs = self.thumbs.iter().map(|(k, v)| (*k, v.2)).collect();
        for (k, v) in &self.fill_thumbs {
            thumbs.insert((*k, false), v.2);
        }
        let ch = self.row_channel;
        egui::ScrollArea::vertical().id_salt("layers").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for l in s.layers.iter().rev() {
                layer_row(ui, s, l, 0, ch, cmds, &thumbs);
            }
            // The empty area below the rows takes drops too (top of the stack).
            let rest = ui.available_rect_before_wrap();
            let rest = Rect::from_min_size(rest.min, egui::vec2(rest.width(), rest.height().max(40.0)));
            let resp = ui.allocate_rect(rest, Sense::hover());
            super::mark(ui.ctx(), "layers_drop", rest.intersect(ui.clip_rect()));
            if s.layers.is_empty() {
                ui.painter().text(
                    rest.center_top() + egui::vec2(0.0, 24.0),
                    egui::Align2::CENTER_CENTER,
                    "Drop a material here, or add a layer",
                    egui::FontId::proportional(12.0),
                    Color32::from_gray(130),
                );
            }
            if super::drop_hover::<Item>(ui, resp.rect).is_some() {
                ui.painter().rect_stroke(rest.shrink(2.0), 3.0, egui::Stroke::new(1.5f32, crate::style::ACCENT));
            }
            if let Some(item) = super::drop_take::<Item>(ui, resp.rect) {
                cmds.push(Cmd::DropItem(None, (*item).clone()));
            }
            if let Some(src) = super::drop_hover::<super::panels::LayerDrag>(ui, resp.rect) {
                if s.layers.first().is_some_and(|b| !src.0.contains(&b.desc.id)) {
                    ui.painter().line_segment([rest.left_top(), rest.right_top()], egui::Stroke::new(2.0f32, crate::style::ACCENT));
                }
            }
            if let Some(src) = super::drop_take::<super::panels::LayerDrag>(ui, resp.rect) {
                // Dropped below every row: bottom of the stack (root level).
                if let Some(bottom) = s.layers.first().map(|l| l.desc.id) {
                    if !src.0.contains(&bottom) {
                        cmds.push(Cmd::DropOn(src.0.clone(), bottom, crate::document::Place::Below));
                    }
                }
            }
        });
    }

    fn set_settings_tab(&mut self, ui: &mut Ui) {
        let mut res_change = None;
        let mut bake = false;
        let doc = self.doc.as_mut().unwrap();
        let s = &mut doc.sets[doc.active];
        egui::ScrollArea::vertical().id_salt("setsettings").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::none().inner_margin(egui::Margin::symmetric(8.0, 6.0)).show(ui, |ui| {
                // Sliders sized to the tab: a fixed width pushed the right
                // column (formats, remove buttons) out of narrow docks.
                ui.set_max_width(ui.available_width());
                ui.spacing_mut().slider_width = (ui.available_width() - 200.0).clamp(50.0, 150.0);
                ui.label(RichText::new(&s.name).strong().size(13.0));
                ui.add_space(4.0);
                egui::Grid::new("set grid").num_columns(2).spacing([10.0, 5.0]).show(ui, |ui| {
                    let cur = s.resolution();
                    ui.label("Size");
                    egui::ComboBox::from_id_salt("set res").selected_text(format!("{cur}")).show_ui(ui, |ui| {
                        for r in [256u32, 512, 1024, 2048, 4096] {
                            if ui.selectable_label(r == cur, format!("{r}")).clicked() && r != cur {
                                res_change = Some(r);
                            }
                        }
                    });
                    ui.end_row();
                    let st = &mut s.settings;
                    ui.label("Shader alpha");
                    egui::ComboBox::from_id_salt("alpha mode")
                        .selected_text(match st.alpha_mode {
                            AlphaMode::Auto => "Auto",
                            AlphaMode::Opaque => "Opaque",
                            AlphaMode::AlphaTest => "Alpha test",
                            AlphaMode::AlphaBlend => "Alpha blend",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut st.alpha_mode, AlphaMode::Auto, "Auto")
                                .on_hover_text("Alpha blend when a layer uses the opacity channel, opaque otherwise");
                            ui.selectable_value(&mut st.alpha_mode, AlphaMode::Opaque, "Opaque");
                            ui.selectable_value(&mut st.alpha_mode, AlphaMode::AlphaTest, "Alpha test");
                            ui.selectable_value(&mut st.alpha_mode, AlphaMode::AlphaBlend, "Alpha blend");
                        });
                    ui.end_row();
                    if st.alpha_mode == AlphaMode::AlphaTest {
                        ui.label("Alpha cutoff");
                        ui.add(egui::Slider::new(&mut st.alpha_cutoff, 0.0..=1.0));
                        ui.end_row();
                    }
                    ui.label("Emissive intensity");
                    ui.add(egui::Slider::new(&mut st.emissive_intensity, 0.0..=20.0).logarithmic(true));
                    ui.end_row();
                    ui.label("Height to normal");
                    ui.add(egui::Slider::new(&mut st.height_scale, 0.0..=0.2));
                    ui.end_row();
                    ui.label("Baked AO strength");
                    ui.add(egui::Slider::new(&mut st.ao_strength, 0.0..=1.0));
                    ui.end_row();
                    ui.label("Base color");
                    color_edit(ui, &mut st.base_color);
                    ui.end_row();
                    ui.label("Base roughness");
                    ui.add(egui::Slider::new(&mut st.base_values[0], 0.0..=1.0));
                    ui.end_row();
                    ui.label("Base metallic");
                    ui.add(egui::Slider::new(&mut st.base_values[1], 0.0..=1.0));
                    ui.end_row();
                });
                ui.weak("Changing the size resamples every layer (history is cleared).");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("CHANNELS").font(crate::style::bold(11.5)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let plus = icons::button(ui, Icon::Plus, false, 20.0, "Add a channel");
                        let pid = ui.id().with("add channel");
                        if plus.clicked() {
                            ui.memory_mut(|m| m.toggle_popup(pid));
                        }
                        super::widgets::popup_below(ui, pid, &plus, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                            ui.set_min_width(170.0);
                            for i in 0..NEXT {
                                if ui.add_enabled(!s.settings.ext_channels[i], egui::Button::new(EXT_NAMES[i])).clicked() {
                                    s.settings.ext_channels[i] = true;
                                }
                            }
                        });
                    });
                });
                // Core channels with their formats, then the optional ones.
                let formats = ["sRGB8", "L8", "L8", "L16F", "RGB16F", "L8", "RGB16F"];
                for c in 0..NCH {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(CHANNELS[c]).font(crate::style::bold(11.5)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(24.0);
                            ui.label(RichText::new(formats[c]).size(11.0).color(Color32::from_gray(150)));
                        });
                    });
                }
                for i in 0..NEXT {
                    if !s.settings.ext_channels[i] {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(EXT_NAMES[i]).font(crate::style::bold(11.5)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icons::button(ui, Icon::Close, false, 18.0, "Remove the channel").clicked() {
                                s.settings.ext_channels[i] = false;
                            }
                            ui.add_space(4.0);
                            ui.label(RichText::new("L8").size(11.0).color(Color32::from_gray(150)));
                        });
                    });
                }
                ui.add_space(8.0);
                ui.label(RichText::new("MESH MAPS").strong().size(11.5));
                ui.add_space(2.0);
                for (i, name) in MESH_MAPS.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), Sense::hover());
                        let tex = self.map_thumbs.get(&i).map(|t| t.2);
                        draw_thumb(ui.painter(), r, if s.baked { tex } else { None }, None, None, false);
                        ui.vertical(|ui| {
                            ui.add_space(4.0);
                            ui.label(*name);
                            ui.weak(if s.baked { "baked" } else { "empty" });
                        });
                    });
                }
                ui.add_space(6.0);
                if ui.add_enabled(self.bake_job.is_none(), egui::Button::new("Bake mesh maps...")).clicked() {
                    bake = true;
                }
            });
        });
        if let Some(r) = res_change {
            self.pending_res = Some(r);
        }
        if bake {
            self.open_bake_dialog();
        }
    }
}

/// Names of the mesh map thumbnails, in `map_textures` order.
pub(super) const MESH_MAPS: [&str; 7] = ["Normal", "World space normal", "ID", "Ambient occlusion", "Curvature", "Position", "Thickness"];

/// Source texture and grayscale flag of mesh map `i`.
pub(super) fn map_texture(gpu: &crate::engine::TextureSetGpu, i: usize) -> (&crate::engine::Tex, bool) {
    match i {
        0 => (&gpu.baked_normal, false),
        1 => (&gpu.baked_wnormal, false),
        2 => (&gpu.baked_id, false),
        3 => (&gpu.baked_ao, true),
        4 => (&gpu.baked_curvature, true),
        5 => (&gpu.baked_position, false),
        _ => (&gpu.baked_thickness, true),
    }
}

fn layer_row(ui: &mut Ui, s: &TextureSet, l: &Layer, depth: usize, ch: usize, cmds: &mut Vec<Cmd>, thumbs: &RowThumbs) {
    let id = l.desc.id;
    let selected = s.selected == Some(id);
    // Part of a multi-selection (Ctrl / Shift + click).
    let in_selection = selected || s.extra.contains(&id);
    let (r, row) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
    if depth == 0 && s.layers.last().is_some_and(|t| t.desc.id == id) {
        super::mark(ui.ctx(), "layer_top", r);
    }
    let p = ui.painter().clone();
    let content_sel = selected && s.target == Target::Content;
    let mask_sel = selected && s.target.is_mask();
    p.rect_filled(r, 0.0, if in_selection { crate::style::ROW_SELECTED } else if row.hovered() { Color32::from_gray(56) } else { crate::style::PANEL });
    p.line_segment([r.left_bottom(), r.right_bottom()], egui::Stroke::new(1.0f32, crate::style::SEPARATOR));
    if l.desc.tag > 0 {
        let c = TAG_COLORS[(l.desc.tag as usize).min(TAG_COLORS.len() - 1)];
        p.rect_filled(Rect::from_min_size(r.min + egui::vec2(0.0, 2.0), egui::vec2(3.0, r.height() - 4.0)), 0.0, c);
    }
    let mut x = r.min.x + 6.0 + depth as f32 * 16.0;
    let cy = r.center().y;

    // Visibility.
    let eye = Rect::from_center_size(Pos2::new(x + 9.0, cy), egui::vec2(18.0, 18.0));
    let eresp = ui.interact(eye, ui.id().with(("eye", id)), Sense::click()).on_hover_text("Visibility");
    icons::paint(
        &p,
        eye,
        if l.desc.visible { Icon::Eye } else { Icon::EyeOff },
        if eresp.hovered() { Color32::WHITE } else if l.desc.visible { Color32::from_gray(200) } else { Color32::from_gray(110) },
    );
    if eresp.clicked() {
        cmds.push(Cmd::ToggleVisible(id));
    }
    x += 24.0;

    // Folder arrow.
    let mut arrow_clicked = false;
    if l.is_folder() {
        let ar = Rect::from_center_size(Pos2::new(x + 6.0, cy), egui::vec2(14.0, 14.0));
        let aresp = ui.interact(ar, ui.id().with(("arrow", id)), Sense::click());
        let c = if aresp.hovered() { Color32::WHITE } else { Color32::from_gray(190) };
        icons::paint(&p, ar, if l.desc.expanded { Icon::Chevron } else { Icon::Arrow }, c);
        if aresp.clicked() {
            cmds.push(Cmd::ToggleExpand(id));
            arrow_clicked = true;
        }
        x += 16.0;
    }

    // Content thumbnail with the small bar under it.
    let tr = Rect::from_min_size(Pos2::new(x, cy - THUMB / 2.0 - 3.0), egui::vec2(THUMB, THUMB));
    let (tex, swatch, icon) = match &l.desc.kind {
        LayerKind::Paint => (thumbs.get(&(id, false)).copied(), None, None),
        LayerKind::Fill(f) => (
            thumbs.get(&(id, false)).copied(),
            Some(if f.channels[CH_COLOR].enabled { f.channels[CH_COLOR].value } else { [0.3; 3] }),
            None,
        ),
        LayerKind::Filter(_) => (None, None, Some(Icon::Effect)),
        LayerKind::Folder => (None, None, Some(Icon::Folder)),
    };
    draw_thumb(&p, tr, tex, swatch, icon, content_sel);
    effects_bar(&p, tr, false);
    x += THUMB + 8.0;

    // Right column: blend mode over opacity for the chosen channel.
    let right = Rect::from_min_max(Pos2::new(r.max.x - 58.0, r.min.y + 4.0), Pos2::new(r.max.x - 6.0, r.max.y - 4.0));
    let brect = Rect::from_min_max(right.min, Pos2::new(right.max.x, right.center().y));
    let orect = Rect::from_min_max(Pos2::new(right.min.x, right.center().y), right.max);
    let bresp = ui.interact(brect, ui.id().with(("rowblend", id)), Sense::click()).on_hover_text(format!("{} blending", CHANNELS[ch]));
    let oresp = ui.interact(orect, ui.id().with(("rowop", id)), Sense::click_and_drag()).on_hover_text("Opacity: drag or click");
    let small = egui::FontId::proportional(11.5);
    let tc = |h: bool| if h { Color32::WHITE } else { Color32::from_gray(205) };
    p.text(Pos2::new(brect.max.x - 14.0, brect.center().y), egui::Align2::RIGHT_CENTER, short_blend(l.desc.blend[ch]), small.clone(), tc(bresp.hovered()));
    icons::paint(&p, Rect::from_center_size(Pos2::new(brect.max.x - 5.0, brect.center().y), egui::vec2(9.0, 9.0)), Icon::Chevron, Color32::from_gray(150));
    p.text(Pos2::new(orect.max.x - 14.0, orect.center().y), egui::Align2::RIGHT_CENTER, format!("{:.0}", l.desc.opacity[ch] * 100.0), small, tc(oresp.hovered() || oresp.dragged()));
    icons::paint(&p, Rect::from_center_size(Pos2::new(orect.max.x - 5.0, orect.center().y), egui::vec2(9.0, 9.0)), Icon::Chevron, Color32::from_gray(150));
    let bid = ui.id().with(("blend popup", id));
    if bresp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(bid));
    }
    super::widgets::popup_below(ui, bid, &bresp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(150.0);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for m in BlendMode::ALL {
                if ui.selectable_label(l.desc.blend[ch] == *m, m.name()).clicked() {
                    cmds.push(Cmd::SetBlend(id, ch, *m));
                }
            }
        });
    });
    if oresp.dragged() {
        let d = ui.input(|i| i.pointer.delta());
        let v = (l.desc.opacity[ch] + (d.x - d.y) * 0.005).clamp(0.0, 1.0);
        if v != l.desc.opacity[ch] {
            cmds.push(Cmd::SetOpacity(id, ch, v));
        }
    }
    let oid = ui.id().with(("op popup", id));
    if oresp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(oid));
    }
    super::widgets::popup_below(ui, oid, &oresp, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_width(170.0);
        let mut v = l.desc.opacity[ch];
        if super::widgets::Slider::new("Opacity", &mut v, 0.0..=1.0).percent().show(ui) {
            cmds.push(Cmd::SetOpacity(id, ch, v));
        }
    });

    // Mask slot: the mask thumbnail, or a dashed placeholder.
    let mr = Rect::from_min_size(Pos2::new(right.min.x - 10.0 - 26.0, cy - 13.0), egui::vec2(26.0, 26.0));
    let mut mask_clicked = false;
    if l.desc.mask.is_some() {
        let mr = Rect::from_min_size(Pos2::new(right.min.x - 10.0 - THUMB, cy - THUMB / 2.0 - 3.0), egui::vec2(THUMB, THUMB));
        let mresp = ui
            .interact(mr, ui.id().with(("maskthumb", id)), Sense::click())
            .on_hover_text("Mask: click to select, Alt + click to view it, Shift + click to enable / disable");
        let off = l.desc.mask.as_ref().is_some_and(|m| !m.enabled);
        draw_thumb(&p, mr, thumbs.get(&(id, true)).copied(), Some([0.0; 3]), if off { Some(Icon::Close) } else { None }, mask_sel);
        effects_bar(&p, mr, l.desc.mask.as_ref().is_some_and(|m| !m.effects.is_empty()));
        if mresp.clicked() {
            let m = ui.input(|i| i.modifiers);
            if m.shift {
                cmds.push(Cmd::ToggleMask(id));
            } else {
                cmds.push(Cmd::Select(id, Target::Mask));
                if m.alt {
                    cmds.push(Cmd::ViewMask);
                }
            }
            mask_clicked = true;
        }
        mask_menu_for(&mresp, id, l, cmds);
    } else if matches!(l.desc.kind, LayerKind::Paint | LayerKind::Fill(_) | LayerKind::Folder | LayerKind::Filter(_)) {
        let mresp = ui.interact(mr, ui.id().with(("maskslot", id)), Sense::click()).on_hover_text("Add a mask");
        let c = if mresp.hovered() { Color32::from_gray(200) } else { Color32::from_gray(95) };
        dashed_rect(&p, mr, c);
        let mid = ui.id().with(("mask slot popup", id));
        if mresp.clicked() {
            cmds.push(Cmd::Select(id, Target::Content));
            ui.memory_mut(|m| m.toggle_popup(mid));
            mask_clicked = true;
        }
        let before = cmds.len();
        super::widgets::popup_below(ui, mid, &mresp, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(190.0);
            mask_menu(ui, cmds);
        });
        if cmds.len() > before {
            ui.memory_mut(|m| m.close_popup());
        }
        // Right click on the empty mask slot: the same menu, plus paste.
        let mut picked = Vec::new();
        mresp.context_menu(|ui| {
            ui.set_min_width(190.0);
            let mut sub = Vec::new();
            mask_menu(ui, &mut sub);
            if ui.button("Paste into mask").clicked() {
                sub.push(Cmd::PasteMask(id));
                ui.close_menu();
            }
            if !sub.is_empty() {
                picked.push(Cmd::Select(id, Target::Content));
                picked.extend(sub);
            }
        });
        cmds.extend(picked);
    }
    let name_max = mr.min.x - 8.0;

    // Name, or the inline rename field after a double click.
    let rid = egui::Id::new("layer rename");
    let renaming = ui.data(|d| d.get_temp::<(u64, String)>(rid)).filter(|(i, _)| *i == id);
    let name_rect = Rect::from_min_max(Pos2::new(x, cy - 11.0), Pos2::new(name_max, cy + 11.0));
    if let Some((_, mut text)) = renaming {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(name_rect));
        let te = child.add(egui::TextEdit::singleline(&mut text).desired_width(name_rect.width()).id(rid.with(id)));
        // Focus is requested once: a click elsewhere then ends the rename.
        let fid = egui::Id::new("layer rename focused");
        let asked = ui.data(|d| d.get_temp::<u64>(fid)) == Some(id);
        if !asked {
            te.request_focus();
            ui.data_mut(|d| d.insert_temp(fid, id));
        }
        let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
        if enter || esc || (asked && !te.has_focus()) {
            if !esc && !text.trim().is_empty() && text != l.desc.name {
                cmds.push(Cmd::Rename(id, text.trim().to_string()));
            }
            ui.data_mut(|d| {
                d.remove::<(u64, String)>(rid);
                d.remove::<u64>(fid);
            });
        } else {
            ui.data_mut(|d| d.insert_temp(rid, (id, text)));
        }
    } else {
        let galley = p.layout_no_wrap(
            l.desc.name.clone(),
            egui::FontId::proportional(12.0),
            if l.desc.visible { Color32::from_gray(225) } else { Color32::from_gray(120) },
        );
        p.with_clip_rect(name_rect).galley(Pos2::new(name_rect.min.x, cy - galley.size().y / 2.0), galley, Color32::WHITE);
        if row.double_clicked() && ui.input(|i| i.pointer.interact_pos()).is_some_and(|q| name_rect.contains(q)) {
            ui.data_mut(|d| d.insert_temp(rid, (id, l.desc.name.clone())));
        }
    }

    if in_selection {
        p.rect_stroke(r.shrink(0.5), 0.0, egui::Stroke::new(1.0f32, crate::style::ACCENT));
    }
    let clicked_sub = eresp.clicked() || mask_clicked || arrow_clicked || bresp.clicked() || oresp.clicked();
    if row.clicked() && !clicked_sub {
        // Ctrl + click adds / removes, Shift + click selects a range (Painter).
        let m = ui.input(|i| i.modifiers);
        cmds.push(if m.command {
            Cmd::SelectToggle(id)
        } else if m.shift {
            Cmd::SelectRange(id)
        } else {
            Cmd::Select(id, Target::Content)
        });
    }

    // Drag the row (with the rest of the selection when it is part of it)
    // onto another one; drop assets on a row to insert / apply them there.
    if row.drag_started() {
        let ids = if in_selection { s.selection() } else { vec![id] };
        row.dnd_set_drag_payload(super::panels::LayerDrag(ids));
    }
    // Upper half: above, lower half: below; the middle of a folder row: into it.
    let place = {
        let y = ui.input(|i| i.pointer.hover_pos()).map_or(r.center().y, |q| q.y);
        let f = (y - r.min.y) / r.height().max(1.0);
        if l.is_folder() && (0.3..0.7).contains(&f) {
            crate::document::Place::Into
        } else if f < 0.5 {
            crate::document::Place::Above
        } else if l.is_folder() && l.desc.expanded {
            // Below an open folder's row is its top child slot.
            crate::document::Place::Into
        } else {
            crate::document::Place::Below
        }
    };
    if let Some(src) = super::drop_hover::<super::panels::LayerDrag>(ui, r) {
        if !src.0.contains(&id) {
            let st = egui::Stroke::new(2.0f32, crate::style::ACCENT);
            match place {
                crate::document::Place::Above => {
                    p.line_segment([r.left_top(), r.right_top()], st);
                }
                crate::document::Place::Below => {
                    p.line_segment([r.left_bottom(), r.right_bottom()], st);
                }
                crate::document::Place::Into => {
                    p.rect_stroke(r.shrink(1.0), 2.0, st);
                }
            }
        }
    }
    if let Some(src) = super::drop_take::<super::panels::LayerDrag>(ui, r) {
        if !src.0.contains(&id) {
            cmds.push(Cmd::DropOn(src.0.clone(), id, place));
        }
    }
    if let Some(item) = super::drop_hover::<Item>(ui, r) {
        if matches!(*item, Item::SmartMask(..)) {
            p.rect_stroke(r.shrink(1.0), 2.0, egui::Stroke::new(2.0f32, crate::style::ACCENT));
        } else {
            p.line_segment([r.left_top(), r.right_top()], egui::Stroke::new(2.0f32, crate::style::ACCENT));
        }
    }
    if let Some(item) = super::drop_take::<Item>(ui, r) {
        cmds.push(Cmd::DropItem(Some(id), (*item).clone()));
    }

    let mut ctx_cmds = Vec::new();
    let has_mask = l.desc.mask.is_some();
    row.context_menu(|ui| {
        ui.set_min_width(260.0);
        // A right click inside a multi-selection keeps it (group, delete...).
        ctx_cmds.push(if in_selection { Cmd::MakePrimary(id) } else { Cmd::Select(id, Target::Content) });
        fn item(ui: &mut Ui, out: &mut Vec<Cmd>, label: &str, short: &str, enabled: bool, c: Vec<Cmd>) {
            if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(short)).clicked() {
                out.extend(c);
                ui.close_menu();
            }
        }
        let o = &mut ctx_cmds;
        item(ui, o, "Add white mask", "", !has_mask, vec![Cmd::AddMask(true)]);
        item(ui, o, "Add black mask", "", !has_mask, vec![Cmd::AddMask(false)]);
        item(ui, o, "Add bitmap mask", "", !has_mask, vec![Cmd::AddMask(false)]);
        item(ui, o, "Add mask with color selection", "", !has_mask, vec![Cmd::AddMask(false), Cmd::AddEffect(MaskEffectKind::ColorSelect { colors: Vec::new(), tolerance: 0.05 })]);
        item(ui, o, "Add mask with height combination", "", !has_mask, vec![Cmd::AddMask(true)]);
        ui.separator();
        item(ui, o, "Toggle mask", "Shift + click", has_mask, vec![Cmd::ToggleMask(id)]);
        item(ui, o, "Invert mask background", "", has_mask, vec![Cmd::InvertMaskBase(id)]);
        item(ui, o, "Invert mask", "", has_mask, vec![Cmd::InvertMask(id)]);
        item(ui, o, "Clear mask", "", has_mask, vec![Cmd::ClearMask(id)]);
        item(ui, o, "Remove mask", "", has_mask, vec![Cmd::RemoveMask]);
        ui.separator();
        item(ui, o, "Copy mask", "", has_mask, vec![Cmd::CopyMask(id)]);
        item(ui, o, "Paste into mask", "", true, vec![Cmd::PasteMask(id)]);
        item(ui, o, "Create smart mask", "", has_mask, vec![Cmd::SaveSmartMask]);
        ui.separator();
        item(ui, o, "Copy layer", "Ctrl+C", true, vec![Cmd::CopyLayer(id)]);
        item(ui, o, "Paste layer", "Ctrl+V", true, vec![Cmd::PasteLayer]);
        item(ui, o, "Duplicate layer", "Ctrl+D", true, vec![Cmd::Duplicate]);
        item(ui, o, "Delete layer", "Del", true, vec![Cmd::Delete]);
        item(ui, o, "Group layers", "Ctrl+G", true, vec![Cmd::Group]);
        item(ui, o, "Move into folder below", "", true, vec![Cmd::IntoFolder]);
        item(ui, o, "Move out of folder", "", true, vec![Cmd::Ungroup]);
        if ui.button("Rename").clicked() {
            ui.data_mut(|d| d.insert_temp(rid, (id, l.desc.name.clone())));
            ui.close_menu();
        }
        ui.separator();
        // Color tags.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (t, c) in TAG_COLORS.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
                if t == 0 {
                    ui.painter().rect_stroke(r.shrink(1.0), 1.0, egui::Stroke::new(1.0f32, Color32::from_gray(150)));
                    ui.painter().line_segment([r.left_bottom(), r.right_top()], egui::Stroke::new(1.0f32, Color32::from_gray(150)));
                } else {
                    ui.painter().rect_filled(r.shrink(1.0), 1.0, *c);
                }
                if l.desc.tag as usize == t {
                    ui.painter().rect_stroke(r, 1.0, egui::Stroke::new(1.5f32, Color32::WHITE));
                }
                if resp.clicked() {
                    o.push(Cmd::SetTag(id, t as u8));
                    ui.close_menu();
                }
            }
        });
        ui.separator();
        super::widgets::sub_menu(ui, "Blending options", |ui| {
            ui.set_min_width(240.0);
            item(ui, o, "Apply blend mode to all channels", "", true, vec![Cmd::BlendToAll(id, ch, true, false)]);
            item(ui, o, "Apply opacity to all channels", "", true, vec![Cmd::BlendToAll(id, ch, false, true)]);
            item(ui, o, "Apply both to all channels", "", true, vec![Cmd::BlendToAll(id, ch, true, true)]);
            ui.separator();
            item(ui, o, "Copy channel blending parameters", "", true, vec![Cmd::CopyBlending(id)]);
            item(ui, o, "Paste channel blending parameters", "", true, vec![Cmd::PasteBlending(id)]);
        });
        ui.separator();
        item(ui, o, "Create smart material", "", true, vec![Cmd::SaveSmartMaterial]);
        ui.separator();
        let mut fx = Vec::new();
        add_effect_items(ui, &mut fx, false);
        if !fx.is_empty() {
            o.extend(fx);
        }
    });
    if ctx_cmds.len() > 1 {
        cmds.extend(ctx_cmds);
    }

    // The content effect stack, listed under the row when the content (or
    // one of its effects) is selected.
    if selected && !s.target.is_mask() && !l.desc.effects.is_empty() {
        for e in l.desc.effects.iter().rev() {
            let (er, eresp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), Sense::click());
            let sel = s.target == Target::ContentEffect(e.id);
            ui.painter().rect_filled(er, 0.0, if sel { crate::style::ROW_SELECTED } else if eresp.hovered() { Color32::from_gray(56) } else { Color32::from_gray(46) });
            if sel {
                ui.painter().rect_stroke(er.shrink(0.5), 0.0, egui::Stroke::new(1.0f32, crate::style::ACCENT));
            }
            let ex = er.min.x + 30.0 + depth as f32 * 16.0;
            // Orange bar on the left: these effects belong to the content.
            ui.painter().rect_filled(Rect::from_min_size(Pos2::new(ex - 8.0, er.min.y + 3.0), egui::vec2(2.0, er.height() - 6.0)), 0.0, Color32::from_rgb(255, 137, 34));
            let eye = Rect::from_center_size(Pos2::new(ex + 8.0, er.center().y), egui::vec2(16.0, 16.0));
            let vresp = ui.interact(eye, ui.id().with(("cfxeye", e.id)), Sense::click());
            icons::paint(ui.painter(), eye, if e.enabled { Icon::Eye } else { Icon::EyeOff }, Color32::from_gray(if e.enabled { 190 } else { 110 }));
            if vresp.clicked() {
                cmds.push(Cmd::ToggleContentEffect(id, e.id));
            }
            icons::paint(ui.painter(), Rect::from_center_size(Pos2::new(ex + 30.0, er.center().y), egui::vec2(14.0, 14.0)), Icon::Effect, Color32::from_gray(170));
            ui.painter().text(Pos2::new(ex + 44.0, er.center().y), egui::Align2::LEFT_CENTER, format!("Filter - {}", e.filter.kind.name()), egui::FontId::proportional(12.0), Color32::from_gray(215));
            ui.painter().text(Pos2::new(er.max.x - 10.0, er.center().y), egui::Align2::RIGHT_CENTER, format!("{:.0}", e.opacity * 100.0), egui::FontId::proportional(11.0), Color32::from_gray(150));
            let xr = Rect::from_center_size(Pos2::new(er.max.x - 50.0, er.center().y), egui::vec2(14.0, 14.0));
            let xresp = ui.interact(xr, ui.id().with(("cfxdel", e.id)), Sense::click()).on_hover_text("Remove effect (Del)");
            icons::paint(ui.painter(), xr.shrink(2.0), Icon::Close, if xresp.hovered() { Color32::WHITE } else { Color32::from_gray(120) });
            if xresp.clicked() {
                cmds.push(Cmd::Select(id, Target::ContentEffect(e.id)));
                cmds.push(Cmd::RemoveContentEffect(e.id));
            }
            if eresp.clicked() && !vresp.clicked() && !xresp.clicked() {
                cmds.push(Cmd::Select(id, Target::ContentEffect(e.id)));
            }
            let eid = e.id;
            let mut picked = Vec::new();
            eresp.context_menu(|ui| {
                ui.set_min_width(220.0);
                for (label, short, c) in [
                    ("Remove effect", "Del", Cmd::RemoveContentEffect(eid)),
                    ("Move effect up", "Alt+Up", Cmd::MoveContentEffect(eid, 1)),
                    ("Move effect down", "Alt+Down", Cmd::MoveContentEffect(eid, -1)),
                ] {
                    if ui.add(egui::Button::new(label).shortcut_text(short)).clicked() {
                        picked.push(Cmd::Select(id, Target::ContentEffect(eid)));
                        picked.push(c);
                        ui.close_menu();
                    }
                }
                ui.separator();
                let mut sub = Vec::new();
                add_effect_items(ui, &mut sub, false);
                if !sub.is_empty() {
                    picked.push(Cmd::Select(id, Target::Content));
                    picked.extend(sub);
                }
            });
            cmds.extend(picked);
        }
    }

    // The mask's effect stack, listed under the row when the mask (or one of
    // its effects) is selected, like the real application.
    if selected && s.target.is_mask() {
        if let Some(m) = &l.desc.mask {
            let (hr, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), Sense::hover());
            let hx = hr.min.x + 30.0 + depth as f32 * 16.0;
            ui.painter().rect_filled(hr, 0.0, Color32::from_gray(40));
            icons::paint(ui.painter(), Rect::from_center_size(Pos2::new(hx + 8.0, hr.center().y), egui::vec2(12.0, 12.0)), Icon::Mask, crate::style::ACCENT);
            ui.painter().text(
                Pos2::new(hx + 20.0, hr.center().y),
                egui::Align2::LEFT_CENTER,
                format!("MASK EFFECTS ({})", m.effects.len()),
                crate::style::bold(10.5),
                Color32::from_gray(180),
            );
            for e in m.effects.iter().rev() {
                let (er, eresp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), Sense::click());
                let sel = s.target == Target::MaskEffect(e.id);
                ui.painter().rect_filled(er, 0.0, if sel { crate::style::ROW_SELECTED } else if eresp.hovered() { Color32::from_gray(56) } else { Color32::from_gray(46) });
                if sel {
                    ui.painter().rect_stroke(er.shrink(0.5), 0.0, egui::Stroke::new(1.0f32, crate::style::ACCENT));
                }
                let ex = er.min.x + 30.0 + depth as f32 * 16.0;
                let eye = Rect::from_center_size(Pos2::new(ex + 8.0, er.center().y), egui::vec2(16.0, 16.0));
                let vresp = ui.interact(eye, ui.id().with(("effeye", e.id)), Sense::click());
                icons::paint(ui.painter(), eye, if e.enabled { Icon::Eye } else { Icon::EyeOff }, Color32::from_gray(if e.enabled { 190 } else { 110 }));
                if vresp.clicked() {
                    cmds.push(Cmd::ToggleEffect(id, e.id));
                }
                icons::paint(ui.painter(), Rect::from_center_size(Pos2::new(ex + 30.0, er.center().y), egui::vec2(14.0, 14.0)), Icon::Mask, Color32::from_gray(150));
                ui.painter().text(Pos2::new(ex + 44.0, er.center().y), egui::Align2::LEFT_CENTER, e.kind.label(), egui::FontId::proportional(12.0), Color32::from_gray(215));
                ui.painter().text(
                    Pos2::new(er.max.x - 10.0, er.center().y),
                    egui::Align2::RIGHT_CENTER,
                    format!("{}   {:.0}", e.blend.name(), e.opacity * 100.0),
                    egui::FontId::proportional(11.0),
                    Color32::from_gray(150),
                );
                // Mask-colored bar on the left: these effects belong to the mask.
                ui.painter().rect_filled(Rect::from_min_size(Pos2::new(ex - 8.0, er.min.y + 3.0), egui::vec2(2.0, er.height() - 6.0)), 0.0, crate::style::ACCENT.linear_multiply(0.7));
                let xr = Rect::from_center_size(Pos2::new(er.max.x - 104.0, er.center().y), egui::vec2(14.0, 14.0));
                let xresp = ui.interact(xr, ui.id().with(("effdel", e.id)), Sense::click()).on_hover_text("Remove effect (Del)");
                icons::paint(ui.painter(), xr.shrink(2.0), Icon::Close, if xresp.hovered() { Color32::WHITE } else { Color32::from_gray(120) });
                if xresp.clicked() {
                    cmds.push(Cmd::RemoveEffect(e.id));
                }
                if eresp.clicked() && !vresp.clicked() && !xresp.clicked() {
                    cmds.push(Cmd::Select(id, Target::MaskEffect(e.id)));
                }
                let eid = e.id;
                let mut picked = Vec::new();
                eresp.context_menu(|ui| {
                    ui.set_min_width(220.0);
                    for (label, short, c) in [
                        ("Remove effect", "Del", Cmd::RemoveEffect(eid)),
                        ("Move effect up", "Alt+Up", Cmd::MoveEffect(eid, 1)),
                        ("Move effect down", "Alt+Down", Cmd::MoveEffect(eid, -1)),
                    ] {
                        if ui.add(egui::Button::new(label).shortcut_text(short)).clicked() {
                            picked.push(Cmd::Select(id, Target::MaskEffect(eid)));
                            picked.push(c);
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    let mut sub = Vec::new();
                    effect_menu(ui, &mut sub);
                    if !sub.is_empty() {
                        picked.push(Cmd::Select(id, Target::Mask));
                        picked.extend(sub);
                    }
                });
                cmds.extend(picked);
            }
        }
    }
    if l.is_folder() && l.desc.expanded {
        for c in l.children.iter().rev() {
            layer_row(ui, s, c, depth + 1, ch, cmds, thumbs);
        }
    }
}

/// Right click on a mask thumbnail (the row menu cannot open there: the
/// thumbnail takes the click).
fn mask_menu_for(resp: &egui::Response, id: u64, l: &Layer, cmds: &mut Vec<Cmd>) {
    let enabled = l.desc.mask.as_ref().is_some_and(|m| m.enabled);
    let mut picked = Vec::new();
    resp.context_menu(|ui| {
        ui.set_min_width(200.0);
        fn item(ui: &mut Ui, picked: &mut Vec<Cmd>, id: u64, label: &str, shortcut: &str, c: Cmd) {
            if ui.add(egui::Button::new(label).shortcut_text(shortcut)).clicked() {
                picked.push(Cmd::Select(id, Target::Mask));
                picked.push(c);
                ui.close_menu();
            }
        }
        item(ui, &mut picked, id, if enabled { "Disable mask" } else { "Enable mask" }, "Shift + click", Cmd::ToggleMask(id));
        item(ui, &mut picked, id, "Invert mask background", "", Cmd::InvertMaskBase(id));
        ui.separator();
        super::widgets::sub_menu(ui, "Add effect", |ui| {
            let mut sub = Vec::new();
            effect_menu(ui, &mut sub);
            if !sub.is_empty() {
                picked.push(Cmd::Select(id, Target::Mask));
                picked.extend(sub);
            }
        });
        ui.separator();
        item(ui, &mut picked, id, "Copy mask", "", Cmd::CopyMask(id));
        item(ui, &mut picked, id, "Paste into mask", "", Cmd::PasteMask(id));
        item(ui, &mut picked, id, "Create smart mask", "", Cmd::SaveSmartMask);
        ui.separator();
        item(ui, &mut picked, id, "Show mask in viewport", "Alt + click", Cmd::ViewMask);
        item(ui, &mut picked, id, "Remove mask", "", Cmd::RemoveMask);
    });
    cmds.extend(picked);
}

/// Thin bar under a thumbnail: orange when that stack has effects.
fn effects_bar(p: &egui::Painter, thumb: Rect, has_effects: bool) {
    let c = if has_effects { Color32::from_rgb(255, 137, 34) } else { Color32::from_gray(74) };
    p.rect_filled(Rect::from_min_size(Pos2::new(thumb.min.x + 2.0, thumb.max.y + 3.0), egui::vec2(thumb.width() - 4.0, 3.0)), 1.0, c);
}

fn dashed_rect(p: &egui::Painter, r: Rect, c: Color32) {
    let st = egui::Stroke::new(1.0f32, c);
    let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
    for w in corners.windows(2) {
        p.extend(egui::Shape::dashed_line(&[w[0], w[1]], st, 3.0, 3.0));
    }
}

/// Layer color tags (0 = none), as in the layer menu.
const TAG_COLORS: [Color32; 8] = [
    Color32::TRANSPARENT,
    Color32::from_rgb(179, 64, 56),
    Color32::from_rgb(194, 104, 46),
    Color32::from_rgb(196, 160, 52),
    Color32::from_rgb(110, 150, 70),
    Color32::from_rgb(48, 140, 130),
    Color32::from_rgb(52, 110, 190),
    Color32::from_rgb(130, 80, 170),
];

/// The "Add ..." effect items of the real application: on a mask they add
/// mask effects, on content they add content effects (filters, levels).
fn add_effect_items(ui: &mut Ui, cmds: &mut Vec<Cmd>, on_mask: bool) {
    let unavailable = "Not available on layer content";
    if on_mask {
        super::widgets::sub_menu(ui, "Add generator", |ui| {
            for g in Generator::ALL {
                if ui.button(g.name()).clicked() {
                    cmds.push(Cmd::AddEffect(MaskEffectKind::Generator(GeneratorDesc::of(*g))));
                    ui.close_menu();
                }
            }
        });
        if ui.button("Add paint").clicked() {
            cmds.push(Cmd::AddEffect(MaskEffectKind::Paint));
            ui.close_menu();
        }
        super::widgets::sub_menu(ui, "Add fill", |ui| {
            if ui.button("Uniform / texture").clicked() {
                cmds.push(Cmd::AddEffect(MaskEffectKind::Fill {
                    value: 1.0,
                    source: SourceKind::Uniform,
                    texture: None,
                    procedural: ProcParams::default(),
                    projection: Projection::default(),
                    range: [0.0, 1.0],
                }));
                ui.close_menu();
            }
            ui.separator();
            for p in ProcKind::ALL {
                if ui.button(p.name()).clicked() {
                    cmds.push(Cmd::AddEffect(MaskEffectKind::fill_procedural(*p)));
                    ui.close_menu();
                }
            }
        });
        if ui.button("Add levels").clicked() {
            cmds.push(Cmd::AddEffect(MaskEffectKind::levels()));
            ui.close_menu();
        }
        ui.add_enabled(false, egui::Button::new("Add compare mask"));
        super::widgets::sub_menu(ui, "Add filter", |ui| {
            for (n, k) in [
                ("Blur", MaskEffectKind::Blur { radius: 4.0 }),
                ("Invert", MaskEffectKind::Invert),
                ("Threshold", MaskEffectKind::Threshold { value: 0.5 }),
            ] {
                if ui.button(n).clicked() {
                    cmds.push(Cmd::AddEffect(k));
                    ui.close_menu();
                }
            }
        });
        if ui.button("Add color selection").clicked() {
            cmds.push(Cmd::AddEffect(MaskEffectKind::ColorSelect { colors: Vec::new(), tolerance: 0.05 }));
            ui.close_menu();
        }
    } else {
        ui.add_enabled(false, egui::Button::new("Add generator")).on_disabled_hover_text(unavailable);
        ui.add_enabled(false, egui::Button::new("Add paint")).on_disabled_hover_text(unavailable);
        ui.add_enabled(false, egui::Button::new("Add fill")).on_disabled_hover_text(unavailable);
        if ui.button("Add levels").clicked() {
            cmds.push(Cmd::AddContentEffect(FilterKind::Levels));
            ui.close_menu();
        }
        ui.add_enabled(false, egui::Button::new("Add compare mask")).on_disabled_hover_text("Masks only");
        super::widgets::sub_menu(ui, "Add filter", |ui| {
            for k in FilterKind::ALL {
                if ui.button(k.name()).clicked() {
                    cmds.push(Cmd::AddContentEffect(*k));
                    ui.close_menu();
                }
            }
        });
        ui.add_enabled(false, egui::Button::new("Add color selection")).on_disabled_hover_text("Masks only");
    }
    ui.add_enabled(false, egui::Button::new("Add anchor point")).on_disabled_hover_text("Not available yet");
}
