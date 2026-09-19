//! Watermark — a tiled text layer for stamping "Confidential",
//! "Draft", or user-identifying marks over content.
//!
//! Mirrors Ant Design `Watermark`. This is a **leaf** widget — stack
//! it above content with [`crate::widgets::Stack`] (later children
//! paint on top):
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::Watermark;
//!
//! let wm = Watermark::new("CONFIDENTIAL").gap(160.0, 100.0);
//! assert_eq!(wm.text(), "CONFIDENTIAL");
//! ```
//!
//! Tiles stagger every other row by half a tile (the watermark
//! signature); a true diagonal rotation needs a paint-transform
//! command the `PaintList` doesn't expose yet.

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

/// Default tile pitch in points.
const TILE_W_PT: f32 = 180.0;
/// Default vertical pitch in points.
const TILE_H_PT: f32 = 110.0;
/// Glyph size in points.
const FONT_PT: f32 = 14.0;

/// A tiled text overlay.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Watermark;
///
/// let wm = Watermark::new("Draft").opacity(0.15);
/// assert!((wm.opacity - 0.15).abs() < f32::EPSILON);
/// ```
pub struct Watermark {
    /// The stamped text.
    pub text: String,
    /// Tile opacity multiplier over the muted ink (0.0–1.0).
    pub opacity: f32,
    /// Whether the widget is enabled.
    pub enabled: bool,
    /// Horizontal/vertical tile pitch in points.
    gap: Vec2,
    /// Origin offset in points.
    offset: Vec2,
    /// Glyph size in points.
    font_size: f32,
    /// Shared shaped-text painter — see [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Watermark {
    /// Creates a watermark stamping `text`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Watermark;
    ///
    /// assert_eq!(Watermark::new("a").text, "a");
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            opacity: 0.16,
            enabled: true,
            gap: Vec2::new(TILE_W_PT, TILE_H_PT),
            offset: Vec2::ZERO,
            font_size: FONT_PT,
            text_painter: None,
        }
    }

    /// Sets the tile pitch in points.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Watermark;
    ///
    /// let wm = Watermark::new("a").gap(200.0, 120.0);
    /// assert_eq!(wm.tile_gap(), (200.0, 120.0));
    /// ```
    #[must_use]
    pub fn gap(mut self, x: f32, y: f32) -> Self {
        self.gap = Vec2::new(x.max(20.0), y.max(20.0));
        self
    }

    /// Sets the origin offset in points.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Watermark;
    ///
    /// let wm = Watermark::new("a").offset(10.0, 20.0);
    /// assert_eq!(wm.tile_offset(), (10.0, 20.0));
    /// ```
    #[must_use]
    pub fn offset(mut self, x: f32, y: f32) -> Self {
        self.offset = Vec2::new(x, y);
        self
    }

    /// Sets the tile opacity (0.0–1.0).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Watermark;
    ///
    /// assert!((Watermark::new("a").opacity(0.5).opacity - 0.5).abs() < f32::EPSILON);
    /// ```
    #[must_use]
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// Sets the glyph size in points.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Watermark;
    ///
    /// let wm = Watermark::new("a").font_size(18.0);
    /// assert!((wm.get_font_size() - 18.0).abs() < f32::EPSILON);
    /// ```
    #[must_use]
    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size.max(6.0);
        self
    }

    /// Sets whether the widget is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Watermark;
    ///
    /// let wm = Watermark::new("a").enabled(false);
    /// assert!(!wm.enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so tiles emit real
    /// glyph runs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The stamped text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Watermark;
    ///
    /// assert_eq!(Watermark::new("a").text(), "a");
    /// ```
    #[inline]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The tile pitch `(x, y)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Watermark;
    ///
    /// assert_eq!(Watermark::new("a").tile_gap(), (180.0, 110.0));
    /// ```
    #[inline]
    pub fn tile_gap(&self) -> (f32, f32) {
        (self.gap.x, self.gap.y)
    }

    /// The origin offset `(x, y)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Watermark;
    ///
    /// assert_eq!(Watermark::new("a").tile_offset(), (0.0, 0.0));
    /// ```
    #[inline]
    pub fn tile_offset(&self) -> (f32, f32) {
        (self.offset.x, self.offset.y)
    }

    /// The glyph size in points.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Watermark;
    ///
    /// assert!((Watermark::new("a").get_font_size() - 14.0).abs() < f32::EPSILON);
    /// ```
    #[inline]
    pub fn get_font_size(&self) -> f32 {
        self.font_size
    }
}

impl Default for Watermark {
    fn default() -> Self {
        Self::new("")
    }
}

impl std::fmt::Debug for Watermark {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watermark")
            .field("text", &self.text)
            .field("opacity", &self.opacity)
            .field("gap", &self.gap)
            .finish()
    }
}

impl Widget for Watermark {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // An overlay fills whatever space it's given.
        constraints.max_size.max(Vec2::ZERO)
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(0.0, 0.0)).with_policy(UnderflowPolicy::Allow)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        // Decorative — hidden from the AT tree like Ant's aria-hidden
        // watermark layer.
        node.set_role(accesskit::Role::GenericContainer);
        node.set_hidden();
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        // Stamps must never eat input meant for the content beneath.
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        if self.text.is_empty() || self.opacity <= 0.0 {
            return;
        }
        let b = cx.bounds;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let base = cx.color(TokenKey::TextMutedColor, [110, 110, 118, 255]);
        let col = [
            base[0],
            base[1],
            base[2],
            (f32::from(base[3]) * self.opacity) as u8,
        ];
        let font_px = cx.pt(self.font_size);
        let gap_x = cx.pt(self.gap.x);
        let gap_y = cx.pt(self.gap.y);
        let ox = b.min_x() + cx.pt(self.offset.x);
        let oy = b.min_y() + cx.pt(self.offset.y);
        let clip = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let mut row = 0usize;
        let mut y = oy;
        while y < b.max_y() {
            // Staggered signature: odd rows shift right half a tile.
            let mut x = ox + if row % 2 == 1 { gap_x / 2.0 } else { 0.0 };
            while x < b.max_x() {
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    clip,
                    kurbo::Point::new(f64::from(x), f64::from(y)),
                    &self.text,
                    font_px,
                    col,
                );
                x += gap_x;
            }
            y += gap_y;
            row += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::Theme;

    #[test]
    fn builder() {
        let wm = Watermark::new("CONF")
            .gap(200.0, 120.0)
            .offset(5.0, 10.0)
            .opacity(0.3)
            .font_size(16.0)
            .enabled(false);
        assert_eq!(wm.text(), "CONF");
        assert_eq!(wm.tile_gap(), (200.0, 120.0));
        assert_eq!(wm.tile_offset(), (5.0, 10.0));
        assert!((wm.opacity - 0.3).abs() < f32::EPSILON);
        assert!((wm.get_font_size() - 16.0).abs() < f32::EPSILON);
        assert!(!wm.enabled);
    }

    #[test]
    fn clamps() {
        let wm = Watermark::new("a")
            .opacity(2.0)
            .font_size(1.0)
            .gap(1.0, 1.0);
        assert!((wm.opacity - 1.0).abs() < f32::EPSILON);
        assert!((wm.get_font_size() - 6.0).abs() < f32::EPSILON);
        assert_eq!(wm.tile_gap(), (20.0, 20.0));
    }

    #[test]
    fn empty_text_paints_nothing() {
        let wm = Watermark::new("");
        let theme = Theme::new("test");
        let mut list = martensite_core::paint::PaintList::default();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: Rect::new(0.0, 0.0, 300.0, 200.0),
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        };
        wm.paint(&mut cx);
        assert!(list.is_empty());
    }

    #[test]
    fn tiles_across_bounds() {
        let wm = Watermark::new("X").gap(40.0, 40.0);
        let theme = Theme::new("test");
        let mut list = martensite_core::paint::PaintList::default();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: Rect::new(0.0, 0.0, 200.0, 120.0),
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        };
        wm.paint(&mut cx);
        assert!(!list.is_empty());
    }

    #[test]
    fn never_eats_input() {
        let mut wm = Watermark::new("a");
        let ev = martensite_core::WidgetEvent::PointerPressed {
            position: Vec2::new(5.0, 5.0),
            button: martensite_core::PointerButton::Primary,
            count: 1,
        };
        let mut cx = EventContext {
            event: &ev,
            bounds: Rect::new(0.0, 0.0, 100.0, 60.0),
            scale: 1.0,
        };
        assert_eq!(wm.event(&mut cx), EventResponse::Ignored);
    }

    #[test]
    fn hidden_from_accessibility() {
        let wm = Watermark::new("a");
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        wm.accessibility(&mut node);
        assert!(node.is_hidden());
    }
}
