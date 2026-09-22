//! `Steps` widget: a wizard/progress indicator — numbered step nodes
//! connected by lines, each with a label and optional description
//! (Ant `Steps`, Carbon `ProgressIndicator`, `QWizard` page header).
//!
//! Steps advance programmatically via [`Steps::set_current`]; completed
//! steps show a check, the current step an accent node, upcoming steps
//! a muted outline. Completed steps can be clickable for backward
//! navigation — poll [`Steps::take_navigated`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::steps::Steps;
//!
//! let s = Steps::new().steps(["Account", "Profile", "Done"]).current(1);
//! assert_eq!(s.current_step(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

/// Node circle diameter, logical points.
const NODE_PT: f32 = 24.0;
/// Label font size, logical points.
const LABEL_PT: f32 = 13.0;
/// Description font size, logical points.
const DESC_PT: f32 = 11.0;
/// Connector line thickness, logical points.
const LINE_PT: f32 = 2.0;
/// Gap between node and its label, logical points.
const TEXT_GAP_PT: f32 = 8.0;
/// Strip height, logical points.
const HEIGHT_PT: f32 = 56.0;

/// Connector behind completed steps.
const DONE_LINE: [u8; 4] = [70, 110, 200, 255];
/// Connector ahead of the current step.
const TODO_LINE: [u8; 4] = [200, 203, 210, 255];
/// Current step node face.
const CURRENT_FACE: [u8; 4] = [70, 110, 200, 255];
/// Completed step node face (check inside).
const DONE_FACE: [u8; 4] = [50, 160, 90, 255];
/// Upcoming step node border.
const TODO_EDGE: [u8; 4] = [170, 174, 183, 255];
/// Node ink (numbers, check).
const NODE_INK: [u8; 4] = [255, 255, 255, 255];
/// Label ink — current step.
const LABEL_INK: [u8; 4] = [30, 31, 36, 255];
/// Label ink — other steps.
const LABEL_DIM: [u8; 4] = [110, 114, 123, 255];
/// Hover tint on navigable completed steps.
const HIGHLIGHT: [u8; 4] = [70, 110, 200, 22];

/// One wizard step.
///
/// # Examples
///
/// ```
/// use martensite::widgets::steps::Step;
///
/// let s = Step::new("Shipping").description("Address & method");
/// ```
#[derive(Clone, Debug)]
pub struct Step {
    /// The step's label.
    pub label: String,
    /// Optional secondary line under the label.
    pub description: Option<String>,
}

impl Step {
    /// Creates a step with a label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::steps::Step;
    ///
    /// let s = Step::new("Review");
    /// assert_eq!(s.label, "Review");
    /// ```
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: None,
        }
    }

    /// Sets the description line.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::steps::Step;
    ///
    /// let s = Step::new("Pay").description("Card or invoice");
    /// ```
    #[must_use]
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }
}

/// A horizontal wizard progress indicator.
///
/// # Examples
///
/// ```
/// use martensite::widgets::steps::Steps;
///
/// let s = Steps::new().steps(["A", "B", "C"]);
/// assert_eq!(s.step_count(), 3);
/// ```
pub struct Steps {
    /// The step list.
    steps: Vec<Step>,
    /// Index of the in-progress step (0-based).
    current: usize,
    /// Whether completed steps can be clicked to navigate back.
    clickable_completed: bool,
    /// Hovered step index (completed steps only).
    highlighted: Option<usize>,
    /// Pending navigation — drained by `take_navigated`.
    navigated: Option<usize>,
    /// Per-step node hit rects from the last layout.
    node_rects: Vec<Rect>,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter. See [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Steps {
    /// Creates an empty indicator.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::steps::Steps;
    ///
    /// let s = Steps::new();
    /// assert_eq!(s.step_count(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            current: 0,
            clickable_completed: true,
            highlighted: None,
            navigated: None,
            node_rects: Vec::new(),
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the steps from labels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::steps::Steps;
    ///
    /// let s = Steps::new().steps(["One", "Two"]);
    /// ```
    #[must_use]
    pub fn steps(mut self, labels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.steps = labels.into_iter().map(|l| Step::new(l.into())).collect();
        self.current = self.current.min(self.steps.len().saturating_sub(1));
        self
    }

    /// Sets the steps from full [`Step`] values (with descriptions).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::steps::{Step, Steps};
    ///
    /// let s = Steps::new().steps_full(vec![Step::new("A").description("d")]);
    /// ```
    #[must_use]
    pub fn steps_full(mut self, steps: Vec<Step>) -> Self {
        self.steps = steps;
        self.current = self.current.min(self.steps.len().saturating_sub(1));
        self
    }

    /// Sets the current step index (clamped).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::steps::Steps;
    ///
    /// let s = Steps::new().steps(["A", "B"]).current(1);
    /// assert_eq!(s.current_step(), 1);
    /// ```
    #[must_use]
    pub fn current(mut self, index: usize) -> Self {
        self.current = index.min(self.steps.len().saturating_sub(1));
        self
    }

    /// Whether completed steps are clickable (default true).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::steps::Steps;
    ///
    /// let s = Steps::new().clickable_completed(false);
    /// ```
    #[must_use]
    pub fn clickable_completed(mut self, clickable: bool) -> Self {
        self.clickable_completed = clickable;
        self
    }

    /// The number of steps.
    #[inline]
    #[must_use]
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// The current step index.
    #[inline]
    #[must_use]
    pub fn current_step(&self) -> usize {
        self.current
    }

    /// Sets the current step programmatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::steps::Steps;
    ///
    /// let mut s = Steps::new().steps(["A", "B"]);
    /// s.set_current(1);
    /// assert_eq!(s.current_step(), 1);
    /// ```
    pub fn set_current(&mut self, index: usize) {
        self.current = index.min(self.steps.len().saturating_sub(1));
    }

    /// Advances to the next step (no-op at the end).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::steps::Steps;
    ///
    /// let mut s = Steps::new().steps(["A", "B"]);
    /// s.advance();
    /// assert_eq!(s.current_step(), 1);
    /// ```
    pub fn advance(&mut self) {
        self.set_current(self.current + 1);
    }

    /// Drains a completed-step activation — the index the user wants
    /// to navigate back to.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::steps::Steps;
    ///
    /// let mut s = Steps::new().steps(["A", "B"]);
    /// assert_eq!(s.take_navigated(), None);
    /// ```
    pub fn take_navigated(&mut self) -> Option<usize> {
        self.navigated.take()
    }

    /// Installs a shared shaped-text painter.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Whether a step index is a navigation target.
    fn navigable(&self, index: usize) -> bool {
        self.clickable_completed && index < self.current
    }
}

impl Default for Steps {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Steps {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(cx.pt(120.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.node_rects.clear();
        let n = self.steps.len();
        if n == 0 {
            return;
        }
        let node = cx.pt(NODE_PT);
        let y = bounds.origin.y + cx.pt(4.0);
        // Nodes distribute evenly across the width — first and last
        // at the edges, intermediates evenly spaced.
        for i in 0..n {
            let frac = if n == 1 {
                0.5
            } else {
                i as f32 / (n - 1) as f32
            };
            let cx_step = bounds.origin.x + node / 2.0 + frac * (bounds.size.x - node);
            self.node_rects
                .push(Rect::new(cx_step - node / 2.0, y, node, node));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!("Step {} of {}", self.current + 1, self.steps.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self
                    .node_rects
                    .iter()
                    .position(|r| r.contains(*position))
                    .filter(|i| self.navigable(*i));
                if hit != self.highlighted {
                    self.highlighted = hit;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerLeave => {
                self.highlighted = None;
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self
                    .node_rects
                    .iter()
                    .position(|r| r.contains(*position))
                    .filter(|i| self.navigable(*i))
                {
                    self.navigated = Some(i);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "Enter" | "Space" | " " => {
                    if let Some(i) = self.highlighted {
                        if self.navigable(i) {
                            self.navigated = Some(i);
                            return EventResponse::Handled;
                        }
                    }
                    EventResponse::Ignored
                }
                "ArrowLeft" | "ArrowRight" => {
                    let targets: Vec<usize> =
                        (0..self.current).filter(|i| self.navigable(*i)).collect();
                    if targets.is_empty() {
                        return EventResponse::Ignored;
                    }
                    let pos = targets
                        .iter()
                        .position(|t| Some(*t) == self.highlighted)
                        .map(|p| p as isize)
                        .unwrap_or(-1);
                    let next = match key.as_str() {
                        "ArrowRight" => (pos + 1).min(targets.len() as isize - 1),
                        _ => (pos - 1).max(0),
                    };
                    self.highlighted = Some(targets[next as usize]);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let n = self.steps.len();
        if n == 0 {
            return;
        }
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let label_size = cx.pt(LABEL_PT);
        let desc_size = cx.pt(DESC_PT);

        // Connector lines between adjacent nodes — drawn first so
        // nodes sit on top.
        for i in 0..n.saturating_sub(1) {
            let a = self.node_rects[i];
            let b = self.node_rects[i + 1];
            let y = a.origin.y + a.size.y / 2.0 - cx.pt(LINE_PT) / 2.0;
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(a.max_x()),
                    f64::from(y),
                    f64::from(b.origin.x),
                    f64::from(y + cx.pt(LINE_PT)),
                ),
                if i < self.current {
                    cx.color(TokenKey::AccentColor, DONE_LINE)
                } else {
                    cx.color(TokenKey::BorderColor, TODO_LINE)
                },
            );
        }

        for (i, step) in self.steps.iter().enumerate() {
            let r = self.node_rects[i];
            let done = i < self.current;
            let current = i == self.current;

            // Hover tint on navigable steps.
            if self.highlighted == Some(i) {
                let grow = cx.pt(4.0);
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(r.min_x() - grow),
                        f64::from(r.min_y() - grow),
                        f64::from(r.max_x() + grow),
                        f64::from(r.max_y() + grow),
                    ),
                    &martensite_core::shape::Shape::ELLIPSE,
                    HIGHLIGHT,
                );
            }

            // The node circle.
            let kr = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            if done {
                cx.list
                    .push_fill_shape(kr, &martensite_core::shape::Shape::ELLIPSE, DONE_FACE);
            } else if current {
                cx.list.push_fill_shape(
                    kr,
                    &martensite_core::shape::Shape::ELLIPSE,
                    cx.color(TokenKey::AccentColor, CURRENT_FACE),
                );
            } else {
                cx.list.push_fill_shape(
                    kr,
                    &martensite_core::shape::Shape::ELLIPSE,
                    cx.color(TokenKey::SurfaceColor, [245, 246, 248, 255]),
                );
                cx.list.push_stroke_shape(
                    kr,
                    &martensite_core::shape::Shape::ELLIPSE,
                    cx.pt(1.5),
                    cx.color(TokenKey::BorderColor, TODO_EDGE),
                );
            }

            // Node content: ✓ for done, number otherwise.
            let mark = if done {
                "✓".to_string()
            } else {
                (i + 1).to_string()
            };
            let mark_size = cx.pt(12.0);
            let w = painter
                .and_then(|p| p.measure_text(&mark, mark_size))
                .unwrap_or(mark_size * mark.chars().count() as f32 * 0.55);
            let mx = r.origin.x + (r.size.x - w) / 2.0;
            let my = r.origin.y + (r.size.y - mark_size) / 2.0;
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kr,
                kurbo::Point::new(f64::from(mx), f64::from(my)),
                &mark,
                mark_size,
                if current || done {
                    NODE_INK
                } else {
                    cx.color(TokenKey::TextMutedColor, LABEL_DIM)
                },
            );

            // Label + description centred under the node — clamped
            // into the widget so end-step captions don't spill past
            // the row's edges (they slide inward instead of clipping
            // mid-glyph).
            let wb = cx.bounds;
            let lw = painter
                .and_then(|p| p.measure_text(&step.label, label_size))
                .unwrap_or(label_size * step.label.chars().count() as f32 * 0.55);
            let lx = (r.origin.x + r.size.x / 2.0 - lw / 2.0)
                .clamp(wb.min_x(), (wb.max_x() - lw).max(wb.min_x()));
            let ly = r.max_y() + cx.pt(TEXT_GAP_PT);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(lx - cx.pt(30.0)),
                    f64::from(ly),
                    f64::from(lx + lw + cx.pt(30.0)),
                    f64::from(ly + label_size),
                ),
                kurbo::Point::new(f64::from(lx), f64::from(ly)),
                &step.label,
                label_size,
                if current {
                    cx.color(TokenKey::TextColor, LABEL_INK)
                } else {
                    cx.color(TokenKey::TextMutedColor, LABEL_DIM)
                },
            );
            if let Some(desc) = &step.description {
                let dw = painter
                    .and_then(|p| p.measure_text(desc, desc_size))
                    .unwrap_or(desc_size * desc.chars().count() as f32 * 0.55);
                let dx = (r.origin.x + r.size.x / 2.0 - dw / 2.0)
                    .clamp(wb.min_x(), (wb.max_x() - dw).max(wb.min_x()));
                let dy = ly + label_size + cx.pt(2.0);
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    kurbo::Rect::new(
                        f64::from(dx - cx.pt(30.0)),
                        f64::from(dy),
                        f64::from(dx + dw + cx.pt(30.0)),
                        f64::from(dy + desc_size),
                    ),
                    kurbo::Point::new(f64::from(dx), f64::from(dy)),
                    desc,
                    desc_size,
                    cx.color(TokenKey::TextMutedColor, LABEL_DIM),
                );
            }
        }
    }
}

impl std::fmt::Debug for Steps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Steps")
            .field("steps", &self.steps.len())
            .field("current", &self.current)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 600.0, 56.0),
            scale: 1.0,
        }
    }

    fn laid_out(current: usize) -> Steps {
        let mut s = Steps::new()
            .steps(["One", "Two", "Three", "Four"])
            .current(current);
        let mut hot = HotNode::default();
        s.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 600.0, 56.0));
        s
    }

    #[test]
    fn builder_and_clamping() {
        let s = Steps::new().steps(["A", "B"]).current(9);
        assert_eq!(s.current_step(), 1);
        assert_eq!(s.step_count(), 2);
    }

    #[test]
    fn advance_walks_forward() {
        let mut s = Steps::new().steps(["A", "B", "C"]);
        s.advance();
        s.advance();
        s.advance(); // clamped at end
        assert_eq!(s.current_step(), 2);
    }

    #[test]
    fn nodes_spread_evenly() {
        let s = laid_out(1);
        assert_eq!(s.node_rects.len(), 4);
        let xs: Vec<f32> = s.node_rects.iter().map(|r| r.origin.x).collect();
        assert!(xs[0] < xs[1] && xs[1] < xs[2] && xs[2] < xs[3]);
    }

    #[test]
    fn completed_step_click_navigates() {
        let mut s = laid_out(2); // steps 0,1 completed
        let r = s.node_rects[0];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(r.origin.x + 4.0, r.origin.y + 4.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(s.event(&mut ev(&press)), EventResponse::Handled);
        assert_eq!(s.take_navigated(), Some(0));
    }

    #[test]
    fn current_and_future_steps_inert() {
        let mut s = laid_out(1); // current=1; steps 2,3 upcoming
        for idx in [1, 2, 3] {
            let r = s.node_rects[idx];
            let press = WidgetEvent::PointerPressed {
                position: Vec2::new(r.origin.x + 4.0, r.origin.y + 4.0),
                button: PointerButton::Primary,
                count: 1,
            };
            assert_eq!(s.event(&mut ev(&press)), EventResponse::Ignored);
        }
        assert_eq!(s.take_navigated(), None);
    }

    #[test]
    fn clickable_completed_false_disables() {
        let mut s = Steps::new()
            .steps(["A", "B", "C"])
            .current(2)
            .clickable_completed(false);
        let mut hot = HotNode::default();
        s.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 600.0, 56.0));
        let r = s.node_rects[0];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(r.origin.x + 4.0, r.origin.y + 4.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(s.event(&mut ev(&press)), EventResponse::Ignored);
    }

    #[test]
    fn descriptions_flow_through() {
        let s = Steps::new().steps_full(vec![Step::new("A").description("desc")]);
        assert_eq!(s.step_count(), 1);
    }
}
