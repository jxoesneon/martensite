//! `TextInput` widget: an editable text field with an accessible label.
//!
//! The `TextInput` widget exposes `Role::TextInput`, an accessible label,
//! the `Action::Focus` and `Action::SetValue` accessibility actions, and
//! the current value. It integrates with the focus system via
//! `NodeFlags::FOCUSABLE`.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::text_input::TextInput;
//!
//! let input = TextInput::new("Search").placeholder("Type here...");
//! assert_eq!(input.label, "Search");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

/// Field background colour.
const FACE: [u8; 4] = [255, 255, 255, 255];
/// Field border colour.
const EDGE: [u8; 4] = [140, 145, 155, 255];
/// Border colour while focused.
const EDGE_FOCUSED: [u8; 4] = [40, 110, 220, 255];
/// Text ink colour.
const INK: [u8; 4] = [20, 20, 25, 255];
/// Placeholder ink colour.
const INK_PLACEHOLDER: [u8; 4] = [150, 150, 155, 255];
/// Caret colour.
const CARET: [u8; 4] = [30, 30, 35, 255];
/// Horizontal inset for the editable text.
const TEXT_PAD_X: f32 = 8.0;

/// A text input widget with a label and editable value.
///
/// # Examples
///
/// ```
/// use martensite::widgets::TextInput;
///
/// let input = TextInput::new("Email")
///     .value("user@example.com")
///     .placeholder("Enter your email");
/// assert_eq!(input.label, "Email");
/// assert_eq!(input.value, "user@example.com");
/// ```
#[derive(Clone)]
pub struct TextInput {
    /// The accessible label for the text input.
    pub label: String,
    /// The current text value.
    pub value: String,
    /// Placeholder text shown when the value is empty.
    pub placeholder: String,
    /// Whether the text input is enabled.
    pub enabled: bool,
    /// Whether the text input is read-only.
    pub read_only: bool,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter — when set, `paint` emits real
    /// `GlyphRun`s; without it text falls back to `DrawText`
    /// placeholder boxes. See [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
    /// Whether the input currently holds keyboard focus. Updated by the
    /// `FocusGained`/`FocusLost` widget events.
    focused: bool,
}

impl TextInput {
    /// Creates a new text input with the given label and empty value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Name");
    /// assert_eq!(input.label, "Name");
    /// assert!(input.value.is_empty());
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: String::new(),
            placeholder: String::new(),
            enabled: true,
            read_only: false,
            cached_bounds: Rect::default(),
            text_painter: None,
            focused: false,
        }
    }

    /// Sets the current text value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Username").value("alice");
    /// assert_eq!(input.value, "alice");
    /// ```
    #[inline]
    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    /// Sets the placeholder text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Search").placeholder("Type query...");
    /// assert_eq!(input.placeholder, "Type query...");
    /// ```
    #[inline]
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets whether the text input is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Locked").enabled(false);
    /// assert!(!input.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets whether the text input is read-only.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("ID").read_only(true);
    /// assert!(input.read_only);
    /// ```
    #[inline]
    #[must_use]
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Sets the value (mutable version for programmatic updates).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let mut input = TextInput::new("Name");
    /// input.set_value("Bob");
    /// assert_eq!(input.value, "Bob");
    /// ```
    #[inline]
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
    }

    /// Returns the cached bounds from the last layout pass.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TextInput;
    ///
    /// let input = TextInput::new("Address");
    /// let bounds = input.cached_bounds();
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
}

impl Widget for TextInput {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // A text input has a default minimum size of 120x24 logical pt.
        let min_w = cx.pt(120.0).min(constraints.max_size.x.max(0.0));
        let min_h = cx.pt(24.0).min(constraints.max_size.y.max(0.0));
        Vec2::new(min_w, min_h)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        // Declare keyboard focusability on the arena node — standalone
        // inputs need it for `ImeCommitted` delivery.
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::TextInput);
        node.set_label(self.label.as_str());
        node.set_value(self.value.as_str());
        node.add_action(accesskit::Action::Focus);
        if !self.read_only {
            node.add_action(accesskit::Action::SetValue);
        }
        if !self.enabled {
            node.set_disabled();
        }
        if self.read_only {
            node.set_read_only();
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
            } => EventResponse::CaptureFocus,
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                EventResponse::RequestRepaint
            }
            WidgetEvent::ImeCommitted { text } if !self.read_only => {
                self.value.push_str(text);
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, .. } if !self.read_only && key == "Backspace" => {
                self.value.pop();
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
        cx.list
            .push_fill_rect(rect, cx.color(TokenKey::SurfaceColor, FACE));
        cx.list.push_stroke_rect(
            rect,
            cx.pt(1.0),
            if self.focused {
                cx.color(TokenKey::AccentColor, EDGE_FOCUSED)
            } else {
                cx.color(TokenKey::BorderColor, EDGE)
            },
        );

        // `DrawText` positions by the text run's top edge — centre the
        // 14 pt font box within the field.
        let font_px = cx.pt(14.0);
        let text_y = b.origin.y + (b.size.y - font_px) / 2.0;
        let text_x = b.origin.x + cx.pt(TEXT_PAD_X);
        if self.value.is_empty() {
            crate::text_paint::paint_label(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Point::new(f64::from(text_x), f64::from(text_y)),
                &self.placeholder,
                font_px,
                cx.color(TokenKey::TextMutedColor, INK_PLACEHOLDER),
            );
        } else {
            crate::text_paint::paint_label(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Point::new(f64::from(text_x), f64::from(text_y)),
                &self.value,
                font_px,
                cx.color(TokenKey::TextColor, INK),
            );
        }

        // End-of-text caret — approximate x advance at 7 px per
        // character until real shaping lands in this widget.
        if self.focused {
            let caret_x = f64::from(text_x + self.value.chars().count() as f32 * cx.pt(7.0));
            let top = f64::from(b.origin.y + cx.pt(4.0));
            let mut caret = kurbo::BezPath::new();
            caret.move_to((caret_x, top));
            caret.line_to((caret_x, f64::from(b.max_y()) - cx.ptf(4.0)));
            cx.list
                .push_stroke_path(caret, cx.pt(1.0), cx.color(TokenKey::TextColor, CARET));
        }
    }
}

impl std::fmt::Debug for TextInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextInput")
            .field("label", &self.label)
            .field("value", &self.value)
            .field("enabled", &self.enabled)
            .field("read_only", &self.read_only)
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
    fn text_input_new() {
        let input = TextInput::new("Name");
        assert_eq!(input.label, "Name");
        assert!(input.value.is_empty());
        assert!(input.enabled);
        assert!(!input.read_only);
    }

    #[test]
    fn text_input_builder_methods() {
        let input = TextInput::new("Email")
            .value("test@test.com")
            .placeholder("Enter email")
            .enabled(false)
            .read_only(true);
        assert_eq!(input.value, "test@test.com");
        assert_eq!(input.placeholder, "Enter email");
        assert!(!input.enabled);
        assert!(input.read_only);
    }

    #[test]
    fn text_input_set_value() {
        let mut input = TextInput::new("Name");
        input.set_value("John");
        assert_eq!(input.value, "John");
    }

    #[test]
    fn text_input_measure_returns_size() {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let mut input = TextInput::new("Test");
        let size = input.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(300.0, 100.0),
            },
        );
        assert!(size.x >= 0.0 && size.y >= 0.0);
    }

    #[test]
    fn text_input_layout_sets_bounds() {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let mut input = TextInput::new("Test");
        let bounds = Rect::new(0.0, 0.0, 120.0, 24.0);
        input.layout(&mut cx, bounds);
        assert_eq!(input.cached_bounds(), bounds);
    }

    #[test]
    fn text_input_accessibility_sets_role_label_value() {
        let input = TextInput::new("Email").value("hello@test.com");
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        input.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::TextInput);
        assert_eq!(node.label(), Some("Email"));
        assert_eq!(node.value(), Some("hello@test.com"));
        assert!(node.supports_action(accesskit::Action::Focus));
        assert!(node.supports_action(accesskit::Action::SetValue));
    }

    #[test]
    fn text_input_accessibility_read_only_no_set_value() {
        let input = TextInput::new("Read").read_only(true);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        input.accessibility(&mut node);
        assert!(!node.supports_action(accesskit::Action::SetValue));
    }

    #[test]
    fn text_input_accessibility_disabled() {
        let input = TextInput::new("Disabled").enabled(false);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        input.accessibility(&mut node);
        assert!(node.is_disabled());
    }

    #[test]
    fn text_input_clone() {
        let input = TextInput::new("Test").value("hello");
        let cloned = input.clone();
        assert_eq!(input.label, cloned.label);
        assert_eq!(input.value, cloned.value);
    }

    #[test]
    fn text_input_debug_format() {
        let input = TextInput::new("Test");
        let debug = format!("{:?}", input);
        assert!(debug.contains("TextInput"));
        assert!(debug.contains("Test"));
    }
}
