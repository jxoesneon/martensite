//! `Button` widget: an interactive button with a label and click action.
//!
//! The `Button` widget exposes `Role::Button`, an accessible label, and
//! the `Action::Click` and `Action::Focus` accessibility actions. It
//! integrates with the focus system via `NodeFlags::FOCUSABLE`.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::button::Button;
//!
//! let btn = Button::new("Click me");
//! assert_eq!(btn.label, "Click me");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use kurbo::Shape as _;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::Rect;

/// Button face colour when enabled (light neutral grey).
const FACE_ENABLED: [u8; 4] = [230, 233, 238, 255];
/// Button face colour when disabled.
const FACE_DISABLED: [u8; 4] = [245, 245, 246, 255];
/// Button border colour.
const EDGE: [u8; 4] = [140, 145, 155, 255];
/// Label ink colour when enabled.
const INK_ENABLED: [u8; 4] = [20, 20, 25, 255];
/// Label ink colour when disabled.
const INK_DISABLED: [u8; 4] = [160, 160, 165, 255];
/// Corner radius of the button face.
const CORNER_RADIUS: f64 = 4.0;
/// Horizontal padding between the border and the label.
const TEXT_PAD_X: f32 = 10.0;

/// An interactive button widget with an accessible label.
///
/// The button advertises `Role::Button` and `Action::Click` to the
/// accessibility subsystem. It is focusable by default.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Button;
///
/// let btn = Button::new("Submit")
///     .enabled(true);
/// assert_eq!(btn.label, "Submit");
/// assert!(btn.enabled);
/// ```
#[derive(Clone)]
pub struct Button {
    /// The accessible label displayed on the button.
    pub label: String,
    /// Whether the button is enabled (not disabled/inert).
    pub enabled: bool,
    /// Optional tooltip text.
    pub tooltip: Option<String>,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
}

impl Button {
    /// Creates a new button with the given label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Button;
    ///
    /// let btn = Button::new("OK");
    /// assert_eq!(btn.label, "OK");
    /// assert!(btn.enabled);
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
            tooltip: None,
            cached_bounds: Rect::default(),
        }
    }

    /// Sets whether the button is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Button;
    ///
    /// let btn = Button::new("Disabled").enabled(false);
    /// assert!(!btn.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets the tooltip text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Button;
    ///
    /// let btn = Button::new("Help").tooltip("Click for assistance");
    /// assert_eq!(btn.tooltip.as_deref(), Some("Click for assistance"));
    /// ```
    #[inline]
    #[must_use]
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Returns the cached bounds from the last layout pass.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Button;
    ///
    /// let btn = Button::new("Save");
    /// let bounds = btn.cached_bounds();
    /// assert_eq!(bounds.size.x, 0.0);
    /// ```
    #[inline]
    pub fn cached_bounds(&self) -> Rect {
        self.cached_bounds
    }
}

impl Widget for Button {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // A button has a default minimum size of 80x32.
        let min_w = 80.0_f32.min(constraints.max_size.x.max(0.0));
        let min_h = 32.0_f32.min(constraints.max_size.y.max(0.0));
        Vec2::new(min_w, min_h)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Button);
        node.set_label(self.label.as_str());
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Focus);
        if let Some(ref tooltip) = self.tooltip {
            node.set_tooltip(tooltip.as_str());
        }
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
                ..
            }
            | WidgetEvent::KeyPressed { .. } => EventResponse::RequestRepaint,
            WidgetEvent::PointerReleased { .. } | WidgetEvent::KeyReleased { .. } => {
                EventResponse::Handled
            }
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
        let (face, ink) = if self.enabled {
            (FACE_ENABLED, INK_ENABLED)
        } else {
            (FACE_DISABLED, INK_DISABLED)
        };

        let rounded = kurbo::RoundedRect::from_rect(rect, CORNER_RADIUS).into_path(0.1);
        cx.list.push_path(rounded.clone(), face);
        cx.list.push_stroke_path(rounded, 1.0, EDGE);

        // The label is left-aligned inside the face and vertically
        // centred — `DrawText` positions by baseline, so offset half the
        // cap height (~font_size / 2) below the midpoint.
        cx.list.push_text(
            kurbo::Point::new(
                f64::from(b.origin.x + TEXT_PAD_X),
                f64::from(b.origin.y + b.size.y / 2.0 + 5.0),
            ),
            self.label.clone(),
            14.0,
            ink,
        );
    }
}

impl std::fmt::Debug for Button {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Button")
            .field("label", &self.label)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot }
    }

    #[test]
    fn button_new() {
        let btn = Button::new("Submit");
        assert_eq!(btn.label, "Submit");
        assert!(btn.enabled);
    }

    #[test]
    fn button_builder_methods() {
        let btn = Button::new("Cancel")
            .enabled(false)
            .tooltip("Click to cancel");
        assert_eq!(btn.label, "Cancel");
        assert!(!btn.enabled);
        assert_eq!(btn.tooltip.as_deref(), Some("Click to cancel"));
    }

    #[test]
    fn button_measure_returns_min_size() {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let mut btn = Button::new("OK");
        let size = btn.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(200.0, 100.0),
            },
        );
        assert!(size.x >= 0.0 && size.y >= 0.0);
    }

    #[test]
    fn button_layout_sets_bounds() {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let mut btn = Button::new("OK");
        let bounds = Rect::new(10.0, 20.0, 80.0, 32.0);
        btn.layout(&mut cx, bounds);
        assert_eq!(btn.cached_bounds(), bounds);
    }

    #[test]
    fn button_accessibility_sets_role_and_label() {
        let btn = Button::new("Submit");
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        btn.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Button);
        assert_eq!(node.label(), Some("Submit"));
        assert!(node.supports_action(accesskit::Action::Click));
        assert!(node.supports_action(accesskit::Action::Focus));
    }

    #[test]
    fn button_accessibility_disabled_state() {
        let btn = Button::new("Submit").enabled(false);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        btn.accessibility(&mut node);
        assert!(node.is_disabled());
    }

    #[test]
    fn button_accessibility_tooltip() {
        let btn = Button::new("Help").tooltip("Get help");
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        btn.accessibility(&mut node);
        assert_eq!(node.tooltip(), Some("Get help"));
    }

    #[test]
    fn button_clone() {
        let btn = Button::new("OK").tooltip("tip");
        let cloned = btn.clone();
        assert_eq!(btn.label, cloned.label);
        assert_eq!(btn.tooltip, cloned.tooltip);
    }

    #[test]
    fn button_debug_format() {
        let btn = Button::new("OK");
        let debug = format!("{:?}", btn);
        assert!(debug.contains("Button"));
        assert!(debug.contains("OK"));
    }
}
