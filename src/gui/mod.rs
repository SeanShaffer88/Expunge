//! The Expunge desktop interface (egui, rendered with DirectX 12 on Windows).

mod a11y;
mod app;
mod backups;
mod cleaner;
mod dashboard;
mod lab;
mod logs;
mod monitor;
mod plan;
mod programs;
mod settings_tab;
mod startup_tab;
mod theme;
mod updates;
mod widgets;

use expunge_core::snapshot::Progress;
use std::sync::mpsc;
use std::sync::Arc;

/// Work running on a background thread, polled once per frame.
pub struct Job<T> {
    rx: mpsc::Receiver<T>,
    pub progress: Arc<Progress>,
}

impl<T: Send + 'static> Job<T> {
    pub fn spawn(ctx: &egui::Context, f: impl FnOnce(&Progress) -> T + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let progress = Arc::new(Progress::default());
        let p = progress.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = f(&p);
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        Job { rx, progress }
    }

    /// The result, once the work has finished.
    pub fn poll(&self) -> Option<T> {
        self.rx.try_recv().ok()
    }

    pub fn cancel(&self) {
        self.progress
            .cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Window/taskbar icon: the small-size variant (red card, black E), which
/// stays legible at taskbar size.
fn load_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../../assets/icon-taskbar.png");
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (w, h) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    })
}

/// Use Windows' own fonts: Segoe UI for the interface, plus the CJK fonts
/// that ship with Windows 11 so Chinese/Japanese/Korean text renders. Nothing
/// is bundled or downloaded.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    let candidates: [(&str, &str, bool); 5] = [
        ("segoe", "segoeui.ttf", true),
        ("yahei", "msyh.ttc", false),
        ("yugothic", "YuGothM.ttc", false),
        ("malgun", "malgun.ttf", false),
        ("segoe-symbol", "seguisym.ttf", false),
    ];
    let mut primary_set = false;
    for (name, file, primary) in candidates {
        let path = std::path::Path::new(&windir).join("Fonts").join(file);
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        fonts.font_data.insert(
            name.to_string(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );
        let fam = fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default();
        if primary {
            fam.insert(0, name.to_string());
            primary_set = true;
        } else {
            fam.push(name.to_string());
        }
        // CJK fallbacks for monospace text (paths, registry keys) too.
        if !primary {
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push(name.to_string());
        }
    }
    let _ = primary_set;
    ctx.set_fonts(fonts);
}

pub fn run(install_target: Option<String>, launch_args: Vec<String>) -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Expunge")
        .with_inner_size([1180.0, 760.0])
        .with_min_inner_size([820.0, 520.0])
        .with_app_id("Expunge");
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "Expunge",
        options,
        Box::new(|cc| {
            install_fonts(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc, install_target, launch_args)))
        }),
    )
}
