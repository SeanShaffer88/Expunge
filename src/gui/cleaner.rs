//! Cleaner page: temporary files, caches, crash dumps, Recycle Bin.

use super::app::{App, Confirm};
use super::theme;
use super::widgets::*;
use super::Job;
use egui::RichText;
use expunge_core::cleaner::{self, Category, ScanResult};
use expunge_core::i18n::{t, tf};
use expunge_core::model::human_size;

pub struct CleanerState {
    pub cats: Vec<(Category, bool, Option<ScanResult>)>,
    pub scan_job: Option<Job<Vec<ScanResult>>>,
    pub clean_job: Option<Job<Vec<ScanResult>>>,
    pub freed: Option<u64>,
}

impl Default for CleanerState {
    fn default() -> Self {
        CleanerState {
            cats: cleaner::categories()
                .into_iter()
                .map(|c| {
                    let on = c.default_on;
                    (c, on, None)
                })
                .collect(),
            scan_job: None,
            clean_job: None,
            freed: None,
        }
    }
}

fn cat_text(id: &str) -> (&'static str, &'static str) {
    match id {
        "user_temp" => (
            "Your temporary files",
            "Files programs left in your Temp folder",
        ),
        "windows_temp" => ("Windows temporary files", "The system-wide Temp folder"),
        "crash_dumps" => (
            "Crash dumps and error reports",
            "Memory dumps and Windows Error Reporting files",
        ),
        "thumbnails" => ("Thumbnail cache", "Explorer rebuilds thumbnails as needed"),
        "shader_cache" => (
            "Graphics shader caches",
            "DirectX and GPU driver shader caches",
        ),
        "update_cache" => (
            "Windows Update downloads",
            "Already-installed update packages",
        ),
        "chrome_cache" => (
            "Google Chrome cache",
            "Cached web content (not history, passwords or cookies)",
        ),
        "edge_cache" => (
            "Microsoft Edge cache",
            "Cached web content (not history, passwords or cookies)",
        ),
        "firefox_cache" => (
            "Firefox cache",
            "Cached web content (not history, passwords or cookies)",
        ),
        "recycle_bin" => (
            "Recycle Bin",
            "Permanently deletes what is in the Recycle Bin",
        ),
        _ => ("", ""),
    }
}

impl App {
    fn start_scan(&mut self) {
        let cats: Vec<Category> = self
            .cleaner
            .cats
            .iter()
            .map(|(c, _, _)| c.clone())
            .collect();
        self.cleaner.freed = None;
        self.cleaner.scan_job = Some(Job::spawn(&self.ctx, move |pr| {
            cats.iter()
                .map(|c| {
                    pr.set_phase(c.id);
                    cleaner::scan(c)
                })
                .collect()
        }));
    }

    pub fn start_clean(&mut self) {
        let cats: Vec<Category> = self
            .cleaner
            .cats
            .iter()
            .filter(|(_, on, _)| *on)
            .map(|(c, _, _)| c.clone())
            .collect();
        self.cleaner.clean_job = Some(Job::spawn(&self.ctx, move |_| {
            cats.iter().map(cleaner::clean).collect()
        }));
    }

    pub fn poll_cleaner(&mut self) {
        if let Some(r) = self.cleaner.scan_job.as_ref().and_then(|j| j.poll()) {
            self.cleaner.scan_job = None;
            for ((_, _, res), s) in self.cleaner.cats.iter_mut().zip(r) {
                *res = Some(s);
            }
        }
        if let Some(r) = self.cleaner.clean_job.as_ref().and_then(|j| j.poll()) {
            self.cleaner.clean_job = None;
            let freed: u64 = r.iter().map(|s| s.bytes).sum();
            self.cleaner.freed = Some(freed);
            self.set_status(tf("Freed {size}", &[("size", human_size(freed))]));
            self.start_scan();
        }
    }

    pub fn ui_cleaner(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().frame(theme::content_frame()).show(ctx, |ui| {
            if let Some(f) = self.cleaner.freed {
                theme::card(ui, |ui| {
                    ui.label(RichText::new(tf("✔ Freed {size}", &[("size", human_size(f))])).color(theme::OK).strong());
                    small_muted(ui, t("Files that were in use were skipped."));
                });
                ui.add_space(8.0);
            }
            let busy = self.cleaner.scan_job.is_some() || self.cleaner.clean_job.is_some();
            egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(ui.available_height() - 52.0).show(ui, |ui| {
                let elevated = self.elevated;
                for (cat, on, res) in &mut self.cleaner.cats {
                    let (title, desc) = cat_text(cat.id);
                    let locked = cat.needs_admin && !elevated;
                    egui::Frame::new().fill(theme::CARD).corner_radius(12).inner_margin(egui::Margin::symmetric(16, 10)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.add_enabled_ui(!locked, |ui| ui.checkbox(on, ""));
                            ui.vertical(|ui| {
                                ui.label(RichText::new(t(title)).strong());
                                small_muted(ui, t(desc));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if locked {
                                    theme::badge(ui, t("Needs Administrator"), theme::WARN);
                                } else if let Some(r) = res {
                                    let color = if r.bytes > 100 * 1024 * 1024 { theme::WARN } else { theme::MUTED };
                                    ui.label(RichText::new(human_size(r.bytes)).strong().color(color));
                                }
                            });
                        });
                    });
                    if locked {
                        *on = false;
                    }
                    ui.add_space(6.0);
                }
            });
            ui.add_space(6.0);
            if busy {
                job_progress(ui, &if self.cleaner.scan_job.is_some() { t("Scanning…") } else { t("Cleaning…") }, 0);
                return;
            }
            let selected: u64 = self.cleaner.cats.iter().filter(|(_, on, _)| *on).filter_map(|(_, _, r)| r.as_ref()).map(|r| r.bytes).sum();
            let scanned = self.cleaner.cats.iter().any(|(_, _, r)| r.is_some());
            ui.columns(2, |cols| {
                if theme::wide_button(&mut cols[0], format!("🔍 {}", t("Scan")), true).clicked() {
                    self.start_scan();
                }
                let any = self.cleaner.cats.iter().any(|(_, on, _)| *on);
                let label = if scanned { tf("Clean {size}", &[("size", human_size(selected))]) } else { t("Clean") };
                if theme::wide_button(&mut cols[1], format!("🗑 {label}"), any).clicked() {
                    self.ask(
                        "Clean selected items?",
                        t("Cleaned files are deleted permanently (they are temporary and regenerable, so they are not backed up)."),
                        "Clean",
                        Confirm::CleanJunk,
                    );
                }
            });
        });
    }
}
