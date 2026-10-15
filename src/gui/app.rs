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
    Privacy,
    Disk,
    Dups,
    Lab,
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
    /// Turn an optional Windows feature on (true) or off.
    SetFeature(String, bool),
    /// Run one deep (Windows) cleanup.
    DeepClean(expunge_core::slimmer::DeepItem),
    /// Apply the ticked Anti-Telemetry changes.
    ApplyTweaks,
    /// Remove the ticked pre-installed apps.
    RemoveBloat,
    RemoveDuplicates,
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
    pub logo_small: egui::TextureHandle,
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
    pub extras: super::extras::ExtrasState,
    pub privacy: super::privacy::PrivacyState,
    pub disk: super::disk::DiskState,
    pub dups: super::dups::DupState,
    /// Live activity while monitoring an installation.
    pub live: Option<expunge_core::live::LiveWatcher>,
    /// Whether background monitoring is on (read from Task Scheduler once).
    pub auto_watch: Option<bool>,
    pub auto_watch_job: Option<super::Job<bool>>,
    pub health: Option<(expunge_core::health::Health, expunge_core::health::History)>,
    pub health_job:
        Option<super::Job<(expunge_core::health::Health, expunge_core::health::History)>>,
    /// Transfer export or restore window.
    pub transfer: Option<super::transfer_ui::TransferUi>,
    pub monitor: monitor::MonitorState,
    pub logs: logs::LogsState,
    pub backups: backups::BackupsState,
    /// A file operation (deleting a backup) running in the background.
    pub file_job: Option<Job<Result<(), String>>>,
    /// System-wide scan for registrations left by removed programs.
    pub orphan_job: Option<Job<expunge_core::revert::Plan>>,
    pub startup: startup_tab::StartupState,
    pub cleaner: cleaner::CleanerState,
    pub settings_ui: settings_tab::SettingsState,
    pub updates: updates::UpdatesState,
    pub lab: lab::LabState,
    pub help_open: bool,
    /// F10 was pressed: move keyboard focus to the menu bar.
    pub focus_menu: bool,
    /// Ctrl+Tab (false) or Ctrl+Shift+Tab (true), taken out of the input
    /// before egui sees it, so it switches pages instead of moving focus.
    pub ctrl_tab: Option<bool>,
    pub welcome_open: bool,
    pub focus_search: bool,
    /// Installer passed from Explorer's "Install with Expunge".
    pub pending_install: Option<String>,
    /// Arguments to pass on when relaunching as Administrator.
    pub launch_args: Vec<String>,
    pub wizard: Option<programs::Wizard>,
    pub removal: Option<plan::RemovalFlow>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        install_target: Option<String>,
        launch_args: Vec<String>,
    ) -> Self {
        let store = Store::open().unwrap_or_else(|_| {
            Store::at(&std::env::temp_dir().join("Expunge")).expect("data folder")
        });
        let settings = store.load_settings();
        let first_run_done = settings.first_run_done;
        theme::apply(&cc.egui_ctx, settings.high_contrast);
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
            logo_small: cc.egui_ctx.load_texture(
                "logo_small",
                logo_small_image(),
                egui::TextureOptions::LINEAR,
            ),
            reg,
            store,
            settings,
            tab: Tab::Dashboard,
            status: String::new(),
            elevated,
            admin_prompt: !elevated && first_run_done,
            about_open: false,
            style_frames: 30,
            frame: 0,
            confirm: None,
            languages,
            programs: Default::default(),
            extras: Default::default(),
            privacy: Default::default(),
            disk: Default::default(),
            dups: Default::default(),
            live: None,
            auto_watch: None,
            auto_watch_job: None,
            health: None,
            health_job: None,
            transfer: None,
            monitor: Default::default(),
            logs: Default::default(),
            backups: Default::default(),
            file_job: None,
            orphan_job: None,
            startup: Default::default(),
            cleaner: Default::default(),
            settings_ui: Default::default(),
            updates: Default::default(),
            lab: Default::default(),
            help_open: false,
            focus_menu: false,
            ctrl_tab: None,
            welcome_open: false,
            focus_search: false,
            pending_install: install_target,
            launch_args,
            wizard: None,
            removal: None,
        };
        app.programs.cache = std::sync::Arc::new(std::sync::Mutex::new(
            expunge_core::cache::Cache::load(&app.store.cache_path()),
        ));
        app.reload_logs();
        app.reload_backups();
        app.refresh_programs(&cc.egui_ctx);
        app.monitor.resume_pending(&app.store);
        app.welcome_open = !app.settings.first_run_done;
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

    /// Re-read install logs from disk in the background.
    pub fn reload_logs(&mut self) {
        let store = self.store.clone();
        self.logs
            .loader
            .request(&self.ctx, move || store.list_logs());
    }

    /// Re-read backups from disk in the background.
    pub fn reload_backups(&mut self) {
        let dir = self.store.backups_dir();
        self.backups
            .loader
            .request(&self.ctx, move || expunge_core::backup::list_backups(&dir));
    }

    /// Start the system-wide orphan scan.
    pub fn scan_orphans(&mut self) {
        if self.orphan_job.is_some() {
            return;
        }
        let reg = self.reg.clone();
        self.set_status(t("Looking for leftovers of removed programs…"));
        self.orphan_job = Some(Job::spawn(&self.ctx, move |_| {
            expunge_core::orphans::scan_system(reg.as_ref())
        }));
    }

    fn poll_orphans(&mut self) {
        let Some(plan) = self.orphan_job.as_ref().and_then(|j| j.poll()) else {
            return;
        };
        self.orphan_job = None;
        if plan.items.is_empty() {
            self.set_status(t("No leftovers of removed programs found."));
            return;
        }
        let n = plan.items.len();
        self.set_status(tf(
            "Found {n} leftovers of removed programs.",
            &[("n", n.to_string())],
        ));
        let mut flow =
            super::plan::RemovalFlow::new(t("Leftovers of removed programs"), plan, false, false);
        flow.plan.info.push(t(
            "These registrations point to files or folders that no longer exist. Windows' own entries are never included.",
        ));
        self.open_removal(flow);
    }

    fn poll_lists(&mut self) {
        if let Some((list, again)) = self.logs.loader.poll() {
            self.logs.stats = list
                .iter()
                .map(|l| (l.id.clone(), l.changes.stats()))
                .collect();
            self.logs.logs = list;
            if again {
                self.reload_logs();
            }
        }
        if let Some((list, again)) = self.backups.loader.poll() {
            self.backups.list = list;
            if again {
                self.reload_backups();
            }
        }
        if let Some(r) = self.file_job.as_ref().and_then(|j| j.poll()) {
            self.file_job = None;
            if let Err(e) = r {
                self.set_status(e);
            }
            self.reload_logs();
            self.reload_backups();
        }
    }

    pub fn ask(&mut self, title: &str, body: String, ok: &str, action: Confirm) {
        if !self.settings.confirm_enabled() {
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
            Confirm::SetFeature(name, enable) => self.set_feature(name, enable),
            Confirm::DeepClean(item) => self.start_deep(item),
            Confirm::ApplyTweaks => self.start_apply_tweaks(false),
            Confirm::RemoveBloat => self.start_remove_bloat(),
            Confirm::RemoveDuplicates => {
                let ctx = self.ctx.clone();
                self.start_remove_duplicates(&ctx)
            }
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
                    let file = ui.menu_button(t("File"), |ui| {
                        if ui.button(t("Restore programs from a package…")).clicked() {
                            ui.close();
                            self.start_transfer_import();
                        }
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
                    if std::mem::take(&mut self.focus_menu) {
                        file.response.request_focus();
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(egui::Id::new("expunge_keyboard_mode"), true)
                        });
                    }
                    ui.menu_button(t("Tools"), |ui| {
                        if ui.button(t("Install and monitor…")).clicked() {
                            ui.close();
                            self.tab = Tab::Monitor;
                            self.start_install_and_monitor(ctx);
                        }
                        if self.settings.advanced_mode && ui.button(t("Windows Updates…")).clicked()
                        {
                            ui.close();
                            self.updates.open(ctx);
                            self.style_frames = 30;
                        }
                        if ui.button(t("Junk cleaner")).clicked() {
                            ui.close();
                            self.tab = Tab::Cleaner;
                        }
                        if self.settings.advanced_mode
                            && ui
                                .button(t("Find leftovers of removed programs…"))
                                .clicked()
                        {
                            ui.close();
                            self.scan_orphans();
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
                            if ui.button(t("Restart as Administrator")).clicked() {
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
                            .on_hover_text(t("Restart as Administrator"))
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
        if platform::relaunch_elevated(&self.launch_args) {
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
                    let tex = self.logo_small.clone();
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
                let items: [(Tab, &str, &str, Option<String>); 8] = [
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
                    (Tab::Privacy, "🔒", "Anti-Telemetry", None),
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
                if theme::nav_item(ui, "📊", &t("Disk space"), self.tab == Tab::Disk, None)
                    .clicked()
                {
                    self.tab = Tab::Disk;
                }
                if theme::nav_item(ui, "📑", &t("Duplicate files"), self.tab == Tab::Dups, None)
                    .clicked()
                {
                    self.tab = Tab::Dups;
                }
                if self.settings.advanced_mode
                    && theme::nav_item(ui, "🔁", &t("Windows Updates"), false, None).clicked()
                {
                    self.updates.open(ctx);
                    self.style_frames = 30;
                }
                if self.settings.advanced_mode
                    && theme::nav_item(ui, "🛠", &t("Installer Lab"), self.tab == Tab::Lab, None)
                        .clicked()
                {
                    self.tab = Tab::Lab;
                    self.reload_logs();
                    self.on_lab_opened();
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    if !self.elevated
                        && theme::nav_item(ui, "🛡", &t("Run as Administrator"), false, None)
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
            Tab::Privacy => ("Anti-Telemetry", "Stop Windows collecting data, remove clutter, and tune Windows — every change can be undone"),
            Tab::Disk => (
                "Disk space",
                "See which folders and files take up the most space",
            ),
            Tab::Dups => (
                "Duplicate files",
                "Find identical copies of files and free the space they waste",
            ),
            Tab::Lab => (
                "Installer Lab",
                "Tools for developers: lint installers, test uninstallers, compare snapshots",
            ),
            Tab::Settings => ("Settings", ""),
        };
        let help = page_help(self.tab);
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
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| theme::page_title(ui, &t(title), &t(sub)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.add_space(4.0);
                        let label = if self.help_open {
                            format!("✖ {}", t("Close help"))
                        } else {
                            format!("❓ {}", t("Help"))
                        };
                        if ui
                            .button(label)
                            .on_hover_text(t("What is this page for?"))
                            .clicked()
                        {
                            self.help_open = !self.help_open;
                        }
                    });
                });
                if self.help_open {
                    theme::card(ui, |ui| {
                        for line in help {
                            ui.label(t(line));
                        }
                    });
                    ui.add_space(8.0);
                }
            });
    }

    pub fn on_tab_opened(&mut self, ctx: &egui::Context, tab: Tab) {
        match tab {
            Tab::Logs => self.reload_logs(),
            Tab::Backups => self.reload_backups(),
            Tab::Startup if !self.startup.loaded => self.refresh_startup(ctx),
            Tab::Settings => self.settings_ui.load(&self.settings),
            Tab::Lab => self.on_lab_opened(),
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
                    "Expunge is not running as Administrator. It needs admin rights to work effectively: \
                     to monitor and undo installations system-wide, remove programs installed for all users, \
                     manage services and Windows updates, and clean protected folders.",
                ));
                ui.add_space(4.0);
                small_muted(ui, t("Without them, many changes will fail with \"access denied\"."));
                ui.add_space(10.0);
                let mut close = false;
                ui.horizontal(|ui| {
                    let r = theme::primary_button(ui, format!("🛡 {}", t("Relaunch as Administrator")));
                    if ui.memory(|m| m.focused().is_none()) {
                        r.request_focus();
                    }
                    if r.clicked() {
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
                    let r = theme::primary_button(ui, ok);
                    // Keyboard users start on the confirm button: Enter accepts,
                    // Tab moves to Cancel, Esc cancels.
                    if ui.memory(|m| m.focused().is_none()) {
                        r.request_focus();
                    }
                    if r.clicked() {
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

        if self.about_open && super::a11y::escape_pressed(ctx) {
            self.about_open = false;
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
                        ui.heading(RichText::new("Expunge").color(theme::ACCENT_TEXT).strong());
                        ui.label(tf(
                            "Version {n}",
                            &[("n", expunge_core::APP_VERSION.to_string())],
                        ));
                        ui.add_space(6.0);
                        ui.label(t("Open Source Install Monitor & Uninstaller"));
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
        self.poll_lists();
        self.poll_orphans();
        self.poll_repackage();
        self.poll_extras(ctx);
        self.poll_programs(ctx);
        self.poll_monitor(ctx);
        self.poll_startup();
        self.poll_cleaner();
        self.poll_learning();
        self.poll_backups();
        self.poll_removal();
        self.updates.poll();
        self.poll_lab();
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

/// Large logo (dark card, red E) for the About box and welcome screen.
pub fn logo_image() -> egui::ColorImage {
    decode_png(include_bytes!("../../assets/icon-256.png"))
}

/// Small logo (red card, black E), legible at sidebar/taskbar size.
pub fn logo_small_image() -> egui::ColorImage {
    decode_png(include_bytes!("../../assets/icon-taskbar.png"))
}

fn decode_png(bytes: &[u8]) -> egui::ColorImage {
    match image::load_from_memory_with_format(bytes, image::ImageFormat::Png) {
        Ok(img) => {
            let img = img.into_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw())
        }
        Err(_) => egui::ColorImage::new([1, 1], vec![theme::ACCENT]),
    }
}

/// Plain-language explanation of each page, shown by the Help button.
fn page_help(tab: Tab) -> &'static [&'static str] {
    match tab {
        Tab::Dashboard => &[
            "The dashboard shows how many programs are installed, which ones Expunge recorded, and anything that needs attention.",
            "Click a tile or a quick action to jump straight to it.",
        ],
        Tab::Programs => &[
            "This is the list of programs installed on this PC. Select one to see its details, or double-click it to uninstall.",
            "Uninstall first runs the program's own uninstaller, then Expunge looks for anything left behind (leftovers) and shows you before removing it.",
            "Tick several programs to uninstall them one after another. \"Broken\" lists entries whose files are already gone.",
            "Everything Expunge removes is backed up first, so you can undo it on the Backups page.",
        ],
        Tab::Monitor => &[
            "Monitoring means Expunge takes a \"before\" picture of your system, you install a program, and Expunge takes an \"after\" picture.",
            "The difference is exactly what the installation added or changed. Expunge saves it as an install log.",
            "Later, Expunge can use that log to remove the program completely and precisely — even things its own uninstaller would forget.",
        ],
        Tab::Logs => &[
            "Each install log lists everything one monitored installation added or changed: files, folders, registry entries, services and tasks.",
            "Use \"Which program created this?\" to find out which program a file or registry entry belongs to.",
            "You can export a log as a readable report, or as scripts that undo the installation on another PC.",
        ],
        Tab::Backups => &[
            "Before Expunge deletes or changes anything, it copies it here.",
            "If something stops working after a removal, click Restore to put everything back exactly as it was.",
        ],
        Tab::Startup => &[
            "These programs start automatically when Windows starts. Fewer startup programs means Windows starts faster.",
            "Turning an entry off is safe and reversible: it stays in the list and you can turn it back on.",
            "In Advanced mode you can also manage services (background programs) and scheduled tasks.",
        ],
        Tab::Cleaner => &[
            "Programs and Windows leave temporary files behind. They are safe to delete and are recreated when needed.",
            "Click Scan to see how much space each kind uses, choose what to clean, then click Clean.",
        ],
        Tab::Privacy => &[
            "Windows sends data about how you use your PC to Microsoft and shows ads and suggestions. These switches turn that off.",
            "Pick a preset (Minimal is safe for everyone) or tick individual items, then click Apply. A restore point is created first.",
            "Every change is recorded: untick an item and apply again to undo it exactly.",
        ],
        Tab::Disk => &[
            "The map shows a drive or folder as blocks: the bigger the block, the more space it uses. Colors show the kind of file.",
            "Click a block or double-click a list entry to look inside a folder. Use the arrow button or Backspace to go back up.",
            "Nothing is deleted here. Use Show in File Explorer to deal with large files yourself, or the Programs and Cleaner pages to remove programs and junk safely.",
        ],
        Tab::Dups => &[
            "Expunge compares the contents of files, not just their names, so only truly identical files are listed.",
            "Tick the copies you don't need, or let Expunge tick all but one copy of each. They go to the Recycle Bin, so you can restore them.",
            "Windows, program and app-data folders are never searched, so nothing a program needs can be removed.",
        ],
        Tab::Lab => &[
            "Installer lint reviews a recorded installation and points out packaging problems, with advice on fixing them.",
            "Uninstaller test runs a program's own uninstaller and reports exactly what it left behind — useful for testing your own installers.",
            "Snapshot compare lets you capture the system at any moment and compare any two captures. Everything is also available from the command line (expunge --help).",
        ],
        Tab::Settings => &[
            "Simple mode keeps the interface focused and plain. Advanced mode shows technical details, services, tasks and the Installer Lab.",
            "The monitoring scope decides which parts of the system snapshots look at. The defaults suit almost everyone.",
        ],
    }
}

impl App {
    /// First-run welcome: language, mode, and what Expunge does.
    fn welcome(&mut self, ctx: &egui::Context) {
        if !self.welcome_open {
            return;
        }
        let mut done = false;
        egui::Modal::new(egui::Id::new("welcome")).show(ctx, |ui| {
            ui.set_max_width(560.0);
            ui.horizontal(|ui| {
                ui.add(egui::Image::new(&self.logo).fit_to_exact_size(egui::vec2(48.0, 48.0)));
                ui.vertical(|ui| {
                    ui.heading(RichText::new(t("Welcome to Expunge")).strong());
                    small_muted(ui, t("Remove programs completely — and only what belongs to them."));
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(t("Language"));
                let current = self
                    .languages
                    .iter()
                    .find(|(c, _)| *c == self.settings.language)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| t("Windows default"));
                egui::ComboBox::from_id_salt("welcome_lang").selected_text(current).show_ui(ui, |ui| {
                    let mut changed = ui.selectable_value(&mut self.settings.language, String::new(), t("Windows default")).changed();
                    for (code, name) in &self.languages {
                        changed |= ui.selectable_value(&mut self.settings.language, code.clone(), name).changed();
                    }
                    if changed {
                        i18n::set_language(&self.settings.language);
                    }
                });
            });
            ui.add_space(10.0);
            for (icon, title, body) in [
                ("🗑", "Uninstall completely", "Remove programs plus the leftovers their uninstallers forget."),
                ("📡", "Monitor installations", "Record what an installer changes so it can be undone precisely later."),
                ("💾", "Undo anything", "Everything removed is backed up first and can be restored."),
            ] {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon).size(20.0).color(theme::ACCENT_TEXT));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(t(title)).strong());
                        small_muted(ui, t(body));
                    });
                });
                ui.add_space(4.0);
            }
            ui.add_space(8.0);
            ui.label(RichText::new(t("How do you want Expunge to look?")).strong());
            ui.columns(2, |cols| {
                for (i, (adv, title, body)) in [
                    (false, "Simple", "Plain language, fewer technical details. Recommended if you're not sure."),
                    (true, "Advanced", "All details: registry keys, services, scheduled tasks and the Installer Lab for developers."),
                ]
                .into_iter()
                .enumerate()
                {
                    let selected = self.settings.advanced_mode == adv;
                    let resp = egui::Frame::new()
                        .fill(if selected { theme::NAV_SELECTED } else { theme::CARD })
                        .corner_radius(12)
                        .inner_margin(12)
                        .show(&mut cols[i], |ui| {
                            ui.set_width(ui.available_width());
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.label(RichText::new(t(title)).strong());
                                small_muted(ui, t(body));
                            });
                        })
                        .response
                        .interact(egui::Sense::click());
                    super::a11y::selectable(&resp, &format!("{}. {}", t(title), t(body)), selected);
                    if resp.clicked() {
                        self.settings.advanced_mode = adv;
                    }
                }
            });
            ui.add_space(10.0);
            small_muted(ui, t("You can change both at any time in Settings. Press F1 on any page for help."));
            ui.add_space(6.0);
            let start = theme::wide_button(ui, t("Get started"), true);
            if ui.memory(|m| m.focused().is_none()) {
                start.request_focus();
            }
            if start.clicked() {
                done = true;
            }
        });
        if done {
            self.welcome_open = false;
            self.settings.first_run_done = true;
            self.save_settings();
            self.admin_prompt = !self.elevated;
        }
    }

    /// Start monitoring an installer handed over by Explorer, once the
    /// welcome and admin dialogs are out of the way.
    fn start_pending_install(&mut self, ctx: &egui::Context) {
        if self.pending_install.is_none() || self.welcome_open || self.admin_prompt {
            return;
        }
        let path = self.pending_install.take().unwrap();
        self.tab = Tab::Monitor;
        if self.monitor.is_active() {
            self.set_status(t(
                "Another installation is already being monitored. Finish or cancel it first.",
            ));
            return;
        }
        let name = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        self.begin_monitoring(ctx, name, path);
    }

    /// Keyboard shortcuts.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        let tabs = [
            Tab::Dashboard,
            Tab::Programs,
            Tab::Monitor,
            Tab::Logs,
            Tab::Backups,
            Tab::Startup,
            Tab::Cleaner,
            Tab::Privacy,
            Tab::Settings,
        ];
        let mut go: Option<Tab> = None;
        ctx.input_mut(|i| {
            for (n, key) in [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
                Key::Num9,
            ]
            .iter()
            .enumerate()
            {
                if i.consume_key(Modifiers::CTRL, *key) {
                    go = Some(tabs[n]);
                }
            }
            if i.consume_key(Modifiers::CTRL, Key::F) {
                self.focus_search = true;
            }
            if i.consume_key(Modifiers::NONE, Key::F1) {
                self.help_open = !self.help_open;
            }
            if i.consume_key(Modifiers::NONE, Key::F10) {
                self.focus_menu = true;
            }
            let ctrl_tab = self.ctrl_tab.take();
            if let Some(back) = ctrl_tab {
                let mut order = tabs.to_vec();
                order.insert(8, Tab::Disk);
                order.insert(9, Tab::Dups);
                if self.settings.advanced_mode {
                    order.insert(10, Tab::Lab);
                }
                let pos = order.iter().position(|t| *t == self.tab).unwrap_or(0);
                let n = order.len();
                let next = if back {
                    (pos + n - 1) % n
                } else {
                    (pos + 1) % n
                };
                go = Some(order[next]);
            }
        });
        if self.help_open
            && self.confirm.is_none()
            && self.wizard.is_none()
            && self.removal.is_none()
            && !self.about_open
            && super::a11y::escape_pressed(ctx)
        {
            self.help_open = false;
        }
        if let Some(tab) = go {
            self.tab = tab;
            self.on_tab_opened(ctx, tab);
        }
        let typing = ctx.wants_keyboard_input();
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F5)) {
            match self.tab {
                Tab::Programs | Tab::Dashboard => self.refresh_programs(ctx),
                Tab::Startup => self.refresh_startup(ctx),
                Tab::Logs => self.reload_logs(),
                Tab::Backups => self.reload_backups(),
                _ => {}
            }
        }
        if !typing
            && self.tab == Tab::Programs
            && self.wizard.is_none()
            && self.removal.is_none()
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete))
        {
            if let Some(key) = self.programs.selected.clone() {
                let all: Vec<_> = self
                    .programs
                    .list
                    .iter()
                    .chain(self.programs.store_apps.iter())
                    .filter(|p| p.key == key)
                    .cloned()
                    .collect();
                self.start_uninstall(all, false);
            }
        }
    }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.events.retain(|e| match e {
            egui::Event::Key {
                key: egui::Key::Tab,
                pressed,
                modifiers,
                ..
            } if modifiers.ctrl => {
                if *pressed {
                    self.ctrl_tab = Some(modifiers.shift);
                }
                false
            }
            _ => true,
        });
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame += 1;
        super::a11y::track_input_mode(ctx);
        // Ctrl+Plus / Ctrl+Minus / Ctrl+0 zoom (built into egui): remember it.
        let z = ctx.zoom_factor();
        if (z - self.settings.zoom).abs() > 0.001 && !ctx.input(|i| i.pointer.any_down()) {
            self.settings.zoom = z.clamp(0.75, 2.0);
            if (self.settings.zoom - z).abs() > 0.001 {
                ctx.set_zoom_factor(self.settings.zoom);
            }
            self.save_settings();
        }
        if self.style_frames > 0 || self.frame.is_multiple_of(120) {
            platform::apply_window_style(theme::BORDER_RGB, true);
            self.style_frames = self.style_frames.saturating_sub(1);
        }
        self.poll_jobs(ctx);
        self.start_pending_install(ctx);

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
            Tab::Privacy => self.ui_privacy(ctx),
            Tab::Disk => self.ui_disk(ctx),
            Tab::Dups => self.ui_dups(ctx),
            Tab::Lab => self.ui_lab(ctx),
            Tab::Settings => self.ui_settings(ctx),
        }
        self.ui_wizard(ctx);
        self.ui_removal(ctx);
        self.ui_transfer(ctx);
        let elevated = self.elevated;
        if self.updates.show(ctx, elevated) {
            self.style_frames = self.style_frames.max(5);
        }
        self.dialogs(ctx);
        self.welcome(ctx);
        self.shortcuts(ctx);
        super::a11y::focus_ring(ctx, theme::ACCENT_TEXT);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [17.0 / 255.0, 17.0 / 255.0, 19.0 / 255.0, 1.0]
    }
}
