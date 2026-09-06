//! Fine-grained push-pull reactive signal DAG for Martensite.
//!
//! Provides transactional batching, topological scheduling, dynamic dependency pruning,
//! and 3-color DFS cycle detection with zero unsafe code.
#![forbid(unsafe_code)]

pub mod cycle;
pub mod effect;
pub mod memo;
pub mod runtime;
pub mod scheduler;
pub mod signal;

pub use cycle::{CycleError, NodeColor};
pub use effect::Effect;
pub use memo::Memo;
pub use runtime::{
    batch, create_effect, create_memo, create_signal, flush, NodeEvaluator, ReactiveError,
    ReactiveRuntime,
};
pub use scheduler::{FastBuildHasher, FastHasher, NodeRecord, SchedulerState};
pub use signal::{Signal, SignalId};

/// Convenience prelude module for importing fundamental reactive abstractions.
pub mod prelude {
    pub use crate::cycle::{CycleError, NodeColor};
    pub use crate::effect::Effect;
    pub use crate::memo::Memo;
    pub use crate::runtime::{
        batch, create_effect, create_memo, create_signal, flush, ReactiveError, ReactiveRuntime,
    };
    pub use crate::signal::{Signal, SignalId};
}
