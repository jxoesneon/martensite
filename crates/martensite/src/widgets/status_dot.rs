//! `StatusDot` — a severity status lamp (industrial HMI lamp, Ant
//! `Badge.Status`, GTK status icon).
//!
//! A filled severity-colored circle with an optional text label —
//! the at-a-glance "pump online" / "link degraded" indicator rows of
//! an operations dashboard. Display-only; the app flips the status
//! through [`StatusDot::set_status`]. A `pulse` flag paints a halo
//! ring for active-but-attention states.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::status_dot::{Status, StatusDot};
//!
//! let d = StatusDot::new("Pump A").status(Status::Ok);
//! assert_eq!(d.status_value(), Status::Ok);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

const DOT_PT: f32 = 10.0;
const GAP_PT: f32 = 6.0;
const FONT_PT: f32 = 12.0;
const HEIGHT_PT: f32 = 18.0;

const FG: [u8; 4] = [30, 30, 36, 255];
const MUTED: [u8; 4] = [110, 110, 118, 255];
const OK: [u8; 4] = [46, 160, 90, 255];
const WARN: [u8; 4] = [220, 160, 40, 255];
const ERR: [u8; 4] = [210, 60, 60, 255];
const OFF: [u8; 4] = [160, 160, 166, 255];
const INFO: [u8; 4] = [60, 120, 220, 255];

/// Status severity for the lamp.
///
/// ```
/// use martensite::widgets::status_dot::Status;
///
/// assert_ne!(Status::Ok, Status::Error);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Neutral/no signal — gray.
    Off,
    /// Informational — blue.
    Info,
    /// Nominal — green.
    Ok,
    /// Degraded — amber.
    Warning,
    /// Fault — red.
    Error,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Info => "info",
            Self::Ok => "ok",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// A status lamp — see the module docs.
///
/// ```
/// use martensite::widgets::status_dot::StatusDot;
///
/// let d = StatusDot::new("Link");
/// assert_eq!(d.text(), "Link");
/// ```
pub struct StatusDot {
    /// Optional label text.
    pub text: String,
    /// When `false` the lamp renders muted.
    pub enabled: bool,
    /// Halo ring for active-attention states.
    pub pulse: bool,
    status: Status,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl StatusDot {
    /// Creates a lamp labeled `text` in the `Off` state.
    ///
    /// ```
    /// use martensite::widgets::status_dot::{Status, StatusDot};
    ///
    /// let d = StatusDot::new("Pump");
    /// assert_eq!(d.status_value(), Status::Off);
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            enabled: true,
            pulse: false,
            status: Status::Off,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Sets the status.
    ///
    /// ```
    /// use martensite::widgets::status_dot::{Status, StatusDot};
    ///
    /// let d = StatusDot::new("L").status(Status::Ok);
    /// assert_eq!(d.status_value(), Status::Ok);
    /// ```
    pub fn status(mut self, status: Status) -> Self {
        self.status = status;
        self
    }

    /// Paints a halo ring (active-attention states).
    ///
    /// ```
    /// use martensite::widgets::status_dot::StatusDot;
    ///
    /// let d = StatusDot::new("L").pulse(true);
    /// assert!(d.pulse);
    /// ```
    pub fn pulse(mut self, pulse: bool) -> Self {
        self.pulse = pulse;
        self
    }

    /// Enables or disables (mutes) the lamp.
    ///
    /// ```
    /// use martensite::widgets::status_dot::StatusDot;
    ///
    /// let d = StatusDot::new("L").enabled(false);
    /// assert!(!d.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::status_dot::StatusDot;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let d = StatusDot::new("L").with_text_painter(shared_painter());
    /// assert_eq!(d.text(), "L");
    /// ```
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Label text.
    ///
    /// ```
    /// use martensite::widgets::status_dot::StatusDot;
    ///
    /// assert_eq!(StatusDot::new("X").text(), "X");
    /// ```
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Current status.
    ///
    /// ```
    /// use martensite::widgets::status_dot::{Status, StatusDot};
    ///
    /// assert_eq!(StatusDot::new("X").status_value(), Status::Off);
    /// ```
    pub fn status_value(&self) -> Status {
        self.status
    }

    /// Updates the status (post-construction flips).
    ///
    /// ```
    /// use martensite::widgets::status_dot::{Status, StatusDot};
    ///
    /// let mut d = StatusDot::new("X");
    /// d.set_status(Status::Error);
    /// assert_eq!(d.status_value(), Status::Error);
    /// ```
    pub fn set_status(&mut self, status: Status) {
        self.status = status;
    }

    /// Base color for the status.
    fn base_color(&self) -> [u8; 4] {
        match self.status {
            Status::Off => OFF,
            Status::Info => INFO,
            Status::Ok => OK,
            Status::Warning => WARN,
            Status::Error => ERR,
        }
    }
}

impl Widget for StatusDot {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let text_w = if self.text.is_empty() {
            0.0
        } else {
            self.text.chars().count() as f32 * FONT_PT * 0.55 * cx.scale + cx.pt(GAP_PT)
        };
        Vec2::new(
            (cx.pt(DOT_PT) + text_w).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(10.0, 10.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Status);
        node.set_label(if self.text.is_empty() {
            "Status".to_string()
        } else {
            self.text.clone()
        });
        node.set_value(self.status.label());
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let color = cx.color(
            match self.status {
                Status::Off => TokenKey::TextMutedColor,
                Status::Info => TokenKey::AccentColor,
                Status::Ok => TokenKey::SuccessColor,
                Status::Warning => TokenKey::WarningColor,
                Status::Error => TokenKey::ErrorColor,
            },
            self.base_color(),
        );
        let color = if self.enabled {
            color
        } else {
            cx.color(TokenKey::TextMutedColor, MUTED)
        };
        let d = cx.pt(DOT_PT);
        let cy = self.bounds.min_y() + self.bounds.height() / 2.0;
        let cxdot = self.bounds.min_x() + d / 2.0;
        let center = Vec2::new(cxdot, cy);
        let r = d / 2.0;
        let dot_rect = kurbo::Rect::new(
            f64::from(cxdot - r),
            f64::from(cy - r),
            f64::from(cxdot + r),
            f64::from(cy + r),
        );
        if self.pulse {
            let halo_r = r * 1.8;
            let halo = [color[0], color[1], color[2], 70];
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(cxdot - halo_r),
                    f64::from(cy - halo_r),
                    f64::from(cxdot + halo_r),
                    f64::from(cy + halo_r),
                ),
                &martensite_core::shape::Shape::circle(center, halo_r),
                halo,
            );
        }
        cx.list.push_fill_shape(
            dot_rect,
            &martensite_core::shape::Shape::circle(center, r),
            color,
        );

        if !self.text.is_empty() {
            let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
            let size = FONT_PT * cx.scale;
            let x = self.bounds.min_x() + d + cx.pt(GAP_PT);
            let clip = Rect::new(
                x,
                self.bounds.min_y(),
                (self.bounds.max_x() - x).max(0.0),
                self.bounds.height(),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(clip.min_x()),
                    f64::from(clip.min_y()),
                    f64::from(clip.max_x()),
                    f64::from(clip.max_y()),
                ),
                kurbo::Point::new(f64::from(x), f64::from(cy - size / 2.0)),
                &self.text,
                size,
                cx.color(TokenKey::TextColor, FG),
            );
        }
    }
}

impl std::fmt::Debug for StatusDot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusDot")
            .field("text", &self.text)
            .field("status", &self.status)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn status_roundtrip() {
        let mut d = StatusDot::new("A").status(Status::Warning);
        assert_eq!(d.status_value(), Status::Warning);
        d.set_status(Status::Ok);
        assert_eq!(d.status_value(), Status::Ok);
    }

    #[test]
    fn a11y_value_is_status() {
        let d = StatusDot::new("Pump").status(Status::Error);
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        d.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Status);
        assert_eq!(node.value(), Some("error"));
        assert_eq!(node.label(), Some("Pump"));
    }

    #[test]
    fn empty_label_measures_dot_only() {
        let mut with = StatusDot::new("Long Label Text");
        let mut bare = StatusDot::new("");
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let cons = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(500.0, 50.0),
        };
        let w1 = with.measure(&mut cx, cons).x;
        let w2 = bare.measure(&mut cx, cons).x;
        assert!(w1 > w2);
        assert!(w2 <= 20.0);
    }

    #[test]
    fn colors_map() {
        assert_eq!(StatusDot::new("x").status(Status::Ok).base_color(), OK);
        assert_eq!(StatusDot::new("x").status(Status::Off).base_color(), OFF);
    }
}
