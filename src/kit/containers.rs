//! Containers: frames, panels, scroll areas, grids, collapsing sections,
//! combo boxes, menus, tooltips, windows and modal dialogs.

use super::context::Context;
use super::geom::*;
use super::style::*;
use super::ui::{InnerResponse, Layout, Response, Ui, UiBuilder};
use super::*;

// ---------------------------------------------------------------------------
// Frame
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub fill: Color32,
    pub stroke: Stroke,
    pub corner_radius: CornerRadius,
    pub inner_margin: Margin,
    pub outer_margin: Margin,
    pub shadow: Shadow,
}

impl Frame {
    pub fn new() -> Frame {
        Frame::default()
    }
    pub fn none() -> Frame {
        Frame::default()
    }
    pub fn group(style: &Style) -> Frame {
        Frame {
            stroke: style.visuals.widgets.noninteractive.bg_stroke,
            corner_radius: style.visuals.widgets.noninteractive.corner_radius,
            inner_margin: Margin::same(6),
            ..Frame::default()
        }
    }
    pub fn window(style: &Style) -> Frame {
        Frame {
            fill: style.visuals.window_fill,
            stroke: style.visuals.window_stroke,
            corner_radius: style.visuals.window_corner_radius,
            inner_margin: style.spacing.window_margin,
            shadow: style.visuals.window_shadow,
            ..Frame::default()
        }
    }
    pub fn popup(style: &Style) -> Frame {
        Frame {
            fill: style.visuals.window_fill,
            stroke: style.visuals.window_stroke,
            corner_radius: style.visuals.menu_corner_radius,
            inner_margin: style.spacing.menu_margin,
            shadow: style.visuals.popup_shadow,
            ..Frame::default()
        }
    }
    pub fn fill(mut self, c: impl Into<Color32>) -> Self {
        self.fill = c.into();
        self
    }
    pub fn stroke(mut self, s: impl Into<Stroke>) -> Self {
        self.stroke = s.into();
        self
    }
    pub fn corner_radius(mut self, r: impl Into<CornerRadius>) -> Self {
        self.corner_radius = r.into();
        self
    }
    pub fn inner_margin(mut self, m: impl Into<Margin>) -> Self {
        self.inner_margin = m.into();
        self
    }
    pub fn outer_margin(mut self, m: impl Into<Margin>) -> Self {
        self.outer_margin = m.into();
        self
    }
    pub fn shadow(mut self, s: Shadow) -> Self {
        self.shadow = s;
        self
    }

    /// Paint the frame behind content laid out by `add`.
    pub fn show<R>(self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        // Reserve a slot in the shape list now, so the background goes under the content.
        let slot = ShapeSlot::reserve(ui);
        let avail = ui.available_rect_before_wrap();
        let outer_min = avail.min + self.outer_margin.left_top();
        let inner_rect = Rect::from_min_max(
            outer_min + self.inner_margin.left_top(),
            pos2(
                avail.max.x - self.outer_margin.right as f32 - self.inner_margin.right as f32,
                avail.max.y.max(avail.min.y),
            ),
        );
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(inner_rect)
                .layout(*ui.layout_for_frame()),
        );
        let inner = add(&mut child);
        let content = child.min_rect();
        let frame_rect = Rect::from_min_max(
            content.min - self.inner_margin.left_top(),
            content.max + self.inner_margin.right_bottom(),
        );
        let frame_rect = Rect::from_min_max(
            pos2(
                frame_rect.min.x.min(outer_min.x),
                frame_rect.min.y.min(outer_min.y),
            ),
            frame_rect.max,
        );
        slot.fill(ui, self.shapes(frame_rect));
        let outer = Rect::from_min_max(
            frame_rect.min - self.outer_margin.left_top(),
            frame_rect.max + self.outer_margin.right_bottom(),
        );
        let response = ui.allocate_rect_for_child(outer);
        InnerResponse::new(inner, response)
    }

    pub fn shapes(&self, rect: Rect) -> Vec<Shape> {
        let mut v = Vec::new();
        if self.shadow.color.a > 0 {
            v.push(Shape::Shadow {
                rect,
                radius: self.corner_radius,
                shadow: self.shadow,
            });
        }
        if self.fill.a > 0 || !self.stroke.is_empty() {
            v.push(Shape::Rect {
                rect,
                radius: self.corner_radius,
                fill: self.fill,
                stroke: self.stroke,
                kind: StrokeKind::Inside,
            });
        }
        v
    }
}

/// A place in a layer's shape list filled in later.
pub(crate) struct ShapeSlot {
    layer: LayerId,
    index: usize,
}

impl ShapeSlot {
    pub(crate) fn reserve(ui: &Ui) -> ShapeSlot {
        let layer = ui.layer_id();
        let index = ui.ctx().reserve_shape(layer, ui.clip_rect());
        ShapeSlot { layer, index }
    }
    pub(crate) fn fill(self, ui: &Ui, shapes: Vec<Shape>) {
        let fade = ui.opacity();
        let shapes = shapes
            .into_iter()
            .map(|s| match s {
                Shape::Rect {
                    rect,
                    radius,
                    fill,
                    stroke,
                    kind,
                } if fade < 1.0 => Shape::Rect {
                    rect,
                    radius,
                    fill: fill.gamma_multiply(fade),
                    stroke: Stroke::new(stroke.width, stroke.color.gamma_multiply(fade)),
                    kind,
                },
                s => s,
            })
            .collect();
        ui.ctx().fill_shape_slot(self.layer, self.index, shapes);
    }
}

impl Context {
    pub(crate) fn reserve_shape(&self, layer: LayerId, clip: Rect) -> usize {
        let mut c = self.lock();
        let v = c.layers_mut().entry(layer).or_default();
        v.push(super::paint::ClippedShape {
            clip,
            shape: Shape::Noop,
        });
        v.len() - 1
    }
    pub(crate) fn fill_shape_slot(&self, layer: LayerId, index: usize, shapes: Vec<Shape>) {
        let mut c = self.lock();
        let Some(v) = c.layers_mut().get_mut(&layer) else {
            return;
        };
        if index >= v.len() {
            return;
        }
        let clip = v[index].clip;
        let mut iter = shapes.into_iter();
        if let Some(first) = iter.next() {
            v[index].shape = first;
        }
        let rest: Vec<_> = iter
            .map(|shape| super::paint::ClippedShape { clip, shape })
            .collect();
        if !rest.is_empty() {
            let tail = v.split_off(index + 1);
            v.extend(rest);
            v.extend(tail);
        }
    }
}

impl Ui {
    fn layout_for_frame(&self) -> &Layout {
        static TOP_DOWN: std::sync::OnceLock<Layout> = std::sync::OnceLock::new();
        if self.layout().is_horizontal() {
            TOP_DOWN.get_or_init(|| Layout::top_down(Align::Min))
        } else {
            let l = self.layout();
            if l.cross_align == Align::Center && !l.cross_justify {
                l
            } else {
                TOP_DOWN.get_or_init(|| Layout::top_down(Align::Min))
            }
        }
    }

    /// Account for a child region placed at `rect` (already laid out at the cursor).
    pub(crate) fn allocate_rect_for_child(&mut self, rect: Rect) -> Response {
        let placed = self.allocate_rect_for(rect.size());
        let id = self.next_auto_id();
        let _ = placed;
        self.interact(rect, id, Sense::hover())
    }
}

// ---------------------------------------------------------------------------
// Panels
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

fn panel_frame(ctx: &Context) -> Frame {
    let s = ctx.style();
    Frame::new()
        .fill(s.visuals.panel_fill)
        .inner_margin(Margin::symmetric(8, 2))
}

fn run_panel<R>(
    ctx: &Context,
    id: Id,
    rect: Rect,
    frame: Frame,
    add: impl FnOnce(&mut Ui) -> R,
) -> (R, Rect) {
    let layer = LayerId::background();
    ctx.register_area(layer, rect);
    let painter = super::paint::Painter::new(ctx.clone(), layer, rect);
    for s in frame.shapes(rect) {
        painter.add(s);
    }
    let inner_rect = Rect::from_min_max(
        rect.min + frame.inner_margin.left_top(),
        rect.max - frame.inner_margin.right_bottom(),
    );
    let mut ui = Ui::new(ctx.clone(), layer, id, inner_rect);
    ui.set_clip_rect(rect);
    let r = add(&mut ui);
    (r, ui.min_rect())
}

pub struct SidePanel {
    id: Id,
    side: Side,
    frame: Option<Frame>,
    resizable: bool,
    default_width: f32,
    min_width: f32,
    exact: Option<f32>,
    separator: bool,
}

impl SidePanel {
    fn new(side: Side, id: impl std::hash::Hash) -> Self {
        SidePanel {
            id: Id::new(id),
            side,
            frame: None,
            resizable: true,
            default_width: 200.0,
            min_width: 96.0,
            exact: None,
            separator: true,
        }
    }
    pub fn left(id: impl std::hash::Hash) -> Self {
        SidePanel::new(Side::Left, id)
    }
    pub fn right(id: impl std::hash::Hash) -> Self {
        SidePanel::new(Side::Right, id)
    }
    pub fn frame(mut self, f: Frame) -> Self {
        self.frame = Some(f);
        self
    }
    pub fn resizable(mut self, r: bool) -> Self {
        self.resizable = r;
        self
    }
    pub fn default_width(mut self, w: f32) -> Self {
        self.default_width = w;
        self
    }
    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = w;
        self
    }
    pub fn exact_width(mut self, w: f32) -> Self {
        self.exact = Some(w);
        self.resizable = false;
        self
    }
    pub fn show_separator_line(mut self, s: bool) -> Self {
        self.separator = s;
        self
    }

    pub fn show<R>(self, ctx: &Context, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        let avail = ctx.available_rect();
        let key = self.id.with("width");
        let mut width = self
            .exact
            .unwrap_or_else(|| {
                ctx.data(|d| d.get_temp::<f32>(key))
                    .unwrap_or(self.default_width)
            })
            .clamp(self.min_width.min(avail.width()), avail.width().max(0.0));
        // Drag the inner edge to resize.
        if self.resizable {
            let edge_x = if self.side == Side::Left {
                avail.min.x + width
            } else {
                avail.max.x - width
            };
            let grip = Rect::from_min_max(
                pos2(edge_x - 3.0, avail.min.y),
                pos2(edge_x + 3.0, avail.max.y),
            );
            let r = ctx.interact(
                LayerId::background(),
                avail,
                grip,
                self.id.with("resize"),
                Sense::drag(),
                true,
            );
            if r.hovered() || r.dragged() {
                ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
            }
            if r.dragged() {
                let dx = r.drag_delta().x;
                width = (width + if self.side == Side::Left { dx } else { -dx })
                    .clamp(self.min_width, avail.width() * 0.8);
                ctx.data_mut(|d| d.insert_temp(key, width));
            }
        }
        let rect = if self.side == Side::Left {
            Rect::from_min_size(avail.min, vec2(width, avail.height()))
        } else {
            Rect::from_min_max(pos2(avail.max.x - width, avail.min.y), avail.max)
        };
        let frame = self.frame.unwrap_or_else(|| panel_frame(ctx));
        let (inner, _) = run_panel(ctx, self.id, rect, frame, add);
        if self.separator {
            let x = if self.side == Side::Left {
                rect.max.x - 0.5
            } else {
                rect.min.x + 0.5
            };
            let stroke = ctx.style().visuals.widgets.noninteractive.bg_stroke;
            ctx.layer_painter(LayerId::background())
                .vline(x, rect.y_range(), stroke);
        }
        ctx.lock().available = if self.side == Side::Left {
            Rect::from_min_max(pos2(rect.max.x, avail.min.y), avail.max)
        } else {
            Rect::from_min_max(avail.min, pos2(rect.min.x, avail.max.y))
        };
        let resp = ctx.interact(
            LayerId::background(),
            rect,
            rect,
            self.id,
            Sense::hover(),
            true,
        );
        InnerResponse::new(inner, resp)
    }
}

pub struct TopBottomPanel {
    id: Id,
    side: Side,
    frame: Option<Frame>,
    separator: bool,
    exact: Option<f32>,
}

impl TopBottomPanel {
    pub fn top(id: impl std::hash::Hash) -> Self {
        TopBottomPanel {
            id: Id::new(id),
            side: Side::Top,
            frame: None,
            separator: true,
            exact: None,
        }
    }
    pub fn bottom(id: impl std::hash::Hash) -> Self {
        TopBottomPanel {
            side: Side::Bottom,
            ..TopBottomPanel::top(id)
        }
    }
    pub fn frame(mut self, f: Frame) -> Self {
        self.frame = Some(f);
        self
    }
    pub fn show_separator_line(mut self, s: bool) -> Self {
        self.separator = s;
        self
    }
    pub fn exact_height(mut self, h: f32) -> Self {
        self.exact = Some(h);
        self
    }

    pub fn show<R>(self, ctx: &Context, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        let avail = ctx.available_rect();
        let frame = self.frame.unwrap_or_else(|| panel_frame(ctx));
        // Height: what the content needed last frame.
        let key = self.id.with("height");
        let h = self
            .exact
            .unwrap_or_else(|| {
                ctx.data(|d| d.get_temp::<f32>(key))
                    .unwrap_or(ctx.style().spacing.interact_size.y + frame.inner_margin.sum().y)
            })
            .min(avail.height());
        let rect = if self.side == Side::Top {
            Rect::from_min_size(avail.min, vec2(avail.width(), h))
        } else {
            Rect::from_min_max(pos2(avail.min.x, avail.max.y - h), avail.max)
        };
        let (inner, used) = run_panel(ctx, self.id, rect, frame, |ui| {
            if self.side == Side::Top {
                ui.set_min_width(rect.width() - frame.inner_margin.sum().x);
            }
            add(ui)
        });
        let needed = (used.height() + frame.inner_margin.sum().y).max(1.0);
        if self.exact.is_none() && (needed - h).abs() > 0.5 {
            ctx.data_mut(|d| d.insert_temp(key, needed));
            ctx.request_repaint();
        }
        if self.separator {
            let y = if self.side == Side::Top {
                rect.max.y - 0.5
            } else {
                rect.min.y + 0.5
            };
            let stroke = ctx.style().visuals.widgets.noninteractive.bg_stroke;
            ctx.layer_painter(LayerId::background())
                .hline(rect.x_range(), y, stroke);
        }
        ctx.lock().available = if self.side == Side::Top {
            Rect::from_min_max(pos2(avail.min.x, rect.max.y), avail.max)
        } else {
            Rect::from_min_max(avail.min, pos2(avail.max.x, rect.min.y))
        };
        let resp = ctx.interact(
            LayerId::background(),
            rect,
            rect,
            self.id,
            Sense::hover(),
            true,
        );
        InnerResponse::new(inner, resp)
    }
}

#[derive(Default)]
pub struct CentralPanel {
    frame: Option<Frame>,
}

impl CentralPanel {
    pub fn frame(mut self, f: Frame) -> Self {
        self.frame = Some(f);
        self
    }
    pub fn show<R>(self, ctx: &Context, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        let rect = ctx.available_rect();
        let frame = self.frame.unwrap_or_else(|| panel_frame(ctx));
        let id = Id::new("central_panel");
        let (inner, _) = run_panel(ctx, id, rect, frame, add);
        let resp = ctx.interact(LayerId::background(), rect, rect, id, Sense::hover(), true);
        InnerResponse::new(inner, resp)
    }
}

// ---------------------------------------------------------------------------
// Scroll area
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
struct ScrollState {
    offset: f32,
    content: f32,
    viewport: f32,
    at_bottom: bool,
}

pub struct ScrollArea {
    id_salt: Option<Id>,
    max_height: f32,
    auto_shrink: [bool; 2],
    stick_to_bottom: bool,
}

pub struct ScrollAreaOutput<R> {
    pub inner: R,
    pub id: Id,
    pub content_size: Vec2,
    pub inner_rect: Rect,
}

/// A request to bring a rectangle into view.
#[derive(Clone, Copy)]
struct ScrollTo {
    rect: Rect,
    align: Option<Align>,
}

pub(crate) fn scroll_to(ctx: &Context, rect: Rect, align: Option<Align>) {
    ctx.data_mut(|d| d.insert_temp(Id::new("scroll_to"), Some(ScrollTo { rect, align })));
}

impl ScrollArea {
    pub fn vertical() -> ScrollArea {
        ScrollArea {
            id_salt: None,
            max_height: f32::INFINITY,
            auto_shrink: [true, true],
            stick_to_bottom: false,
        }
    }
    pub fn both() -> ScrollArea {
        ScrollArea::vertical()
    }
    pub fn id_salt(mut self, s: impl std::hash::Hash) -> Self {
        self.id_salt = Some(Id::new(s));
        self
    }
    pub fn max_height(mut self, h: f32) -> Self {
        self.max_height = h;
        self
    }
    pub fn auto_shrink(mut self, a: impl Into<[bool; 2]>) -> Self {
        self.auto_shrink = a.into();
        self
    }
    pub fn stick_to_bottom(mut self, s: bool) -> Self {
        self.stick_to_bottom = s;
        self
    }

    pub fn show<R>(self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> ScrollAreaOutput<R> {
        self.show_viewport(ui, |ui, _| add(ui))
    }

    /// Only the visible rows are built: `add` gets the range to show.
    pub fn show_rows<R>(
        self,
        ui: &mut Ui,
        row_height: f32,
        total_rows: usize,
        add: impl FnOnce(&mut Ui, std::ops::Range<usize>) -> R,
    ) -> ScrollAreaOutput<R> {
        let spacing = ui.spacing().item_spacing.y;
        let step = row_height + spacing;
        self.show_viewport(ui, |ui, viewport| {
            let first = ((viewport.min.y / step).floor().max(0.0) as usize).min(total_rows);
            let last = (((viewport.max.y / step).ceil().max(0.0) as usize) + 1).min(total_rows);
            ui.add_space(first as f32 * step);
            let r = add(ui, first..last);
            let rest = total_rows.saturating_sub(last) as f32 * step;
            if rest > 0.0 {
                ui.add_space(rest - spacing);
            }
            r
        })
    }

    fn show_viewport<R>(
        self,
        ui: &mut Ui,
        add: impl FnOnce(&mut Ui, Rect) -> R,
    ) -> ScrollAreaOutput<R> {
        let id = match self.id_salt {
            Some(s) => ui.id().with(s),
            None => ui.next_auto_id(),
        };
        let mut st = ui
            .data(|d| d.get_temp::<ScrollState>(id))
            .unwrap_or_default();
        let bar_w = ui.spacing().scroll_bar_width;
        let avail = ui.available_rect_before_wrap();
        let max_h = self.max_height.min(avail.height().max(0.0));
        let height = if self.auto_shrink[1] {
            st.content.min(max_h).max(0.0)
        } else {
            max_h
        };
        let width = avail.width();
        let outer = Rect::from_min_size(avail.min, vec2(width, height));
        let max_offset = (st.content - height).max(0.0);
        if self.stick_to_bottom && st.at_bottom {
            st.offset = max_offset;
        }
        st.offset = st.offset.clamp(0.0, max_offset);
        let content_w = if st.content > height + 0.5 {
            width - bar_w - 2.0
        } else {
            width
        };
        let content_rect = Rect::from_min_size(
            pos2(outer.min.x, outer.min.y - st.offset),
            vec2(content_w, f32::INFINITY),
        );
        let clip = outer.intersect(ui.clip_rect());
        let mut child = ui.new_child(UiBuilder {
            max_rect: Some(content_rect),
            layout: Some(Layout::top_down(Align::Min)),
            id_salt: Some(id),
            clip: Some(clip),
            ..Default::default()
        });
        let viewport = Rect::from_min_size(pos2(0.0, st.offset), vec2(content_w, height));
        let inner = add(&mut child, viewport);
        let content_h = (child.min_rect().max.y - content_rect.min.y).max(0.0);
        st.content = content_h;
        st.viewport = height;
        let max_offset = (content_h - height).max(0.0);

        // Wheel scrolling: the innermost area under the pointer takes it.
        let hovered = ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| clip.contains(p))
            && ui
                .ctx()
                .layer_under_pointer()
                .is_none_or(|l| l == ui.layer_id());
        if hovered && max_offset > 0.0 {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy != 0.0 {
                st.offset = (st.offset - dy).clamp(0.0, max_offset);
                ui.input_mut(|i| i.smooth_scroll_delta.y = 0.0);
                ui.ctx().request_repaint();
            }
        }
        // Keep focused or requested items in view.
        let req = ui.data(|d| {
            d.get_temp::<Option<ScrollTo>>(Id::new("scroll_to"))
                .flatten()
        });
        if let Some(r) = req {
            if r.rect.min.x >= outer.min.x - 1.0
                && r.rect.max.x <= outer.max.x + 1.0
                && r.rect.min.y >= content_rect.min.y
                && r.rect.max.y <= content_rect.min.y + content_h + 1.0
            {
                let top = r.rect.min.y - content_rect.min.y;
                let bottom = r.rect.max.y - content_rect.min.y;
                let target = match r.align {
                    Some(Align::Min) => top,
                    Some(Align::Center) => (top + bottom) / 2.0 - height / 2.0,
                    Some(Align::Max) => bottom - height,
                    None if top < st.offset => top,
                    None if bottom > st.offset + height => bottom - height,
                    None => st.offset,
                };
                st.offset = target.clamp(0.0, max_offset);
                ui.data_mut(|d| d.insert_temp::<Option<ScrollTo>>(Id::new("scroll_to"), None));
                ui.ctx().request_repaint();
            }
        }
        // Scroll bar.
        if max_offset > 0.0 {
            let track = Rect::from_min_max(pos2(outer.max.x - bar_w, outer.min.y), outer.max);
            let thumb_h = (height * height / content_h).max(24.0).min(height);
            let t = st.offset / max_offset;
            let thumb = Rect::from_min_size(
                pos2(track.min.x, track.min.y + t * (height - thumb_h)),
                vec2(bar_w, thumb_h),
            );
            let r = ui.interact(track, id.with("bar"), Sense::drag());
            if r.dragged() {
                let dy = r.drag_delta().y;
                st.offset = (st.offset + dy * (content_h - height) / (height - thumb_h).max(1.0))
                    .clamp(0.0, max_offset);
            } else if r.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
                if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                    if !thumb.contains(p) {
                        let page = if p.y < thumb.min.y { -height } else { height };
                        st.offset = (st.offset + page).clamp(0.0, max_offset);
                    }
                }
            }
            let v = &ui.visuals().widgets;
            let col = if r.dragged() {
                v.active.bg_fill
            } else if r.hovered() {
                v.hovered.bg_fill
            } else {
                v.inactive.bg_fill
            };
            let p = ui.painter();
            let w = if r.hovered() || r.dragged() {
                bar_w
            } else {
                bar_w * 0.6
            };
            let thumb = Rect::from_min_max(
                pos2(track.max.x - w, thumb.min.y),
                pos2(track.max.x, thumb.max.y),
            );
            p.rect_filled(thumb, (w / 2.0) as u8, col);
        }
        st.at_bottom = st.offset >= max_offset - 1.0;
        ui.data_mut(|d| d.insert_temp(id, st));
        let used_h = if self.auto_shrink[1] {
            content_h.min(max_h)
        } else {
            height
        };
        let used_w = if self.auto_shrink[0] {
            child.min_rect().width().min(width)
        } else {
            width
        };
        ui.allocate_rect_for(vec2(used_w, used_h));
        ScrollAreaOutput {
            inner,
            id,
            content_size: vec2(content_w, content_h),
            inner_rect: outer,
        }
    }
}

// ---------------------------------------------------------------------------
// Grid
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct GridLayout {
    prev: Vec<f32>,
    widths: Vec<f32>,
    col: usize,
    x0: f32,
    row_top: f32,
    row_h: f32,
    prev_row_h: Vec<f32>,
    row_heights: Vec<f32>,
    row: usize,
    spacing: Vec2,
    min_row_h: f32,
}

impl GridLayout {
    fn col_x(&self, col: usize) -> f32 {
        let mut x = self.x0;
        for c in 0..col {
            x += self
                .prev
                .get(c)
                .copied()
                .unwrap_or(0.0)
                .max(self.widths.get(c).copied().unwrap_or(0.0))
                + self.spacing.x;
        }
        x
    }
    /// Place the next cell; returns its rectangle.
    pub(crate) fn place(&mut self, size: Vec2) -> Rect {
        let x = self.col_x(self.col);
        let row_h = self
            .prev_row_h
            .get(self.row)
            .copied()
            .unwrap_or(self.min_row_h)
            .max(self.min_row_h);
        let y = self.row_top + ((row_h - size.y) / 2.0).max(0.0);
        let r = Rect::from_min_size(pos2(x, y), size);
        self.cell_used(r);
        r
    }
    pub(crate) fn cell_used(&mut self, r: Rect) {
        if self.widths.len() <= self.col {
            self.widths.resize(self.col + 1, 0.0);
        }
        let x = self.col_x(self.col);
        self.widths[self.col] = self.widths[self.col].max(r.max.x - x);
        self.row_h = self.row_h.max(r.max.y - self.row_top);
        self.col += 1;
    }
    pub(crate) fn cell_rect(&self) -> Rect {
        let x = self.col_x(self.col);
        Rect::from_min_max(pos2(x, self.row_top), pos2(f32::INFINITY, f32::INFINITY))
    }
    pub(crate) fn end_row(&mut self) {
        self.row_heights.push(self.row_h.max(self.min_row_h));
        self.row_top += self.row_h.max(self.min_row_h) + self.spacing.y;
        self.row_h = 0.0;
        self.col = 0;
        self.row += 1;
    }
    pub(crate) fn row_start(&self) -> (f32, f32) {
        (self.x0, self.row_top)
    }
}

pub struct Grid {
    id: Id,
    spacing: Option<Vec2>,
    num_columns: usize,
    striped: bool,
}

impl Grid {
    pub fn new(id: impl std::hash::Hash) -> Grid {
        Grid {
            id: Id::new(id),
            spacing: None,
            num_columns: 0,
            striped: false,
        }
    }
    pub fn num_columns(mut self, n: usize) -> Self {
        self.num_columns = n;
        self
    }
    pub fn spacing(mut self, s: impl Into<Vec2>) -> Self {
        self.spacing = Some(s.into());
        self
    }
    pub fn striped(mut self, s: bool) -> Self {
        self.striped = s;
        self
    }

    pub fn show<R>(self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        let id = ui.id().with(self.id);
        let (prev, prev_row_h) = ui
            .data(|d| d.get_temp::<(Vec<f32>, Vec<f32>)>(id))
            .unwrap_or_default();
        let spacing = self.spacing.unwrap_or(ui.spacing().item_spacing);
        let avail = ui.available_rect_before_wrap();
        let min_row_h = ui.spacing().interact_size.y;
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(avail)
                .layout(Layout::left_to_right(Align::Center))
                .id_salt(self.id),
        );
        child.grid = Some(GridLayout {
            prev,
            widths: vec![0.0; self.num_columns],
            col: 0,
            x0: avail.min.x,
            row_top: avail.min.y,
            row_h: 0.0,
            prev_row_h,
            row_heights: Vec::new(),
            row: 0,
            spacing,
            min_row_h,
        });
        let inner = add(&mut child);
        let g = child.grid.take().unwrap_or_default();
        let mut rows = g.row_heights.clone();
        if g.col > 0 {
            rows.push(g.row_h.max(min_row_h));
        }
        let total_w: f32 =
            g.widths.iter().sum::<f32>() + spacing.x * g.widths.len().saturating_sub(1) as f32;
        let total_h: f32 =
            rows.iter().sum::<f32>() + spacing.y * rows.len().saturating_sub(1) as f32;
        if g.widths != g.prev || rows != g.prev_row_h {
            ui.ctx().request_repaint();
        }
        ui.data_mut(|d| d.insert_temp(id, (g.widths.clone(), rows)));
        let rect = ui.allocate_rect_for(vec2(total_w.max(0.0), total_h.max(0.0)));
        let resp = ui.interact(rect, id, Sense::hover());
        InnerResponse::new(inner, resp)
    }
}

// ---------------------------------------------------------------------------
// Collapsing header
// ---------------------------------------------------------------------------

pub struct CollapsingHeader {
    text: WidgetText,
    id: Option<Id>,
    default_open: bool,
}

pub struct CollapsingResponse<R> {
    pub header_response: Response,
    pub body_returned: Option<R>,
    pub openness: f32,
}

impl CollapsingHeader {
    pub fn new(text: impl Into<WidgetText>) -> Self {
        CollapsingHeader {
            text: text.into(),
            id: None,
            default_open: false,
        }
    }
    pub fn id_salt(mut self, s: impl std::hash::Hash) -> Self {
        self.id = Some(Id::new(s));
        self
    }
    pub fn default_open(mut self, o: bool) -> Self {
        self.default_open = o;
        self
    }
    pub fn show<R>(self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> CollapsingResponse<R> {
        let id = ui
            .id()
            .with(self.id.unwrap_or_else(|| Id::new(self.text.text())));
        let mut open = ui
            .data(|d| d.get_temp::<bool>(id))
            .unwrap_or(self.default_open);
        let style = ui.style().clone();
        let color = ui.text_color();
        let (g, c) = ui.layout_text(&self.text, &TextStyle::Button, f32::INFINITY, color);
        let icon = style.spacing.icon_width;
        let w = ui.available_width();
        let h = g.size().y.max(style.spacing.interact_size.y);
        let (_, rect) = ui.allocate_space(vec2(w, h));
        let resp = ui.interact(rect, id.with("header"), Sense::click());
        if resp.clicked() {
            open = !open;
            ui.data_mut(|d| d.insert_temp(id, open));
        }
        let v = *style.visuals.widgets.style(
            resp.hovered(),
            resp.is_pointer_button_down_on(),
            ui.is_enabled(),
        );
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(rect, v.corner_radius, v.weak_bg_fill);
        }
        let cx = rect.min.x + icon / 2.0 + 2.0;
        let cy = rect.center().y;
        let s = icon * 0.3;
        let tri = if open {
            vec![
                pos2(cx - s, cy - s * 0.6),
                pos2(cx + s, cy - s * 0.6),
                pos2(cx, cy + s * 0.8),
            ]
        } else {
            vec![
                pos2(cx - s * 0.6, cy - s),
                pos2(cx - s * 0.6, cy + s),
                pos2(cx + s * 0.8, cy),
            ]
        };
        p.add(Shape::convex_polygon(tri, v.fg_stroke.color, Stroke::NONE));
        p.galley(
            pos2(rect.min.x + icon + 6.0, cy - g.size().y / 2.0),
            g,
            if self.text.rich().has_color() {
                c
            } else {
                v.fg_stroke.color.lerp_to(c, 0.5)
            },
        );
        let label = self.text.text().to_string();
        resp.widget_info(|| WidgetInfo::selected(WidgetType::CollapsingHeader, true, open, &label));
        let body = if open {
            let r = ui.indent(id.with("body"), add);
            Some(r.inner)
        } else {
            None
        };
        CollapsingResponse {
            header_response: resp,
            body_returned: body,
            openness: if open { 1.0 } else { 0.0 },
        }
    }
}

// ---------------------------------------------------------------------------
// Popups: tooltips, combo boxes, menus
// ---------------------------------------------------------------------------

/// Show `add` in a floating area at `pos` on `order`; returns the area's rect.
fn floating<R>(
    ctx: &Context,
    layer: LayerId,
    pos: Pos2,
    frame: Frame,
    width: Option<f32>,
    add: impl FnOnce(&mut Ui) -> R,
) -> (R, Rect, bool) {
    let screen = ctx.screen_rect();
    // Last frame's size, to keep the popup on screen.
    let prev = ctx.area_rect(layer);
    let size = prev
        .map(|r| r.size())
        .unwrap_or(vec2(width.unwrap_or(200.0), 40.0));
    let mut origin = pos;
    if origin.x + size.x > screen.max.x {
        origin.x = (screen.max.x - size.x - 4.0).max(screen.min.x);
    }
    if origin.y + size.y > screen.max.y {
        origin.y = (pos.y - size.y - 24.0).max(screen.min.y);
    }
    let max_w = width.unwrap_or(screen.width() * 0.6);
    let inner_max = Rect::from_min_size(
        origin + frame.inner_margin.left_top(),
        vec2(max_w, screen.height()),
    );
    let slot_ctx = ctx.clone();
    let index = slot_ctx.reserve_shape(layer, screen);
    let mut ui = Ui::new(ctx.clone(), layer, layer.id, inner_max);
    ui.set_clip_rect(screen);
    if let Some(w) = width {
        ui.set_min_width(w - frame.inner_margin.sum().x);
    }
    let r = add(&mut ui);
    let content = ui.min_rect();
    let rect = Rect::from_min_max(origin, content.max + frame.inner_margin.right_bottom());
    slot_ctx.fill_shape_slot(layer, index, frame.shapes(rect));
    ctx.register_area(layer, rect);
    let closed = ui.close_requested.get();
    (r, rect, closed)
}

pub(crate) fn tooltip_at(ctx: &Context, id: Id, pos: Pos2, add: impl FnOnce(&mut Ui)) {
    let style = ctx.style();
    let frame = Frame::popup(&style);
    let layer = LayerId::new(Order::Tooltip, id);
    floating(ctx, layer, pos, frame, None, add);
}

/// Right-click menu on a response.
pub(crate) fn context_menu(
    resp: &Response,
    add: impl FnOnce(&mut Ui),
) -> Option<InnerResponse<()>> {
    let ctx = &resp.ctx;
    let id = resp.id.with("context_menu");
    let pos_key = id.with("pos");
    if resp.secondary_clicked() {
        let p = ctx
            .input(|i| i.pointer.hover_pos())
            .unwrap_or(resp.rect.center());
        ctx.memory_mut(|m| m.open_popup(id));
        ctx.data_mut(|d| d.insert_temp(pos_key, p));
    }
    if !ctx.memory(|m| m.is_popup_open(id)) {
        return None;
    }
    let pos = ctx
        .data(|d| d.get_temp::<Pos2>(pos_key))
        .unwrap_or(resp.rect.left_bottom());
    let r = popup_body(ctx, id, pos, None, None, |ui| {
        ui.in_menu = true;
        add(ui)
    });
    Some(InnerResponse::new((), r.1))
}

/// A popup below `anchor`: closes on Escape, on a click outside, or when
/// something inside asks.
fn popup_body<R>(
    ctx: &Context,
    id: Id,
    pos: Pos2,
    width: Option<f32>,
    anchor: Option<Rect>,
    add: impl FnOnce(&mut Ui) -> R,
) -> (R, Response) {
    let style = ctx.style();
    let layer = LayerId::new(Order::Foreground, id);
    let (r, rect, closed) = floating(ctx, layer, pos, Frame::popup(&style), width, add);
    let pressed_outside = ctx.input(|i| {
        i.pointer.any_pressed() && i.pointer.press_origin().is_some_and(|p| !rect.contains(p))
    }) && ctx.area_rect(layer).is_some()
        && !anchor.is_some_and(|a| {
            ctx.input(|i| i.pointer.press_origin().is_some_and(|p| a.contains(p)))
        });
    let esc = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
    if closed || pressed_outside || esc {
        ctx.memory_mut(|m| {
            if m.is_popup_open(id) {
                m.close_popup()
            }
        });
    }
    let resp = ctx.interact(layer, rect, rect, id.with("popup"), Sense::hover(), true);
    (r, resp)
}

pub struct ComboBox {
    id: Id,
    selected: WidgetText,
    width: Option<f32>,
}

impl ComboBox {
    pub fn from_id_salt(id: impl std::hash::Hash) -> Self {
        ComboBox {
            id: Id::new(id),
            selected: "".into(),
            width: None,
        }
    }
    pub fn new(id: impl std::hash::Hash, _label: impl Into<WidgetText>) -> Self {
        Self::from_id_salt(id)
    }
    pub fn selected_text(mut self, t: impl Into<WidgetText>) -> Self {
        self.selected = t.into();
        self
    }
    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }
    pub fn show_ui<R>(
        self,
        ui: &mut Ui,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<Option<R>> {
        let id = ui.id().with(self.id);
        let style = ui.style().clone();
        let w = self.width.unwrap_or(style.spacing.combo_width);
        let color = ui.text_color();
        let (g, c) = ui.layout_text(&self.selected, &TextStyle::Button, f32::INFINITY, color);
        let h = style
            .spacing
            .interact_size
            .y
            .max(g.size().y + style.spacing.button_padding.y * 2.0);
        let (_, rect) = ui.allocate_space(vec2(w, h));
        let resp = ui.interact(rect, id, Sense::click());
        let open = ui.memory(|m| m.is_popup_open(id));
        if resp.clicked() {
            ui.memory_mut(|m| m.toggle_popup(id));
        }
        let v = if open {
            style.visuals.widgets.open
        } else {
            *style.visuals.widgets.style(
                resp.hovered(),
                resp.is_pointer_button_down_on(),
                ui.is_enabled(),
            )
        };
        let p = ui.painter();
        p.rect(
            rect,
            v.corner_radius,
            v.weak_bg_fill,
            v.bg_stroke,
            StrokeKind::Inside,
        );
        let pad = style.spacing.button_padding.x;
        let clip = p.with_clip_rect(rect.shrink2(vec2(pad, 0.0)).with_max_x(rect.max.x - 18.0));
        clip.galley(
            pos2(rect.min.x + pad, rect.center().y - g.size().y / 2.0),
            g,
            if self.selected.rich().has_color() {
                c
            } else {
                v.fg_stroke.color.lerp_to(c, 0.5)
            },
        );
        let cx = rect.max.x - 10.0;
        let cy = rect.center().y;
        p.add(Shape::convex_polygon(
            vec![
                pos2(cx - 4.0, cy - 2.0),
                pos2(cx + 4.0, cy - 2.0),
                pos2(cx, cy + 3.0),
            ],
            v.fg_stroke.color,
            Stroke::NONE,
        ));
        let label = self.selected.text().to_string();
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, ui.is_enabled(), &label));
        let mut inner = None;
        if ui.memory(|m| m.is_popup_open(id)) {
            let (r, _) = popup_body(
                ui.ctx(),
                id,
                rect.left_bottom() + vec2(0.0, 2.0),
                Some(w.max(rect.width())),
                Some(rect),
                |ui| {
                    ui.in_menu = true;
                    let mut ui2 = ui
                        .new_child(UiBuilder::new().layout(Layout::top_down_justified(Align::Min)));
                    let r = ScrollArea::vertical()
                        .max_height(320.0)
                        .show(&mut ui2, add)
                        .inner;
                    let used = ui2.min_rect();
                    ui.expand_to_include_rect(used);
                    r
                },
            );
            inner = Some(r);
        }
        InnerResponse::new(inner, resp)
    }
}

/// A button that opens a drop-down menu.
pub fn menu_button<R>(
    ui: &mut Ui,
    title: impl Into<WidgetText>,
    add: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<Option<R>> {
    let title: WidgetText = title.into();
    let id = ui.id().with(("menu", title.text().to_string()));
    let open = ui.memory(|m| m.is_popup_open(id));
    let mut button = Button::new(title).frame(false);
    if open {
        button = button.selected(true);
    }
    let resp = ui.add(button);
    // Hovering another menu title while one is open switches menus.
    let any_open = ui.memory(|m| m.any_popup_open());
    if resp.clicked()
        || (any_open
            && !open
            && resp.hovered()
            && ui.data(|d| {
                d.get_temp::<bool>(Id::new("menu_bar_active"))
                    .unwrap_or(false)
            }))
    {
        ui.memory_mut(|m| m.toggle_popup(id));
        ui.data_mut(|d| d.insert_temp(Id::new("menu_bar_active"), true));
    }
    let mut inner = None;
    if ui.memory(|m| m.is_popup_open(id)) {
        let (r, _) = popup_body(
            ui.ctx(),
            id,
            resp.rect.left_bottom() + vec2(0.0, 2.0),
            None,
            Some(resp.rect),
            |ui| {
                ui.in_menu = true;
                ui.set_min_width(160.0);
                let mut col =
                    ui.new_child(UiBuilder::new().layout(Layout::top_down_justified(Align::Min)));
                let r = add(&mut col);
                let used = col.min_rect();
                ui.expand_to_include_rect(used);
                r
            },
        );
        inner = Some(r);
    } else if !any_open {
        ui.data_mut(|d| d.insert_temp(Id::new("menu_bar_active"), false));
    }
    InnerResponse::new(inner, resp)
}

impl Ui {
    pub fn menu_button<R>(
        &mut self,
        title: impl Into<WidgetText>,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<Option<R>> {
        menu_button(self, title, add)
    }
}

#[derive(Default)]
pub struct MenuBar;

impl MenuBar {
    pub fn new() -> MenuBar {
        MenuBar
    }
    pub fn ui<R>(self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        ui.horizontal(|ui| {
            ui.spacing_mut().button_padding = vec2(8.0, 3.0);
            add(ui)
        })
    }
}

// ---------------------------------------------------------------------------
// Windows and modal dialogs
// ---------------------------------------------------------------------------

pub struct Window<'a> {
    title: WidgetText,
    id: Id,
    anchor: Option<(Align2, Vec2)>,
    open: Option<&'a mut bool>,
    default_width: Option<f32>,
    default_size: Option<Vec2>,
    resizable: bool,
    collapsible: bool,
}

impl<'a> Window<'a> {
    pub fn new(title: impl Into<WidgetText>) -> Self {
        let title: WidgetText = title.into();
        Window {
            id: Id::new(title.text()),
            title,
            anchor: None,
            open: None,
            default_width: None,
            default_size: None,
            resizable: false,
            collapsible: false,
        }
    }
    pub fn id(mut self, id: Id) -> Self {
        self.id = id;
        self
    }
    pub fn anchor(mut self, a: Align2, offset: impl Into<Vec2>) -> Self {
        self.anchor = Some((a, offset.into()));
        self
    }
    pub fn open(mut self, o: &'a mut bool) -> Self {
        self.open = Some(o);
        self
    }
    pub fn default_width(mut self, w: f32) -> Self {
        self.default_width = Some(w);
        self
    }
    pub fn default_size(mut self, s: impl Into<Vec2>) -> Self {
        self.default_size = Some(s.into());
        self
    }
    pub fn resizable(mut self, r: bool) -> Self {
        self.resizable = r;
        self
    }
    pub fn collapsible(mut self, c: bool) -> Self {
        self.collapsible = c;
        self
    }

    pub fn show<R>(
        self,
        ctx: &Context,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> Option<InnerResponse<Option<R>>> {
        if self.open.as_deref() == Some(&false) {
            return None;
        }
        let style = ctx.style();
        let frame = Frame::window(&style);
        let layer = LayerId::new(Order::Middle, self.id);
        let width = self
            .default_size
            .map(|s| s.x)
            .or(self.default_width)
            .unwrap_or(360.0);
        let (r, rect, close_clicked) = window_body(
            ctx,
            layer,
            frame,
            width,
            self.anchor,
            Some(&self.title),
            self.default_size.map(|s| s.y),
            add,
        );
        if close_clicked {
            if let Some(o) = self.open {
                *o = false;
            }
        }
        let resp = ctx.interact(
            layer,
            rect,
            rect,
            self.id.with("window"),
            Sense::hover(),
            true,
        );
        Some(InnerResponse::new(Some(r), resp))
    }
}

/// Lay out a window; returns (result, rect, close button clicked).
#[allow(clippy::too_many_arguments)]
fn window_body<R>(
    ctx: &Context,
    layer: LayerId,
    frame: Frame,
    width: f32,
    anchor: Option<(Align2, Vec2)>,
    title: Option<&WidgetText>,
    max_h: Option<f32>,
    add: impl FnOnce(&mut Ui) -> R,
) -> (R, Rect, bool) {
    let screen = ctx.screen_rect();
    let width = width.min(screen.width() - 16.0);
    let prev = ctx.area_rect(layer);
    let size = prev
        .map(|r| r.size())
        .unwrap_or(vec2(width + frame.inner_margin.sum().x, 120.0));
    let (align, off) = anchor.unwrap_or((Align2::CENTER_CENTER, Vec2::ZERO));
    let base = match align.0 {
        [Align::Center, Align::Center] => screen.center(),
        [Align::Center, Align::Min] => screen.center_top(),
        [Align::Min, Align::Min] => screen.min,
        _ => screen.center(),
    };
    let mut origin = align.anchor_size(base + off, size).min;
    origin.x = origin
        .x
        .clamp(screen.min.x, (screen.max.x - size.x).max(screen.min.x));
    origin.y = origin
        .y
        .clamp(screen.min.y, (screen.max.y - size.y).max(screen.min.y));
    let index = ctx.reserve_shape(layer, screen);
    let max_h = max_h
        .unwrap_or(screen.height() - 40.0)
        .min(screen.height() - 40.0);
    let inner = Rect::from_min_size(origin + frame.inner_margin.left_top(), vec2(width, max_h));
    let mut ui = Ui::new(ctx.clone(), layer, layer.id, inner);
    ui.set_clip_rect(screen);
    ui.set_min_width(width);
    let mut close = false;
    if let Some(t) = title {
        ui.horizontal(|ui| {
            ui.label(t.0.clone().heading());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(Button::new("✕").frame(false))
                    .on_hover_text("Close")
                    .clicked()
                {
                    close = true;
                }
            });
        });
        ui.separator();
    }
    let r = add(&mut ui);
    let content = ui.min_rect();
    let rect = Rect::from_min_max(origin, content.max + frame.inner_margin.right_bottom());
    ctx.fill_shape_slot(layer, index, frame.shapes(rect));
    ctx.register_area(layer, rect);
    if prev.map(|p| p.size()) != Some(rect.size()) {
        ctx.request_repaint();
    }
    (r, rect, close || ui.close_requested.get())
}

pub struct Modal {
    id: Id,
}

pub struct ModalResponse<R> {
    pub inner: R,
    pub response: Response,
    close: bool,
}

impl<R> ModalResponse<R> {
    /// Escape was pressed or the backdrop clicked.
    pub fn should_close(&self) -> bool {
        self.close
    }
}

impl Modal {
    pub fn new(id: Id) -> Modal {
        Modal { id }
    }
    pub fn show<R>(self, ctx: &Context, add: impl FnOnce(&mut Ui) -> R) -> ModalResponse<R> {
        let screen = ctx.screen_rect();
        // A backdrop that dims the window and takes all pointer input.
        let back = LayerId::new(Order::Middle, self.id.with("backdrop"));
        let layer = LayerId::new(Order::Middle, self.id);
        ctx.register_area(back, screen);
        ctx.layer_painter(back)
            .rect_filled(screen, 0, Color32::from_black_alpha(140));
        ctx.bring_to_front(back.id);
        ctx.bring_to_front(layer.id);
        ctx.data_mut(|d| d.insert_temp(layer.id.with("modal"), true));
        let backdrop = ctx.interact(
            back,
            screen,
            screen,
            self.id.with("backdrop_click"),
            Sense::click(),
            true,
        );
        let style = ctx.style();
        let frame = Frame::window(&style);
        let (r, rect, _) = window_body(ctx, layer, frame, 420.0, None, None, None, add);
        let esc = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        let resp = ctx.interact(
            layer,
            rect,
            rect,
            self.id.with("modal"),
            Sense::hover(),
            true,
        );
        ModalResponse {
            inner: r,
            response: resp,
            close: esc || backdrop.clicked(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        ctx: &Context,
        events: Vec<Event>,
        f: impl FnOnce(&Context),
    ) -> super::super::context::FullOutput {
        ctx.run(
            RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                ..Default::default()
            },
            f,
        )
    }

    #[test]
    fn panels_carve_the_window_and_scroll_areas_scroll() {
        let ctx = Context::default();
        let mut central = Rect::NOTHING;
        let mut first_row = Rect::NOTHING;
        let ui_fn = |ctx: &Context, central: &mut Rect, first_row: &mut Rect| {
            SidePanel::left("nav").exact_width(200.0).show(ctx, |ui| {
                ui.label("Nav");
            });
            CentralPanel::default().show(ctx, |ui| {
                *central = ui.max_rect();
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for i in 0..100 {
                            let r = ui.label(format!("Row {i}")).rect;
                            if i == 0 {
                                *first_row = r;
                            }
                        }
                    });
            });
        };
        run(&ctx, vec![], |c| ui_fn(c, &mut central, &mut first_row));
        assert!(central.min.x >= 200.0);
        let y0 = first_row.min.y;
        run(&ctx, vec![Event::PointerMoved(pos2(400.0, 300.0))], |c| {
            ui_fn(c, &mut central, &mut first_row)
        });
        run(
            &ctx,
            vec![Event::MouseWheel {
                delta: vec2(0.0, -120.0),
                modifiers: Modifiers::NONE,
            }],
            |c| ui_fn(c, &mut central, &mut first_row),
        );
        run(&ctx, vec![], |c| ui_fn(c, &mut central, &mut first_row));
        assert!(
            first_row.min.y < y0 - 100.0,
            "{} vs {}",
            first_row.min.y,
            y0
        );
    }

    #[test]
    fn grid_aligns_columns_and_combo_opens() {
        let ctx = Context::default();
        let mut cells = Vec::new();
        for _ in 0..2 {
            cells.clear();
            run(&ctx, vec![], |c| {
                CentralPanel::default().show(c, |ui| {
                    Grid::new("g").num_columns(2).show(ui, |ui| {
                        cells.push(ui.label("a").rect);
                        cells.push(ui.label("b").rect);
                        ui.end_row();
                        cells.push(ui.label("longer text").rect);
                        cells.push(ui.label("c").rect);
                        ui.end_row();
                    });
                });
            });
        }
        assert_eq!(cells[1].min.x, cells[3].min.x);
        assert!(cells[1].min.x > cells[2].max.x);
        assert!(cells[2].min.y > cells[0].max.y);

        let ctx = Context::default();
        let mut choice = 0;
        let mut rect = Rect::NOTHING;
        let frame = |c: &Context, choice: &mut i32, rect: &mut Rect| {
            CentralPanel::default().show(c, |ui| {
                let r = ComboBox::from_id_salt("pick")
                    .selected_text(format!("{choice}"))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(choice, 1, "One");
                        ui.selectable_value(choice, 2, "Two");
                    });
                *rect = r.response.rect;
            });
        };
        run(&ctx, vec![], |c| frame(c, &mut choice, &mut rect));
        let p = rect.center();
        let press = |down| Event::PointerButton {
            pos: p,
            button: PointerButton::Primary,
            pressed: down,
            modifiers: Modifiers::NONE,
        };
        run(&ctx, vec![Event::PointerMoved(p)], |c| {
            frame(c, &mut choice, &mut rect)
        });
        run(&ctx, vec![press(true)], |c| {
            frame(c, &mut choice, &mut rect)
        });
        run(&ctx, vec![press(false)], |c| {
            frame(c, &mut choice, &mut rect)
        });
        run(&ctx, vec![], |c| frame(c, &mut choice, &mut rect));
        assert!(ctx.memory(|m| m.any_popup_open()));
    }
}
