//! Duplicate files page: find identical files in your folders and move the
//! extra copies to the Recycle Bin, always keeping at least one.

use super::app::{App, Confirm};
use super::theme;
use super::widgets::*;
use super::Job;
use egui::RichText;
use expunge_core::duplicates::{self, DupResult, Keep};
use expunge_core::i18n::{t, tf};
use expunge_core::model::human_size;
use std::path::PathBuf;

/// Smallest file compared, with its label.
const MIN_SIZES: &[(u64, &str)] = &[
    (100 * 1024, "100 KB"),
    (1024 * 1024, "1 MB"),
    (10 * 1024 * 1024, "10 MB"),
    (100 * 1024 * 1024, "100 MB"),
];
/// Sets drawn at once; the rest are summarized.
const SHOWN: usize = 500;

#[derive(Default)]
pub struct DupState {
    pub ready: bool,
    pub roots: Vec<PathBuf>,
    pub min_idx: usize,
    pub job: Option<Job<DupResult>>,
    pub result: Option<DupResult>,
    /// Ticked = move to the Recycle Bin. Same shape as the groups.
    pub marked: Vec<Vec<bool>>,
    pub remove_job: Option<Job<Vec<(PathBuf, Result<(), String>)>>>,
    pub failures: Vec<String>,
    pub freed: Option<(usize, u64)>,
}

/// The usual personal folders that exist on this PC.
fn default_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) else {
            return Vec::new();
        };
        [
            "Desktop",
            "Documents",
            "Downloads",
            "Pictures",
            "Music",
            "Videos",
        ]
        .iter()
        .map(|d| home.join(d))
        .filter(|p| p.is_dir())
        .collect()
    }
    #[cfg(not(windows))]
    {
        vec![std::env::temp_dir()]
    }
}

fn move_to_recycle_bin(p: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        expunge_core::platform::recycle(p).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        // Development builds have no Recycle Bin.
        std::fs::remove_file(p).map_err(|e| e.to_string())
    }
}

impl App {
    fn dups_selected(&self) -> (usize, u64) {
        let Some(r) = &self.dups.result else {
            return (0, 0);
        };
        let mut n = 0;
        let mut bytes = 0;
        for (g, m) in r.groups.iter().zip(&self.dups.marked) {
            let k = m.iter().filter(|x| **x).count();
            n += k;
            bytes += g.size * k as u64;
        }
        (n, bytes)
    }

    fn dups_auto_select(&mut self, keep: Keep) {
        let Some(r) = &self.dups.result else { return };
        self.dups.marked = r
            .groups
            .iter()
            .map(|g| {
                let k = duplicates::keeper(g, keep);
                (0..g.files.len()).map(|i| i != k).collect()
            })
            .collect();
    }

    pub fn start_remove_duplicates(&mut self, ctx: &egui::Context) {
        let Some(r) = &self.dups.result else { return };
        let mut work = Vec::new();
        for (g, m) in r.groups.iter().zip(&self.dups.marked) {
            let remove: Vec<usize> = (0..g.files.len())
                .filter(|&i| m.get(i) == Some(&true))
                .collect();
            if !remove.is_empty() {
                work.push((g.clone(), remove));
            }
        }
        self.dups.failures.clear();
        self.dups.remove_job = Some(Job::spawn(ctx, move |p| {
            let mut out = Vec::new();
            for (g, remove) in work {
                // Re-check right before removing: one copy must stay, and
                // nothing may have changed since the scan.
                if let Err(e) = duplicates::check_removal(&g, &remove) {
                    for i in remove {
                        out.push((g.files[i].path.clone(), Err(e.clone())));
                    }
                    continue;
                }
                for i in remove {
                    let path = g.files[i].path.clone();
                    p.set_phase(path.to_string_lossy().to_string());
                    let r = move_to_recycle_bin(&path);
                    out.push((path, r));
                }
            }
            out
        }));
    }

    fn poll_dups(&mut self) {
        if let Some(r) = self.dups.job.as_ref().and_then(|j| j.poll()) {
            self.dups.job = None;
            self.dups.marked = r
                .groups
                .iter()
                .map(|g| vec![false; g.files.len()])
                .collect();
            self.dups.result = Some(r);
            self.dups.freed = None;
        }
        if let Some(done) = self.dups.remove_job.as_ref().and_then(|j| j.poll()) {
            self.dups.remove_job = None;
            let mut removed = std::collections::HashSet::new();
            let (mut n, mut bytes) = (0usize, 0u64);
            if let Some(r) = &self.dups.result {
                let size_of: std::collections::HashMap<&PathBuf, u64> = r
                    .groups
                    .iter()
                    .flat_map(|g| g.files.iter().map(move |f| (&f.path, g.size)))
                    .collect();
                for (path, res) in &done {
                    match res {
                        Ok(()) => {
                            n += 1;
                            bytes += size_of.get(path).copied().unwrap_or(0);
                            removed.insert(path.clone());
                        }
                        Err(e) => self.dups.failures.push(format!("{}: {e}", path.display())),
                    }
                }
            }
            self.dups.freed = Some((n, bytes));
            // Drop removed files, and sets with only one copy left.
            if let Some(r) = &mut self.dups.result {
                for g in &mut r.groups {
                    g.files.retain(|f| !removed.contains(&f.path));
                }
                r.groups.retain(|g| g.files.len() > 1);
                self.dups.marked = r
                    .groups
                    .iter()
                    .map(|g| vec![false; g.files.len()])
                    .collect();
            }
        }
    }

    pub fn ui_dups(&mut self, ctx: &egui::Context) {
        if !self.dups.ready {
            self.dups.ready = true;
            self.dups.roots = default_roots();
            self.dups.min_idx = 1;
        }
        self.poll_dups();
        egui::CentralPanel::default().frame(theme::content_frame()).show(ctx, |ui| {
            if let Some(job) = &self.dups.remove_job {
                job_progress(ui, &tf("Moving to the Recycle Bin… {name}", &[("name", job.progress.phase())]), 0);
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
                return;
            }
            let busy = self.dups.job.is_some();
            theme::card(ui, |ui| {
                ui.label(RichText::new(t("Where to look")).strong());
                let mut drop = None;
                for (i, r) in self.dups.roots.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!busy, egui::Button::new("✖").small())
                            .on_hover_text(t("Don't search this folder"))
                            .clicked()
                        {
                            drop = Some(i);
                        }
                        ui.label(r.to_string_lossy());
                    });
                }
                if let Some(i) = drop {
                    self.dups.roots.remove(i);
                }
                if self.dups.roots.is_empty() {
                    muted(ui, t("Add at least one folder."));
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!busy, egui::Button::new(format!("➕ {}", t("Add folder…")))).clicked() {
                        if let Some(f) = pick_folder(&t("Choose a folder to search")) {
                            if !self.dups.roots.contains(&f) {
                                self.dups.roots.push(f);
                            }
                        }
                    }
                    ui.add_space(12.0);
                    ui.label(t("Ignore files smaller than"));
                    egui::ComboBox::from_id_salt("dup_min")
                        .selected_text(MIN_SIZES[self.dups.min_idx].1)
                        .show_ui(ui, |ui| {
                            for (i, (_, label)) in MIN_SIZES.iter().enumerate() {
                                ui.selectable_value(&mut self.dups.min_idx, i, *label);
                            }
                        });
                });
                small_muted(ui, t("Windows, program and app-data folders are never searched, and files stored only in the cloud are skipped so they aren't downloaded."));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if busy {
                        if ui.button(t("Cancel")).clicked() {
                            if let Some(j) = &self.dups.job {
                                j.cancel();
                            }
                        }
                    } else if !self.dups.roots.is_empty()
                        && theme::primary_button(ui, format!("🔍 {}", t("Find duplicates"))).clicked()
                    {
                        let roots = self.dups.roots.clone();
                        let min = MIN_SIZES[self.dups.min_idx].0;
                        self.dups.result = None;
                        self.dups.job = Some(Job::spawn(ctx, move |p| duplicates::find(&roots, min, p)));
                    }
                });
            });
            ui.add_space(8.0);

            if let Some(job) = &self.dups.job {
                let phase = if job.progress.phase() == "compare" {
                    t("Comparing files with the same size…")
                } else {
                    t("Listing files…")
                };
                job_progress(ui, &phase, job.progress.count());
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
                return;
            }
            if let Some((n, bytes)) = self.dups.freed {
                if n > 0 {
                    ui.label(RichText::new(tf(
                        "Moved {n} files ({size}) to the Recycle Bin. Empty the Recycle Bin to free the space, or restore files from it if needed.",
                        &[("n", n.to_string()), ("size", human_size(bytes))],
                    )).color(theme::OK));
                }
            }
            for f in self.dups.failures.iter().take(10) {
                ui.label(RichText::new(f).small().color(theme::BAD));
            }
            let Some(result) = &self.dups.result else { return };
            if result.cancelled {
                theme::warning_banner(ui, t("The search was cancelled."));
                return;
            }
            if result.groups.is_empty() {
                theme::card(ui, |ui| {
                    ui.label(RichText::new(t("No duplicate files found.")).strong());
                    small_muted(ui, tf("{n} files checked.", &[("n", result.files_scanned.to_string())]));
                });
                return;
            }
            ui.label(RichText::new(tf(
                "{groups} sets of identical files. Keeping one copy of each would free {size}.",
                &[("groups", result.groups.len().to_string()), ("size", human_size(result.wasted()))],
            )).strong());
            let mut auto = None;
            ui.horizontal_wrapped(|ui| {
                ui.label(t("Select all copies except:"));
                if ui.button(t("The oldest")).clicked() {
                    auto = Some(Some(Keep::Oldest));
                }
                if ui.button(t("The newest")).clicked() {
                    auto = Some(Some(Keep::Newest));
                }
                if ui.button(t("The shortest path")).clicked() {
                    auto = Some(Some(Keep::ShortestPath));
                }
                if ui.button(t("Clear selection")).clicked() {
                    auto = Some(None);
                }
            });
            let (n, bytes) = self.dups_selected();
            let mut remove = false;
            egui::TopBottomPanel::bottom("dups_bottom")
                .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::symmetric(0, 6)))
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                n > 0,
                                egui::Button::new(tf(
                                    "Move {n} files ({size}) to the Recycle Bin",
                                    &[("n", n.to_string()), ("size", human_size(bytes))],
                                ))
                                .fill(if n > 0 { theme::ACCENT } else { theme::CARD }),
                            )
                            .clicked()
                        {
                            remove = true;
                        }
                        small_muted(ui, t("At least one copy of every file is always kept."));
                    });
                });
            ui.add_space(4.0);
            let mut reveal: Option<PathBuf> = None;
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (gi, g) in result.groups.iter().enumerate().take(SHOWN) {
                    let Some(marks) = self.dups.marked.get_mut(gi) else { break };
                    theme::card(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(tf(
                                "{n} copies · {size} each",
                                &[("n", g.files.len().to_string()), ("size", human_size(g.size))],
                            )).strong());
                            small_muted(ui, tf("{size} can be freed", &[("size", human_size(g.wasted()))]));
                        });
                        let unticked = marks.iter().filter(|m| !**m).count();
                        for (fi, f) in g.files.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let last_kept = !marks[fi] && unticked <= 1;
                                let mut v = marks[fi];
                                let r = ui
                                    .add_enabled(!last_kept, egui::Checkbox::without_text(&mut v))
                                    .on_disabled_hover_text(t("The last remaining copy is always kept."));
                                if r.changed() {
                                    marks[fi] = v;
                                }
                                if ui.small_button("📂").on_hover_text(t("Show in File Explorer")).clicked() {
                                    reveal = Some(f.path.clone());
                                }
                                let date = chrono::DateTime::from_timestamp(f.modified, 0)
                                    .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
                                    .unwrap_or_default();
                                ui.label(RichText::new(date).small().color(theme::MUTED));
                                let text = f.path.to_string_lossy().to_string();
                                let label = if marks[fi] {
                                    RichText::new(text).strikethrough().color(theme::MUTED)
                                } else {
                                    RichText::new(text)
                                };
                                ui.add(egui::Label::new(label).truncate());
                            });
                        }
                    });
                    ui.add_space(4.0);
                }
                if result.groups.len() > SHOWN {
                    small_muted(ui, tf(
                        "Showing the {n} sets that waste the most space. Remove some and search again to see the rest.",
                        &[("n", SHOWN.to_string())],
                    ));
                }
            });
            if let Some(p) = reveal {
                expunge_core::platform::reveal(&p.to_string_lossy());
            }
            if let Some(a) = auto {
                match a {
                    Some(k) => self.dups_auto_select(k),
                    None => {
                        for m in &mut self.dups.marked {
                            m.iter_mut().for_each(|x| *x = false);
                        }
                    }
                }
            }
            if remove {
                self.ask(
                    "Move these files to the Recycle Bin?",
                    tf(
                        "{n} duplicate files ({size}) will be moved to the Recycle Bin. One copy of each stays where it is. Files too large for the Recycle Bin are only deleted after Windows asks you.",
                        &[("n", n.to_string()), ("size", human_size(bytes))],
                    ),
                    "Move to Recycle Bin",
                    Confirm::RemoveDuplicates,
                );
            }
        });
    }
}
