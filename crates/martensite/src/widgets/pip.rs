//! `Pip` — a floating picture-in-picture mini window hosting one
//! child (video-call / media overlay idiom).
//!
//! The widget *is* the mini window: it frames its child with a
//! rounded border, reveals close/expand buttons on hover, and
//! reports drags as cumulative deltas through
//! [`Pip::take_dragged`] — the host owns the overlay placement and
//! applies the delta to its layout. Button presses park
//! [`Pip::take_closed`]/[`Pip::take_expanded`].
//!
//! # Examples
//!
//! ```
//! use martensite::core::Widget;
//! use martensite::widgets::{Pip, Text};
//!
//! let p = Pip::new(Text::new("call"));
//! assert_eq!(p.child_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const BTN_PT: f32 = 18.0;
const BTN_GAP_PT: f32 = 6.0;
const BTN_PAD_PT: f32 = 6.0;
const BORDER_PT: f32 = 1.0;

const FACE_HOVER: [u8; 4] = [58, 60, 68, 255];
const CLOSE_HOVER: [u8; 4] = [196, 43, 43, 255];
const GLYPH: [u8; 4] = [220, 222, 228, 255];
const EDGE: [u8; 4] = [0, 0, 0, 90];

/// A draggable PiP window — see the module docs.
///
/// ```
/// use martensite::core::Widget;
/// use martensite::widgets::Pip;
///
/// assert_eq!(Pip::new(martensite_core::DummyWidget).child_count(), 1);
/// ```
pub struct Pip {
    /// Accessibility label.
    pub label: String,
    content: Box<dyn Widget>,
    /// Show the close button.
    pub closable: bool,
    /// Show the expand button.
    pub maximizable: bool,
    hovered: bool,
    held_drag: bool,
    last_pos: Option<Vec2>,
    drag_delta: Option<Vec2>,
    closed: bool,
    expanded: bool,
    close_rect: Rect,
    expand_rect: Rect,
    content_rect: Rect,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Pip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pip")
            .field("closable", &self.closable)
            .field("maximizable", &self.maximizable)
            .finish()
    }
}

impl Pip {
    /// PiP window hosting `content`.
    ///
    /// ```
    /// use martensite::core::Widget;
    /// use martensite::widgets::Pip;
    ///
    /// assert_eq!(Pip::new(martensite_core::DummyWidget).child_count(), 1);
    /// ```
    pub fn new(content: impl Widget + 'static) -> Self {
        Self {
            label: "Picture in picture".to_string(),
            content: Box::new(content),
            closable: true,
            maximizable: true,
            hovered: false,
            held_drag: false,
            last_pos: None,
            drag_delta: None,
            closed: false,
            expanded: false,
            close_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            expand_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            content_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::Pip;
    ///
    /// assert_eq!(Pip::new(martensite_core::DummyWidget).label("Call").label, "Call");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Replaces the hosted content.
    ///
    /// ```
    /// use martensite::core::Widget;
    /// use martensite::widgets::{Pip, Text};
    ///
    /// let mut p = Pip::new(martensite_core::DummyWidget);
    /// p.set_content(Text::new("v"));
    /// assert_eq!(p.child_count(), 1);
    /// ```
    pub fn set_content(&mut self, content: impl Widget + 'static) {
        self.content = Box::new(content);
    }

    /// Drains the cumulative drag delta — the host applies it to
    /// the overlay's placement.
    ///
    /// ```
    /// use martensite::widgets::Pip;
    ///
    /// let mut p = Pip::new(martensite_core::DummyWidget);
    /// assert_eq!(p.take_dragged(), None);
    /// ```
    pub fn take_dragged(&mut self) -> Option<Vec2> {
        self.drag_delta.take()
    }

    /// Drains a close-button press.
    ///
    /// ```
    /// use martensite::widgets::Pip;
    ///
    /// let mut p = Pip::new(martensite_core::DummyWidget);
    /// assert!(!p.take_closed());
    /// ```
    pub fn take_closed(&mut self) -> bool {
        std::mem::take(&mut self.closed)
    }

    /// Drains an expand-button press.
    ///
    /// ```
    /// use martensite::widgets::Pip;
    ///
    /// let mut p = Pip::new(martensite_core::DummyWidget);
    /// assert!(!p.take_expanded());
    /// ```
    pub fn take_expanded(&mut self) -> bool {
        std::mem::take(&mut self.expanded)
    }

    /// Button hit zones — visible only while hovered.
    fn button_at(&self, p: Vec2) -> Option<&'static str> {
        if !self.hovered {
            return None;
        }
        if self.closable && self.close_rect.contains(p) {
            return Some("close");
        }
        if self.maximizable && self.expand_rect.contains(p) {
            return Some("expand");
        }
        None
    }
}

impl Widget for Pip {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let inner = self.content.measure(cx, constraints);
        Vec2::new(
            (inner.x + BORDER_PT * 2.0 * cx.scale).min(constraints.max_size.x.max(0.0)),
            (inner.y + BORDER_PT * 2.0 * cx.scale).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(48.0, 36.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let inset = BORDER_PT * cx.scale;
        self.content_rect = Rect::new(
            bounds.min_x() + inset,
            bounds.min_y() + inset,
            (bounds.width() - inset * 2.0).max(0.0),
            (bounds.height() - inset * 2.0).max(0.0),
        );
        let btn = BTN_PT * cx.scale;
        let pad = BTN_PAD_PT * cx.scale;
        let y = bounds.min_y() + pad;
        if self.closable {
            self.close_rect = Rect::new(bounds.max_x() - pad - btn, y, btn, btn);
        }
        if self.maximizable {
            let x = if self.closable {
                self.close_rect.min_x() - BTN_GAP_PT * cx.scale - btn
            } else {
                bounds.max_x() - pad - btn
            };
            self.expand_rect = Rect::new(x, y, btn, btn);
        }
        self.content.layout(cx, self.content_rect);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Window);
        node.set_label(self.label.clone());
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                if self.held_drag {
                    if let Some(last) = self.last_pos {
                        let d = *position - last;
                        if d != Vec2::ZERO {
                            let acc = self.drag_delta.get_or_insert(Vec2::ZERO);
                            *acc += d;
                        }
                    }
                    self.last_pos = Some(*position);
                    return EventResponse::Handled;
                }
                let inside = self.bounds.contains(*position);
                if inside != self.hovered {
                    self.hovered = inside;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered {
                    self.hovered = false;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                match self.button_at(*position) {
                    Some("close") => {
                        self.closed = true;
                        return EventResponse::Handled;
                    }
                    Some("expand") => {
                        self.expanded = true;
                        return EventResponse::Handled;
                    }
                    _ => {}
                }
                // Drag anywhere else — the host repositions us.
                self.held_drag = true;
                self.last_pos = Some(*position);
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.held_drag {
                    self.held_drag = false;
                    self.last_pos = None;
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        // Edge shadow + frame.
        let outer = kurbo::Rect::new(
            f64::from(self.bounds.min_x()),
            f64::from(self.bounds.min_y()),
            f64::from(self.bounds.max_x()),
            f64::from(self.bounds.max_y()),
        );
        let edge = cx.color(TokenKey::DividerColor, EDGE);
        cx.list.push_stroke_rect(outer, BORDER_PT * s, edge);

        // Hover buttons.
        if self.hovered {
            let pt = |v: Vec2| kurbo::Point::new(f64::from(v.x), f64::from(v.y));
            for (rect, face, kind) in [
                (self.expand_rect, FACE_HOVER, 0u8),
                (self.close_rect, CLOSE_HOVER, 1u8),
            ] {
                let r = kurbo::Rect::new(
                    f64::from(rect.min_x()),
                    f64::from(rect.min_y()),
                    f64::from(rect.max_x()),
                    f64::from(rect.max_y()),
                );
                if rect.width() <= 0.0 {
                    continue;
                }
                let shape = martensite_core::shape::Shape::rounded(4.0 * s);
                cx.list.push_fill_shape(r, &shape, face);
                let cxm = (rect.min_x() + rect.max_x()) / 2.0;
                let cym = (rect.min_y() + rect.max_y()) / 2.0;
                let g = 3.0 * s;
                let glyph = cx.color(TokenKey::TextColor, GLYPH);
                let mut path = kurbo::BezPath::new();
                if kind == 0 {
                    // Expand: ↗ arrow.
                    path.move_to(pt(Vec2::new(cxm - g, cym + g)));
                    path.line_to(pt(Vec2::new(cxm + g, cym - g)));
                    path.move_to(pt(Vec2::new(cxm, cym - g)));
                    path.line_to(pt(Vec2::new(cxm + g, cym - g)));
                    path.line_to(pt(Vec2::new(cxm + g, cym)));
                } else {
                    // Close: ×.
                    path.move_to(pt(Vec2::new(cxm - g, cym - g)));
                    path.line_to(pt(Vec2::new(cxm + g, cym + g)));
                    path.move_to(pt(Vec2::new(cxm + g, cym - g)));
                    path.line_to(pt(Vec2::new(cxm - g, cym + g)));
                }
                cx.list.push_stroke_path(path, 1.4 * s, glyph);
            }
        }
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&*self.content)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut *self.content)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.content_rect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{DummyWidget, HotNode};

    fn laid_out(p: &mut Pip) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        p.layout(&mut cx, Rect::new(100.0, 100.0, 160.0, 90.0));
    }

    fn ev(p: &mut Pip, e: &WidgetEvent) -> EventResponse {
        p.event(&mut EventContext {
            event: e,
            bounds: p.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn hosts_child() {
        let mut p = Pip::new(DummyWidget);
        laid_out(&mut p);
        assert_eq!(p.child_count(), 1);
        let cb = p.child_bounds(0).unwrap();
        assert!(cb.width() < p.bounds.width());
    }

    #[test]
    fn drag_parks_delta() {
        let mut p = Pip::new(DummyWidget);
        laid_out(&mut p);
        let start = Vec2::new(150.0, 150.0);
        assert_eq!(
            ev(
                &mut p,
                &WidgetEvent::PointerPressed {
                    button: PointerButton::Primary,
                    position: start,
                    count: 1,
                }
            ),
            EventResponse::CapturePointer
        );
        ev(
            &mut p,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(170.0, 140.0),
            },
        );
        ev(
            &mut p,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(180.0, 140.0),
            },
        );
        assert_eq!(
            ev(
                &mut p,
                &WidgetEvent::PointerReleased {
                    button: PointerButton::Primary,
                    position: Vec2::new(180.0, 140.0),
                }
            ),
            EventResponse::ReleasePointer
        );
        assert_eq!(p.take_dragged(), Some(Vec2::new(30.0, -10.0)));
        assert_eq!(p.take_dragged(), None);
    }

    #[test]
    fn buttons_need_hover() {
        let mut p = Pip::new(DummyWidget);
        laid_out(&mut p);
        // Without hover the close rect is not hot.
        let c = p.close_rect;
        let mid = Vec2::new((c.min_x() + c.max_x()) / 2.0, (c.min_y() + c.max_y()) / 2.0);
        ev(
            &mut p,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: mid,
                count: 1,
            },
        );
        assert!(!p.take_closed());
        ev(
            &mut p,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: mid,
            },
        );
        // Hover first, then press.
        ev(
            &mut p,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(150.0, 150.0),
            },
        );
        ev(
            &mut p,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: mid,
                count: 1,
            },
        );
        assert!(p.take_closed());
    }

    #[test]
    fn expand_parks() {
        let mut p = Pip::new(DummyWidget);
        laid_out(&mut p);
        ev(
            &mut p,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(150.0, 150.0),
            },
        );
        let r = p.expand_rect;
        ev(
            &mut p,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
                count: 1,
            },
        );
        assert!(p.take_expanded());
    }

    #[test]
    fn paint_without_painter() {
        let mut p = Pip::new(DummyWidget);
        laid_out(&mut p);
        p.hovered = true;
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        p.paint(&mut PaintContext {
            list: &mut list,
            bounds: p.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
