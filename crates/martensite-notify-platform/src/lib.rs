//! OS-native notification backends for Martensite.
//!
//! This crate provides platform-specific notification delivery:
//!
//! - **macOS**: `osascript` `display notification` (Notification Center)
//! - **Windows**: PowerShell `Windows.UI.Notifications` toast
//! - **Linux**: `notify-send` (freedesktop notification daemon)
//!
//! # Architecture
//!
//! This crate intentionally does **not** depend on `martensite-notify` to
//! avoid a cyclic dependency. It defines its own [`NotifyBackend`] trait,
//! wire types ([`NotifySpec`], [`SpecUrgency`]), and a
//! [`native_backend`] factory. The `martensite-notify` crate wraps this
//! crate's API behind its own `PlatformNotifier` trait when the
//! `platform` feature is enabled.
//!
//! # Safety policy
//!
//! This crate carries `#![forbid(unsafe_code)]`: every backend drives the
//! platform's notification facility through a short-lived subprocess, so
//! no FFI or unsafe code is required — mirroring the `wl-clipboard`
//! subprocess backend in `martensite-clipboard-platform`.
//!
//! # Examples
//!
//! ```
//! use martensite_notify_platform::native_backend;
//!
//! // `Some` on desktop targets with a usable notification tool;
//! // `None` elsewhere (e.g. a bare Linux CI box without notify-send).
//! let _backend = native_backend();
//! ```
#![forbid(unsafe_code)]
#![forbid(missing_docs)]

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// Presentation urgency for a [`NotifySpec`].
///
/// Mirrors `martensite_notify::Urgency`; duplicated here to keep this
/// crate dependency-free (see the crate-level docs).
///
/// # Examples
///
/// ```
/// use martensite_notify_platform::SpecUrgency;
///
/// assert_eq!(SpecUrgency::default(), SpecUrgency::Normal);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpecUrgency {
    /// Passive, auto-dismissed, no sound.
    Low,
    /// Standard notification.
    #[default]
    Normal,
    /// Important — persists and may play a sound.
    Critical,
}

/// A platform-agnostic notification request (wire type).
///
/// Mirrors `martensite_notify::Notification`.
///
/// # Examples
///
/// ```
/// use martensite_notify_platform::NotifySpec;
///
/// let s = NotifySpec { title: "T".into(), ..Default::default() };
/// assert_eq!(s.title, "T");
/// ```
#[derive(Clone, Debug, Default)]
pub struct NotifySpec {
    /// Headline.
    pub title: String,
    /// Secondary text.
    pub body: String,
    /// Tertiary caption (macOS only).
    pub subtitle: String,
    /// Presentation priority.
    pub urgency: SpecUrgency,
    /// Notification sound name.
    pub sound: Option<String>,
}

/// A native notification backend.
///
/// Delivery is one-shot and best-effort; there is no delivery receipt or
/// click-action callback.
///
/// # Examples
///
/// ```
/// use martensite_notify_platform::{NotifyBackend, NotifySpec};
///
/// struct Null;
/// impl NotifyBackend for Null {
///     fn notify(&mut self, _s: &NotifySpec) -> Result<(), String> { Ok(()) }
///     fn platform_name(&self) -> &str { "null" }
/// }
/// assert_eq!(Null.platform_name(), "null");
/// ```
pub trait NotifyBackend {
    /// Deliver `spec` to the OS notification facility.
    fn notify(&mut self, spec: &NotifySpec) -> Result<(), String>;
    /// Human-readable backend name, e.g. `"macos-osascript"`.
    fn platform_name(&self) -> &str;
}

/// Returns the best available [`NotifyBackend`] for the current target.
///
/// | Target | Backend | Availability gate |
/// |--------|---------|-------------------|
/// | `windows` | `windows-toast` | always (PowerShell is in-box) |
/// | `macos` | `macos-osascript` | always (osascript is in-box) |
/// | `linux` | `linux-notify-send` | binary on `PATH` |
/// | other | — | `None` |
///
/// # Examples
///
/// ```
/// use martensite_notify_platform::native_backend;
///
/// if let Some(b) = native_backend() {
///     assert!(!b.platform_name().is_empty());
/// }
/// ```
pub fn native_backend() -> Option<Box<dyn NotifyBackend>> {
    #[cfg(target_os = "macos")]
    {
        Some(Box::new(macos::OsascriptNotifier::new()))
    }
    #[cfg(target_os = "windows")]
    {
        Some(Box::new(windows::ToastNotifier::new()))
    }
    #[cfg(target_os = "linux")]
    {
        linux::select_backend()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        None
    }
}
