//! Swipe-to-reveal row actions — a horizontal drag on the wrapped
//! row exposes action buttons at the leading/trailing edge (iOS
//! `UISwipeActionsConfiguration`, Android `ItemTouchHelper`).
//!
//! Dragging left reveals [`SwipeActions::trailing`] actions (the
//! delete/archive edge); dragging right reveals `leading`. A drag
//! past `FULL_SWIPE` of the widget width auto-triggers the outermost
//! action on release — the iOS full-swipe shortcut. Otherwise the
//! row rests with the strip open until an action is tapped or the
//! row is dragged back.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{SwipeAction, SwipeActions, Text};
//!
//! let s = SwipeActions::new(Text::new("row"))
//!     .trailing([SwipeAction::new("Delete").destructive()]);
//! assert_eq!(s.trailing_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::paint_label_clipped;

/// Fraction of the widget width that triggers the outermost action
/// on release (iOS full-swipe).
const FULL_SWIPE: f32 = 0.75;
/// Width granted per action button (points).
const ACTION_W_PT: f32 = 72.0;

/// Which edge an action strip hangs from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeEdge {
    /// Revealed by dragging right (archive/pin side on iOS).
    Leading,
    /// Revealed by dragging left (delete side on iOS).
    Trailing,
}

/// One swipe-revealed action button.
///
/// # Examples
///
/// ```
/// use martensite::widgets::SwipeAction;
///
/// let a = SwipeAction::new("Delete").destructive();
/// assert!(a.destructive);
/// ```
#[derive(Debug, Clone)]
pub struct SwipeAction {
    /// Button text.
    pub label: String,
    /// Destructive styling (red) — delete/remove semantics.
    pub destructive: bool,
    /// Whether the row closes after the action fires.
    pub closes: bool,
    /// Accessibility label override.
    pub at_label: Option<String>,
}

impl SwipeAction {
    /// Creates an action with `label`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SwipeAction;
    ///
    /// assert_eq!(SwipeAction::new("Archive").label, "Archive");
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            destructive: false,
            closes: true,
            at_label: None,
        }
    }

    /// Marks the action destructive.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SwipeAction;
    ///
    /// assert!(SwipeAction::new("x").destructive().destructive);
    /// ```
    #[must_use]
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    /// Sets whether the row closes after firing.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SwipeAction;
    ///
    /// assert!(!SwipeAction::new("x").closes(false).closes);
    /// ```
    #[must_use]
    pub fn closes(mut self, flag: bool) -> Self {
        self.closes = flag;
        self
    }
}

/// A swipeable row wrapping one child.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{SwipeActions, Text};
///
/// assert_eq!(SwipeActions::new(Text::new("r")).offset(), 0.0);
/// ```
pub struct SwipeActions {
    /// Accessibility label override.
    pub label: Option<String>,
    /// Whether the gesture is armed.
    pub enabled: bool,
    /// Width per action button in points.
    pub action_width: f32,
    child: Box<dyn Widget>,
    leading: Vec<SwipeAction>,
    trailing: Vec<SwipeAction>,
    /// Signed horizontal offset: negative = trailing strip open.
    offset: f32,
    /// Whether the strip is resting open (post-release).
    open_edge: Option<SwipeEdge>,
    drag_x: Option<f32>,
    drag_start_offset: f32,
    /// Parked `(edge, action_index)` for the consumer.
    triggered: Option<(SwipeEdge, usize)>,
    bounds: Rect,
    child_bounds: Option<Rect>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl SwipeActions {
    /// Wraps `child`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::core::Widget;
    /// use martensite::widgets::{SwipeActions, Text};
    ///
    /// assert_eq!(SwipeActions::new(Text::new("r")).child_count(), 1);
    /// ```
    pub fn new(child: impl Widget + 'static) -> Self {
        Self {
            label: None,
            enabled: true,
            action_width: ACTION_W_PT,
            child: Box::new(child),
            leading: Vec::new(),
            trailing: Vec::new(),
            offset: 0.0,
            open_edge: None,
            drag_x: None,
            drag_start_offset: 0.0,
            triggered: None,
            bounds: Rect::default(),
            child_bounds: None,
            text_painter: None,
        }
    }

    /// Sets the leading-edge actions (reveal by dragging right).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{SwipeAction, SwipeActions, Text};
    ///
    /// let s = SwipeActions::new(Text::new("r")).leading([SwipeAction::new("Pin")]);
    /// assert_eq!(s.leading_count(), 1);
    /// ```
    #[must_use]
    pub fn leading(mut self, actions: impl Into<Vec<SwipeAction>>) -> Self {
        self.leading = actions.into();
        self
    }

    /// Sets the trailing-edge actions (reveal by dragging left).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{SwipeAction, SwipeActions, Text};
    ///
    /// let s = SwipeActions::new(Text::new("r")).trailing([SwipeAction::new("Delete")]);
    /// assert_eq!(s.trailing_count(), 1);
    /// ```
    #[must_use]
    pub fn trailing(mut self, actions: impl Into<Vec<SwipeAction>>) -> Self {
        self.trailing = actions.into();
        self
    }

    /// Sets the accessibility label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{SwipeActions, Text};
    ///
    /// let s = SwipeActions::new(Text::new("r")).label("Inbox row");
    /// assert_eq!(s.label.as_deref(), Some("Inbox row"));
    /// ```
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// Sets whether the gesture is armed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{SwipeActions, Text};
    ///
    /// assert!(!SwipeActions::new(Text::new("r")).enabled(false).enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, flag: bool) -> Self {
        self.enabled = flag;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] for action text.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Leading action count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{SwipeActions, Text};
    ///
    /// assert_eq!(SwipeActions::new(Text::new("r")).leading_count(), 0);
    /// ```
    #[inline]
    pub fn leading_count(&self) -> usize {
        self.leading.len()
    }

    /// Trailing action count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{SwipeActions, Text};
    ///
    /// assert_eq!(SwipeActions::new(Text::new("r")).trailing_count(), 0);
    /// ```
    #[inline]
    pub fn trailing_count(&self) -> usize {
        self.trailing.len()
    }

    /// Current horizontal offset — negative reveals the trailing
    /// strip, positive the leading.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{SwipeActions, Text};
    ///
    /// assert_eq!(SwipeActions::new(Text::new("r")).offset(), 0.0);
    /// ```
    #[inline]
    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// Whether an action strip is resting open.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{SwipeActions, Text};
    ///
    /// assert!(SwipeActions::new(Text::new("r")).open_edge().is_none());
    /// ```
    #[inline]
    pub fn open_edge(&self) -> Option<SwipeEdge> {
        self.open_edge
    }

    /// Takes the parked `(edge, action_index)` trigger.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{SwipeActions, Text};
    ///
    /// assert!(SwipeActions::new(Text::new("r")).take_triggered().is_none());
    /// ```
    #[inline]
    pub fn take_triggered(&mut self) -> Option<(SwipeEdge, usize)> {
        self.triggered.take()
    }

    /// Closes any open strip.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{SwipeActions, Text};
    ///
    /// let mut s = SwipeActions::new(Text::new("r"));
    /// s.close();
    /// assert_eq!(s.offset(), 0.0);
    /// ```
    pub fn close(&mut self) {
        self.offset = 0.0;
        self.open_edge = None;
        self.update_child();
    }

    /// The strip width for `edge` at `scale`.
    fn strip_w(&self, edge: SwipeEdge, scale: f32) -> f32 {
        let n = match edge {
            SwipeEdge::Leading => self.leading.len(),
            SwipeEdge::Trailing => self.trailing.len(),
        };
        n as f32 * self.action_width * scale
    }

    /// Clamps `offset` to the configured strips.
    fn clamp_offset(&self, raw: f32, scale: f32) -> f32 {
        let lead = self.strip_w(SwipeEdge::Leading, scale);
        let trail = self.strip_w(SwipeEdge::Trailing, scale);
        raw.clamp(
            -trail - self.bounds.width() * FULL_SWIPE,
            lead + self.bounds.width() * FULL_SWIPE,
        )
    }

    /// Recomputes the child rect from `offset`.
    fn update_child(&mut self) {
        let b = self.bounds;
        self.child_bounds = if b.width() > 0.0 {
            Some(Rect::new(
                b.min_x() + self.offset,
                b.min_y(),
                b.width(),
                b.height(),
            ))
        } else {
            None
        };
    }

    /// Action rect for `(edge, index)` — index 0 outermost (nearest
    /// the row edge it reveals from), matching iOS stacking.
    fn action_rect(&self, edge: SwipeEdge, index: usize, scale: f32) -> Option<Rect> {
        let b = self.bounds;
        let w = self.action_width * scale;
        let n = match edge {
            SwipeEdge::Leading => self.leading.len(),
            SwipeEdge::Trailing => self.trailing.len(),
        };
        if index >= n || self.offset == 0.0 {
            return None;
        }
        Some(match edge {
            SwipeEdge::Leading => Rect::new(b.min_x() + index as f32 * w, b.min_y(), w, b.height()),
            SwipeEdge::Trailing => {
                Rect::new(b.max_x() - (index + 1) as f32 * w, b.min_y(), w, b.height())
            }
        })
    }

    /// Whether `position` hits an open action; returns `(edge, idx)`.
    fn hit_action(&self, position: Vec2, scale: f32) -> Option<(SwipeEdge, usize)> {
        for edge in [SwipeEdge::Leading, SwipeEdge::Trailing] {
            let n = match edge {
                SwipeEdge::Leading => self.leading.len(),
                SwipeEdge::Trailing => self.trailing.len(),
            };
            for i in 0..n {
                if self
                    .action_rect(edge, i, scale)
                    .is_some_and(|r| r.contains(position))
                {
                    return Some((edge, i));
                }
            }
        }
        None
    }

    /// Release resolution — rest open on a partial drag, full-swipe
    /// triggers, tiny drag snaps shut.
    fn settle(&mut self, scale: f32) {
        let b = self.bounds;
        let trail_open = -self.offset;
        let lead_open = self.offset;
        // Full-swipe triggers the outermost action.
        if trail_open >= b.width() * FULL_SWIPE && !self.trailing.is_empty() {
            self.triggered = Some((SwipeEdge::Trailing, self.trailing.len() - 1));
            self.close();
            return;
        }
        if lead_open >= b.width() * FULL_SWIPE && !self.leading.is_empty() {
            self.triggered = Some((SwipeEdge::Leading, self.leading.len() - 1));
            self.close();
            return;
        }
        let trail_rest = self.strip_w(SwipeEdge::Trailing, scale);
        let lead_rest = self.strip_w(SwipeEdge::Leading, scale);
        if trail_open > trail_rest / 2.0 && trail_rest > 0.0 {
            self.offset = -trail_rest;
            self.open_edge = Some(SwipeEdge::Trailing);
        } else if lead_open > lead_rest / 2.0 && lead_rest > 0.0 {
            self.offset = lead_rest;
            self.open_edge = Some(SwipeEdge::Leading);
        } else {
            self.offset = 0.0;
            self.open_edge = None;
        }
        self.update_child();
    }
}

impl std::fmt::Debug for SwipeActions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SwipeActions")
            .field("offset", &self.offset)
            .field("open_edge", &self.open_edge)
            .finish()
    }
}

impl Widget for SwipeActions {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let inner = self.child.measure(cx, constraints);
        Vec2::new(
            constraints
                .max_size
                .x
                .max(inner.x.max(cx.pt(160.0).min(constraints.max_size.x))),
            inner.y.max(cx.pt(32.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 32.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.update_child();
        if let Some(cb) = self.child_bounds {
            cx.layout_child(&mut *self.child, cb);
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        if self.offset == 0.0 {
            return;
        }
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let surface = cx.color(TokenKey::SurfaceColor, [45, 45, 48, 255]);
        let fg = cx.color(TokenKey::TextColor, [235, 235, 235, 255]);
        let danger = cx.color(TokenKey::ErrorColor, [200, 60, 60, 255]);
        let accent = cx.color(TokenKey::AccentColor, [0, 122, 204, 255]);
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size = 11.0 * cx.scale;
        for (edge, actions) in [
            (SwipeEdge::Leading, &self.leading),
            (SwipeEdge::Trailing, &self.trailing),
        ] {
            for (i, action) in actions.iter().enumerate() {
                let Some(r) = self.action_rect(edge, i, cx.scale) else {
                    continue;
                };
                // Only the revealed portion paints.
                let visible = match edge {
                    SwipeEdge::Leading => Rect::new(
                        r.min_x(),
                        r.min_y(),
                        r.width()
                            .min(self.offset - r.min_x() + self.bounds.min_x())
                            .max(0.0),
                        r.height(),
                    ),
                    SwipeEdge::Trailing => {
                        let reveal = self.bounds.max_x() + self.offset;
                        Rect::new(
                            r.min_x().max(reveal),
                            r.min_y(),
                            (r.max_x() - r.min_x().max(reveal)).max(0.0),
                            r.height(),
                        )
                    }
                };
                if visible.width() < 1.0 {
                    continue;
                }
                let fill = if action.destructive { danger } else { accent };
                cx.list.push_fill_rect(f(visible), fill);
                let w = painter
                    .and_then(|p| p.measure_text(&action.label, size))
                    .unwrap_or(size * action.label.chars().count() as f32 * 0.5);
                let origin = kurbo::Point::new(
                    f64::from(r.min_x() + (r.width() - w.min(r.width())) / 2.0),
                    f64::from(r.min_y() + (r.height() - size) / 2.0),
                );
                paint_label_clipped(painter, cx.list, f(r), origin, &action.label, size, fg);
                let _ = surface;
            }
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                position, button, ..
            } => {
                if *button != martensite_core::PointerButton::Primary {
                    return EventResponse::Ignored;
                }
                // Tapping an open action fires it.
                if self.open_edge.is_some() {
                    if let Some((edge, i)) = self.hit_action(*position, cx.scale) {
                        self.triggered = Some((edge, i));
                        let closes = match edge {
                            SwipeEdge::Leading => self.leading[i].closes,
                            SwipeEdge::Trailing => self.trailing[i].closes,
                        };
                        if closes {
                            self.close();
                        }
                        return EventResponse::Handled;
                    }
                    // Tap elsewhere closes.
                    self.close();
                    return EventResponse::Handled;
                }
                if self.bounds.contains(*position) {
                    self.drag_x = Some(position.x);
                    self.drag_start_offset = self.offset;
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some(start) = self.drag_x {
                    self.offset =
                        self.clamp_offset(self.drag_start_offset + position.x - start, cx.scale);
                    self.update_child();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased { position, .. } => {
                if self.drag_x.take().is_some() {
                    // Tiny motion with no strip = a click on the row.
                    if (self.drag_start_offset - self.offset).abs() < 1.0
                        && self.offset == 0.0
                        && self.open_edge.is_none()
                    {
                        if let Some(cb) = self.child_bounds {
                            if cb.contains(*position) {
                                let mut child_cx = EventContext {
                                    event: cx.event,
                                    bounds: cb,
                                    scale: cx.scale,
                                };
                                let r = self.child.event(&mut child_cx);
                                return if r == EventResponse::Ignored {
                                    EventResponse::ReleasePointer
                                } else {
                                    r
                                };
                            }
                        }
                    }
                    self.settle(cx.scale);
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&*self.child as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut *self.child as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.child_bounds).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Text;
    use martensite_core::{HotNode, PointerButton, WidgetEvent};

    fn row() -> SwipeActions {
        SwipeActions::new(Text::new("row"))
            .leading([SwipeAction::new("Pin")])
            .trailing([
                SwipeAction::new("Delete").destructive(),
                SwipeAction::new("More"),
            ])
    }

    fn laid_out(s: &mut SwipeActions) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        s.layout(&mut cx, Rect::new(0.0, 0.0, 300.0, 40.0));
    }

    fn ev(s: &mut SwipeActions, event: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 300.0, 40.0),
            scale: 1.0,
        };
        s.event(&mut cx)
    }

    fn drag(s: &mut SwipeActions, dx: f32) {
        let down = WidgetEvent::PointerPressed {
            position: Vec2::new(150.0, 20.0),
            button: PointerButton::Primary,
            count: 1,
        };
        let mv = WidgetEvent::PointerMoved {
            position: Vec2::new(150.0 + dx, 20.0),
        };
        let up = WidgetEvent::PointerReleased {
            position: Vec2::new(150.0 + dx, 20.0),
            button: PointerButton::Primary,
        };
        ev(s, &down);
        ev(s, &mv);
        ev(s, &up);
    }

    #[test]
    fn builder() {
        let s = row();
        assert_eq!(s.leading_count(), 1);
        assert_eq!(s.trailing_count(), 2);
        assert_eq!(s.offset(), 0.0);
    }

    #[test]
    fn partial_left_drag_rests_trailing_open() {
        let mut s = row();
        laid_out(&mut s);
        drag(&mut s, -90.0); // less than 2*72=144? rest = 144, half=72 → 90>72 rests open
        assert_eq!(s.open_edge(), Some(SwipeEdge::Trailing));
        assert_eq!(s.offset(), -144.0);
    }

    #[test]
    fn partial_right_drag_rests_leading_open() {
        let mut s = row();
        laid_out(&mut s);
        drag(&mut s, 50.0); // rest = 72, half = 36 → 50 > 36 rests open
        assert_eq!(s.open_edge(), Some(SwipeEdge::Leading));
        assert_eq!(s.offset(), 72.0);
    }

    #[test]
    fn tiny_drag_snaps_shut() {
        let mut s = row();
        laid_out(&mut s);
        drag(&mut s, -10.0);
        assert_eq!(s.offset(), 0.0);
        assert!(s.open_edge().is_none());
    }

    #[test]
    fn full_swipe_triggers_outermost() {
        let mut s = row();
        laid_out(&mut s);
        drag(&mut s, -240.0); // > 0.75*300 = 225
        assert_eq!(s.take_triggered(), Some((SwipeEdge::Trailing, 1)));
        assert_eq!(s.offset(), 0.0);
    }

    #[test]
    fn action_tap_fires() {
        let mut s = row();
        laid_out(&mut s);
        drag(&mut s, -90.0); // rests open
                             // Trailing action 0 (Delete) is at the right edge.
        let tap = WidgetEvent::PointerPressed {
            position: Vec2::new(300.0 - 36.0, 20.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(ev(&mut s, &tap), EventResponse::Handled);
        assert_eq!(s.take_triggered(), Some((SwipeEdge::Trailing, 0)));
        assert!(s.open_edge().is_none()); // closes by default
    }

    #[test]
    fn tap_elsewhere_closes() {
        let mut s = row();
        laid_out(&mut s);
        drag(&mut s, -90.0);
        let tap = WidgetEvent::PointerPressed {
            position: Vec2::new(60.0, 20.0),
            button: PointerButton::Primary,
            count: 1,
        };
        ev(&mut s, &tap);
        assert!(s.open_edge().is_none());
        assert_eq!(s.offset(), 0.0);
    }

    #[test]
    fn disabled_inert() {
        let mut s = row().enabled(false);
        laid_out(&mut s);
        drag(&mut s, -90.0);
        assert_eq!(s.offset(), 0.0);
    }
}
