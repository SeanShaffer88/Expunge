//! Basic widgets: text, buttons, check boxes, text boxes, sliders, images.

use super::geom::*;
use super::style::*;
use super::text::Galley;
use super::ui::{Response, Ui, Widget};
use super::*;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Label
// ---------------------------------------------------------------------------

pub struct Label {
    text: WidgetText,
    wrap: Option<bool>,
    truncate: bool,
    sense: Option<Sense>,
}

impl Label {
    pub fn new(text: impl Into<WidgetText>) -> Label {
        Label {
            text: text.into(),
            wrap: None,
            truncate: false,
            sense: None,
        }
    }
    pub fn wrap(mut self) -> Self {
        self.wrap = Some(true);
        self
    }
    pub fn extend(mut self) -> Self {
        self.wrap = Some(false);
        self
    }
    /// One line, cut with "…" when too long.
    pub fn truncate(mut self) -> Self {
        self.truncate = true;
        self
    }
    pub fn selectable(self, _s: bool) -> Self {
        self
    }
    pub fn sense(mut self, s: Sense) -> Self {
        self.sense = Some(s);
        self
    }
}

/// Cut `text` so it fits `width` with an ellipsis.
fn truncated(ui: &Ui, g: &Arc<Galley>, width: f32) -> Arc<Galley> {
    if g.size.x <= width || g.rows.is_empty() {
        return g.clone();
    }
    let ell = ui.ctx().text_width("…", g.style);
    let row = &g.rows[0];
    let mut n = row.x.iter().rposition(|x| *x + ell <= width).unwrap_or(0);
    let chars: Vec<(usize, char)> = g.text[row.start..row.end].char_indices().collect();
    n = n.min(chars.len());
    let cut = chars.get(n).map_or(row.end - row.start, |(i, _)| *i);
    let mut s = g.text[row.start..row.start + cut].trim_end().to_string();
    s.push('…');
    ui.ctx().layout(&s, g.style, f32::INFINITY)
}

impl Widget for Label {
    fn ui(self, ui: &mut Ui) -> Response {
        let color = ui.text_color();
        let avail = ui.available_width();
        let wrap = if self.truncate {
            f32::INFINITY
        } else {
            match self.wrap {
                Some(true) => ui.max_rect().width().min(if ui.layout().is_horizontal() {
                    ui.max_rect().width()
                } else {
                    avail
                }),
                Some(false) => f32::INFINITY,
                None => ui.wrap_width(),
            }
        };
        let (mut g, mut c) = ui.layout_text(&self.text, &TextStyle::Body, wrap.max(1.0), color);
        if !ui.is_enabled() && self.text.rich().has_color() {
            c = c.gamma_multiply(0.5);
        }
        let full_text = g.text.clone();
        if self.truncate {
            g = truncated(ui, &g, avail.max(0.0));
        }
        let sense = self.sense.unwrap_or(Sense::hover());
        let (rect, resp) = ui.allocate_exact_size(g.size(), sense);
        if ui.is_rect_visible(rect) {
            ui.painter().galley(rect.min, g.clone(), c);
        }
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &full_text));
        if self.truncate && g.text != full_text {
            return resp.on_hover_text(full_text);
        }
        resp
    }
}

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

pub struct Button {
    text: WidgetText,
    fill: Option<Color32>,
    stroke: Option<Stroke>,
    radius: Option<CornerRadius>,
    min_size: Vec2,
    small: bool,
    frame: bool,
    selected: bool,
    sense: Sense,
}

impl Button {
    pub fn new(text: impl Into<WidgetText>) -> Button {
        Button {
            text: text.into(),
            fill: None,
            stroke: None,
            radius: None,
            min_size: Vec2::ZERO,
            small: false,
            frame: true,
            selected: false,
            sense: Sense::click(),
        }
    }
    pub fn fill(mut self, c: impl Into<Color32>) -> Self {
        self.fill = Some(c.into());
        self
    }
    pub fn stroke(mut self, s: impl Into<Stroke>) -> Self {
        self.stroke = Some(s.into());
        self
    }
    pub fn corner_radius(mut self, r: impl Into<CornerRadius>) -> Self {
        self.radius = Some(r.into());
        self
    }
    pub fn min_size(mut self, s: Vec2) -> Self {
        self.min_size = s;
        self
    }
    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }
    pub fn frame(mut self, f: bool) -> Self {
        self.frame = f;
        self
    }
    pub fn selected(mut self, s: bool) -> Self {
        self.selected = s;
        self
    }
    pub fn sense(mut self, s: Sense) -> Self {
        self.sense = s;
        self
    }
}

impl Widget for Button {
    fn ui(self, ui: &mut Ui) -> Response {
        let style = ui.style().clone();
        let pad = if self.small {
            vec2(style.spacing.button_padding.x * 0.5, 0.0)
        } else {
            style.spacing.button_padding
        };
        let text_style = if self.small {
            TextStyle::Small
        } else {
            TextStyle::Button
        };
        let base_color = ui.text_color();
        let (g, color) = ui.layout_text(&self.text, &text_style, f32::INFINITY, base_color);
        let min_h = if self.small {
            0.0
        } else {
            style.spacing.interact_size.y
        };
        let mut size = g.size() + pad * 2.0;
        size.y = size.y.max(min_h);
        size = size.max(self.min_size);
        // Centred and justified layouts (add_sized) give the button the full space.
        let layout = *ui.layout();
        if layout.main_justify && layout.cross_justify {
            size = size.max(ui.available_size());
        }
        let (rect, resp) = ui.allocate_exact_size(size, self.sense);
        if ui.is_rect_visible(rect) {
            let v = *style.visuals.widgets.style(
                resp.hovered(),
                resp.is_pointer_button_down_on(),
                ui.is_enabled(),
            );
            let painter = ui.painter();
            if self.frame || resp.hovered() || self.selected {
                let mut fill = if self.selected {
                    style.visuals.selection.bg_fill
                } else {
                    self.fill.unwrap_or(v.weak_bg_fill)
                };
                // A coloured button brightens a little under the pointer.
                if self.fill.is_some() && ui.is_enabled() {
                    if resp.is_pointer_button_down_on() {
                        fill = fill.lerp_to(Color32::BLACK, 0.15);
                    } else if resp.hovered() {
                        fill = fill.lerp_to(Color32::WHITE, 0.12);
                    }
                }
                if !self.frame && !self.selected {
                    fill = v.weak_bg_fill;
                }
                let stroke = self.stroke.unwrap_or(v.bg_stroke);
                let radius = self.radius.unwrap_or(v.corner_radius);
                let r = rect.expand(if self.fill.is_none() {
                    v.expansion
                } else {
                    0.0
                });
                painter.rect(r, radius, fill, stroke, StrokeKind::Inside);
            }
            let tc = if !ui.is_enabled() {
                color.gamma_multiply(0.6)
            } else if self.text.rich().has_color() {
                color
            } else {
                v.fg_stroke.color.max_alpha(color)
            };
            let pos = Align2::CENTER_CENTER
                .anchor_size(rect.center(), g.size())
                .min;
            painter.galley(pos, g.clone(), tc);
        }
        let text = self.text.text().to_string();
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), &text));
        if resp.clicked() && ui.in_menu {
            ui.close();
        }
        resp
    }
}

trait MaxAlpha {
    fn max_alpha(self, other: Color32) -> Color32;
}
impl MaxAlpha for Color32 {
    /// The widget's state colour, unless the theme overrides all text.
    fn max_alpha(self, other: Color32) -> Color32 {
        if other.a == 0 {
            self
        } else {
            other
        }
    }
}

// ---------------------------------------------------------------------------
// Checkbox and radio button
// ---------------------------------------------------------------------------

pub struct Checkbox<'a> {
    checked: &'a mut bool,
    text: Option<WidgetText>,
}

impl<'a> Checkbox<'a> {
    pub fn new(checked: &'a mut bool, text: impl Into<WidgetText>) -> Self {
        Checkbox {
            checked,
            text: Some(text.into()),
        }
    }
    pub fn without_text(checked: &'a mut bool) -> Self {
        Checkbox {
            checked,
            text: None,
        }
    }
}

fn check_like(
    ui: &mut Ui,
    text: Option<&WidgetText>,
    round: bool,
    on: bool,
    typ: WidgetType,
) -> Response {
    let style = ui.style().clone();
    let icon = style.spacing.icon_width;
    let gap = style.spacing.icon_spacing;
    let color = ui.text_color();
    let g = text.map(|t| {
        ui.layout_text(
            t,
            &TextStyle::Body,
            ui.wrap_width().max(1.0) - icon - gap,
            color,
        )
    });
    let mut size = vec2(icon, style.spacing.interact_size.y);
    if let Some((g, _)) = &g {
        size.x += gap + g.size().x;
        size.y = size.y.max(g.size().y);
    }
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let v = *style.visuals.widgets.style(
            resp.hovered(),
            resp.is_pointer_button_down_on(),
            ui.is_enabled(),
        );
        let p = ui.painter();
        let box_r = Rect::from_center_size(
            pos2(rect.min.x + icon / 2.0, rect.center().y),
            vec2(icon, icon),
        );
        if round {
            p.circle(box_r.center(), icon / 2.0, v.bg_fill, v.bg_stroke);
            if on {
                p.circle_filled(box_r.center(), icon / 4.0, v.fg_stroke.color);
            }
        } else {
            let fill = if on {
                style.visuals.selection.bg_fill
            } else {
                v.bg_fill
            };
            p.rect(
                box_r,
                v.corner_radius,
                fill,
                v.bg_stroke,
                StrokeKind::Inside,
            );
            if on {
                let s = Stroke::new(2.0_f32, Color32::WHITE);
                let r = box_r.shrink(3.0);
                p.add(Shape::line(
                    vec![
                        pos2(r.left(), r.center().y),
                        pos2(r.center().x - 1.0, r.bottom() - 1.0),
                        pos2(r.right(), r.top() + 1.0),
                    ],
                    s,
                ));
            }
        }
        if let Some((g, c)) = g.clone() {
            let pos = pos2(box_r.max.x + gap, rect.center().y - g.size().y / 2.0);
            let c = if ui.is_enabled() {
                c
            } else {
                c.gamma_multiply(0.5)
            };
            p.galley(pos, g, c);
        }
    }
    let label = text.map(|t| t.text().to_string()).unwrap_or_default();
    resp.widget_info(|| WidgetInfo::selected(typ, ui.is_enabled(), on, &label));
    resp
}

impl Widget for Checkbox<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let mut resp = check_like(
            ui,
            self.text.as_ref(),
            false,
            *self.checked,
            WidgetType::Checkbox,
        );
        if resp.clicked() {
            *self.checked = !*self.checked;
            resp.mark_changed();
        }
        resp
    }
}

pub struct RadioButton {
    checked: bool,
    text: WidgetText,
}

impl RadioButton {
    pub fn new(checked: bool, text: impl Into<WidgetText>) -> Self {
        RadioButton {
            checked,
            text: text.into(),
        }
    }
}

impl Widget for RadioButton {
    fn ui(self, ui: &mut Ui) -> Response {
        check_like(
            ui,
            Some(&self.text),
            true,
            self.checked,
            WidgetType::RadioButton,
        )
    }
}

// ---------------------------------------------------------------------------
// Selectable label
// ---------------------------------------------------------------------------

pub struct SelectableLabel {
    selected: bool,
    text: WidgetText,
}

impl SelectableLabel {
    pub fn new(selected: bool, text: impl Into<WidgetText>) -> Self {
        SelectableLabel {
            selected,
            text: text.into(),
        }
    }
}

impl Widget for SelectableLabel {
    fn ui(self, ui: &mut Ui) -> Response {
        let style = ui.style().clone();
        let pad = style.spacing.button_padding;
        let color = ui.text_color();
        let (g, c) = ui.layout_text(
            &self.text,
            &TextStyle::Button,
            ui.wrap_width().max(1.0) - pad.x * 2.0,
            color,
        );
        let mut size = g.size() + pad * 2.0;
        size.y = size.y.max(style.spacing.interact_size.y);
        if ui.layout().is_vertical() && ui.layout().cross_justify {
            size.x = size.x.max(ui.available_width());
        }
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        if ui.is_rect_visible(rect) {
            let v = *style.visuals.widgets.style(
                resp.hovered(),
                resp.is_pointer_button_down_on(),
                ui.is_enabled(),
            );
            let p = ui.painter();
            if self.selected {
                p.rect_filled(rect, v.corner_radius, style.visuals.selection.bg_fill);
            } else if resp.hovered() {
                p.rect(
                    rect,
                    v.corner_radius,
                    v.weak_bg_fill,
                    v.bg_stroke,
                    StrokeKind::Inside,
                );
            }
            let tc = if self.selected {
                style.visuals.selection.stroke.color
            } else if self.text.rich().has_color() {
                c
            } else {
                v.fg_stroke.color.max_alpha(c)
            };
            let pos = pos2(rect.min.x + pad.x, rect.center().y - g.size().y / 2.0);
            p.galley(
                pos,
                g,
                if ui.is_enabled() {
                    tc
                } else {
                    tc.gamma_multiply(0.5)
                },
            );
        }
        let label = self.text.text().to_string();
        resp.widget_info(|| {
            WidgetInfo::selected(
                WidgetType::SelectableLabel,
                ui.is_enabled(),
                self.selected,
                &label,
            )
        });
        if resp.clicked() && ui.in_menu {
            ui.close();
        }
        resp
    }
}

// ---------------------------------------------------------------------------
// Links
// ---------------------------------------------------------------------------

pub struct Link {
    text: WidgetText,
}

impl Link {
    pub fn new(text: impl Into<WidgetText>) -> Self {
        Link { text: text.into() }
    }
}

impl Widget for Link {
    fn ui(self, ui: &mut Ui) -> Response {
        let color = ui.visuals().hyperlink_color;
        let (g, c) = ui.layout_text(
            &self.text,
            &TextStyle::Body,
            ui.wrap_width().max(1.0),
            color,
        );
        let (rect, resp) = ui.allocate_exact_size(g.size(), Sense::click());
        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            p.galley(rect.min, g.clone(), c);
            if resp.hovered() {
                let y = rect.min.y + g.ascent + 2.0;
                p.line_segment(
                    [pos2(rect.min.x, y), pos2(rect.max.x, y)],
                    Stroke::new(1.0_f32, c),
                );
            }
        }
        let label = self.text.text().to_string();
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Link, ui.is_enabled(), &label));
        resp.on_hover_cursor(CursorIcon::PointingHand)
    }
}

pub struct Hyperlink {
    url: String,
    text: WidgetText,
}

impl Hyperlink {
    pub fn new(url: impl ToString) -> Self {
        let url = url.to_string();
        Hyperlink {
            text: url.clone().into(),
            url,
        }
    }
    pub fn from_label_and_url(text: impl Into<WidgetText>, url: impl ToString) -> Self {
        Hyperlink {
            url: url.to_string(),
            text: text.into(),
        }
    }
}

impl Widget for Hyperlink {
    fn ui(self, ui: &mut Ui) -> Response {
        let resp = Link::new(self.text).ui(ui);
        if resp.clicked() {
            ui.ctx().open_url(self.url.clone());
        }
        resp.on_hover_text(self.url)
    }
}

// ---------------------------------------------------------------------------
// Separator
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Separator {
    spacing: Option<f32>,
}

impl Separator {
    pub fn spacing(mut self, s: f32) -> Self {
        self.spacing = Some(s);
        self
    }
}

impl Widget for Separator {
    fn ui(self, ui: &mut Ui) -> Response {
        let sp = self.spacing.unwrap_or(6.0);
        let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
        if ui.layout().is_vertical() {
            let w = ui.available_width();
            let (rect, resp) = ui.allocate_exact_size(vec2(w, sp), Sense::hover());
            ui.painter().hline(rect.x_range(), rect.center().y, stroke);
            resp
        } else {
            let h = ui.style().spacing.interact_size.y;
            let (rect, resp) = ui.allocate_exact_size(vec2(sp, h), Sense::hover());
            ui.painter().vline(rect.center().x, rect.y_range(), stroke);
            resp
        }
    }
}

// ---------------------------------------------------------------------------
// Text box
// ---------------------------------------------------------------------------

/// Caret and selection, kept between frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextCursor {
    /// Byte offsets: where the selection started and where the caret is.
    pub anchor: usize,
    pub caret: usize,
}

impl TextCursor {
    fn range(&self) -> (usize, usize) {
        (self.anchor.min(self.caret), self.anchor.max(self.caret))
    }
}

pub struct TextEdit<'a> {
    text: &'a mut String,
    multiline: bool,
    hint: Option<WidgetText>,
    desired_width: Option<f32>,
    desired_rows: usize,
    char_limit: usize,
    interactive: bool,
    monospace: bool,
    id: Option<Id>,
    frame: bool,
    password: bool,
}

impl<'a> TextEdit<'a> {
    pub fn singleline(text: &'a mut String) -> Self {
        TextEdit {
            text,
            multiline: false,
            hint: None,
            desired_width: None,
            desired_rows: 1,
            char_limit: usize::MAX,
            interactive: true,
            monospace: false,
            id: None,
            frame: true,
            password: false,
        }
    }
    pub fn multiline(text: &'a mut String) -> Self {
        TextEdit {
            multiline: true,
            desired_rows: 4,
            ..TextEdit::singleline(text)
        }
    }
    pub fn hint_text(mut self, h: impl Into<WidgetText>) -> Self {
        self.hint = Some(h.into());
        self
    }
    pub fn desired_width(mut self, w: f32) -> Self {
        self.desired_width = Some(w);
        self
    }
    pub fn desired_rows(mut self, n: usize) -> Self {
        self.desired_rows = n.max(1);
        self
    }
    pub fn char_limit(mut self, n: usize) -> Self {
        self.char_limit = n;
        self
    }
    pub fn interactive(mut self, i: bool) -> Self {
        self.interactive = i;
        self
    }
    pub fn code_editor(mut self) -> Self {
        self.monospace = true;
        self
    }
    pub fn id(mut self, id: Id) -> Self {
        self.id = Some(id);
        self
    }
    pub fn id_salt(mut self, s: impl std::hash::Hash) -> Self {
        self.id = Some(Id::new(s));
        self
    }
    pub fn frame(mut self, f: bool) -> Self {
        self.frame = f;
        self
    }
    pub fn password(mut self, p: bool) -> Self {
        self.password = p;
        self
    }

    /// Show the box and also return the caret state.
    pub fn show(self, ui: &mut Ui) -> TextEditOutput {
        let response = self.ui(ui);
        TextEditOutput { response }
    }
}

pub struct TextEditOutput {
    pub response: Response,
}

fn prev_boundary(s: &str, i: usize) -> usize {
    s[..i].char_indices().next_back().map_or(0, |(k, _)| k)
}
fn next_boundary(s: &str, i: usize) -> usize {
    s[i..].chars().next().map_or(i, |c| i + c.len_utf8())
}
fn word_left(s: &str, i: usize) -> usize {
    let mut k = i;
    while k > 0 && s[..k].ends_with(|c: char| c.is_whitespace()) {
        k = prev_boundary(s, k);
    }
    while k > 0 && !s[..k].ends_with(|c: char| c.is_whitespace()) {
        k = prev_boundary(s, k);
    }
    k
}
fn word_right(s: &str, i: usize) -> usize {
    let mut k = i;
    while k < s.len() && !s[k..].starts_with(|c: char| c.is_whitespace()) {
        k = next_boundary(s, k);
    }
    while k < s.len() && s[k..].starts_with(|c: char| c.is_whitespace()) {
        k = next_boundary(s, k);
    }
    k
}

impl Widget for TextEdit<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let style = ui.style().clone();
        let id = self.id.unwrap_or_else(|| ui.next_auto_id());
        let ts = if self.monospace {
            TextStyle::Monospace
        } else {
            TextStyle::Body
        };
        let font = style.font(&ts);
        let text_style = super::text::TextStyleResolved {
            size: font.size,
            family: font.family,
            ..Default::default()
        };
        let pad = vec2(4.0, 2.0);
        let width = self
            .desired_width
            .unwrap_or(style.spacing.text_edit_width)
            .min(ui.available_width().max(24.0));
        let line_h = ui.ctx().with_fonts(|f| f.metrics_pt(&text_style).1);
        let rows = if self.multiline { self.desired_rows } else { 1 };
        let wrap = if self.multiline {
            width - pad.x * 2.0
        } else {
            f32::INFINITY
        };
        let shown = |t: &str| {
            if self.password {
                "•".repeat(t.chars().count())
            } else {
                t.to_string()
            }
        };
        let g0 = ui.ctx().layout(&shown(self.text), text_style, wrap);
        let h = (g0.size.y.max(line_h * rows as f32)).max(line_h) + pad.y * 2.0;
        let h = h.max(style.spacing.interact_size.y);
        let sense = if self.interactive && ui.is_enabled() {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        };
        let (_, rect) = ui.allocate_space(vec2(width, h));
        let mut resp = ui.interact(rect, id, sense);
        let text_origin = rect.min
            + pad
            + vec2(
                0.0,
                if self.multiline {
                    0.0
                } else {
                    (h - pad.y * 2.0 - line_h) / 2.0
                },
            );
        let mut cur = ui
            .data(|d| d.get_temp::<TextCursor>(id))
            .unwrap_or(TextCursor {
                anchor: self.text.len(),
                caret: self.text.len(),
            });
        cur.anchor = cur.anchor.min(self.text.len());
        cur.caret = cur.caret.min(self.text.len());
        if !self.text.is_char_boundary(cur.caret) || !self.text.is_char_boundary(cur.anchor) {
            cur = TextCursor {
                anchor: self.text.len(),
                caret: self.text.len(),
            };
        }
        let mut changed = false;
        if resp.has_focus() && self.interactive && ui.is_enabled() {
            ui.ctx().lock().wants_keyboard = true;
            // Pointer: place the caret, drag to select.
            if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                if resp.is_pointer_button_down_on() {
                    let idx = g0.index_at(p - text_origin.to_vec2());
                    let idx = map_shown_index(self.text, &g0.text, idx, self.password);
                    if ui.input(|i| i.pointer.primary_pressed()) {
                        if !ui.input(|i| i.modifiers.shift) {
                            cur.anchor = idx;
                        }
                        cur.caret = idx;
                    } else if resp.dragged() {
                        cur.caret = idx;
                    }
                }
            }
            if resp.double_clicked() {
                cur.anchor = word_left(self.text, cur.caret);
                cur.caret = word_right(self.text, cur.caret);
            }
            let events = ui.input(|i| i.events.clone());
            let mut consumed: Vec<usize> = Vec::new();
            for (ei, e) in events.iter().enumerate() {
                let (lo, hi) = cur.range();
                match e {
                    Event::Text(t) => {
                        let t: String = if self.multiline {
                            t.clone()
                        } else {
                            t.replace(['\n', '\r'], "")
                        };
                        let t: String = t
                            .chars()
                            .filter(|c| !c.is_control() || (*c == '\n' && self.multiline))
                            .collect();
                        if t.is_empty() {
                            continue;
                        }
                        let room = self.char_limit.saturating_sub(
                            self.text.chars().count() - self.text[lo..hi].chars().count(),
                        );
                        let t: String = t.chars().take(room).collect();
                        self.text.replace_range(lo..hi, &t);
                        cur.caret = lo + t.len();
                        cur.anchor = cur.caret;
                        changed = true;
                        consumed.push(ei);
                    }
                    Event::Paste(t) => {
                        let t = if self.multiline {
                            t.replace("\r\n", "\n")
                        } else {
                            t.lines().next().unwrap_or("").to_string()
                        };
                        let room = self.char_limit.saturating_sub(
                            self.text.chars().count() - self.text[lo..hi].chars().count(),
                        );
                        let t: String = t.chars().take(room).collect();
                        self.text.replace_range(lo..hi, &t);
                        cur.caret = lo + t.len();
                        cur.anchor = cur.caret;
                        changed = true;
                        consumed.push(ei);
                    }
                    Event::Copy | Event::Cut => {
                        if hi > lo && !self.password {
                            ui.ctx().copy_text(self.text[lo..hi].to_string());
                            if matches!(e, Event::Cut) {
                                self.text.replace_range(lo..hi, "");
                                cur = TextCursor {
                                    anchor: lo,
                                    caret: lo,
                                };
                                changed = true;
                            }
                        }
                        consumed.push(ei);
                    }
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        let s = &*self.text;
                        let shift = modifiers.shift;
                        let word = modifiers.ctrl;
                        let mut moved = None;
                        match key {
                            Key::ArrowLeft => {
                                moved = Some(if hi > lo && !shift {
                                    lo
                                } else if word {
                                    word_left(s, cur.caret)
                                } else {
                                    prev_boundary(s, cur.caret)
                                })
                            }
                            Key::ArrowRight => {
                                moved = Some(if hi > lo && !shift {
                                    hi
                                } else if word {
                                    word_right(s, cur.caret)
                                } else {
                                    next_boundary(s, cur.caret)
                                })
                            }
                            Key::Home => {
                                moved = Some(if self.multiline && !word {
                                    s[..cur.caret].rfind('\n').map_or(0, |i| i + 1)
                                } else {
                                    0
                                })
                            }
                            Key::End => {
                                moved = Some(if self.multiline && !word {
                                    s[cur.caret..].find('\n').map_or(s.len(), |i| cur.caret + i)
                                } else {
                                    s.len()
                                })
                            }
                            Key::ArrowUp | Key::ArrowDown if self.multiline => {
                                let p = g0.pos_of(cur.caret);
                                let dy = if *key == Key::ArrowUp {
                                    -g0.line_height
                                } else {
                                    g0.line_height
                                };
                                let target = pos2(p.x, p.y + dy + g0.line_height / 2.0);
                                if target.y >= 0.0 && target.y <= g0.size.y {
                                    moved = Some(g0.index_at(target));
                                }
                            }
                            Key::A if word => {
                                cur.anchor = 0;
                                cur.caret = s.len();
                            }
                            Key::Backspace => {
                                let start = if hi > lo {
                                    lo
                                } else if word {
                                    word_left(s, cur.caret)
                                } else {
                                    prev_boundary(s, cur.caret)
                                };
                                let end = if hi > lo { hi } else { cur.caret };
                                if end > start {
                                    self.text.replace_range(start..end, "");
                                    cur = TextCursor {
                                        anchor: start,
                                        caret: start,
                                    };
                                    changed = true;
                                }
                            }
                            Key::Delete => {
                                let (start, end) = if hi > lo {
                                    (lo, hi)
                                } else {
                                    (
                                        cur.caret,
                                        if word {
                                            word_right(s, cur.caret)
                                        } else {
                                            next_boundary(s, cur.caret)
                                        },
                                    )
                                };
                                if end > start {
                                    self.text.replace_range(start..end, "");
                                    cur = TextCursor {
                                        anchor: start,
                                        caret: start,
                                    };
                                    changed = true;
                                }
                            }
                            Key::Enter if self.multiline => {
                                self.text.replace_range(lo..hi, "\n");
                                cur.caret = lo + 1;
                                cur.anchor = cur.caret;
                                changed = true;
                            }
                            Key::Enter => {
                                // Left in the input so callers can check `key_pressed(Enter)`.
                                resp.surrender_focus();
                                resp.lost_focus = true;
                                ui.ctx()
                                    .memory_mut(|m| m.data.insert_temp(id.with("enter"), true));
                                continue;
                            }
                            Key::Escape => {
                                resp.surrender_focus();
                                resp.lost_focus = true;
                            }
                            _ => continue,
                        }
                        if let Some(m) = moved {
                            cur.caret = m;
                            if !shift {
                                cur.anchor = m;
                            }
                        }
                        consumed.push(ei);
                    }
                    _ => {}
                }
            }
            if !consumed.is_empty() {
                ui.input_mut(|i| {
                    let mut k = 0;
                    i.events.retain(|_| {
                        let keep = !consumed.contains(&k);
                        k += 1;
                        keep
                    });
                });
            }
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        } else if resp.hovered() && self.interactive {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }
        let g = if changed {
            ui.ctx().layout(&shown(self.text), text_style, wrap)
        } else {
            g0
        };
        ui.data_mut(|d| d.insert_temp(id, cur));
        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            let v = if resp.has_focus() {
                style.visuals.widgets.active
            } else {
                *style
                    .visuals
                    .widgets
                    .style(resp.hovered(), false, ui.is_enabled())
            };
            if self.frame {
                let stroke = if resp.has_focus() {
                    style.visuals.selection.stroke
                } else {
                    v.bg_stroke
                };
                p.rect(
                    rect,
                    v.corner_radius,
                    style.visuals.extreme_bg_color,
                    stroke,
                    StrokeKind::Inside,
                );
            }
            let clip = p.with_clip_rect(rect.shrink(1.0));
            let color = ui.text_color();
            if self.text.is_empty() {
                if let Some(h) = &self.hint {
                    let (hg, hc) = ui.layout_text(h, &ts, wrap, style.visuals.weak_text_color());
                    clip.galley(text_origin, hg, hc);
                }
            }
            // Keep the caret visible in a long single line.
            let mut origin = text_origin;
            if resp.has_focus() {
                let caret_pos = g.pos_of(shown_index(self.text, cur.caret, self.password));
                let visible_w = rect.width() - pad.x * 2.0;
                if !self.multiline && caret_pos.x > visible_w {
                    origin.x -= caret_pos.x - visible_w + 2.0;
                }
                let (lo, hi) = cur.range();
                if hi > lo {
                    for row in &g.rows {
                        let a = shown_index(self.text, lo, self.password).max(row.start);
                        let b = shown_index(self.text, hi, self.password).min(row.end);
                        if b > a
                            || (b == a
                                && row.end < shown_index(self.text, hi, self.password)
                                && a == row.end)
                        {
                            let xa = g.pos_of(a).x;
                            let xb = if b == row.end && !row.ends_paragraph {
                                row.width
                            } else {
                                g.pos_of(b).x
                            };
                            let r = Rect::from_min_size(
                                origin + vec2(xa, row.y),
                                vec2((xb - xa).max(3.0), g.line_height),
                            );
                            clip.rect_filled(r, 0, style.visuals.selection.bg_fill);
                        }
                    }
                }
                let cp = origin + caret_pos.to_vec2();
                clip.line_segment(
                    [cp, cp + vec2(0.0, g.line_height)],
                    Stroke::new(1.0_f32, color),
                );
                ui.ctx()
                    .set_ime_rect(Rect::from_min_size(cp, vec2(1.0, g.line_height)));
            }
            clip.galley(origin, g, color);
        }
        if changed {
            resp.mark_changed();
        }
        let label = self
            .hint
            .as_ref()
            .map(|h| h.text().to_string())
            .unwrap_or_default();
        let value = if self.password {
            String::new()
        } else {
            self.text.clone()
        };
        resp.widget_info(|| WidgetInfo {
            typ: WidgetType::TextEdit,
            enabled: true,
            label: Some(label),
            selected: None,
            value: Some(value),
        });
        resp
    }
}

/// Byte offset in the shown text (bullets for passwords) for one in the real text.
fn shown_index(real: &str, i: usize, password: bool) -> usize {
    if password {
        real[..i].chars().count() * '•'.len_utf8()
    } else {
        i
    }
}

fn map_shown_index(real: &str, _shown: &str, i: usize, password: bool) -> usize {
    if password {
        let n = i / '•'.len_utf8();
        real.char_indices().nth(n).map_or(real.len(), |(k, _)| k)
    } else {
        i
    }
}

impl Ui {
    /// Whether Enter was pressed in this text box this frame.
    pub fn text_edit_entered(&self, resp: &Response) -> bool {
        let k = resp.id.with("enter");
        let v = self.data(|d| d.get_temp::<bool>(k)).unwrap_or(false);
        if v {
            self.data_mut(|d| d.remove::<bool>(k));
        }
        v
    }
}

// ---------------------------------------------------------------------------
// Slider
// ---------------------------------------------------------------------------

pub trait Numeric: Copy {
    fn to_f64(self) -> f64;
    fn from_f64(v: f64) -> Self;
}
macro_rules! numeric {
    ($($t:ty),*) => {$(
        impl Numeric for $t {
            fn to_f64(self) -> f64 { self as f64 }
            fn from_f64(v: f64) -> Self { v as $t }
        }
    )*};
}
numeric!(f32, f64, u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

pub struct Slider<'a, T: Numeric> {
    value: &'a mut T,
    range: std::ops::RangeInclusive<T>,
    suffix: String,
    step: f64,
    text: Option<WidgetText>,
}

impl<'a, T: Numeric> Slider<'a, T> {
    pub fn new(value: &'a mut T, range: std::ops::RangeInclusive<T>) -> Self {
        Slider {
            value,
            range,
            suffix: String::new(),
            step: 0.0,
            text: None,
        }
    }
    pub fn suffix(mut self, s: impl ToString) -> Self {
        self.suffix = s.to_string();
        self
    }
    pub fn step_by(mut self, s: f64) -> Self {
        self.step = s;
        self
    }
    pub fn text(mut self, t: impl Into<WidgetText>) -> Self {
        self.text = Some(t.into());
        self
    }
}

impl<T: Numeric> Widget for Slider<'_, T> {
    fn ui(self, ui: &mut Ui) -> Response {
        let style = ui.style().clone();
        let (lo, hi) = (self.range.start().to_f64(), self.range.end().to_f64());
        let w = style.spacing.slider_width;
        let h = style.spacing.interact_size.y;
        let r = ui.horizontal(|ui| {
            let (rect, mut resp) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
            let rail = Rect::from_center_size(rect.center(), vec2(w - h * 0.5, 4.0));
            let mut v = self.value.to_f64();
            let mut changed = false;
            if resp.is_pointer_button_down_on() {
                if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                    let t = ((p.x - rail.min.x) / rail.width()).clamp(0.0, 1.0) as f64;
                    let mut nv = lo + (hi - lo) * t;
                    if self.step > 0.0 {
                        nv = (nv / self.step).round() * self.step;
                    }
                    if (nv - v).abs() > f64::EPSILON {
                        v = nv.clamp(lo, hi);
                        changed = true;
                    }
                }
            }
            if resp.has_focus() {
                let step = if self.step > 0.0 {
                    self.step
                } else {
                    (hi - lo) / 100.0
                };
                let (l, rgt) = ui.input_mut(|i| {
                    (
                        i.consume_key(Modifiers::NONE, Key::ArrowLeft),
                        i.consume_key(Modifiers::NONE, Key::ArrowRight),
                    )
                });
                if l || rgt {
                    v = (v + if rgt { step } else { -step }).clamp(lo, hi);
                    changed = true;
                }
            }
            if changed {
                *self.value = T::from_f64(v);
                resp.mark_changed();
            }
            let t = if hi > lo {
                ((v - lo) / (hi - lo)) as f32
            } else {
                0.0
            };
            let vis = *style.visuals.widgets.style(
                resp.hovered(),
                resp.is_pointer_button_down_on(),
                ui.is_enabled(),
            );
            let p = ui.painter();
            p.rect_filled(rail, 2, style.visuals.widgets.inactive.bg_fill);
            let filled =
                Rect::from_min_max(rail.min, pos2(lerp(rail.min.x, rail.max.x, t), rail.max.y));
            p.rect_filled(filled, 2, style.visuals.selection.bg_fill);
            p.circle(
                pos2(filled.max.x, rail.center().y),
                h * 0.32,
                vis.bg_fill,
                vis.fg_stroke,
            );
            let shown = if (v.fract()).abs() < 1e-9 {
                format!("{}{}", v as i64, self.suffix)
            } else {
                format!("{:.2}{}", v, self.suffix)
            };
            ui.label(shown);
            if let Some(t) = &self.text {
                ui.label(t.clone());
            }
            resp
        });
        r.inner
    }
}

// ---------------------------------------------------------------------------
// Image, progress bar, spinner
// ---------------------------------------------------------------------------

pub struct Image {
    texture: TextureHandle,
    size: Option<Vec2>,
    tint: Color32,
}

impl Image {
    pub fn new(texture: &TextureHandle) -> Self {
        Image {
            texture: texture.clone(),
            size: None,
            tint: Color32::WHITE,
        }
    }
    pub fn fit_to_exact_size(mut self, s: Vec2) -> Self {
        self.size = Some(s);
        self
    }
    pub fn max_size(self, s: Vec2) -> Self {
        self.fit_to_exact_size(s)
    }
    pub fn tint(mut self, c: impl Into<Color32>) -> Self {
        self.tint = c.into();
        self
    }
}

impl From<&TextureHandle> for Image {
    fn from(t: &TextureHandle) -> Self {
        Image::new(t)
    }
}

impl Widget for Image {
    fn ui(self, ui: &mut Ui) -> Response {
        let size = self.size.unwrap_or_else(|| self.texture.size_vec2());
        let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
        if ui.is_rect_visible(rect) {
            ui.painter().image(&self.texture, rect, self.tint);
        }
        resp
    }
}

pub struct ProgressBar {
    progress: f32,
    height: Option<f32>,
    text: Option<WidgetText>,
}

impl ProgressBar {
    pub fn new(progress: f32) -> Self {
        ProgressBar {
            progress: progress.clamp(0.0, 1.0),
            height: None,
            text: None,
        }
    }
    pub fn desired_height(mut self, h: f32) -> Self {
        self.height = Some(h);
        self
    }
    pub fn text(mut self, t: impl Into<WidgetText>) -> Self {
        self.text = Some(t.into());
        self
    }
    pub fn show_percentage(mut self) -> Self {
        self.text = Some(format!("{}%", (self.progress * 100.0).round() as i32).into());
        self
    }
}

impl Widget for ProgressBar {
    fn ui(self, ui: &mut Ui) -> Response {
        let h = self.height.unwrap_or(ui.style().spacing.interact_size.y);
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
        let v = ui.visuals().clone();
        let p = ui.painter();
        let radius = (h / 2.0).min(255.0) as u8;
        p.rect_filled(rect, radius, v.extreme_bg_color);
        let filled = Rect::from_min_size(rect.min, vec2(rect.width() * self.progress, h));
        if filled.width() > 0.5 {
            p.rect_filled(filled, radius, v.selection.bg_fill);
        }
        if let Some(t) = self.text {
            let (g, c) = ui.layout_text(&t, &TextStyle::Body, f32::INFINITY, v.text_color());
            p.galley(
                Align2::CENTER_CENTER
                    .anchor_size(rect.center(), g.size())
                    .min,
                g,
                c,
            );
        }
        resp
    }
}

// ---------------------------------------------------------------------------
// Convenience methods on Ui
// ---------------------------------------------------------------------------

impl Ui {
    pub fn label(&mut self, text: impl Into<WidgetText>) -> Response {
        self.add(Label::new(text))
    }
    pub fn colored_label(
        &mut self,
        color: impl Into<Color32>,
        text: impl Into<WidgetText>,
    ) -> Response {
        let t: WidgetText = text.into();
        self.label(t.0.color(color))
    }
    pub fn heading(&mut self, text: impl Into<RichText>) -> Response {
        self.label(text.into().heading())
    }
    pub fn strong(&mut self, text: impl Into<RichText>) -> Response {
        self.label(text.into().strong())
    }
    pub fn weak(&mut self, text: impl Into<RichText>) -> Response {
        self.label(text.into().weak())
    }
    pub fn small(&mut self, text: impl Into<RichText>) -> Response {
        self.label(text.into().small())
    }
    pub fn monospace(&mut self, text: impl Into<RichText>) -> Response {
        self.label(text.into().monospace())
    }
    pub fn code(&mut self, text: impl Into<RichText>) -> Response {
        self.monospace(text)
    }
    pub fn button(&mut self, text: impl Into<WidgetText>) -> Response {
        self.add(Button::new(text))
    }
    pub fn small_button(&mut self, text: impl Into<WidgetText>) -> Response {
        self.add(Button::new(text).small())
    }
    pub fn checkbox(&mut self, checked: &mut bool, text: impl Into<WidgetText>) -> Response {
        self.add(Checkbox::new(checked, text))
    }
    pub fn radio(&mut self, selected: bool, text: impl Into<WidgetText>) -> Response {
        self.add(RadioButton::new(selected, text))
    }
    pub fn radio_value<V: PartialEq>(
        &mut self,
        current: &mut V,
        value: V,
        text: impl Into<WidgetText>,
    ) -> Response {
        let mut r = self.radio(*current == value, text);
        if r.clicked() && *current != value {
            *current = value;
            r.mark_changed();
        }
        r
    }
    pub fn selectable_label(&mut self, selected: bool, text: impl Into<WidgetText>) -> Response {
        self.add(SelectableLabel::new(selected, text))
    }
    pub fn selectable_value<V: PartialEq>(
        &mut self,
        current: &mut V,
        value: V,
        text: impl Into<WidgetText>,
    ) -> Response {
        let mut r = self.selectable_label(*current == value, text);
        if r.clicked() && *current != value {
            *current = value;
            r.mark_changed();
        }
        r
    }
    pub fn link(&mut self, text: impl Into<WidgetText>) -> Response {
        self.add(Link::new(text))
    }
    pub fn hyperlink(&mut self, url: impl ToString) -> Response {
        self.add(Hyperlink::new(url))
    }
    pub fn hyperlink_to(&mut self, text: impl Into<WidgetText>, url: impl ToString) -> Response {
        self.add(Hyperlink::from_label_and_url(text, url))
    }
    pub fn separator(&mut self) -> Response {
        self.add(Separator::default())
    }
    pub fn text_edit_singleline(&mut self, text: &mut String) -> Response {
        self.add(TextEdit::singleline(text))
    }
    pub fn text_edit_multiline(&mut self, text: &mut String) -> Response {
        self.add(TextEdit::multiline(text))
    }
    pub fn image(&mut self, img: impl Into<Image>) -> Response {
        self.add(img.into())
    }
    pub fn spinner(&mut self) -> Response {
        let size = self.style().spacing.interact_size.y;
        let (rect, resp) = self.allocate_exact_size(vec2(size, size), Sense::hover());
        let time = self.input(|i| i.time);
        let color = self.visuals().strong_text_color();
        let radius = rect.height() / 2.0 - 2.0;
        let start = time * std::f64::consts::TAU;
        let end = start + 4.2;
        let pts: Vec<Pos2> = (0..=24)
            .map(|i| {
                let a = start + (end - start) * i as f64 / 24.0;
                rect.center() + vec2(a.cos() as f32, a.sin() as f32) * radius
            })
            .collect();
        self.painter()
            .add(Shape::line(pts, Stroke::new(3.0_f32, color)));
        self.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::super::context::Context;
    use super::*;

    fn frame(ctx: &Context, events: Vec<Event>, f: impl FnOnce(&mut Ui)) {
        let raw = RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))),
            ..Default::default()
        };
        ctx.run(raw, |ctx| {
            let mut ui = Ui::new(
                ctx.clone(),
                LayerId::background(),
                Id::new("root"),
                ctx.screen_rect(),
            );
            ctx.register_area(LayerId::background(), ctx.screen_rect());
            f(&mut ui);
        });
    }

    fn click_at(p: Pos2) -> Vec<Vec<Event>> {
        vec![
            vec![Event::PointerMoved(p)],
            vec![Event::PointerButton {
                pos: p,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }],
            vec![Event::PointerButton {
                pos: p,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        ]
    }

    #[test]
    fn button_clicks_and_checkbox_toggles() {
        let ctx = Context::default();
        let mut rect = Rect::NOTHING;
        frame(&ctx, vec![], |ui| rect = ui.button("Press").rect);
        let mut clicks = 0;
        for ev in click_at(rect.center()) {
            frame(&ctx, ev, |ui| {
                if ui.button("Press").clicked() {
                    clicks += 1;
                }
            });
        }
        assert_eq!(clicks, 1);

        let ctx = Context::default();
        let mut on = false;
        let mut r = Rect::NOTHING;
        frame(&ctx, vec![], |ui| r = ui.checkbox(&mut on, "Tick").rect);
        for ev in click_at(r.center()) {
            frame(&ctx, ev, |ui| {
                ui.checkbox(&mut on, "Tick");
            });
        }
        assert!(on);
    }

    #[test]
    fn typing_into_a_text_box() {
        let ctx = Context::default();
        let mut s = String::from("ab");
        let mut r = Rect::NOTHING;
        frame(&ctx, vec![], |ui| r = ui.text_edit_singleline(&mut s).rect);
        for ev in click_at(r.center()) {
            frame(&ctx, ev, |ui| {
                ui.text_edit_singleline(&mut s);
            });
        }
        let key = |k| Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        frame(
            &ctx,
            vec![
                key(Key::End),
                Event::Text("cd".into()),
                key(Key::Backspace),
                key(Key::Home),
                Event::Text("X".into()),
            ],
            |ui| {
                ui.text_edit_singleline(&mut s);
            },
        );
        assert_eq!(s, "Xabc");
        assert!(ctx.memory(|m| m.focused().is_some()));
    }

    #[test]
    fn rows_place_widgets_side_by_side_and_columns_below() {
        let ctx = Context::default();
        let (mut a, mut b, mut c) = (Rect::NOTHING, Rect::NOTHING, Rect::NOTHING);
        frame(&ctx, vec![], |ui| {
            ui.horizontal(|ui| {
                a = ui.button("One").rect;
                b = ui.button("Two").rect;
            });
            c = ui.label("Below").rect;
        });
        assert!(b.min.x > a.max.x);
        assert!((a.center().y - b.center().y).abs() < 0.01);
        assert!(c.min.y >= a.max.y);
    }
}
