//! Modal focus scope stack with focus trapping and auto-restoration.
//!
//! A [`FocusScope`] represents a modal dialog, popover, or context menu
//! that traps keyboard navigation within its boundaries. When a scope is
//! pushed onto the [`FocusScopeStack`], the previously focused widget is
//! captured. When the scope is popped, focus is restored to that widget.
//!
//! If the prior focused widget was deleted during modal presentation,
//! the caller is responsible for falling back to a suitable target
//! (see [`FocusManager::pop_scope`](crate::FocusManager::pop_scope)).

use martensite_core::WidgetId;

/// A single entry on the focus scope stack, representing an active modal
/// dialog, popover, or context menu.
///
/// # Examples
///
/// ```
/// use martensite_focus::scope::FocusScope;
/// use martensite_core::WidgetId;
///
/// let root = WidgetId::from_parts(0, 1);
/// let prior = WidgetId::from_parts(1, 1);
/// let scope = FocusScope::new(root, Some(prior));
/// assert_eq!(scope.root(), root);
/// assert_eq!(scope.prior_focus(), Some(prior));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusScope {
    /// The root widget ID of the scope. Navigation is restricted to
    /// the subtree rooted at this widget.
    root: WidgetId,
    /// The widget that was focused before this scope was pushed.
    /// Used for restoration when the scope is popped.
    prior_focus: Option<WidgetId>,
}

impl FocusScope {
    /// Creates a new focus scope with the given root and prior focus.
    pub fn new(root: WidgetId, prior_focus: Option<WidgetId>) -> Self {
        Self { root, prior_focus }
    }

    /// Returns the root widget ID of this scope.
    #[inline]
    pub fn root(&self) -> WidgetId {
        self.root
    }

    /// Returns the widget that was focused before this scope was pushed.
    #[inline]
    pub fn prior_focus(&self) -> Option<WidgetId> {
        self.prior_focus
    }
}

/// A stack of modal focus scopes.
///
/// The stack supports nesting: multiple modal dialogs can be open
/// simultaneously, with each new scope trapping focus within its
/// subtree. When a scope is popped, focus returns to the prior scope
/// (or to no scope if the stack is empty).
///
/// # Examples
///
/// ```
/// use martensite_focus::scope::FocusScopeStack;
/// use martensite_core::WidgetId;
///
/// let mut stack = FocusScopeStack::new();
/// assert!(!stack.has_active_scope());
///
/// let root = WidgetId::from_parts(0, 1);
/// let prior = WidgetId::from_parts(1, 1);
/// stack.push(root, Some(prior));
/// assert!(stack.has_active_scope());
/// assert_eq!(stack.current_scope_root(), Some(root));
///
/// let restored = stack.pop();
/// assert_eq!(restored, Some(prior));
/// assert!(!stack.has_active_scope());
/// ```
pub struct FocusScopeStack {
    stack: Vec<FocusScope>,
}

impl Default for FocusScopeStack {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusScopeStack {
    /// Creates a new empty focus scope stack.
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Pushes a new focus scope onto the stack.
    ///
    /// `prior_focus` should be the currently focused widget ID, which
    /// will be restored when this scope is popped.
    pub fn push(&mut self, root: WidgetId, prior_focus: Option<WidgetId>) {
        self.stack.push(FocusScope::new(root, prior_focus));
    }

    /// Pops the top focus scope from the stack.
    ///
    /// Returns the prior focus that was captured when this scope was
    /// pushed, or `None` if the stack was empty.
    pub fn pop(&mut self) -> Option<WidgetId> {
        self.stack.pop().and_then(|scope| scope.prior_focus)
    }

    /// Returns the root widget ID of the current (top) scope, if any.
    pub fn current_scope_root(&self) -> Option<WidgetId> {
        self.stack.last().map(|scope| scope.root)
    }

    /// Returns a reference to the current (top) scope, if any.
    pub fn current_scope(&self) -> Option<&FocusScope> {
        self.stack.last()
    }

    /// Returns `true` if there is at least one active scope on the stack.
    #[inline]
    pub fn has_active_scope(&self) -> bool {
        !self.stack.is_empty()
    }

    /// Returns the number of scopes on the stack.
    #[inline]
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// Returns `true` if the stack is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Returns an iterator over the scopes, from bottom to top.
    pub fn iter(&self) -> impl Iterator<Item = &FocusScope> {
        self.stack.iter()
    }

    /// Clears all scopes from the stack without restoring focus.
    ///
    /// This is useful when the entire modal stack is dismissed at once
    /// (e.g., when switching views). The caller is responsible for
    /// setting focus to an appropriate widget afterwards.
    pub fn clear(&mut self) {
        self.stack.clear();
    }

    /// Returns `true` if the given widget ID is the root of the current
    /// (top) scope.
    ///
    /// This only checks equality with the scope root. For a full subtree
    /// membership test, walk the arena hierarchy via
    /// `WidgetArena::parent` or use `WidgetArena::iter_subtree`.
    pub fn is_current_scope_root(&self, id: WidgetId) -> bool {
        self.current_scope_root().is_some_and(|root| root == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stack_is_empty() {
        let stack = FocusScopeStack::new();
        assert!(stack.is_empty());
        assert!(!stack.has_active_scope());
        assert_eq!(stack.len(), 0);
    }

    #[test]
    fn default_stack_is_empty() {
        let stack = FocusScopeStack::default();
        assert!(stack.is_empty());
    }

    #[test]
    fn push_and_pop_single_scope() {
        let mut stack = FocusScopeStack::new();
        let root = WidgetId::from_parts(0, 1);
        let prior = WidgetId::from_parts(1, 1);

        stack.push(root, Some(prior));
        assert!(stack.has_active_scope());
        assert_eq!(stack.len(), 1);
        assert_eq!(stack.current_scope_root(), Some(root));

        let restored = stack.pop();
        assert_eq!(restored, Some(prior));
        assert!(!stack.has_active_scope());
    }

    #[test]
    fn push_with_no_prior_focus() {
        let mut stack = FocusScopeStack::new();
        let root = WidgetId::from_parts(0, 1);

        stack.push(root, None);
        assert!(stack.has_active_scope());

        let restored = stack.pop();
        assert_eq!(restored, None);
    }

    #[test]
    fn nested_scopes_pop_in_reverse_order() {
        let mut stack = FocusScopeStack::new();
        let root1 = WidgetId::from_parts(0, 1);
        let root2 = WidgetId::from_parts(1, 1);
        let prior1 = WidgetId::from_parts(2, 1);
        let prior2 = WidgetId::from_parts(3, 1);

        stack.push(root1, Some(prior1));
        stack.push(root2, Some(prior2));

        assert_eq!(stack.len(), 2);
        assert_eq!(stack.current_scope_root(), Some(root2));

        // Pop inner scope — should restore prior2.
        let restored = stack.pop();
        assert_eq!(restored, Some(prior2));
        assert_eq!(stack.current_scope_root(), Some(root1));

        // Pop outer scope — should restore prior1.
        let restored = stack.pop();
        assert_eq!(restored, Some(prior1));
        assert!(!stack.has_active_scope());
    }

    #[test]
    fn pop_empty_stack_returns_none() {
        let mut stack = FocusScopeStack::new();
        assert_eq!(stack.pop(), None);
    }

    #[test]
    fn current_scope_returns_top() {
        let mut stack = FocusScopeStack::new();
        let root1 = WidgetId::from_parts(0, 1);
        let root2 = WidgetId::from_parts(1, 1);

        stack.push(root1, None);
        assert_eq!(stack.current_scope().unwrap().root(), root1);

        stack.push(root2, None);
        assert_eq!(stack.current_scope().unwrap().root(), root2);
    }

    #[test]
    fn clear_removes_all_scopes() {
        let mut stack = FocusScopeStack::new();
        stack.push(WidgetId::from_parts(0, 1), None);
        stack.push(WidgetId::from_parts(1, 1), None);
        assert_eq!(stack.len(), 2);

        stack.clear();
        assert!(stack.is_empty());
        assert!(!stack.has_active_scope());
    }

    #[test]
    fn iter_returns_bottom_to_top() {
        let mut stack = FocusScopeStack::new();
        let root1 = WidgetId::from_parts(0, 1);
        let root2 = WidgetId::from_parts(1, 1);
        let root3 = WidgetId::from_parts(2, 1);

        stack.push(root1, None);
        stack.push(root2, None);
        stack.push(root3, None);

        let roots: Vec<WidgetId> = stack.iter().map(|s| s.root()).collect();
        assert_eq!(roots, vec![root1, root2, root3]);
    }

    #[test]
    fn is_current_scope_root_check() {
        let mut stack = FocusScopeStack::new();
        let root = WidgetId::from_parts(0, 1);
        let other = WidgetId::from_parts(1, 1);

        stack.push(root, None);
        assert!(stack.is_current_scope_root(root));
        assert!(!stack.is_current_scope_root(other));
    }

    #[test]
    fn is_current_scope_root_when_empty() {
        let stack = FocusScopeStack::new();
        let id = WidgetId::from_parts(0, 1);
        assert!(!stack.is_current_scope_root(id));
    }

    #[test]
    fn focus_scope_new_and_accessors() {
        let root = WidgetId::from_parts(0, 1);
        let prior = WidgetId::from_parts(1, 1);
        let scope = FocusScope::new(root, Some(prior));
        assert_eq!(scope.root(), root);
        assert_eq!(scope.prior_focus(), Some(prior));
    }

    #[test]
    fn focus_scope_with_no_prior() {
        let root = WidgetId::from_parts(0, 1);
        let scope = FocusScope::new(root, None);
        assert_eq!(scope.root(), root);
        assert_eq!(scope.prior_focus(), None);
    }

    #[test]
    fn focus_scope_equality() {
        let root = WidgetId::from_parts(0, 1);
        let prior = WidgetId::from_parts(1, 1);
        let s1 = FocusScope::new(root, Some(prior));
        let s2 = FocusScope::new(root, Some(prior));
        assert_eq!(s1, s2);
    }
}
