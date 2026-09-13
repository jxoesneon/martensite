//! The [`PaintList`] command stream and supporting types.
//!
//! The paint vocabulary lives in [`martensite_core::paint`] — it is the
//! output language of the widget paint pass, produced by widgets and
//! consumed by this crate's backends. This module re-exports it so that
//! existing `martensite_render::paint::*` paths keep working.

pub use martensite_core::paint::*;
