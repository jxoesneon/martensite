//! `CountdownRing` — a circular countdown timer (iOS Clock timer /
//! watchOS workout-ring idiom).
//!
//! Unlike [`Countdown`](crate::widgets::Countdown), which is a
//! whole-second digital readout, the ring tracks fractional
//! remaining time so its arc sweeps smoothly every frame. The arc
//! drains clockwise from 12 o'clock; the center shows `MM:SS`
//! (or `H:MM:SS` past an hour). Space toggles pause, `r` resets to
//! the full duration, and expiry parks a flag in
//! [`CountdownRing::take_finished`]. Under `warn_under` the arc and
//! digits switch to the warning color.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::countdown_ring::CountdownRing;
//! use std::time::Duration;
//!
//! let r = CountdownRing::new(Duration::from_secs(60));
//! assert_eq!(r.fraction(), 1.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use std::time::Duration;

use crate::text_paint::SharedTextPainter;

const SIZE_PT: f32 = 96.0;
const RING_PT: f32 = 7.0;

const TRACK: [u8; 4] = [70, 72, 80, 255];
const ARC: [u8; 4] = [96, 165, 250, 255];
const WARN: [u8; 4] = [248, 113, 113, 255];
const FG: [u8; 4] = [230, 232, 238, 255];
const MUTED: [u8; 4] = [140, 142, 150, 255];

/// A circular countdown — see the module docs.
///
/// ```
/// use martensite::widgets::countdown_ring::CountdownRing;
/// use std::time::Duration;
///
/// assert_eq!(CountdownRing::new(Duration::from_secs(30)).total(), Duration::from_secs(30));
/// ```
pub struct CountdownRing {
    /// Accessibility label.
    pub label: String,
    /// Warn threshold — arc/digits alert under this remaining time.
    pub warn_under: Duration,
    /// When `false` the timer halts.
    pub enabled: bool,
    total: Duration,
    /// Fractional remaining seconds (drives the smooth arc).
    remaining: f32,
    running: bool,
    finished: bool,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
}

impl std::fmt::Debug for CountdownRing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CountdownRing")
            .field("remaining", &self.remaining)
            .field("running", &self.running)
            .finish()
    }
}

impl CountdownRing {
    /// A running countdown ring of `duration`.
    ///
    /// ```
    /// use martensite::widgets::countdown_ring::CountdownRing;
    /// use std::time::Duration;
    ///
    /// let r = CountdownRing::new(Duration::from_secs(45));
    /// assert!(r.is_running());
    /// ```
    pub fn new(duration: Duration) -> Self {
        Self {
            label: "Timer".to_string(),
            warn_under: Duration::from_secs(10),
            enabled: true,
            total: duration,
            remaining: duration.as_secs_f32(),
            running: true,
            finished: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::countdown_ring::CountdownRing;
    /// use std::time::Duration;
    ///
    /// assert_eq!(
    ///     CountdownRing::new(Duration::from_secs(5)).label("Rest").label,
    ///     "Rest"
    /// );
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Warn threshold builder.
    ///
    /// ```
    /// use martensite::widgets::countdown_ring::CountdownRing;
    /// use std::time::Duration;
    ///
    /// let r = CountdownRing::new(Duration::from_secs(5)).warn_under(Duration::from_secs(2));
    /// assert_eq!(r.warn_under, Duration::from_secs(2));
    /// ```
    pub fn warn_under(mut self, d: Duration) -> Self {
        self.warn_under = d;
        self
    }

    /// Starts paused.
    ///
    /// ```
    /// use martensite::widgets::countdown_ring::CountdownRing;
    /// use std::time::Duration;
    ///
    /// assert!(!CountdownRing::new(Duration::from_secs(5)).paused(true).is_running());
    /// ```
    pub fn paused(mut self, paused: bool) -> Self {
        self.running = !paused;
        self
    }

    /// Enables or disables ticking.
    ///
    /// ```
    /// use martensite::widgets::countdown_ring::CountdownRing;
    /// use std::time::Duration;
    ///
    /// assert!(!CountdownRing::new(Duration::from_secs(5)).enabled(false).enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Optional painter override (tests / headless).
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The configured total duration.
    ///
    /// ```
    /// use martensite::widgets::countdown_ring::CountdownRing;
    /// use std::time::Duration;
    ///
    /// assert_eq!(CountdownRing::new(Duration::from_secs(9)).total(), Duration::from_secs(9));
    /// ```
    pub fn total(&self) -> Duration {
        self.total
    }

    /// Remaining time (truncated to whole seconds).
    ///
    /// ```
    /// use martensite::widgets::countdown_ring::CountdownRing;
    /// use std::time::Duration;
    ///
    /// assert_eq!(
    ///     CountdownRing::new(Duration::from_secs(9)).remaining().as_secs(),
    ///     9
    /// );
    /// ```
    pub fn remaining(&self) -> Duration {
        Duration::from_secs_f32(self.remaining.max(0.0))
    }

    /// Remaining fraction of the total `0.0..=1.0` (arc driver).
    ///
    /// ```
    /// use martensite::widgets::countdown_ring::CountdownRing;
    /// use std::time::Duration;
    ///
    /// let mut r = CountdownRing::new(Duration::from_secs(4));
    /// r.set_remaining(Duration::from_secs(1));
    /// assert_eq!(r.fraction(), 0.25);
    /// ```
    pub fn fraction(&self) -> f32 {
        let total = self.total.as_secs_f32();
        if total <= 0.0 {
            return 0.0;
        }
        (self.remaining / total).clamp(0.0, 1.0)
    }

    /// Whether the timer is counting down.
    ///
    /// ```
    /// use martensite::widgets::countdown_ring::CountdownRing;
    /// use std::time::Duration;
    ///
    /// assert!(CountdownRing::new(Duration::from_secs(3)).is_running());
    /// ```
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Whether the countdown has expired.
    ///
    /// ```
    /// use martensite::widgets::countdown_ring::CountdownRing;
    /// use std::time::Duration;
    ///
    /// assert!(!CountdownRing::new(Duration::from_secs(3)).is_finished());
    /// ```
    pub fn is_finished(&self) -> bool {
        self.remaining <= 0.0
    }

    /// Starts or stops the countdown.
    ///
    /// ```
    /// use martensite::widgets::countdown_ring::CountdownRing;
    /// use std::time::Duration;
    ///
    /// let mut r = CountdownRing::new(Duration::from_secs(3));
    /// r.set_running(false);
    /// assert!(!r.is_running());
    /// ```
    pub fn set_running(&mut self, running: bool) {
        self.running = running;
    }

    /// Sets the remaining time directly.
    ///
    /// ```
    /// use martensite::widgets::countdown_ring::CountdownRing;
    /// use std::time::Duration;
    ///
    /// let mut r = CountdownRing::new(Duration::from_secs(10));
    /// r.set_remaining(Duration::from_secs(4));
    /// assert_eq!(r.remaining().as_secs(), 4);
    /// ```
    pub fn set_remaining(&mut self, remaining: Duration) {
        self.remaining = remaining.as_secs_f32().clamp(0.0, self.total.as_secs_f32());
        if self.remaining > 0.0 {
            self.finished = false;
        }
    }

    /// Restarts at the full duration.
    ///
    /// ```
    /// use martensite::widgets::countdown_ring::CountdownRing;
    /// use std::time::Duration;
    ///
    /// let mut r = CountdownRing::new(Duration::from_secs(10));
    /// r.set_remaining(Duration::from_secs(2));
    /// r.reset();
    /// assert_eq!(r.fraction(), 1.0);
    /// ```
    pub fn reset(&mut self) {
        self.remaining = self.total.as_secs_f32();
        self.finished = false;
    }

    /// Drains the expiry flag — fires once per countdown.
    ///
    /// ```
    /// use martensite::widgets::countdown_ring::CountdownRing;
    /// use std::time::Duration;
    ///
    /// assert!(!CountdownRing::new(Duration::from_secs(5)).take_finished());
    /// ```
    pub fn take_finished(&mut self) -> bool {
        std::mem::take(&mut self.finished)
    }

    /// `MM:SS` / `H:MM:SS` readout of the remaining time.
    ///
    /// ```
    /// use martensite::widgets::countdown_ring::CountdownRing;
    /// use std::time::Duration;
    ///
    /// assert_eq!(CountdownRing::new(Duration::from_secs(75)).face(), "01:15");
    /// ```
    pub fn face(&self) -> String {
        let secs = self.remaining.ceil().max(0.0) as u64;
        let h = secs / 3600;
        let m = secs / 60 % 60;
        let s = secs % 60;
        if h > 0 {
            format!("{h}:{m:02}:{s:02}")
        } else {
            format!("{m:02}:{s:02}")
        }
    }
}

impl Widget for CountdownRing {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(SIZE_PT);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(28.0, 28.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        let _ = cx;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Timer);
        node.set_label(format!("{} — {} remaining", self.label, self.face()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } if key == " " => {
                self.running = !self.running;
                EventResponse::Handled
            }
            WidgetEvent::KeyPressed { key, .. } if key == "r" || key == "R" => {
                self.reset();
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, dt: Duration) -> bool {
        if !self.enabled || !self.running || self.remaining <= 0.0 {
            return false;
        }
        self.remaining = (self.remaining - dt.as_secs_f32()).max(0.0);
        if self.remaining <= 0.0 {
            self.finished = true;
        }
        // Always repaint — the arc sweeps sub-second.
        true
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let side = b.width().min(b.height());
        let cxm = (b.min_x() + b.max_x()) / 2.0;
        let cym = (b.min_y() + b.max_y()) / 2.0;
        let ring_w = RING_PT * cx.scale;
        let r = (side - ring_w) / 2.0;
        if r <= 0.0 {
            return;
        }
        let center = Vec2::new(cxm, cym);
        let warn = self.remaining <= self.warn_under.as_secs_f32() && self.remaining > 0.0;
        let expired = self.remaining <= 0.0;
        let arc_color = cx.color(
            if warn || expired {
                TokenKey::ErrorColor
            } else {
                TokenKey::AccentColor
            },
            if warn || expired { WARN } else { ARC },
        );

        // Track ring.
        cx.list.push_stroke_shape(
            kurbo::Rect::new(
                f64::from(cxm - r),
                f64::from(cym - r),
                f64::from(cxm + r),
                f64::from(cym + r),
            ),
            &martensite_core::shape::Shape::ELLIPSE,
            ring_w,
            cx.color(TokenKey::DividerColor, TRACK),
        );
        // Remaining arc: clockwise from 12 o'clock.
        let frac = self.fraction();
        if frac > 0.0 {
            let sweep = frac * std::f32::consts::TAU;
            let mut path = kurbo::BezPath::new();
            let mut i = 0;
            let steps = (sweep / 0.1).ceil().max(2.0) as usize;
            while i <= steps {
                let a = -std::f32::consts::FRAC_PI_2 + sweep * (i as f32 / steps as f32);
                let p = Vec2::new(center.x + r * a.cos(), center.y + r * a.sin());
                if i == 0 {
                    path.move_to(kurbo::Point::new(f64::from(p.x), f64::from(p.y)));
                } else {
                    path.line_to(kurbo::Point::new(f64::from(p.x), f64::from(p.y)));
                }
                i += 1;
            }
            cx.list.push_stroke_path(path, ring_w, arc_color);
        }

        // Center readout.
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size = side * 0.22;
        let face = self.face();
        let fg = if warn || expired {
            arc_color
        } else if self.enabled && self.running {
            cx.color(TokenKey::TextColor, FG)
        } else {
            cx.color(TokenKey::TextMutedColor, MUTED)
        };
        let w = painter
            .and_then(|p| p.measure_text(&face, size))
            .unwrap_or(face.chars().count() as f32 * size * 0.55);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            kurbo::Point::new(f64::from(cxm - w / 2.0), f64::from(cym - size * 0.6)),
            &face,
            size,
            fg,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(r: &mut CountdownRing) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        r.layout(&mut cx, Rect::new(0.0, 0.0, 96.0, 96.0));
    }

    #[test]
    fn tick_sweeps_subsecond() {
        let mut r = CountdownRing::new(Duration::from_secs(10));
        laid_out(&mut r);
        assert!(r.tick(Duration::from_millis(250)));
        assert!((r.fraction() - 0.975).abs() < 1e-4);
    }

    #[test]
    fn expiry_fires_once() {
        let mut r = CountdownRing::new(Duration::from_secs(1));
        laid_out(&mut r);
        r.tick(Duration::from_secs(2));
        assert!(r.is_finished());
        assert!(r.take_finished());
        assert!(!r.take_finished());
        assert!(!r.tick(Duration::from_secs(1)));
    }

    #[test]
    fn pause_and_reset() {
        let mut r = CountdownRing::new(Duration::from_secs(10));
        laid_out(&mut r);
        r.set_running(false);
        assert!(!r.tick(Duration::from_secs(5)));
        assert_eq!(r.remaining().as_secs(), 10);
        r.set_remaining(Duration::from_secs(3));
        r.reset();
        assert_eq!(r.fraction(), 1.0);
    }

    #[test]
    fn space_toggles_and_r_resets() {
        let mut r = CountdownRing::new(Duration::from_secs(10));
        laid_out(&mut r);
        let bounds = r.bounds;
        r.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: " ".to_string(),
                repeat: false,
            },
            bounds,
            scale: 1.0,
        });
        assert!(!r.is_running());
        r.set_remaining(Duration::from_secs(4));
        r.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: "r".to_string(),
                repeat: false,
            },
            bounds,
            scale: 1.0,
        });
        assert_eq!(r.remaining().as_secs(), 10);
    }

    #[test]
    fn face_formats() {
        assert_eq!(CountdownRing::new(Duration::from_secs(75)).face(), "01:15");
        assert_eq!(
            CountdownRing::new(Duration::from_secs(3700)).face(),
            "1:01:40"
        );
        assert_eq!(CountdownRing::new(Duration::ZERO).face(), "00:00");
    }

    #[test]
    fn paint_without_painter() {
        let mut r = CountdownRing::new(Duration::from_secs(30));
        laid_out(&mut r);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        r.paint(&mut PaintContext {
            list: &mut list,
            bounds: r.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
