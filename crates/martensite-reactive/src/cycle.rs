//! 3-Color DFS cycle detection and circuit breaker.
#![forbid(unsafe_code)]

use crate::signal::SignalId;

/// Color assigned to nodes during 3-color DFS graph traversal and active evaluation.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeColor {
    /// Node has not been visited during current traversal.
    White,
    /// Node is actively on the evaluation or DFS traversal call stack.
    Gray,
    /// Node has been fully resolved and its subgraph is acyclic.
    Black,
}

/// Description of a cyclic dependency edge detected in the reactive graph.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CycleError {
    /// The originating node of the back-edge.
    pub from: SignalId,
    /// The target node on the active stack completing the cycle.
    pub to: SignalId,
}

impl std::fmt::Display for CycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cyclic dependency detected: signal {:?} depends on active ancestor {:?}",
            self.from, self.to
        )
    }
}

impl std::error::Error for CycleError {}
