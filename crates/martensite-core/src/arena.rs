//! Generational slotmap arena, tree hierarchy topology, and idle defragmentation.
//!
//! # Invariant Panics
//!
//! Internal tree-mutation methods use `expect("arena invariant")` on
//! `get_hot`/`get_hot_mut` calls. These are **deliberate** — each is
//! guarded by a prior `is_alive` check on the same `WidgetId`, so the
//! slot is guaranteed to be valid. Converting these to `Result` would
//! require changing the internal API to propagate errors that can
//! only occur if the arena's own data structures are corrupted (a
//! bug, not a user error). If such corruption occurs, panicking with
//! a clear message is the correct behavior (fail-fast).
use std::collections::VecDeque;
use std::iter::FusedIterator;

use crate::fence::FrameFence;
use crate::id::WidgetId;
use crate::node::{ColdNode, HotNode};

/// Error variants for arena tree operations and synchronization.
///
/// # Examples
///
/// ```
/// use martensite_core::{ArenaError, DummyWidget, HotNode, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
///
/// // Self-parenting is rejected with `SelfParenting`.
/// let err = arena.append_child(a, a).unwrap_err();
/// assert_eq!(err, ArenaError::SelfParenting(a));
///
/// // A stale handle is rejected with `InvalidNode`.
/// arena.remove(a);
/// let err = arena.detach(a).unwrap_err();
/// assert_eq!(err, ArenaError::InvalidNode(a));
///
/// // `ArenaError` implements `std::error::Error` and `Display`.
/// let msg = format!("{}", ArenaError::CycleDetected);
/// assert!(msg.contains("Cycle detected"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArenaError {
    /// Provided widget handle is invalid or has expired generation.
    InvalidNode(WidgetId),
    /// Parent widget handle is invalid or expired.
    InvalidParent(WidgetId),
    /// Child widget handle is invalid or expired.
    InvalidChild(WidgetId),
    /// Target widget handle is invalid or expired.
    InvalidTarget(WidgetId),
    /// Target widget has no parent node.
    NoParent(WidgetId),
    /// Node is not a child of the specified parent.
    NotAChild {
        /// Specified parent widget.
        parent: WidgetId,
        /// Specified child widget.
        child: WidgetId,
    },
    /// Attempted to set a node as its own parent.
    SelfParenting(WidgetId),
    /// Insertion would form a cycle in the tree hierarchy.
    CycleDetected,
    /// Invalid tree mutation operation requested.
    InvalidOperation(&'static str),
}

impl std::fmt::Display for ArenaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidNode(id) => write!(f, "Invalid or expired widget handle: {:?}", id),
            Self::InvalidParent(id) => {
                write!(f, "Invalid or expired parent widget handle: {:?}", id)
            }
            Self::InvalidChild(id) => write!(f, "Invalid or expired child widget handle: {:?}", id),
            Self::InvalidTarget(id) => {
                write!(f, "Invalid or expired target widget handle: {:?}", id)
            }
            Self::NoParent(id) => write!(f, "Widget {:?} has no parent", id),
            Self::NotAChild { parent, child } => {
                write!(
                    f,
                    "Widget {:?} is not a child of parent {:?}",
                    child, parent
                )
            }
            Self::SelfParenting(id) => write!(f, "Cannot parent widget {:?} to itself", id),
            Self::CycleDetected => write!(f, "Cycle detected in scene graph hierarchy"),
            Self::InvalidOperation(msg) => write!(f, "Invalid arena tree operation: {}", msg),
        }
    }
}

impl std::error::Error for ArenaError {}

/// Slot entry in the sparse lookup table.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Slot {
    /// Generational counter tracking allocation lifecycle.
    pub generation: u32,
    /// Index into dense parallel arrays.
    pub dense_idx: u32,
}

/// Generational slotmap arena maintaining packed 64-byte HotNode elements alongside ColdNode storage.
///
/// # Examples
///
/// Construct an arena, insert nodes, build a tree, and traverse it:
///
/// ```
/// use martensite_core::{DummyWidget, HotNode, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// let b = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// arena.append_child(root, a).unwrap();
/// arena.append_child(root, b).unwrap();
///
/// // Pre-order subtree traversal visits root, then its children in order.
/// let visited: Vec<_> = arena.iter_subtree(root).collect();
/// assert_eq!(visited, vec![root, a, b]);
///
/// // Removing a node unparents its children and invalidates the handle.
/// arena.remove(a);
/// assert!(!arena.is_alive(a));
/// assert_eq!(arena.children(root).count(), 1);
/// ```
pub struct WidgetArena {
    /// Sparse slot indirection table.
    slots: Vec<Slot>,
    /// Dense cache-line aligned hot node records.
    hot_nodes: Vec<HotNode>,
    /// Parallel cold node storage (widgets, metadata, accessibility).
    cold_nodes: Vec<ColdNode>,
    /// Reverse mapping from dense index to sparse slot index.
    dense_to_slot: Vec<u32>,
    /// Strict FIFO queue distributing recycled slot indices.
    free_slots: VecDeque<u32>,
}

impl std::fmt::Debug for WidgetArena {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WidgetArena")
            .field("len", &self.len())
            .field("capacity", &self.capacity())
            .field("slots_len", &self.slot_count())
            .field("free_slots_len", &self.free_slots_len())
            .finish()
    }
}

impl Default for WidgetArena {
    fn default() -> Self {
        Self::new()
    }
}

impl WidgetArena {
    /// Construct a new empty WidgetArena with default initial capacity (256 nodes).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetArena;
    ///
    /// let arena = WidgetArena::new();
    /// assert!(arena.is_empty());
    /// assert_eq!(arena.len(), 0);
    /// assert!(arena.capacity() >= 256);
    /// ```
    pub fn new() -> Self {
        Self::with_capacity(256)
    }

    /// Construct a new empty WidgetArena pre-allocated to the specified node capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetArena;
    ///
    /// let arena = WidgetArena::with_capacity(1024);
    /// assert!(arena.is_empty());
    /// assert!(arena.capacity() >= 1024);
    /// ```
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            slots: Vec::with_capacity(capacity),
            hot_nodes: Vec::with_capacity(capacity),
            cold_nodes: Vec::with_capacity(capacity),
            dense_to_slot: Vec::with_capacity(capacity),
            free_slots: VecDeque::new(),
        }
    }

    /// Return the count of currently active nodes in the arena.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.hot_nodes.len()
    }

    /// Return true if the arena contains zero active nodes.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.hot_nodes.is_empty()
    }

    /// Return current allocated dense node capacity.
    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.hot_nodes.capacity()
    }

    /// Return the number of sparse slot entries currently allocated.
    #[inline(always)]
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Return the number of recycled slot indices awaiting reuse.
    #[inline(always)]
    pub fn free_slots_len(&self) -> usize {
        self.free_slots.len()
    }

    /// Return a read-only view of the dense hot node storage.
    #[inline(always)]
    pub fn hot_nodes(&self) -> &[HotNode] {
        &self.hot_nodes
    }

    /// Return a read-only view of the dense-to-slot reverse mapping.
    #[inline(always)]
    pub fn dense_to_slot(&self) -> &[u32] {
        &self.dense_to_slot
    }

    /// Return the generation of a sparse slot, or `None` if the slot index is out of bounds.
    #[inline(always)]
    pub fn slot_generation(&self, slot_idx: u32) -> Option<u32> {
        self.slots.get(slot_idx as usize).map(|s| s.generation)
    }

    #[cfg(test)]
    /// Test-only helper to directly set a slot's generation value.
    pub fn set_slot_generation_for_test(&mut self, slot_idx: u32, generation: u32) {
        if let Some(slot) = self.slots.get_mut(slot_idx as usize) {
            slot.generation = generation;
        }
    }

    /// Check whether a widget handle is alive and points to a valid active node.
    #[inline(always)]
    pub fn is_alive(&self, id: WidgetId) -> bool {
        self.slots
            .get(id.slot_idx() as usize)
            .is_some_and(|slot| slot.generation == id.generation())
    }

    /// Retrieve an immutable reference to the HotNode for the given widget handle.
    #[inline(always)]
    pub fn get_hot(&self, id: WidgetId) -> Option<&HotNode> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation == id.generation() {
            Some(&self.hot_nodes[slot.dense_idx as usize])
        } else {
            None
        }
    }

    /// Retrieve a mutable reference to the HotNode for the given widget handle.
    #[inline(always)]
    pub fn get_hot_mut(&mut self, id: WidgetId) -> Option<&mut HotNode> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation == id.generation() {
            let dense = slot.dense_idx as usize;
            Some(&mut self.hot_nodes[dense])
        } else {
            None
        }
    }

    /// Retrieve an immutable reference to the ColdNode for the given widget handle.
    #[inline(always)]
    pub fn get_cold(&self, id: WidgetId) -> Option<&ColdNode> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation == id.generation() {
            Some(&self.cold_nodes[slot.dense_idx as usize])
        } else {
            None
        }
    }

    /// Retrieve a mutable reference to the ColdNode for the given widget handle.
    #[inline(always)]
    pub fn get_cold_mut(&mut self, id: WidgetId) -> Option<&mut ColdNode> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation == id.generation() {
            let dense = slot.dense_idx as usize;
            Some(&mut self.cold_nodes[dense])
        } else {
            None
        }
    }

    /// Retrieve immutable references to both HotNode and ColdNode concurrently.
    #[inline(always)]
    pub fn get_both(&self, id: WidgetId) -> Option<(&HotNode, &ColdNode)> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation == id.generation() {
            let dense = slot.dense_idx as usize;
            Some((&self.hot_nodes[dense], &self.cold_nodes[dense]))
        } else {
            None
        }
    }

    /// Retrieve mutable references to both HotNode and ColdNode concurrently.
    #[inline(always)]
    pub fn get_both_mut(&mut self, id: WidgetId) -> Option<(&mut HotNode, &mut ColdNode)> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation == id.generation() {
            let dense = slot.dense_idx as usize;
            Some((&mut self.hot_nodes[dense], &mut self.cold_nodes[dense]))
        } else {
            None
        }
    }

    /// Insert a new node into the arena, returning a generational handle.
    pub fn insert(&mut self, hot: HotNode, cold: ColdNode) -> WidgetId {
        let dense_idx = self.hot_nodes.len() as u32;
        self.hot_nodes.push(hot);
        self.cold_nodes.push(cold);

        // Strict FIFO slot recycling: pop from the front of the queue
        let slot_idx = if let Some(free_idx) = self.free_slots.pop_front() {
            let slot = &mut self.slots[free_idx as usize];
            slot.dense_idx = dense_idx;
            free_idx
        } else {
            let idx = self.slots.len() as u32;
            self.slots.push(Slot {
                generation: 1,
                dense_idx,
            });
            idx
        };

        self.dense_to_slot.push(slot_idx);
        // WidgetId::new returns None only if generation is zero.
        // Generation is set to 1 above for new slots and incremented
        // (skipping zero) for reused slots, so it is always >= 1.
        // The fallback handles the theoretical edge case where
        // generation wraps, which is unreachable in practice.
        WidgetId::new(slot_idx, self.slots[slot_idx as usize].generation).unwrap_or_else(|| {
            // Fallback: construct a valid WidgetId with generation 1.
            // This path is unreachable but avoids a panic.
            WidgetId::from_parts(slot_idx, 1)
        })
    }

    /// Insert a new node wrapping a boxed widget implementation with default cold metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// assert!(arena.is_alive(id));
    /// assert_eq!(arena.len(), 1);
    /// ```
    pub fn insert_with_widget(
        &mut self,
        hot: HotNode,
        widget: Box<dyn crate::widget::Widget>,
    ) -> WidgetId {
        self.insert(hot, ColdNode::new(widget))
    }

    /// Remove a node from the arena, unparenting its children and detaching from its hierarchy.
    pub fn remove(&mut self, id: WidgetId) -> Option<(HotNode, ColdNode)> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation != id.generation() {
            return None;
        }

        // 1. Unparent all immediate children so they become clean root-level nodes
        let mut child_opt = self.first_child(id);
        if let Some(hot) = self.get_hot_mut(id) {
            hot.first_child = None;
        }
        while let Some(child_id) = child_opt {
            let next_sibling = self.next_sibling(child_id);
            if let Some(hot) = self.get_hot_mut(child_id) {
                hot.parent = None;
                hot.prev_sibling = None;
                hot.next_sibling = None;
                hot.depth_rank = 0;
            }
            self.update_subtree_depths(child_id, 0);
            child_opt = next_sibling;
        }

        // 2. Detach the target node from its parent and sibling chains.
        // Detach failure is acceptable here because the node is being
        // removed entirely; if it was already detached, that's fine.
        if self.detach(id).is_err() {
            // Node may have already been detached; continue with removal.
        }

        // 3. Re-read slot reference to advance generation skipping zero
        let slot = &mut self.slots[id.slot_idx() as usize];
        slot.generation = if slot.generation == u32::MAX {
            1
        } else {
            slot.generation + 1
        };

        let removed_dense = slot.dense_idx as usize;
        let last_dense = self.hot_nodes.len() - 1;

        let hot = self.hot_nodes.swap_remove(removed_dense);
        let cold = self.cold_nodes.swap_remove(removed_dense);
        self.dense_to_slot.swap_remove(removed_dense);

        // FIFO recycling: push freed slot index to the back
        self.free_slots.push_back(id.slot_idx());

        if removed_dense != last_dense {
            let relocated_slot_idx = self.dense_to_slot[removed_dense] as usize;
            self.slots[relocated_slot_idx].dense_idx = removed_dense as u32;
        }

        Some((hot, cold))
    }

    // --- Tree Hierarchy Accessors ---

    /// Retrieve the parent handle of the specified node.
    #[inline(always)]
    pub fn parent(&self, id: WidgetId) -> Option<WidgetId> {
        if !self.is_alive(id) {
            return None;
        }
        self.get_hot(id).and_then(|h| h.parent)
    }

    /// Retrieve the first child handle of the specified node.
    #[inline(always)]
    pub fn first_child(&self, id: WidgetId) -> Option<WidgetId> {
        if !self.is_alive(id) {
            return None;
        }
        self.get_hot(id).and_then(|h| h.first_child)
    }

    /// Retrieve the last child handle of the specified node.
    pub fn last_child(&self, id: WidgetId) -> Option<WidgetId> {
        let mut curr = self.first_child(id)?;
        while let Some(next) = self.next_sibling(curr) {
            curr = next;
        }
        Some(curr)
    }

    /// Retrieve the next sibling handle of the specified node.
    #[inline(always)]
    pub fn next_sibling(&self, id: WidgetId) -> Option<WidgetId> {
        if !self.is_alive(id) {
            return None;
        }
        self.get_hot(id).and_then(|h| h.next_sibling)
    }

    /// Retrieve the previous sibling handle of the specified node.
    #[inline(always)]
    pub fn prev_sibling(&self, id: WidgetId) -> Option<WidgetId> {
        if !self.is_alive(id) {
            return None;
        }
        self.get_hot(id).and_then(|h| h.prev_sibling)
    }

    /// Retrieve the topological depth rank of the specified node.
    #[inline(always)]
    pub fn depth_rank(&self, id: WidgetId) -> Option<u16> {
        if !self.is_alive(id) {
            return None;
        }
        self.get_hot(id).map(|h| h.depth_rank)
    }

    /// Check if `ancestor` is an ancestor of `descendant` (or identical).
    pub fn is_ancestor_of(&self, ancestor: WidgetId, descendant: WidgetId) -> bool {
        if ancestor == descendant {
            return true;
        }
        let mut curr = self.parent(descendant);
        while let Some(p) = curr {
            if p == ancestor {
                return true;
            }
            curr = self.parent(p);
        }
        false
    }

    // --- Tree Hierarchy Mutations ---

    /// Detach a node from its parent and sibling chains, leaving it as an unattached root.
    pub fn detach(&mut self, id: WidgetId) -> Result<(), ArenaError> {
        if !self.is_alive(id) {
            return Err(ArenaError::InvalidNode(id));
        }

        let (parent_opt, prev_opt, next_opt) = {
            let hot = self.get_hot(id).expect("arena invariant");
            (hot.parent, hot.prev_sibling, hot.next_sibling)
        };

        // Update parent's first_child pointer if id was head
        if let Some(parent_id) = parent_opt {
            if self.is_alive(parent_id) {
                let is_first = self
                    .get_hot(parent_id)
                    .expect("arena invariant")
                    .first_child
                    == Some(id);
                if is_first {
                    self.get_hot_mut(parent_id)
                        .expect("arena invariant")
                        .first_child = next_opt;
                }
            }
        }

        // Link prev sibling to next sibling
        if let Some(prev_id) = prev_opt {
            if self.is_alive(prev_id) {
                self.get_hot_mut(prev_id)
                    .expect("arena invariant")
                    .next_sibling = next_opt;
            }
        }

        // Link next sibling to prev sibling
        if let Some(next_id) = next_opt {
            if self.is_alive(next_id) {
                self.get_hot_mut(next_id)
                    .expect("arena invariant")
                    .prev_sibling = prev_opt;
            }
        }

        // Clear id's sibling and parent links
        {
            let hot = self.get_hot_mut(id).expect("arena invariant");
            hot.parent = None;
            hot.prev_sibling = None;
            hot.next_sibling = None;
            hot.depth_rank = 0;
        }

        self.update_subtree_depths(id, 0);
        Ok(())
    }

    /// Append a child node to the end of a parent's children list.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{ArenaError, DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let parent = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let b = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    ///
    /// arena.append_child(parent, a).unwrap();
    /// arena.append_child(parent, b).unwrap();
    /// assert_eq!(arena.children(parent).collect::<Vec<_>>(), vec![a, b]);
    /// assert_eq!(arena.parent(a), Some(parent));
    ///
    /// // Forming a cycle is rejected.
    /// assert_eq!(arena.append_child(a, parent).unwrap_err(), ArenaError::CycleDetected);
    /// ```
    pub fn append_child(&mut self, parent: WidgetId, child: WidgetId) -> Result<(), ArenaError> {
        if !self.is_alive(parent) {
            return Err(ArenaError::InvalidParent(parent));
        }
        if !self.is_alive(child) {
            return Err(ArenaError::InvalidChild(child));
        }
        if parent == child {
            return Err(ArenaError::SelfParenting(parent));
        }
        if self.is_ancestor_of(child, parent) {
            return Err(ArenaError::CycleDetected);
        }

        // Detach child from existing location
        self.detach(child)?;

        let parent_first = self.get_hot(parent).expect("arena invariant").first_child;
        match parent_first {
            None => {
                self.get_hot_mut(parent)
                    .expect("arena invariant")
                    .first_child = Some(child);
                let child_hot = self.get_hot_mut(child).expect("arena invariant");
                child_hot.parent = Some(parent);
                child_hot.prev_sibling = None;
                child_hot.next_sibling = None;
            }
            Some(first) => {
                let mut last = first;
                while let Some(next) = self.next_sibling(last) {
                    last = next;
                }
                self.get_hot_mut(last)
                    .expect("arena invariant")
                    .next_sibling = Some(child);
                let child_hot = self.get_hot_mut(child).expect("arena invariant");
                child_hot.parent = Some(parent);
                child_hot.prev_sibling = Some(last);
                child_hot.next_sibling = None;
            }
        }

        let parent_depth = self.get_hot(parent).expect("arena invariant").depth_rank;
        let child_depth = parent_depth.saturating_add(1);
        self.get_hot_mut(child).expect("arena invariant").depth_rank = child_depth;
        self.update_subtree_depths(child, child_depth);

        Ok(())
    }

    /// Prepend a child node to the beginning of a parent's children list.
    pub fn prepend_child(&mut self, parent: WidgetId, child: WidgetId) -> Result<(), ArenaError> {
        if !self.is_alive(parent) {
            return Err(ArenaError::InvalidParent(parent));
        }
        if !self.is_alive(child) {
            return Err(ArenaError::InvalidChild(child));
        }
        if parent == child {
            return Err(ArenaError::SelfParenting(parent));
        }
        if self.is_ancestor_of(child, parent) {
            return Err(ArenaError::CycleDetected);
        }

        self.detach(child)?;

        let old_first = self.get_hot(parent).expect("arena invariant").first_child;
        self.get_hot_mut(parent)
            .expect("arena invariant")
            .first_child = Some(child);

        let child_hot = self.get_hot_mut(child).expect("arena invariant");
        child_hot.parent = Some(parent);
        child_hot.prev_sibling = None;
        child_hot.next_sibling = old_first;

        if let Some(old_first_id) = old_first {
            self.get_hot_mut(old_first_id)
                .expect("arena invariant")
                .prev_sibling = Some(child);
        }

        let parent_depth = self.get_hot(parent).expect("arena invariant").depth_rank;
        let child_depth = parent_depth.saturating_add(1);
        self.get_hot_mut(child).expect("arena invariant").depth_rank = child_depth;
        self.update_subtree_depths(child, child_depth);

        Ok(())
    }

    /// Insert a node immediately before a target sibling.
    pub fn insert_before(&mut self, target: WidgetId, node: WidgetId) -> Result<(), ArenaError> {
        if !self.is_alive(target) {
            return Err(ArenaError::InvalidTarget(target));
        }
        if !self.is_alive(node) {
            return Err(ArenaError::InvalidNode(node));
        }
        if target == node {
            return Err(ArenaError::InvalidOperation(
                "Cannot insert node before itself",
            ));
        }

        let parent = self.parent(target).ok_or(ArenaError::NoParent(target))?;
        if self.is_ancestor_of(node, parent) {
            return Err(ArenaError::CycleDetected);
        }

        self.detach(node)?;

        let target_prev = self.get_hot(target).expect("arena invariant").prev_sibling;
        {
            let node_hot = self.get_hot_mut(node).expect("arena invariant");
            node_hot.parent = Some(parent);
            node_hot.prev_sibling = target_prev;
            node_hot.next_sibling = Some(target);
        }

        self.get_hot_mut(target)
            .expect("arena invariant")
            .prev_sibling = Some(node);

        if let Some(prev_id) = target_prev {
            self.get_hot_mut(prev_id)
                .expect("arena invariant")
                .next_sibling = Some(node);
        } else {
            self.get_hot_mut(parent)
                .expect("arena invariant")
                .first_child = Some(node);
        }

        let parent_depth = self.get_hot(parent).expect("arena invariant").depth_rank;
        let node_depth = parent_depth.saturating_add(1);
        self.get_hot_mut(node).expect("arena invariant").depth_rank = node_depth;
        self.update_subtree_depths(node, node_depth);

        Ok(())
    }

    /// Insert a node immediately after a target sibling.
    pub fn insert_after(&mut self, target: WidgetId, node: WidgetId) -> Result<(), ArenaError> {
        if !self.is_alive(target) {
            return Err(ArenaError::InvalidTarget(target));
        }
        if !self.is_alive(node) {
            return Err(ArenaError::InvalidNode(node));
        }
        if target == node {
            return Err(ArenaError::InvalidOperation(
                "Cannot insert node after itself",
            ));
        }

        let parent = self.parent(target).ok_or(ArenaError::NoParent(target))?;
        if self.is_ancestor_of(node, parent) {
            return Err(ArenaError::CycleDetected);
        }

        self.detach(node)?;

        let target_next = self.get_hot(target).expect("arena invariant").next_sibling;
        {
            let node_hot = self.get_hot_mut(node).expect("arena invariant");
            node_hot.parent = Some(parent);
            node_hot.prev_sibling = Some(target);
            node_hot.next_sibling = target_next;
        }

        self.get_hot_mut(target)
            .expect("arena invariant")
            .next_sibling = Some(node);

        if let Some(next_id) = target_next {
            self.get_hot_mut(next_id)
                .expect("arena invariant")
                .prev_sibling = Some(node);
        }

        let parent_depth = self.get_hot(parent).expect("arena invariant").depth_rank;
        let node_depth = parent_depth.saturating_add(1);
        self.get_hot_mut(node).expect("arena invariant").depth_rank = node_depth;
        self.update_subtree_depths(node, node_depth);

        Ok(())
    }

    /// Remove a child from a parent node.
    pub fn remove_child(&mut self, parent: WidgetId, child: WidgetId) -> Result<(), ArenaError> {
        if !self.is_alive(parent) {
            return Err(ArenaError::InvalidParent(parent));
        }
        if !self.is_alive(child) {
            return Err(ArenaError::InvalidChild(child));
        }
        if self.parent(child) != Some(parent) {
            return Err(ArenaError::NotAChild { parent, child });
        }

        self.detach(child)
    }

    /// Recursively update depth ranks across all descendants of the specified node.
    fn update_subtree_depths(&mut self, root: WidgetId, root_depth: u16) {
        let mut child_opt = self.first_child(root);
        while let Some(child_id) = child_opt {
            let next_sibling = self.next_sibling(child_id);
            let child_depth = root_depth.saturating_add(1);
            if let Some(hot) = self.get_hot_mut(child_id) {
                hot.depth_rank = child_depth;
            }
            self.update_subtree_depths(child_id, child_depth);
            child_opt = next_sibling;
        }
    }

    // --- Iterators ---

    /// Returns a zero-allocation iterator over the immediate children of `id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let parent = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let b = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.append_child(parent, a).unwrap();
    /// arena.append_child(parent, b).unwrap();
    ///
    /// // Forward iteration yields children in insertion order.
    /// assert_eq!(arena.children(parent).collect::<Vec<_>>(), vec![a, b]);
    /// // `Children` is a double-ended iterator.
    /// assert_eq!(arena.children(parent).rev().collect::<Vec<_>>(), vec![b, a]);
    /// ```
    pub fn children(&self, id: WidgetId) -> Children<'_> {
        let front = self.first_child(id);
        let back = self.last_child(id);
        Children {
            arena: self,
            front,
            back,
        }
    }

    /// Returns a zero-allocation depth-first pre-order iterator over the subtree rooted at `id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// // Build: root
    /// //        ├── a
    /// //        │   └── a1
    /// //        └── b
    /// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let a1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let b = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.append_child(root, a).unwrap();
    /// arena.append_child(a, a1).unwrap();
    /// arena.append_child(root, b).unwrap();
    ///
    /// // Pre-order DFS visits parents before their children.
    /// assert_eq!(arena.iter_subtree(root).collect::<Vec<_>>(), vec![root, a, a1, b]);
    /// ```
    pub fn iter_subtree(&self, id: WidgetId) -> SubtreeIter<'_> {
        SubtreeIter {
            arena: self,
            root: id,
            current: None,
            started: false,
        }
    }

    /// Returns a zero-allocation depth-first iterator visiting all trees in the arena.
    pub fn iter_depth_first(&self) -> DepthFirstIter<'_> {
        DepthFirstIter {
            arena: self,
            root_dense_idx: 0,
            current_subtree: None,
        }
    }

    /// Returns a breadth-first iterator visiting all trees in the arena level-by-level.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// // Build: root
    /// //        ├── a
    /// //        │   └── a1
    /// //        └── b
    /// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let a1 = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let b = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.append_child(root, a).unwrap();
    /// arena.append_child(a, a1).unwrap();
    /// arena.append_child(root, b).unwrap();
    ///
    /// // BFS visits each level fully before descending: root, then a & b, then a1.
    /// assert_eq!(arena.iter_breadth_first().collect::<Vec<_>>(), vec![root, a, b, a1]);
    /// ```
    pub fn iter_breadth_first(&self) -> BreadthFirstIter<'_> {
        let mut queue = VecDeque::new();
        for i in 0..self.hot_nodes.len() {
            let hot = &self.hot_nodes[i];
            if hot.parent.is_none() && hot.prev_sibling.is_none() {
                let slot_idx = self.dense_to_slot[i];
                let slot = self.slots[slot_idx as usize];
                // Generation is never zero for an active slot (see arena docs).
                queue.push_back(WidgetId::from_parts(slot_idx, slot.generation));
            }
        }
        BreadthFirstIter { arena: self, queue }
    }

    /// Returns a breadth-first iterator visiting the subtree rooted at `root` level-by-level.
    pub fn iter_subtree_breadth_first(&self, root: WidgetId) -> BreadthFirstIter<'_> {
        let mut queue = VecDeque::new();
        if self.is_alive(root) {
            queue.push_back(root);
        }
        BreadthFirstIter { arena: self, queue }
    }

    // --- Compaction and Reader Synchronization ---

    /// Synchronizes with active reader leases before commencing arena compaction.
    ///
    /// Blocks until all readers drop their leases or the configured timeout elapses,
    /// in which case stale reader leases are forcibly reclaimed and the epoch is incremented.
    pub fn begin_compaction(&mut self, fence: &FrameFence) -> Result<(), ArenaError> {
        fence.mark_compaction_start();
        fence.wait_for_quiescence();
        Ok(())
    }

    /// Concludes an active compaction pass, clearing the compaction flag.
    pub fn end_compaction(&mut self, fence: &FrameFence) {
        fence.mark_compaction_end();
    }

    /// Shrinks unused capacity across all arena buffers to compact memory during idle periods.
    pub fn shrink_to_fit_idle(&mut self) {
        if self.dense_to_slot.is_empty() {
            self.slots.clear();
            self.free_slots.clear();
        } else {
            let max_active_slot = self.dense_to_slot.iter().copied().max().unwrap_or(0);
            let needed_slots = (max_active_slot + 1) as usize;
            if needed_slots < self.slots.len() {
                self.slots.truncate(needed_slots);
                self.free_slots
                    .retain(|&slot_idx| slot_idx <= max_active_slot);
            }
        }

        self.hot_nodes.shrink_to_fit();
        self.cold_nodes.shrink_to_fit();
        self.dense_to_slot.shrink_to_fit();
        self.slots.shrink_to_fit();
        self.free_slots.shrink_to_fit();
    }

    /// Coordinates compaction synchronization via FrameFence and executes idle shrink_to_fit.
    pub fn compact_and_shrink_idle(&mut self, fence: &FrameFence) -> Result<(), ArenaError> {
        self.begin_compaction(fence)?;
        self.shrink_to_fit_idle();
        self.end_compaction(fence);
        Ok(())
    }
}

// --- Iterator Implementations ---

/// Double-ended iterator over the direct children of a widget node.
///
/// # Examples
///
/// ```
/// use martensite_core::{DummyWidget, HotNode, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// let parent = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// let b = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// arena.append_child(parent, a).unwrap();
/// arena.append_child(parent, b).unwrap();
///
/// let children = arena.children(parent);
/// assert_eq!(children.count(), 2);
/// ```
#[derive(Clone, Debug)]
pub struct Children<'a> {
    arena: &'a WidgetArena,
    front: Option<WidgetId>,
    back: Option<WidgetId>,
}

impl<'a> Iterator for Children<'a> {
    type Item = WidgetId;

    fn next(&mut self) -> Option<Self::Item> {
        let curr = self.front?;
        if self.front == self.back {
            self.front = None;
            self.back = None;
        } else {
            self.front = self.arena.next_sibling(curr);
        }
        Some(curr)
    }
}

impl<'a> DoubleEndedIterator for Children<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let curr = self.back?;
        if self.front == self.back {
            self.front = None;
            self.back = None;
        } else {
            self.back = self.arena.prev_sibling(curr);
        }
        Some(curr)
    }
}

impl<'a> FusedIterator for Children<'a> {}

/// Zero-allocation pre-order depth-first traversal iterator over a node subtree.
///
/// # Examples
///
/// ```
/// use martensite_core::{DummyWidget, HotNode, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// let child = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// arena.append_child(root, child).unwrap();
///
/// let mut it = arena.iter_subtree(root);
/// assert_eq!(it.next(), Some(root));
/// assert_eq!(it.next(), Some(child));
/// assert_eq!(it.next(), None);
/// ```
#[derive(Clone, Debug)]
pub struct SubtreeIter<'a> {
    arena: &'a WidgetArena,
    root: WidgetId,
    current: Option<WidgetId>,
    started: bool,
}

impl<'a> Iterator for SubtreeIter<'a> {
    type Item = WidgetId;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.started {
            self.started = true;
            if self.arena.is_alive(self.root) {
                self.current = Some(self.root);
                return Some(self.root);
            } else {
                return None;
            }
        }

        let curr = self.current?;

        // 1. Visit first child if available
        if let Some(child) = self.arena.first_child(curr) {
            self.current = Some(child);
            return Some(child);
        }

        // 2. Walk up parent chain looking for an ancestor's next sibling
        let mut node = curr;
        loop {
            if node == self.root {
                self.current = None;
                return None;
            }
            if let Some(sibling) = self.arena.next_sibling(node) {
                self.current = Some(sibling);
                return Some(sibling);
            }
            match self.arena.parent(node) {
                Some(parent) => node = parent,
                None => {
                    self.current = None;
                    return None;
                }
            }
        }
    }
}

impl<'a> FusedIterator for SubtreeIter<'a> {}

/// Zero-allocation depth-first iterator traversing all trees in the arena.
///
/// # Examples
///
/// ```
/// use martensite_core::{DummyWidget, HotNode, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// let child = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// arena.append_child(root, child).unwrap();
///
/// // Visits every tree in the arena in depth-first pre-order.
/// assert_eq!(arena.iter_depth_first().collect::<Vec<_>>(), vec![root, child]);
/// ```
#[derive(Clone, Debug)]
pub struct DepthFirstIter<'a> {
    arena: &'a WidgetArena,
    root_dense_idx: usize,
    current_subtree: Option<SubtreeIter<'a>>,
}

impl<'a> Iterator for DepthFirstIter<'a> {
    type Item = WidgetId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut subtree) = self.current_subtree {
                if let Some(id) = subtree.next() {
                    return Some(id);
                }
            }

            if self.root_dense_idx >= self.arena.hot_nodes.len() {
                self.current_subtree = None;
                return None;
            }

            let dense_idx = self.root_dense_idx;
            self.root_dense_idx += 1;

            let slot_idx = self.arena.dense_to_slot[dense_idx];
            let slot = self.arena.slots[slot_idx as usize];
            // Generation is never zero for an active slot (see arena docs).
            let id = WidgetId::from_parts(slot_idx, slot.generation);
            let hot = &self.arena.hot_nodes[dense_idx];

            if hot.parent.is_none() && hot.prev_sibling.is_none() {
                self.current_subtree = Some(self.arena.iter_subtree(id));
            }
        }
    }
}

impl<'a> FusedIterator for DepthFirstIter<'a> {}

/// Breadth-first iterator traversing trees level-by-level using a FIFO queue.
///
/// # Examples
///
/// ```
/// use martensite_core::{DummyWidget, HotNode, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// let a = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// arena.append_child(root, a).unwrap();
///
/// // `iter_subtree_breadth_first` walks a single subtree level-by-level.
/// assert_eq!(arena.iter_subtree_breadth_first(root).collect::<Vec<_>>(), vec![root, a]);
/// ```
#[derive(Clone, Debug)]
pub struct BreadthFirstIter<'a> {
    arena: &'a WidgetArena,
    queue: VecDeque<WidgetId>,
}

impl<'a> Iterator for BreadthFirstIter<'a> {
    type Item = WidgetId;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.queue.pop_front()?;
        let mut child = self.arena.first_child(id);
        while let Some(c) = child {
            self.queue.push_back(c);
            child = self.arena.next_sibling(c);
        }
        Some(id)
    }
}

impl<'a> FusedIterator for BreadthFirstIter<'a> {}
