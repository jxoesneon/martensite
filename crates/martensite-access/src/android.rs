//! Android accessibility adapter wiring (GameActivity only).
//!
//! On Android the platform adapter is `accesskit_android`'s
//! `InjectingAdapter`, reached through the same
//! `accesskit_winit::Adapter::with_direct_handlers` seam used on the
//! desktop platforms. The adapter injects an accessibility delegate into
//! the GameActivity `InputEnabledSurfaceView`; the JNI boundary itself
//! lives in `martensite-access-platform`, the workspace's audited-unsafe
//! mobile FFI crate.
//!
//! **GameActivity is required.** `NativeActivity` is unsupported: the
//! `mSurfaceView` field the adapter resolves does not exist there, IME
//! events are unreliable, and AccessKit cannot inject its delegate (the
//! same reason other Rust UI frameworks disable accessibility on
//! `NativeActivity`). This crate enables winit's `android-game-activity`
//! feature for `cfg(target_os = "android")`, which selects the
//! GameActivity backend for the whole application.
//!
//! # Examples
//!
//! ```ignore
//! use martensite_access::android;
//! # fn example(
//! #     event_loop: &dyn winit::event_loop::ActiveEventLoop,
//! #     window: &dyn winit::window::Window,
//! #     bridge: &std::sync::Arc<martensite_access::winit::MartensiteAccessBridge>,
//! # ) {
//! // Create inside `ApplicationHandler::can_create_surfaces`, right after
//! // the window.
//! let adapter = android::create_adapter(event_loop, window, bridge);
//! # }
//! ```

use std::sync::Arc;

use winit::{event_loop::ActiveEventLoop, window::Window};

use crate::winit::MartensiteAccessBridge;

/// Creates the platform AccessKit adapter for an Android window.
///
/// `event_loop` and `window` are the values winit hands to
/// [`ApplicationHandler`] callbacks; `bridge` is the shared
/// [`MartensiteAccessBridge`] that owns the arena and produces tree
/// updates. The returned adapter should be driven from the window-event
/// path: forward `winit` window events via
/// [`accesskit_winit::Adapter::process_event`] and push tree changes via
/// [`accesskit_winit::Adapter::update_if_active`].
///
/// [`ApplicationHandler`]: winit::application::ApplicationHandler
///
/// # Panics
///
/// Panics if the window is already visible (per
/// [`accesskit_winit::Adapter::with_direct_handlers`]; Android reports
/// `None` so this cannot fire there) or if the `GameActivity` surface
/// view cannot be resolved — i.e. the process is not running under
/// `GameActivity`.
///
/// # Examples
///
/// ```ignore
/// use martensite_access::android;
/// # fn example(
/// #     event_loop: &dyn winit::event_loop::ActiveEventLoop,
/// #     window: &dyn winit::window::Window,
/// #     bridge: &std::sync::Arc<martensite_access::winit::MartensiteAccessBridge>,
/// # ) {
/// let adapter = android::create_adapter(event_loop, window, bridge);
/// # }
/// ```
pub fn create_adapter(
    event_loop: &dyn ActiveEventLoop,
    window: &dyn Window,
    bridge: &Arc<MartensiteAccessBridge>,
) -> accesskit_winit::Adapter {
    let handlers = bridge.handlers();
    accesskit_winit::Adapter::with_direct_handlers(
        event_loop,
        window,
        handlers.clone(),
        handlers.clone(),
        handlers,
    )
}
