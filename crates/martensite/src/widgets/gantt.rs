//! `Gantt` — a horizontal task-bar timeline (MS Project /
//! enterprise Gantt idiom).
//!
//! Each [`GanttTask`] paints as a rounded bar positioned by
//! `start_day`/`duration` across a day-scale axis; task names list
//! down the left column and day ticks run along the bottom.
//! `progress` shades a fraction of the bar. Hovering a row parks
//! its index in [`Gantt::take_hovered`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::gantt::Gantt;
//!
//! let g = Gantt::new().task("Design", 0.0, 3.0).task("Build", 3.0, 5.0);
//! assert_eq!(g.task_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const WIDTH_PT: f32 = 420.0;
const ROW_PT: f32 = 24.0;
const LABEL_PT: f32 = 110.0;
const AXIS_PT: f32 = 18.0;

const PALETTE: [[u8; 4]; 6] = [
    [90, 140, 220, 255],
    [110, 180, 130, 255],
    [230, 170, 80, 255],
    [210, 110, 90, 255],
    [150, 110, 200, 255],
    [90, 180, 190, 255],
];
const TRACK: [u8; 4] = [48, 48, 52, 255];
const GRID: [u8; 4] = [70, 70, 76, 255];
const FG: [u8; 4] = [230, 230, 235, 255];
const MUTED: [u8; 4] = [140, 140, 148, 255];

/// One task bar — name, start day, duration in days, 0..1 progress.
#[derive(Clone, Debug, PartialEq)]
pub struct GanttTask {
    /// Task name (left column).
    pub name: String,
    /// Day offset from the chart origin.
    pub start: f32,
    /// Duration in days.
    pub duration: f32,
    /// Completion fraction `0..=1` (shaded bar segment).
    pub progress: f32,
}

/// A task-bar timeline — see the module docs.
///
/// ```
/// use martensite::widgets::gantt::Gantt;
///
/// assert_eq!(Gantt::new().task_count(), 0);
/// ```
pub struct Gantt {
    /// Accessibility label.
    pub label: String,
    tasks: Vec<GanttTask>,
    total_days: f32,
    hovered: Option<usize>,
    pending_hover: Option<usize>,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
}

impl Default for Gantt {
    fn default() -> Self {
        Self::new()
    }
}

impl Gantt {
    /// Creates an empty chart spanning 30 days.
    ///
    /// ```
    /// use martensite::widgets::gantt::Gantt;
    ///
    /// assert_eq!(Gantt::new().span_days(), 30.0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Schedule".to_string(),
            tasks: Vec::new(),
            total_days: 30.0,
            hovered: None,
            pending_hover: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::gantt::Gantt;
    ///
    /// let g = Gantt::new().label("Roadmap");
    /// assert_eq!(g.label, "Roadmap");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Total axis span in days (bar overflow clamps).
    ///
    /// ```
    /// use martensite::widgets::gantt::Gantt;
    ///
    /// let g = Gantt::new().total_days(14.0);
    /// assert_eq!(g.span_days(), 14.0);
    /// ```
    pub fn total_days(mut self, days: f32) -> Self {
        self.total_days = days.max(1.0);
        self
    }

    /// Appends a task (`start` day, `duration` days).
    ///
    /// ```
    /// use martensite::widgets::gantt::Gantt;
    ///
    /// let g = Gantt::new().task("Plan", 0.0, 5.0);
    /// assert_eq!(g.task_count(), 1);
    /// ```
    pub fn task(mut self, name: impl Into<String>, start: f32, duration: f32) -> Self {
        self.tasks.push(GanttTask {
            name: name.into(),
            start: start.max(0.0),
            duration: duration.max(0.0),
            progress: 0.0,
        });
        self
    }

    /// Sets the last task's completion fraction.
    ///
    /// ```
    /// use martensite::widgets::gantt::Gantt;
    ///
    /// let g = Gantt::new().task("Plan", 0.0, 5.0).progress(0.5);
    /// assert_eq!(g.tasks()[0].progress, 0.5);
    /// ```
    pub fn progress(mut self, p: f32) -> Self {
        if let Some(t) = self.tasks.last_mut() {
            t.progress = p.clamp(0.0, 1.0);
        }
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::gantt::Gantt;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let g = Gantt::new().with_text_painter(shared_painter());
    /// assert_eq!(g.task_count(), 0);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Task count.
    ///
    /// ```
    /// use martensite::widgets::gantt::Gantt;
    ///
    /// assert_eq!(Gantt::new().task_count(), 0);
    /// ```
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Axis span in days.
    ///
    /// ```
    /// use martensite::widgets::gantt::Gantt;
    ///
    /// assert_eq!(Gantt::new().span_days(), 30.0);
    /// ```
    pub fn span_days(&self) -> f32 {
        self.total_days
    }

    /// Task list.
    ///
    /// ```
    /// use martensite::widgets::gantt::Gantt;
    ///
    /// let g = Gantt::new().task("A", 1.0, 2.0);
    /// assert_eq!(g.tasks()[0].name, "A");
    /// ```
    pub fn tasks(&self) -> &[GanttTask] {
        &self.tasks
    }

    /// Drains the row index hovered since the last drain.
    ///
    /// ```
    /// use martensite::widgets::gantt::Gantt;
    ///
    /// let mut g = Gantt::new();
    /// assert!(g.take_hovered().is_none());
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.pending_hover.take()
    }

    /// Chart region (right of the label column, above the axis).
    fn chart_rect(&self, cx_scale: f32) -> Rect {
        Rect::new(
            self.bounds.min_x() + LABEL_PT * cx_scale,
            self.bounds.min_y(),
            (self.bounds.width() - LABEL_PT * cx_scale).max(0.0),
            (self.bounds.height() - AXIS_PT * cx_scale).max(0.0),
        )
    }

    /// Row index at a device-space point.
    fn row_at(&self, p: Vec2, cx_scale: f32) -> Option<usize> {
        let chart = self.chart_rect(cx_scale);
        if !self.bounds.contains(p) || self.tasks.is_empty() {
            return None;
        }
        let h = chart.height() / self.tasks.len() as f32;
        if h <= 0.0 || p.y >= chart.max_y() {
            return None;
        }
        let idx = ((p.y - chart.min_y()) / h) as usize;
        Some(idx.min(self.tasks.len() - 1))
    }
}

impl Widget for Gantt {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let rows = (self.tasks.len() as f32 * ROW_PT + AXIS_PT).max(48.0);
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(rows).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.clone());
        node.set_description(format!(
            "{} tasks over {:.0} days",
            self.tasks.len(),
            self.total_days
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.row_at(*position, cx.scale);
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
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let track = cx.color(TokenKey::SurfaceColor, TRACK);
        let grid = cx.color(TokenKey::DividerColor, GRID);
        let fg = cx.color(TokenKey::TextColor, FG);
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        cx.list.push_fill_rect(f(self.bounds), track);

        let chart = self.chart_rect(cx.scale);
        if chart.width() <= 0.0 || self.tasks.is_empty() {
            return;
        }
        let row_h = chart.height() / self.tasks.len() as f32;
        let day_w = chart.width() / self.total_days.max(1.0);
        let day_x = |d: f32| chart.min_x() + d * day_w;

        // Day grid — a tick every max(1, total/8) days.
        let step = (self.total_days / 8.0).ceil().max(1.0);
        let mut d = 0.0;
        while d <= self.total_days {
            let mut v = kurbo::BezPath::new();
            v.move_to((f64::from(day_x(d)), f64::from(chart.min_y())));
            v.line_to((f64::from(day_x(d)), f64::from(chart.max_y())));
            cx.list.push_stroke_path(v, cx.pt(0.5), grid);
            d += step;
        }

        let size = 9.0 * cx.scale;
        for (i, task) in self.tasks.iter().enumerate() {
            let y0 = chart.min_y() + i as f32 * row_h;
            let bar_h = (row_h - cx.pt(8.0)).max(2.0);
            let bar_y = y0 + (row_h - bar_h) / 2.0;
            let x0 = day_x(task.start.min(self.total_days));
            let x1 = day_x((task.start + task.duration).min(self.total_days));
            let mut color = PALETTE[i % PALETTE.len()];
            if self.hovered == Some(i) {
                color = [
                    color[0].saturating_add(30),
                    color[1].saturating_add(30),
                    color[2].saturating_add(30),
                    255,
                ];
            }
            let bar = Rect::new(x0, bar_y, (x1 - x0).max(1.0), bar_h);
            let shape = martensite_core::shape::Shape::rounded(cx.pt(3.0));
            cx.list.push_fill_shape(f(bar), &shape, color);
            if task.progress > 0.0 {
                let fill = Rect::new(
                    bar.min_x(),
                    bar.min_y(),
                    bar.width() * task.progress,
                    bar.height(),
                );
                cx.list.push_fill_shape(
                    f(fill),
                    &shape,
                    [color[0] / 2, color[1] / 2, color[2] / 2, 255],
                );
            }

            // Name in the left column.
            let label_r = Rect::new(
                self.bounds.min_x(),
                y0,
                LABEL_PT * cx.scale - cx.pt(6.0),
                row_h,
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                f(label_r),
                kurbo::Point::new(
                    f64::from(label_r.min_x() + cx.pt(4.0)),
                    f64::from(y0 + (row_h - size * 1.2) / 2.0),
                ),
                &task.name,
                size,
                if self.hovered == Some(i) { fg } else { muted },
            );
        }

        // Axis labels along the bottom.
        let axis_y = chart.max_y() + cx.pt(3.0);
        let mut d = 0.0;
        while d <= self.total_days {
            let s = format!("{d:.0}");
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                f(Rect::new(
                    day_x(d) - cx.pt(12.0),
                    axis_y,
                    cx.pt(24.0),
                    AXIS_PT * cx.scale,
                )),
                kurbo::Point::new(f64::from(day_x(d) - cx.pt(3.0)), f64::from(axis_y)),
                &s,
                size,
                muted,
            );
            d += step;
        }
    }
}

impl std::fmt::Debug for Gantt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gantt")
            .field("tasks", &self.tasks.len())
            .field("total_days", &self.total_days)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(g: &mut Gantt, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        g.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        g.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn tasks_accumulate() {
        let g = Gantt::new()
            .task("Design", 0.0, 3.0)
            .progress(0.5)
            .task("Build", 3.0, 5.0);
        assert_eq!(g.task_count(), 2);
        assert_eq!(g.tasks()[0].progress, 0.5);
        assert_eq!(g.tasks()[1].duration, 5.0);
    }

    #[test]
    fn negative_inputs_clamp() {
        let g = Gantt::new().task("A", -2.0, -1.0).progress(1.5);
        assert_eq!(g.tasks()[0].start, 0.0);
        assert_eq!(g.tasks()[0].duration, 0.0);
        assert_eq!(g.tasks()[0].progress, 1.0);
    }

    #[test]
    fn measure_grows_with_tasks() {
        let mut one = Gantt::new().task("A", 0.0, 1.0);
        let mut five = (0..5).fold(Gantt::new(), |g, i| g.task(format!("t{i}"), 0.0, 1.0));
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let c = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(1000.0, 1000.0),
        };
        let s1 = one.measure(&mut cx, c);
        let s5 = five.measure(&mut cx, c);
        assert!(s5.y > s1.y);
    }

    #[test]
    fn hover_parks_row() {
        let mut g = Gantt::new().task("A", 0.0, 5.0).task("B", 5.0, 5.0);
        laid_out(&mut g, 400.0, 66.0);
        // Row 2 occupies the lower half of the chart region.
        g.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(200.0, 40.0),
            },
            bounds: Rect::new(0.0, 0.0, 400.0, 66.0),
            scale: 1.0,
        });
        assert_eq!(g.take_hovered(), Some(1));
        assert!(g.take_hovered().is_none());
    }

    #[test]
    fn hover_axis_region_ignored() {
        let mut g = Gantt::new().task("A", 0.0, 5.0);
        laid_out(&mut g, 400.0, 42.0);
        g.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(200.0, 40.0), // inside axis strip
            },
            bounds: Rect::new(0.0, 0.0, 400.0, 42.0),
            scale: 1.0,
        });
        assert!(g.take_hovered().is_none());
    }
}
