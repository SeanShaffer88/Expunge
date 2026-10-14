//! Small shared UI pieces.

use super::theme;
use egui::RichText;
use expunge_core::backup::ActionResult;
use expunge_core::i18n::{t, tf};
use std::path::PathBuf;

pub fn muted(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.label(RichText::new(text.into()).color(theme::MUTED))
}

pub fn small_muted(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.label(RichText::new(text.into()).color(theme::MUTED).small())
}

/// Label + selectable value on one line of a grid.
pub fn field(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(RichText::new(t(label)).color(theme::MUTED));
    if value.is_empty() {
        ui.label(RichText::new("—").color(theme::MUTED));
    } else {
        ui.add(egui::Label::new(value).wrap().selectable(true));
    }
    ui.end_row();
}

/// Progress line for a running job.
pub fn job_progress(ui: &mut egui::Ui, phase: &str, count: u64) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(phase);
        if count > 0 {
            small_muted(ui, tf("{n} items", &[("n", count.to_string())]));
        }
    });
}

pub fn pick_file(title: &str, filter_name: &str, exts: &[&str]) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter(filter_name, exts)
            .pick_file()
    }
    #[cfg(not(windows))]
    {
        let _ = (title, filter_name, exts);
        None
    }
}

pub fn save_file(title: &str, default_name: &str, filter_name: &str, ext: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(default_name)
            .add_filter(filter_name, &[ext])
            .save_file()
    }
    #[cfg(not(windows))]
    {
        let _ = (title, filter_name, ext);
        Some(std::env::temp_dir().join(default_name))
    }
}

/// Summary and details of an operation's results.
pub fn results_ui(ui: &mut egui::Ui, results: &[ActionResult]) {
    let failed = results.iter().filter(|r| !r.ok).count();
    let ok = results.len() - failed;
    ui.horizontal(|ui| {
        theme::badge(ui, tf("{n} succeeded", &[("n", ok.to_string())]), theme::OK);
        if failed > 0 {
            theme::badge(
                ui,
                tf("{n} failed", &[("n", failed.to_string())]),
                theme::BAD,
            );
        }
        if ui.small_button(t("Copy report")).clicked() {
            let text: String = results
                .iter()
                .map(|r| {
                    format!(
                        "[{}] {} — {}\n",
                        if r.ok { "OK" } else { "FAILED" },
                        r.description,
                        r.message
                    )
                })
                .collect();
            ui.ctx().copy_text(text);
        }
    });
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .max_height(360.0)
        .show(ui, |ui| {
            let mut sorted: Vec<&ActionResult> = results.iter().collect();
            sorted.sort_by_key(|r| r.ok);
            for r in sorted {
                ui.horizontal_wrapped(|ui| {
                    let (sym, col) = if r.ok {
                        ("✔", theme::OK)
                    } else {
                        ("✖", theme::BAD)
                    };
                    ui.label(RichText::new(sym).color(col));
                    ui.label(&r.description);
                    ui.label(
                        RichText::new(&r.message)
                            .color(if r.ok { theme::MUTED } else { theme::BAD })
                            .small(),
                    );
                });
            }
        });
}

/// A search box with a clear button.
pub fn search_box(ui: &mut egui::Ui, text: &mut String, hint: &str, width: f32) -> egui::Response {
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(text)
                .hint_text(format!("🔍 {}", t(hint)))
                .desired_width(width),
        );
        if !text.is_empty() && ui.small_button("✖").clicked() {
            text.clear();
        }
        r
    })
    .inner
}
