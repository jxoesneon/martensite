//! Event-driven sleep enforcement via [`winit::event_loop::ControlFlow`].
//!
//! Martensite's Law III requires the event loop to yield to kernel wait
//! states when the application has no pending work. This module provides
//! [`QuiescentApp`], an [`ApplicationHandler`] decorator that wraps the
//! user's handler and enforces `ControlFlow::Wait` when the app reports
//! itself as idle.
//!
//! # What counts as "idle"
//!
//! The app is quiescent when **all** of the following hold:
//!
//! - No widget node has `DIRTY_LAYOUT`, `DIRTY_PAINT`, or `DIRTY_A11Y`
//!   flags set (the reactive system has nothing to propagate).
//! - No spring animations are active (all `AnimationDriver`s report
//!   `active_count() == 0`).
//! - No window has a pending `request_redraw` that hasn't been serviced.
//! - No external engine surface has a frame awaiting composite.
//!
//! # Example
//!
//! ```no_run
//! use martensite_window::quiescent::{QuiescentApp, Quiescence};
//! use winit::application::ApplicationHandler;
//! use winit::event::WindowEvent;
//! use winit::event_loop::{ActiveEventLoop, EventLoop};
//! use winit::window::{WindowAttributes, WindowId};
//!
//! struct App {
//!     dirty: bool,
//! }
//!
//! impl ApplicationHandler for App {
//!     fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
//!         let attrs = WindowAttributes::default().with_title("App");
//!         let _window = event_loop.create_window(attrs).unwrap();
//!     }
//!     fn window_event(
//!         &mut self,
//!         event_loop: &dyn ActiveEventLoop,
//!         _id: WindowId,
//!         event: WindowEvent,
//!     ) {
//!         if matches!(event, WindowEvent::CloseRequested) {
//!             event_loop.exit();
//!         }
//!         self.dirty = true;
//!     }
//!     fn about_to_wait(&mut self, _event_loop: &dyn ActiveEventLoop) {}
//! }
//!
//! let event_loop = EventLoop::new().unwrap();
//! let app = QuiescentApp::new(App { dirty: false }, |app: &App| {
//!     Quiescence {
//!         arena_dirty: app.dirty,
//!         ..Quiescence::default()
//!     }
//! });
//! event_loop.run_app(app).unwrap();
//! ```

use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

/// A snapshot of whether the application has pending work.
///
/// The user supplies this via a closure each time the event loop is
/// about to wait. When [`Quiescence::is_idle`] returns `true`, the
/// event loop transitions to [`ControlFlow::Wait`], yielding to the
/// kernel until the next OS event arrives.
///
/// # Examples
///
/// ```
/// use martensite_window::quiescent::Quiescence;
///
/// let idle = Quiescence::default();
/// assert!(idle.is_idle());
///
/// let busy = Quiescence {
///     arena_dirty: true,
///     ..Quiescence::default()
/// };
/// assert!(!busy.is_idle());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Quiescence {
    /// Any widget node has `DIRTY_LAYOUT`, `DIRTY_PAINT`, or
    /// `DIRTY_A11Y` set — the reactive system has unpropagated changes.
    pub arena_dirty: bool,
    /// Number of active (non-settled) spring animations across all
    /// `AnimationDriver`s.
    pub active_animations: usize,
    /// Number of windows with a pending `request_redraw` that has not
    /// yet been serviced by a `RedrawRequested` event.
    pub pending_redraws: usize,
    /// Number of external engine surfaces with frames awaiting
    /// composite into the paint ring.
    pub engine_frames_pending: usize,
}

impl Quiescence {
    /// Returns `true` when the application has no pending work and the
    /// event loop should sleep in `ControlFlow::Wait`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::quiescent::Quiescence;
    ///
    /// assert!(Quiescence::default().is_idle());
    /// ```
    pub fn is_idle(&self) -> bool {
        !self.arena_dirty
            && self.active_animations == 0
            && self.pending_redraws == 0
            && self.engine_frames_pending == 0
    }
}

/// An [`ApplicationHandler`] decorator that enforces event-driven sleep.
///
/// Wraps the user's handler and, after each `about_to_wait` call,
/// checks the app's quiescence state. If idle, sets
/// `ControlFlow::Wait`; otherwise `ControlFlow::Poll`.
///
/// The check is a `FnMut(&H) -> Quiescence` closure that receives the
/// inner handler and returns the current quiescence snapshot.
///
/// # Examples
///
/// See the [module-level documentation](self) for a complete example.
pub struct QuiescentApp<H, C>
where
    H: ApplicationHandler,
    C: FnMut(&H) -> Quiescence,
{
    inner: H,
    quiescence_check: C,
    /// Whether we've already logged the first quiescence transition.
    /// Prevents log spam on every frame.
    logged_first_idle: bool,
}

impl<H, C> QuiescentApp<H, C>
where
    H: ApplicationHandler,
    C: FnMut(&H) -> Quiescence,
{
    /// Wraps `inner` with quiescence enforcement.
    ///
    /// `check` is called after every `about_to_wait` to decide whether
    /// the event loop should sleep (`ControlFlow::Wait`) or keep
    /// processing (`ControlFlow::Poll`).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_window::quiescent::{QuiescentApp, Quiescence};
    /// use winit::application::ApplicationHandler;
    /// use winit::event::WindowEvent;
    /// use winit::event_loop::ActiveEventLoop;
    /// use winit::window::WindowId;
    ///
    /// struct MyApp;
    /// impl ApplicationHandler for MyApp {
    ///     fn can_create_surfaces(&mut self, _event_loop: &dyn ActiveEventLoop) {}
    ///     fn window_event(
    ///         &mut self,
    ///         _event_loop: &dyn ActiveEventLoop,
    ///         _window_id: WindowId,
    ///         _event: WindowEvent,
    ///     ) {
    ///     }
    /// }
    ///
    /// let app = QuiescentApp::new(MyApp, |_app: &MyApp| Quiescence::default());
    /// ```
    pub fn new(inner: H, check: C) -> Self {
        Self {
            inner,
            quiescence_check: check,
            logged_first_idle: false,
        }
    }

    /// Returns a reference to the inner handler.
    pub fn inner(&self) -> &H {
        &self.inner
    }

    /// Returns a mutable reference to the inner handler.
    pub fn inner_mut(&mut self) -> &mut H {
        &mut self.inner
    }
}

impl<H, C> ApplicationHandler for QuiescentApp<H, C>
where
    H: ApplicationHandler,
    C: FnMut(&H) -> Quiescence,
{
    fn new_events(&mut self, event_loop: &dyn ActiveEventLoop, cause: StartCause) {
        self.inner.new_events(event_loop, cause);
    }

    fn resumed(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.resumed(event_loop);
    }

    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.can_create_surfaces(event_loop);
    }

    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.proxy_wake_up(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        self.inner.window_event(event_loop, window_id, event);
    }

    fn device_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        device_id: Option<DeviceId>,
        event: DeviceEvent,
    ) {
        self.inner.device_event(event_loop, device_id, event);
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.about_to_wait(event_loop);

        let q = (self.quiescence_check)(&self.inner);
        if q.is_idle() {
            if !self.logged_first_idle {
                tracing::debug!("event loop entering quiescent state (ControlFlow::Wait)");
                self.logged_first_idle = true;
            }
            event_loop.set_control_flow(ControlFlow::Wait);
        } else {
            if self.logged_first_idle {
                tracing::debug!("event loop waking from quiescent state");
                self.logged_first_idle = false;
            }
            event_loop.set_control_flow(ControlFlow::Poll);
        }
    }

    fn suspended(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.suspended(event_loop);
    }

    fn destroy_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.destroy_surfaces(event_loop);
    }

    fn memory_warning(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.memory_warning(event_loop);
    }

    fn macos_handler(
        &mut self,
    ) -> Option<&mut dyn winit::application::macos::ApplicationHandlerExtMacOS> {
        self.inner.macos_handler()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiescence_default_is_idle() {
        assert!(Quiescence::default().is_idle());
    }

    #[test]
    fn quiescence_dirty_arena_not_idle() {
        let q = Quiescence {
            arena_dirty: true,
            ..Quiescence::default()
        };
        assert!(!q.is_idle());
    }

    #[test]
    fn quiescence_active_animations_not_idle() {
        let q = Quiescence {
            active_animations: 3,
            ..Quiescence::default()
        };
        assert!(!q.is_idle());
    }

    #[test]
    fn quiescence_pending_redraws_not_idle() {
        let q = Quiescence {
            pending_redraws: 1,
            ..Quiescence::default()
        };
        assert!(!q.is_idle());
    }

    #[test]
    fn quiescence_engine_frames_not_idle() {
        let q = Quiescence {
            engine_frames_pending: 1,
            ..Quiescence::default()
        };
        assert!(!q.is_idle());
    }

    #[test]
    fn quiescence_all_sources_blocks_idle() {
        let q = Quiescence {
            arena_dirty: true,
            active_animations: 1,
            pending_redraws: 2,
            engine_frames_pending: 1,
        };
        assert!(!q.is_idle());
    }
}
