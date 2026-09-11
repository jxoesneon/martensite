//! Platform-specific backend implementations.
//!
//! This module re-exports the platform backend that matches the current
//! target OS. Each backend is gated behind *both* a `#[cfg(target_os =
//! "...")]` attribute and a corresponding `*-backend` feature so that
//! only the relevant platform's code is compiled, and only when its
//! backend feature is explicitly enabled:
//!
//! - `windows` — Windows 11 DWM system materials and Snap Layouts, gated
//!   behind `target_os = "windows"` and the `windows-backend` feature.
//! - `macos` — macOS vibrancy (NSVisualEffectView / Liquid Glass), gated
//!   behind `target_os = "macos"` and the `macos-backend` feature
//!   (requires `objc2`/`block2`).
//! - `wayland` — Linux/Wayland (client-side decorations, fractional
//!   scale, system tray), gated behind `target_os = "linux"` and the
//!   `wayland-backend` feature.
//!
//! When a platform's backend feature is not enabled, its module is not
//! compiled at all; the user explicitly chooses which backend to enable.
//!
//! The core abstractions live in the crate root ([`crate::backdrop`],
//! [`crate::snap`]); these backends implement them with platform FFI.

#[cfg(all(target_os = "macos", feature = "macos-backend"))]
pub mod macos;
#[cfg(all(target_os = "linux", feature = "wayland-backend"))]
pub mod wayland;
#[cfg(all(target_os = "windows", feature = "windows-backend"))]
pub mod windows;
