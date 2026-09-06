//! Transactional undo/redo ledger and branching LCA history tree.
#![forbid(unsafe_code)]

pub trait ChangeOp: Send + Sync + 'static {
    fn revert(&self);
    fn apply(&self);
}
