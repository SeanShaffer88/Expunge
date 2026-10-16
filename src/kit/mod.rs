//! Expunge's own immediate-mode interface toolkit.
//!
//! The interface is described afresh every frame (`ui.button("OK")` both
//! draws the button and reports whether it was clicked), much like egui,
//! whose API it follows so pages read the same. Everything is drawn by the
//! software rasteriser in [`raster`]; text goes through a [`TextBackend`],
//! Windows' own renderer in the application.
//!
//! Frames are only produced when something happened (input, a timer a
//! widget asked for, a background job finishing), so an idle window uses
//! no processor time.

pub mod containers;
pub mod context;
pub mod geom;
pub mod paint;
pub mod raster;
pub mod style;
pub mod text;
pub mod ui;
pub mod widgets;

pub use containers::*;
pub use context::{
    Context, FullOutput, IdTypeMap, InputState, Memory, PointerState, ViewportCommand,
};
pub use geom::*;
pub use paint::{render, Painter, Shape};
pub use style::*;
pub use text::{FontFamily, FontId, Galley, MockText, TextBackend};
pub use ui::{Direction, InnerResponse, Layout, Response, Ui, UiBuilder, Widget};
pub use widgets::*;

use std::hash::{Hash, Hasher};

/// A stable identity for a widget or area, derived from a hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Id(pub u64);

impl Id {
    pub const NULL: Id = Id(0);

    fn hash_of(v: impl Hash, seed: u64) -> Id {
        // FNV-style mixing over a DefaultHasher keeps ids stable within a run.
        let mut h = std::collections::hash_map::DefaultHasher::new();
        seed.hash(&mut h);
        v.hash(&mut h);
        Id(h.finish() | 1)
    }

    pub fn new(v: impl Hash) -> Id {
        Id::hash_of(v, 0x9E37_79B9_7F4A_7C15)
    }

    pub fn with(self, v: impl Hash) -> Id {
        Id::hash_of(v, self.0)
    }

    pub fn value(&self) -> u64 {
        self.0
    }
}

/// What a widget reacts to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Sense {
    pub click: bool,
    pub drag: bool,
    pub focusable: bool,
}

impl Sense {
    pub const fn hover() -> Sense {
        Sense {
            click: false,
            drag: false,
            focusable: false,
        }
    }
    pub const fn click() -> Sense {
        Sense {
            click: true,
            drag: false,
            focusable: true,
        }
    }
    pub const fn drag() -> Sense {
        Sense {
            click: false,
            drag: true,
            focusable: true,
        }
    }
    pub const fn click_and_drag() -> Sense {
        Sense {
            click: true,
            drag: true,
            focusable: true,
        }
    }
    pub const fn focusable_noninteractive() -> Sense {
        Sense {
            click: false,
            drag: false,
            focusable: true,
        }
    }
    pub fn interactive(&self) -> bool {
        self.click || self.drag
    }
}

impl std::ops::BitOr for Sense {
    type Output = Sense;
    fn bitor(self, o: Sense) -> Sense {
        Sense {
            click: self.click || o.click,
            drag: self.drag || o.drag,
            focusable: self.focusable || o.focusable,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorIcon {
    #[default]
    Default,
    PointingHand,
    Text,
    ResizeHorizontal,
    ResizeVertical,
    Wait,
    Progress,
    NotAllowed,
    Grab,
    Grabbing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    Escape,
    Tab,
    Backspace,
    Enter,
    Space,
    Insert,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Plus,
    Minus,
    Equals,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub struct Modifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    /// Same as ctrl on Windows.
    pub command: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        alt: false,
        ctrl: false,
        shift: false,
        command: false,
    };
    pub const CTRL: Modifiers = Modifiers {
        alt: false,
        ctrl: true,
        shift: false,
        command: true,
    };
    pub const SHIFT: Modifiers = Modifiers {
        alt: false,
        ctrl: false,
        shift: true,
        command: false,
    };
    pub const ALT: Modifiers = Modifiers {
        alt: true,
        ctrl: false,
        shift: false,
        command: false,
    };
    /// Exactly these modifiers are held (command counts as ctrl).
    pub fn matches_exact(&self, o: Modifiers) -> bool {
        self.alt == o.alt && self.ctrl == o.ctrl && self.shift == o.shift
    }
    pub fn is_none(&self) -> bool {
        !self.alt && !self.ctrl && !self.shift
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Key {
        key: Key,
        physical_key: Option<Key>,
        pressed: bool,
        repeat: bool,
        modifiers: Modifiers,
    },
    Text(String),
    PointerMoved(Pos2),
    PointerButton {
        pos: Pos2,
        button: PointerButton,
        pressed: bool,
        modifiers: Modifiers,
    },
    PointerGone,
    /// Wheel movement in points (positive y scrolls content down / view up).
    MouseWheel {
        delta: Vec2,
        modifiers: Modifiers,
    },
    Copy,
    Cut,
    Paste(String),
    WindowFocused(bool),
}

/// Input for one frame, from the platform.
#[derive(Clone, Debug, Default)]
pub struct RawInput {
    pub events: Vec<Event>,
    /// The window's client area in points.
    pub screen_rect: Option<Rect>,
    /// Seconds since the program started.
    pub time: Option<f64>,
    pub modifiers: Modifiers,
    /// Display scale (pixels per point before zoom).
    pub pixels_per_point: Option<f32>,
}

/// Straight-alpha RGBA pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColorImage {
    pub size: [usize; 2],
    pub pixels: Vec<Color32>,
}

impl ColorImage {
    pub fn new(size: [usize; 2], pixels: Vec<Color32>) -> ColorImage {
        ColorImage { size, pixels }
    }
    pub fn from_rgba_unmultiplied(size: [usize; 2], rgba: &[u8]) -> ColorImage {
        let pixels = rgba
            .chunks_exact(4)
            .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
            .collect();
        ColorImage { size, pixels }
    }
    pub fn width(&self) -> usize {
        self.size[0]
    }
    pub fn height(&self) -> usize {
        self.size[1]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextureOptions;

impl TextureOptions {
    pub const LINEAR: TextureOptions = TextureOptions;
    pub const NEAREST: TextureOptions = TextureOptions;
}

/// An image loaded once and drawn many times.
#[derive(Clone, Debug)]
pub struct TextureHandle {
    pub(crate) rgba: std::sync::Arc<Vec<u8>>,
    pub(crate) size: [usize; 2],
    pub(crate) name: String,
}

impl TextureHandle {
    pub fn size(&self) -> [usize; 2] {
        self.size
    }
    pub fn size_vec2(&self) -> Vec2 {
        vec2(self.size[0] as f32, self.size[1] as f32)
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn id(&self) -> u64 {
        std::sync::Arc::as_ptr(&self.rgba) as u64
    }
}

/// Window icon pixels.
#[derive(Clone, Debug, Default)]
pub struct IconData {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Drawing order of areas: background panels, then windows, then menus
/// and the focus ring, then tooltips.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Order {
    Background,
    Middle,
    Foreground,
    Tooltip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerId {
    pub order: Order,
    pub id: Id,
}

impl LayerId {
    pub fn new(order: Order, id: Id) -> LayerId {
        LayerId { order, id }
    }
    pub fn background() -> LayerId {
        LayerId::new(Order::Background, Id::new("background"))
    }
}

/// Accessibility description of a widget (for UI Automation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetType {
    Label,
    Button,
    Checkbox,
    RadioButton,
    SelectableLabel,
    TextEdit,
    ComboBox,
    Slider,
    Link,
    Image,
    CollapsingHeader,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WidgetInfo {
    pub typ: WidgetType,
    pub enabled: bool,
    pub label: Option<String>,
    pub selected: Option<bool>,
    pub value: Option<String>,
}

impl WidgetInfo {
    pub fn labeled(typ: WidgetType, enabled: bool, label: impl ToString) -> WidgetInfo {
        WidgetInfo {
            typ,
            enabled,
            label: Some(label.to_string()),
            selected: None,
            value: None,
        }
    }
    pub fn selected(
        typ: WidgetType,
        enabled: bool,
        selected: bool,
        label: impl ToString,
    ) -> WidgetInfo {
        WidgetInfo {
            selected: Some(selected),
            ..WidgetInfo::labeled(typ, enabled, label)
        }
    }
}

/// Interpolate between `a` and `b`.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Like `std::ops::RangeInclusive` remapping in egui.
pub fn remap_clamp(
    x: f32,
    from: std::ops::RangeInclusive<f32>,
    to: std::ops::RangeInclusive<f32>,
) -> f32 {
    let t = ((x - from.start()) / (from.end() - from.start())).clamp(0.0, 1.0);
    lerp(*to.start(), *to.end(), t)
}
