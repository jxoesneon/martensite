//! `AnalogClock` — a clock-face display widget (Qt `QAnalogClock`
//! example, WinUI community-toolkit clock).
//!
//! Hour/minute tick marks on a dial face with hour, minute, and
//! optional second hands. The widget is display-only and driven —
//! the app sets the time (e.g. from a timer tick); nothing in the
//! widget reads the wall clock, keeping tests deterministic.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::analog_clock::AnalogClock;
//!
//! let clock = AnalogClock::new().time(10, 9, 30);
//! assert_eq!(clock.time_value(), (10, 9, 30));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

const SIZE_PT: f32 = 120.0;
const SURFACE: [u8; 4] = [250, 250, 252, 255];
const BORDER: [u8; 4] = [200, 202, 206, 255];
const TICK: [u8; 4] = [120, 122, 128, 255];
const HANDS: [u8; 4] = [40, 40, 44, 255];
const SECOND: [u8; 4] = [220, 80, 60, 255];

/// A driven clock-face display — see the module docs.
///
/// ```
/// use martensite::widgets::analog_clock::AnalogClock;
///
/// let clock = AnalogClock::new();
/// assert_eq!(clock.time_value(), (0, 0, 0));
/// ```
pub struct AnalogClock {
    /// When `false` the hands render muted (no interaction either
    /// way — the widget is display-only).
    pub enabled: bool,
    /// Whether the thin second hand renders.
    pub show_seconds: bool,
    hour: u8,
    minute: u8,
    second: u8,
    bounds: Rect,
}

impl Default for AnalogClock {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalogClock {
    /// Creates a clock at 00:00:00.
    ///
    /// ```
    /// use martensite::widgets::analog_clock::AnalogClock;
    ///
    /// assert_eq!(AnalogClock::new().time_value(), (0, 0, 0));
    /// ```
    pub fn new() -> Self {
        Self {
            enabled: true,
            show_seconds: true,
            hour: 0,
            minute: 0,
            second: 0,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Sets the displayed time (`hour` is `0..24`; 12/24 wrap).
    ///
    /// ```
    /// use martensite::widgets::analog_clock::AnalogClock;
    ///
    /// let clock = AnalogClock::new().time(15, 30, 45);
    /// assert_eq!(clock.time_value(), (15, 30, 45));
    /// ```
    pub fn time(mut self, hour: u8, minute: u8, second: u8) -> Self {
        self.set_time(hour, minute, second);
        self
    }

    /// Mutable set — for a timer-driven app tick.
    ///
    /// ```
    /// use martensite::widgets::analog_clock::AnalogClock;
    ///
    /// let mut clock = AnalogClock::new();
    /// clock.set_time(6, 15, 0);
    /// assert_eq!(clock.time_value(), (6, 15, 0));
    /// ```
    pub fn set_time(&mut self, hour: u8, minute: u8, second: u8) {
        self.hour = hour % 24;
        self.minute = minute % 60;
        self.second = second % 60;
    }

    /// The displayed `(hour, minute, second)`.
    ///
    /// ```
    /// use martensite::widgets::analog_clock::AnalogClock;
    ///
    /// assert_eq!(AnalogClock::new().time(25, 61, 0).time_value(), (1, 1, 0));
    /// ```
    pub fn time_value(&self) -> (u8, u8, u8) {
        (self.hour, self.minute, self.second)
    }

    /// Whether the second hand renders.
    ///
    /// ```
    /// use martensite::widgets::analog_clock::AnalogClock;
    ///
    /// let clock = AnalogClock::new().show_seconds(false);
    /// assert!(!clock.show_seconds);
    /// ```
    pub fn show_seconds(mut self, show: bool) -> Self {
        self.show_seconds = show;
        self
    }

    /// Enables or disables (mutes) the face.
    ///
    /// ```
    /// use martensite::widgets::analog_clock::AnalogClock;
    ///
    /// let clock = AnalogClock::new().enabled(false);
    /// assert!(!clock.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Clockwise angle from 12 o'clock for a `fraction` of a turn.
    fn hand_angle(fraction: f32) -> f32 {
        fraction * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2
    }
}

impl Widget for AnalogClock {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(SIZE_PT);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(32.0, 32.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label("Clock");
        node.set_value(format!(
            "{:02}:{:02}:{:02}",
            self.hour, self.minute, self.second
        ));
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let dim = self.bounds.width().min(self.bounds.height());
        if dim <= 0.0 {
            return;
        }
        let center = Vec2::new(
            self.bounds.origin.x + self.bounds.size.x / 2.0,
            self.bounds.origin.y + self.bounds.size.y / 2.0,
        );
        let r = dim / 2.0 - cx.pt(2.0);
        let face = martensite_core::shape::Shape::circle(center, r);
        let face_rect = kurbo::Rect::new(
            f64::from(center.x - r),
            f64::from(center.y - r),
            f64::from(center.x + r),
            f64::from(center.y + r),
        );
        let fade = |c: [u8; 4]| -> [u8; 4] {
            if self.enabled {
                c
            } else {
                [
                    ((c[0] as u16 + 200) / 2) as u8,
                    ((c[1] as u16 + 200) / 2) as u8,
                    ((c[2] as u16 + 200) / 2) as u8,
                    160,
                ]
            }
        };
        cx.list.push_fill_shape(
            face_rect,
            &face,
            cx.color(TokenKey::SurfaceColor, fade(SURFACE)),
        );
        cx.list.push_stroke_shape(
            face_rect,
            &face,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, fade(BORDER)),
        );

        // Tick marks: longer at the hours.
        let tick_color = cx.color(TokenKey::TextMutedColor, fade(TICK));
        for i in 0..60 {
            let hour_tick = i % 5 == 0;
            let len = if hour_tick { r * 0.14 } else { r * 0.06 };
            let w = if hour_tick { cx.pt(1.5) } else { cx.pt(0.75) };
            let a = Self::hand_angle(i as f32 / 60.0);
            let (sin, cos) = a.sin_cos();
            let mut p = kurbo::BezPath::new();
            p.move_to((
                f64::from(center.x + cos * (r - len)),
                f64::from(center.y + sin * (r - len)),
            ));
            p.line_to((
                f64::from(center.x + cos * r * 0.96),
                f64::from(center.y + sin * r * 0.96),
            ));
            cx.list.push_stroke_path(p, w, tick_color);
        }

        // Hands: hour (short+thick), minute (long+mid), second (thin,
        // accent) — plus a tail stub behind center on minute/second.
        let ink = cx.color(TokenKey::TextColor, fade(HANDS));
        let hour_frac = (self.hour % 12) as f32 / 12.0 + self.minute as f32 / 720.0;
        let min_frac = self.minute as f32 / 60.0 + self.second as f32 / 3600.0;
        let sec_frac = self.second as f32 / 60.0;
        let hand =
            |cx: &mut PaintContext, frac: f32, len: f32, tail: f32, w: f32, color: [u8; 4]| {
                let a = Self::hand_angle(frac);
                let (sin, cos) = a.sin_cos();
                let mut p = kurbo::BezPath::new();
                p.move_to((
                    f64::from(center.x - cos * tail),
                    f64::from(center.y - sin * tail),
                ));
                p.line_to((
                    f64::from(center.x + cos * len),
                    f64::from(center.y + sin * len),
                ));
                cx.list.push_stroke_path(p, w, color);
            };
        hand(cx, hour_frac, r * 0.5, 0.0, cx.pt(3.0), ink);
        hand(cx, min_frac, r * 0.74, r * 0.1, cx.pt(2.0), ink);
        if self.show_seconds {
            hand(
                cx,
                sec_frac,
                r * 0.8,
                r * 0.14,
                cx.pt(1.0),
                cx.color(TokenKey::ErrorColor, fade(SECOND)),
            );
        }
        // Center pin.
        let pin_r = cx.pt(2.5);
        let pin = martensite_core::shape::Shape::circle(center, pin_r);
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(center.x - pin_r),
                f64::from(center.y - pin_r),
                f64::from(center.x + pin_r),
                f64::from(center.y + pin_r),
            ),
            &pin,
            ink,
        );
    }
}

impl std::fmt::Debug for AnalogClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnalogClock")
            .field("time", &self.time_value())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn time_wraps() {
        let clock = AnalogClock::new().time(25, 61, 70);
        assert_eq!(clock.time_value(), (1, 1, 10));
    }

    #[test]
    fn set_time_updates() {
        let mut clock = AnalogClock::new();
        clock.set_time(23, 59, 58);
        assert_eq!(clock.time_value(), (23, 59, 58));
        clock.set_time(0, 0, 0);
        assert_eq!(clock.time_value(), (0, 0, 0));
    }

    #[test]
    fn hand_angles_at_quarters() {
        // 12 o'clock points straight up.
        let a = AnalogClock::hand_angle(0.0);
        assert!(a.sin().abs() > 0.99 && a.cos().abs() < 0.01);
        assert!(a < 0.0); // sin(−π/2) = −1 → up in y-down space
                          // Quarter turn points right.
        let a = AnalogClock::hand_angle(0.25);
        assert!(a.cos() > 0.99);
    }

    #[test]
    fn measure_prefers_square() {
        let mut clock = AnalogClock::new();
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let s = clock.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(500.0, 500.0),
            },
        );
        assert_eq!(s.x, s.y);
        assert!(s.x > 100.0);
    }

    #[test]
    fn a11y_value_is_time() {
        let clock = AnalogClock::new().time(9, 5, 3);
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        clock.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Image);
        assert_eq!(node.value(), Some("09:05:03"));
    }
}
