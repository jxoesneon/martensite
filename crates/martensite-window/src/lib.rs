//! Multi-window management and fractional DPI scaling for Martensite.
//!
//! This crate provides two cooperating modules:
//!
//! - [`dpi`] — the [`DpiScale`] type for converting between physical and
//!   logical pixels with full fractional scale-factor support (e.g. `1.25x`,
//!   `1.5x`, `1.75x`), plus runtime updates when a window moves between
//!   monitors with different DPIs.
//! - [`manager`] — the [`WindowManager`] which owns every open window,
//!   tracks per-window DPI scale factors, and routes [`winit`] window
//!   events to the appropriate per-window state via a [`slotmap`]-backed
//!   store.
//! - [`event`] — the [`EventRouter`] event routing pipeline with normalized
//!   [`PointerEvent`]s, pointer capture, hover tracking, and winit event
//!   conversion.
//!
//! # Example
//!
//! ```no_run
//! use martensite_window::{WindowManager, dpi::DpiScale};
//! use winit::application::ApplicationHandler;
//! use winit::event::WindowEvent;
//! use winit::event_loop::{ActiveEventLoop, EventLoop};
//! use winit::window::{WindowAttributes, WindowId};
//!
//! struct App {
//!     mgr: WindowManager,
//! }
//!
//! impl ApplicationHandler for App {
//!     fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
//!         let attrs = WindowAttributes::default().with_title("Martensite");
//!         let _key = self.mgr.create_window(event_loop, attrs)
//!             .expect("window creation failed");
//!     }
//!
//!     fn window_event(
//!         &mut self,
//!         event_loop: &dyn ActiveEventLoop,
//!         id: WindowId,
//!         event: WindowEvent,
//!     ) {
//!         use martensite_window::manager::WindowEventOutcome;
//!         match self.mgr.handle_window_event(id, &event) {
//!             WindowEventOutcome::CloseRequested => event_loop.exit(),
//!             _ => {}
//!         }
//!     }
//!
//!     fn about_to_wait(&mut self, _event_loop: &dyn ActiveEventLoop) {}
//! }
//!
//! let event_loop = EventLoop::new().expect("failed to create event loop");
//! let app = App { mgr: WindowManager::new() };
//! event_loop.run_app(app).expect("event loop exited with error");
//! ```
#![forbid(unsafe_code)]

pub mod csd;
pub mod dpi;
pub mod event;
pub mod hit_test;
pub mod manager;
pub mod stylus;

pub use winit::error::RequestError;
pub use winit::event::WindowEvent;
pub use winit::event_loop::ActiveEventLoop;
pub use winit::window::{Window, WindowAttributes, WindowId};

pub use csd::{csd_region_for_point, CsdController, CsdHitRegion};
pub use dpi::DpiScale;
pub use event::{
    convert_drop_event, convert_modifiers, convert_modifiers_state, convert_mouse_button,
    convert_window_event, DropAction, DropEvent, EventDispatchOutcome, EventRouter, ModifierKeys,
    MouseTracker, PointerCapture, PointerEvent, PointerId, PointerState,
};
pub use hit_test::{AffineTransform, ClipShape, HitTestResult, HitTester, RoundedRect};
pub use manager::{WindowEntry, WindowEventOutcome, WindowKey, WindowManager};
