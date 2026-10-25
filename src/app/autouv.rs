//! The Auto UV window (top right "Auto UV" button): new UVs for the open
//! project. The unwrap runs on a thread and shows each texture set's layout
//! and numbers before anything changes; Apply carries the painted layers
//! and masks over to the new UVs (`Document::apply_new_uvs`), clears the
//! history and asks for a bake (mesh maps depend on the UVs).

use super::*;
use crate::mesh::{MeshData, UvMethod, UvReport, UvSettings};
use eframe::egui::{Color32, RichText};

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(widgets::DIM));
}

type UvJob = std::thread::JoinHandle<anyhow::Result<(MeshData, Vec<UvReport>)>>;

pub struct UvDialog {
    pub settings: UvSettings,
    job: Option<(UvJob, Instant)>,
    /// The last unwrap: the new mesh, its reports, how long it took.
    result: Option<(Arc<MeshData>, Vec<UvReport>, f32)>,
    /// Layout preview: texture set shown and its image.
    preview: Option<(usize, egui::TextureHandle)>,
    pub set: usize,
    /// Texture sets to unwrap (the others keep their UVs).
    pub sets: Vec<bool>,
    error: Option<String>,
}

impl UvDialog {
    pub fn new(page: u32) -> Self {
        let mut settings = UvSettings::default();
        settings.smart.page = page.clamp(512, 4096);
        UvDialog { settings, job: None, result: None, preview: None, set: 0, sets: Vec::new(), error: None }
    }
}

/// A texture set's UV layout as lines on a dark square.
fn layout_image(mesh: &MeshData, sub: usize, size: u32) -> image::RgbaImage {
    let mut img = image::RgbaImage::from_pixel(size, size, image::Rgba([24, 24, 26, 255]));
    let Some(sm) = mesh.submeshes.get(sub) else { return img };
    let s = size as f32 - 1.0;
    let mut line = |a: [f32; 2], b: [f32; 2]| {
        let (x0, y0, x1, y1) = (a[0] * s, a[1] * s, b[0] * s, b[1] * s);
        let n = ((x1 - x0).abs().max((y1 - y0).abs()).ceil() as usize).max(1);
        for k in 0..=n {
            let t = k as f32 / n as f32;
            let (x, y) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
            if x >= 0.0 && y >= 0.0 && x <= s && y <= s {
                img.put_pixel(x as u32, y as u32, image::Rgba([242, 107, 29, 255]));
            }
        }
    };
    for t in sm.indices.chunks_exact(3) {
        let uv = |i: u32| mesh.vertices[i as usize].uv;
        let (a, b, c) = (uv(t[0]), uv(t[1]), uv(t[2]));
        line(a, b);
        line(b, c);
        line(c, a);
    }
    img
}

impl RetroApp {
    /// Closes the window; its preview texture may be drawn this frame, so it
    /// is retired rather than freed.
    pub(super) fn close_uv_dialog(&mut self) {
        if let Some((_, h)) = self.uv_dialog.take().and_then(|d| d.preview) {
            self.retired.push(h);
        }
    }

    pub(super) fn open_uv_dialog(&mut self) {
        let Some(doc) = &self.doc else { return };
        let res = doc.sets.get(doc.active).map(|s| s.resolution()).unwrap_or(2048);
        let mut d = UvDialog::new(res);
        d.set = doc.active;
        d.sets = vec![true; doc.sets.len()];
        self.uv_dialog = Some(d);
    }

    pub(super) fn uv_window(&mut self, ctx: &egui::Context) {
        let Some(doc) = &self.doc else {
            self.close_uv_dialog();
            return;
        };
        let names: Vec<String> = doc.sets.iter().map(|s| s.name.clone()).collect();
        // Chosen texture sets, by submesh (what the unwrap works on).
        let subs: Vec<usize> = doc.sets.iter().map(|s| s.submesh).collect();
        let mesh = doc.mesh.clone();
        let Some(d) = &mut self.uv_dialog else { return };
        // A finished unwrap: keep it and draw the layout of the shown set.
        if d.job.as_ref().is_some_and(|(j, _)| j.is_finished()) {
            let (job, started) = d.job.take().unwrap();
            match job.join() {
                Ok(Ok((m, reports))) => {
                    d.result = Some((Arc::new(m), reports, started.elapsed().as_secs_f32()));
                    // Not drawn yet this frame: safe to free.
                    d.preview = None;
                    d.error = None;
                }
                Ok(Err(e)) => d.error = Some(format!("{e:#}")),
                Err(_) => d.error = Some("the unwrap stopped unexpectedly".into()),
            }
        }
        if d.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let shown_mesh = d.result.as_ref().map(|r| r.0.clone()).unwrap_or_else(|| mesh.clone());
        if d.preview.as_ref().is_none_or(|(s, _)| *s != d.set) {
            let sub = doc.sets.get(d.set).map(|s| s.submesh).unwrap_or(0);
            let img = layout_image(&shown_mesh, sub, 360);
            let ci = egui::ColorImage::from_rgba_unmultiplied([360, 360], img.as_raw());
            d.preview = Some((d.set, ctx.load_texture("auto uv preview", ci, egui::TextureOptions::LINEAR)));
        }
        let mut open = true;
        let (mut compute, mut apply, mut cancel) = (false, false, false);
        egui::Window::new(RichText::new("AUTO UV").font(crate::style::bold(11.5)))
            .id(egui::Id::new("auto uv window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(300.0);
                        if names.len() > 1 {
                            d.sets.resize(names.len(), true);
                            ui.label(RichText::new("Texture sets").size(12.0).color(widgets::LABEL));
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                                for (i, n) in names.iter().enumerate() {
                                    let r = widgets::chip(ui, n, d.sets[i], "Unwrap this texture set (the others keep their UVs)");
                                    super::mark(ui.ctx(), format!("auto uv set:{i}"), r.rect);
                                    if r.clicked() {
                                        d.sets[i] = !d.sets[i];
                                    }
                                }
                            });
                            hint(ui, "Sets left out keep their UVs and their paint.");
                            ui.add_space(4.0);
                        }
                        let s = &mut d.settings;
                        widgets::combo_row(ui, "Method", "uv method", &mut s.method, &UvMethod::ALL, UvMethod::name);
                        hint(ui, s.method.about());
                        if s.method == UvMethod::Smart {
                            widgets::Slider::new("Hard edges", &mut s.smart.hard_angle, 20.0..=90.0).decimals(0).show(ui);
                            hint(ui, "Edges sharper than this angle become seams.");
                            widgets::toggle_row(ui, "Hide seams", &mut s.smart.hide_seams);
                            hint(ui, "Seams run where they show least: occluded places, folds, creases.");
                            widgets::Slider::new("Max stretch", &mut s.smart.max_stretch, 1.2..=4.0).decimals(1).show(ui);
                            hint(ui, "Lower: less distortion, more islands.");
                            widgets::toggle_row(ui, "Keep upright", &mut s.smart.upright);
                            hint(ui, "Islands stand as on the model, limbs straight. Off: denser packing.");
                        }
                        let mut pad = s.smart.padding as f32;
                        if widgets::Slider::new("Padding", &mut pad, 1.0..=16.0).decimals(0).show(ui) {
                            s.smart.padding = pad.round() as u32;
                        }
                        ui.add_space(6.0);
                        let busy = d.job.is_some();
                        let any = names.len() <= 1 || d.sets.iter().any(|s| *s);
                        let label = if busy { "Unwrapping..." } else if !any { "Choose a texture set" } else if d.result.is_some() { "Unwrap again" } else { "Unwrap" };
                        if ui.add_enabled(!busy && any, egui::Button::new(label).min_size(egui::vec2(ui.available_width(), 28.0))).clicked() {
                            compute = true;
                        }
                        if let Some(e) = &d.error {
                            ui.colored_label(Color32::from_rgb(240, 90, 80), e);
                        }
                        if let Some((_, reports, secs)) = &d.result {
                            ui.add_space(4.0);
                            ui.label(RichText::new(format!("Unwrapped in {secs:.1} s")).size(11.5).color(crate::style::TEXT3));
                            egui::Grid::new("uv report").num_columns(4).spacing(egui::vec2(10.0, 3.0)).show(ui, |ui| {
                                for h in ["Texture set", "Islands", "Used", "Stretch"] {
                                    ui.label(RichText::new(h).size(11.0).color(crate::style::TEXT3));
                                }
                                ui.end_row();
                                for r in reports {
                                    ui.label(RichText::new(&r.name).size(11.5));
                                    ui.label(RichText::new(r.charts.to_string()).size(11.5));
                                    ui.label(RichText::new(format!("{:.0} %", r.coverage * 100.0)).size(11.5));
                                    ui.label(RichText::new(format!("{:.2}", r.stretch_mean)).size(11.5)).on_hover_text("Area stretch: 1 is none (worst 5 % in the tooltip of the layout)");
                                    ui.end_row();
                                }
                            });
                        }
                    });
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        if names.len() > 1 {
                            egui::ComboBox::from_id_salt("uv set").width(200.0).selected_text(names.get(d.set).cloned().unwrap_or_default()).show_ui(ui, |ui| {
                                for (i, n) in names.iter().enumerate() {
                                    ui.selectable_value(&mut d.set, i, n);
                                }
                            });
                        }
                        let title = if d.result.is_some() { "New layout" } else { "Current layout" };
                        ui.label(RichText::new(title).size(11.5).color(crate::style::TEXT3));
                        if let Some((_, h)) = &d.preview {
                            ui.image((h.id(), egui::vec2(300.0, 300.0)));
                        }
                    });
                });
                ui.separator();
                hint(
                    ui,
                    "Apply moves the painted layers and masks to the new UVs, clears the history and asks for a bake (mesh maps follow the UVs). File > Export mesh writes the new UVs out.",
                );
                ui.horizontal(|ui| {
                    if ui.add_enabled(d.result.is_some() && d.job.is_none(), egui::Button::new(RichText::new("Apply").color(Color32::WHITE)).fill(crate::style::ACCENT)).clicked() {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if compute {
            let settings = d.settings;
            let m = mesh.clone();
            // The preview drawn this frame stays until the new layout replaces
            // it (next frames).
            d.result = None;
            d.error = None;
            let mut only = vec![true; m.submeshes.len()];
            for (i, sub) in subs.iter().enumerate() {
                if let Some(o) = only.get_mut(*sub) {
                    *o = d.sets.get(i).copied().unwrap_or(true);
                }
            }
            d.job = Some((std::thread::spawn(move || crate::mesh::unwrap_sets(&m, &settings, &only)), Instant::now()));
        }
        if apply {
            if let Some((m, _, _)) = d.result.take() {
                self.close_uv_dialog();
                self.apply_auto_uv((*m).clone());
                return;
            }
        }
        if !open || cancel {
            self.close_uv_dialog();
        }
    }

    /// Swaps in the unwrapped mesh, its paint carried over.
    pub(super) fn apply_auto_uv(&mut self, mesh: MeshData) {
        let Some(doc) = &mut self.doc else { return };
        let notes = doc.apply_new_uvs(&self.engine, mesh);
        self.reset_doc_state();
        for n in notes {
            self.message(n);
        }
        self.message("Auto UV applied: paint moved to the new UVs. Bake the mesh maps again for the generators.");
        self.open_bake_dialog();
    }
}
