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
use martensite_core::{
    A11yEmittedNode, NodeFlags, OverlayA11yRef, OverlayEntry, OverlayLayer, Rect as MartensiteRect,
    Widget, WidgetArena, WidgetId,
};
use std::collections::HashMap;

use crate::{node_id_to_widget_id, rect_to_accesskit, widget_id_to_node_id};

/// A stable path to a widget-internal child: the owning arena node plus
/// the chain of [`Widget::child`] indices that reaches the internal
/// widget. Internal children have no arena `WidgetId`, so the adapter
/// mints virtual [`NodeId`]s for them from the generation-0 space —
/// real widget handles always carry a non-zero generation in their high
/// 32 bits, so values `1..=u32::MAX` can never collide.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct InternalPath {
    /// The arena node whose widget owns the internal subtree.
    owner: WidgetId,
    /// Indices of nested `Widget::child` calls from the owner down to
    /// the target internal widget.
    indices: Vec<u32>,
}

/// Key for a virtual [`NodeId`] minted for a node inside an overlay
/// popup subtree — the overlay analogue of [`InternalPath`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct OverlayPath {
    /// The overlay entry id, as returned by `OverlayLayer::open`.
    entry: u64,
    /// Indices of nested `Widget::child` calls from the popup's content
    /// root down to the target widget; empty for the popup root.
    indices: Vec<u32>,
}

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
    /// Virtual [`NodeId`]s allocated for widget-internal children, keyed
    /// by their internal path. Persisted across updates so a given
    /// internal child keeps a stable `NodeId` for its lifetime.
    internal_ids: HashMap<InternalPath, NodeId>,
    /// Reverse lookup from a virtual [`NodeId`] to its internal path,
    /// used by [`resolve_internal`](Self::resolve_internal).
    internal_targets: HashMap<NodeId, InternalPath>,
    /// Virtual [`NodeId`]s allocated for overlay popup content, keyed by
    /// `(entry id, path inside the popup)`. Shares the `next_internal`
    /// mint with `internal_ids` — all virtual ids live in the
    /// generation-0 space.
    overlay_ids: HashMap<OverlayPath, NodeId>,
    /// Reverse lookup from a virtual [`NodeId`] to its overlay path,
    /// used by [`resolve_overlay`](Self::resolve_overlay).
    overlay_targets: HashMap<NodeId, OverlayPath>,
    /// NodeId references for every node emitted inside overlay popups
    /// during the current build, handed to `Widget::a11y_fixup` so
    /// widgets can wire `aria-activedescendant` and similar relations
    /// into their popups.
    overlay_refs: Vec<OverlayA11yRef>,
    /// Root `NodeId`s of the overlay popups emitted during the current
    /// build; appended to the tree root's children.
    overlay_roots: Vec<NodeId>,
    /// Next virtual `NodeId` to allocate (counts up in generation-0
    /// space).
    next_internal: u32,
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
            internal_ids: HashMap::new(),
            internal_targets: HashMap::new(),
            overlay_ids: HashMap::new(),
            overlay_targets: HashMap::new(),
            overlay_refs: Vec::new(),
            overlay_roots: Vec::new(),
            next_internal: 1,
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
        self.build_update_impl(arena, None)
    }

    /// Builds a full [`TreeUpdate`] that additionally emits every popup
    /// currently open in `overlay` as a virtual-node subtree rooted at
    /// the tree root — the accessibility view of the in-window
    /// [`OverlayLayer`].
    ///
    /// Each popup's content root becomes a child of the tree root
    /// (painted above window content, so appended last in z-order);
    /// its internal children are emitted beneath it with ids minted
    /// from the same generation-0 virtual space as widget-internal
    /// children. Before each widget's node is finalized, its
    /// `Widget::a11y_fixup` hook runs so it can wire relations into
    /// popups it owns (`aria-activedescendant`, `aria-describedby`).
    /// `resolve_overlay` maps the emitted virtual ids back to
    /// `(entry id, path)` pairs for action dispatch.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::AccessKitAdapter;
    /// use martensite_core::{OverlayAnchor, OverlayLayer, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert(Default::default(), Default::default());
    /// let mut adapter = AccessKitAdapter::new(root);
    /// let mut overlay = OverlayLayer::new();
    ///
    /// let update = adapter.build_update_with_overlay(&mut arena, &overlay);
    /// assert!(!update.nodes.is_empty());
    /// ```
    pub fn build_update_with_overlay(
        &mut self,
        arena: &mut WidgetArena,
        overlay: &OverlayLayer,
    ) -> TreeUpdate {
        self.build_update_impl(arena, Some(overlay))
    }

    /// Shared implementation of [`Self::build_update`] and
    /// [`Self::build_update_with_overlay`].
    fn build_update_impl(
        &mut self,
        arena: &mut WidgetArena,
        overlay: Option<&OverlayLayer>,
    ) -> TreeUpdate {
        self.overlay_roots.clear();
        self.overlay_refs.clear();

        // Let widgets apply pending assistive-technology activations
        // before anything is emitted.
        let ids: Vec<WidgetId> = arena.iter_subtree(self.root).collect();
        for id in ids {
            if let Some(cold) = arena.get_cold_mut(id) {
                cold.widget.a11y_prepare();
            }
        }

        let mut nodes = Vec::new();

        // Emit open popups first so `Widget::a11y_fixup` calls during the
        // arena walk can resolve their ids through `overlay_refs`.
        if let Some(layer) = overlay {
            for entry in layer.entries() {
                let (root_id, mut emitted) = self.emit_overlay_entry(entry);
                nodes.append(&mut emitted);
                self.overlay_roots.push(root_id);
            }
            let open: std::collections::HashSet<u64> = layer.entries().map(|e| e.id()).collect();
            self.overlay_ids.retain(|p, _| open.contains(&p.entry));
            self.overlay_targets.retain(|_, p| open.contains(&p.entry));
        } else {
            self.overlay_ids.clear();
            self.overlay_targets.clear();
        }

        for widget_id in arena.iter_subtree(self.root) {
            let Some((hot, cold)) = arena.get_both(widget_id) else {
                continue;
            };
            let node_id = widget_id_to_node_id(widget_id);
            let mut node = self.build_node(widget_id, hot.bounds, cold, arena);
            // Internal children are "inside" the widget's own subtree and
            // precede arena children, matching paint order.
            let mut emitted = Vec::new();
            let mut children = Vec::new();
            self.build_internal_children(
                widget_id,
                &*cold.widget,
                &mut Vec::new(),
                &mut emitted,
                &mut children,
            );
            children.extend(arena.children(widget_id).map(widget_id_to_node_id));
            // Overlay popups are top-level surfaces: attach their roots
            // to the tree root, painted above everything else.
            if widget_id == self.root {
                children.extend(self.overlay_roots.iter().copied());
            }
            if !children.is_empty() {
                node.set_children(children);
            }
            cold.widget
                .a11y_fixup(&mut emitted, &self.overlay_refs, &mut node);
            nodes.extend(emitted.into_iter().map(|e| (e.id, e.node)));
            nodes.push((node_id, node));
        }

        // Drop virtual ids whose owning arena node has died; a node that
        // reappears carries a fresh generation and gets a fresh id.
        self.internal_ids
            .retain(|path, _| arena.is_alive(path.owner));
        self.internal_targets
            .retain(|_, path| arena.is_alive(path.owner));

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

        // Apply pending AT activations on the widgets being emitted.
        for id in &to_emit {
            if let Some(cold) = arena.get_cold_mut(*id) {
                cold.widget.a11y_prepare();
            }
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
            let mut emitted = Vec::new();
            let mut children = Vec::new();
            self.build_internal_children(
                widget_id,
                &*cold.widget,
                &mut Vec::new(),
                &mut emitted,
                &mut children,
            );
            children.extend(arena.children(widget_id).map(widget_id_to_node_id));
            if !children.is_empty() {
                node.set_children(children);
            }
            cold.widget
                .a11y_fixup(&mut emitted, &self.overlay_refs, &mut node);
            nodes.extend(emitted.into_iter().map(|e| (e.id, e.node)));
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

    /// Returns an existing virtual [`NodeId`] for an internal-child path,
    /// minting a new one from the generation-0 space on first use.
    fn internal_node_id(&mut self, path: InternalPath) -> NodeId {
        if let Some(id) = self.internal_ids.get(&path) {
            return *id;
        }
        let id = NodeId(u64::from(self.next_internal));
        self.next_internal = self
            .next_internal
            .checked_add(1)
            .expect("virtual NodeId space exhausted");
        self.internal_targets.insert(id, path.clone());
        self.internal_ids.insert(path, id);
        id
    }

    /// Emits AccessKit nodes for a widget's internal children — and
    /// their internal children, recursively — collecting each into
    /// `emitted` (as [`A11yEmittedNode`] so `Widget::a11y_fixup` can
    /// patch them) and each child id to `out_children` in child order.
    ///
    /// Internal children have no arena node and no `ColdNode`, so their
    /// node is built from `Role::Unknown`, their layout-cached
    /// [`Widget::child_bounds`], and their own `accessibility` hook —
    /// which is expected to set the real role and semantic properties.
    fn build_internal_children(
        &mut self,
        owner: WidgetId,
        widget: &dyn Widget,
        prefix: &mut Vec<u32>,
        emitted: &mut Vec<A11yEmittedNode>,
        out_children: &mut Vec<NodeId>,
    ) {
        for i in 0..widget.child_count() {
            let Some(child) = widget.child(i) else {
                continue;
            };
            prefix.push(i as u32);
            let id = self.internal_node_id(InternalPath {
                owner,
                indices: prefix.clone(),
            });

            let mut node = Node::new(accesskit::Role::Unknown);
            if let Some(b) = widget.child_bounds(i) {
                if b.width() > 0.0 && b.height() > 0.0 {
                    node.set_bounds(rect_to_accesskit(b));
                }
            }
            child.accessibility(&mut node);

            // Recurse so grandchildren attach to this node, not the
            // arena owner.
            let mut grandchildren = Vec::new();
            self.build_internal_children(owner, child, prefix, emitted, &mut grandchildren);
            if !grandchildren.is_empty() {
                node.set_children(grandchildren);
            }
            let path = prefix.clone();
            prefix.pop();

            emitted.push(A11yEmittedNode { path, id, node });
            out_children.push(id);
        }
    }

    /// Returns an existing virtual [`NodeId`] for an overlay popup node,
    /// minting a new one from the shared generation-0 space on first
    /// use.
    fn overlay_node_id(&mut self, path: OverlayPath) -> NodeId {
        if let Some(id) = self.overlay_ids.get(&path) {
            return *id;
        }
        let id = NodeId(u64::from(self.next_internal));
        self.next_internal = self
            .next_internal
            .checked_add(1)
            .expect("virtual NodeId space exhausted");
        self.overlay_targets.insert(id, path.clone());
        self.overlay_ids.insert(path, id);
        id
    }

    /// Emits one overlay popup subtree: the entry's content root plus
    /// its internal children, recursively. Returns the popup root's
    /// `NodeId` and every emitted `(NodeId, Node)` pair (descendants
    /// first, root last). Each node's `(entry, path)` reference is
    /// recorded in `overlay_refs` for `Widget::a11y_fixup` lookups.
    fn emit_overlay_entry(&mut self, entry: &OverlayEntry) -> (NodeId, Vec<(NodeId, Node)>) {
        let entry_id = entry.id();
        let root_id = self.overlay_node_id(OverlayPath {
            entry: entry_id,
            indices: Vec::new(),
        });

        let mut node = Node::new(accesskit::Role::Unknown);
        let b = entry.bounds();
        if b.width() > 0.0 && b.height() > 0.0 {
            node.set_bounds(rect_to_accesskit(b));
        }
        entry.content().accessibility(&mut node);

        let mut nodes = Vec::new();
        let mut children = Vec::new();
        self.build_overlay_children(
            entry_id,
            entry.content(),
            &mut Vec::new(),
            &mut nodes,
            &mut children,
        );
        if !children.is_empty() {
            node.set_children(children);
        }
        self.overlay_refs.push(OverlayA11yRef {
            entry: entry_id,
            path: Vec::new(),
            id: root_id,
        });
        nodes.push((root_id, node));
        (root_id, nodes)
    }

    /// Emits AccessKit nodes for an overlay entry's internal children,
    /// recursively — the popup analogue of
    /// [`build_internal_children`](Self::build_internal_children), keyed
    /// by `(entry id, path)` and recording [`OverlayA11yRef`]s.
    fn build_overlay_children(
        &mut self,
        entry: u64,
        widget: &dyn Widget,
        prefix: &mut Vec<u32>,
        nodes: &mut Vec<(NodeId, Node)>,
        out_children: &mut Vec<NodeId>,
    ) {
        for i in 0..widget.child_count() {
            let Some(child) = widget.child(i) else {
                continue;
            };
            prefix.push(i as u32);
            let id = self.overlay_node_id(OverlayPath {
                entry,
                indices: prefix.clone(),
            });

            let mut node = Node::new(accesskit::Role::Unknown);
            if let Some(b) = widget.child_bounds(i) {
                if b.width() > 0.0 && b.height() > 0.0 {
                    node.set_bounds(rect_to_accesskit(b));
                }
            }
            child.accessibility(&mut node);

            let mut grandchildren = Vec::new();
            self.build_overlay_children(entry, child, prefix, nodes, &mut grandchildren);
            if !grandchildren.is_empty() {
                node.set_children(grandchildren);
            }
            self.overlay_refs.push(OverlayA11yRef {
                entry,
                path: prefix.clone(),
                id,
            });
            prefix.pop();

            nodes.push((id, node));
            out_children.push(id);
        }
    }

    /// Resolves a virtual [`NodeId`] minted for overlay popup content
    /// to the popup's entry id and the `Widget::child` index path
    /// inside it. Returns `None` for arena nodes and widget-internal
    /// virtual ids — use [`resolve`](Self::resolve) /
    /// [`resolve_internal`](Self::resolve_internal) for those. Action
    /// dispatch on a popup target walks the path through
    /// `OverlayLayer::widget_at_mut`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::{widget_id_to_node_id, AccessKitAdapter};
    /// use martensite_core::{HotNode, OverlayLayer, WidgetArena};
    /// # use martensite_core::ColdNode;
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert(HotNode::default(), ColdNode::default());
    /// let mut adapter = AccessKitAdapter::new(root);
    /// let overlay = OverlayLayer::new();
    /// let _ = adapter.build_update_with_overlay(&mut arena, &overlay);
    ///
    /// // Arena node ids are not overlay targets.
    /// assert!(adapter.resolve_overlay(widget_id_to_node_id(root)).is_none());
    /// ```
    pub fn resolve_overlay(&self, node_id: NodeId) -> Option<(u64, &[u32])> {
        self.overlay_targets
            .get(&node_id)
            .map(|path| (path.entry, path.indices.as_slice()))
    }

    /// Resolves a virtual [`NodeId`] minted for a widget-internal child
    /// to the owning arena widget and the `Widget::child` index path
    /// that reaches it.
    ///
    /// Returns `None` for ordinary (arena) node ids — use
    /// [`resolve`](Self::resolve) for those. Action dispatch on an
    /// internal target is the caller's responsibility: walk `indices`
    /// through `Widget::child_mut` starting from the owning widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::{widget_id_to_node_id, AccessKitAdapter};
    /// use martensite_core::{HotNode, WidgetArena};
    /// # use martensite_core::{ColdNode, DummyWidget};
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert(HotNode::default(), ColdNode::default());
    /// let mut adapter = AccessKitAdapter::new(root);
    /// let _ = adapter.build_update(&mut arena);
    ///
    /// // Arena node ids are not internal targets.
    /// assert!(adapter.resolve_internal(widget_id_to_node_id(root)).is_none());
    /// ```
    pub fn resolve_internal(&self, node_id: NodeId) -> Option<(WidgetId, &[u32])> {
        self.internal_targets
            .get(&node_id)
            .map(|path| (path.owner, path.indices.as_slice()))
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
    #[test]
    fn internal_children_emitted_with_virtual_ids() {
        use martensite_core::widget::{LayoutConstraints, LayoutContext, Widget};
        use martensite_core::Rect;

        struct LabelLeaf;
        impl Widget for LabelLeaf {
            fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> glam::Vec2 {
                glam::Vec2::ZERO
            }
            fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
            fn accessibility(&self, node: &mut accesskit::Node) {
                node.set_role(accesskit::Role::Label);
                node.set_label("inner text");
            }
        }

        struct ParentComposite {
            child: LabelLeaf,
        }
        impl Widget for ParentComposite {
            fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> glam::Vec2 {
                glam::Vec2::ZERO
            }
            fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
            fn accessibility(&self, node: &mut accesskit::Node) {
                node.set_role(accesskit::Role::GenericContainer);
            }
            fn child_count(&self) -> usize {
                1
            }
            fn child(&self, index: usize) -> Option<&dyn Widget> {
                (index == 0).then_some(&self.child as &dyn Widget)
            }
            fn child_bounds(&self, index: usize) -> Option<Rect> {
                (index == 0).then_some(Rect::new(4.0, 4.0, 20.0, 10.0))
            }
        }

        let mut arena = WidgetArena::new();
        let hot = HotNode {
            bounds: Rect::new(0.0, 0.0, 100.0, 40.0),
            ..HotNode::default()
        };
        let root = arena.insert(
            hot,
            ColdNode::new(Box::new(ParentComposite { child: LabelLeaf })),
        );

        let mut adapter = AccessKitAdapter::new(root);
        let update = adapter.build_update(&mut arena);

        // Two nodes: the arena root plus the internal child.
        assert_eq!(update.nodes.len(), 2);
        let root_nid = widget_id_to_node_id(root);
        let (virtual_id, node) = update
            .nodes
            .iter()
            .find(|(id, _)| *id != root_nid)
            .expect("internal child node emitted");
        let n: &accesskit::Node = node;
        assert_eq!(n.role(), accesskit::Role::Label);
        assert_eq!(n.label(), Some("inner text"));
        assert!(n.bounds().is_some(), "child bounds propagated");

        // The virtual id lives in generation-0 space: it must not
        // resolve to an arena widget, but resolve_internal maps it back.
        assert!(adapter.resolve(&arena, *virtual_id).is_none());
        let (owner, path) = adapter
            .resolve_internal(*virtual_id)
            .expect("internal path");
        assert_eq!(owner, root);
        assert_eq!(path, &[0]);

        // The parent's children list contains the virtual id.
        let root_node = &update
            .nodes
            .iter()
            .find(|(id, _)| *id == root_nid)
            .expect("root node")
            .1;
        assert_eq!(root_node.children(), &[*virtual_id]);

        // Stability: a second full build reuses the same virtual id.
        let update2 = adapter.build_update(&mut arena);
        let (virtual_id2, _) = update2
            .nodes
            .iter()
            .find(|(id, _)| *id != root_nid)
            .expect("internal child re-emitted");
        assert_eq!(*virtual_id, *virtual_id2);
    }
}
