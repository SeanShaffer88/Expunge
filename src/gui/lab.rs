//! Installer Lab: tools for developers and power users — installer lint,
//! uninstaller testing, and arbitrary snapshot comparison.

use super::app::App;
use super::logs::changes_ui;
use super::plan::RemovalFlow;
use super::theme;
use super::widgets::*;
use super::Job;
use egui::RichText;
use expunge_core::i18n::{t, tf};
use expunge_core::lint::{self, Finding, Severity};
use expunge_core::model::InstallLog;
use expunge_core::platform::{self, ChildProcess};
use expunge_core::revert::{residue, residue_markdown, Plan};
use expunge_core::snapshot::{
    detect_program, diff, filter_changes, load_snapshot, save_snapshot, take_snapshot,
};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum LabSub {
    #[default]
    Lint,
    UninstallTest,
    Snapshots,
}

#[derive(Default)]
pub struct LabState {
    pub sub: LabSub,
    pub log_id: Option<String>,
    findings: Option<(String, Vec<Finding>)>,
    // Uninstaller test
    test_quiet: bool,
    test_skip_run: bool,
    test_child: Option<ChildProcess>,
    test_running: bool,
    test_error: String,
    test_job: Option<Job<Plan>>,
    test_result: Option<(String, Plan)>,
    // Snapshots
    snap_name: String,
    snap_job: Option<Job<Result<String, String>>>,
    cmp_a: Option<usize>,
    cmp_b: Option<usize>,
    cmp_job: Option<Job<Result<InstallLog, String>>>,
    cmp_result: Option<InstallLog>,
    message: String,
}

fn severity_color(s: Severity) -> egui::Color32 {
    match s {
        Severity::Error => theme::BAD,
        Severity::Warning => theme::WARN,
        Severity::Info => theme::MUTED,
    }
}

impl App {
    fn lab_log_picker(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(t("Install log"));
            let current = self
                .lab
                .log_id
                .as_ref()
                .and_then(|id| self.logs.logs.iter().find(|l| &l.id == id))
                .map(|l| format!("{}  ({})", l.name, l.created))
                .unwrap_or_else(|| t("Choose…"));
            egui::ComboBox::from_id_salt("lab_log")
                .width(380.0)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for l in &self.logs.logs {
                        if ui
                            .selectable_label(
                                self.lab.log_id.as_deref() == Some(&l.id),
                                format!("{}  ({})", l.name, l.created),
                            )
                            .clicked()
                        {
                            self.lab.log_id = Some(l.id.clone());
                            self.lab.findings = None;
                            self.lab.test_result = None;
                        }
                    }
                });
        });
        if self.logs.logs.is_empty() {
            small_muted(
                ui,
                t("No monitored installations yet. Use Monitor to record one."),
            );
        }
    }

    fn lab_log(&self) -> Option<InstallLog> {
        let id = self.lab.log_id.as_ref()?;
        self.logs.logs.iter().find(|l| &l.id == id).cloned()
    }

    pub fn poll_lab(&mut self) {
        if let Some(plan) = self.lab.test_job.as_ref().and_then(|j| j.poll()) {
            self.lab.test_job = None;
            let name = self.lab_log().map(|l| l.name).unwrap_or_default();
            self.lab.test_result = Some((name, plan));
        }
        if let Some(r) = self.lab.snap_job.as_ref().and_then(|j| j.poll()) {
            self.lab.snap_job = None;
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
        // Uninstaller test: wait for the uninstaller, then check residue.
        if self.lab.test_running {
            let log = self.lab_log();
            let done = match (&self.lab.test_child, &log) {
                (Some(c), Some(l)) => {
                    c.try_wait().is_some()
                        && (l.uninstall_key.is_empty() || !self.reg.exists(&l.uninstall_key))
                }
                _ => true,
            };
            if done {
                self.start_residue_check();
            } else {
                self.ctx
                    .request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
    }

    fn start_residue_check(&mut self) {
        self.lab.test_running = false;
        self.lab.test_child = None;
        let Some(log) = self.lab_log() else { return };
        let reg = self.reg.clone();
        self.lab.test_job = Some(Job::spawn(&self.ctx, move |_| {
            residue(&log.changes, reg.as_ref())
        }));
    }

    fn start_uninstaller_test(&mut self) {
        self.lab.test_error.clear();
        self.lab.test_result = None;
        let Some(log) = self.lab_log() else { return };
        if self.lab.test_skip_run {
            self.start_residue_check();
            return;
        }
        let key = self.reg.read_key(&log.uninstall_key);
        let cmd = key.as_ref().and_then(|k| {
            let quiet = k.get_string("QuietUninstallString");
            if self.lab.test_quiet && quiet.is_some() {
                quiet
            } else {
                k.get_string("UninstallString")
            }
        });
        let Some(cmd) = cmd else {
            self.lab.test_error = t("This program has no uninstaller registered (or it is already uninstalled). Use \"Only check what is left\".");
            return;
        };
        match platform::spawn_command_line(&platform::expand_env(&cmd)) {
            Ok(c) => {
                self.lab.test_child = Some(c);
                self.lab.test_running = true;
            }
            Err(e) => self.lab.test_error = e.to_string(),
        }
    }

    pub fn ui_lab(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(theme::content_frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (s, label) in [
                        (LabSub::Lint, t("Installer lint")),
                        (LabSub::UninstallTest, t("Uninstaller test")),
                        (LabSub::Snapshots, t("Snapshot compare")),
                    ] {
                        if ui.selectable_label(self.lab.sub == s, label).clicked() {
                            self.lab.sub = s;
                        }
                    }
                });
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.lab.sub {
                        LabSub::Lint => self.ui_lab_lint(ui),
                        LabSub::UninstallTest => self.ui_lab_test(ui),
                        LabSub::Snapshots => self.ui_lab_snapshots(ui),
                    });
            });
    }

    fn ui_lab_lint(&mut self, ui: &mut egui::Ui) {
        theme::card(ui, |ui| {
            ui.label(RichText::new(t("Check an installer for packaging problems")).strong());
            small_muted(ui, t("Monitor an installation first, then pick its log. Expunge reviews what the installer did and flags anything that commonly causes trouble."));
            ui.add_space(6.0);
            self.lab_log_picker(ui);
        });
        ui.add_space(8.0);
        let Some(log) = self.lab_log() else { return };
        if self
            .lab
            .findings
            .as_ref()
            .is_none_or(|(id, _)| *id != log.id)
        {
            self.lab.findings = Some((log.id.clone(), lint::lint(&log)));
        }
        let findings = self
            .lab
            .findings
            .as_ref()
            .map(|(_, f)| f.clone())
            .unwrap_or_default();
        let (e, w, i) = lint::counts(&findings);
        ui.horizontal(|ui| {
            theme::badge(
                ui,
                tf("{n} errors", &[("n", e.to_string())]),
                if e > 0 { theme::BAD } else { theme::MUTED },
            );
            theme::badge(
                ui,
                tf("{n} warnings", &[("n", w.to_string())]),
                if w > 0 { theme::WARN } else { theme::MUTED },
            );
            theme::badge(ui, tf("{n} notes", &[("n", i.to_string())]), theme::MUTED);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t("Export report (Markdown)")).clicked() {
                    let name = format!("{}-lint.md", expunge_core::util::safe_file_name(&log.name));
                    if let Some(p) = save_file(&t("Export report"), &name, "Markdown", "md") {
                        let _ = std::fs::write(&p, lint::to_markdown(&log, &findings));
                        self.set_status(tf("Saved {path}", &[("path", p.display().to_string())]));
                    }
                }
            });
        });
        ui.add_space(6.0);
        if findings.is_empty() {
            theme::card(ui, |ui| {
                ui.label(
                    RichText::new(format!(
                        "✔ {}",
                        t("No problems found. This installer is tidy.")
                    ))
                    .color(theme::OK)
                    .strong(),
                );
            });
        }
        for f in &findings {
            theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    theme::badge(ui, t(f.severity.label()), severity_color(f.severity));
                    ui.label(RichText::new(t(f.title)).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        small_muted(ui, f.code);
                    });
                });
                small_muted(ui, t(f.advice));
                if !f.items.is_empty() {
                    egui::CollapsingHeader::new(tf(
                        "{n} items",
                        &[("n", f.items.len().to_string())],
                    ))
                    .id_salt(("lint", f.code))
                    .default_open(f.items.len() <= 5)
                    .show(ui, |ui| {
                        for it in f.items.iter().take(200) {
                            ui.add(egui::Label::new(RichText::new(it).monospace().small()).wrap());
                        }
                    });
                }
            });
            ui.add_space(6.0);
        }
    }

    fn ui_lab_test(&mut self, ui: &mut egui::Ui) {
        theme::card(ui, |ui| {
            ui.label(RichText::new(t("Test a program's own uninstaller")).strong());
            small_muted(ui, t("Expunge runs the program's uninstaller, then compares the system with the install log and lists everything that was left behind or not restored."));
            ui.add_space(6.0);
            self.lab_log_picker(ui);
            ui.checkbox(
                &mut self.lab.test_quiet,
                t("Use the silent uninstall command if available"),
            );
            ui.checkbox(
                &mut self.lab.test_skip_run,
                t("Only check what is left (I already uninstalled it)"),
            );
            ui.add_space(6.0);
            let busy = self.lab.test_running || self.lab.test_job.is_some();
            ui.horizontal(|ui| {
                ui.add_enabled_ui(self.lab.log_id.is_some() && !busy, |ui| {
                    let label = if self.lab.test_skip_run {
                        t("Check now")
                    } else {
                        t("Run uninstaller and check")
                    };
                    if theme::primary_button(ui, label).clicked() {
                        self.start_uninstaller_test();
                    }
                });
                if self.lab.test_running && ui.button(t("It's finished, continue")).clicked() {
                    self.start_residue_check();
                }
            });
            if self.lab.test_running {
                job_progress(ui, &t("Waiting for the uninstaller to finish…"), 0);
            }
            if self.lab.test_job.is_some() {
                job_progress(ui, &t("Checking what is left…"), 0);
            }
            if !self.lab.test_error.is_empty() {
                ui.colored_label(theme::BAD, &self.lab.test_error);
            }
        });
        ui.add_space(8.0);
        let Some((name, plan)) = self.lab.test_result.clone() else {
            return;
        };
        theme::card(ui, |ui| {
            if plan.items.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "✔ {}",
                        t("Clean. The uninstaller removed everything the installation added.")
                    ))
                    .color(theme::OK)
                    .strong(),
                );
            } else {
                ui.label(
                    RichText::new(tf(
                        "{n} items were left behind or not restored.",
                        &[("n", plan.items.len().to_string())],
                    ))
                    .color(theme::WARN)
                    .strong(),
                );
                ui.add_space(4.0);
                for i in plan.items.iter().take(300) {
                    ui.horizontal(|ui| {
                        theme::badge(ui, t(i.action.category()), theme::MUTED);
                        ui.add(
                            egui::Label::new(RichText::new(i.action.describe()).small()).truncate(),
                        );
                    });
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(t("Export report (Markdown)")).clicked() {
                    let fname = format!(
                        "{}-uninstall-test.md",
                        expunge_core::util::safe_file_name(&name)
                    );
                    if let Some(p) = save_file(&t("Export report"), &fname, "Markdown", "md") {
                        let _ = std::fs::write(&p, residue_markdown(&name, &plan));
                        self.set_status(tf("Saved {path}", &[("path", p.display().to_string())]));
                    }
                }
                if !plan.items.is_empty()
                    && theme::primary_button(ui, t("Clean up what was left")).clicked()
                {
                    let mut flow = RemovalFlow::new(
                        tf("Leftovers: {name}", &[("name", name.clone())]),
                        plan.clone(),
                        true,
                        self.settings.remove_unlogged_enabled(),
                    );
                    flow.names = vec![name.clone()];
                    if let Some(id) = &self.lab.log_id {
                        flow.logs_to_delete = vec![id.clone()];
                    }
                    self.open_removal(flow);
                }
            });
        });
    }

    fn ui_lab_snapshots(&mut self, ui: &mut egui::Ui) {
        let snaps = self.store.list_snapshots();
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
