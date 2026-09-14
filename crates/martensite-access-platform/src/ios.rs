//! iOS accessibility adapter — the Objective-C FFI boundary.
//!
//! This module wraps [`accesskit_ios::SubclassingAdapter`], which uses
//! dynamic Objective-C subclassing to install `UIAccessibilityContainer`
//! (`isAccessibilityElement`, `accessibilityElements`) and
//! `UIAccessibilityHitTest` (`accessibilityHitTest:`) methods on the
//! `UIView` that winit creates, without requiring control over the view's
//! class.
//!
//! # Upstream maturity (Phase 1)
//!
//! `accesskit_ios` 0.2.x is a Phase-1 implementation: it exports the
//! accessibility tree's basic traits and properties (roles, labels,
//! bounds, focus, actions such as tap/focus) to VoiceOver, but its
//! editable-text support is incomplete — `UIAccessibilityTextInput`-
//! style editing callbacks (insert/delete, selection ranges, attributed
//! text) are not yet implemented upstream. VoiceOver users can navigate
//! and activate widgets, but full text-field editing via the rotor is
//! degraded until upstream matures. This limitation is inherited by every
//! consumer of this adapter and is tracked as upstream debt for v1.0.

use std::ffi::c_void;
use std::ptr::NonNull;

use accesskit::{ActionHandler, ActivationHandler, DeactivationHandler, TreeUpdate};
use accesskit_ios::SubclassingAdapter;

/// iOS platform adapter wrapping `accesskit_ios`'s `SubclassingAdapter`.
///
/// The adapter dynamically subclasses the `UIView` behind a winit window
/// so VoiceOver can enumerate and interact with the AccessKit tree. All
/// handler callbacks (`request_initial_tree`, `do_action`,
/// `deactivate_accessibility`) are invoked on the main thread by the
/// platform adapter.
///
/// Create the adapter before the view is shown or focused for the first
/// time — in practice, immediately after window creation inside
/// `ApplicationHandler::can_create_surfaces`.
///
/// # Upstream maturity
///
/// See the [module-level documentation](self) for the Phase-1 editable
/// text limitation inherited from `accesskit_ios` 0.2.x.
///
/// # Examples
///
/// ```ignore
/// // iOS only: `view` is the `UIView *` behind a winit window, obtained
/// // from `RawWindowHandle::UiKit(handle).ui_view`.
/// let adapter = martensite_access_platform::ios::IosAdapter::new(
///     view, activation_handler, action_handler, deactivation_handler,
/// );
/// ```
pub struct IosAdapter {
    inner: SubclassingAdapter,
}

impl IosAdapter {
    /// Creates an adapter that dynamically subclasses `view`.
    ///
    /// `view` is the raw `UIView *` owned by the winit window (obtained via
    /// `RawWindowHandle::UiKit(handle).ui_view`). The Objective-C
    /// subclassing is applied immediately and is reverted when the adapter
    /// is dropped.
    ///
    /// # Caller requirements
    ///
    /// These preconditions come from the upstream
    /// `accesskit_ios::SubclassingAdapter` contract and cannot be checked
    /// by the type system for a raw windowing handle; callers must uphold
    /// them:
    ///
    /// - `view` must point to a valid, live `UIView` that outlives the
    ///   adapter (the winit window's view does).
    /// - The call must run on the main thread. Upstream enforces this
    ///   with an internal `MainThreadMarker` unwrap, so a violation
    ///   panics rather than corrupting state.
    /// - The adapter must be created before the view is shown or focused
    ///   for the first time — in practice, inside
    ///   `ApplicationHandler::can_create_surfaces` right after window
    ///   creation. Note that winit-uikit's `Window::is_visible` returns
    ///   `None`, so the before-show check in
    ///   `accesskit_winit::Adapter::with_direct_handlers` cannot fire on
    ///   iOS; the ordering is upheld by convention at the call site.
    /// - Only one adapter may exist per view; creating a second panics
    ///   upstream.
    ///
    /// # Panics
    ///
    /// Panics if called off the main thread or if an adapter already
    /// exists on `view` (upstream `MainThreadMarker`/associated-object
    /// checks).
    pub fn new(
        view: NonNull<c_void>,
        activation_handler: impl 'static + ActivationHandler,
        action_handler: impl 'static + ActionHandler,
        deactivation_handler: impl 'static + DeactivationHandler,
    ) -> Self {
        // SAFETY: forwarded from the documented caller requirements —
        // `view` is a valid, unreleased `UIView *`, and this call happens
        // on the main thread before the view is shown. All Objective-C
        // runtime FFI is confined to this whitelisted-unsafe crate.
        let inner = unsafe {
            SubclassingAdapter::new(
                view.as_ptr(),
                activation_handler,
                action_handler,
                deactivation_handler,
            )
        };
        Self { inner }
    }

    /// Creates an adapter for the root view of a `UIWindow`.
    ///
    /// Equivalent to [`IosAdapter::new`] but resolves the view from the
    /// window's `rootViewController` instead of taking it directly.
    ///
    /// # Caller requirements
    ///
    /// - `window` must point to a valid, live `UIWindow` whose root view
    ///   controller currently has a view (the function panics otherwise).
    /// - The call must run on the main thread.
    ///
    /// # Panics
    ///
    /// Panics if `window` has no root view controller with a view, or if
    /// called off the main thread.
    pub fn for_window(
        window: NonNull<c_void>,
        activation_handler: impl 'static + ActivationHandler,
        action_handler: impl 'static + ActionHandler,
        deactivation_handler: impl 'static + DeactivationHandler,
    ) -> Self {
        // SAFETY: forwarded from the documented caller requirements —
        // `window` is a valid, unreleased `UIWindow *` with a root view
        // controller, and this call happens on the main thread.
        let inner = unsafe {
            SubclassingAdapter::for_window(
                window.as_ptr(),
                activation_handler,
                action_handler,
                deactivation_handler,
            )
        };
        Self { inner }
    }

    /// If and only if the accessibility tree has been initialized, calls
    /// `updater` and applies the resulting [`TreeUpdate`], raising any
    /// queued platform events (e.g. VoiceOver focus/announcement
    /// notifications).
    ///
    /// If the caller's [`ActivationHandler::request_initial_tree`]
    /// initially returned `None`, the [`TreeUpdate`] produced by `updater`
    /// must contain a full tree.
    pub fn update_if_active(&self, updater: impl FnOnce() -> TreeUpdate) {
        if let Some(events) = self.inner.update_if_active(updater) {
            events.raise();
        }
    }
}

impl std::fmt::Debug for IosAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IosAdapter").finish_non_exhaustive()
    }
}
