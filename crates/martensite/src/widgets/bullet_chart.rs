//! `BulletChart` — the Stephen Few bullet graph: a compact KPI bar
//! that reads value-vs-target against qualitative range bands.
//!
//! The widget paints up to three background bands (poor / ok /
//! good — supplied as ascending `[f32; 3]` thresholds), the
//! quantitative measure as a solid bar, and the comparative target
//! as a vertical tick. A label and formatted value paint beside the
//! bar when a text painter is present.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::bullet_chart::BulletChart;
//!
//! let chart = BulletChart::new()
//!     .label("Revenue")
//!     .value(75.0)
//!     .target(90.0)
//!     .ranges([40.0, 70.0, 100.0]);
//! assert_eq!(chart.measure_value(), 75.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const WIDTH_PT: f32 = 220.0;
const HEIGHT_PT: f32 = 36.0;
const BAR_PT: f32 = 10.0;

const TRACK: [u8; 4] = [48, 48, 52, 255];
const BAND: [u8; 4] = [70, 70, 76, 255];
const VALUE: [u8; 4] = [90, 150, 230, 255];
const TARGET: [u8; 4] = [230, 230, 235, 255];
const FG: [u8; 4] = [230, 230, 235, 255];
const MUTED: [u8; 4] = [140, 140, 148, 255];

/// A bullet-graph KPI — see the module docs.
///
/// ```
/// use martensite::widgets::bullet_chart::BulletChart;
///
/// let chart = BulletChart::new().value(50.0).target(75.0);
/// assert_eq!(chart.target_value(), 75.0);
/// ```
pub struct BulletChart {
    /// Accessibility / edge label.
    pub label: String,
    value: f32,
    target: f32,
    ranges: [f32; 3],
    hovered: bool,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
}

impl Default for BulletChart {
    fn default() -> Self {
        Self::new()
    }
}

impl BulletChart {
    /// Creates an empty chart (`0..=100` bands).
    ///
    /// ```
    /// use martensite::widgets::bullet_chart::BulletChart;
    ///
    /// assert_eq!(BulletChart::new().measure_value(), 0.0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Metric".to_string(),
            value: 0.0,
            target: 0.0,
            ranges: [60.0, 80.0, 100.0],
            hovered: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
        }
    }

    /// Label text.
    ///
    /// ```
    /// use martensite::widgets::bullet_chart::BulletChart;
    ///
    /// let chart = BulletChart::new().label("Uptime");
    /// assert_eq!(chart.label, "Uptime");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Quantitative measure.
    ///
    /// ```
    /// use martensite::widgets::bullet_chart::BulletChart;
    ///
    /// let chart = BulletChart::new().value(42.0);
    /// assert_eq!(chart.measure_value(), 42.0);
    /// ```
    pub fn value(mut self, v: f32) -> Self {
        self.value = v;
        self
    }

    /// Comparative target marker.
    ///
    /// ```
    /// use martensite::widgets::bullet_chart::BulletChart;
    ///
    /// let chart = BulletChart::new().target(80.0);
    /// assert_eq!(chart.target_value(), 80.0);
    /// ```
    pub fn target(mut self, t: f32) -> Self {
        self.target = t;
        self
    }

    /// Qualitative band thresholds — ascending `[poor_end, ok_end,
    /// max]`.
    ///
    /// ```
    /// use martensite::widgets::bullet_chart::BulletChart;
    ///
    /// let chart = BulletChart::new().ranges([30.0, 60.0, 100.0]);
    /// assert_eq!(chart.max(), 100.0);
    /// ```
    pub fn ranges(mut self, ranges: [f32; 3]) -> Self {
        self.ranges = ranges;
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::bullet_chart::BulletChart;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let chart = BulletChart::new().with_text_painter(shared_painter());
    /// assert_eq!(chart.measure_value(), 0.0);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Current measure.
    ///
    /// ```
    /// use martensite::widgets::bullet_chart::BulletChart;
    ///
    /// assert_eq!(BulletChart::new().measure_value(), 0.0);
    /// ```
    pub fn measure_value(&self) -> f32 {
        self.value
    }

    /// Comparative target.
    ///
    /// ```
    /// use martensite::widgets::bullet_chart::BulletChart;
    ///
    /// assert_eq!(BulletChart::new().target_value(), 0.0);
    /// ```
    pub fn target_value(&self) -> f32 {
        self.target
    }

    /// Scale maximum (the third band threshold).
    ///
    /// ```
    /// use martensite::widgets::bullet_chart::BulletChart;
    ///
    /// assert_eq!(BulletChart::new().max(), 100.0);
    /// ```
    pub fn max(&self) -> f32 {
        self.ranges[2]
    }
}

impl Widget for BulletChart {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(96.0, 20.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.clone());
        node.set_description(format!(
            "{:.0} of {:.0}, target {:.0}",
            self.value, self.ranges[2], self.target
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let inside = self.bounds.contains(*position);
                if inside != self.hovered {
                    self.hovered = inside;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
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
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        let fg = cx.color(TokenKey::TextColor, FG);
        let track = cx.color(TokenKey::SurfaceColor, TRACK);
        let band = cx.color(TokenKey::BorderColor, BAND);
        let value_c = cx.color(TokenKey::AccentColor, VALUE);
        let target_c = cx.color(TokenKey::TextColor, TARGET);

        // Label strip on the left — 64pt reserved.
        let label_w = cx.pt(64.0);
        let track_r = Rect::new(
            self.bounds.min_x() + label_w,
            self.bounds.min_y() + self.bounds.height() / 2.0 - cx.pt(8.0),
            (self.bounds.width() - label_w).max(0.0),
            cx.pt(16.0),
        );
        if track_r.width() <= 0.0 {
            return;
        }
        let max = self.ranges[2].max(0.001);
        let px = |v: f32| track_r.min_x() + (v / max).clamp(0.0, 1.0) * track_r.width();

        // Qualitative bands — darkest at the outside.
        let shape = martensite_core::shape::Shape::rounded(cx.pt(3.0));
        cx.list.push_fill_shape(f(track_r), &shape, track);
        let ok_r = Rect::new(
            track_r.min_x(),
            track_r.min_y(),
            px(self.ranges[1]) - track_r.min_x(),
            track_r.height(),
        );
        cx.list
            .push_fill_shape(f(ok_r), &shape, [band[0], band[1], band[2], 160]);
        let poor_r = Rect::new(
            track_r.min_x(),
            track_r.min_y(),
            px(self.ranges[0]) - track_r.min_x(),
            track_r.height(),
        );
        cx.list
            .push_fill_shape(f(poor_r), &shape, [band[0], band[1], band[2], 220]);

        // Measure bar.
        let bar_h = cx.pt(BAR_PT);
        let bar = Rect::new(
            track_r.min_x(),
            track_r.min_y() + (track_r.height() - bar_h) / 2.0,
            (px(self.value) - track_r.min_x()).max(0.0),
            bar_h,
        );
        cx.list.push_fill_shape(
            f(bar),
            &martensite_core::shape::Shape::rounded(cx.pt(1.5)),
            value_c,
        );

        // Target tick.
        let tick_w = cx.pt(2.5);
        let tick = kurbo::Rect::new(
            f64::from(px(self.target) - tick_w / 2.0),
            f64::from(track_r.min_y() - cx.pt(2.0)),
            f64::from(px(self.target) + tick_w / 2.0),
            f64::from(track_r.max_y() + cx.pt(2.0)),
        );
        cx.list.push_fill_rect(tick, target_c);

        // Label + value text.
        let size = 10.0 * cx.scale;
        let label_r = Rect::new(
            self.bounds.min_x(),
            self.bounds.min_y(),
            label_w - cx.pt(6.0),
            self.bounds.height(),
        );
        let ly = label_r.min_y() + (label_r.height() - size * 1.2) / 2.0;
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            f(label_r),
            kurbo::Point::new(f64::from(label_r.min_x()), f64::from(ly)),
            &self.label,
            size,
            muted,
        );
        let val = format!("{:.0}", self.value);
        let vw = painter
            .and_then(|p| p.measure_text(&val, size))
            .unwrap_or(val.chars().count() as f32 * size * 0.6);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            f(Rect::new(
                track_r.max_x() - vw - cx.pt(2.0),
                track_r.min_y() - size - cx.pt(3.0),
                vw + cx.pt(4.0),
                size * 1.4,
            )),
            kurbo::Point::new(
                f64::from(track_r.max_x() - vw),
                f64::from(track_r.min_y() - size - cx.pt(2.0)),
            ),
            &val,
            size,
            if self.hovered { fg } else { muted },
        );
    }
}

impl std::fmt::Debug for BulletChart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BulletChart")
            .field("value", &self.value)
            .field("target", &self.target)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut BulletChart, w: f32, h: f32) {
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
    fn builders() {
        let c = BulletChart::new()
            .label("Sales")
            .value(60.0)
            .target(80.0)
            .ranges([30.0, 60.0, 100.0]);
        assert_eq!(c.label, "Sales");
        assert_eq!(c.measure_value(), 60.0);
        assert_eq!(c.target_value(), 80.0);
        assert_eq!(c.max(), 100.0);
    }

    #[test]
    fn measure_respects_constraints() {
        let mut c = BulletChart::new();
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let size = c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, 20.0),
            },
        );
        assert!(size.x <= 100.0 && size.y <= 20.0);
    }

    #[test]
    fn hover_tracks() {
        let mut c = BulletChart::new();
        laid_out(&mut c, 300.0, 40.0);
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(100.0, 20.0),
            },
            bounds: Rect::new(0.0, 0.0, 300.0, 40.0),
            scale: 1.0,
        });
        assert!(c.hovered);
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(500.0, 500.0),
            },
            bounds: Rect::new(0.0, 0.0, 300.0, 40.0),
            scale: 1.0,
        });
        assert!(!c.hovered);
    }

    #[test]
    fn paint_smoke() {
        let mut c = BulletChart::new().value(50.0).target(75.0);
        laid_out(&mut c, 300.0, 40.0);
        // Painting is covered by render tests; this just proves the
        // widget lays out without degenerate rects.
        assert!(c.bounds.width() > 0.0);
    }
}
