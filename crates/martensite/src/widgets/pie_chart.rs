//! `PieChart` — proportional wedges in a circle (Ant `Pie`, WinUI
//! ring chart, Swift Charts `SectorMark`).
//!
//! [`PieSlice`]s paint clockwise from 12 o'clock in declaration order,
//! scaled to their share of the total. [`PieChart::donut`] cuts a
//! center hole for the modern ring look. Pointer hit-testing maps the
//! angle+radius to a slice — hover highlights, press parks the index
//! in [`PieChart::take_selected`].
//!
//! Colors come from a built-in categorical palette unless a slice sets
//! its own; a legend is deliberately app-space (compose a `ListView`
//! or `Descriptions` next to it).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::pie_chart::{PieChart, PieSlice};
//!
//! let chart = PieChart::new(vec![
//!     PieSlice::new(40.0, "Alpha"),
//!     PieSlice::new(60.0, "Beta"),
//! ])
//! .donut();
//! assert_eq!(chart.slices.len(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, SemanticAction, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use std::f32::consts::TAU;

/// Categorical palette (Ant/material chart hues).
const PALETTE: [[u8; 4]; 8] = [
    [24, 144, 255, 255], // blue
    [19, 194, 194, 255], // cyan
    [82, 196, 26, 255],  // green
    [250, 173, 20, 255], // gold
    [250, 84, 28, 255],  // orange
    [245, 34, 45, 255],  // red
    [114, 46, 209, 255], // purple
    [235, 47, 150, 255], // magenta
];
const RING_PT: f32 = 2.0;

/// One wedge — `value` is a share, not a percentage.
///
/// ```
/// use martensite::widgets::pie_chart::PieSlice;
///
/// let s = PieSlice::new(3.0, "Slice");
/// assert_eq!(s.label, "Slice");
/// ```
pub struct PieSlice {
    /// Proportional weight (any positive number).
    pub value: f32,
    /// Accessible label and tooltip text.
    pub label: String,
    /// Explicit fill; `None` uses the categorical palette.
    pub color: Option<[u8; 4]>,
}

impl PieSlice {
    /// Creates a slice.
    ///
    /// ```
    /// use martensite::widgets::pie_chart::PieSlice;
    ///
    /// let s = PieSlice::new(5.0, "S");
    /// assert_eq!(s.value, 5.0);
    /// ```
    pub fn new(value: f32, label: impl Into<String>) -> Self {
        Self {
            value: value.max(0.0),
            label: label.into(),
            color: None,
        }
    }

    /// Overrides the palette color.
    ///
    /// ```
    /// use martensite::widgets::pie_chart::PieSlice;
    ///
    /// let s = PieSlice::new(1.0, "S").color([255, 0, 0, 255]);
    /// assert_eq!(s.color, Some([255, 0, 0, 255]));
    /// ```
    pub fn color(mut self, c: [u8; 4]) -> Self {
        self.color = Some(c);
        self
    }
}

/// A pie/donut chart — see the module docs.
///
/// ```
/// use martensite::widgets::pie_chart::PieChart;
///
/// let c = PieChart::new(vec![]);
/// assert!(!c.is_donut());
/// ```
pub struct PieChart {
    /// Slices in draw order (clockwise from 12 o'clock).
    pub slices: Vec<PieSlice>,
    /// When `true`, a center hole is cut (ring chart).
    pub donut_mode: bool,
    /// Hole radius as a fraction of the chart radius (donut only).
    pub hole: f32,
    /// When `false` the chart is inert.
    pub enabled: bool,
    selected: Option<usize>,
    hovered: Option<usize>,
    bounds: Rect,
    scale: f32,
}

impl PieChart {
    /// Creates a full pie.
    ///
    /// ```
    /// use martensite::widgets::pie_chart::{PieChart, PieSlice};
    ///
    /// let c = PieChart::new(vec![PieSlice::new(1.0, "A")]);
    /// assert_eq!(c.slices.len(), 1);
    /// ```
    pub fn new(slices: Vec<PieSlice>) -> Self {
        Self {
            slices,
            donut_mode: false,
            hole: 0.55,
            enabled: true,
            selected: None,
            hovered: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Renders as a ring chart (hole radius from [`Self::hole`]).
    ///
    /// ```
    /// use martensite::widgets::pie_chart::{PieChart, PieSlice};
    ///
    /// let c = PieChart::new(vec![PieSlice::new(1.0, "A")]).donut();
    /// assert!(c.is_donut());
    /// ```
    pub fn donut(mut self) -> Self {
        self.donut_mode = true;
        self
    }

    /// Sets the donut hole fraction (`0..1`).
    ///
    /// ```
    /// use martensite::widgets::pie_chart::{PieChart, PieSlice};
    ///
    /// let c = PieChart::new(vec![PieSlice::new(1.0, "A")]).donut().hole(0.7);
    /// assert_eq!(c.hole, 0.7);
    /// ```
    pub fn hole(mut self, hole: f32) -> Self {
        self.hole = hole.clamp(0.1, 0.9);
        self
    }

    /// Enables or disables the chart.
    ///
    /// ```
    /// use martensite::widgets::pie_chart::{PieChart, PieSlice};
    ///
    /// let c = PieChart::new(vec![PieSlice::new(1.0, "A")]).enabled(false);
    /// assert!(!c.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Whether the chart renders as a ring.
    ///
    /// ```
    /// use martensite::widgets::pie_chart::{PieChart, PieSlice};
    ///
    /// let c = PieChart::new(vec![PieSlice::new(1.0, "A")]).donut();
    /// assert!(c.is_donut());
    /// ```
    pub fn is_donut(&self) -> bool {
        self.donut_mode
    }

    /// Drains the last slice-pressed index.
    ///
    /// ```
    /// use martensite::widgets::pie_chart::PieChart;
    ///
    /// let mut c = PieChart::new(vec![]);
    /// assert_eq!(c.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.selected.take()
    }

    /// Center and radius of the drawn circle.
    fn circle(&self) -> (Vec2, f32) {
        let c = self.bounds.origin + self.bounds.size / 2.0;
        (
            c,
            (self.bounds.size.x.min(self.bounds.size.y) / 2.0).max(0.0),
        )
    }

    /// `[start, end)` angle in radians for slice `i` (12-o'clock
    /// start, clockwise).
    fn span(&self, index: usize) -> (f32, f32) {
        let total: f32 = self.slices.iter().map(|s| s.value).sum();
        if total <= 0.0 {
            return (0.0, 0.0);
        }
        let mut a = -std::f32::consts::FRAC_PI_2;
        for (i, s) in self.slices.iter().enumerate() {
            let sweep = s.value / total * TAU;
            if i == index {
                return (a, a + sweep);
            }
            a += sweep;
        }
        (0.0, 0.0)
    }

    /// Hit-test: slice index under `position`, or `None`.
    fn hit(&self, position: Vec2) -> Option<usize> {
        if self.slices.is_empty() {
            return None;
        }
        let (center, r) = self.circle();
        if r <= 0.0 {
            return None;
        }
        let d = position - center;
        let dist = d.length();
        if dist > r || (self.donut_mode && dist < r * self.hole) {
            return None;
        }
        let mut angle = d.y.atan2(d.x); // -π..π, 0 = +x
        if angle < -std::f32::consts::FRAC_PI_2 {
            angle += TAU;
        }
        for (i, _) in self.slices.iter().enumerate() {
            let (a0, a1) = self.span(i);
            if angle >= a0 && angle < a1 {
                return Some(i);
            }
        }
        // Wraparound safety for the last slice crossing -π/2 + τ.
        (!self.slices.is_empty()).then_some(self.slices.len() - 1)
    }

    /// Wedge path for slice `i` — sampled arc, closed through the
    /// inner radius (donut) or center (pie).
    fn wedge(&self, index: usize) -> kurbo::BezPath {
        let (center, r) = self.circle();
        let (a0, a1) = self.span(index);
        let inner = if self.donut_mode { r * self.hole } else { 0.0 };
        let steps = (((a1 - a0).abs() / TAU) * 64.0).ceil().max(2.0) as usize;
        let pt = |a: f32, rad: f32| {
            kurbo::Point::new(
                f64::from(center.x + a.cos() * rad),
                f64::from(center.y + a.sin() * rad),
            )
        };
        let mut els = Vec::with_capacity(steps * 2 + 3);
        els.push(kurbo::PathEl::MoveTo(pt(a0, r)));
        for s in 1..=steps {
            els.push(kurbo::PathEl::LineTo(pt(
                a0 + (a1 - a0) * s as f32 / steps as f32,
                r,
            )));
        }
        if inner > 0.0 {
            els.push(kurbo::PathEl::LineTo(pt(a1, inner)));
            for s in (0..steps).rev() {
                els.push(kurbo::PathEl::LineTo(pt(
                    a0 + (a1 - a0) * s as f32 / steps as f32,
                    inner,
                )));
            }
        } else {
            els.push(kurbo::PathEl::LineTo(kurbo::Point::new(
                f64::from(center.x),
                f64::from(center.y),
            )));
        }
        els.push(kurbo::PathEl::ClosePath);
        kurbo::BezPath::from_vec(els)
    }
}

impl Widget for PieChart {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let side = cx.pt(120.0);
        Vec2::new(
            side.min(constraints.max_size.x.max(0.0)),
            side.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(48.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        let total: f32 = self.slices.iter().map(|s| s.value).sum();
        let desc = self
            .slices
            .iter()
            .map(|s| {
                format!(
                    "{} {:.0}%",
                    s.label,
                    if total > 0.0 {
                        s.value / total * 100.0
                    } else {
                        0.0
                    }
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        node.set_label("Pie chart");
        node.set_description(desc);
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.hit(*position);
                if h != self.hovered {
                    self.hovered = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.is_some() {
                    self.hovered = None;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => match self.hit(*position) {
                Some(i) => {
                    self.selected = Some(i);
                    EventResponse::RequestRepaint
                }
                None => EventResponse::Ignored,
            },
            WidgetEvent::SemanticAction(SemanticAction::Increment)
            | WidgetEvent::SemanticAction(SemanticAction::Decrement) => {
                if self.slices.is_empty() {
                    return EventResponse::Ignored;
                }
                let cur = self.hovered.unwrap_or(0);
                let next = if matches!(
                    cx.event,
                    WidgetEvent::SemanticAction(SemanticAction::Increment)
                ) {
                    (cur + 1) % self.slices.len()
                } else {
                    (cur + self.slices.len() - 1) % self.slices.len()
                };
                self.hovered = Some(next);
                self.selected = Some(next);
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        if self.slices.is_empty() {
            return;
        }
        let t = cx.pt(RING_PT);
        for (i, s) in self.slices.iter().enumerate() {
            let path = self.wedge(i);
            let base = s.color.unwrap_or(PALETTE[i % PALETTE.len()]);
            let mut color = cx.color(TokenKey::AccentColor, base);
            if self.hovered == Some(i) {
                // Lighten toward white for the hover cue.
                for c in color.iter_mut().take(3) {
                    *c = (*c as u16 + (255 - *c as u16) / 3) as u8;
                }
            }
            cx.list.push_path(path.clone(), color);
            cx.list.push_stroke_path(
                path,
                t,
                cx.color(TokenKey::SurfaceColor, [255, 255, 255, 255]),
            );
        }
    }
}

impl std::fmt::Debug for PieChart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PieChart")
            .field("slices", &self.slices.len())
            .field("donut", &self.donut_mode)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut PieChart, w: f32, h: f32) {
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

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 200.0, 200.0),
            scale: 1.0,
        }
    }

    #[test]
    fn spans_partition_circle() {
        let c = PieChart::new(vec![
            PieSlice::new(1.0, "a"),
            PieSlice::new(1.0, "b"),
            PieSlice::new(2.0, "c"),
        ]);
        let (a0, a1) = c.span(0);
        let (_, b1) = c.span(1);
        let (_, c1) = c.span(2);
        assert!((a1 - a0 - TAU / 4.0).abs() < 1e-4);
        assert!((c1 - a0 - TAU).abs() < 1e-4);
        let _ = b1;
    }

    #[test]
    fn hit_maps_angle_to_slice() {
        let mut c = PieChart::new(vec![
            PieSlice::new(1.0, "top"),
            PieSlice::new(1.0, "bottom"),
        ]);
        laid_out(&mut c, 200.0, 200.0);
        // Center (100,100). First slice spans -90°..+90° (right half).
        assert_eq!(c.hit(Vec2::new(160.0, 100.0)), Some(0));
        assert_eq!(c.hit(Vec2::new(40.0, 100.0)), Some(1));
        assert_eq!(c.hit(Vec2::new(300.0, 100.0)), None);
    }

    #[test]
    fn donut_hole_misses() {
        let mut c = PieChart::new(vec![PieSlice::new(1.0, "a")]).donut();
        laid_out(&mut c, 200.0, 200.0);
        assert_eq!(c.hit(Vec2::new(100.0, 100.0)), None); // inside hole
        assert_eq!(c.hit(Vec2::new(190.0, 100.0)), Some(0)); // on ring
    }

    #[test]
    fn press_parks_selection() {
        let mut c = PieChart::new(vec![PieSlice::new(1.0, "a"), PieSlice::new(1.0, "b")]);
        laid_out(&mut c, 200.0, 200.0);
        c.event(&mut ev(&WidgetEvent::PointerPressed {
            position: Vec2::new(40.0, 100.0),
            button: PointerButton::Primary,
            count: 1,
        }));
        assert_eq!(c.take_selected(), Some(1));
        assert_eq!(c.take_selected(), None);
    }

    #[test]
    fn empty_is_inert() {
        let mut c = PieChart::new(vec![]);
        laid_out(&mut c, 200.0, 200.0);
        assert_eq!(
            c.event(&mut ev(&WidgetEvent::PointerPressed {
                position: Vec2::new(100.0, 100.0),
                button: PointerButton::Primary,
                count: 1,
            })),
            EventResponse::Ignored
        );
    }
}
