//! `Burndown` — an agile sprint burndown chart: the *ideal* line
//! falling diagonally from the sprint's total work to zero, plus
//! the *actual* remaining-work polyline the host extends once per
//! day via [`Burndown::push_day`].
//!
//! Hovering a day column parks its index in
//! [`Burndown::take_hovered`]. Display-only otherwise; companion
//! to [`LineChart`](crate::widgets::LineChart).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::burndown::Burndown;
//!
//! let mut b = Burndown::new(40.0, 10);
//! b.push_day(36.0);
//! b.push_day(30.0);
//! assert_eq!(b.days_logged(), 2);
//! assert_eq!(b.remaining(), Some(30.0));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 8.0;
const AXIS_PT: f32 = 18.0;
const FONT_PT: f32 = 9.5;

const FACE: [u8; 4] = [30, 32, 40, 255];
const GRID: [u8; 4] = [255, 255, 255, 18];
const IDEAL: [u8; 4] = [150, 154, 164, 200];
const ACTUAL: [u8; 4] = [90, 140, 220, 255];
const OVER: [u8; 4] = [220, 90, 90, 255];
const TEXT: [u8; 4] = [180, 184, 194, 255];
const COL_HOVER: [u8; 4] = [255, 255, 255, 12];

/// The chart — see the module docs.
///
/// ```
/// use martensite::widgets::burndown::Burndown;
///
/// assert_eq!(Burndown::new(40.0, 10).days_logged(), 0);
/// ```
pub struct Burndown {
    /// Accessibility label.
    pub label: String,
    /// Total work at sprint start.
    pub total: f32,
    /// Sprint length in days.
    pub days: usize,
    actual: Vec<f32>,
    hovered: Option<usize>,
    columns: Vec<Rect>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Burndown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Burndown")
            .field("days", &self.days)
            .field("logged", &self.actual.len())
            .finish()
    }
}

impl Burndown {
    /// A sprint of `total` work over `days` days.
    ///
    /// ```
    /// use martensite::widgets::burndown::Burndown;
    ///
    /// assert_eq!(Burndown::new(40.0, 10).days, 10);
    /// ```
    pub fn new(total: f32, days: usize) -> Self {
        Self {
            label: "Burndown".to_string(),
            total,
            days,
            actual: Vec::new(),
            hovered: None,
            columns: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::burndown::Burndown;
    ///
    /// assert_eq!(Burndown::new(40.0, 10).label("Sprint 12").label, "Sprint 12");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::burndown::Burndown;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _b = Burndown::new(40.0, 10).with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Logs one day's remaining work (day 1 = first push).
    ///
    /// ```
    /// use martensite::widgets::burndown::Burndown;
    ///
    /// let mut b = Burndown::new(40.0, 10);
    /// b.push_day(36.0);
    /// assert_eq!(b.remaining(), Some(36.0));
    /// ```
    pub fn push_day(&mut self, remaining: f32) {
        self.actual.push(remaining.max(0.0));
    }

    /// Days logged so far.
    ///
    /// ```
    /// use martensite::widgets::burndown::Burndown;
    ///
    /// assert_eq!(Burndown::new(40.0, 10).days_logged(), 0);
    /// ```
    pub fn days_logged(&self) -> usize {
        self.actual.len()
    }

    /// Latest remaining value.
    ///
    /// ```
    /// use martensite::widgets::burndown::Burndown;
    ///
    /// assert_eq!(Burndown::new(40.0, 10).remaining(), None);
    /// ```
    pub fn remaining(&self) -> Option<f32> {
        self.actual.last().copied()
    }

    /// The ideal remaining value after `day` days (day 0 = total).
    ///
    /// ```
    /// use martensite::widgets::burndown::Burndown;
    ///
    /// let b = Burndown::new(40.0, 10);
    /// assert_eq!(b.ideal_at(0), 40.0);
    /// assert_eq!(b.ideal_at(10), 0.0);
    /// ```
    pub fn ideal_at(&self, day: usize) -> f32 {
        if self.days == 0 {
            return 0.0;
        }
        (self.total * (1.0 - day as f32 / self.days as f32)).max(0.0)
    }

    /// `true` when actual is ahead of (below) the ideal line.
    ///
    /// ```
    /// use martensite::widgets::burndown::Burndown;
    ///
    /// let mut b = Burndown::new(40.0, 10);
    /// b.push_day(30.0); // ideal after day 1 is 36
    /// assert!(b.ahead());
    /// ```
    pub fn ahead(&self) -> bool {
        self.remaining()
            .is_some_and(|r| r < self.ideal_at(self.actual.len()))
    }

    /// Drains the last hovered day column index.
    ///
    /// ```
    /// use martensite::widgets::burndown::Burndown;
    ///
    /// let mut b = Burndown::new(40.0, 10);
    /// assert_eq!(b.take_hovered(), None);
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.hovered.take()
    }

    fn plot(&self) -> Rect {
        let s = self.scale;
        Rect::new(
            self.bounds.min_x() + (PAD_PT + AXIS_PT) * s,
            self.bounds.min_y() + PAD_PT * s,
            (self.bounds.width() - (PAD_PT * 2.0 + AXIS_PT) * s).max(0.0),
            (self.bounds.height() - (PAD_PT * 2.0 + AXIS_PT) * s).max(0.0),
        )
    }

    fn point(&self, day: usize, value: f32) -> Vec2 {
        let p = self.plot();
        let x = p.min_x() + p.width() * day as f32 / self.days.max(1) as f32;
        let y = p.max_y() - p.height() * (value / self.total.max(1.0)).clamp(0.0, 1.0);
        Vec2::new(x, y)
    }
}

fn line_path(a: Vec2, b: Vec2) -> kurbo::BezPath {
    let mut p = kurbo::BezPath::new();
    p.move_to((f64::from(a.x), f64::from(a.y)));
    p.line_to((f64::from(b.x), f64::from(b.y)));
    p
}

impl Widget for Burndown {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        Vec2::new(
            (320.0 * s).min(constraints.max_size.x.max(0.0)),
            (200.0 * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 120.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let p = self.plot();
        self.columns.clear();
        let w = p.width() / self.days.max(1) as f32;
        for i in 0..self.days {
            self.columns.push(Rect::new(
                p.min_x() + i as f32 * w,
                p.min_y(),
                w,
                p.height(),
            ));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.clone());
        node.set_value(format!(
            "{} of {} days, {:.0} remaining",
            self.actual.len(),
            self.days,
            self.remaining().unwrap_or(self.total),
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.columns.iter().position(|r| r.contains(*position));
                if h != self.hovered {
                    self.hovered = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let p = self.plot();
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(self.bounds.min_x()),
                f64::from(self.bounds.min_y()),
                f64::from(self.bounds.max_x()),
                f64::from(self.bounds.max_y()),
            ),
            cx.color(TokenKey::BackgroundColor, FACE),
        );
        // Hovered day column.
        if let Some(h) = self.hovered {
            if let Some(r) = self.columns.get(h) {
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(r.min_x()),
                        f64::from(r.min_y()),
                        f64::from(r.max_x()),
                        f64::from(r.max_y()),
                    ),
                    COL_HOVER,
                );
            }
        }
        // Quarter gridlines + axis labels.
        for q in 1..4 {
            let y = p.min_y() + p.height() * q as f32 / 4.0;
            cx.list.push_stroke_path(
                line_path(Vec2::new(p.min_x(), y), Vec2::new(p.max_x(), y)),
                1.0,
                GRID,
            );
        }
        for q in 0..=4 {
            let v = self.total * (1.0 - q as f32 / 4.0);
            let y = p.min_y() + p.height() * q as f32 / 4.0;
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(self.bounds.min_x() + PAD_PT * s),
                    f64::from(y + 3.0 * s),
                ),
                &format!("{v:.0}"),
                FONT_PT * s,
                TEXT,
            );
        }
        // Ideal diagonal (dashed-ish: solid muted).
        cx.list.push_stroke_path(
            line_path(self.point(0, self.total), self.point(self.days, 0.0)),
            1.5 * s,
            IDEAL,
        );
        // Actual polyline — red when above ideal, accent below.
        let color = if self.ahead() || self.actual.is_empty() {
            cx.color(TokenKey::AccentColor, ACTUAL)
        } else {
            cx.color(TokenKey::ErrorColor, OVER)
        };
        let mut prev = self.point(0, self.total);
        for (i, &v) in self.actual.iter().enumerate() {
            let pt = self.point(i + 1, v);
            cx.list
                .push_stroke_path(line_path(prev, pt), 2.0 * s, color);
            prev = pt;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> Burndown {
        let mut b = Burndown::new(40.0, 10);
        b.push_day(36.0);
        b.push_day(30.0);
        b.push_day(31.0);
        b
    }

    fn laid_out(b: &mut Burndown) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        b.layout(&mut cx, Rect::new(0.0, 0.0, 360.0, 220.0));
    }

    #[test]
    fn ideal_falls_linearly() {
        let b = Burndown::new(40.0, 10);
        assert_eq!(b.ideal_at(0), 40.0);
        assert_eq!(b.ideal_at(5), 20.0);
        assert_eq!(b.ideal_at(10), 0.0);
    }

    #[test]
    fn ahead_tracks_ideal() {
        let b = fixture();
        assert!(!b.ahead()); // 31 > ideal 28 after day 3
        let mut b = Burndown::new(40.0, 10);
        b.push_day(30.0);
        assert!(b.ahead());
    }

    #[test]
    fn hover_parks_column() {
        let mut b = fixture();
        laid_out(&mut b);
        let r = b.columns[4];
        b.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
            bounds: b.bounds,
            scale: 1.0,
        });
        assert_eq!(b.take_hovered(), Some(4));
    }

    #[test]
    fn paint_without_painter() {
        let mut b = fixture();
        laid_out(&mut b);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        b.paint(&mut PaintContext {
            list: &mut list,
            bounds: b.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
