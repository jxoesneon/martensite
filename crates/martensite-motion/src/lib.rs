//! Analytical closed-form spring motion and animation drivers.
//!
//! This crate provides a frame-rate-independent, energy-conserving spring
//! physics solver ([`SpringSolver`]) together with higher-level animation
//! drivers ([`AnimationDriver`] and [`AnimationDriver2D`]) that manage
//! collections of active springs with C¹ velocity handoff on interruption.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod animation;
pub mod rubber_band;
pub mod spring;

pub use animation::{AnimationDriver, AnimationDriver2D, AnimationError, AnimationId};
pub use rubber_band::{
    AxisLock, RubberBandScroller, RubberBandScroller2D, RUBBER_BAND_COEFFICIENT,
};
pub use spring::{DampingRegime, SpringConfig, SpringConfigError, SpringSolver};
