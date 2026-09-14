//! Android backend glue for Martensite windows.
//!
//! # Backend: GameActivity
//!
//! Martensite uses winit's **GameActivity** backend — *not*
//! `NativeActivity`. `NativeActivity` delivers IME events unreliably and
//! the AccessKit adapter cannot inject its accessibility delegate into
//! its window, so other Rust UI frameworks disable accessibility there;
//! Martensite requires text input and accessibility to work. This crate
//! enables winit's `android-game-activity` feature for
//! `cfg(target_os = "android")`, which forwards to
//! `android-activity`'s `game-activity` feature. Do not enable
//! `android-native-activity` in the same build.
//!
//! # Entry point
//!
//! The application's `cdylib` must export `android_main`, which receives
//! the process-global [`AndroidApp`]. Hand it to [`new_event_loop`] to
//! associate it with the winit event loop:
//!
//! ```ignore
//! use martensite_window::android;
//!
//! #[no_mangle]
//! fn android_main(app: android::AndroidApp) {
//!     let event_loop = android::new_event_loop(app).expect("event loop");
//!     // event_loop.run_app(...);
//! }
//! ```
//!
//! # Surface lifecycle
//!
//! Android can destroy the `ANativeWindow` backing the surface while the
//! process keeps running. winit emits
//! [`ApplicationHandler::destroy_surfaces`] at that point — the app must
//! drop every `wgpu::Surface` *and* every `winit::window::Window` there
//! ([`WindowManager::destroy_all_windows`] is the manager-level entry
//! point), then recreate both in
//! [`ApplicationHandler::can_create_surfaces`]. `resumed`/`suspended`
//! bracket the activity's visible lifetime and are the right place to
//! start/stop frame production.
//!
//! [`WindowManager::destroy_all_windows`]: crate::WindowManager::destroy_all_windows
//!
//! # IME
//!
//! winit's Android backend implements `Window::request_ime_update` by
//! calling GameActivity's `GameTextInput` bridge
//! (`AndroidApp::show_soft_input` / `hide_soft_input`). The
//! [`show_soft_input`]/[`hide_soft_input`] helpers here wrap the
//! `ImeRequest` ceremony for the common show/hide cases.
//!
//! # Safe area
//!
//! [`Window::safe_area`] (via [`WindowEntry::safe_area`]) currently
//! returns zero insets on Android: `WindowInsets` must be read through
//! platform code until winit lands Android support.
//!
//! [`Window::safe_area`]: winit::window::Window::safe_area
//! [`WindowEntry::safe_area`]: crate::WindowEntry::safe_area
//! [`ApplicationHandler::destroy_surfaces`]: winit::application::ApplicationHandler::destroy_surfaces
//! [`ApplicationHandler::can_create_surfaces`]: winit::application::ApplicationHandler::can_create_surfaces

use winit::error::EventLoopError;
use winit::event_loop::EventLoop;
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::window::{
    ImeCapabilities, ImeEnableRequest, ImeRequest, ImeRequestData, ImeRequestError, Window,
};

// Re-exported so applications can type `android_main` without depending
// on `android-activity` directly — winit owns the version selection.
pub use winit::platform::android::activity::AndroidApp;
// Convenience re-exports of the winit Android extension traits used with
// the helpers in this module.
pub use winit::platform::android::{
    ActiveEventLoopExtAndroid, EventLoopExtAndroid, WindowExtAndroid,
};

/// Creates a winit [`EventLoop`] bound to the [`AndroidApp`] passed to
/// `android_main`.
///
/// The `AndroidApp` is not global state — it must be threaded into the
/// event loop at build time via
/// [`EventLoopBuilderExtAndroid::with_android_app`].
///
/// # Errors
///
/// Returns [`EventLoopError`] if the platform event loop cannot be
/// created (for example when called more than once, or off the
/// `android_main` thread).
///
/// # Examples
///
/// ```ignore
/// use martensite_window::android;
///
/// # fn android_main(app: android::AndroidApp) {
/// let event_loop = android::new_event_loop(app).expect("event loop");
/// # }
/// ```
pub fn new_event_loop(app: AndroidApp) -> Result<EventLoop, EventLoopError> {
    EventLoop::builder().with_android_app(app).build()
}

/// Requests that the soft (on-screen) keyboard be shown for `window`.
///
/// This issues an [`ImeRequest::Enable`] with no extra capabilities,
/// which the Android backend forwards to
/// `GameActivity_showSoftInput`/`GameTextInput_showIme`. Composition and
/// commit text then arrive as `WindowEvent::Ime` events.
///
/// # Errors
///
/// Returns [`ImeRequestError::AlreadyEnabled`] if IME input was already
/// enabled for this window.
///
/// # Examples
///
/// ```ignore
/// # fn example(window: &dyn winit::window::Window) {
/// martensite_window::android::show_soft_input(window).ok();
/// # }
/// ```
pub fn show_soft_input(window: &dyn Window) -> Result<(), ImeRequestError> {
    // `ImeEnableRequest::new` only fails when requested capabilities are
    // missing their matching request data; empty capabilities with a
    // default request always succeed.
    let enable = ImeEnableRequest::new(ImeCapabilities::new(), ImeRequestData::default())
        .expect("empty IME capabilities require no request data");
    window.request_ime_update(ImeRequest::Enable(enable))
}

/// Requests that the soft (on-screen) keyboard be hidden for `window`.
///
/// Issues [`ImeRequest::Disable`], which the Android backend forwards to
/// `GameActivity_hideSoftInput`. The request cannot fail.
///
/// # Errors
///
/// Returns [`ImeRequestError`] from the underlying
/// `Window::request_ime_update` call (none are produced for `Disable`
/// today; the `Result` is kept for forward compatibility).
///
/// # Examples
///
/// ```ignore
/// # fn example(window: &dyn winit::window::Window) {
/// martensite_window::android::hide_soft_input(window).ok();
/// # }
/// ```
pub fn hide_soft_input(window: &dyn Window) -> Result<(), ImeRequestError> {
    window.request_ime_update(ImeRequest::Disable)
}
