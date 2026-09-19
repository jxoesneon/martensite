//! `FunnelChart` — the conversion-funnel chart (Ant `Funnel` /
//! Salesforce pipeline idiom).
//!
//! Stages paint as a vertical stack of centered trapezoids: each
//! stage's top width is proportional to its own value and its
//! bottom width to the next stage's, producing the classic tapered
//! funnel. Stage labels and values paint beside each band when a
//! text painter is present; hovering a stage parks its index in
//! [`FunnelChart::take_hovered`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::funnel_chart::FunnelChart;
//!
//! let funnel = FunnelChart::new()
//!     .stage("Visits", 1000.0)
//!     .stage("Signups", 320.0)
//!     .stage("Paid", 90.0);
//! assert_eq!(funnel.stage_count(), 3);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const WIDTH_PT: f32 = 280.0;
const HEIGHT_PT: f32 = 200.0;

const PALETTE: [[u8; 4]; 6] = [
    [90, 140, 220, 255],
    [110, 180, 130, 255],
    [230, 170, 80, 255],
    [210, 110, 90, 255],
    [150, 110, 200, 255],
    [90, 180, 190, 255],
];
const FG: [u8; 4] = [230, 230, 235, 255];
const MUTED: [u8; 4] = [140, 140, 148, 255];
const TRACK: [u8; 4] = [48, 48, 52, 255];

/// A conversion-funnel chart — see the module docs.
///
/// ```
/// use martensite::widgets::funnel_chart::FunnelChart;
///
/// let funnel = FunnelChart::new().stage("A", 10.0).stage("B", 5.0);
/// assert_eq!(funnel.stage_count(), 2);
/// ```
pub struct FunnelChart {
    /// Accessibility label.
    pub label: String,
    stages: Vec<(String, f32)>,
    hovered: Option<usize>,
    pending_hover: Option<usize>,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
}

impl Default for FunnelChart {
    fn default() -> Self {
        Self::new()
    }
}

impl FunnelChart {
    /// Creates an empty funnel.
    ///
    /// ```
    /// use martensite::widgets::funnel_chart::FunnelChart;
    ///
    /// assert_eq!(FunnelChart::new().stage_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Funnel".to_string(),
            stages: Vec::new(),
            hovered: None,
            pending_hover: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::funnel_chart::FunnelChart;
    ///
    /// let funnel = FunnelChart::new().label("Sales");
    /// assert_eq!(funnel.label, "Sales");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Appends a stage.
    ///
    /// ```
    /// use martensite::widgets::funnel_chart::FunnelChart;
    ///
    /// let funnel = FunnelChart::new().stage("Leads", 500.0);
    /// assert_eq!(funnel.stage_count(), 1);
    /// ```
    pub fn stage(mut self, name: impl Into<String>, value: f32) -> Self {
        self.stages.push((name.into(), value.max(0.0)));
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::funnel_chart::FunnelChart;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let funnel = FunnelChart::new().with_text_painter(shared_painter());
    /// assert_eq!(funnel.stage_count(), 0);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Stage count.
    ///
    /// ```
    /// use martensite::widgets::funnel_chart::FunnelChart;
    ///
    /// assert_eq!(FunnelChart::new().stage_count(), 0);
    /// ```
    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }

    /// Stage `(name, value)` pairs.
    ///
    /// ```
    /// use martensite::widgets::funnel_chart::FunnelChart;
    ///
    /// let funnel = FunnelChart::new().stage("A", 1.0);
    /// assert_eq!(funnel.stages()[0].0, "A");
    /// ```
    pub fn stages(&self) -> &[(String, f32)] {
        &self.stages
    }

    /// Drains the stage index hovered since the last drain.
    ///
    /// ```
    /// use martensite::widgets::funnel_chart::FunnelChart;
    ///
    /// let mut funnel = FunnelChart::new();
    /// assert!(funnel.take_hovered().is_none());
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.pending_hover.take()
    }

    /// Stage index at a device-space point.
    fn stage_at(&self, p: Vec2) -> Option<usize> {
        if !self.bounds.contains(p) || self.stages.is_empty() {
            return None;
        }
        let h = self.bounds.height() / self.stages.len() as f32;
        let idx = ((p.y - self.bounds.min_y()) / h) as usize;
        Some(idx.min(self.stages.len() - 1))
    }
}

impl Widget for FunnelChart {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(96.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.clone());
        let summary = self
            .stages
            .iter()
            .map(|(n, v)| format!("{n} {v:.0}"))
            .collect::<Vec<_>>()
            .join(", ");
        node.set_description(summary);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.stage_at(*position);
                if hit != self.hovered {
                    self.hovered = hit;
                    if hit.is_some() {
                        self.pending_hover = hit;
                    }
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
        cx.list
            .push_fill_rect(f(self.bounds), cx.color(TokenKey::SurfaceColor, TRACK));
        if self.stages.is_empty() {
            return;
        }
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let fg = cx.color(TokenKey::TextColor, FG);
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        let max = self
            .stages
            .iter()
            .map(|s| s.1)
            .fold(0.0f32, f32::max)
            .max(0.001);
        // Right ~35% reserved for labels.
        let chart_w = self.bounds.width() * 0.62;
        let cx_mid = self.bounds.min_x() + chart_w / 2.0;
        let n = self.stages.len();
        let band_h = self.bounds.height() / n as f32;
        let gap = cx.pt(2.0);

        for (i, (name, value)) in self.stages.iter().enumerate() {
            let top_w = (value / max) * chart_w;
            let next = self.stages.get(i + 1).map(|s| s.1).unwrap_or(0.0);
            let bot_w = (next / max) * chart_w;
            let y0 = self.bounds.min_y() + i as f32 * band_h;
            let y1 = y0 + band_h - gap;
            let mut path = kurbo::BezPath::new();
            path.move_to((f64::from(cx_mid - top_w / 2.0), f64::from(y0)));
            path.line_to((f64::from(cx_mid + top_w / 2.0), f64::from(y0)));
            path.line_to((f64::from(cx_mid + bot_w / 2.0), f64::from(y1)));
            path.line_to((f64::from(cx_mid - bot_w / 2.0), f64::from(y1)));
            path.close_path();
            let mut color = PALETTE[i % PALETTE.len()];
            if self.hovered == Some(i) {
                color = [
                    color[0].saturating_add(30),
                    color[1].saturating_add(30),
                    color[2].saturating_add(30),
                    255,
                ];
            }
            cx.list.push_path(path, color);

            // "name — value" to the right of the funnel.
            let size = 9.5 * cx.scale;
            let text = format!("{name} — {value:.0}");
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                f(Rect::new(
                    self.bounds.min_x() + chart_w + cx.pt(6.0),
                    y0,
                    self.bounds.width() - chart_w - cx.pt(8.0),
                    band_h,
                )),
                kurbo::Point::new(
                    f64::from(self.bounds.min_x() + chart_w + cx.pt(6.0)),
                    f64::from(y0 + (band_h - size * 1.2) / 2.0),
                ),
                &text,
                size,
                if self.hovered == Some(i) { fg } else { muted },
            );
        }
    }
}

impl std::fmt::Debug for FunnelChart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunnelChart")
            .field("stages", &self.stages.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut FunnelChart, w: f32, h: f32) {
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

    fn funnel() -> FunnelChart {
        FunnelChart::new()
            .stage("Visits", 1000.0)
            .stage("Signups", 320.0)
            .stage("Paid", 90.0)
    }

    #[test]
    fn stages_accumulate() {
        let c = funnel();
        assert_eq!(c.stage_count(), 3);
        assert_eq!(c.stages()[1].0, "Signups");
    }

    #[test]
    fn negative_values_clamp() {
        let c = FunnelChart::new().stage("A", -5.0);
        assert_eq!(c.stages()[0].1, 0.0);
    }

    #[test]
    fn hover_parks_stage() {
        let mut c = funnel();
        laid_out(&mut c, 300.0, 150.0);
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(50.0, 25.0), // first band (150/3 = 50px bands)
            },
            bounds: Rect::new(0.0, 0.0, 300.0, 150.0),
            scale: 1.0,
        });
        assert_eq!(c.take_hovered(), Some(0));
        assert!(c.take_hovered().is_none());
    }

    #[test]
    fn hover_leaving_clears() {
        let mut c = funnel();
        laid_out(&mut c, 300.0, 150.0);
        for p in [Vec2::new(50.0, 25.0), Vec2::new(500.0, 500.0)] {
            c.event(&mut EventContext {
                event: &WidgetEvent::PointerMoved { position: p },
                bounds: Rect::new(0.0, 0.0, 300.0, 150.0),
                scale: 1.0,
            });
        }
        assert_eq!(c.hovered, None);
    }

    #[test]
    fn empty_hover_safe() {
        let mut c = FunnelChart::new();
        laid_out(&mut c, 300.0, 150.0);
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(50.0, 25.0),
            },
            bounds: Rect::new(0.0, 0.0, 300.0, 150.0),
            scale: 1.0,
        });
        assert!(c.take_hovered().is_none());
    }
}
