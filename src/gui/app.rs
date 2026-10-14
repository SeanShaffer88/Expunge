//! Application shell: menus, tabs, dialogs, and per-frame polling.

use super::theme;
use super::widgets::*;
use super::*;
use egui::RichText;
use expunge_core::i18n::{self, t, tf};
use expunge_core::platform::{self, Registry};
use expunge_core::settings::Settings;
use expunge_core::store::Store;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Dashboard,
    Programs,
    Monitor,
    Logs,
    Backups,
    Startup,
    Cleaner,
    Settings,
}

/// Actions that need a yes/no confirmation first.
#[derive(Clone, Debug)]
pub enum Confirm {
    DeleteLog(String),
    RestoreBackup(String),
    DeleteBackup(String),
    CleanJunk,
    DeleteStartup(usize),
    DeleteTask(usize),
    ResetSettings,
    CancelMonitoring,
}

pub struct ConfirmDialog {
    pub title: String,
    pub body: String,
    pub ok: String,
    pub action: Confirm,
}

pub struct App {
    pub ctx: egui::Context,
    pub logo: egui::TextureHandle,
    pub reg: Arc<dyn Registry>,
    pub store: Store,
    pub settings: Settings,
    pub tab: Tab,
    pub status: String,
    pub elevated: bool,
    pub admin_prompt: bool,
    pub about_open: bool,
    pub style_frames: u32,
    pub frame: u64,
    pub confirm: Option<ConfirmDialog>,
    pub languages: Vec<(String, String)>,
    pub programs: programs::ProgramsState,
    pub monitor: monitor::MonitorState,
    pub logs: logs::LogsState,
    pub backups: backups::BackupsState,
    pub startup: startup_tab::StartupState,
    pub cleaner: cleaner::CleanerState,
    pub settings_ui: settings_tab::SettingsState,
    pub updates: updates::UpdatesState,
    pub wizard: Option<programs::Wizard>,
    pub removal: Option<plan::RemovalFlow>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let store = Store::open().unwrap_or_else(|_| {
            Store::at(&std::env::temp_dir().join("Expunge")).expect("data folder")
        });
        let settings = store.load_settings();
        i18n::set_language(&settings.language);
        cc.egui_ctx.set_zoom_factor(settings.zoom.clamp(0.75, 2.0));
        let reg: Arc<dyn Registry> = Arc::from(platform::system_registry());
        let elevated = platform::is_elevated();
        let languages = i18n::languages()
            .into_iter()
            .map(|l| (l.code, l.name))
            .collect();
        let mut app = App {
            ctx: cc.egui_ctx.clone(),
            logo: cc
                .egui_ctx
                .load_texture("logo", logo_image(), egui::TextureOptions::LINEAR),
            reg,
            store,
            settings,
            tab: Tab::Dashboard,
            status: String::new(),
            elevated,
            admin_prompt: !elevated,
            about_open: false,
            style_frames: 30,
            frame: 0,
            confirm: None,
            languages,
            programs: Default::default(),
            monitor: Default::default(),
            logs: Default::default(),
            backups: Default::default(),
            startup: Default::default(),
            cleaner: Default::default(),
            settings_ui: Default::default(),
            updates: Default::default(),
            wizard: None,
            removal: None,
        };
        app.reload_logs();
        app.reload_backups();
        app.refresh_programs(&cc.egui_ctx);
        app.monitor.resume_pending(&app.store);
        app
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
    }

    pub fn save_settings(&mut self) {
        if let Err(e) = self.store.save_settings(&self.settings) {
            self.set_status(tf(
                "Could not save settings: {error}",
                &[("error", e.to_string())],
            ));
        }
    }

    pub fn reload_logs(&mut self) {
        self.logs.logs = self.store.list_logs();
    }

    pub fn reload_backups(&mut self) {
        self.backups.list = expunge_core::backup::list_backups(&self.store.backups_dir());
    }

    pub fn ask(&mut self, title: &str, body: String, ok: &str, action: Confirm) {
        if !self.settings.confirm_actions {
            self.run_confirmed(action);
            return;
        }
        self.confirm = Some(ConfirmDialog {
            title: t(title),
            body,
            ok: t(ok),
            action,
        });
    }

    fn run_confirmed(&mut self, action: Confirm) {
        match action {
            Confirm::DeleteLog(id) => self.delete_log(&id),
            Confirm::RestoreBackup(id) => self.restore_backup(&id),
            Confirm::DeleteBackup(id) => self.delete_backup(&id),
            Confirm::CleanJunk => self.start_clean(),
            Confirm::DeleteStartup(i) => self.delete_startup(i),
            Confirm::DeleteTask(i) => self.delete_task(i),
            Confirm::ResetSettings => {
                let lang = self.settings.language.clone();
                self.settings = Settings {
                    language: lang,
                    ..Settings::default()
                };
                self.settings_ui.load(&self.settings);
                self.save_settings();
            }
            Confirm::CancelMonitoring => {
                self.monitor.cancel(&self.store);
            }
        }
    }

    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu")
            .frame(
                egui::Frame::new()
                    .fill(theme::SIDEBAR)
                    .inner_margin(egui::Margin::symmetric(8, 3)),
            )
            .show(ctx, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button(t("File"), |ui| {
                        if ui.button(t("Import install log…")).clicked() {
                            ui.close();
                            self.import_log();
                        }
                        if ui.button(t("Export program list (CSV)…")).clicked() {
                            ui.close();
                            self.export_program_list();
                        }
                        ui.separator();
                        if ui.button(t("Open data folder")).clicked() {
                            ui.close();
                            platform::shell_open(&self.store.root.to_string_lossy());
                        }
                        ui.separator();
                        if ui.button(t("Exit")).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.menu_button(t("Tools"), |ui| {
                        if ui.button(t("Install and monitor…")).clicked() {
                            ui.close();
                            self.tab = Tab::Monitor;
                            self.start_install_and_monitor(ctx);
                        }
                        if ui.button(t("Windows Updates…")).clicked() {
                            ui.close();
                            self.updates.open(ctx);
                            self.style_frames = 30;
                        }
                        if ui.button(t("Junk cleaner")).clicked() {
                            ui.close();
                            self.tab = Tab::Cleaner;
                        }
                        if ui.button(t("Create restore point")).clicked() {
                            ui.close();
                            self.programs.restore_job = Some(Job::spawn(ctx, |_| {
                                expunge_core::updates::create_restore_point(
                                    "Expunge manual restore point",
                                )
                                .map_err(|e| e.to_string())
                            }));
                        }
                        ui.separator();
                        ui.add_enabled_ui(!self.elevated, |ui| {
                            if ui.button(t("Restart as administrator")).clicked() {
                                ui.close();
                                self.relaunch_elevated(ctx);
                            }
                        });
                    });
                    ui.menu_button(t("View"), |ui| {
                        ui.menu_button(t("Language"), |ui| {
                            let mut changed = false;
                            changed |= ui
                                .radio_value(
                                    &mut self.settings.language,
                                    String::new(),
                                    t("Windows default"),
                                )
                                .clicked();
                            for (code, name) in self.languages.clone() {
                                changed |= ui
                                    .radio_value(&mut self.settings.language, code, name)
                                    .clicked();
                            }
                            if changed {
                                i18n::set_language(&self.settings.language);
                                self.save_settings();
                            }
                        });
                        ui.separator();
                        if ui.button(t("Zoom in")).clicked() {
                            self.zoom(ctx, 0.1);
                        }
                        if ui.button(t("Zoom out")).clicked() {
                            self.zoom(ctx, -0.1);
                        }
                        if ui.button(t("Reset zoom")).clicked() {
                            self.settings.zoom = 1.0;
                            ctx.set_zoom_factor(1.0);
                            self.save_settings();
                        }
                    });
                    ui.menu_button(t("Help"), |ui| {
                        if ui.button(t("Project page")).clicked() {
                            ui.close();
                            platform::shell_open(env!("CARGO_PKG_REPOSITORY"));
                        }
                        if ui.button(t("Report a problem")).clicked() {
                            ui.close();
                            platform::shell_open(&format!(
                                "{}/issues",
                                env!("CARGO_PKG_REPOSITORY")
                            ));
                        }
                        ui.separator();
                        if ui.button(t("About Expunge")).clicked() {
                            ui.close();
                            self.about_open = true;
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.elevated {
                            theme::badge(ui, t("Administrator"), theme::OK);
                        } else if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("🛡 {}", t("Not elevated")))
                                        .color(theme::WARN),
                                )
                                .frame(false),
                            )
                            .on_hover_text(t("Restart as administrator"))
                            .clicked()
                        {
                            self.admin_prompt = true;
                        }
                    });
                });
            });
    }

    fn zoom(&mut self, ctx: &egui::Context, delta: f32) {
        self.settings.zoom = (self.settings.zoom + delta).clamp(0.75, 2.0);
        ctx.set_zoom_factor(self.settings.zoom);
        self.save_settings();
    }

    pub fn relaunch_elevated(&mut self, ctx: &egui::Context) {
        if platform::relaunch_elevated() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            self.set_status(t("Elevation was cancelled."));
        }
    }

    fn nav_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("nav")
            .resizable(false)
            .exact_width(214.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::SIDEBAR)
                    .inner_margin(egui::Margin {
                        left: 10,
                        right: 10,
                        top: 14,
                        bottom: 10,
                    })
                    .stroke(egui::Stroke::new(1.0_f32, theme::LINE)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let tex = self.logo.clone();
                    ui.add(egui::Image::new(&tex).fit_to_exact_size(egui::vec2(28.0, 28.0)));
                    ui.label(
                        RichText::new("Expunge")
                            .size(19.0)
                            .strong()
                            .color(egui::Color32::WHITE),
                    );
                });
                ui.add_space(14.0);
                let monitoring = self.monitor.is_active();
                let broken = self.programs.broken.len();
                let items: [(Tab, &str, &str, Option<String>); 7] = [
                    (Tab::Dashboard, "🏠", "Dashboard", None),
                    (
                        Tab::Programs,
                        "📦",
                        "Programs",
                        (broken > 0).then(|| broken.to_string()),
                    ),
                    (
                        Tab::Monitor,
                        if monitoring { "⏺" } else { "📡" },
                        "Monitor",
                        monitoring.then(|| "REC".to_string()),
                    ),
                    (Tab::Logs, "📋", "Install logs", None),
                    (Tab::Backups, "💾", "Backups", None),
                    (Tab::Startup, "🚀", "Startup", None),
                    (Tab::Cleaner, "🗑", "Cleaner", None),
                ];
                for (tab, icon, label, badge) in items {
                    if theme::nav_item(ui, icon, &t(label), self.tab == tab, badge).clicked() {
                        self.tab = tab;
                        self.on_tab_opened(ctx, tab);
                    }
                }
                ui.add_space(14.0);
                ui.label(RichText::new(t("TOOLS")).small().color(theme::MUTED));
                ui.add_space(2.0);
                if theme::nav_item(ui, "🔁", &t("Windows Updates"), false, None).clicked() {
                    self.updates.open(ctx);
                    self.style_frames = 30;
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    if !self.elevated
                        && theme::nav_item(ui, "🛡", &t("Run as administrator"), false, None)
                            .clicked()
                    {
                        self.admin_prompt = true;
                    }
                    if theme::nav_item(ui, "⚙", &t("Settings"), self.tab == Tab::Settings, None)
                        .clicked()
                    {
                        self.tab = Tab::Settings;
                        self.on_tab_opened(ctx, Tab::Settings);
                    }
                });
            });
    }

    fn page_header(&mut self, ctx: &egui::Context) {
        let (title, sub) = match self.tab {
            Tab::Dashboard => ("Dashboard", "Your system at a glance"),
            Tab::Programs => (
                "Programs",
                "Uninstall programs completely, including everything they leave behind",
            ),
            Tab::Monitor => (
                "Monitor",
                "Record installations so they can be removed precisely",
            ),
            Tab::Logs => (
                "Install logs",
                "Everything each monitored installation changed",
            ),
            Tab::Backups => ("Backups", "Undo any removal, cleanup or startup change"),
            Tab::Startup => (
                "Startup",
                "Programs, services and tasks that start with Windows",
            ),
            Tab::Cleaner => ("Cleaner", "Free space taken by temporary files and caches"),
            Tab::Settings => ("Settings", ""),
        };
        egui::TopBottomPanel::top("page_header")
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin {
                        left: 18,
                        right: 18,
                        top: 10,
                        bottom: 0,
                    }),
            )
            .show(ctx, |ui| theme::page_title(ui, &t(title), &t(sub)));
    }

    pub fn on_tab_opened(&mut self, ctx: &egui::Context, tab: Tab) {
        match tab {
            Tab::Logs => self.reload_logs(),
            Tab::Backups => self.reload_backups(),
            Tab::Startup if !self.startup.loaded => self.refresh_startup(ctx),
            Tab::Settings => self.settings_ui.load(&self.settings),
            _ => {}
        }
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(theme::SIDEBAR)
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    small_muted(ui, &self.status);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        small_muted(
                            ui,
                            format!(
                                "Expunge {} · © {}",
                                expunge_core::APP_VERSION,
                                expunge_core::APP_AUTHOR
                            ),
                        );
                        if self.store.portable {
                            theme::badge(ui, t("Portable"), theme::MUTED);
                        }
                    });
                });
            });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.admin_prompt {
            let resp = egui::Modal::new(egui::Id::new("admin_prompt")).show(ctx, |ui| {
                ui.set_max_width(460.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("🛡").size(28.0).color(theme::WARN));
                    ui.heading(t("Administrator rights needed"));
                });
                ui.add_space(6.0);
                ui.label(t(
                    "Expunge is not running as administrator. It needs admin rights to work effectively: \
                     to monitor and undo installations system-wide, remove programs installed for all users, \
                     manage services and Windows updates, and clean protected folders.",
                ));
                ui.add_space(4.0);
                small_muted(ui, t("Without them, many changes will fail with \"access denied\"."));
                ui.add_space(10.0);
                let mut close = false;
                ui.horizontal(|ui| {
                    if theme::primary_button(ui, format!("🛡 {}", t("Relaunch as administrator"))).clicked() {
                        self.relaunch_elevated(ctx);
                        close = true;
                    }
                    if ui.button(t("Continue anyway")).clicked() {
                        close = true;
                    }
                });
                close
            });
            if resp.inner || resp.should_close() {
                self.admin_prompt = false;
            }
        }

        if let Some(c) = &self.confirm {
            let (title, body, ok) = (c.title.clone(), c.body.clone(), c.ok.clone());
            let resp = egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
                ui.set_max_width(440.0);
                ui.heading(title);
                ui.add_space(6.0);
                ui.label(body);
                ui.add_space(10.0);
                let mut choice = None;
                ui.horizontal(|ui| {
                    if theme::primary_button(ui, ok).clicked() {
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
                    let action = self.confirm.take().unwrap().action;
                    self.run_confirmed(action);
                }
                Some(false) => self.confirm = None,
                None if resp.should_close() => self.confirm = None,
                None => {}
            }
        }

        if self.about_open {
            let logo = self.logo.clone();
            egui::Window::new(t("About Expunge"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut self.about_open)
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        let tex = logo.clone();
                        ui.add(egui::Image::new(&tex).fit_to_exact_size(egui::vec2(96.0, 96.0)));
                        ui.heading(RichText::new("Expunge").color(theme::ACCENT).strong());
                        ui.label(tf(
                            "Version {n}",
                            &[("n", expunge_core::APP_VERSION.to_string())],
                        ));
                        ui.add_space(6.0);
                        ui.label(t("Install monitor and complete uninstaller for Windows."));
                        ui.label(format!("© 2026 {}", expunge_core::APP_AUTHOR));
                        ui.add_space(6.0);
                        small_muted(
                            ui,
                            t("Free software under the GNU General Public License v3."),
                        );
                        ui.hyperlink_to(t("Project page"), env!("CARGO_PKG_REPOSITORY"));
                    });
                });
        }
    }

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        self.poll_programs(ctx);
        self.poll_monitor(ctx);
        self.poll_startup();
        self.poll_cleaner();
        self.poll_backups();
        self.poll_removal();
        self.updates.poll();
        if let Some(job) = &self.programs.restore_job {
            if let Some(r) = job.poll() {
                self.programs.restore_job = None;
                self.set_status(match r {
                    Ok(()) => t("Restore point created."),
                    Err(e) => tf("Could not create a restore point: {error}", &[("error", e)]),
                });
            }
        }
    }

    fn import_log(&mut self) {
        let Some(path) = pick_file(&t("Import install log"), "Expunge log", &["json"]) else {
            return;
        };
        match self.store.import_log(&path) {
            Ok(log) => {
                self.set_status(tf("Imported {name}", &[("name", log.name)]));
                self.reload_logs();
                self.tab = Tab::Logs;
            }
            Err(e) => self.set_status(tf("Import failed: {error}", &[("error", format!("{e:#}"))])),
        }
    }

    fn export_program_list(&mut self) {
        let Some(path) = save_file(&t("Export program list"), "programs.csv", "CSV", "csv") else {
            return;
        };
        let mut all = self.programs.list.clone();
        all.extend(self.programs.store_apps.clone());
        match std::fs::write(&path, expunge_core::programs::to_csv(&all)) {
            Ok(()) => self.set_status(tf("Saved {path}", &[("path", path.display().to_string())])),
            Err(e) => self.set_status(e.to_string()),
        }
    }
}

pub fn logo_image() -> egui::ColorImage {
    let bytes = include_bytes!("../../assets/icon-256.png");
    match image::load_from_memory_with_format(bytes, image::ImageFormat::Png) {
        Ok(img) => {
            let img = img.into_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw())
        }
        Err(_) => egui::ColorImage::new([1, 1], vec![theme::ACCENT]),
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame += 1;
        if self.style_frames > 0 || self.frame.is_multiple_of(120) {
            platform::apply_window_style(theme::BORDER_RGB, true);
            self.style_frames = self.style_frames.saturating_sub(1);
        }
        self.poll_jobs(ctx);

        self.menu_bar(ctx);
        self.status_bar(ctx);
        self.nav_sidebar(ctx);
        self.page_header(ctx);
        match self.tab {
            Tab::Dashboard => self.ui_dashboard(ctx),
            Tab::Programs => self.ui_programs(ctx),
            Tab::Monitor => self.ui_monitor(ctx),
            Tab::Logs => self.ui_logs(ctx),
            Tab::Backups => self.ui_backups(ctx),
            Tab::Startup => self.ui_startup(ctx),
            Tab::Cleaner => self.ui_cleaner(ctx),
            Tab::Settings => self.ui_settings(ctx),
        }
        self.ui_wizard(ctx);
        self.ui_removal(ctx);
        let elevated = self.elevated;
        if self.updates.show(ctx, elevated) {
            self.style_frames = self.style_frames.max(5);
        }
        self.dialogs(ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [17.0 / 255.0, 17.0 / 255.0, 19.0 / 255.0, 1.0]
    }
}
