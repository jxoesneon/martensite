//! `CheckBox` widget: a toggleable checkbox with an accessible label.
//!
//! The `CheckBox` widget exposes `Role::CheckBox`, an accessible label,
//! the `Action::Click` and `Action::Focus` accessibility actions, and
//! the `Toggled` state. It integrates with the focus system via
//! `NodeFlags::FOCUSABLE`.

use accesskit::{Node as AccessKitNode, Toggled};
use glam::Vec2;
use martensite_core::widget::{LayoutConstraints, LayoutContext, Widget};
use martensite_core::Rect;

/// A checkbox widget with a label and toggle state.
///
/// # Examples
///
/// ```
/// use martensite::widgets::CheckBox;
///
/// let cb = CheckBox::new("Accept terms")
///     .checked(true);
/// assert_eq!(cb.label, "Accept terms");
/// assert!(cb.checked);
/// ```
pub struct CheckBox {
    /// The accessible label for the checkbox.
    pub label: String,
    /// Whether the checkbox is currently checked.
    pub checked: bool,
    /// Whether the checkbox is enabled.
    pub enabled: bool,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
}

impl CheckBox {
    /// Creates a new checkbox with the given label, unchecked by default.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CheckBox;
    ///
    /// let cb = CheckBox::new("Subscribe");
    /// assert_eq!(cb.label, "Subscribe");
    /// assert!(!cb.checked);
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            checked: false,
            enabled: true,
            cached_bounds: Rect::default(),
        }
    }

    /// Sets the checked state.
    #[inline]
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Sets whether the checkbox is enabled.
    #[inline]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Toggles the checked state.
    #[inline]
    pub fn toggle(&mut self) {
        self.checked = !self.checked;
    }

    /// Returns the cached bounds from the last layout pass.
    #[inline]
    pub fn cached_bounds(&self) -> Rect {
        self.cached_bounds
    }
}

impl Widget for CheckBox {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let min_w = 20.0_f32.min(constraints.max_size.x.max(0.0));
        let min_h = 20.0_f32.min(constraints.max_size.y.max(0.0));
        Vec2::new(min_w, min_h)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::CheckBox);
        node.set_label(self.label.as_str());
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Focus);
        node.set_toggled(if self.checked {
            Toggled::True
        } else {
            Toggled::False
        });
        if !self.enabled {
            node.set_disabled();
        }
    }
}

impl std::fmt::Debug for CheckBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckBox")
            .field("label", &self.label)
            .field("checked", &self.checked)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Clone for CheckBox {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            checked: self.checked,
            enabled: self.enabled,
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
    fn checkbox_new() {
        let cb = CheckBox::new("Accept");
        assert_eq!(cb.label, "Accept");
        assert!(!cb.checked);
        assert!(cb.enabled);
    }

    #[test]
    fn checkbox_builder_methods() {
        let cb = CheckBox::new("Agree").checked(true).enabled(false);
        assert!(cb.checked);
        assert!(!cb.enabled);
    }

    #[test]
    fn checkbox_toggle() {
        let mut cb = CheckBox::new("Test");
        assert!(!cb.checked);
        cb.toggle();
        assert!(cb.checked);
        cb.toggle();
        assert!(!cb.checked);
    }

    #[test]
    fn checkbox_measure_returns_size() {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let mut cb = CheckBox::new("Test");
        let size = cb.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(200.0, 100.0),
            },
        );
        assert!(size.x >= 0.0 && size.y >= 0.0);
    }

    #[test]
    fn checkbox_layout_sets_bounds() {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let mut cb = CheckBox::new("Test");
        let bounds = Rect::new(0.0, 0.0, 20.0, 20.0);
        cb.layout(&mut cx, bounds);
        assert_eq!(cb.cached_bounds(), bounds);
    }

    #[test]
    fn checkbox_accessibility_sets_role_label_toggled() {
        let cb = CheckBox::new("Accept").checked(true);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        cb.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::CheckBox);
        assert_eq!(node.label(), Some("Accept"));
        assert_eq!(node.toggled(), Some(Toggled::True));
        assert!(node.supports_action(accesskit::Action::Click));
        assert!(node.supports_action(accesskit::Action::Focus));
    }

    #[test]
    fn checkbox_accessibility_unchecked_state() {
        let cb = CheckBox::new("Decline");
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        cb.accessibility(&mut node);
        assert_eq!(node.toggled(), Some(Toggled::False));
    }

    #[test]
    fn checkbox_accessibility_disabled() {
        let cb = CheckBox::new("Locked").enabled(false);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        cb.accessibility(&mut node);
        assert!(node.is_disabled());
    }

    #[test]
    fn checkbox_clone() {
        let cb = CheckBox::new("Test").checked(true);
        let cloned = cb.clone();
        assert_eq!(cb.label, cloned.label);
        assert_eq!(cb.checked, cloned.checked);
    }

    #[test]
    fn checkbox_debug_format() {
        let cb = CheckBox::new("Test");
        let debug = format!("{:?}", cb);
        assert!(debug.contains("CheckBox"));
        assert!(debug.contains("Test"));
    }
}
