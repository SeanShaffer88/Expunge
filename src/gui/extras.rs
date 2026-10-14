//! Programs page extras: winget updates, optional Windows features and
//! App Execution Aliases.

use super::app::{App, Confirm};
use super::theme;
use super::widgets::*;
use super::Job;
use egui::RichText;
use expunge_core::features::{self, Alias, Feature};
use expunge_core::i18n::{t, tf};
use expunge_core::programs::InstalledProgram;
use expunge_core::winget::{self, Upgrade};

#[derive(Default)]
pub struct ExtrasState {
    /// winget installed? None until checked.
    pub winget: Option<bool>,
    pub upgrades: Vec<Upgrade>,
    pub upgrade_job: Option<Job<Result<Vec<Upgrade>, String>>>,
    /// Upgrades running one after another: (name, result) per package.
    pub update_job: Option<Job<Vec<(String, Result<(), String>)>>>,
    pub update_results: Vec<(String, Result<(), String>)>,
    pub features: Option<Result<Vec<Feature>, String>>,
    pub features_job: Option<Job<Result<Vec<Feature>, String>>>,
    pub feature_job: Option<Job<Result<bool, String>>>,
    pub feature_busy: String,
    pub aliases: Option<Vec<Alias>>,
}

impl App {
    /// Ask winget which programs have updates (once per program-list load).
    pub fn check_upgrades(&mut self, ctx: &egui::Context) {
        if self.extras.upgrade_job.is_some() || self.extras.winget == Some(false) {
            return;
        }
        self.extras.upgrade_job = Some(Job::spawn(ctx, |_| {
            if !winget::available() {
                return Err("winget is not installed".into());
            }
            winget::list_upgrades().map_err(|e| e.to_string())
        }));
    }

    pub fn poll_extras(&mut self, ctx: &egui::Context) {
        if let Some(r) = self.extras.upgrade_job.as_ref().and_then(|j| j.poll()) {
            self.extras.upgrade_job = None;
            match r {
                Ok(list) => {
                    self.extras.winget = Some(true);
                    self.extras.upgrades = list;
                }
                Err(_) => self.extras.winget = Some(false),
            }
        }
        if let Some(r) = self.extras.update_job.as_ref().and_then(|j| j.poll()) {
            self.extras.update_job = None;
            let failed = r.iter().filter(|(_, r)| r.is_err()).count();
            self.set_status(if failed == 0 {
                tf("Updated {n} programs.", &[("n", r.len().to_string())])
            } else {
                tf("{n} updates failed.", &[("n", failed.to_string())])
            });
            self.extras.update_results = r;
            self.refresh_programs(ctx);
        }
        if let Some(r) = self.extras.features_job.as_ref().and_then(|j| j.poll()) {
            self.extras.features_job = None;
            self.extras.features = Some(r);
        }
        if let Some(r) = self.extras.feature_job.as_ref().and_then(|j| j.poll()) {
            self.extras.feature_job = None;
            let name = std::mem::take(&mut self.extras.feature_busy);
            match r {
                Ok(true) => self.set_status(tf(
                    "{name} changed. Restart Windows to finish.",
                    &[("name", name)],
                )),
                Ok(false) => self.set_status(tf("{name} changed.", &[("name", name)])),
                Err(e) => self.set_status(e),
            }
            self.load_features(ctx);
        }
    }

    /// The winget update for an installed program, if any.
    pub fn upgrade_for(&self, p: &InstalledProgram) -> Option<&Upgrade> {
        self.extras.upgrades.iter().find(|u| winget::matches(u, p))
    }

    /// Upgrade these packages one after another with winget.
    pub fn start_updates(&mut self, ctx: &egui::Context, list: Vec<Upgrade>) {
        if list.is_empty() || self.extras.update_job.is_some() {
            return;
        }
        self.extras.update_results.clear();
        self.extras.update_job = Some(Job::spawn(ctx, move |pr| {
            let mut out = Vec::new();
            for u in list {
                if pr.cancelled() {
                    break;
                }
                pr.set_phase(u.name.clone());
                out.push((
                    u.name.clone(),
                    winget::upgrade(&u.id).map_err(|e| e.to_string()),
                ));
            }
            out
        }));
    }

    /// Header of the Updates view: progress, results and "Update all".
    pub fn ui_updates_bar(&mut self, ui: &mut egui::Ui, visible: &[InstalledProgram]) {
        let ctx = ui.ctx().clone();
        if let Some(job) = &self.extras.update_job {
            job_progress(
                ui,
                &tf("Updating {name}…", &[("name", job.progress.phase())]),
                0,
            );
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
            return;
        }
        ui.horizontal(|ui| {
            small_muted(
                ui,
                t("Updates come from winget, the Windows Package Manager, and install silently."),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let list: Vec<Upgrade> = visible
                    .iter()
                    .filter_map(|p| self.upgrade_for(p).cloned())
                    .collect();
                if !list.is_empty()
                    && theme::primary_button(
                        ui,
                        tf("Update all ({n})", &[("n", list.len().to_string())]),
                    )
                    .clicked()
                {
                    self.start_updates(&ctx, list);
                }
            });
        });
        for (name, r) in &self.extras.update_results {
            if let Err(e) = r {
                ui.label(
                    RichText::new(format!("✖ {name}: {e}"))
                        .color(theme::BAD)
                        .small(),
                );
            }
        }
        ui.add_space(4.0);
    }

    pub fn load_features(&mut self, ctx: &egui::Context) {
        if self.extras.features_job.is_none() {
            self.extras.features_job = Some(Job::spawn(ctx, |_| {
                features::list_features().map_err(|e| e.to_string())
            }));
        }
    }

    pub fn set_feature(&mut self, name: String, enable: bool) {
        if self.extras.feature_job.is_some() {
            return;
        }
        self.extras.feature_busy = features::friendly_name(&name).unwrap_or(&name).to_string();
        self.extras.feature_job = Some(Job::spawn(&self.ctx, move |_| {
            features::set_feature(&name, enable).map_err(|e| e.to_string())
        }));
    }

    pub fn ui_features(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.extras.features.is_none() {
            self.load_features(&ctx);
        }
        small_muted(
            ui,
            t("Optional parts of Windows. Turning one off removes it from Windows (its files stay available to turn it back on). Most changes need a restart."),
        );
        if !self.elevated {
            theme::warning_banner(
                ui,
                t("Run Expunge as Administrator to turn Windows features on or off."),
            );
        }
        ui.add_space(6.0);
        if self.extras.features_job.is_some() {
            job_progress(ui, &t("Reading Windows features…"), 0);
            return;
        }
        if let Some(job) = &self.extras.feature_job {
            let _ = job;
            job_progress(
                ui,
                &tf(
                    "Changing {name}… this can take a few minutes.",
                    &[("name", self.extras.feature_busy.clone())],
                ),
                0,
            );
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
            return;
        }
        let list = match &self.extras.features {
            Some(Ok(l)) => l.clone(),
            Some(Err(e)) => {
                theme::warning_banner(ui, e.clone());
                return;
            }
            None => return,
        };
        let f = self.programs.filter.to_lowercase();
        let mut toggle: Option<(String, bool)> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for feat in list.iter().filter(|x| {
                    f.is_empty()
                        || x.name.to_lowercase().contains(&f)
                        || x.display_name().to_lowercase().contains(&f)
                }) {
                    ui.horizontal(|ui| {
                        let mut on = feat.enabled;
                        let r = ui.add_enabled(
                            self.elevated && !feat.pending(),
                            egui::Checkbox::new(&mut on, ""),
                        );
                        let label = feat.display_name().to_string();
                        r.widget_info(|| {
                            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, &label)
                        });
                        if r.changed() {
                            toggle = Some((feat.name.clone(), on));
                        }
                        ui.label(t(feat.display_name()));
                        if features::friendly_name(&feat.name).is_some() {
                            small_muted(ui, &feat.name);
                        }
                        if feat.pending() {
                            theme::badge(ui, t("Restart pending"), theme::WARN);
                        }
                    });
                    ui.add_space(2.0);
                }
            });
        if let Some((name, enable)) = toggle {
            let shown = features::friendly_name(&name).unwrap_or(&name).to_string();
            let (title, body, ok) = if enable {
                (
                    "Turn on this Windows feature?",
                    tf(
                        "{name} will be turned on. Windows may need to restart.",
                        &[("name", t(&shown))],
                    ),
                    "Turn on",
                )
            } else {
                ("Turn off this Windows feature?", tf("{name} will be turned off. Programs that rely on it may stop working until it is turned back on.", &[("name", t(&shown))]), "Turn off")
            };
            self.ask(title, body, ok, Confirm::SetFeature(name, enable));
        }
    }

    pub fn ui_aliases(&mut self, ui: &mut egui::Ui) {
        if self.extras.aliases.is_none() {
            self.extras.aliases = Some(features::list_aliases());
        }
        small_muted(ui, t("Commands that Store apps add to Windows, such as python.exe. Windows only lets you switch them on and off in Settings."));
        ui.horizontal(|ui| {
            if ui.button(t("Manage in Settings")).clicked() {
                expunge_core::platform::shell_open(features::ALIAS_SETTINGS_URI);
            }
            if ui.button(format!("⟳ {}", t("Refresh"))).clicked() {
                self.extras.aliases = None;
            }
        });
        ui.add_space(6.0);
        let list = self.extras.aliases.clone().unwrap_or_default();
        if list.is_empty() {
            muted(ui, t("No app execution aliases are switched on."));
            return;
        }
        let f = self.programs.filter.to_lowercase();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("aliases")
                    .num_columns(2)
                    .striped(true)
                    .spacing([18.0, 6.0])
                    .show(ui, |ui| {
                        for a in list.iter().filter(|a| {
                            f.is_empty()
                                || a.command.to_lowercase().contains(&f)
                                || a.package.to_lowercase().contains(&f)
                        }) {
                            ui.label(RichText::new(&a.command).monospace());
                            small_muted(
                                ui,
                                if a.package.is_empty() {
                                    t("Unknown app")
                                } else {
                                    a.package.clone()
                                },
                            );
                            ui.end_row();
                        }
                    });
            });
    }
}
