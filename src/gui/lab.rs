//! Snapshot compare: capture the system at any moment and compare any two
//! captures.

use super::app::App;
use super::logs::changes_ui;
use super::theme;
use super::widgets::*;
use super::Job;
use egui::RichText;
use expunge_core::i18n::{t, tf};
use expunge_core::model::InstallLog;
use expunge_core::platform;
use expunge_core::snapshot::{
    detect_program, diff, filter_changes, load_snapshot, save_snapshot, take_snapshot,
};

#[derive(Default)]
pub struct LabState {
    snap_name: String,
    snap_job: Option<Job<Result<String, String>>>,
    /// Saved snapshots (name, path, date), read when needed rather than every frame.
    snaps: Option<Vec<(String, std::path::PathBuf, String)>>,
    cmp_a: Option<usize>,
    cmp_b: Option<usize>,
    cmp_job: Option<Job<Result<InstallLog, String>>>,
    cmp_result: Option<InstallLog>,
    message: String,
}

impl App {
    pub fn poll_lab(&mut self) {
        if let Some(r) = self.lab.snap_job.as_ref().and_then(|j| j.poll()) {
            self.lab.snap_job = None;
            self.lab.snaps = None;
            self.lab.message = match r {
                Ok(name) => tf("Saved snapshot {name}", &[("name", name)]),
                Err(e) => e,
            };
        }
        if let Some(r) = self.lab.cmp_job.as_ref().and_then(|j| j.poll()) {
            self.lab.cmp_job = None;
            match r {
                Ok(log) => self.lab.cmp_result = Some(log),
                Err(e) => self.lab.message = e,
            }
        }
    }

    pub fn ui_lab(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(theme::content_frame())
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.ui_lab_snapshots(ui));
            });
    }

    /// Re-read the snapshot folder next time the list is shown.
    pub fn on_lab_opened(&mut self) {
        self.lab.snaps = None;
    }

    fn ui_lab_snapshots(&mut self, ui: &mut egui::Ui) {
        if self.lab.snaps.is_none() {
            self.lab.snaps = Some(self.store.list_snapshots());
        }
        let snaps = self.lab.snaps.clone().unwrap_or_default();
        theme::card(ui, |ui| {
            ui.label(RichText::new(t("Take a named snapshot")).strong());
            small_muted(ui, t("Capture the system now, do anything, capture again, and compare any two snapshots."));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.lab.snap_name)
                        .hint_text(t("e.g. before-build-42"))
                        .desired_width(240.0),
                );
                ui.add_enabled_ui(self.lab.snap_job.is_none(), |ui| {
                    if theme::primary_button(ui, format!("📷 {}", t("Take snapshot"))).clicked() {
                        let name = if self.lab.snap_name.trim().is_empty() {
                            chrono::Local::now()
                                .format("snapshot-%Y%m%d-%H%M%S")
                                .to_string()
                        } else {
                            expunge_core::util::safe_file_name(self.lab.snap_name.trim())
                        };
                        let path = self.store.snapshots_dir().join(format!("{name}.snap"));
                        let reg = self.reg.clone();
                        let settings = self.settings.clone();
                        self.lab.snap_job = Some(Job::spawn(&self.ctx, move |pr| {
                            let s = take_snapshot(reg.as_ref(), &settings, pr)
                                .map_err(|e| e.to_string())?;
                            save_snapshot(&path, &s).map_err(|e| e.to_string())?;
                            Ok(name)
                        }));
                    }
                });
            });
            if let Some(j) = &self.lab.snap_job {
                job_progress(ui, &j.progress.phase(), j.progress.count());
            }
            if !self.lab.message.is_empty() {
                small_muted(ui, &self.lab.message);
            }
        });
        ui.add_space(8.0);
        theme::card(ui, |ui| {
            ui.label(RichText::new(t("Compare two snapshots")).strong());
            if snaps.len() < 2 {
                small_muted(ui, t("Take at least two snapshots to compare them."));
                return;
            }
            let label = |i: Option<usize>| {
                i.and_then(|i| snaps.get(i))
                    .map(|s| format!("{}  ({})", s.0, s.2))
                    .unwrap_or_else(|| t("Choose…"))
            };
            ui.horizontal(|ui| {
                ui.label(t("Before"));
                egui::ComboBox::from_id_salt("cmp_a")
                    .width(260.0)
                    .selected_text(label(self.lab.cmp_a))
                    .show_ui(ui, |ui| {
                        for (i, s) in snaps.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.lab.cmp_a,
                                Some(i),
                                format!("{}  ({})", s.0, s.2),
                            );
                        }
                    });
                ui.label(t("After"));
                egui::ComboBox::from_id_salt("cmp_b")
                    .width(260.0)
                    .selected_text(label(self.lab.cmp_b))
                    .show_ui(ui, |ui| {
                        for (i, s) in snaps.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.lab.cmp_b,
                                Some(i),
                                format!("{}  ({})", s.0, s.2),
                            );
                        }
                    });
            });
            let ready = self.lab.cmp_a.is_some()
                && self.lab.cmp_b.is_some()
                && self.lab.cmp_a != self.lab.cmp_b
                && self.lab.cmp_job.is_none();
            ui.horizontal(|ui| {
                ui.add_enabled_ui(ready, |ui| {
                    if theme::primary_button(ui, t("Compare")).clicked() {
                        let a = snaps[self.lab.cmp_a.unwrap()].clone();
                        let b = snaps[self.lab.cmp_b.unwrap()].clone();
                        let settings = self.settings.clone();
                        self.lab.cmp_result = None;
                        self.lab.cmp_job = Some(Job::spawn(&self.ctx, move |_| {
                            let before = load_snapshot(&a.1).map_err(|e| e.to_string())?;
                            let after = load_snapshot(&b.1).map_err(|e| e.to_string())?;
                            let mut changes = diff(&before, &after);
                            filter_changes(
                                &mut changes,
                                &settings.effective_registry_exclusions(),
                                &settings.effective_file_exclusions(),
                            );
                            let mut log = InstallLog {
                                id: expunge_core::util::new_id(),
                                name: format!("{} → {}", a.0, b.0),
                                created: expunge_core::util::now_string(),
                                ..Default::default()
                            };
                            if let Some(d) = detect_program(&changes) {
                                log.publisher = d.publisher;
                                log.version = d.version;
                                log.uninstall_key = d.key;
                            }
                            log.changes = changes;
                            Ok(log)
                        }));
                    }
                });
                if ui.button(t("Open snapshot folder")).clicked() {
                    platform::shell_open(&self.store.snapshots_dir().to_string_lossy());
                }
            });
            if self.lab.cmp_job.is_some() {
                job_progress(ui, &t("Comparing"), 0);
            }
        });
        if self.lab.cmp_result.is_some() {
            ui.add_space(8.0);
            let mut save = false;
            theme::card(ui, |ui| {
                let log = self.lab.cmp_result.as_mut().unwrap();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&log.name).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(t("Save as install log")).clicked() {
                            save = true;
                        }
                    });
                });
                changes_ui(ui, &mut log.changes, true);
            });
            if save {
                let log = self.lab.cmp_result.take().unwrap();
                match self.store.save_log(&log) {
                    Ok(()) => {
                        self.reload_logs();
                        self.lab.message =
                            tf("Saved install log for {name}", &[("name", log.name)]);
                    }
                    Err(e) => self.lab.message = e.to_string(),
                }
            }
        }
    }
}
