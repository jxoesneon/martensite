//! Platform backend abstraction.
//!
//! This module defines the [`PlatformClipboard`] trait, which extends
//! [`ClipboardService`] with a [`PlatformClipboard::platform_name`] accessor,
//! together with a [`StubClipboard`] no-op implementation and a
//! [`default_platform_clipboard`] factory that selects the best available
//! backend for the current target.
//!
//! # `#![forbid(unsafe_code)]` and platform FFI
//!
//! Real clipboard IPC on every desktop platform requires `unsafe` platform
//! FFI:
//!
//! * **Windows** — OLE clipboard access (`OpenClipboard`, `SetClipboardData`,
//!   `RegisterClipboardFormat`, …) is exposed through the Win32 API which is
//!   only callable via `unsafe` FFI bindings.
//! * **macOS** — `NSPasteboard` is an Objective-C class interacted with via
//!   the Objective-C runtime, which is inherently `unsafe`.
//! * **Wayland** — the `wl_data_device` / `wl_data_source` protocols are
//!   driven through a system compositor connection requiring `unsafe` FFI to
//!   `libwayland`.
//! * **X11** — the X11 selection mechanism (`XSetSelectionOwner`,
//!   `XConvertSelection`, …) is exposed through `libX11` FFI, also `unsafe`.
//!
//! This crate carries `#![forbid(unsafe_code)]`, so none of that FFI can
//! live here. The platform backends in this module are therefore **safe
//! stubs** that document the intended integration point and fall back to
//! [`StubClipboard`] behavior. The real, `unsafe` FFI bindings are deferred
//! to a future milestone and will live in a separate crate (or behind a
//! feature gate that opts out of `forbid(unsafe_code)`) so that this crate
//! remains a safe, auditable dependency.
//!
//! # Examples
//!
//! ```
//! use martensite_clipboard::{ClipboardService, PlatformClipboard,
//!     default_platform_clipboard};
//!
//! let mut cb = default_platform_clipboard();
//! // The factory always returns a usable (possibly stub) clipboard.
//! let _ = cb.platform_name();
//! cb.clear();
//! assert!(cb.available_types().is_empty());
//! ```

use crate::clipboard::{ClipboardItem, ClipboardService};

/// A [`ClipboardService`] that is backed by a specific platform clipboard.
///
/// Implementations identify themselves via [`platform_name`](PlatformClipboard::platform_name)
/// so that callers and diagnostics can report which backend is active.
pub trait PlatformClipboard: ClipboardService {
    /// Returns a human-readable name for the platform backend, e.g.
    /// `"windows-ole"`, `"macos-nspasteboard"`, `"wayland"`, `"x11"` or
    /// `"stub"`.
    fn platform_name(&self) -> &str;
}

/// A no-op platform clipboard for environments without OS clipboard support.
///
/// All read operations return empty results and all write operations are
/// silently discarded. This is the fallback used by
/// [`default_platform_clipboard`] when no platform-specific backend is
/// compiled in, and is useful for headless tests and CI.
///
/// # Examples
///
/// ```
/// use martensite_clipboard::{ClipboardItem, ClipboardService, PlatformClipboard,
///     StubClipboard};
///
/// let mut cb = StubClipboard::new();
/// assert_eq!(cb.platform_name(), "stub");
/// cb.set_contents(&ClipboardItem::new().offer_text("ignored"));
/// assert!(cb.available_types().is_empty());
/// assert!(cb.get_contents("text/plain;charset=utf-8").is_none());
/// ```
#[derive(Default, Clone, Debug)]
pub struct StubClipboard;

impl StubClipboard {
    /// Creates a new [`StubClipboard`].
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

impl ClipboardService for StubClipboard {
    fn set_contents(&mut self, _item: &ClipboardItem) {
        // Intentionally no-op: no platform clipboard is available.
    }

    fn get_contents(&self, _mime: &str) -> Option<Vec<u8>> {
        None
    }

    fn available_types(&self) -> Vec<String> {
        Vec::new()
    }

    fn clear(&mut self) {
        // Nothing to clear.
    }
}

impl PlatformClipboard for StubClipboard {
    #[inline]
    fn platform_name(&self) -> &str {
        "stub"
    }
}

// ---------------------------------------------------------------------------
// Platform-specific stubs.
//
// Each stub documents the real integration it would perform and forwards to
// `StubClipboard` for actual behavior. This keeps the crate `unsafe`-free
// while making the intended backend selection explicit and testable.
// ---------------------------------------------------------------------------

/// Windows OLE clipboard stub.
///
/// A real implementation would call `OpenClipboard` / `EmptyClipboard` /
/// `SetClipboardData` and register custom formats via
/// `RegisterClipboardFormat`, delayed-rendering via the
/// `WM_RENDERFORMAT` / `WM_RENDERALLFORMATS` messages. All of these require
/// `unsafe` Win32 FFI, which is forbidden by this crate's
/// `#![forbid(unsafe_code)]` attribute. This struct is therefore a **safe
/// stub** that forwards to [`StubClipboard`]: every read returns [`None`] and
/// every write is silently discarded.
///
/// Real platform integration will be provided by a separate FFI crate in a
/// future milestone, keeping this crate a safe, auditable dependency. For
/// Martensite v0.5.0 the usable implementations are [`crate::InMemoryClipboard`]
/// (for tests and headless environments) and [`StubClipboard`] (the
/// platform-agnostic fallback). This is a deliberate design decision, not a
/// missing feature — see the [module docs](crate::platform) for the full
/// rationale.
#[cfg(target_os = "windows")]
#[derive(Default, Clone, Debug)]
pub struct WindowsOleClipboard(StubClipboard);

#[cfg(target_os = "windows")]
impl WindowsOleClipboard {
    /// Creates a new [`WindowsOleClipboard`] stub.
    #[inline]
    pub fn new() -> Self {
        Self(StubClipboard)
    }
}

#[cfg(target_os = "windows")]
impl ClipboardService for WindowsOleClipboard {
    #[inline]
    fn set_contents(&mut self, item: &ClipboardItem) {
        self.0.set_contents(item);
    }
    #[inline]
    fn get_contents(&self, mime: &str) -> Option<Vec<u8>> {
        self.0.get_contents(mime)
    }
    #[inline]
    fn available_types(&self) -> Vec<String> {
        self.0.available_types()
    }
    #[inline]
    fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(target_os = "windows")]
impl PlatformClipboard for WindowsOleClipboard {
    #[inline]
    fn platform_name(&self) -> &str {
        "windows-ole"
    }
}

/// macOS `NSPasteboard` clipboard stub.
///
/// A real implementation would use `NSPasteboard`'s
/// `clearContents` / `setDataObjects:forTypes:` and
/// `dataForType:` APIs via the Objective-C runtime, which is inherently
/// `unsafe` and therefore forbidden by this crate's
/// `#![forbid(unsafe_code)]` attribute. This struct is therefore a **safe
/// stub** that forwards to [`StubClipboard`]: every read returns [`None`]
/// and every write is silently discarded.
///
/// Real platform integration will be provided by a separate FFI crate in a
/// future milestone, keeping this crate a safe, auditable dependency. For
/// Martensite v0.5.0 the usable implementations are [`crate::InMemoryClipboard`]
/// (for tests and headless environments) and [`StubClipboard`] (the
/// platform-agnostic fallback). This is a deliberate design decision, not a
/// missing feature — see the [module docs](crate::platform) for the full
/// rationale.
#[cfg(target_os = "macos")]
#[derive(Default, Clone, Debug)]
pub struct NsPasteboardClipboard(StubClipboard);

#[cfg(target_os = "macos")]
impl NsPasteboardClipboard {
    /// Creates a new [`NsPasteboardClipboard`] stub.
    #[inline]
    pub fn new() -> Self {
        Self(StubClipboard)
    }
}

#[cfg(target_os = "macos")]
impl ClipboardService for NsPasteboardClipboard {
    #[inline]
    fn set_contents(&mut self, item: &ClipboardItem) {
        self.0.set_contents(item);
    }
    #[inline]
    fn get_contents(&self, mime: &str) -> Option<Vec<u8>> {
        self.0.get_contents(mime)
    }
    #[inline]
    fn available_types(&self) -> Vec<String> {
        self.0.available_types()
    }
    #[inline]
    fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(target_os = "macos")]
impl PlatformClipboard for NsPasteboardClipboard {
    #[inline]
    fn platform_name(&self) -> &str {
        "macos-nspasteboard"
    }
}

/// Wayland `wl_data_device` / `wl_data_source` clipboard stub.
///
/// A real implementation would create a `wl_data_source`, offer the
/// requested MIME types, and handle `send` events by writing payload bytes
/// to the offered file descriptor, all via `libwayland` FFI. This requires
/// `unsafe` FFI, which is forbidden by this crate's
/// `#![forbid(unsafe_code)]` attribute. This struct is therefore a **safe
/// stub** that forwards to [`StubClipboard`]: every read returns [`None`]
/// and every write is silently discarded.
///
/// Real platform integration will be provided by a separate FFI crate in a
/// future milestone, keeping this crate a safe, auditable dependency. For
/// Martensite v0.5.0 the usable implementations are [`crate::InMemoryClipboard`]
/// (for tests and headless environments) and [`StubClipboard`] (the
/// platform-agnostic fallback). This is a deliberate design decision, not a
/// missing feature — see the [module docs](crate::platform) for the full
/// rationale.
///
/// Enabled only on Linux when the `wayland` feature is active.
#[cfg(all(target_os = "linux", feature = "wayland"))]
#[derive(Default, Clone, Debug)]
pub struct WaylandClipboard(StubClipboard);

#[cfg(all(target_os = "linux", feature = "wayland"))]
impl WaylandClipboard {
    /// Creates a new [`WaylandClipboard`] stub.
    #[inline]
    pub fn new() -> Self {
        Self(StubClipboard)
    }
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
impl ClipboardService for WaylandClipboard {
    #[inline]
    fn set_contents(&mut self, item: &ClipboardItem) {
        self.0.set_contents(item);
    }
    #[inline]
    fn get_contents(&self, mime: &str) -> Option<Vec<u8>> {
        self.0.get_contents(mime)
    }
    #[inline]
    fn available_types(&self) -> Vec<String> {
        self.0.available_types()
    }
    #[inline]
    fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
impl PlatformClipboard for WaylandClipboard {
    #[inline]
    fn platform_name(&self) -> &str {
        "wayland"
    }
}

/// X11 clipboard stub (PRIMARY/CLIPBOARD selections).
///
/// A real implementation would own the `CLIPBOARD` selection via
/// `XSetSelectionOwner`, serve `SelectionRequest` events by converting the
/// requested target (MIME type) to a property, and use
/// `XConvertSelection` for reads. All of these require `unsafe` `libX11`
/// FFI, which is forbidden by this crate's `#![forbid(unsafe_code)]`
/// attribute. This struct is therefore a **safe stub** that forwards to
/// [`StubClipboard`]: every read returns [`None`] and every write is
/// silently discarded.
///
/// Real platform integration will be provided by a separate FFI crate in a
/// future milestone, keeping this crate a safe, auditable dependency. For
/// Martensite v0.5.0 the usable implementations are [`crate::InMemoryClipboard`]
/// (for tests and headless environments) and [`StubClipboard`] (the
/// platform-agnostic fallback). This is a deliberate design decision, not a
/// missing feature — see the [module docs](crate::platform) for the full
/// rationale.
///
/// Selected on Linux when the `wayland` feature is **not** active.
#[cfg(all(target_os = "linux", not(feature = "wayland")))]
#[derive(Default, Clone, Debug)]
pub struct X11Clipboard(StubClipboard);

#[cfg(all(target_os = "linux", not(feature = "wayland")))]
impl X11Clipboard {
    /// Creates a new [`X11Clipboard`] stub.
    #[inline]
    pub fn new() -> Self {
        Self(StubClipboard)
    }
}

#[cfg(all(target_os = "linux", not(feature = "wayland")))]
impl ClipboardService for X11Clipboard {
    #[inline]
    fn set_contents(&mut self, item: &ClipboardItem) {
        self.0.set_contents(item);
    }
    #[inline]
    fn get_contents(&self, mime: &str) -> Option<Vec<u8>> {
        self.0.get_contents(mime)
    }
    #[inline]
    fn available_types(&self) -> Vec<String> {
        self.0.available_types()
    }
    #[inline]
    fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(all(target_os = "linux", not(feature = "wayland")))]
impl PlatformClipboard for X11Clipboard {
    #[inline]
    fn platform_name(&self) -> &str {
        "x11"
    }
}

/// Returns the best available [`PlatformClipboard`] for the current target.
///
/// The selection rules are:
///
/// | Target | Feature | Backend |
/// |--------|---------|---------|
/// | `windows` | — | `WindowsOleClipboard` |
/// | `macos` | — | `NsPasteboardClipboard` |
/// | `linux` | `wayland` | `WaylandClipboard` |
/// | `linux` | (no `wayland`) | `X11Clipboard` |
/// | other | — | [`StubClipboard`] |
///
/// Every backend is currently a **safe stub** (see the [module
/// docs](crate::platform)): real OS clipboard FFI requires `unsafe` code,
/// which is forbidden by this crate's `#![forbid(unsafe_code)]` attribute.
/// The stubs return empty/`None` for reads and silently discard writes.
/// Real platform integration will be provided by a separate FFI crate in a
/// future milestone. For Martensite v0.5.0 the usable implementations are
/// [`crate::InMemoryClipboard`] (for tests and headless environments) and
/// [`StubClipboard`] (the platform-agnostic fallback). This is a deliberate
/// design decision, not a missing feature — the returned clipboard is
/// always usable and never panics.
///
/// # Examples
///
/// ```
/// use martensite_clipboard::{PlatformClipboard, default_platform_clipboard};
///
/// let cb = default_platform_clipboard();
/// // The name is one of the documented platform identifiers or "stub".
/// assert!(!cb.platform_name().is_empty());
/// ```
pub fn default_platform_clipboard() -> Box<dyn PlatformClipboard> {
    cfg_default_platform_clipboard()
}

#[cfg(target_os = "windows")]
fn cfg_default_platform_clipboard() -> Box<dyn PlatformClipboard> {
    Box::new(WindowsOleClipboard::new())
}

#[cfg(target_os = "macos")]
fn cfg_default_platform_clipboard() -> Box<dyn PlatformClipboard> {
    Box::new(NsPasteboardClipboard::new())
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
fn cfg_default_platform_clipboard() -> Box<dyn PlatformClipboard> {
    Box::new(WaylandClipboard::new())
}

#[cfg(all(target_os = "linux", not(feature = "wayland")))]
fn cfg_default_platform_clipboard() -> Box<dyn PlatformClipboard> {
    Box::new(X11Clipboard::new())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn cfg_default_platform_clipboard() -> Box<dyn PlatformClipboard> {
    Box::new(StubClipboard::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::MIME_TEXT_PLAIN;

    #[test]
    fn stub_is_empty_by_default() {
        let cb = StubClipboard::new();
        assert!(cb.available_types().is_empty());
        assert!(cb.get_contents(MIME_TEXT_PLAIN).is_none());
    }

    #[test]
    fn stub_ignores_set_contents() {
        let mut cb = StubClipboard::new();
        cb.set_contents(&ClipboardItem::new().offer_text("ignored"));
        assert!(cb.available_types().is_empty());
        assert!(cb.get_contents(MIME_TEXT_PLAIN).is_none());
    }

    #[test]
    fn stub_clear_is_noop() {
        let mut cb = StubClipboard::new();
        cb.clear();
        assert!(cb.available_types().is_empty());
    }

    #[test]
    fn stub_platform_name() {
        let cb = StubClipboard::new();
        assert_eq!(cb.platform_name(), "stub");
    }

    #[test]
    fn default_platform_returns_usable_clipboard() {
        let cb = default_platform_clipboard();
        assert!(!cb.platform_name().is_empty());
        // Reads are empty regardless of backend (all are stubs today).
        assert!(cb.get_contents(MIME_TEXT_PLAIN).is_none());
        assert!(cb.available_types().is_empty());
    }

    #[test]
    fn default_platform_name_matches_target() {
        let cb = default_platform_clipboard();
        let name = cb.platform_name();
        #[cfg(target_os = "windows")]
        assert_eq!(name, "windows-ole");
        #[cfg(target_os = "macos")]
        assert_eq!(name, "macos-nspasteboard");
        #[cfg(all(target_os = "linux", feature = "wayland"))]
        assert_eq!(name, "wayland");
        #[cfg(all(target_os = "linux", not(feature = "wayland")))]
        assert_eq!(name, "x11");
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        assert_eq!(name, "stub");
    }

    #[test]
    fn platform_clipboard_is_object_safe_via_box() {
        let mut cb: Box<dyn PlatformClipboard> = default_platform_clipboard();
        cb.clear();
        let _name: &str = cb.platform_name();
        let _types: Vec<String> = cb.available_types();
    }

    #[test]
    fn stub_default_matches_new() {
        let a = StubClipboard;
        let b = StubClipboard::new();
        assert_eq!(a.platform_name(), b.platform_name());
    }
}
