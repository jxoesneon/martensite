//! Pull-to-refresh wrapper (mobile idiom — `UIRefreshControl`,
//! Android `SwipeRefreshLayout`).
//!
//! Wraps a single scrollable child. A downward drag past the top
//! edge reveals an indicator strip and shifts the child downward by
//! the pull distance; releasing beyond `PTR_THRESHOLD_PT` parks a
//! refresh request (`take_refresh`) and holds the indicator open
//! until the consumer calls [`PullToRefresh::finish_refresh`]. Under
//! the threshold the pull snaps back.
//!
//! The facade cannot know whether the wrapped child is scrolled to
//! its top — that knowledge lives in the child's scroll model — so
//! this widget exposes the raw gesture machinery and lets the
//! consumer gate it: forward the drag only when the child reports
//! top-scroll, or accept the default which arms on any downward
//! drag starting in the top `ARM_ZONE_PT` of the widget.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{PullToRefresh, Text};
//!
//! let p = PullToRefresh::new(Text::new("feed"));
//! assert_eq!(p.pull(), 0.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::paint_label_clipped;

/// Pull distance (points) that arms a refresh on release.
const PTR_THRESHOLD_PT: f32 = 64.0;
/// Top-edge strip (points) where a downward drag may begin.
const ARM_ZONE_PT: f32 = 16.0;
/// Indicator strip height while refreshing (points).
const INDICATOR_PT: f32 = 40.0;
/// Rubber-band resistance applied past the threshold.
const RESIST: f32 = 0.5;

/// Gesture phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// No pull in progress.
    Idle,
    /// Pointer down, accumulating pull distance.
    Pulling,
    /// Threshold crossed — indicator held open for the consumer.
    Refreshing,
}

/// Pull-to-refresh wrapper around one child.
///
/// # Examples
///
/// ```
/// use martensite::core::Widget;
/// use martensite::widgets::{PullToRefresh, Text};
///
/// assert_eq!(PullToRefresh::new(Text::new("x")).child_count(), 1);
/// ```
pub struct PullToRefresh {
    /// Accessibility label override.
    pub label: Option<String>,
    /// Whether the gesture is armed.
    pub enabled: bool,
    /// Indicator text while pulling / refreshing.
    pub pull_text: String,
    /// Text shown once the threshold is crossed.
    pub release_text: String,
    /// Text shown while a refresh is in flight.
    pub refreshing_text: String,
    child: Box<dyn Widget>,
    phase: Phase,
    /// Current pull distance in device px (drives the child offset).
    pull: f32,
    /// Pointer Y at press.
    drag_y: Option<f32>,
    /// Whether the press started inside the arm zone.
    armed: bool,
    refresh: bool,
    bounds: Rect,
    child_bounds: Option<Rect>,
    indicator_bounds: Option<Rect>,
    /// Explicit painter opt-in.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl PullToRefresh {
    /// Wraps `child`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PullToRefresh, Text};
    ///
    /// let p = PullToRefresh::new(Text::new("feed"));
    /// assert!(!p.refreshing());
    /// ```
    pub fn new(child: impl Widget + 'static) -> Self {
        Self {
            label: None,
            enabled: true,
            pull_text: "Pull to refresh".to_string(),
            release_text: "Release to refresh".to_string(),
            refreshing_text: "Refreshing…".to_string(),
            child: Box::new(child),
            phase: Phase::Idle,
            pull: 0.0,
            drag_y: None,
            armed: false,
            refresh: false,
            bounds: Rect::default(),
            child_bounds: None,
            indicator_bounds: None,
            text_painter: None,
        }
    }

    /// Sets the accessibility label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PullToRefresh, Text};
    ///
    /// let p = PullToRefresh::new(Text::new("x")).label("Feed");
    /// assert_eq!(p.label.as_deref(), Some("Feed"));
    /// ```
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// Sets whether the gesture is armed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PullToRefresh, Text};
    ///
    /// assert!(!PullToRefresh::new(Text::new("x")).enabled(false).enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, flag: bool) -> Self {
        self.enabled = flag;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] for the indicator.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Current pull distance in device px.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PullToRefresh, Text};
    ///
    /// assert_eq!(PullToRefresh::new(Text::new("x")).pull(), 0.0);
    /// ```
    #[inline]
    pub fn pull(&self) -> f32 {
        self.pull
    }

    /// Whether a refresh is in flight (indicator held open).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PullToRefresh, Text};
    ///
    /// assert!(!PullToRefresh::new(Text::new("x")).refreshing());
    /// ```
    #[inline]
    pub fn refreshing(&self) -> bool {
        self.phase == Phase::Refreshing
    }

    /// Pull distance that triggers a refresh, in device px.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PullToRefresh, Text};
    ///
    /// assert!(PullToRefresh::new(Text::new("x")).threshold() > 0.0);
    /// ```
    #[inline]
    pub fn threshold(&self) -> f32 {
        PTR_THRESHOLD_PT
    }

    /// Takes the parked refresh request.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PullToRefresh, Text};
    ///
    /// assert!(!PullToRefresh::new(Text::new("x")).take_refresh());
    /// ```
    #[inline]
    pub fn take_refresh(&mut self) -> bool {
        std::mem::take(&mut self.refresh)
    }

    /// Ends the refreshing phase — the consumer calls this after
    /// completing the reload the request asked for.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PullToRefresh, Text};
    ///
    /// let mut p = PullToRefresh::new(Text::new("x"));
    /// p.finish_refresh();
    /// assert!(!p.refreshing());
    /// ```
    pub fn finish_refresh(&mut self) {
        if self.phase == Phase::Refreshing {
            self.phase = Phase::Idle;
            self.pull = 0.0;
            self.update_regions();
        }
    }

    /// Pull amount (0‥1+) normalized to the trigger threshold.
    fn progress(&self, scale: f32) -> f32 {
        self.pull / (PTR_THRESHOLD_PT * scale)
    }

    /// Rubber-band mapping — linear to the threshold, resisted past.
    fn resisted(&self, raw: f32, scale: f32) -> f32 {
        let t = PTR_THRESHOLD_PT * scale;
        if raw <= t {
            raw
        } else {
            t + (raw - t) * RESIST
        }
    }

    /// Recomputes the indicator + child regions from `pull`.
    fn update_regions(&mut self) {
        let b = self.bounds;
        if b.width() <= 0.0 || b.height() <= 0.0 {
            return;
        }
        if self.pull > 0.0 {
            self.indicator_bounds = Some(Rect::new(b.min_x(), b.min_y(), b.width(), self.pull));
            self.child_bounds = Some(Rect::new(
                b.min_x(),
                b.min_y() + self.pull,
                b.width(),
                (b.height() - self.pull).max(0.0),
            ));
        } else {
            self.indicator_bounds = None;
            self.child_bounds = Some(b);
        }
    }

    /// End-of-gesture resolution.
    fn release(&mut self, scale: f32) {
        if self.phase == Phase::Pulling {
            if self.pull >= PTR_THRESHOLD_PT * scale {
                self.phase = Phase::Refreshing;
                self.pull = INDICATOR_PT * scale;
                self.refresh = true;
            } else {
                self.phase = Phase::Idle;
                self.pull = 0.0;
            }
            self.update_regions();
        }
        self.drag_y = None;
        self.armed = false;
    }
}

impl std::fmt::Debug for PullToRefresh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PullToRefresh")
            .field("phase", &self.phase)
            .field("pull", &self.pull)
            .finish()
    }
}

impl Widget for PullToRefresh {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Fill; delegate the interesting measurement to the child.
        let _ = self.child.measure(cx, constraints);
        Vec2::new(
            constraints
                .max_size
                .x
                .max(cx.pt(160.0).min(constraints.max_size.x)),
            constraints
                .max_size
                .y
                .max(cx.pt(120.0).min(constraints.max_size.y)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 96.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.update_regions();
        if let Some(cb) = self.child_bounds {
            cx.layout_child(&mut *self.child, cb);
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        // Indicator strip — accent progress bar + status text.
        if let Some(ind) = self.indicator_bounds {
            if ind.height() > 0.5 {
                let surface = cx.color(TokenKey::SurfaceColor, [45, 45, 48, 255]);
                let accent = cx.color(TokenKey::AccentColor, [0, 122, 204, 255]);
                let muted = cx.color(TokenKey::TextMutedColor, [140, 140, 140, 255]);
                let f = |r: Rect| {
                    kurbo::Rect::new(
                        f64::from(r.min_x()),
                        f64::from(r.min_y()),
                        f64::from(r.max_x()),
                        f64::from(r.max_y()),
                    )
                };
                cx.list.push_fill_rect(f(ind), surface);
                // Progress bar along the strip's bottom edge.
                let prog = self.progress(cx.scale).min(1.0);
                let bar = Rect::new(
                    ind.min_x(),
                    ind.max_y() - cx.pt(2.0),
                    ind.width() * prog.max(0.0),
                    cx.pt(2.0),
                );
                cx.list.push_fill_rect(f(bar), accent);
                let text = match self.phase {
                    Phase::Refreshing => &self.refreshing_text,
                    _ if self.progress(cx.scale) >= 1.0 => &self.release_text,
                    _ => &self.pull_text,
                };
                let size = 11.0 * cx.scale;
                let painter =
                    crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
                let w = painter
                    .and_then(|p| p.measure_text(text, size))
                    .unwrap_or(size * text.chars().count() as f32 * 0.5)
                    .min(ind.width());
                let origin = kurbo::Point::new(
                    f64::from(ind.min_x() + (ind.width() - w) / 2.0),
                    f64::from(ind.min_y() + (ind.height() - size) / 2.0),
                );
                paint_label_clipped(painter, cx.list, f(ind), origin, text, size, muted);
            }
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                position, button, ..
            } => {
                if *button == martensite_core::PointerButton::Primary
                    && self.bounds.contains(*position)
                    && position.y <= self.bounds.min_y() + ARM_ZONE_PT * cx.scale
                {
                    self.drag_y = Some(position.y);
                    self.armed = true;
                    self.phase = Phase::Pulling;
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some(start) = self.drag_y {
                    let raw = (position.y - start).max(0.0);
                    self.pull = self.resisted(raw, cx.scale);
                    self.update_regions();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased { .. } => {
                if self.drag_y.is_some() {
                    self.release(cx.scale);
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            _ => {
                // Everything else routes to the child.
                if let Some(cb) = self.child_bounds {
                    let mut child_cx = EventContext {
                        event: cx.event,
                        bounds: cb,
                        scale: cx.scale,
                    };
                    return self.child.event(&mut child_cx);
                }
                EventResponse::Ignored
            }
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        if !self.enabled {
            node.set_disabled();
        }
        if self.refreshing() {
            node.set_busy();
        }
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&*self.child as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut *self.child as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.child_bounds).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Text;
    use martensite_core::{HotNode, PointerButton, WidgetEvent};

    fn laid_out(w: &mut PullToRefresh, width: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, width, h));
    }

    fn ev(w: &mut PullToRefresh, event: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 300.0, 200.0),
            scale: 1.0,
        };
        w.event(&mut cx)
    }

    fn drag(w: &mut PullToRefresh, dy: f32) {
        let down = WidgetEvent::PointerPressed {
            position: Vec2::new(150.0, 8.0),
            button: PointerButton::Primary,
            count: 1,
        };
        let mv = WidgetEvent::PointerMoved {
            position: Vec2::new(150.0, 8.0 + dy),
        };
        let up = WidgetEvent::PointerReleased {
            position: Vec2::new(150.0, 8.0 + dy),
            button: PointerButton::Primary,
        };
        ev(w, &down);
        ev(w, &mv);
        ev(w, &up);
    }

    #[test]
    fn idle_by_default() {
        let mut p = PullToRefresh::new(Text::new("x"));
        assert_eq!(p.pull(), 0.0);
        assert!(!p.refreshing());
        assert!(!p.take_refresh());
    }

    #[test]
    fn short_pull_snaps_back() {
        let mut p = PullToRefresh::new(Text::new("x"));
        laid_out(&mut p, 300.0, 200.0);
        drag(&mut p, 20.0);
        assert_eq!(p.pull(), 0.0);
        assert!(!p.take_refresh());
    }

    #[test]
    fn full_pull_requests_refresh() {
        let mut p = PullToRefresh::new(Text::new("x"));
        laid_out(&mut p, 300.0, 200.0);
        drag(&mut p, 80.0);
        assert!(p.refreshing());
        assert!(p.take_refresh());
        // Indicator held open at the strip height.
        assert_eq!(p.pull(), INDICATOR_PT);
    }

    #[test]
    fn finish_refresh_closes_indicator() {
        let mut p = PullToRefresh::new(Text::new("x"));
        laid_out(&mut p, 300.0, 200.0);
        drag(&mut p, 80.0);
        p.finish_refresh();
        assert!(!p.refreshing());
        assert_eq!(p.pull(), 0.0);
    }

    #[test]
    fn pull_shifts_child_bounds() {
        let mut p = PullToRefresh::new(Text::new("x"));
        laid_out(&mut p, 300.0, 200.0);
        let down = WidgetEvent::PointerPressed {
            position: Vec2::new(150.0, 8.0),
            button: PointerButton::Primary,
            count: 1,
        };
        let mv = WidgetEvent::PointerMoved {
            position: Vec2::new(150.0, 38.0),
        };
        ev(&mut p, &down);
        ev(&mut p, &mv);
        assert_eq!(p.pull(), 30.0);
        let cb = p.child_bounds(0).unwrap();
        assert_eq!(cb.min_y(), 30.0);
        assert!(p.indicator_bounds.is_some());
    }

    #[test]
    fn press_below_arm_zone_ignored() {
        let mut p = PullToRefresh::new(Text::new("x"));
        laid_out(&mut p, 300.0, 200.0);
        let down = WidgetEvent::PointerPressed {
            position: Vec2::new(150.0, 100.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(ev(&mut p, &down), EventResponse::Ignored);
        let mv = WidgetEvent::PointerMoved {
            position: Vec2::new(150.0, 160.0),
        };
        assert_eq!(ev(&mut p, &mv), EventResponse::Ignored);
        assert_eq!(p.pull(), 0.0);
    }

    #[test]
    fn resisted_past_threshold() {
        let p = PullToRefresh::new(Text::new("x"));
        // 64 + (96-64)*0.5 = 80
        assert_eq!(p.resisted(96.0, 1.0), 80.0);
        assert_eq!(p.resisted(30.0, 1.0), 30.0);
    }

    #[test]
    fn disabled_inert() {
        let mut p = PullToRefresh::new(Text::new("x")).enabled(false);
        laid_out(&mut p, 300.0, 200.0);
        drag(&mut p, 80.0);
        assert_eq!(p.pull(), 0.0);
        assert!(!p.take_refresh());
    }
}
