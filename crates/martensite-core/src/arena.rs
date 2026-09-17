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
use std::collections::{HashSet, VecDeque};
use std::iter::FusedIterator;
use std::time::Duration;

use crate::fence::FrameFence;
use crate::id::WidgetId;
use crate::node::{ColdNode, HotNode, NodeFlags};
use crate::overlay::OverlayLayer;
use crate::paint::PaintList;
use crate::widget::{EventContext, EventResponse, PaintContext, Widget, WidgetEvent};

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
    pub(crate) slots: Vec<Slot>,
    /// Dense cache-line aligned hot node records.
    pub(crate) hot_nodes: Vec<HotNode>,
    /// Parallel cold node storage (widgets, metadata, accessibility).
    pub(crate) cold_nodes: Vec<ColdNode>,
    /// Reverse mapping from dense index to sparse slot index.
    pub(crate) dense_to_slot: Vec<u32>,
    /// Strict FIFO queue distributing recycled slot indices.
    pub(crate) free_slots: VecDeque<u32>,
    /// In-window popup layer owned by the arena. Popups are painted
    /// after arena content in [`build_paint_list`](Self::build_paint_list),
    /// offered input before arena hit-testing by `martensite-window`'s
    /// `EventRouter`, and emitted into the AccessKit tree by
    /// `martensite-access`'s `AccessKitAdapter`.
    overlay: OverlayLayer,
    /// The most recent widget that asked for keyboard focus —
    /// drained by [`take_focus_request`](Self::take_focus_request).
    pending_focus: Option<WidgetId>,
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
            overlay: OverlayLayer::new(),
            pending_focus: None,
        }
    }

    /// Returns the arena-owned in-window [`OverlayLayer`].
    ///
    /// The layer holds popups opened by widgets (e.g. `Dropdown`,
    /// `Tooltip`) through [`Widget::sync_overlay`]. Popups live outside
    /// the widget hierarchy but participate in painting, routed input,
    /// and accessibility: [`build_paint_list`](Self::build_paint_list)
    /// appends them after arena content, `martensite-window`'s
    /// `EventRouter` offers pointer/scroll/Escape input to the layer
    /// before arena hit-testing, and `martensite-access`'s
    /// `AccessKitAdapter` emits them as top-level virtual nodes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::WidgetArena;
    ///
    /// let arena = WidgetArena::new();
    /// assert!(arena.overlay().is_empty());
    /// ```
    pub fn overlay(&self) -> &OverlayLayer {
        &self.overlay
    }

    /// Returns a mutable reference to the arena-owned [`OverlayLayer`].
    ///
    /// Set the viewport with
    /// [`OverlayLayer::set_viewport`](crate::overlay::OverlayLayer::set_viewport)
    /// before any popup opens so placement clamping has meaningful
    /// bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{Rect, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// arena
    ///     .overlay_mut()
    ///     .set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// ```
    pub fn overlay_mut(&mut self) -> &mut OverlayLayer {
        &mut self.overlay
    }

    /// Drives one frame of overlay synchronization.
    ///
    /// Calls [`Widget::sync_overlay`] on every live widget **and** every
    /// internal child (recursively), so widgets anywhere in the
    /// hierarchy can open, move, dismiss, or observe their popups; then
    /// runs [`OverlayLayer::layout_pass`] to resolve anchors for entries
    /// opened this frame. Entries opened during a widget's sync are
    /// stamped with that widget as their [`owner`](crate::overlay::OverlayEntry::owner) —
    /// see [`Self::remove`] for how the stamp keeps popups from
    /// outliving dead widgets.
    ///
    /// Afterwards, every widget whose popup was opened, closed, or
    /// touched by an event since the last sync is marked
    /// `DIRTY_PAINT | DIRTY_A11Y` — popup open/close changes emitted
    /// accessibility state (`expanded`, `controls`, `described_by`) and
    /// must not wait for a widget event to reach an incremental
    /// `TreeUpdate`.
    ///
    /// Call once per frame after layout and before
    /// painting/accessibility emission — or drive both steps at once
    /// via [`Self::tick`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.sync_overlays(); // no popups — cheap no-op
    /// ```
    pub fn sync_overlays(&mut self) {
        let mut overlay = std::mem::take(&mut self.overlay);
        let open_before: HashSet<u64> = overlay.entries().map(|e| e.id()).collect();
        let ids: Vec<WidgetId> = self.iter_depth_first().collect();
        for id in ids {
            overlay.set_current_owner(Some(id));
            if let Some(cold) = self.get_cold_mut(id) {
                Self::sync_overlay_recursive(cold.widget.as_mut(), &mut overlay);
            }
        }
        overlay.set_current_owner(None);
        overlay.layout_pass();
        // Owners whose popup state changed: entries opened this frame
        // carry a fresh owner stamp; entries closed (outside press,
        // Escape, owner-driven close) or touched by popup events left
        // their owner in the layer's dirty log.
        let mut owners: Vec<WidgetId> = overlay
            .entries()
            .filter(|e| !open_before.contains(&e.id()))
            .filter_map(|e| e.owner())
            .collect();
        for owner in overlay.take_dirty_owners() {
            if !owners.contains(&owner) {
                owners.push(owner);
            }
        }
        self.overlay = overlay;
        for owner in owners {
            self.mark_dirty(owner);
        }
    }

    /// Drives one frame of widget state advancement plus overlay
    /// synchronization — the production frame seam.
    ///
    /// Calls [`Widget::tick`] on every live arena widget and,
    /// recursively, on its internal children; widgets that returned
    /// `true` (animation in flight, delay countdown running) are marked
    /// `DIRTY_PAINT | DIRTY_A11Y`. Then runs [`Self::sync_overlays`] so
    /// effects the tick produced — a `Tooltip` finishing its hover
    /// delay, a `ScrollView` animation completing — reconcile their
    /// popups the same frame.
    ///
    /// Call once per frame from the application/windowing frame loop
    /// (`martensite-access`'s `MartensiteAccessBridge::tick` forwards
    /// here), before `build_paint_list` and before building an
    /// AccessKit `TreeUpdate`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.tick(Duration::from_millis(16)); // drives widgets + overlays
    /// ```
    pub fn tick(&mut self, dt: Duration) {
        let ids: Vec<WidgetId> = self.iter_depth_first().collect();
        for id in ids {
            let dirty = self
                .get_cold_mut(id)
                .map(|cold| Self::tick_recursive(cold.widget.as_mut(), dt))
                .unwrap_or(false);
            if dirty {
                self.mark_dirty(id);
            }
        }
        self.sync_overlays();
    }

    /// Recursive helper for [`tick`](Self::tick): ticks `widget` then
    /// its internal children, `true` if any returned `true`.
    fn tick_recursive(widget: &mut dyn Widget, dt: Duration) -> bool {
        let mut dirty = widget.tick(dt);
        for i in 0..widget.child_count() {
            if let Some(child) = widget.child_mut(i) {
                dirty |= Self::tick_recursive(child, dt);
            }
        }
        dirty
    }

    /// Recursive helper for [`sync_overlays`](Self::sync_overlays).
    fn sync_overlay_recursive(widget: &mut dyn Widget, overlay: &mut OverlayLayer) {
        widget.sync_overlay(overlay);
        for i in 0..widget.child_count() {
            if let Some(child) = widget.child_mut(i) {
                Self::sync_overlay_recursive(child, overlay);
            }
        }
    }

    /// Drains the pending keyboard-focus request, if any.
    ///
    /// A request is recorded when a widget answers an event with
    /// [`EventResponse::CaptureFocus`], or when a `PointerPressed` is
    /// handled by a node carrying [`NodeFlags::FOCUSABLE`]
    /// (press-to-focus). The request names the arena widget that
    /// responded — even when the responder was an internal child, the
    /// request resolves to its arena owner.
    ///
    /// Applying it is the app's responsibility: pass the id to
    /// `martensite-focus`'s `FocusManager::set_focus`, then dispatch
    /// [`WidgetEvent::FocusLost`] to the previously focused widget and
    /// [`WidgetEvent::FocusGained`] to the new one via
    /// [`dispatch_event`](Self::dispatch_event).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, NodeFlags, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let mut hot = HotNode::default();
    /// hot.flags = NodeFlags::VISIBLE | NodeFlags::FOCUSABLE;
    /// let id = arena.insert_with_widget(hot, Box::new(DummyWidget));
    ///
    /// assert_eq!(arena.take_focus_request(), None);
    /// ```
    pub fn take_focus_request(&mut self) -> Option<WidgetId> {
        self.pending_focus.take()
    }

    /// Records a pending focus request for `id` without routing an
    /// event — the arena-level counterpart of
    /// [`EventResponse::CaptureFocus`].
    ///
    /// Used when a *virtual* node (a widget's internal child or popup
    /// content inside an overlay entry) answers an assistive-technology
    /// action by requesting focus: the request must resolve to the
    /// owning arena widget, which the action dispatcher reaches via
    /// this method. Dead widgets are ignored. Drain with
    /// [`take_focus_request`](Self::take_focus_request) and apply
    /// through `martensite-focus`'s `FocusManager`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.request_focus(id);
    /// assert_eq!(arena.take_focus_request(), Some(id));
    /// ```
    pub fn request_focus(&mut self, id: WidgetId) {
        if !self.is_alive(id) {
            return;
        }
        self.pending_focus = Some(id);
        if let Some(h) = self.get_hot_mut(id) {
            h.flags |= NodeFlags::DIRTY_PAINT;
        }
    }

    /// Marks `id` `DIRTY_PAINT | DIRTY_A11Y` — the widget's emitted
    /// appearance and accessibility state are stale and the next frame
    /// must repaint it and include it in an incremental `TreeUpdate`.
    /// Dead widgets are ignored.
    ///
    /// Used by the accessibility action dispatcher when an action is
    /// delivered to a virtual (internal or overlay) target: the
    /// response mutates the owning widget's state without going through
    /// `dispatch_event`, which would have marked it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, NodeFlags, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// arena.mark_dirty(id);
    /// assert!(arena.get_hot(id).unwrap().flags.contains(NodeFlags::DIRTY_A11Y));
    /// ```
    pub fn mark_dirty(&mut self, id: WidgetId) {
        if let Some(h) = self.get_hot_mut(id) {
            h.flags |= NodeFlags::DIRTY_PAINT | NodeFlags::DIRTY_A11Y;
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
    ///
    /// Overlay popups owned by the widget (stamped during
    /// [`sync_overlays`](Self::sync_overlays)) are closed so they cannot
    /// keep painting/hit-testing with a dead owner, and a pending focus
    /// request naming this widget is dropped.
    pub fn remove(&mut self, id: WidgetId) -> Option<(HotNode, ColdNode)> {
        let slot = self.slots.get(id.slot_idx() as usize)?;
        if slot.generation != id.generation() {
            return None;
        }

        // 0. Orphaned state: close popups this widget owns and drop a
        // stale focus request — a dead widget must not leave its popup
        // painting above content or focus a non-existent node.
        self.overlay.close_owner(id);
        if self.pending_focus == Some(id) {
            self.pending_focus = None;
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

    /// Deliver `event` to the widget at `target`, bubbling to arena
    /// ancestors while widgets return [`EventResponse::Ignored`].
    ///
    /// The hit-test resolution (which `WidgetId` receives a positional
    /// event) is the caller's job — `martensite-window`'s `EventRouter`
    /// produces the target. This method owns in-arena propagation:
    ///
    /// - `INERT` nodes are skipped (they ignore input) — the event keeps
    ///   bubbling to the next ancestor.
    /// - [`EventResponse::RequestRepaint`] additionally marks the
    ///   responding node [`NodeFlags::DIRTY_PAINT`].
    /// - Within a node, the widget's own `event` implementation governs
    ///   internal children (the trait default forwards to
    ///   [`Widget::child_mut`] in reverse order,
    ///   gated on [`Widget::child_bounds`]).
    ///
    /// Returns the terminal response, or `Ignored` if the event bubbled
    /// past the root.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec2;
    /// use martensite_core::{
    ///     DummyWidget, EventResponse, HotNode, PointerButton, WidgetArena, WidgetEvent,
    /// };
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    ///
    /// // `DummyWidget` ignores input — the event bubbles past the root.
    /// let event = WidgetEvent::PointerPressed {
    ///     position: Vec2::ZERO,
    ///     button: PointerButton::Primary,
    /// };
    /// assert_eq!(arena.dispatch_event(root, &event), EventResponse::Ignored);
    /// ```
    pub fn dispatch_event(&mut self, target: WidgetId, event: &WidgetEvent) -> EventResponse {
        self.dispatch_event_ex(target, event)
            .map(|(_, response)| response)
            .unwrap_or(EventResponse::Ignored)
    }

    /// Like [`Self::dispatch_event`], but also reports the arena node
    /// that produced the terminal response.
    ///
    /// Event routers need the responder's [`WidgetId`] to apply
    /// [`EventResponse::CapturePointer`] /
    /// [`EventResponse::ReleasePointer`] to the widget that actually
    /// handled the event — which may be an ancestor of the hit target
    /// after bubbling — rather than to the hit target itself.
    ///
    /// Returns `Some((responder, response))` when a widget handled the
    /// event, or `None` if it bubbled past the root.
    pub fn dispatch_event_ex(
        &mut self,
        target: WidgetId,
        event: &WidgetEvent,
    ) -> Option<(WidgetId, EventResponse)> {
        let mut current = Some(target);
        while let Some(id) = current {
            let Some(hot) = self.get_hot(id) else {
                break;
            };
            let bounds = hot.bounds;
            let parent = hot.parent;
            if hot.flags.contains(NodeFlags::INERT) {
                current = parent;
                continue;
            }
            let Some(cold) = self.get_cold_mut(id) else {
                break;
            };
            let mut cx = EventContext { event, bounds };
            let response = cold.widget.event(&mut cx);
            match response {
                EventResponse::Ignored => current = parent,
                EventResponse::CaptureFocus => {
                    // Explicit focus request: record the responder and
                    // repaint so a focus indicator can appear.
                    self.pending_focus = Some(id);
                    if let Some(h) = self.get_hot_mut(id) {
                        h.flags |= NodeFlags::DIRTY_PAINT | NodeFlags::DIRTY_A11Y;
                    }
                    return Some((id, response));
                }
                EventResponse::RequestRepaint
                | EventResponse::CapturePointer
                | EventResponse::ReleasePointer => {
                    // Press-to-focus: a handled press on a focusable
                    // node requests focus implicitly, matching
                    // platform mousedown-to-focus conventions.
                    if matches!(event, WidgetEvent::PointerPressed { .. })
                        && self
                            .get_hot(id)
                            .is_some_and(|h| h.flags.contains(NodeFlags::FOCUSABLE))
                    {
                        self.pending_focus = Some(id);
                    }
                    // A handled event may change emitted accessibility
                    // state (expanded, selected, value) — mark the
                    // responder for re-emission in the next incremental
                    // `TreeUpdate`, alongside the repaint.
                    if let Some(h) = self.get_hot_mut(id) {
                        h.flags |= NodeFlags::DIRTY_PAINT | NodeFlags::DIRTY_A11Y;
                    }
                    return Some((id, response));
                }
                other => {
                    // `Handled` — still honour implicit press-to-focus,
                    // and dirty-mark like the responses above: handled
                    // events routinely mutate emitted a11y state.
                    if matches!(event, WidgetEvent::PointerPressed { .. }) {
                        if let Some(h) = self.get_hot_mut(id) {
                            if h.flags.contains(NodeFlags::FOCUSABLE) {
                                self.pending_focus = Some(id);
                            }
                        }
                    }
                    if let Some(h) = self.get_hot_mut(id) {
                        h.flags |= NodeFlags::DIRTY_PAINT | NodeFlags::DIRTY_A11Y;
                    }
                    return Some((id, other));
                }
            }
        }
        None
    }

    /// Walks a `Widget::child_mut` index path from an arena widget to a
    /// nested internal child, returning it mutably.
    ///
    /// Used to deliver accessibility actions to internal targets: the
    /// adapter's `resolve_internal` yields `(owner, path)` pairs and
    /// this reaches the widget at that path — the internal analogue of
    /// `OverlayLayer::widget_at_mut`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// // `DummyWidget` has no internal children — only the empty path resolves.
    /// assert!(arena.internal_widget_mut(root, &[]).is_some());
    /// assert!(arena.internal_widget_mut(root, &[0]).is_none());
    /// ```
    pub fn internal_widget_mut(
        &mut self,
        owner: WidgetId,
        path: &[u32],
    ) -> Option<&mut dyn crate::Widget> {
        let cold = self.get_cold_mut(owner)?;
        let mut widget: &mut dyn crate::Widget = &mut *cold.widget;
        for &index in path {
            widget = widget.child_mut(index as usize)?;
        }
        Some(widget)
    }

    /// Record the visible subtree rooted at `root` into `list`, in
    /// document paint order.
    ///
    /// For each arena node: invisible subtrees are skipped entirely, the
    /// widget's own `paint` emits its chrome first, then internal
    /// children (via the `Widget::child_count`/`child`/`child_bounds`
    /// protocol), then arena children in sibling order. Popups open in
    /// the arena-owned [`OverlayLayer`]
    /// are appended last — above all window content.
    ///
    /// Every widget's commands are wrapped in
    /// [`PaintCommand::PushScope`](crate::PaintCommand::PushScope) /
    /// [`PaintCommand::PopScope`](crate::PaintCommand::PopScope)
    /// provenance markers — nested so the scope tree mirrors the widget
    /// tree — letting downstream tooling (the paint audit) attribute
    /// findings to the emitting widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{
    ///     DummyWidget, HotNode, NodeFlags, PaintList, WidgetArena,
    /// };
    ///
    /// let mut arena = WidgetArena::new();
    /// let mut hot = HotNode::default();
    /// hot.flags = NodeFlags::VISIBLE;
    /// let root = arena.insert_with_widget(hot, Box::new(DummyWidget));
    ///
    /// let mut list = PaintList::new();
    /// arena.build_paint_list(root, &mut list);
    /// // `DummyWidget` emits no chrome — only its provenance scope.
    /// assert_eq!(list.commands.len(), 2);
    /// ```
    pub fn build_paint_list(&self, root: WidgetId, list: &mut PaintList) {
        self.paint_node(root, list);
        // In-window popups paint above everything else.
        self.overlay.paint(list);
    }

    /// Recursive helper for [`WidgetArena::build_paint_list`].
    fn paint_node(&self, id: WidgetId, list: &mut PaintList) {
        let Some(hot) = self.get_hot(id) else {
            return;
        };
        if !hot.flags.contains(NodeFlags::VISIBLE) {
            return;
        }
        let Some(cold) = self.get_cold(id) else {
            return;
        };

        // Provenance scope — covers this widget's paint commands, its
        // internal children, AND its arena children, so the scope tree
        // mirrors the widget tree exactly. Backends ignore the marker;
        // the audit attributes findings to the innermost scope. The
        // instance-level `ColdNode::debug_name` wins over the widget
        // type's `debug_name` — apps naming nodes get their name.
        list.push_scope(
            Some(id),
            cold.debug_name.unwrap_or_else(|| cold.widget.debug_name()),
            rect_to_kurbo(hot.bounds),
        );
        paint_widget_body(&*cold.widget, hot.bounds, list);

        // Arena children honour the node's `CLIPS_CHILDREN` flag: their
        // paint commands are wrapped in a clip for the node bounds.
        // (`Widget::clips_children` governs *internal* children inside
        // `paint_widget_body`.)
        let clip_children = hot.flags.contains(NodeFlags::CLIPS_CHILDREN);
        if clip_children {
            list.push_clip(rect_to_kurbo(hot.bounds));
        }
        let mut child = hot.first_child;
        while let Some(child_id) = child {
            self.paint_node(child_id, list);
            child = self.get_hot(child_id).and_then(|h| h.next_sibling);
        }
        if clip_children {
            list.pop_clip();
        }
        list.pop_scope();
    }
}

/// Convert a [`crate::Rect`] to the `kurbo` rectangle paint commands use.
pub(crate) fn rect_to_kurbo(rect: crate::Rect) -> kurbo::Rect {
    kurbo::Rect::new(
        f64::from(rect.min_x()),
        f64::from(rect.min_y()),
        f64::from(rect.max_x()),
        f64::from(rect.max_y()),
    )
}

/// Paint a widget and its internal children recursively.
///
/// Emits the widget's own chrome via `paint`, then recurses into each
/// internal child using the child's layout-assigned bounds. Internal
/// children have no arena nodes, so this walk is driven entirely by the
/// `Widget::child_count`/`child`/`child_bounds` protocol. When
/// [`Widget::clips_children`](crate::Widget::clips_children) reports
/// `true` the recursion is wrapped in a
/// [`PaintCommand::ClipRect`](crate::PaintCommand::ClipRect) /
/// [`PaintCommand::PopClip`](crate::PaintCommand::PopClip) pair for
/// `bounds`.
///
/// `pub(crate)` so the [`OverlayLayer`](crate::overlay::OverlayLayer)
/// can paint popup content through the same walk.
pub(crate) fn paint_widget_recursive(
    widget: &dyn crate::Widget,
    bounds: crate::Rect,
    list: &mut PaintList,
) {
    // Scope with no arena handle — callers of this entry point (overlay
    // content) have no `WidgetId` to report. Internal children recurse
    // through this same function and get their own scopes.
    list.push_scope(None, widget.debug_name(), rect_to_kurbo(bounds));
    paint_widget_body(widget, bounds, list);
    list.pop_scope();
}

/// Paint a widget's chrome and internal children *without* a scope —
/// [`WidgetArena::paint_node`] manages the scope itself so it can also
/// cover arena children. [`Widget::clips_children`] wraps the internal
/// children in a clip pair, matching the arena-level `CLIPS_CHILDREN`
/// behaviour.
fn paint_widget_body(widget: &dyn crate::Widget, bounds: crate::Rect, list: &mut PaintList) {
    let mut cx = PaintContext { list, bounds };
    widget.paint(&mut cx);
    let clip = widget.clips_children();
    if clip {
        cx.list.push_clip(rect_to_kurbo(bounds));
    }
    for i in 0..widget.child_count() {
        let (Some(child), Some(child_bounds)) = (widget.child(i), widget.child_bounds(i)) else {
            continue;
        };
        paint_widget_recursive(child, child_bounds, &mut *cx.list);
    }
    if clip {
        cx.list.pop_clip();
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

#[cfg(test)]
mod scope_tests {
    use crate::{
        DummyWidget, HotNode, LayoutConstraints, LayoutContext, NodeFlags, PaintCommand, PaintList,
        Rect, Widget, WidgetArena,
    };
    use glam::Vec2;

    fn visible(flags: NodeFlags) -> HotNode {
        let mut hot = HotNode::default();
        hot.flags |= NodeFlags::VISIBLE | flags;
        hot
    }

    /// Widget with one internal child — exercises the scope nesting of
    /// widget-internal children (no arena nodes of their own).
    struct ParentWithChild {
        child: DummyWidget,
    }

    impl Widget for ParentWithChild {
        fn debug_name(&self) -> &'static str {
            "Parent"
        }
        fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
            Vec2::ZERO
        }
        fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}
        fn child_count(&self) -> usize {
            1
        }
        fn child(&self, i: usize) -> Option<&dyn Widget> {
            (i == 0).then_some(&self.child)
        }
        fn child_bounds(&self, i: usize) -> Option<Rect> {
            (i == 0).then_some(Rect::new(0.0, 0.0, 10.0, 10.0))
        }
    }

    #[test]
    fn paint_list_emits_balanced_scopes() {
        let mut arena = WidgetArena::new();
        let root = arena.insert_with_widget(visible(NodeFlags::empty()), Box::new(DummyWidget));
        let child = arena.insert_with_widget(visible(NodeFlags::empty()), Box::new(DummyWidget));
        arena.append_child(root, child).unwrap();

        let mut list = PaintList::new();
        arena.build_paint_list(root, &mut list);

        let mut depth = 0i32;
        let mut names = Vec::new();
        let mut ids = Vec::new();
        for cmd in &list.commands {
            match cmd {
                PaintCommand::PushScope { id, name, .. } => {
                    depth += 1;
                    names.push(*name);
                    ids.push(*id);
                }
                PaintCommand::PopScope => depth -= 1,
                _ => {}
            }
        }
        assert_eq!(depth, 0, "scopes must balance");
        assert_eq!(names.len(), 2, "root + arena child each open a scope");
        assert!(names.iter().all(|n| n.contains("DummyWidget")));
        assert_eq!(ids, vec![Some(root), Some(child)]);
        // Child scope nests inside the parent's — the scope tree mirrors
        // the widget tree. The second PushScope must precede the first
        // PopScope (child opens before parent closes).
        let first_pop = list
            .commands
            .iter()
            .position(|c| matches!(c, PaintCommand::PopScope))
            .unwrap();
        let second_push = list
            .commands
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c, PaintCommand::PushScope { .. }))
            .nth(1)
            .map(|(i, _)| i)
            .unwrap();
        assert!(second_push < first_pop, "child scope must nest");
    }

    #[test]
    fn cold_node_debug_name_wins_over_type_name() {
        let mut arena = WidgetArena::new();
        let mut hot = visible(NodeFlags::empty());
        hot.bounds = Rect::new(0.0, 0.0, 10.0, 10.0);
        let id = arena.insert_with_widget(hot, Box::new(DummyWidget));
        if let Some(cold) = arena.get_cold_mut(id) {
            cold.debug_name = Some("Process Grid");
        }

        let mut list = PaintList::new();
        arena.build_paint_list(id, &mut list);
        let name = list.commands.iter().find_map(|c| match c {
            PaintCommand::PushScope { name, .. } => Some(*name),
            _ => None,
        });
        assert_eq!(name, Some("Process Grid"));
    }

    #[test]
    fn internal_children_get_own_scopes() {
        let mut arena = WidgetArena::new();
        let root = arena.insert_with_widget(
            visible(NodeFlags::empty()),
            Box::new(ParentWithChild { child: DummyWidget }),
        );

        let mut list = PaintList::new();
        arena.build_paint_list(root, &mut list);

        let scopes: Vec<_> = list
            .commands
            .iter()
            .filter_map(|c| match c {
                PaintCommand::PushScope { id, name, .. } => Some((*id, *name)),
                _ => None,
            })
            .collect();
        assert_eq!(scopes.len(), 2);
        assert_eq!(scopes[0], (Some(root), "Parent"));
        // Internal child: no arena handle, but still named.
        assert_eq!(scopes[1].0, None);
        assert!(scopes[1].1.contains("DummyWidget"));
    }
}
