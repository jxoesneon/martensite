//! Cross-platform shell integration for Martensite windows.
//!
//! This crate provides the cross-platform abstraction layer for system
//! shell features that platform backends (Windows, macOS, Wayland) build on:
//!
//! - **[`backdrop`]**: System backdrop materials (Mica, Acrylic, vibrancy)
//!   and the [`BackdropController`] trait for applying them to a window.
//! - **[`event`]**: Platform-agnostic [`ShellEvent`] queue for
//!   asynchronous shell events (appearance changes, fractional scale
//!   changes) that the window manager polls and translates into
//!   `WindowEventOutcome`s.
//! - **[`snap`]**: Window snap-layout / tiling configuration
//!   ([`SnapLayout`]).
//! - **status_notifier**: StatusNotifierItem D-Bus system tray
//!   registration (Linux only, `wayland-backend` feature).
//!
//! The core is pure, safe Rust with no platform-specific dependencies.
//! Platform backends (Windows, macOS, Wayland) add `unsafe` FFI in
//! cfg-gated modules under [`platform_impl`].
//!
//! # Examples
//!
//! ```
//! use martensite_shell::{
//!     BackdropMaterial, StubBackdropController, BackdropController, Window,
//! };
//! use core::ffi::c_void;
//!
//! struct TestWindow;
//! impl Window for TestWindow {
//!     unsafe fn raw_handle(&self) -> *mut c_void { core::ptr::null_mut() }
//! }
//!
//! let mut controller = StubBackdropController::new();
//! controller.set_material(&TestWindow, BackdropMaterial::Mica);
//! // The stub ignores the request and always reports None.
//! assert_eq!(controller.current_material(), BackdropMaterial::None);
//! ```

#![deny(missing_docs)]
#![deny(unsafe_code)]

pub mod backdrop;
pub mod event;
pub mod platform_impl;
pub mod snap;
pub mod status_notifier;

pub use backdrop::{
    BackdropController, BackdropMaterial, BackdropMode, StubBackdropController, VibrancyMaterial,
    Window,
};
pub use event::{ShellEvent, ShellEventQueue};
pub use snap::SnapLayout;
