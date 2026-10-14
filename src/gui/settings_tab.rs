//! Settings page.

use super::app::{App, Confirm};
use super::theme;
use super::widgets::*;
use egui::RichText;
use expunge_core::i18n::{self, t};
use expunge_core::settings::{FileRoot, Settings};

/// Text-editable copies of the list settings.
#[derive(Default)]
pub struct SettingsState {
    pub reg_roots: String,
    pub file_roots_rec: String,
    pub file_roots_top: String,
    pub reg_excl: String,
    pub file_excl: String,
    pub dirty: bool,
}

impl SettingsState {
    pub fn load(&mut self, s: &Settings) {
        self.reg_roots = s.registry_roots.join("\n");
        self.file_roots_rec = s
            .file_roots
            .iter()
            .filter(|r| r.recursive)
            .map(|r| r.path.clone())
            .collect::<Vec<_>>()
            .join("\n");
        self.file_roots_top = s
            .file_roots
            .iter()
            .filter(|r| !r.recursive)
            .map(|r| r.path.clone())
            .collect::<Vec<_>>()
            .join("\n");
        self.reg_excl = s.registry_exclusions.join("\n");
        self.file_excl = s.file_exclusions.join("\n");
        self.dirty = false;
    }

    fn apply(&self, s: &mut Settings) {
        let lines = |t: &str| {
            t.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
        };
        s.registry_roots = lines(&self.reg_roots);
        s.file_roots = lines(&self.file_roots_rec)
            .into_iter()
            .map(|p| FileRoot {
                path: p,
                recursive: true,
            })
            .chain(lines(&self.file_roots_top).into_iter().map(|p| FileRoot {
                path: p,
                recursive: false,
            }))
            .collect();
        s.registry_exclusions = lines(&self.reg_excl);
        s.file_exclusions = lines(&self.file_excl);
    }
}

fn list_editor(ui: &mut egui::Ui, label: &str, help: &str, text: &mut String, dirty: &mut bool) {
    ui.label(RichText::new(t(label)).strong());
    small_muted(ui, t(help));
    let r = ui.add(
        egui::TextEdit::multiline(text)
            .code_editor()
            .desired_rows(5)
            .desired_width(f32::INFINITY),
    );
    *dirty |= r.changed();
    ui.add_space(8.0);
}

impl App {
    pub fn ui_settings(&mut self, ctx: &egui::Context) {
        if self.settings_ui.reg_roots.is_empty() && !self.settings_ui.dirty {
            self.settings_ui.load(&self.settings);
        }
        egui::CentralPanel::default().frame(theme::content_frame()).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let mut changed = false;
                theme::card(ui, |ui| {
                    ui.label(RichText::new(t("General")).heading());
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(t("Language"));
                        let current = self
                            .languages
                            .iter()
                            .find(|(c, _)| *c == self.settings.language)
                            .map(|(_, n)| n.clone())
                            .unwrap_or_else(|| t("Windows default"));
                        egui::ComboBox::from_id_salt("lang").selected_text(current).show_ui(ui, |ui| {
                            changed |= ui.selectable_value(&mut self.settings.language, String::new(), t("Windows default")).changed();
                            for (code, name) in &self.languages {
                                changed |= ui.selectable_value(&mut self.settings.language, code.clone(), name).changed();
                            }
                        });
                        if changed {
                            i18n::set_language(&self.settings.language);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(t("Interface size"));
                        let r = ui.add(egui::Slider::new(&mut self.settings.zoom, 0.75..=2.0).step_by(0.05));
                        if r.changed() {
                            ctx.set_zoom_factor(self.settings.zoom);
                        }
                        changed |= r.drag_stopped() || r.lost_focus();
                    });
                    small_muted(ui, t("Translations live in the lang folder; you can add or improve languages without rebuilding."));
                    ui.add_space(8.0);
                    let was_advanced = self.settings.advanced_mode;
                    ui.horizontal(|ui| {
                        ui.label(t("Interface mode"));
                        changed |= ui
                            .radio_value(&mut self.settings.advanced_mode, false, t("Simple"))
                            .on_hover_text(t("Plain language, fewer technical details. Recommended if you're not sure."))
                            .changed();
                        changed |= ui
                            .radio_value(&mut self.settings.advanced_mode, true, t("Advanced"))
                            .on_hover_text(t("All details: registry keys, services, scheduled tasks and the Installer Lab for developers."))
                            .changed();
                    });
                    if was_advanced != self.settings.advanced_mode {
                        // System components appear or disappear with the mode.
                        self.programs.needs_refresh = true;
                    }
                    if ui.checkbox(&mut self.settings.high_contrast, t("High contrast (brighter text, stronger outlines)")).changed() {
                        changed = true;
                        super::theme::apply(ctx, self.settings.high_contrast);
                    }
                    ui.add_space(4.0);
                    small_muted(ui, t("Keyboard: Ctrl+1–8 switch pages · Ctrl+F search · F5 refresh · Delete uninstalls the selected program · F1 help."));
                });
                ui.add_space(10.0);
                theme::card(ui, |ui| {
                    ui.label(RichText::new(t("Safety")).heading());
                    ui.add_space(4.0);
                    changed |= ui.checkbox(&mut self.settings.create_restore_point, t("Create a System Restore point before uninstalling")).changed();
                    if self.settings.advanced_mode {
                        changed |= ui.checkbox(&mut self.settings.backup_before_removal, t("Back up everything before removing it (recommended)")).changed();
                        changed |= ui.checkbox(&mut self.settings.confirm_actions, t("Ask before deleting things")).changed();
                        changed |= ui
                            .checkbox(&mut self.settings.use_recycle_bin, t("Also send removed files to the Recycle Bin"))
                            .on_hover_text(t("Everything is backed up either way. This additionally keeps removed files in the Recycle Bin, using extra disk space."))
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.remove_unlogged_files, t("When uninstalling a monitored program, also remove files it created later"))
                            .changed();
                    } else {
                        small_muted(ui, t("Simple mode always backs up before removing and asks before deleting. Options that could harm Windows or slow your PC are only in Advanced mode."));
                    }
                });
                ui.add_space(10.0);
                theme::card(ui, |ui| {
                    ui.label(RichText::new(t("Program list")).heading());
                    ui.add_space(4.0);
                    let mut refresh = false;
                    if self.settings.advanced_mode {
                        refresh |= ui.checkbox(&mut self.settings.show_system_components, t("Show system components and updates")).changed();
                    }
                    refresh |= ui.checkbox(&mut self.settings.show_store_apps, t("Load Microsoft Store apps at startup")).changed();
                    if refresh {
                        changed = true;
                        self.programs.needs_refresh = true;
                    }
                });
                if changed {
                    self.save_settings();
                }
                ui.add_space(10.0);
                theme::card(ui, |ui| {
                    ui.label(RichText::new(t("Explorer integration")).heading());
                    ui.add_space(4.0);
                    let reg = self.reg.clone();
                    let machine = expunge_core::shell::is_registered(reg.as_ref(), true);
                    let mut user = expunge_core::shell::is_registered(reg.as_ref(), false);
                    if machine {
                        ui.label(format!("✔ {}", t("\"Install with Expunge\" is in the right-click menu for all users (added by Setup).")));
                    } else if ui
                        .checkbox(&mut user, t("Add \"Install with Expunge\" to the right-click menu of installers"))
                        .changed()
                    {
                        let res = if user {
                            let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                            expunge_core::shell::register(reg.as_ref(), &exe, &t("Install with Expunge"), false)
                        } else {
                            expunge_core::shell::unregister(reg.as_ref(), false)
                        };
                        if let Err(e) = res {
                            self.set_status(e.to_string());
                        }
                    }
                    small_muted(
                        ui,
                        t("Right-click any .exe or .msi installer to install it under monitoring. On Windows 11 it is under \"Show more options\"."),
                    );
                });
                ui.add_space(10.0);
                if !self.settings.advanced_mode {
                    theme::card(ui, |ui| {
                        small_muted(ui, t("Switch to Advanced mode to change what snapshots capture."));
                        if ui.button(t("Reset everything to defaults")).clicked() {
                            self.ask("Reset settings?", t("All settings except the language go back to their defaults."), "Reset", Confirm::ResetSettings);
                        }
                    });
                } else {
                theme::card(ui, |ui| {
                    ui.label(RichText::new(t("Monitoring scope")).heading());
                    small_muted(ui, t("What snapshots capture. One entry per line; %VARIABLES% and * wildcards work."));
                    ui.add_space(6.0);
                    let st = &mut self.settings_ui;
                    let mut dirty = st.dirty;
                    list_editor(ui, "Registry keys", "Whole subtrees are captured.", &mut st.reg_roots, &mut dirty);
                    list_editor(ui, "Folders (with subfolders)", "", &mut st.file_roots_rec, &mut dirty);
                    list_editor(ui, "Folders (top level only)", "", &mut st.file_roots_top, &mut dirty);
                    list_editor(ui, "Ignored registry keys", "Background noise Windows changes on its own.", &mut st.reg_excl, &mut dirty);
                    list_editor(ui, "Ignored files and folders", "", &mut st.file_excl, &mut dirty);
                    st.dirty = dirty;
                    ui.horizontal(|ui| {
                        if ui.add_enabled(self.settings_ui.dirty, egui::Button::new(t("Save scope"))).clicked() {
                            self.settings_ui.apply(&mut self.settings);
                            self.settings_ui.dirty = false;
                            self.save_settings();
                            self.set_status(t("Settings saved."));
                        }
                        if ui.button(t("Reset everything to defaults")).clicked() {
                            self.ask("Reset settings?", t("All settings except the language go back to their defaults."), "Reset", Confirm::ResetSettings);
                        }
                    });
                });
                }
                ui.add_space(10.0);
                theme::card(ui, |ui| {
                    ui.label(RichText::new(t("Data")).heading());
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(t("Data folder"));
                        ui.add(egui::Label::new(RichText::new(self.store.root.to_string_lossy()).monospace()).truncate());
                        if ui.small_button(t("Open")).clicked() {
                            expunge_core::platform::shell_open(&self.store.root.to_string_lossy());
                        }
                    });
                    small_muted(
                        ui,
                        if self.store.portable {
                            t("Portable mode: everything is stored next to Expunge.exe.")
                        } else {
                            t("Tip: put an empty file named \"portable\" next to Expunge.exe to keep all data beside it.")
                        },
                    );
                });
            });
        });
    }
}
