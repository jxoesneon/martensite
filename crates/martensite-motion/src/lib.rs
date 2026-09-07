//! Analytical closed-form spring motion and animation drivers.
//!
//! This crate provides a frame-rate-independent, energy-conserving spring
//! physics solver ([`SpringSolver`]) together with higher-level animation
//! drivers ([`AnimationDriver`] and [`AnimationDriver2D`]) that manage
//! collections of active springs with C¹ velocity handoff on interruption.
#![forbid(unsafe_code)]

pub mod animation;
pub mod spring;

pub use animation::{AnimationDriver, AnimationDriver2D, AnimationError, AnimationId};
pub use spring::{DampingRegime, SpringConfig, SpringConfigError, SpringSolver};
