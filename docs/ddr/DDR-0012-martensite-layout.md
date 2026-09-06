# Detailed Design Record: DDR-0012
## Title: `martensite-layout` Taffy Integration & Measure/Layout Separation

### 1. Architectural Role & Invariants
`martensite-layout` bridges the `martensite-core` Generational Arena with `taffy`, providing Flexbox/CSS Grid support without dual-tree memory overhead.
* **Invariant 1.1**: Layout calculation operates strictly in two decoupled passes (Intrinsic Measurement, Constraint Placement) exactly once per frame.
* **Invariant 1.2**: `taffy::TraversePartialTree` must be implemented directly on the `WidgetArena` without allocating a parallel shadow tree.
* **Invariant 1.3**: The dirty layout bitset propagates strictly bottom-up to avoid $O(N)$ re-evaluation of clean branches.

### 2. TraversePartialTree Implementation
```rust
use taffy::{TraversePartialTree, TraverseTree, NodeId};
use crate::arena::{WidgetArena, WidgetId};

impl TraversePartialTree for WidgetArena {
    type ChildIter<'a> = ArenaChildIter<'a>;

    fn child_ids(&self, parent_node_id: NodeId) -> Self::ChildIter<'_> {
        // O(1) slotmap lookup; returns iterator over `next_sibling` pointers.
        let widget_id = self.taffy_map.get(&parent_node_id).unwrap();
        ArenaChildIter::new(self, *widget_id)
    }
    
    fn child_count(&self, parent_node_id: NodeId) -> usize {
        let widget_id = self.taffy_map.get(&parent_node_id).unwrap();
        self.count_children(*widget_id) // Cached in HotNode
    }
    
    fn get_node(&self, node_id: NodeId) -> taffy::NodeItem {
        let widget_id = self.taffy_map.get(&node_id).unwrap();
        // Maps HotNode bounds and properties to Taffy types without allocation
        taffy::NodeItem::Node(node_id)
    }
}
```

### 3. Dirty Propagation Algorithm
1. **Invalidation**: When a widget's intrinsic size or layout style changes (e.g., text mutated), set `NodeFlags::DIRTY_LAYOUT`.
2. **Bottom-Up Sweep**: Walk up `parent` pointers, marking all ancestors with `NodeFlags::DIRTY_LAYOUT` until the root is reached or an already-dirty ancestor is found.
3. **Pass 1 - Measure**: Taffy traverses top-down. If a node is not dirty, return cached metrics.
4. **Pass 2 - Placement**: Taffy assigns absolute (x, y) coordinates.
5. **Clear**: All `DIRTY_LAYOUT` flags are cleared simultaneously via bitwise reset on the `hot_nodes` array (SIMD accelerated).

### 4. Performance Invariants
- Traversal overhead: Zero allocations. 100% cache locality via `HotNode` 64-byte alignment.
- Latency budget: < 2.0ms for a 10,000 node dynamic tree layout constraint solve.
