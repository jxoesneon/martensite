//! `AlertDialog` widget: a modal severity-tinted alert card
//! (NSAlert / `AlertDialog`).
//!
//! Host it in the [`OverlayLayer`] at `OverlayAnchor::Center` with
//! `OverlayOptions::modal().light_dismiss()` — a scrim covers the
//! window beneath the card, positional input outside it is consumed,
//! and a press on the scrim dismisses the entry.
//!
//! [`OverlayLayer`]: martensite_core::overlay::OverlayLayer
//! [`OverlayOptions::modal`]: martensite_core::overlay::OverlayOptions::modal
//!
//! - **Severity**: [`AlertSeverity`] tints the painted icon and (for
//!   `Warning`/`Error`) the title ink. `destructive(true)` styles the
//!   confirm button in the error colour — the NSAlert destructive
//!   convention.
//! - **Results**: button presses, `Enter` (the highlighted — default
//!   confirm — button), and `Escape` (`Cancel` when a `Cancel`-role
//!   button exists, `Dismissed` otherwise) land in
//!   [`AlertDialog::take_result`] — or the shared cell wired via
//!   [`AlertDialog::result_sink`], the overlay-host observation seam
//!   (mirrors `Dialog::response_sink`). When the entry is closed
//!   without a result — scrim dismissal, layer `Escape`, `clear` —
//!   the card's `Drop` reports `Dismissed` into the sink, so a hosted
//!   alert always produces a terminal [`AlertResult`]. (Hosted
//!   `Escape` reaches the layer before the card and therefore reports
//!   `Dismissed`; embedded, the card's own handler maps it to
//!   `Cancel` first.)
//! - **Traversal**: arrow keys move a highlight across the footer
//!   buttons; `Enter` activates the highlighted one.
//! - **Accessibility**: `Role::AlertDialog` (present in the vendored
//!   AccessKit), the title as the label, and the message as the
//!   description.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::alert_dialog::{AlertDialog, AlertRole};
//!
//! let d = AlertDialog::new()
//!     .title("Delete file?")
//!     .message("This cannot be undone.")
//!     .button("Cancel", AlertRole::Cancel)
//!     .button("Delete", AlertRole::Confirm)
//!     .destructive(true);
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
const INFO: [u8; 4] = [60, 120, 220, 255];
const WARNING: [u8; 4] = [220, 160, 40, 255];
const ERROR: [u8; 4] = [210, 60, 60, 255];
/// Card geometry (logical points).
const CARD_W: f32 = 380.0;
const PAD: f32 = 20.0;
const ICON: f32 = 28.0;
const TITLE_H: f32 = 28.0;
const MESSAGE_H: f32 = 40.0;
const BUTTON_H: f32 = 28.0;
const BUTTON_GAP: f32 = 8.0;

/// Severity of an [`AlertDialog`] — tints the icon and, for
/// `Warning`/`Error`, the title.
///
/// # Examples
///
/// ```
/// use martensite::widgets::alert_dialog::AlertSeverity;
///
/// assert_ne!(AlertSeverity::Info, AlertSeverity::Error);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertSeverity {
    /// Neutral informational alert (blue icon).
    Info,
    /// Caution — attention needed, action still possible (amber icon).
    Warning,
    /// Failure / irreversible consequence (red icon).
    Error,
}

impl AlertSeverity {
    /// Icon (and `Warning`/`Error` title) tint.
    fn accent(self) -> [u8; 4] {
        match self {
            Self::Info => INFO,
            Self::Warning => WARNING,
            Self::Error => ERROR,
        }
    }

    /// The glyph painted inside the severity disc.
    fn glyph(self) -> &'static str {
        match self {
            Self::Info => "i",
            Self::Warning => "!",
            Self::Error => "×",
        }
    }
}

/// The semantic role of an [`AlertDialog`] footer button.
///
/// # Examples
///
/// ```
/// use martensite::widgets::alert_dialog::AlertRole;
///
/// assert_eq!(AlertRole::Other(2), AlertRole::Other(2));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertRole {
    /// The primary action — accent-styled (error-styled when the alert
    /// is `destructive`), the `Enter` default.
    Confirm,
    /// The dismiss action — the `Escape` target.
    Cancel,
    /// Any other action; the tag is echoed back in
    /// [`AlertResult::Other`] unchanged.
    Other(usize),
}

/// The terminal result of an [`AlertDialog`].
///
/// # Examples
///
/// ```
/// use martensite::widgets::alert_dialog::AlertResult;
///
/// assert_ne!(AlertResult::Confirm, AlertResult::Dismissed);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertResult {
    /// A `Confirm`-role button (or `Enter` on it) was activated.
    Confirm,
    /// A `Cancel`-role button (or `Escape` reaching the card) was
    /// activated.
    Cancel,
    /// An `Other(tag)`-role button was activated; `tag` echoes the
    /// role's payload.
    Other(usize),
    /// The alert closed without a choice — scrim dismissal, layer
    /// `Escape`, `clear`, or an embedded `Escape` with no
    /// `Cancel`-role button.
    Dismissed,
}

/// A modal alert card — severity icon, title, message, and a footer
/// button row — hosted in the overlay layer at
/// `OverlayAnchor::Center` with `OverlayOptions::modal().light_dismiss()`.
///
/// Poll [`AlertDialog::take_result`] (embedded) or the
/// [`AlertDialog::result_sink`] cell (hosted) after dispatch; the host
/// then closes the overlay entry.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{AlertDialog, AlertRole};
///
/// let d = AlertDialog::new()
///     .title("Confirm")
///     .button("OK", AlertRole::Confirm);
/// assert_eq!(d.buttons.len(), 1);
/// ```
pub struct AlertDialog {
    /// Title shown beside the severity icon.
    pub title: String,
    /// Message body under the title.
    pub message: String,
    /// Severity tint for the icon/title.
    pub severity: AlertSeverity,
    /// `true` styles the confirm button in the error colour — the
    /// destructive-action convention.
    pub destructive: bool,
    /// Footer buttons `(label, role)`, left-to-right. The first
    /// `Confirm` (or the last button when none is `Confirm`) is the
    /// accent default.
    pub buttons: Vec<(String, AlertRole)>,
    /// Result since the last `take_result`.
    result: Option<AlertResult>,
    /// Shared cell also receiving the result — the overlay-host
    /// observation seam (the host can't downcast the entry's
    /// `dyn Widget`, so results travel through a shared cell like
    /// `Dialog`). On `Drop` an unset slot receives `Dismissed` —
    /// scrim-dismissal and layer `Escape` always produce a terminal
    /// result.
    result_sink: Option<Arc<Mutex<Option<AlertResult>>>>,
    /// Footer highlight for arrow-key traversal.
    highlighted: usize,
    /// Cached card bounds.
    cached_bounds: Rect,
    /// Cached message rect.
    message_rect: Rect,
    /// Cached button rects (device px).
    button_rects: Vec<Rect>,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl AlertDialog {
    /// An empty alert card — compose with the builder methods.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AlertDialog;
    /// use martensite::widgets::alert_dialog::AlertSeverity;
    ///
    /// let d = AlertDialog::new();
    /// assert_eq!(d.severity, AlertSeverity::Info);
    /// ```
    pub fn new() -> Self {
        Self {
            title: String::new(),
            message: String::new(),
            severity: AlertSeverity::Info,
            destructive: false,
            buttons: Vec::new(),
            result: None,
            result_sink: None,
            highlighted: 0,
            cached_bounds: Rect::default(),
            message_rect: Rect::default(),
            button_rects: Vec::new(),
            text_painter: None,
        }
    }

    /// Sets the title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Sets the message body.
    #[must_use]
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// Sets the severity tint.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AlertDialog;
    /// use martensite::widgets::alert_dialog::AlertSeverity;
    ///
    /// let d = AlertDialog::new().severity(AlertSeverity::Warning);
    /// assert_eq!(d.severity, AlertSeverity::Warning);
    /// ```
    #[must_use]
    pub fn severity(mut self, severity: AlertSeverity) -> Self {
        self.severity = severity;
        self
    }

    /// Marks the confirm action destructive (error-styled).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AlertDialog;
    ///
    /// let d = AlertDialog::new().destructive(true);
    /// assert!(d.destructive);
    /// ```
    #[must_use]
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }

    /// Appends a footer button; the first `Confirm` becomes the
    /// accent default (and `Enter` target).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{AlertDialog, AlertRole};
    ///
    /// let d = AlertDialog::new()
    ///     .button("Cancel", AlertRole::Cancel)
    ///     .button("OK", AlertRole::Confirm);
    /// assert_eq!(d.buttons.len(), 2);
    /// ```
    #[must_use]
    pub fn button(mut self, label: impl Into<String>, role: AlertRole) -> Self {
        if role == AlertRole::Confirm && !self.buttons.iter().any(|(_, r)| *r == AlertRole::Confirm)
        {
            self.highlighted = self.buttons.len();
        }
        self.buttons.push((label.into(), role));
        if self.highlighted >= self.buttons.len() {
            self.highlighted = self.buttons.len() - 1;
        }
        self
    }

    /// The index of the accent (primary) button — the first
    /// `Confirm`, or the last button when none is `Confirm`.
    fn confirm_index(&self) -> usize {
        self.buttons
            .iter()
            .position(|(_, r)| *r == AlertRole::Confirm)
            .unwrap_or_else(|| self.buttons.len().saturating_sub(1))
    }

    /// The arrow-key highlight index.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{AlertDialog, AlertRole};
    ///
    /// let d = AlertDialog::new()
    ///     .button("Cancel", AlertRole::Cancel)
    ///     .button("OK", AlertRole::Confirm);
    /// assert_eq!(d.highlighted(), 1);
    /// ```
    #[inline]
    pub fn highlighted(&self) -> usize {
        self.highlighted
    }

    /// Returns the result once, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AlertDialog;
    ///
    /// let mut d = AlertDialog::new();
    /// assert_eq!(d.take_result(), None);
    /// ```
    pub fn take_result(&mut self) -> Option<AlertResult> {
        self.result.take()
    }

    /// Wires a shared cell that receives the result — the overlay-host
    /// observation seam (mirrors `take_result`). When the card is
    /// dropped without a result the slot receives
    /// [`AlertResult::Dismissed`].
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::{Arc, Mutex};
    /// use martensite::widgets::AlertDialog;
    ///
    /// let sink = Arc::new(Mutex::new(None));
    /// let d = AlertDialog::new().result_sink(sink.clone());
    /// drop(d); // closed without a choice
    /// assert!(sink.lock().unwrap().is_some());
    /// ```
    #[must_use]
    pub fn result_sink(mut self, sink: Arc<Mutex<Option<AlertResult>>>) -> Self {
        self.result_sink = Some(sink);
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] for real glyph runs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Records a result — locally and into the observation sink.
    fn set_result(&mut self, result: AlertResult) {
        self.result = Some(result);
        if let Some(sink) = &self.result_sink {
            if let Ok(mut cell) = sink.lock() {
                *cell = Some(result);
            }
        }
    }
}

impl Default for AlertDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AlertDialog {
    fn drop(&mut self) {
        // Closed without a choice — scrim dismissal, layer Escape,
        // `clear`, or the host simply dropping the card. The terminal
        // result the sink reports is `Dismissed`.
        if self.result.is_none() {
            if let Some(sink) = &self.result_sink {
                if let Ok(mut cell) = sink.lock() {
                    if cell.is_none() {
                        *cell = Some(AlertResult::Dismissed);
                    }
                }
            }
        }
    }
}

impl Widget for AlertDialog {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let w = cx.pt(CARD_W).min(constraints.max_size.x.max(0.0));
        let pad = cx.pt(PAD);
        let title_h = cx.pt(TITLE_H);
        let message_h = if self.message.is_empty() {
            0.0
        } else {
            cx.pt(MESSAGE_H)
        };
        let button_h = if self.buttons.is_empty() {
            0.0
        } else {
            cx.pt(BUTTON_H) + pad
        };
        let h = title_h + pad + message_h + pad + button_h;
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
        for (label, _) in &self.buttons {
            // Width follows the label with a sensible minimum — alert
            // buttons carry short verbs.
            let w = (label.chars().count() as f32 * cx.pt(7.5) + cx.pt(28.0)).max(cx.pt(72.0));
            bx -= w;
            self.button_rects.push(Rect::new(bx, by, w, button_h));
            bx -= cx.pt(BUTTON_GAP);
        }
        let message_top = bounds.min_y() + title_h + pad * 0.75;
        self.message_rect = Rect::new(
            bounds.min_x() + pad,
            message_top,
            (bounds.width() - pad * 2.0).max(0.0),
            (by - pad - message_top).max(0.0),
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::AlertDialog);
        node.set_label(self.title.as_str());
        if !self.message.is_empty() {
            node.set_description(self.message.as_str());
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerReleased {
            position,
            button: PointerButton::Primary,
        } = cx.event
        {
            for (i, r) in self.button_rects.iter().enumerate() {
                if r.contains(*position) {
                    let result = match self.buttons[i].1 {
                        AlertRole::Confirm => AlertResult::Confirm,
                        AlertRole::Cancel => AlertResult::Cancel,
                        AlertRole::Other(tag) => AlertResult::Other(tag),
                    };
                    self.set_result(result);
                    return EventResponse::RequestRepaint;
                }
            }
        }
        if let WidgetEvent::KeyPressed { key, .. } = cx.event {
            match key.as_str() {
                "Enter" => {
                    if !self.buttons.is_empty() {
                        let i = self.highlighted.min(self.buttons.len() - 1);
                        let result = match self.buttons[i].1 {
                            AlertRole::Confirm => AlertResult::Confirm,
                            AlertRole::Cancel => AlertResult::Cancel,
                            AlertRole::Other(tag) => AlertResult::Other(tag),
                        };
                        self.set_result(result);
                    }
                    return EventResponse::Handled;
                }
                "Escape" => {
                    // Cancel when a Cancel-role button exists,
                    // Dismissed otherwise. (Hosted in the overlay the
                    // layer consumes Escape first — dismissal there
                    // reports Dismissed through the sink on Drop.)
                    let result = if self.buttons.iter().any(|(_, r)| *r == AlertRole::Cancel) {
                        AlertResult::Cancel
                    } else {
                        AlertResult::Dismissed
                    };
                    self.set_result(result);
                    return EventResponse::Handled;
                }
                "ArrowRight" | "ArrowDown" if !self.buttons.is_empty() => {
                    self.highlighted = (self.highlighted + 1).min(self.buttons.len() - 1);
                    return EventResponse::RequestRepaint;
                }
                "ArrowLeft" | "ArrowUp" if !self.buttons.is_empty() => {
                    self.highlighted = self.highlighted.saturating_sub(1);
                    return EventResponse::RequestRepaint;
                }
                _ => {}
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
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        // Subtle offset shadow beneath the elevated card.
        cx.list.push_blurred_rect(
            [b.min_x(), b.min_y() + cx.pt(3.0), b.width(), b.height()],
            cx.pt(12.0),
            [0.0, 0.0, 0.0, 0.22],
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
        let pad = cx.pt(PAD);
        // Severity icon — a painted disc + glyph (no image dep).
        let key = match self.severity {
            AlertSeverity::Info => TokenKey::InfoColor,
            AlertSeverity::Warning => TokenKey::WarningColor,
            AlertSeverity::Error => TokenKey::ErrorColor,
        };
        let accent = cx.color(key, self.severity.accent());
        let icon_side = cx.pt(ICON);
        let icon_rect = kurbo::Rect::new(
            f64::from(b.min_x() + pad),
            f64::from(b.min_y() + pad * 0.8),
            f64::from(b.min_x() + pad + icon_side),
            f64::from(b.min_y() + pad * 0.8 + icon_side),
        );
        cx.list.push_fill_shape(icon_rect, &Shape::ELLIPSE, accent);
        // Centre the glyph in the disc.
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            icon_rect,
            kurbo::Point::new(
                f64::from(b.min_x() + pad + icon_side * 0.34),
                f64::from(b.min_y() + pad * 0.8 + (icon_side - cx.pt(16.0)) / 2.0),
            ),
            self.severity.glyph(),
            cx.pt(16.0),
            [255, 255, 255, 255],
        );

        // Title — severity-tinted for Warning/Error, plain ink for
        // Info — clipped to the card interior right of the icon.
        let title_x = b.min_x() + pad + icon_side + cx.pt(10.0);
        let title_ink = match self.severity {
            AlertSeverity::Info => cx.color(TokenKey::TextColor, INK),
            _ => accent,
        };
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(title_x),
                f64::from(b.min_y()),
                f64::from(b.max_x() - pad),
                f64::from(b.min_y() + cx.pt(TITLE_H) + pad),
            ),
            kurbo::Point::new(f64::from(title_x), f64::from(b.min_y() + pad * 0.9)),
            &self.title,
            cx.pt(16.0),
            title_ink,
        );
        if !self.message.is_empty() {
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(self.message_rect.min_x()),
                    f64::from(self.message_rect.min_y()),
                    f64::from(self.message_rect.max_x()),
                    f64::from(self.message_rect.max_y()),
                ),
                kurbo::Point::new(
                    f64::from(self.message_rect.min_x()),
                    f64::from(self.message_rect.min_y()),
                ),
                &self.message,
                cx.pt(13.0),
                cx.color(TokenKey::TextMutedColor, MUTED),
            );
        }
        let confirm = self.confirm_index();
        for (i, ((label, role), r)) in self.buttons.iter().zip(&self.button_rects).enumerate() {
            let br = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            let primary = i == confirm;
            let destructive_primary = primary && self.destructive && *role == AlertRole::Confirm;
            let bshape = Shape::rounded(cx.dim(TokenKey::BorderRadius, 6.0));
            cx.list.push_fill_shape(
                br,
                &bshape,
                if destructive_primary {
                    cx.color(TokenKey::ErrorColor, ERROR)
                } else if primary {
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
            // Arrow-key traversal ring.
            if i == self.highlighted && !primary {
                cx.list.push_stroke_shape(
                    br,
                    &bshape,
                    cx.pt(2.0),
                    cx.color(TokenKey::AccentColor, ACCENT),
                );
            }
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(r.min_x() + cx.pt(8.0)),
                    f64::from(r.min_y()),
                    f64::from(r.max_x() - cx.pt(8.0)),
                    f64::from(r.max_y()),
                ),
                kurbo::Point::new(
                    f64::from(r.min_x() + cx.pt(12.0)),
                    f64::from(r.min_y() + (r.height() - cx.pt(13.0)) / 2.0),
                ),
                label,
                cx.pt(13.0),
                if primary {
                    cx.color(TokenKey::TextInverseColor, [255, 255, 255, 255])
                } else {
                    cx.color(TokenKey::TextColor, INK)
                },
            );
        }
    }

    fn child_count(&self) -> usize {
        0
    }
}

impl std::fmt::Debug for AlertDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlertDialog")
            .field("title", &self.title)
            .field("severity", &self.severity)
            .field("buttons", &self.buttons)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{EventContext, HotNode};

    fn alert() -> AlertDialog {
        AlertDialog::new()
            .title("Delete file?")
            .message("This cannot be undone.")
            .button("Cancel", AlertRole::Cancel)
            .button("Delete", AlertRole::Confirm)
    }

    fn laid_out(d: &mut AlertDialog) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        d.layout(&mut cx, Rect::new(100.0, 100.0, 380.0, 180.0));
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn event(d: &mut AlertDialog, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: d.cached_bounds,
            scale: 1.0,
        };
        d.event(&mut cx)
    }

    #[test]
    fn button_press_reports_role() {
        let mut d = alert();
        laid_out(&mut d);
        // Press the Cancel button (first).
        let r = d.button_rects[0];
        let release = WidgetEvent::PointerReleased {
            position: Vec2::new(r.min_x() + 4.0, r.min_y() + 4.0),
            button: PointerButton::Primary,
        };
        assert_eq!(event(&mut d, &release), EventResponse::RequestRepaint);
        assert_eq!(d.take_result(), Some(AlertResult::Cancel));
        assert_eq!(d.take_result(), None);
    }

    #[test]
    fn other_role_echoes_tag() {
        let mut d = AlertDialog::new()
            .button("Later", AlertRole::Other(7))
            .button("OK", AlertRole::Confirm);
        laid_out(&mut d);
        let r = d.button_rects[0];
        let release = WidgetEvent::PointerReleased {
            position: Vec2::new(r.min_x() + 4.0, r.min_y() + 4.0),
            button: PointerButton::Primary,
        };
        event(&mut d, &release);
        assert_eq!(d.take_result(), Some(AlertResult::Other(7)));
    }

    #[test]
    fn enter_activates_confirm() {
        let mut d = alert();
        laid_out(&mut d);
        assert_eq!(event(&mut d, &key("Enter")), EventResponse::Handled);
        assert_eq!(d.take_result(), Some(AlertResult::Confirm));
    }

    #[test]
    fn arrows_traverse_then_enter() {
        let mut d = alert();
        laid_out(&mut d);
        assert_eq!(d.highlighted(), 1); // confirm is the default
        event(&mut d, &key("ArrowLeft"));
        assert_eq!(d.highlighted(), 0);
        event(&mut d, &key("ArrowLeft")); // clamped
        assert_eq!(d.highlighted(), 0);
        event(&mut d, &key("ArrowRight"));
        assert_eq!(d.highlighted(), 1);
        // Traverse to Cancel and activate it with Enter.
        event(&mut d, &key("ArrowLeft"));
        event(&mut d, &key("Enter"));
        assert_eq!(d.take_result(), Some(AlertResult::Cancel));
    }

    #[test]
    fn escape_reports_cancel_then_dismissed() {
        let mut d = alert();
        laid_out(&mut d);
        event(&mut d, &key("Escape"));
        assert_eq!(d.take_result(), Some(AlertResult::Cancel));

        let mut no_cancel = AlertDialog::new().button("OK", AlertRole::Confirm);
        laid_out(&mut no_cancel);
        event(&mut no_cancel, &key("Escape"));
        assert_eq!(no_cancel.take_result(), Some(AlertResult::Dismissed));
    }

    #[test]
    fn drop_without_result_reports_dismissed() {
        let sink = Arc::new(Mutex::new(None));
        {
            let _d = AlertDialog::new().result_sink(Arc::clone(&sink));
        }
        assert_eq!(*sink.lock().unwrap(), Some(AlertResult::Dismissed));
    }

    #[test]
    fn drop_keeps_existing_result() {
        let sink = Arc::new(Mutex::new(None));
        {
            let mut d = alert().result_sink(Arc::clone(&sink));
            laid_out(&mut d);
            let r = d.button_rects[1];
            let release = WidgetEvent::PointerReleased {
                position: Vec2::new(r.min_x() + 4.0, r.min_y() + 4.0),
                button: PointerButton::Primary,
            };
            event(&mut d, &release);
        }
        // The Confirm result survives the drop — not overwritten by
        // Dismissed.
        assert_eq!(*sink.lock().unwrap(), Some(AlertResult::Confirm));
    }

    #[test]
    fn card_swallows_inside_presses() {
        let mut d = alert();
        let mut cx = EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(120.0, 120.0),
            },
            bounds: Rect::new(100.0, 100.0, 380.0, 180.0),
            scale: 1.0,
        };
        assert_eq!(d.event(&mut cx), EventResponse::Handled);
    }

    #[test]
    fn alert_accessibility() {
        let d = alert();
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        d.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::AlertDialog);
        assert_eq!(node.label(), Some("Delete file?"));
        assert_eq!(node.description(), Some("This cannot be undone."));
    }

    #[test]
    fn severity_defaults_to_info() {
        let d = AlertDialog::new();
        assert_eq!(d.severity, AlertSeverity::Info);
        assert!(!d.destructive);
    }
}
