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
use martensite_core::widget::{LayoutConstraints, LayoutContext, Widget};
use martensite_core::Rect;

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
}

impl Widget for TextInput {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // A text input has a default minimum size of 120x24.
        let min_w = 120.0_f32.min(constraints.max_size.x.max(0.0));
        let min_h = 24.0_f32.min(constraints.max_size.y.max(0.0));
        Vec2::new(min_w, min_h)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
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

impl Clone for TextInput {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            value: self.value.clone(),
            placeholder: self.placeholder.clone(),
            enabled: self.enabled,
            read_only: self.read_only,
            cached_bounds: self.cached_bounds,
        }
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
