//! Backups page: everything Expunge removed, ready to be put back.

use super::app::{App, Confirm};
use super::theme;
use super::widgets::*;
use super::Job;
use egui::RichText;
use expunge_core::backup::{self, ActionResult, Backup};
use expunge_core::i18n::{t, tf};

#[derive(Default)]
pub struct BackupsState {
    pub list: Vec<Backup>,
    pub job: Option<Job<Result<Vec<ActionResult>, String>>>,
    pub results: Option<Vec<ActionResult>>,
}

impl App {
    pub fn restore_backup(&mut self, id: &str) {
        let reg = self.reg.clone();
        let root = self.store.backups_dir();
        let id = id.to_string();
        self.backups.results = None;
        self.backups.job = Some(Job::spawn(&self.ctx, move |_| {
            backup::restore_backup(&root, &id, reg.as_ref()).map_err(|e| e.to_string())
        }));
    }

    pub fn delete_backup(&mut self, id: &str) {
        if let Err(e) = backup::delete_backup(&self.store.backups_dir(), id) {
            self.set_status(e.to_string());
        }
        self.reload_backups();
    }

    pub fn poll_backups(&mut self) {
        if let Some(r) = self.backups.job.as_ref().and_then(|j| j.poll()) {
            self.backups.job = None;
            match r {
                Ok(results) => {
                    let failed = results.iter().filter(|r| !r.ok).count();
                    self.status = if failed == 0 {
                        t("Backup restored.")
                    } else {
                        tf("Finished with {n} problems.", &[("n", failed.to_string())])
                    };
                    self.backups.results = Some(results);
                    self.programs.needs_refresh = true;
                    self.startup.loaded = false;
                }
                Err(e) => self.status = e,
            }
            self.reload_backups();
        }
    }

    pub fn ui_backups(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().frame(theme::content_frame()).show(ctx, |ui| {
            if self.backups.job.is_some() {
                job_progress(ui, &t("Restoring…"), 0);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                return;
            }
            if let Some(results) = &self.backups.results {
                let mut close = false;
                theme::card(ui, |ui| {
                    ui.label(RichText::new(t("Restore finished")).strong());
                    results_ui(ui, results);
                    if ui.button(t("Close")).clicked() {
                        close = true;
                    }
                });
                if close {
                    self.backups.results = None;
                }
                ui.add_space(10.0);
            }
            let total: u64 = self.backups.list.iter().map(|b| b.bytes()).sum();
            ui.horizontal(|ui| {
                muted(ui, tf("{n} backups", &[("n", self.backups.list.len().to_string())]));
                muted(ui, format!("· {}", expunge_core::model::human_size(total)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t("Open backup folder")).clicked() {
                        expunge_core::platform::shell_open(&self.store.backups_dir().to_string_lossy());
                    }
                });
            });
            ui.add_space(6.0);
            if self.backups.list.is_empty() {
                theme::card(ui, |ui| {
                    muted(ui, t("No backups yet. Every removal Expunge makes is backed up here first."));
                });
                return;
            }
            let mut action: Option<Confirm> = None;
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for b in &self.backups.list {
                    theme::card(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&b.title).strong());
                                    if b.restored {
                                        theme::badge(ui, t("Restored"), theme::OK);
                                    }
                                });
                                small_muted(ui, format!("{}  ·  {}", b.created, b.summary()));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button(t("Delete")).clicked() {
                                    action = Some(Confirm::DeleteBackup(b.id.clone()));
                                }
                                if theme::primary_button(ui, format!("↺ {}", t("Restore"))).clicked() {
                                    action = Some(Confirm::RestoreBackup(b.id.clone()));
                                }
                            });
                        });
                    });
                    ui.add_space(6.0);
                }
            });
            match action {
                Some(Confirm::RestoreBackup(id)) => self.ask(
                    "Restore backup?",
                    t("Everything in this backup will be put back where it was."),
                    "Restore",
                    Confirm::RestoreBackup(id),
                ),
                Some(Confirm::DeleteBackup(id)) => self.ask(
                    "Delete backup?",
                    t("This backup will be deleted permanently and can no longer be restored."),
                    "Delete",
                    Confirm::DeleteBackup(id),
                ),
                _ => {}
            }
        });
    }
}
