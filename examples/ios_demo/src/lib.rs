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
//!   `safeAreaInsets`; the demo logs them after window creation and again
//!   on every `SurfaceResized` (insets can change on rotation/split-view,
//!   and are only authoritative once the view is laid out).
//! - **Touch routing:** winit `PointerMoved`/`PointerButton` events are
//!   normalized through [`convert_window_event`], preserving the
//!   `PointerKind` (`Touch`, `Mouse`, `Tablet`, `Unknown`) and the
//!   per-finger `PointerId`.
//! - **IME:** [`WindowEvent::Ime`] events are logged; production callers
//!   drive the software keyboard through `martensite_window::ime`.
//! - **Accessibility (iOS):** [`can_create_surfaces`] constructs the real
//!   adapter chain — `MartensiteAccessBridge` handlers behind
//!   `martensite_access_platform::ios::IosAdapter` — against the window's
//!   `UIView`, exporting the tree to VoiceOver.
//!
//! [`can_create_surfaces`]: winit::application::ApplicationHandler::can_create_surfaces
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
    /// The AccessKit adapter chain, alive for the window's lifetime
    /// (iOS only). Kept so the `SubclassingAdapter` stays attached to the
    /// `UIView`; the bridge lets the demo flush queued a11y actions.
    #[cfg(target_os = "ios")]
    access: Option<IosA11y>,
}

/// The iOS accessibility bundle: the bridge (arena + tree adapter +
/// action queue) plus the platform adapter subclassing the `UIView`.
#[cfg(target_os = "ios")]
struct IosA11y {
    #[allow(dead_code)]
    bridge: SharedBridge,
    #[allow(dead_code)]
    adapter: martensite_access_platform::ios::IosAdapter,
}

/// Shares one `MartensiteAccessBridge` across the three AccessKit handler
/// roles the platform adapter requires.
///
/// AccessKit's `ActivationHandler`/`ActionHandler`/`DeactivationHandler`
/// traits take `&mut self`, and the adapter stores each handler as a
/// separate object — so a single bridge cannot be passed three times
/// directly. Locking the shared bridge per callback satisfies the
/// ownership split; the bridge's own internal `Mutex` already serializes
/// its state, so the outer lock is contention-free on the main thread.
#[cfg(target_os = "ios")]
#[derive(Clone)]
struct SharedBridge(
    std::sync::Arc<std::sync::Mutex<martensite_access::winit::MartensiteAccessBridge>>,
);

#[cfg(target_os = "ios")]
impl SharedBridge {
    fn lock(&self) -> std::sync::MutexGuard<'_, martensite_access::winit::MartensiteAccessBridge> {
        self.0.lock().expect("access bridge mutex poisoned")
    }
}

#[cfg(target_os = "ios")]
impl accesskit::ActivationHandler for SharedBridge {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        self.lock().request_initial_tree()
    }
}

#[cfg(target_os = "ios")]
impl accesskit::ActionHandler for SharedBridge {
    fn do_action(&mut self, request: accesskit::ActionRequest) {
        self.lock().do_action(request);
    }
}

#[cfg(target_os = "ios")]
impl accesskit::DeactivationHandler for SharedBridge {
    fn deactivate_accessibility(&mut self) {
        self.lock().deactivate_accessibility();
    }
}

/// Logs the window's safe-area insets. Called after window creation and
/// on every `SurfaceResized`: UIKit reports meaningful `safeAreaInsets`
/// only once the view is laid out, and the insets change on rotation and
/// multitasking size changes, so a single creation-time query is not
/// authoritative.
fn log_safe_area(window: &dyn Window, context: &str) {
    let insets = window.safe_area();
    println!(
        "ios_demo: safe_area ({context}) physical insets \
         left={} top={} right={} bottom={}",
        insets.left, insets.top, insets.right, insets.bottom,
    );
}

/// Constructs the real iOS accessibility adapter chain for `window`:
/// a `MartensiteAccessBridge` over a fresh `WidgetArena` feeding an
/// `IosAdapter` that subclasses the window's `UIView`.
///
/// Must run on the main thread before the view is shown —
/// `can_create_surfaces` satisfies both. The adapter activates lazily:
/// UIKit queries `accessibilityElements` when VoiceOver (or another
/// assistive technology) is enabled.
#[cfg(target_os = "ios")]
fn create_accessibility(window: &dyn Window) -> Option<IosA11y> {
    use martensite_access::{winit::MartensiteAccessBridge, AccessKitAdapter};
    use martensite_access_platform::ios::IosAdapter;
    use martensite_core::WidgetArena;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let view = match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::UiKit(handle) => handle.ui_view,
        _ => return None,
    };
    let mut arena = WidgetArena::new();
    let root = arena.insert(Default::default(), Default::default());
    let bridge = SharedBridge(std::sync::Arc::new(std::sync::Mutex::new(
        MartensiteAccessBridge::new(arena, AccessKitAdapter::new(root)),
    )));
    let adapter = IosAdapter::new(view, bridge.clone(), bridge.clone(), bridge.clone());
    Some(IosA11y { bridge, adapter })
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
                // Note: insets queried pre-layout may be zero; they are
                // re-queried on every SurfaceResized below.
                log_safe_area(&*window, "window created");
                #[cfg(target_os = "ios")]
                {
                    self.access = create_accessibility(&*window);
                    println!(
                        "ios_demo: accesskit adapter {}",
                        if self.access.is_some() {
                            "attached to UIView"
                        } else {
                            "unavailable (no UIKit handle)"
                        },
                    );
                }
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
                // Safe-area insets change with layout (rotation, split
                // view, first layout pass) — re-query here rather than
                // trusting the creation-time value.
                log_safe_area(&**window, "surface resized");
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
/// it is the body of the exported `martensite_ios_demo_main` entry
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
// `#[no_mangle]` is an unsafe *attribute*: since Rust 1.82 the
// `unsafe_code` lint rejects it in every edition (verified — this crate
// failed to compile for iOS under `#![forbid(unsafe_code)]` without this
// scoped allow). The attribute can collide linker symbols; the function
// body itself contains no unsafe code.
#[cfg(target_os = "ios")]
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn martensite_ios_demo_main() {
    // Unreachable on iOS: `run_app` calls `UIApplicationMain` and diverges.
    let _ = run();
    std::process::abort();
}
