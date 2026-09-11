//! Multi-window management built on top of [`slotmap`] storage.
//!
//! [`WindowManager`] owns every open window in the application, tracks the
//! DPI scale factor reported by the platform for each one, and routes
//! [`winit`] window events to the appropriate per-window state.
//!
//! Windows are addressed by an opaque [`WindowKey`] returned from
//! [`WindowManager::create_window`]. Internally the manager uses a
//! [`SlotMap`] so that keys remain stable across insertions and removals and
//! dangling keys (from a window that has since been destroyed) are detected
//! cheaply.
//!
//! # Event routing
//!
//! [`WindowManager::handle_window_event`] inspects a `WindowEvent` for a
//! given [`WindowId`] and:
//!
//! - updates the per-window DPI scale factor on
//!   `WindowEvent::ScaleFactorChanged`,
//! - signals that the window should be closed on
//!   `WindowEvent::CloseRequested`,
//! - signals that the window entry must be dropped on
//!   `WindowEvent::Destroyed`.
//!
//! The returned [`WindowEventOutcome`] lets the surrounding event loop
//! decide whether to tear down a surface, exit the application when the last
//! window closes, and so on.

use slotmap::{new_key_type, SlotMap};

use martensite_shell::{ShellEvent, ShellEventQueue};

use crate::dpi::DpiScale;
use crate::{Window, WindowId};

// Generate a strongly-typed key for the window slotmap. This prevents the
// window key from being confused with any other slotmap key in the program.
new_key_type! {
    /// Opaque, stable handle to a window tracked by a [`WindowManager`].
    ///
    /// Keys remain valid across insertions and removals and are cheap to
    /// copy. A key whose window has been destroyed will no longer resolve
    /// via [`WindowManager::window`] — such a key is simply stale and
    /// yields `None` rather than panicking.
    pub struct WindowKey;
}

/// The outcome of routing a single `WindowEvent` through
/// [`WindowManager::handle_window_event`].
///
/// Callers (typically an [`ApplicationHandler`]) inspect this to decide
/// whether to tear down GPU surfaces, request application exit, or simply
/// continue processing further events.
///
/// # Examples
///
/// ```
/// use martensite_window::WindowEventOutcome;
///
/// // `None` means no special action is required.
/// assert_eq!(WindowEventOutcome::None, WindowEventOutcome::None);
///
/// // Scale-factor changes carry the new factor for surface reconfiguration.
/// assert_ne!(
///     WindowEventOutcome::ScaleFactorChanged(1.0),
///     WindowEventOutcome::ScaleFactorChanged(2.0),
/// );
///
/// // Occlusion reports let callers skip rendering while hidden.
/// assert_ne!(WindowEventOutcome::Occluded(true), WindowEventOutcome::Occluded(false));
/// ```
///
/// [`ApplicationHandler`]: winit::application::ApplicationHandler
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum WindowEventOutcome {
    /// The event was observed but requires no special action from the
    /// caller.
    None,
    /// The user requested the window be closed (e.g. clicked the title-bar
    /// close button). The manager has *not* removed the window yet — the
    /// caller should destroy any associated GPU surface and then call
    /// [`WindowManager::destroy_window`].
    CloseRequested,
    /// The platform destroyed the underlying window. The manager has
    /// already removed the corresponding [`WindowEntry`]; the caller should
    /// drop any resources keyed on that [`WindowId`].
    Destroyed,
    /// The window's DPI scale factor changed. The manager has updated the
    /// per-window scale; the new factor is provided so the caller can
    /// reconfigure surface extents and re-rasterize at the new resolution.
    ScaleFactorChanged(f64),
    /// A redraw was requested for the window. The caller should render and
    /// present a new frame.
    RedrawRequested,
    /// The window's occlusion state changed. `true` means the window is fully
    /// occluded (e.g. another window covers it); `false` means it is at least
    /// partially visible. The caller can use this to skip rendering while
    /// occluded to save power.
    Occluded(bool),
    /// The window's fractional scale factor changed (Wayland
    /// `wp_fractional_scale_v1`). The value is a fractional scale
    /// factor (e.g. 1.5 for 150% DPI). The caller should reconfigure
    /// the surface at the new physical resolution.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::WindowEventOutcome;
    ///
    /// // Fractional scale changes carry the new factor.
    /// assert_ne!(
    ///     WindowEventOutcome::FractionalScaleChanged(1.0),
    ///     WindowEventOutcome::FractionalScaleChanged(1.5),
    /// );
    /// ```
    FractionalScaleChanged(f64),
    /// The system theme appearance changed (macOS `NSAppearance`
    /// notification). The caller should trigger a theme transition.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::WindowEventOutcome;
    ///
    /// // Theme appearance changes carry no payload.
    /// assert_eq!(
    ///     WindowEventOutcome::ThemeAppearanceChanged,
    ///     WindowEventOutcome::ThemeAppearanceChanged,
    /// );
    /// ```
    ThemeAppearanceChanged,
}

/// A single tracked window and its associated per-window state.
///
/// Each entry bundles the owning [`Window`] handle, the DPI scale factor
/// currently in effect for the window, and the winit-assigned [`WindowId`]
/// used to correlate incoming events.
#[derive(Debug)]
pub struct WindowEntry {
    /// The platform window handle.
    pub window: Box<dyn Window>,
    /// The current DPI scale factor (physical-to-logical ratio) for this
    /// window.
    pub dpi_scale: f64,
    /// The winit identifier used to route events to this window.
    pub id: WindowId,
}

impl WindowEntry {
    /// Returns a [`DpiScale`] view over this window's current scale factor.
    ///
    /// This is a convenience for callers that want the full
    /// physical↔logical conversion API without having to construct a
    /// [`DpiScale`] themselves. If the stored scale factor is invalid
    /// (non-finite or non-positive), falls back to a default 1.0 scale
    /// rather than panicking.
    #[must_use]
    pub fn dpi(&self) -> DpiScale {
        if DpiScale::is_valid(self.dpi_scale) {
            DpiScale::new(self.dpi_scale)
        } else {
            DpiScale::new(1.0)
        }
    }
}

/// Owns and dispatches events for every open window in the application.
///
/// Storage is backed by a [`SlotMap`] keyed by [`WindowKey`], giving stable
/// O(1) lookup, insertion, and removal even as windows are created and
/// destroyed over the lifetime of the process.
///
/// # Examples
///
/// Creating windows requires a running winit event loop, but the manager's
/// storage logic can be inspected headlessly:
///
/// ```
/// use martensite_window::WindowManager;
///
/// let mgr = WindowManager::new();
/// assert!(mgr.is_empty());
/// assert_eq!(mgr.len(), 0);
/// assert_eq!(mgr.window_count(), 0);
/// ```
///
/// Driving a real event loop is shown in the crate-level example, which uses
/// [`WindowManager::create_window`] inside an [`ApplicationHandler`].
///
/// [`ApplicationHandler`]: winit::application::ApplicationHandler
#[derive(Debug)]
pub struct WindowManager {
    /// Slotmap holding one [`WindowEntry`] per open window.
    windows: SlotMap<WindowKey, WindowEntry>,
    /// Queue for asynchronous shell events (appearance changes,
    /// fractional scale changes) emitted by platform backends.
    /// Drained via [`drain_shell_events`](Self::drain_shell_events).
    shell_events: ShellEventQueue,
}

impl WindowManager {
    /// Creates a new, empty [`WindowManager`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            windows: SlotMap::with_key(),
            shell_events: ShellEventQueue::new(),
        }
    }

    /// Returns a clone of the shell event queue.
    ///
    /// Platform backends (macOS `AppearanceObserver`, Wayland
    /// `FractionalScaleTracker`) hold a clone of this queue and push
    /// [`ShellEvent`]s onto it when system-level changes occur. The
    /// window manager drains the queue via
    /// [`drain_shell_events`](Self::drain_shell_events) and translates
    /// the events into [`WindowEventOutcome`]s.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::WindowManager;
    /// use martensite_shell::{ShellEvent, ShellEventQueue};
    ///
    /// let manager = WindowManager::new();
    /// let queue = manager.shell_event_queue();
    /// queue.push(ShellEvent::ThemeAppearanceChanged);
    /// assert!(manager.drain_shell_events().len() == 1);
    /// ```
    #[must_use]
    pub fn shell_event_queue(&self) -> ShellEventQueue {
        self.shell_events.clone()
    }

    /// Drains all pending shell events and translates them to
    /// [`WindowEventOutcome`]s.
    ///
    /// This method should be called regularly (e.g. in the
    /// `about_to_wait` phase of the event loop) to poll for
    /// asynchronous shell events that arrive outside the normal
    /// winit event stream — specifically:
    ///
    /// - macOS `NSAppearance` changes → [`WindowEventOutcome::ThemeAppearanceChanged`]
    /// - Wayland `wp_fractional_scale_v1` changes → [`WindowEventOutcome::FractionalScaleChanged`]
    ///
    /// After draining, the queue is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::WindowManager;
    /// use martensite_shell::ShellEvent;
    ///
    /// let manager = WindowManager::new();
    /// // No events pending.
    /// assert!(manager.drain_shell_events().is_empty());
    /// ```
    #[must_use]
    pub fn drain_shell_events(&self) -> Vec<WindowEventOutcome> {
        self.shell_events
            .drain()
            .into_iter()
            .map(|event| match event {
                ShellEvent::ThemeAppearanceChanged => WindowEventOutcome::ThemeAppearanceChanged,
                ShellEvent::FractionalScaleChanged(scale) => {
                    WindowEventOutcome::FractionalScaleChanged(scale)
                }
                _ => WindowEventOutcome::None,
            })
            .collect()
    }

    /// Creates and registers a new window.
    ///
    /// `event_loop` is the active winit event loop that owns window
    /// creation; `attributes` configure the window's size, title, and other
    /// platform properties. On success the window's current scale factor is
    /// queried from the platform, validated via [`DpiScale::is_valid`], and
    /// stored alongside it. If the platform returns an invalid scale factor
    /// (non-finite or non-positive), a default of `1.0` is used instead.
    ///
    /// # Errors
    ///
    /// Propagates the [`winit`] [`RequestError`] if the platform refuses to
    /// create the window (denied permission, out of memory, incompatible
    /// system, etc.).
    ///
    /// [`RequestError`]: winit::error::RequestError
    pub fn create_window(
        &mut self,
        event_loop: &dyn winit::event_loop::ActiveEventLoop,
        attributes: winit::window::WindowAttributes,
    ) -> Result<WindowKey, winit::error::RequestError> {
        let window = event_loop.create_window(attributes)?;
        let id = window.id();
        let raw_scale = window.scale_factor();
        // Validate the platform-provided scale factor. If it is invalid
        // (non-finite or non-positive), fall back to 1.0 rather than
        // storing a value that would panic DpiScale::new later.
        let dpi_scale = if DpiScale::is_valid(raw_scale) {
            raw_scale
        } else {
            1.0
        };
        let key = self.windows.insert(WindowEntry {
            window,
            dpi_scale,
            id,
        });
        Ok(key)
    }

    /// Removes a window from management and returns its [`WindowEntry`].
    ///
    /// Returns `None` if `key` does not refer to a currently-tracked window
    /// (for example, if it was already destroyed via a
    /// `WindowEvent::Destroyed` event).
    ///
    /// `WindowEvent::Destroyed`: winit::event::WindowEvent::Destroyed
    pub fn destroy_window(&mut self, key: WindowKey) -> Option<WindowEntry> {
        self.windows.remove(key)
    }

    /// Returns a shared reference to the [`WindowEntry`] for `key`, or
    /// `None` if the key is stale.
    #[inline]
    #[must_use]
    pub fn window(&self, key: WindowKey) -> Option<&WindowEntry> {
        self.windows.get(key)
    }

    /// Backwards-compatible alias for [`window`](Self::window).
    #[inline]
    #[must_use]
    pub fn get_window(&self, key: WindowKey) -> Option<&WindowEntry> {
        self.window(key)
    }

    /// Returns a mutable reference to the [`WindowEntry`] for `key`, or
    /// `None` if the key is stale.
    #[inline]
    #[must_use]
    pub fn window_mut(&mut self, key: WindowKey) -> Option<&mut WindowEntry> {
        self.windows.get_mut(key)
    }

    /// Backwards-compatible alias for [`window_mut`](Self::window_mut).
    #[inline]
    #[must_use]
    pub fn get_window_mut(&mut self, key: WindowKey) -> Option<&mut WindowEntry> {
        self.window_mut(key)
    }

    /// Iterates over all tracked windows by reference.
    #[inline]
    pub fn windows(&self) -> impl Iterator<Item = (WindowKey, &WindowEntry)> {
        self.windows.iter()
    }

    /// Backwards-compatible alias for [`windows`](Self::windows).
    #[inline]
    pub fn iter_windows(&self) -> impl Iterator<Item = (WindowKey, &WindowEntry)> {
        self.windows()
    }

    /// Iterates over all tracked windows by mutable reference.
    #[inline]
    pub fn windows_mut(&mut self) -> impl Iterator<Item = (WindowKey, &mut WindowEntry)> {
        self.windows.iter_mut()
    }

    /// Backwards-compatible alias for [`windows_mut`](Self::windows_mut).
    #[inline]
    pub fn iter_windows_mut(&mut self) -> impl Iterator<Item = (WindowKey, &mut WindowEntry)> {
        self.windows_mut()
    }

    /// Returns the number of windows currently tracked.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::WindowManager;
    ///
    /// let mgr = WindowManager::new();
    /// assert_eq!(mgr.len(), 0);
    /// ```
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.windows.len()
    }

    /// Backwards-compatible alias for [`len`](Self::len).
    #[inline]
    #[must_use]
    pub fn window_count(&self) -> usize {
        self.len()
    }

    /// Returns `true` if no windows are currently tracked.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::WindowManager;
    ///
    /// let mgr = WindowManager::new();
    /// assert!(mgr.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    /// Finds the [`WindowKey`] whose window matches the given [`WindowId`].
    ///
    /// This is the bridge between winit events (which carry a [`WindowId`])
    /// and the slotmap-keyed storage used internally. Returns `None` if no
    /// tracked window has that id.
    #[must_use]
    pub fn key_for_id(&self, id: WindowId) -> Option<WindowKey> {
        self.windows
            .iter()
            .find(|(_, entry)| entry.id == id)
            .map(|(key, _)| key)
    }

    /// Routes a `WindowEvent` for the window identified by `id`.
    ///
    /// The manager updates its internal per-window state as appropriate
    /// (e.g. refreshing the DPI scale factor) and returns a
    /// [`WindowEventOutcome`] describing what the caller should do next.
    ///
    /// If `id` does not correspond to a tracked window, [`WindowEventOutcome::None`]
    /// is returned — this can happen transiently for events delivered after
    /// a window has been destroyed.
    pub fn handle_window_event(
        &mut self,
        id: WindowId,
        event: &winit::event::WindowEvent,
    ) -> WindowEventOutcome {
        use winit::event::WindowEvent;

        let Some(key) = self.key_for_id(id) else {
            return WindowEventOutcome::None;
        };

        match event {
            WindowEvent::CloseRequested => WindowEventOutcome::CloseRequested,
            WindowEvent::Destroyed => {
                // The platform window is gone; drop our entry so the key
                // becomes stale and resources can be reclaimed.
                self.destroy_window(key);
                WindowEventOutcome::Destroyed
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(entry) = self.window_mut(key) {
                    // Validate through DpiScale::is_valid so non-finite or
                    // non-positive scale factors cannot be stored and
                    // later panic when WindowEntry::dpi() is called.
                    // If the platform delivers an invalid scale factor,
                    // keep the previous value and report it back.
                    if DpiScale::is_valid(*scale_factor) {
                        entry.dpi_scale = *scale_factor;
                    }
                }
                // Return the validated/stored scale factor, not the raw
                // platform value, so callers never receive an invalid
                // factor that would panic DpiScale::new.
                let stored = self
                    .window(key)
                    .map(|e| e.dpi_scale)
                    .unwrap_or(*scale_factor);
                WindowEventOutcome::ScaleFactorChanged(stored)
            }
            WindowEvent::RedrawRequested => WindowEventOutcome::RedrawRequested,
            WindowEvent::Occluded(occluded) => WindowEventOutcome::Occluded(*occluded),
            _ => WindowEventOutcome::None,
        }
    }
}

impl Default for WindowManager {
    /// Returns an empty [`WindowManager`], equivalent to [`WindowManager::new`].
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{WindowEventOutcome, WindowManager};
    use crate::dpi::DpiScale;

    // NOTE: `WindowManager::create_window` requires a running winit event
    // loop, which cannot be spawned inside a normal unit test. The tests
    // below therefore exercise the slotmap storage logic directly by
    // inserting synthetic `WindowEntry` values through a test-only helper.
    //
    // Integration tests that drive a real event loop live in the
    // `#[ignore]`-gated tests at the bottom of this module.

    // `WindowId` is opaque and `Box<dyn Window>` cannot be constructed
    // without a running event loop, and both would require `unsafe` to
    // fabricate. To keep the storage tests `#![forbid(unsafe_code)]`-clean we
    // instead verify the slotmap mechanics through a parallel helper that
    // does not require real `WindowId`/`Window` values: see the
    // `slotmap_mechanics` tests below which use a stand-in entry type.
    //
    // The tests that *do* need real `WindowId`s are gated behind `#[ignore]`
    // and run against a live event loop.

    /// Stand-in entry mirroring the slotmap mechanics of `WindowEntry`
    /// without requiring a real `Window` or `WindowId`.
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct FakeEntry {
        id: u64,
        dpi_scale: f64,
    }

    /// A minimal slotmap wrapper used to validate the storage logic that
    /// `WindowManager` relies on, without needing winit types.
    #[derive(Debug, Default)]
    struct FakeManager {
        windows: slotmap::SlotMap<super::WindowKey, FakeEntry>,
    }

    impl FakeManager {
        fn new() -> Self {
            Self {
                windows: slotmap::SlotMap::with_key(),
            }
        }

        fn insert(&mut self, id: u64, dpi_scale: f64) -> super::WindowKey {
            self.windows.insert(FakeEntry { id, dpi_scale })
        }

        fn remove(&mut self, key: super::WindowKey) -> Option<FakeEntry> {
            self.windows.remove(key)
        }

        fn get(&self, key: super::WindowKey) -> Option<&FakeEntry> {
            self.windows.get(key)
        }

        fn get_mut(&mut self, key: super::WindowKey) -> Option<&mut FakeEntry> {
            self.windows.get_mut(key)
        }

        fn len(&self) -> usize {
            self.windows.len()
        }

        fn is_empty(&self) -> bool {
            self.windows.is_empty()
        }

        fn iter(&self) -> impl Iterator<Item = (super::WindowKey, &FakeEntry)> {
            self.windows.iter()
        }

        fn key_for_id(&self, id: u64) -> Option<super::WindowKey> {
            self.windows
                .iter()
                .find(|(_, entry)| entry.id == id)
                .map(|(key, _)| key)
        }
    }

    #[test]
    fn new_manager_is_empty() {
        let mgr = WindowManager::new();
        assert!(mgr.is_empty());
        assert_eq!(mgr.window_count(), 0);
    }

    #[test]
    fn default_equals_new() {
        let a = WindowManager::new();
        let b = WindowManager::default();
        assert_eq!(a.window_count(), b.window_count());
        assert!(a.is_empty() && b.is_empty());
    }

    #[test]
    fn slotmap_mechanics_insert_and_get() {
        let mut mgr = FakeManager::new();
        assert!(mgr.is_empty());

        let k0 = mgr.insert(10, 1.0);
        let k1 = mgr.insert(20, 2.0);
        assert_ne!(k0, k1);
        assert_eq!(mgr.len(), 2);
        assert!(!mgr.is_empty());

        assert_eq!(mgr.get(k0).map(|e| e.id), Some(10));
        assert_eq!(mgr.get(k1).map(|e| e.id), Some(20));
    }

    #[test]
    fn slotmap_mechanics_remove_returns_entry() {
        let mut mgr = FakeManager::new();
        let k = mgr.insert(42, 1.5);
        assert_eq!(mgr.len(), 1);

        let removed = mgr.remove(k);
        assert_eq!(removed.map(|e| (e.id, e.dpi_scale)), Some((42, 1.5)));
        assert_eq!(mgr.len(), 0);
        assert!(mgr.get(k).is_none());
    }

    #[test]
    fn slotmap_mechanics_remove_stale_key_returns_none() {
        let mut mgr = FakeManager::new();
        let k = mgr.insert(1, 1.0);
        let _ = mgr.remove(k);
        assert!(mgr.remove(k).is_none());
    }

    #[test]
    fn slotmap_mechanics_get_mut_updates_dpi() {
        let mut mgr = FakeManager::new();
        let k = mgr.insert(7, 1.0);
        {
            let entry = mgr.get_mut(k).expect("just-inserted key must resolve");
            entry.dpi_scale = 1.5;
        }
        assert_eq!(mgr.get(k).map(|e| e.dpi_scale), Some(1.5));
    }

    #[test]
    fn slotmap_mechanics_key_for_id_resolves() {
        let mut mgr = FakeManager::new();
        let k0 = mgr.insert(100, 1.0);
        let k1 = mgr.insert(200, 2.0);

        assert_eq!(mgr.key_for_id(100), Some(k0));
        assert_eq!(mgr.key_for_id(200), Some(k1));
        assert_eq!(mgr.key_for_id(999), None);
    }

    #[test]
    fn slotmap_mechanics_iter_visits_all() {
        let mut mgr = FakeManager::new();
        let k0 = mgr.insert(1, 1.0);
        let k1 = mgr.insert(2, 2.0);
        let k2 = mgr.insert(3, 3.0);

        let mut seen: Vec<(super::WindowKey, u64)> = mgr.iter().map(|(k, e)| (k, e.id)).collect();
        seen.sort_by_key(|(_, id)| *id);

        assert_eq!(seen, vec![(k0, 1), (k1, 2), (k2, 3)],);
    }

    #[test]
    fn slotmap_mechanics_remove_makes_key_stale_but_others_survive() {
        let mut mgr = FakeManager::new();
        let k0 = mgr.insert(1, 1.0);
        let k1 = mgr.insert(2, 2.0);

        assert!(mgr.remove(k0).is_some());
        assert!(mgr.get(k0).is_none(), "removed key should be stale");
        assert!(
            mgr.get(k1).is_some(),
            "unrelated key must remain valid after a removal",
        );
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn window_entry_dpi_returns_scale_view() {
        // We cannot build a real `WindowEntry` (no event loop), but the
        // `dpi()` helper is pure arithmetic over `dpi_scale`, so verify it
        // through the `DpiScale` type directly to lock the contract.
        let scale = DpiScale::new(2.0);
        assert_eq!(scale.to_physical(100.0), 200.0);
        assert_eq!(scale.to_logical(200.0), 100.0);
    }

    #[test]
    fn window_event_outcome_variants_are_distinct() {
        assert_ne!(WindowEventOutcome::None, WindowEventOutcome::CloseRequested);
        assert_ne!(
            WindowEventOutcome::CloseRequested,
            WindowEventOutcome::Destroyed,
        );
        assert_ne!(
            WindowEventOutcome::ScaleFactorChanged(1.0),
            WindowEventOutcome::ScaleFactorChanged(2.0),
        );
        assert_eq!(
            WindowEventOutcome::RedrawRequested,
            WindowEventOutcome::RedrawRequested,
        );
    }

    #[test]
    fn window_event_outcome_occluded_distinct() {
        assert_ne!(
            WindowEventOutcome::Occluded(true),
            WindowEventOutcome::Occluded(false),
        );
        assert_ne!(WindowEventOutcome::Occluded(true), WindowEventOutcome::None);
        assert_ne!(
            WindowEventOutcome::Occluded(false),
            WindowEventOutcome::RedrawRequested,
        );
        assert_eq!(
            WindowEventOutcome::Occluded(true),
            WindowEventOutcome::Occluded(true),
        );
    }

    // ---------------------------------------------------------------------
    // Event-loop-gated tests.
    //
    // These tests require a running winit `EventLoop` to construct real
    // `Window`/`WindowId` values. They are `#[ignore]` by default because
    // they cannot run in headless CI and may block on platform windowing
    // systems. Run them explicitly with:
    //
    //     cargo test -p martensite-window -- --ignored
    //
    /// Verifies that a real window can be created, tracked, and destroyed
    /// through `WindowManager` on a live event loop.
    #[test]
    #[ignore = "requires a running winit event loop and a windowing system"]
    fn create_and_destroy_real_window() {
        use winit::application::ApplicationHandler;
        use winit::event_loop::{ActiveEventLoop, EventLoop};
        use winit::window::WindowAttributes;

        struct App {
            mgr: WindowManager,
            done: bool,
        }

        impl ApplicationHandler for App {
            fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
                let attrs = WindowAttributes::default().with_title("martensite-window test");
                let key = self
                    .mgr
                    .create_window(event_loop, attrs)
                    .expect("window creation should succeed on a live event loop");
                assert_eq!(self.mgr.len(), 1);
                assert_eq!(self.mgr.window_count(), 1);
                assert!(self.mgr.window(key).is_some());
                assert!(self.mgr.get_window(key).is_some());
                assert!(self.mgr.destroy_window(key).is_some());
                assert_eq!(self.mgr.len(), 0);
                assert_eq!(self.mgr.window_count(), 0);
                self.done = true;
                event_loop.exit();
            }

            fn window_event(
                &mut self,
                _event_loop: &dyn ActiveEventLoop,
                _id: winit::window::WindowId,
                _event: winit::event::WindowEvent,
            ) {
            }

            fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
                if self.done {
                    event_loop.exit();
                }
            }
        }

        // winit's Linux backend panics inside `EventLoop::new()` when no
        // display server is available (headless CI), so `catch_unwind` is
        // required — the `Result` is never returned.
        let event_loop = match std::panic::catch_unwind(EventLoop::new) {
            Ok(Ok(el)) => el,
            _ => {
                eprintln!("skipping: no display server available for winit EventLoop");
                return;
            }
        };
        let app = App {
            mgr: WindowManager::new(),
            done: false,
        };
        event_loop
            .run_app(app)
            .expect("event loop should run cleanly");
    }
}
