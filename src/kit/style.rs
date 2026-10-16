//! Colours, sizes and text styles shared by all widgets.

use super::geom::*;
use super::text::{FontFamily, FontId, TextStyleResolved};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TextStyle {
    Small,
    Body,
    Monospace,
    Button,
    Heading,
    Name(Arc<str>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidgetVisuals {
    pub bg_fill: Color32,
    pub weak_bg_fill: Color32,
    pub bg_stroke: Stroke,
    pub corner_radius: CornerRadius,
    pub fg_stroke: Stroke,
    pub expansion: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Widgets {
    pub noninteractive: WidgetVisuals,
    pub inactive: WidgetVisuals,
    pub hovered: WidgetVisuals,
    pub active: WidgetVisuals,
    pub open: WidgetVisuals,
}

impl Widgets {
    /// Visuals for a widget in this interaction state.
    pub fn style(&self, hovered: bool, pressed: bool, enabled: bool) -> &WidgetVisuals {
        if !enabled {
            &self.noninteractive
        } else if pressed {
            &self.active
        } else if hovered {
            &self.hovered
        } else {
            &self.inactive
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    pub bg_fill: Color32,
    pub stroke: Stroke,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Visuals {
    pub dark_mode: bool,
    pub override_text_color: Option<Color32>,
    pub widgets: Widgets,
    pub selection: Selection,
    pub hyperlink_color: Color32,
    pub faint_bg_color: Color32,
    pub extreme_bg_color: Color32,
    pub code_bg_color: Color32,
    pub warn_fg_color: Color32,
    pub error_fg_color: Color32,
    pub window_corner_radius: CornerRadius,
    pub window_shadow: Shadow,
    pub window_fill: Color32,
    pub window_stroke: Stroke,
    pub menu_corner_radius: CornerRadius,
    pub panel_fill: Color32,
    pub popup_shadow: Shadow,
    pub striped: bool,
}

impl Visuals {
    pub fn dark() -> Visuals {
        let wv = |bg: u8, stroke: Option<(f32, u8)>, fg: u8| WidgetVisuals {
            bg_fill: Color32::from_gray(bg),
            weak_bg_fill: Color32::from_gray(bg),
            bg_stroke: stroke.map_or(Stroke::NONE, |(w, c)| Stroke::new(w, Color32::from_gray(c))),
            corner_radius: CornerRadius::same(2),
            fg_stroke: Stroke::new(1.0_f32, Color32::from_gray(fg)),
            expansion: 0.0,
        };
        Visuals {
            dark_mode: true,
            override_text_color: None,
            widgets: Widgets {
                noninteractive: wv(27, Some((1.0, 60)), 140),
                inactive: wv(60, None, 180),
                hovered: WidgetVisuals {
                    expansion: 1.0,
                    ..wv(70, Some((1.0, 150)), 240)
                },
                active: WidgetVisuals {
                    expansion: 1.0,
                    ..wv(55, Some((1.0, 255)), 255)
                },
                open: wv(45, Some((1.0, 60)), 210),
            },
            selection: Selection {
                bg_fill: Color32::from_rgb(0, 92, 128),
                stroke: Stroke::new(1.0_f32, Color32::from_rgb(192, 222, 255)),
            },
            hyperlink_color: Color32::from_rgb(90, 170, 255),
            faint_bg_color: Color32::from_gray(5),
            extreme_bg_color: Color32::from_gray(10),
            code_bg_color: Color32::from_gray(64),
            warn_fg_color: Color32::from_rgb(255, 143, 0),
            error_fg_color: Color32::from_rgb(255, 0, 0),
            window_corner_radius: CornerRadius::same(6),
            window_shadow: Shadow {
                offset: [10, 20],
                blur: 15,
                spread: 0,
                color: Color32::from_black_alpha(96),
            },
            window_fill: Color32::from_gray(27),
            window_stroke: Stroke::new(1.0_f32, Color32::from_gray(60)),
            menu_corner_radius: CornerRadius::same(6),
            panel_fill: Color32::from_gray(27),
            popup_shadow: Shadow {
                offset: [6, 10],
                blur: 8,
                spread: 0,
                color: Color32::from_black_alpha(96),
            },
            striped: false,
        }
    }

    pub fn text_color(&self) -> Color32 {
        self.override_text_color
            .unwrap_or(self.widgets.noninteractive.fg_stroke.color)
    }
    pub fn strong_text_color(&self) -> Color32 {
        self.widgets.active.fg_stroke.color
    }
    pub fn weak_text_color(&self) -> Color32 {
        self.text_color().gamma_multiply(0.6)
    }
}

impl Default for Visuals {
    fn default() -> Self {
        Visuals::dark()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spacing {
    pub item_spacing: Vec2,
    pub button_padding: Vec2,
    pub interact_size: Vec2,
    pub indent: f32,
    pub icon_width: f32,
    pub icon_spacing: f32,
    pub window_margin: Margin,
    pub menu_margin: Margin,
    pub scroll_bar_width: f32,
    pub combo_width: f32,
    pub text_edit_width: f32,
    pub slider_width: f32,
}

impl Default for Spacing {
    fn default() -> Self {
        Spacing {
            item_spacing: vec2(8.0, 3.0),
            button_padding: vec2(4.0, 1.0),
            interact_size: vec2(40.0, 18.0),
            indent: 18.0,
            icon_width: 14.0,
            icon_spacing: 4.0,
            window_margin: Margin::same(12),
            menu_margin: Margin::same(6),
            scroll_bar_width: 10.0,
            combo_width: 100.0,
            text_edit_width: 280.0,
            slider_width: 100.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub spacing: Spacing,
    pub visuals: Visuals,
    pub text_styles: BTreeMap<TextStyle, FontId>,
    /// Seconds for short transitions; 0 turns animation off.
    pub animation_time: f32,
    pub scroll_animation: ScrollAnimation,
    pub override_text_style: Option<TextStyle>,
}

impl Default for Style {
    fn default() -> Self {
        let mut text_styles = BTreeMap::new();
        text_styles.insert(TextStyle::Small, FontId::proportional(9.0));
        text_styles.insert(TextStyle::Body, FontId::proportional(12.5));
        text_styles.insert(TextStyle::Button, FontId::proportional(12.5));
        text_styles.insert(TextStyle::Heading, FontId::proportional(18.0));
        text_styles.insert(TextStyle::Monospace, FontId::monospace(12.0));
        Style {
            spacing: Spacing::default(),
            visuals: Visuals::dark(),
            text_styles,
            animation_time: 1.0 / 12.0,
            scroll_animation: ScrollAnimation::default(),
            override_text_style: None,
        }
    }
}

impl Style {
    pub fn font(&self, ts: &TextStyle) -> FontId {
        self.text_styles
            .get(ts)
            .copied()
            .or_else(|| self.text_styles.get(&TextStyle::Body).copied())
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ScrollAnimation {
    pub duration: f32,
}

impl ScrollAnimation {
    pub fn duration(seconds: f32) -> ScrollAnimation {
        ScrollAnimation { duration: seconds }
    }
}

/// Text with formatting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichText {
    text: String,
    size: Option<f32>,
    text_style: Option<TextStyle>,
    monospace: bool,
    color: Option<Color32>,
    strong: bool,
    weak: bool,
    italics: bool,
    underline: bool,
    strikethrough: bool,
    background: Option<Color32>,
}

impl RichText {
    pub fn new(text: impl Into<String>) -> RichText {
        RichText {
            text: text.into(),
            ..Default::default()
        }
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn size(mut self, s: f32) -> Self {
        self.size = Some(s);
        self
    }
    pub fn text_style(mut self, ts: TextStyle) -> Self {
        self.text_style = Some(ts);
        self
    }
    pub fn small(self) -> Self {
        self.text_style(TextStyle::Small)
    }
    pub fn heading(self) -> Self {
        self.text_style(TextStyle::Heading)
    }
    pub fn monospace(mut self) -> Self {
        self.monospace = true;
        self.text_style(TextStyle::Monospace)
    }
    pub fn code(self) -> Self {
        self.monospace()
    }
    pub fn color(mut self, c: impl Into<Color32>) -> Self {
        self.color = Some(c.into());
        self
    }
    pub fn strong(mut self) -> Self {
        self.strong = true;
        self
    }
    pub fn weak(mut self) -> Self {
        self.weak = true;
        self
    }
    pub fn italics(mut self) -> Self {
        self.italics = true;
        self
    }
    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }
    pub fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }
    pub fn background_color(mut self, c: impl Into<Color32>) -> Self {
        self.background = Some(c.into());
        self
    }
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Font and colour, given the style and the widget's defaults.
    pub fn resolve(
        &self,
        style: &Style,
        default_style: &TextStyle,
        default_color: Color32,
    ) -> (TextStyleResolved, Color32) {
        let ts = self
            .text_style
            .clone()
            .or_else(|| style.override_text_style.clone())
            .unwrap_or_else(|| default_style.clone());
        let mut font = style.font(&ts);
        if let Some(s) = self.size {
            font.size = s;
        }
        if self.monospace {
            font.family = FontFamily::Monospace;
        }
        let color = if let Some(c) = self.color {
            c
        } else if self.strong {
            style.visuals.strong_text_color()
        } else if self.weak {
            style.visuals.weak_text_color()
        } else {
            default_color
        };
        (
            TextStyleResolved {
                size: font.size,
                family: font.family,
                bold: false,
                italic: self.italics,
                underline: self.underline,
                strikethrough: self.strikethrough,
            },
            color,
        )
    }

    pub fn has_color(&self) -> bool {
        self.color.is_some()
    }
}

impl From<&str> for RichText {
    fn from(s: &str) -> Self {
        RichText::new(s)
    }
}
impl From<String> for RichText {
    fn from(s: String) -> Self {
        RichText::new(s)
    }
}
impl From<&String> for RichText {
    fn from(s: &String) -> Self {
        RichText::new(s.clone())
    }
}

/// Anything a widget can show as its text.
#[derive(Clone, Debug, PartialEq)]
pub struct WidgetText(pub RichText);

impl WidgetText {
    pub fn text(&self) -> &str {
        self.0.text()
    }
    pub fn rich(&self) -> &RichText {
        &self.0
    }
}

impl From<&str> for WidgetText {
    fn from(s: &str) -> Self {
        WidgetText(RichText::new(s))
    }
}
impl From<String> for WidgetText {
    fn from(s: String) -> Self {
        WidgetText(RichText::new(s))
    }
}
impl From<&String> for WidgetText {
    fn from(s: &String) -> Self {
        WidgetText(RichText::new(s.clone()))
    }
}
impl From<RichText> for WidgetText {
    fn from(r: RichText) -> Self {
        WidgetText(r)
    }
}
impl From<&RichText> for WidgetText {
    fn from(r: &RichText) -> Self {
        WidgetText(r.clone())
    }
}
impl From<std::borrow::Cow<'_, str>> for WidgetText {
    fn from(s: std::borrow::Cow<'_, str>) -> Self {
        WidgetText(RichText::new(s.into_owned()))
    }
}
