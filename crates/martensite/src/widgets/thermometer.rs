//! `Thermometer` — a classic temperature-scale indicator
//! (bulb + column fill against a ticked range — the status-display
//! companion to [`crate::widgets::battery::Battery`] and
//! [`crate::widgets::vu_meter::VuMeter`]).
//!
//! The value maps onto a `min..=max` range; the column fill
//! follows the mercury into the bulb and a ticked scale runs
//! alongside. Optional `warning`/`critical` thresholds tint the
//! fill above each level.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::thermometer::Thermometer;
//!
//! let t = Thermometer::new().range(0.0, 100.0).value(42.0);
//! assert_eq!(t.reading(), 42.0);
//! assert!(t.fraction() > 0.4 && t.fraction() < 0.43);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    LayoutConstraints, LayoutContext, PaintContext, Rect, RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

const WIDTH_PT: f32 = 40.0;
const HEIGHT_PT: f32 = 140.0;
const TUBE_PT: f32 = 8.0;
const BULB_PT: f32 = 16.0;
const TICK_PT: f32 = 6.0;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const FLUID: [u8; 4] = [210, 110, 90, 255];
const WARN: [u8; 4] = [230, 170, 80, 255];
const CRIT: [u8; 4] = [230, 80, 70, 255];
const TICK: [u8; 4] = [140, 140, 150, 255];

/// A temperature-scale indicator — see the module docs.
///
/// ```
/// use martensite::widgets::thermometer::Thermometer;
///
/// assert_eq!(Thermometer::new().reading(), 0.0);
/// ```
#[derive(Debug)]
pub struct Thermometer {
    /// Accessibility label.
    pub label: String,
    min: f32,
    max: f32,
    value: f32,
    /// Tint the fluid above this fraction of range.
    warning: Option<f32>,
    /// Tint the fluid above this fraction of range (wins over warning).
    critical: Option<f32>,
    /// Number of scale ticks.
    ticks: u32,
    /// Units shown in the accessibility value (e.g. `"°C"`).
    units: String,
    bounds: Rect,
    scale: f32,
}

impl Default for Thermometer {
    fn default() -> Self {
        Self::new()
    }
}

impl Thermometer {
    /// Creates a `0.0..=100.0` thermometer at zero.
    ///
    /// ```
    /// use martensite::widgets::thermometer::Thermometer;
    ///
    /// assert_eq!(Thermometer::new().reading(), 0.0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Temperature".to_string(),
            min: 0.0,
            max: 100.0,
            value: 0.0,
            warning: None,
            critical: None,
            ticks: 5,
            units: String::new(),
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets the displayed range.
    ///
    /// ```
    /// use martensite::widgets::thermometer::Thermometer;
    ///
    /// assert_eq!(Thermometer::new().range(-20.0, 50.0).value_range(), (-20.0, 50.0));
    /// ```
    pub fn range(mut self, min: f32, max: f32) -> Self {
        if max > min {
            self.min = min;
            self.max = max;
        }
        self
    }

    /// Sets the current reading (clamped to the range).
    ///
    /// ```
    /// use martensite::widgets::thermometer::Thermometer;
    ///
    /// assert_eq!(Thermometer::new().value(150.0).reading(), 100.0);
    /// ```
    pub fn value(mut self, v: f32) -> Self {
        self.value = v.clamp(self.min, self.max);
        self
    }

    /// Tints the fluid amber above `fraction` of the range.
    ///
    /// ```
    /// use martensite::widgets::thermometer::Thermometer;
    ///
    /// assert_eq!(Thermometer::new().warning(0.75).warning_level(), Some(0.75));
    /// ```
    pub fn warning(mut self, fraction: f32) -> Self {
        self.warning = Some(fraction.clamp(0.0, 1.0));
        self
    }

    /// Tints the fluid red above `fraction` of the range.
    ///
    /// ```
    /// use martensite::widgets::thermometer::Thermometer;
    ///
    /// assert_eq!(Thermometer::new().critical(0.9).critical_level(), Some(0.9));
    /// ```
    pub fn critical(mut self, fraction: f32) -> Self {
        self.critical = Some(fraction.clamp(0.0, 1.0));
        self
    }

    /// Number of scale ticks drawn alongside the tube.
    ///
    /// ```
    /// use martensite::widgets::thermometer::Thermometer;
    ///
    /// assert_eq!(Thermometer::new().ticks(9).tick_count(), 9);
    /// ```
    pub fn ticks(mut self, n: u32) -> Self {
        self.ticks = n.min(20);
        self
    }

    /// Units appended to the accessibility value.
    ///
    /// ```
    /// use martensite::widgets::thermometer::Thermometer;
    ///
    /// assert_eq!(Thermometer::new().units("°C").unit_text(), "°C");
    /// ```
    pub fn units(mut self, u: impl Into<String>) -> Self {
        self.units = u.into();
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::thermometer::Thermometer;
    ///
    /// assert_eq!(Thermometer::new().label("CPU").label, "CPU");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Current reading.
    ///
    /// ```
    /// use martensite::widgets::thermometer::Thermometer;
    ///
    /// assert_eq!(Thermometer::new().value(55.0).reading(), 55.0);
    /// ```
    pub fn reading(&self) -> f32 {
        self.value
    }

    /// The `min..=max` range.
    ///
    /// ```
    /// use martensite::widgets::thermometer::Thermometer;
    ///
    /// assert_eq!(Thermometer::new().value_range(), (0.0, 100.0));
    /// ```
    pub fn value_range(&self) -> (f32, f32) {
        (self.min, self.max)
    }

    /// Fill fraction `0.0..=1.0`.
    ///
    /// ```
    /// use martensite::widgets::thermometer::Thermometer;
    ///
    /// assert_eq!(Thermometer::new().value(50.0).fraction(), 0.5);
    /// ```
    pub fn fraction(&self) -> f32 {
        ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    /// Warning threshold fraction.
    ///
    /// ```
    /// use martensite::widgets::thermometer::Thermometer;
    ///
    /// assert_eq!(Thermometer::new().warning_level(), None);
    /// ```
    pub fn warning_level(&self) -> Option<f32> {
        self.warning
    }

    /// Critical threshold fraction.
    ///
    /// ```
    /// use martensite::widgets::thermometer::Thermometer;
    ///
    /// assert_eq!(Thermometer::new().critical_level(), None);
    /// ```
    pub fn critical_level(&self) -> Option<f32> {
        self.critical
    }

    /// Tick count.
    ///
    /// ```
    /// use martensite::widgets::thermometer::Thermometer;
    ///
    /// assert_eq!(Thermometer::new().tick_count(), 5);
    /// ```
    pub fn tick_count(&self) -> u32 {
        self.ticks
    }

    /// Unit suffix.
    ///
    /// ```
    /// use martensite::widgets::thermometer::Thermometer;
    ///
    /// assert_eq!(Thermometer::new().units("%").unit_text(), "%");
    /// ```
    pub fn unit_text(&self) -> &str {
        &self.units
    }

    /// Fluid color at the current value (threshold-tinted).
    fn fluid_color(&self) -> [u8; 4] {
        let f = self.fraction();
        if self.critical.is_some_and(|c| f >= c) {
            CRIT
        } else if self.warning.is_some_and(|w| f >= w) {
            WARN
        } else {
            FLUID
        }
    }
}

impl Widget for Thermometer {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(20.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Meter);
        node.set_label(&self.label);
        node.set_value(format!("{}{}", self.value.round() as i64, self.units));
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
        let tube_w = cx.pt(TUBE_PT);
        let bulb_r = cx.pt(BULB_PT) / 2.0;
        let bulb_cy = self.bounds.max_y() - bulb_r - cx.pt(4.0);
        let tube_top = self.bounds.min_y() + cx.pt(6.0);
        let tube_x = self.bounds.min_x() + cx.pt(6.0);
        let tube_h = (bulb_cy - tube_top).max(0.0);
        let tube = Rect::new(tube_x, tube_top, tube_w, tube_h);
        // Tube outline + bulb outline.
        cx.list.push_stroke_shape(
            krect(tube),
            &martensite_core::shape::Shape::rounded(tube_w / 2.0),
            cx.pt(1.0),
            edge,
        );
        let bulb_center = Vec2::new(tube_x + tube_w / 2.0, bulb_cy);
        let bulb = kurbo::Rect::new(
            f64::from(bulb_center.x - bulb_r),
            f64::from(bulb_center.y - bulb_r),
            f64::from(bulb_center.x + bulb_r),
            f64::from(bulb_center.y + bulb_r),
        );
        cx.list.push_stroke_shape(
            bulb,
            &martensite_core::shape::Shape::circle(bulb_center, bulb_r),
            cx.pt(1.0),
            edge,
        );
        // Fluid: always fills the bulb, rises up the tube by fraction.
        let fluid = self.fluid_color();
        cx.list.push_fill_shape(
            bulb,
            &martensite_core::shape::Shape::circle(bulb_center, bulb_r - cx.pt(1.0)),
            fluid,
        );
        let fill_h = self.fraction() * tube_h;
        if fill_h > 0.0 {
            let fill = Rect::new(
                tube_x + cx.pt(1.0),
                tube_top + tube_h - fill_h,
                tube_w - 2.0 * cx.pt(1.0),
                fill_h,
            );
            cx.list
                .push_fill_shape(krect(fill), &martensite_core::shape::Shape::RECT, fluid);
        }
        // Scale ticks to the right of the tube.
        if self.ticks > 0 {
            let tick_len = cx.pt(TICK_PT);
            for i in 0..=self.ticks {
                let y = tube_top + tube_h * i as f32 / self.ticks as f32;
                let mut t = kurbo::BezPath::new();
                t.move_to((f64::from(tube_x + tube_w + cx.pt(2.0)), f64::from(y)));
                t.line_to((
                    f64::from(tube_x + tube_w + cx.pt(2.0) + tick_len),
                    f64::from(y),
                ));
                cx.list
                    .push_stroke_path(t, cx.pt(0.75), cx.color(TokenKey::TextMutedColor, TICK));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(t: &mut Thermometer, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        t.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        t.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn clamps_to_range() {
        assert_eq!(Thermometer::new().value(150.0).reading(), 100.0);
        assert_eq!(Thermometer::new().value(-10.0).reading(), 0.0);
        assert_eq!(
            Thermometer::new().range(10.0, 20.0).value(15.0).fraction(),
            0.5
        );
    }

    #[test]
    fn bad_range_ignored() {
        let t = Thermometer::new().range(50.0, 50.0);
        assert_eq!(t.value_range(), (0.0, 100.0));
    }

    #[test]
    fn thresholds() {
        let t = Thermometer::new().warning(0.5).critical(0.9).value(95.0);
        assert_eq!(t.fluid_color(), CRIT);
        let t = Thermometer::new().warning(0.5).critical(0.9).value(60.0);
        assert_eq!(t.fluid_color(), WARN);
        let t = Thermometer::new().warning(0.5).critical(0.9).value(10.0);
        assert_eq!(t.fluid_color(), FLUID);
    }

    #[test]
    fn ticks_capped() {
        assert_eq!(Thermometer::new().ticks(99).tick_count(), 20);
    }

    #[test]
    fn smoke() {
        let mut t = Thermometer::new()
            .range(-20.0, 120.0)
            .value(35.0)
            .units("°C")
            .label("Coolant");
        laid_out(&mut t, 40.0, 140.0);
        assert!(t.fraction() > 0.39 && t.fraction() < 0.40);
        assert_eq!(t.unit_text(), "°C");
    }
}
