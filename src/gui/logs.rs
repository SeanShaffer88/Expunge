//! Install logs page: browse monitored installations, search which program
//! created a path, export, and uninstall precisely.

use super::app::{App, Confirm};
use super::plan::RemovalFlow;
use super::theme;
use super::widgets::*;
use egui::RichText;
use expunge_core::i18n::{t, tf};
use expunge_core::model::{human_size, ChangeSet, InstallLog};
use expunge_core::report;
use expunge_core::revert::build_plan;

#[derive(Default)]
pub struct LogsState {
    pub logs: Vec<InstallLog>,
    pub selected: Option<String>,
    pub filter: String,
    /// "Which program created this?" search.
    pub path_search: String,
    pub editing: bool,
}

/// Collapsible lists of every change. With `editable`, each entry gets a ✖
/// button to drop it (used to strip noise before saving a capture).
pub fn changes_ui(ui: &mut egui::Ui, cs: &mut ChangeSet, editable: bool) {
    fn section<T>(
        ui: &mut egui::Ui,
        id: &str,
        title: String,
        color: egui::Color32,
        items: &mut Vec<T>,
        editable: bool,
        render: impl Fn(&T) -> (String, String),
    ) {
        if items.is_empty() {
            return;
        }
        let mut remove: Option<usize> = None;
        egui::CollapsingHeader::new(
            RichText::new(format!("{title}  ({})", items.len()))
                .color(color)
                .strong(),
        )
        .id_salt(id)
        .default_open(items.len() <= 40)
        .show(ui, |ui| {
            let row_h = 20.0;
            egui::ScrollArea::vertical()
                .id_salt(id)
                .max_height(320.0)
                .auto_shrink([false, true])
                .show_rows(ui, row_h, items.len(), |ui, range| {
                    for i in range {
                        let (main, detail) = render(&items[i]);
                        ui.horizontal(|ui| {
                            if editable
                                && ui
                                    .small_button("✖")
                                    .on_hover_text(t("Not part of this program"))
                                    .clicked()
                            {
                                remove = Some(i);
                            }
                            ui.add(egui::Label::new(RichText::new(&main).monospace()).truncate())
                                .on_hover_text(&main);
                            if !detail.is_empty() {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(detail).small().color(theme::MUTED),
                                    )
                                    .truncate(),
                                );
                            }
                        });
                    }
                });
        });
        if let Some(i) = remove {
            items.remove(i);
        }
    }
    section(
        ui,
        "fa",
        t("Files added"),
        theme::ADDED,
        &mut cs.files_added,
        editable,
        |f| {
            (
                f.path.clone(),
                if f.is_dir {
                    t("folder")
                } else {
                    human_size(f.size)
                },
            )
        },
    );
    section(
        ui,
        "ka",
        t("Registry keys added"),
        theme::ADDED,
        &mut cs.keys_added,
        editable,
        |k| {
            (
                k.path.clone(),
                tf("{n} values", &[("n", k.values.len().to_string())]),
            )
        },
    );
    section(
        ui,
        "va",
        t("Registry values added"),
        theme::ADDED,
        &mut cs.values_added,
        editable,
        |v| {
            (
                format!("{}\\{}", v.key, v.value.display_name()),
                v.value.display_data(),
            )
        },
    );
    section(
        ui,
        "vm",
        t("Registry values changed"),
        theme::CHANGED,
        &mut cs.values_modified,
        editable,
        |v| {
            (
                format!("{}\\{}", v.key, v.after.display_name()),
                format!("{} → {}", v.before.display_data(), v.after.display_data()),
            )
        },
    );
    section(
        ui,
        "fm",
        t("Files changed"),
        theme::CHANGED,
        &mut cs.files_modified,
        editable,
        |f| {
            (
                f.after.path.clone(),
                format!(
                    "{} → {}",
                    human_size(f.before.size),
                    human_size(f.after.size)
                ),
            )
        },
    );
    section(
        ui,
        "vr",
        t("Registry values removed"),
        theme::REMOVED,
        &mut cs.values_removed,
        editable,
        |v| {
            (
                format!("{}\\{}", v.key, v.value.display_name()),
                v.value.display_data(),
            )
        },
    );
    section(
        ui,
        "kr",
        t("Registry keys removed"),
        theme::REMOVED,
        &mut cs.keys_removed,
        editable,
        |k| (k.path.clone(), String::new()),
    );
    section(
        ui,
        "fr",
        t("Files removed"),
        theme::REMOVED,
        &mut cs.files_removed,
        editable,
        |f| (f.path.clone(), String::new()),
    );
    if cs.is_empty() {
        muted(ui, t("No changes were recorded."));
    }
}

fn clone_meta(l: &InstallLog) -> InstallLog {
    InstallLog {
        id: l.id.clone(),
        name: l.name.clone(),
        publisher: l.publisher.clone(),
        version: l.version.clone(),
        created: l.created.clone(),
        installer: l.installer.clone(),
        uninstall_key: l.uninstall_key.clone(),
        notes: l.notes.clone(),
        group: l.group.clone(),
        changes: ChangeSet::default(),
    }
}

impl App {
    pub fn delete_log(&mut self, id: &str) {
        match self.store.delete_log(id) {
            Ok(()) => {
                self.logs.selected = None;
                self.reload_logs();
                self.programs.needs_refresh = true;
            }
            Err(e) => self.set_status(e.to_string()),
        }
    }

    fn uninstall_from_log(&mut self, log: &InstallLog) {
        let plan = build_plan(&log.changes, self.reg.as_ref());
        let mut flow = RemovalFlow::new(
            tf("Uninstall {name}", &[("name", log.name.clone())]),
            plan,
            true,
            self.settings.remove_unlogged_files,
        );
        flow.logs_to_delete = vec![log.id.clone()];
        flow.names = vec![log.name.clone()];
        self.open_removal(flow);
    }

    fn export_log(&mut self, log: &InstallLog, ext: &str) {
        let base = expunge_core::util::safe_file_name(&log.name);
        let (content, filter) = match ext {
            "html" => (report::to_html(log), "HTML"),
            "reg" => (report::to_reg_undo(log), "Registry script"),
            "ps1" => (report::to_ps1(log), "PowerShell"),
            _ => (
                serde_json::to_string_pretty(log).unwrap_or_default(),
                "Expunge log",
            ),
        };
        let default = format!("{base}.{ext}");
        let Some(path) = save_file(&t("Export install log"), &default, filter, ext) else {
            return;
        };
        // .reg files are expected to be UTF-16 with a BOM.
        let res = if ext == "reg" {
            let mut bytes = vec![0xFF, 0xFE];
            for u in content.encode_utf16() {
                bytes.extend_from_slice(&u.to_le_bytes());
            }
            std::fs::write(&path, bytes)
        } else {
            std::fs::write(&path, content)
        };
        match res {
            Ok(()) => {
                self.set_status(tf("Saved {path}", &[("path", path.display().to_string())]));
                if ext == "html" {
                    expunge_core::platform::shell_open(&path.to_string_lossy());
                }
            }
            Err(e) => self.set_status(e.to_string()),
        }
    }

    pub fn ui_logs(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("logs_list")
            .frame(theme::side_frame().inner_margin(egui::Margin {
                left: 18,
                right: 8,
                top: 4,
                bottom: 12,
            }))
            .show_separator_line(false)
            .resizable(true)
            .default_width(300.0)
            .show(ctx, |ui| {
                search_box(
                    ui,
                    &mut self.logs.filter,
                    "Search logs",
                    ui.available_width() - 30.0,
                );
                ui.add_space(6.0);
                let f = self.logs.filter.to_lowercase();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.logs.logs.is_empty() {
                            ui.add_space(20.0);
                            muted(
                                ui,
                                t("No monitored installations yet. Use Monitor to record one."),
                            );
                        }
                        for log in &self.logs.logs {
                            if !f.is_empty()
                                && !log.name.to_lowercase().contains(&f)
                                && !log.publisher.to_lowercase().contains(&f)
                            {
                                continue;
                            }
                            let sel = self.logs.selected.as_deref() == Some(&log.id);
                            let fill = if sel {
                                theme::NAV_SELECTED
                            } else {
                                theme::CARD
                            };
                            let resp = egui::Frame::new()
                                .fill(fill)
                                .corner_radius(10)
                                .inner_margin(egui::Margin::symmetric(12, 8))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(RichText::new(&log.name).strong());
                                    small_muted(
                                        ui,
                                        format!(
                                            "{}  ·  {}",
                                            log.created,
                                            human_size(log.changes.stats().bytes_added)
                                        ),
                                    );
                                })
                                .response
                                .interact(egui::Sense::click());
                            if resp.clicked() {
                                self.logs.selected = Some(log.id.clone());
                                self.logs.editing = false;
                            }
                            ui.add_space(4.0);
                        }
                    });
            });

        egui::CentralPanel::default().frame(theme::content_frame()).show(ctx, |ui| {
            theme::card(ui, |ui| {
                ui.label(RichText::new(t("Which program created this?")).strong());
                small_muted(ui, t("Type part of a file path or registry key to find the monitored program it belongs to."));
                search_box(ui, &mut self.logs.path_search, "e.g. AppData\\Roaming\\Acme or HKCU\\Software\\Acme", 420.0);
                let hits = report::search_logs(&self.logs.logs, &self.logs.path_search);
                let mut jump = None;
                for (log, paths) in hits.iter().take(10) {
                    ui.horizontal(|ui| {
                        if ui.link(RichText::new(&log.name).strong()).clicked() {
                            jump = Some(log.id.clone());
                        }
                        small_muted(ui, tf("{n} matches, e.g. {path}", &[("n", paths.len().to_string()), ("path", paths[0].clone())]));
                    });
                }
                if !self.logs.path_search.trim().is_empty() && hits.is_empty() {
                    small_muted(ui, t("No monitored program created anything matching that."));
                }
                if let Some(id) = jump {
                    self.logs.selected = Some(id);
                }
            });
            ui.add_space(10.0);

            let Some(idx) = self.logs.selected.as_ref().and_then(|id| self.logs.logs.iter().position(|l| &l.id == id)) else {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| muted(ui, t("Select an install log on the left.")));
                return;
            };
            // Clone only the metadata; the change lists can be huge.
            let log = {
                let l = &self.logs.logs[idx];
                clone_meta(l)
            };
            let st = self.logs.logs[idx].changes.stats();
            theme::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new(&log.name).strong());
                    if !log.version.is_empty() {
                        theme::badge(ui, &log.version, theme::MUTED);
                    }
                });
                small_muted(
                    ui,
                    format!(
                        "{}{}  ·  {}",
                        log.publisher,
                        if log.publisher.is_empty() { "" } else { "  ·" },
                        tf("monitored {name}", &[("name", log.created.clone())])
                    ),
                );
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    theme::badge(ui, tf("{n} files", &[("n", st.files_added.to_string())]), theme::ADDED);
                    theme::badge(ui, tf("{n} folders", &[("n", st.dirs_added.to_string())]), theme::ADDED);
                    theme::badge(ui, tf("{n} keys", &[("n", st.keys_added.to_string())]), theme::ADDED);
                    theme::badge(ui, tf("{n} values", &[("n", st.values_added.to_string())]), theme::ADDED);
                    theme::badge(ui, tf("{n} changed", &[("n", (st.values_modified + st.files_modified).to_string())]), theme::CHANGED);
                    theme::badge(ui, human_size(st.bytes_added), theme::MUTED);
                });
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if theme::primary_button(ui, format!("🗑 {}", t("Uninstall using this log"))).clicked() {
                        let full = self.logs.logs[idx].clone();
                        self.uninstall_from_log(&full);
                    }
                    ui.menu_button(t("Export"), |ui| {
                        for (ext, label) in [
                            ("html", "HTML report"),
                            ("reg", "Registry undo script (.reg)"),
                            ("ps1", "PowerShell removal script"),
                            ("json", "Install log (.json)"),
                        ] {
                            if ui.button(t(label)).clicked() {
                                ui.close();
                                let full = self.logs.logs[idx].clone();
                                self.export_log(&full, ext);
                            }
                        }
                    });
                    if ui.button(t("Edit details")).clicked() {
                        self.logs.editing = !self.logs.editing;
                    }
                    if ui.button(t("Delete log")).clicked() {
                        self.ask(
                            "Delete install log?",
                            tf("The record of {name} will be deleted. The program itself is not touched.", &[("name", log.name.clone())]),
                            "Delete",
                            Confirm::DeleteLog(log.id.clone()),
                        );
                    }
                });
                if !log.notes.is_empty() && !self.logs.editing {
                    ui.add_space(4.0);
                    small_muted(ui, &log.notes);
                }
            });
            if self.logs.editing {
                ui.add_space(8.0);
                let mut save = false;
                theme::card(ui, |ui| {
                    let l = &mut self.logs.logs[idx];
                    egui::Grid::new("edit_log").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label(t("Name"));
                        ui.text_edit_singleline(&mut l.name);
                        ui.end_row();
                        ui.label(t("Publisher"));
                        ui.text_edit_singleline(&mut l.publisher);
                        ui.end_row();
                        ui.label(t("Version"));
                        ui.text_edit_singleline(&mut l.version);
                        ui.end_row();
                        ui.label(t("Notes"));
                        ui.text_edit_multiline(&mut l.notes);
                        ui.end_row();
                    });
                    if theme::primary_button(ui, t("Save")).clicked() {
                        save = true;
                    }
                });
                if save {
                    let l = self.logs.logs[idx].clone();
                    if let Err(e) = self.store.save_log(&l) {
                        self.set_status(e.to_string());
                    }
                    self.logs.editing = false;
                }
            }
            ui.add_space(8.0);
            let cs = &mut self.logs.logs[idx].changes;
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                theme::card(ui, |ui| changes_ui(ui, cs, false));
            });
        });
    }
}
