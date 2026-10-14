//! Expunge's look: a single dark theme with a red accent and red window border.

use egui::{Color32, CornerRadius, Stroke, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(229, 57, 53);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(120, 32, 32);
/// Native window border colour (RGB).
pub const BORDER_RGB: (u8, u8, u8) = (229, 57, 53);

pub const OK: Color32 = Color32::from_rgb(76, 201, 128);
pub const WARN: Color32 = Color32::from_rgb(255, 170, 80);
pub const BAD: Color32 = Color32::from_rgb(255, 99, 105);
pub const MUTED: Color32 = Color32::from_rgb(168, 158, 163);
pub const ADDED: Color32 = OK;
pub const CHANGED: Color32 = WARN;
pub const REMOVED: Color32 = BAD;

pub const BG: Color32 = Color32::from_rgb(20, 17, 19);
pub const SIDEBAR: Color32 = Color32::from_rgb(25, 21, 23);
pub const CARD: Color32 = Color32::from_rgb(34, 29, 32);
const PANEL: Color32 = SIDEBAR;
const WIDGET: Color32 = Color32::from_rgb(46, 40, 43);
const HOVER: Color32 = Color32::from_rgb(60, 51, 55);
pub const LINE: Color32 = Color32::from_rgb(52, 45, 48);
pub const NAV_SELECTED: Color32 = Color32::from_rgb(92, 34, 38);
const TEXT: Color32 = Color32::from_rgb(236, 230, 233);

pub fn apply(ctx: &egui::Context) {
    ctx.options_mut(|o| o.theme_preference = egui::ThemePreference::Dark);
    let mut v = Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.window_fill = PANEL;
    v.panel_fill = BG;
    v.faint_bg_color = Color32::from_rgb(22, 22, 25);
    v.extreme_bg_color = Color32::from_rgb(12, 12, 14);
    v.code_bg_color = Color32::from_rgb(30, 30, 34);
    v.hyperlink_color = Color32::from_rgb(255, 128, 120);
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.window_stroke = Stroke::new(1.0_f32, ACCENT_DIM);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(6);
    v.warn_fg_color = WARN;
    v.error_fg_color = BAD;

    let r = CornerRadius::same(8);
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.weak_bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.bg_fill = WIDGET;
    v.widgets.inactive.weak_bg_fill = WIDGET;
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.bg_fill = HOVER;
    v.widgets.hovered.weak_bg_fill = HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.hovered.corner_radius = r;
    v.widgets.active.bg_fill = ACCENT_DIM;
    v.widgets.active.weak_bg_fill = ACCENT_DIM;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.active.corner_radius = r;
    v.widgets.open.bg_fill = HOVER;
    v.widgets.open.weak_bg_fill = HOVER;
    v.widgets.open.corner_radius = r;
    ctx.set_visuals_of(egui::Theme::Dark, v);

    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size.y = 24.0;
        s.text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        s.text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        s.text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(18.0));
        s.text_styles.insert(
            egui::TextStyle::Name("Title".into()),
            egui::FontId::proportional(24.0),
        );
        s.text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        s.text_styles
            .insert(egui::TextStyle::Monospace, egui::FontId::monospace(13.0));
    });
}

/// A filled red pill button for the primary action of a view.
pub fn primary_button(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(text.into())
                .color(Color32::WHITE)
                .strong(),
        )
        .fill(ACCENT)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(16))
        .min_size(egui::vec2(0.0, 30.0)),
    )
}

/// A wide pill button spanning the available width (bottom-of-page action).
pub fn wide_button(ui: &mut egui::Ui, text: impl Into<String>, enabled: bool) -> egui::Response {
    let w = ui.available_width();
    let (fill, color) = if enabled {
        (ACCENT, Color32::WHITE)
    } else {
        (WIDGET, MUTED)
    };
    ui.add_enabled(
        enabled,
        egui::Button::new(egui::RichText::new(text.into()).color(color).strong())
            .fill(fill)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(18))
            .min_size(egui::vec2(w, 36.0)),
    )
}

/// A rounded card, like the panels in the main pages.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A red warning banner.
pub fn warning_banner(ui: &mut egui::Ui, text: impl Into<String>) {
    egui::Frame::new()
        .fill(Color32::from_rgb(64, 20, 24))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(170, 60, 64)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("⚠").color(Color32::from_rgb(255, 150, 150)));
                ui.label(
                    egui::RichText::new(text.into())
                        .color(Color32::from_rgb(255, 170, 170))
                        .strong(),
                );
            });
        });
}

/// Page title at the top of the content area.
pub fn page_title(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(title).text_style(egui::TextStyle::Name("Title".into())));
    if !subtitle.is_empty() {
        ui.label(egui::RichText::new(subtitle).color(MUTED));
    }
    ui.add_space(8.0);
}

/// One entry in the left navigation column.
pub fn nav_item(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    selected: bool,
    badge: Option<String>,
) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 38.0), egui::Sense::click());
    let painter = ui.painter_at(rect);
    let pill = rect.shrink2(egui::vec2(2.0, 2.0));
    if selected {
        painter.rect_filled(pill, CornerRadius::same(18), NAV_SELECTED);
    } else if resp.hovered() {
        painter.rect_filled(pill, CornerRadius::same(18), Color32::from_rgb(40, 34, 37));
    }
    let icon_color = if selected {
        Color32::from_rgb(255, 140, 136)
    } else {
        TEXT
    };
    painter.text(
        egui::pos2(rect.left() + 26.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(17.0),
        icon_color,
    );
    painter.text(
        egui::pos2(rect.left() + 54.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(14.0),
        if selected { Color32::WHITE } else { TEXT },
    );
    if let Some(b) = badge {
        let galley = painter.layout_no_wrap(b, egui::FontId::proportional(11.0), Color32::WHITE);
        let w = galley.size().x + 12.0;
        let br = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 12.0 - w / 2.0, rect.center().y),
            egui::vec2(w, 18.0),
        );
        painter.rect_filled(br, CornerRadius::same(9), ACCENT);
        painter.galley(br.center() - galley.size() / 2.0, galley, Color32::WHITE);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small coloured badge.
pub fn badge(ui: &mut egui::Ui, text: impl Into<String>, color: Color32) {
    let text = text.into();
    egui::Frame::new()
        .fill(color.gamma_multiply(0.18))
        .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.6)))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).color(color).small().strong());
        });
}

/// Frame for the main content area of a page.
pub fn content_frame() -> egui::Frame {
    egui::Frame::new().fill(BG).inner_margin(egui::Margin {
        left: 18,
        right: 18,
        top: 4,
        bottom: 12,
    })
}

/// Frame for a side panel inside a page (details pane).
pub fn side_frame() -> egui::Frame {
    egui::Frame::new().fill(BG).inner_margin(egui::Margin {
        left: 6,
        right: 18,
        top: 4,
        bottom: 12,
    })
}
