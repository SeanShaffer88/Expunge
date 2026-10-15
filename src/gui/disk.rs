//! Disk space page: a treemap of where space goes on a drive or in a folder,
//! with a size-sorted list beside it for keyboard and screen-reader use.

use super::app::App;
use super::theme;
use super::widgets::*;
use super::Job;
use egui::{Color32, CornerRadius, Pos2, RichText, Sense, Stroke, StrokeKind, Vec2};
use expunge_core::diskmap::{self, DiskMap, Kind, Node};
use expunge_core::i18n::{t, tf};
use expunge_core::model::human_size;
use std::path::PathBuf;

#[derive(Default)]
pub struct DiskState {
    pub job: Option<Job<DiskMap>>,
    pub map: Option<DiskMap>,
    /// Child indices from the root to the folder being shown.
    pub path: Vec<usize>,
    pub selected: Option<usize>,
    pub drives: Vec<PathBuf>,
    pub target: Option<PathBuf>,
}

/// File categories, each with its own colour.
const CATEGORIES: &[(&str, Color32, &[&str])] = &[
    (
        "Videos",
        Color32::from_rgb(0x8E, 0x3B, 0x46),
        &[
            "mp4", "mkv", "avi", "mov", "wmv", "webm", "m4v", "mpg", "mpeg", "flv", "ts",
        ],
    ),
    (
        "Music",
        Color32::from_rgb(0x9C, 0x6B, 0x2F),
        &[
            "mp3", "flac", "wav", "m4a", "aac", "ogg", "wma", "opus", "aiff",
        ],
    ),
    (
        "Pictures",
        Color32::from_rgb(0x3F, 0x7A, 0x5C),
        &[
            "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "raw", "cr2", "nef",
            "psd", "svg",
        ],
    ),
    (
        "Archives and disk images",
        Color32::from_rgb(0x6C, 0x55, 0x8E),
        &[
            "zip", "7z", "rar", "tar", "gz", "xz", "bz2", "zst", "cab", "iso", "img", "vhd",
            "vhdx", "wim", "esd",
        ],
    ),
    (
        "Programs and libraries",
        Color32::from_rgb(0x3D, 0x62, 0x8A),
        &[
            "exe", "dll", "sys", "msi", "msp", "msix", "appx", "node", "so", "pak", "bin", "dat",
        ],
    ),
    (
        "Documents",
        Color32::from_rgb(0x2F, 0x7C, 0x80),
        &[
            "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "txt", "md", "epub",
            "csv",
        ],
    ),
];
const OTHER_FILE: Color32 = Color32::from_rgb(0x5E, 0x56, 0x5A);
const SMALL_FILES: Color32 = Color32::from_rgb(0x46, 0x40, 0x43);
const FOLDER: Color32 = Color32::from_rgb(0x52, 0x2A, 0x28);

fn color_of(n: &Node) -> Color32 {
    match n.kind {
        Kind::Dir => FOLDER,
        Kind::Others => SMALL_FILES,
        Kind::File => {
            let ext = n.ext().to_ascii_lowercase();
            CATEGORIES
                .iter()
                .find(|c| c.2.contains(&ext.as_str()))
                .map(|c| c.1)
                .unwrap_or(OTHER_FILE)
        }
    }
}

fn label_of(n: &Node) -> String {
    match n.kind {
        Kind::Others => tf("Smaller files ({n})", &[("n", n.files.to_string())]),
        _ => n.name.clone(),
    }
}

fn icon_of(n: &Node) -> &'static str {
    match n.kind {
        Kind::Dir => "📁",
        Kind::File => "📄",
        Kind::Others => "🗋",
    }
}

fn to_egui(r: diskmap::Rect) -> egui::Rect {
    egui::Rect::from_min_size(Pos2::new(r.x, r.y), Vec2::new(r.w, r.h))
}

fn from_egui(r: egui::Rect) -> diskmap::Rect {
    diskmap::Rect {
        x: r.min.x,
        y: r.min.y,
        w: r.width(),
        h: r.height(),
    }
}

impl App {
    fn disk_start(&mut self, ctx: &egui::Context, target: PathBuf) {
        self.disk.target = Some(target.clone());
        self.disk.map = None;
        self.disk.path.clear();
        self.disk.selected = None;
        self.disk.job = Some(Job::spawn(ctx, move |p| diskmap::scan(&target, p)));
    }

    pub fn ui_disk(&mut self, ctx: &egui::Context) {
        if self.disk.drives.is_empty() {
            self.disk.drives = diskmap::drives();
        }
        if let Some(m) = self.disk.job.as_ref().and_then(|j| j.poll()) {
            self.disk.job = None;
            self.disk.map = Some(m);
        }
        egui::CentralPanel::default().frame(theme::content_frame()).show(ctx, |ui| {
            // Location bar.
            ui.horizontal_wrapped(|ui| {
                ui.label(t("Location:"));
                let busy = self.disk.job.is_some();
                for d in self.disk.drives.clone() {
                    let label = d.to_string_lossy().to_string();
                    let current = self.disk.target.as_ref() == Some(&d);
                    if ui
                        .add_enabled(!busy, egui::Button::selectable(current, format!("💽 {label}")))
                        .on_hover_text(t("Scan this drive"))
                        .clicked()
                    {
                        self.disk_start(ctx, d);
                    }
                }
                if ui
                    .add_enabled(!busy, egui::Button::new(format!("📂 {}", t("Choose folder…"))))
                    .clicked()
                {
                    if let Some(f) = pick_folder(&t("Choose a folder to scan")) {
                        self.disk_start(ctx, f);
                    }
                }
                if busy {
                    if ui.button(t("Cancel")).clicked() {
                        if let Some(j) = &self.disk.job {
                            j.cancel();
                        }
                    }
                } else if let Some(tgt) = self.disk.target.clone() {
                    if ui.button(format!("⟳ {}", t("Rescan"))).clicked() {
                        self.disk_start(ctx, tgt);
                    }
                }
            });
            ui.add_space(6.0);

            if let Some(job) = &self.disk.job {
                job_progress(ui, &t("Measuring folders…"), job.progress.count());
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
                return;
            }
            let Some(map) = self.disk.map.take() else {
                theme::card(ui, |ui| {
                    ui.label(RichText::new(t("See what takes up space")).strong());
                    muted(ui, t("Choose a drive or folder above. Expunge measures every folder and draws a map where each block's size matches the space it uses. Click a block to look inside it."));
                });
                return;
            };
            self.disk_view(ui, &map);
            self.disk.map = Some(map);
        });
    }

    fn disk_view(&mut self, ui: &mut egui::Ui, map: &DiskMap) {
        // Fix up a path that no longer points anywhere.
        while !self.disk.path.is_empty() && map.root.at(&self.disk.path).is_none() {
            self.disk.path.pop();
        }
        let node = map.root.at(&self.disk.path).unwrap_or(&map.root);

        // Breadcrumb.
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.disk.path.is_empty(), egui::Button::new("⬆"))
                .on_hover_text(t("Up one level"))
                .clicked()
            {
                self.disk.selected = self.disk.path.pop();
            }
            if ui.link(map.root_path.to_string_lossy()).clicked() {
                self.disk.path.clear();
                self.disk.selected = None;
            }
            let mut n = &map.root;
            for depth in 0..self.disk.path.len() {
                let Some(c) = n.children.get(self.disk.path[depth]) else {
                    break;
                };
                ui.label(RichText::new("›").color(theme::MUTED));
                if ui.link(&c.name).clicked() {
                    self.disk.path.truncate(depth + 1);
                    self.disk.selected = None;
                }
                n = c;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(tf(
                        "{size} in {files} files",
                        &[
                            ("size", human_size(node.size)),
                            ("files", node.files.to_string()),
                        ],
                    ))
                    .strong(),
                );
            });
        });
        if map.cancelled {
            theme::warning_banner(
                ui,
                t("The scan was cancelled, so these sizes are incomplete."),
            );
        }
        if map.unreadable > 0 {
            let text = if self.elevated {
                "{n} folders couldn't be read and aren't counted."
            } else {
                "{n} folders couldn't be read and aren't counted. Run as Administrator to include them."
            };
            small_muted(ui, tf(text, &[("n", map.unreadable.to_string())]));
        }
        ui.add_space(4.0);

        let mut open: Option<usize> = None;
        let list_w = 330.0f32.min(ui.available_width() * 0.4);
        let avail = ui.available_size();
        ui.horizontal_top(|ui| {
            // Treemap.
            let map_size = Vec2::new(
                (avail.x - list_w - 12.0).max(120.0),
                (avail.y - 34.0).max(120.0),
            );
            let (rect, resp) = ui.allocate_exact_size(map_size, Sense::click());
            if let Some(i) = self.draw_treemap(ui, rect, &resp, node) {
                if node.children[i].is_dir() {
                    open = Some(i);
                } else {
                    self.disk.selected = Some(i);
                }
            }
            ui.add_space(12.0);

            // Size-sorted list.
            ui.vertical(|ui| {
                ui.set_width(list_w);
                if node.children.is_empty() {
                    muted(
                        ui,
                        if node.size < diskmap::DETAIL_MIN && node.size > 0 {
                            t("This folder is small, so its contents aren't listed.")
                        } else {
                            t("This folder is empty.")
                        },
                    );
                }
                egui::ScrollArea::vertical()
                    .max_height(map_size.y - 70.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (i, c) in node.children.iter().enumerate() {
                            let share = if node.size > 0 {
                                c.size as f32 / node.size as f32
                            } else {
                                0.0
                            };
                            let sel = self.disk.selected == Some(i);
                            let r = ui.add(
                                egui::Button::selectable(
                                    sel,
                                    format!("{} {}", icon_of(c), label_of(c)),
                                )
                                .right_text(RichText::new(human_size(c.size)).color(theme::MUTED))
                                .min_size(Vec2::new(list_w - 14.0, 0.0))
                                .truncate(),
                            );
                            let bar = egui::Rect::from_min_size(
                                Pos2::new(r.rect.min.x, r.rect.max.y - 2.0),
                                Vec2::new(r.rect.width() * share, 2.0),
                            );
                            ui.painter()
                                .rect_filled(bar, 0.0, color_of(c).gamma_multiply(1.6));
                            if r.clicked() {
                                self.disk.selected = Some(i);
                            }
                            if r.double_clicked() && c.is_dir() {
                                open = Some(i);
                            }
                            r.on_hover_text(format!("{:.1} %", share * 100.0));
                        }
                    });
                ui.add_space(6.0);
                if let Some(c) = self.disk.selected.and_then(|i| node.children.get(i)) {
                    let i = self.disk.selected.unwrap_or(0);
                    ui.label(RichText::new(label_of(c)).strong());
                    small_muted(
                        ui,
                        tf(
                            "{size} · {files} files",
                            &[("size", human_size(c.size)), ("files", c.files.to_string())],
                        ),
                    );
                    ui.horizontal_wrapped(|ui| {
                        if c.is_dir() && ui.button(t("Open")).clicked() {
                            open = Some(i);
                        }
                        let mut p = self.disk.path.clone();
                        p.push(i);
                        let target = if c.kind == Kind::Others {
                            map.path_of(&self.disk.path)
                        } else {
                            map.path_of(&p)
                        };
                        if ui.button(t("Show in File Explorer")).clicked() {
                            expunge_core::platform::reveal(&target.to_string_lossy());
                        }
                    });
                }
            });
        });
        self.disk_legend(ui);
        if let Some(i) = open {
            self.disk.path.push(i);
            self.disk.selected = None;
        }
        // Keyboard: Backspace goes up a level.
        if !self.disk.path.is_empty()
            && ui.ctx().memory(|m| m.focused().is_none())
            && ui.input(|i| i.key_pressed(egui::Key::Backspace))
        {
            self.disk.selected = self.disk.path.pop();
        }
    }

    /// Draws `node`'s children with their contents nested inside, several
    /// levels deep while there is room. Returns the child that was clicked.
    fn draw_treemap(
        &self,
        ui: &egui::Ui,
        rect: egui::Rect,
        resp: &egui::Response,
        node: &Node,
    ) -> Option<usize> {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::same(4), theme::CARD);
        let hover = resp.hover_pos();
        let mut hovered: Option<(String, u64)> = None;
        let sizes: Vec<u64> = node.children.iter().map(|c| c.size).collect();
        let rects = diskmap::layout(&sizes, from_egui(rect.shrink(1.0)));
        let mut clicked = None;
        for (i, (c, r)) in node.children.iter().zip(&rects).enumerate() {
            let r = to_egui(*r).shrink(1.0);
            if r.width() < 1.0 || r.height() < 1.0 {
                continue;
            }
            let lit = hover.is_some_and(|p| r.contains(p));
            draw_block(&painter, c, r, 0, i, lit, hover, &label_of(c), &mut hovered);
            if self.disk.selected == Some(i) {
                painter.rect_stroke(
                    r,
                    CornerRadius::same(2),
                    Stroke::new(2.0, theme::ACCENT_TEXT),
                    StrokeKind::Inside,
                );
            }
            if lit && resp.clicked() {
                clicked = Some(i);
            }
        }
        if let Some((name, size)) = hovered {
            let share = if node.size > 0 {
                size as f64 * 100.0 / node.size as f64
            } else {
                0.0
            };
            resp.clone()
                .on_hover_text_at_pointer(format!("{name}\n{} · {share:.1} %", human_size(size)));
        }
        clicked
    }

    fn disk_legend(&self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let item = |ui: &mut egui::Ui, color: Color32, name: &str| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(2), color);
                ui.label(RichText::new(t(name)).small().color(theme::MUTED));
                ui.add_space(6.0);
            };
            item(ui, FOLDER, "Folders");
            for (name, color, _) in CATEGORIES {
                item(ui, *color, name);
            }
            item(ui, OTHER_FILE, "Other files");
            item(ui, SMALL_FILES, "Smaller files grouped together");
        });
    }
}

/// Shorten a label with an ellipsis so it fits `max_w`.
fn clip_galley(
    painter: &egui::Painter,
    galley: std::sync::Arc<egui::Galley>,
    max_w: f32,
    font: &egui::FontId,
    text: &str,
) -> std::sync::Arc<egui::Galley> {
    if galley.size().x <= max_w {
        return galley;
    }
    let color = galley
        .job
        .sections
        .first()
        .map(|s| s.format.color)
        .unwrap_or(theme::TEXT);
    let chars: Vec<char> = text.chars().collect();
    let mut n = chars.len();
    while n > 1 {
        n = n * 3 / 4;
        let s: String = chars[..n].iter().collect::<String>() + "…";
        let g = painter.layout_no_wrap(s, font.clone(), color);
        if g.size().x <= max_w {
            return g;
        }
    }
    painter.layout_no_wrap(String::new(), font.clone(), color)
}

/// Deepest level drawn inside the shown folder.
const MAX_DEPTH: usize = 6;

/// Draw one block and, for folders with room, its contents inside it.
/// `hovered` receives the deepest block under the pointer.
#[allow(clippy::too_many_arguments)]
fn draw_block(
    painter: &egui::Painter,
    n: &Node,
    r: egui::Rect,
    depth: usize,
    index: usize,
    lit: bool,
    hover: Option<Pos2>,
    name: &str,
    hovered: &mut Option<(String, u64)>,
) {
    // Folders get lighter with depth, and neighbours differ slightly.
    let mut fill = if n.is_dir() {
        FOLDER.gamma_multiply(1.0 + 0.16 * depth as f32 + 0.1 * (index % 3) as f32)
    } else {
        color_of(n)
    };
    if lit {
        fill = fill.gamma_multiply(1.18);
    }
    painter.rect_filled(r, CornerRadius::same(if depth == 0 { 2 } else { 1 }), fill);
    if hover.is_some_and(|p| r.contains(p)) {
        *hovered = Some((name.to_string(), n.size));
    }
    let header: f32 = if r.width() > 60.0 && r.height() > 34.0 {
        15.0
    } else {
        0.0
    };
    if header > 0.0 || (depth == 0 && r.width() > 44.0 && r.height() > 14.0) {
        let font = egui::FontId::proportional(if depth == 0 { 12.0 } else { 10.5 });
        let color = if depth == 0 {
            theme::TEXT
        } else {
            theme::TEXT.gamma_multiply(0.82)
        };
        let text = label_of(n);
        let galley = painter.layout_no_wrap(text.clone(), font.clone(), color);
        let gal = clip_galley(painter, galley, r.width() - 7.0, &font, &text);
        painter.galley(r.min + Vec2::new(3.5, 1.0), gal, color);
    }
    if !n.is_dir() || n.children.is_empty() || depth + 1 >= MAX_DEPTH {
        return;
    }
    let pad = 2.0;
    let inner = egui::Rect::from_min_max(
        Pos2::new(r.min.x + pad, r.min.y + header.max(pad)),
        Pos2::new(r.max.x - pad, r.max.y - pad),
    );
    if inner.width() < 6.0 || inner.height() < 6.0 {
        return;
    }
    let sizes: Vec<u64> = n.children.iter().map(|c| c.size).collect();
    let rects = diskmap::layout(&sizes, from_egui(inner));
    for (j, (c, cr)) in n.children.iter().zip(&rects).enumerate() {
        let cr = to_egui(*cr).shrink(0.5);
        if cr.width() < 1.5 || cr.height() < 1.5 {
            continue;
        }
        let child_name = format!("{name} › {}", label_of(c));
        draw_block(
            painter,
            c,
            cr,
            depth + 1,
            j,
            lit,
            hover,
            &child_name,
            hovered,
        );
    }
}
