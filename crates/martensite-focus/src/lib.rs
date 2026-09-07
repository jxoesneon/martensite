//! 2D Spatial projected-beam focus engine with modal focus trapping.
//!
//! This crate provides:
//!
//! - **[`FocusManager`]**: Active focus tracking, tab navigation ordering,
//!   and integration with the widget arena.
//! - **[`spatial`]**: Projected-beam 2D directional navigation algorithm
//!   for arrow key / gamepad / TV remote navigation across non-linear
//!   widget grids.
//! - **[`scope`]**: Modal `FocusScope` stack with focus trapping and
//!   auto-restoration upon modal dismiss.
//!
//! ## Architecture
//!
//! The focus system operates on the `WidgetArena`, using the
//! `NodeFlags::FOCUSABLE` flag
//! to identify focusable widgets and their [`Rect`](martensite_core::Rect)
//! bounds for spatial navigation scoring.
#![forbid(unsafe_code)]

pub mod manager;
pub mod scope;
pub mod spatial;

pub use manager::{FocusManager, TabNavigation};
pub use scope::{FocusScope, FocusScopeStack};
pub use spatial::{
    FocusDirection, SpatialNavigator, DEFAULT_ALPHA, DEFAULT_BETA, FORWARD_CONE_DEGREES,
};

// Re-export FocusDirection from spatial for backward compatibility.
pub use spatial::FocusDirection as Direction;
