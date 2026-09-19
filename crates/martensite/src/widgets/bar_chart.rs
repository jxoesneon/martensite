//! `BarChart` — categorical column chart.
//!
//! The categorical companion to [`Sparkline`](crate::widgets::sparkline::Sparkline)
//! (which is a continuous trend line): labeled columns scaled to
//! `max`, an optional baseline axis, and per-bar accent coloring.
//! Display-only — the small-multiples KPI block idiom, not an
//! interactive charting surface.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::bar_chart::BarChart;
//!
//! let c = BarChart::new()
//!     .bar("Q1", 12.0)
//!     .bar("Q2", 30.0)
//!     .bar("Q3", 18.0);
//! assert_eq!(c.bar_count(), 3);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

/// Bar ink.
const BAR: TokenKey = TokenKey::AccentColor;
/// Axis ink.
const AXIS: TokenKey = TokenKey::DividerColor;
/// Label ink.
const LABEL: TokenKey = TokenKey::TextMutedColor;
/// Label font size (logical points).
const LABEL_PT: f32 = 9.0;
/// Bar gap fraction of the slot width.
const GAP_FRAC: f32 = 0.35;
/// Fallback bar ink.
const FALLBACK_BAR: [u8; 4] = [50, 115, 230, 255];
/// Fallback axis ink.
const FALLBACK_AXIS: [u8; 4] = [208, 211, 217, 255];

/// One categorical bar.
#[derive(Debug, Clone)]
struct Bar {
    label: Option<String>,
    value: f32,
    color: Option<[u8; 4]>,
}

/// A categorical column chart — see the module docs. Leaf widget.
///
/// # Examples
///
/// ```
/// use martensite::widgets::bar_chart::BarChart;
/// use martensite::core::Widget;
///
/// assert_eq!(BarChart::new().child_count(), 0);
/// ```
pub struct BarChart {
    bars: Vec<Bar>,
    /// Whether the baseline axis paints (default `true`).
    pub axis: bool,
    /// Whether category labels paint under the bars (default `true`
    /// when labels exist).
    pub labels: bool,
    /// Accessibility label.
    pub label: Option<String>,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl BarChart {
    /// An empty chart.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::bar_chart::BarChart;
    ///
    /// assert_eq!(BarChart::new().bar_count(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            bars: Vec::new(),
            axis: true,
            labels: true,
            label: None,
            text_painter: None,
        }
    }

    /// Appends an unlabeled bar.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::bar_chart::BarChart;
    ///
    /// assert_eq!(BarChart::new().bar("a", 5.0).bar_count(), 1);
    /// ```
    #[must_use]
    pub fn bar(mut self, label: impl Into<String>, value: f32) -> Self {
        self.bars.push(Bar {
            label: Some(label.into()),
            value,
            color: None,
        });
        self
    }

    /// Appends a bar with an explicit color.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::bar_chart::BarChart;
    ///
    /// let c = BarChart::new().bar_colored("x", 2.0, [200, 60, 60, 255]);
    /// ```
    #[must_use]
    pub fn bar_colored(mut self, label: impl Into<String>, value: f32, color: [u8; 4]) -> Self {
        self.bars.push(Bar {
            label: Some(label.into()),
            value,
            color: Some(color),
        });
        self
    }

    /// Sets bars from `(label, value)` pairs (replaces existing).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::bar_chart::BarChart;
    ///
    /// let c = BarChart::new().bars([("a", 1.0), ("b", 2.0)]);
    /// assert_eq!(c.bar_count(), 2);
    /// ```
    #[must_use]
    pub fn bars(mut self, bars: impl IntoIterator<Item = (impl Into<String>, f32)>) -> Self {
        self.bars = bars
            .into_iter()
            .map(|(l, v)| Bar {
                label: Some(l.into()),
                value: v,
                color: None,
            })
            .collect();
        self
    }

    /// Sets the accessibility label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::bar_chart::BarChart;
    ///
    /// let c = BarChart::new().label("Quarterly revenue");
    /// ```
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// Overrides the shaped-text painter (tests and tooling).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::bar_chart::BarChart;
    ///
    /// let c = BarChart::new();
    /// ```
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Bar count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::bar_chart::BarChart;
    ///
    /// assert_eq!(BarChart::new().bar_count(), 0);
    /// ```
    pub fn bar_count(&self) -> usize {
        self.bars.len()
    }

    /// The largest bar value (≥1e-6 so scaling never divides by
    /// zero).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::bar_chart::BarChart;
    ///
    /// let c = BarChart::new().bar("a", 4.0).bar("b", 8.0);
    /// assert_eq!(c.max_value(), 8.0);
    /// ```
    pub fn max_value(&self) -> f32 {
        self.bars
            .iter()
            .map(|b| b.value.max(0.0))
            .fold(0.0_f32, f32::max)
            .max(1e-6)
    }
}

impl Default for BarChart {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for BarChart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BarChart")
            .field("bars", &self.bars.len())
            .finish()
    }
}

impl Widget for BarChart {
    fn measure(&mut self, _cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(160.0, 80.0)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let label_h = if self.labels && self.bars.iter().any(|b| b.label.is_some()) {
            cx.pt(LABEL_PT) + cx.pt(4.0)
        } else {
            0.0
        };
        let axis_h = if self.axis { cx.pt(1.0) } else { 0.0 };
        let chart_h = (b.height() - label_h - axis_h).max(0.0);
        let n = self.bars.len();
        if n == 0 {
            return;
        }
        let slot = b.width() / n as f32;
        let gap = slot * GAP_FRAC;
        let bar_w = (slot - gap).max(1.0);
        let max = self.max_value();
        let bar_ink = cx.color(BAR, FALLBACK_BAR);
        let baseline_y = b.min_y() + chart_h;

        for (i, bar) in self.bars.iter().enumerate() {
            let x = b.min_x() + i as f32 * slot + gap / 2.0;
            let h = (bar.value.max(0.0) / max * chart_h).max(if bar.value > 0.0 {
                cx.pt(1.0)
            } else {
                0.0
            });
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(x),
                    f64::from(baseline_y - h),
                    f64::from(x + bar_w),
                    f64::from(baseline_y),
                ),
                &martensite_core::shape::Shape::rounded(cx.pt(2.0)),
                bar.color.unwrap_or(bar_ink),
            );
            if self.labels {
                if let Some(lbl) = &bar.label {
                    let size = cx.pt(LABEL_PT);
                    let w = painter
                        .and_then(|p| p.measure_text(lbl, size))
                        .unwrap_or(size * lbl.len() as f32 * 0.55)
                        .min(slot);
                    crate::text_paint::paint_label_clipped(
                        painter,
                        cx.list,
                        kurbo::Rect::new(
                            f64::from(b.min_x() + i as f32 * slot),
                            f64::from(baseline_y + axis_h),
                            f64::from(b.min_x() + (i + 1) as f32 * slot),
                            f64::from(b.max_y()),
                        ),
                        kurbo::Point::new(
                            f64::from(b.min_x() + i as f32 * slot + (slot - w) / 2.0),
                            f64::from(baseline_y + axis_h + cx.pt(2.0)),
                        ),
                        lbl,
                        size,
                        cx.color(LABEL, [110, 114, 123, 255]),
                    );
                }
            }
        }
        if self.axis {
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(b.min_x()),
                    f64::from(baseline_y),
                    f64::from(b.max_x()),
                    f64::from(baseline_y + axis_h),
                ),
                cx.color(AXIS, FALLBACK_AXIS),
            );
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        // Summarize the series for AT users.
        let summary = self
            .bars
            .iter()
            .map(|b| format!("{} {}", b.label.as_deref().unwrap_or("bar"), b.value))
            .collect::<Vec<_>>()
            .join(", ");
        if !summary.is_empty() {
            node.set_value(summary);
        }
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 24.0)).with_policy(UnderflowPolicy::Lint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn max_value_scales_to_largest() {
        let c = BarChart::new().bar("a", 3.0).bar("b", 9.0).bar("c", 6.0);
        assert_eq!(c.max_value(), 9.0);
    }

    #[test]
    fn max_value_floors_on_empty() {
        assert_eq!(BarChart::new().max_value(), 1e-6);
    }

    #[test]
    fn negative_values_clamp_to_zero() {
        let c = BarChart::new().bar("a", -5.0).bar("b", 2.0);
        assert_eq!(c.max_value(), 2.0);
    }

    #[test]
    fn bars_builder_replaces() {
        let c = BarChart::new()
            .bar("old", 1.0)
            .bars([("x", 5.0), ("y", 6.0)]);
        assert_eq!(c.bar_count(), 2);
    }

    #[test]
    fn measure_reports_default_size() {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let mut c = BarChart::new();
        let s = c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(400.0, 400.0),
            },
        );
        assert!(s.x > 0.0 && s.y > 0.0);
    }
}
