//! Shared shaped-text emission for widgets whose labels change at
//! paint time.
//!
//! [`PaintList::push_text`] emits `DrawText`, which the render backends
//! approximate as opaque bounding-box rectangles — real glyph outlines
//! go through `DrawGlyphRun` + `FontResource`. `Widget::paint` takes
//! `&self`, so widgets cannot own a `FontManager` and shape lazily the
//! way `Text` does; `TextPainter` is the interior-mutable escape hatch:
//! wrap it in a [`SharedTextPainter`] (`Arc<Mutex<…>>`), hand clones to
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

/// `SharedTextPainter` satisfies the core [`TextShaper`] seam —
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
}
