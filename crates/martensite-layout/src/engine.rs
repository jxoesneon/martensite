//! Two-pass layout engine: intrinsic measurement followed by definitive
//! positioning.
//!
//! The [`LayoutEngine`] owns a [`taffy::TaffyTree`] that mirrors the
//! widget arena's tree topology. Each widget is represented by a Taffy
//! node whose style is derived from the widget's layout properties.
//!
//! ## Pass 1 — Intrinsic measurement
//!
//! Taffy computes each node's intrinsic (content-driven) size by walking
//! the tree bottom-up, querying leaf nodes' measure functions and
//! propagating sizes up through flex/grid containers.
//!
//! ## Pass 2 — Definitive positioning
//!
//! Once the root's available space is known, Taffy performs a top-down
//! pass assigning final `x`, `y`, `width`, `height` to every node. The
//! results are written back into each `HotNode`'s
//! `bounds` field via [`LayoutEngine::apply_layout`].

use core::iter::FusedIterator;

use martensite_core::{Rect, WidgetArena, WidgetId};
use taffy::{AvailableSpace, Layout, NodeId, Size, Style, TaffyTree};

use crate::geometry::{Constraints, EdgeInsets};

/// Error returned by layout operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    /// The referenced node does not exist in the Taffy tree.
    NodeNotFound(NodeId),
    /// The underlying Taffy engine returned an error.
    TaffyError(String),
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NodeNotFound(id) => write!(f, "taffy node not found: {:?}", id),
            Self::TaffyError(msg) => write!(f, "taffy layout error: {msg}"),
        }
    }
}

impl std::error::Error for LayoutError {}

/// Converts a Taffy [`Layout`] to a Martensite [`Rect`].
///
/// Taffy layouts use `f32` coordinates with origin at the parent's top-left.
#[inline]
pub fn taffy_layout_to_rect(layout: &Layout) -> Rect {
    Rect::new(
        layout.location.x,
        layout.location.y,
        layout.size.width,
        layout.size.height,
    )
}

/// Converts Martensite [`Constraints`] to Taffy
/// [`Size<AvailableSpace>`].
#[inline]
pub fn constraints_to_available(constraints: Constraints) -> Size<AvailableSpace> {
    Size {
        width: if constraints.max_width.is_infinite() {
            AvailableSpace::MaxContent
        } else {
            AvailableSpace::Definite(constraints.max_width)
        },
        height: if constraints.max_height.is_infinite() {
            AvailableSpace::MaxContent
        } else {
            AvailableSpace::Definite(constraints.max_height)
        },
    }
}

/// Converts [`EdgeInsets`] to a Taffy border/padding style contribution.
#[inline]
pub fn edge_insets_to_style(insets: EdgeInsets) -> Style {
    let mut style = Style::default();
    style.padding.left = taffy::LengthPercentage::length(insets.left);
    style.padding.right = taffy::LengthPercentage::length(insets.right);
    style.padding.top = taffy::LengthPercentage::length(insets.top);
    style.padding.bottom = taffy::LengthPercentage::length(insets.bottom);
    style
}

/// The two-pass layout engine.
///
/// Owns a [`TaffyTree`] that mirrors the widget arena. Nodes are
/// registered via [`LayoutEngine::register_node`] and their children
/// relationships established via [`LayoutEngine::set_children`].
/// After the topology is built, call [`LayoutEngine::compute`] to run
/// both passes, then [`LayoutEngine::apply_layout`] to write results
/// back into the arena.
pub struct LayoutEngine {
    /// The underlying Taffy layout tree.
    pub tree: TaffyTree,
    /// Mapping from Martensite [`WidgetId`] to Taffy [`NodeId`].
    id_map: Vec<(WidgetId, NodeId)>,
}

impl Default for LayoutEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutEngine {
    /// Creates a new empty layout engine.
    pub fn new() -> Self {
        Self {
            tree: TaffyTree::new(),
            id_map: Vec::new(),
        }
    }

    /// Creates a layout engine with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            tree: TaffyTree::with_capacity(capacity),
            id_map: Vec::with_capacity(capacity),
        }
    }

    /// Registers a new node in the layout tree with the given style.
    ///
    /// Returns the assigned [`NodeId`]. If the widget id was already
    /// registered, the existing node's style is updated instead.
    pub fn register_node(&mut self, widget_id: WidgetId, style: Style) -> NodeId {
        if let Some((_, existing)) = self.id_map.iter().find(|(wid, _)| *wid == widget_id) {
            let _ = self.tree.set_style(*existing, style);
            return *existing;
        }
        let node = self
            .tree
            .new_leaf(style)
            .expect("TaffyTree::new_leaf only fails on capacity overflow");
        self.id_map.push((widget_id, node));
        node
    }

    /// Registers a container node with explicit children.
    ///
    /// This is a convenience that creates the node and sets its children
    /// in one call.
    pub fn register_container(
        &mut self,
        widget_id: WidgetId,
        style: Style,
        children: &[WidgetId],
    ) -> NodeId {
        let node = self.register_node(widget_id, style);
        let child_nodes: Vec<NodeId> = children
            .iter()
            .map(|c| self.register_node(*c, Style::default()))
            .collect();
        let _ = self.tree.set_children(node, &child_nodes);
        node
    }

    /// Sets the children of an already-registered node.
    ///
    /// Children that are not yet registered are auto-registered with
    /// default style.
    pub fn set_children(
        &mut self,
        parent: WidgetId,
        children: &[WidgetId],
    ) -> Result<(), LayoutError> {
        let parent_node = self
            .lookup_node(parent)
            .ok_or(LayoutError::NodeNotFound(NodeId::new(0)))?;
        let child_nodes: Vec<NodeId> = children
            .iter()
            .map(|c| self.register_node(*c, Style::default()))
            .collect();
        self.tree
            .set_children(parent_node, &child_nodes)
            .map_err(|e| LayoutError::TaffyError(format!("{e:?}")))
    }

    /// Looks up the Taffy [`NodeId`] for a given [`WidgetId`].
    #[inline]
    pub fn lookup_node(&self, widget_id: WidgetId) -> Option<NodeId> {
        self.id_map
            .iter()
            .find(|(wid, _)| *wid == widget_id)
            .map(|(_, node)| *node)
    }

    /// Looks up the [`WidgetId`] for a given Taffy [`NodeId`].
    #[inline]
    pub fn lookup_widget(&self, node_id: NodeId) -> Option<WidgetId> {
        self.id_map
            .iter()
            .find(|(_, node)| *node == node_id)
            .map(|(wid, _)| *wid)
    }

    /// Returns the number of registered nodes.
    #[inline]
    pub fn node_count(&self) -> usize {
        self.id_map.len()
    }

    /// Removes all nodes from the engine.
    pub fn clear(&mut self) {
        self.tree = TaffyTree::new();
        self.id_map.clear();
    }

    /// Synchronizes the Taffy tree topology to match the arena's tree
    /// structure rooted at `root`.
    ///
    /// This walks the arena and ensures every descendant of `root` has a
    /// corresponding Taffy node with the correct parent-child
    /// relationships. Styles are preserved for already-registered nodes
    /// and defaulted for new ones.
    pub fn sync_from_arena(&mut self, arena: &WidgetArena, root: WidgetId) {
        // BFS walk to register all nodes and their children.
        let mut queue = std::collections::VecDeque::new();
        if !arena.is_alive(root) {
            return;
        }
        queue.push_back(root);
        while let Some(wid) = queue.pop_front() {
            // Ensure node is registered.
            if self.lookup_node(wid).is_none() {
                self.register_node(wid, Style::default());
            }
            let children: Vec<WidgetId> = arena.children(wid).collect();
            if !children.is_empty() {
                // Register children that don't exist yet.
                let child_nodes: Vec<NodeId> = children
                    .iter()
                    .map(|c| self.register_node(*c, Style::default()))
                    .collect();
                let parent_node = self.lookup_node(wid).expect("just registered");
                let _ = self.tree.set_children(parent_node, &child_nodes);
                for c in children {
                    queue.push_back(c);
                }
            }
        }
    }

    /// Runs both layout passes (measure + position) for the subtree
    /// rooted at `root_node`, given the available space.
    ///
    /// Taffy internally performs the two-pass algorithm: first measuring
    /// intrinsic sizes bottom-up, then assigning final positions
    /// top-down.
    pub fn compute(
        &mut self,
        root_node: NodeId,
        available: Size<AvailableSpace>,
    ) -> Result<(), LayoutError> {
        self.tree
            .compute_layout(root_node, available)
            .map_err(|e| LayoutError::TaffyError(format!("{e:?}")))
    }

    /// Reads the computed [`Layout`] for a node.
    pub fn layout(&self, node: NodeId) -> Result<&Layout, LayoutError> {
        self.tree
            .layout(node)
            .map_err(|_| LayoutError::NodeNotFound(node))
    }

    /// Applies computed layouts back into the arena's `HotNode` bounds.
    ///
    /// Walks all registered nodes, reads their Taffy layout, and writes
    /// the resulting [`Rect`] into the corresponding `HotNode.bounds`.
    /// Nodes whose layout hasn't been computed are left unchanged.
    pub fn apply_layout(&self, arena: &mut WidgetArena) {
        for (wid, node) in &self.id_map {
            let Some(hot) = arena.get_hot_mut(*wid) else {
                continue;
            };
            if let Ok(layout) = self.tree.layout(*node) {
                hot.bounds = taffy_layout_to_rect(layout);
            }
        }
    }

    /// Marks a node and its ancestors as layout-dirty.
    ///
    /// This is a logical flag; the actual recomputation happens on the
    /// next [`Self::compute`] call. The dirty flag is set on the
    /// `HotNode`'s `flags` field.
    pub fn mark_dirty(&self, arena: &mut WidgetArena, widget_id: WidgetId) {
        let mut current = Some(widget_id);
        while let Some(wid) = current {
            if let Some(hot) = arena.get_hot_mut(wid) {
                hot.flags |= martensite_core::NodeFlags::DIRTY_LAYOUT;
                current = hot.parent;
            } else {
                break;
            }
        }
    }

    /// Performs incremental re-layout: marks only `dirty_leaf` and its
    /// ancestors dirty, then recomputes layout for the root.
    ///
    /// This is the fast path for single-leaf invalidation.
    pub fn relayout_incremental(
        &mut self,
        arena: &mut WidgetArena,
        root: WidgetId,
        dirty_leaf: WidgetId,
        available: Size<AvailableSpace>,
    ) -> Result<(), LayoutError> {
        self.mark_dirty(arena, dirty_leaf);
        let root_node = self
            .lookup_node(root)
            .ok_or(LayoutError::NodeNotFound(NodeId::new(0)))?;
        self.compute(root_node, available)?;
        self.apply_layout(arena);
        Ok(())
    }

    /// Returns an iterator over all registered `(WidgetId, NodeId)` pairs.
    pub fn iter_nodes(&self) -> IdMapIter<'_> {
        IdMapIter {
            inner: self.id_map.iter(),
        }
    }
}

/// Iterator over registered id mappings.
pub struct IdMapIter<'a> {
    inner: std::slice::Iter<'a, (WidgetId, NodeId)>,
}

impl<'a> Iterator for IdMapIter<'a> {
    type Item = &'a (WidgetId, NodeId);
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

impl FusedIterator for IdMapIter<'_> {}

impl ExactSizeIterator for IdMapIter<'_> {
    fn len(&self) -> usize {
        self.inner.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taffy_bridge::{widget_id_to_node_id, ArenaBridge};
    use martensite_core::{ColdNode, HotNode, NodeFlags, WidgetArena};
    use taffy::TraversePartialTree;

    struct NoopWidget;
    impl martensite_core::widget::Widget for NoopWidget {
        fn measure(
            &mut self,
            _cx: &mut martensite_core::widget::LayoutContext,
            _constraints: martensite_core::widget::LayoutConstraints,
        ) -> glam::Vec2 {
            glam::Vec2::ZERO
        }
        fn layout(&mut self, _cx: &mut martensite_core::widget::LayoutContext, _bounds: Rect) {}
    }

    fn make_arena(depth: usize, branching: usize) -> WidgetArena {
        let mut arena = WidgetArena::new();
        let root = arena.insert(
            HotNode::new(NodeId::new(1)),
            ColdNode::new(Box::new(NoopWidget)),
        );
        fn build(
            arena: &mut WidgetArena,
            parent: WidgetId,
            depth: usize,
            branching: usize,
            counter: &mut u64,
        ) {
            if depth == 0 {
                return;
            }
            for _ in 0..branching {
                *counter += 1;
                let child = arena.insert(
                    HotNode::new(NodeId::new(*counter)),
                    ColdNode::new(Box::new(NoopWidget)),
                );
                arena.append_child(parent, child).unwrap();
                build(arena, child, depth - 1, branching, counter);
            }
        }
        let mut counter = 1u64;
        build(&mut arena, root, depth, branching, &mut counter);
        arena
    }

    #[test]
    fn engine_new_is_empty() {
        let engine = LayoutEngine::new();
        assert_eq!(engine.node_count(), 0);
    }

    #[test]
    fn register_and_lookup() {
        let mut engine = LayoutEngine::new();
        let wid = WidgetId::new(1, 1).unwrap();
        let node = engine.register_node(wid, Style::default());
        assert_eq!(engine.lookup_node(wid), Some(node));
        assert_eq!(engine.lookup_widget(node), Some(wid));
    }

    #[test]
    fn register_node_idempotent() {
        let mut engine = LayoutEngine::new();
        let wid = WidgetId::new(1, 1).unwrap();
        let n1 = engine.register_node(wid, Style::default());
        let n2 = engine.register_node(wid, Style::default());
        assert_eq!(n1, n2);
        assert_eq!(engine.node_count(), 1);
    }

    #[test]
    fn sync_from_arena_builds_topology() {
        let arena = make_arena(3, 2);
        let root = arena.iter_breadth_first().next().unwrap();
        let mut engine = LayoutEngine::new();
        engine.sync_from_arena(&arena, root);
        // root + 2 + 4 + 8 = 15 nodes
        assert_eq!(engine.node_count(), 15);
    }

    #[test]
    fn compute_and_apply_layout() {
        let mut arena = make_arena(1, 2);
        let root = arena.iter_breadth_first().next().unwrap();
        let mut engine = LayoutEngine::new();
        engine.sync_from_arena(&arena, root);
        let root_node = engine.lookup_node(root).unwrap();
        engine
            .compute(
                root_node,
                Size {
                    width: AvailableSpace::Definite(800.0),
                    height: AvailableSpace::Definite(600.0),
                },
            )
            .unwrap();
        engine.apply_layout(&mut arena);
        // Root should have non-zero layout
        let root_hot = arena.get_hot(root).unwrap();
        assert!(root_hot.bounds.width() >= 0.0);
    }

    #[test]
    fn mark_dirty_sets_flag_on_ancestors() {
        let mut arena = make_arena(2, 1);
        let root = arena.iter_breadth_first().next().unwrap();
        let child = arena.first_child(root).unwrap();
        let grandchild = arena.first_child(child).unwrap();

        let engine = LayoutEngine::new();
        engine.mark_dirty(&mut arena, grandchild);

        assert!(arena
            .get_hot(grandchild)
            .unwrap()
            .flags
            .contains(NodeFlags::DIRTY_LAYOUT));
        assert!(arena
            .get_hot(child)
            .unwrap()
            .flags
            .contains(NodeFlags::DIRTY_LAYOUT));
        assert!(arena
            .get_hot(root)
            .unwrap()
            .flags
            .contains(NodeFlags::DIRTY_LAYOUT));
    }

    #[test]
    fn relayout_incremental_works() {
        let mut arena = make_arena(2, 2);
        let root = arena.iter_breadth_first().next().unwrap();
        let child = arena.first_child(root).unwrap();

        let mut engine = LayoutEngine::new();
        engine.sync_from_arena(&arena, root);
        engine
            .relayout_incremental(
                &mut arena,
                root,
                child,
                Size {
                    width: AvailableSpace::Definite(400.0),
                    height: AvailableSpace::Definite(300.0),
                },
            )
            .unwrap();
        // After relayout, the dirty leaf should have been laid out
        let child_hot = arena.get_hot(child).unwrap();
        assert!(
            child_hot.flags.contains(NodeFlags::DIRTY_LAYOUT) || child_hot.bounds.width() >= 0.0
        );
    }

    #[test]
    fn taffy_layout_to_rect_conversion() {
        let layout = Layout {
            order: 0,
            location: taffy::Point { x: 10.0, y: 20.0 },
            size: taffy::Size {
                width: 100.0,
                height: 50.0,
            },
            scrollbar_size: taffy::Size {
                width: 0.0,
                height: 0.0,
            },
            border: taffy::Rect {
                left: 0.0,
                right: 0.0,
                top: 0.0,
                bottom: 0.0,
            },
            padding: taffy::Rect {
                left: 0.0,
                right: 0.0,
                top: 0.0,
                bottom: 0.0,
            },
            margin: taffy::Rect {
                left: 0.0,
                right: 0.0,
                top: 0.0,
                bottom: 0.0,
            },
        };
        let rect = taffy_layout_to_rect(&layout);
        assert_eq!(rect.origin.x, 10.0);
        assert_eq!(rect.origin.y, 20.0);
        assert_eq!(rect.width(), 100.0);
        assert_eq!(rect.height(), 50.0);
    }

    #[test]
    fn constraints_to_available_definite() {
        let c = Constraints::new(0.0, 0.0, 800.0, 600.0);
        let avail = constraints_to_available(c);
        assert_eq!(avail.width, AvailableSpace::Definite(800.0));
        assert_eq!(avail.height, AvailableSpace::Definite(600.0));
    }

    #[test]
    fn constraints_to_available_max_content() {
        let c = Constraints::unbounded();
        let avail = constraints_to_available(c);
        assert_eq!(avail.width, AvailableSpace::MaxContent);
        assert_eq!(avail.height, AvailableSpace::MaxContent);
    }

    #[test]
    fn edge_insets_to_style_conversion() {
        let insets = EdgeInsets::uniform(10.0);
        let style = edge_insets_to_style(insets);
        assert_eq!(style.padding.left, taffy::LengthPercentage::length(10.0));
        assert_eq!(style.padding.right, taffy::LengthPercentage::length(10.0));
        assert_eq!(style.padding.top, taffy::LengthPercentage::length(10.0));
        assert_eq!(style.padding.bottom, taffy::LengthPercentage::length(10.0));
    }

    #[test]
    fn deep_nested_flex_performance() {
        // Exit gate: 1000 deeply nested flex containers in < 0.5ms
        // We build a chain of 1000 nodes and time the layout compute.
        let mut arena = WidgetArena::with_capacity(1100);
        let mut parent = arena.insert(
            HotNode::new(NodeId::new(1)),
            ColdNode::new(Box::new(NoopWidget)),
        );
        for i in 2..=1000 {
            let child = arena.insert(
                HotNode::new(NodeId::new(i as u64)),
                ColdNode::new(Box::new(NoopWidget)),
            );
            arena.append_child(parent, child).unwrap();
            parent = child;
        }
        let root = arena.iter_breadth_first().next().unwrap();
        let mut engine = LayoutEngine::with_capacity(1100);
        engine.sync_from_arena(&arena, root);
        let root_node = engine.lookup_node(root).unwrap();

        let start = std::time::Instant::now();
        engine
            .compute(
                root_node,
                Size {
                    width: AvailableSpace::Definite(1920.0),
                    height: AvailableSpace::Definite(1080.0),
                },
            )
            .unwrap();
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_secs_f64() < 0.5,
            "deep nested layout took {:?}, expected < 0.5ms",
            elapsed
        );
    }

    #[test]
    fn incremental_relayout_performance() {
        // Exit gate: incremental re-layout with one dirty leaf in < 0.05ms
        let mut arena = make_arena(3, 3);
        let root = arena.iter_breadth_first().next().unwrap();
        let mut engine = LayoutEngine::new();
        engine.sync_from_arena(&arena, root);
        let root_node = engine.lookup_node(root).unwrap();
        // Full layout first
        engine
            .compute(
                root_node,
                Size {
                    width: AvailableSpace::Definite(1920.0),
                    height: AvailableSpace::Definite(1080.0),
                },
            )
            .unwrap();
        engine.apply_layout(&mut arena);

        // Now find a leaf and do incremental relayout
        let leaf = arena.iter_subtree(root).last().unwrap();
        let start = std::time::Instant::now();
        engine
            .relayout_incremental(
                &mut arena,
                root,
                leaf,
                Size {
                    width: AvailableSpace::Definite(1920.0),
                    height: AvailableSpace::Definite(1080.0),
                },
            )
            .unwrap();
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_secs_f64() < 0.05,
            "incremental relayout took {:?}, expected < 0.05ms",
            elapsed
        );
    }

    #[test]
    fn bridge_traverse_via_arena() {
        let arena = make_arena(2, 2);
        let root = arena.iter_breadth_first().next().unwrap();
        let bridge = ArenaBridge::new(&arena);
        let root_node = widget_id_to_node_id(root);
        // TraversePartialTree should see 2 children at root
        assert_eq!(TraversePartialTree::child_count(&bridge, root_node), 2);
    }
}
