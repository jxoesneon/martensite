//! `Switch` widget: an iOS-style pill toggle.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::switch::Switch;
//!
//! let sw = Switch::new("Enabled").on(true);
//! assert!(sw.on);
//! ```

use accesskit::{Node as AccessKitNode, Toggled};
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

const EDGE: [u8; 4] = [110, 115, 125, 255];
const ACCENT: [u8; 4] = [40, 110, 220, 255];
const KNOB: [u8; 4] = [255, 255, 255, 255];
const INK: [u8; 4] = [20, 20, 25, 255];
/// Track size in logical points (roughly the iOS/macOS proportion).
const TRACK_W: f32 = 34.0;
const TRACK_H: f32 = 20.0;
const LABEL_GAP: f32 = 8.0;

/// A pill-shaped toggle switch with an accessible label.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Switch;
///
/// let mut sw = Switch::new("Live updates");
/// sw.toggle();
/// assert!(sw.on);
/// ```
#[derive(Clone)]
pub struct Switch {
    /// Accessible label shown beside the track.
    pub label: String,
    /// Whether the switch is on.
    pub on: bool,
    /// Whether the switch is enabled.
    pub enabled: bool,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter (see [`crate::text_paint`]).
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Switch {
    /// Creates a switch in the off state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Switch;
    ///
    /// let sw = Switch::new("Notifications");
    /// assert!(!sw.on);
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            on: false,
            enabled: true,
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the on state.
    #[inline]
    #[must_use]
    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    /// Sets whether the switch is enabled.
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Flips the switch.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Switch;
    ///
    /// let mut sw = Switch::new("Dark mode");
    /// sw.toggle();
    /// assert!(sw.on);
    /// ```
    #[inline]
    pub fn toggle(&mut self) {
        self.on = !self.on;
    }

    /// Shares a [`crate::text_paint::TextPainter`] for real glyph runs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }
}

impl Widget for Switch {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let w = cx.pt(TRACK_W + LABEL_GAP + 8.0 * self.label.chars().count() as f32);
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            cx.pt(TRACK_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Switch);
        node.set_label(self.label.as_str());
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Focus);
        node.set_toggled(if self.on {
            Toggled::True
        } else {
            Toggled::False
        });
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        let toggle = matches!(
            cx.event,
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } | WidgetEvent::KeyPressed { .. }
        );
        if toggle {
            self.on = !self.on;
            EventResponse::RequestRepaint
        } else {
            EventResponse::Ignored
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let tw = cx.pt(TRACK_W);
        let th = cx.pt(TRACK_H);
        let ty = b.origin.y + (b.size.y - th) / 2.0;
        let track = kurbo::Rect::new(
            f64::from(b.origin.x),
            f64::from(ty),
            f64::from(b.origin.x + tw),
            f64::from(ty + th),
        );
        let pill = Shape::PILL;
        let track_fill = if self.on {
            cx.color(TokenKey::AccentColor, ACCENT)
        } else {
            cx.color(TokenKey::SurfaceColor, [70, 74, 82, 255])
        };
        cx.list.push_fill_shape(track, &pill, track_fill);
        cx.list.push_stroke_shape(
            track,
            &pill,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, EDGE),
        );

        // Knob: circle inset by 2px, parked right when on.
        let inset = cx.pt(3.0);
        let d = th - inset * 2.0;
        let kx = if self.on {
            b.origin.x + tw - inset - d
        } else {
            b.origin.x + inset
        };
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(kx),
                f64::from(ty + inset),
                f64::from(kx + d),
                f64::from(ty + inset + d),
            ),
            &Shape::ELLIPSE,
            KNOB,
        );

        crate::text_paint::paint_label(
            crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
            cx.list,
            kurbo::Point::new(
                f64::from(b.origin.x + tw + cx.pt(LABEL_GAP)),
                f64::from(b.origin.y + (b.size.y - cx.pt(14.0)) / 2.0),
            ),
            &self.label,
            cx.pt(14.0),
            cx.color(TokenKey::TextColor, INK),
        );
    }
}

impl std::fmt::Debug for Switch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Switch")
            .field("label", &self.label)
            .field("on", &self.on)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn switch_toggle() {
        let mut sw = Switch::new("Live");
        sw.toggle();
        assert!(sw.on);
    }

    #[test]
    fn switch_a11y() {
        let sw = Switch::new("Live").on(true);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        sw.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Switch);
        assert_eq!(node.toggled(), Some(Toggled::True));
    }

    #[test]
    fn switch_disabled_not_focusable() {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let mut sw = Switch::new("Live").enabled(false);
        sw.layout(&mut cx, Rect::new(0.0, 0.0, 60.0, 24.0));
        assert!(!hot.flags.contains(NodeFlags::FOCUSABLE));
    }
}
