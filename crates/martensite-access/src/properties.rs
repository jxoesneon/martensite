//! Accessibility property helpers for constructing AccessKit nodes.
//!
//! This module provides convenience functions and builders for setting
//! common accessibility properties on [`accesskit::Node`] instances,
//! including roles, names, descriptions, and state bitmasks
//! (expanded, checked, disabled).

use accesskit::{Node, Role, Toggled};

/// A builder for constructing accessibility property sets on a [`Node`].
///
/// This is a convenience wrapper that chains property setters and then
/// applies them to a node. It is particularly useful for widgets that
/// want to declare their accessibility properties in a single expression.
///
/// # Examples
///
/// ```
/// use martensite_access::properties::AccessibilityBuilder;
/// use accesskit::Role;
///
/// let mut node = accesskit::Node::new(Role::Button);
/// AccessibilityBuilder::new(Role::Button)
///     .label("Submit")
///     .description("Click to submit the form")
///     .focusable()
///     .apply(&mut node);
///
/// assert_eq!(node.label(), Some("Submit"));
/// assert_eq!(node.description(), Some("Click to submit the form"));
/// assert!(node.supports_action(accesskit::Action::Focus));
/// ```
#[derive(Clone, Debug)]
pub struct AccessibilityBuilder {
    role: Role,
    label: Option<String>,
    description: Option<String>,
    value: Option<String>,
    tooltip: Option<String>,
    focusable: bool,
    disabled: bool,
    expanded: Option<bool>,
    toggled: Option<Toggled>,
    clicked: bool,
    live: Option<accesskit::Live>,
}

impl AccessibilityBuilder {
    /// Creates a new builder with the given role.
    pub fn new(role: Role) -> Self {
        Self {
            role,
            label: None,
            description: None,
            value: None,
            tooltip: None,
            focusable: false,
            disabled: false,
            expanded: None,
            toggled: None,
            clicked: false,
            live: None,
        }
    }

    /// Sets the accessible label (name).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the accessible description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Sets the accessible value (e.g., text input content).
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Sets the tooltip text.
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Marks the node as focusable (adds `Action::Focus`).
    pub fn focusable(mut self) -> Self {
        self.focusable = true;
        self
    }

    /// Marks the node as disabled.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// Sets the expanded state (for expandable widgets like drop-downs).
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }

    /// Sets the toggled (checked) state.
    pub fn toggled(mut self, toggled: Toggled) -> Self {
        self.toggled = Some(toggled);
        self
    }

    /// Marks the node as clickable (adds `Action::Click`).
    pub fn clickable(mut self) -> Self {
        self.clicked = true;
        self
    }

    /// Sets the live region attribute (e.g. for polite or assertive screen reader announcements).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::properties::AccessibilityBuilder;
    /// use accesskit::{Live, Node, Role};
    ///
    /// let mut node = Node::new(Role::Alert);
    /// AccessibilityBuilder::new(Role::Alert)
    ///     .live(Live::Polite)
    ///     .apply(&mut node);
    /// assert_eq!(node.live(), Some(Live::Polite));
    /// ```
    pub fn live(mut self, live: accesskit::Live) -> Self {
        self.live = Some(live);
        self
    }

    /// Applies all accumulated properties to the given [`Node`].
    pub fn apply(self, node: &mut Node) {
        node.set_role(self.role);

        if let Some(label) = self.label {
            node.set_label(label);
        }
        if let Some(desc) = self.description {
            node.set_description(desc);
        }
        if let Some(value) = self.value {
            node.set_value(value);
        }
        if let Some(tooltip) = self.tooltip {
            node.set_tooltip(tooltip);
        }
        if self.focusable {
            node.add_action(accesskit::Action::Focus);
        }
        if self.disabled {
            node.set_disabled();
        }
        if let Some(expanded) = self.expanded {
            node.set_expanded(expanded);
        }
        if let Some(toggled) = self.toggled {
            node.set_toggled(toggled);
        }
        if self.clicked {
            node.add_action(accesskit::Action::Click);
        }
        if let Some(live) = self.live {
            node.set_live(live);
        }
    }
}

/// Sets the accessible name (label) on a node.
#[inline]
pub fn set_label(node: &mut Node, label: impl Into<String>) {
    node.set_label(label);
}

/// Sets the accessible description on a node.
#[inline]
pub fn set_description(node: &mut Node, desc: impl Into<String>) {
    node.set_description(desc);
}

/// Sets the accessible value on a node.
#[inline]
pub fn set_value(node: &mut Node, value: impl Into<String>) {
    node.set_value(value);
}

/// Marks a node as focusable by adding the `Action::Focus` action.
#[inline]
pub fn set_focusable(node: &mut Node) {
    node.add_action(accesskit::Action::Focus);
}

/// Marks a node as disabled.
#[inline]
pub fn set_disabled(node: &mut Node) {
    node.set_disabled();
}

/// Marks a node as expanded or collapsed.
#[inline]
pub fn set_expanded(node: &mut Node, expanded: bool) {
    node.set_expanded(expanded);
}

/// Sets the toggled (checked) state on a node.
#[inline]
pub fn set_toggled(node: &mut Node, toggled: Toggled) {
    node.set_toggled(toggled);
}

/// Marks a node as clickable by adding the `Action::Click` action.
#[inline]
pub fn set_clickable(node: &mut Node) {
    node.add_action(accesskit::Action::Click);
}

/// Sets the live region mode on a node.
///
/// # Examples
///
/// ```
/// use martensite_access::properties::set_live;
/// use accesskit::{Live, Node, Role};
///
/// let mut node = Node::new(Role::Alert);
/// set_live(&mut node, Live::Assertive);
/// assert_eq!(node.live(), Some(Live::Assertive));
/// ```
#[inline]
pub fn set_live(node: &mut Node, live: accesskit::Live) {
    node.set_live(live);
}

/// Returns the appropriate AccessKit [`Role`] for a widget based on its
/// semantic intent.
///
/// This is a convenience function for common widget types.
pub fn role_for_button() -> Role {
    Role::Button
}

/// Returns the [`Role`] for a text input widget.
pub fn role_for_text_input() -> Role {
    Role::TextInput
}

/// Returns the [`Role`] for a checkbox widget.
pub fn role_for_checkbox() -> Role {
    Role::CheckBox
}

/// Returns the [`Role`] for a radio button widget.
pub fn role_for_radio_button() -> Role {
    Role::RadioButton
}

/// Returns the [`Role`] for a slider widget.
pub fn role_for_slider() -> Role {
    Role::Slider
}

/// Returns the [`Role`] for a generic container widget.
pub fn role_for_container() -> Role {
    Role::GenericContainer
}

/// Returns the [`Role`] for a text display widget.
pub fn role_for_text_display() -> Role {
    Role::TextRun
}

/// Returns the [`Role`] for an image widget.
pub fn role_for_image() -> Role {
    Role::Image
}

/// Returns the [`Role`] for a link widget.
pub fn role_for_link() -> Role {
    Role::Link
}

/// Returns the [`Role`] for a list widget.
pub fn role_for_list() -> Role {
    Role::List
}

/// Returns the [`Role`] for a list item widget.
pub fn role_for_list_item() -> Role {
    Role::ListItem
}

/// Returns the [`Role`] for a dialog/modal widget.
pub fn role_for_dialog() -> Role {
    Role::Dialog
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_label() {
        let mut node = Node::new(Role::Unknown);
        AccessibilityBuilder::new(Role::Button)
            .label("OK")
            .apply(&mut node);
        assert_eq!(node.label(), Some("OK"));
        assert_eq!(node.role(), Role::Button);
    }

    #[test]
    fn builder_sets_description() {
        let mut node = Node::new(Role::Unknown);
        AccessibilityBuilder::new(Role::Button)
            .description("Confirms the action")
            .apply(&mut node);
        assert_eq!(node.description(), Some("Confirms the action"));
    }

    #[test]
    fn builder_sets_value() {
        let mut node = Node::new(Role::Unknown);
        AccessibilityBuilder::new(Role::TextInput)
            .value("hello")
            .apply(&mut node);
        assert_eq!(node.value(), Some("hello"));
    }

    #[test]
    fn builder_sets_tooltip() {
        let mut node = Node::new(Role::Unknown);
        AccessibilityBuilder::new(Role::Button)
            .tooltip("Click me")
            .apply(&mut node);
        assert_eq!(node.tooltip(), Some("Click me"));
    }

    #[test]
    fn builder_focusable_adds_action() {
        let mut node = Node::new(Role::Unknown);
        AccessibilityBuilder::new(Role::Button)
            .focusable()
            .apply(&mut node);
        assert!(node.supports_action(accesskit::Action::Focus));
    }

    #[test]
    fn builder_disabled_sets_flag() {
        let mut node = Node::new(Role::Unknown);
        AccessibilityBuilder::new(Role::Button)
            .disabled()
            .apply(&mut node);
        assert!(node.is_disabled());
    }

    #[test]
    fn builder_expanded_sets_flag() {
        let mut node = Node::new(Role::Unknown);
        AccessibilityBuilder::new(Role::Button)
            .expanded(true)
            .apply(&mut node);
        assert_eq!(node.is_expanded(), Some(true));
    }

    #[test]
    fn builder_toggled_sets_state() {
        let mut node = Node::new(Role::Unknown);
        AccessibilityBuilder::new(Role::CheckBox)
            .toggled(Toggled::True)
            .apply(&mut node);
        assert_eq!(node.toggled(), Some(Toggled::True));
    }

    #[test]
    fn builder_clickable_adds_action() {
        let mut node = Node::new(Role::Unknown);
        AccessibilityBuilder::new(Role::Button)
            .clickable()
            .apply(&mut node);
        assert!(node.supports_action(accesskit::Action::Click));
    }

    #[test]
    fn builder_sets_live() {
        let mut node = Node::new(Role::Alert);
        AccessibilityBuilder::new(Role::Alert)
            .live(accesskit::Live::Polite)
            .apply(&mut node);
        assert_eq!(node.live(), Some(accesskit::Live::Polite));
    }

    #[test]
    fn helper_set_live() {
        let mut node = Node::new(Role::Alert);
        set_live(&mut node, accesskit::Live::Assertive);
        assert_eq!(node.live(), Some(accesskit::Live::Assertive));
    }

    #[test]
    fn builder_chains_all_properties() {
        let mut node = Node::new(Role::Unknown);
        AccessibilityBuilder::new(Role::CheckBox)
            .label("Accept terms")
            .description("Check to accept the terms of service")
            .value("checked")
            .tooltip("Required")
            .focusable()
            .clickable()
            .toggled(Toggled::True)
            .apply(&mut node);

        assert_eq!(node.role(), Role::CheckBox);
        assert_eq!(node.label(), Some("Accept terms"));
        assert_eq!(
            node.description(),
            Some("Check to accept the terms of service")
        );
        assert_eq!(node.value(), Some("checked"));
        assert_eq!(node.tooltip(), Some("Required"));
        assert!(node.supports_action(accesskit::Action::Focus));
        assert!(node.supports_action(accesskit::Action::Click));
        assert_eq!(node.toggled(), Some(Toggled::True));
    }

    #[test]
    fn helper_functions_set_properties() {
        let mut node = Node::new(Role::Button);
        set_label(&mut node, "Test");
        set_description(&mut node, "Desc");
        set_value(&mut node, "Val");
        set_focusable(&mut node);
        set_disabled(&mut node);
        set_expanded(&mut node, true);
        set_toggled(&mut node, Toggled::True);
        set_clickable(&mut node);

        assert_eq!(node.label(), Some("Test"));
        assert_eq!(node.description(), Some("Desc"));
        assert_eq!(node.value(), Some("Val"));
        assert!(node.supports_action(accesskit::Action::Focus));
        assert!(node.is_disabled());
        assert_eq!(node.is_expanded(), Some(true));
        assert_eq!(node.toggled(), Some(Toggled::True));
        assert!(node.supports_action(accesskit::Action::Click));
    }

    #[test]
    fn role_helpers_return_expected_roles() {
        assert_eq!(role_for_button(), Role::Button);
        assert_eq!(role_for_text_input(), Role::TextInput);
        assert_eq!(role_for_checkbox(), Role::CheckBox);
        assert_eq!(role_for_radio_button(), Role::RadioButton);
        assert_eq!(role_for_slider(), Role::Slider);
        assert_eq!(role_for_container(), Role::GenericContainer);
        assert_eq!(role_for_text_display(), Role::TextRun);
        assert_eq!(role_for_image(), Role::Image);
        assert_eq!(role_for_link(), Role::Link);
        assert_eq!(role_for_list(), Role::List);
        assert_eq!(role_for_list_item(), Role::ListItem);
        assert_eq!(role_for_dialog(), Role::Dialog);
    }
}
