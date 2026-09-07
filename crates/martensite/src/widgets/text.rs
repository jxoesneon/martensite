//! `Text` widget: displays shaped text with font, size, and color.
//!
//! The `Text` widget integrates with `martensite_text` for shaping
//! and measurement. It uses the [`InlineTextCache`] in `ColdNode` for
//! fast flexbox constraint probing during two-pass layout, and the
//! global `TextShapeCache` for full shaping results.

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{LayoutConstraints, LayoutContext, Widget};
use martensite_core::{InlineTextCache, Rect};
use martensite_text::TextMetrics;

/// A text widget that displays a string with specified font properties.
pub struct Text {
    /// The text content to display.
    pub content: String,
    /// Font size in logical pixels.
    pub font_size: f32,
    /// Line height in logical pixels. If `None`, uses `font_size * 1.2`.
    pub line_height: Option<f32>,
    /// Font family name.
    pub family: String,
    /// Optional text color (Oklab).
    pub color: Option<martensite_theme::Oklab>,
    /// Whether the text direction is RTL.
    pub rtl: bool,
    /// Inline text cache for fast measurement probing.
    inline_cache: InlineTextCache,
    /// Cached metrics from the last measure pass.
    cached_metrics: TextMetrics,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
}

impl Text {
    /// Creates a new text widget with the given content and default
    /// font size (16px).
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            font_size: 16.0,
            line_height: None,
            family: String::new(), // Empty = default/sans-serif
            color: None,
            rtl: false,
            inline_cache: InlineTextCache::new(),
            cached_metrics: TextMetrics::zero(),
            cached_bounds: Rect::default(),
        }
    }

    /// Sets the font size.
    #[inline]
    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// Sets the line height.
    #[inline]
    pub fn line_height(mut self, height: f32) -> Self {
        self.line_height = Some(height);
        self
    }

    /// Sets the font family name.
    #[inline]
    pub fn family(mut self, family: impl Into<String>) -> Self {
        self.family = family.into();
        self
    }

    /// Sets the text color.
    #[inline]
    pub fn color(mut self, color: martensite_theme::Oklab) -> Self {
        self.color = Some(color);
        self
    }

    /// Sets the RTL direction.
    #[inline]
    pub fn rtl(mut self) -> Self {
        self.rtl = true;
        self
    }

    /// Returns the effective line height.
    #[inline]
    fn effective_line_height(&self) -> f32 {
        self.line_height.unwrap_or(self.font_size * 1.2)
    }

    /// Returns the cached metrics from the last measure pass.
    #[inline]
    pub fn cached_metrics(&self) -> TextMetrics {
        self.cached_metrics
    }

    /// Returns the cached bounds from the last layout pass.
    #[inline]
    pub fn cached_bounds(&self) -> Rect {
        self.cached_bounds
    }

    /// Returns the inline text cache.
    #[inline]
    pub fn inline_cache(&self) -> &InlineTextCache {
        &self.inline_cache
    }

    /// Returns a mutable reference to the inline text cache.
    #[inline]
    pub fn inline_cache_mut(&mut self) -> &mut InlineTextCache {
        &mut self.inline_cache
    }
}

impl Widget for Text {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let available_width = constraints.max_size.x;

        // Try the inline cache first (Tier 1)
        if let Some(cached_height) = self.inline_cache.get(available_width) {
            let width = if available_width.is_finite() {
                available_width.min(self.cached_metrics.width)
            } else {
                self.cached_metrics.width
            };
            return Vec2::new(width, cached_height);
        }

        // Full measurement (Tier 2 would be checked here in a full
        // implementation; for now we do a direct measurement using
        // a simplified estimation. The real implementation would use
        // the Shaper with the application's shared FontSystem.)
        let line_height = self.effective_line_height();
        let max_width = if available_width.is_finite() {
            Some(available_width)
        } else {
            None
        };
        let _ = max_width;

        // For the widget's measure, we use a simplified approach:
        // estimate text width based on character count and font size.
        // This is a rough approximation; the real implementation would
        // use the Shaper with the application's FontSystem.
        let char_count = self.content.chars().count();
        let estimated_width = char_count as f32 * self.font_size * 0.5; // rough average
        let height = if self.content.is_empty() {
            line_height
        } else {
            let lines = if available_width.is_finite() && available_width > 0.0 {
                (estimated_width / available_width).ceil().max(1.0) as usize
            } else {
                1
            };
            lines as f32 * line_height
        };

        let width = if available_width.is_finite() {
            estimated_width.min(available_width)
        } else {
            estimated_width
        };

        self.cached_metrics = TextMetrics {
            width,
            height,
            line_count: if self.content.is_empty() { 0 } else { 1 },
        };

        // Store in inline cache
        self.inline_cache.put(available_width, height);

        let _ = cx;
        Vec2::new(width, height)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        let _ = cx;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::TextRun);
        node.set_value(&self.content);
    }
}

impl std::fmt::Debug for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Text")
            .field("content", &self.content)
            .field("font_size", &self.font_size)
            .field("family", &self.family)
            .field("rtl", &self.rtl)
            .finish()
    }
}

impl Clone for Text {
    fn clone(&self) -> Self {
        Self {
            content: self.content.clone(),
            font_size: self.font_size,
            line_height: self.line_height,
            family: self.family.clone(),
            color: self.color,
            rtl: self.rtl,
            inline_cache: InlineTextCache::new(),
            cached_metrics: self.cached_metrics,
            cached_bounds: self.cached_bounds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot }
    }

    #[test]
    fn text_new() {
        let t = Text::new("Hello");
        assert_eq!(t.content, "Hello");
        assert_eq!(t.font_size, 16.0);
        assert!(!t.rtl);
    }

    #[test]
    fn text_builder_methods() {
        let t = Text::new("Hello")
            .font_size(20.0)
            .line_height(24.0)
            .family("Helvetica")
            .color(martensite_theme::Oklab {
                l: 1.0,
                a: 0.0,
                b: 0.0,
                alpha: 1.0,
            })
            .rtl();
        assert_eq!(t.font_size, 20.0);
        assert_eq!(t.line_height, Some(24.0));
        assert_eq!(t.family, "Helvetica");
        assert!(t.color.is_some());
        assert!(t.rtl);
    }

    #[test]
    fn text_effective_line_height() {
        let t = Text::new("Hello").font_size(20.0);
        // Default: font_size * 1.2
        assert!((t.effective_line_height() - 24.0).abs() < 0.001);
        let t2 = Text::new("Hello").font_size(20.0).line_height(30.0);
        assert!((t2.effective_line_height() - 30.0).abs() < 0.001);
    }

    #[test]
    fn text_measure_returns_size() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut t = Text::new("Hello World").font_size(16.0);
        let size = t.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(1000.0, 1000.0),
            },
        );
        // Should have positive width for non-empty text
        assert!(size.x > 0.0, "width should be positive, got {}", size.x);
        assert!(size.y > 0.0, "height should be positive, got {}", size.y);
    }

    #[test]
    fn text_measure_empty() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut t = Text::new("").font_size(16.0);
        let size = t.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(1000.0, 1000.0),
            },
        );
        // Empty text should have zero width but line height for height
        assert_eq!(size.x, 0.0);
    }

    #[test]
    fn text_measure_uses_inline_cache() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut t = Text::new("Hello").font_size(16.0);

        // First measure populates cache
        let s1 = t.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(1000.0, 1000.0),
            },
        );

        // Second measure with same width should hit cache
        let s2 = t.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(1000.0, 1000.0),
            },
        );

        assert_eq!(s1, s2, "cached measurement should match");
    }

    #[test]
    fn text_layout_sets_bounds() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut t = Text::new("Hello");
        let bounds = Rect::new(10.0, 20.0, 100.0, 30.0);
        t.layout(&mut cx, bounds);
        assert_eq!(t.cached_bounds, bounds);
    }

    #[test]
    fn text_clone() {
        let t1 = Text::new("Hello").font_size(20.0);
        let t2 = t1.clone();
        assert_eq!(t1.content, t2.content);
        assert_eq!(t1.font_size, t2.font_size);
    }

    #[test]
    fn text_debug_format() {
        let t = Text::new("Hello").font_size(16.0);
        let debug = format!("{:?}", t);
        assert!(debug.contains("Text"));
        assert!(debug.contains("Hello"));
    }
}
