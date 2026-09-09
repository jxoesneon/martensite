//! Focus manager: active focus tracking and tab navigation ordering.
//!
//! The [`FocusManager`] tracks which widget currently holds keyboard
//! focus and provides tab navigation (forward and reverse) through the
//! focusable widgets in the arena.

use martensite_core::{NodeFlags, WidgetArena, WidgetId};

use crate::scope::FocusScopeStack;
use crate::spatial::{FocusDirection, SpatialNavigator};

/// Tab navigation direction.
///
/// # Examples
///
/// ```
/// use martensite_focus::TabNavigation;
///
/// assert_ne!(TabNavigation::Forward, TabNavigation::Reverse);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TabNavigation {
    /// Forward tab navigation (Tab key).
    Forward,
    /// Reverse tab navigation (Shift+Tab key).
    Reverse,
}

/// Tracks and manages the currently focused widget within the widget arena.
///
/// The focus manager integrates with the widget arena's
/// [`NodeFlags::FOCUSABLE`] flag to determine which widgets can receive
/// focus, and with the [`FocusScopeStack`] for modal focus trapping.
///
/// # Examples
///
/// ```
/// use martensite_focus::FocusManager;
/// use martensite_core::{WidgetArena, HotNode, ColdNode, NodeFlags, Rect};
///
/// let mut arena = WidgetArena::new();
/// let mut hot = HotNode::default();
/// hot.flags |= NodeFlags::FOCUSABLE | NodeFlags::VISIBLE;
/// let id = arena.insert(hot, ColdNode::default());
///
/// let mut manager = FocusManager::new();
/// manager.set_focus(&mut arena, id);
/// assert_eq!(manager.current_focus(), Some(id));
/// ```
pub struct FocusManager {
    /// The widget currently holding focus, if any.
    current_focus: Option<WidgetId>,
    /// The spatial navigator for directional (arrow key) navigation.
    navigator: SpatialNavigator,
    /// The modal focus scope stack.
    scopes: FocusScopeStack,
    /// The root container of the widget tree, used as the final fallback
    /// when a modal scope is dismissed and no other focus target is
    /// available. Set via [`FocusManager::set_root`].
    root: Option<WidgetId>,
}

impl Default for FocusManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusManager {
    /// Creates a new `FocusManager` with no widget currently focused.
    pub fn new() -> Self {
        Self {
            current_focus: None,
            navigator: SpatialNavigator::new(),
            scopes: FocusScopeStack::new(),
            root: None,
        }
    }

    /// Sets the root container used as the final focus fallback when a
    /// modal scope is dismissed and no other target (prior focus, sibling,
    /// or in-scope focusable) is available.
    ///
    /// When [`Self::pop_scope`] exhausts all other fallbacks, it restores
    /// focus to this root container (if it is a valid focus target) or to
    /// the first focusable descendant of the root, rather than to an
    /// arbitrary arena-first node.
    pub fn set_root(&mut self, root: WidgetId) {
        self.root = Some(root);
    }

    /// Returns the root container, if set.
    pub fn root(&self) -> Option<WidgetId> {
        self.root
    }

    /// Returns the currently focused widget ID, if any.
    #[inline]
    pub fn current_focus(&self) -> Option<WidgetId> {
        self.current_focus
    }

    /// Sets focus to the given widget.
    ///
    /// The widget must be alive, visible, focusable, and not inert. If a
    /// modal scope is active, the widget must also be within the active
    /// scope's subtree; otherwise focus is not changed (modal trapping).
    pub fn set_focus(&mut self, arena: &mut WidgetArena, id: WidgetId) {
        if !self.is_focusable_target(arena, id) {
            return;
        }
        self.current_focus = Some(id);
    }

    /// Returns `true` if `id` is a valid focus target: alive, visible,
    /// focusable, not inert, and within the active scope (if any).
    fn is_focusable_target(&self, arena: &WidgetArena, id: WidgetId) -> bool {
        let Some(hot) = arena.get_hot(id) else {
            return false;
        };
        if !hot.flags.contains(NodeFlags::FOCUSABLE) {
            return false;
        }
        if !hot.flags.contains(NodeFlags::VISIBLE) {
            return false;
        }
        if hot.flags.contains(NodeFlags::INERT) {
            return false;
        }
        if let Some(scope_root) = self.scopes.current_scope_root() {
            if !self.is_in_subtree(arena, scope_root, id) {
                return false;
            }
        }
        true
    }

    /// Returns `true` if `id` is within the subtree rooted at `root`
    /// (inclusive of `root` itself).
    fn is_in_subtree(&self, arena: &WidgetArena, root: WidgetId, id: WidgetId) -> bool {
        if root == id {
            return true;
        }
        arena.iter_subtree(root).any(|w| w == id)
    }

    /// Sets focus to the given widget unconditionally, bypassing the
    /// focusable check.
    ///
    /// This is useful for programmatic focus assignment (e.g., from
    /// accessibility actions) where the caller has already verified
    /// the target is valid.
    pub fn set_focus_unchecked(&mut self, id: WidgetId) {
        self.current_focus = Some(id);
    }

    /// Clears the current focus.
    pub fn clear_focus(&mut self) {
        self.current_focus = None;
    }

    /// Advances focus in the given tab navigation direction.
    ///
    /// If a modal scope is active, tab navigation is restricted to
    /// focusable widgets within the scope. If no scope is active,
    /// all focusable widgets in the arena are considered.
    ///
    /// Returns the newly focused widget ID, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_focus::{FocusManager, TabNavigation};
    /// use martensite_core::{ColdNode, HotNode, NodeFlags, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let make_focusable = |arena: &mut WidgetArena| {
    ///     arena.insert(
    ///         HotNode {
    ///             flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE,
    ///             ..HotNode::default()
    ///         },
    ///         ColdNode::default(),
    ///     )
    /// };
    /// let a = make_focusable(&mut arena);
    /// let b = make_focusable(&mut arena);
    /// let c = make_focusable(&mut arena);
    ///
    /// let mut manager = FocusManager::new();
    /// // Forward tab cycles a -> b -> c -> a.
    /// assert_eq!(manager.tab(&arena, TabNavigation::Forward), Some(a));
    /// assert_eq!(manager.tab(&arena, TabNavigation::Forward), Some(b));
    /// assert_eq!(manager.tab(&arena, TabNavigation::Forward), Some(c));
    /// assert_eq!(manager.tab(&arena, TabNavigation::Forward), Some(a));
    /// // Reverse tab wraps back to c.
    /// assert_eq!(manager.tab(&arena, TabNavigation::Reverse), Some(c));
    /// ```
    pub fn tab(&mut self, arena: &WidgetArena, direction: TabNavigation) -> Option<WidgetId> {
        let candidates = self.collect_tab_candidates(arena);
        if candidates.is_empty() {
            return None;
        }

        let next = match (self.current_focus, direction) {
            (Some(current), TabNavigation::Forward) => {
                // Find the current position and advance.
                let idx = candidates.iter().position(|&id| id == current);
                match idx {
                    Some(i) => candidates[(i + 1) % candidates.len()],
                    None => candidates[0],
                }
            }
            (Some(current), TabNavigation::Reverse) => {
                let idx = candidates.iter().position(|&id| id == current);
                match idx {
                    Some(0) => *candidates.last().unwrap(),
                    Some(i) => candidates[i - 1],
                    // Current focus is not a candidate (invisible, inert,
                    // or outside scope): start from the last candidate.
                    None => *candidates.last().unwrap(),
                }
            }
            (None, _) => candidates[0],
        };

        self.current_focus = Some(next);
        Some(next)
    }

    /// Navigates focus in the given spatial direction (arrow keys).
    ///
    /// If a modal scope is active, spatial navigation is restricted to
    /// focusable widgets within the scope. If the current focus is
    /// outside the active scope, navigation starts from the scope root
    /// instead, ensuring focus cannot drift into the modal from outside.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_focus::{FocusDirection, FocusManager};
    /// use martensite_core::{ColdNode, HotNode, NodeFlags, Rect, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let src = arena.insert(
    ///     HotNode {
    ///         bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
    ///         flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE,
    ///         ..HotNode::default()
    ///     },
    ///     ColdNode::default(),
    /// );
    /// let right = arena.insert(
    ///     HotNode {
    ///         bounds: Rect::new(100.0, 0.0, 10.0, 10.0),
    ///         flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE,
    ///         ..HotNode::default()
    ///     },
    ///     ColdNode::default(),
    /// );
    ///
    /// let mut manager = FocusManager::new();
    /// manager.set_focus(&mut arena, src);
    /// // Pressing "Right" moves focus to the node on the right.
    /// assert_eq!(manager.navigate(&arena, FocusDirection::Right), Some(right));
    /// assert_eq!(manager.current_focus(), Some(right));
    /// ```
    pub fn navigate(&mut self, arena: &WidgetArena, direction: FocusDirection) -> Option<WidgetId> {
        let source = self.current_focus?;

        // If a modal scope is active and the current focus is outside it,
        // clamp the source to the scope root to enforce trapping.
        let source = if let Some(scope) = self.scopes.current_scope_root() {
            if self.is_in_subtree(arena, scope, source) {
                source
            } else {
                scope
            }
        } else {
            source
        };

        let next = if let Some(scope) = self.scopes.current_scope_root() {
            self.navigator
                .navigate_within_scope(arena, source, direction, scope)
        } else {
            self.navigator.navigate(arena, source, direction)
        };

        if let Some(next) = next {
            self.current_focus = Some(next);
        }
        next
    }

    /// Pushes a new modal focus scope onto the stack.
    ///
    /// The current focus is captured for later restoration. Navigation
    /// (tab and spatial) will be restricted to widgets within `scope_root`.
    ///
    /// Returns `false` if `scope_root` is not alive in the arena, in which
    /// case the scope is not pushed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_focus::FocusManager;
    /// use martensite_core::{ColdNode, HotNode, NodeFlags, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let scope_root = arena.insert(
    ///     HotNode {
    ///         flags: NodeFlags::VISIBLE,
    ///         ..HotNode::default()
    ///     },
    ///     ColdNode::default(),
    /// );
    ///
    /// let mut manager = FocusManager::new();
    /// assert!(!manager.has_active_scope());
    ///
    /// // Pushing a live scope root activates modal trapping.
    /// assert!(manager.push_scope(&arena, scope_root));
    /// assert!(manager.has_active_scope());
    ///
    /// // Pushing a fabricated (dead) id is rejected.
    /// use martensite_core::WidgetId;
    /// assert!(!manager.push_scope(&arena, WidgetId::from_parts(99, 99)));
    /// ```
    pub fn push_scope(&mut self, arena: &WidgetArena, scope_root: WidgetId) -> bool {
        if !arena.is_alive(scope_root) {
            return false;
        }
        self.scopes.push(scope_root, self.current_focus);
        true
    }

    /// Pops the top modal focus scope from the stack.
    ///
    /// Focus is restored to the widget that was focused before the scope
    /// was pushed. If the restored widget is no longer alive, visible, or
    /// focusable, focus falls back to the nearest visible focusable
    /// sibling of the prior node, then to the nearest visible focusable
    /// widget within the remaining active scope (if any), then to the
    /// root container (or its first focusable descendant), and finally
    /// to the first visible focusable widget in the arena.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_focus::FocusManager;
    /// use martensite_core::{ColdNode, HotNode, NodeFlags, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let prior = arena.insert(
    ///     HotNode {
    ///         flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE,
    ///         ..HotNode::default()
    ///     },
    ///     ColdNode::default(),
    /// );
    /// let scope_root = arena.insert(
    ///     HotNode {
    ///         flags: NodeFlags::VISIBLE,
    ///         ..HotNode::default()
    ///     },
    ///     ColdNode::default(),
    /// );
    ///
    /// let mut manager = FocusManager::new();
    /// manager.set_focus(&mut arena, prior);
    /// manager.push_scope(&arena, scope_root);
    /// assert!(manager.has_active_scope());
    ///
    /// // Popping restores the focus captured when the scope was pushed.
    /// assert_eq!(manager.pop_scope(&arena), Some(prior));
    /// assert!(!manager.has_active_scope());
    /// ```
    pub fn pop_scope(&mut self, arena: &WidgetArena) -> Option<WidgetId> {
        let restored = self.scopes.pop();
        let Some(prior_focus) = restored else {
            // No prior focus captured — clear focus to avoid leaving it
            // inside a dismissed modal.
            self.current_focus = None;
            return None;
        };

        // Fallback 0: restore the prior focus if it is still a valid target.
        if self.is_focusable_target(arena, prior_focus) {
            self.current_focus = Some(prior_focus);
            return Some(prior_focus);
        }

        // Fallback 1: nearest visible focusable sibling of the prior node.
        if let Some(sibling) = self.nearest_focusable_sibling(arena, prior_focus) {
            self.current_focus = Some(sibling);
            return Some(sibling);
        }

        // Fallback 2: first visible focusable widget in the remaining
        // active scope (if any).
        if let Some(scope) = self.scopes.current_scope_root() {
            if let Some(fallback) = arena
                .iter_subtree(scope)
                .find(|id| self.is_focusable_target(arena, *id))
            {
                self.current_focus = Some(fallback);
                return Some(fallback);
            }
        }

        // Fallback 3: the root container. If the root itself is a valid
        // focus target, restore focus to it; otherwise restore to the
        // first focusable descendant of the root. This ensures a
        // dismissed modal returns focus to the application's root rather
        // than to an arbitrary arena-first node.
        if let Some(root) = self.root {
            if self.is_focusable_target(arena, root) {
                self.current_focus = Some(root);
                return Some(root);
            }
            if let Some(fallback) = arena
                .iter_subtree(root)
                .find(|id| self.is_focusable_target(arena, *id))
            {
                self.current_focus = Some(fallback);
                return Some(fallback);
            }
        }

        // Fallback 4: first visible focusable widget in the whole arena.
        let fallback = arena
            .iter_depth_first()
            .find(|id| self.is_focusable_target(arena, *id));
        self.current_focus = fallback;
        fallback
    }

    /// Finds the nearest visible, focusable, non-inert sibling of `id`
    /// by walking the sibling chain. Returns `None` if `id` has no
    /// suitable sibling.
    fn nearest_focusable_sibling(&self, arena: &WidgetArena, id: WidgetId) -> Option<WidgetId> {
        // Walk forward through next siblings.
        let mut next = arena.next_sibling(id);
        while let Some(sibling) = next {
            if self.is_focusable_target(arena, sibling) {
                return Some(sibling);
            }
            next = arena.next_sibling(sibling);
        }

        // Walk backward through prev siblings.
        let mut prev = arena.prev_sibling(id);
        while let Some(sibling) = prev {
            if self.is_focusable_target(arena, sibling) {
                return Some(sibling);
            }
            prev = arena.prev_sibling(sibling);
        }

        None
    }

    /// Returns the current scope stack for inspection.
    pub fn scopes(&self) -> &FocusScopeStack {
        &self.scopes
    }

    /// Returns `true` if a modal focus scope is currently active.
    pub fn has_active_scope(&self) -> bool {
        self.scopes.has_active_scope()
    }

    /// Collects the list of focusable widget IDs in tab order
    /// (depth-first tree order), filtered by the active scope if any.
    fn collect_tab_candidates(&self, arena: &WidgetArena) -> Vec<WidgetId> {
        let mut candidates = Vec::new();

        let mut collect = |id: WidgetId| {
            if let Some(hot) = arena.get_hot(id) {
                if hot.flags.contains(NodeFlags::FOCUSABLE)
                    && hot.flags.contains(NodeFlags::VISIBLE)
                    && !hot.flags.contains(NodeFlags::INERT)
                {
                    candidates.push(id);
                }
            }
        };

        if let Some(scope) = self.scopes.current_scope_root() {
            for id in arena.iter_subtree(scope) {
                collect(id);
            }
        } else {
            for id in arena.iter_depth_first() {
                collect(id);
            }
        }

        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spatial::FocusDirection;
    use martensite_core::{ColdNode, HotNode, NodeFlags, Rect, WidgetArena};

    fn make_focusable(arena: &mut WidgetArena) -> WidgetId {
        let hot = HotNode {
            flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE,
            ..Default::default()
        };
        arena.insert(hot, ColdNode::default())
    }

    fn make_focusable_at(arena: &mut WidgetArena, bounds: Rect) -> WidgetId {
        let hot = HotNode {
            bounds,
            flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE,
            ..Default::default()
        };
        arena.insert(hot, ColdNode::default())
    }

    #[test]
    fn new_has_no_focus() {
        let manager = FocusManager::new();
        assert!(manager.current_focus().is_none());
    }

    #[test]
    fn default_has_no_focus() {
        let manager = FocusManager::default();
        assert!(manager.current_focus().is_none());
    }

    #[test]
    fn set_focus_on_focusable_widget() {
        let mut arena = WidgetArena::new();
        let id = make_focusable(&mut arena);
        let mut manager = FocusManager::new();
        manager.set_focus(&mut arena, id);
        assert_eq!(manager.current_focus(), Some(id));
    }

    #[test]
    fn set_focus_on_non_focusable_widget_does_nothing() {
        let mut arena = WidgetArena::new();
        let id = arena.insert(HotNode::default(), ColdNode::default());
        let mut manager = FocusManager::new();
        manager.set_focus(&mut arena, id);
        assert_eq!(manager.current_focus(), None);
    }

    #[test]
    fn set_focus_on_dead_widget_does_nothing() {
        let mut arena = WidgetArena::new();
        let id = make_focusable(&mut arena);
        arena.remove(id);
        let mut manager = FocusManager::new();
        manager.set_focus(&mut arena, id);
        assert_eq!(manager.current_focus(), None);
    }

    #[test]
    fn clear_focus() {
        let mut arena = WidgetArena::new();
        let id = make_focusable(&mut arena);
        let mut manager = FocusManager::new();
        manager.set_focus(&mut arena, id);
        manager.clear_focus();
        assert_eq!(manager.current_focus(), None);
    }

    #[test]
    fn set_focus_unchecked() {
        let mut manager = FocusManager::new();
        let id = WidgetId::from_parts(0, 1);
        manager.set_focus_unchecked(id);
        assert_eq!(manager.current_focus(), Some(id));
    }

    #[test]
    fn tab_forward_cycles_through_candidates() {
        let mut arena = WidgetArena::new();
        let a = make_focusable(&mut arena);
        let b = make_focusable(&mut arena);
        let c = make_focusable(&mut arena);

        let mut manager = FocusManager::new();

        // No focus — tab should focus the first candidate.
        let next = manager.tab(&arena, TabNavigation::Forward);
        assert_eq!(next, Some(a));
        assert_eq!(manager.current_focus(), Some(a));

        // Tab again — should go to b.
        let next = manager.tab(&arena, TabNavigation::Forward);
        assert_eq!(next, Some(b));

        // Tab again — should go to c.
        let next = manager.tab(&arena, TabNavigation::Forward);
        assert_eq!(next, Some(c));

        // Tab again — should wrap to a.
        let next = manager.tab(&arena, TabNavigation::Forward);
        assert_eq!(next, Some(a));
    }

    #[test]
    fn tab_reverse_cycles_through_candidates() {
        let mut arena = WidgetArena::new();
        let a = make_focusable(&mut arena);
        let b = make_focusable(&mut arena);
        let c = make_focusable(&mut arena);

        let mut manager = FocusManager::new();
        manager.set_focus(&mut arena, c);

        // Reverse tab from c — should go to b.
        let next = manager.tab(&arena, TabNavigation::Reverse);
        assert_eq!(next, Some(b));

        // Reverse tab from b — should go to a.
        let next = manager.tab(&arena, TabNavigation::Reverse);
        assert_eq!(next, Some(a));

        // Reverse tab from a — should wrap to c.
        let next = manager.tab(&arena, TabNavigation::Reverse);
        assert_eq!(next, Some(c));
    }

    #[test]
    fn tab_with_no_candidates_returns_none() {
        let arena = WidgetArena::new();
        let mut manager = FocusManager::new();
        let next = manager.tab(&arena, TabNavigation::Forward);
        assert_eq!(next, None);
    }

    #[test]
    fn tab_skips_non_focusable() {
        let mut arena = WidgetArena::new();
        let a = make_focusable(&mut arena);
        let _non_focus = arena.insert(HotNode::default(), ColdNode::default());
        let b = make_focusable(&mut arena);

        let mut manager = FocusManager::new();
        let next = manager.tab(&arena, TabNavigation::Forward);
        assert_eq!(next, Some(a));
        let next = manager.tab(&arena, TabNavigation::Forward);
        assert_eq!(next, Some(b));
    }

    #[test]
    fn tab_skips_invisible() {
        let mut arena = WidgetArena::new();
        let a = make_focusable(&mut arena);
        let hot = HotNode {
            flags: NodeFlags::FOCUSABLE, // Not VISIBLE
            ..Default::default()
        };
        let _invisible = arena.insert(hot, ColdNode::default());
        let b = make_focusable(&mut arena);

        let mut manager = FocusManager::new();
        let next = manager.tab(&arena, TabNavigation::Forward);
        assert_eq!(next, Some(a));
        let next = manager.tab(&arena, TabNavigation::Forward);
        assert_eq!(next, Some(b));
    }

    #[test]
    fn navigate_uses_spatial_navigator() {
        let mut arena = WidgetArena::new();
        let src = make_focusable_at(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        let right = make_focusable_at(&mut arena, Rect::new(100.0, 0.0, 10.0, 10.0));

        let mut manager = FocusManager::new();
        manager.set_focus(&mut arena, src);

        let next = manager.navigate(&arena, FocusDirection::Right);
        assert_eq!(next, Some(right));
        assert_eq!(manager.current_focus(), Some(right));
    }

    #[test]
    fn navigate_with_no_current_focus_returns_none() {
        let mut arena = WidgetArena::new();
        let _a = make_focusable_at(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        let _b = make_focusable_at(&mut arena, Rect::new(100.0, 0.0, 10.0, 10.0));

        let mut manager = FocusManager::new();
        let next = manager.navigate(&arena, FocusDirection::Right);
        assert_eq!(next, None);
    }

    #[test]
    fn push_scope_restricts_tab_navigation() {
        let mut arena = WidgetArena::new();
        let _outside = make_focusable(&mut arena);
        let scope_root = make_focusable(&mut arena);
        let inside = make_focusable(&mut arena);
        arena.append_child(scope_root, inside).unwrap();

        let mut manager = FocusManager::new();
        assert!(manager.push_scope(&arena, scope_root));

        // Tab should only cycle within the scope.
        let next = manager.tab(&arena, TabNavigation::Forward);
        assert_eq!(next, Some(scope_root));
        let next = manager.tab(&arena, TabNavigation::Forward);
        assert_eq!(next, Some(inside));
        // Should wrap back to scope_root, not escape to outside.
        let next = manager.tab(&arena, TabNavigation::Forward);
        assert_eq!(next, Some(scope_root));
    }

    #[test]
    fn pop_scope_restores_focus() {
        let mut arena = WidgetArena::new();
        let prior = make_focusable(&mut arena);
        let scope_root = make_focusable(&mut arena);

        let mut manager = FocusManager::new();
        manager.set_focus(&mut arena, prior);
        assert!(manager.push_scope(&arena, scope_root));

        // Focus is now trapped in scope.
        manager.set_focus(&mut arena, scope_root);

        // Pop scope — should restore focus to prior.
        let restored = manager.pop_scope(&arena);
        assert_eq!(restored, Some(prior));
        assert_eq!(manager.current_focus(), Some(prior));
    }

    #[test]
    fn pop_scope_fallback_when_prior_is_dead() {
        let mut arena = WidgetArena::new();
        let prior = make_focusable(&mut arena);
        let scope_root = make_focusable(&mut arena);
        let _fallback = make_focusable(&mut arena);

        let mut manager = FocusManager::new();
        manager.set_focus(&mut arena, prior);
        assert!(manager.push_scope(&arena, scope_root));

        // Remove the prior focus target while the scope is active.
        arena.remove(prior);

        // Pop scope — prior is dead, should fall back to a focusable widget.
        let restored = manager.pop_scope(&arena);
        assert!(restored.is_some());
        assert_ne!(restored, Some(prior));
    }

    #[test]
    fn has_active_scope_reflects_stack_state() {
        let mut arena = WidgetArena::new();
        let scope_root = make_focusable(&mut arena);

        let mut manager = FocusManager::new();
        assert!(!manager.has_active_scope());

        assert!(manager.push_scope(&arena, scope_root));
        assert!(manager.has_active_scope());

        manager.pop_scope(&arena);
        assert!(!manager.has_active_scope());
    }

    #[test]
    fn tab_from_unknown_position_starts_at_first() {
        let mut arena = WidgetArena::new();
        let a = make_focusable(&mut arena);
        let _b = make_focusable(&mut arena);

        let mut manager = FocusManager::new();
        // Set focus to a non-candidate (non-focusable) widget.
        let non_focus = arena.insert(HotNode::default(), ColdNode::default());
        manager.set_focus_unchecked(non_focus);

        let next = manager.tab(&arena, TabNavigation::Forward);
        assert_eq!(next, Some(a));
    }

    #[test]
    fn tab_reverse_from_unknown_starts_at_last() {
        let mut arena = WidgetArena::new();
        let _a = make_focusable(&mut arena);
        let b = make_focusable(&mut arena);

        let mut manager = FocusManager::new();
        // Set focus to a non-candidate (non-focusable) widget.
        let non_focus = arena.insert(HotNode::default(), ColdNode::default());
        manager.set_focus_unchecked(non_focus);

        // Reverse tab from unknown position should start at the LAST candidate.
        let next = manager.tab(&arena, TabNavigation::Reverse);
        assert_eq!(next, Some(b));
    }

    #[test]
    fn set_focus_rejects_invisible_widget() {
        let mut arena = WidgetArena::new();
        let hot = HotNode {
            flags: NodeFlags::FOCUSABLE, // Not VISIBLE
            ..Default::default()
        };
        let id = arena.insert(hot, ColdNode::default());
        let mut manager = FocusManager::new();
        manager.set_focus(&mut arena, id);
        assert_eq!(manager.current_focus(), None);
    }

    #[test]
    fn set_focus_rejects_inert_widget() {
        let mut arena = WidgetArena::new();
        let hot = HotNode {
            flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE | NodeFlags::INERT,
            ..Default::default()
        };
        let id = arena.insert(hot, ColdNode::default());
        let mut manager = FocusManager::new();
        manager.set_focus(&mut arena, id);
        assert_eq!(manager.current_focus(), None);
    }

    #[test]
    fn set_focus_trapped_by_modal_scope() {
        let mut arena = WidgetArena::new();
        let outside = make_focusable(&mut arena);
        let scope_root = make_focusable(&mut arena);

        let mut manager = FocusManager::new();
        assert!(manager.push_scope(&arena, scope_root));

        // Attempting to set focus to a widget outside the scope should fail.
        manager.set_focus(&mut arena, outside);
        assert_eq!(manager.current_focus(), None);

        // Setting focus inside the scope should succeed.
        manager.set_focus(&mut arena, scope_root);
        assert_eq!(manager.current_focus(), Some(scope_root));
    }

    #[test]
    fn push_scope_rejects_dead_root() {
        let mut arena = WidgetArena::new();
        let dead = make_focusable(&mut arena);
        arena.remove(dead);

        let mut manager = FocusManager::new();
        assert!(!manager.push_scope(&arena, dead));
        assert!(!manager.has_active_scope());
    }

    #[test]
    fn tab_skips_inert_widgets() {
        let mut arena = WidgetArena::new();
        let a = make_focusable(&mut arena);
        let hot = HotNode {
            flags: NodeFlags::FOCUSABLE | NodeFlags::VISIBLE | NodeFlags::INERT,
            ..Default::default()
        };
        let _inert = arena.insert(hot, ColdNode::default());
        let b = make_focusable(&mut arena);

        let mut manager = FocusManager::new();
        let next = manager.tab(&arena, TabNavigation::Forward);
        assert_eq!(next, Some(a));
        let next = manager.tab(&arena, TabNavigation::Forward);
        // Should skip the inert widget and go to b.
        assert_eq!(next, Some(b));
    }

    #[test]
    fn pop_scope_clears_focus_when_no_prior() {
        let mut arena = WidgetArena::new();
        let scope_root = make_focusable(&mut arena);

        let mut manager = FocusManager::new();
        assert!(manager.push_scope(&arena, scope_root));
        manager.set_focus(&mut arena, scope_root);
        assert_eq!(manager.current_focus(), Some(scope_root));

        // Pop scope with no prior focus — should clear focus.
        let restored = manager.pop_scope(&arena);
        assert_eq!(restored, None);
        assert_eq!(manager.current_focus(), None);
    }

    #[test]
    fn pop_scope_fallback_to_nearest_sibling() {
        let mut arena = WidgetArena::new();
        let parent = arena.insert(HotNode::default(), ColdNode::default());
        let prior = make_focusable(&mut arena);
        let next_sibling = make_focusable(&mut arena);
        arena.append_child(parent, prior).unwrap();
        arena.append_child(parent, next_sibling).unwrap();
        let scope_root = make_focusable(&mut arena);

        let mut manager = FocusManager::new();
        manager.set_focus(&mut arena, prior);
        assert!(manager.push_scope(&arena, scope_root));

        // Remove the prior focus target while the scope is active.
        arena.remove(prior);

        // Pop scope — prior is dead, should fall back to nearest sibling.
        let restored = manager.pop_scope(&arena);
        assert_eq!(restored, Some(next_sibling));
    }

    #[test]
    fn navigate_trapped_inside_modal_scope() {
        let mut arena = WidgetArena::new();
        let outside = make_focusable_at(&mut arena, Rect::new(0.0, 0.0, 10.0, 10.0));
        let scope_root = make_focusable_at(&mut arena, Rect::new(100.0, 0.0, 10.0, 10.0));
        let inside = make_focusable_at(&mut arena, Rect::new(200.0, 0.0, 10.0, 10.0));
        arena.append_child(scope_root, inside).unwrap();

        let mut manager = FocusManager::new();
        manager.set_focus(&mut arena, outside);
        assert!(manager.push_scope(&arena, scope_root));

        // Navigate right — source is outside scope, so it should be clamped
        // to scope_root, and the result should be inside the scope.
        let next = manager.navigate(&arena, FocusDirection::Right);
        assert_eq!(next, Some(inside));
        // Focus should now be inside the scope.
        assert_eq!(manager.current_focus(), Some(inside));
    }

    #[test]
    fn pop_scope_falls_back_to_root_container() {
        // When the prior focus is dead, has no focusable siblings, and no
        // remaining scope is active, pop_scope should fall back to the root
        // container (set via set_root) rather than an arbitrary arena-first
        // node.
        let mut arena = WidgetArena::new();

        // Tree structure:
        //   root (focusable)
        //   ├── prior (focusable, will be removed)
        //   └── other (NOT focusable)
        let root = make_focusable(&mut arena);
        let prior = make_focusable(&mut arena);
        let _other = arena.insert(HotNode::default(), ColdNode::default()); // not focusable
        arena.append_child(root, prior).unwrap();
        arena.append_child(root, _other).unwrap();

        let mut manager = FocusManager::new();
        manager.set_root(root);
        manager.set_focus(&mut arena, prior);
        assert!(manager.push_scope(&arena, root));

        // Remove the prior focus target while the scope is active.
        arena.remove(prior);

        // Pop scope — prior is dead, no focusable siblings (other is not
        // focusable), no remaining scope. Should fall back to root.
        let restored = manager.pop_scope(&arena);
        assert_eq!(restored, Some(root));
        assert_eq!(manager.current_focus(), Some(root));
    }

    #[test]
    fn pop_scope_falls_back_to_root_descendant_when_root_not_focusable() {
        // When the root itself is not focusable, pop_scope should fall
        // back to the first focusable descendant of the root.
        let mut arena = WidgetArena::new();

        // Tree structure:
        //   root (NOT focusable)
        //   ├── prior (focusable, will be removed)
        //   └── child (focusable)
        let root = arena.insert(HotNode::default(), ColdNode::default()); // not focusable
        let prior = make_focusable(&mut arena);
        let child = make_focusable(&mut arena);
        arena.append_child(root, prior).unwrap();
        arena.append_child(root, child).unwrap();

        let mut manager = FocusManager::new();
        manager.set_root(root);
        manager.set_focus(&mut arena, prior);
        assert!(manager.push_scope(&arena, root));

        // Remove the prior focus target while the scope is active.
        arena.remove(prior);

        // Pop scope — prior is dead, no focusable siblings of prior (it was
        // removed), no remaining scope. Root is not focusable, so fall back
        // to the first focusable descendant of root, which is `child`.
        let restored = manager.pop_scope(&arena);
        assert_eq!(restored, Some(child));
        assert_eq!(manager.current_focus(), Some(child));
    }

    #[test]
    fn set_root_and_get_root() {
        let mut arena = WidgetArena::new();
        let root = make_focusable(&mut arena);
        let mut manager = FocusManager::new();
        assert!(manager.root().is_none());
        manager.set_root(root);
        assert_eq!(manager.root(), Some(root));
    }
}
