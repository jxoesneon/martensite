//! `Countdown` — a tick-driven timer display counting down to zero
//! (pomodoro / cycle-time / quiz-timer idiom).
//!
//! The widget needs a `tick` each frame; it decrements
//! `remaining` while running, paints `MM:SS` (or `H:MM:SS` past an
//! hour), flashes the accent when it lapses, and parks a flag in
//! [`Countdown::take_elapsed`] exactly once when it hits zero.
//! `Space` toggles pause while focused.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::countdown::Countdown;
//! use std::time::Duration;
//!
//! let c = Countdown::new(Duration::from_secs(90));
//! assert_eq!(c.remaining().as_secs(), 90);
//! ```

use std::time::Duration;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const WIDTH_PT: f32 = 96.0;
const HEIGHT_PT: f32 = 28.0;

const FG: [u8; 4] = [230, 230, 235, 255];
const MUTED: [u8; 4] = [140, 140, 148, 255];
const ALERT: [u8; 4] = [230, 90, 90, 255];

/// A tick-driven countdown display — see the module docs.
///
/// ```
/// use martensite::widgets::countdown::Countdown;
/// use std::time::Duration;
///
/// let c = Countdown::new(Duration::from_secs(30));
/// assert_eq!(c.remaining().as_secs(), 30);
/// ```
pub struct Countdown {
    /// When `false` the timer halts.
    pub enabled: bool,
    /// Accessibility label.
    pub label: String,
    /// Flash the alert color when the remaining time drops under
    /// this threshold.
    pub warn_under: Duration,
    remaining: Duration,
    carry: f32,
    running: bool,
    elapsed_flag: bool,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
}

impl Countdown {
    /// Creates a running countdown of `duration`.
    ///
    /// ```
    /// use martensite::widgets::countdown::Countdown;
    /// use std::time::Duration;
    ///
    /// let c = Countdown::new(Duration::from_secs(60));
    /// assert_eq!(c.remaining().as_secs(), 60);
    /// ```
    pub fn new(duration: Duration) -> Self {
        Self {
            enabled: true,
            label: "Countdown".to_string(),
            warn_under: Duration::from_secs(10),
            remaining: duration,
            carry: 0.0,
            running: true,
            elapsed_flag: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::countdown::Countdown;
    /// use std::time::Duration;
    ///
    /// let c = Countdown::new(Duration::from_secs(5)).label("Break");
    /// assert_eq!(c.label, "Break");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Warn threshold — the alert color under this remaining time.
    ///
    /// ```
    /// use martensite::widgets::countdown::Countdown;
    /// use std::time::Duration;
    ///
    /// let c = Countdown::new(Duration::from_secs(5))
    ///     .warn_under(Duration::from_secs(2));
    /// assert_eq!(c.warn_under, Duration::from_secs(2));
    /// ```
    pub fn warn_under(mut self, d: Duration) -> Self {
        self.warn_under = d;
        self
    }

    /// Starts paused.
    ///
    /// ```
    /// use martensite::widgets::countdown::Countdown;
    /// use std::time::Duration;
    ///
    /// let c = Countdown::new(Duration::from_secs(5)).paused(true);
    /// assert!(!c.is_running());
    /// ```
    pub fn paused(mut self, paused: bool) -> Self {
        self.running = !paused;
        self
    }

    /// Enables or disables ticking.
    ///
    /// ```
    /// use martensite::widgets::countdown::Countdown;
    /// use std::time::Duration;
    ///
    /// let c = Countdown::new(Duration::from_secs(5)).enabled(false);
    /// assert!(!c.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::countdown::Countdown;
    /// use martensite::text_paint::shared_painter;
    /// use std::time::Duration;
    ///
    /// let c = Countdown::new(Duration::from_secs(5))
    ///     .with_text_painter(shared_painter());
    /// assert_eq!(c.remaining().as_secs(), 5);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Time remaining.
    ///
    /// ```
    /// use martensite::widgets::countdown::Countdown;
    /// use std::time::Duration;
    ///
    /// assert_eq!(Countdown::new(Duration::from_secs(7)).remaining().as_secs(), 7);
    /// ```
    pub fn remaining(&self) -> Duration {
        self.remaining
    }

    /// Whether the timer is counting down.
    ///
    /// ```
    /// use martensite::widgets::countdown::Countdown;
    /// use std::time::Duration;
    ///
    /// assert!(Countdown::new(Duration::from_secs(1)).is_running());
    /// ```
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Pauses or resumes.
    ///
    /// ```
    /// use martensite::widgets::countdown::Countdown;
    /// use std::time::Duration;
    ///
    /// let mut c = Countdown::new(Duration::from_secs(5));
    /// c.set_running(false);
    /// assert!(!c.is_running());
    /// ```
    pub fn set_running(&mut self, running: bool) {
        self.running = running;
    }

    /// Resets to a new duration and resumes.
    ///
    /// ```
    /// use martensite::widgets::countdown::Countdown;
    /// use std::time::Duration;
    ///
    /// let mut c = Countdown::new(Duration::from_secs(1));
    /// c.reset(Duration::from_secs(10));
    /// assert_eq!(c.remaining().as_secs(), 10);
    /// ```
    pub fn reset(&mut self, duration: Duration) {
        self.remaining = duration;
        self.carry = 0.0;
        self.running = true;
        self.elapsed_flag = false;
    }

    /// Drains the one-shot elapsed flag (fires once at zero).
    ///
    /// ```
    /// use martensite::widgets::countdown::Countdown;
    /// use std::time::Duration;
    ///
    /// let mut c = Countdown::new(Duration::from_secs(1));
    /// assert!(!c.take_elapsed());
    /// ```
    pub fn take_elapsed(&mut self) -> bool {
        std::mem::take(&mut self.elapsed_flag)
    }

    /// `MM:SS` (or `H:MM:SS`) text for the remaining time.
    fn face(&self) -> String {
        let s = self.remaining.as_secs();
        let (h, m, sec) = (s / 3600, (s / 60) % 60, s % 60);
        if h > 0 {
            format!("{h}:{m:02}:{sec:02}")
        } else {
            format!("{m:02}:{sec:02}")
        }
    }
}

impl Widget for Countdown {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(40.0, 14.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Label);
        node.set_label(format!("{} — {}", self.label, self.face()));
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
                self.running = !self.running;
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, dt: Duration) -> bool {
        if !self.enabled || !self.running || self.remaining.is_zero() {
            return false;
        }
        self.carry += dt.as_secs_f32();
        if self.carry >= 1.0 {
            let whole = self.carry.floor() as u64;
            self.carry -= whole as f32;
            self.remaining = self.remaining.saturating_sub(Duration::from_secs(whole));
            if self.remaining.is_zero() {
                self.elapsed_flag = true;
            }
            true
        } else {
            false
        }
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
        let size = 13.0 * cx.scale;
        let color = if self.remaining.is_zero() {
            cx.color(TokenKey::ErrorColor, ALERT)
        } else if self.remaining <= self.warn_under {
            cx.color(TokenKey::WarningColor, ALERT)
        } else if self.enabled {
            cx.color(TokenKey::TextColor, FG)
        } else {
            cx.color(TokenKey::TextMutedColor, MUTED)
        };
        let face = self.face();
        let w = painter
            .and_then(|p| p.measure_text(&face, size))
            .unwrap_or(face.chars().count() as f32 * size * 0.6);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            f(self.bounds),
            kurbo::Point::new(
                f64::from(self.bounds.min_x() + (self.bounds.width() - w).max(0.0) / 2.0),
                f64::from(self.bounds.min_y() + (self.bounds.height() - size * 1.2) / 2.0),
            ),
            &face,
            size,
            color,
        );
    }
}

impl std::fmt::Debug for Countdown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Countdown")
            .field("remaining", &self.remaining)
            .field("running", &self.running)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut Countdown) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 96.0, 28.0));
    }

    #[test]
    fn tick_decrements_whole_seconds() {
        let mut c = Countdown::new(Duration::from_secs(90));
        laid_out(&mut c);
        assert!(c.tick(Duration::from_secs(1)));
        assert_eq!(c.remaining().as_secs(), 89);
    }

    #[test]
    fn subsecond_ticks_accumulate() {
        let mut c = Countdown::new(Duration::from_secs(10));
        laid_out(&mut c);
        assert!(!c.tick(Duration::from_millis(400)));
        assert!(c.tick(Duration::from_millis(700)));
        assert_eq!(c.remaining().as_secs(), 9);
    }

    #[test]
    fn zero_fires_elapsed_once() {
        let mut c = Countdown::new(Duration::from_secs(1));
        laid_out(&mut c);
        c.tick(Duration::from_secs(1));
        assert!(c.take_elapsed());
        assert!(!c.take_elapsed());
        assert!(!c.tick(Duration::from_secs(1))); // stays at zero
    }

    #[test]
    fn pause_halts() {
        let mut c = Countdown::new(Duration::from_secs(10));
        laid_out(&mut c);
        c.set_running(false);
        assert!(!c.tick(Duration::from_secs(1)));
        assert_eq!(c.remaining().as_secs(), 10);
    }

    #[test]
    fn space_toggles() {
        let mut c = Countdown::new(Duration::from_secs(10));
        laid_out(&mut c);
        c.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: " ".to_string(),
                repeat: false,
            },
            bounds: Rect::new(0.0, 0.0, 96.0, 28.0),
            scale: 1.0,
        });
        assert!(!c.is_running());
    }

    #[test]
    fn face_formats() {
        assert_eq!(Countdown::new(Duration::from_secs(75)).face(), "01:15");
        assert_eq!(Countdown::new(Duration::from_secs(3700)).face(), "1:01:40");
        assert_eq!(Countdown::new(Duration::ZERO).face(), "00:00");
    }
}
