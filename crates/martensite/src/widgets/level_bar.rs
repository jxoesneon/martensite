//! `LevelBar` widget: a read-only level/capacity indicator —
//! battery-charge, disk-usage, or signal-strength style (GTK
//! `GtkLevelBar`, `NSLevelIndicator`, KDE `KCapacityBar`).
//!
//! Paints a filled track proportional to `value` inside `min..max`,
//! colored by the zone the value falls into (low → warning → ok →
//! full, each configurable). `segments(n)` switches to a discrete
//! separated-segment presentation (GTK `LEVEL_BAR_MODE_DISCRETE`).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::level_bar::LevelBar;
//!
//! let b = LevelBar::new().value(0.75);
//! assert_eq!(b.get_value(), 0.75);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Widget,
};
use martensite_core::{Rect, TokenKey};

/// Track height, logical points.
const HEIGHT_PT: f32 = 8.0;
/// Default widget width, logical points.
const WIDTH_PT: f32 = 120.0;
/// Corner radius, logical points.
const RADIUS_PT: f32 = 4.0;
/// Segment gap in discrete mode, logical points.
const SEG_GAP_PT: f32 = 3.0;

/// Track (empty) fill.
const TRACK: [u8; 4] = [222, 225, 231, 255];
/// Low-zone ink (below `low` threshold).
const ZONE_LOW: [u8; 4] = [210, 75, 70, 255];
/// Warning-zone ink.
const ZONE_WARN: [u8; 4] = [230, 165, 40, 255];
/// Ok-zone ink.
const ZONE_OK: [u8; 4] = [70, 160, 90, 255];
/// Full-zone ink (at/above `full` threshold).
const ZONE_FULL: [u8; 4] = [70, 110, 200, 255];

/// Which zone the current value falls into.
///
/// # Examples
///
/// ```
/// use martensite::widgets::level_bar::LevelZone;
///
/// assert_ne!(LevelZone::Low, LevelZone::Ok);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LevelZone {
    /// Below the `low` threshold (e.g. nearly empty).
    #[default]
    Low,
    /// Below the `ok` threshold but above `low`.
    Warning,
    /// Below `full` but above `ok`.
    Ok,
    /// At or above `full` (e.g. charged/full).
    Full,
}

/// A level/capacity meter.
///
/// # Examples
///
/// ```
/// use martensite::widgets::level_bar::LevelBar;
///
/// let b = LevelBar::new().value(0.5).segments(5);
/// ```
pub struct LevelBar {
    /// Current value in `0.0..=1.0` (fraction of capacity).
    value: f32,
    /// Fraction at/above which the value is `Full` (default 1.0).
    full_at: f32,
    /// Fraction at/above which the value is `Ok` (default 0.5).
    ok_at: f32,
    /// Fraction at/above which the value is `Warning` (default 0.25);
    /// below it the value is `Low`.
    warn_at: f32,
    /// Discrete segment count; `0` = continuous bar.
    segments: usize,
}

impl LevelBar {
    /// Creates a continuous 0..1 bar.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::level_bar::LevelBar;
    ///
    /// let b = LevelBar::new();
    /// assert_eq!(b.get_value(), 0.0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            value: 0.0,
            full_at: 1.0,
            ok_at: 0.5,
            warn_at: 0.25,
            segments: 0,
        }
    }

    /// Sets the value (fraction of capacity, clamped to `0..=1`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::level_bar::LevelBar;
    ///
    /// let b = LevelBar::new().value(1.5);
    /// assert_eq!(b.get_value(), 1.0);
    /// ```
    #[must_use]
    pub fn value(mut self, value: f32) -> Self {
        self.value = value.clamp(0.0, 1.0);
        self
    }

    /// Sets the zone thresholds — `warn_at`/`ok_at`/`full_at` as
    /// fractions in `0..=1` (sorted ascending on input).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::level_bar::LevelBar;
    ///
    /// let b = LevelBar::new().zones(0.2, 0.6, 0.9);
    /// ```
    #[must_use]
    pub fn zones(mut self, warn_at: f32, ok_at: f32, full_at: f32) -> Self {
        self.warn_at = warn_at.clamp(0.0, 1.0);
        self.ok_at = ok_at.clamp(0.0, 1.0);
        self.full_at = full_at.clamp(0.0, 1.0);
        self
    }

    /// Switches to discrete presentation with `n` segments; `0`
    /// restores the continuous bar.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::level_bar::LevelBar;
    ///
    /// let b = LevelBar::new().segments(4);
    /// ```
    #[must_use]
    pub fn segments(mut self, n: usize) -> Self {
        self.segments = n;
        self
    }

    /// The current value.
    #[inline]
    #[must_use]
    pub fn get_value(&self) -> f32 {
        self.value
    }

    /// Sets the value programmatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::level_bar::LevelBar;
    ///
    /// let mut b = LevelBar::new();
    /// b.set_value(0.3);
    /// assert_eq!(b.get_value(), 0.3);
    /// ```
    pub fn set_value(&mut self, value: f32) {
        self.value = value.clamp(0.0, 1.0);
    }

    /// The zone the value currently falls into.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::level_bar::{LevelBar, LevelZone};
    ///
    /// assert_eq!(LevelBar::new().value(0.1).zone(), LevelZone::Low);
    /// assert_eq!(LevelBar::new().value(1.0).zone(), LevelZone::Full);
    /// ```
    #[must_use]
    pub fn zone(&self) -> LevelZone {
        if self.value >= self.full_at {
            LevelZone::Full
        } else if self.value >= self.ok_at {
            LevelZone::Ok
        } else if self.value >= self.warn_at {
            LevelZone::Warning
        } else {
            LevelZone::Low
        }
    }

    /// Ink for a zone.
    fn zone_ink(&self, cx: &PaintContext) -> [u8; 4] {
        match self.zone() {
            LevelZone::Low => cx.color(TokenKey::ErrorColor, ZONE_LOW),
            LevelZone::Warning => cx.color(TokenKey::WarningColor, ZONE_WARN),
            LevelZone::Ok => cx.color(TokenKey::SuccessColor, ZONE_OK),
            LevelZone::Full => cx.color(TokenKey::AccentColor, ZONE_FULL),
        }
    }
}

impl Default for LevelBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for LevelBar {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ProgressIndicator);
        node.set_numeric_value(f64::from(self.value) * 100.0);
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(100.0);
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let track = kurbo::Rect::new(
            f64::from(cx.bounds.min_x()),
            f64::from(cx.bounds.min_y()),
            f64::from(cx.bounds.max_x()),
            f64::from(cx.bounds.max_y()),
        );
        let shape = martensite_core::shape::Shape::rounded(cx.pt(RADIUS_PT));
        let ink = self.zone_ink(cx);
        let track_ink = cx.color(TokenKey::DividerColor, TRACK);

        if self.segments == 0 {
            cx.list.push_fill_shape(track, &shape, track_ink);
            if self.value > 0.0 {
                let w = track.width() * f64::from(self.value);
                let fill = kurbo::Rect::new(track.x0, track.y0, track.x0 + w, track.y1);
                cx.list.push_clip(fill);
                cx.list.push_fill_shape(track, &shape, ink);
                cx.list.pop_clip();
            }
            return;
        }

        // Discrete: `segments` equally sized cells separated by gaps;
        // cells up to round(value * segments) are filled.
        let n = self.segments;
        let gap = f64::from(cx.pt(SEG_GAP_PT));
        let cell_w = (track.width() - gap * (n.saturating_sub(1)) as f64) / n as f64;
        let lit = (self.value * n as f32).round() as usize;
        for i in 0..n {
            let x0 = track.x0 + i as f64 * (cell_w + gap);
            let cell = kurbo::Rect::new(x0, track.y0, x0 + cell_w, track.y1);
            let cell_shape = martensite_core::shape::Shape::rounded(cx.pt(2.0));
            cx.list
                .push_fill_shape(cell, &cell_shape, if i < lit { ink } else { track_ink });
        }
    }
}

impl std::fmt::Debug for LevelBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LevelBar")
            .field("value", &self.value)
            .field("segments", &self.segments)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn value_clamps() {
        assert_eq!(LevelBar::new().value(2.0).get_value(), 1.0);
        assert_eq!(LevelBar::new().value(-1.0).get_value(), 0.0);
    }

    #[test]
    fn zone_thresholds() {
        let b = LevelBar::new().zones(0.25, 0.5, 1.0);
        assert_eq!(b.zone(), LevelZone::Low);
        assert_eq!(LevelBar::new().value(0.3).zone(), LevelZone::Warning);
        assert_eq!(LevelBar::new().value(0.7).zone(), LevelZone::Ok);
        assert_eq!(LevelBar::new().value(1.0).zone(), LevelZone::Full);
    }

    #[test]
    fn measure_is_scale_aware() {
        let mut b = LevelBar::new();
        let mut hot = HotNode::default();
        let size = b.measure(
            &mut LayoutContext {
                hot: &mut hot,
                scale: 2.0,
            },
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(500.0, 500.0),
            },
        );
        assert_eq!(size.y, 16.0); // 8pt @ 2x
        assert_eq!(size.x, 240.0); // 120pt @ 2x
    }
}
