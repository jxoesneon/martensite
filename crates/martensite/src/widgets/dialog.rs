//! `Dialog` widget: a modal card — title, body, and a row of action
//! buttons — meant to be hosted in [`OverlayLayer`] with
//! `OverlayAnchor::Center` + [`OverlayOptions::modal`].
//!
//! [`OverlayLayer`]: martensite_core::overlay::OverlayLayer
//! [`OverlayOptions::modal`]: martensite_core::overlay::OverlayOptions::modal
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::dialog::Dialog;
//!
//! let d = Dialog::new("Delete file?")
//!     .body("This cannot be undone.")
//!     .buttons(&["Cancel", "Delete"]);
//! assert_eq!(d.buttons.len(), 2);
//! ```

use std::sync::{Arc, Mutex};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

const INK: [u8; 4] = [20, 20, 25, 255];
const MUTED: [u8; 4] = [110, 115, 125, 255];
const ACCENT: [u8; 4] = [40, 110, 220, 255];
const RAISED: [u8; 4] = [48, 51, 58, 255];
/// Card geometry (logical points).
const CARD_W: f32 = 360.0;
const PAD: f32 = 20.0;
const TITLE_H: f32 = 28.0;
const BUTTON_H: f32 = 28.0;
const BUTTON_W: f32 = 88.0;
const BUTTON_GAP: f32 = 8.0;

/// A modal dialog card. Host it in the overlay layer at
/// `OverlayAnchor::Center` with `OverlayOptions::modal()`; poll
/// [`Dialog::take_response`] after dispatch to learn which button index
/// was pressed, then close the overlay entry.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Dialog;
///
/// let d = Dialog::new("Confirm").buttons(&["OK"]);
/// assert_eq!(d.buttons, vec!["OK".to_string()]);
/// ```
pub struct Dialog {
    /// Title shown at the top of the card.
    pub title: String,
    /// Plain-text body (mutually exclusive with `child`).
    pub body: String,
    /// Optional rich body widget (mutually exclusive with `body`).
    pub child: Option<Box<dyn Widget>>,
    /// Button labels, left-to-right. The LAST button is painted as the
    /// accent (primary) action — the platform convention.
    pub buttons: Vec<String>,
    /// Button index pressed since the last `take_response`.
    response: Option<usize>,
    /// Shared cell also receiving the pressed index — the overlay-host
    /// pattern (the host can't downcast the entry's `dyn Widget`, so
    /// responses travel through a shared cell like `ContextMenu`).
    response_sink: Option<Arc<Mutex<Option<usize>>>>,
    /// Cached card bounds.
    cached_bounds: Rect,
    /// Cached body rect for the child.
    body_rect: Rect,
    /// Cached button rects (device px).
    button_rects: Vec<Rect>,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Dialog {
    /// A dialog card with the given title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Dialog;
    ///
    /// let d = Dialog::new("About");
    /// assert_eq!(d.title, "About");
    /// ```
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: String::new(),
            child: None,
            buttons: Vec::new(),
            response: None,
            response_sink: None,
            cached_bounds: Rect::default(),
            body_rect: Rect::default(),
            button_rects: Vec::new(),
            text_painter: None,
        }
    }

    /// Sets a plain-text body.
    #[must_use]
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    /// Sets a rich body widget.
    #[must_use]
    pub fn content(mut self, child: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    /// Sets the action buttons (last = primary).
    #[must_use]
    pub fn buttons(mut self, labels: &[&str]) -> Self {
        self.buttons = labels.iter().map(|s| (*s).to_string()).collect();
        self
    }

    /// Returns the pressed button index once, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Dialog;
    ///
    /// let mut d = Dialog::new("X").buttons(&["A", "B"]);
    /// assert_eq!(d.take_response(), None);
    /// ```
    pub fn take_response(&mut self) -> Option<usize> {
        self.response.take()
    }

    /// Wires a shared cell that receives the pressed button index —
    /// the overlay-host observation seam (mirrors `take_response`).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::{Arc, Mutex};
    /// use martensite::widgets::Dialog;
    ///
    /// let sink = Arc::new(Mutex::new(None));
    /// let d = Dialog::new("X").buttons(&["OK"]).response_sink(sink.clone());
    /// assert!(sink.lock().unwrap().is_none());
    /// ```
    #[must_use]
    pub fn response_sink(mut self, sink: Arc<Mutex<Option<usize>>>) -> Self {
        self.response_sink = Some(sink);
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] for real glyph runs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }
}

impl Widget for Dialog {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let w = cx.pt(CARD_W).min(constraints.max_size.x.max(0.0));
        let pad = cx.pt(PAD);
        let title_h = cx.pt(TITLE_H);
        let button_h = if self.buttons.is_empty() {
            0.0
        } else {
            cx.pt(BUTTON_H) + pad
        };
        let body_h = if let Some(child) = &mut self.child {
            child
                .measure(
                    cx,
                    LayoutConstraints {
                        min_size: Vec2::ZERO,
                        max_size: Vec2::new(w - pad * 2.0, constraints.max_size.y),
                    },
                )
                .y
        } else {
            cx.pt(if self.body.is_empty() { 0.0 } else { 20.0 })
        };
        let h = title_h + pad + body_h + pad + button_h;
        Vec2::new(w, h.min(constraints.max_size.y.max(0.0)))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        cx.hot.flags |= martensite_core::NodeFlags::FOCUSABLE;
        let pad = cx.pt(PAD);
        let title_h = cx.pt(TITLE_H);
        let button_h = cx.pt(BUTTON_H);
        // Buttons sit right-aligned at the bottom, laid out in order.
        self.button_rects.clear();
        let mut bx = bounds.max_x() - pad;
        let by = bounds.max_y() - pad - button_h;
        for _ in &self.buttons {
            let w = cx.pt(BUTTON_W);
            bx -= w;
            self.button_rects.push(Rect::new(bx, by, w, button_h));
            bx -= cx.pt(BUTTON_GAP);
        }
        let body_top = bounds.origin.y + title_h + pad;
        self.body_rect = Rect::new(
            bounds.origin.x + pad,
            body_top,
            (bounds.size.x - pad * 2.0).max(0.0),
            (by - pad - body_top).max(0.0),
        );
        if let Some(child) = &mut self.child {
            cx.layout_child(child.as_mut(), self.body_rect);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Dialog);
        node.set_label(self.title.as_str());
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Body child first — it owns its rect.
        if let Some(pos) = cx.event.position() {
            if let Some(child) = &mut self.child {
                if self.body_rect.contains(pos) {
                    let r = child.event(cx);
                    if r != EventResponse::Ignored {
                        return r;
                    }
                }
            }
        }
        if let WidgetEvent::PointerReleased {
            position,
            button: PointerButton::Primary,
        } = cx.event
        {
            for (i, r) in self.button_rects.iter().enumerate() {
                if r.contains(*position) {
                    self.response = Some(i);
                    if let Some(sink) = &self.response_sink {
                        if let Ok(mut cell) = sink.lock() {
                            *cell = Some(i);
                        }
                    }
                    return EventResponse::RequestRepaint;
                }
            }
        }
        // A modal card swallows everything inside its bounds — clicks
        // must not leak to the scrim's dismissal path.
        match cx.event {
            WidgetEvent::PointerMoved { .. }
            | WidgetEvent::PointerPressed { .. }
            | WidgetEvent::PointerReleased { .. }
            | WidgetEvent::Scroll { .. } => EventResponse::Handled,
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.origin.x),
            f64::from(b.origin.y),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let shape = Shape::rounded(cx.dim(TokenKey::BorderRadiusLarge, 12.0));
        cx.list
            .push_fill_shape(rect, &shape, cx.color(TokenKey::SurfaceColor, RAISED));
        cx.list.push_stroke_shape(
            rect,
            &shape,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, [110, 115, 125, 255]),
        );

        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        // Title and body are clipped to the card interior — an
        // over-long string can't spill past the rounded chrome.
        let interior = kurbo::Rect::new(
            f64::from(b.origin.x + cx.pt(PAD)),
            f64::from(b.origin.y),
            f64::from(b.max_x() - cx.pt(PAD)),
            f64::from(b.max_y()),
        );
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            interior,
            kurbo::Point::new(
                f64::from(b.origin.x + cx.pt(PAD)),
                f64::from(b.origin.y + cx.pt(PAD) * 0.75),
            ),
            &self.title,
            cx.pt(16.0),
            cx.color(TokenKey::TextColor, INK),
        );
        if self.child.is_none() && !self.body.is_empty() {
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(self.body_rect.origin.x),
                    f64::from(self.body_rect.origin.y),
                    f64::from(self.body_rect.max_x()),
                    f64::from(self.body_rect.max_y()),
                ),
                kurbo::Point::new(
                    f64::from(self.body_rect.origin.x),
                    f64::from(self.body_rect.origin.y),
                ),
                &self.body,
                cx.pt(13.0),
                cx.color(TokenKey::TextMutedColor, MUTED),
            );
        }
        let last = self.buttons.len().saturating_sub(1);
        for (i, (label, r)) in self.buttons.iter().zip(&self.button_rects).enumerate() {
            let br = kurbo::Rect::new(
                f64::from(r.origin.x),
                f64::from(r.origin.y),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            let primary = i == last;
            let bshape = Shape::rounded(cx.dim(TokenKey::BorderRadius, 6.0));
            cx.list.push_fill_shape(
                br,
                &bshape,
                if primary {
                    cx.color(TokenKey::AccentColor, ACCENT)
                } else {
                    cx.color(TokenKey::SurfaceColor, RAISED)
                },
            );
            cx.list.push_stroke_shape(
                br,
                &bshape,
                cx.pt(1.0),
                cx.color(TokenKey::BorderColor, [110, 115, 125, 255]),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(r.origin.x + cx.pt(12.0)),
                    f64::from(r.origin.y),
                    f64::from(r.max_x() - cx.pt(8.0)),
                    f64::from(r.max_y()),
                ),
                kurbo::Point::new(
                    f64::from(r.origin.x + cx.pt(12.0)),
                    f64::from(r.origin.y + (r.size.y - cx.pt(13.0)) / 2.0),
                ),
                label,
                cx.pt(13.0),
                if primary {
                    [255, 255, 255, 255]
                } else {
                    cx.color(TokenKey::TextColor, INK)
                },
            );
        }
    }

    fn child_count(&self) -> usize {
        usize::from(self.child.is_some())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index == 0 {
            self.child.as_deref()
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index == 0 {
            self.child.as_deref_mut()
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index == 0 && self.child.is_some() {
            Some(self.body_rect)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for Dialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dialog")
            .field("title", &self.title)
            .field("buttons", &self.buttons)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{EventContext, HotNode};

    #[test]
    fn dialog_button_response() {
        let mut hot = HotNode::default();
        let mut d = Dialog::new("Confirm").buttons(&["Cancel", "OK"]);
        let mut lcx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let bounds = Rect::new(100.0, 100.0, 360.0, 160.0);
        d.layout(&mut lcx, bounds);
        // Press inside the primary (last) button rect.
        let r = d.button_rects[1];
        let mut ecx = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: Vec2::new(r.origin.x + 4.0, r.origin.y + 4.0),
                button: PointerButton::Primary,
            },
            bounds,
            scale: 1.0,
        };
        assert_eq!(d.event(&mut ecx), EventResponse::RequestRepaint);
        assert_eq!(d.take_response(), Some(1));
        assert_eq!(d.take_response(), None);
    }

    #[test]
    fn dialog_swallows_card_presses() {
        let mut d = Dialog::new("Confirm");
        let mut ecx = EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(120.0, 120.0),
            },
            bounds: Rect::new(100.0, 100.0, 360.0, 160.0),
            scale: 1.0,
        };
        assert_eq!(d.event(&mut ecx), EventResponse::Handled);
    }
}
