//! Shapes, the painter that records them, and drawing them to pixels.

use super::context::Context;
use super::geom::*;
use super::raster::Canvas;
use super::text::{Fonts, Galley, TextStyleResolved};
use super::{FontId, LayerId, TextureHandle};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum Shape {
    Rect {
        rect: Rect,
        radius: CornerRadius,
        fill: Color32,
        stroke: Stroke,
        kind: StrokeKind,
    },
    Shadow {
        rect: Rect,
        radius: CornerRadius,
        shadow: Shadow,
    },
    Circle {
        center: Pos2,
        radius: f32,
        fill: Color32,
        stroke: Stroke,
    },
    LineSegment {
        points: [Pos2; 2],
        stroke: Stroke,
    },
    Path {
        points: Vec<Pos2>,
        closed: bool,
        fill: Color32,
        stroke: Stroke,
    },
    Text {
        pos: Pos2,
        galley: Arc<Galley>,
        color: Color32,
    },
    Image {
        rect: Rect,
        texture: TextureHandle,
        tint: Color32,
        radius: CornerRadius,
    },
    Noop,
}

impl Shape {
    pub fn line(points: Vec<Pos2>, stroke: impl Into<Stroke>) -> Shape {
        Shape::Path {
            points,
            closed: false,
            fill: Color32::TRANSPARENT,
            stroke: stroke.into(),
        }
    }
    pub fn line_segment(points: [Pos2; 2], stroke: impl Into<Stroke>) -> Shape {
        Shape::LineSegment {
            points,
            stroke: stroke.into(),
        }
    }
    pub fn convex_polygon(
        points: Vec<Pos2>,
        fill: impl Into<Color32>,
        stroke: impl Into<Stroke>,
    ) -> Shape {
        Shape::Path {
            points,
            closed: true,
            fill: fill.into(),
            stroke: stroke.into(),
        }
    }
}

/// A shape with the clip rectangle it was drawn under.
#[derive(Clone, Debug)]
pub struct ClippedShape {
    pub clip: Rect,
    pub shape: Shape,
}

/// Records shapes into one layer of the current frame.
#[derive(Clone)]
pub struct Painter {
    ctx: Context,
    layer: LayerId,
    clip: Rect,
    fade: Option<f32>,
}

impl Painter {
    pub fn new(ctx: Context, layer: LayerId, clip: Rect) -> Painter {
        Painter {
            ctx,
            layer,
            clip,
            fade: None,
        }
    }
    pub fn ctx(&self) -> &Context {
        &self.ctx
    }
    pub fn layer_id(&self) -> LayerId {
        self.layer
    }
    pub fn clip_rect(&self) -> Rect {
        self.clip
    }
    pub fn set_clip_rect(&mut self, r: Rect) {
        self.clip = r;
    }
    pub fn with_clip_rect(&self, r: Rect) -> Painter {
        let mut p = self.clone();
        p.clip = r.intersect(self.clip);
        p
    }
    /// Fade everything this painter draws (0 = invisible).
    pub fn set_opacity(&mut self, a: f32) {
        self.fade = Some(a.clamp(0.0, 1.0));
    }

    fn faded(&self, c: Color32) -> Color32 {
        match self.fade {
            Some(a) => c.gamma_multiply(a),
            None => c,
        }
    }

    pub fn add(&self, shape: Shape) {
        let shape = match (self.fade, shape) {
            (None, s) => s,
            (
                Some(_),
                Shape::Rect {
                    rect,
                    radius,
                    fill,
                    stroke,
                    kind,
                },
            ) => Shape::Rect {
                rect,
                radius,
                fill: self.faded(fill),
                stroke: Stroke::new(stroke.width, self.faded(stroke.color)),
                kind,
            },
            (Some(_), Shape::Text { pos, galley, color }) => Shape::Text {
                pos,
                galley,
                color: self.faded(color),
            },
            (
                Some(_),
                Shape::Circle {
                    center,
                    radius,
                    fill,
                    stroke,
                },
            ) => Shape::Circle {
                center,
                radius,
                fill: self.faded(fill),
                stroke: Stroke::new(stroke.width, self.faded(stroke.color)),
            },
            (Some(_), Shape::LineSegment { points, stroke }) => Shape::LineSegment {
                points,
                stroke: Stroke::new(stroke.width, self.faded(stroke.color)),
            },
            (
                Some(_),
                Shape::Image {
                    rect,
                    texture,
                    tint,
                    radius,
                },
            ) => Shape::Image {
                rect,
                texture,
                tint: self.faded(tint),
                radius,
            },
            (Some(_), s) => s,
        };
        self.ctx.add_shape(self.layer, self.clip, shape);
    }

    pub fn extend(&self, shapes: impl IntoIterator<Item = Shape>) {
        for s in shapes {
            self.add(s);
        }
    }

    pub fn rect(
        &self,
        rect: Rect,
        radius: impl Into<CornerRadius>,
        fill: impl Into<Color32>,
        stroke: impl Into<Stroke>,
        kind: StrokeKind,
    ) {
        self.add(Shape::Rect {
            rect,
            radius: radius.into(),
            fill: fill.into(),
            stroke: stroke.into(),
            kind,
        });
    }

    pub fn rect_filled(
        &self,
        rect: Rect,
        radius: impl Into<CornerRadius>,
        fill: impl Into<Color32>,
    ) {
        self.rect(rect, radius, fill, Stroke::NONE, StrokeKind::Inside);
    }

    pub fn rect_stroke(
        &self,
        rect: Rect,
        radius: impl Into<CornerRadius>,
        stroke: impl Into<Stroke>,
        kind: StrokeKind,
    ) {
        self.rect(rect, radius, Color32::TRANSPARENT, stroke, kind);
    }

    pub fn circle_filled(&self, center: Pos2, radius: f32, fill: impl Into<Color32>) {
        self.add(Shape::Circle {
            center,
            radius,
            fill: fill.into(),
            stroke: Stroke::NONE,
        });
    }

    pub fn circle_stroke(&self, center: Pos2, radius: f32, stroke: impl Into<Stroke>) {
        self.add(Shape::Circle {
            center,
            radius,
            fill: Color32::TRANSPARENT,
            stroke: stroke.into(),
        });
    }

    pub fn circle(
        &self,
        center: Pos2,
        radius: f32,
        fill: impl Into<Color32>,
        stroke: impl Into<Stroke>,
    ) {
        self.add(Shape::Circle {
            center,
            radius,
            fill: fill.into(),
            stroke: stroke.into(),
        });
    }

    pub fn line_segment(&self, points: [Pos2; 2], stroke: impl Into<Stroke>) {
        self.add(Shape::LineSegment {
            points,
            stroke: stroke.into(),
        });
    }

    pub fn hline(&self, x: std::ops::RangeInclusive<f32>, y: f32, stroke: impl Into<Stroke>) {
        self.line_segment([pos2(*x.start(), y), pos2(*x.end(), y)], stroke);
    }

    pub fn vline(&self, x: f32, y: std::ops::RangeInclusive<f32>, stroke: impl Into<Stroke>) {
        self.line_segment([pos2(x, *y.start()), pos2(x, *y.end())], stroke);
    }

    pub fn image(&self, texture: &TextureHandle, rect: Rect, tint: Color32) {
        self.add(Shape::Image {
            rect,
            texture: texture.clone(),
            tint,
            radius: CornerRadius::ZERO,
        });
    }

    /// Lay out text on one line.
    pub fn layout_no_wrap(&self, text: String, font: FontId, _color: Color32) -> Arc<Galley> {
        self.ctx.layout(&text, style_of(font), f32::INFINITY)
    }

    pub fn layout(
        &self,
        text: String,
        font: FontId,
        _color: Color32,
        wrap_width: f32,
    ) -> Arc<Galley> {
        self.ctx.layout(&text, style_of(font), wrap_width)
    }

    pub fn galley(&self, pos: Pos2, galley: Arc<Galley>, color: Color32) {
        self.add(Shape::Text { pos, galley, color });
    }

    /// Draw text anchored at `pos`; returns where it went.
    pub fn text(
        &self,
        pos: Pos2,
        anchor: Align2,
        text: impl ToString,
        font: FontId,
        color: Color32,
    ) -> Rect {
        let g = self.layout_no_wrap(text.to_string(), font, color);
        let rect = anchor.anchor_size(pos, g.size());
        self.galley(rect.min, g, color);
        rect
    }
}

fn style_of(font: FontId) -> TextStyleResolved {
    TextStyleResolved {
        size: font.size,
        family: font.family,
        ..Default::default()
    }
}

/// Draw shapes (points) into `canvas` (pixels) at `ppp` pixels per point.
pub fn render(shapes: &[ClippedShape], canvas: &mut Canvas, fonts: &mut Fonts) {
    let ppp = fonts.ppp;
    let s = |r: Rect| {
        Rect::from_min_max(
            pos2(r.min.x * ppp, r.min.y * ppp),
            pos2(r.max.x * ppp, r.max.y * ppp),
        )
    };
    let p = |q: Pos2| pos2(q.x * ppp, q.y * ppp);
    let full = Rect::from_min_size(Pos2::ZERO, vec2(canvas.width as f32, canvas.height as f32));
    for cs in shapes {
        let clip = if cs.clip.is_finite() {
            s(cs.clip).intersect(full)
        } else {
            full
        };
        if !clip.is_positive() {
            continue;
        }
        canvas.set_clip(clip);
        match &cs.shape {
            Shape::Rect {
                rect,
                radius,
                fill,
                stroke,
                kind,
            } => {
                let r = s(*rect);
                let rad = radius.max() * ppp;
                canvas.fill_rect(r, rad, *fill);
                if !stroke.is_empty() {
                    let w = (stroke.width * ppp).max(1.0);
                    // Centre the stroke so its pixels line up with the edge.
                    let (r2, rad2) = match kind {
                        StrokeKind::Inside => (r.shrink(w / 2.0), (rad - w / 2.0).max(0.0)),
                        StrokeKind::Middle => (r, rad),
                        StrokeKind::Outside => (r.expand(w / 2.0), rad + w / 2.0),
                    };
                    canvas.stroke_rect(r2, rad2, w, stroke.color);
                }
            }
            Shape::Shadow {
                rect,
                radius,
                shadow,
            } => {
                let off = vec2(shadow.offset[0] as f32, shadow.offset[1] as f32);
                let r = s(rect.translate(off).expand(shadow.spread as f32));
                canvas.shadow(
                    r,
                    radius.max() * ppp,
                    shadow.blur as f32 * ppp,
                    shadow.color,
                );
            }
            Shape::Circle {
                center,
                radius,
                fill,
                stroke,
            } => {
                canvas.fill_circle(p(*center), radius * ppp, *fill);
                if !stroke.is_empty() {
                    canvas.stroke_circle(
                        p(*center),
                        radius * ppp,
                        (stroke.width * ppp).max(1.0),
                        stroke.color,
                    );
                }
            }
            Shape::LineSegment { points, stroke } => {
                let (mut a, mut b) = (p(points[0]), p(points[1]));
                let w = (stroke.width * ppp).max(1.0);
                // Crisp one-pixel horizontal and vertical lines.
                if (w - 1.0).abs() < 0.01 {
                    if a.y == b.y {
                        a.y = a.y.floor() + 0.5;
                        b.y = a.y;
                    } else if a.x == b.x {
                        a.x = a.x.floor() + 0.5;
                        b.x = a.x;
                    }
                }
                canvas.line(a, b, w, stroke.color);
            }
            Shape::Path {
                points,
                closed,
                fill,
                stroke,
            } => {
                let pts: Vec<Pos2> = points.iter().map(|q| p(*q)).collect();
                if *closed && fill.a > 0 {
                    canvas.fill_convex(&pts, *fill);
                }
                if !stroke.is_empty() {
                    let w = (stroke.width * ppp).max(1.0);
                    for win in pts.windows(2) {
                        canvas.line(win[0], win[1], w, stroke.color);
                    }
                    if *closed && pts.len() > 2 {
                        canvas.line(pts[pts.len() - 1], pts[0], w, stroke.color);
                    }
                }
            }
            Shape::Text { pos, galley, color } => {
                fonts.paint(canvas, galley, *pos, *color);
            }
            Shape::Image {
                rect,
                texture,
                tint,
                ..
            } => {
                canvas.image(
                    &texture.rgba,
                    texture.size[0],
                    texture.size[1],
                    s(*rect),
                    *tint,
                );
            }
            Shape::Noop => {}
        }
    }
}
