//! Shared shaped-text emission for widgets whose labels change at
//! paint time.
//!
//! [`PaintList::push_text`](martensite_core::paint::PaintList::push_text)
//! emits `DrawText`, which the render backends
//! approximate as opaque bounding-box rectangles — real glyph outlines
//! go through `DrawGlyphRun` + `FontResource`. `Widget::paint` takes
//! `&self`, so widgets cannot own a `FontManager` and shape lazily the
//! way `Text` does; `TextPainter` is the interior-mutable escape hatch:
//! wrap it in a [`SharedTextPainter`](crate::text_paint::SharedTextPainter)
//! (`Arc<Mutex<…>>`), hand clones to
//! every widget that should paint real text, and each `paint` locks it
//! briefly to shape.
//!
//! Without a painter widgets keep emitting `DrawText` placeholder
//! boxes — honest degraded output, documented in each widget's `paint`
//! rather than silently changed.

use std::sync::Arc;

use parking_lot::Mutex;

use kurbo::Point;
use martensite_core::paint::{FontResource, GlyphInstance, GlyphRun, PaintList};
use martensite_text::{shape_text, FontManager};

/// A shared, lockable [`TextPainter`].
///
/// Cheap to clone; every clone shares one `FontManager` so font
/// discovery happens once per app rather than once per widget. The
/// mutex is `parking_lot` — poison-free, so `paint` never unwraps.
/// Newtype (rather than a bare `Arc<Mutex>`) so it can implement
/// [`martensite_core::paint::TextShaper`], which lets it serve both as
/// a widget's explicit painter and as the arena's ambient one.
#[derive(Clone)]
pub struct SharedTextPainter(Arc<Mutex<TextPainter>>);

/// Creates a [`SharedTextPainter`] — convenience for the `Arc<Mutex>`
/// ceremony.
///
/// # Examples
///
/// ```
/// use martensite::text_paint::shared_painter;
///
/// let painter = shared_painter();
/// ```
pub fn shared_painter() -> SharedTextPainter {
    SharedTextPainter(Arc::new(Mutex::new(TextPainter::new())))
}

/// Owns font discovery + shaping for widget text. `FontManager`
/// creation scans the system font set, so construction defers it until
/// the first emission (the same lazy pattern `Text` uses).
pub struct TextPainter {
    fonts: Option<FontManager>,
}

impl Default for TextPainter {
    fn default() -> Self {
        Self::new()
    }
}

impl TextPainter {
    /// Creates a painter whose `FontManager` initializes on first use.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::TextPainter;
    ///
    /// let _painter = TextPainter::new();
    /// ```
    pub fn new() -> Self {
        Self { fonts: None }
    }

    fn fonts(&mut self) -> &mut FontManager {
        self.fonts.get_or_insert_with(FontManager::new)
    }

    /// Advance width of `text` at `size` — used for right-aligned cells
    /// and caret positioning. Shapes a single line with no wrap.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::TextPainter;
    ///
    /// let mut p = TextPainter::new();
    /// let w = p.measure("hello", 14.0);
    /// assert!(w >= 0.0);
    /// ```
    pub fn measure(&mut self, text: &str, size: f32) -> f32 {
        let mut end = 0.0f32;
        for line in shape_text(self.fonts(), text, size, size * 1.25, None) {
            for g in &line.glyphs {
                end = end.max(g.x + g.w);
            }
        }
        end
    }

    /// Device-pixel x of the insertion boundary before `byte_idx`,
    /// derived from one shape pass over the whole text so kerning and
    /// ligature context agree with `push` — this is the caret position
    /// for that byte offset. `byte_idx` is clamped to `text.len()`;
    /// boundaries inside a multi-byte cluster interpolate within the
    /// cluster's glyph. Assumes a single LTR line (text inputs, labels).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::TextPainter;
    ///
    /// let mut p = TextPainter::new();
    /// let w = p.measure("hi", 14.0);
    /// assert!((p.caret_x("hi", 14.0, 2) - w).abs() < 1.0);
    /// ```
    pub fn caret_x(&mut self, text: &str, size: f32, byte_idx: usize) -> f32 {
        let byte_idx = byte_idx.min(text.len());
        let Some(line) = shape_text(self.fonts(), text, size, size * 1.25, None)
            .into_iter()
            .next()
        else {
            return 0.0;
        };
        let mut right_edge = 0.0f32;
        for g in &line.glyphs {
            if g.start >= byte_idx {
                // First glyph at or after the boundary — its left edge
                // is where the next character would begin.
                return g.x;
            }
            if g.end > byte_idx {
                // Boundary inside a cluster (e.g. a ligature) —
                // interpolate so round-tripping `byte_at` stays stable.
                let span = (g.end - g.start).max(1) as f32;
                return g.x + g.w * (byte_idx - g.start) as f32 / span;
            }
            right_edge = right_edge.max(g.x + g.w);
        }
        right_edge
    }

    /// Byte offset of the insertion boundary nearest device-pixel `x`,
    /// the inverse of [`caret_x`](Self::caret_x) — click and drag
    /// hit-testing. Always returns a boundary between clusters.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::TextPainter;
    ///
    /// let mut p = TextPainter::new();
    /// assert_eq!(p.byte_at("hi", 14.0, -10.0), 0);
    /// assert_eq!(p.byte_at("hi", 14.0, 10_000.0), 2);
    /// ```
    pub fn byte_at(&mut self, text: &str, size: f32, x: f32) -> usize {
        let Some(line) = shape_text(self.fonts(), text, size, size * 1.25, None)
            .into_iter()
            .next()
        else {
            return 0;
        };
        for g in &line.glyphs {
            if x < g.x + g.w * 0.5 {
                return g.start.min(text.len());
            }
            if x < g.x + g.w {
                return g.end.min(text.len());
            }
        }
        text.len()
    }

    /// Shapes `text` and emits `DrawGlyphRun` commands; returns the y
    /// coordinate of the rendered block's bottom so callers can stack
    /// text without colliding. `origin` is the top-left of the block.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::TextPainter;
    /// use kurbo::Point;
    /// use martensite_core::PaintList;
    ///
    /// let mut p = TextPainter::new();
    /// let mut list = PaintList::new();
    /// p.push(&mut list, Point::new(10.0, 10.0), "PAUSE", 14.0, [255; 4], None);
    /// ```
    pub fn push(
        &mut self,
        list: &mut PaintList,
        origin: Point,
        text: &str,
        size: f32,
        color: [u8; 4],
        max_width: Option<f32>,
    ) -> f64 {
        if text.is_empty() || size <= 0.0 {
            return origin.y;
        }
        let mut block_bottom = origin.y;
        let fonts = self.fonts();
        for line in shape_text(fonts, text, size, size * 1.25, max_width) {
            block_bottom = origin.y + f64::from(line.line_y + size * 1.25);
            let baseline_y = origin.y as f32 + line.line_y;
            let mut run = GlyphRun::new(size, color);
            let mut run_font = None;
            for g in &line.glyphs {
                if let Some(prev) = run_font {
                    if prev != g.font_id {
                        if let Some((data, index)) = fonts.font_data(prev) {
                            run.set_font(FontResource::new(data, index));
                        }
                        list.push_glyph_run(run);
                        run = GlyphRun::new(g.font_size, color);
                    }
                }
                run.font_size = g.font_size;
                run_font = Some(g.font_id);
                run.push(GlyphInstance::new(
                    origin.x as f32 + g.x,
                    baseline_y + g.y,
                    u32::from(g.glyph_id),
                    g.w,
                    line.line_height,
                ));
            }
            if let Some(font_id) = run_font.filter(|_| !run.is_empty()) {
                if let Some((data, index)) = fonts.font_data(font_id) {
                    run.set_font(FontResource::new(data, index));
                }
                list.push_glyph_run(run);
            }
        }
        block_bottom
    }
}

impl SharedTextPainter {
    /// Advance width of `text` at `size` device pixels — see
    /// [`TextPainter::measure`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::shared_painter;
    ///
    /// let p = shared_painter();
    /// assert!(p.measure("hi", 14.0) >= 0.0);
    /// ```
    pub fn measure(&self, text: &str, size: f32) -> f32 {
        self.0.lock().measure(text, size)
    }

    /// Device-pixel x of the insertion boundary before `byte_idx` —
    /// see [`TextPainter::caret_x`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::shared_painter;
    ///
    /// let p = shared_painter();
    /// assert_eq!(p.caret_x("", 14.0, 0), 0.0);
    /// ```
    pub fn caret_x(&self, text: &str, size: f32, byte_idx: usize) -> f32 {
        self.0.lock().caret_x(text, size, byte_idx)
    }

    /// Byte offset of the insertion boundary nearest device-pixel `x`
    /// — see [`TextPainter::byte_at`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::shared_painter;
    ///
    /// let p = shared_painter();
    /// assert_eq!(p.byte_at("hi", 14.0, 10_000.0), 2);
    /// ```
    pub fn byte_at(&self, text: &str, size: f32, x: f32) -> usize {
        self.0.lock().byte_at(text, size, x)
    }
}

/// `SharedTextPainter` satisfies the core
/// [`TextShaper`](martensite_core::paint::TextShaper) seam —
/// widgets use it as their explicit painter, and
/// [`martensite_core::WidgetArena::set_text_painter`] stores one as the
/// arena's ambient painter.
impl martensite_core::paint::TextShaper for SharedTextPainter {
    fn paint_shaped_text(
        &self,
        list: &mut PaintList,
        origin: Point,
        text: &str,
        size_px: f32,
        color: [u8; 4],
    ) {
        self.0.lock().push(list, origin, text, size_px, color, None);
    }

    fn measure_text(&self, text: &str, size_px: f32) -> Option<f32> {
        Some(self.0.lock().measure(text, size_px))
    }
}

/// Resolves the painter a widget should use for this paint pass: its
/// own explicitly-injected one first, then the ambient
/// [`PaintContext::text_painter`](martensite_core::PaintContext::text_painter).
pub(crate) fn resolve_painter<'a>(
    explicit: &'a Option<SharedTextPainter>,
    ambient: Option<&'a (dyn martensite_core::paint::TextShaper + Send + Sync)>,
) -> Option<&'a (dyn martensite_core::paint::TextShaper + Send + Sync)> {
    explicit
        .as_ref()
        .map(|p| p as &(dyn martensite_core::paint::TextShaper + Send + Sync))
        .or(ambient)
}

/// Emits `text` through `painter` when present, else falls back to
/// [`PaintList::push_text`]'s placeholder boxes. `pub(crate)` — the
/// facade widgets share this so the opt-in is one line in each `paint`.
pub(crate) fn paint_label(
    painter: Option<&(dyn martensite_core::paint::TextShaper + Send + Sync)>,
    list: &mut PaintList,
    origin: Point,
    text: &str,
    size_px: f32,
    color: [u8; 4],
) {
    match painter {
        Some(tp) => {
            tp.paint_shaped_text(list, origin, text, size_px, color);
        }
        None => {
            list.push_text(origin, text.to_string(), size_px, color);
        }
    }
}

/// [`paint_label`] clipped to `clip` — for labels painted inside a
/// fixed-size container (toast cards, dialog cards, input faces, tab
/// slots), where an over-long string must not spill past the chrome.
pub(crate) fn paint_label_clipped(
    painter: Option<&(dyn martensite_core::paint::TextShaper + Send + Sync)>,
    list: &mut PaintList,
    clip: kurbo::Rect,
    origin: Point,
    text: &str,
    size_px: f32,
    color: [u8; 4],
) {
    // A degenerate or inverted clip provably paints nothing — emitting
    // the clip+text anyway is dead work the paint audit flags as
    // clipped text. Narrow widgets (a face too small for its label)
    // hit this path legitimately.
    if clip.x1 <= clip.x0 || clip.y1 <= clip.y0 {
        return;
    }
    list.push_clip(clip);
    paint_label(painter, list, origin, text, size_px, color);
    list.pop_clip();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_emits_nothing() {
        let mut p = TextPainter::new();
        let mut list = PaintList::new();
        let bottom = p.push(&mut list, Point::new(0.0, 5.0), "", 14.0, [255; 4], None);
        assert_eq!(bottom, 5.0);
        assert!(list.commands.is_empty());
    }

    #[test]
    fn zero_size_emits_nothing() {
        let mut p = TextPainter::new();
        let mut list = PaintList::new();
        p.push(&mut list, Point::new(0.0, 0.0), "x", 0.0, [255; 4], None);
        assert!(list.commands.is_empty());
    }

    #[test]
    fn caret_x_matches_measure_at_end() {
        let mut p = TextPainter::new();
        let text = "mixed Width iiWWwww";
        let w = p.measure(text, 14.0);
        let end = p.caret_x(text, 14.0, text.len());
        // The caret past the last glyph sits at the run's advance —
        // the property the old 7px/char estimate violated on
        // mixed-width text.
        assert!((end - w).abs() < 0.5, "caret {end} vs measure {w}");
    }

    #[test]
    fn caret_x_is_monotonic() {
        let mut p = TextPainter::new();
        let text = "iiiiWWWW";
        let mut prev = -1.0f32;
        for b in 0..=text.len() {
            let x = p.caret_x(text, 14.0, b);
            assert!(x >= prev, "caret moved backwards at byte {b}");
            prev = x;
        }
    }

    #[test]
    fn paint_label_clipped_balances_clip() {
        let mut list = PaintList::new();
        paint_label_clipped(
            None,
            &mut list,
            kurbo::Rect::new(10.0, 2.0, 50.0, 20.0),
            Point::new(12.0, 4.0),
            "label",
            14.0,
            [255; 4],
        );
        // ClipRect → text → PopClip, in order.
        assert!(matches!(
            list.commands[0],
            martensite_core::paint::PaintCommand::ClipRect(r)
                if r == kurbo::Rect::new(10.0, 2.0, 50.0, 20.0)
        ));
        assert!(matches!(
            list.commands.last(),
            Some(martensite_core::paint::PaintCommand::PopClip)
        ));
    }

    #[test]
    fn byte_at_roundtrips_caret_x() {
        let mut p = TextPainter::new();
        // No common ligatures here — every byte boundary is a real
        // cluster boundary, so caret_x inverts exactly.
        let text = "aXgTm";
        for b in 0..=text.len() {
            let x = p.caret_x(text, 14.0, b);
            assert_eq!(p.byte_at(text, 14.0, x), b, "round-trip at {b}");
        }
        // Clamping outside the run.
        assert_eq!(p.byte_at(text, 14.0, -100.0), 0);
        assert_eq!(p.byte_at(text, 14.0, 100_000.0), text.len());
    }
}
