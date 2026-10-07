//! Martensite Widget Catalog — interactive per-widget showcase and
//! developer reference: searchable rail, live stage, bespoke props,
//! event log, and dev-channel control.
//!
//! Dual-target: the same [`view::CatalogView`] drives the desktop
//! window (`run`, winit + Vello/TinySkia) and the browser showcase
//! (wasm32 + wasm-bindgen — the `web` module's `start` entry serves
//! `web/index.html`).

pub mod app;
pub mod dynamic_column;
pub mod page;
pub mod pages;
pub mod stage;
pub mod view;

/// Browser bootstrap — `#[wasm_bindgen(start)]` entry, async GPU probe
/// and font fetch, and the JS-side a11y-enable export.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod web;

/// ADR-0042 web dev channel — `?dev_token=` opt-in WebSocket dial to
/// the `cargo martensite dev-web` relay, serving `DevSession` over it.
/// Compiled only in `web-dev` builds; a regular web build carries none
/// of the transport.
#[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "web-dev"))]
mod web_dev;

pub use app::{run, run_live_headless};
pub use page::{Page, PageMeta, PropSpec, PropValue, PropValues};
pub use pages::all_pages;
pub use stage::{FramePreset, StageHost};
pub use view::CatalogView;
