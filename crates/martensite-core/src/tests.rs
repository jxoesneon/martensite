//! Comprehensive test suite for martensite-core.
#[cfg(test)]
mod suite {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use crate::arena::{ArenaError, WidgetArena};
    use crate::fence::FrameFence;
    use crate::id::WidgetId;
    use crate::node::{ColdNode, HotNode, NodeFlags, Rect};
    use crate::widget::{DummyWidget, LayoutConstraints, LayoutContext, Widget};
    use glam::Vec2;

    // --- 1. WidgetId Tests ---

    #[test]
    fn test_widget_id_lifecycle() {
        let id = WidgetId::new(42, 100).unwrap();
        assert_eq!(id.slot_idx(), 42);
        assert_eq!(id.generation(), 100);
        assert_eq!(id.to_u64(), (100u64 << 32) | 42u64);

        // Generation 0 is invalid and must be rejected
        let id_zero = WidgetId::new(5, 0);
        assert!(id_zero.is_none());

        // u64 conversions
        let raw = id.to_u64();
        let from_raw = WidgetId::from_u64(raw);
        assert_eq!(from_raw, Some(id));
        assert_eq!(WidgetId::from_u64(0), None);

        // Byte conversions (little-endian)
        let le = id.to_le_bytes();
        assert_eq!(le, raw.to_le_bytes());
        let from_le = WidgetId::from_le_bytes(le);
        assert_eq!(from_le, Some(id));
        assert_eq!(WidgetId::from_le_bytes([0u8; 8]), None);

        // Niche optimization: Option<WidgetId> must be exactly 8 bytes
        assert_eq!(std::mem::size_of::<WidgetId>(), 8);
        assert_eq!(std::mem::size_of::<Option<WidgetId>>(), 8);
    }

    // --- 2. HotNode and ColdNode Layout Tests ---

    #[test]
    fn test_hot_node_64_byte_layout_and_offsets() {
        use std::mem::{align_of, size_of};

        assert_eq!(size_of::<HotNode>(), 64);
        assert_eq!(align_of::<HotNode>(), 64);

        // Verify exact byte offsets within HotNode
        let node = HotNode::default();
        let base = &node as *const _ as usize;

        let bounds_offset = &node.bounds as *const _ as usize - base;
        let layout_offset = &node.layout_id as *const _ as usize - base;
        let flags_offset = &node.flags as *const _ as usize - base;
        let depth_offset = &node.depth_rank as *const _ as usize - base;
        let z_index_offset = &node.z_index as *const _ as usize - base;
        let parent_offset = &node.parent as *const _ as usize - base;
        let first_child_offset = &node.first_child as *const _ as usize - base;
        let next_sibling_offset = &node.next_sibling as *const _ as usize - base;
        let prev_sibling_offset = &node.prev_sibling as *const _ as usize - base;

        assert_eq!(bounds_offset, 0, "bounds offset");
        assert_eq!(layout_offset, 16, "layout_id offset");
        assert_eq!(flags_offset, 24, "flags offset");
        assert_eq!(depth_offset, 28, "depth_rank offset");
        assert_eq!(z_index_offset, 30, "z_index offset");
        assert_eq!(parent_offset, 32, "parent offset");
        assert_eq!(first_child_offset, 40, "first_child offset");
        assert_eq!(next_sibling_offset, 48, "next_sibling offset");
        assert_eq!(prev_sibling_offset, 56, "prev_sibling offset");
    }

    #[test]
    fn test_hot_node_methods() {
        let mut node = HotNode::new(taffy::NodeId::new(99));
        assert_eq!(node.depth_rank(), 0);
        assert_eq!(node.layer_depth(), 0);

        node.set_depth_rank(7);
        assert_eq!(node.depth_rank(), 7);
        assert_eq!(node.layer_depth(), 7);

        node.set_layer_depth(12);
        assert_eq!(node.depth_rank(), 12);
        assert_eq!(node.layer_depth(), 12);
    }

    #[test]
    fn test_cold_node_builders() {
        let cold = ColdNode::new(Box::new(DummyWidget))
            .with_name("button_primary")
            .with_role(accesskit::Role::Button)
            .with_tooltip("Click to submit")
            .with_a11y_name("Submit Button");

        assert_eq!(cold.debug_name, Some("button_primary"));
        assert_eq!(cold.a11y_role, accesskit::Role::Button);
        assert_eq!(cold.tooltip.as_deref(), Some("Click to submit"));
        assert_eq!(cold.a11y_name.as_deref(), Some("Submit Button"));

        let default_cold = ColdNode::default();
        assert_eq!(default_cold.debug_name, None);
    }

    #[test]
    fn test_rect_and_flags() {
        let r = Rect::new(10.0, 20.0, 100.0, 200.0);
        assert_eq!(r.min_x(), 10.0);
        assert_eq!(r.max_x(), 110.0);
        assert_eq!(r.min_y(), 20.0);
        assert_eq!(r.max_y(), 220.0);
        assert_eq!(r.width(), 100.0);
        assert_eq!(r.height(), 200.0);

        let flags = NodeFlags::DIRTY_LAYOUT | NodeFlags::VISIBLE;
        assert!(flags.contains(NodeFlags::DIRTY_LAYOUT));
        assert!(flags.contains(NodeFlags::VISIBLE));
        assert!(!flags.contains(NodeFlags::FOCUSABLE));
    }

    #[test]
    fn test_dummy_widget() {
        let mut widget = DummyWidget;
        let mut hot = HotNode::default();
        let mut cx = LayoutContext { hot: &mut hot };
        let constraints = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::splat(100.0),
        };
        assert_eq!(widget.measure(&mut cx, constraints), Vec2::ZERO);
        widget.layout(&mut cx, Rect::new(0.0, 0.0, 50.0, 50.0));
        assert_eq!(
            widget.event(&mut crate::widget::EventContext {}),
            crate::widget::EventResponse::Ignored
        );
    }

    // --- 3. WidgetArena Basic Operations ---

    #[test]
    fn test_arena_basic_lifecycle() {
        let mut arena = WidgetArena::new();
        assert!(arena.is_empty());
        assert_eq!(arena.len(), 0);
        assert!(arena.capacity() >= 256);

        let id1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let id2 = arena.insert(HotNode::default(), ColdNode::default());

        assert_eq!(arena.len(), 2);
        assert!(!arena.is_empty());
        assert!(arena.is_alive(id1));
        assert!(arena.is_alive(id2));

        // Dead ID checks
        let fake_id = WidgetId::new(999, 1).unwrap();
        assert!(!arena.is_alive(fake_id));
        assert!(arena.get_hot(fake_id).is_none());
        assert!(arena.get_cold(fake_id).is_none());
        assert!(arena.get_both(fake_id).is_none());

        // Mutable access
        if let Some(hot) = arena.get_hot_mut(id1) {
            hot.z_index = 5;
        }
        assert_eq!(arena.get_hot(id1).unwrap().z_index, 5);

        if let Some(cold) = arena.get_cold_mut(id1) {
            cold.debug_name = Some("custom_node");
        }
        assert_eq!(arena.get_cold(id1).unwrap().debug_name, Some("custom_node"));

        let both = arena.get_both(id1);
        assert!(both.is_some());
        let (h, c) = both.unwrap();
        assert_eq!(h.z_index, 5);
        assert_eq!(c.debug_name, Some("custom_node"));

        let both_mut = arena.get_both_mut(id1);
        assert!(both_mut.is_some());
        let (h_mut, c_mut) = both_mut.unwrap();
        h_mut.z_index = 10;
        c_mut.debug_name = Some("mutated");
        assert_eq!(arena.get_hot(id1).unwrap().z_index, 10);
        assert_eq!(arena.get_cold(id1).unwrap().debug_name, Some("mutated"));
    }

    // --- 4. Tree Hierarchy and Mutation Tests ---

    #[test]
    fn test_tree_append_and_accessors() {
        let mut arena = WidgetArena::new();
        let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c2 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c3 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));

        assert_eq!(arena.depth_rank(root), Some(0));
        assert_eq!(arena.first_child(root), None);
        assert_eq!(arena.last_child(root), None);

        // Append c1
        arena.append_child(root, c1).unwrap();
        assert_eq!(arena.parent(c1), Some(root));
        assert_eq!(arena.first_child(root), Some(c1));
        assert_eq!(arena.last_child(root), Some(c1));
        assert_eq!(arena.next_sibling(c1), None);
        assert_eq!(arena.prev_sibling(c1), None);
        assert_eq!(arena.depth_rank(c1), Some(1));

        // Append c2
        arena.append_child(root, c2).unwrap();
        assert_eq!(arena.first_child(root), Some(c1));
        assert_eq!(arena.last_child(root), Some(c2));
        assert_eq!(arena.next_sibling(c1), Some(c2));
        assert_eq!(arena.prev_sibling(c2), Some(c1));
        assert_eq!(arena.next_sibling(c2), None);
        assert_eq!(arena.depth_rank(c2), Some(1));

        // Append c3
        arena.append_child(root, c3).unwrap();
        assert_eq!(arena.first_child(root), Some(c1));
        assert_eq!(arena.last_child(root), Some(c3));
        assert_eq!(arena.next_sibling(c2), Some(c3));
        assert_eq!(arena.prev_sibling(c3), Some(c2));
        assert_eq!(arena.next_sibling(c3), None);
        assert_eq!(arena.depth_rank(c3), Some(1));

        // Append grandchild
        let g1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        arena.append_child(c1, g1).unwrap();
        assert_eq!(arena.depth_rank(g1), Some(2));
        assert_eq!(arena.parent(g1), Some(c1));
    }

    #[test]
    fn test_tree_prepend() {
        let mut arena = WidgetArena::new();
        let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c2 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));

        arena.prepend_child(root, c1).unwrap();
        assert_eq!(arena.first_child(root), Some(c1));
        assert_eq!(arena.last_child(root), Some(c1));

        arena.prepend_child(root, c2).unwrap();
        assert_eq!(arena.first_child(root), Some(c2));
        assert_eq!(arena.last_child(root), Some(c1));
        assert_eq!(arena.next_sibling(c2), Some(c1));
        assert_eq!(arena.prev_sibling(c1), Some(c2));
    }

    #[test]
    fn test_tree_insert_before_and_after() {
        let mut arena = WidgetArena::new();
        let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c2 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c3 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let mid = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));

        arena.append_child(root, c1).unwrap();
        arena.append_child(root, c3).unwrap();

        // Insert mid before c3
        arena.insert_before(c3, mid).unwrap();
        assert_eq!(arena.next_sibling(c1), Some(mid));
        assert_eq!(arena.prev_sibling(mid), Some(c1));
        assert_eq!(arena.next_sibling(mid), Some(c3));
        assert_eq!(arena.prev_sibling(c3), Some(mid));
        assert_eq!(arena.parent(mid), Some(root));
        assert_eq!(arena.depth_rank(mid), Some(1));

        // Insert c2 after mid
        arena.insert_after(mid, c2).unwrap();
        assert_eq!(arena.next_sibling(mid), Some(c2));
        assert_eq!(arena.prev_sibling(c2), Some(mid));
        assert_eq!(arena.next_sibling(c2), Some(c3));
        assert_eq!(arena.prev_sibling(c3), Some(c2));

        // Insert before head
        let head = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        arena.insert_before(c1, head).unwrap();
        assert_eq!(arena.first_child(root), Some(head));
        assert_eq!(arena.next_sibling(head), Some(c1));
        assert_eq!(arena.prev_sibling(c1), Some(head));

        // Insert after tail
        let tail = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        arena.insert_after(c3, tail).unwrap();
        assert_eq!(arena.last_child(root), Some(tail));
        assert_eq!(arena.next_sibling(c3), Some(tail));
        assert_eq!(arena.prev_sibling(tail), Some(c3));
        assert_eq!(arena.next_sibling(tail), None);
    }

    #[test]
    fn test_tree_reparenting_updates_depths() {
        let mut arena = WidgetArena::new();
        let r1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let r2 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let sub = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let leaf = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));

        arena.append_child(r1, sub).unwrap();
        arena.append_child(sub, leaf).unwrap();

        assert_eq!(arena.depth_rank(sub), Some(1));
        assert_eq!(arena.depth_rank(leaf), Some(2));

        // Nest r2 deeper: make r2 a child of an ancestor r0
        let r0 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        arena.append_child(r0, r2).unwrap();
        assert_eq!(arena.depth_rank(r2), Some(1));

        // Move sub from r1 to r2
        arena.append_child(r2, sub).unwrap();
        assert_eq!(arena.parent(sub), Some(r2));
        assert_eq!(arena.first_child(r1), None);
        assert_eq!(arena.depth_rank(sub), Some(2));
        assert_eq!(arena.depth_rank(leaf), Some(3));
    }

    #[test]
    fn test_tree_remove_child_and_detach() {
        let mut arena = WidgetArena::new();
        let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c2 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c3 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));

        arena.append_child(root, c1).unwrap();
        arena.append_child(root, c2).unwrap();
        arena.append_child(root, c3).unwrap();

        // Remove middle child c2
        arena.remove_child(root, c2).unwrap();
        assert_eq!(arena.next_sibling(c1), Some(c3));
        assert_eq!(arena.prev_sibling(c3), Some(c1));
        assert_eq!(arena.parent(c2), None);
        assert_eq!(arena.depth_rank(c2), Some(0));

        // Remove head child c1
        arena.detach(c1).unwrap();
        assert_eq!(arena.first_child(root), Some(c3));
        assert_eq!(arena.prev_sibling(c3), None);

        // Remove remaining tail child c3
        arena.detach(c3).unwrap();
        assert_eq!(arena.first_child(root), None);
        assert_eq!(arena.last_child(root), None);
    }

    #[test]
    fn test_tree_cycle_detection_and_invalid_operations() {
        let mut arena = WidgetArena::new();
        let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let b = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));

        arena.append_child(a, b).unwrap();
        arena.append_child(b, c).unwrap();

        // Self-parenting
        assert!(matches!(
            arena.append_child(a, a),
            Err(ArenaError::SelfParenting(_))
        ));
        assert!(matches!(
            arena.prepend_child(a, a),
            Err(ArenaError::SelfParenting(_))
        ));

        // Cycle: append ancestor a to descendant c
        assert_eq!(arena.append_child(c, a), Err(ArenaError::CycleDetected));
        assert_eq!(arena.prepend_child(c, a), Err(ArenaError::CycleDetected));

        // Not a child error
        let other = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        assert!(matches!(
            arena.remove_child(a, other),
            Err(ArenaError::NotAChild { .. })
        ));

        // Target with no parent
        let orphan = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        assert!(matches!(
            arena.insert_before(orphan, other),
            Err(ArenaError::NoParent(_))
        ));
        assert!(matches!(
            arena.insert_after(orphan, other),
            Err(ArenaError::NoParent(_))
        ));

        // Inserting before/after self
        assert!(matches!(
            arena.insert_before(b, b),
            Err(ArenaError::InvalidOperation(_))
        ));
        assert!(matches!(
            arena.insert_after(b, b),
            Err(ArenaError::InvalidOperation(_))
        ));
    }

    // --- 5. Removal, Swap-Remove, and Strict FIFO Freelist Tests ---

    #[test]
    fn test_arena_node_removal_preserves_tree_integrity() {
        let mut arena = WidgetArena::new();
        let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c2 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c3 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));

        arena.append_child(root, c1).unwrap();
        arena.append_child(root, c2).unwrap();
        arena.append_child(root, c3).unwrap();

        // Remove c2 from arena via arena.remove()
        let removed = arena.remove(c2);
        assert!(removed.is_some());
        assert!(!arena.is_alive(c2));

        // Verify c1 and c3 are still connected
        assert_eq!(arena.first_child(root), Some(c1));
        assert_eq!(arena.last_child(root), Some(c3));
        assert_eq!(arena.next_sibling(c1), Some(c3));
        assert_eq!(arena.prev_sibling(c3), Some(c1));

        // Remove root: c1 and c3 must become clean root nodes
        let removed_root = arena.remove(root);
        assert!(removed_root.is_some());
        assert_eq!(arena.parent(c1), None);
        assert_eq!(arena.parent(c3), None);
        assert_eq!(arena.depth_rank(c1), Some(0));
        assert_eq!(arena.depth_rank(c3), Some(0));
    }

    #[test]
    fn test_strict_fifo_freelist_slot_distribution() {
        let mut arena = WidgetArena::new();
        let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let b = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));

        assert_eq!(a.slot_idx(), 0);
        assert_eq!(b.slot_idx(), 1);
        assert_eq!(c.slot_idx(), 2);

        // Delete a, then b
        arena.remove(a);
        arena.remove(b);

        // Strict FIFO: next insert must reuse slot 0 (a), then slot 1 (b)
        let d = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let e = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));

        assert_eq!(d.slot_idx(), 0);
        assert_eq!(d.generation(), 2); // Generation advanced from 1 to 2
        assert_eq!(e.slot_idx(), 1);
        assert_eq!(e.generation(), 2);
    }

    #[test]
    fn test_generational_rollover_skips_zero() {
        let mut arena = WidgetArena::new();
        let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));

        // Manually set generation to u32::MAX to simulate near-overflow
        let slot_idx = id.slot_idx();
        arena.set_slot_generation_for_test(slot_idx, u32::MAX);
        let max_gen_id = WidgetId::new(slot_idx, u32::MAX).unwrap();

        assert!(arena.is_alive(max_gen_id));

        // Remove node: generation must wrap to 1, skipping 0
        arena.remove(max_gen_id);
        assert_eq!(arena.slot_generation(slot_idx), Some(1));
        assert!(!arena.is_alive(max_gen_id));

        // Next allocation of this slot gets generation 1
        let new_id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        assert_eq!(new_id.slot_idx(), id.slot_idx());
        assert_eq!(new_id.generation(), 1);
        assert!(arena.is_alive(new_id));
    }

    // --- 6. Traversal Iterators Tests ---

    #[test]
    fn test_children_iterator() {
        let mut arena = WidgetArena::new();
        let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c2 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c3 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));

        arena.append_child(root, c1).unwrap();
        arena.append_child(root, c2).unwrap();
        arena.append_child(root, c3).unwrap();

        // Forward traversal
        let collected: Vec<WidgetId> = arena.children(root).collect();
        assert_eq!(collected, vec![c1, c2, c3]);

        // Double-ended backward traversal
        let mut iter = arena.children(root);
        assert_eq!(iter.next_back(), Some(c3));
        assert_eq!(iter.next_back(), Some(c2));
        assert_eq!(iter.next_back(), Some(c1));
        assert_eq!(iter.next_back(), None);

        // Mixed forward and backward
        let mut iter2 = arena.children(root);
        assert_eq!(iter2.next(), Some(c1));
        assert_eq!(iter2.next_back(), Some(c3));
        assert_eq!(iter2.next(), Some(c2));
        assert_eq!(iter2.next(), None);
        assert_eq!(iter2.next_back(), None);

        // Empty children
        let empty_leaf = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        assert_eq!(arena.children(empty_leaf).next(), None);
    }

    #[test]
    fn test_subtree_iterator() {
        let mut arena = WidgetArena::new();
        // Tree layout:
        //       root
        //      /    \
        //    c1      c2
        //   /  \
        //  g1  g2
        let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c2 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let g1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let g2 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));

        arena.append_child(root, c1).unwrap();
        arena.append_child(root, c2).unwrap();
        arena.append_child(c1, g1).unwrap();
        arena.append_child(c1, g2).unwrap();

        let visited: Vec<WidgetId> = arena.iter_subtree(root).collect();
        assert_eq!(visited, vec![root, c1, g1, g2, c2]);

        // Subtree of c1
        let c1_visited: Vec<WidgetId> = arena.iter_subtree(c1).collect();
        assert_eq!(c1_visited, vec![c1, g1, g2]);

        // Dead handle returns empty
        let dead = WidgetId::new(999, 1).unwrap();
        assert_eq!(arena.iter_subtree(dead).next(), None);
    }

    #[test]
    fn test_depth_first_and_breadth_first_iterators() {
        let mut arena = WidgetArena::new();
        // Two independent trees in the same arena:
        // Tree 1: R1 -> (A, B)
        // Tree 2: R2 -> C
        let r1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let b = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let r2 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let c = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));

        arena.append_child(r1, a).unwrap();
        arena.append_child(r1, b).unwrap();
        arena.append_child(r2, c).unwrap();

        // Depth-first traversal
        let df: Vec<WidgetId> = arena.iter_depth_first().collect();
        assert_eq!(df, vec![r1, a, b, r2, c]);

        // Breadth-first traversal
        let bf: Vec<WidgetId> = arena.iter_breadth_first().collect();
        // Roots first: r1, r2. Then depth 1: a, b, c.
        assert_eq!(bf, vec![r1, r2, a, b, c]);

        // Subtree breadth-first
        let sbf: Vec<WidgetId> = arena.iter_subtree_breadth_first(r1).collect();
        assert_eq!(sbf, vec![r1, a, b]);

        // Empty arena iteration
        let empty_arena = WidgetArena::new();
        assert_eq!(empty_arena.iter_depth_first().next(), None);
        assert_eq!(empty_arena.iter_breadth_first().next(), None);
    }

    // --- 7. Idle Defragmentation Tests ---

    #[test]
    fn test_idle_defragmentation_shrink_to_fit() {
        let mut arena = WidgetArena::new();
        let mut ids = Vec::new();

        for _ in 0..1000 {
            ids.push(arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget)));
        }

        assert_eq!(arena.len(), 1000);
        assert!(arena.capacity() >= 1000);

        // Shrink when all slots are fully active (needed_slots == slots.len())
        arena.shrink_to_fit_idle();
        assert_eq!(arena.len(), 1000);

        // Delete 980 elements, leaving 20
        for id in ids.drain(20..) {
            arena.remove(id);
        }

        assert_eq!(arena.len(), 20);

        // Run idle compaction
        arena.shrink_to_fit_idle();

        assert_eq!(arena.len(), 20);
        assert_eq!(arena.capacity(), 20);

        // Verify surviving 20 widgets remain fully intact
        for id in ids {
            assert!(arena.is_alive(id));
        }

        // Complete purge
        let ids_to_remove: Vec<u32> = arena.dense_to_slot().to_vec();
        for &id in &ids_to_remove {
            if let Some(gen) = arena.slot_generation(id) {
                arena.remove(WidgetId::new(id, gen).unwrap());
            }
        }
        arena.shrink_to_fit_idle();
        assert_eq!(arena.len(), 0);
        assert_eq!(arena.capacity(), 0);
    }

    // --- 8. FrameFence Reader Synchronization Tests ---

    #[test]
    fn test_frame_fence_basic_read_and_raii_drop() {
        let fence = FrameFence::new();
        assert_eq!(fence.active_readers(), 0);
        assert_eq!(fence.epoch(), 1);

        {
            let guard = fence.read();
            assert_eq!(fence.active_readers(), 1);
            assert_eq!(guard.epoch(), 1);
            assert!(guard.is_valid());
        }

        assert_eq!(fence.active_readers(), 0);
    }

    #[test]
    fn test_frame_fence_multiple_readers() {
        let fence = FrameFence::new();
        let g1 = fence.read();
        let g2 = fence.acquire_lease();
        let g3 = fence.read();

        assert_eq!(fence.active_readers(), 3);
        drop(g2);
        assert_eq!(fence.active_readers(), 2);
        drop(g1);
        drop(g3);
        assert_eq!(fence.active_readers(), 0);
    }

    #[test]
    fn test_frame_fence_legacy_ddr0021_api() {
        let fence = Arc::new(FrameFence::new());
        fence.begin_frame();
        fence.begin_frame();
        assert_eq!(fence.active_readers(), 2);

        fence.end_frame();
        assert_eq!(fence.active_readers(), 1);

        let f_clone = Arc::clone(&fence);
        let h = thread::spawn(move || {
            thread::sleep(Duration::from_millis(5));
            f_clone.end_frame();
        });

        fence.wait_for_zero();
        h.join().unwrap();
        assert_eq!(fence.active_readers(), 0);
    }

    #[test]
    fn test_frame_fence_compaction_wait_with_readers() {
        let fence = Arc::new(FrameFence::new());
        let reader_fence = Arc::clone(&fence);
        let completed = Arc::new(AtomicBool::new(false));
        let completed_clone = Arc::clone(&completed);
        let started = Arc::new(AtomicBool::new(false));
        let started_clone = Arc::clone(&started);

        let handle = thread::spawn(move || {
            let guard = reader_fence.read();
            started_clone.store(true, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(30));
            drop(guard);
            completed_clone.store(true, Ordering::SeqCst);
        });

        while !started.load(Ordering::SeqCst) {
            std::hint::spin_loop();
        }

        let mut arena = WidgetArena::new();
        arena.begin_compaction(&fence).unwrap();
        arena.end_compaction(&fence);

        handle.join().unwrap();
        assert!(completed.load(Ordering::SeqCst));
        assert_eq!(fence.active_readers(), 0);
        assert_eq!(fence.epoch(), 2);
    }

    #[test]
    fn test_frame_fence_timeout_lease_reclamation() {
        // Configure fence with short 25ms timeout
        let fence = FrameFence::with_timeout(Duration::from_millis(25));
        let stalled_guard = fence.read();
        assert_eq!(fence.active_readers(), 1);
        assert!(stalled_guard.is_valid());

        let mut arena = WidgetArena::new();
        // Compaction should timeout and force-reclaim the stalled reader lease
        arena.begin_compaction(&fence).unwrap();
        arena.end_compaction(&fence);

        assert_eq!(fence.reclaimed_leases(), 1);
        assert_eq!(fence.active_readers(), 0);
        assert_eq!(fence.epoch(), 2);
        assert!(!stalled_guard.is_valid());

        // When the stalled reader finally drops, it must NOT decrement reader count below zero
        drop(stalled_guard);
        assert_eq!(fence.active_readers(), 0);
    }

    #[test]
    fn test_frame_fence_multithreaded_concurrency() {
        let fence = Arc::new(FrameFence::with_timeout(Duration::from_millis(100)));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let mut handles = Vec::new();

        // Spawn 4 concurrent reader threads
        for _ in 0..4 {
            let f = Arc::clone(&fence);
            let stop = Arc::clone(&stop_flag);
            handles.push(thread::spawn(move || {
                let mut ops = 0;
                while !stop.load(Ordering::Relaxed) && ops < 200 {
                    let guard = f.read();
                    std::hint::spin_loop();
                    drop(guard);
                    ops += 1;
                }
            }));
        }

        let mut arena = WidgetArena::new();
        for _ in 0..5 {
            arena.compact_and_shrink_idle(&fence).unwrap();
            thread::sleep(Duration::from_millis(5));
        }

        stop_flag.store(true, Ordering::SeqCst);
        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(fence.active_readers(), 0);
    }

    // --- 9. Display and Debug Formatting Tests ---

    #[test]
    fn test_error_display_and_formatting() {
        let dummy_id = WidgetId::new(1, 1).unwrap();
        let dummy_id2 = WidgetId::new(2, 1).unwrap();

        let errs = vec![
            ArenaError::InvalidNode(dummy_id),
            ArenaError::InvalidParent(dummy_id),
            ArenaError::InvalidChild(dummy_id),
            ArenaError::InvalidTarget(dummy_id),
            ArenaError::NoParent(dummy_id),
            ArenaError::NotAChild {
                parent: dummy_id,
                child: dummy_id2,
            },
            ArenaError::SelfParenting(dummy_id),
            ArenaError::CycleDetected,
            ArenaError::InvalidOperation("test error"),
        ];

        for err in errs {
            let msg = format!("{}", err);
            assert!(!msg.is_empty());
            let debug = format!("{:?}", err);
            assert!(!debug.is_empty());
        }
    }

    #[test]
    fn test_debug_formatting_on_structures() {
        let mut arena = WidgetArena::new();
        let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));

        let arena_debug = format!("{:?}", arena);
        assert!(arena_debug.contains("WidgetArena"));

        let children_debug = format!("{:?}", arena.children(id));
        assert!(children_debug.contains("Children"));

        let subtree_debug = format!("{:?}", arena.iter_subtree(id));
        assert!(subtree_debug.contains("SubtreeIter"));

        let df_debug = format!("{:?}", arena.iter_depth_first());
        assert!(df_debug.contains("DepthFirstIter"));

        let bf_debug = format!("{:?}", arena.iter_breadth_first());
        assert!(bf_debug.contains("BreadthFirstIter"));

        let fence = FrameFence::new();
        let fence_debug = format!("{:?}", fence);
        assert!(fence_debug.contains("FrameFence"));

        let guard = fence.read();
        let guard_debug = format!("{:?}", guard);
        assert!(guard_debug.contains("FrameGuard"));
    }

    #[test]
    fn test_coverage_edge_cases() {
        // WidgetArena default
        let mut arena = WidgetArena::default();
        let dead = WidgetId::new(999, 1).unwrap();
        let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        // Remove id to create an expired generation handle with existing slot
        arena.remove(id);
        let expired = id;

        assert!(arena.get_hot(expired).is_none());
        assert!(arena.get_hot_mut(expired).is_none());
        assert!(arena.get_cold(expired).is_none());
        assert!(arena.get_cold_mut(expired).is_none());
        assert!(arena.get_both(expired).is_none());
        assert!(arena.get_both_mut(expired).is_none());
        assert!(arena.remove(expired).is_none());

        assert!(arena.parent(dead).is_none());
        assert!(arena.first_child(dead).is_none());
        assert!(arena.last_child(dead).is_none());
        assert!(arena.next_sibling(dead).is_none());
        assert!(arena.prev_sibling(dead).is_none());
        assert!(arena.depth_rank(dead).is_none());
        assert!(arena.is_ancestor_of(dead, dead));

        assert!(matches!(
            arena.detach(dead),
            Err(ArenaError::InvalidNode(_))
        ));

        let live1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let live2 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let live3 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));

        // append_child dead checks
        assert!(matches!(
            arena.append_child(dead, live1),
            Err(ArenaError::InvalidParent(_))
        ));
        assert!(matches!(
            arena.append_child(live1, dead),
            Err(ArenaError::InvalidChild(_))
        ));

        // prepend_child dead checks
        assert!(matches!(
            arena.prepend_child(dead, live1),
            Err(ArenaError::InvalidParent(_))
        ));
        assert!(matches!(
            arena.prepend_child(live1, dead),
            Err(ArenaError::InvalidChild(_))
        ));

        // insert_before dead checks and cycle
        assert!(matches!(
            arena.insert_before(dead, live1),
            Err(ArenaError::InvalidTarget(_))
        ));
        assert!(matches!(
            arena.insert_before(live1, dead),
            Err(ArenaError::InvalidNode(_))
        ));

        arena.append_child(live1, live2).unwrap();
        arena.append_child(live2, live3).unwrap();
        assert_eq!(
            arena.insert_before(live3, live1),
            Err(ArenaError::CycleDetected)
        );

        // insert_after dead checks and cycle
        assert!(matches!(
            arena.insert_after(dead, live1),
            Err(ArenaError::InvalidTarget(_))
        ));
        assert!(matches!(
            arena.insert_after(live1, dead),
            Err(ArenaError::InvalidNode(_))
        ));
        assert_eq!(
            arena.insert_after(live3, live1),
            Err(ArenaError::CycleDetected)
        );

        // remove_child dead checks
        assert!(matches!(
            arena.remove_child(dead, live2),
            Err(ArenaError::InvalidParent(_))
        ));
        assert!(matches!(
            arena.remove_child(live1, dead),
            Err(ArenaError::InvalidChild(_))
        ));

        // FrameFence methods
        let fence = FrameFence::default();
        assert_eq!(fence.lease_timeout(), Duration::from_millis(500));
        assert!(!fence.is_compaction_in_progress());

        fence.mark_compaction_start();
        assert!(fence.is_compaction_in_progress());

        let fence_arc = Arc::new(FrameFence::with_timeout(Duration::from_millis(10)));
        let f_clone = Arc::clone(&fence_arc);
        fence_arc.mark_compaction_start();

        let reader_thread = thread::spawn(move || {
            let g = f_clone.read();
            drop(g);
        });

        thread::sleep(Duration::from_millis(15));
        fence_arc.mark_compaction_end();
        reader_thread.join().unwrap();

        fence.mark_compaction_end();
        assert!(!fence.is_compaction_in_progress());

        // Test guard dismiss (no-op drop)
        let f = FrameFence::new();
        let mut guard = f.read();
        guard.dismiss();
        drop(guard);

        // Subtree iteration edge case: walk up parent reaches None without hitting root
        // Detached node within a subtree
        let n1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        let n2 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
        arena.append_child(n1, n2).unwrap();
        // Artificially clear parent of n2 while keeping it as child of n1
        arena.get_hot_mut(n2).unwrap().parent = None;
        let collected: Vec<_> = arena.iter_subtree(n1).collect();
        assert_eq!(collected, vec![n1, n2]);

        // Widget trait default methods
        let w = DummyWidget;
        w.paint(&mut crate::widget::PaintContext {});
        let mut node = accesskit::Node::new(accesskit::Role::GenericContainer);
        w.accessibility(&mut node);
    }

    #[test]
    fn test_arena_randomized_10m_operations() {
        // Deterministic fuzz stress test: 10,000,000 random insert / dereference / remove
        // operations against WidgetArena, verifying generational integrity and no panics.
        const TOTAL_OPS: usize = 10_000_000;

        let mut arena = WidgetArena::with_capacity(1024);
        let mut active: Vec<WidgetId> = Vec::with_capacity(4096);
        let mut rng: u64 = 0x1234_5678_9ABC_DEF0;

        // Use a simple LCG to keep the test self-contained and reproducible.
        let mut next_rand = || {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            rng
        };

        let mut inserts = 0usize;
        let mut derefs = 0usize;
        let mut removes = 0usize;

        for op_idx in 0..TOTAL_OPS {
            let op = next_rand() % 100;
            match op {
                0..=49 => {
                    // 50% inserts
                    let val = (next_rand() % 1000) as f32 + 1.0;
                    let hot = HotNode {
                        bounds: Rect::new(val, val, val * 2.0, val * 2.0),
                        ..HotNode::default()
                    };
                    let id = arena.insert(hot, ColdNode::default());
                    assert!(arena.is_alive(id));
                    active.push(id);
                    inserts += 1;
                }
                50..=79 => {
                    // 30% dereferences
                    if !active.is_empty() {
                        let idx = (next_rand() as usize) % active.len();
                        let id = active[idx];
                        if let Some(hot) = arena.get_hot(id) {
                            let b = hot.bounds;
                            let x_diff = (b.size.x - b.origin.x * 2.0).abs();
                            let y_diff = (b.size.y - b.origin.y * 2.0).abs();
                            assert!(
                                x_diff <= 1e-3 && y_diff <= 1e-3,
                                "torn or corrupted bounds on active id {:?}",
                                id
                            );
                            assert!(arena.is_alive(id));
                            derefs += 1;
                        }
                    }
                }
                _ => {
                    // 20% removals
                    if !active.is_empty() {
                        let idx = (next_rand() as usize) % active.len();
                        let id = active.swap_remove(idx);
                        assert!(
                            arena.is_alive(id),
                            "active list must only contain live handles"
                        );
                        let removed = arena.remove(id);
                        assert!(removed.is_some(), "remove must succeed for a live handle");
                        assert!(!arena.is_alive(id), "removed handle must become stale");
                        removes += 1;
                    }
                }
            }

            // Trigger periodic compaction every 1M operations.
            if op_idx % 1_000_000 == 0 {
                arena.shrink_to_fit_idle();
            }
        }

        // All handles that remain in `active` must still be valid.
        for &id in &active {
            assert!(arena.is_alive(id), "remaining active handle must be alive");
        }

        // Drain the arena and confirm every handle becomes stale after removal.
        for id in active.drain(..) {
            assert!(arena.is_alive(id));
            assert!(arena.remove(id).is_some());
            assert!(!arena.is_alive(id));
        }

        println!(
            "10M fuzz ops: {} inserts, {} derefs, {} removes, final_len={}",
            inserts,
            derefs,
            removes,
            arena.len()
        );

        assert_eq!(
            arena.len(),
            0,
            "arena must be empty after draining active handles"
        );
    }
}
