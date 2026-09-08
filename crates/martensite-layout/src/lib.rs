//! Taffy layout engine bridge for Martensite.
//!
//! This crate provides:
//! - [`geometry`]: foundational spatial types (`Point`, `Size`,
//!   `Constraints`, `EdgeInsets`).
//! - [`taffy_bridge`]: adapts the Martensite `WidgetArena` to Taffy's
//!   [`TraversePartialTree`] trait.
//! - [`engine`]: the [`LayoutEngine`] performing two-pass measurement and
//!   positioning.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Re-exports the Taffy prelude, providing common layout types and helpers.
pub use taffy::prelude::*;
/// Re-exports the [`TaffyTree`] layout tree implementation from Taffy.
pub use taffy::TaffyTree;

/// Two-pass layout engine.
pub mod engine;
/// Geometry primitives: `Point`, `Size`, `Constraints`, `EdgeInsets`.
pub mod geometry;
/// Taffy bridge: `ArenaBridge`, `TraversePartialTree` over `WidgetArena`.
pub mod taffy_bridge;
/// Vertical layout axis transposition bridge for `writing-mode: vertical-rl` and `vertical-lr`.
pub mod vertical_flow;

pub use engine::{
    constraints_to_available, edge_insets_to_style, taffy_layout_to_rect, IdMapIter, LayoutEngine,
    LayoutError,
};
pub use geometry::{Constraints, EdgeInsets, Point, Size};
pub use taffy_bridge::{node_id_to_widget_id, widget_id_to_node_id, ArenaBridge, ArenaChildIter};
pub use vertical_flow::{FlowTransposition, LogicalPoint, LogicalSize, WritingMode};
