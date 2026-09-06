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

#[cfg(test)]
mod tests {
    use super::ChangeOp;
    use std::sync::atomic::{AtomicI32, Ordering};

    /// A reversible counter operation that adds/subtracts a delta to a shared atomic value.
    struct CounterOp {
        value: AtomicI32,
        delta: i32,
    }

    impl CounterOp {
        fn new(delta: i32) -> Self {
            Self {
                value: AtomicI32::new(0),
                delta,
            }
        }

        fn current(&self) -> i32 {
            self.value.load(Ordering::SeqCst)
        }
    }

    impl ChangeOp for CounterOp {
        fn apply(&self) {
            self.value.fetch_add(self.delta, Ordering::SeqCst);
        }

        fn revert(&self) {
            self.value.fetch_sub(self.delta, Ordering::SeqCst);
        }
    }

    #[test]
    fn apply_mutates_state() {
        let op = CounterOp::new(5);
        assert_eq!(op.current(), 0);
        op.apply();
        assert_eq!(op.current(), 5);
    }

    #[test]
    fn revert_restores_state() {
        let op = CounterOp::new(5);
        op.apply();
        assert_eq!(op.current(), 5);
        op.revert();
        assert_eq!(op.current(), 0);
    }

    #[test]
    fn multiple_apply_revert_cycles() {
        let op = CounterOp::new(3);
        op.apply();
        assert_eq!(op.current(), 3);
        op.apply();
        assert_eq!(op.current(), 6);
        op.revert();
        assert_eq!(op.current(), 3);
        op.apply();
        assert_eq!(op.current(), 6);
        op.revert();
        assert_eq!(op.current(), 3);
        op.revert();
        assert_eq!(op.current(), 0);
    }
}
