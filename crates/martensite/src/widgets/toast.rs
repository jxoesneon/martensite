//! `Toast` / `ToastHost`: stacked transient notifications.
//!
//! `ToastHost` is designed to live in a [`OverlayLayer`] entry opened
//! with [`OverlayOptions::passthrough`] at a
//! `OverlayAnchor::Viewport` corner — it owns a strip, paints its
//! cards, and ignores (falls through) any event that lands outside a
//! card.
//!
//! [`OverlayLayer`]: martensite_core::overlay::OverlayLayer
//! [`OverlayOptions::passthrough`]: martensite_core::overlay::OverlayOptions::passthrough
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::toast::{Toast, ToastHost};
//! use martensite::widgets::banner::Severity;
//!
//! let mut host = ToastHost::new();
//! host.push(Toast::new(Severity::Info, "Saved").ttl_secs(3.0));
//! assert_eq!(host.len(), 1);
//! ```

use std::sync::{Arc, Mutex};
use std::time::Instant;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

use crate::widgets::banner::Severity;

const INK: [u8; 4] = [20, 20, 25, 255];
const CARD_BG: [u8; 4] = [48, 51, 58, 255];
/// Card geometry in logical points.
const CARD_W: f32 = 280.0;
const CARD_H: f32 = 44.0;
const CARD_GAP: f32 = 8.0;

/// A single toast card's payload.
///
/// # Examples
///
/// ```
/// use martensite::widgets::toast::Toast;
/// use martensite::widgets::banner::Severity;
///
/// let t = Toast::new(Severity::Info, "Copied").ttl_secs(2.5);
/// assert_eq!(t.message, "Copied");
/// ```
#[derive(Clone, Debug)]
pub struct Toast {
    /// Severity accent.
    pub severity: Severity,
    /// Message text.
    pub message: String,
    /// Seconds before the toast expires (0 = sticky).
    pub ttl_secs: f32,
    /// When the toast was pushed.
    created: Instant,
}

impl Toast {
    /// A toast with the default 4s lifetime.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::toast::Toast;
    /// use martensite::widgets::banner::Severity;
    ///
    /// let t = Toast::new(Severity::Warning, "Slow network");
    /// assert!(t.ttl_secs > 0.0);
    /// ```
    pub fn new(severity: Severity, message: impl Into<String>) -> Self {
        Self {
            severity,
            message: message.into(),
            ttl_secs: 4.0,
            created: Instant::now(),
        }
    }

    /// Sets the lifetime in seconds (0 = sticky until dismissed).
    #[must_use]
    pub fn ttl_secs(mut self, secs: f32) -> Self {
        self.ttl_secs = secs.max(0.0);
        self
    }

    /// Whether the toast's TTL has elapsed.
    pub fn expired(&self) -> bool {
        self.ttl_secs > 0.0 && self.created.elapsed().as_secs_f32() >= self.ttl_secs
    }

    /// Fraction of the TTL remaining, 1→0 (sticky toasts stay at 1).
    pub fn remaining(&self) -> f32 {
        if self.ttl_secs <= 0.0 {
            1.0
        } else {
            (1.0 - self.created.elapsed().as_secs_f32() / self.ttl_secs).clamp(0.0, 1.0)
        }
    }
}

/// A stack of toast cards, newest at the bottom. Push with
/// [`ToastHost::push`]; call [`ToastHost::tick`] once per frame to reap
/// expired cards (it returns `true` when a repaint is needed).
///
/// # Examples
///
/// ```
/// use martensite::widgets::toast::{Toast, ToastHost};
/// use martensite::widgets::banner::Severity;
///
/// let mut host = ToastHost::new();
/// host.push(Toast::new(Severity::Info, "A"));
/// host.push(Toast::new(Severity::Info, "B"));
/// assert_eq!(host.len(), 2);
/// ```
#[derive(Clone)]
pub struct ToastHost {
    /// Live toasts, oldest first (painted top→bottom). Shared through
    /// `Arc<Mutex>` so a clone pushed into an overlay entry and the
    /// owning widget mutate the same list — a card dismissed inside
    /// the overlay is gone for the owner too (no resurrection on the
    /// next `replace_content`).
    toasts: Arc<Mutex<Vec<Toast>>>,
    /// Maximum visible cards — older ones are dropped.
    pub max_visible: usize,
    /// Cached bounds of the strip.
    cached_bounds: Rect,
    /// Per-card rects from the last layout pass (device px).
    card_rects: Vec<Rect>,
    /// Shared inbox drained by `tick` — the overlay-host push seam:
    /// producers enqueue `Toast`s through the cell; the host reaps
    /// them on the next tick.
    queue: Option<Arc<Mutex<Vec<Toast>>>>,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl ToastHost {
    /// An empty toast stack.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::toast::ToastHost;
    ///
    /// let host = ToastHost::new();
    /// assert!(host.is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            toasts: Arc::new(Mutex::new(Vec::new())),
            max_visible: 5,
            cached_bounds: Rect::default(),
            card_rects: Vec::new(),
            queue: None,
            text_painter: None,
        }
    }

    /// Wires a shared inbox of pending toasts drained each `tick`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::{Arc, Mutex};
    /// use martensite::widgets::toast::{Toast, ToastHost};
    /// use martensite::widgets::banner::Severity;
    ///
    /// let q = Arc::new(Mutex::new(Vec::new()));
    /// let mut host = ToastHost::new().queue(q.clone());
    /// q.lock().unwrap().push(Toast::new(Severity::Info, "hi"));
    /// host.tick();
    /// assert_eq!(host.len(), 1);
    /// ```
    #[must_use]
    pub fn queue(mut self, queue: Arc<Mutex<Vec<Toast>>>) -> Self {
        self.queue = Some(queue);
        self
    }

    /// Pushes a toast, dropping the oldest beyond `max_visible`.
    pub fn push(&mut self, toast: Toast) {
        if let Ok(mut toasts) = self.toasts.lock() {
            toasts.push(toast);
            let overflow = toasts.len().saturating_sub(self.max_visible);
            if overflow > 0 {
                toasts.drain(..overflow);
            }
        }
    }

    /// Number of live toasts.
    pub fn len(&self) -> usize {
        self.toasts.lock().map(|t| t.len()).unwrap_or(0)
    }

    /// Whether the stack is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The message of the toast at `index`, if present.
    pub fn message_at(&self, index: usize) -> Option<String> {
        self.toasts
            .lock()
            .ok()
            .and_then(|t| t.get(index).map(|t| t.message.clone()))
    }

    /// Reaps expired toasts; returns `true` when a repaint is needed
    /// (something expired or cards remain animating).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::toast::{Toast, ToastHost};
    /// use martensite::widgets::banner::Severity;
    ///
    /// let mut host = ToastHost::new();
    /// host.push(Toast::new(Severity::Info, "x").ttl_secs(30.0));
    /// assert!(host.tick());
    /// ```
    pub fn tick(&mut self) -> bool {
        let pending: Vec<Toast> = self
            .queue
            .as_ref()
            .and_then(|q| q.lock().ok().map(|mut q| q.drain(..).collect()))
            .unwrap_or_default();
        for t in pending {
            self.push(t);
        }
        let Ok(mut toasts) = self.toasts.lock() else {
            return false;
        };
        let before = toasts.len();
        toasts.retain(|t| !t.expired());
        // Repaint whenever a card is still shrinking its remaining bar.
        before != toasts.len() || toasts.iter().any(|t| t.ttl_secs > 0.0)
    }

    /// Shares a [`crate::text_paint::TextPainter`] for real glyph runs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The card rect at index (device px).
    pub fn card_bounds(&self, index: usize) -> Option<Rect> {
        self.card_rects.get(index).copied()
    }
}

impl Default for ToastHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ToastHost {
    fn measure(&mut self, cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        let n = self.len() as f32;
        let h = if n == 0.0 {
            0.0
        } else {
            n * CARD_H + (n - 1.0) * CARD_GAP
        };
        Vec2::new(cx.pt(CARD_W), cx.pt(h))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.card_rects.clear();
        let card_h = cx.pt(CARD_H);
        let gap = cx.pt(CARD_GAP);
        let mut y = bounds.origin.y;
        for _ in 0..self.len() {
            self.card_rects
                .push(Rect::new(bounds.origin.x, y, bounds.size.x, card_h));
            y += card_h + gap;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Status);
        if let Ok(toasts) = self.toasts.lock() {
            if let Some(t) = toasts.last() {
                node.set_label(t.message.as_str());
            }
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Only card interiors are interactive — everything else falls
        // through (the passthrough contract).
        if let WidgetEvent::PointerReleased {
            position,
            button: PointerButton::Primary,
        } = cx.event
        {
            for (i, r) in self.card_rects.iter().enumerate() {
                if r.contains(*position) {
                    if let Ok(mut toasts) = self.toasts.lock() {
                        toasts.remove(i);
                    }
                    return EventResponse::RequestRepaint;
                }
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let shape = Shape::rounded(cx.dim(TokenKey::BorderRadius, 8.0));
        let Ok(toasts) = self.toasts.lock() else {
            return;
        };
        for (toast, r) in toasts.iter().zip(&self.card_rects) {
            let rect = kurbo::Rect::new(
                f64::from(r.origin.x),
                f64::from(r.origin.y),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            cx.list
                .push_fill_shape(rect, &shape, cx.color(TokenKey::SurfaceColor, CARD_BG));
            cx.list.push_stroke_shape(
                rect,
                &shape,
                cx.pt(1.0),
                cx.color(TokenKey::BorderColor, [110, 115, 125, 255]),
            );
            // Severity dot.
            let accent = toast.severity.accent();
            let d = cx.pt(8.0);
            let dy = r.origin.y + (r.size.y - d) / 2.0;
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(r.origin.x + cx.pt(12.0)),
                    f64::from(dy),
                    f64::from(r.origin.x + cx.pt(12.0) + d),
                    f64::from(dy + d),
                ),
                &Shape::ELLIPSE,
                accent,
            );
            // Clip the message to the card minus the leading dot and
            // trailing pad — an over-long message can't spill past
            // the card edge.
            let text_x = r.origin.x + cx.pt(28.0);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(text_x),
                    f64::from(r.origin.y),
                    f64::from(r.max_x() - cx.pt(8.0)),
                    f64::from(r.max_y()),
                ),
                kurbo::Point::new(
                    f64::from(text_x),
                    f64::from(r.origin.y + (r.size.y - cx.pt(13.0)) / 2.0),
                ),
                &toast.message,
                cx.pt(13.0),
                cx.color(TokenKey::TextColor, INK),
            );
            // TTL remaining bar along the card's bottom edge.
            if toast.ttl_secs > 0.0 {
                let frac = toast.remaining();
                if frac > 0.0 {
                    let bw = r.size.x * frac;
                    cx.list.push_fill_rect(
                        kurbo::Rect::new(
                            f64::from(r.origin.x),
                            f64::from(r.max_y() - 2.0),
                            f64::from(r.origin.x + bw),
                            f64::from(r.max_y()),
                        ),
                        accent,
                    );
                }
            }
        }
    }
}

impl std::fmt::Debug for ToastHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToastHost")
            .field("toasts", &self.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{EventContext, HotNode};

    #[test]
    fn toast_push_caps() {
        let mut host = ToastHost::new();
        host.max_visible = 2;
        for i in 0..4 {
            host.push(Toast::new(Severity::Info, format!("t{i}")));
        }
        assert_eq!(host.len(), 2);
        assert_eq!(host.message_at(0).as_deref(), Some("t2"));
    }

    #[test]
    fn toast_expiry() {
        let mut host = ToastHost::new();
        host.push(Toast::new(Severity::Info, "gone").ttl_secs(0.001));
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(host.tick());
        assert!(host.is_empty());
    }

    #[test]
    fn toast_click_dismisses_card() {
        let mut hot = HotNode::default();
        let mut host = ToastHost::new();
        host.push(Toast::new(Severity::Info, "x").ttl_secs(0.0));
        let mut lcx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        host.layout(&mut lcx, Rect::new(0.0, 0.0, 280.0, 44.0));
        let mut ecx = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: Vec2::new(100.0, 20.0),
                button: PointerButton::Primary,
            },
            bounds: Rect::new(0.0, 0.0, 280.0, 44.0),
            scale: 1.0,
        };
        assert_eq!(host.event(&mut ecx), EventResponse::RequestRepaint);
        assert!(host.is_empty());
    }

    #[test]
    fn toast_outside_card_ignored() {
        let mut host = ToastHost::new();
        host.push(Toast::new(Severity::Info, "x"));
        let mut ecx = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: Vec2::new(5000.0, 5000.0),
                button: PointerButton::Primary,
            },
            bounds: Rect::new(0.0, 0.0, 280.0, 44.0),
            scale: 1.0,
        };
        assert_eq!(host.event(&mut ecx), EventResponse::Ignored);
        assert_eq!(host.len(), 1);
    }
}
