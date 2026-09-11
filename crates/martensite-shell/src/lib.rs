//! Cross-platform shell integration for Martensite windows.
//!
//! This crate provides the cross-platform abstraction layer for system
//! shell features that platform backends (Windows, macOS, Wayland) build on:
//!
//! - **[`backdrop`]**: System backdrop materials (Mica, Acrylic, vibrancy)
//!   and the [`BackdropController`] trait for applying them to a window.
//! - **[`snap`]**: Window snap-layout / tiling configuration
//!   ([`SnapLayout`]).
//!
//! The core is pure, safe Rust with no platform-specific dependencies.
//! Platform backends (Phase 2) will add `unsafe` FFI in cfg-gated modules.
//!
//! # Examples
//!
//! ```
//! use martensite_shell::{BackdropMaterial, StubBackdropController, BackdropController};
//!
//! let mut controller = StubBackdropController::new();
//! controller.set_material(BackdropMaterial::Mica);
//! // The stub ignores the request and always reports None.
//! assert_eq!(controller.current_material(), BackdropMaterial::None);
//! ```

#![deny(missing_docs)]
#![deny(unsafe_code)]

pub mod backdrop;
pub mod platform_impl;
pub mod snap;

pub use backdrop::{
    BackdropController, BackdropMaterial, BackdropMode, StubBackdropController, VibrancyMaterial,
};
pub use snap::SnapLayout;
