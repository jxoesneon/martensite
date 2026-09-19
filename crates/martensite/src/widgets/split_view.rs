//! `SplitView` widget: a two-pane container with a draggable divider
//! (Qt `QSplitter`, AppKit `NSSplitViewController`, WinUI `SplitView`
//! pane/content split).
//!
//! Two children share the allocation at `ratio` — drag the divider
//! (or arrow-key when focused) to resize; double-click the divider
//! to reset to `default_ratio`. Minimum fractions clamp each pane.
//! Poll [`SplitView::take_moved`] for drag-driven ratio changes.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::split_view::{SplitView, SplitOrientation};
//! use martensite::widgets::container::Container;
//!
//! let s = SplitView::horizontal()
//!     .first(Container::new())
//!     .second(Container::new())
//!     .ratio(0.3);
//! assert_eq!(s.get_ratio(), 0.3);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

/// Divider hit area, logical points.
const DIVIDER_PT: f32 = 6.0;
/// Arrow-key ratio step.
const KEY_STEP: f32 = 0.05;
/// Divider ink.
const DIVIDER_INK: [u8; 4] = [200, 203, 210, 255];
/// Hovered/dragging divider ink.
const DIVIDER_HOT: [u8; 4] = [70, 110, 200, 255];

/// Split axis.
///
/// # Examples
///
/// ```
/// use martensite::widgets::split_view::SplitOrientation;
///
/// assert_ne!(SplitOrientation::Horizontal, SplitOrientation::Vertical);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SplitOrientation {
    /// Panes side by side — the divider is a vertical bar.
    #[default]
    Horizontal,
    /// Panes stacked — the divider is a horizontal bar.
    Vertical,
}

/// A two-pane resizable split container.
///
/// # Examples
///
/// ```
/// use martensite::widgets::split_view::SplitView;
///
/// let s = SplitView::vertical().ratio(0.5);
/// ```
pub struct SplitView {
    /// Split axis.
    orientation: SplitOrientation,
    /// First pane (left/top).
    first: Option<Box<dyn Widget>>,
    /// Second pane (right/bottom).
    second: Option<Box<dyn Widget>>,
    /// Current fraction given to the first pane (0..1).
    ratio: f32,
    /// Ratio restored on divider double-click.
    default_ratio: f32,
    /// Minimum fraction for the first pane.
    min_first: f32,
    /// Minimum fraction for the second pane.
    min_second: f32,
    /// Whether the divider is hovered.
    highlighted: bool,
    /// Drag state.
    dragging: bool,
    /// Pending move notification.
    moved: Option<f32>,
    /// Enabled flag.
    enabled: bool,
    /// Divider rect from the last layout.
    divider_rect: Rect,
    /// Cached bounds.
    bounds: Rect,
}

impl SplitView {
    /// Creates a horizontal (side-by-side) split.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::split_view::SplitView;
    ///
    /// let s = SplitView::horizontal();
    /// assert_eq!(s.get_ratio(), 0.5);
    /// ```
    #[must_use]
    pub fn horizontal() -> Self {
        Self::new(SplitOrientation::Horizontal)
    }

    /// Creates a vertical (stacked) split.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::split_view::SplitView;
    ///
    /// let s = SplitView::vertical();
    /// ```
    #[must_use]
    pub fn vertical() -> Self {
        Self::new(SplitOrientation::Vertical)
    }

    /// Creates a split with the given orientation.
    #[must_use]
    fn new(orientation: SplitOrientation) -> Self {
        Self {
            orientation,
            first: None,
            second: None,
            ratio: 0.5,
            default_ratio: 0.5,
            min_first: 0.0,
            min_second: 0.0,
            highlighted: false,
            dragging: false,
            moved: None,
            enabled: true,
            divider_rect: Rect::default(),
            bounds: Rect::default(),
        }
    }

    /// Sets the first (left/top) pane.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::split_view::SplitView;
    /// use martensite::widgets::container::Container;
    ///
    /// let s = SplitView::horizontal().first(Container::new());
    /// ```
    #[must_use]
    pub fn first(mut self, child: impl Widget + 'static) -> Self {
        self.first = Some(Box::new(child));
        self
    }

    /// Sets the second (right/bottom) pane.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::split_view::SplitView;
    /// use martensite::widgets::container::Container;
    ///
    /// let s = SplitView::horizontal().second(Container::new());
    /// ```
    #[must_use]
    pub fn second(mut self, child: impl Widget + 'static) -> Self {
        self.second = Some(Box::new(child));
        self
    }

    /// Sets the split ratio (fraction for the first pane, `0..=1`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::split_view::SplitView;
    ///
    /// let s = SplitView::horizontal().ratio(0.25);
    /// ```
    #[must_use]
    pub fn ratio(mut self, ratio: f32) -> Self {
        self.ratio = ratio.clamp(0.0, 1.0);
        self.default_ratio = self.ratio;
        self
    }

    /// Sets the double-click-reset ratio (default = construction
    /// ratio).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::split_view::SplitView;
    ///
    /// let s = SplitView::horizontal().default_ratio(0.4);
    /// ```
    #[must_use]
    pub fn default_ratio(mut self, ratio: f32) -> Self {
        self.default_ratio = ratio.clamp(0.0, 1.0);
        self
    }

    /// Sets minimum pane fractions — `min` applies to each side.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::split_view::SplitView;
    ///
    /// let s = SplitView::horizontal().minimums(0.1);
    /// ```
    #[must_use]
    pub fn minimums(mut self, min: f32) -> Self {
        self.min_first = min.clamp(0.0, 0.5);
        self.min_second = self.min_first;
        self
    }

    /// Enables or disables divider interaction.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::split_view::SplitView;
    ///
    /// let s = SplitView::horizontal().enabled(false);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The current ratio.
    #[inline]
    #[must_use]
    pub fn get_ratio(&self) -> f32 {
        self.ratio
    }

    /// Sets the ratio programmatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::split_view::SplitView;
    ///
    /// let mut s = SplitView::horizontal();
    /// s.set_ratio(0.7);
    /// assert_eq!(s.get_ratio(), 0.7);
    /// ```
    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = ratio.clamp(self.min_first, 1.0 - self.min_second);
    }

    /// Drains a divider-move notification — the new ratio.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::split_view::SplitView;
    ///
    /// let mut s = SplitView::horizontal();
    /// assert_eq!(s.take_moved(), None);
    /// ```
    pub fn take_moved(&mut self) -> Option<f32> {
        self.moved.take()
    }

    /// Applies a user-driven ratio (clamped, notified).
    fn commit(&mut self, ratio: f32) {
        let clamped = ratio.clamp(self.min_first, 1.0 - self.min_second);
        if (clamped - self.ratio).abs() > f32::EPSILON {
            self.ratio = clamped;
            self.moved = Some(clamped);
        }
    }

    /// Divider rect at a given ratio (recomputes for hit-testing).
    fn divider_at(&self, scale: f32) -> Rect {
        let d = scale * DIVIDER_PT;
        let b = self.bounds;
        match self.orientation {
            SplitOrientation::Horizontal => {
                let x = b.origin.x + b.size.x * self.ratio - d / 2.0;
                Rect::new(x, b.origin.y, d, b.size.y)
            }
            SplitOrientation::Vertical => {
                let y = b.origin.y + b.size.y * self.ratio - d / 2.0;
                Rect::new(b.origin.x, y, b.size.x, d)
            }
        }
    }
}

impl Default for SplitView {
    fn default() -> Self {
        Self::horizontal()
    }
}

impl Widget for SplitView {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Two-pane minimums: both panes want their minimums side by
        // side (or stacked) plus the divider.
        let d = cx.pt(DIVIDER_PT);
        let (w1, h1) = self
            .first
            .as_mut()
            .map(|c| {
                let s = c.measure(cx, constraints);
                (s.x, s.y)
            })
            .unwrap_or((0.0, 0.0));
        let (w2, h2) = self
            .second
            .as_mut()
            .map(|c| {
                let s = c.measure(cx, constraints);
                (s.x, s.y)
            })
            .unwrap_or((0.0, 0.0));
        match self.orientation {
            SplitOrientation::Horizontal => Vec2::new(
                (w1 + d + w2).min(constraints.max_size.x.max(0.0)),
                h1.max(h2),
            ),
            SplitOrientation::Vertical => Vec2::new(
                w1.max(w2),
                (h1 + d + h2).min(constraints.max_size.y.max(0.0)),
            ),
        }
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.divider_rect = self.divider_at(cx.scale);
        let d = cx.pt(DIVIDER_PT);
        let (a, b) = match self.orientation {
            SplitOrientation::Horizontal => {
                let w = (bounds.size.x - d) * self.ratio;
                (
                    Rect::new(bounds.origin.x, bounds.origin.y, w.max(0.0), bounds.size.y),
                    Rect::new(
                        bounds.origin.x + w + d,
                        bounds.origin.y,
                        (bounds.size.x - w - d).max(0.0),
                        bounds.size.y,
                    ),
                )
            }
            SplitOrientation::Vertical => {
                let h = (bounds.size.y - d) * self.ratio;
                (
                    Rect::new(bounds.origin.x, bounds.origin.y, bounds.size.x, h.max(0.0)),
                    Rect::new(
                        bounds.origin.x,
                        bounds.origin.y + h + d,
                        bounds.size.x,
                        (bounds.size.y - h - d).max(0.0),
                    ),
                )
            }
        };
        if let Some(c) = self.first.as_mut() {
            cx.layout_child(c.as_mut(), a);
        }
        if let Some(c) = self.second.as_mut() {
            cx.layout_child(c.as_mut(), b);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Splitter);
        node.set_numeric_value(f64::from(self.ratio) * 100.0);
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(100.0);
        if !self.enabled {
            node.set_disabled();
        }
        if self.enabled {
            node.add_action(accesskit::Action::SetValue);
            node.add_action(accesskit::Action::Increment);
            node.add_action(accesskit::Action::Decrement);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                if self.dragging {
                    let frac = match self.orientation {
                        SplitOrientation::Horizontal => {
                            (position.x - self.bounds.origin.x) / self.bounds.size.x
                        }
                        SplitOrientation::Vertical => {
                            (position.y - self.bounds.origin.y) / self.bounds.size.y
                        }
                    };
                    self.commit(frac);
                    return EventResponse::RequestRepaint;
                }
                let hot = self.divider_at(cx.scale).contains(*position);
                if hot != self.highlighted {
                    self.highlighted = hot;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if !self.dragging {
                    self.highlighted = false;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                count,
            } => {
                if self.divider_at(cx.scale).contains(*position) {
                    if *count == 2 {
                        // Double-click resets to the default split.
                        self.commit(self.default_ratio);
                        return EventResponse::RequestRepaint;
                    }
                    self.dragging = true;
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased { .. } => {
                if self.dragging {
                    self.dragging = false;
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => {
                let step = KEY_STEP;
                match key.as_str() {
                    "ArrowLeft" | "ArrowUp" => {
                        self.commit(self.ratio - step);
                        EventResponse::Handled
                    }
                    "ArrowRight" | "ArrowDown" => {
                        self.commit(self.ratio + step);
                        EventResponse::Handled
                    }
                    "Home" => {
                        self.commit(0.0);
                        EventResponse::Handled
                    }
                    "End" => {
                        self.commit(1.0);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            }
            WidgetEvent::SemanticAction(action) => match action {
                martensite_core::widget::SemanticAction::Increment => {
                    self.commit(self.ratio + KEY_STEP);
                    EventResponse::Handled
                }
                martensite_core::widget::SemanticAction::Decrement => {
                    self.commit(self.ratio - KEY_STEP);
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let r = self.divider_at(cx.scale);
        let ink = if self.highlighted || self.dragging {
            cx.color(TokenKey::AccentColor, DIVIDER_HOT)
        } else {
            cx.color(TokenKey::DividerColor, DIVIDER_INK)
        };
        // Paint the center stripe only — the hit zone is wider than
        // the visual grip (standard splitter affordance).
        let stripe = match self.orientation {
            SplitOrientation::Horizontal => kurbo::Rect::new(
                f64::from(r.origin.x + r.size.x / 2.0 - cx.pt(1.0)),
                f64::from(r.min_y()),
                f64::from(r.origin.x + r.size.x / 2.0 + cx.pt(1.0)),
                f64::from(r.max_y()),
            ),
            SplitOrientation::Vertical => kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.origin.y + r.size.y / 2.0 - cx.pt(1.0)),
                f64::from(r.max_x()),
                f64::from(r.origin.y + r.size.y / 2.0 + cx.pt(1.0)),
            ),
        };
        cx.list.push_fill_rect(stripe, ink);
    }

    fn child_count(&self) -> usize {
        self.first.is_some() as usize + self.second.is_some() as usize
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        match (index, &self.first, &self.second) {
            (0, Some(f), _) => Some(f.as_ref()),
            (1, _, Some(s)) | (0, None, Some(s)) => Some(s.as_ref()),
            _ => None,
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        match (index, &mut self.first, &mut self.second) {
            (0, Some(f), _) => Some(f.as_mut()),
            (1, _, Some(s)) | (0, None, Some(s)) => Some(s.as_mut()),
            _ => None,
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        let d = DIVIDER_PT; // logical — bounds are already scaled at
                            // call time in production; the layout pass stored real rects
                            // only through layout_child, so recompute the same split.
        let b = self.bounds;
        let first_rect = match self.orientation {
            SplitOrientation::Horizontal => {
                let w = (b.size.x - d) * self.ratio;
                Rect::new(b.origin.x, b.origin.y, w.max(0.0), b.size.y)
            }
            SplitOrientation::Vertical => {
                let h = (b.size.y - d) * self.ratio;
                Rect::new(b.origin.x, b.origin.y, b.size.x, h.max(0.0))
            }
        };
        let second_rect = match self.orientation {
            SplitOrientation::Horizontal => {
                let w = (b.size.x - d) * self.ratio;
                Rect::new(
                    b.origin.x + w + d,
                    b.origin.y,
                    (b.size.x - w - d).max(0.0),
                    b.size.y,
                )
            }
            SplitOrientation::Vertical => {
                let h = (b.size.y - d) * self.ratio;
                Rect::new(
                    b.origin.x,
                    b.origin.y + h + d,
                    b.size.x,
                    (b.size.y - h - d).max(0.0),
                )
            }
        };
        match (index, &self.first, &self.second) {
            (0, Some(_), _) => Some(first_rect),
            (1, _, Some(_)) | (0, None, Some(_)) => Some(second_rect),
            _ => None,
        }
    }
}

impl std::fmt::Debug for SplitView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitView")
            .field("ratio", &self.ratio)
            .field("orientation", &self.orientation)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::container::Container;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 400.0, 300.0),
            scale: 1.0,
        }
    }

    fn view() -> SplitView {
        let mut s = SplitView::horizontal()
            .first(Container::new())
            .second(Container::new());
        let mut hot = HotNode::default();
        s.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 400.0, 300.0));
        s
    }

    #[test]
    fn builder_and_children() {
        let s = view();
        assert_eq!(Widget::child_count(&s), 2);
        assert_eq!(s.get_ratio(), 0.5);
    }

    #[test]
    fn divider_hit_grabs_pointer() {
        let mut s = view();
        let d = s.divider_at(1.0);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(d.origin.x + 1.0, d.origin.y + 5.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(s.event(&mut ev(&press)), EventResponse::CapturePointer);
        assert!(s.dragging);
    }

    #[test]
    fn drag_moves_ratio() {
        let mut s = view();
        let d = s.divider_at(1.0);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(d.origin.x + 1.0, d.origin.y + 5.0),
            button: PointerButton::Primary,
            count: 1,
        };
        s.event(&mut ev(&press));
        let mv = WidgetEvent::PointerMoved {
            position: Vec2::new(300.0, 150.0), // 75% across
        };
        s.event(&mut ev(&mv));
        assert!((s.get_ratio() - 0.75).abs() < 0.001);
        assert_eq!(s.take_moved(), Some(s.get_ratio()));
    }

    #[test]
    fn double_click_resets() {
        let mut s = view();
        s.set_ratio(0.8);
        let d = s.divider_at(1.0);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(d.origin.x + 1.0, d.origin.y + 5.0),
            button: PointerButton::Primary,
            count: 2,
        };
        s.event(&mut ev(&press));
        assert_eq!(s.get_ratio(), 0.5);
    }

    #[test]
    fn minimums_clamp() {
        let mut s = SplitView::horizontal().minimums(0.2);
        s.set_ratio(0.0);
        assert_eq!(s.get_ratio(), 0.2);
        s.set_ratio(1.0);
        assert_eq!(s.get_ratio(), 0.8);
    }

    #[test]
    fn arrows_step() {
        let mut s = view();
        let right = WidgetEvent::KeyPressed {
            key: "ArrowRight".into(),
            repeat: false,
        };
        s.event(&mut ev(&right));
        assert!((s.get_ratio() - 0.55).abs() < 0.001);
    }

    #[test]
    fn vertical_split_hit_zone() {
        let mut s = SplitView::vertical()
            .first(Container::new())
            .second(Container::new());
        let mut hot = HotNode::default();
        s.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 400.0, 300.0));
        let d = s.divider_at(1.0);
        // Divider is a horizontal bar mid-height.
        assert!((d.origin.y - 147.0).abs() < 4.0);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(10.0, d.origin.y + 1.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(s.event(&mut ev(&press)), EventResponse::CapturePointer);
    }

    #[test]
    fn pane_press_falls_through() {
        let mut s = view();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(20.0, 20.0), // inside first pane, off divider
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(s.event(&mut ev(&press)), EventResponse::Ignored);
    }

    #[test]
    fn disabled_inert() {
        let mut s = view();
        s.enabled = false;
        let d = s.divider_at(1.0);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(d.origin.x + 1.0, d.origin.y + 5.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(s.event(&mut ev(&press)), EventResponse::Ignored);
    }
}
