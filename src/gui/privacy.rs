//! Anti-Telemetry page: privacy tweaks with presets and undo, pre-installed
//! app removal, secure DNS, and Windows repair commands.

use super::app::{App, Confirm};
use super::theme;
use super::widgets::*;
use super::Job;
use egui::RichText;
use expunge_core::i18n::{t, tf};
use expunge_core::programs::InstalledProgram;
use expunge_core::tweaks::{self, Status, Tier, TWEAKS};
use std::collections::BTreeSet;

#[derive(Default)]
pub struct PrivacyState {
    pub loaded: bool,
    pub status_job: Option<Job<(Vec<Status>, Option<usize>)>>,
    pub statuses: Vec<Status>,
    /// Desired state per tweak (ticked = on).
    pub wanted: Vec<bool>,
    pub dns_current: Option<usize>,
    pub dns_choice: usize,
    pub dns_job: Option<Job<Result<(), String>>>,
    pub apply_job: Option<Job<Result<Vec<String>, String>>>,
    /// Creating the restore point failed; offer to continue without one.
    pub restore_point_failed: Option<String>,
    pub notes: Vec<String>,
    pub apps: Option<Vec<InstalledProgram>>,
    pub apps_job: Option<Job<Vec<InstalledProgram>>>,
    pub apps_checked: BTreeSet<String>,
    pub deprovision: bool,
    pub bloat_job: Option<Job<Vec<(String, Result<(), String>)>>>,
    pub repair_job: Option<(usize, Job<Result<String, String>>)>,
    pub repair_results: Vec<Option<Result<String, String>>>,
}

fn tier_color(t: Tier) -> egui::Color32 {
    match t {
        Tier::Minimal => theme::OK,
        Tier::Standard => theme::WARN,
        Tier::Advanced => theme::BAD,
    }
}

impl App {
    fn tweak_visible(&self, i: usize) -> bool {
        self.settings.advanced_mode || TWEAKS[i].tier == Tier::Minimal
    }

    pub fn refresh_privacy(&mut self) {
        let reg = self.reg.clone();
        self.privacy.loaded = true;
        self.privacy.status_job = Some(Job::spawn(&self.ctx, move |_| {
            let power = tweaks::active_power_scheme();
            let st = TWEAKS
                .iter()
                .map(|t| tweaks::status(t, reg.as_ref(), &power))
                .collect();
            let dns = tweaks::current_dns()
                .and_then(|p| tweaks::DNS_PROVIDERS.iter().position(|x| x.name == p.name));
            (st, dns)
        }));
    }

    fn poll_privacy(&mut self) {
        let st = &mut self.privacy;
        if let Some((s, dns)) = st.status_job.as_ref().and_then(|j| j.poll()) {
            st.status_job = None;
            st.wanted = s.iter().map(|x| *x != Status::Off).collect();
            st.statuses = s;
            st.dns_current = dns;
            st.dns_choice = dns.unwrap_or(0);
        }
        if let Some(r) = st.apply_job.as_ref().and_then(|j| j.poll()) {
            st.apply_job = None;
            match r {
                Ok(notes) => {
                    st.restore_point_failed = None;
                    st.notes = notes;
                    self.set_status(t("Changes applied."));
                }
                Err(e) if e.starts_with("restore-point:") => {
                    st.restore_point_failed =
                        Some(e.trim_start_matches("restore-point:").to_string());
                }
                Err(e) => self.set_status(e),
            }
            self.refresh_privacy();
        }
        let st = &mut self.privacy;
        if let Some(r) = st.dns_job.as_ref().and_then(|j| j.poll()) {
            st.dns_job = None;
            match r {
                Ok(()) => self.set_status(t("DNS changed.")),
                Err(e) => self.set_status(e),
            }
            self.refresh_privacy();
        }
        let st = &mut self.privacy;
        if let Some(list) = st.apps_job.as_ref().and_then(|j| j.poll()) {
            st.apps_job = None;
            let mut v: Vec<InstalledProgram> = list
                .into_iter()
                .filter(|p| tweaks::bloat_name(&p.name).is_some())
                .collect();
            v.sort_by_key(|p| tweaks::bloat_name(&p.name).unwrap_or("").to_lowercase());
            st.apps = Some(v);
        }
        if let Some(r) = st.bloat_job.as_ref().and_then(|j| j.poll()) {
            st.bloat_job = None;
            let failed: Vec<String> = r
                .iter()
                .filter_map(|(n, r)| r.as_ref().err().map(|e| format!("{n}: {e}")))
                .collect();
            st.notes = failed.clone();
            st.apps = None;
            st.apps_checked.clear();
            self.programs.store_apps.clear();
            self.set_status(if failed.is_empty() {
                tf("Removed {n} apps.", &[("n", r.len().to_string())])
            } else {
                tf(
                    "{n} apps could not be removed.",
                    &[("n", failed.len().to_string())],
                )
            });
        }
        let st = &mut self.privacy;
        if let Some(r) = st.repair_job.as_ref().and_then(|(_, j)| j.poll()) {
            let (i, _) = st.repair_job.take().unwrap();
            if st.repair_results.len() < tweaks::REPAIRS.len() {
                st.repair_results.resize(tweaks::REPAIRS.len(), None);
            }
            st.repair_results[i] = Some(r);
        }
    }

    /// Changes between what's ticked and what's in effect.
    fn pending_changes(&self) -> Vec<(usize, bool)> {
        let st = &self.privacy;
        (0..TWEAKS.len())
            .filter(|&i| self.tweak_visible(i))
            .filter_map(|i| {
                let now_on = st.statuses.get(i).is_some_and(|s| *s != Status::Off);
                let want = *st.wanted.get(i)?;
                // "Partly" counts as on; ticking it again re-applies.
                let partly = st.statuses.get(i) == Some(&Status::Partly);
                (want != now_on || (want && partly)).then_some((i, want))
            })
            .collect()
    }

    /// Apply the ticked changes (called after confirmation).
    pub fn start_apply_tweaks(&mut self, skip_restore_point: bool) {
        let changes = self.pending_changes();
        if changes.is_empty() || self.privacy.apply_job.is_some() {
            return;
        }
        let reg = self.reg.clone();
        let state_path = self.store.root.join("tweaks-state.json");
        self.privacy.notes.clear();
        self.privacy.apply_job = Some(Job::spawn(&self.ctx, move |pr| {
            if !skip_restore_point {
                pr.set_phase(t("Creating a restore point…"));
                if let Err(e) =
                    expunge_core::updates::create_restore_point("Before Expunge Anti-Telemetry")
                {
                    return Err(format!("restore-point:{e}"));
                }
            }
            let mut state = tweaks::State::load(&state_path);
            let mut notes = Vec::new();
            for (i, on) in changes {
                let tw = &TWEAKS[i];
                pr.set_phase(t(tw.title));
                let r = if on {
                    tweaks::apply(tw, reg.as_ref(), &mut state)
                } else {
                    tweaks::undo(tw, reg.as_ref(), &mut state)
                };
                if let Err(e) = r {
                    notes.push(format!("{}: {e}", t(tw.title)));
                }
            }
            state.save(&state_path).map_err(|e| e.to_string())?;
            Ok(notes)
        }));
    }

    pub fn start_remove_bloat(&mut self) {
        let Some(apps) = &self.privacy.apps else {
            return;
        };
        let chosen: Vec<InstalledProgram> = apps
            .iter()
            .filter(|p| self.privacy.apps_checked.contains(&p.key))
            .cloned()
            .collect();
        if chosen.is_empty() {
            return;
        }
        let deprov = self.privacy.deprovision;
        self.privacy.bloat_job = Some(Job::spawn(&self.ctx, move |pr| {
            chosen
                .iter()
                .map(|p| {
                    let shown = tweaks::bloat_name(&p.name).unwrap_or(&p.name).to_string();
                    pr.set_phase(shown.clone());
                    let mut r = expunge_core::programs::remove_store_app(&p.key)
                        .map(|_| ())
                        .map_err(|e| e.to_string());
                    if r.is_ok() && deprov {
                        r = tweaks::deprovision(&p.name).map_err(|e| e.to_string());
                    }
                    (shown, r)
                })
                .collect()
        }));
    }

    pub fn ui_privacy(&mut self, ctx: &egui::Context) {
        if !self.privacy.loaded {
            self.refresh_privacy();
        }
        self.poll_privacy();
        egui::CentralPanel::default().frame(theme::content_frame()).show(ctx, |ui| {
            if self.privacy.status_job.is_some() && self.privacy.statuses.is_empty() {
                job_progress(ui, &t("Reading current settings…"), 0);
                return;
            }
            if let Some(job) = &self.privacy.apply_job {
                job_progress(ui, &tf("Applying… {name}", &[("name", job.progress.phase())]), 0);
                ctx.request_repaint_after(std::time::Duration::from_millis(300));
                return;
            }
            if !self.elevated {
                theme::warning_banner(ui, t("Most of these settings are machine-wide. Run Expunge as Administrator to change them."));
                ui.add_space(6.0);
            }
            if let Some(err) = self.privacy.restore_point_failed.clone() {
                theme::warning_banner(ui, tf("A restore point couldn't be created ({error}). System Protection may be turned off.", &[("error", err)]));
                ui.horizontal(|ui| {
                    if ui.button(t("Apply without a restore point")).clicked() {
                        self.privacy.restore_point_failed = None;
                        self.start_apply_tweaks(true);
                    }
                    if ui.button(t("Cancel")).clicked() {
                        self.privacy.restore_point_failed = None;
                    }
                });
                ui.add_space(6.0);
            }
            // Toolbar: presets and apply.
            let changes = self.pending_changes();
            ui.horizontal_wrapped(|ui| {
                ui.label(t("Presets:"));
                let presets: &[Tier] = if self.settings.advanced_mode {
                    &[Tier::Minimal, Tier::Standard, Tier::Advanced]
                } else {
                    &[Tier::Minimal]
                };
                for tier in presets {
                    if ui
                        .button(RichText::new(t(tier.label())).color(tier_color(*tier)))
                        .on_hover_text(match tier {
                            Tier::Minimal => t("Safe for everyone: nothing you rely on stops working."),
                            Tier::Standard => t("Typical choices for a daily PC; some convenience features turn off."),
                            Tier::Advanced => t("Aggressive: can break features or delay security updates."),
                        })
                        .clicked()
                    {
                        for (i, tw) in TWEAKS.iter().enumerate() {
                            if tw.tier <= *tier && self.tweak_visible(i) {
                                if let Some(w) = self.privacy.wanted.get_mut(i) {
                                    *w = true;
                                }
                            }
                        }
                    }
                }
                if ui.button(t("Windows defaults")).on_hover_text(t("Untick everything, to undo all changes.")).clicked() {
                    for i in 0..TWEAKS.len() {
                        if self.tweak_visible(i) {
                            if let Some(w) = self.privacy.wanted.get_mut(i) {
                                *w = false;
                            }
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let n = changes.len();
                    if ui
                        .add_enabled(n > 0 && self.elevated, egui::Button::new(RichText::new(tf("Apply {n} changes", &[("n", n.to_string())])).color(egui::Color32::WHITE)).fill(theme::ACCENT))
                        .clicked()
                    {
                        let on = changes.iter().filter(|(_, w)| *w).count();
                        let off = n - on;
                        self.ask(
                            "Apply these changes?",
                            tf("{on} settings will be turned on and {off} undone. A restore point is created first.", &[("on", on.to_string()), ("off", off.to_string())]),
                            "Apply",
                            Confirm::ApplyTweaks,
                        );
                    }
                    if ui.button(format!("⟳ {}", t("Refresh"))).clicked() {
                        self.refresh_privacy();
                    }
                });
            });
            for n in &self.privacy.notes {
                ui.label(RichText::new(format!("⚠ {n}")).color(theme::WARN).small());
            }
            ui.add_space(6.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for cat in tweaks::CATEGORIES {
                    let idx: Vec<usize> = (0..TWEAKS.len())
                        .filter(|&i| TWEAKS[i].category == cat && self.tweak_visible(i))
                        .collect();
                    let extra = matches!(cat, "Bloatware and AI features" | "Network" | "Performance") && self.settings.advanced_mode;
                    if idx.is_empty() && !extra {
                        continue;
                    }
                    theme::card(ui, |ui| {
                        ui.label(RichText::new(t(cat)).heading());
                        ui.add_space(4.0);
                        for i in idx {
                            self.tweak_row(ui, i);
                        }
                        if extra {
                            match cat {
                                "Bloatware and AI features" => self.ui_bloat(ui),
                                "Network" => self.ui_dns(ui),
                                "Performance" => self.ui_repairs(ui),
                                _ => {}
                            }
                        }
                    });
                    ui.add_space(8.0);
                }
                if !self.settings.advanced_mode {
                    theme::card(ui, |ui| {
                        ui.label(RichText::new(t("Windows repair")).heading());
                        self.ui_repairs(ui);
                    });
                    ui.add_space(6.0);
                    small_muted(ui, t("Advanced mode shows more settings: services, Windows Update delays, app permissions, removing pre-installed apps, secure DNS and performance tweaks."));
                }
            });
        });
    }

    fn tweak_row(&mut self, ui: &mut egui::Ui, i: usize) {
        let tw = &TWEAKS[i];
        let status = self.privacy.statuses.get(i).copied().unwrap_or(Status::Off);
        ui.horizontal(|ui| {
            let mut want = self.privacy.wanted.get(i).copied().unwrap_or(false);
            let r = ui.add_enabled(
                self.elevated || !tw.needs_admin(),
                egui::Checkbox::new(&mut want, ""),
            );
            let title = t(tw.title);
            r.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, want, &title)
            });
            if r.changed() {
                if let Some(w) = self.privacy.wanted.get_mut(i) {
                    *w = want;
                }
            }
            ui.vertical(|ui| {
                ui.set_max_width(ui.available_width() - 150.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t(tw.title)).strong());
                    theme::badge(ui, t(tw.tier.label()), tier_color(tw.tier));
                });
                small_muted(ui, t(tw.description));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (txt, c) = match status {
                    Status::On => (t("On"), theme::OK),
                    Status::Partly => (t("Partly on"), theme::WARN),
                    Status::Off => (t("Off"), theme::MUTED),
                };
                ui.label(RichText::new(txt).color(c));
            });
        });
        ui.add_space(4.0);
    }

    fn ui_bloat(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(RichText::new(t("Pre-installed apps")).strong());
        small_muted(ui, t("Remove consumer apps Windows installs by default. They can be reinstalled from the Microsoft Store."));
        ui.add_space(4.0);
        if let Some(job) = &self.privacy.bloat_job {
            job_progress(
                ui,
                &tf("Removing {name}…", &[("name", job.progress.phase())]),
                0,
            );
            return;
        }
        match &self.privacy.apps {
            None => {
                if self.privacy.apps_job.is_some() {
                    job_progress(ui, &t("Looking for pre-installed apps…"), 0);
                } else if ui
                    .button(format!("🔍 {}", t("Find pre-installed apps")))
                    .clicked()
                {
                    self.privacy.apps_job = Some(Job::spawn(&self.ctx, |_| {
                        expunge_core::programs::list_store_apps()
                    }));
                }
            }
            Some(apps) if apps.is_empty() => {
                ui.label(
                    RichText::new(format!(
                        "✔ {}",
                        t("None of the usual pre-installed apps are on this PC.")
                    ))
                    .color(theme::OK),
                );
            }
            Some(apps) => {
                let apps = apps.clone();
                ui.horizontal_wrapped(|ui| {
                    for p in &apps {
                        let mut c = self.privacy.apps_checked.contains(&p.key);
                        if ui
                            .checkbox(&mut c, tweaks::bloat_name(&p.name).unwrap_or(&p.name))
                            .changed()
                        {
                            if c {
                                self.privacy.apps_checked.insert(p.key.clone());
                            } else {
                                self.privacy.apps_checked.remove(&p.key);
                            }
                        }
                    }
                });
                ui.checkbox(
                    &mut self.privacy.deprovision,
                    t("Also stop them being installed for new user accounts"),
                );
                let n = self.privacy.apps_checked.len();
                if ui
                    .add_enabled(
                        n > 0,
                        egui::Button::new(tf("Remove {n} apps", &[("n", n.to_string())])),
                    )
                    .clicked()
                {
                    self.ask(
                        "Remove these apps?",
                        t("The selected apps are removed for your account. They can be reinstalled from the Microsoft Store."),
                        "Remove",
                        Confirm::RemoveBloat,
                    );
                }
            }
        }
    }

    fn ui_dns(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(RichText::new(t("Secure DNS")).strong());
        small_muted(ui, t("Choose who looks up website addresses for this PC. Applies to every connected network adapter."));
        ui.add_space(4.0);
        if self.privacy.dns_job.is_some() {
            job_progress(ui, &t("Changing DNS…"), 0);
            return;
        }
        ui.horizontal(|ui| {
            let providers = tweaks::DNS_PROVIDERS;
            let cur = self.privacy.dns_choice.min(providers.len() - 1);
            egui::ComboBox::from_id_salt("dns")
                .selected_text(t(providers[cur].name))
                .show_ui(ui, |ui| {
                    for (i, p) in providers.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.privacy.dns_choice,
                            i,
                            format!("{} — {}", t(p.name), t(p.description)),
                        );
                    }
                });
            let changed = Some(self.privacy.dns_choice) != self.privacy.dns_current;
            if ui
                .add_enabled(
                    changed && self.elevated,
                    egui::Button::new(t("Use this DNS")),
                )
                .clicked()
            {
                let p = providers[self.privacy.dns_choice];
                self.privacy.dns_job = Some(Job::spawn(&self.ctx, move |_| {
                    tweaks::set_dns(&p).map_err(|e| e.to_string())
                }));
            }
            if let Some(c) = self.privacy.dns_current {
                small_muted(ui, tf("Now: {name}", &[("name", t(providers[c].name))]));
            }
        });
    }

    fn ui_repairs(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(RichText::new(t("Repair tools")).strong());
        let busy = self.privacy.repair_job.is_some();
        for (i, r) in tweaks::REPAIRS.iter().enumerate() {
            if r.advanced && !self.settings.advanced_mode {
                continue;
            }
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(ui.available_width() - 150.0);
                    ui.label(t(r.title));
                    small_muted(ui, t(r.description));
                    match self.privacy.repair_results.get(i).and_then(|x| x.as_ref()) {
                        Some(Ok(line)) if !line.is_empty() => {
                            ui.label(RichText::new(format!("✔ {line}")).color(theme::OK).small());
                        }
                        Some(Ok(_)) => {
                            ui.label(
                                RichText::new(format!("✔ {}", t("Done")))
                                    .color(theme::OK)
                                    .small(),
                            );
                        }
                        Some(Err(e)) => {
                            ui.label(RichText::new(format!("✖ {e}")).color(theme::BAD).small());
                        }
                        None => {}
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some((j, job)) = &self.privacy.repair_job {
                        if *j == i {
                            super::widgets::spinner(ui);
                            ui.ctx()
                                .request_repaint_after(std::time::Duration::from_millis(500));
                            let _ = job;
                            return;
                        }
                    }
                    if ui
                        .add_enabled(!busy && self.elevated, egui::Button::new(t("Run")))
                        .clicked()
                    {
                        let job = Job::spawn(&self.ctx, move |pr| {
                            tweaks::run_repair(&tweaks::REPAIRS[i], &|s| pr.set_phase(s))
                                .map_err(|e| e.to_string())
                        });
                        self.privacy.repair_job = Some((i, job));
                    }
                });
            });
            ui.add_space(4.0);
        }
    }
}
