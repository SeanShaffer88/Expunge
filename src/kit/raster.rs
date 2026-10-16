//! Software rasteriser: anti-aliased rounded rectangles, outlines, lines,
//! convex polygons, soft shadows and scaled images into a 32-bit buffer
//! (0x00RRGGBB per pixel, the layout of a Windows DIB section).
//!
//! All coordinates are in pixels. Coverage is analytic (signed distance),
//! so edges stay smooth at any size without supersampling.

use super::geom::{Color32, Pos2, Rect};

/// A borrowed pixel buffer with a clip rectangle.
pub struct Canvas<'a> {
    pub px: &'a mut [u32],
    pub width: usize,
    pub height: usize,
    clip: (i32, i32, i32, i32),
}

#[inline]
fn blend(dst: u32, c: Color32, cov: f32) -> u32 {
    let a = c.a as f32 * cov;
    if a <= 0.5 {
        return dst;
    }
    let a = (a + 0.5) as u32; // 1..=255
    if a >= 255 {
        return (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32;
    }
    let inv = 255 - a;
    let ch = |shift: u32, s: u8| -> u32 {
        let d = (dst >> shift) & 0xFF;
        ((s as u32 * a + d * inv + 127) / 255) << shift
    };
    ch(16, c.r) | ch(8, c.g) | ch(0, c.b)
}

#[inline]
fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Signed distance from `(x, y)` to a rounded rectangle (negative inside).
#[inline]
fn rrect_sd(x: f32, y: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (x - cx).abs() - (hw - r);
    let qy = (y - cy).abs() - (hh - r);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

impl<'a> Canvas<'a> {
    pub fn new(px: &'a mut [u32], width: usize, height: usize) -> Self {
        assert!(px.len() >= width * height);
        Canvas {
            px,
            width,
            height,
            clip: (0, 0, width as i32, height as i32),
        }
    }

    /// Restrict drawing to `r` (pixels), within the buffer.
    pub fn set_clip(&mut self, r: Rect) {
        let x0 = (r.min.x.floor() as i32).clamp(0, self.width as i32);
        let y0 = (r.min.y.floor() as i32).clamp(0, self.height as i32);
        let x1 = (r.max.x.ceil() as i32).clamp(0, self.width as i32);
        let y1 = (r.max.y.ceil() as i32).clamp(0, self.height as i32);
        self.clip = (x0, y0, x1.max(x0), y1.max(y0));
    }

    pub fn clip(&self) -> Rect {
        let (x0, y0, x1, y1) = self.clip;
        Rect::from_min_max(
            Pos2 {
                x: x0 as f32,
                y: y0 as f32,
            },
            Pos2 {
                x: x1 as f32,
                y: y1 as f32,
            },
        )
    }

    pub fn clear(&mut self, c: Color32) {
        let v = (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32;
        self.px[..self.width * self.height].fill(v);
    }

    /// Pixel bounds of `r` grown by `pad`, clipped.
    fn bounds(&self, r: Rect, pad: f32) -> Option<(i32, i32, i32, i32)> {
        let (cx0, cy0, cx1, cy1) = self.clip;
        let x0 = ((r.min.x - pad).floor() as i32).max(cx0);
        let y0 = ((r.min.y - pad).floor() as i32).max(cy0);
        let x1 = ((r.max.x + pad).ceil() as i32).min(cx1);
        let y1 = ((r.max.y + pad).ceil() as i32).min(cy1);
        (x0 < x1 && y0 < y1).then_some((x0, y0, x1, y1))
    }

    #[inline]
    fn put(&mut self, x: i32, y: i32, c: Color32, cov: f32) {
        let i = y as usize * self.width + x as usize;
        self.px[i] = blend(self.px[i], c, cov);
    }

    /// Fill a rounded rectangle.
    pub fn fill_rect(&mut self, r: Rect, radius: f32, c: Color32) {
        if c.a == 0 || !r.is_positive() {
            return;
        }
        let radius = radius.clamp(0.0, r.width().min(r.height()) / 2.0);
        let Some((x0, y0, x1, y1)) = self.bounds(r, 1.0) else {
            return;
        };
        let (cx, cy) = (r.center().x, r.center().y);
        let (hw, hh) = (r.width() / 2.0, r.height() / 2.0);
        // Rows clear of the rounded corners and the top/bottom edges are
        // solid in the middle: blend only their two ends.
        for y in y0..y1 {
            let py = y as f32 + 0.5;
            let solid_row = py - 0.5 >= r.min.y + radius && py + 0.5 <= r.max.y - radius;
            if solid_row {
                let left = r.min.x.ceil() as i32;
                let right = r.max.x.floor() as i32;
                for x in x0..x1 {
                    let px = x as f32 + 0.5;
                    if x >= left && x < right {
                        self.put(x, y, c, 1.0);
                    } else {
                        let cov = clamp01(0.5 - rrect_sd(px, py, cx, cy, hw, hh, radius));
                        if cov > 0.0 {
                            self.put(x, y, c, cov);
                        }
                    }
                }
            } else {
                for x in x0..x1 {
                    let cov = clamp01(0.5 - rrect_sd(x as f32 + 0.5, py, cx, cy, hw, hh, radius));
                    if cov > 0.0 {
                        self.put(x, y, c, cov);
                    }
                }
            }
        }
    }

    /// Outline a rounded rectangle; the stroke is centred on `r`'s edge.
    pub fn stroke_rect(&mut self, r: Rect, radius: f32, width: f32, c: Color32) {
        if c.a == 0 || width <= 0.0 {
            return;
        }
        let half = width / 2.0;
        let radius = radius.clamp(0.0, r.width().min(r.height()) / 2.0);
        let Some((x0, y0, x1, y1)) = self.bounds(r, half + 1.0) else {
            return;
        };
        let (cx, cy) = (r.center().x, r.center().y);
        let (hw, hh) = (r.width() / 2.0, r.height() / 2.0);
        for y in y0..y1 {
            let py = y as f32 + 0.5;
            for x in x0..x1 {
                let d = rrect_sd(x as f32 + 0.5, py, cx, cy, hw, hh, radius).abs();
                let cov = clamp01(half + 0.5 - d);
                if cov > 0.0 {
                    self.put(x, y, c, cov);
                }
            }
        }
    }

    /// A soft shadow: a rounded rectangle whose edge fades over `blur`.
    pub fn shadow(&mut self, r: Rect, radius: f32, blur: f32, c: Color32) {
        if c.a == 0 {
            return;
        }
        let blur = blur.max(1.0);
        let Some((x0, y0, x1, y1)) = self.bounds(r, blur) else {
            return;
        };
        let radius = radius.clamp(0.0, r.width().min(r.height()) / 2.0);
        let (cx, cy) = (r.center().x, r.center().y);
        let (hw, hh) = (r.width() / 2.0, r.height() / 2.0);
        for y in y0..y1 {
            let py = y as f32 + 0.5;
            for x in x0..x1 {
                let d = rrect_sd(x as f32 + 0.5, py, cx, cy, hw, hh, radius);
                // Smoothstep from fully dark at the edge to clear at `blur`.
                let t = clamp01(1.0 - (d + blur / 2.0) / blur);
                let cov = t * t * (3.0 - 2.0 * t);
                if cov > 0.0 {
                    self.put(x, y, c, cov);
                }
            }
        }
    }

    /// A line segment with round caps.
    pub fn line(&mut self, a: Pos2, b: Pos2, width: f32, c: Color32) {
        if c.a == 0 || width <= 0.0 {
            return;
        }
        let half = width / 2.0;
        let bb = Rect::from_min_max(
            Pos2 {
                x: a.x.min(b.x),
                y: a.y.min(b.y),
            },
            Pos2 {
                x: a.x.max(b.x),
                y: a.y.max(b.y),
            },
        );
        let Some((x0, y0, x1, y1)) = self.bounds(bb, half + 1.0) else {
            return;
        };
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        for y in y0..y1 {
            let py = y as f32 + 0.5;
            for x in x0..x1 {
                let px = x as f32 + 0.5;
                let t = clamp01(((px - a.x) * dx + (py - a.y) * dy) / len2);
                let (qx, qy) = (a.x + t * dx - px, a.y + t * dy - py);
                let d = (qx * qx + qy * qy).sqrt();
                let cov = clamp01(half + 0.5 - d);
                if cov > 0.0 {
                    self.put(x, y, c, cov);
                }
            }
        }
    }

    /// Fill a convex polygon (points in either winding order).
    pub fn fill_convex(&mut self, pts: &[Pos2], c: Color32) {
        if pts.len() < 3 || c.a == 0 {
            return;
        }
        let mut bb = Rect::NOTHING;
        for p in pts {
            bb = bb.union(Rect::from_min_max(*p, *p));
        }
        let Some((x0, y0, x1, y1)) = self.bounds(bb, 1.0) else {
            return;
        };
        // Orientation, so "inside" is the same side of every edge.
        let mut area = 0.0;
        for i in 0..pts.len() {
            let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
            area += p.x * q.y - q.x * p.y;
        }
        let sign = if area >= 0.0 { 1.0 } else { -1.0 };
        for y in y0..y1 {
            let py = y as f32 + 0.5;
            for x in x0..x1 {
                let px = x as f32 + 0.5;
                // Distance inside = smallest signed distance to an edge.
                let mut inside = f32::INFINITY;
                for i in 0..pts.len() {
                    let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
                    let (ex, ey) = (q.x - p.x, q.y - p.y);
                    let len = (ex * ex + ey * ey).sqrt().max(1e-6);
                    let d = sign * (ex * (py - p.y) - ey * (px - p.x)) / len;
                    inside = inside.min(d);
                }
                let cov = clamp01(inside + 0.5);
                if cov > 0.0 {
                    self.put(x, y, c, cov);
                }
            }
        }
    }

    pub fn fill_circle(&mut self, center: Pos2, radius: f32, c: Color32) {
        let r = Rect::from_center_size(center, super::geom::vec2(radius * 2.0, radius * 2.0));
        self.fill_rect(r, radius, c);
    }

    pub fn stroke_circle(&mut self, center: Pos2, radius: f32, width: f32, c: Color32) {
        let r = Rect::from_center_size(center, super::geom::vec2(radius * 2.0, radius * 2.0));
        self.stroke_rect(r, radius, width, c);
    }

    /// Draw RGBA pixels (straight alpha) scaled into `dst`, multiplied by
    /// `tint`, with bilinear filtering.
    pub fn image(&mut self, rgba: &[u8], iw: usize, ih: usize, dst: Rect, tint: Color32) {
        if iw == 0 || ih == 0 || !dst.is_positive() {
            return;
        }
        let Some((x0, y0, x1, y1)) = self.bounds(dst, 0.0) else {
            return;
        };
        let sx = iw as f32 / dst.width();
        let sy = ih as f32 / dst.height();
        let sample = |x: usize, y: usize| -> [f32; 4] {
            let i = (y * iw + x) * 4;
            [
                rgba[i] as f32,
                rgba[i + 1] as f32,
                rgba[i + 2] as f32,
                rgba[i + 3] as f32,
            ]
        };
        for y in y0..y1 {
            let fy = ((y as f32 + 0.5 - dst.min.y) * sy - 0.5).clamp(0.0, ih as f32 - 1.0);
            let (iy, ty) = (fy as usize, fy.fract());
            let iy1 = (iy + 1).min(ih - 1);
            for x in x0..x1 {
                let fx = ((x as f32 + 0.5 - dst.min.x) * sx - 0.5).clamp(0.0, iw as f32 - 1.0);
                let (ix, tx) = (fx as usize, fx.fract());
                let ix1 = (ix + 1).min(iw - 1);
                let (a, b, c, d) = (
                    sample(ix, iy),
                    sample(ix1, iy),
                    sample(ix, iy1),
                    sample(ix1, iy1),
                );
                // Interpolate premultiplied, so transparent texels don't bleed.
                let mut acc = [0f32; 4];
                for (s, w) in [
                    (a, (1.0 - tx) * (1.0 - ty)),
                    (b, tx * (1.0 - ty)),
                    (c, (1.0 - tx) * ty),
                    (d, tx * ty),
                ] {
                    let al = s[3] / 255.0;
                    acc[0] += s[0] * al * w;
                    acc[1] += s[1] * al * w;
                    acc[2] += s[2] * al * w;
                    acc[3] += s[3] * w;
                }
                if acc[3] < 0.5 {
                    continue;
                }
                let al = acc[3] / 255.0;
                let col = Color32::from_rgba_unmultiplied(
                    (acc[0] / al * tint.r as f32 / 255.0).min(255.0) as u8,
                    (acc[1] / al * tint.g as f32 / 255.0).min(255.0) as u8,
                    (acc[2] / al * tint.b as f32 / 255.0).min(255.0) as u8,
                    (acc[3] * tint.a as f32 / 255.0) as u8,
                );
                self.put(x, y, col, 1.0);
            }
        }
    }

    /// Read back one pixel (tests and text blending).
    pub fn get(&self, x: usize, y: usize) -> Color32 {
        let v = self.px[y * self.width + x];
        Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    /// Blend a coverage mask (one byte per pixel) at (x, y) in colour `c`.
    pub fn mask(&mut self, mask: &[u8], mw: usize, mh: usize, x: i32, y: i32, c: Color32) {
        let (cx0, cy0, cx1, cy1) = self.clip;
        for my in 0..mh as i32 {
            let py = y + my;
            if py < cy0 || py >= cy1 {
                continue;
            }
            for mx in 0..mw as i32 {
                let px = x + mx;
                if px < cx0 || px >= cx1 {
                    continue;
                }
                let m = mask[(my as usize) * mw + mx as usize];
                if m > 0 {
                    self.put(px, py, c, m as f32 / 255.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::geom::{pos2, vec2};
    use super::*;

    fn canvas(buf: &mut [u32]) -> Canvas<'_> {
        Canvas::new(buf, 40, 40)
    }

    #[test]
    fn fills_with_soft_edges_and_respects_clip() {
        let mut buf = vec![0u32; 1600];
        let mut c = canvas(&mut buf);
        c.fill_rect(
            Rect::from_min_size(pos2(10.0, 10.0), vec2(20.0, 20.0)),
            6.0,
            Color32::WHITE,
        );
        assert_eq!(c.get(20, 20), Color32::WHITE);
        assert_eq!(c.get(5, 5), Color32::BLACK);
        // The very corner pixel lies outside the rounded corner.
        assert_eq!(c.get(10, 10), Color32::BLACK);
        // An edge pixel half covered is grey.
        let mut buf2 = vec![0u32; 1600];
        let mut c2 = canvas(&mut buf2);
        c2.fill_rect(
            Rect::from_min_size(pos2(10.5, 10.0), vec2(20.0, 20.0)),
            0.0,
            Color32::WHITE,
        );
        let g = c2.get(10, 20).r;
        assert!((100..=155).contains(&g), "{g}");
        // Clip.
        let mut buf3 = vec![0u32; 1600];
        let mut c3 = canvas(&mut buf3);
        c3.set_clip(Rect::from_min_size(pos2(0.0, 0.0), vec2(15.0, 40.0)));
        c3.fill_rect(
            Rect::from_min_size(pos2(0.0, 0.0), vec2(40.0, 40.0)),
            0.0,
            Color32::WHITE,
        );
        assert_eq!(c3.get(14, 5), Color32::WHITE);
        assert_eq!(c3.get(15, 5), Color32::BLACK);
    }

    #[test]
    fn blending_lines_and_images() {
        let mut buf = vec![0u32; 1600];
        let mut c = canvas(&mut buf);
        c.fill_rect(
            Rect::from_min_size(pos2(0.0, 0.0), vec2(40.0, 40.0)),
            0.0,
            Color32::from_rgb(0, 0, 200),
        );
        c.fill_rect(
            Rect::from_min_size(pos2(0.0, 0.0), vec2(40.0, 40.0)),
            0.0,
            Color32::from_rgba_unmultiplied(200, 0, 0, 128),
        );
        let p = c.get(5, 5);
        assert!(
            (95..=105).contains(&p.r) && (95..=105).contains(&p.b),
            "{p:?}"
        );
        c.line(pos2(2.0, 20.5), pos2(38.0, 20.5), 1.0, Color32::WHITE);
        assert_eq!(c.get(20, 20), Color32::WHITE);
        let img = [255u8, 255, 0, 255].repeat(4);
        c.image(
            &img,
            2,
            2,
            Rect::from_min_size(pos2(30.0, 30.0), vec2(8.0, 8.0)),
            Color32::WHITE,
        );
        assert_eq!(c.get(34, 34), Color32::from_rgb(255, 255, 0));
        c.fill_convex(
            &[pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(0.0, 10.0)],
            Color32::GREEN,
        );
        assert_eq!(c.get(1, 1), Color32::GREEN);
    }
}
