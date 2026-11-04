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

/// Whether a fill layer's row shows a material ball: its base color is a
/// map (otherwise a flat swatch of the color).
pub(super) fn fill_has_ball(f: &crate::document::FillDesc) -> bool {
    let ch = &f.channels[CH_COLOR];
    ch.enabled && ch.source != crate::document::SourceKind::Uniform
}

fn draw_thumb(p: &egui::Painter, r: Rect, tex: Option<egui::TextureId>, swatch: Option<[f32; 3]>, icon: Option<Icon>, outline: bool) {
    p.rect_filled(r, 2.0, crate::style::warm(26));
    match (tex, swatch) {
        (Some(t), c) => {
            p.image(t, r, uv01(), Color32::WHITE);
            if let Some(c) = c {
                // The exact uniform color in a corner (the eyedropper reads it).
                let col: Color32 = egui::Rgba::from_rgb(c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0)).into();
                let sw = Rect::from_min_size(Pos2::new(r.min.x + 2.0, r.max.y - 12.0), egui::vec2(10.0, 10.0));
                p.rect_filled(sw.expand(1.0), 2.0, Color32::from_black_alpha(200));
                p.rect_filled(sw, 1.5, col);
            }
        }
        (None, Some(c)) => {
            // Exact sRGB: the eyedropper reads back the layer's color.
            let col: Color32 = egui::Rgba::from_rgb(c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0)).into();
            p.rect_filled(r.shrink(1.0), 2.0, col);
        }
        _ => {}
    }
    if let Some(i) = icon {
        icons::paint(p, r.shrink(8.0), i, Color32::from_gray(190));
    }
    let stroke = if outline { egui::Stroke::new(1.5f32, crate::style::ACCENT) } else { egui::Stroke::new(1.0f32, crate::style::warm(70)) };
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
            let names: Vec<String> = doc.sets.iter().map(|s| s.name.clone()).collect();
            for (i, s) in doc.sets.iter_mut().enumerate() {
                let others: Vec<&String> = names.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, n)| n).collect();
                let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::click());
                let sel = doc.active == i;
                let p = ui.painter();
                let card = r.shrink2(egui::vec2(6.0, 1.0));
                if sel {
                    p.rect(card, crate::style::R_CONTROL, crate::style::ROW_SELECTED, egui::Stroke::new(1.0f32, crate::style::ACCENT.gamma_multiply(0.7)));
                } else if resp.hovered() {
                    p.rect_filled(card, crate::style::R_CONTROL, crate::style::FIELD);
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
                // The resolution column hides on narrow docks.
                let res_x = r.max.x - 8.0;
                if w > 190.0 {
                    let align = egui::Align2::RIGHT_CENTER;
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
                    // Empty or duplicate names would break reimport matching
                    // and export file names: they are kept only when valid.
                    let mut name = s.name.clone();
                    if ui.text_edit_singleline(&mut name).changed() {
                        let n = name.trim();
                        if !n.is_empty() && !others.iter().any(|o| o.as_str() == n) {
                            s.name = n.to_string();
                            modified = true;
                        }
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
        // The texture set being edited (when the list panel is not shown):
        // a menu, its visibility, its settings.
        if !self.tab_open(super::dock::Tab::TextureSets) {
            ui.add_space(6.0);
            let mut open_settings = false;
            let settings_on = self.tab_open(super::dock::Tab::SetSettings);
            if let Some(doc) = &mut self.doc {
                let names: Vec<String> = doc.sets.iter().map(|s| format!("{}  ({} px)", s.name, s.resolution())).collect();
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new("Texture set").size(11.5).color(Color32::from_gray(150)));
                    let w = (ui.available_width() - 70.0).max(80.0);
                    egui::ComboBox::from_id_salt("set menu").width(w).selected_text(names[doc.active].clone()).show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            ui.selectable_value(&mut doc.active, i, n);
                        }
                    });
                    let s = &mut doc.sets[doc.active];
                    let vis = s.settings.visible;
                    if icons::button(ui, if vis { Icon::Eye } else { Icon::EyeOff }, false, 24.0, "Show / hide this texture set").clicked() {
                        s.settings.visible = !vis;
                        doc.modified = true;
                    }
                    let g = icons::button(ui, Icon::Gear, settings_on, 24.0, "Texture set settings (resolution, channels, bake)");
                    super::mark(ui.ctx(), "setsettings:gear", g.rect);
                    open_settings = g.clicked();
                });
            }
            if open_settings {
                self.toggle_tab(super::dock::Tab::SetSettings);
            }
            ui.add_space(2.0);
        }
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
                // The selected layer's mask state (Toggle / Invert items).
                let masked = self.doc.as_ref().and_then(|d| {
                    let l = d.sets[d.active].selected_layer()?;
                    Some((l.desc.id, l.desc.mask.as_ref().map(|m| m.enabled)))
                });
                super::widgets::popup_below(ui, mid, &m, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
                    ui.set_min_width(180.0);
                    mask_menu(ui, cmds);
                    ui.separator();
                    let (lid, state) = masked.map_or((0, None), |(i, s)| (i, s));
                    let mut on = state.unwrap_or(false);
                    if ui.add_enabled(state.is_some(), egui::Checkbox::new(&mut on, "Toggle mask")).clicked() {
                        cmds.push(Cmd::ToggleMask(lid));
                    }
                    if ui.add_enabled(state.is_some(), egui::Button::new("Invert mask background")).clicked() {
                        cmds.push(Cmd::InvertMaskBase(lid));
                    }
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
        let mut remove_map = None;
        let baking = self.bake_job.is_some();
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
                    ui.add_enabled_ui(!baking, |ui| {
                        egui::ComboBox::from_id_salt("set res").selected_text(format!("{cur}")).show_ui(ui, |ui| {
                            for r in [256u32, 512, 1024, 2048, 4096] {
                                if ui.selectable_label(r == cur, format!("{r}")).clicked() && r != cur {
                                    res_change = Some(r);
                                }
                            }
                        });
                    })
                    .response
                    .on_disabled_hover_text("Wait for the bake to finish");
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
                        super::widgets::slide(ui, &mut st.alpha_cutoff, 0.0..=1.0, false, "");
                        ui.end_row();
                    }
                    ui.label("Emissive intensity");
                    super::widgets::slide(ui, &mut st.emissive_intensity, 0.0..=20.0, true, "");
                    ui.end_row();
                    ui.label("Height to normal");
                    super::widgets::slide(ui, &mut st.height_scale, 0.0..=0.2, false, "");
                    ui.end_row();
                    ui.label("Baked AO strength");
                    super::widgets::slide(ui, &mut st.ao_strength, 0.0..=1.0, false, "");
                    ui.end_row();
                    ui.label("Base color");
                    color_edit(ui, &mut st.base_color);
                    ui.end_row();
                    ui.label("Base roughness");
                    super::widgets::slide(ui, &mut st.base_values[0], 0.0..=1.0, false, "");
                    ui.end_row();
                    ui.label("Base metallic");
                    super::widgets::slide(ui, &mut st.base_values[1], 0.0..=1.0, false, "");
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
                    let removed = s.removed_maps & (1 << i) != 0;
                    let has = s.baked && !removed;
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), Sense::hover());
                        let tex = self.map_thumbs.get(&i).map(|t| t.2);
                        draw_thumb(ui.painter(), r, if has { tex } else { None }, None, None, false);
                        ui.vertical(|ui| {
                            ui.add_space(4.0);
                            ui.label(*name);
                            ui.weak(if has { "baked" } else if removed { "removed (bake again to get it back)" } else { "empty" });
                        });
                        // Position and world normal place the tri-planar projections.
                        if has && REMOVABLE_MAPS.contains(&i) {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if icons::button(ui, Icon::Close, false, 18.0, "Remove this map (the generators that read it get nothing)").clicked() {
                                    remove_map = Some(i);
                                }
                            });
                        }
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
        if let Some(i) = remove_map {
            let doc = self.doc.as_mut().unwrap();
            let s = &mut doc.sets[doc.active];
            s.gpu.clear_map(&self.engine.queue, i);
            s.removed_maps |= 1 << i;
            s.bake_rev += 1;
            s.dirty = true;
            doc.modified = true;
            let text = format!("{} map removed from {} (bake again to get it back).", MESH_MAPS[i], s.name);
            self.message(text);
        }
    }
}

/// The mesh maps the user may remove (not position and world normal).
const REMOVABLE_MAPS: [usize; 5] = [0, 2, 3, 4, 6];

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
    // Rounded rows (design system): the selection is a soft orange card.
    let card = r.shrink2(egui::vec2(6.0, 1.5));
    if in_selection {
        p.rect_filled(card, crate::style::R_CONTROL, crate::style::ROW_SELECTED);
    } else if row.hovered() {
        p.rect_filled(card, crate::style::R_CONTROL, crate::style::FIELD);
    }
    if l.desc.tag > 0 {
        let c = TAG_COLORS[(l.desc.tag as usize).min(TAG_COLORS.len() - 1)];
        p.rect_filled(Rect::from_min_size(r.min + egui::vec2(0.0, 2.0), egui::vec2(3.0, r.height() - 4.0)), 0.0, c);
    }
    let mut x = r.min.x + 6.0 + depth as f32 * 16.0;
    let cy = r.center().y;

    // Visibility.
    let eye = Rect::from_center_size(Pos2::new(x + 9.0, cy), egui::vec2(18.0, 18.0));
    let eresp = ui.interact(eye, ui.id().with(("eye", id)), Sense::click()).on_hover_text("Visibility (Alt + click: this layer alone)");
    icons::paint(
        &p,
        eye,
        if l.desc.visible { Icon::Eye } else { Icon::EyeOff },
        if eresp.hovered() { Color32::WHITE } else if l.desc.visible { Color32::from_gray(200) } else { Color32::from_gray(110) },
    );
    if eresp.clicked() {
        cmds.push(if ui.input(|i| i.modifiers.alt) { Cmd::Solo(id) } else { Cmd::ToggleVisible(id) });
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
        // A fill shows its base color, flat (it can be eyedropped); a map in
        // the base color shows on a material ball.
        LayerKind::Fill(f) => {
            if fill_has_ball(f) {
                (thumbs.get(&(id, false)).copied(), None, None)
            } else {
                let ch = &f.channels[CH_COLOR];
                (None, Some(if ch.enabled { ch.value } else { [0.3; 3] }), None)
            }
        }
        LayerKind::Filter(_) => (None, None, Some(Icon::Effect)),
        LayerKind::Folder => (None, None, Some(Icon::Folder)),
    };
    draw_thumb(&p, tr, tex, swatch, icon, content_sel);
    if l.desc.paths.is_some() {
        // Path layer: painted from its paths.
        let br = Rect::from_min_size(tr.max - egui::vec2(13.0, 13.0), egui::vec2(13.0, 13.0));
        p.rect_filled(br, 2.0, Color32::from_black_alpha(190));
        icons::paint(&p, br.shrink(1.5), Icon::Path, crate::style::ACCENT);
    }
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
        let c = if mresp.hovered() { Color32::from_gray(200) } else { crate::style::warm(95) };
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
        p.rect_stroke(card, crate::style::R_CONTROL, egui::Stroke::new(1.0f32, crate::style::ACCENT.gamma_multiply(0.7)));
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
        // The ghost row that follows the pointer (`layer_drag_ghost`).
        let grab = ui.input(|i| i.pointer.press_origin()).map_or(egui::Vec2::ZERO, |q| q - r.min);
        let ghost = LayerGhost { name: l.desc.name.clone(), thumb: thumbs.get(&(id, false)).copied(), count: ids.len(), size: r.size(), grab };
        ui.data_mut(|d| d.insert_temp(egui::Id::new("layer ghost"), ghost));
        row.dnd_set_drag_payload(super::panels::LayerDrag(ids));
    }
    // Rows being dragged are dimmed in place.
    if egui::DragAndDrop::payload::<super::panels::LayerDrag>(ui.ctx()).is_some_and(|d| d.0.contains(&id)) {
        p.rect_filled(r, 0.0, Color32::from_black_alpha(110));
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
        // Bitmap mask: a black mask with a fill effect waiting for a bitmap.
        let bitmap = MaskEffectKind::Fill {
            value: 1.0,
            source: SourceKind::Texture,
            texture: None,
            procedural: ProcParams::default(),
            projection: Projection::default(),
            range: [0.0, 1.0],
        };
        item(ui, o, "Add bitmap mask", "", !has_mask, vec![Cmd::AddMask(false), Cmd::AddEffect(bitmap)]);
        item(ui, o, "Add mask with color selection", "", !has_mask, vec![Cmd::AddMask(false), Cmd::AddEffect(MaskEffectKind::ColorSelect { colors: Vec::new(), tolerance: 0.05 })]);
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
        item(ui, o, "Duplicate layer", "Ctrl+J", true, vec![Cmd::Duplicate]);
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
            let sel = s.target == Target::ContentEffect(e.id);
            let fx = EffectRow { key: ("cfx", e.id), depth, mask: false, sel, enabled: e.enabled, icon: Icon::Effect, name: e.filter.kind.name().to_string(), blend: None, opacity: e.opacity };
            let (eresp, vresp, xresp) = effect_row(ui, &fx);
            if vresp.clicked() {
                cmds.push(Cmd::ToggleContentEffect(id, e.id));
            }
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
            for e in m.effects.iter().rev() {
                let sel = s.target == Target::MaskEffect(e.id);
                let (icon, name) = effect_look(&e.kind);
                let blends = !matches!(e.kind, MaskEffectKind::Levels { .. } | MaskEffectKind::Invert | MaskEffectKind::Threshold { .. } | MaskEffectKind::Blur { .. });
                let fx = EffectRow { key: ("mfx", e.id), depth, mask: true, sel, enabled: e.enabled, icon, name, blend: blends.then(|| short_mask_blend(e.blend)), opacity: e.opacity };
                let (eresp, vresp, xresp) = effect_row(ui, &fx);
                if vresp.clicked() {
                    cmds.push(Cmd::ToggleEffect(id, e.id));
                }
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

/// One effect row under a layer, like Painter's: indented to the
/// thumbnail column, eye, type icon, name, blending and opacity, remove.
struct EffectRow {
    key: (&'static str, u64),
    depth: usize,
    /// Mask effects are tinted red, content effects gray.
    mask: bool,
    sel: bool,
    enabled: bool,
    icon: Icon,
    name: String,
    blend: Option<&'static str>,
    opacity: f32,
}

/// Draws an effect row; returns the row, eye and remove responses.
fn effect_row(ui: &mut Ui, fx: &EffectRow) -> (egui::Response, egui::Response, egui::Response) {
    let (er, eresp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 25.0), Sense::click());
    let ex = er.min.x + 30.0 + fx.depth as f32 * 16.0;
    let body = Rect::from_min_max(Pos2::new(ex - 4.0, er.min.y), er.max);
    let p = ui.painter();
    let bg = match (fx.mask, fx.sel, eresp.hovered()) {
        (true, true, _) => Color32::from_rgb(132, 40, 38),
        (true, false, true) => Color32::from_rgb(70, 40, 40),
        (true, false, false) => Color32::from_rgb(54, 34, 34),
        (false, true, _) => crate::style::ROW_SELECTED,
        (false, false, true) => crate::style::warm(58),
        (false, false, false) => crate::style::warm(44),
    };
    p.rect_filled(body, 0.0, bg);
    p.line_segment([body.left_bottom(), body.right_bottom()], egui::Stroke::new(1.0f32, crate::style::warm(30)));
    let eye = Rect::from_center_size(Pos2::new(ex + 8.0, er.center().y), egui::vec2(16.0, 16.0));
    let vresp = ui.interact(eye, ui.id().with((fx.key, "eye")), Sense::click());
    icons::paint(ui.painter(), eye, if fx.enabled { Icon::Eye } else { Icon::EyeOff }, Color32::from_gray(if fx.enabled { 200 } else { 110 }));
    let p = ui.painter();
    icons::paint(p, Rect::from_center_size(Pos2::new(ex + 30.0, er.center().y), egui::vec2(15.0, 15.0)), fx.icon, Color32::from_gray(235));
    let text = if fx.enabled { Color32::from_gray(225) } else { Color32::from_gray(130) };
    let xr = Rect::from_center_size(Pos2::new(er.max.x - 12.0, er.center().y), egui::vec2(14.0, 14.0));
    let mut right = xr.min.x - 8.0;
    p.text(Pos2::new(right, er.center().y), egui::Align2::RIGHT_CENTER, format!("{:.0}", fx.opacity * 100.0), egui::FontId::proportional(11.5), text);
    right -= 30.0;
    if let Some(b) = fx.blend {
        p.text(Pos2::new(right, er.center().y), egui::Align2::RIGHT_CENTER, b, egui::FontId::proportional(11.5), text);
        right -= 34.0;
    }
    let name_rect = Rect::from_min_max(Pos2::new(ex + 44.0, er.min.y), Pos2::new(right.max(ex + 60.0), er.max.y));
    let galley = p.layout_no_wrap(fx.name.clone(), egui::FontId::proportional(12.0), text);
    p.with_clip_rect(name_rect).galley(Pos2::new(name_rect.min.x, er.center().y - galley.size().y / 2.0), galley, text);
    let xresp = ui.interact(xr, ui.id().with((fx.key, "del")), Sense::click()).on_hover_text("Remove effect (Del)");
    icons::paint(ui.painter(), xr.shrink(2.0), Icon::Close, if xresp.hovered() { Color32::WHITE } else { Color32::from_gray(170) });
    (eresp, vresp, xresp)
}

/// Icon and row name of a mask effect (Painter shows the generator's name).
fn effect_look(k: &MaskEffectKind) -> (Icon, String) {
    match k {
        MaskEffectKind::Paint => (Icon::Brush, "Paint".into()),
        MaskEffectKind::Fill { .. } => (Icon::Fill, "Fill".into()),
        MaskEffectKind::Generator(g) => (Icon::Generator, g.kind.name().into()),
        MaskEffectKind::Levels { .. } => (Icon::Levels, "Levels".into()),
        MaskEffectKind::Blur { .. } => (Icon::Effect, "Blur".into()),
        MaskEffectKind::Invert => (Icon::Effect, "Invert".into()),
        MaskEffectKind::Threshold { .. } => (Icon::Effect, "Threshold".into()),
        MaskEffectKind::ColorSelect { .. } => (Icon::Picker, "Color selection".into()),
    }
}

fn short_mask_blend(b: crate::document::MaskBlend) -> &'static str {
    let n = b.name();
    match n.len() {
        0..=4 => n,
        _ => &n[..4],
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
        let mut sub = Vec::new();
        effect_menu(ui, &mut sub);
        if !sub.is_empty() {
            picked.push(Cmd::Select(id, Target::Mask));
            picked.extend(sub);
        }
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

/// What a dragged layer row looks like under the pointer.
#[derive(Clone)]
struct LayerGhost {
    name: String,
    thumb: Option<egui::TextureId>,
    /// Layers dragged (the multi-selection).
    count: usize,
    size: egui::Vec2,
    /// Pointer offset from the row's corner when the drag began.
    grab: egui::Vec2,
}

/// The dragged layer row following the pointer, like Painter's.
pub(super) fn layer_drag_ghost(ctx: &egui::Context) {
    let gid = egui::Id::new("layer ghost");
    if egui::DragAndDrop::payload::<super::panels::LayerDrag>(ctx).is_none() {
        ctx.data_mut(|d| d.remove::<LayerGhost>(gid));
        return;
    }
    let (Some(g), Some(pos)) = (ctx.data(|d| d.get_temp::<LayerGhost>(gid)), ctx.pointer_interact_pos()) else { return };
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("layer drag ghost")));
    let r = Rect::from_min_size(pos - g.grab, g.size);
    // A second card behind for a multi-selection.
    if g.count > 1 {
        p.rect_filled(r.translate(egui::vec2(5.0, 5.0)), 3.0, Color32::from_rgba_unmultiplied(48, 48, 52, 170));
    }
    p.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(58, 58, 62, 215));
    p.rect_stroke(r, 3.0, egui::Stroke::new(1.0f32, crate::style::ACCENT));
    let t = Rect::from_min_size(Pos2::new(r.min.x + 30.0, r.center().y - THUMB / 2.0), egui::vec2(THUMB, THUMB));
    match g.thumb {
        Some(tex) => {
            p.image(tex, t, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::from_white_alpha(230));
        }
        None => {
            p.rect_filled(t, 2.0, crate::style::warm(80));
        }
    }
    let label = if g.count > 1 { format!("{}  (+{})", g.name, g.count - 1) } else { g.name.clone() };
    p.text(Pos2::new(t.max.x + 8.0, r.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.0), Color32::WHITE);
}

/// Thin bar under a thumbnail: red when that stack has effects (Painter).
fn effects_bar(p: &egui::Painter, thumb: Rect, has_effects: bool) {
    let c = if has_effects { crate::style::MASK } else { crate::style::warm(74) };
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

/// The "Add ..." effect items: on a mask they add mask effects, on content
/// they add content effects (filters, levels). Only what exists is listed.
fn add_effect_items(ui: &mut Ui, cmds: &mut Vec<Cmd>, on_mask: bool) {
    if on_mask {
        effect_menu(ui, cmds);
        return;
    }
    if ui.button("Add levels").clicked() {
        cmds.push(Cmd::AddContentEffect(FilterKind::Levels));
        ui.close_menu();
    }
    super::widgets::sub_menu(ui, "Add filter", |ui| {
        for k in FilterKind::ALL {
            if ui.button(k.name()).clicked() {
                cmds.push(Cmd::AddContentEffect(*k));
                ui.close_menu();
            }
        }
    });
}
