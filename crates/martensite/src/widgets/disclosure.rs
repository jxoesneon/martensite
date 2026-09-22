//! `Disclosure` widget: a collapsible section — chevron header plus a
//! child shown only while open.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::disclosure::Disclosure;
//! use martensite::widgets::text::Text;
//!
//! let d = Disclosure::new("Advanced").child(Text::new("details"));
//! assert!(!d.open);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

const INK: [u8; 4] = [20, 20, 25, 255];
/// Header row height in logical points.
const HEADER_H: f32 = 24.0;
/// Chevron glyph side length in logical points.
const CHEVRON: f32 = 8.0;

/// A collapsible section: a header row with a disclosure chevron and
/// title, and a child widget visible only while `open`. The child rides
/// the internal-children protocol (layout, paint, events, a11y).
///
/// # Examples
///
/// ```
/// use martensite::widgets::Disclosure;
/// use martensite::widgets::Text;
///
/// let mut d = Disclosure::new("Details").child(Text::new("body"));
/// d.set_open(true);
/// assert!(d.open);
/// ```
pub struct Disclosure {
    /// Header label.
    pub title: String,
    /// Whether the body is expanded.
    pub open: bool,
    /// The collapsible body widget.
    pub child: Option<Box<dyn Widget>>,
    /// Cached overall bounds.
    cached_bounds: Rect,
    /// Cached header rect (device px).
    header_rect: Rect,
    /// Cached body rect (device px).
    body_rect: Rect,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Disclosure {
    /// A closed disclosure section.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Disclosure;
    ///
    /// let d = Disclosure::new("More");
    /// assert_eq!(d.title, "More");
    /// ```
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            open: false,
            child: None,
            cached_bounds: Rect::default(),
            header_rect: Rect::default(),
            body_rect: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the collapsible body widget.
    #[must_use]
    pub fn child(mut self, child: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    /// Sets the open state.
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    /// Toggles the open state.
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// Shares a [`crate::text_paint::TextPainter`] for real glyph runs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }
}

impl Widget for Disclosure {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let header_h = cx.pt(HEADER_H);
        let w = constraints.max_size.x.max(0.0);
        // Measure the content even when closed — a disclosure opened
        // after layout (`set_open`, the Page rail's responsive
        // collapse) lays out the child, and a child whose caches were
        // never populated renders every row at zero height.
        let body = if let Some(child) = &mut self.child {
            let m = child.measure(cx, constraints).y;
            if self.open {
                m
            } else {
                0.0
            }
        } else {
            0.0
        };
        Vec2::new(w, (header_h + body).min(constraints.max_size.y.max(0.0)))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        cx.hot.flags |= NodeFlags::FOCUSABLE;
        let header_h = cx.pt(HEADER_H).min(bounds.size.y);
        self.header_rect = Rect::new(bounds.origin.x, bounds.origin.y, bounds.size.x, header_h);
        self.body_rect = Rect::new(
            bounds.origin.x,
            bounds.origin.y + header_h,
            bounds.size.x,
            (bounds.size.y - header_h).max(0.0),
        );
        if self.open {
            if let Some(child) = &mut self.child {
                // Re-measure tight against the real allotment — the
                // measure pass may have seen a smaller offer (or run
                // while closed), leaving the child's caches stale.
                child.measure(
                    cx,
                    LayoutConstraints {
                        min_size: Vec2::ZERO,
                        max_size: self.body_rect.size,
                    },
                );
                cx.layout_child(child.as_mut(), self.body_rect);
            }
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::DisclosureTriangle);
        node.set_label(self.title.as_str());
        node.set_expanded(self.open);
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Focus);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Header toggles on release and on Space/Enter.
        let header_hit = matches!(
            cx.event,
            WidgetEvent::PointerReleased {
                position,
                button: PointerButton::Primary,
            } if self.header_rect.contains(*position)
        ) || matches!(cx.event, WidgetEvent::KeyPressed { .. });
        if header_hit {
            self.open = !self.open;
            // Size changed — repaint dirties the node and the next
            // layout pass re-measures it.
            return EventResponse::RequestRepaint;
        }
        // Body events route to the child when open and inside its rect.
        if self.open {
            if let Some(pos) = cx.event.position() {
                if let Some(child) = &mut self.child {
                    if self.body_rect.contains(pos) {
                        return child.event(cx);
                    }
                }
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let h = &self.header_rect;
        let cs = cx.pt(CHEVRON);
        let cy = h.origin.y + (h.size.y - cs) / 2.0;
        let cxl = h.origin.x + cx.pt(6.0);
        // Chevron: right-pointing triangle when closed, down when open —
        // drawn as a stroked caret.
        let mut caret = kurbo::BezPath::new();
        let x0 = f64::from(cxl);
        let y0 = f64::from(cy);
        let s = f64::from(cs);
        if self.open {
            caret.move_to((x0, y0 + s * 0.25));
            caret.line_to((x0 + s / 2.0, y0 + s * 0.75));
            caret.line_to((x0 + s, y0 + s * 0.25));
        } else {
            caret.move_to((x0 + s * 0.25, y0));
            caret.line_to((x0 + s * 0.75, y0 + s / 2.0));
            caret.line_to((x0 + s * 0.25, y0 + s));
        }
        cx.list
            .push_stroke_path(caret, cx.pt(1.5), cx.color(TokenKey::TextMutedColor, INK));

        // Clip the title to the header row — an over-long title
        // can't spill past the widget's right edge.
        let title_x = cxl + cs + cx.pt(6.0);
        crate::text_paint::paint_label_clipped(
            crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
            cx.list,
            kurbo::Rect::new(
                f64::from(title_x),
                f64::from(h.origin.y),
                f64::from(h.max_x() - cx.pt(4.0)),
                f64::from(h.max_y()),
            ),
            kurbo::Point::new(
                f64::from(title_x),
                f64::from(h.origin.y + (h.size.y - cx.pt(14.0)) / 2.0),
            ),
            &self.title,
            cx.pt(14.0),
            cx.color(TokenKey::TextColor, INK),
        );
    }

    fn child_count(&self) -> usize {
        usize::from(self.open && self.child.is_some())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index == 0 && self.open {
            self.child.as_deref()
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index == 0 && self.open {
            self.child.as_deref_mut()
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index == 0 && self.open && self.child.is_some() {
            Some(self.body_rect)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for Disclosure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Disclosure")
            .field("title", &self.title)
            .field("open", &self.open)
            .field("has_child", &self.child.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn disclosure_closed_hides_child() {
        let d = Disclosure::new("More").child(crate::widgets::text::Text::new("x"));
        assert_eq!(d.child_count(), 0);
        assert!(Widget::child(&d, 0).is_none());
    }

    #[test]
    fn disclosure_toggle_reveals_child() {
        let mut hot = HotNode::default();
        let mut d = Disclosure::new("More").child(crate::widgets::text::Text::new("x"));
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let bounds = Rect::new(0.0, 0.0, 200.0, 100.0);
        d.layout(&mut cx, bounds);
        let mut ecx = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: Vec2::new(10.0, 10.0),
                button: PointerButton::Primary,
            },
            bounds,
            scale: 1.0,
        };
        assert_eq!(d.event(&mut ecx), EventResponse::RequestRepaint);
        assert!(d.open);
        assert_eq!(d.child_count(), 1);
    }

    #[test]
    fn disclosure_a11y_expanded() {
        let mut d = Disclosure::new("More");
        d.set_open(true);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        d.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::DisclosureTriangle);
        assert_eq!(node.is_expanded(), Some(true));
    }
}
