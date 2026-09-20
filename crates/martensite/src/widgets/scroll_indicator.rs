//! `ScrollIndicator` — an iOS-style overlay scroll thumb: a thin
//! rounded pill at the track edge that flashes on scroll and fades
//! out while idle.
//!
//! Unlike the interactive scrollbars inside
//! [`ScrollView`](crate::widgets::ScrollView), this is display-only:
//! the host feeds [`ScrollIndicator::set_scroll`] with the viewport
//! position and visible fraction (e.g. on every scroll event, which
//! also calls [`ScrollIndicator::flash`]), and `tick` decays the
//! opacity. No hit-testing — the indicator ignores pointer input.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::scroll_indicator::ScrollIndicator;
//!
//! let i = ScrollIndicator::vertical().scroll(0.5, 0.25);
//! assert_eq!(i.scroll_fraction(), 0.5);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;
use std::time::Duration;

use crate::widgets::split_view::SplitOrientation;

const THICK_PT: f32 = 3.0;
const INSET_PT: f32 = 2.0;
const MIN_THUMB_FRAC: f32 = 0.08;
const HOLD_SECS: f32 = 0.4;
const FADE_SECS: f32 = 0.5;

const THUMB: [u8; 4] = [200, 202, 210, 180];

/// An overlay scroll thumb — see the module docs.
///
/// ```
/// use martensite::widgets::scroll_indicator::ScrollIndicator;
///
/// assert_eq!(ScrollIndicator::vertical().scroll_fraction(), 0.0);
/// ```
pub struct ScrollIndicator {
    /// Accessibility label.
    pub label: String,
    orientation: SplitOrientation,
    /// Scroll position `0..=1` (0 = top/left).
    scroll: f32,
    /// Viewport fraction of the content `0..=1` (thumb length).
    visible: f32,
    /// Current opacity `0..=1` (1 while scrolling).
    opacity: f32,
    /// Seconds left at full opacity before fading.
    hold: f32,
    /// Stays visible when `true` (macOS "always" scrollbars).
    always: bool,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for ScrollIndicator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollIndicator")
            .field("scroll", &self.scroll)
            .field("visible", &self.visible)
            .field("opacity", &self.opacity)
            .finish()
    }
}

impl ScrollIndicator {
    /// A vertical thumb pinned to the right edge.
    ///
    /// ```
    /// use martensite::widgets::scroll_indicator::ScrollIndicator;
    ///
    /// assert_eq!(ScrollIndicator::vertical().opacity(), 0.0);
    /// ```
    pub fn vertical() -> Self {
        Self::new(SplitOrientation::Vertical)
    }

    /// A horizontal thumb pinned to the bottom edge.
    ///
    /// ```
    /// use martensite::widgets::scroll_indicator::ScrollIndicator;
    ///
    /// assert_eq!(ScrollIndicator::horizontal().opacity(), 0.0);
    /// ```
    pub fn horizontal() -> Self {
        Self::new(SplitOrientation::Horizontal)
    }

    /// An indicator for the given axis. `Vertical` paints a
    /// vertical bar (right edge); `Horizontal` a horizontal bar
    /// (bottom edge).
    ///
    /// ```
    /// use martensite::widgets::scroll_indicator::ScrollIndicator;
    /// use martensite::widgets::split_view::SplitOrientation;
    ///
    /// let i = ScrollIndicator::new(SplitOrientation::Horizontal);
    /// assert_eq!(i.orientation(), SplitOrientation::Horizontal);
    /// ```
    pub fn new(orientation: SplitOrientation) -> Self {
        Self {
            label: "Scroll position".to_string(),
            orientation,
            scroll: 0.0,
            visible: 1.0,
            opacity: 0.0,
            hold: 0.0,
            always: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::scroll_indicator::ScrollIndicator;
    ///
    /// assert_eq!(ScrollIndicator::vertical().label("Chat").label, "Chat");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Always-visible builder (no fade).
    ///
    /// ```
    /// use martensite::widgets::scroll_indicator::ScrollIndicator;
    ///
    /// assert_eq!(ScrollIndicator::vertical().always_visible(true).opacity(), 1.0);
    /// ```
    pub fn always_visible(mut self, always: bool) -> Self {
        self.always = always;
        self.opacity = if always { 1.0 } else { self.opacity };
        self
    }

    /// Initial scroll state — `position` `0..=1`, `visible_fraction`
    /// `0..=1`.
    ///
    /// ```
    /// use martensite::widgets::scroll_indicator::ScrollIndicator;
    ///
    /// let i = ScrollIndicator::vertical().scroll(0.25, 0.5);
    /// assert_eq!(i.scroll_fraction(), 0.25);
    /// assert_eq!(i.visible_fraction(), 0.5);
    /// ```
    pub fn scroll(mut self, position: f32, visible_fraction: f32) -> Self {
        self.scroll = position.clamp(0.0, 1.0);
        self.visible = visible_fraction.clamp(0.0, 1.0);
        self
    }

    /// Feeds new scroll state from the host. Call on every scroll
    /// event (with [`ScrollIndicator::flash`]) to keep the thumb
    /// current and visible.
    ///
    /// ```
    /// use martensite::widgets::scroll_indicator::ScrollIndicator;
    ///
    /// let mut i = ScrollIndicator::vertical();
    /// i.set_scroll(0.75, 0.2);
    /// assert_eq!(i.scroll_fraction(), 0.75);
    /// ```
    pub fn set_scroll(&mut self, position: f32, visible_fraction: f32) {
        self.scroll = position.clamp(0.0, 1.0);
        self.visible = visible_fraction.clamp(0.0, 1.0);
    }

    /// Shows the thumb at full opacity and re-arms the fade timer.
    ///
    /// ```
    /// use martensite::widgets::scroll_indicator::ScrollIndicator;
    ///
    /// let mut i = ScrollIndicator::vertical();
    /// i.flash();
    /// assert_eq!(i.opacity(), 1.0);
    /// ```
    pub fn flash(&mut self) {
        self.opacity = 1.0;
        self.hold = HOLD_SECS;
    }

    /// Current scroll position `0..=1`.
    ///
    /// ```
    /// use martensite::widgets::scroll_indicator::ScrollIndicator;
    ///
    /// assert_eq!(ScrollIndicator::vertical().scroll_fraction(), 0.0);
    /// ```
    pub fn scroll_fraction(&self) -> f32 {
        self.scroll
    }

    /// Viewport fraction of the content `0..=1` (thumb length).
    ///
    /// ```
    /// use martensite::widgets::scroll_indicator::ScrollIndicator;
    ///
    /// assert_eq!(ScrollIndicator::vertical().visible_fraction(), 1.0);
    /// ```
    pub fn visible_fraction(&self) -> f32 {
        self.visible
    }

    /// Current opacity `0..=1`.
    ///
    /// ```
    /// use martensite::widgets::scroll_indicator::ScrollIndicator;
    ///
    /// assert_eq!(ScrollIndicator::vertical().opacity(), 0.0);
    /// ```
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// The bar orientation.
    ///
    /// ```
    /// use martensite::widgets::scroll_indicator::ScrollIndicator;
    /// use martensite::widgets::split_view::SplitOrientation;
    ///
    /// assert_eq!(
    ///     ScrollIndicator::vertical().orientation(),
    ///     SplitOrientation::Vertical
    /// );
    /// ```
    pub fn orientation(&self) -> SplitOrientation {
        self.orientation
    }

    /// The thumb rect given the current scroll state — exposed for
    /// tests and hosts compositing their own chrome.
    ///
    /// ```
    /// use martensite::widgets::scroll_indicator::ScrollIndicator;
    ///
    /// // Zero-length before layout.
    /// assert_eq!(ScrollIndicator::vertical().thumb_rect().height(), 0.0);
    /// ```
    pub fn thumb_rect(&self) -> Rect {
        let s = self.scale;
        let thick = THICK_PT * s;
        let inset = INSET_PT * s;
        let b = self.bounds;
        match self.orientation {
            SplitOrientation::Vertical => {
                let track = (b.height() - inset * 2.0).max(0.0);
                let len = (track * self.visible)
                    .max(track * MIN_THUMB_FRAC)
                    .min(track);
                let travel = (track - len).max(0.0);
                let y = b.min_y() + inset + travel * self.scroll;
                Rect::new(b.max_x() - inset - thick, y, thick, len)
            }
            SplitOrientation::Horizontal => {
                let track = (b.width() - inset * 2.0).max(0.0);
                let len = (track * self.visible)
                    .max(track * MIN_THUMB_FRAC)
                    .min(track);
                let travel = (track - len).max(0.0);
                let x = b.min_x() + inset + travel * self.scroll;
                Rect::new(x, b.max_y() - inset - thick, len, thick)
            }
        }
    }
}

impl Widget for ScrollIndicator {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Overlays fill the scrollable's bounds.
        let _ = cx;
        Vec2::new(
            constraints.max_size.x.max(0.0),
            constraints.max_size.y.max(0.0),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(4.0, 4.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ScrollBar);
        node.set_label(self.label.clone());
        node.set_numeric_value(f64::from(self.scroll));
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        // Pure overlay — never hit.
        EventResponse::Ignored
    }

    fn tick(&mut self, dt: Duration) -> bool {
        if self.always || self.opacity <= 0.0 {
            return false;
        }
        let secs = dt.as_secs_f32();
        if self.hold > 0.0 {
            self.hold = (self.hold - secs).max(0.0);
            return true;
        }
        self.opacity = (self.opacity - secs / FADE_SECS).max(0.0);
        true
    }

    fn paint(&self, cx: &mut PaintContext) {
        let alpha = if self.always { 1.0 } else { self.opacity };
        if alpha <= 0.0 || self.visible >= 1.0 {
            return;
        }
        let r = self.thumb_rect();
        if r.width() <= 0.0 || r.height() <= 0.0 {
            return;
        }
        let mut color = cx.color(TokenKey::TextMutedColor, THUMB);
        color[3] = (f32::from(color[3]) * alpha) as u8;
        let radius = r.width().min(r.height()) / 2.0;
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            ),
            &martensite_core::shape::Shape::rounded(radius),
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(i: &mut ScrollIndicator, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        i.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn thumb_tracks_scroll() {
        let mut i = ScrollIndicator::vertical().scroll(0.0, 0.5);
        laid_out(&mut i, 200.0, 200.0);
        let top = i.thumb_rect();
        i.set_scroll(1.0, 0.5);
        let bottom = i.thumb_rect();
        assert!(bottom.min_y() > top.min_y());
        assert_eq!(bottom.max_y(), 198.0); // inset from the bottom edge
    }

    #[test]
    fn flash_then_fade() {
        let mut i = ScrollIndicator::vertical().scroll(0.5, 0.5);
        laid_out(&mut i, 200.0, 200.0);
        i.flash();
        assert_eq!(i.opacity(), 1.0);
        // Hold period — still full opacity.
        i.tick(Duration::from_millis(200));
        assert_eq!(i.opacity(), 1.0);
        // After the hold, opacity decays.
        i.tick(Duration::from_millis(400)); // drains hold (0.4s)
        i.tick(Duration::from_millis(250)); // half the fade
        assert!(i.opacity() < 1.0 && i.opacity() > 0.0);
        i.tick(Duration::from_secs(1));
        assert_eq!(i.opacity(), 0.0);
    }

    #[test]
    fn always_visible_stays() {
        let mut i = ScrollIndicator::vertical().always_visible(true);
        laid_out(&mut i, 200.0, 200.0);
        i.tick(Duration::from_secs(10));
        assert_eq!(i.opacity(), 1.0);
    }

    #[test]
    fn full_content_hides_thumb() {
        let mut i = ScrollIndicator::vertical()
            .always_visible(true)
            .scroll(0.0, 1.0);
        laid_out(&mut i, 200.0, 200.0);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        i.paint(&mut PaintContext {
            list: &mut list,
            bounds: i.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(list.is_empty());
        // Scrollable content paints a thumb.
        i.set_scroll(0.0, 0.5);
        i.paint(&mut PaintContext {
            list: &mut list,
            bounds: i.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }

    #[test]
    fn horizontal_uses_bottom_edge() {
        let mut i = ScrollIndicator::horizontal().scroll(1.0, 0.25);
        laid_out(&mut i, 200.0, 200.0);
        let r = i.thumb_rect();
        assert_eq!(r.max_x(), 198.0);
        assert!(r.height() < r.width());
    }
}
