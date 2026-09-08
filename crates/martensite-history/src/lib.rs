//! Transactional undo/redo ledger and branching LCA history tree.
//!
//! This crate provides a non-linear undo/redo system based on a directed
//! history tree with parent pointers. When a user undoes and then makes a
//! new edit, a new branch is created rather than discarding the redo
//! history. Navigation between any two points in the tree uses a Lowest
//! Common Ancestor (LCA) algorithm to revert and apply the minimal set
//! of operations.
//!
//! # Architecture
//!
//! - [`HistoryTree`] stores the tree structure of [`HistoryNode`]s in a
//!   [`slotmap::SlotMap`].
//! - [`HistoryLedger`] wraps the tree with a generic state type `S` and
//!   drives [`ChangeOp`] application/reversion.
//! - The LCA algorithm walks parent pointers from two nodes to find their
//!   lowest common ancestor, then reverts backward from the source and
//!   applies forward to the destination.
//! - Bounded depth pruning evicts the least-recently-used leaf nodes when
//!   the node count exceeds the configured maximum.
//!
//! # Example
//!
//! ```
//! use martensite_history::{HistoryLedger, ChangeOp};
//!
//! /// A simple reversible operation on a shared counter.
//! struct AddOp { delta: i32 }
//! impl ChangeOp<i32> for AddOp {
//!     fn apply(&self, state: &mut i32) { *state += self.delta; }
//!     fn revert(&self, state: &mut i32) { *state -= self.delta; }
//! }
//!
//! let mut ledger = HistoryLedger::<i32>::new(0, 100);
//! ledger.commit(Box::new(AddOp { delta: 5 }));
//! ledger.commit(Box::new(AddOp { delta: 3 }));
//! assert_eq!(*ledger.state(), 8);
//! ledger.undo();
//! assert_eq!(*ledger.state(), 5);
//! ledger.commit(Box::new(AddOp { delta: 10 })); // new branch
//! assert_eq!(*ledger.state(), 15);
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod lca;
pub mod ledger;

pub use lca::{HistoryNode, HistoryTree, NodeId, NodeIdError};
pub use ledger::{ChangeOp, HistoryLedger, LedgerError};
