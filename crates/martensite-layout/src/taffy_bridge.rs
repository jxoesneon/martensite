//! Bridge from the Martensite `WidgetArena` to Taffy's
//! [`TraversePartialTree`] trait.
//!
//! The bridge lets Taffy's layout algorithms traverse the widget tree
//! stored in the generational arena without copying nodes into a separate
//! Taffy tree. Each [`WidgetId`](martensite_core::WidgetId) is mapped to a
//! [`taffy::NodeId`] via a lossless `u64` conversion, and child iteration
//! delegates to the arena's sibling-linked tree structure.

use core::iter::FusedIterator;

use martensite_core::{WidgetArena, WidgetId};
use taffy::NodeId;
use taffy::TraversePartialTree;

/// Convert a [`WidgetId`] to a [`taffy::NodeId`].
///
/// The conversion is lossless: `WidgetId` is a `NonZeroU64` and `NodeId`
/// wraps a `u64`, so every valid widget handle maps to a unique node id.
#[inline(always)]
pub fn widget_id_to_node_id(id: WidgetId) -> NodeId {
    NodeId::new(id.to_u64())
}

/// Convert a [`taffy::NodeId`] back to a [`WidgetId`].
///
/// Returns `None` if the node id is zero (which would be an invalid widget
/// handle) or if no live widget maps to that id in `arena`.
#[inline(always)]
pub fn node_id_to_widget_id(arena: &WidgetArena, node: NodeId) -> Option<WidgetId> {
    let raw: u64 = node.into();
    let wid = WidgetId::from_u64(raw)?;
    arena.is_alive(wid).then_some(wid)
}

/// Iterator over the children of a node in the arena, yielding [`NodeId`]s.
///
/// This is the iterator type returned by
/// [`ArenaBridge::child_ids`](crate::taffy_bridge::ArenaBridge::child_ids).
pub struct ArenaChildIter<'a> {
    arena: &'a WidgetArena,
    current: Option<WidgetId>,
}

impl<'a> Iterator for ArenaChildIter<'a> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let cur = self.current?;
        let node = widget_id_to_node_id(cur);
        self.current = self.arena.next_sibling(cur);
        Some(node)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // We cannot cheaply compute remaining count without walking the list.
        if self.current.is_some() {
            (1, None)
        } else {
            (0, Some(0))
        }
    }
}

impl FusedIterator for ArenaChildIter<'_> {}

/// A read-only bridge that adapts a `WidgetArena` to Taffy's
/// [`TraversePartialTree`] trait.
///
/// The bridge borrows the arena and exposes the tree topology (parent →
/// children) that Taffy needs to traverse during layout computation.
/// Style and measurement are supplied separately by the
/// [`LayoutEngine`](crate::engine::LayoutEngine).
pub struct ArenaBridge<'a> {
    arena: &'a WidgetArena,
}

impl<'a> ArenaBridge<'a> {
    /// Create a new bridge borrowing the given arena.
    #[inline(always)]
    pub fn new(arena: &'a WidgetArena) -> Self {
        Self { arena }
    }

    /// Borrow the underlying arena.
    #[inline(always)]
    pub fn arena(&self) -> &'a WidgetArena {
        self.arena
    }

    /// Resolve a [`NodeId`] to a [`WidgetId`], or `None` if invalid.
    #[inline(always)]
    pub fn resolve(&self, node: NodeId) -> Option<WidgetId> {
        node_id_to_widget_id(self.arena, node)
    }
}

impl<'a> TraversePartialTree for ArenaBridge<'a> {
    type ChildIter<'b>
        = ArenaChildIter<'b>
    where
        Self: 'b;

    #[inline]
    fn child_ids(&self, parent_node_id: NodeId) -> Self::ChildIter<'_> {
        let first = self
            .resolve(parent_node_id)
            .and_then(|wid| self.arena.first_child(wid));
        ArenaChildIter {
            arena: self.arena,
            current: first,
        }
    }

    #[inline]
    fn child_count(&self, parent_node_id: NodeId) -> usize {
        match self.resolve(parent_node_id) {
            Some(wid) => self.arena.children(wid).count(),
            None => 0,
        }
    }

    #[inline]
    fn get_child_id(&self, parent_node_id: NodeId, child_index: usize) -> NodeId {
        let parent = match self.resolve(parent_node_id) {
            Some(wid) => wid,
            None => return NodeId::new(0),
        };
        for (idx, child) in self.arena.children(parent).enumerate() {
            if idx == child_index {
                return widget_id_to_node_id(child);
            }
        }
        NodeId::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{ColdNode, HotNode, WidgetArena};
    use taffy::TraversePartialTree;

    fn make_arena_with_chain() -> WidgetArena {
        // root -> child_a -> grandchild
        // root -> child_b
        let mut arena = WidgetArena::new();
        let root = arena.insert(
            HotNode::new(taffy::NodeId::new(1)),
            ColdNode::new(Box::new(NoopWidget)),
        );
        let child_a = arena.insert(
            HotNode::new(taffy::NodeId::new(2)),
            ColdNode::new(Box::new(NoopWidget)),
        );
        let child_b = arena.insert(
            HotNode::new(taffy::NodeId::new(3)),
            ColdNode::new(Box::new(NoopWidget)),
        );
        let grand = arena.insert(
            HotNode::new(taffy::NodeId::new(4)),
            ColdNode::new(Box::new(NoopWidget)),
        );
        arena.append_child(root, child_a).unwrap();
        arena.append_child(root, child_b).unwrap();
        arena.append_child(child_a, grand).unwrap();
        arena
    }

    // Minimal widget for test nodes.
    struct NoopWidget;
    impl martensite_core::widget::Widget for NoopWidget {
        fn measure(
            &mut self,
            _cx: &mut martensite_core::widget::LayoutContext,
            _constraints: martensite_core::widget::LayoutConstraints,
        ) -> glam::Vec2 {
            glam::Vec2::ZERO
        }
        fn layout(
            &mut self,
            _cx: &mut martensite_core::widget::LayoutContext,
            _bounds: martensite_core::Rect,
        ) {
        }
    }

    #[test]
    fn bridge_child_count_root() {
        let arena = make_arena_with_chain();
        let bridge = ArenaBridge::new(&arena);
        let root_wid = arena
            .iter_breadth_first()
            .next()
            .expect("at least one root");
        let root_node = widget_id_to_node_id(root_wid);
        assert_eq!(bridge.child_count(root_node), 2);
    }

    #[test]
    fn bridge_child_ids_yields_all_children() {
        let arena = make_arena_with_chain();
        let bridge = ArenaBridge::new(&arena);
        let root_wid = arena
            .iter_breadth_first()
            .next()
            .expect("at least one root");
        let root_node = widget_id_to_node_id(root_wid);
        let children: Vec<NodeId> = bridge.child_ids(root_node).collect();
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn bridge_get_child_id_by_index() {
        let arena = make_arena_with_chain();
        let bridge = ArenaBridge::new(&arena);
        let root_wid = arena
            .iter_breadth_first()
            .next()
            .expect("at least one root");
        let root_node = widget_id_to_node_id(root_wid);
        let first = bridge.get_child_id(root_node, 0);
        let second = bridge.get_child_id(root_node, 1);
        assert_ne!(first, second);
        // Out-of-range returns NodeId(0)
        assert_eq!(bridge.get_child_id(root_node, 99), NodeId::new(0));
    }

    #[test]
    fn bridge_grandchild_count() {
        let arena = make_arena_with_chain();
        let bridge = ArenaBridge::new(&arena);
        // Find child_a (first child of root)
        let root_wid = arena
            .iter_breadth_first()
            .next()
            .expect("at least one root");
        let child_a = arena.first_child(root_wid).expect("first child");
        let child_a_node = widget_id_to_node_id(child_a);
        assert_eq!(bridge.child_count(child_a_node), 1);
    }

    #[test]
    fn bridge_invalid_node_returns_empty() {
        let arena = make_arena_with_chain();
        let bridge = ArenaBridge::new(&arena);
        let bogus = NodeId::new(999_999);
        assert_eq!(bridge.child_count(bogus), 0);
        let children: Vec<NodeId> = bridge.child_ids(bogus).collect();
        assert!(children.is_empty());
    }

    #[test]
    fn widget_id_node_id_roundtrip() {
        let mut arena = WidgetArena::new();
        let id = arena.insert(
            HotNode::new(taffy::NodeId::new(1)),
            ColdNode::new(Box::new(NoopWidget)),
        );
        let node = widget_id_to_node_id(id);
        let back = node_id_to_widget_id(&arena, node);
        assert_eq!(back, Some(id));
    }
}
