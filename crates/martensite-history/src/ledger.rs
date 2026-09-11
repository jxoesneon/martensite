//! Transactional state ledger with LCA-based undo/redo navigation.
//!
//! The [`HistoryLedger`] wraps a [`crate::HistoryTree`] with a generic
//! state type and drives [`ChangeOp`] application and reversion. It
//! supports linear undo/redo as well as non-linear jumps between any
//! two points in the history tree.

use crate::lca::NodeId;
use crate::HistoryTree;
use std::collections::HashMap;

/// A reversible operation that can be applied to and reverted from
/// application state.
///
/// Operations must be `Send + Sync + 'static` so the ledger can store
/// them in a `Box<dyn ChangeOp<S>>`.
///
/// # Example
///
/// ```
/// use martensite_history::ChangeOp;
///
/// struct SetOp { old: i32, new: i32 }
/// impl ChangeOp<i32> for SetOp {
///     fn apply(&self, state: &mut i32) { *state = self.new; }
///     fn revert(&self, state: &mut i32) { *state = self.old; }
/// }
/// ```
pub trait ChangeOp<S>: Send + Sync + 'static {
    /// Apply this operation, mutating the state.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_history::ChangeOp;
    ///
    /// struct AddOp(i32);
    /// impl ChangeOp<i32> for AddOp {
    ///     fn apply(&self, state: &mut i32) { *state += self.0; }
    ///     fn revert(&self, state: &mut i32) { *state -= self.0; }
    /// }
    ///
    /// let op = AddOp(5);
    /// let mut value = 0;
    /// op.apply(&mut value);
    /// assert_eq!(value, 5);
    /// ```
    fn apply(&self, state: &mut S);

    /// Revert this operation, restoring the state to what it was before
    /// [`ChangeOp::apply`] was called.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_history::ChangeOp;
    ///
    /// struct AddOp(i32);
    /// impl ChangeOp<i32> for AddOp {
    ///     fn apply(&self, state: &mut i32) { *state += self.0; }
    ///     fn revert(&self, state: &mut i32) { *state -= self.0; }
    /// }
    ///
    /// let op = AddOp(5);
    /// let mut value = 5;
    /// op.revert(&mut value);
    /// assert_eq!(value, 0);
    /// ```
    fn revert(&self, state: &mut S);
}

/// Errors that can occur during ledger operations.
///
/// # Examples
///
/// ```
/// use martensite_history::LedgerError;
///
/// assert_eq!(LedgerError::NoUndo.to_string(), "nothing to undo");
/// assert_eq!(LedgerError::NoRedo.to_string(), "nothing to redo");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerError {
    /// The requested node ID does not exist in the history tree.
    InvalidNode,
    /// There is nothing to undo (already at the root).
    NoUndo,
    /// There is nothing to redo (no children on the current branch).
    NoRedo,
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LedgerError::InvalidNode => write!(f, "invalid history node"),
            LedgerError::NoUndo => write!(f, "nothing to undo"),
            LedgerError::NoRedo => write!(f, "nothing to redo"),
        }
    }
}

impl std::error::Error for LedgerError {}

/// Transactional state ledger with branching undo/redo.
///
/// The ledger maintains a [`HistoryTree`] of operations and a current
/// state value. Each [`commit`](Self::commit) applies an operation and
/// creates a new node in the tree. Undo/redo navigates the tree, and
/// [`jump_to`](Self::jump_to) enables non-linear navigation between
/// any two points.
///
/// # Example
///
/// ```
/// use martensite_history::{HistoryLedger, ChangeOp};
///
/// struct AddOp(i32);
/// impl ChangeOp<i32> for AddOp {
///     fn apply(&self, s: &mut i32) { *s += self.0; }
///     fn revert(&self, s: &mut i32) { *s -= self.0; }
/// }
///
/// let mut ledger = HistoryLedger::new(0, 100);
/// ledger.commit(Box::new(AddOp(5)));
/// ledger.commit(Box::new(AddOp(3)));
/// assert_eq!(*ledger.state(), 8);
/// ledger.undo().unwrap();
/// assert_eq!(*ledger.state(), 5);
/// ```
///
/// # Node ID Invalidation
///
/// [`NodeId`]s returned by [`current_node`](Self::current_node) and
/// [`root_node`](Self::root_node) may become invalid if the history tree
/// is pruned (when `node_count` exceeds `max_nodes`). Operations on
/// stale `NodeId`s via [`jump_to`](Self::jump_to) will return
/// `Err(LedgerError::InvalidNode)`. Always check validity before using
/// stored `NodeId`s if pruning may have occurred.
pub struct HistoryLedger<S: 'static> {
    /// The application state.
    state: S,
    /// The history tree tracking node relationships.
    tree: HistoryTree,
    /// Stored operations keyed by tree node ID.
    ops: HashMap<NodeId, Box<dyn ChangeOp<S>>>,
}

impl<S: 'static + std::fmt::Debug> std::fmt::Debug for HistoryLedger<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HistoryLedger")
            .field("state", &self.state)
            .field("node_count", &self.tree.node_count())
            .finish()
    }
}

impl<S: 'static> HistoryLedger<S> {
    /// Creates a new ledger with the given initial state and maximum
    /// history depth.
    ///
    /// # Panics
    ///
    /// Panics if `max_nodes` is zero.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_history::HistoryLedger;
    ///
    /// let ledger = HistoryLedger::<String>::new("hello".to_string(), 500);
    /// assert_eq!(*ledger.state(), "hello");
    /// ```
    pub fn new(initial_state: S, max_nodes: usize) -> Self {
        Self {
            state: initial_state,
            tree: HistoryTree::new(max_nodes),
            ops: HashMap::new(),
        }
    }

    /// Returns a reference to the current state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_history::HistoryLedger;
    ///
    /// let ledger = HistoryLedger::<i32>::new(42, 100);
    /// assert_eq!(*ledger.state(), 42);
    /// ```
    #[inline]
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Returns a mutable reference to the current state.
    ///
    /// **Warning**: Direct mutation bypasses the history system. Use
    /// [`commit`](Self::commit) for tracked changes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_history::HistoryLedger;
    ///
    /// let mut ledger = HistoryLedger::<i32>::new(0, 100);
    /// *ledger.state_mut() = 10;
    /// assert_eq!(*ledger.state(), 10);
    /// ```
    #[inline]
    pub fn state_mut(&mut self) -> &mut S {
        &mut self.state
    }

    /// Returns the current node ID in the history tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_history::{ChangeOp, HistoryLedger};
    ///
    /// struct AddOp(i32);
    /// impl ChangeOp<i32> for AddOp {
    ///     fn apply(&self, s: &mut i32) { *s += self.0; }
    ///     fn revert(&self, s: &mut i32) { *s -= self.0; }
    /// }
    ///
    /// let mut ledger = HistoryLedger::new(0, 100);
    /// let root = ledger.root_node();
    /// ledger.commit(Box::new(AddOp(5)));
    /// assert_ne!(ledger.current_node(), root);
    /// ```
    #[inline]
    pub fn current_node(&self) -> NodeId {
        self.tree.current()
    }

    /// Returns the root node ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_history::HistoryLedger;
    ///
    /// let ledger = HistoryLedger::<i32>::new(0, 100);
    /// let root = ledger.root_node();
    /// assert_eq!(ledger.current_node(), root);
    /// ```
    #[inline]
    pub fn root_node(&self) -> NodeId {
        self.tree.root()
    }

    /// Returns the total number of nodes in the history tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_history::{ChangeOp, HistoryLedger};
    ///
    /// struct AddOp(i32);
    /// impl ChangeOp<i32> for AddOp {
    ///     fn apply(&self, s: &mut i32) { *s += self.0; }
    ///     fn revert(&self, s: &mut i32) { *s -= self.0; }
    /// }
    ///
    /// let mut ledger = HistoryLedger::new(0, 100);
    /// assert_eq!(ledger.node_count(), 1);
    /// ledger.commit(Box::new(AddOp(5)));
    /// assert_eq!(ledger.node_count(), 2);
    /// ```
    #[inline]
    pub fn node_count(&self) -> usize {
        self.tree.node_count()
    }

    /// Returns the depth of the current node (root = 0).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_history::{ChangeOp, HistoryLedger};
    ///
    /// struct AddOp(i32);
    /// impl ChangeOp<i32> for AddOp {
    ///     fn apply(&self, s: &mut i32) { *s += self.0; }
    ///     fn revert(&self, s: &mut i32) { *s -= self.0; }
    /// }
    ///
    /// let mut ledger = HistoryLedger::new(0, 100);
    /// assert_eq!(ledger.current_depth(), 0);
    /// ledger.commit(Box::new(AddOp(5)));
    /// assert_eq!(ledger.current_depth(), 1);
    /// ```
    #[inline]
    pub fn current_depth(&self) -> u32 {
        self.tree.current_depth()
    }

    /// Commits a new operation to the ledger.
    ///
    /// This applies the operation to the state, creates a new child
    /// node in the history tree, and stores the operation for future
    /// revert/apply.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_history::{HistoryLedger, ChangeOp};
    ///
    /// struct PushOp(String);
    /// impl ChangeOp<Vec<String>> for PushOp {
    ///     fn apply(&self, s: &mut Vec<String>) { s.push(self.0.clone()); }
    ///     fn revert(&self, s: &mut Vec<String>) { s.pop(); }
    /// }
    ///
    /// let mut ledger = HistoryLedger::<Vec<String>>::new(Vec::new(), 100);
    /// ledger.commit(Box::new(PushOp("a".to_string())));
    /// ledger.commit(Box::new(PushOp("b".to_string())));
    /// assert_eq!(ledger.state().len(), 2);
    /// ```
    pub fn commit(&mut self, op: Box<dyn ChangeOp<S>>) {
        op.apply(&mut self.state);
        let node_id = self.tree.append_child();
        self.ops.insert(node_id, op);
        // Clean up ops for any nodes that were pruned during append_child.
        self.cleanup_pruned_ops();
    }

    /// Removes operations for nodes that no longer exist in the tree.
    /// This is called automatically after each commit to prevent the
    /// ops map from growing unbounded.
    fn cleanup_pruned_ops(&mut self) {
        let stale: Vec<NodeId> = self
            .ops
            .keys()
            .filter(|id| self.tree.node(**id).is_none())
            .copied()
            .collect();
        for id in stale {
            self.ops.remove(&id);
        }
    }

    /// Undoes the most recent operation on the current branch.
    ///
    /// Moves to the parent node and reverts the operation.
    ///
    /// Returns `Err(LedgerError::NoUndo)` if already at the root.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_history::{HistoryLedger, ChangeOp};
    ///
    /// struct AddOp(i32);
    /// impl ChangeOp<i32> for AddOp {
    ///     fn apply(&self, s: &mut i32) { *s += self.0; }
    ///     fn revert(&self, s: &mut i32) { *s -= self.0; }
    /// }
    ///
    /// let mut ledger = HistoryLedger::new(0, 100);
    /// ledger.commit(Box::new(AddOp(5)));
    /// ledger.undo().unwrap();
    /// assert_eq!(*ledger.state(), 0);
    /// ```
    pub fn undo(&mut self) -> Result<(), LedgerError> {
        let current = self.tree.current();
        if self.tree.current_depth() == 0 {
            return Err(LedgerError::NoUndo);
        }
        if let Some(op) = self.ops.get(&current) {
            op.revert(&mut self.state);
        } else {
            debug_assert!(false, "missing op for current node {:?}", current);
        }
        self.tree.move_to_parent();
        Ok(())
    }

    /// Redoes the most recently undone operation on the current branch.
    ///
    /// Moves to the most recently visited child and applies its operation.
    ///
    /// Returns `Err(LedgerError::NoRedo)` if there are no children.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_history::{HistoryLedger, ChangeOp};
    ///
    /// struct AddOp(i32);
    /// impl ChangeOp<i32> for AddOp {
    ///     fn apply(&self, s: &mut i32) { *s += self.0; }
    ///     fn revert(&self, s: &mut i32) { *s -= self.0; }
    /// }
    ///
    /// let mut ledger = HistoryLedger::new(0, 100);
    /// ledger.commit(Box::new(AddOp(5)));
    /// ledger.undo().unwrap();
    /// ledger.redo().unwrap();
    /// assert_eq!(*ledger.state(), 5);
    /// ```
    pub fn redo(&mut self) -> Result<(), LedgerError> {
        // Pick the most recently visited child.
        let best = self
            .tree
            .current_children()
            .iter()
            .copied()
            .max_by_key(|&c| self.tree.node(c).map(|n| n.last_visited).unwrap_or(0))
            .ok_or(LedgerError::NoRedo)?;

        self.tree.move_to_child(best);
        let current = self.tree.current();
        if let Some(op) = self.ops.get(&current) {
            op.apply(&mut self.state);
        }
        Ok(())
    }

    /// Jumps to a specific node in the history tree using LCA navigation.
    ///
    /// This reverts operations from the current node to the LCA, then
    /// applies operations from the LCA to the target node.
    ///
    /// Returns `Err(LedgerError::InvalidNode)` if the target node
    /// does not exist.
    ///
    /// # Example
    ///
    /// ```
    /// use martensite_history::{HistoryLedger, ChangeOp};
    ///
    /// struct AddOp(i32);
    /// impl ChangeOp<i32> for AddOp {
    ///     fn apply(&self, s: &mut i32) { *s += self.0; }
    ///     fn revert(&self, s: &mut i32) { *s -= self.0; }
    /// }
    ///
    /// let mut ledger = HistoryLedger::new(0, 100);
    /// ledger.commit(Box::new(AddOp(5)));  // node A
    /// ledger.commit(Box::new(AddOp(3)));  // node B
    /// ledger.undo().unwrap();              // back to A
    /// ledger.commit(Box::new(AddOp(10))); // node C (new branch)
    /// let target = ledger.root_node();
    /// ledger.jump_to(target).unwrap();    // jump to root
    /// assert_eq!(*ledger.state(), 0);
    /// ```
    pub fn jump_to(&mut self, target: NodeId) -> Result<(), LedgerError> {
        let source = self.tree.current();

        if source == target {
            return Ok(());
        }

        let nav = self
            .tree
            .path_to(source, target)
            .ok_or(LedgerError::InvalidNode)?;

        // Revert from source to LCA (exclusive).
        for &node_id in &nav.revert {
            if let Some(op) = self.ops.get(&node_id) {
                op.revert(&mut self.state);
            }
        }

        // Apply from LCA to target (inclusive).
        for &node_id in &nav.apply {
            if let Some(op) = self.ops.get(&node_id) {
                op.apply(&mut self.state);
            }
        }

        self.tree
            .set_current(target)
            .map_err(|_| LedgerError::InvalidNode)?;
        Ok(())
    }

    /// Returns the children of the current node (available redo branches).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_history::{ChangeOp, HistoryLedger};
    ///
    /// struct AddOp(i32);
    /// impl ChangeOp<i32> for AddOp {
    ///     fn apply(&self, s: &mut i32) { *s += self.0; }
    ///     fn revert(&self, s: &mut i32) { *s -= self.0; }
    /// }
    ///
    /// let mut ledger = HistoryLedger::new(0, 100);
    /// ledger.commit(Box::new(AddOp(5)));
    /// ledger.undo().unwrap();
    /// assert_eq!(ledger.redo_branches().len(), 1);
    /// ```
    #[inline]
    pub fn redo_branches(&self) -> &[NodeId] {
        self.tree.current_children()
    }

    /// Returns the maximum number of nodes the ledger will retain.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_history::HistoryLedger;
    ///
    /// let ledger = HistoryLedger::<i32>::new(0, 500);
    /// assert_eq!(ledger.max_nodes(), 500);
    /// ```
    #[inline]
    pub fn max_nodes(&self) -> usize {
        self.tree.max_nodes
    }

    /// Returns whether the ledger can undo (current node is not root).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_history::{ChangeOp, HistoryLedger};
    ///
    /// struct AddOp(i32);
    /// impl ChangeOp<i32> for AddOp {
    ///     fn apply(&self, s: &mut i32) { *s += self.0; }
    ///     fn revert(&self, s: &mut i32) { *s -= self.0; }
    /// }
    ///
    /// let mut ledger = HistoryLedger::new(0, 100);
    /// assert!(!ledger.can_undo());
    /// ledger.commit(Box::new(AddOp(5)));
    /// assert!(ledger.can_undo());
    /// ```
    #[inline]
    pub fn can_undo(&self) -> bool {
        self.tree.current_depth() > 0
    }

    /// Returns whether the ledger can redo (current node has children).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_history::{ChangeOp, HistoryLedger};
    ///
    /// struct AddOp(i32);
    /// impl ChangeOp<i32> for AddOp {
    ///     fn apply(&self, s: &mut i32) { *s += self.0; }
    ///     fn revert(&self, s: &mut i32) { *s -= self.0; }
    /// }
    ///
    /// let mut ledger = HistoryLedger::new(0, 100);
    /// assert!(!ledger.can_redo());
    /// ledger.commit(Box::new(AddOp(5)));
    /// ledger.undo().unwrap();
    /// assert!(ledger.can_redo());
    /// ```
    #[inline]
    pub fn can_redo(&self) -> bool {
        !self.tree.current_children().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A simple add operation for testing.
    struct AddOp(i32);
    impl ChangeOp<i32> for AddOp {
        fn apply(&self, s: &mut i32) {
            *s += self.0;
        }
        fn revert(&self, s: &mut i32) {
            *s -= self.0;
        }
    }

    #[test]
    fn new_ledger_has_initial_state() {
        let ledger = HistoryLedger::<i32>::new(42, 100);
        assert_eq!(*ledger.state(), 42);
        assert_eq!(ledger.node_count(), 1);
        assert_eq!(ledger.current_depth(), 0);
    }

    #[test]
    fn commit_applies_and_tracks() {
        let mut ledger = HistoryLedger::new(0, 100);
        ledger.commit(Box::new(AddOp(5)));
        assert_eq!(*ledger.state(), 5);
        assert_eq!(ledger.node_count(), 2);
        assert_eq!(ledger.current_depth(), 1);
    }

    #[test]
    fn undo_restores_state() {
        let mut ledger = HistoryLedger::new(0, 100);
        ledger.commit(Box::new(AddOp(5)));
        ledger.commit(Box::new(AddOp(3)));
        assert_eq!(*ledger.state(), 8);
        ledger.undo().unwrap();
        assert_eq!(*ledger.state(), 5);
        ledger.undo().unwrap();
        assert_eq!(*ledger.state(), 0);
    }

    #[test]
    fn undo_at_root_returns_error() {
        let mut ledger = HistoryLedger::<i32>::new(0, 100);
        assert_eq!(ledger.undo(), Err(LedgerError::NoUndo));
    }

    #[test]
    fn redo_reapplies_state() {
        let mut ledger = HistoryLedger::new(0, 100);
        ledger.commit(Box::new(AddOp(5)));
        ledger.undo().unwrap();
        assert_eq!(*ledger.state(), 0);
        ledger.redo().unwrap();
        assert_eq!(*ledger.state(), 5);
    }

    #[test]
    fn redo_without_children_returns_error() {
        let mut ledger = HistoryLedger::<i32>::new(0, 100);
        assert_eq!(ledger.redo(), Err(LedgerError::NoRedo));
    }

    #[test]
    fn branching_creates_new_branch() {
        let mut ledger = HistoryLedger::new(0, 100);
        ledger.commit(Box::new(AddOp(5))); // A: state=5
        ledger.commit(Box::new(AddOp(3))); // B: state=8
        ledger.undo().unwrap(); // back to A: state=5
        ledger.commit(Box::new(AddOp(10))); // C: state=15
        assert_eq!(*ledger.state(), 15);
        assert_eq!(ledger.redo_branches().len(), 0);
    }

    #[test]
    fn jump_to_root_from_branch() {
        let mut ledger = HistoryLedger::new(0, 100);
        ledger.commit(Box::new(AddOp(5)));
        ledger.commit(Box::new(AddOp(3)));
        let root = ledger.root_node();
        ledger.jump_to(root).unwrap();
        assert_eq!(*ledger.state(), 0);
    }

    #[test]
    fn jump_to_branch_sibling() {
        let mut ledger = HistoryLedger::new(0, 100);
        ledger.commit(Box::new(AddOp(5))); // A: state=5
        let node_a = ledger.current_node();
        ledger.commit(Box::new(AddOp(3))); // B: state=8
        let node_b = ledger.current_node();
        ledger.undo().unwrap(); // back to A
        ledger.commit(Box::new(AddOp(10))); // C: state=15
        let node_c = ledger.current_node();

        // Jump from C to B.
        ledger.jump_to(node_b).unwrap();
        assert_eq!(*ledger.state(), 8);

        // Jump from B to C.
        ledger.jump_to(node_c).unwrap();
        assert_eq!(*ledger.state(), 15);

        // Jump from C to A.
        ledger.jump_to(node_a).unwrap();
        assert_eq!(*ledger.state(), 5);
    }

    #[test]
    fn jump_to_same_node_is_noop() {
        let mut ledger = HistoryLedger::new(0, 100);
        ledger.commit(Box::new(AddOp(5)));
        let current = ledger.current_node();
        ledger.jump_to(current).unwrap();
        assert_eq!(*ledger.state(), 5);
    }

    #[test]
    fn jump_to_invalid_node_returns_error() {
        let mut ledger = HistoryLedger::<i32>::new(0, 100);
        let invalid = NodeId::default();
        assert_eq!(ledger.jump_to(invalid), Err(LedgerError::InvalidNode));
    }

    #[test]
    fn can_undo_and_can_redo() {
        let mut ledger = HistoryLedger::new(0, 100);
        assert!(!ledger.can_undo());
        assert!(!ledger.can_redo());
        ledger.commit(Box::new(AddOp(5)));
        assert!(ledger.can_undo());
        assert!(!ledger.can_redo());
        ledger.undo().unwrap();
        assert!(!ledger.can_undo());
        assert!(ledger.can_redo());
    }

    #[test]
    fn multiple_branches_redo_picks_most_recent() {
        let mut ledger = HistoryLedger::new(0, 100);
        ledger.commit(Box::new(AddOp(5))); // A
        let _a = ledger.current_node();
        ledger.commit(Box::new(AddOp(1))); // B
        let b = ledger.current_node();
        ledger.undo().unwrap(); // back to A
        ledger.commit(Box::new(AddOp(2))); // C
        let c = ledger.current_node();
        ledger.undo().unwrap(); // back to A

        // Redo should pick C (most recently visited).
        ledger.redo().unwrap();
        assert_eq!(ledger.current_node(), c);
        assert_eq!(*ledger.state(), 7);

        // Undo and jump to B to visit it.
        ledger.undo().unwrap();
        ledger.jump_to(b).unwrap();

        // Now undo back to A and redo should pick B (most recently visited).
        ledger.undo().unwrap();
        ledger.redo().unwrap();
        assert_eq!(ledger.current_node(), b);
    }

    #[test]
    fn bounded_depth_pruning_preserves_current() {
        let mut ledger = HistoryLedger::new(0, 5);
        for i in 0..10 {
            ledger.commit(Box::new(AddOp(i)));
        }
        // Should have pruned but current branch is intact.
        assert!(ledger.node_count() <= 5);
        assert!(ledger.can_undo());
    }

    #[test]
    fn random_rollback_integrity() {
        use std::collections::HashMap;

        // Simulate randomized state rollbacks and verify bit-level
        // state integrity. This is a scaled-down version of the
        // 10,000 rollback gate that runs in integration tests.
        let mut ledger = HistoryLedger::new(0i32, 500);
        let mut nodes: Vec<NodeId> = vec![ledger.root_node()];
        let mut expected: HashMap<NodeId, i32> = HashMap::new();
        expected.insert(ledger.root_node(), 0);

        let mut rng_state: u64 = 12345;
        let mut next_val: i32 = 1;
        for _ in 0..500 {
            rng_state = rng_state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let action = (rng_state >> 32) % 4;

            match action {
                0 | 1 => {
                    let val = next_val;
                    next_val += 1;
                    ledger.commit(Box::new(AddOp(val)));
                    let node = ledger.current_node();
                    nodes.push(node);
                    expected.insert(node, *ledger.state());
                }
                2 => {
                    if ledger.can_undo() {
                        ledger.undo().unwrap();
                        let node = ledger.current_node();
                        expected.insert(node, *ledger.state());
                    }
                }
                3 => {
                    if !nodes.is_empty() {
                        rng_state = rng_state
                            .wrapping_mul(6364136223846793005)
                            .wrapping_add(1442695040888963407);
                        let idx = (rng_state >> 32) as usize % nodes.len();
                        let target = nodes[idx];
                        if ledger.jump_to(target).is_ok() {
                            let node = ledger.current_node();
                            expected.insert(node, *ledger.state());
                        }
                    }
                }
                _ => unreachable!(),
            }
        }

        // Verify all surviving known nodes restore correct state.
        for &node in &nodes {
            if ledger.jump_to(node).is_ok() {
                if let Some(&expected_state) = expected.get(&node) {
                    assert_eq!(
                        *ledger.state(),
                        expected_state,
                        "State mismatch at node {:?}: expected {}, got {}",
                        node,
                        expected_state,
                        *ledger.state()
                    );
                }
            }
        }
    }

    #[test]
    fn ledger_error_display() {
        assert_eq!(LedgerError::InvalidNode.to_string(), "invalid history node");
        assert_eq!(LedgerError::NoUndo.to_string(), "nothing to undo");
        assert_eq!(LedgerError::NoRedo.to_string(), "nothing to redo");
    }

    #[test]
    fn state_mut_allows_direct_mutation() {
        let mut ledger = HistoryLedger::new(0, 100);
        *ledger.state_mut() = 42;
        assert_eq!(*ledger.state(), 42);
    }
}
