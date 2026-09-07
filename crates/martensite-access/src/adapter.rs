//! AccessKit adapter: builds incremental [`TreeUpdate`] payloads from the
//! widget arena.
//!
//! The adapter maintains a mapping from `WidgetId` to `NodeId` and tracks
//! which nodes are dirty (via `NodeFlags::DIRTY_A11Y`). On each call to
//! [`AccessKitAdapter::build_update`] or
//! [`AccessKitAdapter::build_incremental_update`], it walks the arena,
//! constructs `Node` payloads from the hot/cold data and the widget's
//! `accessibility` hook, and packages them into a `TreeUpdate`.
//!
//! ## Thread safety
//!
//! AccessKit updates are emitted synchronously following layout
//! finalization, preventing race conditions where screen readers query
//! stale coordinate boundaries. The adapter itself is not `Sync`; it is
//! intended to be used on the UI thread.

use accesskit::{Node, NodeId, TreeInfo, TreeUpdate};
use martensite_core::{NodeFlags, Rect as MartensiteRect, WidgetArena, WidgetId};

use crate::{node_id_to_widget_id, rect_to_accesskit, widget_id_to_node_id};

/// The AccessKit adapter that bridges the Martensite widget arena to the
/// platform accessibility subsystem.
///
/// It tracks the root node, the currently focused node, and the set of
/// dirty node IDs for incremental updates.
///
/// # Examples
///
/// ```
/// use martensite_access::AccessKitAdapter;
/// use martensite_core::WidgetArena;
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert(Default::default(), Default::default());
/// let mut adapter = AccessKitAdapter::new(root);
///
/// // Build a full tree update from the arena.
/// let update = adapter.build_update(&mut arena);
/// assert!(!update.nodes.is_empty());
/// ```
pub struct AccessKitAdapter {
    /// The root widget ID of the accessibility tree.
    root: WidgetId,
    /// The currently focused widget ID, if any.
    focus: Option<WidgetId>,
    /// The focus ID emitted in the last update, used to detect focus
    /// changes for incremental updates.
    last_emitted_focus: Option<WidgetId>,
    /// The AccessKit tree ID. Uses `TreeId::ROOT` for the main tree.
    tree_id: accesskit::TreeId,
    /// Toolkit name reported to the platform.
    toolkit_name: Option<String>,
    /// Toolkit version reported to the platform.
    toolkit_version: Option<String>,
}

impl AccessKitAdapter {
    /// Creates a new adapter with the given root widget ID.
    ///
    /// The root node must exist in the arena when [`build_update`] is called.
    ///
    /// [`build_update`]: Self::build_update
    pub fn new(root: WidgetId) -> Self {
        Self {
            root,
            focus: None,
            last_emitted_focus: None,
            tree_id: accesskit::TreeId::ROOT,
            toolkit_name: Some("Martensite".to_string()),
            toolkit_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }
    }

    /// Sets the toolkit name reported to the platform accessibility subsystem.
    pub fn set_toolkit_name(&mut self, name: impl Into<String>) {
        self.toolkit_name = Some(name.into());
    }

    /// Sets the toolkit version reported to the platform accessibility subsystem.
    pub fn set_toolkit_version(&mut self, version: impl Into<String>) {
        self.toolkit_version = Some(version.into());
    }

    /// Sets the currently focused widget ID.
    ///
    /// Pass `None` to clear focus. The focus will be reflected in the next
    /// `TreeUpdate` built by this adapter.
    pub fn set_focus(&mut self, focus: Option<WidgetId>) {
        self.focus = focus;
    }

    /// Returns the AccessKit tree ID used by this adapter.
    pub fn tree_id(&self) -> accesskit::TreeId {
        self.tree_id
    }

    /// Returns the currently focused widget ID, if any.
    pub fn focus(&self) -> Option<WidgetId> {
        self.focus
    }

    /// Returns the root widget ID of the accessibility tree.
    pub fn root(&self) -> WidgetId {
        self.root
    }

    /// Builds a full [`TreeUpdate`] from the entire widget arena.
    ///
    /// This walks all nodes in the subtree rooted at `self.root` in
    /// depth-first order, constructs an [`accesskit::Node`] for each, and
    /// includes the complete tree structure. Use this for the initial
    /// tree submission or when a large portion of the tree has changed.
    ///
    /// After this call, the `DIRTY_A11Y` flags on all emitted nodes are
    /// cleared and `last_emitted_focus` is updated.
    pub fn build_update(&mut self, arena: &mut WidgetArena) -> TreeUpdate {
        let mut nodes = Vec::new();

        for widget_id in arena.iter_subtree(self.root) {
            let Some((hot, cold)) = arena.get_both(widget_id) else {
                continue;
            };
            let node_id = widget_id_to_node_id(widget_id);
            let mut node = self.build_node(widget_id, hot.bounds, cold, arena);
            // Set children from arena topology.
            let children: Vec<NodeId> = arena
                .children(widget_id)
                .map(widget_id_to_node_id)
                .collect();
            if !children.is_empty() {
                node.set_children(children);
            }
            nodes.push((node_id, node));
        }

        let focus_id = self
            .focus
            .filter(|id| arena.is_alive(*id))
            .map(widget_id_to_node_id)
            .unwrap_or_else(|| widget_id_to_node_id(self.root));

        // Clear dirty flags on all emitted nodes.
        let dirty_ids: Vec<WidgetId> = arena.iter_subtree(self.root).collect();
        for widget_id in dirty_ids {
            if let Some(hot) = arena.get_hot_mut(widget_id) {
                hot.flags.remove(NodeFlags::DIRTY_A11Y);
            }
        }

        self.last_emitted_focus = self.focus;

        let tree = TreeInfo {
            root: widget_id_to_node_id(self.root),
            toolkit_name: self.toolkit_name.clone(),
            toolkit_version: self.toolkit_version.clone(),
        };

        TreeUpdate {
            nodes,
            tree: Some(tree),
            tree_id: self.tree_id,
            focus: focus_id,
        }
    }

    /// Builds an incremental [`TreeUpdate`] containing only dirty nodes
    /// and their parents.
    ///
    /// A node is considered dirty if its [`NodeFlags::DIRTY_A11Y`] flag
    /// is set. When a node is dirty, its parent is also included in the
    /// update so that AccessKit receives an updated `children` list —
    /// this is required for correct removal and reparenting semantics.
    ///
    /// If focus has changed since the last update, the new focus node is
    /// also included. If no nodes are dirty and focus hasn't changed,
    /// returns `None`.
    ///
    /// After this call, the `DIRTY_A11Y` flags on emitted nodes are
    /// cleared and `last_emitted_focus` is updated.
    pub fn build_incremental_update(&mut self, arena: &mut WidgetArena) -> Option<TreeUpdate> {
        let focus_changed = self.focus != self.last_emitted_focus;

        // Collect the set of widget IDs to emit: dirty nodes, their
        // ancestors, and the focused node (if focus changed).
        let mut to_emit: std::collections::HashSet<WidgetId> = std::collections::HashSet::new();

        for widget_id in arena.iter_subtree(self.root) {
            let Some(hot) = arena.get_hot(widget_id) else {
                continue;
            };
            if hot.flags.contains(NodeFlags::DIRTY_A11Y) {
                to_emit.insert(widget_id);
                // Also emit all ancestors so their children lists are updated.
                let mut ancestor = arena.parent(widget_id);
                while let Some(parent_id) = ancestor {
                    to_emit.insert(parent_id);
                    ancestor = arena.parent(parent_id);
                }
            }
        }

        // Include the focused node if focus changed.
        if focus_changed {
            if let Some(focus_id) = self.focus {
                if arena.is_alive(focus_id) {
                    to_emit.insert(focus_id);
                }
            }
        }

        if to_emit.is_empty() && !focus_changed {
            return None;
        }

        let mut nodes = Vec::new();
        for widget_id in arena.iter_subtree(self.root) {
            if !to_emit.contains(&widget_id) {
                continue;
            }
            let Some((hot, cold)) = arena.get_both(widget_id) else {
                continue;
            };
            let node_id = widget_id_to_node_id(widget_id);
            let mut node = self.build_node(widget_id, hot.bounds, cold, arena);
            let children: Vec<NodeId> = arena
                .children(widget_id)
                .map(widget_id_to_node_id)
                .collect();
            if !children.is_empty() {
                node.set_children(children);
            }
            nodes.push((node_id, node));
        }

        // Clear dirty flags on emitted nodes.
        for widget_id in &to_emit {
            if let Some(hot) = arena.get_hot_mut(*widget_id) {
                hot.flags.remove(NodeFlags::DIRTY_A11Y);
            }
        }

        let focus_id = self
            .focus
            .filter(|id| arena.is_alive(*id))
            .map(widget_id_to_node_id)
            .unwrap_or_else(|| widget_id_to_node_id(self.root));

        self.last_emitted_focus = self.focus;

        Some(TreeUpdate {
            nodes,
            tree: None, // No tree info on incremental updates
            tree_id: self.tree_id,
            focus: focus_id,
        })
    }

    /// Builds a single [`accesskit::Node`] from hot/cold data and the
    /// widget's accessibility hook.
    fn build_node(
        &self,
        widget_id: WidgetId,
        bounds: MartensiteRect,
        cold: &martensite_core::ColdNode,
        arena: &WidgetArena,
    ) -> Node {
        let mut node = Node::new(cold.a11y_role);

        // Set bounds in window coordinates.
        if bounds.width() > 0.0 && bounds.height() > 0.0 {
            node.set_bounds(rect_to_accesskit(bounds));
        }

        // Set accessible name from cold node.
        if let Some(ref name) = cold.a11y_name {
            if !name.is_empty() {
                node.set_label(name.as_str());
            }
        }

        // Set tooltip if present.
        if let Some(ref tooltip) = cold.tooltip {
            node.set_tooltip(tooltip.as_str());
        }

        // Read hot node flags for focusability/visibility state.
        let hot_flags = arena
            .get_hot(widget_id)
            .map(|h| h.flags)
            .unwrap_or(NodeFlags::empty());

        // Add Focus action if the node is focusable and visible.
        // Hidden nodes should not advertise focus to assistive technologies.
        if hot_flags.contains(NodeFlags::FOCUSABLE) && hot_flags.contains(NodeFlags::VISIBLE) {
            node.add_action(accesskit::Action::Focus);
        }

        // Set disabled state if the node is inert.
        if hot_flags.contains(NodeFlags::INERT) {
            node.set_disabled();
        }

        // Set hidden state if not visible.
        if !hot_flags.contains(NodeFlags::VISIBLE) {
            node.set_hidden();
        }

        // Let the widget customize the node with role-specific properties.
        cold.widget.accessibility(&mut node);

        node
    }

    /// Marks a widget as dirty for the next incremental update.
    ///
    /// This sets the [`NodeFlags::DIRTY_A11Y`] flag on the widget's hot node.
    pub fn mark_dirty(&self, arena: &mut WidgetArena, id: WidgetId) {
        if let Some(hot) = arena.get_hot_mut(id) {
            hot.flags |= NodeFlags::DIRTY_A11Y;
        }
    }

    /// Clears the dirty flag on a widget after an update has been built.
    pub fn clear_dirty(&self, arena: &mut WidgetArena, id: WidgetId) {
        if let Some(hot) = arena.get_hot_mut(id) {
            hot.flags.remove(NodeFlags::DIRTY_A11Y);
        }
    }

    /// Clears the dirty flag on all nodes in the arena.
    pub fn clear_all_dirty(&self, arena: &mut WidgetArena) {
        let ids: Vec<WidgetId> = arena.iter_depth_first().collect();
        for widget_id in ids {
            if let Some(hot) = arena.get_hot_mut(widget_id) {
                hot.flags.remove(NodeFlags::DIRTY_A11Y);
            }
        }
    }

    /// Resolves an incoming AccessKit [`NodeId`] back to a [`WidgetId`]
    /// and verifies the widget is still alive in the arena.
    pub fn resolve(&self, arena: &WidgetArena, node_id: NodeId) -> Option<WidgetId> {
        let widget_id = node_id_to_widget_id(node_id)?;
        if arena.is_alive(widget_id) {
            Some(widget_id)
        } else {
            None
        }
    }

    /// Decodes an incoming AccessKit `ActionRequest` into an
    /// [`A11yAction`](crate::actions::A11yAction), validating that the
    /// request targets this adapter's tree.
    pub fn decode_action(
        &self,
        arena: &WidgetArena,
        request: &accesskit::ActionRequest,
    ) -> Option<crate::actions::A11yAction> {
        crate::actions::decode_action_request(arena, request, &self.tree_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{ColdNode, HotNode, NodeFlags, WidgetArena};

    fn make_arena_with_root() -> (WidgetArena, WidgetId) {
        let mut arena = WidgetArena::new();
        let root = arena.insert(HotNode::default(), ColdNode::default());
        (arena, root)
    }

    #[test]
    fn build_update_empty_arena_has_root() {
        let (mut arena, root) = make_arena_with_root();
        let mut adapter = AccessKitAdapter::new(root);
        let update = adapter.build_update(&mut arena);
        assert!(!update.nodes.is_empty());
        assert_eq!(update.nodes.len(), 1);
        assert_eq!(update.nodes[0].0, widget_id_to_node_id(root));
        assert_eq!(update.focus, widget_id_to_node_id(root));
    }

    #[test]
    fn build_update_includes_children() {
        let (mut arena, root) = make_arena_with_root();
        let child1 = arena.insert(HotNode::default(), ColdNode::default());
        let child2 = arena.insert(HotNode::default(), ColdNode::default());
        arena.append_child(root, child1).unwrap();
        arena.append_child(root, child2).unwrap();

        let mut adapter = AccessKitAdapter::new(root);
        let update = adapter.build_update(&mut arena);
        assert_eq!(update.nodes.len(), 3);

        // Root node should have 2 children.
        let root_node = &update.nodes[0].1;
        assert_eq!(root_node.children().len(), 2);
    }

    #[test]
    fn build_update_sets_focus() {
        let (mut arena, root) = make_arena_with_root();
        let child = arena.insert(HotNode::default(), ColdNode::default());
        arena.append_child(root, child).unwrap();

        let mut adapter = AccessKitAdapter::new(root);
        adapter.set_focus(Some(child));
        let update = adapter.build_update(&mut arena);
        assert_eq!(update.focus, widget_id_to_node_id(child));
    }

    #[test]
    fn build_update_focus_falls_back_to_root_if_dead() {
        let (mut arena, root) = make_arena_with_root();
        let child = arena.insert(HotNode::default(), ColdNode::default());
        arena.append_child(root, child).unwrap();

        let mut adapter = AccessKitAdapter::new(root);
        adapter.set_focus(Some(child));
        arena.remove(child);
        let update = adapter.build_update(&mut arena);
        assert_eq!(update.focus, widget_id_to_node_id(root));
    }

    #[test]
    fn build_update_sets_bounds() {
        let (mut arena, root) = make_arena_with_root();
        if let Some(hot) = arena.get_hot_mut(root) {
            hot.bounds = MartensiteRect::new(10.0, 20.0, 100.0, 50.0);
        }
        let mut adapter = AccessKitAdapter::new(root);
        let update = adapter.build_update(&mut arena);
        let node = &update.nodes[0].1;
        let bounds = node.bounds().unwrap();
        assert_eq!(bounds.x0, 10.0);
        assert_eq!(bounds.y0, 20.0);
        assert_eq!(bounds.x1, 110.0);
        assert_eq!(bounds.y1, 70.0);
    }

    #[test]
    fn build_update_sets_label_from_cold_node() {
        let (mut arena, root) = make_arena_with_root();
        if let Some(cold) = arena.get_cold_mut(root) {
            cold.a11y_name = Some("Submit".to_string());
        }
        let mut adapter = AccessKitAdapter::new(root);
        let update = adapter.build_update(&mut arena);
        let node = &update.nodes[0].1;
        assert_eq!(node.label(), Some("Submit"));
    }

    #[test]
    fn build_update_sets_tooltip() {
        let (mut arena, root) = make_arena_with_root();
        if let Some(cold) = arena.get_cold_mut(root) {
            cold.tooltip = Some("Click to submit".to_string());
        }
        let mut adapter = AccessKitAdapter::new(root);
        let update = adapter.build_update(&mut arena);
        let node = &update.nodes[0].1;
        assert_eq!(node.tooltip(), Some("Click to submit"));
    }

    #[test]
    fn build_update_focusable_adds_focus_action() {
        let (mut arena, root) = make_arena_with_root();
        if let Some(hot) = arena.get_hot_mut(root) {
            hot.flags |= NodeFlags::FOCUSABLE | NodeFlags::VISIBLE;
        }
        let mut adapter = AccessKitAdapter::new(root);
        let update = adapter.build_update(&mut arena);
        let node = &update.nodes[0].1;
        assert!(node.supports_action(accesskit::Action::Focus));
    }

    #[test]
    fn build_update_inert_sets_disabled() {
        let (mut arena, root) = make_arena_with_root();
        if let Some(hot) = arena.get_hot_mut(root) {
            hot.flags |= NodeFlags::INERT;
        }
        let mut adapter = AccessKitAdapter::new(root);
        let update = adapter.build_update(&mut arena);
        let node = &update.nodes[0].1;
        assert!(node.is_disabled());
    }

    #[test]
    fn build_update_not_visible_sets_hidden() {
        let (mut arena, root) = make_arena_with_root();
        if let Some(hot) = arena.get_hot_mut(root) {
            hot.flags.remove(NodeFlags::VISIBLE);
        }
        let mut adapter = AccessKitAdapter::new(root);
        let update = adapter.build_update(&mut arena);
        let node = &update.nodes[0].1;
        assert!(node.is_hidden());
    }

    #[test]
    fn build_update_hidden_focusable_no_focus_action() {
        let (mut arena, root) = make_arena_with_root();
        if let Some(hot) = arena.get_hot_mut(root) {
            // Focusable but NOT visible — should not advertise Focus action.
            hot.flags |= NodeFlags::FOCUSABLE;
            hot.flags.remove(NodeFlags::VISIBLE);
        }
        let mut adapter = AccessKitAdapter::new(root);
        let update = adapter.build_update(&mut arena);
        let node = &update.nodes[0].1;
        assert!(node.is_hidden());
        assert!(!node.supports_action(accesskit::Action::Focus));
    }

    #[test]
    fn build_update_sets_role_from_cold_node() {
        let (mut arena, root) = make_arena_with_root();
        if let Some(cold) = arena.get_cold_mut(root) {
            cold.a11y_role = accesskit::Role::Button;
        }
        let mut adapter = AccessKitAdapter::new(root);
        let update = adapter.build_update(&mut arena);
        let node = &update.nodes[0].1;
        assert_eq!(node.role(), accesskit::Role::Button);
    }

    #[test]
    fn build_update_clears_dirty_flags() {
        let (mut arena, root) = make_arena_with_root();
        let child = arena.insert(HotNode::default(), ColdNode::default());
        arena.append_child(root, child).unwrap();

        if let Some(hot) = arena.get_hot_mut(root) {
            hot.flags |= NodeFlags::DIRTY_A11Y;
        }
        if let Some(hot) = arena.get_hot_mut(child) {
            hot.flags |= NodeFlags::DIRTY_A11Y;
        }

        let mut adapter = AccessKitAdapter::new(root);
        let _update = adapter.build_update(&mut arena);

        // After build_update, dirty flags should be cleared.
        for id in arena.iter_subtree(root) {
            let hot = arena.get_hot(id).unwrap();
            assert!(!hot.flags.contains(NodeFlags::DIRTY_A11Y));
        }
    }

    #[test]
    fn incremental_update_only_dirty_nodes_and_parents() {
        let (mut arena, root) = make_arena_with_root();
        let child1 = arena.insert(HotNode::default(), ColdNode::default());
        let child2 = arena.insert(HotNode::default(), ColdNode::default());
        arena.append_child(root, child1).unwrap();
        arena.append_child(root, child2).unwrap();

        // Mark only child1 as dirty.
        if let Some(hot) = arena.get_hot_mut(child1) {
            hot.flags |= NodeFlags::DIRTY_A11Y;
        }

        let mut adapter = AccessKitAdapter::new(root);
        let update = adapter.build_incremental_update(&mut arena).unwrap();
        // Should include child1 (dirty) and root (its parent for children list).
        assert_eq!(update.nodes.len(), 2);
        let node_ids: Vec<_> = update.nodes.iter().map(|(id, _)| *id).collect();
        assert!(node_ids.contains(&widget_id_to_node_id(child1)));
        assert!(node_ids.contains(&widget_id_to_node_id(root)));
        // child2 should NOT be included.
        assert!(!node_ids.contains(&widget_id_to_node_id(child2)));
    }

    #[test]
    fn incremental_update_none_when_nothing_dirty() {
        let (mut arena, root) = make_arena_with_root();
        let mut adapter = AccessKitAdapter::new(root);
        // Initialize last_emitted_focus by calling build_update first.
        let _ = adapter.build_update(&mut arena);
        assert!(adapter.build_incremental_update(&mut arena).is_none());
    }

    #[test]
    fn incremental_update_includes_focused_node() {
        let (mut arena, root) = make_arena_with_root();
        let child = arena.insert(HotNode::default(), ColdNode::default());
        arena.append_child(root, child).unwrap();

        let mut adapter = AccessKitAdapter::new(root);
        // Initialize last_emitted_focus.
        let _ = adapter.build_update(&mut arena);
        adapter.set_focus(Some(child));
        let update = adapter.build_incremental_update(&mut arena).unwrap();
        assert_eq!(update.focus, widget_id_to_node_id(child));
    }

    #[test]
    fn incremental_update_emits_focus_clear() {
        let (mut arena, root) = make_arena_with_root();
        let child = arena.insert(HotNode::default(), ColdNode::default());
        arena.append_child(root, child).unwrap();

        let mut adapter = AccessKitAdapter::new(root);
        adapter.set_focus(Some(child));
        // Initial build — focus is on child.
        let _ = adapter.build_update(&mut arena);
        // Clear focus — this is a focus change, should produce an update.
        adapter.set_focus(None);
        let update = adapter.build_incremental_update(&mut arena);
        assert!(update.is_some(), "focus clear should produce an update");
        let update = update.unwrap();
        // Focus should fall back to root.
        assert_eq!(update.focus, widget_id_to_node_id(root));
    }

    #[test]
    fn incremental_update_clears_dirty_flags() {
        let (mut arena, root) = make_arena_with_root();
        let child = arena.insert(HotNode::default(), ColdNode::default());
        arena.append_child(root, child).unwrap();

        if let Some(hot) = arena.get_hot_mut(child) {
            hot.flags |= NodeFlags::DIRTY_A11Y;
        }

        let mut adapter = AccessKitAdapter::new(root);
        let _ = adapter.build_incremental_update(&mut arena);

        // After incremental update, dirty flags should be cleared.
        for id in arena.iter_subtree(root) {
            let hot = arena.get_hot(id).unwrap();
            assert!(!hot.flags.contains(NodeFlags::DIRTY_A11Y));
        }
    }

    #[test]
    fn mark_dirty_sets_flag() {
        let (mut arena, root) = make_arena_with_root();
        let adapter = AccessKitAdapter::new(root);
        adapter.mark_dirty(&mut arena, root);
        let hot = arena.get_hot(root).unwrap();
        assert!(hot.flags.contains(NodeFlags::DIRTY_A11Y));
    }

    #[test]
    fn clear_dirty_removes_flag() {
        let (mut arena, root) = make_arena_with_root();
        let adapter = AccessKitAdapter::new(root);
        adapter.mark_dirty(&mut arena, root);
        adapter.clear_dirty(&mut arena, root);
        let hot = arena.get_hot(root).unwrap();
        assert!(!hot.flags.contains(NodeFlags::DIRTY_A11Y));
    }

    #[test]
    fn clear_all_dirty_removes_all_flags() {
        let (mut arena, root) = make_arena_with_root();
        let child = arena.insert(HotNode::default(), ColdNode::default());
        arena.append_child(root, child).unwrap();

        let adapter = AccessKitAdapter::new(root);
        adapter.mark_dirty(&mut arena, root);
        adapter.mark_dirty(&mut arena, child);
        adapter.clear_all_dirty(&mut arena);

        for id in arena.iter_subtree(root) {
            let hot = arena.get_hot(id).unwrap();
            assert!(!hot.flags.contains(NodeFlags::DIRTY_A11Y));
        }
    }

    #[test]
    fn resolve_alive_widget() {
        let (arena, root) = make_arena_with_root();
        let adapter = AccessKitAdapter::new(root);
        let nid = widget_id_to_node_id(root);
        assert_eq!(adapter.resolve(&arena, nid), Some(root));
    }

    #[test]
    fn resolve_dead_widget_returns_none() {
        let (mut arena, root) = make_arena_with_root();
        let child = arena.insert(HotNode::default(), ColdNode::default());
        arena.remove(child);
        let adapter = AccessKitAdapter::new(root);
        let nid = widget_id_to_node_id(child);
        assert_eq!(adapter.resolve(&arena, nid), None);
    }

    #[test]
    fn tree_update_has_toolkit_info() {
        let (mut arena, root) = make_arena_with_root();
        let mut adapter = AccessKitAdapter::new(root);
        let update = adapter.build_update(&mut arena);
        let tree = update.tree.unwrap();
        assert_eq!(tree.toolkit_name.as_deref(), Some("Martensite"));
        assert_eq!(
            tree.toolkit_version.as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn tree_id_is_root() {
        let (_arena, root) = make_arena_with_root();
        let adapter = AccessKitAdapter::new(root);
        assert_eq!(adapter.tree_id(), accesskit::TreeId::ROOT);
    }

    #[test]
    fn widget_accessibility_hook_is_called() {
        use martensite_core::widget::{LayoutConstraints, LayoutContext, Widget};
        use martensite_core::Rect;

        struct ButtonWidget;
        impl Widget for ButtonWidget {
            fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> glam::Vec2 {
                glam::Vec2::ZERO
            }
            fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
            fn accessibility(&self, node: &mut accesskit::Node) {
                node.set_label("Custom Button Label");
                node.add_action(accesskit::Action::Click);
            }
        }

        let mut arena = WidgetArena::new();
        let root = arena.insert(HotNode::default(), ColdNode::new(Box::new(ButtonWidget)));
        let mut adapter = AccessKitAdapter::new(root);
        let update = adapter.build_update(&mut arena);
        let node = &update.nodes[0].1;
        assert_eq!(node.label(), Some("Custom Button Label"));
        assert!(node.supports_action(accesskit::Action::Click));
    }
}
