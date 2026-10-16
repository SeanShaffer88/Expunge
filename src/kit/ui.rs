//! `Ui`: a region being laid out, widgets placed one after another.

use super::context::{Context, IdTypeMap, InputState, Memory};
use super::geom::*;
use super::paint::Painter;
use super::style::*;
use super::text::Galley;
use super::*;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    LeftToRight,
    RightToLeft,
    TopDown,
    BottomUp,
}

/// How widgets are placed in a `Ui`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub main_dir: Direction,
    pub main_wrap: bool,
    pub main_align: Align,
    pub main_justify: bool,
    pub cross_align: Align,
    pub cross_justify: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Layout::top_down(Align::Min)
    }
}

impl Layout {
    pub fn top_down(cross: Align) -> Layout {
        Layout {
            main_dir: Direction::TopDown,
            main_wrap: false,
            main_align: Align::Min,
            main_justify: false,
            cross_align: cross,
            cross_justify: false,
        }
    }
    pub fn top_down_justified(cross: Align) -> Layout {
        Layout {
            cross_justify: true,
            ..Layout::top_down(cross)
        }
    }
    pub fn bottom_up(cross: Align) -> Layout {
        Layout {
            main_dir: Direction::BottomUp,
            ..Layout::top_down(cross)
        }
    }
    pub fn left_to_right(cross: Align) -> Layout {
        Layout {
            main_dir: Direction::LeftToRight,
            ..Layout::top_down(cross)
        }
    }
    pub fn right_to_left(cross: Align) -> Layout {
        Layout {
            main_dir: Direction::RightToLeft,
            ..Layout::top_down(cross)
        }
    }
    pub fn centered_and_justified(dir: Direction) -> Layout {
        Layout {
            main_dir: dir,
            main_align: Align::Center,
            main_justify: true,
            cross_align: Align::Center,
            cross_justify: true,
            main_wrap: false,
        }
    }
    pub fn with_main_wrap(mut self, w: bool) -> Self {
        self.main_wrap = w;
        self
    }
    pub fn with_cross_align(mut self, a: Align) -> Self {
        self.cross_align = a;
        self
    }
    pub fn with_cross_justify(mut self, j: bool) -> Self {
        self.cross_justify = j;
        self
    }
    pub fn with_main_justify(mut self, j: bool) -> Self {
        self.main_justify = j;
        self
    }
    pub fn with_main_align(mut self, a: Align) -> Self {
        self.main_align = a;
        self
    }
    pub fn is_horizontal(&self) -> bool {
        matches!(
            self.main_dir,
            Direction::LeftToRight | Direction::RightToLeft
        )
    }
    pub fn is_vertical(&self) -> bool {
        !self.is_horizontal()
    }
}

/// The result of placing and interacting with a widget.
#[derive(Clone)]
pub struct Response {
    pub ctx: Context,
    pub layer_id: LayerId,
    pub id: Id,
    pub rect: Rect,
    pub interact_rect: Rect,
    pub sense: Sense,
    pub enabled: bool,
    pub contains_pointer: bool,
    pub hovered: bool,
    pub(crate) disabled_hovered: bool,
    pub(crate) clicked: bool,
    pub(crate) secondary_clicked: bool,
    pub(crate) double_clicked: bool,
    pub(crate) is_pointer_button_down_on: bool,
    pub(crate) dragged: bool,
    pub(crate) drag_started: bool,
    pub(crate) drag_stopped: bool,
    pub(crate) drag_delta: Vec2,
    pub(crate) has_focus: bool,
    pub(crate) gained_focus: bool,
    pub(crate) lost_focus: bool,
    pub(crate) changed: bool,
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("id", &self.id)
            .field("rect", &self.rect)
            .finish()
    }
}

impl Response {
    pub fn clicked(&self) -> bool {
        self.clicked
    }
    pub fn secondary_clicked(&self) -> bool {
        self.secondary_clicked
    }
    pub fn double_clicked(&self) -> bool {
        self.double_clicked
    }
    pub fn hovered(&self) -> bool {
        self.hovered
    }
    pub fn contains_pointer(&self) -> bool {
        self.contains_pointer
    }
    pub fn changed(&self) -> bool {
        self.changed
    }
    pub fn mark_changed(&mut self) {
        self.changed = true;
    }
    pub fn has_focus(&self) -> bool {
        self.has_focus
    }
    pub fn gained_focus(&self) -> bool {
        self.gained_focus
    }
    pub fn lost_focus(&self) -> bool {
        self.lost_focus
    }
    pub fn dragged(&self) -> bool {
        self.dragged
    }
    pub fn drag_started(&self) -> bool {
        self.drag_started
    }
    pub fn drag_stopped(&self) -> bool {
        self.drag_stopped
    }
    pub fn drag_delta(&self) -> Vec2 {
        self.drag_delta
    }
    pub fn is_pointer_button_down_on(&self) -> bool {
        self.is_pointer_button_down_on
    }
    pub fn request_focus(&self) {
        self.ctx.memory_mut(|m| m.request_focus(self.id));
    }
    pub fn surrender_focus(&self) {
        self.ctx.memory_mut(|m| m.surrender_focus(self.id));
    }
    pub fn interact_pointer_pos(&self) -> Option<Pos2> {
        self.ctx.input(|i| {
            i.pointer
                .pos_if(self.is_pointer_button_down_on || self.clicked)
        })
    }
    pub fn hover_pos(&self) -> Option<Pos2> {
        if self.hovered {
            self.ctx.input(|i| i.pointer.hover_pos())
        } else {
            None
        }
    }
    /// Screen-reader description.
    pub fn widget_info(&self, f: impl FnOnce() -> WidgetInfo) {
        self.ctx.set_widget_info(self.id, f());
    }
    pub fn on_hover_cursor(self, cursor: CursorIcon) -> Self {
        if self.hovered {
            self.ctx.set_cursor_icon(cursor);
        }
        self
    }
    pub fn on_hover_text(self, text: impl Into<WidgetText>) -> Self {
        if self.hovered {
            show_tooltip(&self, text.into());
        }
        self
    }
    pub fn on_hover_text_at_pointer(self, text: impl Into<WidgetText>) -> Self {
        self.on_hover_text(text)
    }
    pub fn on_disabled_hover_text(self, text: impl Into<WidgetText>) -> Self {
        if self.disabled_hovered {
            let mut r = self.clone();
            r.hovered = true;
            show_tooltip(&r, text.into());
        }
        self
    }
    pub fn on_hover_ui(self, add: impl FnOnce(&mut Ui)) -> Self {
        if self.hovered {
            show_tooltip_ui(&self, add);
        }
        self
    }
    /// Right-click menu.
    pub fn context_menu(&self, add: impl FnOnce(&mut Ui)) -> Option<InnerResponse<()>> {
        super::containers::context_menu(self, add)
    }
    pub fn union(&self, o: Response) -> Response {
        let mut r = self.clone();
        r.rect = self.rect.union(o.rect);
        r.hovered |= o.hovered;
        r.clicked |= o.clicked;
        r.changed |= o.changed;
        r.secondary_clicked |= o.secondary_clicked;
        r.double_clicked |= o.double_clicked;
        r.has_focus |= o.has_focus;
        r.lost_focus |= o.lost_focus;
        r
    }
    pub fn highlight(self) -> Self {
        self
    }
    pub fn scroll_to_me(&self, align: Option<Align>) {
        super::containers::scroll_to(&self.ctx, self.rect, align);
    }
}

impl std::ops::BitOr for Response {
    type Output = Response;
    fn bitor(self, o: Response) -> Response {
        self.union(o)
    }
}

impl super::context::PointerState {
    fn pos_if(&self, cond: bool) -> Option<Pos2> {
        if cond {
            self.hover_pos()
        } else {
            None
        }
    }
}

const TOOLTIP_DELAY: f32 = 0.45;

fn tooltip_ready(r: &Response) -> bool {
    let still = r.ctx.input(|i| i.time_since_last_movement());
    if still < TOOLTIP_DELAY {
        r.ctx
            .request_repaint_after(std::time::Duration::from_secs_f32(
                TOOLTIP_DELAY - still + 0.02,
            ));
        return false;
    }
    !r.ctx.input(|i| i.pointer.any_down())
}

fn show_tooltip(r: &Response, text: WidgetText) {
    if !tooltip_ready(r) {
        return;
    }
    show_tooltip_ui(r, |ui| {
        ui.set_max_width(360.0);
        ui.label(text);
    });
}

fn show_tooltip_ui(r: &Response, add: impl FnOnce(&mut Ui)) {
    if !tooltip_ready(r) {
        return;
    }
    let pos = r
        .ctx
        .input(|i| i.pointer.hover_pos())
        .unwrap_or(r.rect.left_bottom());
    super::containers::tooltip_at(&r.ctx, r.id.with("tooltip"), pos + vec2(12.0, 18.0), add);
}

/// What a closure returned plus the response of the region it filled.
pub struct InnerResponse<R> {
    pub inner: R,
    pub response: Response,
}

impl<R> InnerResponse<R> {
    pub fn new(inner: R, response: Response) -> Self {
        InnerResponse { inner, response }
    }
}

/// Options for a child `Ui`.
#[derive(Clone, Default)]
pub struct UiBuilder {
    pub max_rect: Option<Rect>,
    pub layout: Option<Layout>,
    pub id_salt: Option<Id>,
    pub disabled: bool,
    pub layer: Option<LayerId>,
    pub clip: Option<Rect>,
}

impl UiBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn max_rect(mut self, r: Rect) -> Self {
        self.max_rect = Some(r);
        self
    }
    pub fn layout(mut self, l: Layout) -> Self {
        self.layout = Some(l);
        self
    }
    pub fn id_salt(mut self, s: impl std::hash::Hash) -> Self {
        self.id_salt = Some(Id::new(s));
        self
    }
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

/// A region being filled with widgets.
pub struct Ui {
    ctx: Context,
    id: Id,
    next_auto: u64,
    layer: LayerId,
    clip: Rect,
    /// Where widgets may go.
    max_rect: Rect,
    /// Space used so far.
    min_rect: Rect,
    /// Next placement position: left/top edge for forward layouts,
    /// right/bottom for reversed ones; `row` is the current line's extent.
    cursor: Pos2,
    row_height: f32,
    row_top: f32,
    placed_any: bool,
    layout: Layout,
    style: Arc<Style>,
    enabled: bool,
    pub(crate) grid: Option<super::containers::GridLayout>,
    /// A fade applied to everything drawn (page transitions).
    opacity: f32,
    /// Set by `ui.close()`: the containing popup or window should close.
    pub(crate) close_requested: std::rc::Rc<std::cell::Cell<bool>>,
    /// Widgets in a menu close the menu when clicked.
    pub(crate) in_menu: bool,
}

impl Ui {
    /// A top-level `Ui` covering `rect` on `layer`.
    pub fn new(ctx: Context, layer: LayerId, id: Id, rect: Rect) -> Ui {
        let style = ctx.style();
        let h = style.spacing.interact_size.y;
        Ui {
            ctx,
            id,
            next_auto: 0,
            layer,
            clip: rect,
            max_rect: rect,
            min_rect: Rect::from_min_size(rect.min, Vec2::ZERO),
            cursor: rect.min,
            row_height: h,
            row_top: rect.min.y,
            placed_any: false,
            layout: Layout::default(),
            style,
            enabled: true,
            grid: None,
            opacity: 1.0,
            close_requested: Default::default(),
            in_menu: false,
        }
    }

    /// A child laid out inside `max_rect`.
    pub fn new_child(&mut self, b: UiBuilder) -> Ui {
        let max_rect = b
            .max_rect
            .unwrap_or_else(|| self.available_rect_before_wrap());
        let layout = b.layout.unwrap_or(self.layout);
        let id = match b.id_salt {
            Some(s) => self.id.with(s),
            None => self.auto_id(),
        };
        let mut child = Ui {
            ctx: self.ctx.clone(),
            id,
            next_auto: 0,
            layer: b.layer.unwrap_or(self.layer),
            clip: b.clip.unwrap_or(self.clip),
            max_rect,
            min_rect: Rect::from_min_size(max_rect.min, Vec2::ZERO),
            cursor: max_rect.min,
            row_height: self.style.spacing.interact_size.y,
            row_top: max_rect.min.y,
            placed_any: false,
            layout,
            style: self.style.clone(),
            enabled: self.enabled && !b.disabled,
            grid: None,
            opacity: self.opacity,
            close_requested: self.close_requested.clone(),
            in_menu: self.in_menu,
        };
        child.reset_cursor();
        child
    }

    fn reset_cursor(&mut self) {
        let r = self.max_rect;
        self.cursor = match self.layout.main_dir {
            Direction::LeftToRight | Direction::TopDown => r.min,
            Direction::RightToLeft => pos2(r.max.x, r.min.y),
            Direction::BottomUp => pos2(r.min.x, r.max.y),
        };
        self.row_top = r.min.y;
        self.min_rect = match self.layout.main_dir {
            Direction::RightToLeft => Rect::from_min_size(pos2(r.max.x, r.min.y), Vec2::ZERO),
            Direction::BottomUp => Rect::from_min_size(pos2(r.min.x, r.max.y), Vec2::ZERO),
            _ => Rect::from_min_size(r.min, Vec2::ZERO),
        };
    }

    fn auto_id(&mut self) -> Id {
        self.next_auto += 1;
        self.id.with(("auto", self.next_auto))
    }

    /// Id for the next widget that doesn't name its own.
    pub fn next_auto_id(&mut self) -> Id {
        self.auto_id()
    }

    // ---- Accessors ------------------------------------------------------

    pub fn ctx(&self) -> &Context {
        &self.ctx
    }
    pub fn id(&self) -> Id {
        self.id
    }
    pub fn layer_id(&self) -> LayerId {
        self.layer
    }
    pub fn style(&self) -> &Arc<Style> {
        &self.style
    }
    pub fn style_mut(&mut self) -> &mut Style {
        Arc::make_mut(&mut self.style)
    }
    pub fn visuals(&self) -> &Visuals {
        &self.style.visuals
    }
    pub fn visuals_mut(&mut self) -> &mut Visuals {
        &mut self.style_mut().visuals
    }
    pub fn spacing(&self) -> &Spacing {
        &self.style.spacing
    }
    pub fn spacing_mut(&mut self) -> &mut Spacing {
        &mut self.style_mut().spacing
    }
    pub fn layout(&self) -> &Layout {
        &self.layout
    }
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    pub fn set_enabled(&mut self, e: bool) {
        self.enabled &= e;
    }
    pub fn disable(&mut self) {
        self.enabled = false;
    }
    pub fn set_opacity(&mut self, a: f32) {
        self.opacity = a.clamp(0.0, 1.0);
    }
    pub fn opacity(&self) -> f32 {
        self.opacity
    }
    pub fn input<R>(&self, f: impl FnOnce(&InputState) -> R) -> R {
        self.ctx.input(f)
    }
    pub fn input_mut<R>(&self, f: impl FnOnce(&mut InputState) -> R) -> R {
        self.ctx.input_mut(f)
    }
    pub fn memory<R>(&self, f: impl FnOnce(&Memory) -> R) -> R {
        self.ctx.memory(f)
    }
    pub fn memory_mut<R>(&self, f: impl FnOnce(&mut Memory) -> R) -> R {
        self.ctx.memory_mut(f)
    }
    pub fn data<R>(&self, f: impl FnOnce(&IdTypeMap) -> R) -> R {
        self.ctx.data(f)
    }
    pub fn data_mut<R>(&self, f: impl FnOnce(&mut IdTypeMap) -> R) -> R {
        self.ctx.data_mut(f)
    }
    pub fn clip_rect(&self) -> Rect {
        self.clip
    }
    pub fn set_clip_rect(&mut self, r: Rect) {
        self.clip = r;
    }
    pub fn is_rect_visible(&self, r: Rect) -> bool {
        self.clip.intersects(r)
    }
    pub fn max_rect(&self) -> Rect {
        self.max_rect
    }
    pub fn min_rect(&self) -> Rect {
        self.min_rect
    }
    pub fn min_size(&self) -> Vec2 {
        self.min_rect.size()
    }
    pub fn cursor(&self) -> Rect {
        Rect::from_min_size(self.cursor, Vec2::ZERO)
    }

    /// Close the popup, menu or window this `Ui` is in.
    pub fn close(&self) {
        self.close_requested.set(true);
        self.ctx.close_popups();
    }
    pub fn close_menu(&self) {
        self.close();
    }
    pub fn request_repaint(&self) {
        self.ctx.request_repaint();
    }

    pub fn painter(&self) -> Painter {
        let mut p = Painter::new(self.ctx.clone(), self.layer, self.clip);
        if self.opacity < 1.0 {
            p.set_opacity(self.opacity);
        }
        p
    }
    pub fn painter_at(&self, r: Rect) -> Painter {
        self.painter().with_clip_rect(r)
    }

    // ---- Sizes ----------------------------------------------------------

    /// Space left on the current line (horizontal) or below (vertical).
    pub fn available_rect_before_wrap(&self) -> Rect {
        let r = self.max_rect;
        if let Some(g) = &self.grid {
            let c = g.cell_rect();
            return Rect::from_min_max(c.min, pos2(r.max.x.max(c.min.x), r.max.y.max(c.min.y)));
        }
        match self.layout.main_dir {
            Direction::LeftToRight => Rect::from_min_max(
                pos2(self.cursor.x, self.cursor.y),
                pos2(r.max.x, r.max.y.max(self.cursor.y)),
            ),
            Direction::RightToLeft => Rect::from_min_max(
                pos2(r.min.x, self.cursor.y),
                pos2(self.cursor.x, r.max.y.max(self.cursor.y)),
            ),
            Direction::TopDown => Rect::from_min_max(
                pos2(r.min.x, self.cursor.y),
                pos2(r.max.x, r.max.y.max(self.cursor.y)),
            ),
            Direction::BottomUp => Rect::from_min_max(r.min, pos2(r.max.x, self.cursor.y)),
        }
    }
    pub fn available_size(&self) -> Vec2 {
        self.available_rect_before_wrap().size().max(Vec2::ZERO)
    }
    pub fn available_size_before_wrap(&self) -> Vec2 {
        self.available_size()
    }
    pub fn available_width(&self) -> f32 {
        self.available_size().x
    }
    pub fn available_height(&self) -> f32 {
        self.available_size().y
    }

    pub fn set_width(&mut self, w: f32) {
        self.set_min_width(w);
        self.set_max_width(w);
    }
    pub fn set_height(&mut self, h: f32) {
        self.set_min_height(h);
        self.max_rect.max.y = self.max_rect.min.y + h;
    }
    pub fn set_min_width(&mut self, w: f32) {
        if self.layout.main_dir == Direction::RightToLeft {
            self.min_rect.min.x = self.min_rect.min.x.min(self.max_rect.max.x - w);
        } else {
            self.min_rect.max.x = self.min_rect.max.x.max(self.max_rect.min.x + w);
        }
    }
    pub fn set_max_width(&mut self, w: f32) {
        if self.layout.main_dir == Direction::RightToLeft {
            self.max_rect.min.x = self.max_rect.max.x - w;
        } else {
            self.max_rect.max.x = self.max_rect.min.x + w;
        }
    }
    pub fn set_min_height(&mut self, h: f32) {
        self.min_rect.max.y = self.min_rect.max.y.max(self.max_rect.min.y + h);
    }
    pub fn set_max_height(&mut self, h: f32) {
        self.max_rect.max.y = self.max_rect.min.y + h;
    }
    pub fn set_min_size(&mut self, s: Vec2) {
        self.set_min_width(s.x);
        self.set_min_height(s.y);
    }
    pub fn expand_to_include_rect(&mut self, r: Rect) {
        self.min_rect = self.min_rect.union(r);
    }

    // ---- Placement ------------------------------------------------------

    /// Space between widgets in the main direction (x for rows, y for columns).
    fn spacing_main(&self) -> f32 {
        if self.layout.is_horizontal() {
            self.style.spacing.item_spacing.x
        } else {
            self.style.spacing.item_spacing.y
        }
    }

    /// Reserve room for a widget of `desired` size; returns its rectangle.
    pub fn allocate_rect_for(&mut self, desired: Vec2) -> Rect {
        if let Some(g) = &mut self.grid {
            let r = g.place(desired);
            self.min_rect = self.min_rect.union(r);
            self.placed_any = true;
            return r;
        }
        let sp = self.style.spacing.item_spacing;
        let r = self.max_rect;
        let rect = match self.layout.main_dir {
            Direction::TopDown | Direction::BottomUp => {
                let w = if self.layout.cross_justify {
                    r.width().max(desired.x)
                } else {
                    desired.x
                };
                let x = if self.layout.cross_justify {
                    r.min.x
                } else {
                    self.layout.cross_align.place(r.min.x, r.max.x, w)
                };
                let y = if self.layout.main_dir == Direction::TopDown {
                    self.cursor.y
                } else {
                    self.cursor.y - desired.y
                };
                let rect = Rect::from_min_size(pos2(x, y), vec2(w, desired.y));
                if self.layout.main_dir == Direction::TopDown {
                    self.cursor.y = rect.max.y + sp.y;
                } else {
                    self.cursor.y = rect.min.y - sp.y;
                }
                rect
            }
            Direction::LeftToRight | Direction::RightToLeft => {
                let ltr = self.layout.main_dir == Direction::LeftToRight;
                // Wrap to a new line when this widget doesn't fit.
                if self.layout.main_wrap && self.placed_any {
                    let overflow = if ltr {
                        self.cursor.x + desired.x > r.max.x + 0.5
                    } else {
                        self.cursor.x - desired.x < r.min.x - 0.5
                    };
                    if overflow {
                        self.row_top =
                            self.min_rect.max.y.max(self.row_top + self.row_height) + sp.y;
                        self.cursor = pos2(if ltr { r.min.x } else { r.max.x }, self.row_top);
                        self.row_height = self.style.spacing.interact_size.y;
                    }
                }
                self.row_height = self.row_height.max(desired.y);
                let h = if self.layout.cross_justify {
                    self.row_height.max(r.height().min(1e6)).max(desired.y)
                } else {
                    desired.y
                };
                let y = match self.layout.cross_align {
                    _ if self.layout.cross_justify => self.row_top,
                    Align::Min => self.row_top,
                    Align::Center => self.row_top + (self.row_height - h) / 2.0,
                    Align::Max => self.row_top + self.row_height - h,
                };
                let x = if ltr {
                    self.cursor.x
                } else {
                    self.cursor.x - desired.x
                };
                let rect = Rect::from_min_size(pos2(x, y), vec2(desired.x, h));
                self.cursor.x = if ltr {
                    rect.max.x + sp.x
                } else {
                    rect.min.x - sp.x
                };
                // Keep the row's full height in the used area.
                let row = Rect::from_min_size(
                    pos2(rect.min.x, self.row_top),
                    vec2(rect.width(), self.row_height),
                );
                self.min_rect = self.min_rect.union(row);
                rect
            }
        };
        self.placed_any = true;
        self.min_rect = self.min_rect.union(rect);
        rect
    }

    /// Reserve exactly `size` and interact with it.
    pub fn allocate_exact_size(&mut self, size: Vec2, sense: Sense) -> (Rect, Response) {
        let rect = self.allocate_rect_for(size);
        let id = self.auto_id();
        let resp = self.interact(rect, id, sense);
        (rect, resp)
    }

    pub fn allocate_response(&mut self, size: Vec2, sense: Sense) -> Response {
        self.allocate_exact_size(size, sense).1
    }

    pub fn allocate_space(&mut self, size: Vec2) -> (Id, Rect) {
        let rect = self.allocate_rect_for(size);
        (self.auto_id(), rect)
    }

    /// Mark `rect` as used (already-drawn custom content).
    pub fn allocate_rect(&mut self, rect: Rect, sense: Sense) -> Response {
        self.min_rect = self.min_rect.union(rect);
        let id = self.auto_id();
        self.interact(rect, id, sense)
    }

    pub fn interact(&self, rect: Rect, id: Id, sense: Sense) -> Response {
        self.ctx
            .interact(self.layer, self.clip, rect, id, sense, self.enabled)
    }

    pub fn add_space(&mut self, amount: f32) {
        match self.layout.main_dir {
            Direction::TopDown => self.cursor.y += amount,
            Direction::BottomUp => self.cursor.y -= amount,
            Direction::LeftToRight => self.cursor.x += amount,
            Direction::RightToLeft => self.cursor.x -= amount,
        }
        // Space at the end still counts toward the used size.
        let c = self.cursor;
        self.min_rect = self.min_rect.union(Rect::from_min_max(c, c));
        // ...minus the spacing the next widget would add.
        let _ = self.spacing_main();
    }

    pub fn end_row(&mut self) {
        if let Some(g) = &mut self.grid {
            g.end_row();
            let (x, y) = g.row_start();
            self.cursor = pos2(x, y);
            return;
        }
        let sp = self.style.spacing.item_spacing.y;
        self.row_top = self.min_rect.max.y + sp;
        self.cursor = pos2(self.max_rect.min.x, self.row_top);
        self.row_height = self.style.spacing.interact_size.y;
    }

    /// Place a finished child: its used area becomes one widget here.
    fn place_child(&mut self, child: &Ui) -> Rect {
        let used = child.min_rect;
        if let Some(g) = &mut self.grid {
            g.cell_used(used);
            if used.is_finite() {
                self.min_rect = self.min_rect.union(used);
            }
            self.placed_any = true;
            return used;
        }
        let size = used.size().max(Vec2::ZERO);
        // The child was laid out at our cursor; account for exactly the space it used.
        let sp = self.style.spacing.item_spacing;
        match self.layout.main_dir {
            Direction::TopDown => {
                self.cursor.y = used.max.y.max(self.cursor.y) + sp.y;
            }
            Direction::BottomUp => {
                self.cursor.y = used.min.y.min(self.cursor.y) - sp.y;
            }
            Direction::LeftToRight => {
                self.row_height = self.row_height.max(size.y);
                self.cursor.x = used.max.x.max(self.cursor.x) + sp.x;
            }
            Direction::RightToLeft => {
                self.row_height = self.row_height.max(size.y);
                self.cursor.x = used.min.x.min(self.cursor.x) - sp.x;
            }
        }
        self.placed_any = true;
        if used.is_finite() {
            self.min_rect = self.min_rect.union(used);
        }
        used
    }

    /// Lay out a child region and place it like a widget.
    pub fn scope_builder<R>(
        &mut self,
        b: UiBuilder,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        let mut child = self.new_child(b);
        let inner = add(&mut child);
        let rect = self.place_child(&child);
        let id = child.id;
        InnerResponse::new(inner, self.interact(rect, id, Sense::hover()))
    }

    pub fn scope<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        self.scope_builder(UiBuilder::new(), add)
    }

    pub fn push_id<R>(
        &mut self,
        salt: impl std::hash::Hash,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        self.scope_builder(UiBuilder::new().id_salt(salt), add)
    }

    pub fn with_layout<R>(
        &mut self,
        layout: Layout,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        // A row inside a column takes one line's height; other layouts take what's left.
        let avail = self.available_rect_before_wrap();
        let mut b = UiBuilder::new().layout(layout).max_rect(avail);
        if layout.is_horizontal() && self.layout.is_vertical() {
            let h = self.style.spacing.interact_size.y;
            b.max_rect = Some(Rect::from_min_size(
                avail.min,
                vec2(avail.width(), avail.height().max(h)),
            ));
        }
        let mut child = self.new_child(b);
        if layout.is_horizontal() {
            child.row_height = self.style.spacing.interact_size.y;
        }
        let inner = add(&mut child);
        // A right-to-left row spans the full width it was given.
        if layout.main_dir == Direction::RightToLeft && self.layout.is_vertical() {
            child.min_rect.min.x = child.min_rect.min.x.min(avail.min.x);
            child.min_rect.max.x = child.min_rect.max.x.max(avail.max.x);
        }
        let rect = self.place_child(&child);
        let id = child.id;
        InnerResponse::new(inner, self.interact(rect, id, Sense::hover()))
    }

    pub fn horizontal<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        self.with_layout(Layout::left_to_right(Align::Center), add)
    }
    pub fn horizontal_top<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        self.with_layout(Layout::left_to_right(Align::Min), add)
    }
    pub fn horizontal_centered<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        self.with_layout(Layout::left_to_right(Align::Center), add)
    }
    pub fn horizontal_wrapped<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        self.with_layout(
            Layout::left_to_right(Align::Center).with_main_wrap(true),
            add,
        )
    }
    pub fn vertical<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        self.with_layout(Layout::top_down(Align::Min), add)
    }
    pub fn vertical_centered<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        self.with_layout(Layout::top_down(Align::Center), add)
    }
    pub fn vertical_centered_justified<R>(
        &mut self,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        self.with_layout(Layout::top_down_justified(Align::Center), add)
    }

    pub fn indent<R>(
        &mut self,
        id_salt: impl std::hash::Hash,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        let indent = self.style.spacing.indent;
        let avail = self.available_rect_before_wrap();
        let r = Rect::from_min_max(pos2(avail.min.x + indent, avail.min.y), avail.max);
        self.scope_builder(
            UiBuilder::new()
                .id_salt(id_salt)
                .max_rect(r)
                .layout(Layout::top_down(Align::Min)),
            add,
        )
    }

    pub fn add_enabled_ui<R>(
        &mut self,
        enabled: bool,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        let mut b = UiBuilder::new();
        b.disabled = !enabled;
        // Keep the parent's layout so a row stays a row.
        let mut child = self.new_child(b);
        child.row_height = self.row_height;
        child.row_top = self.row_top;
        let inner = add(&mut child);
        let rect = self.place_child(&child);
        let id = child.id;
        InnerResponse::new(inner, self.interact(rect, id, Sense::hover()))
    }

    /// `n` equal columns side by side.
    pub fn columns<R>(&mut self, n: usize, add: impl FnOnce(&mut [Ui]) -> R) -> R {
        let avail = self.available_rect_before_wrap();
        let gap = self.style.spacing.item_spacing.x;
        let w = ((avail.width() - gap * (n.saturating_sub(1)) as f32) / n.max(1) as f32).max(0.0);
        let mut cols: Vec<Ui> = (0..n)
            .map(|i| {
                let r = Rect::from_min_size(
                    pos2(avail.min.x + i as f32 * (w + gap), avail.min.y),
                    vec2(w, avail.height()),
                );
                let mut c = self.new_child(
                    UiBuilder::new()
                        .max_rect(r)
                        .layout(Layout::top_down_justified(Align::Min))
                        .id_salt(("col", i)),
                );
                c.layout.cross_justify = false;
                c
            })
            .collect();
        let r = add(&mut cols);
        let mut used = Rect::from_min_size(avail.min, vec2(avail.width(), 0.0));
        for c in &cols {
            used = used.union(c.min_rect);
        }
        self.cursor.y = used.max.y + self.style.spacing.item_spacing.y;
        self.min_rect = self.min_rect.union(used);
        self.placed_any = true;
        r
    }

    /// Place a widget at an exact size, centred in that space.
    pub fn add_sized(&mut self, size: impl Into<Vec2>, widget: impl Widget) -> Response {
        let size = size.into();
        let rect = self.allocate_rect_for(size);
        let mut child = self.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::centered_and_justified(Direction::TopDown)),
        );
        widget.ui(&mut child)
    }

    pub fn put(&mut self, rect: Rect, widget: impl Widget) -> Response {
        self.min_rect = self.min_rect.union(rect);
        let mut child = self.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::centered_and_justified(Direction::TopDown)),
        );
        widget.ui(&mut child)
    }

    pub fn add(&mut self, widget: impl Widget) -> Response {
        widget.ui(self)
    }

    pub fn add_enabled(&mut self, enabled: bool, widget: impl Widget) -> Response {
        if enabled || !self.enabled {
            widget.ui(self)
        } else {
            self.enabled = false;
            let r = widget.ui(self);
            self.enabled = true;
            r
        }
    }

    /// Let a widget size itself from the text it shows.
    pub fn layout_text(
        &self,
        text: &WidgetText,
        default_style: &TextStyle,
        wrap: f32,
        default_color: Color32,
    ) -> (Arc<Galley>, Color32) {
        let (st, color) = text
            .rich()
            .resolve(&self.style, default_style, default_color);
        (self.ctx.layout(text.text(), st, wrap), color)
    }

    /// Text colour for a widget's state.
    pub fn text_color(&self) -> Color32 {
        let c = self.style.visuals.text_color();
        if self.enabled {
            c
        } else {
            c.gamma_multiply(0.5)
        }
    }

    /// Wrap width for text placed now: the rest of the line in a wrapping
    /// row, the full width in a column, unlimited in a plain row.
    pub fn wrap_width(&self) -> f32 {
        if self.layout.is_vertical() || self.layout.main_wrap {
            self.max_rect.width()
        } else {
            f32::INFINITY
        }
    }

    // ---- Convenience widgets (in widgets.rs) are added via `impl Ui` there.
}

/// Anything that can be added with `ui.add(...)`.
pub trait Widget {
    fn ui(self, ui: &mut Ui) -> Response;
}

impl<F: FnOnce(&mut Ui) -> Response> Widget for F {
    fn ui(self, ui: &mut Ui) -> Response {
        self(ui)
    }
}
