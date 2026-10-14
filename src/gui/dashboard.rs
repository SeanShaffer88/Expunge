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
    egui::Frame::new()
        .fill(theme::CARD)
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).size(26.0).color(color));
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(value).size(22.0).strong());
                    small_muted(ui, label);
                });
            });
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
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
                let monitored = self.programs.list.iter().filter(|p| super::programs::log_for(&self.logs.logs, p).is_some()).count();
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
}
