//! The shared state behind every frame: input, focus, memory between
//! frames, the shapes being recorded and which widget the pointer is on.

use super::geom::*;
use super::paint::{ClippedShape, Painter, Shape};
use super::style::Style;
use super::text::{Fonts, Galley, TextBackend, TextStyleResolved};
use super::ui::Response;
use super::*;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// Values kept between frames, by id and type.
#[derive(Default)]
pub struct IdTypeMap {
    map: HashMap<(Id, TypeId), Box<dyn Any + Send + Sync>>,
}

impl IdTypeMap {
    pub fn get_temp<T: Clone + 'static>(&self, id: Id) -> Option<T> {
        self.map
            .get(&(id, TypeId::of::<T>()))
            .and_then(|b| b.downcast_ref::<T>())
            .cloned()
    }
    pub fn insert_temp<T: Clone + Send + Sync + 'static>(&mut self, id: Id, v: T) {
        self.map.insert((id, TypeId::of::<T>()), Box::new(v));
    }
    pub fn get_temp_mut_or_default<T: Default + Clone + Send + Sync + 'static>(
        &mut self,
        id: Id,
    ) -> &mut T {
        self.map
            .entry((id, TypeId::of::<T>()))
            .or_insert_with(|| Box::new(T::default()))
            .downcast_mut::<T>()
            .expect("type")
    }
    pub fn get_temp_mut_or_insert_with<T: Clone + Send + Sync + 'static>(
        &mut self,
        id: Id,
        f: impl FnOnce() -> T,
    ) -> &mut T {
        self.map
            .entry((id, TypeId::of::<T>()))
            .or_insert_with(|| Box::new(f()))
            .downcast_mut::<T>()
            .expect("type")
    }
    pub fn remove<T: 'static>(&mut self, id: Id) {
        self.map.remove(&(id, TypeId::of::<T>()));
    }
    pub fn get_persisted<T: Clone + 'static>(&self, id: Id) -> Option<T> {
        self.get_temp(id)
    }
    pub fn insert_persisted<T: Clone + Send + Sync + 'static>(&mut self, id: Id, v: T) {
        self.insert_temp(id, v)
    }
}

/// Focus, open popups and other state that outlives a frame.
#[derive(Default)]
pub struct Memory {
    pub data: IdTypeMap,
    focus: Option<Id>,
    /// Focus at the start of this frame (for gained/lost focus).
    focus_start: Option<Id>,
    /// Focus one frame ago.
    focus_prev: Option<Id>,
    popup: Option<Id>,
    /// Windows from back to front.
    window_order: Vec<Id>,
    /// Move focus by Tab (+1) / Shift+Tab (−1) once widgets are known.
    pending_tab: Option<i32>,
}

impl Memory {
    pub fn focused(&self) -> Option<Id> {
        self.focus
    }
    pub fn has_focus(&self, id: Id) -> bool {
        self.focus == Some(id)
    }
    pub fn request_focus(&mut self, id: Id) {
        self.focus = Some(id);
    }
    pub fn surrender_focus(&mut self, id: Id) {
        if self.focus == Some(id) {
            self.focus = None;
        }
    }
    pub fn is_popup_open(&self, id: Id) -> bool {
        self.popup == Some(id)
    }
    pub fn any_popup_open(&self) -> bool {
        self.popup.is_some()
    }
    pub fn open_popup(&mut self, id: Id) {
        self.popup = Some(id);
    }
    pub fn toggle_popup(&mut self, id: Id) {
        self.popup = if self.popup == Some(id) {
            None
        } else {
            Some(id)
        };
    }
    pub fn close_popup(&mut self) {
        self.popup = None;
    }
    fn bring_to_front(&mut self, id: Id) {
        self.window_order.retain(|w| *w != id);
        self.window_order.push(id);
    }
}

#[derive(Clone, Debug, Default)]
pub struct PointerState {
    pos: Option<Pos2>,
    down: [bool; 3],
    pressed: [bool; 3],
    released: [bool; 3],
    press_origin: Option<Pos2>,
    delta: Vec2,
    last_move_time: f64,
}

impl PointerState {
    fn idx(b: PointerButton) -> usize {
        match b {
            PointerButton::Primary => 0,
            PointerButton::Secondary => 1,
            PointerButton::Middle => 2,
        }
    }
    pub fn hover_pos(&self) -> Option<Pos2> {
        self.pos
    }
    pub fn interact_pos(&self) -> Option<Pos2> {
        self.pos
    }
    pub fn latest_pos(&self) -> Option<Pos2> {
        self.pos
    }
    pub fn press_origin(&self) -> Option<Pos2> {
        self.press_origin
    }
    pub fn any_pressed(&self) -> bool {
        self.pressed.iter().any(|b| *b)
    }
    pub fn any_released(&self) -> bool {
        self.released.iter().any(|b| *b)
    }
    pub fn any_down(&self) -> bool {
        self.down.iter().any(|b| *b)
    }
    pub fn primary_down(&self) -> bool {
        self.down[0]
    }
    pub fn primary_pressed(&self) -> bool {
        self.pressed[0]
    }
    pub fn primary_released(&self) -> bool {
        self.released[0]
    }
    pub fn primary_clicked(&self) -> bool {
        self.released[0]
    }
    pub fn button_down(&self, b: PointerButton) -> bool {
        self.down[Self::idx(b)]
    }
    pub fn button_pressed(&self, b: PointerButton) -> bool {
        self.pressed[Self::idx(b)]
    }
    pub fn delta(&self) -> Vec2 {
        self.delta
    }
    pub fn is_moving(&self) -> bool {
        self.delta != Vec2::ZERO
    }
}

/// Input of the current frame.
#[derive(Clone, Debug, Default)]
pub struct InputState {
    pub pointer: PointerState,
    pub time: f64,
    pub unstable_dt: f32,
    pub modifiers: Modifiers,
    pub events: Vec<Event>,
    pub screen_rect: Rect,
    /// Wheel movement not yet used by a scroll area (points).
    pub smooth_scroll_delta: Vec2,
    pub raw_scroll_delta: Vec2,
    pub focused: bool,
    keys_down: Vec<Key>,
}

impl InputState {
    pub fn key_pressed(&self, key: Key) -> bool {
        self.events
            .iter()
            .any(|e| matches!(e, Event::Key { key: k, pressed: true, .. } if *k == key))
    }
    pub fn key_down(&self, key: Key) -> bool {
        self.keys_down.contains(&key)
    }
    pub fn key_released(&self, key: Key) -> bool {
        self.events
            .iter()
            .any(|e| matches!(e, Event::Key { key: k, pressed: false, .. } if *k == key))
    }
    /// Remove a press of `key` with exactly `modifiers`; true if there was one.
    pub fn consume_key(&mut self, modifiers: Modifiers, key: Key) -> bool {
        let before = self.events.len();
        let mut found = false;
        self.events.retain(|e| match e {
            Event::Key {
                key: k,
                pressed: true,
                modifiers: m,
                ..
            } if *k == key && m.matches_exact(modifiers) && !found => {
                found = true;
                false
            }
            _ => true,
        });
        found && self.events.len() < before
    }
    pub fn screen_rect(&self) -> Rect {
        self.screen_rect
    }
    pub fn time_since_last_movement(&self) -> f32 {
        (self.time - self.pointer.last_move_time) as f32
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ViewportCommand {
    Close,
    Title(String),
    Focus,
    Minimized(bool),
}

/// A widget placed this frame, for hit testing next frame and for screen readers.
#[derive(Clone, Debug)]
pub struct WidgetRecord {
    pub id: Id,
    pub layer: LayerId,
    pub rect: Rect,
    pub clip: Rect,
    pub sense: Sense,
    pub enabled: bool,
    pub info: Option<WidgetInfo>,
}

/// What the platform should do after a frame.
#[derive(Default)]
pub struct FullOutput {
    /// Shapes in drawing order.
    pub shapes: Vec<ClippedShape>,
    pub cursor: CursorIcon,
    pub copied_text: String,
    /// Draw again after this long even without input (None = only on input).
    pub repaint_after: Option<Duration>,
    pub commands: Vec<ViewportCommand>,
    pub widgets: Vec<WidgetRecord>,
    /// Where text is being typed (points), for the IME candidate window.
    pub ime_rect: Option<Rect>,
    pub pixels_per_point: f32,
    pub open_url: Option<String>,
}

struct Anim {
    value: f32,
    target: bool,
    time: f64,
}

pub(crate) struct Inner {
    pub style: Arc<Style>,
    pub fonts: Fonts,
    pub input: InputState,
    pub memory: Memory,
    layers: HashMap<LayerId, Vec<ClippedShape>>,
    /// Areas placed this frame and last frame, in placement order.
    areas: Vec<(LayerId, Rect)>,
    prev_areas: Vec<(LayerId, Rect)>,
    widgets: Vec<WidgetRecord>,
    prev_widgets: Vec<WidgetRecord>,
    /// The topmost interactive widget under the pointer (from last frame's layout).
    hover_top: Option<Id>,
    /// The layer the pointer is over.
    hover_layer: Option<LayerId>,
    /// Widget the primary button went down on.
    pressed: Option<Id>,
    /// Widget activated with Enter/Space this frame.
    key_click: Option<Id>,
    last_click: Option<(Id, f64)>,
    double_click: Option<Id>,
    anims: HashMap<Id, Anim>,
    repaint_after: Option<f64>,
    commands: Vec<ViewportCommand>,
    cursor: CursorIcon,
    copied: String,
    zoom: f32,
    native_ppp: f32,
    /// Area still free for panels (shrinks as panels are added).
    pub available: Rect,
    ime_rect: Option<Rect>,
    pub wants_keyboard: bool,
    open_url: Option<String>,
    frame_nr: u64,
    /// Close the popup / menu stack at the end of this frame.
    close_popups: bool,
}

struct Shared {
    inner: Mutex<Inner>,
    repaint: AtomicBool,
    waker: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

/// Cheap to clone; all clones share the same state.
#[derive(Clone)]
pub struct Context(Arc<Shared>);

impl Default for Context {
    fn default() -> Self {
        Context::new(Box::new(super::text::MockText))
    }
}

impl Context {
    pub fn new(text: Box<dyn TextBackend>) -> Context {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1024.0, 768.0));
        Context(Arc::new(Shared {
            inner: Mutex::new(Inner {
                style: Arc::new(Style::default()),
                fonts: Fonts::new(text),
                input: InputState {
                    screen_rect: screen,
                    ..Default::default()
                },
                memory: Memory::default(),
                layers: HashMap::new(),
                areas: Vec::new(),
                prev_areas: Vec::new(),
                widgets: Vec::new(),
                prev_widgets: Vec::new(),
                hover_top: None,
                hover_layer: None,
                pressed: None,
                key_click: None,
                last_click: None,
                double_click: None,
                anims: HashMap::new(),
                repaint_after: None,
                commands: Vec::new(),
                cursor: CursorIcon::Default,
                copied: String::new(),
                zoom: 1.0,
                native_ppp: 1.0,
                available: screen,
                ime_rect: None,
                wants_keyboard: false,
                open_url: None,
                frame_nr: 0,
                close_popups: false,
            }),
            repaint: AtomicBool::new(true),
            waker: Mutex::new(None),
        }))
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Called from any thread to wake the window.
    pub fn set_waker(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.0.waker.lock().unwrap() = Some(Arc::new(f));
    }

    // ---- Repaint --------------------------------------------------------

    /// Draw another frame soon (safe from any thread).
    pub fn request_repaint(&self) {
        if !self.0.repaint.swap(true, AtomicOrdering::AcqRel) {
            if let Some(w) = self.0.waker.lock().unwrap().clone() {
                w();
            }
        }
    }

    /// Draw another frame within `d`.
    pub fn request_repaint_after(&self, d: Duration) {
        if d.is_zero() {
            return self.request_repaint();
        }
        // From another thread the frame lock may be held; the waker is enough.
        match self.0.inner.try_lock() {
            Ok(mut c) => {
                let at = c.input.time + d.as_secs_f64();
                c.repaint_after = Some(c.repaint_after.map_or(at, |t| t.min(at)));
            }
            Err(_) => self.request_repaint(),
        }
    }

    /// Whether a repaint was requested since the last frame (and reset it).
    pub fn take_repaint_request(&self) -> bool {
        self.0.repaint.swap(false, AtomicOrdering::AcqRel)
    }

    // ---- Access ---------------------------------------------------------

    pub fn input<R>(&self, f: impl FnOnce(&InputState) -> R) -> R {
        f(&self.lock().input)
    }
    pub fn input_mut<R>(&self, f: impl FnOnce(&mut InputState) -> R) -> R {
        f(&mut self.lock().input)
    }
    pub fn memory<R>(&self, f: impl FnOnce(&Memory) -> R) -> R {
        f(&self.lock().memory)
    }
    pub fn memory_mut<R>(&self, f: impl FnOnce(&mut Memory) -> R) -> R {
        f(&mut self.lock().memory)
    }
    pub fn data<R>(&self, f: impl FnOnce(&IdTypeMap) -> R) -> R {
        f(&self.lock().memory.data)
    }
    pub fn data_mut<R>(&self, f: impl FnOnce(&mut IdTypeMap) -> R) -> R {
        f(&mut self.lock().memory.data)
    }
    pub fn style(&self) -> Arc<Style> {
        self.lock().style.clone()
    }
    pub fn set_style(&self, s: Style) {
        self.lock().style = Arc::new(s);
    }
    pub fn style_mut(&self, f: impl FnOnce(&mut Style)) {
        let mut c = self.lock();
        let mut s = (*c.style).clone();
        f(&mut s);
        c.style = Arc::new(s);
    }
    pub fn set_visuals(&self, v: Visuals) {
        self.style_mut(|s| s.visuals = v);
    }
    pub fn zoom_factor(&self) -> f32 {
        self.lock().zoom
    }
    pub fn set_zoom_factor(&self, z: f32) {
        let mut c = self.lock();
        if (c.zoom - z).abs() > 1e-4 {
            c.zoom = z;
            let ppp = c.native_ppp * z;
            c.fonts.set_ppp(ppp);
            drop(c);
            self.request_repaint();
        }
    }
    pub fn pixels_per_point(&self) -> f32 {
        let c = self.lock();
        c.native_ppp * c.zoom
    }
    pub fn screen_rect(&self) -> Rect {
        self.lock().input.screen_rect
    }
    pub fn content_rect(&self) -> Rect {
        self.screen_rect()
    }
    pub fn available_rect(&self) -> Rect {
        self.lock().available
    }
    pub fn send_viewport_cmd(&self, cmd: ViewportCommand) {
        self.lock().commands.push(cmd);
        self.request_repaint();
    }
    pub fn copy_text(&self, text: String) {
        self.lock().copied = text;
    }
    pub fn open_url(&self, url: impl Into<String>) {
        self.lock().open_url = Some(url.into());
    }
    pub fn set_cursor_icon(&self, c: CursorIcon) {
        self.lock().cursor = c;
    }
    /// A text box has keyboard focus.
    pub fn wants_keyboard_input(&self) -> bool {
        self.lock().wants_keyboard
    }
    pub fn frame_nr(&self) -> u64 {
        self.lock().frame_nr
    }
    pub fn set_ime_rect(&self, r: Rect) {
        self.lock().ime_rect = Some(r);
    }

    pub fn load_texture(
        &self,
        name: impl Into<String>,
        image: ColorImage,
        _opts: TextureOptions,
    ) -> TextureHandle {
        let mut rgba = Vec::with_capacity(image.pixels.len() * 4);
        for p in &image.pixels {
            rgba.extend_from_slice(&[p.r, p.g, p.b, p.a]);
        }
        TextureHandle {
            rgba: Arc::new(rgba),
            size: image.size,
            name: name.into(),
        }
    }

    /// A painter over the whole window on `layer`.
    pub fn layer_painter(&self, layer: LayerId) -> Painter {
        Painter::new(self.clone(), layer, Rect::EVERYTHING)
    }

    pub fn layout(&self, text: &str, style: TextStyleResolved, wrap: f32) -> Arc<Galley> {
        self.lock().fonts.layout(text, style, wrap)
    }

    pub fn text_width(&self, text: &str, style: TextStyleResolved) -> f32 {
        self.lock().fonts.width(text, style_ref(&style))
    }

    pub(crate) fn add_shape(&self, layer: LayerId, clip: Rect, shape: Shape) {
        if let Shape::Noop = shape {
            return;
        }
        self.lock()
            .layers
            .entry(layer)
            .or_default()
            .push(ClippedShape { clip, shape });
    }

    /// Record an area (panel, window, popup) for pointer routing.
    pub(crate) fn register_area(&self, layer: LayerId, rect: Rect) {
        let mut c = self.lock();
        if let Some(a) = c.areas.iter_mut().find(|(l, _)| *l == layer) {
            a.1 = a.1.union(rect);
        } else {
            c.areas.push((layer, rect));
        }
        if layer.order == Order::Middle && !c.memory.window_order.contains(&layer.id) {
            c.memory.window_order.push(layer.id);
        }
    }

    pub(crate) fn bring_to_front(&self, id: Id) {
        self.lock().memory.bring_to_front(id);
    }

    /// The previous frame's rectangle of an area.
    pub fn area_rect(&self, layer: LayerId) -> Option<Rect> {
        self.lock()
            .prev_areas
            .iter()
            .find(|(l, _)| *l == layer)
            .map(|(_, r)| *r)
    }

    pub fn is_pointer_over_area(&self, layer: LayerId) -> bool {
        self.lock().hover_layer == Some(layer)
    }

    pub fn layer_under_pointer(&self) -> Option<LayerId> {
        self.lock().hover_layer
    }

    /// Close any open popup or menu at the end of the frame.
    pub fn close_popups(&self) {
        self.lock().close_popups = true;
    }

    // ---- Animation ------------------------------------------------------

    /// 0..1, moving toward `value` over `time` seconds.
    pub fn animate_bool_with_time(&self, id: Id, value: bool, time: f32) -> f32 {
        let mut c = self.lock();
        let now = c.input.time;
        let anim_time = if c.style.animation_time <= 0.0 {
            0.0
        } else {
            time
        };
        let a = c.anims.entry(id).or_insert(Anim {
            value: if value { 1.0 } else { 0.0 },
            target: value,
            time: now,
        });
        let dt = (now - a.time) as f32;
        a.time = now;
        if a.target != value {
            a.target = value;
        }
        let goal = if value { 1.0 } else { 0.0 };
        if anim_time <= 0.0 {
            a.value = goal;
        } else {
            let step = dt / anim_time;
            a.value = if goal > a.value {
                (a.value + step).min(1.0)
            } else {
                (a.value - step).max(0.0)
            };
        }
        let v = a.value;
        if v != goal {
            let at = now + 1.0 / 60.0;
            c.repaint_after = Some(c.repaint_after.map_or(at, |t| t.min(at)));
        }
        v
    }

    pub fn animate_bool(&self, id: Id, value: bool) -> f32 {
        let t = self.style().animation_time;
        self.animate_bool_with_time(id, value, t)
    }

    // ---- Widgets --------------------------------------------------------

    /// The previous frame's response of a widget.
    pub fn read_response(&self, id: Id) -> Option<Response> {
        let w = {
            let c = self.lock();
            c.prev_widgets.iter().find(|w| w.id == id).cloned()
        }?;
        Some(self.interact_inner(&w, false))
    }

    /// Register a widget and work out how the user is interacting with it.
    pub fn interact(
        &self,
        layer: LayerId,
        clip: Rect,
        rect: Rect,
        id: Id,
        sense: Sense,
        enabled: bool,
    ) -> Response {
        let rec = WidgetRecord {
            id,
            layer,
            rect,
            clip,
            sense,
            enabled,
            info: None,
        };
        let r = self.interact_inner(&rec, true);
        self.lock().widgets.push(rec);
        r
    }

    pub(crate) fn set_widget_info(&self, id: Id, info: WidgetInfo) {
        let mut c = self.lock();
        if let Some(w) = c.widgets.iter_mut().rev().find(|w| w.id == id) {
            w.info = Some(info);
        }
    }

    fn interact_inner(&self, w: &WidgetRecord, live: bool) -> Response {
        let mut c = self.lock();
        let pointer = c.input.pointer.pos;
        let visible = w.rect.intersect(w.clip);
        let contains = pointer.is_some_and(|p| visible.contains(p));
        let on_layer = c.hover_layer.is_none_or(|l| l == w.layer);
        let interactive = w.enabled && w.sense.interactive();
        let hovered = contains
            && on_layer
            && if w.sense.interactive() {
                // Last frame's hit test, or geometry for a widget not seen before.
                c.hover_top.is_none_or(|t| t == w.id)
                    || !c.prev_widgets.iter().any(|p| p.id == w.id)
            } else {
                true
            };
        let pressed_here = c.pressed == Some(w.id);
        let released = c.input.pointer.released[0];
        let mut clicked = interactive && w.sense.click && pressed_here && released && contains;
        if interactive && c.key_click == Some(w.id) {
            clicked = true;
        }
        let secondary_clicked = interactive && contains && hovered && c.input.pointer.released[1];
        let double_clicked = clicked && c.double_click == Some(w.id);
        let dragged = interactive && w.sense.drag && pressed_here && c.input.pointer.down[0];
        let drag_started = dragged && c.input.pointer.pressed[0];
        let drag_stopped = interactive && w.sense.drag && pressed_here && released;
        let focused = c.memory.focus == Some(w.id);
        let gained_focus = focused && c.memory.focus_prev != Some(w.id);
        let lost_focus = !focused && c.memory.focus_prev == Some(w.id);
        if live && hovered && interactive && w.sense.click {
            c.cursor = CursorIcon::Default;
        }
        drop(c);
        Response {
            ctx: self.clone(),
            layer_id: w.layer,
            id: w.id,
            rect: w.rect,
            interact_rect: visible,
            sense: w.sense,
            enabled: w.enabled,
            contains_pointer: contains,
            hovered: hovered && (w.enabled || !w.sense.interactive()),
            disabled_hovered: hovered && !w.enabled,
            clicked,
            secondary_clicked,
            double_clicked,
            is_pointer_button_down_on: pressed_here && interactive,
            dragged,
            drag_started,
            drag_stopped,
            drag_delta: if dragged {
                self.input(|i| i.pointer.delta)
            } else {
                Vec2::ZERO
            },
            has_focus: focused,
            gained_focus,
            lost_focus,
            changed: false,
        }
    }

    // ---- Frames ---------------------------------------------------------

    /// Run one frame: feed input, build the UI with `f`, collect output.
    pub fn run(&self, raw: RawInput, f: impl FnOnce(&Context)) -> FullOutput {
        self.begin_frame(raw);
        f(self);
        self.end_frame()
    }

    pub fn begin_frame(&self, raw: RawInput) {
        self.0.repaint.store(false, AtomicOrdering::Release);
        let mut guard = self.lock();
        let c = &mut *guard;
        c.frame_nr += 1;
        if let Some(ppp) = raw.pixels_per_point {
            c.native_ppp = ppp;
        }
        let ppp = c.native_ppp * c.zoom;
        c.fonts.set_ppp(ppp);
        let prev_time = c.input.time;
        let time = raw.time.unwrap_or(prev_time + 1.0 / 60.0);
        let screen = raw.screen_rect.unwrap_or(c.input.screen_rect);
        c.input.time = time;
        c.input.unstable_dt = (time - prev_time).max(0.0) as f32;
        c.input.screen_rect = screen;
        c.input.modifiers = raw.modifiers;
        c.input.pointer.pressed = [false; 3];
        c.input.pointer.released = [false; 3];
        c.input.pointer.delta = Vec2::ZERO;
        c.input.smooth_scroll_delta = Vec2::ZERO;
        c.input.raw_scroll_delta = Vec2::ZERO;
        c.input.events.clear();
        c.available = screen;
        c.repaint_after = None;
        c.cursor = CursorIcon::Default;
        c.copied.clear();
        c.ime_rect = None;
        c.key_click = None;
        c.double_click = None;
        c.open_url = None;
        c.close_popups = false;
        c.prev_widgets = std::mem::take(&mut c.widgets);
        c.prev_areas = std::mem::take(&mut c.areas);
        for v in c.layers.values_mut() {
            v.clear();
        }

        for e in &raw.events {
            match e {
                Event::PointerMoved(p) => {
                    if let Some(old) = c.input.pointer.pos {
                        c.input.pointer.delta += *p - old;
                    }
                    c.input.pointer.pos = Some(*p);
                    c.input.pointer.last_move_time = time;
                }
                Event::PointerGone => c.input.pointer.pos = None,
                Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    ..
                } => {
                    let i = PointerState::idx(*button);
                    c.input.pointer.pos = Some(*pos);
                    if *pressed {
                        c.input.pointer.pressed[i] = true;
                        c.input.pointer.down[i] = true;
                        if i == 0 {
                            c.input.pointer.press_origin = Some(*pos);
                        }
                    } else {
                        c.input.pointer.released[i] = true;
                        c.input.pointer.down[i] = false;
                    }
                }
                Event::MouseWheel { delta, .. } => {
                    c.input.smooth_scroll_delta += *delta;
                    c.input.raw_scroll_delta += *delta;
                }
                Event::Key { key, pressed, .. } => {
                    if *pressed {
                        if !c.input.keys_down.contains(key) {
                            c.input.keys_down.push(*key);
                        }
                    } else {
                        c.input.keys_down.retain(|k| k != key);
                    }
                }
                Event::WindowFocused(f) => c.input.focused = *f,
                _ => {}
            }
        }
        c.input.events = raw.events;

        // Which area and which interactive widget the pointer is over,
        // judged by last frame's layout.
        let pointer = c.input.pointer.pos;
        let order = Self::area_order(&c.prev_areas, &c.memory.window_order);
        c.hover_layer = pointer.and_then(|p| {
            order
                .iter()
                .rev()
                .find(|(_, r)| r.contains(p))
                .map(|(l, _)| *l)
        });
        let rank = |l: &LayerId| order.iter().position(|(o, _)| o == l).unwrap_or(0);
        c.hover_top = pointer.and_then(|p| {
            c.prev_widgets
                .iter()
                .enumerate()
                .filter(|(_, w)| {
                    w.enabled && w.sense.interactive() && w.rect.intersect(w.clip).contains(p)
                })
                .filter(|(_, w)| c.hover_layer.is_none_or(|l| l == w.layer))
                .max_by_key(|(i, w)| (rank(&w.layer), *i))
                .map(|(_, w)| w.id)
        });

        // Pointer presses decide what is being pressed and what has focus.
        c.memory.focus_prev = c.memory.focus_start;
        if c.input.pointer.pressed[0] {
            c.pressed = c.hover_top;
            let target = c
                .hover_top
                .and_then(|id| c.prev_widgets.iter().find(|w| w.id == id).cloned());
            match target {
                Some(w) if w.sense.focusable => c.memory.focus = Some(w.id),
                _ => c.memory.focus = None,
            }
            if let Some(l) = c.hover_layer {
                if l.order == Order::Middle {
                    c.memory.bring_to_front(l.id);
                }
            }
        }
        if c.input.pointer.released[0] {
            if let Some(id) = c.pressed.filter(|id| c.hover_top == Some(*id)) {
                let double = c
                    .last_click
                    .is_some_and(|(lid, t)| lid == id && time - t < 0.4);
                if double {
                    c.double_click = Some(id);
                    c.last_click = None;
                } else {
                    c.last_click = Some((id, time));
                }
            }
        }

        // Keyboard: Tab moves focus, Enter/Space activates, Escape leaves.
        let mut events = std::mem::take(&mut c.input.events);
        let focus_is_text = c.wants_keyboard;
        events.retain(|e| match e {
            Event::Key {
                key: Key::Tab,
                pressed: true,
                modifiers,
                ..
            } if !modifiers.ctrl && !modifiers.alt => {
                c.memory.pending_tab = Some(if modifiers.shift { -1 } else { 1 });
                false
            }
            Event::Key {
                key: Key::Enter | Key::Space,
                pressed: true,
                modifiers,
                ..
            } if modifiers.is_none() && c.memory.focus.is_some() && !focus_is_text => {
                let f = c.memory.focus;
                let clickable = c
                    .prev_widgets
                    .iter()
                    .any(|w| Some(w.id) == f && w.sense.click && w.enabled);
                if clickable {
                    c.key_click = f;
                    false
                } else {
                    true
                }
            }
            _ => true,
        });
        c.input.events = events;
        if let Some(dir) = c.memory.pending_tab.take() {
            let focusables: Vec<Id> = c
                .prev_widgets
                .iter()
                .filter(|w| w.sense.focusable && w.enabled && c.hover_layer_allows(w))
                .map(|w| w.id)
                .collect();
            if !focusables.is_empty() {
                let cur = c
                    .memory
                    .focus
                    .and_then(|f| focusables.iter().position(|x| *x == f));
                let n = focusables.len() as i32;
                let next = match cur {
                    Some(i) => (i as i32 + dir).rem_euclid(n),
                    None if dir > 0 => 0,
                    None => n - 1,
                };
                c.memory.focus = Some(focusables[next as usize]);
            }
        }
        c.memory.focus_start = c.memory.focus;
        c.wants_keyboard = false;
    }

    /// Areas in drawing order.
    fn area_order(areas: &[(LayerId, Rect)], windows: &[Id]) -> Vec<(LayerId, Rect)> {
        let mut v: Vec<(LayerId, Rect)> = areas.to_vec();
        v.sort_by_key(|(l, _)| {
            let w = if l.order == Order::Middle {
                windows.iter().position(|x| *x == l.id).unwrap_or(0)
            } else {
                0
            };
            (l.order, w)
        });
        v
    }

    pub fn end_frame(&self) -> FullOutput {
        let mut guard = self.lock();
        let c = &mut *guard;
        // A widget that had focus but is gone loses it.
        if let Some(f) = c.memory.focus {
            if !c.widgets.iter().any(|w| w.id == f) && c.frame_nr > 1 {
                c.memory.focus = None;
            }
        }
        if !c.input.pointer.down[0] && !c.input.pointer.released[0] {
            c.pressed = None;
        }
        if c.close_popups {
            c.memory.popup = None;
        }
        c.memory.focus_start = c.memory.focus;
        c.fonts.end_frame();
        let order = Self::area_order(&c.areas, &c.memory.window_order);
        let mut layer_ids: Vec<LayerId> = order.iter().map(|(l, _)| *l).collect();
        // Layers painted without an area (e.g. the focus ring) go on top of their order.
        let mut extra: Vec<LayerId> = c
            .layers
            .keys()
            .filter(|l| !layer_ids.contains(l))
            .copied()
            .collect();
        extra.sort_by_key(|l| (l.order, l.id));
        for l in extra {
            let pos = layer_ids
                .iter()
                .position(|x| x.order > l.order)
                .unwrap_or(layer_ids.len());
            layer_ids.insert(pos, l);
        }
        let mut shapes = Vec::new();
        for l in layer_ids {
            if let Some(v) = c.layers.get_mut(&l) {
                shapes.append(v);
            }
        }
        let now = c.input.time;
        let repaint_after = if self.0.repaint.load(AtomicOrdering::Acquire) {
            Some(Duration::ZERO)
        } else {
            c.repaint_after
                .map(|t| Duration::from_secs_f64((t - now).max(0.0)))
        };
        FullOutput {
            shapes,
            cursor: c.cursor,
            copied_text: std::mem::take(&mut c.copied),
            repaint_after,
            commands: std::mem::take(&mut c.commands),
            widgets: c.widgets.clone(),
            ime_rect: c.ime_rect,
            pixels_per_point: c.native_ppp * c.zoom,
            open_url: c.open_url.take(),
        }
    }

    /// Draw a frame's shapes into a pixel buffer.
    pub fn paint(&self, out: &FullOutput, canvas: &mut super::raster::Canvas, background: Color32) {
        canvas.clear(background);
        let mut c = self.lock();
        super::paint::render(&out.shapes, canvas, &mut c.fonts);
    }

    pub(crate) fn with_fonts<R>(&self, f: impl FnOnce(&mut Fonts) -> R) -> R {
        f(&mut self.lock().fonts)
    }
}

impl Inner {
    pub(crate) fn layers_mut(&mut self) -> &mut HashMap<LayerId, Vec<ClippedShape>> {
        &mut self.layers
    }

    /// While a modal or other window covers the pointer, Tab stays inside it.
    fn hover_layer_allows(&self, w: &WidgetRecord) -> bool {
        let top_window = self.memory.window_order.last().and_then(|id| {
            self.prev_areas
                .iter()
                .find(|(l, _)| l.id == *id && l.order == Order::Middle)
        });
        match top_window {
            Some((l, _))
                if self
                    .memory
                    .data
                    .get_temp::<bool>(l.id.with("modal"))
                    .unwrap_or(false) =>
            {
                w.layer == *l
            }
            _ => true,
        }
    }
}

fn style_ref(s: &TextStyleResolved) -> &TextStyleResolved {
    s
}
