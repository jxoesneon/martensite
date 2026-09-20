//! `ResizeHandle` — a standalone draggable split sash (VS Code /
//! Qt `QSplitterHandle` idiom).
//!
//! [`SplitView`](crate::widgets::SplitView) embeds this logic
//! internally; use `ResizeHandle` when the host manages its own
//! split geometry (multi-pane dashboards, dock layouts, grid
//! splitters). The widget is deliberately geometry-free: it paints
//! the grip, captures the drag, and parks the accumulated delta —
//! in screen px along the split axis — in
//! [`ResizeHandle::take_moved`]. Arrow keys nudge ±1pt (Shift:
//! ±10pt); double-click parks a reset in
//! [`ResizeHandle::take_reset`].
//!
//! Reuses [`SplitOrientation`]: `Horizontal` means side-by-side
//! panes (a vertical bar that drags horizontally).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::resize_handle::ResizeHandle;
//!
//! let mut h = ResizeHandle::horizontal();
//! assert_eq!(h.take_moved(), None);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, NodeFlags, PaintContext,
    PointerButton, Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::widgets::split_view::SplitOrientation;

const THICK_PT: f32 = 6.0;
const GRIP_PT: f32 = 3.0;
const GRIP_SPACING_PT: f32 = 7.0;

const LINE: [u8; 4] = [70, 72, 80, 255];
const LINE_HOT: [u8; 4] = [96, 165, 250, 255];
const GRIP: [u8; 4] = [150, 152, 160, 255];

/// A draggable split sash — see the module docs.
///
/// ```
/// use martensite::widgets::resize_handle::ResizeHandle;
///
/// assert!(!ResizeHandle::horizontal().is_dragging());
/// ```
pub struct ResizeHandle {
    /// Accessibility label.
    pub label: String,
    orientation: SplitOrientation,
    bounds: Rect,
    scale: f32,
    /// Anchor position at drag start.
    drag_start: Option<Vec2>,
    /// Whether the pointer is hovering.
    hovered: bool,
    /// Accumulated px delta since the last `take_moved` drain.
    pending: f32,
    /// Double-click reset seam.
    reset: bool,
    enabled: bool,
}

impl std::fmt::Debug for ResizeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResizeHandle")
            .field("orientation", &self.orientation)
            .field("dragging", &self.drag_start.is_some())
            .finish()
    }
}

impl ResizeHandle {
    /// Side-by-side split — a vertical bar that drags horizontally.
    ///
    /// ```
    /// use martensite::widgets::resize_handle::ResizeHandle;
    ///
    /// assert_eq!(ResizeHandle::horizontal().take_moved(), None);
    /// ```
    pub fn horizontal() -> Self {
        Self::new(SplitOrientation::Horizontal)
    }

    /// Stacked split — a horizontal bar that drags vertically.
    ///
    /// ```
    /// use martensite::widgets::resize_handle::ResizeHandle;
    ///
    /// assert_eq!(ResizeHandle::vertical().take_moved(), None);
    /// ```
    pub fn vertical() -> Self {
        Self::new(SplitOrientation::Vertical)
    }

    /// A handle for the given split orientation.
    ///
    /// ```
    /// use martensite::widgets::resize_handle::ResizeHandle;
    /// use martensite::widgets::split_view::SplitOrientation;
    ///
    /// let h = ResizeHandle::new(SplitOrientation::Vertical);
    /// assert!(!h.is_dragging());
    /// ```
    pub fn new(orientation: SplitOrientation) -> Self {
        Self {
            label: "Resize".to_string(),
            orientation,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            drag_start: None,
            hovered: false,
            pending: 0.0,
            reset: false,
            enabled: true,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::resize_handle::ResizeHandle;
    ///
    /// assert_eq!(ResizeHandle::horizontal().label("Sidebar").label, "Sidebar");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Disabled builder.
    ///
    /// ```
    /// use martensite::widgets::resize_handle::ResizeHandle;
    ///
    /// assert!(!ResizeHandle::horizontal().enabled(false).is_enabled());
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Whether the handle accepts input.
    ///
    /// ```
    /// use martensite::widgets::resize_handle::ResizeHandle;
    ///
    /// assert!(ResizeHandle::horizontal().is_enabled());
    /// ```
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Whether a drag gesture is in progress.
    ///
    /// ```
    /// use martensite::widgets::resize_handle::ResizeHandle;
    ///
    /// assert!(!ResizeHandle::horizontal().is_dragging());
    /// ```
    pub fn is_dragging(&self) -> bool {
        self.drag_start.is_some()
    }

    /// The split orientation.
    ///
    /// ```
    /// use martensite::widgets::resize_handle::ResizeHandle;
    /// use martensite::widgets::split_view::SplitOrientation;
    ///
    /// assert_eq!(
    ///     ResizeHandle::vertical().orientation(),
    ///     SplitOrientation::Vertical
    /// );
    /// ```
    pub fn orientation(&self) -> SplitOrientation {
        self.orientation
    }

    /// Drains the accumulated drag delta in screen px along the
    /// split axis (positive = right / down).
    ///
    /// ```
    /// use martensite::widgets::resize_handle::ResizeHandle;
    ///
    /// assert_eq!(ResizeHandle::horizontal().take_moved(), None);
    /// ```
    pub fn take_moved(&mut self) -> Option<f32> {
        (self.pending != 0.0).then(|| std::mem::take(&mut self.pending))
    }

    /// Drains the double-click reset request.
    ///
    /// ```
    /// use martensite::widgets::resize_handle::ResizeHandle;
    ///
    /// assert!(!ResizeHandle::horizontal().take_reset());
    /// ```
    pub fn take_reset(&mut self) -> bool {
        std::mem::take(&mut self.reset)
    }

    /// Nudges the split by `delta` screen px (keyboard/API path).
    ///
    /// ```
    /// use martensite::widgets::resize_handle::ResizeHandle;
    ///
    /// let mut h = ResizeHandle::horizontal();
    /// h.nudge(12.0);
    /// assert_eq!(h.take_moved(), Some(12.0));
    /// ```
    pub fn nudge(&mut self, delta: f32) {
        self.pending += delta;
    }

    /// Projects a screen position onto the split axis.
    fn axis_pos(&self, p: Vec2) -> f32 {
        match self.orientation {
            SplitOrientation::Horizontal => p.x,
            SplitOrientation::Vertical => p.y,
        }
    }
}

impl Widget for ResizeHandle {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let thick = cx.pt(THICK_PT);
        match self.orientation {
            SplitOrientation::Horizontal => Vec2::new(thick, constraints.max_size.y.max(thick)),
            SplitOrientation::Vertical => Vec2::new(constraints.max_size.x.max(thick), thick),
        }
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(THICK_PT, THICK_PT)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Splitter);
        node.set_label(self.label.clone());
        node.add_action(accesskit::Action::Focus);
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                count,
            } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                if *count >= 2 {
                    self.reset = true;
                    return EventResponse::RequestRepaint;
                }
                self.drag_start = Some(*position);
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some(start) = self.drag_start {
                    let delta = self.axis_pos(*position) - self.axis_pos(start);
                    if delta != 0.0 {
                        self.pending += delta;
                        self.drag_start = Some(*position);
                        return EventResponse::RequestRepaint;
                    }
                } else if !self.hovered && self.bounds.contains(*position) {
                    self.hovered = true;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.drag_start.take().is_some() {
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                let had_hover = std::mem::take(&mut self.hovered);
                if had_hover {
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, repeat } => {
                if *repeat {
                    return EventResponse::Ignored;
                }
                let step = self.scale;
                let delta = match (self.orientation, key.as_str()) {
                    (SplitOrientation::Horizontal, "ArrowLeft")
                    | (SplitOrientation::Vertical, "ArrowUp") => -step,
                    (SplitOrientation::Horizontal, "ArrowRight")
                    | (SplitOrientation::Vertical, "ArrowDown") => step,
                    _ => return EventResponse::Ignored,
                };
                self.pending += delta;
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
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
        let s = self.scale;
        let hot = self.drag_start.is_some() || self.hovered;
        let line = cx.color(TokenKey::DividerColor, if hot { LINE_HOT } else { LINE });
        let b = cx.bounds;
        // Centered 2px divider line.
        let lw = 2.0 * s;
        let line_rect = match self.orientation {
            SplitOrientation::Horizontal => Rect::new(
                b.min_x() + (b.width() - lw) / 2.0,
                b.min_y(),
                lw,
                b.height(),
            ),
            SplitOrientation::Vertical => Rect::new(
                b.min_x(),
                b.min_y() + (b.height() - lw) / 2.0,
                b.width(),
                lw,
            ),
        };
        cx.list.push_fill_rect(krect(line_rect), line);
        // Grip dots at the center.
        let grip = cx.color(TokenKey::TextMutedColor, GRIP);
        let gd = GRIP_PT * s;
        let gap = GRIP_SPACING_PT * s;
        let cxm = (b.min_x() + b.max_x()) / 2.0;
        let cym = (b.min_y() + b.max_y()) / 2.0;
        for i in -1..=1 {
            let (dx, dy) = match self.orientation {
                SplitOrientation::Horizontal => (0.0, i as f32 * gap),
                SplitOrientation::Vertical => (i as f32 * gap, 0.0),
            };
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(cxm + dx - gd / 2.0),
                    f64::from(cym + dy - gd / 2.0),
                    f64::from(cxm + dx + gd / 2.0),
                    f64::from(cym + dy + gd / 2.0),
                ),
                &martensite_core::shape::Shape::ELLIPSE,
                grip,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(h: &mut ResizeHandle, w: f32, hgt: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        h.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, hgt),
            },
        );
        h.layout(&mut cx, Rect::new(0.0, 0.0, w, hgt));
    }

    fn ev(h: &mut ResizeHandle, e: &WidgetEvent) -> EventResponse {
        h.event(&mut EventContext {
            event: e,
            bounds: h.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn drag_reports_axis_delta() {
        let mut h = ResizeHandle::horizontal();
        laid_out(&mut h, 6.0, 200.0);
        assert_eq!(
            ev(
                &mut h,
                &WidgetEvent::PointerPressed {
                    button: PointerButton::Primary,
                    position: Vec2::new(3.0, 100.0),
                    count: 1,
                }
            ),
            EventResponse::CapturePointer
        );
        assert!(h.is_dragging());
        ev(
            &mut h,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(23.0, 100.0),
            },
        );
        // Vertical motion is ignored on a horizontal split.
        ev(
            &mut h,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(23.0, 140.0),
            },
        );
        assert_eq!(h.take_moved(), Some(20.0));
        assert_eq!(
            ev(
                &mut h,
                &WidgetEvent::PointerReleased {
                    button: PointerButton::Primary,
                    position: Vec2::new(23.0, 140.0),
                }
            ),
            EventResponse::ReleasePointer
        );
        assert!(!h.is_dragging());
    }

    #[test]
    fn vertical_split_uses_y() {
        let mut h = ResizeHandle::vertical();
        laid_out(&mut h, 200.0, 6.0);
        ev(
            &mut h,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(100.0, 3.0),
                count: 1,
            },
        );
        ev(
            &mut h,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(80.0, 13.0),
            },
        );
        assert_eq!(h.take_moved(), Some(10.0));
    }

    #[test]
    fn arrows_nudge() {
        let mut h = ResizeHandle::horizontal();
        laid_out(&mut h, 6.0, 200.0);
        ev(
            &mut h,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        ev(
            &mut h,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        ev(
            &mut h,
            &WidgetEvent::KeyPressed {
                key: "ArrowLeft".to_string(),
                repeat: false,
            },
        );
        assert_eq!(h.take_moved(), Some(1.0));
    }

    #[test]
    fn double_click_resets() {
        let mut h = ResizeHandle::horizontal();
        laid_out(&mut h, 6.0, 200.0);
        ev(
            &mut h,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(3.0, 100.0),
                count: 2,
            },
        );
        assert!(h.take_reset());
        assert!(!h.is_dragging());
    }

    #[test]
    fn disabled_ignores_input() {
        let mut h = ResizeHandle::horizontal().enabled(false);
        laid_out(&mut h, 6.0, 200.0);
        assert_eq!(
            ev(
                &mut h,
                &WidgetEvent::PointerPressed {
                    button: PointerButton::Primary,
                    position: Vec2::new(3.0, 100.0),
                    count: 1,
                }
            ),
            EventResponse::Ignored
        );
    }
}
