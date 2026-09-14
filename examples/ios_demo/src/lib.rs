//! Minimal iOS shell demonstrating the Martensite mobile windowing path.
//!
//! This crate builds as a `staticlib` (`libios_demo.a`) for
//! `aarch64-apple-ios` and `aarch64-apple-ios-sim` so it can be linked
//! into an Xcode or cargo-mobile2 application target. The host-side
//! `rlib` keeps `cargo check`/`cargo test` working on desktop.
//!
//! The demo exercises the v0.17.0 iOS surface:
//!
//! - **UIKit lifecycle:** the window is created in
//!   [`ApplicationHandler::can_create_surfaces`] (the first point at which
//!   iOS permits window/GPU-surface creation) and dropped in
//!   [`ApplicationHandler::destroy_surfaces`] only when the platform's
//!   [`surface_lifecycle`] policy requires it — on iOS the policy is
//!   [`SurfaceLifecycle::Persistent`] and `destroy_surfaces` is never
//!   emitted.
//! - **Safe area:** [`Window::safe_area`] returns UIKit's physical-pixel
//!   `safeAreaInsets`; the demo logs them after window creation.
//! - **Touch routing:** winit `PointerMoved`/`PointerButton` events are
//!   normalized through [`convert_window_event`], preserving the
//!   `PointerKind` (`Touch`, `Mouse`, `Tablet`, `Unknown`) and the
//!   per-finger `PointerId`.
//! - **IME:** [`WindowEvent::Ime`] events are logged; production callers
//!   drive the software keyboard through `martensite_window::ime`.
//!
//! See `README.md` for cargo-mobile2/Xcode packaging instructions.
// No crate-level unsafe attribute: the workspace lints deny `unsafe_code`,
// and the single exception is scoped to the `no_mangle` export at the
// bottom of this file (an unsafe *attribute*, not unsafe code).

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use martensite_window::dpi::DpiScale;
use martensite_window::event::convert_window_event;
use martensite_window::lifecycle::{surface_lifecycle, SurfaceLifecycle};

/// Demo [`ApplicationHandler`] driving a single winit window.
///
/// The window is created in `can_create_surfaces` — the only point at
/// which iOS guarantees the `UIView`/`CAMetalLayer` surface can exist —
/// and retained across `suspended`/`resumed` because iOS reports
/// [`SurfaceLifecycle::Persistent`]. On Android, `destroy_surfaces` drops
/// it again so the same handler is portable.
///
/// # Examples
///
/// ```
/// let app = ios_demo::IosDemoApp::default();
/// assert!(app.window().is_none());
/// ```
#[derive(Default)]
pub struct IosDemoApp {
    window: Option<Box<dyn Window>>,
}

impl IosDemoApp {
    /// Returns the live window, if the platform has created surfaces.
    ///
    /// # Examples
    ///
    /// ```
    /// let app = ios_demo::IosDemoApp::default();
    /// assert!(app.window().is_none());
    /// ```
    #[must_use]
    pub fn window(&self) -> Option<&dyn Window> {
        self.window.as_deref()
    }
}

impl ApplicationHandler for IosDemoApp {
    /// Creates the window the first time the platform permits surfaces.
    ///
    /// On iOS this fires once the `UIApplication` delegate reports the app
    /// finished launching; on desktop it fires once after `Init`.
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        match event_loop.create_window(WindowAttributes::default()) {
            Ok(window) => {
                let insets = window.safe_area();
                println!(
                    "ios_demo: window created; safe_area physical insets \
                     left={} top={} right={} bottom={}",
                    insets.left, insets.top, insets.right, insets.bottom,
                );
                window.request_redraw();
                self.window = Some(window);
            }
            Err(err) => {
                eprintln!("ios_demo: window creation failed: {err}");
                event_loop.exit();
            }
        }
    }

    /// iOS `applicationDidBecomeActive`: resume frame production.
    fn resumed(&mut self, _event_loop: &dyn ActiveEventLoop) {
        println!("ios_demo: resumed (lifecycle={:?})", surface_lifecycle());
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// iOS `applicationWillResignActive`: pause frame production; the
    /// surface itself stays alive (`SurfaceLifecycle::Persistent`).
    fn suspended(&mut self, _event_loop: &dyn ActiveEventLoop) {
        println!(
            "ios_demo: suspended; surface kept per {:?}",
            surface_lifecycle()
        );
    }

    /// The platform invalidated the native surface. This is never emitted
    /// on iOS; on Android the window's `NativeWindow` is destroyed and the
    /// surface must be re-created on the next `can_create_surfaces`.
    fn destroy_surfaces(&mut self, _event_loop: &dyn ActiveEventLoop) {
        match surface_lifecycle() {
            SurfaceLifecycle::RecreateOnSuspend => {
                println!("ios_demo: destroy_surfaces — dropping window for recreation");
                self.window = None;
            }
            _ => {
                println!("ios_demo: destroy_surfaces on a persistent-surface platform");
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = &self.window else {
            return;
        };
        if window.id() != window_id {
            return;
        }
        match &event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                event_loop.exit();
            }
            WindowEvent::SurfaceResized(size) => {
                println!("ios_demo: surface resized to {size:?}");
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                println!("ios_demo: scale factor changed to {scale_factor}");
            }
            WindowEvent::Ime(ime) => {
                // Text/preedit/commits from the software keyboard (enabled
                // via `martensite_window::ime::enable_ime`).
                println!("ios_demo: IME event {ime:?}");
            }
            WindowEvent::RedrawRequested => {
                // A real renderer would draw here; the demo requests the
                // next frame only while running in the foreground.
                window.request_redraw();
            }
            _ => {}
        }

        // Normalize pointer events (touch on iOS, mouse elsewhere) through
        // the shared conversion path, preserving PointerKind + PointerId.
        let scale = DpiScale::new(window.scale_factor());
        if let Some(pointer) = convert_window_event(&event, &scale) {
            println!(
                "ios_demo: pointer {:?} id={} kind={:?} at {:?}",
                pointer.state,
                pointer.pointer_id.get(),
                pointer.kind,
                pointer.position,
            );
        }
    }
}

/// Builds the [`EventLoop`] and runs [`IosDemoApp`].
///
/// On iOS this calls `UIApplicationMain` internally and **never returns**;
/// it is the body of the exported [`martensite_ios_demo_main`] entry
/// point. On desktop it returns when the window closes, which makes the
/// same handler smoke-testable from a host binary.
///
/// # Errors
///
/// Returns `Err` if the event loop cannot be created or `run_app` exits
/// with an error (desktop only — the iOS path diverges).
///
/// # Examples
///
/// ```no_run
/// // On a desktop host this runs the demo window until it is closed.
/// ios_demo::run().unwrap();
/// ```
pub fn run() -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::new()?;
    event_loop.run_app(IosDemoApp::default())
}

/// C entry point invoked by the Xcode/cargo-mobile2 app's `main.swift`
/// (or `main.m`) in place of `UIApplicationMain`.
///
/// winit's iOS backend calls `UIApplicationMain` inside `run_app`, so the
/// native launcher only has to call this symbol on the main thread:
///
/// ```swift
/// // main.swift in the generated Xcode app target
/// martensite_ios_demo_main()
/// ```
///
/// `run_app` never returns on iOS, matching the `UIApplicationMain`
/// contract.
// `#[no_mangle]` is an unsafe *attribute* (it can collide linker symbols);
// the workspace `unsafe_code = "deny"` lint is relaxed for this one item.
// The function body itself contains no unsafe code.
#[cfg(target_os = "ios")]
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn martensite_ios_demo_main() {
    // Unreachable on iOS: `run_app` calls `UIApplicationMain` and diverges.
    let _ = run();
    std::process::abort();
}
