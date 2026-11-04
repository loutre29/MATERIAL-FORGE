//! Autosave: a modified project is saved every few minutes, as recovery
//! copies in %APPDATA%\SP_RETRO\autosave (the project file untouched) or
//! into the project file itself. Its settings (Preferences > Autosave) are
//! kept in `autosave.json` next to the library.

use super::RetroApp;
use crate::project;
use eframe::egui;
use egui::RichText;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Target {
    /// Numbered copies in the autosave folder.
    Copies,
    /// The project's own file (a project never saved gets copies).
    Project,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Autosave {
    pub on: bool,
    /// Minutes between saves of a modified project.
    pub minutes: u32,
    pub target: Target,
    /// Recovery copies kept per project (the newest is `<name>_autosave`).
    pub copies: u32,
}

impl Default for Autosave {
    fn default() -> Self {
        Autosave { on: true, minutes: 10, target: Target::Copies, copies: 3 }
    }
}

fn path() -> PathBuf {
    crate::library::library_dir().join("..").join("autosave.json")
}

/// The recovery copies' folder.
pub fn folder() -> PathBuf {
    crate::library::library_dir().parent().map(|p| p.join("autosave")).unwrap_or_else(|| PathBuf::from("autosave"))
}

impl Autosave {
    pub fn load() -> Self {
        std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let p = path();
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, s);
        }
    }
}

/// `<name>_autosave.sprp` (the newest), then `_2`, `_3`... (older).
fn copy_path(name: &str, k: u32) -> PathBuf {
    folder().join(if k <= 1 { format!("{name}_autosave.sprp") } else { format!("{name}_autosave_{k}.sprp") })
}

impl RetroApp {
    /// Saves a modified project once its interval has passed (not during a
    /// stroke, bake, save or studio render), and wakes the app for it.
    pub(super) fn autosave(&mut self, ctx: &egui::Context) {
        let a = &self.autosave;
        if !a.on || !self.doc.as_ref().is_some_and(|d| d.modified) {
            self.last_autosave = Instant::now();
            return;
        }
        let every = Duration::from_secs(a.minutes.max(1) as u64 * 60);
        let gone = self.last_autosave.elapsed();
        if gone < every {
            ctx.request_repaint_after(every - gone);
            return;
        }
        if self.stroke.is_some() || self.bake_job.is_some() || self.pending_save.is_some() || self.studio_job.is_some() {
            return;
        }
        self.last_autosave = Instant::now();
        let doc = self.doc.as_ref().unwrap();
        let t = Instant::now();
        let into_project = a.target == Target::Project && doc.path.is_some();
        let dest = if into_project {
            doc.path.clone().unwrap()
        } else {
            let name: String = doc.mesh_name.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
            let _ = std::fs::create_dir_all(folder());
            let keep = a.copies.max(1);
            let _ = std::fs::remove_file(copy_path(&name, keep));
            for k in (1..keep).rev() {
                let _ = std::fs::rename(copy_path(&name, k), copy_path(&name, k + 1));
            }
            copy_path(&name, 1)
        };
        match project::save(&self.engine, doc, &self.camera, &dest) {
            Ok(()) => {
                if into_project {
                    if let Some(d) = &mut self.doc {
                        d.modified = false;
                    }
                }
                self.status = format!("Autosaved to {} ({:.1} s)", dest.display(), t.elapsed().as_secs_f32());
            }
            Err(e) => self.message(format!("Autosave failed: {e:#}")),
        }
    }

    /// Preferences > Autosave.
    pub(super) fn autosave_prefs(&mut self, ui: &mut egui::Ui) {
        let before = self.autosave.clone();
        let a = &mut self.autosave;
        ui.label(RichText::new("Autosave").strong());
        ui.checkbox(&mut a.on, "Save a modified project automatically");
        ui.add_enabled_ui(a.on, |ui| {
            egui::Grid::new("autosave grid").num_columns(2).show(ui, |ui| {
                ui.label("Every (minutes)");
                super::widgets::slide(ui, &mut a.minutes, 1..=60, false, "");
                ui.end_row();
                ui.label("Save into");
                let names = ["Recovery copies (the project file is untouched)", "The project file"];
                let cur = a.target as usize;
                egui::ComboBox::from_id_salt("autosave target").width(300.0).selected_text(names[cur]).show_ui(ui, |ui| {
                    ui.selectable_value(&mut a.target, Target::Copies, names[0]);
                    ui.selectable_value(&mut a.target, Target::Project, names[1]);
                });
                ui.end_row();
                if a.target == Target::Copies {
                    ui.label("Copies kept per project");
                    super::widgets::slide(ui, &mut a.copies, 1..=20, false, "");
                    ui.end_row();
                }
            });
            if a.target == Target::Project {
                ui.label(RichText::new("A project never saved gets recovery copies until it has a file.").size(11.5).color(crate::style::TEXT3));
            }
            if ui.button("Open the recovery folder").clicked() {
                let _ = std::fs::create_dir_all(folder());
                let _ = std::process::Command::new("explorer").arg(folder()).spawn();
            }
        });
        if self.autosave != before {
            self.autosave.save();
        }
    }
}
