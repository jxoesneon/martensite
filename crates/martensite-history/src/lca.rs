//! Lowest Common Ancestor tree navigation for branching history graphs.
//!
//! History nodes form a directed tree with parent pointers. The LCA
//! algorithm finds the deepest common ancestor of two nodes, enabling
//! minimal-delta transitions between any two points in history.

use slotmap::{new_key_type, SlotMap};
use smallvec::SmallVec;
use std::collections::HashSet;

new_key_type! {
    /// Opaque identifier for a [`HistoryNode`] within a [`HistoryTree`].
    pub struct NodeId;
}

/// Error returned when a [`NodeId`] does not exist in the history tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeIdError;

impl std::fmt::Display for NodeIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid history node id")
    }
}

impl std::error::Error for NodeIdError {}

/// A single node in the history tree.
///
/// Each node records its parent (except the root), its children for
/// branching navigation, and metadata used by the LCA algorithm and
/// bounded-depth pruning.
#[derive(Debug)]
pub struct HistoryNode {
    /// Parent node, or `None` for the root.
    pub(crate) parent: Option<NodeId>,
    /// Child nodes created by branching edits.
    pub(crate) children: SmallVec<[NodeId; 4]>,
    /// Depth in the tree (root = 0).
    pub(crate) depth: u32,
    /// Monotonically increasing sequence number for LRU pruning.
    #[allow(dead_code)]
    pub(crate) sequence: u64,
    /// Whether this node was the most recently visited node.
    pub(crate) last_visited: u64,
}

impl HistoryNode {
    /// Returns the parent of this node, or `None` if this is the root.
    #[inline]
    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    /// Returns the depth of this node (root = 0).
    #[inline]
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// Returns the children of this node.
    #[inline]
    pub fn children(&self) -> &[NodeId] {
        &self.children
    }
}

/// A directed tree of [`HistoryNode`]s supporting LCA navigation.
///
/// The tree stores nodes in a [`SlotMap`] for stable IDs and O(1)
/// insertion/removal. The root node is created at construction time
/// and represents the initial state before any operations.
///
/// # Node ID Invalidation
///
/// `NodeId`s are generational keys backed by `slotmap`. When nodes are
/// pruned (due to exceeding `max_nodes`), their `NodeId`s become invalid.
/// User code that stores `NodeId`s for later use should re-validate them
/// with [`HistoryTree::node`] before calling methods that accept `NodeId`s.
/// Methods like [`HistoryTree::lca`], [`HistoryTree::path_to`], and
/// [`HistoryTree::set_current`] return `None`/`Err` for invalid IDs rather
/// than panicking.
#[derive(Debug)]
pub struct HistoryTree {
    /// All nodes in the tree.
    pub(crate) nodes: SlotMap<NodeId, HistoryNode>,
    /// The root node (initial state).
    pub(crate) root: NodeId,
    /// The currently active node.
    pub(crate) current: NodeId,
    /// Maximum number of nodes before pruning kicks in.
    pub(crate) max_nodes: usize,
    /// Next sequence number to assign.
    pub(crate) next_sequence: u64,
    /// Next last-visited counter for LRU tracking.
    pub(crate) next_visit: u64,
}

impl HistoryTree {
    /// Creates a new history tree with a root node and the given maximum
    /// node count.
    ///
    /// # Panics
    ///
    /// Panics if `max_nodes` is zero.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_history::HistoryTree;
    ///
    /// let tree = HistoryTree::new(500);
    /// assert_eq!(tree.node_count(), 1);
    /// assert_eq!(tree.current_depth(), 0);
    /// ```
    pub fn new(max_nodes: usize) -> Self {
        assert!(max_nodes >= 2, "max_nodes must be >= 2");
        let mut nodes = SlotMap::with_key();
        let root = nodes.insert(HistoryNode {
            parent: None,
            children: SmallVec::new(),
            depth: 0,
            sequence: 0,
            last_visited: 0,
        });
        Self {
            nodes,
            root,
            current: root,
            max_nodes,
            next_sequence: 1,
            next_visit: 1,
        }
    }

    /// Returns the root node ID.
    #[inline]
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Returns the current node ID.
    #[inline]
    pub fn current(&self) -> NodeId {
        self.current
    }

    /// Returns the total number of nodes in the tree.
    #[inline]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns the depth of the current node.
    #[inline]
    pub fn current_depth(&self) -> u32 {
        self.nodes[self.current].depth
    }

    /// Returns a reference to the node at the given ID.
    ///
    /// Returns `None` if the ID is no longer valid (pruned).
    #[inline]
    pub fn node(&self, id: NodeId) -> Option<&HistoryNode> {
        self.nodes.get(id)
    }

    /// Appends a new child to the current node and makes it the current
    /// node. Returns the new node's ID.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_history::HistoryTree;
    ///
    /// let mut tree = HistoryTree::new(100);
    /// let child = tree.append_child();
    /// assert_eq!(tree.current(), child);
    /// assert_eq!(tree.current_depth(), 1);
    /// ```
    pub fn append_child(&mut self) -> NodeId {
        let parent = self.current;
        let depth = self.nodes[parent].depth + 1;
        let seq = self.next_sequence;
        self.next_sequence += 1;
        let visit = self.next_visit;
        self.next_visit += 1;

        let id = self.nodes.insert(HistoryNode {
            parent: Some(parent),
            children: SmallVec::new(),
            depth,
            sequence: seq,
            last_visited: visit,
        });
        self.nodes[parent].children.push(id);
        self.current = id;

        // Prune if over capacity. The removed NodeIds are discarded
        // here; callers using HistoryLedger handle ops cleanup via
        // the prune_and_collect method.
        if self.nodes.len() > self.max_nodes {
            let _ = self.prune();
        }
        id
    }

    /// Moves the current pointer to the parent of the current node.
    ///
    /// Returns `true` if the move succeeded, `false` if the current
    /// node is the root (has no parent).
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_history::HistoryTree;
    ///
    /// let mut tree = HistoryTree::new(100);
    /// tree.append_child();
    /// assert!(tree.move_to_parent());
    /// assert!(!tree.move_to_parent()); // already at root
    /// ```
    pub fn move_to_parent(&mut self) -> bool {
        let visit = self.next_visit;
        self.next_visit += 1;
        if let Some(parent) = self.nodes[self.current].parent {
            self.nodes[self.current].last_visited = visit;
            self.current = parent;
            self.nodes[parent].last_visited = visit;
            true
        } else {
            false
        }
    }

    /// Moves the current pointer to a specific child of the current node.
    ///
    /// Returns `true` if the child was found and the move succeeded.
    pub fn move_to_child(&mut self, child: NodeId) -> bool {
        let visit = self.next_visit;
        self.next_visit += 1;
        if self.nodes[self.current].children.contains(&child) {
            self.current = child;
            self.nodes[child].last_visited = visit;
            true
        } else {
            false
        }
    }

    /// Moves the current pointer to the given target node.
    ///
    /// This does NOT apply or revert operations — it only moves the
    /// pointer. Use [`HistoryTree::path_to`] to compute the revert/apply
    /// path, or use [`crate::HistoryLedger::jump_to`] for the full
    /// transactional navigation.
    ///
    /// Returns `Err(())` if the target node does not exist.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_history::HistoryTree;
    ///
    /// let mut tree = HistoryTree::new(100);
    /// let child = tree.append_child();
    /// tree.move_to_parent();
    /// assert!(tree.set_current(child).is_ok());
    /// ```
    pub fn set_current(&mut self, target: NodeId) -> Result<(), NodeIdError> {
        if !self.nodes.contains_key(target) {
            return Err(NodeIdError);
        }
        let visit = self.next_visit;
        self.next_visit += 1;
        self.current = target;
        self.nodes[target].last_visited = visit;
        Ok(())
    }

    /// Computes the Lowest Common Ancestor of two nodes.
    ///
    /// The LCA is the deepest node that is an ancestor of both `a` and `b`.
    /// Returns `None` if either node ID is invalid.
    ///
    /// # Algorithm
    ///
    /// 1. Collect the path from `a` to root (inclusive).
    /// 2. Walk from `b` toward root, checking each ancestor against
    ///    the path set from `a`.
    /// 3. The first match is the LCA.
    ///
    /// This is O(depth_a + depth_b) — building the path set from `a`
    /// is O(depth_a), and walking from `b` with HashSet lookups is
    /// O(depth_b). Both are bounded by `max_nodes`.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_history::HistoryTree;
    ///
    /// let mut tree = HistoryTree::new(100);
    /// let root = tree.root();
    /// let a = tree.append_child();
    /// tree.move_to_parent();
    /// let b = tree.append_child();
    /// let lca = tree.lca(a, b).unwrap();
    /// assert_eq!(lca, root);
    /// ```
    pub fn lca(&self, a: NodeId, b: NodeId) -> Option<NodeId> {
        if !self.nodes.contains_key(a) || !self.nodes.contains_key(b) {
            return None;
        }

        // Collect path from a to root into a HashSet for O(1) lookups.
        let mut path_set: HashSet<NodeId> = HashSet::with_capacity(64);
        let mut cur = Some(a);
        while let Some(id) = cur {
            path_set.insert(id);
            cur = self.nodes[id].parent;
        }

        // Walk from b toward root, checking against the path set.
        let mut cur = Some(b);
        while let Some(id) = cur {
            if path_set.contains(&id) {
                return Some(id);
            }
            cur = self.nodes[id].parent;
        }

        // Should never reach here since root is always common.
        // But handle gracefully.
        Some(self.root)
    }

    /// Computes the navigation path from `source` to `target`.
    ///
    /// Returns a [`NavPath`] containing:
    /// - `revert`: nodes to revert (from source up to, but excluding, the LCA)
    /// - `apply`: nodes to apply (from the LCA's child down to, and
    ///   including, the target)
    ///
    /// Returns `None` if either node is invalid.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_history::HistoryTree;
    ///
    /// let mut tree = HistoryTree::new(100);
    /// let root = tree.root();
    /// let a = tree.append_child();
    /// tree.move_to_parent();
    /// let b = tree.append_child();
    ///
    /// let nav = tree.path_to(a, b).unwrap();
    /// assert_eq!(nav.lca, root);
    /// assert_eq!(nav.revert, vec![a]);
    /// assert_eq!(nav.apply, vec![b]);
    /// ```
    pub fn path_to(&self, source: NodeId, target: NodeId) -> Option<NavPath> {
        if !self.nodes.contains_key(source) || !self.nodes.contains_key(target) {
            return None;
        }

        let lca = self.lca(source, target)?;

        // Collect revert path: source → LCA (excluding LCA).
        let mut revert: SmallVec<[NodeId; 64]> = SmallVec::new();
        let mut cur = Some(source);
        while let Some(id) = cur {
            if id == lca {
                break;
            }
            revert.push(id);
            cur = self.nodes[id].parent;
        }

        // Collect apply path: LCA → target (excluding LCA, including target).
        // We walk target → LCA, then reverse.
        let mut apply: SmallVec<[NodeId; 64]> = SmallVec::new();
        let mut cur = Some(target);
        while let Some(id) = cur {
            if id == lca {
                break;
            }
            apply.push(id);
            cur = self.nodes[id].parent;
        }
        apply.reverse();

        Some(NavPath {
            lca,
            revert: revert.into_vec(),
            apply: apply.into_vec(),
        })
    }

    /// Prunes the tree to stay within `max_nodes`.
    ///
    /// Eviction strategy:
    /// 1. Find leaf nodes (no children).
    /// 2. Exclude the current node and its ancestors.
    /// 3. Sort remaining leaves by `last_visited` (ascending = least recently used).
    /// 4. Remove the oldest leaves until under the limit.
    ///
    /// If no prunable leaves exist (all nodes are on the active branch),
    /// the oldest non-ancestor leaf is removed.
    ///
    /// Returns the IDs of all removed nodes so callers can clean up
    /// associated data (e.g., operation entries in a ledger).
    pub fn prune(&mut self) -> Vec<NodeId> {
        let mut removed = Vec::new();
        if self.nodes.len() <= self.max_nodes {
            return removed;
        }

        // Collect ancestor IDs of the current node (protected from pruning).
        let mut protected: SmallVec<[NodeId; 64]> = SmallVec::new();
        let mut cur = Some(self.current);
        while let Some(id) = cur {
            protected.push(id);
            cur = self.nodes[id].parent;
        }

        // Find prunable leaf nodes (no children, not protected).
        let mut leaves: SmallVec<[(u64, NodeId); 64]> = SmallVec::new();
        for (id, node) in self.nodes.iter() {
            if node.children.is_empty() && !protected.contains(&id) {
                leaves.push((node.last_visited, id));
            }
        }

        // Sort by last_visited ascending (LRU first).
        leaves.sort_by_key(|&(visit, _)| visit);

        // Remove oldest leaves until under the limit.
        let to_remove = self.nodes.len() - self.max_nodes;
        for i in 0..to_remove.min(leaves.len()) {
            let (_, leaf_id) = leaves[i];
            // Remove from parent's children list.
            if let Some(parent_id) = self.nodes[leaf_id].parent {
                if let Some(parent) = self.nodes.get_mut(parent_id) {
                    parent.children.retain(|c| *c != leaf_id);
                }
            }
            self.nodes.remove(leaf_id);
            removed.push(leaf_id);
        }

        // If still over (no leaves were prunable), try removing non-leaf
        // nodes that are not protected. This is a fallback.
        while self.nodes.len() > self.max_nodes {
            let mut candidate: Option<(u64, NodeId)> = None;
            for (id, node) in self.nodes.iter() {
                if !protected.contains(&id) && id != self.root
                    && (candidate.is_none() || node.last_visited < candidate.unwrap().0)
                {
                    candidate = Some((node.last_visited, id));
                }
            }
            if let Some((_, id)) = candidate {
                // Remove from parent's children.
                if let Some(parent_id) = self.nodes[id].parent {
                    if let Some(parent) = self.nodes.get_mut(parent_id) {
                        parent.children.retain(|c| *c != id);
                    }
                }
                // Re-parent children to the removed node's parent.
                let children: SmallVec<[NodeId; 4]> = self.nodes[id].children.clone();
                let parent_id = self.nodes[id].parent;
                for child in children {
                    if let Some(child_node) = self.nodes.get_mut(child) {
                        child_node.parent = parent_id;
                        if let Some(parent) = parent_id {
                            self.nodes[parent].children.push(child);
                        }
                    }
                }
                self.nodes.remove(id);
                removed.push(id);
            } else {
                // All remaining nodes are on the active branch (protected).
                // Prune the oldest non-root, non-current ancestor to
                // compress the chain. This "forgets" the oldest operation.
                let mut oldest: Option<(u64, NodeId)> = None;
                for &id in &protected {
                    if id == self.root || id == self.current {
                        continue;
                    }
                    let visit = self.nodes[id].last_visited;
                    if oldest.is_none() || visit < oldest.unwrap().0 {
                        oldest = Some((visit, id));
                    }
                }
                if let Some((_, id)) = oldest {
                    let parent_id = self.nodes[id].parent;
                    let children: SmallVec<[NodeId; 4]> = self.nodes[id].children.clone();
                    // Remove from parent's children.
                    if let Some(pid) = parent_id {
                        if let Some(parent) = self.nodes.get_mut(pid) {
                            parent.children.retain(|c| *c != id);
                        }
                    }
                    // Re-parent children to the removed node's parent.
                    for child in children {
                        if let Some(child_node) = self.nodes.get_mut(child) {
                            child_node.parent = parent_id;
                            // Update depth for the child and its descendants.
                            self.recompute_depth(child);
                            if let Some(pid) = parent_id {
                                self.nodes[pid].children.push(child);
                            }
                        }
                    }
                    self.nodes.remove(id);
                    removed.push(id);
                    // Update protected list.
                    protected.retain(|p| *p != id);
                } else {
                    break;
                }
            }
        }
        removed
    }

    /// Recomputes the depth of a node and all its descendants after
    /// a structural change (re-parenting).
    fn recompute_depth(&mut self, node_id: NodeId) {
        let parent_depth = self.nodes[node_id]
            .parent
            .map(|p| self.nodes[p].depth)
            .unwrap_or(0);
        self.nodes[node_id].depth = parent_depth + 1;
        let children: SmallVec<[NodeId; 4]> = self.nodes[node_id].children.clone();
        for child in children {
            self.recompute_depth(child);
        }
    }

    /// Returns an iterator over all node IDs in the tree.
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.keys()
    }

    /// Returns the number of children of the current node.
    #[inline]
    pub fn current_child_count(&self) -> usize {
        self.nodes[self.current].children.len()
    }

    /// Returns the children of the current node.
    #[inline]
    pub fn current_children(&self) -> &[NodeId] {
        &self.nodes[self.current].children
    }
}

/// A navigation path between two nodes in the history tree.
///
/// Produced by [`HistoryTree::path_to`], this describes the minimal
/// set of operations to revert and apply when navigating from one
/// history node to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavPath {
    /// The Lowest Common Ancestor of the source and target.
    pub lca: NodeId,
    /// Nodes to revert, in order from source toward the LCA
    /// (excluding the LCA itself).
    pub revert: Vec<NodeId>,
    /// Nodes to apply, in order from the LCA's child toward the
    /// target (including the target).
    pub apply: Vec<NodeId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tree_has_root_only() {
        let tree = HistoryTree::new(100);
        assert_eq!(tree.node_count(), 1);
        assert_eq!(tree.current(), tree.root());
        assert_eq!(tree.current_depth(), 0);
    }

    #[test]
    fn append_child_increments_depth() {
        let mut tree = HistoryTree::new(100);
        let child = tree.append_child();
        assert_eq!(tree.current(), child);
        assert_eq!(tree.current_depth(), 1);
        assert_eq!(tree.node_count(), 2);
    }

    #[test]
    fn move_to_parent_returns_false_at_root() {
        let mut tree = HistoryTree::new(100);
        assert!(!tree.move_to_parent());
    }

    #[test]
    fn move_to_parent_from_child() {
        let mut tree = HistoryTree::new(100);
        let child = tree.append_child();
        assert!(tree.move_to_parent());
        assert_eq!(tree.current(), tree.root());
        assert!(tree.move_to_child(child));
        assert_eq!(tree.current(), child);
    }

    #[test]
    fn branching_creates_multiple_children() {
        let mut tree = HistoryTree::new(100);
        let root = tree.root();
        let _a = tree.append_child();
        tree.move_to_parent();
        let _b = tree.append_child();
        tree.move_to_parent();

        assert_eq!(tree.node(root).unwrap().children().len(), 2);
    }

    #[test]
    fn lca_of_siblings_is_parent() {
        let mut tree = HistoryTree::new(100);
        let root = tree.root();
        let a = tree.append_child();
        tree.move_to_parent();
        let b = tree.append_child();

        let lca = tree.lca(a, b).unwrap();
        assert_eq!(lca, root);
    }

    #[test]
    fn lca_of_node_and_descendant_is_node() {
        let mut tree = HistoryTree::new(100);
        let a = tree.append_child();
        let b = tree.append_child();

        let lca = tree.lca(a, b).unwrap();
        assert_eq!(lca, a);
    }

    #[test]
    fn lca_of_same_node_is_itself() {
        let mut tree = HistoryTree::new(100);
        let a = tree.append_child();
        let lca = tree.lca(a, a).unwrap();
        assert_eq!(lca, a);
    }

    #[test]
    fn lca_returns_none_for_invalid_node() {
        let tree = HistoryTree::new(100);
        let invalid = NodeId::default();
        assert!(tree.lca(tree.root(), invalid).is_none());
    }

    #[test]
    fn path_to_siblings_reverts_and_applies() {
        let mut tree = HistoryTree::new(100);
        let root = tree.root();
        let a = tree.append_child();
        tree.move_to_parent();
        let b = tree.append_child();

        let nav = tree.path_to(a, b).unwrap();
        assert_eq!(nav.lca, root);
        assert_eq!(nav.revert, vec![a]);
        assert_eq!(nav.apply, vec![b]);
    }

    #[test]
    fn path_to_descendant_only_applies() {
        let mut tree = HistoryTree::new(100);
        let a = tree.append_child();
        let b = tree.append_child();

        let nav = tree.path_to(a, b).unwrap();
        assert_eq!(nav.lca, a);
        assert!(nav.revert.is_empty());
        assert_eq!(nav.apply, vec![b]);
    }

    #[test]
    fn path_to_ancestor_only_reverts() {
        let mut tree = HistoryTree::new(100);
        let root = tree.root();
        let _a = tree.append_child();
        tree.append_child();

        let nav = tree.path_to(tree.current(), root).unwrap();
        assert_eq!(nav.lca, root);
        assert_eq!(nav.revert.len(), 2);
        assert!(nav.apply.is_empty());
    }

    #[test]
    fn path_to_same_node_is_empty() {
        let mut tree = HistoryTree::new(100);
        let a = tree.append_child();

        let nav = tree.path_to(a, a).unwrap();
        assert_eq!(nav.lca, a);
        assert!(nav.revert.is_empty());
        assert!(nav.apply.is_empty());
    }

    #[test]
    fn prune_removes_oldest_leaves() {
        let mut tree = HistoryTree::new(5);
        // Create 6 nodes: root + 5 children.
        for _ in 0..5 {
            tree.append_child();
            tree.move_to_parent();
        }
        // Should have pruned to 5.
        assert!(tree.node_count() <= 5);
    }

    #[test]
    fn prune_protects_current_branch() {
        let mut tree = HistoryTree::new(4);
        // Create a deep branch.
        tree.append_child();
        tree.append_child();
        tree.append_child();
        // Create some side branches.
        tree.move_to_parent();
        tree.append_child();
        tree.move_to_parent();
        tree.append_child();

        // Current should still be valid.
        let current = tree.current();
        assert!(tree.node(current).is_some());
    }

    #[test]
    fn set_current_to_invalid_returns_err() {
        let mut tree = HistoryTree::new(100);
        let invalid = NodeId::default();
        assert!(tree.set_current(invalid).is_err());
    }

    #[test]
    fn set_current_to_valid_succeeds() {
        let mut tree = HistoryTree::new(100);
        let a = tree.append_child();
        tree.move_to_parent();
        assert!(tree.set_current(a).is_ok());
        assert_eq!(tree.current(), a);
    }

    #[test]
    fn deep_tree_lca_correctness() {
        let mut tree = HistoryTree::new(200);
        // Build a deep chain: root -> a -> b -> c -> d
        let _a = tree.append_child();
        let b = tree.append_child();
        let c = tree.append_child();
        let d = tree.append_child();

        // Go back to root and build another chain: root -> e -> f
        tree.move_to_parent();
        tree.move_to_parent();
        tree.move_to_parent();
        tree.move_to_parent();
        let e = tree.append_child();
        let f = tree.append_child();

        // LCA of d and f should be root.
        let lca = tree.lca(d, f).unwrap();
        assert_eq!(lca, tree.root());

        // LCA of d and c should be c (c is ancestor of d).
        let lca2 = tree.lca(d, c).unwrap();
        assert_eq!(lca2, c);

        // LCA of d and e should be root.
        let lca3 = tree.lca(d, e).unwrap();
        assert_eq!(lca3, tree.root());

        // LCA of b and c should be b.
        let lca4 = tree.lca(b, c).unwrap();
        assert_eq!(lca4, b);
    }

    #[test]
    fn path_to_deep_branches() {
        let mut tree = HistoryTree::new(200);
        // root -> a -> b -> c
        let a = tree.append_child();
        let b = tree.append_child();
        let c = tree.append_child();

        // Go back to a and create d -> e
        tree.move_to_parent();
        tree.move_to_parent();
        let d = tree.append_child();
        let e = tree.append_child();

        // Path from c to e: revert c, b; apply d, e
        let nav = tree.path_to(c, e).unwrap();
        assert_eq!(nav.lca, a);
        assert_eq!(nav.revert, vec![c, b]);
        assert_eq!(nav.apply, vec![d, e]);
    }

    #[test]
    fn new_tree_panics_on_zero_max_nodes() {
        let result = std::panic::catch_unwind(|| HistoryTree::new(0));
        assert!(result.is_err());
    }

    #[test]
    fn new_tree_panics_on_one_max_nodes() {
        let result = std::panic::catch_unwind(|| HistoryTree::new(1));
        assert!(result.is_err());
    }
}
