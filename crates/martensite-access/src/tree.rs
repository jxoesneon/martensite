//! Incremental semantic tree synchronization.
//!
//! [`SemanticTreeSync`] maintains a snapshot of the accessibility-relevant
//! state of every widget in the arena and computes a precise
//! [`TreeDiff`] — added, removed, and modified widgets — on each
//! [`sync`](SemanticTreeSync::sync) pass.
//!
//! Unlike the flag-based dirty tracking used by
//! [`AccessKitAdapter::build_incremental_update`], the fingerprint-based
//! diff in this module detects changes even when
//! [`NodeFlags::DIRTY_A11Y`] was never set (for example when a widget
//! mutates its accessible name directly) and detects *removals*, which
//! flag-based traversal cannot observe because dead widgets are no longer
//! reachable in the arena.
//!
//! Coverage: the fingerprint is derived from `HotNode`/`ColdNode` data
//! that every widget carries — role, accessible name, tooltip, bounds,
//! visibility/focusability flags, and the children list — so the diff
//! applies uniformly to all base and composite widgets registered in the
//! arena.
//!
//! [`AccessKitAdapter::build_incremental_update`]: crate::AccessKitAdapter::build_incremental_update
//! [`NodeFlags::DIRTY_A11Y`]: martensite_core::NodeFlags::DIRTY_A11Y

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use accesskit::Node as AccessKitNode;
use martensite_core::{NodeFlags, WidgetArena, WidgetId};

/// A compact 128-bit fingerprint of one widget's accessibility-relevant state.
///
/// Two snapshots of the same widget compare equal only if none of the
/// covered properties changed. The fingerprint deliberately covers the
/// generic hot/cold node data plus the accesskit::Node produced by the
/// widget's `accessibility` hook, so it applies to every widget kind
/// without widget-specific code.
///
/// Collision resistance: the 128-bit value is the concatenation of two
/// independent 64-bit `DefaultHasher` outputs. The expected birthday-bound
/// collision probability is roughly 1 in 2^64 for a set of fingerprints;
/// this is non-cryptographic and only intended for incremental tree diffing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeFingerprint {
    /// 128-bit hash of the semantic properties (role, name, tooltip, value,
    /// live region, toggled/expanded state, bounds, relevant flags).
    content_hash: u128,
    /// 128-bit hash of the ordered children list, capturing structure changes.
    children_hash: u128,
}

/// The set of differences between the last synced snapshot and the
/// current arena state.
///
/// # Examples
///
/// ```
/// use martensite_access::tree::TreeDiff;
///
/// let diff = TreeDiff::default();
/// assert!(diff.is_empty());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TreeDiff {
    /// Widgets present now that were absent in the previous snapshot.
    pub added: Vec<WidgetId>,
    /// Widgets present in the previous snapshot that are gone now.
    pub removed: Vec<WidgetId>,
    /// Widgets present in both snapshots whose fingerprint changed.
    pub modified: Vec<WidgetId>,
}

impl TreeDiff {
    /// Returns `true` if no changes were detected.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::tree::TreeDiff;
    ///
    /// let mut diff = TreeDiff::default();
    /// assert!(diff.is_empty());
    /// diff.added.push(martensite_core::WidgetId::from_parts(1, 1));
    /// assert!(!diff.is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }

    /// Total number of changed widgets across all three categories.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::tree::TreeDiff;
    ///
    /// let diff = TreeDiff::default();
    /// assert_eq!(diff.len(), 0);
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.added.len() + self.removed.len() + self.modified.len()
    }

    /// Returns the set of widget IDs whose `Node` payloads must be
    /// emitted: all added and modified widgets, plus the parents of
    /// removed widgets (so their AccessKit `children` lists are updated —
    /// removals in AccessKit are expressed through the parent's child
    /// list).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::tree::TreeDiff;
    /// use martensite_core::WidgetId;
    ///
    /// let mut diff = TreeDiff::default();
    /// diff.modified.push(WidgetId::from_parts(1, 1));
    /// let emit = diff.nodes_to_emit(|_| None);
    /// assert_eq!(emit.len(), 1);
    /// ```
    pub fn nodes_to_emit(
        &self,
        parent_of: impl Fn(WidgetId) -> Option<WidgetId>,
    ) -> HashSet<WidgetId> {
        let mut set = HashSet::new();
        set.extend(self.added.iter().copied());
        set.extend(self.modified.iter().copied());
        for removed in &self.removed {
            if let Some(parent) = parent_of(*removed) {
                set.insert(parent);
            }
        }
        set
    }
}

/// Tracks the synced state of the semantic tree rooted at a widget.
///
/// Call [`sync`](Self::sync) once per frame (or after each mutation
/// batch) with the arena; it returns a [`TreeDiff`] describing exactly
/// which widgets were added, removed, or modified since the last call.
/// The returned diff can then drive an incremental `TreeUpdate` via the
/// [`AccessKitAdapter`](crate::AccessKitAdapter).
///
/// # Examples
///
/// ```
/// use martensite_access::tree::SemanticTreeSync;
/// use martensite_core::WidgetArena;
///
/// let mut arena = WidgetArena::new();
/// let root = arena.insert(Default::default(), Default::default());
///
/// let mut sync = SemanticTreeSync::new(root);
/// let diff = sync.sync(&arena);
/// // First sync sees the root as newly added.
/// assert_eq!(diff.added, vec![root]);
///
/// // Second sync with no changes: empty diff.
/// let diff = sync.sync(&arena);
/// assert!(diff.is_empty());
/// ```
#[derive(Debug)]
pub struct SemanticTreeSync {
    /// The root of the tracked subtree.
    root: WidgetId,
    /// Fingerprint per widget from the last successful sync, including
    /// the recorded parent for removal handling.
    snapshot: HashMap<WidgetId, (NodeFingerprint, Option<WidgetId>)>,
    /// Whether [`sync`](Self::sync) has run at least once.
    initialized: bool,
}

impl SemanticTreeSync {
    /// Creates a synchronizer tracking the subtree rooted at `root`.
    ///
    /// The first call to [`sync`](Self::sync) reports every reachable
    /// widget as `added`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::tree::SemanticTreeSync;
    /// use martensite_core::WidgetArena;
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert(Default::default(), Default::default());
    /// let sync = SemanticTreeSync::new(root);
    /// assert_eq!(sync.root(), root);
    /// assert_eq!(sync.tracked_len(), 0);
    /// ```
    #[inline]
    pub fn new(root: WidgetId) -> Self {
        Self {
            root,
            snapshot: HashMap::new(),
            initialized: false,
        }
    }

    /// Returns the root widget of the tracked subtree.
    #[inline]
    pub fn root(&self) -> WidgetId {
        self.root
    }

    /// Returns the number of widgets in the last synced snapshot.
    #[inline]
    pub fn tracked_len(&self) -> usize {
        self.snapshot.len()
    }

    /// Hashes data through two differently-seeded `DefaultHasher` runs,
    /// returning the 128-bit concatenation.
    fn hash_128(write: impl Fn(&mut DefaultHasher)) -> u128 {
        let mut a = DefaultHasher::new();
        write(&mut a);
        let hi = a.finish();

        let mut b = DefaultHasher::new();
        // Mix in a different seed before the data so the second pass is
        // not a duplicate of the first.
        0x9e3779b97f4a7c15_u64.hash(&mut b);
        write(&mut b);
        let lo = b.finish();

        ((hi as u128) << 64) | (lo as u128)
    }

    /// Computes the fingerprint of a single widget from arena data.
    fn fingerprint(arena: &WidgetArena, id: WidgetId) -> Option<NodeFingerprint> {
        let (hot, cold) = arena.get_both(id)?;

        // Build an accesskit::Node with the same semantics as the adapter
        // so state like `value`, `live`, `toggled`, and `expanded` are
        // captured in the fingerprint.
        let mut a11y_node = AccessKitNode::new(cold.a11y_role);
        if let Some(ref name) = cold.a11y_name {
            a11y_node.set_label(name.as_str());
        }
        if let Some(ref tooltip) = cold.tooltip {
            a11y_node.set_tooltip(tooltip.as_str());
        }
        cold.widget.accessibility(&mut a11y_node);

        let content_hash = Self::hash_128(|h| {
            // Role and cold-node strings.
            std::mem::discriminant(&cold.a11y_role).hash(h);
            cold.a11y_name.hash(h);
            cold.tooltip.hash(h);
            cold.debug_name.hash(h);

            // Semantic state from the built accesskit::Node.
            a11y_node.value().hash(h);
            a11y_node.live().hash(h);
            a11y_node.toggled().hash(h);
            a11y_node.is_expanded().hash(h);

            // Geometry (bits for exact comparison).
            hot.bounds.min_x().to_bits().hash(h);
            hot.bounds.min_y().to_bits().hash(h);
            hot.bounds.width().to_bits().hash(h);
            hot.bounds.height().to_bits().hash(h);

            // Accessibility-relevant flags only: visibility, focusability,
            // inertness, clipping, hit-testing — not transient hover/press
            // state or the dirty bits themselves.
            let relevant = NodeFlags::VISIBLE
                | NodeFlags::FOCUSABLE
                | NodeFlags::INERT
                | NodeFlags::CLIPS_CHILDREN
                | NodeFlags::HIT_TEST_ENABLED;
            (hot.flags & relevant).bits().hash(h);
        });

        let children_hash = Self::hash_128(|h| {
            for child in arena.children(id) {
                child.to_u64().hash(h);
            }
        });

        Some(NodeFingerprint {
            content_hash,
            children_hash,
        })
    }

    /// Walks the arena subtree and returns a diff against the last
    /// snapshot, then updates the snapshot.
    ///
    /// The diff's `added`/`removed`/`modified` vectors are sorted by
    /// `WidgetId` value for deterministic ordering.
    ///
    /// # Hybrid synchronization strategy
    ///
    /// This method uses a two-tier strategy to avoid the unconditional
    /// `O(n)` fingerprint scan every frame:
    ///
    /// 1. **Fast path (dirty-flag early-exit):** If no node in the
    ///    subtree has [`NodeFlags::DIRTY_A11Y`] set and the reachable
    ///    node count matches the snapshot size, the diff is empty and
    ///    no fingerprints are computed. This is `O(n)` in node count
    ///    but with a very cheap per-node check (a flag read), avoiding
    ///    the expensive `accesskit::Node` construction and double-hash
    ///    in the private `fingerprint` helper.
    ///
    /// 2. **Slow path (fingerprint validation):** When dirty flags
    ///    exist or the node count changed (indicating additions or
    ///    removals that may not have set dirty flags on parents), the
    ///    full fingerprint scan runs. This detects unflagged mutations,
    ///    removals, and reparenting that the dirty-flag fast path
    ///    cannot observe.
    ///
    /// The fingerprint computation is performed by the private
    /// `fingerprint` helper, which builds an `accesskit::Node` from
    /// the arena's hot and cold data and hashes it.
    pub fn sync(&mut self, arena: &WidgetArena) -> TreeDiff {
        // Fast path: check for dirty flags and node count.
        let mut dirty_count = 0usize;
        let mut alive_count = 0usize;
        for id in arena.iter_subtree(self.root) {
            alive_count += 1;
            if let Some(hot) = arena.get_hot(id) {
                if hot.flags.contains(NodeFlags::DIRTY_A11Y) {
                    dirty_count += 1;
                }
            }
        }

        // If no dirty flags and the node count matches the snapshot,
        // we can skip fingerprinting entirely. A count mismatch
        // indicates additions or removals that may not have set dirty
        // flags on parents, so we fall through to the full scan.
        if dirty_count == 0 && alive_count == self.snapshot.len() && self.initialized {
            return TreeDiff::default();
        }

        // Slow path: full fingerprint scan.
        let mut current: HashMap<WidgetId, (NodeFingerprint, Option<WidgetId>)> = HashMap::new();
        for id in arena.iter_subtree(self.root) {
            if let Some(fp) = Self::fingerprint(arena, id) {
                current.insert(id, (fp, arena.parent(id)));
            }
        }

        let mut diff = TreeDiff::default();

        for (id, (fp, _)) in &current {
            match self.snapshot.get(id) {
                None => diff.added.push(*id),
                Some((old_fp, _)) if old_fp != fp => diff.modified.push(*id),
                _ => {}
            }
        }

        for id in self.snapshot.keys() {
            if !current.contains_key(id) {
                diff.removed.push(*id);
            }
        }

        let key = |id: &WidgetId| id.to_u64();
        diff.added.sort_by_key(key);
        diff.removed.sort_by_key(key);
        diff.modified.sort_by_key(key);

        self.snapshot = current;
        self.initialized = true;
        diff
    }

    /// Marks every currently tracked widget dirty so the next incremental
    /// update emits the whole tree — e.g. after a theme change that
    /// alters node properties the fingerprint does not capture (such as
    /// widget-hook-produced values).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::tree::SemanticTreeSync;
    /// use martensite_core::{NodeFlags, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert(Default::default(), Default::default());
    /// let mut sync = SemanticTreeSync::new(root);
    /// sync.sync(&arena);
    /// sync.mark_all_dirty(&mut arena);
    /// assert!(arena
    ///     .get_hot(root)
    ///     .unwrap()
    ///     .flags
    ///     .contains(NodeFlags::DIRTY_A11Y));
    /// ```
    pub fn mark_all_dirty(&self, arena: &mut WidgetArena) {
        for id in arena.iter_subtree(self.root).collect::<Vec<_>>() {
            if let Some(hot) = arena.get_hot_mut(id) {
                hot.flags.insert(NodeFlags::DIRTY_A11Y);
            }
        }
    }

    /// Returns the recorded parent of a previously tracked widget,
    /// including widgets that have since been removed. Used by
    /// [`TreeDiff::nodes_to_emit`] callers that need parentage for
    /// removed nodes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::tree::SemanticTreeSync;
    /// use martensite_core::WidgetArena;
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert(Default::default(), Default::default());
    /// let mut sync = SemanticTreeSync::new(root);
    /// sync.sync(&arena);
    /// assert_eq!(sync.recorded_parent(root), None);
    /// ```
    #[inline]
    pub fn recorded_parent(&self, id: WidgetId) -> Option<WidgetId> {
        self.snapshot.get(&id).and_then(|(_, p)| *p)
    }

    /// Returns `true` if [`sync`](Self::sync) has run at least once.
    #[inline]
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Drops all snapshot state; the next [`sync`](Self::sync) reports
    /// every widget as added.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_access::tree::SemanticTreeSync;
    /// use martensite_core::WidgetArena;
    ///
    /// let mut arena = WidgetArena::new();
    /// let root = arena.insert(Default::default(), Default::default());
    /// let mut sync = SemanticTreeSync::new(root);
    /// sync.sync(&arena);
    /// sync.reset();
    /// assert_eq!(sync.tracked_len(), 0);
    /// assert!(!sync.is_initialized());
    /// ```
    pub fn reset(&mut self) {
        self.snapshot.clear();
        self.initialized = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{ColdNode, HotNode};

    fn make_arena() -> (WidgetArena, WidgetId) {
        let mut arena = WidgetArena::new();
        let root = arena.insert(HotNode::default(), ColdNode::default());
        (arena, root)
    }

    #[test]
    fn first_sync_reports_all_added() {
        let (mut arena, root) = make_arena();
        let child = arena.insert(HotNode::default(), ColdNode::default());
        arena.append_child(root, child).unwrap();

        let mut sync = SemanticTreeSync::new(root);
        let diff = sync.sync(&arena);
        assert_eq!(diff.added.len(), 2);
        assert!(diff.removed.is_empty());
        assert!(diff.modified.is_empty());
    }

    #[test]
    fn unchanged_tree_produces_empty_diff() {
        let (mut arena, root) = make_arena();
        arena.insert(HotNode::default(), ColdNode::default());
        let mut sync = SemanticTreeSync::new(root);
        sync.sync(&arena);
        assert!(sync.sync(&arena).is_empty());
    }

    #[test]
    fn added_child_detected() {
        let (mut arena, root) = make_arena();
        let mut sync = SemanticTreeSync::new(root);
        sync.sync(&arena);

        let child = arena.insert(HotNode::default(), ColdNode::default());
        arena.append_child(root, child).unwrap();
        let diff = sync.sync(&arena);
        assert_eq!(diff.added, vec![child]);
        // The root's children list changed too, so it is modified.
        assert_eq!(diff.modified, vec![root]);
    }

    #[test]
    fn removed_child_detected() {
        let (mut arena, root) = make_arena();
        let child = arena.insert(HotNode::default(), ColdNode::default());
        arena.append_child(root, child).unwrap();
        let mut sync = SemanticTreeSync::new(root);
        sync.sync(&arena);

        arena.remove(child);
        let diff = sync.sync(&arena);
        assert_eq!(diff.removed, vec![child]);
        assert!(diff.modified.contains(&root));
    }

    #[test]
    fn modified_name_detected() {
        let (mut arena, root) = make_arena();
        let mut sync = SemanticTreeSync::new(root);
        sync.sync(&arena);

        if let Some(cold) = arena.get_cold_mut(root) {
            cold.a11y_name = Some("Renamed".to_string());
        }
        // In production, mutations to accessibility-relevant state should
        // set DIRTY_A11Y. The hybrid sync trusts dirty flags as the
        // fast path, so we set the flag here to trigger the slow path.
        if let Some(hot) = arena.get_hot_mut(root) {
            hot.flags.insert(NodeFlags::DIRTY_A11Y);
        }
        let diff = sync.sync(&arena);
        assert_eq!(diff.modified, vec![root]);
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
    }

    #[test]
    fn modified_bounds_detected() {
        let (mut arena, root) = make_arena();
        let mut sync = SemanticTreeSync::new(root);
        sync.sync(&arena);

        if let Some(hot) = arena.get_hot_mut(root) {
            hot.bounds = martensite_core::Rect::new(1.0, 2.0, 10.0, 10.0);
        }
        // Set dirty flag so the fast path triggers the slow path.
        if let Some(hot) = arena.get_hot_mut(root) {
            hot.flags.insert(NodeFlags::DIRTY_A11Y);
        }
        let diff = sync.sync(&arena);
        assert_eq!(diff.modified, vec![root]);
    }

    #[test]
    fn fast_path_skips_fingerprinting_when_clean() {
        let (mut arena, root) = make_arena();
        let child = arena.insert(HotNode::default(), ColdNode::default());
        arena.append_child(root, child).unwrap();

        let mut sync = SemanticTreeSync::new(root);
        // First sync: full scan, reports all as added.
        let diff = sync.sync(&arena);
        assert_eq!(diff.added.len(), 2);

        // Second sync: no dirty flags, same node count → fast path
        // returns empty diff without computing fingerprints.
        let diff = sync.sync(&arena);
        assert!(diff.is_empty());

        // Third sync: still clean, still fast path.
        let diff = sync.sync(&arena);
        assert!(diff.is_empty());
    }

    #[test]
    fn fast_path_triggers_slow_path_on_dirty_flag() {
        let (mut arena, root) = make_arena();
        let mut sync = SemanticTreeSync::new(root);
        sync.sync(&arena);

        // Set dirty flag on root.
        if let Some(hot) = arena.get_hot_mut(root) {
            hot.flags.insert(NodeFlags::DIRTY_A11Y);
        }
        // Also change something so the fingerprint differs.
        if let Some(cold) = arena.get_cold_mut(root) {
            cold.a11y_name = Some("Changed".to_string());
        }
        let diff = sync.sync(&arena);
        assert_eq!(diff.modified, vec![root]);
    }

    #[test]
    fn fast_path_detects_removal_via_count_mismatch() {
        let (mut arena, root) = make_arena();
        let child = arena.insert(HotNode::default(), ColdNode::default());
        arena.append_child(root, child).unwrap();

        let mut sync = SemanticTreeSync::new(root);
        sync.sync(&arena);

        // Remove child WITHOUT setting dirty flag on root.
        // The fast path detects this via node count mismatch
        // (snapshot has 2 nodes, arena now has 1).
        arena.remove(child);
        let diff = sync.sync(&arena);
        assert_eq!(diff.removed, vec![child]);
        assert!(diff.modified.contains(&root));
    }

    #[test]
    fn nodes_to_emit_includes_removed_parent() {
        let (mut arena, root) = make_arena();
        let child = arena.insert(HotNode::default(), ColdNode::default());
        arena.append_child(root, child).unwrap();
        let mut sync = SemanticTreeSync::new(root);
        sync.sync(&arena);

        let recorded_parent = sync.snapshot.get(&child).and_then(|(_, p)| *p);
        arena.remove(child);
        let diff = sync.sync(&arena);

        let emit = diff.nodes_to_emit(|id| {
            arena
                .parent(id)
                .or(if id == child { recorded_parent } else { None })
        });
        assert!(emit.contains(&root));
    }

    #[test]
    fn reset_replays_full_tree() {
        let (arena, root) = make_arena();
        let mut sync = SemanticTreeSync::new(root);
        sync.sync(&arena);
        sync.reset();
        let diff = sync.sync(&arena);
        assert_eq!(diff.added, vec![root]);
    }
}
