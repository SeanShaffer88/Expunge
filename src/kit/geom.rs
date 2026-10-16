//! Points, sizes, rectangles, colours and strokes.

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

pub const fn vec2(x: f32, y: f32) -> Vec2 {
    Vec2 { x, y }
}

impl Vec2 {
    pub const ZERO: Vec2 = vec2(0.0, 0.0);
    pub const INFINITY: Vec2 = vec2(f32::INFINITY, f32::INFINITY);
    pub const fn splat(v: f32) -> Vec2 {
        vec2(v, v)
    }
    pub fn max(self, o: Vec2) -> Vec2 {
        vec2(self.x.max(o.x), self.y.max(o.y))
    }
    pub fn min(self, o: Vec2) -> Vec2 {
        vec2(self.x.min(o.x), self.y.min(o.y))
    }
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
    pub fn to_pos2(self) -> Pos2 {
        pos2(self.x, self.y)
    }
}

impl From<[f32; 2]> for Vec2 {
    fn from(v: [f32; 2]) -> Vec2 {
        vec2(v[0], v[1])
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        vec2(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        vec2(self.x - o.x, self.y - o.y)
    }
}
impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        *self = *self + o;
    }
}
impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Vec2) {
        *self = *self - o;
    }
}
impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, k: f32) -> Vec2 {
        vec2(self.x * k, self.y * k)
    }
}
impl Div<f32> for Vec2 {
    type Output = Vec2;
    fn div(self, k: f32) -> Vec2 {
        vec2(self.x / k, self.y / k)
    }
}
impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        vec2(-self.x, -self.y)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pos2 {
    pub x: f32,
    pub y: f32,
}

pub const fn pos2(x: f32, y: f32) -> Pos2 {
    Pos2 { x, y }
}

impl Pos2 {
    pub const ZERO: Pos2 = pos2(0.0, 0.0);
    pub fn to_vec2(self) -> Vec2 {
        vec2(self.x, self.y)
    }
    pub fn distance(self, o: Pos2) -> f32 {
        (self - o).length()
    }
}

impl Add<Vec2> for Pos2 {
    type Output = Pos2;
    fn add(self, o: Vec2) -> Pos2 {
        pos2(self.x + o.x, self.y + o.y)
    }
}
impl Sub<Vec2> for Pos2 {
    type Output = Pos2;
    fn sub(self, o: Vec2) -> Pos2 {
        pos2(self.x - o.x, self.y - o.y)
    }
}
impl Sub for Pos2 {
    type Output = Vec2;
    fn sub(self, o: Pos2) -> Vec2 {
        vec2(self.x - o.x, self.y - o.y)
    }
}
impl AddAssign<Vec2> for Pos2 {
    fn add_assign(&mut self, o: Vec2) {
        *self = *self + o;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub min: Pos2,
    pub max: Pos2,
}

impl Rect {
    pub const NOTHING: Rect = Rect {
        min: pos2(f32::INFINITY, f32::INFINITY),
        max: pos2(f32::NEG_INFINITY, f32::NEG_INFINITY),
    };
    pub const EVERYTHING: Rect = Rect {
        min: pos2(f32::NEG_INFINITY, f32::NEG_INFINITY),
        max: pos2(f32::INFINITY, f32::INFINITY),
    };
    pub fn from_min_max(min: Pos2, max: Pos2) -> Rect {
        Rect { min, max }
    }
    pub fn from_min_size(min: Pos2, size: Vec2) -> Rect {
        Rect {
            min,
            max: min + size,
        }
    }
    pub fn from_center_size(c: Pos2, size: Vec2) -> Rect {
        Rect::from_min_size(c - size / 2.0, size)
    }
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }
    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }
    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }
    pub fn center(&self) -> Pos2 {
        pos2(
            (self.min.x + self.max.x) / 2.0,
            (self.min.y + self.max.y) / 2.0,
        )
    }
    pub fn left(&self) -> f32 {
        self.min.x
    }
    pub fn right(&self) -> f32 {
        self.max.x
    }
    pub fn top(&self) -> f32 {
        self.min.y
    }
    pub fn bottom(&self) -> f32 {
        self.max.y
    }
    pub fn left_top(&self) -> Pos2 {
        self.min
    }
    pub fn right_top(&self) -> Pos2 {
        pos2(self.max.x, self.min.y)
    }
    pub fn left_bottom(&self) -> Pos2 {
        pos2(self.min.x, self.max.y)
    }
    pub fn right_bottom(&self) -> Pos2 {
        self.max
    }
    pub fn left_center(&self) -> Pos2 {
        pos2(self.min.x, self.center().y)
    }
    pub fn right_center(&self) -> Pos2 {
        pos2(self.max.x, self.center().y)
    }
    pub fn center_top(&self) -> Pos2 {
        pos2(self.center().x, self.min.y)
    }
    pub fn center_bottom(&self) -> Pos2 {
        pos2(self.center().x, self.max.y)
    }
    pub fn is_positive(&self) -> bool {
        self.width() > 0.0 && self.height() > 0.0
    }
    pub fn is_finite(&self) -> bool {
        self.min.x.is_finite()
            && self.min.y.is_finite()
            && self.max.x.is_finite()
            && self.max.y.is_finite()
    }
    pub fn contains(&self, p: Pos2) -> bool {
        p.x >= self.min.x && p.x < self.max.x && p.y >= self.min.y && p.y < self.max.y
    }
    pub fn intersects(&self, o: Rect) -> bool {
        self.min.x < o.max.x && o.min.x < self.max.x && self.min.y < o.max.y && o.min.y < self.max.y
    }
    pub fn intersect(&self, o: Rect) -> Rect {
        Rect {
            min: pos2(self.min.x.max(o.min.x), self.min.y.max(o.min.y)),
            max: pos2(self.max.x.min(o.max.x), self.max.y.min(o.max.y)),
        }
    }
    pub fn union(&self, o: Rect) -> Rect {
        Rect {
            min: pos2(self.min.x.min(o.min.x), self.min.y.min(o.min.y)),
            max: pos2(self.max.x.max(o.max.x), self.max.y.max(o.max.y)),
        }
    }
    pub fn expand(&self, d: f32) -> Rect {
        self.expand2(Vec2::splat(d))
    }
    pub fn expand2(&self, d: Vec2) -> Rect {
        Rect {
            min: self.min - d,
            max: self.max + d,
        }
    }
    pub fn shrink(&self, d: f32) -> Rect {
        self.expand(-d)
    }
    pub fn shrink2(&self, d: Vec2) -> Rect {
        self.expand2(-d)
    }
    pub fn translate(&self, d: Vec2) -> Rect {
        Rect {
            min: self.min + d,
            max: self.max + d,
        }
    }
    pub fn with_min_x(mut self, v: f32) -> Rect {
        self.min.x = v;
        self
    }
    pub fn with_max_x(mut self, v: f32) -> Rect {
        self.max.x = v;
        self
    }
    pub fn with_min_y(mut self, v: f32) -> Rect {
        self.min.y = v;
        self
    }
    pub fn with_max_y(mut self, v: f32) -> Rect {
        self.max.y = v;
        self
    }
    pub fn set_width(&mut self, w: f32) {
        self.max.x = self.min.x + w;
    }
    pub fn set_height(&mut self, h: f32) {
        self.max.y = self.min.y + h;
    }
    pub fn x_range(&self) -> std::ops::RangeInclusive<f32> {
        self.min.x..=self.max.x
    }
    pub fn y_range(&self) -> std::ops::RangeInclusive<f32> {
        self.min.y..=self.max.y
    }
}

/// sRGB colour with straight (not premultiplied) alpha.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Color32 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color32 {
    pub const TRANSPARENT: Color32 = Color32::from_rgba_unmultiplied(0, 0, 0, 0);
    pub const BLACK: Color32 = Color32::from_rgb(0, 0, 0);
    pub const WHITE: Color32 = Color32::from_rgb(255, 255, 255);
    pub const GRAY: Color32 = Color32::from_rgb(160, 160, 160);
    pub const LIGHT_GRAY: Color32 = Color32::from_rgb(220, 220, 220);
    pub const DARK_GRAY: Color32 = Color32::from_rgb(96, 96, 96);
    pub const RED: Color32 = Color32::from_rgb(255, 0, 0);
    pub const GREEN: Color32 = Color32::from_rgb(0, 255, 0);
    pub const YELLOW: Color32 = Color32::from_rgb(255, 255, 0);
    pub const LIGHT_RED: Color32 = Color32::from_rgb(255, 128, 128);

    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Color32 {
        Color32 { r, g, b, a: 255 }
    }
    pub const fn from_rgba_unmultiplied(r: u8, g: u8, b: u8, a: u8) -> Color32 {
        Color32 { r, g, b, a }
    }
    /// Like egui: the arguments are premultiplied; stored straight.
    pub fn from_rgba_premultiplied(r: u8, g: u8, b: u8, a: u8) -> Color32 {
        if a == 0 {
            return Color32::TRANSPARENT;
        }
        let un = |c: u8| ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
        Color32 {
            r: un(r),
            g: un(g),
            b: un(b),
            a,
        }
    }
    pub const fn from_black_alpha(a: u8) -> Color32 {
        Color32 {
            r: 0,
            g: 0,
            b: 0,
            a,
        }
    }
    pub const fn from_white_alpha(a: u8) -> Color32 {
        Color32 {
            r: 255,
            g: 255,
            b: 255,
            a,
        }
    }
    pub const fn from_gray(v: u8) -> Color32 {
        Color32::from_rgb(v, v, v)
    }
    pub fn r(&self) -> u8 {
        self.r
    }
    pub fn g(&self) -> u8 {
        self.g
    }
    pub fn b(&self) -> u8 {
        self.b
    }
    pub fn a(&self) -> u8 {
        self.a
    }
    /// Fade toward transparent (egui's meaning: multiply all channels in
    /// premultiplied space, i.e. scale alpha).
    pub fn gamma_multiply(self, k: f32) -> Color32 {
        let k = k.clamp(0.0, 1.0);
        Color32 {
            a: (self.a as f32 * k + 0.5) as u8,
            ..self
        }
    }
    pub fn linear_multiply(self, k: f32) -> Color32 {
        self.gamma_multiply(k)
    }
    pub fn additive(self) -> Color32 {
        self
    }
    pub fn lerp_to(self, o: Color32, t: f32) -> Color32 {
        let t = t.clamp(0.0, 1.0);
        let l = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t + 0.5) as u8;
        Color32 {
            r: l(self.r, o.r),
            g: l(self.g, o.g),
            b: l(self.b, o.b),
            a: l(self.a, o.a),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stroke {
    pub width: f32,
    pub color: Color32,
}

impl Stroke {
    pub const NONE: Stroke = Stroke {
        width: 0.0,
        color: Color32::TRANSPARENT,
    };
    pub fn new(width: impl Into<f32>, color: impl Into<Color32>) -> Stroke {
        Stroke {
            width: width.into(),
            color: color.into(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.color.a == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeKind {
    Inside,
    Middle,
    Outside,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CornerRadius {
    pub nw: u8,
    pub ne: u8,
    pub sw: u8,
    pub se: u8,
}

impl CornerRadius {
    pub const ZERO: CornerRadius = CornerRadius::same(0);
    pub const fn same(r: u8) -> CornerRadius {
        CornerRadius {
            nw: r,
            ne: r,
            sw: r,
            se: r,
        }
    }
    /// The largest corner (the rasteriser uses one radius per rectangle).
    pub fn max(&self) -> f32 {
        self.nw.max(self.ne).max(self.sw).max(self.se) as f32
    }
}

impl From<u8> for CornerRadius {
    fn from(r: u8) -> Self {
        CornerRadius::same(r)
    }
}
impl From<f32> for CornerRadius {
    fn from(r: f32) -> Self {
        CornerRadius::same(r.clamp(0.0, 255.0) as u8)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Margin {
    pub left: i8,
    pub right: i8,
    pub top: i8,
    pub bottom: i8,
}

impl Margin {
    pub const ZERO: Margin = Margin::same(0);
    pub const fn same(v: i8) -> Margin {
        Margin {
            left: v,
            right: v,
            top: v,
            bottom: v,
        }
    }
    pub const fn symmetric(x: i8, y: i8) -> Margin {
        Margin {
            left: x,
            right: x,
            top: y,
            bottom: y,
        }
    }
    pub fn sum(&self) -> Vec2 {
        vec2(
            (self.left as i16 + self.right as i16) as f32,
            (self.top as i16 + self.bottom as i16) as f32,
        )
    }
    pub fn left_top(&self) -> Vec2 {
        vec2(self.left as f32, self.top as f32)
    }
    pub fn right_bottom(&self) -> Vec2 {
        vec2(self.right as f32, self.bottom as f32)
    }
}

impl From<i8> for Margin {
    fn from(v: i8) -> Self {
        Margin::same(v)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shadow {
    pub offset: [i8; 2],
    pub blur: u8,
    pub spread: u8,
    pub color: Color32,
}

impl Shadow {
    pub const NONE: Shadow = Shadow {
        offset: [0, 0],
        blur: 0,
        spread: 0,
        color: Color32::TRANSPARENT,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Min,
    Center,
    Max,
}

impl Align {
    pub const LEFT: Align = Align::Min;
    pub const TOP: Align = Align::Min;
    pub const RIGHT: Align = Align::Max;
    pub const BOTTOM: Align = Align::Max;
    /// Position of a span of `size` inside [min, max].
    pub fn place(self, min: f32, max: f32, size: f32) -> f32 {
        match self {
            Align::Min => min,
            Align::Center => (min + max - size) / 2.0,
            Align::Max => max - size,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Align2(pub [Align; 2]);

impl Align2 {
    pub const LEFT_TOP: Align2 = Align2([Align::Min, Align::Min]);
    pub const LEFT_CENTER: Align2 = Align2([Align::Min, Align::Center]);
    pub const LEFT_BOTTOM: Align2 = Align2([Align::Min, Align::Max]);
    pub const CENTER_TOP: Align2 = Align2([Align::Center, Align::Min]);
    pub const CENTER_CENTER: Align2 = Align2([Align::Center, Align::Center]);
    pub const CENTER_BOTTOM: Align2 = Align2([Align::Center, Align::Max]);
    pub const RIGHT_TOP: Align2 = Align2([Align::Max, Align::Min]);
    pub const RIGHT_CENTER: Align2 = Align2([Align::Max, Align::Center]);
    pub const RIGHT_BOTTOM: Align2 = Align2([Align::Max, Align::Max]);

    /// The rectangle of `size` anchored at `pos` with this alignment.
    pub fn anchor_size(self, pos: Pos2, size: Vec2) -> Rect {
        let x = match self.0[0] {
            Align::Min => pos.x,
            Align::Center => pos.x - size.x / 2.0,
            Align::Max => pos.x - size.x,
        };
        let y = match self.0[1] {
            Align::Min => pos.y,
            Align::Center => pos.y - size.y / 2.0,
            Align::Max => pos.y - size.y,
        };
        Rect::from_min_size(pos2(x, y), size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_basics() {
        let r = Rect::from_min_size(pos2(10.0, 20.0), vec2(30.0, 40.0));
        assert_eq!(r.center(), pos2(25.0, 40.0));
        assert!(r.contains(pos2(10.0, 20.0)) && !r.contains(pos2(40.0, 20.0)));
        assert_eq!(r.shrink(5.0).size(), vec2(20.0, 30.0));
        assert_eq!(
            Align2::RIGHT_CENTER
                .anchor_size(pos2(100.0, 50.0), vec2(20.0, 10.0))
                .min,
            pos2(80.0, 45.0)
        );
        assert_eq!(Color32::from_rgb(10, 20, 30).gamma_multiply(0.5).a, 128);
    }
}
