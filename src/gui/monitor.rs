//! Monitor tab: snapshot before, install, snapshot after, review, save.

use super::app::{App, Confirm};
use super::logs::changes_ui;
use super::theme;
use super::widgets::*;
use super::Job;
use egui::RichText;
use expunge_core::i18n::{t, tf};
use expunge_core::model::{human_size, InstallLog};
use expunge_core::platform::{self, ChildProcess};
use expunge_core::snapshot::{detect_program, diff, filter_changes, take_snapshot};
use expunge_core::store::{PendingSession, Store};

#[derive(PartialEq, Eq, Default, Clone, Copy, Debug)]
pub enum Phase {
    #[default]
    Idle,
    Before,
    Installing,
    After,
    Review,
}

#[derive(Default)]
pub struct MonitorState {
    pub phase: Phase,
    pub name: String,
    pub installer: String,
    pub started: String,
    pub before_job: Option<Job<Result<(), String>>>,
    pub after_job: Option<Job<Result<InstallLog, String>>>,
    pub child: Option<ChildProcess>,
    pub review: Option<InstallLog>,
    /// Saving the reviewed log (large logs take a moment to write).
    pub save_job: Option<Job<Result<InstallLog, (InstallLog, String)>>>,
    pub error: String,
}

impl MonitorState {
    pub fn is_active(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// A monitoring session survives restarts (installers that need a reboot).
    pub fn resume_pending(&mut self, store: &Store) {
        if let Some(p) = store.pending() {
            self.phase = Phase::Installing;
            self.name = p.name;
            self.installer = p.installer;
            self.started = p.started;
        }
    }

    pub fn cancel(&mut self, store: &Store) {
        if let Some(j) = &self.before_job {
            j.cancel();
        }
        if let Some(j) = &self.after_job {
            j.cancel();
        }
        store.clear_pending();
        *self = MonitorState::default();
    }
}

impl App {
    pub fn start_install_and_monitor(&mut self, ctx: &egui::Context) {
        if self.monitor.is_active() {
            return;
        }
        let Some(path) = pick_file(
            &t("Choose an installer"),
            &t("Installers"),
            &["exe", "msi", "msix", "appx", "bat", "cmd"],
        ) else {
            return;
        };
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        self.begin_monitoring(ctx, name, path.to_string_lossy().to_string());
    }

    pub fn begin_monitoring(&mut self, ctx: &egui::Context, name: String, installer: String) {
        let reg = self.reg.clone();
        let settings = self.settings.clone();
        let root = self.store.root.clone();
        let session = PendingSession {
            name: name.clone(),
            started: expunge_core::util::now_string(),
            installer: installer.clone(),
        };
        self.monitor = MonitorState {
            phase: Phase::Before,
            name,
            installer,
            started: session.started.clone(),
            ..Default::default()
        };
        self.monitor.before_job = Some(Job::spawn(ctx, move |pr| {
            let snap = take_snapshot(reg.as_ref(), &settings, pr).map_err(|e| e.to_string())?;
            let store = Store::at(&root).map_err(|e| e.to_string())?;
            store
                .save_pending(&session, &snap)
                .map_err(|e| e.to_string())
        }));
    }

    fn finish_monitoring(&mut self, ctx: &egui::Context) {
        let reg = self.reg.clone();
        let settings = self.settings.clone();
        let root = self.store.root.clone();
        let (name, installer) = (self.monitor.name.clone(), self.monitor.installer.clone());
        self.monitor.phase = Phase::After;
        self.monitor.child = None;
        self.monitor.after_job = Some(Job::spawn(ctx, move |pr| {
            let store = Store::at(&root).map_err(|e| e.to_string())?;
            pr.set_phase(t("Loading the first snapshot"));
            let before = store.pending_snapshot().map_err(|e| e.to_string())?;
            let after = take_snapshot(reg.as_ref(), &settings, pr).map_err(|e| e.to_string())?;
            pr.set_phase(t("Comparing"));
            let mut changes = diff(&before, &after);
            filter_changes(
                &mut changes,
                &settings.effective_registry_exclusions(),
                &settings.effective_file_exclusions(),
            );
            let mut log = InstallLog {
                id: expunge_core::util::new_id(),
                name,
                created: expunge_core::util::now_string(),
                installer,
                ..Default::default()
            };
            if let Some(d) = detect_program(&changes) {
                log.name = d.name;
                log.publisher = d.publisher;
                log.version = d.version;
                log.uninstall_key = d.key;
            }
            log.changes = changes;
            Ok(log)
        }));
    }

    pub fn poll_monitor(&mut self, ctx: &egui::Context) {
        if let Some(r) = self.monitor.save_job.as_ref().and_then(|j| j.poll()) {
            self.monitor.save_job = None;
            match r {
                Ok(log) => {
                    self.store.clear_pending();
                    self.set_status(tf(
                        "Saved install log for {name}",
                        &[("name", log.name.clone())],
                    ));
                    self.monitor = Default::default();
                    self.reload_logs();
                    self.logs.selected = Some(log.id);
                    self.programs.needs_refresh = true;
                }
                Err((log, e)) => {
                    self.monitor.review = Some(log);
                    self.monitor.error = e;
                }
            }
        }
        if let Some(r) = self.monitor.before_job.as_ref().and_then(|j| j.poll()) {
            self.monitor.before_job = None;
            match r {
                Ok(()) => {
                    self.monitor.phase = Phase::Installing;
                    let inst = self.monitor.installer.clone();
                    if !inst.is_empty() {
                        let lower = inst.to_lowercase();
                        let cmd = if lower.ends_with(".msi") {
                            format!("msiexec.exe /i \"{inst}\"")
                        } else if lower.ends_with(".msix") || lower.ends_with(".appx") {
                            platform::shell_open(&inst);
                            String::new()
                        } else {
                            format!("\"{inst}\"")
                        };
                        if !cmd.is_empty() {
                            match platform::spawn_command_line(&cmd) {
                                Ok(c) => self.monitor.child = Some(c),
                                Err(e) => self.monitor.error = e.to_string(),
                            }
                        }
                    }
                }
                Err(e) => {
                    self.monitor.cancel(&self.store);
                    self.monitor.error = e;
                }
            }
        }
        if let Some(r) = self.monitor.after_job.as_ref().and_then(|j| j.poll()) {
            self.monitor.after_job = None;
            match r {
                Ok(log) => {
                    self.monitor.phase = Phase::Review;
                    self.monitor.review = Some(log);
                }
                Err(e) => {
                    self.monitor.phase = Phase::Installing;
                    self.monitor.error = e;
                }
            }
        }
        if self.monitor.phase == Phase::Installing && self.monitor.child.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }

    pub fn ui_monitor(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().frame(theme::content_frame()).show(ctx, |ui| {
            if !self.monitor.error.is_empty() {
                ui.colored_label(theme::BAD, &self.monitor.error);
                ui.add_space(6.0);
            }
            match self.monitor.phase {
                Phase::Idle => self.monitor_idle(ui, ctx),
                Phase::Before | Phase::After => {
                    let job_p = self
                        .monitor
                        .before_job
                        .as_ref()
                        .map(|j| j.progress.clone())
                        .or_else(|| self.monitor.after_job.as_ref().map(|j| j.progress.clone()));
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.heading(if self.monitor.phase == Phase::Before {
                            t("Taking the \"before\" snapshot…")
                        } else {
                            t("Taking the \"after\" snapshot and comparing…")
                        });
                        ui.add_space(8.0);
                        if let Some(p) = job_p {
                            job_progress(ui, &p.phase(), p.count());
                        }
                        small_muted(ui, t("Don't install anything until this finishes."));
                        ui.add_space(10.0);
                        if ui.button(t("Cancel")).clicked() {
                            self.monitor.cancel(&self.store);
                        }
                    });
                }
                Phase::Installing => {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("⏺").size(36.0).color(theme::ACCENT_TEXT));
                        ui.heading(tf("Monitoring: {name}", &[("name", self.monitor.name.clone())]));
                        small_muted(ui, tf("Started {name}", &[("name", self.monitor.started.clone())]));
                        ui.add_space(10.0);
                        let running = self.monitor.child.as_ref().is_some_and(|c| c.try_wait().is_none());
                        if running {
                            ui.horizontal(|ui| {
                                super::widgets::spinner(ui);
                                ui.label(t("The installer is running."));
                            });
                        }
                        ui.label(t("Install the program now, and run it once if you like, so first-run settings are captured too."));
                        ui.label(t("When everything is finished, click Finish. You can close Expunge or restart Windows in between; monitoring resumes."));
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            ui.add_space((ui.available_width() - 260.0).max(0.0) / 2.0);
                            if theme::primary_button(ui, format!("✔ {}", t("Finish monitoring"))).clicked() {
                                self.monitor.error.clear();
                                self.finish_monitoring(ctx);
                            }
                            if ui.button(t("Cancel")).clicked() {
                                self.ask(
                                    "Cancel monitoring?",
                                    t("The \"before\" snapshot will be discarded. Anything already installed stays installed."),
                                    "Cancel monitoring",
                                    Confirm::CancelMonitoring,
                                );
                            }
                        });
                    });
                }
                Phase::Review => self.monitor_review(ui),
            }
            if self.monitor.phase == Phase::Installing {
                ui.add_space(16.0);
                self.ui_live(ui);
            } else {
                self.live = None;
            }
        });
    }

    fn ui_live(&mut self, ui: &mut egui::Ui) {
        use expunge_core::live::{LiveKind, LiveWatcher};
        if self.live.is_none() {
            let folders: Vec<(std::path::PathBuf, bool)> = self
                .settings
                .file_roots
                .iter()
                .map(|r| {
                    (
                        std::path::PathBuf::from(expunge_core::platform::expand_env(&r.path)),
                        r.recursive,
                    )
                })
                .filter(|(p, _)| p.is_dir())
                .collect();
            let excl =
                expunge_core::util::Exclusions::new(&self.settings.effective_file_exclusions());
            self.live = Some(LiveWatcher::start(
                &folders,
                &self.settings.registry_roots,
                excl,
            ));
        }
        let Some(live) = &self.live else { return };
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
        egui::CollapsingHeader::new(RichText::new(t("Live activity")).strong())
            .default_open(self.settings.advanced_mode)
            .show(ui, |ui| {
                let (a, c, r, g) = live.counts();
                small_muted(ui, tf(
                    "Files added: {a} · changed: {c} · removed: {r} · registry areas changed: {g}",
                    &[("a", a.to_string()), ("c", c.to_string()), ("r", r.to_string()), ("g", g.to_string())],
                ));
                small_muted(ui, t("Changes appear here as they happen. The exact list, including registry values, is compared when you click Finish."));
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .stick_to_bottom(true)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for e in live.recent(300) {
                            let (sym, color) = match e.kind {
                                LiveKind::Added => ("+", theme::ADDED),
                                LiveKind::Changed | LiveKind::Renamed => ("~", theme::CHANGED),
                                LiveKind::Removed => ("−", theme::REMOVED),
                                LiveKind::Registry => ("®", theme::MUTED),
                            };
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&e.time).monospace().small().color(theme::MUTED));
                                ui.label(RichText::new(sym).monospace().color(color));
                                ui.add(egui::Label::new(RichText::new(&e.path).monospace().small()).truncate());
                            });
                        }
                    });
            });
    }

    fn monitor_idle(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.heading(RichText::new(t("Install with monitoring")).strong());
            ui.add_space(4.0);
            muted(ui, t("Expunge records every file, folder, registry key, service and scheduled task an installation adds or changes."));
            muted(ui, t("Later it can remove exactly those changes, and nothing else."));
        });
        ui.add_space(20.0);
        ui.columns(2, |cols| {
            egui::Frame::group(cols[0].style()).inner_margin(16).show(&mut cols[0], |ui| {
                ui.set_min_height(150.0);
                ui.label(RichText::new(format!("📦 {}", t("Install and monitor"))).strong().size(16.0));
                small_muted(ui, t("Choose an installer. Expunge takes a snapshot, runs it, and waits for you to finish."));
                ui.add_space(10.0);
                if theme::primary_button(ui, t("Choose installer…")).clicked() {
                    self.start_install_and_monitor(ctx);
                }
            });
            egui::Frame::group(cols[1].style()).inner_margin(16).show(&mut cols[1], |ui| {
                ui.set_min_height(150.0);
                ui.label(RichText::new(format!("📡 {}", t("Monitor manually"))).strong().size(16.0));
                small_muted(ui, t("Start monitoring, then install anything any way you like (Store, winget, a script)."));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(t("Name"));
                    ui.add(egui::TextEdit::singleline(&mut self.monitor.name).hint_text(t("e.g. Photo Editor")).desired_width(200.0));
                });
                ui.add_space(4.0);
                if ui.button(t("Start monitoring")).clicked() {
                    let name = if self.monitor.name.trim().is_empty() { t("Unnamed program") } else { self.monitor.name.trim().to_string() };
                    self.begin_monitoring(ctx, name, String::new());
                }
            });
        });
        ui.add_space(16.0);
        let roots = self.settings.registry_roots.len() + self.settings.file_roots.len();
        small_muted(
            ui,
            tf(
                "Snapshots cover {n} locations (change them in Settings).",
                &[("n", roots.to_string())],
            ),
        );
    }

    fn monitor_review(&mut self, ui: &mut egui::Ui) {
        if self.monitor.save_job.is_some() {
            job_progress(ui, &t("Saving…"), 0);
            return;
        }
        let Some(log) = &mut self.monitor.review else {
            return;
        };
        let st = log.changes.stats();
        ui.horizontal(|ui| {
            ui.heading(t("Review the installation"));
        });
        egui::Grid::new("review_meta")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label(t("Name"));
                ui.add(egui::TextEdit::singleline(&mut log.name).desired_width(320.0));
                ui.end_row();
                ui.label(t("Publisher"));
                ui.add(egui::TextEdit::singleline(&mut log.publisher).desired_width(320.0));
                ui.end_row();
                ui.label(t("Version"));
                ui.add(egui::TextEdit::singleline(&mut log.version).desired_width(160.0));
                ui.end_row();
                ui.label(t("Notes"));
                ui.add(egui::TextEdit::singleline(&mut log.notes).desired_width(320.0));
                ui.end_row();
            });
        ui.horizontal_wrapped(|ui| {
            theme::badge(
                ui,
                tf("{n} files", &[("n", st.files_added.to_string())]),
                theme::ADDED,
            );
            theme::badge(
                ui,
                tf("{n} folders", &[("n", st.dirs_added.to_string())]),
                theme::ADDED,
            );
            theme::badge(
                ui,
                tf("{n} keys", &[("n", st.keys_added.to_string())]),
                theme::ADDED,
            );
            theme::badge(
                ui,
                tf("{n} values", &[("n", st.values_added.to_string())]),
                theme::ADDED,
            );
            theme::badge(
                ui,
                tf(
                    "{n} changed",
                    &[("n", (st.values_modified + st.files_modified).to_string())],
                ),
                theme::CHANGED,
            );
            theme::badge(ui, human_size(st.bytes_added), theme::MUTED);
        });
        small_muted(ui, t("Remove anything that isn't part of this program (background noise) with ✖ before saving."));
        ui.separator();
        let h = ui.available_height() - 44.0;
        egui::ScrollArea::vertical()
            .max_height(h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                changes_ui(ui, &mut log.changes, true);
            });
        ui.separator();
        let mut save = false;
        let mut discard = false;
        ui.horizontal(|ui| {
            if theme::primary_button(ui, format!("💾 {}", t("Save install log"))).clicked() {
                save = true;
            }
            if ui.button(t("Discard")).clicked() {
                discard = true;
            }
        });
        if save && self.monitor.save_job.is_none() {
            let log = self.monitor.review.take().unwrap();
            let store = self.store.clone();
            self.monitor.save_job =
                Some(Job::spawn(&self.ctx, move |_| match store.save_log(&log) {
                    Ok(()) => Ok(log),
                    Err(e) => Err((log, e.to_string())),
                }));
        } else if discard {
            self.monitor.cancel(&self.store);
        }
    }
}
