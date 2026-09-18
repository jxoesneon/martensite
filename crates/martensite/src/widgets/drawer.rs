//! `Drawer` widget: an edge-docked panel — title bar, close affordance,
//! and a content child — hosted in [`OverlayLayer`] at an
//! `OverlayAnchor::Edge*` anchor.
//!
//! [`OverlayLayer`]: martensite_core::overlay::OverlayLayer
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::drawer::Drawer;
//!
//! let d = Drawer::new("Inspector").width(320.0);
//! assert_eq!(d.depth, 320.0);
//! ```

use std::sync::{Arc, Mutex};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

const INK: [u8; 4] = [20, 20, 25, 255];
const SURFACE: [u8; 4] = [38, 41, 48, 255];
const EDGE: [u8; 4] = [110, 115, 125, 255];
/// Header strip height in logical points.
const HEADER_H: f32 = 40.0;
/// Close button hit box (logical points square).
const CLOSE: f32 = 24.0;

/// A drawer / side panel. Open it with `OverlayAnchor::EdgeRight` (or
/// `EdgeLeft`/`EdgeTop`/`EdgeBottom`) — the anchor pins it to the edge
/// and spans the viewport on the crossing axis; [`Drawer::width`] sets
/// the measured depth on the other axis. Pair with
/// `OverlayOptions::modal().light_dismiss()` for the standard
/// scrim-tap-to-dismiss behavior.
///
/// Poll [`Drawer::take_close_requested`] after dispatch; the host then
/// closes the overlay entry.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Drawer;
///
/// let mut d = Drawer::new("Layers");
/// assert!(!d.take_close_requested());
/// ```
pub struct Drawer {
    /// Title in the header strip.
    pub title: String,
    /// Requested depth (logical points) on the entry axis — width for
    /// left/right drawers, height for top/bottom sheets.
    pub depth: f32,
    /// The content widget filling below the header.
    pub child: Option<Box<dyn Widget>>,
    /// Set when the close affordance was activated.
    close_requested: bool,
    /// Shared cell also receiving the close request — the overlay-host
    /// observation seam (mirrors `take_close_requested`).
    close_sink: Option<Arc<Mutex<bool>>>,
    /// Cached bounds.
    cached_bounds: Rect,
    /// Cached content rect (device px).
    content_rect: Rect,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Drawer {
    /// A drawer with the given header title and a 300pt depth.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Drawer;
    ///
    /// let d = Drawer::new("Inspector");
    /// assert_eq!(d.title, "Inspector");
    /// ```
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            depth: 300.0,
            child: None,
            close_requested: false,
            close_sink: None,
            cached_bounds: Rect::default(),
            content_rect: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the depth on the entry axis (logical points).
    #[must_use]
    pub fn width(mut self, depth: f32) -> Self {
        self.depth = depth.max(80.0);
        self
    }

    /// Sets the content widget.
    #[must_use]
    pub fn content(mut self, child: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    /// Returns true once when the close button was activated.
    pub fn take_close_requested(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }

    /// Wires a shared cell set `true` when the close affordance is
    /// activated — the overlay-host observation seam.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::{Arc, Mutex};
    /// use martensite::widgets::Drawer;
    ///
    /// let sink = Arc::new(Mutex::new(false));
    /// let mut d = Drawer::new("X").close_sink(sink);
    /// assert!(!d.take_close_requested());
    /// ```
    #[must_use]
    pub fn close_sink(mut self, sink: Arc<Mutex<bool>>) -> Self {
        self.close_sink = Some(sink);
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] for real glyph runs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The close-button rect (device px).
    fn close_rect(&self, scale: f32) -> Rect {
        let side = CLOSE * scale;
        Rect::new(
            self.cached_bounds.max_x() - side - 8.0 * scale,
            self.cached_bounds.origin.y + (HEADER_H * scale - side) / 2.0,
            side,
            side,
        )
    }
}

impl Widget for Drawer {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // The edge anchor overrides the spanning axis; the measured
        // axis carries our depth request.
        Vec2::new(
            cx.pt(self.depth).min(constraints.max_size.x.max(0.0)),
            constraints.max_size.y.max(0.0),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        let header_h = cx.pt(HEADER_H).min(bounds.size.y);
        self.content_rect = Rect::new(
            bounds.origin.x,
            bounds.origin.y + header_h,
            bounds.size.x,
            (bounds.size.y - header_h).max(0.0),
        );
        if let Some(child) = &mut self.child {
            cx.layout_child(child.as_mut(), self.content_rect);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.title.as_str());
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let Some(pos) = cx.event.position() {
            if let Some(child) = &mut self.child {
                if self.content_rect.contains(pos) {
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
            if self.close_rect(cx.scale).contains(*position) {
                self.close_requested = true;
                if let Some(sink) = &self.close_sink {
                    if let Ok(mut cell) = sink.lock() {
                        *cell = true;
                    }
                }
                return EventResponse::RequestRepaint;
            }
        }
        // Drawers own their surface — swallow inside events so a scrim
        // press behind us is the only light-dismiss path.
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
        cx.list
            .push_fill_rect(rect, cx.color(TokenKey::SurfaceColor, SURFACE));
        // Hairline separating the header strip.
        let hy = f64::from(b.origin.y + cx.pt(HEADER_H));
        cx.list.push_fill_rect(
            kurbo::Rect::new(f64::from(b.origin.x), hy, f64::from(b.max_x()), hy + 1.0),
            cx.color(TokenKey::BorderColor, EDGE),
        );
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(b.origin.x + cx.pt(14.0)),
                f64::from(b.origin.y + (cx.pt(HEADER_H) - cx.pt(15.0)) / 2.0),
            ),
            &self.title,
            cx.pt(15.0),
            cx.color(TokenKey::TextColor, INK),
        );
        // Close ×.
        let cr = self.close_rect(cx.scale);
        let arm = 5.0 * cx.scale;
        let mid = cr.origin + cr.size / 2.0;
        let mut x = kurbo::BezPath::new();
        x.move_to((mid.x - arm, mid.y - arm));
        x.line_to((mid.x + arm, mid.y + arm));
        x.move_to((mid.x + arm, mid.y - arm));
        x.line_to((mid.x - arm, mid.y + arm));
        cx.list
            .push_stroke_path(x, cx.pt(1.5), cx.color(TokenKey::TextMutedColor, INK));
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
            Some(self.content_rect)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for Drawer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Drawer")
            .field("title", &self.title)
            .field("depth", &self.depth)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{EventContext, HotNode};

    #[test]
    fn drawer_close() {
        let mut hot = HotNode::default();
        let mut d = Drawer::new("Inspector");
        let mut lcx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let bounds = Rect::new(500.0, 0.0, 300.0, 600.0);
        d.layout(&mut lcx, bounds);
        let cr = d.close_rect(1.0);
        let mut ecx = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: Vec2::new(cr.origin.x + 2.0, cr.origin.y + 2.0),
                button: PointerButton::Primary,
            },
            bounds,
            scale: 1.0,
        };
        assert_eq!(d.event(&mut ecx), EventResponse::RequestRepaint);
        assert!(d.take_close_requested());
        assert!(!d.take_close_requested());
    }
}
