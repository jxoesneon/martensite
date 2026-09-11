//! OS-native clipboard backends for Martensite.
//!
//! This crate provides platform-specific native clipboard implementations:
//!
//! - **macOS**: `NSPasteboard` via the Objective-C runtime
//! - **Windows**: Win32 clipboard API (`OpenClipboard`, `SetClipboardData`, …)
//! - **Linux**: X11 `CLIPBOARD` selection via raw `libX11` FFI
//!
//! # Architecture
//!
//! This crate intentionally does **not** depend on `martensite-clipboard` to
//! avoid a cyclic dependency. It defines its own [`ClipboardBackend`] trait
//! and a [`native_backend`] factory. The `martensite-clipboard` crate wraps
//! this crate's API behind its own `PlatformClipboard` trait when the
//! `platform` feature is enabled.
//!
//! # Safety policy
//!
//! This crate uses `#![allow(unsafe_code)]` at the crate level because it
//! contains platform-specific FFI to the Objective-C runtime (macOS), the
//! Win32 clipboard API (Windows), and X11 selections (Linux). The
//! workspace-level `unsafe_code = "deny"` policy is preserved for all other
//! crates; this is the narrowly scoped audited exception, mirroring the
//! `martensite-font-fallback` crate's boundary decision.
//!
//! All `unsafe` blocks in this crate are confined to platform-specific
//! modules (`macos`, `windows`, `x11`) and are audited against the upstream
//! API documentation:
//! - NSPasteboard: <https://developer.apple.com/documentation/appkit/nspasteboard>
//! - Win32 clipboard: <https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard>
//! - X11 selections: <https://www.x.org/releases/current/doc/xlib/xlib.pdf#selections>

#![allow(unsafe_code)]
#![forbid(missing_docs)]

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod x11;

/// A clipboard backend that reads and writes the OS clipboard.
///
/// This trait mirrors `martensite_clipboard::ClipboardService` but lives in
/// this crate to avoid a cyclic dependency. The `martensite-clipboard` crate
/// provides an adapter that wraps any [`ClipboardBackend`] as a
/// `PlatformClipboard`.
///
/// # Examples
///
/// ```
/// use martensite_clipboard_platform::ClipboardBackend;
///
/// if let Some(backend) = martensite_clipboard_platform::native_backend() {
///     let _name = backend.platform_name();
///     let _ = backend.available_types();
/// }
/// ```
pub trait ClipboardBackend {
    /// Writes the given text payload to the OS clipboard, replacing any
    /// previous contents.
    ///
    /// `mime` is the canonical MIME type (e.g. `text/plain;charset=utf-8`).
    /// `bytes` is the materialized payload.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_clipboard_platform::ClipboardBackend;
    ///
    /// if let Some(mut backend) = martensite_clipboard_platform::native_backend() {
    ///     backend.write("text/plain;charset=utf-8", b"hello");
    /// }
    /// ```
    fn write(&mut self, mime: &str, bytes: &[u8]);

    /// Reads the bytes for the requested MIME type from the OS clipboard,
    /// or `None` if unavailable.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_clipboard_platform::ClipboardBackend;
    ///
    /// if let Some(backend) = martensite_clipboard_platform::native_backend() {
    ///     let _ = backend.read("text/plain;charset=utf-8");
    /// }
    /// ```
    fn read(&self, mime: &str) -> Option<Vec<u8>>;

    /// Returns the list of MIME types currently available on the clipboard.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_clipboard_platform::ClipboardBackend;
    ///
    /// if let Some(backend) = martensite_clipboard_platform::native_backend() {
    ///     let _ = backend.available_types();
    /// }
    /// ```
    fn available_types(&self) -> Vec<String>;

    /// Clears the clipboard contents.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_clipboard_platform::ClipboardBackend;
    ///
    /// if let Some(mut backend) = martensite_clipboard_platform::native_backend() {
    ///     backend.clear();
    /// }
    /// ```
    fn clear(&mut self);

    /// Returns a human-readable name for the platform backend, e.g.
    /// `"macos-nspasteboard"`, `"windows-ole"`, `"x11"`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_clipboard_platform::ClipboardBackend;
    ///
    /// if let Some(backend) = martensite_clipboard_platform::native_backend() {
    ///     assert!(!backend.platform_name().is_empty());
    /// }
    /// ```
    fn platform_name(&self) -> &str;
}

/// Returns the best native [`ClipboardBackend`] for the current platform,
/// or `None` if no native backend is available (e.g. no X server on Linux).
///
/// On macOS this returns a [`MacosBackend`](macos::MacosBackend).
/// On Windows this returns a [`Win32Backend`](windows::Win32Backend).
/// On Linux this returns an [`X11Backend`](x11::X11Backend).
///
/// # Examples
///
/// ```no_run
/// use martensite_clipboard_platform::ClipboardBackend;
///
/// if let Some(mut backend) = martensite_clipboard_platform::native_backend() {
///     backend.write("text/plain;charset=utf-8", b"hello");
///     let read = backend.read("text/plain;charset=utf-8");
///     assert_eq!(read, Some(b"hello".to_vec()));
/// }
/// ```
#[allow(rustdoc::broken_intra_doc_links)]
pub fn native_backend() -> Option<Box<dyn ClipboardBackend>> {
    #[cfg(target_os = "macos")]
    {
        #[allow(clippy::needless_return)]
        return Some(Box::new(macos::MacosBackend::new()));
    }
    #[cfg(target_os = "windows")]
    {
        #[allow(clippy::needless_return)]
        return Some(Box::new(windows::Win32Backend::new()));
    }
    #[cfg(target_os = "linux")]
    {
        #[allow(clippy::needless_return)]
        return Some(Box::new(x11::X11Backend::new()));
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        #[allow(clippy::needless_return)]
        return None;
    }
}
