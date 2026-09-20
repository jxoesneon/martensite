//! `DigitalClock` — a seven-segment-style digital time display
//! (the textual sibling of [`crate::widgets::analog_clock::AnalogClock`]
//! and [`crate::widgets::countdown::Countdown`]).
//!
//! The clock is app-driven: set the displayed time with
//! [`DigitalClock::time`] (the same `Time` value
//! [`crate::widgets::time_picker::TimePicker`] edits), or advance
//! it automatically — [`DigitalClock::running`] makes each
//! [`Widget::tick`](martensite_core::widget::Widget::tick) accumulate
//! elapsed time. `hh:mm` is the
//! default; [`DigitalClock::show_seconds`] adds `:ss` and
//! [`DigitalClock::blink`] toggles the colon each half-second.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::digital_clock::DigitalClock;
//! use martensite::widgets::Time;
//!
//! let c = DigitalClock::new().time(Time { hour: 9, minute: 30 });
//! assert_eq!(c.text(), "09:30");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    LayoutConstraints, LayoutContext, PaintContext, Rect, RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;
use std::time::Duration;

use crate::widgets::Time;

const PAD_PT: f32 = 8.0;
const FONT_PT: f32 = 18.0;
const DIGIT_PT: f32 = 10.0;
const COLON_PT: f32 = 6.0;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const DIGIT: [u8; 4] = [110, 220, 140, 255];
const DIM: [u8; 4] = [110, 220, 140, 90];

/// A digital time display — see the module docs.
///
/// ```
/// use martensite::widgets::digital_clock::DigitalClock;
///
/// assert_eq!(DigitalClock::new().text(), "00:00");
/// ```
#[derive(Debug)]
pub struct DigitalClock {
    /// Accessibility label.
    pub label: String,
    /// Displayed time of day.
    time: Time,
    /// Accumulated seconds beyond `time` while running.
    carried: f32,
    /// Whether `tick` advances the clock.
    running: bool,
    /// Whether `:ss` is rendered.
    show_seconds: bool,
    /// 12- or 24-hour display.
    h12: bool,
    /// Whether the colon blinks each half-second.
    blink: bool,
    /// Sub-second accumulator driving the blink.
    blink_acc: f32,
    bounds: Rect,
    scale: f32,
}

impl Default for DigitalClock {
    fn default() -> Self {
        Self::new()
    }
}

impl DigitalClock {
    /// Creates a stopped `00:00` clock.
    ///
    /// ```
    /// use martensite::widgets::digital_clock::DigitalClock;
    ///
    /// assert_eq!(DigitalClock::new().text(), "00:00");
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Clock".to_string(),
            time: Time { hour: 0, minute: 0 },
            carried: 0.0,
            running: false,
            show_seconds: false,
            h12: false,
            blink: false,
            blink_acc: 0.0,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets the displayed time.
    ///
    /// ```
    /// use martensite::widgets::digital_clock::DigitalClock;
    /// use martensite::widgets::Time;
    ///
    /// assert_eq!(DigitalClock::new().time(Time { hour: 23, minute: 5 }).text(), "23:05");
    /// ```
    pub fn time(mut self, time: Time) -> Self {
        self.time = time;
        self.carried = 0.0;
        self
    }

    /// Starts or stops automatic advancement on `tick`.
    ///
    /// ```
    /// use martensite::widgets::digital_clock::DigitalClock;
    ///
    /// assert!(DigitalClock::new().running(true).is_running());
    /// ```
    pub fn running(mut self, on: bool) -> Self {
        self.running = on;
        self
    }

    /// Renders `:ss` after the minutes.
    ///
    /// ```
    /// use martensite::widgets::digital_clock::DigitalClock;
    ///
    /// assert!(DigitalClock::new().show_seconds(true).has_seconds());
    /// ```
    pub fn show_seconds(mut self, on: bool) -> Self {
        self.show_seconds = on;
        self
    }

    /// Switches to 12-hour display with an AM/PM suffix.
    ///
    /// ```
    /// use martensite::widgets::digital_clock::DigitalClock;
    /// use martensite::widgets::Time;
    ///
    /// let c = DigitalClock::new().hour12(true).time(Time { hour: 15, minute: 30 });
    /// assert_eq!(c.text(), "03:30 PM");
    /// ```
    pub fn hour12(mut self, on: bool) -> Self {
        self.h12 = on;
        self
    }

    /// Toggles the colon off and on each half-second.
    ///
    /// ```
    /// use martensite::widgets::digital_clock::DigitalClock;
    ///
    /// assert!(DigitalClock::new().blink(true).has_blink());
    /// ```
    pub fn blink(mut self, on: bool) -> Self {
        self.blink = on;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::digital_clock::DigitalClock;
    ///
    /// assert_eq!(DigitalClock::new().label("Shift").label, "Shift");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Current display text.
    ///
    /// ```
    /// use martensite::widgets::digital_clock::DigitalClock;
    /// use martensite::widgets::Time;
    ///
    /// let c = DigitalClock::new().time(Time { hour: 7, minute: 9 });
    /// assert_eq!(c.text(), "07:09");
    /// ```
    pub fn text(&self) -> String {
        let (hour, minute) = self.display_hm();
        let secs = self.carried as u32 % 60;
        let mut out = if self.h12 {
            let h12 = if hour % 12 == 0 { 12 } else { hour % 12 };
            format!("{:02}:{:02}", h12, minute)
        } else {
            format!("{:02}:{:02}", hour, minute)
        };
        if self.show_seconds {
            out.push_str(&format!(":{:02}", secs));
        }
        if self.h12 {
            out.push_str(if hour < 12 { " AM" } else { " PM" });
        }
        out
    }

    /// Current `Time` (whole seconds carried are folded in).
    ///
    /// ```
    /// use martensite::widgets::digital_clock::DigitalClock;
    /// use martensite::widgets::Time;
    ///
    /// let c = DigitalClock::new().time(Time { hour: 8, minute: 15 });
    /// assert_eq!(c.time_value(), Time { hour: 8, minute: 15 });
    /// ```
    pub fn time_value(&self) -> Time {
        let (hour, minute) = self.display_hm();
        Time { hour, minute }
    }

    /// Whether the clock auto-advances on `tick`.
    ///
    /// ```
    /// use martensite::widgets::digital_clock::DigitalClock;
    ///
    /// assert!(!DigitalClock::new().is_running());
    /// ```
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Whether seconds are rendered.
    ///
    /// ```
    /// use martensite::widgets::digital_clock::DigitalClock;
    ///
    /// assert!(!DigitalClock::new().has_seconds());
    /// ```
    pub fn has_seconds(&self) -> bool {
        self.show_seconds
    }

    /// Whether the colon blink is enabled.
    ///
    /// ```
    /// use martensite::widgets::digital_clock::DigitalClock;
    ///
    /// assert!(!DigitalClock::new().has_blink());
    /// ```
    pub fn has_blink(&self) -> bool {
        self.blink
    }

    /// Whether the colon is currently lit (for blink rendering).
    ///
    /// ```
    /// use martensite::widgets::digital_clock::DigitalClock;
    ///
    /// assert!(DigitalClock::new().colon_lit());
    /// ```
    pub fn colon_lit(&self) -> bool {
        !self.blink || self.blink_acc % 1.0 < 0.5
    }

    /// Hour/minute after folding carried whole seconds into `time`.
    fn display_hm(&self) -> (u32, u32) {
        let base = self.time.hour * 3600 + self.time.minute * 60;
        let total = (base + self.carried as u32) % 86_400;
        (total / 3600, total % 3600 / 60)
    }
}

impl Widget for DigitalClock {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // "HH:MM" (+":ss") (+" AM") — monospace-ish char estimate.
        let chars = self.text().chars().count() as f32;
        let w = chars * cx.pt(DIGIT_PT) + 2.0 * cx.pt(PAD_PT);
        let h = cx.pt(FONT_PT) + 2.0 * cx.pt(PAD_PT);
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(48.0, 20.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Time);
        node.set_label(format!("{} — {}", self.label, self.text()));
    }

    fn tick(&mut self, dt: Duration) -> bool {
        let mut changed = false;
        if self.blink {
            self.blink_acc += dt.as_secs_f32();
            changed = true;
        }
        if self.running {
            self.carried += dt.as_secs_f32();
            changed = true;
        }
        changed
    }

    fn paint(&self, cx: &mut PaintContext) {
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        // Monospace-style block digits drawn as filled slots.
        let text = self.text();
        let pad = PAD_PT * self.scale;
        let digit_w = DIGIT_PT * self.scale;
        let colon_w = COLON_PT * self.scale;
        let h = self.bounds.height() - 2.0 * pad;
        let mut x = self.bounds.min_x() + pad;
        let top = self.bounds.min_y() + pad;
        let colon_lit = self.colon_lit();
        for ch in text.chars() {
            if ch == ':' {
                // Colon = two dots, dimmed when blink phase is off.
                let c = if colon_lit { DIGIT } else { DIM };
                let dot = colon_w * 0.4;
                let cy = top + h / 2.0;
                for dy in [-h * 0.15, h * 0.15] {
                    cx.list.push_fill_shape(
                        kurbo::Rect::new(
                            f64::from(x + colon_w / 2.0 - dot / 2.0),
                            f64::from(cy + dy - dot / 2.0),
                            f64::from(x + colon_w / 2.0 + dot / 2.0),
                            f64::from(cy + dy + dot / 2.0),
                        ),
                        &martensite_core::shape::Shape::rounded(dot / 2.0),
                        c,
                    );
                }
                x += colon_w;
            } else {
                let w = if ch.is_ascii_digit() {
                    digit_w
                } else {
                    digit_w * 0.6
                };
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(x),
                        f64::from(top),
                        f64::from(x + w),
                        f64::from(top + h),
                    ),
                    &martensite_core::shape::Shape::rounded(cx.pt(1.5)),
                    cx.color(TokenKey::SuccessColor, DIGIT),
                );
                x += w + cx.pt(1.0);
            }
        }
        cx.list.push_stroke_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.pt(0.75),
            edge,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut DigitalClock, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        c.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn formats_24h_and_12h() {
        let c = DigitalClock::new().time(Time { hour: 9, minute: 5 });
        assert_eq!(c.text(), "09:05");
        let c = DigitalClock::new()
            .hour12(true)
            .time(Time { hour: 0, minute: 0 });
        assert_eq!(c.text(), "12:00 AM");
        let c = DigitalClock::new().hour12(true).time(Time {
            hour: 12,
            minute: 30,
        });
        assert_eq!(c.text(), "12:30 PM");
    }

    #[test]
    fn seconds_rendered() {
        let c = DigitalClock::new()
            .show_seconds(true)
            .time(Time { hour: 1, minute: 2 });
        assert_eq!(c.text(), "01:02:00");
    }

    #[test]
    fn running_advances_on_tick() {
        let mut c = DigitalClock::new().running(true).time(Time {
            hour: 23,
            minute: 59,
        });
        c.tick(Duration::from_secs(61));
        assert_eq!(c.text(), "00:00"); // wrapped past midnight
        let mut c = DigitalClock::new().time(Time {
            hour: 10,
            minute: 0,
        });
        c.tick(Duration::from_secs(120));
        assert_eq!(c.text(), "10:00"); // stopped clocks don't move
    }

    #[test]
    fn blink_toggles_colon() {
        let mut c = DigitalClock::new().blink(true);
        assert!(c.colon_lit());
        c.tick(Duration::from_millis(600));
        assert!(!c.colon_lit());
        c.tick(Duration::from_millis(600));
        assert!(c.colon_lit());
    }

    #[test]
    fn smoke() {
        let mut c = DigitalClock::new()
            .time(Time {
                hour: 14,
                minute: 30,
            })
            .show_seconds(true)
            .blink(true)
            .label("Line clock");
        laid_out(&mut c, 120.0, 34.0);
        assert_eq!(c.text(), "14:30:00");
        assert!(c.has_seconds() && c.has_blink());
        assert_eq!(
            c.time_value(),
            Time {
                hour: 14,
                minute: 30
            }
        );
    }
}
