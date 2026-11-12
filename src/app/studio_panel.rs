//! The packshot studio's panel, its frame and lights over the view, and the
//! final render (its own path tracer and post pass at the output size, a
//! few samples per frame, denoised, saved as PNG).

use super::RetroApp;
use crate::studio::{Format, LightKind, StudioGpu, StudioLight};
use eframe::egui::{self, Color32, Pos2, Rect, RichText};
use glam::Vec3;

pub(super) struct StudioJob {
    pt: crate::engine::pathtrace::PathTracer,
    vp: crate::engine::viewport::Viewport,
    path: std::path::PathBuf,
    inv_view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    gpu: StudioGpu,
    samples: u32,
    started: std::time::Instant,
    cancel: bool,
}

impl RetroApp {
    /// The model's center, size (half its box's diagonal) and lowest point.
    fn studio_bounds(&self) -> Option<(Vec3, f32, f32)> {
        let d = self.doc.as_ref()?;
        let (min, max) = (d.mesh.bbox_min, d.mesh.bbox_max);
        Some(((min + max) * 0.5, ((max - min).length() * 0.5).max(1e-4), min.y))
    }

    /// The path tracer's studio for the current camera (off outside it).
    pub(super) fn studio_gpu(&self) -> StudioGpu {
        match (&self.doc, self.studio_bounds()) {
            (Some(d), Some((c, s, g))) if self.view.studio => d.studio.gpu(self.camera.yaw, self.camera.eye(), c, s, g, 2f32.powf(self.view.exposure)),
            _ => StudioGpu::default(),
        }
    }

    /// The studio button: on (its panel docks with the others, the model
    /// centered in the frame) or off (the render mode off too).
    pub(super) fn toggle_studio(&mut self) {
        if self.view.studio {
            self.leave_studio();
            return;
        }
        self.view.studio = true;
        self.view.path_trace = true;
        self.view.display = super::Display::Material;
        self.camera.ortho = false;
        // A locked shot stays as it was set.
        if !self.doc.as_ref().is_some_and(|d| d.studio.lock_camera) {
            self.studio_recenter = 3;
        }
    }

    pub(super) fn leave_studio(&mut self) {
        self.view.studio = false;
        self.view.path_trace = false;
    }

    /// What Claude should know of the product and the camera to light it.
    pub(super) fn studio_facts(&self) -> String {
        let (Some(d), Some((_, s, _))) = (&self.doc, self.studio_bounds()) else { return String::new() };
        let e = (d.mesh.bbox_max - d.mesh.bbox_min) / s;
        let st = &d.studio;
        format!(
            "the product is {:.2} wide, {:.2} tall and {:.2} deep (object sizes); the camera looks from {:.0} degrees above level with a {:.0} mm lens, format {}.",
            e.x,
            e.y,
            e.z,
            self.camera.pitch.to_degrees(),
            st.lens_mm,
            st.format.name()
        )
    }

    /// No packshot render running (a Claude check can start one).
    pub(super) fn studio_job_idle(&self) -> bool {
        self.studio_job.is_none()
    }

    /// The model's points for framing (at most about 20000 of them).
    fn studio_points(&self) -> Vec<Vec3> {
        let Some(d) = &self.doc else { return Vec::new() };
        let step = (d.mesh.vertices.len() / 20000).max(1);
        d.mesh.vertices.iter().step_by(step).map(|v| Vec3::from(v.pos)).collect()
    }

    /// The model in the middle of the frame, filling `FRAME_FILL` of it,
    /// seen from the current angle (a locked shot follows).
    pub(super) fn studio_recenter_now(&mut self) {
        let Some((_, fr)) = self.studio_frame() else { return };
        let pts = self.studio_points();
        let Some(doc) = &mut self.doc else { return };
        let aspect = fr.width() / fr.height().max(1.0);
        let tan_y = doc.studio.tan_half_y(aspect);
        crate::studio::fit_camera(&mut self.camera, &pts, tan_y, aspect, crate::studio::FRAME_FILL);
        if doc.studio.lock_camera {
            doc.studio.shot = Some(crate::camera::OrbitCamera { shift: [0.0; 2], ..self.camera });
        }
    }

    /// The 3D view and the format's frame in it (points): the frame fits
    /// the part the panels leave visible, under the view bar and above the
    /// tool dock and its options.
    fn studio_frame(&self) -> Option<(Rect, Rect)> {
        let doc = self.doc.as_ref()?;
        let view = self.vp_rect;
        if !view.is_positive() {
            return None;
        }
        let mut vis = self.visible_part(view);
        if !self.ui_hidden {
            vis.min.y = vis.min.y.max(view.min.y + crate::style::GAP + 46.0);
            let bars = [self.options_rect, self.dock_rect].into_iter().filter(|r| r.is_positive() && r.x_range().intersects(vis.x_range()));
            if let Some(top) = bars.map(|r| r.min.y).reduce(f32::min) {
                vis.max.y = vis.max.y.min(top - 6.0);
            }
        }
        let vis = if vis.width() > 80.0 && vis.height() > 80.0 { vis } else { view };
        let (x, y, w, h) = doc.studio.frame_in(vis.width(), vis.height());
        Some((view, Rect::from_min_size(vis.min + egui::vec2(x, y), egui::vec2(w, h))))
    }

    /// While in the studio: the lens's field of view on the frame, the
    /// camera's target at the frame's center (a lens shift), a locked shot
    /// kept. The view's own field of view comes back after.
    pub(super) fn studio_camera(&mut self) {
        let Some((view, fr)) = self.studio_frame().filter(|_| self.view.studio) else {
            if let Some(f) = self.studio_fov.take() {
                self.camera.fov_y = f;
            }
            self.camera.shift = [0.0; 2];
            return;
        };
        let st = &self.doc.as_ref().unwrap().studio;
        if self.studio_fov.is_none() {
            self.studio_fov = Some(self.camera.fov_y);
        }
        let t = st.tan_half_y(fr.width() / fr.height().max(1.0)) * view.height() / fr.height().max(1.0);
        self.camera.fov_y = 2.0 * t.atan();
        self.camera.ortho = false;
        self.camera.shift = [(fr.center().x - view.center().x) / (view.width() * 0.5), -(fr.center().y - view.center().y) / (view.height() * 0.5)];
        if st.lock_camera {
            if let Some(s) = st.shot {
                (self.camera.target, self.camera.yaw, self.camera.pitch, self.camera.distance) = (s.target, s.yaw, s.pitch, s.distance);
            }
        }
        if self.studio_recenter > 0 {
            self.studio_recenter -= 1;
            self.studio_recenter_now();
        }
    }

    /// The format's frame (outside it darkened) and the lights that are on,
    /// over the 3D view; nothing under the panels.
    pub(super) fn studio_overlay(&self, painter: &egui::Painter, rect: Rect) {
        let Some(doc) = &self.doc else { return };
        if !self.view.studio {
            return;
        }
        let st = &doc.studio;
        let Some((_, frame)) = self.studio_frame() else { return };
        if st.format != Format::View {
            let shade = Color32::from_black_alpha(150);
            painter.rect_filled(Rect::from_min_max(rect.min, Pos2::new(rect.max.x, frame.min.y)), 0.0, shade);
            painter.rect_filled(Rect::from_min_max(Pos2::new(rect.min.x, frame.max.y), rect.max), 0.0, shade);
            painter.rect_filled(Rect::from_min_max(Pos2::new(rect.min.x, frame.min.y), Pos2::new(frame.min.x, frame.max.y)), 0.0, shade);
            painter.rect_filled(Rect::from_min_max(Pos2::new(frame.max.x, frame.min.y), Pos2::new(rect.max.x, frame.max.y)), 0.0, shade);
            painter.rect_stroke(frame, 0.0, egui::Stroke::new(1.0f32, Color32::from_gray(150)));
        }
        // The lights where they stand.
        let Some((center, size, _)) = self.studio_bounds() else { return };
        let vis = self.visible_part(rect);
        let painter = painter.with_clip_rect(vis);
        let vp = self.camera.view_proj(rect.width() / rect.height().max(1.0));
        let font = egui::FontId::proportional(11.0);
        for l in st.lights.iter().filter(|l| l.on) {
            let (pos, ..) = st.place(l, self.camera.yaw, center, size);
            let c = vp * pos.extend(1.0);
            if c.w <= 1e-6 {
                continue;
            }
            let ndc = c.truncate() / c.w;
            let p = Pos2::new(rect.min.x + (ndc.x * 0.5 + 0.5) * rect.width(), rect.min.y + (0.5 - ndc.y * 0.5) * rect.height());
            if !vis.shrink(6.0).contains(p) {
                continue;
            }
            let col = if l.kind == LightKind::Flag { Color32::BLACK } else { Color32::from_rgb((l.color[0] * 255.0) as u8, (l.color[1] * 255.0) as u8, (l.color[2] * 255.0) as u8) };
            let col = if l.kind == LightKind::Card { col.gamma_multiply(0.8) } else { col };
            painter.circle(p, 5.0, col, egui::Stroke::new(1.0f32, Color32::from_gray(140)));
            painter.text(p + egui::vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, &l.name, font.clone(), Color32::from_gray(220));
        }
    }

    /// The studio's panel (docked with the others while the studio is on).
    pub(super) fn studio_panel(&mut self, ui: &mut egui::Ui) {
        let mut close = false;
        super::dock::panel_header(ui, "Studio", "Packshot lights and camera", |ui| close = super::icons::button(ui, super::icons::Icon::Close, false, 24.0, "Leave the studio").clicked());
        if close {
            self.leave_studio();
            return;
        }
        let Some(doc) = &mut self.doc else { return };
        let mut st = doc.studio.clone();
        let mut render = false;
        let mut cancel = false;
        let job = self.studio_job.as_ref().map(|j| (j.pt.frame, j.samples));
        let exposure = &mut self.view.exposure;
        let prev = &mut self.studio_prev;
        let camera = self.camera;
        let mut msg = None;
        let mut go = None;
        let mut render_all = false;
        let mut recenter = false;
        egui::ScrollArea::vertical().id_salt("studio scroll").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::none().inner_margin(egui::Margin { left: 12.0, right: 14.0, top: 4.0, bottom: 10.0 }).show(ui, |ui| {
                ui.label(RichText::new("Lighting setup").strong());
                ui.horizontal(|ui| {
                    let mut pick = None;
                    let mut mine = None;
                    egui::ComboBox::from_id_salt("studio setup").width(ui.available_width().min(260.0)).selected_text("Start from a setup...").show_ui(ui, |ui| {
                        for (i, n) in crate::studio::SETUPS.iter().enumerate() {
                            if ui.selectable_label(false, *n).clicked() {
                                pick = Some(i);
                            }
                        }
                        let presets = crate::studio::user_presets();
                        if !presets.is_empty() {
                            ui.separator();
                            ui.label(RichText::new("My presets").size(11.0).color(crate::style::TEXT3));
                        }
                        for (n, path) in presets {
                            ui.horizontal(|ui| {
                                if ui.selectable_label(false, &n).clicked() {
                                    mine = Some(path.clone());
                                }
                                if ui.small_button("Delete").on_hover_text("Delete this preset from the library").clicked() {
                                    let _ = std::fs::remove_file(&path);
                                    msg = Some(format!("Lighting preset \"{n}\" deleted."));
                                }
                            });
                        }
                    });
                    if let Some(i) = pick {
                        *prev = Some(st.clone());
                        st.setup(i);
                    }
                    if let Some(path) = mine {
                        match crate::studio::load_preset(&path) {
                            Some(p) => {
                                *prev = Some(st.clone());
                                st.take_lighting(&p);
                            }
                            None => msg = Some(format!("Could not read the preset {}.", path.display())),
                        }
                    }
                });
                ui.horizontal(|ui| {
                    let name_id = egui::Id::new("studio preset name");
                    let mut name: String = ui.data(|d| d.get_temp(name_id)).unwrap_or_default();
                    ui.add(egui::TextEdit::singleline(&mut name).hint_text("Preset name").desired_width(140.0));
                    let save = ui.button("Save as preset").on_hover_text("Keep these lights, background and floor in your library, for any project");
                    if save.clicked() {
                        let n = if name.trim().is_empty() { format!("My lights {}", crate::studio::user_presets().len() + 1) } else { name.trim().to_string() };
                        msg = Some(match crate::studio::save_preset(&n, &st) {
                            Ok(p) => format!("Lighting preset saved to {}.", p.display()),
                            Err(e) => format!("Could not save the preset: {e:#}"),
                        });
                        name.clear();
                    }
                    ui.data_mut(|d| d.insert_temp(name_id, name));
                });
                if let Some(p) = prev.as_ref() {
                    if ui.button("Back to my lights").on_hover_text("The lights, background, floor and room before the setup").clicked() {
                        st.take_lighting(p);
                        *prev = None;
                    }
                }
                ui.add_space(6.0);
                ui.label(RichText::new("To light it like a photo: in the Claude panel, attach the photo and ask for its lighting.").size(11.0).color(crate::style::TEXT3));
                ui.add_space(4.0);
                ui.label(RichText::new("Lights").strong());
                let mut remove = None;
                let n = st.lights.len();
                for (i, l) in st.lights.iter_mut().enumerate() {
                    if light_row(ui, i, l, n > 1) {
                        remove = Some(i);
                    }
                }
                if let Some(i) = remove {
                    st.lights.remove(i);
                }
                if st.lights.len() < crate::studio::MAX_LIGHTS {
                    ui.horizontal(|ui| {
                        let mut add = None;
                        egui::ComboBox::from_id_salt("studio add").width(ui.available_width().min(260.0)).selected_text("Add a light...").show_ui(ui, |ui| {
                            for k in LightKind::ALL {
                                if ui.selectable_label(false, k.name()).clicked() {
                                    add = Some(k);
                                }
                            }
                        });
                        if let Some(k) = add {
                            let mut l = StudioLight { name: format!("{} {}", k.name(), st.lights.len() + 1), ..Default::default() };
                            set_kind(&mut l, k);
                            st.lights.push(l);
                        }
                    });
                }
                ui.checkbox(&mut st.follow_camera, "Lights follow the camera").on_hover_text("Azimuth 0 is the camera's side: orbiting keeps the lighting of the shot");
                ui.separator();
                ui.label(RichText::new("Background and ground").strong());
                egui::Grid::new("studio bg").num_columns(2).show(ui, |ui| {
                    ui.label("Background");
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        super::widgets::color_bar_named(ui, "studio bg", "Background", &mut st.background);
                    });
                    ui.end_row();
                    ui.label("Falloff").on_hover_text("The background darkens away from the product: 0 even");
                    super::widgets::slide(ui, &mut st.falloff, 0.0..=1.0, false, "");
                    ui.end_row();
                    if st.falloff > 0.0 {
                        ui.label("Lit pool size").on_hover_text("The bright part behind the product, in frame heights");
                        super::widgets::slide(ui, &mut st.pool, 0.2..=4.0, false, "");
                        ui.end_row();
                    }
                    ui.label("Shadows on the ground");
                    ui.checkbox(&mut st.ground_shadows, "");
                    ui.end_row();
                    if st.ground_shadows {
                        ui.label("Shadow opacity");
                        super::widgets::slide(ui, &mut st.shadow_opacity, 0.0..=1.0, false, "");
                        ui.end_row();
                    }
                    ui.label("Glossy floor (plexi)").on_hover_text("How much the floor reflects the product: 0 a matte seamless");
                    super::widgets::slide(ui, &mut st.floor_reflection, 0.0..=1.0, false, "");
                    ui.end_row();
                    if st.floor_reflection > 0.0 {
                        ui.label("Reflection fade").on_hover_text("The reflection fades out over this height of the product (0: no fade)");
                        super::widgets::slide(ui, &mut st.reflection_fade, 0.0..=4.0, false, " x size");
                        ui.end_row();
                    }
                    ui.label("Room bounce");
                    super::widgets::slide(ui, &mut st.room, 0.0..=1.0, false, "");
                    ui.end_row();
                    ui.label("Environment light");
                    super::widgets::slide(ui, &mut st.environment, 0.0..=2.0, false, "");
                    ui.end_row();
                });
                ui.separator();
                ui.label(RichText::new("Camera").strong());
                egui::Grid::new("studio cam").num_columns(2).show(ui, |ui| {
                    ui.label("Lens");
                    super::widgets::slide_dec(ui, &mut st.lens_mm, 14.0..=300.0, 0, " mm");
                    ui.end_row();
                    ui.label("");
                    ui.horizontal_wrapped(|ui| {
                        for mm in [24.0, 35.0, 50.0, 85.0, 100.0, 135.0] {
                            if ui.selectable_label((st.lens_mm - mm).abs() < 0.5, format!("{mm:.0}")).clicked() {
                                st.lens_mm = mm;
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Format");
                    egui::ComboBox::from_id_salt("studio format").width(160.0).selected_text(st.format.name()).show_ui(ui, |ui| {
                        for f in Format::ALL {
                            ui.selectable_value(&mut st.format, f, f.name());
                        }
                    });
                    ui.end_row();
                    ui.label("");
                    if ui.button("Recenter the model").on_hover_text("The model in the middle of the frame, filling it as packshots do, seen from this angle").clicked() {
                        recenter = true;
                    }
                    ui.end_row();
                    ui.label("Lock the camera");
                    let mut lock = st.lock_camera;
                    if ui.checkbox(&mut lock, "").on_hover_text("Keep the shot: orbit, pan and zoom do nothing").changed() {
                        st.lock_camera = lock;
                        st.shot = lock.then_some(crate::camera::OrbitCamera { shift: [0.0; 2], ..camera });
                    }
                    ui.end_row();
                    ui.label("Exposure (EV)");
                    super::widgets::slide(ui, exposure, -6.0..=6.0, false, "");
                    ui.end_row();
                });
                ui.add_space(4.0);
                ui.label(RichText::new("Camera angles").strong());
                let here = crate::camera::OrbitCamera { shift: [0.0; 2], ..camera };
                let (lens, format) = (st.lens_mm, st.format);
                let mut remove = None;
                for (i, a) in st.angles.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        let c = &a.camera;
                        let current = (c.yaw - here.yaw).abs() < 1e-3 && (c.pitch - here.pitch).abs() < 1e-3 && (c.distance - here.distance).abs() < 1e-3 * c.distance.max(1.0) && a.lens_mm == lens && a.format == format;
                        if ui.selectable_label(current, "View").on_hover_text("Look through this angle").clicked() {
                            go = Some(i);
                        }
                        ui.add(egui::TextEdit::singleline(&mut a.name).desired_width(110.0));
                        if ui.small_button("Update").on_hover_text("This angle becomes the current view, lens and format").clicked() {
                            (a.camera, a.lens_mm, a.format) = (here, lens, format);
                        }
                        if ui.small_button("Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    st.angles.remove(i);
                }
                ui.horizontal(|ui| {
                    if ui.button("Save this angle").on_hover_text("Keep the current view, lens and format as a camera angle").clicked() {
                        let name = format!("Angle {}", st.angles.len() + 1);
                        st.angles.push(crate::studio::CameraAngle { name, camera: here, lens_mm: lens, format });
                    }
                    if !st.angles.is_empty() && job.is_none() && ui.button("Render every angle...").on_hover_text("One image per angle, into a folder").clicked() {
                        render_all = true;
                    }
                });
                ui.separator();
                ui.label(RichText::new("Render the image").strong());
                egui::Grid::new("studio out").num_columns(2).show(ui, |ui| {
                    ui.label("Long side");
                    egui::ComboBox::from_id_salt("studio size").width(160.0).selected_text(format!("{} px", st.out_long)).show_ui(ui, |ui| {
                        for s in [1024u32, 2048, 3000, 4096] {
                            ui.selectable_value(&mut st.out_long, s, format!("{s} px"));
                        }
                    });
                    ui.end_row();
                    ui.label("Samples");
                    egui::ComboBox::from_id_salt("studio spp").width(160.0).selected_text(st.out_samples.to_string()).show_ui(ui, |ui| {
                        for s in [128u32, 256, 512, 1024, 2048] {
                            ui.selectable_value(&mut st.out_samples, s, s.to_string());
                        }
                    });
                    ui.end_row();
                });
                match job {
                    Some((done, total)) => {
                        ui.horizontal(|ui| {
                            let f = done as f32 / total.max(1) as f32;
                            ui.add(egui::ProgressBar::new(f).desired_width(200.0).text(format!("{done} / {total} samples")));
                            cancel = ui.button("Cancel").clicked();
                        });
                    }
                    None => render = ui.button("Render and save the image...").clicked(),
                }
            });
        });
        if let Some(a) = go.and_then(|i| st.angles.get(i)) {
            (st.lens_mm, st.format) = (a.lens_mm, a.format);
            if st.lock_camera {
                st.shot = Some(a.camera);
            }
            let c = a.camera;
            (self.camera.target, self.camera.yaw, self.camera.pitch, self.camera.distance) = (c.target, c.yaw, c.pitch, c.distance);
        }
        let doc = self.doc.as_mut().unwrap();
        if st != doc.studio {
            doc.studio = st;
            doc.modified = true;
        }
        if let Some(m) = msg {
            self.message(m);
        }
        if recenter {
            self.studio_recenter_now();
        }
        if cancel {
            self.studio_queue.clear();
            if let Some(j) = &mut self.studio_job {
                j.cancel = true;
            }
        }
        if render {
            self.start_studio_render();
        }
        if render_all {
            self.render_every_angle();
        }
    }

    /// Renders each camera angle into a folder, one after the other.
    fn render_every_angle(&mut self) {
        let Some(doc) = &self.doc else { return };
        let Some(dir) = super::file_dialog().and_then(|f| f.pick_folder()) else { return };
        let base = crate::studio::file_safe(&doc.mesh_name);
        self.studio_queue = doc
            .studio
            .angles
            .iter()
            .map(|a| {
                let mut st = doc.studio.clone();
                (st.lens_mm, st.format) = (a.lens_mm, a.format);
                (a.camera, st, dir.join(format!("{base} {}.png", crate::studio::file_safe(&a.name))))
            })
            .collect();
        self.next_studio_shot();
    }

    /// Starts the next shot of "Render every angle" (false: none left).
    fn next_studio_shot(&mut self) -> bool {
        if self.studio_queue.is_empty() {
            return false;
        }
        let (cam, st, path) = self.studio_queue.remove(0);
        self.render_studio_shot(cam, &st, path);
        true
    }

    fn start_studio_render(&mut self) {
        let Some(doc) = &self.doc else { return };
        let Some(path) = super::file_dialog().and_then(|f| f.add_filter("PNG image", &["png"]).set_file_name(format!("{} packshot.png", doc.mesh_name)).save_file()) else { return };
        let path = if path.extension().is_none() { path.with_extension("png") } else { path };
        let st = doc.studio.clone();
        self.studio_queue.clear();
        self.render_studio_shot(self.camera, &st, path);
    }

    /// Starts the final render of `cam` with `st` (its lens and format)
    /// into `path`.
    pub(super) fn render_studio_shot(&mut self, cam: crate::camera::OrbitCamera, st: &crate::studio::Studio, path: std::path::PathBuf) {
        let Some(doc) = &self.doc else { return };
        let fr = self.studio_frame().map(|f| f.1);
        let aspect = st.format.aspect().or(fr.map(|f| f.width() / f.height().max(1.0))).unwrap_or(1.0);
        let (w, h) = if aspect >= 1.0 { (st.out_long, (st.out_long as f32 / aspect).round() as u32) } else { ((st.out_long as f32 * aspect).round() as u32, st.out_long) };
        let mut cam = cam;
        cam.shift = [0.0; 2];
        cam.fov_y = 2.0 * st.tan_half_y(aspect).atan();
        let view_proj = cam.view_proj(aspect);
        let gpu = match self.studio_bounds() {
            Some((c, s, g)) => st.gpu(cam.yaw, cam.eye(), c, s, g, 2f32.powf(self.view.exposure)),
            None => StudioGpu::default(),
        };
        let e = &self.engine;
        let mut pt = crate::engine::pathtrace::PathTracer::new(&e.device, &e.queue);
        pt.build_scene(&e.device, doc, 1);
        pt.bounces = self.engine.pathtracer.bounces.max(6);
        pt.max_samples = st.out_samples;
        pt.denoise_on = self.engine.pathtracer.denoise_on;
        let mut out = crate::engine::viewport::Viewport::new(&e.device, &e.queue, &e.env);
        out.resize(&e.device, w, h);
        self.studio_job = Some(StudioJob {
            pt,
            vp: out,
            path,
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            eye: cam.eye().extend(doc.mesh.diagonal()).into(),
            gpu,
            samples: st.out_samples,
            started: std::time::Instant::now(),
            cancel: false,
        });
        self.message(format!("Rendering the packshot ({w} x {h}, {} samples)...", st.out_samples));
    }

    /// A few samples of the final render into `enc` (true while it runs:
    /// the view's own render waits).
    pub(super) fn studio_step(&mut self, enc: &mut wgpu::CommandEncoder) -> bool {
        let (Some(job), Some(doc)) = (&mut self.studio_job, &self.doc) else { return false };
        if job.cancel || job.pt.frame >= job.samples {
            return true;
        }
        let v = &self.view;
        let mut emi = [0.0f32; 8];
        let (mut am, mut cut) = ([0.0f32; 8], [0.5f32; 8]);
        let slots: Vec<[&crate::engine::Tex; 4]> = doc
            .sets
            .iter()
            .take(crate::engine::pathtrace::MAX_SETS)
            .enumerate()
            .map(|(i, s)| {
                emi[i] = s.settings.emissive_intensity;
                am[i] = s.shader_alpha();
                cut[i] = s.settings.alpha_cutoff;
                [&s.gpu.final_base, &s.gpu.final_orm, &s.gpu.final_normal, &s.gpu.final_emissive]
            })
            .collect();
        let u = crate::engine::pathtrace::PtUniform {
            inv_view_proj: job.inv_view_proj,
            eye: job.eye,
            size: [0; 4],
            env: [v.env_rotation.to_radians(), v.bg_intensity, 0.0, v.bg_blur],
            emi_a: [emi[0], emi[1], emi[2], emi[3]],
            emi_b: [emi[4], emi[5], emi[6], emi[7]],
            amode_a: [am[0], am[1], am[2], am[3]],
            amode_b: [am[4], am[5], am[6], am[7]],
            cut_a: [cut[0], cut[1], cut[2], cut[3]],
            cut_b: [cut[4], cut[5], cut[6], cut[7]],
        };
        // About two million pixel samples a frame.
        let px = job.vp.width as u64 * job.vp.height as u64;
        let k = (2_000_000 / px.max(1)).clamp(1, 16) as u32;
        for _ in 0..k.min(job.samples - job.pt.frame) {
            job.pt.render(&self.engine.device, enc, u, &slots, &self.engine.env.view, job.vp.hdr(), &job.gpu);
        }
        true
    }

    /// After the frame's submit: a finished render is denoised, post
    /// processed like the view and saved.
    pub(super) fn studio_finish(&mut self) {
        let Some(job) = &self.studio_job else { return };
        if job.cancel {
            self.studio_job = None;
            self.studio_queue.clear();
            self.muse_light_cancelled();
            self.message("Packshot render cancelled.");
            return;
        }
        if job.pt.frame < job.samples {
            return;
        }
        let mut job = self.studio_job.take().unwrap();
        let e = &self.engine;
        let denoised = job.pt.denoise_on && job.pt.denoise_now(&e.device, &e.queue, job.vp.hdr());
        let v = &self.view;
        let post = crate::engine::viewport::PostUniform {
            a: [2f32.powf(v.exposure), v.tonemap as f32, v.bloom, v.bloom_threshold],
            b: [v.vignette, v.saturation, v.contrast, 0.0],
            texel: [0.0; 4],
        };
        let frame = crate::engine::viewport::Frame {
            camera: bytemuck::Zeroable::zeroed(),
            post,
            mesh: None,
            sets: Vec::new(),
            shadows: false,
            wireframe: false,
            wire_only: None,
            selection: None,
            path_traced: true,
        };
        let mut enc = e.device.create_command_encoder(&Default::default());
        job.vp.render(&e.device, &e.queue, &mut enc, &frame);
        e.queue.submit(Some(enc.finish()));
        let secs = job.started.elapsed().as_secs_f32();
        let saved = crate::selftest::screenshot(e, job.vp.output(), &job.path);
        if saved.is_ok() && self.muse_light_saved(&job.path) {
            return;
        }
        self.muse_light_cancelled();
        match saved {
            Ok(()) => self.message(format!("Packshot saved to {} ({:.0} s{}).", job.path.display(), secs, if denoised { ", denoised" } else { "" })),
            Err(err) => self.message(format!("Could not save the packshot: {err:#}")),
        }
        self.next_studio_shot();
    }
}

/// A new kind for `l`, with that kind's usual size and edges.
fn set_kind(l: &mut StudioLight, k: LightKind) {
    let (w, h, f) = k.shape();
    l.kind = k;
    (l.width, l.height, l.feather) = (w, h, f);
    if k == LightKind::Card {
        l.power = l.power.min(0.9);
    }
}

/// One light of the kit: on, name and power, and its type, color,
/// placement, shape and profile under a fold. True: remove it.
fn light_row(ui: &mut egui::Ui, i: usize, l: &mut StudioLight, removable: bool) -> bool {
    let id = ui.make_persistent_id(("studio light", i));
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    let open = state.is_open();
    let card = l.kind == LightKind::Card;
    let power_label = if card { "Bounce" } else { "Power" };
    let power_tip = if card { "How much of the lights reaching the card it bounces back" } else { "The light on the product (1: a white surface facing it shows white)" };
    ui.horizontal(|ui| {
        ui.checkbox(&mut l.on, "");
        let icon = if open { super::icons::Icon::Chevron } else { super::icons::Icon::Arrow };
        if super::icons::button(ui, icon, false, 18.0, "Type, color, placement, shape and soft edges").clicked() {
            state.toggle(ui);
        }
        ui.add(egui::TextEdit::singleline(&mut l.name).desired_width(120.0));
        ui.label(RichText::new(l.kind.name()).size(11.0).color(crate::style::TEXT3));
    });
    state.store(ui.ctx());
    if !open {
        if l.kind != LightKind::Flag {
            ui.horizontal(|ui| {
                ui.add_space(26.0);
                ui.label(power_label).on_hover_text(power_tip);
                super::widgets::slide(ui, &mut l.power, 0.0..=if card { 1.0 } else { 4.0 }, false, "");
            });
        }
        return false;
    }
    egui::Grid::new(("studio light grid", i)).num_columns(2).show(ui, |ui| {
        ui.label("Type");
        let mut kind = l.kind;
        egui::ComboBox::from_id_salt(("studio kind", i)).width(170.0).selected_text(l.kind.name()).show_ui(ui, |ui| {
            for k in LightKind::ALL {
                ui.selectable_value(&mut kind, k, k.name());
            }
        });
        if kind != l.kind {
            set_kind(l, kind);
        }
        ui.end_row();
        if l.kind != LightKind::Flag {
            ui.label("Color");
            ui.vertical(|ui| {
                ui.set_width(170.0);
                super::widgets::color_bar_named(ui, ("studio light color", i), if card { "Card color" } else { "Light color" }, &mut l.color);
            });
            ui.end_row();
            ui.label(power_label).on_hover_text(power_tip);
            super::widgets::slide(ui, &mut l.power, 0.0..=if card { 1.0 } else { 4.0 }, false, "");
            ui.end_row();
        }
        ui.label("Azimuth");
        super::widgets::slide_dec(ui, &mut l.azimuth, -180.0..=180.0, 0, " deg");
        ui.end_row();
        ui.label("Elevation");
        super::widgets::slide_dec(ui, &mut l.elevation, -60.0..=90.0, 0, " deg");
        ui.end_row();
        ui.label("Distance");
        super::widgets::slide(ui, &mut l.distance, 1.0..=12.0, false, " x size");
        ui.end_row();
        ui.label(if matches!(l.kind, LightKind::Octabox | LightKind::Spot) { "Diameter" } else { "Width" });
        super::widgets::slide(ui, &mut l.width, 0.1..=6.0, false, " x size");
        ui.end_row();
        if !matches!(l.kind, LightKind::Octabox | LightKind::Spot) {
            ui.label("Height");
            super::widgets::slide(ui, &mut l.height, 0.1..=6.0, false, " x size");
            ui.end_row();
        }
        if l.kind.emits() {
            ui.label("Soft edges").on_hover_text("0: even to the edges, crisp highlights; 1: bright in the middle fading out, graded highlights");
            super::widgets::slide(ui, &mut l.feather, 0.0..=1.0, false, "");
            ui.end_row();
            ui.label("Top to bottom").on_hover_text("Brighter at the top (+) or at the bottom (-): a graded highlight down the product");
            super::widgets::slide(ui, &mut l.gradient, -1.0..=1.0, false, "");
            ui.end_row();
            ui.label("Grid (0: none)").on_hover_text("A honeycomb grid narrows the beam: less light on the background");
            super::widgets::slide_dec(ui, &mut l.grid, 0.0..=90.0, 0, " deg");
            ui.end_row();
        }
    });
    removable && ui.small_button("Remove this light").clicked()
}
