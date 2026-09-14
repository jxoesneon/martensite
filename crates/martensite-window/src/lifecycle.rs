//! Platform surface-lifecycle policy for the winit 0.31 mobile model.
//!
//! winit 0.31 expresses the mobile windowing contract through the
//! [`ApplicationHandler`] callbacks:
//!
//! - [`can_create_surfaces`] — the first point at which windows and GPU
//!   surfaces may be created. Every platform emits it (on desktop it is a
//!   one-shot after `StartCause::Init`); applications should create their
//!   window, `wgpu::Surface`, and render pipeline here.
//! - [`destroy_surfaces`] — emitted when the platform invalidates the
//!   native surface. On **Android** the `NativeWindow` is destroyed when
//!   the activity stops, so GPU surfaces must be dropped inside this
//!   callback and re-created on the next [`can_create_surfaces`]. On
//!   **iOS** this callback is *never emitted*: the `UIView`'s
//!   `CAMetalLayer` survives backgrounding, so surfaces are persistent.
//! - [`resumed`] / [`suspended`] — activity-level pause/resume
//!   notifications. On iOS they map to
//!   `applicationDidBecomeActive` / `applicationWillResignActive` and are
//!   the right place to stop and restart frame production (redraw
//!   requests, animation ticks); the surface itself stays alive.
//!
//! [`surface_lifecycle`] returns the policy a target platform requires so
//! the render pipeline can decide whether dropping the surface on
//! `suspended`/`destroy_surfaces` is mandatory or unnecessary.
//!
//! [`ApplicationHandler`]: winit::application::ApplicationHandler
//! [`can_create_surfaces`]: winit::application::ApplicationHandler::can_create_surfaces
//! [`destroy_surfaces`]: winit::application::ApplicationHandler::destroy_surfaces
//! [`resumed`]: winit::application::ApplicationHandler::resumed
//! [`suspended`]: winit::application::ApplicationHandler::suspended

/// How the target platform treats GPU surfaces across the application
/// lifecycle.
///
/// # Examples
///
/// ```
/// use martensite_window::lifecycle::SurfaceLifecycle;
///
/// // On desktop and iOS the surface outlives suspend/resume cycles.
/// assert_eq!(
///     martensite_window::lifecycle::surface_lifecycle(),
///     SurfaceLifecycle::Persistent,
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SurfaceLifecycle {
    /// GPU surfaces remain valid across `suspended`/`resumed`. The
    /// application may keep its `wgpu::Surface` and swapchain for the
    /// entire window lifetime. This is the desktop model, and also the
    /// iOS model: `winit-uikit` never emits `destroy_surfaces`, and the
    /// `CAMetalLayer`-backed view survives backgrounding.
    ///
    /// Rendering should still be paused on `suspended` and resumed on
    /// `resumed` to save power.
    Persistent,
    /// GPU surfaces are invalidated when the platform emits
    /// `destroy_surfaces` and must be re-created on the next
    /// `can_create_surfaces`. This is the Android model (`NativeWindow`
    /// destruction on `onStop`).
    RecreateOnSuspend,
}

/// Returns the [`SurfaceLifecycle`] policy for the target this crate was
/// compiled for.
///
/// - **iOS:** [`SurfaceLifecycle::Persistent`] — `winit-uikit` never emits
///   `destroy_surfaces`; the `UIView`'s `CAMetalLayer` survives
///   `suspended`/`resumed`. Applications should still pause rendering on
///   `suspended` (the system may kill a backgrounded app that keeps
///   producing frames).
/// - **Android:** [`SurfaceLifecycle::RecreateOnSuspend`] — the
///   `NativeWindow` is destroyed on `onStop` and winit emits
///   `destroy_surfaces`.
/// - **All other platforms:** [`SurfaceLifecycle::Persistent`] — desktop
///   platforms have no surface destroy/create lifecycle; surfaces live
///   as long as their window.
///
/// # Examples
///
/// ```
/// use martensite_window::lifecycle::{surface_lifecycle, SurfaceLifecycle};
///
/// // Outside Android, surfaces do not need to be re-created after
/// // suspend/resume.
/// assert_eq!(surface_lifecycle(), SurfaceLifecycle::Persistent);
/// ```
#[must_use]
pub const fn surface_lifecycle() -> SurfaceLifecycle {
    #[cfg(target_os = "android")]
    {
        SurfaceLifecycle::RecreateOnSuspend
    }
    #[cfg(not(target_os = "android"))]
    {
        SurfaceLifecycle::Persistent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_surface_lifecycle_is_persistent() {
        // CI hosts are desktop platforms; the assertion documents that
        // `surface_lifecycle` only diverges on Android.
        #[cfg(not(target_os = "android"))]
        assert_eq!(surface_lifecycle(), SurfaceLifecycle::Persistent);
        #[cfg(target_os = "android")]
        assert_eq!(surface_lifecycle(), SurfaceLifecycle::RecreateOnSuspend);
    }
}
