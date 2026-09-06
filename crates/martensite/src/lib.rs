//! The Martensite GUI Framework.
//!
//! Martensite is a retained-mode, GPU-accelerated graphical user interface
//! framework for Rust. It provides deterministic generational arena storage,
//! fine-grained reactive signals, and a GPU rendering pipeline built on WGPU
//! and Vello.
//!
//! ## Getting started
//!
//! Add `martensite` to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! martensite = "0.1.0"
//! ```
//!
//! Import the prelude for the most commonly used types:
//!
//! ```rust
//! use martensite::prelude::*;
//! ```
//!
//! ## Architecture
//!
//! Each subsystem lives in its own crate, re-exported here for convenience:
//!
//! - [`core`] — generational widget arena, `WidgetId`, `HotNode`, `ColdNode`
//! - [`reactive`] — push-pull signal DAG with cycle detection
//! - [`layout`] — Taffy-based flexbox and grid layout
//! - [`wgpu`] — WGPU device and surface management
//! - [`render`] — Vello GPU rendering pipeline
//! - [`text`] — HarfBuzz shaping, BiDi, and font fallback
//! - [`access`] — AccessKit native accessibility integration
//! - [`window`] — Winit multi-window management
//! - [`focus`] — Focus traversal and management
//! - [`clipboard`] — Clipboard integration
//! - [`dnd`] — Drag and drop
//! - [`theme`] — Oklab color theme system
//! - [`motion`] — Spring physics animation
//! - [`history`] — Undo/redo history
//! - [`l10n`] — Localization via Fluent
//! - [`macros`] — Procedural macros
//! - Testing utilities are available via the `martensite-test` dev-dependency.
#![forbid(unsafe_code)]

pub use martensite_access as access;
pub use martensite_clipboard as clipboard;
pub use martensite_core as core;
pub use martensite_dnd as dnd;
pub use martensite_focus as focus;
pub use martensite_history as history;
pub use martensite_l10n as l10n;
pub use martensite_layout as layout;
pub use martensite_macros as macros;
pub use martensite_motion as motion;
pub use martensite_reactive as reactive;
pub use martensite_render as render;
pub use martensite_text as text;
pub use martensite_theme as theme;
pub use martensite_wgpu as wgpu;
pub use martensite_window as window;

/// Convenience prelude re-exporting the most commonly used Martensite types.
///
/// ```rust
/// use martensite::prelude::*;
/// ```
pub mod prelude {
    pub use martensite_core::{ColdNode, HotNode, NodeFlags, Rect, Widget, WidgetArena, WidgetId};
    pub use martensite_motion::{SpringConfig, SpringSolver};
    pub use martensite_reactive::{Memo, Signal};
    pub use martensite_theme::Oklab;
}
