//! Windows Updates window: a separate native window listing installed
//! updates, each removable individually.

use super::theme;
use super::widgets::*;
use super::Job;
use egui::RichText;
use egui_extras::{Column, TableBuilder};
use expunge_core::i18n::{t, tf};
use expunge_core::updates::{self, RemoveOutcome, UpdateEntry};

#[derive(Default)]
pub struct UpdatesState {
    pub is_open: bool,
    ctx: Option<egui::Context>,
    list: Vec<UpdateEntry>,
    loaded: bool,
    list_job: Option<Job<Result<Vec<UpdateEntry>, String>>>,
    remove_job: Option<Job<Result<RemoveOutcome, String>>>,
    removing: Option<usize>,
    pending: Option<usize>,
    restore_point: bool,
    filter: String,
    message: Option<(bool, String)>,
    restart_needed: bool,
}

impl UpdatesState {
    pub fn open(&mut self, ctx: &egui::Context) {
        self.is_open = true;
        self.ctx = Some(ctx.clone());
        if !self.loaded && self.list_job.is_none() {
            self.refresh();
        }
    }

    fn refresh(&mut self) {
        let Some(ctx) = &self.ctx else { return };
        self.loaded = true;
        self.list_job = Some(Job::spawn(ctx, |_| {
            updates::list_updates().map_err(|e| e.to_string())
        }));
    }

    pub fn start_remove(&mut self, i: usize) {
        let (Some(ctx), Some(u)) = (self.ctx.clone(), self.list.get(i).cloned()) else {
            return;
        };
        let rp = self.restore_point;
        self.removing = Some(i);
        self.message = None;
        self.remove_job = Some(Job::spawn(&ctx, move |_| {
            if rp {
                let _ = updates::create_restore_point(&format!("Before removing {}", u.title()));
            }
            updates::remove_update(&u).map_err(|e| e.to_string())
        }));
    }

    pub fn poll(&mut self) {
        if let Some(r) = self.list_job.as_ref().and_then(|j| j.poll()) {
            self.list_job = None;
            match r {
                Ok(l) => self.list = l,
                Err(e) => self.message = Some((false, e)),
            }
        }
        if let Some(r) = self.remove_job.as_ref().and_then(|j| j.poll()) {
            self.remove_job = None;
            let name = self
                .removing
                .and_then(|i| self.list.get(i))
                .map(|u| u.title())
                .unwrap_or_default();
            self.removing = None;
            match r {
                Ok(RemoveOutcome::Removed) => {
                    self.message = Some((true, tf("{name} was removed.", &[("name", name)])));
                    self.refresh();
                }
                Ok(RemoveOutcome::RestartRequired) => {
                    self.restart_needed = true;
                    self.message = Some((
                        true,
                        tf(
                            "{name} was removed. Restart Windows to finish.",
                            &[("name", name)],
                        ),
                    ));
                    self.refresh();
                }
                Err(e) => {
                    self.message = Some((
                        false,
                        tf(
                            "Could not remove {name}: {error}",
                            &[("name", name), ("error", e)],
                        ),
                    ))
                }
            }
        }
    }

    /// Draw the window if open. Returns true while it is visible.
    pub fn show(&mut self, ctx: &egui::Context, elevated: bool) -> bool {
        if !self.is_open {
            return false;
        }
        let mut builder = egui::ViewportBuilder::default()
            .with_title(format!("Expunge – {}", t("Windows Updates")))
            .with_inner_size([900.0, 600.0])
            .with_min_inner_size([620.0, 380.0]);
        if let Some(icon) = super::load_icon() {
            builder = builder.with_icon(std::sync::Arc::new(icon));
        }
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("windows_updates"),
            builder,
            |ctx, _class| {
                if ctx.input(|i| i.viewport().close_requested()) {
                    self.is_open = false;
                }
                // Esc closes this window, unless a confirmation is showing.
                if self.pending.is_none() && super::a11y::escape_pressed(ctx) {
                    self.is_open = false;
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(theme::BG).inner_margin(18))
                    .show(ctx, |ui| self.body(ui, elevated));
                self.confirm_modal(ctx);
                super::a11y::focus_ring(ctx, theme::ACCENT_TEXT);
                if self.list_job.is_some() || self.remove_job.is_some() {
                    ctx.request_repaint_after(std::time::Duration::from_millis(250));
                }
            },
        );
        true
    }

    fn body(&mut self, ui: &mut egui::Ui, elevated: bool) {
        theme::page_title(
            ui,
            &t("Windows Updates"),
            &t("Installed updates. Remove one if it causes problems."),
        );
        if !elevated {
            theme::warning_banner(
                ui,
                t("Run Expunge as Administrator to see all updates and remove them."),
            );
            ui.add_space(6.0);
        }
        if self.restart_needed {
            theme::warning_banner(ui, t("A restart is needed to finish removing updates."));
            ui.add_space(6.0);
        }
        if let Some((ok, m)) = &self.message {
            ui.colored_label(if *ok { theme::OK } else { theme::BAD }, m);
            ui.add_space(4.0);
        }
        ui.horizontal(|ui| {
            search_box(ui, &mut self.filter, "Search KB or name", 240.0);
            ui.checkbox(
                &mut self.restore_point,
                t("Create a restore point before removing"),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        self.list_job.is_none(),
                        egui::Button::new(format!("⟳ {}", t("Refresh"))),
                    )
                    .clicked()
                {
                    self.refresh();
                }
            });
        });
        ui.add_space(6.0);
        if self.list_job.is_some() {
            job_progress(
                ui,
                &t("Reading installed updates (this can take a minute)…"),
                0,
            );
            return;
        }
        if let Some(job) = &self.remove_job {
            let name = self
                .removing
                .and_then(|i| self.list.get(i))
                .map(|u| u.title())
                .unwrap_or_default();
            job_progress(
                ui,
                &tf(
                    "Removing {name}… this can take several minutes.",
                    &[("name", name)],
                ),
                job.progress.count(),
            );
            return;
        }
        let f = self.filter.to_lowercase();
        let rows: Vec<usize> = (0..self.list.len())
            .filter(|&i| {
                let u = &self.list[i];
                f.is_empty()
                    || u.kb.to_lowercase().contains(&f)
                    || u.description.to_lowercase().contains(&f)
                    || u.package.to_lowercase().contains(&f)
            })
            .collect();
        if rows.is_empty() {
            muted(ui, t("No updates found."));
            return;
        }
        let mut ask: Option<usize> = None;
        TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(110.0))
            .column(Column::initial(170.0).clip(true).resizable(true))
            .column(Column::initial(95.0))
            .column(Column::remainder().at_least(150.0).clip(true))
            .column(Column::exact(110.0))
            .header(24.0, |mut h| {
                for l in ["Update", "Type", "Installed", "Package", ""] {
                    h.col(|ui| {
                        ui.strong(t(l));
                    });
                }
            })
            .body(|body| {
                body.rows(28.0, rows.len(), |mut row| {
                    let i = rows[row.index()];
                    let u = &self.list[i];
                    row.col(|ui| {
                        if u.kb.is_empty() {
                            ui.label(u.title());
                        } else {
                            let url = format!(
                                "https://support.microsoft.com/help/{}",
                                u.kb.trim_start_matches("KB")
                            );
                            ui.hyperlink_to(RichText::new(&u.kb).strong(), url)
                                .on_hover_text(t("Open the Microsoft support article"));
                        }
                    });
                    row.col(|ui| {
                        let d = if !u.description.is_empty() {
                            u.description.clone()
                        } else {
                            u.release_type.clone()
                        };
                        ui.add(egui::Label::new(d).truncate());
                    });
                    row.col(|ui| {
                        ui.label(&u.installed);
                    });
                    row.col(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&u.package)
                                    .small()
                                    .color(theme::MUTED)
                                    .monospace(),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&u.package);
                    });
                    row.col(|ui| {
                        if u.permanent {
                            theme::badge(ui, t("Permanent"), theme::MUTED);
                        } else if ui
                            .add_enabled(elevated, egui::Button::new(t("Uninstall")))
                            .clicked()
                        {
                            ask = Some(i);
                        }
                    });
                });
            });
        if ask.is_some() {
            self.pending = ask;
        }
    }

    fn confirm_modal(&mut self, ctx: &egui::Context) {
        let Some(i) = self.pending else { return };
        let Some(u) = self.list.get(i).cloned() else {
            self.pending = None;
            return;
        };
        let resp = egui::Modal::new(egui::Id::new("confirm_update")).show(ctx, |ui| {
            ui.set_max_width(440.0);
            ui.heading(tf("Uninstall {name}?", &[("name", u.title())]));
            ui.add_space(6.0);
            ui.label(t("Windows may reinstall it automatically later unless you pause updates. A restart is usually needed."));
            ui.add_space(10.0);
            let mut choice = None;
            ui.horizontal(|ui| {
                if theme::primary_button(ui, t("Uninstall")).clicked() {
                    choice = Some(true);
                }
                if ui.button(t("Cancel")).clicked() {
                    choice = Some(false);
                }
            });
            choice
        });
        match resp.inner {
            Some(true) => {
                self.pending = None;
                self.start_remove(i);
            }
            Some(false) => self.pending = None,
            None if resp.should_close() => self.pending = None,
            None => {}
        }
    }
}
