//! Platform backend abstraction.
//!
//! This module defines the [`PlatformNotifier`] trait, which extends
//! [`NotifyService`] with a [`PlatformNotifier::platform_name`] accessor,
//! together with a [`StubNotifier`] no-op implementation and a
//! [`default_platform_notifier`] factory that selects the best available
//! backend for the current target.
//!
//! # Platform mechanisms
//!
//! Native notifications are driven by `martensite-notify-platform` when
//! the `platform` Cargo feature is enabled. That crate shells out to the
//! platform's own notification facility — no FFI is required:
//!
//! * **Windows** — PowerShell `Windows.UI.Notifications` toast.
//! * **macOS** — `osascript` `display notification`.
//! * **Linux** — `notify-send` (freedesktop notifications).
//!
//! Without the `platform` feature every backend is a **safe stub** that
//! silently discards notifications; this crate is
//! `#![forbid(unsafe_code)]` and stays a safe, auditable dependency either
//! way.
//!
//! # Examples
//!
//! ```
//! use martensite_notify::{PlatformNotifier, default_platform_notifier};
//!
//! let n = default_platform_notifier();
//! assert!(!n.platform_name().is_empty());
//! ```

use crate::notify::{Notification, NotifyError, NotifyService};

/// A [`NotifyService`] backed by a specific platform notification
/// facility.
///
/// Implementations identify themselves via
/// [`platform_name`](PlatformNotifier::platform_name) so callers and
/// diagnostics can report which backend is active.
///
/// # Examples
///
/// ```
/// use martensite_notify::{Notification, NotifyService, PlatformNotifier,
///     StubNotifier};
///
/// let mut n = StubNotifier::new();
/// assert_eq!(n.platform_name(), "stub");
/// assert!(n.notify(&Notification::new("T")).is_ok());
/// ```
pub trait PlatformNotifier: NotifyService {
    /// Returns a human-readable backend name, e.g. `"windows-toast"`,
    /// `"macos-osascript"`, `"linux-notify-send"`, or `"stub"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_notify::{PlatformNotifier, StubNotifier};
    ///
    /// assert_eq!(StubNotifier::new().platform_name(), "stub");
    /// ```
    fn platform_name(&self) -> &str;
}

/// A no-op notifier for environments without OS notification support.
///
/// Notifications are silently discarded (`Ok(())`). This is the fallback
/// used by [`default_platform_notifier`] when no backend is compiled in,
/// and is useful for headless tests and CI.
///
/// # Examples
///
/// ```
/// use martensite_notify::{Notification, NotifyService, PlatformNotifier,
///     StubNotifier};
///
/// let mut n = StubNotifier::new();
/// assert!(n.notify(&Notification::new("T")).is_ok());
/// ```
#[derive(Default, Clone, Debug)]
pub struct StubNotifier;

impl StubNotifier {
    /// Creates a new [`StubNotifier`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_notify::{PlatformNotifier, StubNotifier};
    ///
    /// assert_eq!(StubNotifier::new().platform_name(), "stub");
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

impl NotifyService for StubNotifier {
    fn notify(&mut self, _notification: &Notification) -> Result<(), NotifyError> {
        Ok(())
    }
}

impl PlatformNotifier for StubNotifier {
    #[inline]
    fn platform_name(&self) -> &str {
        "stub"
    }
}

/// Returns the best available [`PlatformNotifier`] for the current target.
///
/// The selection rules are:
///
/// | Target | Backend |
/// |--------|---------|
/// | `windows` | `windows-toast` (PowerShell `Windows.UI.Notifications`) |
/// | `macos` | `macos-osascript` (`display notification`) |
/// | `linux` | `linux-notify-send` |
/// | other | [`StubNotifier`] |
///
/// Without the `platform` Cargo feature this returns [`StubNotifier`];
/// with it, the function delegates to
/// `martensite_notify_platform::native_backend` and only falls back to
/// [`StubNotifier`] when no usable backend exists (e.g. no `notify-send`
/// on `PATH`). [`ScriptedNotifier`] remains the choice for tests. The
/// returned notifier is always usable and never panics.
///
/// [`ScriptedNotifier`]: crate::ScriptedNotifier
///
/// # Examples
///
/// ```
/// use martensite_notify::{PlatformNotifier, default_platform_notifier};
///
/// let n = default_platform_notifier();
/// assert!(!n.platform_name().is_empty());
/// ```
pub fn default_platform_notifier() -> Box<dyn PlatformNotifier> {
    cfg_default_platform_notifier()
}

#[cfg(feature = "platform")]
fn cfg_default_platform_notifier() -> Box<dyn PlatformNotifier> {
    if let Some(backend) = martensite_notify_platform::native_backend() {
        return Box::new(PlatformBackendAdapter(backend));
    }
    Box::new(StubNotifier::new())
}

#[cfg(not(feature = "platform"))]
fn cfg_default_platform_notifier() -> Box<dyn PlatformNotifier> {
    Box::new(StubNotifier::new())
}

/// Adapter wrapping a `martensite_notify_platform::NotifyBackend` as a
/// [`PlatformNotifier`].
///
/// The backend trait lives in the platform crate to avoid a cyclic
/// dependency; this adapter maps the safe crate's [`Notification`] onto
/// the platform crate's `NotifySpec` wire type.
#[cfg(feature = "platform")]
struct PlatformBackendAdapter(Box<dyn martensite_notify_platform::NotifyBackend>);

#[cfg(feature = "platform")]
impl NotifyService for PlatformBackendAdapter {
    fn notify(&mut self, notification: &Notification) -> Result<(), NotifyError> {
        use martensite_notify_platform::{NotifySpec, SpecUrgency};
        let urgency = match notification.urgency {
            crate::Urgency::Low => SpecUrgency::Low,
            crate::Urgency::Normal => SpecUrgency::Normal,
            crate::Urgency::Critical => SpecUrgency::Critical,
        };
        let spec = NotifySpec {
            title: notification.title.clone(),
            body: notification.body.clone(),
            subtitle: notification.subtitle.clone(),
            urgency,
            sound: notification.sound.clone(),
        };
        self.0
            .notify(&spec)
            .map_err(|e| NotifyError::Failed(e.to_string()))
    }
}

#[cfg(feature = "platform")]
impl PlatformNotifier for PlatformBackendAdapter {
    fn platform_name(&self) -> &str {
        self.0.platform_name()
    }
}
