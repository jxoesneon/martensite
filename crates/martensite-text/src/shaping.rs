//! Complex text shaping: BiDi, line breaking, font fallback chaining,
//! and text measurement via cosmic-text.
//!
//! The [`Shaper`] wraps a cosmic-text [`Buffer`] and provides high-level
//! methods for shaping text runs, measuring text, and extracting glyph
//! data for rendering. It handles Unicode Bidirectional Algorithm
//! reordering, line breaking, and font fallback automatically through
//! cosmic-text's `Shaping::Advanced` mode.

use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping};

use crate::font::{FontId, FontManager};

/// The result of measuring shaped text.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct TextMetrics {
    /// The total width of the shaped text in logical pixels.
    pub width: f32,
    /// The total height of the shaped text in logical pixels.
    pub height: f32,
    /// The number of lines after wrapping.
    pub line_count: usize,
}

impl TextMetrics {
    /// Creates zero metrics.
    #[inline(always)]
    pub fn zero() -> Self {
        Self::default()
    }

    /// Returns `true` if either dimension is zero or negative.
    ///
    /// This is consistent with `martensite_layout::geometry::Size::is_empty`.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

/// A shaped glyph ready for rendering.
#[derive(Copy, Clone, Debug)]
pub struct ShapedGlyph {
    /// Start index in the original text.
    pub start: usize,
    /// End index in the original text.
    pub end: usize,
    /// Font face ID.
    pub font_id: FontId,
    /// Glyph ID within the font.
    pub glyph_id: u16,
    /// X position of the glyph's hitbox.
    pub x: f32,
    /// Y position of the glyph's hitbox.
    pub y: f32,
    /// Width of the glyph's hitbox.
    pub w: f32,
    /// Font size used for this glyph.
    pub font_size: f32,
    /// Unicode BiDi embedding level (even = LTR, odd = RTL).
    pub bidi_level: u8,
}

/// A single line of shaped text.
#[derive(Clone, Debug)]
pub struct ShapedLine {
    /// The original text of this line.
    pub text: String,
    /// Whether the paragraph direction is RTL.
    pub rtl: bool,
    /// Y offset to the baseline of this line.
    pub line_y: f32,
    /// Y offset to the top of this line.
    pub line_top: f32,
    /// The line height.
    pub line_height: f32,
    /// The width of this line.
    pub line_w: f32,
    /// The glyphs in this line.
    pub glyphs: Vec<ShapedGlyph>,
}

/// The text shaper, wrapping a cosmic-text [`Buffer`].
///
/// The shaper is reusable: call [`Shaper::set_text`] to change the
/// text, then [`Shaper::shape`] to perform shaping and line breaking,
/// and [`Shaper::measure`] or [`Shaper::lines`] to extract results.
pub struct Shaper {
    buffer: Buffer,
}

impl Shaper {
    /// Creates a new `Shaper` with the given font system and font metrics.
    pub fn new(font_system: &mut FontSystem, metrics: Metrics) -> Self {
        Self {
            buffer: Buffer::new(font_system, metrics),
        }
    }

    /// Creates a new `Shaper` with empty metrics (zero font size).
    /// Call [`Shaper::set_metrics`] before shaping.
    pub fn new_empty(metrics: Metrics) -> Self {
        Self {
            buffer: Buffer::new_empty(metrics),
        }
    }

    /// Sets the font size and line height.
    pub fn set_metrics(&mut self, font_size: f32, line_height: f32) {
        let metrics = Metrics::new(font_size, line_height);
        self.buffer.set_metrics(metrics);
    }

    /// Sets the available width and height for wrapping.
    ///
    /// `None` means unbounded (no wrapping on that axis).
    pub fn set_size(&mut self, width: Option<f32>, height: Option<f32>) {
        self.buffer.set_size(width, height);
    }

    /// Sets the text to be shaped with the given attributes.
    ///
    /// Uses `Shaping::Advanced` for full BiDi, fallback, and complex
    /// script support.
    pub fn set_text(&mut self, text: &str, attrs: &Attrs) {
        self.buffer.set_text(text, attrs, Shaping::Advanced, None);
    }

    /// Performs shaping and line breaking up to the current cursor
    /// or the entire buffer.
    ///
    /// After calling this, [`Shaper::measure`] and [`Shaper::lines`]
    /// return the final results.
    pub fn shape(&mut self, font_system: &mut FontSystem) {
        self.buffer.shape_until_scroll(font_system, false);
    }

    /// Measures the shaped text, returning the total width, height,
    /// and line count.
    ///
    /// Must be called after [`Shaper::shape`].
    pub fn measure(&self) -> TextMetrics {
        let mut max_width = 0.0f32;
        let mut total_height = 0.0f32;
        let mut line_count = 0usize;

        for run in self.buffer.layout_runs() {
            let run_w = run.line_w.max(
                run.glyphs
                    .iter()
                    .map(|g| g.x + g.w)
                    .max_by(f32::total_cmp)
                    .unwrap_or(0.0),
            );
            max_width = max_width.max(run_w);
            total_height += run.line_height;
            line_count += 1;
        }

        TextMetrics {
            width: max_width,
            height: total_height,
            line_count,
        }
    }

    /// Returns the shaped lines as [`ShapedLine`]s.
    ///
    /// Must be called after [`Shaper::shape`].
    pub fn lines(&self) -> Vec<ShapedLine> {
        self.buffer
            .layout_runs()
            .map(|run| ShapedLine {
                text: run.text.to_string(),
                rtl: run.rtl,
                line_y: run.line_y,
                line_top: run.line_top,
                line_height: run.line_height,
                line_w: run.line_w,
                glyphs: run
                    .glyphs
                    .iter()
                    .map(|g| ShapedGlyph {
                        start: g.start,
                        end: g.end,
                        font_id: FontId(g.font_id),
                        glyph_id: g.glyph_id,
                        x: g.x,
                        y: g.y,
                        w: g.w,
                        font_size: g.font_size,
                        bidi_level: g.level.number(),
                    })
                    .collect(),
            })
            .collect()
    }

    /// Borrows the underlying cosmic-text [`Buffer`].
    #[inline(always)]
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Mutably borrows the underlying cosmic-text [`Buffer`].
    #[inline(always)]
    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffer
    }

    /// Convenience: shape text and return metrics in one call.
    pub fn measure_text(
        &mut self,
        font_system: &mut FontSystem,
        text: &str,
        attrs: &Attrs,
        font_size: f32,
        line_height: f32,
        max_width: Option<f32>,
    ) -> TextMetrics {
        self.set_metrics(font_size, line_height);
        self.set_size(max_width, None);
        self.set_text(text, attrs);
        self.shape(font_system);
        self.measure()
    }
}

/// Convenience function to measure text using a [`FontManager`].
///
/// Creates a temporary [`Shaper`], shapes the text, and returns the
/// metrics. For repeated measurements, reuse a [`Shaper`] instance.
pub fn measure_text(
    manager: &mut FontManager,
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: Option<f32>,
) -> TextMetrics {
    measure_text_with_attrs(
        manager,
        text,
        &Attrs::new(),
        font_size,
        line_height,
        max_width,
    )
}

/// Convenience function to measure text with explicit attributes
/// (font family, direction, weight, etc.).
pub fn measure_text_with_attrs(
    manager: &mut FontManager,
    text: &str,
    attrs: &Attrs,
    font_size: f32,
    line_height: f32,
    max_width: Option<f32>,
) -> TextMetrics {
    let mut shaper = Shaper::new_empty(Metrics::new(font_size, line_height));
    shaper.set_size(max_width, None);
    shaper.set_text(text, attrs);
    shaper.shape(manager.system_mut());
    shaper.measure()
}

/// Convenience function to shape text and extract lines using a
/// [`FontManager`].
pub fn shape_text(
    manager: &mut FontManager,
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: Option<f32>,
) -> Vec<ShapedLine> {
    let mut shaper = Shaper::new_empty(Metrics::new(font_size, line_height));
    shaper.set_size(max_width, None);
    shaper.set_text(text, &Attrs::new());
    shaper.shape(manager.system_mut());
    shaper.lines()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_metrics_zero() {
        let m = TextMetrics::zero();
        assert_eq!(m.width, 0.0);
        assert_eq!(m.height, 0.0);
        assert_eq!(m.line_count, 0);
        assert!(m.is_empty());
    }

    #[test]
    fn text_metrics_non_empty() {
        let m = TextMetrics {
            width: 100.0,
            height: 20.0,
            line_count: 1,
        };
        assert!(!m.is_empty());
    }

    #[test]
    fn shaper_new_empty() {
        let shaper = Shaper::new_empty(Metrics::new(16.0, 20.0));
        // Empty shaper should measure zero
        let m = shaper.measure();
        assert_eq!(m.line_count, 0);
    }

    #[test]
    fn shaper_measure_empty_text() {
        let mut manager = FontManager::with_fonts(std::iter::empty());
        let metrics = measure_text(&mut manager, "", 16.0, 20.0, None);
        // Empty text may still produce one empty line in cosmic-text
        assert!(
            metrics.line_count <= 1,
            "empty text should have 0 or 1 lines"
        );
        // Width should be zero for empty text
        assert_eq!(metrics.width, 0.0);
    }

    #[test]
    fn shaper_measure_simple_text() {
        let mut manager = FontManager::new();
        let metrics = measure_text(&mut manager, "Hello", 16.0, 20.0, None);
        // Should have at least one line
        assert!(
            metrics.line_count >= 1,
            "should have at least 1 line, got {}",
            metrics.line_count
        );
        // Width should be positive (if a font was found)
        if metrics.width > 0.0 {
            assert!(metrics.height > 0.0);
        }
    }

    #[test]
    fn shaper_measure_with_wrapping() {
        let mut manager = FontManager::new();
        // Long text with narrow width should wrap to multiple lines
        let metrics = measure_text(
            &mut manager,
            "The quick brown fox jumps over the lazy dog repeatedly",
            16.0,
            20.0,
            Some(50.0),
        );
        // With a 50px width, this should wrap to multiple lines (if fonts available)
        if metrics.width > 0.0 {
            assert!(
                metrics.line_count > 1,
                "expected wrapping, got {} lines",
                metrics.line_count
            );
        }
    }

    #[test]
    fn shaper_measure_multilingual() {
        let mut manager = FontManager::new();
        // Mix of LTR and RTL text
        let texts = ["Hello", "مرحبا", "你好", "Привет", "こんにちは"];
        for text in &texts {
            let metrics = measure_text(&mut manager, text, 16.0, 20.0, None);
            // Should not panic; may have zero width if no font covers the script
            let _ = metrics;
        }
    }

    #[test]
    fn shape_text_returns_lines() {
        let mut manager = FontManager::new();
        let lines = shape_text(&mut manager, "Hello World", 16.0, 20.0, None);
        if !lines.is_empty() {
            let first = &lines[0];
            assert!(!first.text.is_empty());
        }
    }

    #[test]
    fn shaped_glyph_bidi_level() {
        let mut manager = FontManager::new();
        let lines = shape_text(&mut manager, "Hello", 16.0, 20.0, None);
        for line in &lines {
            for glyph in &line.glyphs {
                // LTR text should have even bidi level
                assert_eq!(
                    glyph.bidi_level % 2,
                    0,
                    "LTR text should have even bidi level"
                );
            }
        }
    }

    #[test]
    fn shaper_reuse() {
        let mut manager = FontManager::new();
        let mut shaper = Shaper::new(manager.system_mut(), Metrics::new(16.0, 20.0));

        // First text
        shaper.set_text("First", &Attrs::new());
        shaper.shape(manager.system_mut());
        let m1 = shaper.measure();

        // Second text - shaper should be reusable
        shaper.set_text("Second text that is longer", &Attrs::new());
        shaper.shape(manager.system_mut());
        let m2 = shaper.measure();

        // Both should produce valid results
        let _ = (m1, m2);
    }

    #[test]
    fn shaper_set_size_affects_wrapping() {
        let mut manager = FontManager::new();
        let mut shaper = Shaper::new(manager.system_mut(), Metrics::new(16.0, 20.0));

        // Unbounded width
        shaper.set_size(None, None);
        shaper.set_text("The quick brown fox", &Attrs::new());
        shaper.shape(manager.system_mut());
        let unbounded = shaper.measure();

        // Bounded width
        shaper.set_size(Some(30.0), None);
        shaper.set_text("The quick brown fox", &Attrs::new());
        shaper.shape(manager.system_mut());
        let bounded = shaper.measure();

        // Bounded should have more lines or equal (if no font found)
        if unbounded.line_count > 0 && bounded.line_count > 0 {
            assert!(
                bounded.line_count >= unbounded.line_count,
                "bounded width should have >= lines than unbounded"
            );
        }
    }
}
