//! Platform-specific backend implementations.
//!
//! This module re-exports the platform backend that matches the current
//! target OS. Each backend is gated behind a `#[cfg(target_os = "...")]`
//! attribute so that only the relevant platform's code is compiled:
//!
//! - `windows` — Windows 11 DWM system materials and Snap Layouts.
//! - `macos` — macOS vibrancy (NSVisualEffectView / Liquid Glass).
//! - `wayland` — Linux/Wayland (stub for now).
//!
//! The core abstractions live in the crate root ([`crate::backdrop`],
//! [`crate::snap`]); these backends implement them with platform FFI.

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "linux")]
pub mod wayland;
#[cfg(target_os = "windows")]
pub mod windows;
