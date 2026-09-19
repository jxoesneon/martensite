//! `Gauge` widget: a radial gauge *display* — read-only value arc
//! with tick marks and a value label (Windows Community Toolkit
//! `RadialGauge`, SwiftUI `Gauge`, automotive-dial idiom).
//!
//! Unlike [`Dial`](crate::widgets::dial::Dial) (an input), `Gauge`
//! only reports: value arc + needle + optional min/max ticks and a
//! centered value readout. Danger zones paint via `zones` like
//! [`LevelBar`](crate::widgets::level_bar::LevelBar).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::gauge::Gauge;
//!
//! let g = Gauge::new().range(0.0, 100.0).value(62.0);
//! assert_eq!(g.get_value(), 62.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Widget,
};
use martensite_core::{Rect, TokenKey};

/// Gauge diameter, logical points.
const SIZE_PT: f32 = 96.0;
/// Arc stroke width, logical points.
const ARC_PT: f32 = 6.0;
/// Value label font size, logical points.
const FONT_PT: f32 = 14.0;

/// Sweep: value `min` sits at 225°, `max` at −45° (270° over the top).
const START_DEG: f64 = 225.0;
/// Total sweep.
const SWEEP_DEG: f64 = -270.0;

/// Track ink.
const TRACK: [u8; 4] = [222, 225, 231, 255];
/// Value arc.
const VALUE: [u8; 4] = [70, 110, 200, 255];
/// Warning-zone arc.
const WARN: [u8; 4] = [230, 165, 40, 255];
/// Critical-zone arc.
const CRIT: [u8; 4] = [210, 75, 70, 255];
/// Tick + label ink.
const INK: [u8; 4] = [110, 114, 123, 255];

/// A radial value display.
///
/// # Examples
///
/// ```
/// use martensite::widgets::gauge::Gauge;
///
/// let g = Gauge::new().range(0.0, 120.0).value(80.0).label("km/h");
/// ```
pub struct Gauge {
    /// Range minimum.
    min: f64,
    /// Range maximum.
    max: f64,
    /// Current value.
    value: f64,
    /// Unit caption under the value (e.g. "km/h").
    label: String,
    /// Fraction at/above which the arc paints warning ink.
    warn_at: Option<f64>,
    /// Fraction at/above which the arc paints critical ink.
    crit_at: Option<f64>,
    /// Whether to paint min/max tick marks.
    ticks: bool,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Gauge {
    /// Creates a 0..100 gauge at 0.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::gauge::Gauge;
    ///
    /// let g = Gauge::new();
    /// assert_eq!(g.get_value(), 0.0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            min: 0.0,
            max: 100.0,
            value: 0.0,
            label: String::new(),
            warn_at: None,
            crit_at: None,
            ticks: true,
            text_painter: None,
        }
    }

    /// Sets the range.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::gauge::Gauge;
    ///
    /// let g = Gauge::new().range(0.0, 240.0);
    /// ```
    #[must_use]
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = min;
        self.max = max.max(min);
        self.value = self.value.clamp(self.min, self.max);
        self
    }

    /// Sets the displayed value (clamped).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::gauge::Gauge;
    ///
    /// let g = Gauge::new().value(42.0);
    /// ```
    #[must_use]
    pub fn value(mut self, value: f64) -> Self {
        self.value = value.clamp(self.min, self.max);
        self
    }

    /// Sets the unit caption.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::gauge::Gauge;
    ///
    /// let g = Gauge::new().label("rpm");
    /// ```
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Sets the warning/critical thresholds as fractions `0..=1`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::gauge::Gauge;
    ///
    /// let g = Gauge::new().zones(0.6, 0.85);
    /// ```
    #[must_use]
    pub fn zones(mut self, warn_at: f64, crit_at: f64) -> Self {
        self.warn_at = Some(warn_at.clamp(0.0, 1.0));
        self.crit_at = Some(crit_at.clamp(0.0, 1.0));
        self
    }

    /// Whether min/max tick marks paint (default true).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::gauge::Gauge;
    ///
    /// let g = Gauge::new().ticks(false);
    /// ```
    #[must_use]
    pub fn ticks(mut self, ticks: bool) -> Self {
        self.ticks = ticks;
        self
    }

    /// The current value.
    #[inline]
    #[must_use]
    pub fn get_value(&self) -> f64 {
        self.value
    }

    /// Sets the value programmatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::gauge::Gauge;
    ///
    /// let mut g = Gauge::new();
    /// g.set_value(55.0);
    /// assert_eq!(g.get_value(), 55.0);
    /// ```
    pub fn set_value(&mut self, value: f64) {
        self.value = value.clamp(self.min, self.max);
    }

    /// Installs a shared shaped-text painter.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Value fraction `0..=1`.
    fn frac(&self) -> f64 {
        if self.max <= self.min {
            0.0
        } else {
            ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        }
    }

    /// Arc ink for the current value's zone.
    fn arc_ink(&self, cx: &PaintContext) -> [u8; 4] {
        let f = self.frac();
        if self.crit_at.is_some_and(|t| f >= t) {
            cx.color(TokenKey::ErrorColor, CRIT)
        } else if self.warn_at.is_some_and(|t| f >= t) {
            cx.color(TokenKey::WarningColor, WARN)
        } else {
            cx.color(TokenKey::AccentColor, VALUE)
        }
    }
}

impl Default for Gauge {
    fn default() -> Self {
        Self::new()
    }
}

/// Samples `sweep` degrees of arc into a polyline `BezPath`.
fn arc_path(center: kurbo::Point, radius: f64, start_deg: f64, sweep_deg: f64) -> kurbo::BezPath {
    let mut path = kurbo::BezPath::new();
    let segs = 32usize;
    for i in 0..=segs {
        let t = (start_deg + sweep_deg * i as f64 / segs as f64).to_radians();
        let p = kurbo::Point::new(center.x + radius * t.cos(), center.y + radius * t.sin());
        if i == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    path
}

impl Widget for Gauge {
    fn measure(&mut self, cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(SIZE_PT);
        Vec2::new(s, s * 0.85) // the open bottom of the sweep trims height
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Meter);
        node.set_label("Gauge");
        node.set_numeric_value(self.value);
        node.set_min_numeric_value(self.min);
        node.set_max_numeric_value(self.max);
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let r = kurbo::Rect::new(
            f64::from(cx.bounds.min_x()),
            f64::from(cx.bounds.min_y()),
            f64::from(cx.bounds.max_x()),
            f64::from(cx.bounds.max_y()),
        );
        let center = kurbo::Point::new((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
        let radius = (r.width().min(r.height() * 1.15)) / 2.0;
        let arc_w = f64::from(cx.pt(ARC_PT));
        let arc_r = radius - arc_w / 2.0 - 1.0;
        let track_ink = cx.color(TokenKey::DividerColor, TRACK);

        // Track + value arcs.
        cx.list.push_stroke_path(
            arc_path(center, arc_r, START_DEG, SWEEP_DEG),
            cx.pt(ARC_PT),
            track_ink,
        );
        let sweep = SWEEP_DEG * self.frac();
        if sweep.abs() > 0.01 {
            cx.list.push_stroke_path(
                arc_path(center, arc_r, START_DEG, sweep),
                cx.pt(ARC_PT),
                self.arc_ink(cx),
            );
        }

        // Min/max ticks at the sweep ends.
        if self.ticks {
            let tick_ink = cx.color(TokenKey::TextMutedColor, INK);
            for deg in [START_DEG, START_DEG + SWEEP_DEG] {
                let a = deg.to_radians();
                let mut tick = kurbo::BezPath::new();
                tick.move_to(kurbo::Point::new(
                    center.x + a.cos() * (arc_r - arc_w),
                    center.y + a.sin() * (arc_r - arc_w),
                ));
                tick.line_to(kurbo::Point::new(
                    center.x + a.cos() * (arc_r + arc_w),
                    center.y + a.sin() * (arc_r + arc_w),
                ));
                cx.list.push_stroke_path(tick, cx.pt(1.5), tick_ink);
            }
        }

        // Centered value + unit readout.
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size = cx.pt(FONT_PT);
        let text = if self.label.is_empty() {
            format!("{}", self.value.round() as i64)
        } else {
            format!("{} {}", self.value.round() as i64, self.label)
        };
        let w = painter
            .and_then(|p| p.measure_text(&text, size))
            .unwrap_or(size * text.chars().count() as f32 * 0.55);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            r,
            kurbo::Point::new(
                center.x - f64::from(w) / 2.0,
                center.y - f64::from(size) / 4.0,
            ),
            &text,
            size,
            cx.color(TokenKey::TextColor, [30, 31, 36, 255]),
        );
    }
}

impl std::fmt::Debug for Gauge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gauge")
            .field("value", &self.value)
            .field("min", &self.min)
            .field("max", &self.max)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn builder_clamps() {
        assert_eq!(Gauge::new().range(0.0, 10.0).value(99.0).get_value(), 10.0);
        assert_eq!(Gauge::new().value(-5.0).get_value(), 0.0);
    }

    #[test]
    fn frac_math() {
        let g = Gauge::new().range(0.0, 200.0).value(50.0);
        assert_eq!(g.frac(), 0.25);
    }

    #[test]
    fn measure_is_scale_aware() {
        let mut g = Gauge::new();
        let mut hot = HotNode::default();
        let s = g.measure(
            &mut LayoutContext {
                hot: &mut hot,
                scale: 2.0,
            },
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(500.0, 500.0),
            },
        );
        assert_eq!(s.x, 192.0); // 96pt @ 2x
    }
}
