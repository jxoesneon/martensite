//! `Odometer` — a mechanical-reel digit counter (trip-odometer /
//! web hit-counter idiom).
//!
//! The value renders as a row of digit windows, each rolling
//! vertically toward its target digit inside the reel — a `tick`
//! animates the reels at `speed` digits per second. Reels show
//! the neighboring digits above and below, the mechanical-odometer
//! look.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::odometer::Odometer;
//!
//! let o = Odometer::new().digits(5).value(42);
//! assert_eq!(o.reading(), 42);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    LayoutConstraints, LayoutContext, PaintContext, Rect, RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;
use std::time::Duration;

const DIGIT_W_PT: f32 = 14.0;
const DIGIT_H_PT: f32 = 22.0;
const SPEED: f32 = 8.0; // digits/sec

const FACE: [u8; 4] = [42, 42, 46, 255];
const FG: [u8; 4] = [230, 230, 235, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];

/// A mechanical-reel digit counter — see the module docs.
///
/// ```
/// use martensite::widgets::odometer::Odometer;
///
/// assert_eq!(Odometer::new().reading(), 0);
/// ```
pub struct Odometer {
    /// Accessibility label.
    pub label: String,
    /// Reel animation speed in digits per second.
    pub speed: f32,
    digits: usize,
    value: u64,
    /// Current reel position per window (fractional digits).
    reels: Vec<f32>,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
}

impl Default for Odometer {
    fn default() -> Self {
        Self::new()
    }
}

impl Odometer {
    /// Creates a six-digit counter at zero.
    ///
    /// ```
    /// use martensite::widgets::odometer::Odometer;
    ///
    /// assert_eq!(Odometer::new().reading(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Counter".to_string(),
            speed: SPEED,
            digits: 6,
            value: 0,
            reels: vec![0.0; 6],
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
        }
    }

    /// Window (digit) count `1..=12`.
    ///
    /// ```
    /// use martensite::widgets::odometer::Odometer;
    ///
    /// let o = Odometer::new().digits(4);
    /// assert_eq!(o.digit_count(), 4);
    /// ```
    pub fn digits(mut self, n: usize) -> Self {
        self.digits = n.clamp(1, 12);
        self.reels = vec![0.0; self.digits];
        self
    }

    /// Displayed value (clamped to the digit count).
    ///
    /// ```
    /// use martensite::widgets::odometer::Odometer;
    ///
    /// let o = Odometer::new().digits(3).value(7);
    /// assert_eq!(o.reading(), 7);
    /// ```
    pub fn value(mut self, v: u64) -> Self {
        self.value = v.min(self.max_value());
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::odometer::Odometer;
    ///
    /// let o = Odometer::new().label("Hits");
    /// assert_eq!(o.label, "Hits");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Reel speed in digits per second.
    ///
    /// ```
    /// use martensite::widgets::odometer::Odometer;
    ///
    /// let o = Odometer::new().speed(4.0);
    /// assert_eq!(o.speed, 4.0);
    /// ```
    pub fn speed(mut self, digits_per_sec: f32) -> Self {
        self.speed = digits_per_sec.max(0.1);
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::odometer::Odometer;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let o = Odometer::new().with_text_painter(shared_painter());
    /// assert_eq!(o.reading(), 0);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Current reading.
    ///
    /// ```
    /// use martensite::widgets::odometer::Odometer;
    ///
    /// assert_eq!(Odometer::new().digits(3).value(42).reading(), 42);
    /// ```
    pub fn reading(&self) -> u64 {
        self.value
    }

    /// Digit window count.
    ///
    /// ```
    /// use martensite::widgets::odometer::Odometer;
    ///
    /// assert_eq!(Odometer::new().digit_count(), 6);
    /// ```
    pub fn digit_count(&self) -> usize {
        self.digits
    }

    /// Sets the value; reels animate toward it on `tick`.
    ///
    /// ```
    /// use martensite::widgets::odometer::Odometer;
    ///
    /// let mut o = Odometer::new();
    /// o.set_value(99);
    /// assert_eq!(o.reading(), 99);
    /// ```
    pub fn set_value(&mut self, v: u64) {
        self.value = v.min(self.max_value());
    }

    /// Snaps the reels to the current value (no animation).
    ///
    /// ```
    /// use martensite::widgets::odometer::Odometer;
    ///
    /// let mut o = Odometer::new().value(50);
    /// o.snap();
    /// assert!(!o.is_rolling());
    /// ```
    pub fn snap(&mut self) {
        let (v, d) = (self.value, self.digits);
        for (i, r) in self.reels.iter_mut().enumerate() {
            *r = Self::digit_at(v, d, i) as f32;
        }
    }

    /// Whether any reel is still rolling.
    ///
    /// ```
    /// use martensite::widgets::odometer::Odometer;
    ///
    /// assert!(!Odometer::new().is_rolling());
    /// ```
    pub fn is_rolling(&self) -> bool {
        self.reels
            .iter()
            .enumerate()
            .any(|(i, r)| (*r - self.target_digit(i) as f32).abs() > 0.001)
    }

    /// Digit of `v` at window `i` in a `d`-digit display.
    fn digit_at(v: u64, digits: usize, i: usize) -> u8 {
        let pos = digits - 1 - i;
        ((v / 10u64.pow(pos as u32)) % 10) as u8
    }

    /// Largest representable value (`10^digits - 1`).
    fn max_value(&self) -> u64 {
        10u64.pow(self.digits as u32).saturating_sub(1)
    }

    /// Target digit for reel `i` (0 = leftmost window).
    fn target_digit(&self, i: usize) -> u8 {
        Self::digit_at(self.value, self.digits, i)
    }
}

impl Widget for Odometer {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let w = cx.pt(DIGIT_W_PT) * self.digits as f32 + cx.pt(6.0);
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            cx.pt(DIGIT_H_PT + 4.0).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(20.0, 16.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Label);
        node.set_label(format!("{} — {}", self.label, self.value));
    }

    fn tick(&mut self, dt: Duration) -> bool {
        let step = self.speed * dt.as_secs_f32();
        let (v, d) = (self.value, self.digits);
        let mut rolling = false;
        for (i, r) in self.reels.iter_mut().enumerate() {
            let target = Self::digit_at(v, d, i) as f32;
            if (*r - target).abs() > 0.001 {
                // Roll upward through the digits, wrapping 9→0 like
                // a real reel; land when this step covers the
                // remaining forward distance.
                let dist = (target - *r).rem_euclid(10.0);
                if step >= dist {
                    *r = target;
                } else {
                    *r = (*r + step).rem_euclid(10.0);
                    rolling = true;
                }
            }
        }
        rolling
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
        let face = cx.color(TokenKey::SurfaceColor, FACE);
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        let fg = cx.color(TokenKey::TextColor, FG);
        let frame = martensite_core::shape::Shape::rounded(cx.pt(3.0));
        cx.list.push_fill_shape(f(self.bounds), &frame, edge);
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size = 13.0 * cx.scale;
        let digit_w = cx.pt(DIGIT_W_PT);
        let inner = Rect::new(
            self.bounds.min_x() + cx.pt(1.0),
            self.bounds.min_y() + cx.pt(1.0),
            (self.bounds.width() - cx.pt(2.0)).max(0.0),
            (self.bounds.height() - cx.pt(2.0)).max(0.0),
        );
        for (i, &reel) in self.reels.iter().enumerate() {
            let x = inner.min_x() + cx.pt(2.0) + i as f32 * digit_w;
            let cell = Rect::new(x, inner.min_y(), digit_w, inner.height());
            cx.list.push_fill_shape(
                f(cell),
                &martensite_core::shape::Shape::rounded(cx.pt(2.0)),
                face,
            );
            cx.list.push_clip(f(cell));
            // Current digit + neighbors rolling through the window.
            let base = reel.floor();
            let frac = reel - base;
            for d in -1..=1 {
                let digit = ((base as i32 + d).rem_euclid(10)) as u32;
                let y = cell.min_y() + cell.height() / 2.0 - size / 2.0
                    + (d as f32 - frac) * cell.height();
                if y > cell.max_y() || y + size < cell.min_y() {
                    continue;
                }
                let s = digit.to_string();
                let w = painter
                    .and_then(|p| p.measure_text(&s, size))
                    .unwrap_or(size * 0.6);
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    f(cell),
                    kurbo::Point::new(f64::from(x + (digit_w - w) / 2.0), f64::from(y)),
                    &s,
                    size,
                    fg,
                );
            }
            cx.list.pop_clip();
        }
    }
}

impl std::fmt::Debug for Odometer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Odometer")
            .field("value", &self.value)
            .field("rolling", &self.is_rolling())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_clamps_to_digits() {
        let o = Odometer::new().digits(3).value(5000);
        assert_eq!(o.reading(), 999);
    }

    #[test]
    fn target_digits_decompose() {
        let o = Odometer::new().digits(4).value(1234);
        assert_eq!(o.target_digit(0), 1);
        assert_eq!(o.target_digit(3), 4);
    }

    #[test]
    fn tick_rolls_toward_target() {
        let mut o = Odometer::new().digits(1);
        o.set_value(5);
        assert!(o.is_rolling());
        // At 8 digits/sec, 1s passes the target.
        o.tick(Duration::from_secs(1));
        assert!(!o.is_rolling());
        assert_eq!(o.reels[0], 5.0);
    }

    #[test]
    fn snap_settles() {
        let mut o = Odometer::new().digits(2).value(42);
        o.snap();
        assert!(!o.is_rolling());
        assert_eq!(o.reels[1], 2.0);
    }

    #[test]
    fn settled_reel_does_not_tick() {
        let mut o = Odometer::new().digits(1).value(3);
        o.snap();
        assert!(!o.tick(Duration::from_millis(500)));
    }
}
