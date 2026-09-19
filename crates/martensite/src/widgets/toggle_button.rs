//! `ToggleButton` widget: a button with a persistent pressed state.
//!
//! The `ToggleButton` is the checkable-button control — Qt's checkable
//! `QPushButton`, Radix `Toggle`, an `aria-pressed` button: it looks
//! like a [`Button`](crate::widgets::Button) but latches into a
//! pressed appearance on activation.
//!
//! - The widget emits `Role::Button` with the `Toggled` state — the
//!   `aria-pressed` mapping (AccessKit has no distinct
//!   `ToggleButton` role).
//! - Pointer interaction is armed-state: a press arms the button, the
//!   release inside fires the toggle (a press dragged off the face
//!   cancels), matching `Button`'s press contract.
//! - `Space`/`Enter` and `SemanticAction::Click` toggle it.
//! - Toggle changes are reported outward through
//!   [`take_toggled`](ToggleButton::take_toggled), the `take_*`
//!   signal seam used by `Dialog::take_response` and friends.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::ToggleButton;
//!
//! let btn = ToggleButton::new("Bold").pressed(true);
//! assert!(btn.pressed);
//! ```

use accesskit::{Node as AccessKitNode, Toggled};
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

/// Button face colour when enabled and unpressed (light neutral grey).
const FACE_ENABLED: [u8; 4] = [230, 233, 238, 255];
/// Button face colour when disabled.
const FACE_DISABLED: [u8; 4] = [245, 245, 246, 255];
/// Button border colour.
const EDGE: [u8; 4] = [140, 145, 155, 255];
/// Pressed-state accent.
const ACCENT: [u8; 4] = [40, 110, 220, 255];
/// Label ink colour when enabled.
const INK_ENABLED: [u8; 4] = [20, 20, 25, 255];
/// Label ink colour when disabled.
const INK_DISABLED: [u8; 4] = [160, 160, 165, 255];
/// Label ink on the pressed face.
const INK_PRESSED: [u8; 4] = [255, 255, 255, 255];
/// Corner radius of the button face.
const CORNER_RADIUS: f32 = 4.0;
/// Horizontal padding between the border and the label.
const TEXT_PAD_X: f32 = 10.0;
/// Focus ring colour (translucent accent wash).
const FOCUS_RING: [u8; 4] = [60, 110, 220, 128];

/// A button widget with a persistent pressed (latched) state.
///
/// The button advertises `Role::Button` plus the `Toggled` state to
/// the accessibility subsystem — the `aria-pressed` contract. It is
/// focusable by default.
///
/// # Examples
///
/// ```
/// use martensite::widgets::ToggleButton;
///
/// let btn = ToggleButton::new("Italic")
///     .pressed(true)
///     .enabled(true);
/// assert_eq!(btn.label, "Italic");
/// assert!(btn.pressed);
/// ```
#[derive(Clone)]
pub struct ToggleButton {
    /// The accessible label displayed on the button.
    pub label: String,
    /// Whether the button is currently pressed (latched on).
    pub pressed: bool,
    /// Whether the button is enabled (not disabled/inert).
    pub enabled: bool,
    /// Optional tooltip text.
    pub tooltip: Option<String>,
    /// Whether the button is currently armed — a primary press is
    /// down and the release inside will fire the toggle.
    armed: bool,
    /// Whether the pointer is currently over the face while armed —
    /// drives the pressed-in hover shading.
    armed_hover: bool,
    /// Whether the button holds keyboard focus (focus ring).
    has_focus: bool,
    /// The last user-initiated toggle not yet drained by
    /// [`take_toggled`](Self::take_toggled).
    changed: Option<bool>,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter — when set, `paint` emits real
    /// `GlyphRun`s; without it the label falls back to `DrawText`
    /// placeholder boxes. See [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl ToggleButton {
    /// Creates a new toggle button with the given label, unpressed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ToggleButton;
    ///
    /// let btn = ToggleButton::new("Bold");
    /// assert_eq!(btn.label, "Bold");
    /// assert!(!btn.pressed);
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            pressed: false,
            enabled: true,
            tooltip: None,
            armed: false,
            armed_hover: false,
            has_focus: false,
            changed: None,
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the pressed state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ToggleButton;
    ///
    /// let btn = ToggleButton::new("Underline").pressed(true);
    /// assert!(btn.pressed);
    /// ```
    #[inline]
    #[must_use]
    pub fn pressed(mut self, pressed: bool) -> Self {
        self.pressed = pressed;
        self
    }

    /// Sets the pressed state (mutating form of
    /// [`pressed`](Self::pressed)).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ToggleButton;
    ///
    /// let mut btn = ToggleButton::new("Mute");
    /// btn.set_pressed(true);
    /// assert!(btn.pressed);
    /// ```
    #[inline]
    pub fn set_pressed(&mut self, pressed: bool) {
        self.pressed = pressed;
    }

    /// Sets whether the button is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ToggleButton;
    ///
    /// let btn = ToggleButton::new("Disabled").enabled(false);
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
    /// use martensite::widgets::ToggleButton;
    ///
    /// let btn = ToggleButton::new("Help").tooltip("Toggle help mode");
    /// assert_eq!(btn.tooltip.as_deref(), Some("Toggle help mode"));
    /// ```
    #[inline]
    #[must_use]
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Flips the pressed state and parks it for
    /// [`take_toggled`](Self::take_toggled).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ToggleButton;
    ///
    /// let mut btn = ToggleButton::new("Wrap text");
    /// btn.toggle();
    /// assert!(btn.pressed);
    /// assert_eq!(btn.take_toggled(), Some(true));
    /// ```
    #[inline]
    pub fn toggle(&mut self) {
        self.pressed = !self.pressed;
        self.changed = Some(self.pressed);
    }

    /// Returns the pressed state once after each user-initiated
    /// toggle — the widget's signal-out seam. Apps poll it per frame
    /// (or after dispatching input) to react without a callback.
    /// Programmatic writes via [`set_pressed`](Self::set_pressed) and
    /// the `pressed` field are not reported; [`toggle`](Self::toggle)
    /// is.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ToggleButton;
    ///
    /// let mut btn = ToggleButton::new("Pin");
    /// btn.toggle();
    /// assert_eq!(btn.take_toggled(), Some(true));
    /// assert_eq!(btn.take_toggled(), None); // drained
    /// ```
    #[inline]
    pub fn take_toggled(&mut self) -> Option<bool> {
        self.changed.take()
    }

    /// Returns the cached bounds from the last layout pass.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ToggleButton;
    ///
    /// let btn = ToggleButton::new("Save");
    /// let bounds = btn.cached_bounds();
    /// assert_eq!(bounds.size.x, 0.0);
    /// ```
    #[inline]
    pub fn cached_bounds(&self) -> Rect {
        self.cached_bounds
    }

    /// Shares a [`crate::text_paint::TextPainter`] so `paint` emits real
    /// glyph runs instead of `DrawText` placeholder boxes.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Fires a user-initiated toggle (input and semantic paths share
    /// this so the `take_toggled` signal stays consistent).
    fn activate(&mut self) {
        self.toggle();
    }
}

impl Widget for ToggleButton {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Same default minimum as `Button`: 80x32 logical pt.
        let min_w = cx.pt(80.0).min(constraints.max_size.x.max(0.0));
        let min_h = cx.pt(32.0).min(constraints.max_size.y.max(0.0));
        Vec2::new(min_w, min_h)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        // Declare keyboard focusability on the arena node — the
        // `FocusManager` rejects focus requests for nodes without it.
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        // `aria-pressed`: a toggle button is a `button` role carrying
        // the pressed state — AccessKit models that as `Toggled` on
        // `Role::Button` (there is no distinct ToggleButton role).
        node.set_role(accesskit::Role::Button);
        node.set_label(self.label.as_str());
        node.set_toggled(Toggled::from(self.pressed));
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
                position,
                button: PointerButton::Primary,
                ..
            } => {
                // Arm the button and capture the pointer so a release
                // outside can still cancel — press-to-focus is
                // automatic for FOCUSABLE nodes, and the capture
                // guarantees the paired release reaches us.
                self.armed = true;
                self.armed_hover = cx.bounds.contains(*position);
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerMoved { position } => {
                // While armed, the face shades only under the pointer —
                // dragging off deflates it back to the unpressed look.
                if self.armed {
                    let hover = cx.bounds.contains(*position);
                    if hover != self.armed_hover {
                        self.armed_hover = hover;
                        EventResponse::RequestRepaint
                    } else {
                        EventResponse::Handled
                    }
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerReleased {
                position,
                button: PointerButton::Primary,
            } => {
                if !self.armed {
                    return EventResponse::Ignored;
                }
                self.armed = false;
                self.armed_hover = false;
                if cx.bounds.contains(*position) {
                    // Release inside fires; release outside cancels.
                    self.activate();
                }
                EventResponse::ReleasePointer
            }
            WidgetEvent::KeyPressed { key, repeat } => {
                if *repeat {
                    // Held Space/Enter must not machine-gun the toggle.
                    return EventResponse::Handled;
                }
                if key == " " || key == "Space" || key == "Enter" {
                    self.activate();
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::FocusGained => {
                self.has_focus = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.has_focus = false;
                self.armed = false;
                self.armed_hover = false;
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                self.activate();
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => EventResponse::CaptureFocus,
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
        let accent = cx.color(TokenKey::AccentColor, ACCENT);
        let (face, ink, edge) = if !self.enabled {
            (
                cx.color(TokenKey::SurfaceColor, FACE_DISABLED),
                cx.color(TokenKey::TextMutedColor, INK_DISABLED),
                cx.color(TokenKey::BorderColor, EDGE),
            )
        } else if self.pressed || (self.armed && self.armed_hover) {
            // The latched (and pressed-in-armed) face is the accent —
            // persistent, unlike Button's momentary shading.
            (
                accent,
                cx.color(TokenKey::TextInverseColor, INK_PRESSED),
                accent,
            )
        } else {
            (
                cx.color(TokenKey::SurfaceColor, FACE_ENABLED),
                cx.color(TokenKey::TextColor, INK_ENABLED),
                cx.color(TokenKey::BorderColor, EDGE),
            )
        };

        let shape = Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, CORNER_RADIUS));
        cx.list.push_fill_shape(rect, &shape, face);
        cx.list.push_stroke_shape(rect, &shape, cx.pt(1.0), edge);

        if self.has_focus {
            // Translucent accent wash around the face.
            let wash = [accent[0], accent[1], accent[2], FOCUS_RING[3]];
            cx.list.push_stroke_shape(rect, &shape, cx.pt(2.0), wash);
        }

        // Centre the label horizontally in the face — toggle buttons
        // read as chips, not as left-aligned action buttons. Clipped
        // to the face interior so a long label can't spill past the
        // rounded edge.
        let font_px = cx.pt(14.0);
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let text_w = painter
            .and_then(|p| p.measure_text(&self.label, font_px))
            .unwrap_or_else(|| 8.0 * self.label.chars().count() as f32 * cx.scale);
        let pad = cx.pt(TEXT_PAD_X);
        let text_x = (b.min_x() + (b.width() - text_w) / 2.0).max(b.min_x() + pad);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(b.min_x() + pad),
                f64::from(b.min_y()),
                f64::from(b.max_x() - pad),
                f64::from(b.max_y()),
            ),
            kurbo::Point::new(
                f64::from(text_x),
                f64::from(b.min_y() + (b.height() - font_px) / 2.0),
            ),
            &self.label,
            font_px,
            ink,
        );
    }
}

impl std::fmt::Debug for ToggleButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToggleButton")
            .field("label", &self.label)
            .field("pressed", &self.pressed)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    fn event(btn: &mut ToggleButton, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: btn.cached_bounds,
            scale: 1.0,
        };
        btn.event(&mut cx)
    }

    fn laid_out(btn: &mut ToggleButton, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        btn.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn toggle_button_new() {
        let btn = ToggleButton::new("Bold");
        assert_eq!(btn.label, "Bold");
        assert!(!btn.pressed);
        assert!(btn.enabled);
    }

    #[test]
    fn toggle_button_builder_methods() {
        let btn = ToggleButton::new("Italic")
            .pressed(true)
            .enabled(false)
            .tooltip("tip");
        assert!(btn.pressed);
        assert!(!btn.enabled);
        assert_eq!(btn.tooltip.as_deref(), Some("tip"));
    }

    #[test]
    fn toggle_button_toggle() {
        let mut btn = ToggleButton::new("Test");
        btn.toggle();
        assert!(btn.pressed);
        btn.toggle();
        assert!(!btn.pressed);
    }

    #[test]
    fn take_toggled_drains_once() {
        let mut btn = ToggleButton::new("Test");
        assert_eq!(btn.take_toggled(), None);
        btn.toggle();
        assert_eq!(btn.take_toggled(), Some(true));
        assert_eq!(btn.take_toggled(), None);
        // Programmatic set_pressed does not signal.
        btn.set_pressed(true);
        assert_eq!(btn.take_toggled(), None);
    }

    #[test]
    fn press_release_inside_toggles() {
        let mut btn = ToggleButton::new("Test");
        laid_out(&mut btn, 80.0, 32.0);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(40.0, 16.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(event(&mut btn, &press), EventResponse::CapturePointer);
        let release = WidgetEvent::PointerReleased {
            position: Vec2::new(40.0, 16.0),
            button: PointerButton::Primary,
        };
        assert_eq!(event(&mut btn, &release), EventResponse::ReleasePointer);
        assert!(btn.pressed);
        assert_eq!(btn.take_toggled(), Some(true));
    }

    #[test]
    fn press_release_outside_cancels() {
        let mut btn = ToggleButton::new("Test");
        laid_out(&mut btn, 80.0, 32.0);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(40.0, 16.0),
            button: PointerButton::Primary,
            count: 1,
        };
        event(&mut btn, &press);
        let release = WidgetEvent::PointerReleased {
            position: Vec2::new(400.0, 400.0),
            button: PointerButton::Primary,
        };
        event(&mut btn, &release);
        assert!(!btn.pressed);
        assert_eq!(btn.take_toggled(), None);
    }

    #[test]
    fn space_and_enter_toggle() {
        let mut btn = ToggleButton::new("Test");
        laid_out(&mut btn, 80.0, 32.0);
        let space = WidgetEvent::KeyPressed {
            key: "Space".to_string(),
            repeat: false,
        };
        event(&mut btn, &space);
        assert!(btn.pressed);
        let enter = WidgetEvent::KeyPressed {
            key: "Enter".to_string(),
            repeat: false,
        };
        event(&mut btn, &enter);
        assert!(!btn.pressed);
        // Auto-repeat does not fire.
        let rep = WidgetEvent::KeyPressed {
            key: "Space".to_string(),
            repeat: true,
        };
        event(&mut btn, &rep);
        assert!(!btn.pressed);
    }

    #[test]
    fn semantic_click_toggles() {
        let mut btn = ToggleButton::new("Test");
        let ev = WidgetEvent::SemanticAction(SemanticAction::Click);
        assert_eq!(event(&mut btn, &ev), EventResponse::RequestRepaint);
        assert!(btn.pressed);
    }

    #[test]
    fn disabled_ignores_events() {
        let mut btn = ToggleButton::new("Test").enabled(false);
        let ev = WidgetEvent::SemanticAction(SemanticAction::Click);
        assert_eq!(event(&mut btn, &ev), EventResponse::Ignored);
        assert!(!btn.pressed);
    }

    #[test]
    fn accessibility_role_pressed_state() {
        let btn = ToggleButton::new("Bold").pressed(true);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        btn.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Button);
        assert_eq!(node.toggled(), Some(Toggled::True));
        assert_eq!(node.label(), Some("Bold"));
        assert!(node.supports_action(accesskit::Action::Click));
        assert!(node.supports_action(accesskit::Action::Focus));
    }

    #[test]
    fn accessibility_unpressed_and_disabled() {
        let btn = ToggleButton::new("X").enabled(false);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        btn.accessibility(&mut node);
        assert_eq!(node.toggled(), Some(Toggled::False));
        assert!(node.is_disabled());
    }

    #[test]
    fn measure_returns_min_size() {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let mut btn = ToggleButton::new("OK");
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
    fn debug_format() {
        let btn = ToggleButton::new("B");
        let debug = format!("{:?}", btn);
        assert!(debug.contains("ToggleButton"));
        assert!(debug.contains("B"));
    }
}
