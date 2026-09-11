//! Platform-agnostic shell events and event queue.
//!
//! Platform backends (macOS `AppearanceObserver`, Wayland
//! `FractionalScaleTracker`) emit [`ShellEvent`]s into a
//! [`ShellEventQueue`]. The window manager drains the queue and
//! translates the events into `WindowEventOutcome`
//! variants so the application can react to system-level shell
//! changes.
//!
//! The queue is a thin `Arc<Mutex<Vec<ShellEvent>>>` wrapper — no
//! unsafe code, no platform dependencies, available on every target.

use std::sync::{Arc, Mutex};

/// An event emitted by a platform shell backend.
///
/// These events are asynchronous — they arrive outside the normal
/// winit event loop — and must be polled via
/// [`ShellEventQueue::drain`].
///
/// # Examples
///
/// ```
/// use martensite_shell::ShellEvent;
///
/// let appearance = ShellEvent::ThemeAppearanceChanged;
/// let scale = ShellEvent::FractionalScaleChanged(1.5);
/// assert_ne!(appearance, scale);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ShellEvent {
    /// The system theme appearance changed (macOS `NSAppearance`
    /// notification). The application should query the new appearance
    /// and trigger a `ThemeDiff`
    /// transition.
    ThemeAppearanceChanged,
    /// The Wayland fractional scale factor for a surface changed
    /// (e.g. the window was dragged to a monitor with a different
    /// DPR). The value is the new fractional scale factor (e.g.
    /// `1.5` for 150% DPI).
    FractionalScaleChanged(f64),
}

/// A thread-safe FIFO queue of [`ShellEvent`]s shared between
/// platform backends (producers) and the window manager (consumer).
///
/// The queue is cloneable — each clone shares the same underlying
/// `Arc<Mutex<Vec>>`. Platform backends hold a clone and call
/// [`push`](Self::push); the window manager holds the original
/// and calls [`drain`](Self::drain).
///
/// # Examples
///
/// ```
/// use martensite_shell::{ShellEvent, ShellEventQueue};
///
/// let queue = ShellEventQueue::new();
/// let producer = queue.clone();
///
/// producer.push(ShellEvent::ThemeAppearanceChanged);
/// producer.push(ShellEvent::FractionalScaleChanged(2.0));
///
/// let events = queue.drain();
/// assert_eq!(events.len(), 2);
/// assert_eq!(events[0], ShellEvent::ThemeAppearanceChanged);
/// assert_eq!(events[1], ShellEvent::FractionalScaleChanged(2.0));
///
/// // After draining, the queue is empty.
/// assert!(queue.drain().is_empty());
/// ```
#[derive(Debug, Clone, Default)]
pub struct ShellEventQueue {
    events: Arc<Mutex<Vec<ShellEvent>>>,
}

impl PartialEq for ShellEventQueue {
    /// Two queues are equal if they share the same underlying buffer
    /// (pointer identity via [`Arc::ptr_eq`]).
    ///
    /// This is the correct semantics for a handle-like type: two clones
    /// of the same queue are equal, but two independently-created queues
    /// are not, even if both are empty.
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.events, &other.events)
    }
}

impl Eq for ShellEventQueue {}

impl ShellEventQueue {
    /// Creates a new empty event queue.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::ShellEventQueue;
    ///
    /// let queue = ShellEventQueue::new();
    /// assert!(queue.drain().is_empty());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Pushes a shell event onto the queue.
    ///
    /// Called by platform backends when a system-level change is
    /// detected (e.g. macOS appearance change, Wayland fractional
    /// scale change).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::{ShellEvent, ShellEventQueue};
    ///
    /// let queue = ShellEventQueue::new();
    /// queue.push(ShellEvent::ThemeAppearanceChanged);
    /// assert_eq!(queue.drain().len(), 1);
    /// ```
    pub fn push(&self, event: ShellEvent) {
        let mut guard = self.events.lock().expect(
            "ShellEventQueue mutex poisoned — a producer thread panicked while holding the lock",
        );
        guard.push(event);
    }

    /// Drains and returns all pending shell events in FIFO order.
    ///
    /// Called by the window manager to poll for asynchronous shell
    /// events. After draining, the queue is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::{ShellEvent, ShellEventQueue};
    ///
    /// let queue = ShellEventQueue::new();
    /// queue.push(ShellEvent::FractionalScaleChanged(1.25));
    /// let events = queue.drain();
    /// assert_eq!(events, vec![ShellEvent::FractionalScaleChanged(1.25)]);
    /// assert!(queue.drain().is_empty());
    /// ```
    #[must_use]
    pub fn drain(&self) -> Vec<ShellEvent> {
        let mut guard = self.events.lock().expect(
            "ShellEventQueue mutex poisoned — a consumer thread panicked while holding the lock",
        );
        std::mem::take(&mut *guard)
    }

    /// Returns `true` if the queue currently holds any pending events.
    ///
    /// This is a cheap check that does not drain the queue.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_shell::{ShellEvent, ShellEventQueue};
    ///
    /// let queue = ShellEventQueue::new();
    /// assert!(!queue.has_events());
    /// queue.push(ShellEvent::ThemeAppearanceChanged);
    /// assert!(queue.has_events());
    /// ```
    #[must_use]
    pub fn has_events(&self) -> bool {
        let guard = self.events.lock().expect("ShellEventQueue mutex poisoned");
        !guard.is_empty()
    }
}
