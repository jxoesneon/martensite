//! "`martensite-web` — consolidated `wasm32-unknown-unknown` entry point.
//!
//! Martensite's web backends live one-module-per-crate (`martensite-window`'s
//! canvas/event loop, `martensite-wgpu`'s surface probe, `martensite-text`'s
//! font loading, and the clipboard / DnD / a11y bridges). This crate
//! re-exports them behind a single umbrella plus a few small bootstrap
//! helpers so an application crate depends on exactly one package.
//!
//! The crate body is entirely `#[cfg(all(target_arch = "wasm32",
//! target_os = "unknown"))]`: on the host it compiles to an empty library,
//! so the workspace build and its doctests are unaffected.
//!
//! # Quick start
//!
//! ```ignore
//! use martensite_web::{access, canvas_element, clipboard, dnd, gpu, prepare_canvas, run, text};
//!
//! #[wasm_bindgen(start)]
//! pub fn start() -> Result<(), JsValue> {
//!     let canvas = canvas_element("martensite-canvas")?;
//!     prepare_canvas(&canvas, 800, 600);
//!     run(MyApp::default())
//! }
//! ```
#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub use accesskit;
pub use martensite_access::web as access;
pub use martensite_clipboard::web as clipboard;
pub use martensite_dnd::web as dnd;
pub use martensite_text::web as text;
pub use martensite_text::FontManager;
pub use martensite_wgpu::device::GpuContext;
pub use martensite_wgpu::web as gpu;
pub use martensite_wgpu::wgpu;
pub use martensite_window::web as window;
pub use wasm_bindgen;
pub use wasm_bindgen_futures;
pub use winit;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlCanvasElement;

/// Fetches a canvas element by DOM `id`.
///
/// # Errors
///
/// Returns a JS error string when no element with that id exists or the
/// element is not a `<canvas>`.
pub fn canvas_element(id: &str) -> Result<HtmlCanvasElement, JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("no window/document"))?;
    document
        .get_element_by_id(id)
        .and_then(|element| element.dyn_into().ok())
        .ok_or_else(|| JsValue::from_str("no <canvas> element with that id"))
}

/// Sizes a canvas backing store to `logical × devicePixelRatio` and
/// pins its CSS size to `logical` pixels (one DPI-correct initial
/// backing-store sync; subsequent updates come from
/// [`WindowEvent::SurfaceResized`](winit::event::WindowEvent::SurfaceResized)).
pub fn prepare_canvas(canvas: &HtmlCanvasElement, logical_width: u32, logical_height: u32) {
    let scale = web_sys::window()
        .map(|window| window.device_pixel_ratio())
        .unwrap_or(1.0);
    window::sync_canvas_backing_store(canvas, logical_width, logical_height, scale);
}

/// Installs the panic hook, builds a winit event loop with the web
/// poll/rAF strategy configured, and spawns `app` on it.
///
/// # Errors
///
/// Returns a JS error when the event loop cannot be created or the
/// application handler cannot be registered.
pub fn run(app: impl winit::application::ApplicationHandler + 'static) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    let event_loop = winit::event_loop::EventLoop::new()
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    window::configure_web_event_loop(&event_loop);
    window::spawn_app(event_loop, app).map_err(|error| JsValue::from_str(&error.to_string()))
}
