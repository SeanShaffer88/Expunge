//! Text: fonts, styled strings and line layout.
//!
//! Glyphs are measured and drawn by a [`TextBackend`] (Windows' own text
//! renderer in the app; a fixed-width stand-in in tests). Layout — line
//! breaking, alignment, caret positions — happens here, in points.

use super::geom::{pos2, vec2, Color32, Pos2, Rect, Vec2};
use super::raster::Canvas;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FontFamily {
    #[default]
    Proportional,
    Monospace,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontId {
    pub size: f32,
    pub family: FontFamily,
}

impl FontId {
    pub const fn new(size: f32, family: FontFamily) -> FontId {
        FontId { size, family }
    }
    pub const fn proportional(size: f32) -> FontId {
        FontId::new(size, FontFamily::Proportional)
    }
    pub const fn monospace(size: f32) -> FontId {
        FontId::new(size, FontFamily::Monospace)
    }
}

impl Default for FontId {
    fn default() -> Self {
        FontId::proportional(14.0)
    }
}

/// A font as the backend sees it: size in pixels and weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontKey {
    /// Size in 1/64 pixel, so the key can be hashed.
    pub size64: u32,
    pub family: FontFamily,
    pub bold: bool,
    pub italic: bool,
}

impl FontKey {
    pub fn new(px: f32, family: FontFamily, bold: bool, italic: bool) -> FontKey {
        FontKey {
            size64: (px.max(1.0) * 64.0).round() as u32,
            family,
            bold,
            italic,
        }
    }
    pub fn px(&self) -> f32 {
        self.size64 as f32 / 64.0
    }
}

/// Measures and draws text in pixels.
pub trait TextBackend: Send {
    /// Pixel x after each character of `text` (one entry per `char`).
    fn advances(&mut self, text: &str, font: FontKey, out: &mut Vec<f32>);
    /// (ascent, line height) in pixels.
    fn metrics(&mut self, font: FontKey) -> (f32, f32);
    /// Draw one line with its top-left at `pos` (pixels).
    fn draw(&mut self, canvas: &mut Canvas, text: &str, font: FontKey, pos: Pos2, color: Color32);
}

/// Fixed-width stand-in: Latin 0.55 em, wide (CJK) characters 1 em; draws
/// each glyph as a thin box so layouts can be checked in tests.
#[derive(Default)]
pub struct MockText;

pub fn is_wide(c: char) -> bool {
    matches!(c as u32, 0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6)
}

impl TextBackend for MockText {
    fn advances(&mut self, text: &str, font: FontKey, out: &mut Vec<f32>) {
        let em = font.px();
        let mut x = 0.0;
        for c in text.chars() {
            x += if is_wide(c) { em } else { (em * 0.55).round() };
            out.push(x);
        }
    }
    fn metrics(&mut self, font: FontKey) -> (f32, f32) {
        let em = font.px();
        ((em * 0.95).round(), (em * 1.3).round())
    }
    fn draw(&mut self, canvas: &mut Canvas, text: &str, font: FontKey, pos: Pos2, color: Color32) {
        let mut adv = Vec::new();
        self.advances(text, font, &mut adv);
        let (ascent, _) = self.metrics(font);
        let mut x0 = pos.x;
        for (c, x1) in text.chars().zip(adv) {
            if !c.is_whitespace() {
                let r = Rect::from_min_max(
                    pos2(x0 + 1.0, pos.y + ascent * 0.3),
                    pos2(pos.x + x1 - 1.0, pos.y + ascent),
                );
                canvas.fill_rect(r, 0.0, color);
            }
            x0 = pos.x + x1;
        }
    }
}

/// A measured string: its text, the advances and the frame it was last used.
type Measured = (Arc<str>, Arc<[f32]>, u64);

/// Measurements reused between frames: a string measured in one frame is
/// usually measured again in the next.
pub struct Fonts {
    backend: Box<dyn TextBackend>,
    /// Pixels per point (display scale × zoom).
    pub ppp: f32,
    cache: HashMap<(FontKey, u64), Measured>,
    metrics: HashMap<FontKey, (f32, f32)>,
    generation: u64,
    scratch: Vec<f32>,
}

impl Fonts {
    pub fn new(backend: Box<dyn TextBackend>) -> Fonts {
        Fonts {
            backend,
            ppp: 1.0,
            cache: HashMap::new(),
            metrics: HashMap::new(),
            generation: 0,
            scratch: Vec::new(),
        }
    }

    pub fn backend(&mut self) -> &mut dyn TextBackend {
        self.backend.as_mut()
    }

    /// Call once per frame: drops measurements unused for a few frames.
    pub fn end_frame(&mut self) {
        self.generation += 1;
        if self.cache.len() > 4096 {
            let keep = self.generation.saturating_sub(3);
            self.cache.retain(|_, v| v.2 >= keep);
        }
    }

    pub fn set_ppp(&mut self, ppp: f32) {
        if (ppp - self.ppp).abs() > f32::EPSILON {
            self.ppp = ppp;
            self.cache.clear();
            self.metrics.clear();
        }
    }

    fn key(&self, style: &TextStyleResolved) -> FontKey {
        FontKey::new(
            style.size * self.ppp,
            style.family,
            style.bold,
            style.italic,
        )
    }

    /// (ascent, line height) in points.
    pub fn metrics_pt(&mut self, style: &TextStyleResolved) -> (f32, f32) {
        let key = self.key(style);
        let ppp = self.ppp;
        let backend = &mut self.backend;
        let (a, h) = *self
            .metrics
            .entry(key)
            .or_insert_with(|| backend.metrics(key));
        (a / ppp, h / ppp)
    }

    /// Cumulative x (points) after each char of one line of text.
    fn advances_pt(&mut self, text: &str, style: &TextStyleResolved) -> Arc<[f32]> {
        use std::hash::{Hash, Hasher};
        let key = self.key(style);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut h);
        let slot = (key, h.finish());
        let generation = self.generation;
        if let Some(v) = self.cache.get_mut(&slot) {
            if &*v.0 == text {
                v.2 = generation;
                return v.1.clone();
            }
        }
        self.scratch.clear();
        self.backend.advances(text, key, &mut self.scratch);
        let ppp = self.ppp;
        let adv: Arc<[f32]> = self.scratch.iter().map(|x| x / ppp).collect();
        self.cache
            .insert(slot, (Arc::from(text), adv.clone(), generation));
        adv
    }

    /// Width of one line of text, in points.
    pub fn width(&mut self, text: &str, style: &TextStyleResolved) -> f32 {
        self.advances_pt(text, style).last().copied().unwrap_or(0.0)
    }

    /// Lay out `text` into lines no wider than `wrap_width` (points;
    /// infinite = no wrapping, only hard line breaks).
    pub fn layout(&mut self, text: &str, style: TextStyleResolved, wrap_width: f32) -> Arc<Galley> {
        let (ascent, line_h) = self.metrics_pt(&style);
        let mut rows = Vec::new();
        let mut y = 0.0;
        let mut max_w: f32 = 0.0;
        let mut para_start = 0;
        for para in text.split('\n') {
            let adv = self.advances_pt(para, &style);
            // Character byte offsets and cumulative x for this paragraph.
            let offs: Vec<usize> = para.char_indices().map(|(i, _)| i).collect();
            let chars: Vec<char> = para.chars().collect();
            let n = chars.len();
            let mut start = 0usize; // char index of row start
            loop {
                let base = if start == 0 { 0.0 } else { adv[start - 1] };
                // Find the end of this row.
                let mut end = n;
                if wrap_width.is_finite() {
                    let mut last_break: Option<usize> = None;
                    let mut i = start;
                    while i < n {
                        // A space may hang past the edge; the line breaks after it.
                        if chars[i] == ' ' {
                            last_break = Some(i + 1);
                            i += 1;
                            continue;
                        }
                        if adv[i] - base > wrap_width && i > start {
                            break;
                        }
                        // Break opportunities: after a space, or around wide characters.
                        if chars[i] == ' '
                            || is_wide(chars[i])
                            || (i + 1 < n && is_wide(chars[i + 1]))
                        {
                            last_break = Some(i + 1);
                        }
                        i += 1;
                    }
                    if i < n {
                        end = match last_break {
                            Some(b) if b > start => b,
                            _ => i.max(start + 1),
                        };
                    }
                }
                let row_text_end = if end < n { offs[end] } else { para.len() };
                let mut xs = Vec::with_capacity(end - start + 1);
                xs.push(0.0);
                xs.extend(adv[start..end].iter().map(|x| x - base));
                // Trailing spaces at a wrap don't count toward the width.
                let mut w = *xs.last().unwrap();
                let mut k = end;
                while k > start && chars[k - 1] == ' ' && end < n {
                    k -= 1;
                    w = xs[k - start];
                }
                max_w = max_w.max(w);
                rows.push(Row {
                    start: para_start + if start < n { offs[start] } else { para.len() },
                    end: para_start + row_text_end,
                    x: xs,
                    y,
                    width: w,
                    ends_paragraph: end >= n,
                });
                y += line_h;
                if end >= n {
                    break;
                }
                start = end;
            }
            para_start += para.len() + 1;
        }
        Arc::new(Galley {
            text: text.to_string(),
            rows,
            size: vec2(max_w, y),
            line_height: line_h,
            ascent,
            style,
        })
    }

    /// Draw a laid-out galley with its top-left at `pos` (points).
    pub fn paint(&mut self, canvas: &mut Canvas, g: &Galley, pos: Pos2, color: Color32) {
        let key = self.key(&g.style);
        let ppp = self.ppp;
        for row in &g.rows {
            let s = &g.text[row.start..row.end];
            let s = s.trim_end_matches('\n');
            if s.is_empty() {
                continue;
            }
            let p = pos2(((pos.x) * ppp).round(), ((pos.y + row.y) * ppp).round());
            self.backend.draw(canvas, s, key, p, color);
            if g.style.strikethrough || g.style.underline {
                let yy = if g.style.underline {
                    g.ascent + 1.5
                } else {
                    g.ascent * 0.62
                };
                let y = ((pos.y + row.y + yy) * ppp).round() + 0.5;
                canvas.line(
                    pos2(p.x, y),
                    pos2(p.x + row.width * ppp, y),
                    ppp.max(1.0),
                    color,
                );
            }
        }
    }
}

/// Resolved formatting of a run of text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyleResolved {
    pub size: f32,
    pub family: FontFamily,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

impl Default for TextStyleResolved {
    fn default() -> Self {
        TextStyleResolved {
            size: 14.0,
            family: FontFamily::Proportional,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        }
    }
}

/// One laid-out line.
#[derive(Clone, Debug)]
pub struct Row {
    /// Byte range in the galley's text.
    pub start: usize,
    pub end: usize,
    /// x of each character boundary (len = chars + 1).
    pub x: Vec<f32>,
    pub y: f32,
    pub width: f32,
    pub ends_paragraph: bool,
}

/// Laid-out text.
#[derive(Clone, Debug)]
pub struct Galley {
    pub text: String,
    pub rows: Vec<Row>,
    pub size: Vec2,
    pub line_height: f32,
    pub ascent: f32,
    pub style: TextStyleResolved,
}

impl Galley {
    pub fn size(&self) -> Vec2 {
        self.size
    }
    pub fn rect(&self) -> Rect {
        Rect::from_min_size(Pos2::ZERO, self.size)
    }

    /// Caret position (top of the line) for a byte offset.
    pub fn pos_of(&self, byte: usize) -> Pos2 {
        for (i, row) in self.rows.iter().enumerate() {
            let last = i + 1 == self.rows.len();
            let inside = byte >= row.start
                && (byte < row.end || (byte == row.end && (row.ends_paragraph || last)));
            if inside {
                let n = self.text[row.start..byte].chars().count();
                return pos2(row.x[n.min(row.x.len() - 1)], row.y);
            }
        }
        match self.rows.last() {
            Some(r) => pos2(r.width, r.y),
            None => Pos2::ZERO,
        }
    }

    /// Byte offset of the character boundary nearest to `p`.
    pub fn index_at(&self, p: Pos2) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        let ri = ((p.y / self.line_height).floor().max(0.0) as usize).min(self.rows.len() - 1);
        let row = &self.rows[ri];
        let mut best = 0;
        let mut best_d = f32::INFINITY;
        for (k, x) in row.x.iter().enumerate() {
            let d = (x - p.x).abs();
            if d < best_d {
                best_d = d;
                best = k;
            }
        }
        let s = &self.text[row.start..row.end];
        row.start + s.char_indices().nth(best).map_or(s.len(), |(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> Fonts {
        Fonts::new(Box::new(MockText))
    }

    #[test]
    fn wraps_at_spaces_and_keeps_hard_breaks() {
        let mut f = fonts();
        let st = TextStyleResolved {
            size: 10.0,
            ..Default::default()
        };
        // Each Latin char is 5.5 → round(5.5) = 6 px wide at 10 px.
        let g = f.layout("aaa bbb ccc", st, 45.0);
        let lines: Vec<&str> = g
            .rows
            .iter()
            .map(|r| g.text[r.start..r.end].trim_end())
            .collect();
        assert_eq!(lines, ["aaa bbb", "ccc"]);
        assert_eq!(g.rows[0].width, 42.0);
        let g = f.layout("one\ntwo", st, f32::INFINITY);
        assert_eq!(g.rows.len(), 2);
        assert_eq!(&g.text[g.rows[1].start..g.rows[1].end], "two");
        // A word longer than the width is split.
        let g = f.layout("abcdefghij", st, 25.0);
        assert!(g.rows.len() >= 2 && g.rows.iter().all(|r| r.width <= 25.0));
        // Wide characters break anywhere.
        let g = f.layout("日本語の文章", st, 25.0);
        assert_eq!(g.rows.len(), 3);
    }

    #[test]
    fn caret_positions_round_trip() {
        let mut f = fonts();
        let st = TextStyleResolved {
            size: 10.0,
            ..Default::default()
        };
        let g = f.layout("héllo wörld", st, f32::INFINITY);
        for (i, _) in g.text.char_indices() {
            assert_eq!(g.index_at(g.pos_of(i)), i);
        }
        assert_eq!(g.pos_of(g.text.len()).x, g.rows[0].width);
        let g = f.layout("ab\ncd", st, f32::INFINITY);
        assert_eq!(g.pos_of(3), pos2(0.0, g.line_height));
        assert_eq!(g.index_at(pos2(100.0, g.line_height + 1.0)), 5);
    }
}
