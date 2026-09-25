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

use crate::text_paint::estimate_label_width;
use accesskit::Node as AccessKitNode;
use glam::Vec2;
use kurbo::Shape as _;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::TokenKey;
use martensite_core::{NodeFlags, Rect};

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
/// Filled primary-variant face — the `TokenKey::PrimaryColor`
/// fallback (a blue-600 grade fill).
const PRIMARY_FACE: [u8; 4] = [37, 99, 235, 255];
/// Inverse ink on the primary fill — the `TokenKey::TextInverseColor`
/// fallback.
const INK_INVERSE: [u8; 4] = [255, 255, 255, 255];
/// Corner radius of the button face.
const CORNER_RADIUS: f64 = 4.0;
/// Horizontal padding between the border and the label.
const TEXT_PAD_X: f32 = 10.0;

/// Enter and Space activate a focused button — the same keys every
/// platform button idiom accepts.
fn is_activation_key(key: &str) -> bool {
    matches!(key, "Enter" | "Space" | " ")
}

/// Multiplies the RGB channels of a resolved colour — used for the
/// pressed-face tint so the effect applies on top of whichever face
/// the active theme resolved.
fn shade(color: [u8; 4], factor: f32) -> [u8; 4] {
    let scale = |c: u8| (f32::from(c) * factor).round().clamp(0.0, 255.0) as u8;
    [scale(color[0]), scale(color[1]), scale(color[2]), color[3]]
}

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
    /// Whether the button paints the filled primary variant — a
    /// `TokenKey::PrimaryColor` face with inverse text, for the one
    /// dominant verb on a surface. `false` (default) keeps the
    /// neutral outlined appearance.
    pub primary: bool,
    /// Optional tooltip text.
    pub tooltip: Option<String>,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Pointer currently held down on the button (press-to-release
    /// span; the visual "pressed" state is `held && inside`).
    held: bool,
    /// Whether the pointer is inside the face while `held` — dragging
    /// out and releasing cancels the activation, matching platform
    /// button semantics.
    inside: bool,
    /// One-shot activation flag set when a press completes inside the
    /// face (or Enter/Space/AT Click fires) — drained by
    /// [`Button::take_activated`].
    activated: bool,
    /// Shared shaped-text painter — when set, `paint` emits real
    /// `GlyphRun`s; without it the label falls back to `DrawText`
    /// placeholder boxes. See [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
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
            primary: false,
            tooltip: None,
            cached_bounds: Rect::default(),
            held: false,
            inside: false,
            activated: false,
            text_painter: None,
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

    /// Selects the filled primary variant (`true`) or the default
    /// neutral outlined appearance (`false`). The primary variant
    /// resolves `TokenKey::PrimaryColor` for its face and
    /// `TokenKey::TextInverseColor` for its label — the "one dominant
    /// verb per page" idiom. A disabled primary button keeps the
    /// muted disabled presentation either way.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Button;
    ///
    /// let btn = Button::new("Acknowledge").primary(true);
    /// assert!(btn.primary);
    /// ```
    #[inline]
    #[must_use]
    pub fn primary(mut self, primary: bool) -> Self {
        self.primary = primary;
        self
    }

    /// Drains the one-shot activation flag: returns `true` once per
    /// completed activation — a primary-button release inside the face
    /// after a press, an Enter/Space key release, or an accessibility
    /// `Click` action. Parent widgets poll this to react to clicks.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Button;
    ///
    /// let mut btn = Button::new("OK");
    /// assert!(!btn.take_activated());
    /// ```
    #[inline]
    pub fn take_activated(&mut self) -> bool {
        std::mem::take(&mut self.activated)
    }

    /// Whether the button is currently drawn in its pressed state —
    /// a held press with the pointer still inside the face.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Button;
    ///
    /// assert!(!Button::new("OK").is_pressed());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_pressed(&self) -> bool {
        self.held && self.inside
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

    /// Shares a [`crate::text_paint::TextPainter`] so `paint` emits real
    /// glyph runs instead of `DrawText` placeholder boxes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::shared_painter;
    ///
    /// let painter = shared_painter();
    /// ```
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }
}

impl Widget for Button {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // A button has a default minimum size of 80x32 logical pt, but
        // grows to fit its label (same case-aware estimate as tabs) —
        // a fixed 80 pt slot clipped every label past ~8 chars.
        let label_w = cx.pt(estimate_label_width(&self.label) + 2.0 * TEXT_PAD_X);
        let min_w = cx
            .pt(80.0)
            .max(label_w)
            .min(constraints.max_size.x.max(0.0));
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
                position,
                ..
            } if cx.bounds.contains(*position) => {
                self.held = true;
                self.inside = true;
                // Capture so the release reaches us even when the
                // pointer is dragged off the face — that release
                // cancels rather than activates.
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerMoved { position } if self.held => {
                let inside = cx.bounds.contains(*position);
                if inside != self.inside {
                    self.inside = inside;
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Handled
                }
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } if self.held => {
                self.held = false;
                if cx.bounds.contains(*position) {
                    self.activated = true;
                }
                self.inside = false;
                EventResponse::ReleasePointer
            }
            WidgetEvent::KeyPressed { key, repeat } if is_activation_key(key) => {
                if !*repeat {
                    self.held = true;
                    self.inside = true;
                }
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyReleased { key } if is_activation_key(key) => {
                if self.held {
                    self.activated = true;
                }
                self.held = false;
                self.inside = false;
                EventResponse::Handled
            }
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                self.activated = true;
                EventResponse::Handled
            }
            WidgetEvent::FocusLost if self.held => {
                // A keyboard-armed button disarms on focus loss.
                self.held = false;
                self.inside = false;
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerLeave if self.held => {
                // Pointer capture means the press survives leaving the
                // face — only the pressed *visual* drops until the
                // pointer re-enters or releases.
                self.inside = false;
                EventResponse::RequestRepaint
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
            // The primary variant fills with the theme's primary
            // token and inverse ink; the default keeps the neutral
            // surface face.
            let face = if self.primary {
                cx.color(TokenKey::PrimaryColor, PRIMARY_FACE)
            } else {
                cx.color(TokenKey::SurfaceColor, FACE_ENABLED)
            };
            (
                if self.is_pressed() {
                    shade(face, 0.9)
                } else {
                    face
                },
                if self.primary {
                    cx.color(TokenKey::TextInverseColor, INK_INVERSE)
                } else {
                    cx.color(TokenKey::TextColor, INK_ENABLED)
                },
            )
        } else {
            (
                cx.color(TokenKey::SurfaceColor, FACE_DISABLED),
                cx.color(TokenKey::TextMutedColor, INK_DISABLED),
            )
        };

        let rounded = kurbo::RoundedRect::from_rect(rect, cx.ptf(CORNER_RADIUS)).into_path(0.1);
        cx.list.push_path(rounded.clone(), face);
        // A filled primary face carries its own edge — stroking it
        // with the border token would read as a second outline.
        let edge = if self.primary && self.enabled {
            face
        } else {
            cx.color(TokenKey::BorderColor, EDGE)
        };
        cx.list.push_stroke_path(rounded, cx.pt(1.0), edge);

        // The label is left-aligned inside the face and vertically
        // centred — `DrawText` positions by the text run's top edge, so
        // centre the font box within the face. Clipped to the face
        // interior — a long label can't spill past the rounded edge.
        let text_x = b.origin.x + cx.pt(TEXT_PAD_X);
        crate::text_paint::paint_label_clipped(
            crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
            cx.list,
            kurbo::Rect::new(
                f64::from(text_x),
                f64::from(b.origin.y),
                f64::from(b.max_x() - cx.pt(TEXT_PAD_X)),
                f64::from(b.max_y()),
            ),
            kurbo::Point::new(
                f64::from(text_x),
                f64::from(b.origin.y + (b.size.y - cx.pt(14.0)) / 2.0),
            ),
            &self.label,
            cx.pt(14.0),
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
        LayoutContext { hot, scale: 1.0 }
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
    fn button_primary_paints_filled_face() {
        use martensite_core::{PaintCommand, PaintList, Theme};
        let paint_face = |b: &Button| {
            let mut list = PaintList::new();
            let theme = Theme::new("test");
            let mut cx = PaintContext {
                list: &mut list,
                bounds: Rect::new(0.0, 0.0, 80.0, 32.0),
                theme: &theme,
                scale: 1.0,
                text_painter: None,
            };
            b.paint(&mut cx);
            cx.list.commands.iter().find_map(|c| match c {
                PaintCommand::FillPath(_, color) => Some(*color),
                _ => None,
            })
        };
        assert_eq!(paint_face(&Button::new("OK")), Some(FACE_ENABLED));
        assert_eq!(
            paint_face(&Button::new("OK").primary(true)),
            Some(PRIMARY_FACE)
        );
    }

    #[test]
    fn button_debug_format() {
        let btn = Button::new("OK");
        let debug = format!("{:?}", btn);
        assert!(debug.contains("Button"));
        assert!(debug.contains("OK"));
    }
}
