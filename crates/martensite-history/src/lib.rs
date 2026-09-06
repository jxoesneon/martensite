//! Transactional undo/redo ledger and branching LCA history tree.
#![forbid(unsafe_code)]

/// A reversible operation that can be applied to and reverted from the
/// application state, forming the atomic unit of the undo/redo ledger.
pub trait ChangeOp: Send + Sync + 'static {
    /// Undo this operation, restoring the state to what it was before
    /// [`ChangeOp::apply`] was called.
    fn revert(&self);
    /// Perform this operation, mutating the application state.
    fn apply(&self);
}
