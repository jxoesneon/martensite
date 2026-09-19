//! `Marquee` — a horizontally scrolling text ticker (LED marquee /
//! news-crawl idiom).
//!
//! The text slides left by `speed` device px per second inside the
//! widget's clip; when the tail clears the left edge the head wraps
//! back in from the right after a configurable gap. Without a text
//! painter the scroll still advances (width falls back to a
//! character estimate). `Space` toggles pause while focused.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::marquee::Marquee;
//!
//! let m = Marquee::new("Breaking news").speed(60.0);
//! assert_eq!(m.text(), "Breaking news");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const HEIGHT_PT: f32 = 22.0;
const SPEED_PT: f32 = 60.0;
const GAP_PT: f32 = 40.0;

const FG: [u8; 4] = [230, 230, 235, 255];

/// A horizontally scrolling ticker — see the module docs.
///
/// ```
/// use martensite::widgets::marquee::Marquee;
///
/// let m = Marquee::new("crawl");
/// assert_eq!(m.text(), "crawl");
/// ```
pub struct Marquee {
    /// When `false` scrolling halts.
    pub enabled: bool,
    /// Scroll speed in device px per second.
    pub speed: f32,
    /// Gap between tail-clear and head-wrap, in pt.
    pub gap: f32,
    text: String,
    offset: f32,
    paused: bool,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
}

impl Marquee {
    /// Creates a ticker scrolling at the default speed.
    ///
    /// ```
    /// use martensite::widgets::marquee::Marquee;
    ///
    /// assert_eq!(Marquee::new("x").text(), "x");
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            enabled: true,
            speed: SPEED_PT,
            gap: GAP_PT,
            text: text.into(),
            offset: 0.0,
            paused: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
        }
    }

    /// Scroll speed in device px per second.
    ///
    /// ```
    /// use martensite::widgets::marquee::Marquee;
    ///
    /// let m = Marquee::new("x").speed(120.0);
    /// assert_eq!(m.speed, 120.0);
    /// ```
    pub fn speed(mut self, px_per_sec: f32) -> Self {
        self.speed = px_per_sec.max(0.0);
        self
    }

    /// Wrap gap in pt.
    ///
    /// ```
    /// use martensite::widgets::marquee::Marquee;
    ///
    /// let m = Marquee::new("x").gap(80.0);
    /// assert_eq!(m.gap, 80.0);
    /// ```
    pub fn gap(mut self, pt: f32) -> Self {
        self.gap = pt.max(0.0);
        self
    }

    /// Enables or disables scrolling.
    ///
    /// ```
    /// use martensite::widgets::marquee::Marquee;
    ///
    /// assert!(!Marquee::new("x").enabled(false).enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::marquee::Marquee;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let m = Marquee::new("x").with_text_painter(shared_painter());
    /// assert_eq!(m.text(), "x");
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The scrolling text.
    ///
    /// ```
    /// use martensite::widgets::marquee::Marquee;
    ///
    /// assert_eq!(Marquee::new("crawl").text(), "crawl");
    /// ```
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the text and rewinds the scroll.
    ///
    /// ```
    /// use martensite::widgets::marquee::Marquee;
    ///
    /// let mut m = Marquee::new("old");
    /// m.set_text("new");
    /// assert_eq!(m.text(), "new");
    /// ```
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.offset = 0.0;
    }

    /// Whether scrolling is paused (via `Space`).
    ///
    /// ```
    /// use martensite::widgets::marquee::Marquee;
    ///
    /// assert!(!Marquee::new("x").is_paused());
    /// ```
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Pauses or resumes scrolling.
    ///
    /// ```
    /// use martensite::widgets::marquee::Marquee;
    ///
    /// let mut m = Marquee::new("x");
    /// m.set_paused(true);
    /// assert!(m.is_paused());
    /// ```
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Scroll offset in device px.
    ///
    /// ```
    /// use martensite::widgets::marquee::Marquee;
    ///
    /// assert_eq!(Marquee::new("x").scroll_offset(), 0.0);
    /// ```
    pub fn scroll_offset(&self) -> f32 {
        self.offset
    }

    /// Text advance width — measured or a char estimate.
    fn text_width(
        &self,
        painter: Option<&dyn martensite_core::paint::TextShaper>,
        scale: f32,
    ) -> f32 {
        let size = 11.0 * scale;
        painter
            .and_then(|p| p.measure_text(&self.text, size))
            .unwrap_or(self.text.chars().count() as f32 * size * 0.55)
    }
}

impl Widget for Marquee {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(cx.pt(80.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(48.0, 12.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Label);
        node.set_label(self.text.clone());
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } if key == " " => {
                self.paused = !self.paused;
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, dt: std::time::Duration) -> bool {
        if !self.enabled || self.paused || self.bounds.width() <= 0.0 {
            return false;
        }
        self.offset += self.speed * dt.as_secs_f32();
        // Wrap once the tail clears the left edge.
        let width = self.text_width(None, 1.0) + self.bounds.width() + 100.0;
        if self.offset > width {
            self.offset = -(self.bounds.width().max(0.0));
        }
        true
    }

    fn paint(&self, cx: &mut PaintContext) {
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size = 11.0 * cx.scale;
        let clip = f(self.bounds);
        cx.list.push_clip(clip);
        let x = self.bounds.max_x() - self.offset;
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            clip,
            kurbo::Point::new(
                f64::from(x),
                f64::from(self.bounds.min_y() + (self.bounds.height() - size * 1.2) / 2.0),
            ),
            &self.text,
            size,
            cx.color(TokenKey::TextColor, FG),
        );
        cx.list.pop_clip();
    }
}

impl std::fmt::Debug for Marquee {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Marquee")
            .field("text", &self.text)
            .field("offset", &self.offset)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;
    use std::time::Duration;

    fn laid_out(m: &mut Marquee, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        m.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        m.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn tick_advances_offset() {
        let mut m = Marquee::new("hello");
        laid_out(&mut m, 200.0, 22.0);
        assert!(m.tick(Duration::from_millis(500)));
        assert!(m.scroll_offset() > 0.0);
    }

    #[test]
    fn paused_tick_still() {
        let mut m = Marquee::new("hello");
        laid_out(&mut m, 200.0, 22.0);
        m.set_paused(true);
        assert!(!m.tick(Duration::from_millis(500)));
        assert_eq!(m.scroll_offset(), 0.0);
    }

    #[test]
    fn disabled_tick_still() {
        let mut m = Marquee::new("hello").enabled(false);
        laid_out(&mut m, 200.0, 22.0);
        assert!(!m.tick(Duration::from_millis(500)));
    }

    #[test]
    fn space_toggles_pause() {
        let mut m = Marquee::new("hello");
        laid_out(&mut m, 200.0, 22.0);
        m.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: " ".to_string(),
                repeat: false,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 22.0),
            scale: 1.0,
        });
        assert!(m.is_paused());
    }

    #[test]
    fn set_text_rewinds() {
        let mut m = Marquee::new("a");
        laid_out(&mut m, 200.0, 22.0);
        m.tick(Duration::from_millis(100));
        m.set_text("b");
        assert_eq!(m.scroll_offset(), 0.0);
        assert_eq!(m.text(), "b");
    }

    #[test]
    fn offset_wraps() {
        let mut m = Marquee::new("x");
        laid_out(&mut m, 200.0, 22.0);
        m.speed = 1_000_000.0;
        m.tick(Duration::from_secs(1));
        assert!(m.scroll_offset() < 0.0);
    }
}
