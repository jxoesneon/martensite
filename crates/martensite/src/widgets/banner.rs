//! `Banner` widget: an inline severity strip (InfoBar / InlineNotification).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::banner::{Banner, Severity};
//!
//! let b = Banner::new(Severity::Warning, "Connection unstable");
//! assert!(!b.is_dismissed());
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

/// Severity tint of a [`Banner`].
///
/// # Examples
///
/// ```
/// use martensite::widgets::banner::Severity;
///
/// assert_ne!(Severity::Info, Severity::Error);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// Neutral informational strip.
    Info,
    /// Warning — attention needed, action still possible.
    Warning,
    /// Error — something failed.
    Error,
}

impl Severity {
    /// Tint color for the severity dot/accent edge.
    pub(crate) fn accent(self) -> [u8; 4] {
        match self {
            Self::Info => [60, 120, 220, 255],
            Self::Warning => [220, 160, 40, 255],
            Self::Error => [210, 60, 60, 255],
        }
    }
}

const INK: [u8; 4] = [20, 20, 25, 255];
/// Banner height in logical points.
const HEIGHT: f32 = 36.0;
/// Close button hit box (logical points square).
const CLOSE: f32 = 24.0;

/// An inline banner strip with a severity dot, a message, and an
/// optional close affordance. Poll [`Banner::take_dismissed`] after
/// event dispatch to learn when the user closed it.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Banner, Severity};
///
/// let b = Banner::new(Severity::Info, "Update available").dismissible(true);
/// assert!(!b.is_dismissed());
/// ```
#[derive(Clone)]
pub struct Banner {
    /// Severity tint.
    pub severity: Severity,
    /// The message text.
    pub message: String,
    /// Whether a close button is shown.
    pub dismissible: bool,
    /// Set when the user clicked close (take via `take_dismissed`).
    dismissed: bool,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Banner {
    /// A dismissible banner with the given severity and message.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Banner, Severity};
    ///
    /// let b = Banner::new(Severity::Error, "Export failed");
    /// assert_eq!(b.severity, Severity::Error);
    /// ```
    pub fn new(severity: Severity, message: impl Into<String>) -> Self {
        Self {
            severity,
            message: message.into(),
            dismissible: true,
            dismissed: false,
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets whether the close affordance is shown.
    #[must_use]
    pub fn dismissible(mut self, dismissible: bool) -> Self {
        self.dismissible = dismissible;
        self
    }

    /// Whether the user dismissed this banner.
    pub fn is_dismissed(&self) -> bool {
        self.dismissed
    }

    /// Clears and returns the dismissed flag.
    pub fn take_dismissed(&mut self) -> bool {
        std::mem::take(&mut self.dismissed)
    }

    /// Shares a [`crate::text_paint::TextPainter`] for real glyph runs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The close-button rect (device px) from the last layout.
    fn close_rect(&self, scale: f32) -> Rect {
        let side = CLOSE * scale;
        Rect::new(
            self.cached_bounds.max_x() - side - 8.0 * scale,
            self.cached_bounds.origin.y + (self.cached_bounds.size.y - side) / 2.0,
            side,
            side,
        )
    }
}

impl Widget for Banner {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(0.0),
            cx.pt(HEIGHT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        // Status strips are live regions in platform APIs.
        node.set_role(accesskit::Role::Status);
        node.set_label(self.message.as_str());
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.dismissible {
            return EventResponse::Ignored;
        }
        if let WidgetEvent::PointerReleased {
            position,
            button: PointerButton::Primary,
        } = cx.event
        {
            if self.close_rect(cx.scale).contains(*position) {
                self.dismissed = true;
                return EventResponse::RequestRepaint;
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.origin.x),
            f64::from(b.origin.y),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let shape = Shape::rounded(cx.dim(TokenKey::BorderRadius, 6.0));
        cx.list.push_fill_shape(
            rect,
            &shape,
            cx.color(TokenKey::SurfaceColor, [70, 74, 82, 255]),
        );
        // Severity accent bar on the leading edge.
        let accent = self.severity.accent();
        let bar_w = cx.pt(3.0);
        cx.list.push_clip_shape(rect, &shape);
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.origin.x),
                f64::from(b.origin.y),
                f64::from(b.origin.x + bar_w),
                f64::from(b.max_y()),
            ),
            accent,
        );
        cx.list.pop_clip();

        let dot_d = cx.pt(8.0);
        let dot_y = b.origin.y + (b.size.y - dot_d) / 2.0;
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(b.origin.x + cx.pt(12.0)),
                f64::from(dot_y),
                f64::from(b.origin.x + cx.pt(12.0) + dot_d),
                f64::from(dot_y + dot_d),
            ),
            &Shape::ELLIPSE,
            accent,
        );

        crate::text_paint::paint_label(
            crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
            cx.list,
            kurbo::Point::new(
                f64::from(b.origin.x + cx.pt(28.0)),
                f64::from(b.origin.y + (b.size.y - cx.pt(14.0)) / 2.0),
            ),
            &self.message,
            cx.pt(14.0),
            cx.color(TokenKey::TextColor, INK),
        );

        if self.dismissible {
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
    }
}

impl std::fmt::Debug for Banner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Banner")
            .field("severity", &self.severity)
            .field("message", &self.message)
            .field("dismissed", &self.dismissed)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{EventContext, HotNode};

    #[test]
    fn banner_dismiss() {
        let mut hot = HotNode::default();
        let mut b = Banner::new(Severity::Warning, "careful");
        let mut lcx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        b.layout(&mut lcx, Rect::new(0.0, 0.0, 300.0, 36.0));
        let cr = b.close_rect(1.0);
        let mut ecx = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: Vec2::new(cr.origin.x + 2.0, cr.origin.y + 2.0),
                button: PointerButton::Primary,
            },
            bounds: Rect::new(0.0, 0.0, 300.0, 36.0),
            scale: 1.0,
        };
        assert_eq!(b.event(&mut ecx), EventResponse::RequestRepaint);
        assert!(b.take_dismissed());
        assert!(!b.take_dismissed());
    }

    #[test]
    fn banner_non_dismissible_ignores() {
        let mut b = Banner::new(Severity::Info, "sticky").dismissible(false);
        let mut ecx = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: Vec2::new(1.0, 1.0),
                button: PointerButton::Primary,
            },
            bounds: Rect::new(0.0, 0.0, 300.0, 36.0),
            scale: 1.0,
        };
        assert_eq!(b.event(&mut ecx), EventResponse::Ignored);
    }
}
