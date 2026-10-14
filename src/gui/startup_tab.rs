//! Startup page: Run entries, Startup folders, services, scheduled tasks.

use super::app::{App, Confirm};
use super::theme;
use super::widgets::*;
use super::Job;
use egui::RichText;
use egui_extras::{Column, TableBuilder};
use expunge_core::backup::BackupWriter;
use expunge_core::i18n::{t, tf};
use expunge_core::startup::{self, ServiceEntry, StartupEntry, TaskEntry};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Sub {
    #[default]
    Entries,
    Services,
    Tasks,
}

type StartupLists = (Vec<StartupEntry>, Vec<ServiceEntry>, Vec<TaskEntry>);

#[derive(Default)]
pub struct StartupState {
    pub loaded: bool,
    pub sub: Sub,
    pub entries: Vec<StartupEntry>,
    pub services: Vec<ServiceEntry>,
    pub tasks: Vec<TaskEntry>,
    pub job: Option<Job<StartupLists>>,
    pub show_windows_services: bool,
    pub show_microsoft_tasks: bool,
    pub filter: String,
}

impl App {
    pub fn refresh_startup(&mut self, ctx: &egui::Context) {
        let reg = self.reg.clone();
        self.startup.loaded = true;
        self.startup.job = Some(Job::spawn(ctx, move |_| {
            (
                startup::list_startup(reg.as_ref()),
                startup::list_services(reg.as_ref()),
                startup::list_tasks(),
            )
        }));
    }

    pub fn poll_startup(&mut self) {
        if let Some((e, s, tk)) = self.startup.job.as_ref().and_then(|j| j.poll()) {
            self.startup.job = None;
            self.startup.entries = e;
            self.startup.services = s;
            self.startup.tasks = tk;
        }
    }

    /// Run a small change with a backup, reporting the outcome in the status bar.
    fn with_backup(
        &mut self,
        title: &str,
        f: impl FnOnce(
            &dyn expunge_core::platform::Registry,
            Option<&mut BackupWriter>,
        ) -> anyhow::Result<()>,
    ) {
        let mut w = if self.settings.backup_before_removal {
            BackupWriter::new(&self.store.backups_dir(), title).ok()
        } else {
            None
        };
        let r = f(self.reg.as_ref(), w.as_mut());
        if let Some(w) = w {
            if w.is_empty() {
                w.discard();
            } else {
                let _ = w.finish();
            }
        }
        match r {
            Ok(()) => self.set_status(format!("✔ {title}")),
            Err(e) => self.set_status(format!("✖ {title}: {e:#}")),
        }
        let ctx = self.ctx.clone();
        self.refresh_startup(&ctx);
        self.reload_backups();
    }

    pub fn delete_startup(&mut self, i: usize) {
        let Some(e) = self.startup.entries.get(i).cloned() else {
            return;
        };
        self.with_backup(
            &tf("Delete startup entry {name}", &[("name", e.name.clone())]),
            |reg, b| startup::delete_entry(reg, &e, b),
        );
    }

    pub fn delete_task(&mut self, i: usize) {
        let Some(task) = self.startup.tasks.get(i).cloned() else {
            return;
        };
        self.with_backup(
            &tf(
                "Delete scheduled task {name}",
                &[("name", task.name.clone())],
            ),
            |_, b| startup::delete_task(&task, b),
        );
    }

    pub fn ui_startup(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(theme::content_frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (s, label) in [
                        (Sub::Entries, t("Startup programs")),
                        (Sub::Services, t("Services")),
                        (Sub::Tasks, t("Scheduled tasks")),
                    ] {
                        if ui.selectable_label(self.startup.sub == s, label).clicked() {
                            self.startup.sub = s;
                        }
                    }
                    ui.separator();
                    search_box(ui, &mut self.startup.filter, "Filter", 200.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(format!("⟳ {}", t("Refresh"))).clicked() {
                            self.refresh_startup(ctx);
                        }
                    });
                });
                ui.add_space(6.0);
                if self.startup.job.is_some() {
                    job_progress(ui, &t("Loading…"), 0);
                    return;
                }
                match self.startup.sub {
                    Sub::Entries => self.ui_startup_entries(ui),
                    Sub::Services => self.ui_services(ui),
                    Sub::Tasks => self.ui_tasks(ui),
                }
            });
    }

    fn ui_startup_entries(&mut self, ui: &mut egui::Ui) {
        small_muted(ui, t("Disabling works like Task Manager: the entry stays but doesn't run. Deleting removes it (backed up)."));
        ui.add_space(4.0);
        let f = self.startup.filter.to_lowercase();
        let rows: Vec<usize> = (0..self.startup.entries.len())
            .filter(|&i| {
                let e = &self.startup.entries[i];
                f.is_empty()
                    || e.name.to_lowercase().contains(&f)
                    || e.command.to_lowercase().contains(&f)
            })
            .collect();
        let mut toggle: Option<(usize, bool)> = None;
        let mut delete: Option<usize> = None;
        TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(70.0))
            .column(
                Column::initial(200.0)
                    .at_least(100.0)
                    .clip(true)
                    .resizable(true),
            )
            .column(Column::remainder().at_least(200.0).clip(true))
            .column(Column::initial(110.0))
            .column(Column::exact(80.0))
            .header(24.0, |mut h| {
                for l in ["Enabled", "Name", "Command", "Scope", ""] {
                    h.col(|ui| {
                        ui.strong(t(l));
                    });
                }
            })
            .body(|body| {
                body.rows(26.0, rows.len(), |mut row| {
                    let i = rows[row.index()];
                    let e = &self.startup.entries[i];
                    row.col(|ui| {
                        let mut on = e.enabled;
                        let resp =
                            ui.add_enabled(e.approved.is_some(), egui::Checkbox::new(&mut on, ""));
                        if resp.changed() {
                            toggle = Some((i, on));
                        }
                    });
                    row.col(|ui| {
                        if e.missing {
                            ui.label(RichText::new("⚠").color(theme::WARN))
                                .on_hover_text(t("The program this entry starts no longer exists"));
                        }
                        ui.add(egui::Label::new(&e.name).truncate());
                    });
                    row.col(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&e.command).color(theme::MUTED).monospace(),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&e.location);
                    });
                    row.col(|ui| {
                        ui.label(if e.all_users {
                            t("All users")
                        } else {
                            t("Current user")
                        });
                    });
                    row.col(|ui| {
                        if ui.small_button(t("Delete")).clicked() {
                            delete = Some(i);
                        }
                    });
                });
            });
        if let Some((i, on)) = toggle {
            let e = self.startup.entries[i].clone();
            let title = if on {
                tf("Enable {name}", &[("name", e.name.clone())])
            } else {
                tf("Disable {name}", &[("name", e.name.clone())])
            };
            self.with_backup(&title, |reg, b| startup::set_enabled(reg, &e, on, b));
        }
        if let Some(i) = delete {
            let name = self.startup.entries[i].name.clone();
            self.ask(
                "Delete startup entry?",
                tf(
                    "{name} will no longer start with Windows. A backup is kept.",
                    &[("name", name)],
                ),
                "Delete",
                Confirm::DeleteStartup(i),
            );
        }
    }

    fn ui_services(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut self.startup.show_windows_services,
                t("Show Windows services"),
            );
            small_muted(ui, t("Start type changes apply at the next restart."));
        });
        ui.add_space(4.0);
        let f = self.startup.filter.to_lowercase();
        let rows: Vec<usize> = (0..self.startup.services.len())
            .filter(|&i| {
                let s = &self.startup.services[i];
                (self.startup.show_windows_services || !s.windows)
                    && !s.is_driver
                    && (f.is_empty()
                        || s.display_name.to_lowercase().contains(&f)
                        || s.name.to_lowercase().contains(&f))
            })
            .collect();
        let mut change: Option<(usize, u32, bool)> = None;
        TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(
                Column::initial(260.0)
                    .at_least(120.0)
                    .clip(true)
                    .resizable(true),
            )
            .column(Column::initial(160.0).clip(true).resizable(true))
            .column(Column::remainder().at_least(160.0).clip(true))
            .header(24.0, |mut h| {
                for l in ["Service", "Start type", "Program"] {
                    h.col(|ui| {
                        ui.strong(t(l));
                    });
                }
            })
            .body(|body| {
                body.rows(26.0, rows.len(), |mut row| {
                    let i = rows[row.index()];
                    let s = &self.startup.services[i];
                    row.col(|ui| {
                        ui.add(egui::Label::new(&s.display_name).truncate())
                            .on_hover_text(&s.name);
                    });
                    row.col(|ui| {
                        let label = |k: &str| {
                            t(match k {
                                "automatic" => "Automatic",
                                "auto_delayed" => "Automatic (delayed)",
                                "manual" => "Manual",
                                "disabled" => "Disabled",
                                "boot" => "Boot",
                                "system" => "System",
                                _ => "Unknown",
                            })
                        };
                        egui::ComboBox::from_id_salt(("svc", i))
                            .width(150.0)
                            .selected_text(label(s.start_label()))
                            .show_ui(ui, |ui| {
                                for (start, delayed, key) in [
                                    (2, false, "automatic"),
                                    (2, true, "auto_delayed"),
                                    (3, false, "manual"),
                                    (4, false, "disabled"),
                                ] {
                                    if ui
                                        .selectable_label(s.start_label() == key, label(key))
                                        .clicked()
                                    {
                                        change = Some((i, start, delayed));
                                    }
                                }
                            });
                    });
                    row.col(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&s.image_path).color(theme::MUTED).monospace(),
                            )
                            .truncate(),
                        );
                    });
                });
            });
        if let Some((i, start, delayed)) = change {
            let name = self.startup.services[i].name.clone();
            self.with_backup(
                &tf("Change start type of {name}", &[("name", name.clone())]),
                |reg, b| startup::set_service_start(reg, &name, start, delayed, b),
            );
        }
    }

    fn ui_tasks(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(
            &mut self.startup.show_microsoft_tasks,
            t("Show Microsoft tasks"),
        );
        ui.add_space(4.0);
        let f = self.startup.filter.to_lowercase();
        let rows: Vec<usize> = (0..self.startup.tasks.len())
            .filter(|&i| {
                let tk = &self.startup.tasks[i];
                (self.startup.show_microsoft_tasks || !tk.microsoft)
                    && (f.is_empty()
                        || tk.name.to_lowercase().contains(&f)
                        || tk.command.to_lowercase().contains(&f))
            })
            .collect();
        if rows.is_empty() && !self.elevated {
            theme::warning_banner(
                ui,
                t("Scheduled tasks can only be read when Expunge runs as administrator."),
            );
        }
        let mut toggle: Option<(usize, bool)> = None;
        let mut delete: Option<usize> = None;
        TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(70.0))
            .column(
                Column::initial(260.0)
                    .at_least(120.0)
                    .clip(true)
                    .resizable(true),
            )
            .column(Column::remainder().at_least(160.0).clip(true))
            .column(Column::exact(80.0))
            .header(24.0, |mut h| {
                for l in ["Enabled", "Task", "Runs", ""] {
                    h.col(|ui| {
                        ui.strong(t(l));
                    });
                }
            })
            .body(|body| {
                body.rows(26.0, rows.len(), |mut row| {
                    let i = rows[row.index()];
                    let tk = &self.startup.tasks[i];
                    row.col(|ui| {
                        let mut on = tk.enabled;
                        if ui.checkbox(&mut on, "").changed() {
                            toggle = Some((i, on));
                        }
                    });
                    row.col(|ui| {
                        ui.add(egui::Label::new(&tk.name).truncate())
                            .on_hover_text(&tk.author);
                    });
                    row.col(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&tk.command).color(theme::MUTED).monospace(),
                            )
                            .truncate(),
                        );
                    });
                    row.col(|ui| {
                        if ui.small_button(t("Delete")).clicked() {
                            delete = Some(i);
                        }
                    });
                });
            });
        if let Some((i, on)) = toggle {
            let name = self.startup.tasks[i].name.clone();
            match startup::set_task_enabled(&name, on) {
                Ok(()) => self.startup.tasks[i].enabled = on,
                Err(e) => self.set_status(format!("✖ {e:#}")),
            }
        }
        if let Some(i) = delete {
            let name = self.startup.tasks[i].name.clone();
            self.ask(
                "Delete scheduled task?",
                tf(
                    "{name} will be deleted. A backup is kept.",
                    &[("name", name)],
                ),
                "Delete",
                Confirm::DeleteTask(i),
            );
        }
    }
}
