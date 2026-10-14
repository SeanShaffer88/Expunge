//! Programs tab: installed programs, details, single and batch uninstall.

use super::app::App;
use super::plan::RemovalFlow;
use super::theme;
use super::widgets::*;
use super::Job;
use egui::RichText;
use egui_extras::{Column, TableBuilder};
use expunge_core::i18n::{t, tf};
use expunge_core::leftovers::{find_leftovers, LeftoverScope};
use expunge_core::model::{fold, human_size, InstallLog};
use expunge_core::platform::{self, ChildProcess};
use expunge_core::programs::{self, InstalledProgram, Source};
use expunge_core::revert::{build_plan, Action, Plan, PlanItem};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    All,
    Monitored,
    Broken,
    Store,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SortCol {
    #[default]
    Name,
    Publisher,
    Version,
    Date,
    Size,
}

#[derive(Default)]
pub struct ProgramsState {
    pub list: Vec<InstalledProgram>,
    pub store_apps: Vec<InstalledProgram>,
    pub broken: HashSet<String>,
    pub sizes: HashMap<String, u64>,
    pub job: Option<Job<(Vec<InstalledProgram>, HashSet<String>)>>,
    pub store_job: Option<Job<Vec<InstalledProgram>>>,
    pub size_job: Option<Job<HashMap<String, u64>>>,
    /// Program icons by registry key (loaded in the background).
    pub icons: HashMap<String, egui::TextureHandle>,
    pub icon_job: Option<Job<Vec<(String, egui::ColorImage)>>>,
    pub icons_requested: HashSet<String>,
    pub restore_job: Option<Job<Result<(), String>>>,
    pub filter: String,
    pub view: View,
    pub sort: SortCol,
    pub ascending: bool,
    pub selected: Option<String>,
    pub checked: BTreeSet<String>,
    pub needs_refresh: bool,
    pub sorted_once: bool,
}

#[derive(PartialEq, Eq)]
enum Stage {
    Confirm,
    RestorePoint,
    Running,
    Scanning,
}

pub struct Wizard {
    pub programs: Vec<InstalledProgram>,
    /// Skip the vendor uninstaller and go straight to cleanup.
    pub force: bool,
    quiet: bool,
    restore_point: bool,
    stage: Stage,
    index: usize,
    launched: bool,
    child: Option<ChildProcess>,
    started: Option<Instant>,
    notes: Vec<String>,
    rp_job: Option<Job<Result<(), String>>>,
    store_job: Option<Job<Result<String, String>>>,
    scan_job: Option<Job<(Plan, Vec<String>, bool)>>,
}

impl Wizard {
    pub fn new(programs: Vec<InstalledProgram>, force: bool, restore_point: bool) -> Self {
        Wizard {
            programs,
            force,
            quiet: false,
            restore_point,
            stage: Stage::Confirm,
            index: 0,
            launched: false,
            child: None,
            started: None,
            notes: Vec::new(),
            rp_job: None,
            store_job: None,
            scan_job: None,
        }
    }

    fn advance(&mut self) {
        self.index += 1;
        self.launched = false;
        self.child = None;
        self.started = None;
        self.store_job = None;
    }
}

/// Merge plan items, dropping duplicates.
pub fn merge_plans(into: &mut Plan, other: Plan) {
    let mut seen: HashSet<String> = into
        .items
        .iter()
        .map(|i| fold(&i.action.describe()))
        .collect();
    for item in other.items {
        if seen.insert(fold(&item.action.describe())) {
            into.items.push(item);
        }
    }
    into.info.extend(other.info);
}

pub fn log_for<'a>(logs: &'a [InstallLog], p: &InstalledProgram) -> Option<&'a InstallLog> {
    logs.iter()
        .find(|l| !l.uninstall_key.is_empty() && fold(&l.uninstall_key) == fold(&p.key))
        .or_else(|| logs.iter().find(|l| fold(&l.name) == fold(&p.name)))
}

impl App {
    pub fn refresh_programs(&mut self, ctx: &egui::Context) {
        let reg = self.reg.clone();
        let hidden = self.settings.show_system_components;
        self.programs.job = Some(Job::spawn(ctx, move |_| {
            let list = programs::list_programs(reg.as_ref(), hidden);
            let broken = list
                .iter()
                .filter(|p| p.is_broken())
                .map(|p| p.key.clone())
                .collect();
            (list, broken)
        }));
        if self.settings.show_store_apps {
            self.programs.store_job = Some(Job::spawn(ctx, |_| programs::list_store_apps()));
        } else {
            self.programs.store_apps.clear();
        }
        self.programs.needs_refresh = false;
    }

    pub fn poll_programs(&mut self, ctx: &egui::Context) {
        if self.programs.needs_refresh && self.removal.as_ref().is_none_or(|r| r.job.is_none()) {
            self.refresh_programs(ctx);
        }
        if let Some((list, broken)) = self.programs.job.as_ref().and_then(|j| j.poll()) {
            self.programs.job = None;
            self.programs.list = list;
            self.programs.broken = broken;
            let n = self.programs.list.len();
            self.set_status(tf("{n} programs installed", &[("n", n.to_string())]));
            self.load_icons(ctx);
            // Measure folders for programs that don't report their size.
            let todo: Vec<(String, String)> = self
                .programs
                .list
                .iter()
                .filter(|p| {
                    p.size_kb == 0
                        && !p.install_location.is_empty()
                        && !self.programs.sizes.contains_key(&p.key)
                })
                .map(|p| (p.key.clone(), p.install_location.clone()))
                .collect();
            if !todo.is_empty() {
                let protected = expunge_core::util::protected_dirs();
                self.programs.size_job = Some(Job::spawn(ctx, move |pr| {
                    let mut m = HashMap::new();
                    for (k, loc) in todo {
                        if pr.cancelled() {
                            break;
                        }
                        if !expunge_core::util::is_protected_dir(&loc, &protected)
                            && std::path::Path::new(&loc).is_dir()
                        {
                            m.insert(k, programs::dir_size(&loc));
                        }
                    }
                    m
                }));
            }
        }
        if let Some(apps) = self.programs.store_job.as_ref().and_then(|j| j.poll()) {
            self.programs.store_job = None;
            self.programs.store_apps = apps;
        }
        if let Some(sizes) = self.programs.size_job.as_ref().and_then(|j| j.poll()) {
            self.programs.size_job = None;
            self.programs.sizes.extend(sizes);
        }
        if let Some(icons) = self.programs.icon_job.as_ref().and_then(|j| j.poll()) {
            self.programs.icon_job = None;
            for (key, img) in icons {
                let tex =
                    ctx.load_texture(format!("icon:{key}"), img, egui::TextureOptions::LINEAR);
                self.programs.icons.insert(key, tex);
            }
        }
    }

    /// Extract icons for programs we haven't seen yet, off the UI thread.
    fn load_icons(&mut self, ctx: &egui::Context) {
        if self.programs.icon_job.is_some() {
            return;
        }
        let todo: Vec<(String, String, i32)> = self
            .programs
            .list
            .iter()
            .filter(|p| !self.programs.icons_requested.contains(&p.key))
            .filter_map(|p| {
                p.icon_source()
                    .map(|(path, idx)| (p.key.clone(), path, idx))
            })
            .collect();
        for p in &self.programs.list {
            self.programs.icons_requested.insert(p.key.clone());
        }
        if todo.is_empty() {
            return;
        }
        self.programs.icon_job = Some(Job::spawn(ctx, move |_| {
            todo.into_iter()
                .filter_map(|(key, path, idx)| {
                    let (w, h, rgba) = platform::icon_rgba(&path, idx, 32)?;
                    Some((
                        key,
                        egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                    ))
                })
                .collect()
        }));
    }

    fn size_of(&self, p: &InstalledProgram) -> u64 {
        if p.size_kb > 0 {
            p.size_kb * 1024
        } else {
            self.programs.sizes.get(&p.key).copied().unwrap_or(0)
        }
    }

    fn visible_programs(&self) -> Vec<InstalledProgram> {
        let st = &self.programs;
        let f = st.filter.to_lowercase();
        let source: Vec<&InstalledProgram> = match st.view {
            View::Store => st.store_apps.iter().collect(),
            _ => st.list.iter().collect(),
        };
        let mut v: Vec<InstalledProgram> = source
            .into_iter()
            .filter(|p| match st.view {
                View::Monitored => log_for(&self.logs.logs, p).is_some(),
                View::Broken => st.broken.contains(&p.key),
                _ => true,
            })
            .filter(|p| {
                f.is_empty()
                    || p.name.to_lowercase().contains(&f)
                    || p.publisher.to_lowercase().contains(&f)
            })
            .cloned()
            .collect();
        match st.sort {
            SortCol::Name => v.sort_by_key(|p| p.name.to_lowercase()),
            SortCol::Publisher => v.sort_by_key(|p| p.publisher.to_lowercase()),
            SortCol::Version => v.sort_by(|a, b| a.version.cmp(&b.version)),
            SortCol::Date => v.sort_by(|a, b| a.install_date.cmp(&b.install_date)),
            SortCol::Size => v.sort_by_key(|p| self.size_of(p)),
        }
        if !st.ascending {
            v.reverse();
        }
        v
    }

    pub fn start_uninstall(&mut self, programs: Vec<InstalledProgram>, force: bool) {
        if programs.is_empty() {
            return;
        }
        self.wizard = Some(Wizard::new(
            programs,
            force,
            self.settings.create_restore_point,
        ));
    }

    pub fn ui_programs(&mut self, ctx: &egui::Context) {
        if !self.programs.sorted_once {
            self.programs.ascending = true;
            self.programs.sorted_once = true;
        }
        let visible = self.visible_programs();
        let selected = self
            .programs
            .selected
            .as_ref()
            .and_then(|k| visible.iter().find(|p| &p.key == k).cloned());

        egui::SidePanel::right("program_details")
            .frame(theme::side_frame())
            .show_separator_line(false)
            .resizable(true)
            .default_width(310.0)
            .min_width(260.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| match &selected {
                    Some(p) => self.program_details(ui, p),
                    None => {
                        ui.add_space(30.0);
                        ui.vertical_centered(|ui| {
                            muted(ui, t("Select a program to see its details."));
                        });
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(theme::content_frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let r = search_box(ui, &mut self.programs.filter, "Search programs", 260.0);
                    if self.focus_search {
                        r.request_focus();
                        self.focus_search = false;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(format!("⟳ {}", t("Refresh"))).clicked() {
                            self.refresh_programs(ctx);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    let views = [
                        (View::All, t("All")),
                        (View::Monitored, t("Monitored")),
                        (
                            View::Broken,
                            tf(
                                "Broken ({n})",
                                &[("n", self.programs.broken.len().to_string())],
                            ),
                        ),
                        (View::Store, t("Store apps")),
                    ];
                    for (v, label) in views {
                        if ui
                            .selectable_label(self.programs.view == v, label)
                            .clicked()
                        {
                            self.programs.view = v;
                            if v == View::Store
                                && self.programs.store_apps.is_empty()
                                && self.programs.store_job.is_none()
                            {
                                self.programs.store_job =
                                    Some(Job::spawn(ctx, |_| programs::list_store_apps()));
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let n = self.programs.checked.len();
                        if n > 0 {
                            if theme::primary_button(
                                ui,
                                tf("Uninstall {n} selected", &[("n", n.to_string())]),
                            )
                            .clicked()
                            {
                                let mut all = self.programs.list.clone();
                                all.extend(self.programs.store_apps.clone());
                                let chosen: Vec<InstalledProgram> = all
                                    .into_iter()
                                    .filter(|p| self.programs.checked.contains(&p.key))
                                    .collect();
                                self.start_uninstall(chosen, false);
                            }
                            if ui.small_button(t("Clear")).clicked() {
                                self.programs.checked.clear();
                            }
                        }
                    });
                });
                ui.add_space(4.0);
                if self.programs.job.is_some()
                    || (self.programs.view == View::Store && self.programs.store_job.is_some())
                {
                    job_progress(ui, &t("Reading installed programs…"), 0);
                    return;
                }
                if visible.is_empty() {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| muted(ui, t("No programs match.")));
                    return;
                }
                self.programs_table(ui, &visible);
            });
    }

    fn programs_table(&mut self, ui: &mut egui::Ui, visible: &[InstalledProgram]) {
        let row_h = 24.0;
        let mut clicked: Option<String> = None;
        let mut double: Option<InstalledProgram> = None;
        let mut sort_click: Option<SortCol> = None;
        let logs = &self.logs.logs;
        let st = &mut self.programs;
        let sizes = &st.sizes;
        let size_of = |p: &InstalledProgram| {
            if p.size_kb > 0 {
                p.size_kb * 1024
            } else {
                sizes.get(&p.key).copied().unwrap_or(0)
            }
        };
        let header =
            |ui: &mut egui::Ui, label: &str, col: SortCol, cur: SortCol, asc: bool| -> bool {
                let arrow = if col == cur {
                    if asc {
                        " ⏶"
                    } else {
                        " ⏷"
                    }
                } else {
                    ""
                };
                ui.add(
                    egui::Button::new(RichText::new(format!("{}{arrow}", t(label))).strong())
                        .frame(false),
                )
                .clicked()
            };
        let (cur, asc) = (st.sort, st.ascending);
        TableBuilder::new(ui)
            .striped(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(22.0))
            .column(Column::remainder().at_least(160.0).clip(true))
            .column(
                Column::initial(170.0)
                    .at_least(80.0)
                    .clip(true)
                    .resizable(true),
            )
            .column(
                Column::initial(100.0)
                    .at_least(60.0)
                    .clip(true)
                    .resizable(true),
            )
            .column(Column::initial(92.0).at_least(60.0).resizable(true))
            .column(Column::initial(92.0).at_least(70.0).resizable(true))
            .header(row_h, |mut h| {
                h.col(|_| {});
                h.col(|ui| {
                    if header(ui, "Name", SortCol::Name, cur, asc) {
                        sort_click = Some(SortCol::Name);
                    }
                });
                h.col(|ui| {
                    if header(ui, "Publisher", SortCol::Publisher, cur, asc) {
                        sort_click = Some(SortCol::Publisher);
                    }
                });
                h.col(|ui| {
                    if header(ui, "Version", SortCol::Version, cur, asc) {
                        sort_click = Some(SortCol::Version);
                    }
                });
                h.col(|ui| {
                    if header(ui, "Installed", SortCol::Date, cur, asc) {
                        sort_click = Some(SortCol::Date);
                    }
                });
                h.col(|ui| {
                    if header(ui, "Size", SortCol::Size, cur, asc) {
                        sort_click = Some(SortCol::Size);
                    }
                });
            })
            .body(|body| {
                body.rows(row_h, visible.len(), |mut row| {
                    let p = &visible[row.index()];
                    let is_sel = st.selected.as_deref() == Some(&p.key);
                    row.set_selected(is_sel);
                    row.col(|ui| {
                        let mut c = st.checked.contains(&p.key);
                        if ui.checkbox(&mut c, "").changed() {
                            if c {
                                st.checked.insert(p.key.clone());
                            } else {
                                st.checked.remove(&p.key);
                            }
                        }
                    });
                    row.col(|ui| {
                        if log_for(logs, p).is_some() {
                            ui.label(RichText::new("⏺").color(theme::ACCENT_TEXT))
                                .on_hover_text(t("Monitored: precise uninstall available"));
                        }
                        if st.broken.contains(&p.key) {
                            ui.label(RichText::new("⚠").color(theme::WARN))
                                .on_hover_text(t("Broken entry: uninstaller or folder is missing"));
                        }
                        match st.icons.get(&p.key) {
                            Some(tex) => {
                                ui.add(
                                    egui::Image::new(tex).fit_to_exact_size(egui::vec2(18.0, 18.0)),
                                );
                            }
                            None => {
                                ui.add_sized(
                                    [18.0, 18.0],
                                    egui::Label::new(
                                        RichText::new("📦").color(theme::MUTED).small(),
                                    ),
                                );
                            }
                        }
                        ui.add(egui::Label::new(&p.name).truncate().selectable(false));
                    });
                    row.col(|ui| {
                        ui.add(
                            egui::Label::new(RichText::new(&p.publisher).color(theme::MUTED))
                                .truncate()
                                .selectable(false),
                        );
                    });
                    row.col(|ui| {
                        ui.add(egui::Label::new(&p.version).truncate().selectable(false));
                    });
                    row.col(|ui| {
                        ui.add(egui::Label::new(&p.install_date).selectable(false));
                    });
                    row.col(|ui| {
                        let s = size_of(p);
                        ui.add(
                            egui::Label::new(if s > 0 { human_size(s) } else { String::new() })
                                .selectable(false),
                        );
                    });
                    let resp = row.response();
                    if resp.clicked() {
                        clicked = Some(p.key.clone());
                    }
                    if resp.double_clicked() {
                        double = Some(p.clone());
                    }
                });
            });
        if let Some(c) = sort_click {
            if st.sort == c {
                st.ascending = !st.ascending;
            } else {
                st.sort = c;
                st.ascending = c != SortCol::Size && c != SortCol::Date;
            }
        }
        if let Some(k) = clicked {
            st.selected = Some(k);
        }
        if let Some(p) = double {
            self.start_uninstall(vec![p], false);
        }
    }

    fn program_details(&mut self, ui: &mut egui::Ui, p: &InstalledProgram) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if let Some(tex) = self.programs.icons.get(&p.key) {
                ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(32.0, 32.0)));
            }
            ui.add(egui::Label::new(RichText::new(&p.name).heading().strong()).wrap());
        });
        ui.horizontal_wrapped(|ui| {
            theme::badge(ui, t(p.source.label()), theme::MUTED);
            if p.is_msi {
                theme::badge(ui, "MSI", theme::MUTED);
            }
            if log_for(&self.logs.logs, p).is_some() {
                theme::badge(ui, t("Monitored"), theme::ACCENT_TEXT);
            }
            if self.programs.broken.contains(&p.key) {
                theme::badge(ui, t("Broken entry"), theme::WARN);
            }
        });
        ui.add_space(8.0);
        let size = self.size_of(p);
        egui::Grid::new("pd")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                field(ui, "Publisher", &p.publisher);
                field(ui, "Version", &p.version);
                field(ui, "Installed", &p.install_date);
                field(
                    ui,
                    "Size",
                    &if size > 0 {
                        human_size(size)
                    } else {
                        String::new()
                    },
                );
                field(ui, "Location", &p.install_location);
                if p.source != Source::Store && self.settings.advanced_mode {
                    field(ui, "Uninstaller", &p.uninstall_string);
                    field(ui, "Registry key", &p.key);
                } else if self.settings.advanced_mode {
                    field(ui, "Package", &p.key);
                }
                if !p.comments.is_empty() {
                    field(ui, "Comments", &p.comments);
                }
            });
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            if theme::primary_button(ui, format!("🗑 {}", t("Uninstall"))).clicked() {
                self.start_uninstall(vec![p.clone()], false);
            }
            if p.has_quiet_uninstall() && ui.button(t("Quiet uninstall")).clicked() {
                let mut w = Wizard::new(vec![p.clone()], false, self.settings.create_restore_point);
                w.quiet = true;
                self.wizard = Some(w);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if p.source != Source::Store
                && ui
                    .button(t("Forced removal"))
                    .on_hover_text(t("Skip the program's own uninstaller and remove everything Expunge can find."))
                    .clicked()
            {
                self.start_uninstall(vec![p.clone()], true);
            }
            if !p.install_location.is_empty() && ui.button(t("Open folder")).clicked() {
                platform::shell_open(&p.install_location);
            }
            if p.source != Source::Store
                && self.settings.advanced_mode
                && ui.button(t("Open in Registry Editor")).clicked()
            {
                if let Err(e) = platform::open_in_regedit(&p.key) {
                    self.set_status(e.to_string());
                }
            }
            if ui.button(t("Search online")).clicked() {
                let q: String = p.name.chars().map(|c| if c.is_alphanumeric() { c.to_string() } else { "+".into() }).collect();
                platform::shell_open(&format!("https://www.bing.com/search?q={q}"));
            }
            if !p.url.is_empty() && ui.button(t("Website")).clicked() {
                platform::shell_open(&p.url);
            }
        });
        if let Some(log) = log_for(&self.logs.logs, p) {
            ui.add_space(10.0);
            let id = log.id.clone();
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(RichText::new(format!("⏺ {}", t("Monitored installation"))).color(theme::ACCENT_TEXT).strong());
                small_muted(ui, t("Expunge recorded this installation, so it can remove exactly what was added."));
                if ui.link(t("View install log")).clicked() {
                    self.logs.selected = Some(id);
                    self.tab = super::app::Tab::Logs;
                }
            });
        }
    }

    pub fn ui_wizard(&mut self, ctx: &egui::Context) {
        let Some(w) = &mut self.wizard else { return };
        let reg = self.reg.clone();
        let mut close = false;
        let mut start_scan = false;
        let names: Vec<String> = w.programs.iter().map(|p| p.name.clone()).collect();
        let title = if w.force {
            t("Forced removal")
        } else {
            t("Uninstall")
        };

        // --- state machine -------------------------------------------------
        if w.stage == Stage::RestorePoint {
            if let Some(r) = w.rp_job.as_ref().and_then(|j| j.poll()) {
                w.rp_job = None;
                if let Err(e) = r {
                    w.notes
                        .push(tf("Restore point not created: {error}", &[("error", e)]));
                }
                w.stage = if w.force {
                    Stage::Scanning
                } else {
                    Stage::Running
                };
                if w.force {
                    start_scan = true;
                }
            }
        }
        if w.stage == Stage::Running {
            if w.index >= w.programs.len() {
                w.stage = Stage::Scanning;
                start_scan = true;
            } else {
                let p = w.programs[w.index].clone();
                if !w.launched {
                    w.launched = true;
                    w.started = Some(Instant::now());
                    if p.source == Source::Store {
                        let key = p.key.clone();
                        w.store_job = Some(Job::spawn(ctx, move |_| {
                            programs::remove_store_app(&key).map_err(|e| e.to_string())
                        }));
                    } else if let Some(cmd) =
                        p.uninstall_command(w.quiet && p.has_quiet_uninstall())
                    {
                        match platform::spawn_command_line(&platform::expand_env(&cmd)) {
                            Ok(c) => w.child = Some(c),
                            Err(e) => {
                                w.notes.push(format!("{}: {e}", p.name));
                                w.advance();
                            }
                        }
                    } else {
                        w.notes.push(tf(
                            "{name} has no uninstaller; cleaning up directly.",
                            &[("name", p.name.clone())],
                        ));
                        w.advance();
                    }
                } else if let Some(r) = w.store_job.as_ref().and_then(|j| j.poll()) {
                    if let Err(e) = r {
                        w.notes.push(format!("{}: {e}", p.name));
                    }
                    w.advance();
                } else if let Some(c) = &w.child {
                    let exited = c.try_wait().is_some();
                    // Many uninstallers relaunch themselves from %TEMP% and exit
                    // at once; wait for the Uninstall entry to disappear too.
                    let gone = !reg.exists(&p.key);
                    if exited && (gone || p.is_msi) {
                        w.advance();
                    }
                    ctx.request_repaint_after(std::time::Duration::from_millis(500));
                }
            }
        }
        if let Some((plan, logs, offer)) = w.scan_job.as_ref().and_then(|j| j.poll()) {
            w.scan_job = None;
            let name = if names.len() == 1 {
                names[0].clone()
            } else {
                tf("{n} programs", &[("n", names.len().to_string())])
            };
            let mut flow = RemovalFlow::new(
                tf("Leftovers: {name}", &[("name", name)]),
                plan,
                offer,
                self.settings.remove_unlogged_files,
            );
            flow.logs_to_delete = logs;
            flow.names = names.clone();
            flow.plan.info.extend(w.notes.clone());
            self.removal = Some(flow);
            self.wizard = None;
            self.programs.checked.clear();
            self.programs.needs_refresh = true;
            return;
        }
        if w.stage == Stage::Scanning && w.scan_job.is_none() {
            start_scan = true;
        }
        if start_scan {
            let progs = w.programs.clone();
            let logs = self.logs.logs.clone();
            w.scan_job = Some(Job::spawn(ctx, move |pr| {
                let mut plan = Plan::default();
                let mut used_logs = Vec::new();
                let scope = LeftoverScope::default();
                for p in &progs {
                    pr.set_phase(p.name.clone());
                    if let Some(log) = log_for(&logs, p) {
                        let mut lp = build_plan(&log.changes, reg.as_ref());
                        if reg.exists(&p.key) {
                            lp.items.push(PlanItem {
                                action: Action::DeleteKey {
                                    path: p.key.clone(),
                                },
                                selected: true,
                                exists: true,
                                note: String::new(),
                            });
                        }
                        merge_plans(&mut plan, lp);
                        used_logs.push(log.id.clone());
                    }
                    // Heuristics catch what happened outside monitoring (e.g. updates).
                    let mut lo = find_leftovers(p, reg.as_ref(), &scope);
                    if !used_logs.is_empty() {
                        lo.items.iter_mut().for_each(|i| {
                            if matches!(i.action, Action::DeleteTree { .. }) {
                                i.selected = false;
                                i.note = "Name match".into();
                            }
                        });
                    }
                    merge_plans(&mut plan, lo);
                }
                plan.items
                    .sort_by(|a, b| a.action.category().cmp(b.action.category()));
                let offer = !used_logs.is_empty();
                (plan, used_logs, offer)
            }));
        }

        // --- window --------------------------------------------------------
        let elevated = self.elevated;
        let mut open = true;
        egui::Window::new(title)
            .id(egui::Id::new("wizard"))
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                let Some(w) = &mut self.wizard else { return };
                match w.stage {
                    Stage::Confirm => {
                        ui.label(if w.force {
                            t("Expunge will skip the programs' own uninstallers and remove everything it can find:")
                        } else {
                            t("These programs will be uninstalled, then Expunge scans for leftovers:")
                        });
                        egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                            for p in &w.programs {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("• {}", p.name)).strong());
                                    if log_for(&self.logs.logs, p).is_some() {
                                        theme::badge(ui, t("Monitored"), theme::ACCENT_TEXT);
                                    }
                                });
                            }
                        });
                        ui.add_space(6.0);
                        if !w.force && w.programs.iter().any(|p| p.has_quiet_uninstall()) {
                            ui.checkbox(&mut w.quiet, t("Uninstall quietly where possible (no prompts)"));
                        }
                        ui.add_enabled_ui(elevated, |ui| {
                            ui.checkbox(&mut w.restore_point, t("Create a System Restore point first"));
                        });
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if theme::primary_button(ui, t("Start")).clicked() {
                                if w.restore_point && elevated {
                                    w.stage = Stage::RestorePoint;
                                    w.rp_job = Some(Job::spawn(ui.ctx(), |_| {
                                        expunge_core::updates::create_restore_point("Before Expunge uninstall").map_err(|e| e.to_string())
                                    }));
                                } else if w.force {
                                    w.stage = Stage::Scanning;
                                    start_scan_later(ui.ctx());
                                } else {
                                    w.stage = Stage::Running;
                                }
                            }
                            if ui.button(t("Cancel")).clicked() {
                                close = true;
                            }
                        });
                    }
                    Stage::RestorePoint => job_progress(ui, &t("Creating a restore point…"), 0),
                    Stage::Running => {
                        let total = w.programs.len();
                        let cur = w.index.min(total.saturating_sub(1));
                        let p = w.programs[cur].clone();
                        ui.label(tf("Uninstalling {done} of {total}", &[("done", (cur + 1).to_string()), ("total", total.to_string())]));
                        ui.add(egui::ProgressBar::new(cur as f32 / total as f32).desired_height(6.0));
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new(&p.name).strong());
                        });
                        small_muted(ui, t("Follow the program's own uninstaller. When it has finished, Expunge continues automatically."));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button(t("It's finished, continue")).clicked() {
                                w.advance();
                            }
                            if ui.button(t("Skip this program")).clicked() {
                                w.notes.push(tf("Skipped {name}", &[("name", p.name.clone())]));
                                w.advance();
                            }
                        });
                    }
                    Stage::Scanning => {
                        let phase = w.scan_job.as_ref().map(|j| j.progress.phase()).unwrap_or_default();
                        job_progress(ui, &tf("Scanning for leftovers… {name}", &[("name", phase)]), 0);
                    }
                }
            });
        if !open || close {
            self.wizard = None;
        }
    }
}

/// The scan starts on the next frame (the window closure can't start jobs
/// that need `self`), so just make sure that frame happens.
fn start_scan_later(ctx: &egui::Context) {
    ctx.request_repaint();
}
