//! Dashboard: status at a glance and quick actions.

use super::app::{App, Tab};
use super::theme;
use super::widgets::*;
use egui::RichText;
use expunge_core::i18n::{t, tf};
use expunge_core::model::human_size;

fn stat_tile(
    ui: &mut egui::Ui,
    icon: &str,
    value: String,
    label: String,
    color: egui::Color32,
) -> egui::Response {
    let resp = egui::Frame::new()
        .fill(theme::CARD)
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).size(26.0).color(color));
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(&value).size(22.0).strong());
                    small_muted(ui, &label);
                });
            });
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    super::a11y::button(&resp, &format!("{value} {label}"));
    resp
}

fn action_row(ui: &mut egui::Ui, icon: &str, title: &str, desc: &str) -> bool {
    let resp = egui::Frame::new()
        .fill(theme::CARD)
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).size(20.0).color(theme::ACCENT_TEXT));
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(t(title)).strong());
                    small_muted(ui, t(desc));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("›").size(20.0).color(theme::MUTED));
                });
            });
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    super::a11y::button(&resp, &format!("{}. {}", t(title), t(desc)));
    resp.clicked()
}

impl App {
    pub fn ui_dashboard(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().frame(theme::content_frame()).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if !self.elevated {
                    theme::warning_banner(ui, t("Expunge is not running as Administrator. Some removals will fail. Use \"Run as Administrator\" in the left column."));
                    ui.add_space(10.0);
                }
                if self.monitor.is_active() {
                    theme::card(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("⏺").size(22.0).color(theme::ACCENT_TEXT));
                            ui.vertical(|ui| {
                                ui.label(RichText::new(tf("Monitoring: {name}", &[("name", self.monitor.name.clone())])).strong());
                                small_muted(ui, t("Finish monitoring when the installation is complete."));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if theme::primary_button(ui, t("Go to Monitor")).clicked() {
                                    self.tab = Tab::Monitor;
                                }
                            });
                        });
                    });
                    ui.add_space(10.0);
                }

                let programs = self.programs.list.len();
                let monitored = self.monitored_keys().len();
                let broken = self.programs.broken.len();
                let backup_bytes: u64 = self.backups.list.iter().map(|b| b.bytes()).sum();
                let mut go: Option<(Tab, Option<super::programs::View>)> = None;
                ui.columns(4, |cols| {
                    if stat_tile(&mut cols[0], "📦", programs.to_string(), t("Programs installed"), theme::ACCENT_TEXT).clicked() {
                        go = Some((Tab::Programs, Some(super::programs::View::All)));
                    }
                    if stat_tile(&mut cols[1], "⏺", monitored.to_string(), t("Monitored"), theme::OK).clicked() {
                        go = Some((Tab::Programs, Some(super::programs::View::Monitored)));
                    }
                    let bc = if broken > 0 { theme::WARN } else { theme::MUTED };
                    if stat_tile(&mut cols[2], "⚠", broken.to_string(), t("Broken entries"), bc).clicked() {
                        go = Some((Tab::Programs, Some(super::programs::View::Broken)));
                    }
                    if stat_tile(&mut cols[3], "💾", self.backups.list.len().to_string(), tf("Backups ({size})", &[("size", human_size(backup_bytes))]), theme::MUTED).clicked() {
                        go = Some((Tab::Backups, None));
                    }
                });
                if let Some((tab, view)) = go {
                    self.tab = tab;
                    if let Some(v) = view {
                        self.programs.view = v;
                    }
                }

                ui.add_space(16.0);
                self.ui_health(ui, ctx);
                ui.add_space(16.0);
                ui.label(RichText::new(t("Quick actions")).heading().strong());
                ui.add_space(6.0);
                if action_row(ui, "📡", "Install a program with monitoring", "Record everything it changes so it can be removed precisely later") {
                    self.tab = Tab::Monitor;
                    self.start_install_and_monitor(ctx);
                }
                ui.add_space(6.0);
                if action_row(ui, "🗑", "Uninstall programs", "Remove programs and everything they leave behind") {
                    self.tab = Tab::Programs;
                }
                ui.add_space(6.0);
                if action_row(ui, "🚀", "Speed up startup", "Choose which programs, services and tasks start with Windows") {
                    self.tab = Tab::Startup;
                    self.on_tab_opened(ctx, Tab::Startup);
                }
                ui.add_space(6.0);
                if action_row(ui, "✨", "Clean junk files", "Temporary files, caches and crash dumps") {
                    self.tab = Tab::Cleaner;
                }
                if self.settings.advanced_mode {
                    ui.add_space(6.0);
                    if self.orphan_job.is_some() {
                        job_progress(ui, &t("Looking for leftovers of removed programs…"), 0);
                    } else if action_row(
                        ui,
                        "🧹",
                        "Find leftovers of removed programs",
                        "Registrations, file types, PATH entries, firewall rules and tasks pointing to files that are gone",
                    ) {
                        self.scan_orphans();
                    }
                }
                ui.add_space(6.0);
                if self.settings.advanced_mode && action_row(ui, "🔁", "Manage Windows updates", "See installed updates and remove one that causes problems") {
                    self.updates.open(ctx);
                    self.style_frames = 30;
                }

                if !self.logs.logs.is_empty() {
                    ui.add_space(16.0);
                    ui.label(RichText::new(t("Recently monitored")).heading().strong());
                    ui.add_space(6.0);
                    let recent: Vec<(String, String, String)> =
                        self.logs.logs.iter().take(5).map(|l| (l.id.clone(), l.name.clone(), l.created.clone())).collect();
                    theme::card(ui, |ui| {
                        for (id, name, created) in recent {
                            ui.horizontal(|ui| {
                                if ui.link(RichText::new(name).strong()).clicked() {
                                    self.logs.selected = Some(id);
                                    self.tab = Tab::Logs;
                                }
                                small_muted(ui, created);
                            });
                        }
                    });
                }
            });
        });
    }

    fn ui_health(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.health.is_none() && self.health_job.is_none() {
            let root = self.store.root.clone();
            self.health_job = Some(super::Job::spawn(ctx, move |_| {
                expunge_core::health::check(&root)
            }));
        }
        if let Some(h) = self.health_job.as_ref().and_then(|j| j.poll()) {
            self.health_job = None;
            self.health = Some(h);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(t("System health")).heading().strong());
            if self.health_job.is_some() {
                super::widgets::spinner(ui);
            } else if ui
                .small_button("⟳")
                .on_hover_text(t("Check again"))
                .clicked()
            {
                self.health = None;
            }
        });
        ui.add_space(6.0);
        let Some((h, hist)) = &self.health else {
            return;
        };
        let mut open_disk = false;
        let mut open_startup = false;
        theme::card(ui, |ui| {
            for d in &h.drives {
                let used = d.total.saturating_sub(d.free);
                let frac = if d.total > 0 {
                    used as f32 / d.total as f32
                } else {
                    0.0
                };
                let color = if frac > 0.9 {
                    theme::BAD
                } else if frac > 0.8 {
                    theme::WARN
                } else {
                    theme::OK
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("💽 {}", d.root)).strong());
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(220.0)
                            .fill(color.gamma_multiply(0.7)),
                    );
                    ui.label(tf(
                        "{free} free of {total}",
                        &[("free", human_size(d.free)), ("total", human_size(d.total))],
                    ));
                    if let Some(ch) = hist.change(&d.root, 30) {
                        let text = if ch < 0 {
                            tf(
                                "{size} less free space than {days} days ago",
                                &[
                                    ("size", human_size(ch.unsigned_abs())),
                                    ("days", hist.series(&d.root).len().min(30).to_string()),
                                ],
                            )
                        } else {
                            tf(
                                "{size} more free space than {days} days ago",
                                &[
                                    ("size", human_size(ch as u64)),
                                    ("days", hist.series(&d.root).len().min(30).to_string()),
                                ],
                            )
                        };
                        small_muted(ui, text);
                    }
                    sparkline(ui, &hist.series(&d.root));
                    if frac > 0.8 && ui.small_button(t("See what uses space")).clicked() {
                        open_disk = true;
                    }
                });
            }
            if let Some(m) = h.memory {
                let pct = m.used_percent();
                let color = if pct > 90 {
                    theme::BAD
                } else if pct > 75 {
                    theme::WARN
                } else {
                    theme::OK
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t("Memory")).strong());
                    ui.add(
                        egui::ProgressBar::new(pct as f32 / 100.0)
                            .desired_width(220.0)
                            .fill(color.gamma_multiply(0.7)),
                    );
                    ui.label(tf(
                        "{pct}% in use ({used} of {total})",
                        &[
                            ("pct", pct.to_string()),
                            ("used", human_size(m.total - m.available.min(m.total))),
                            ("total", human_size(m.total)),
                        ],
                    ));
                });
            }
            ui.add_space(2.0);
            if h.crashes.is_empty() {
                small_muted(
                    ui,
                    t("No services stopped unexpectedly in the last 7 days."),
                );
            } else {
                ui.label(
                    RichText::new(tf(
                        "{n} services stopped unexpectedly in the last 7 days:",
                        &[("n", h.crashes.len().to_string())],
                    ))
                    .color(theme::WARN),
                );
                for c in h.crashes.iter().take(6) {
                    let when = c.last.get(..16).unwrap_or(&c.last).replace('T', " ");
                    small_muted(
                        ui,
                        tf(
                            "{name}: {n} times, last at {time} (UTC)",
                            &[
                                ("name", c.service.clone()),
                                ("n", c.count.to_string()),
                                ("time", when),
                            ],
                        ),
                    );
                }
                if self.settings.advanced_mode && ui.small_button(t("Open services")).clicked() {
                    open_startup = true;
                }
            }
        });
        if open_disk {
            self.tab = Tab::Disk;
        }
        if open_startup {
            self.tab = Tab::Startup;
            self.on_tab_opened(ctx, Tab::Startup);
        }
    }
}

/// A tiny line chart of free space over time.
fn sparkline(ui: &mut egui::Ui, series: &[(String, u64)]) {
    if series.len() < 2 {
        return;
    }
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(90.0, 18.0), egui::Sense::hover());
    let min = series.iter().map(|s| s.1).min().unwrap_or(0) as f64;
    let max = series.iter().map(|s| s.1).max().unwrap_or(0) as f64;
    let span = (max - min).max(1.0);
    let n = series.len() as f32 - 1.0;
    let pts: Vec<egui::Pos2> = series
        .iter()
        .enumerate()
        .map(|(i, (_, v))| {
            egui::pos2(
                rect.left() + rect.width() * i as f32 / n,
                rect.bottom() - rect.height() * ((*v as f64 - min) / span) as f32,
            )
        })
        .collect();
    ui.painter().add(egui::Shape::line(
        pts,
        egui::Stroke::new(1.5_f32, theme::ACCENT_TEXT),
    ));
    resp.on_hover_text(format!(
        "{}: {}\n{}: {}",
        series[0].0,
        human_size(series[0].1),
        series[series.len() - 1].0,
        human_size(series[series.len() - 1].1)
    ));
}
