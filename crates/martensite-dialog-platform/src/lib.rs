//! OS-native file dialog backends for Martensite.
//!
//! This crate provides platform-specific native file/folder dialogs:
//!
//! - **macOS**: `osascript` driving AppleScript `choose file` /
//!   `choose folder` / `choose file name`, which presents the real
//!   `NSOpenPanel` / `NSSavePanel`
//! - **Windows**: PowerShell `System.Windows.Forms` (`OpenFileDialog`,
//!   `SaveFileDialog`, `FolderBrowserDialog`)
//! - **Linux**: `zenity --file-selection` (GTK chooser) or `kdialog`
//!   (Qt chooser), selected automatically by `PATH` probing
//!
//! # Architecture
//!
//! This crate intentionally does **not** depend on `martensite-dialog` to
//! avoid a cyclic dependency. It defines its own [`DialogBackend`] trait,
//! wire types ([`DialogSpec`], [`SpecKind`], [`DialogReply`]), and a
//! [`native_backend`] factory. The `martensite-dialog` crate wraps this
//! crate's API behind its own `PlatformDialog` trait when the `platform`
//! feature is enabled.
//!
//! # Safety policy
//!
//! Unlike the other `martensite-*-platform` crates this crate carries
//! `#![forbid(unsafe_code)]`: every backend drives the platform's own
//! dialog facility through a short-lived subprocess, so no FFI or unsafe
//! code is required at all. This mirrors the `wl-clipboard` subprocess
//! backend in `martensite-clipboard-platform`.
//!
//! # Examples
//!
//! ```
//! use martensite_dialog_platform::native_backend;
//!
//! // `Some` on desktop targets with a usable dialog tool on PATH;
//! // `None` elsewhere (e.g. a bare Linux CI box without zenity).
//! let _backend = native_backend();
//! ```
#![forbid(unsafe_code)]
#![forbid(missing_docs)]

use std::path::PathBuf;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// Selection mode for a [`DialogSpec`].
///
/// Mirrors `martensite_dialog::DialogKind`; duplicated here to keep this
/// crate dependency-free (see the crate-level docs).
///
/// # Examples
///
/// ```
/// use martensite_dialog_platform::SpecKind;
///
/// assert_ne!(SpecKind::OpenFile, SpecKind::SaveFile);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpecKind {
    /// Select one existing file.
    #[default]
    OpenFile,
    /// Select one or more existing files.
    OpenFiles,
    /// Select one existing directory.
    PickFolder,
    /// Choose a destination path for writing.
    SaveFile,
}

/// A platform-agnostic dialog request (wire type).
///
/// `filters` are `(name, extensions)` pairs; extensions have no leading
/// dot. Mirrors `martensite_dialog::FileDialogRequest`.
///
/// # Examples
///
/// ```
/// use martensite_dialog_platform::{DialogSpec, SpecKind};
///
/// let s = DialogSpec {
///     kind: SpecKind::OpenFile,
///     title: "Pick".to_string(),
///     ..Default::default()
/// };
/// assert_eq!(s.kind, SpecKind::OpenFile);
/// ```
#[derive(Clone, Debug, Default)]
pub struct DialogSpec {
    /// Selection mode.
    pub kind: SpecKind,
    /// Window title override (empty = OS default).
    pub title: String,
    /// Directory the dialog opens in.
    pub start_dir: Option<PathBuf>,
    /// `(name, extensions)` file-type filters (empty = all files).
    pub filters: Vec<(String, Vec<String>)>,
    /// Suggested filename for [`SpecKind::SaveFile`].
    pub default_name: Option<String>,
}

/// The reply a backend produces for a [`DialogSpec`].
///
/// # Examples
///
/// ```
/// use martensite_dialog_platform::DialogReply;
///
/// assert!(matches!(DialogReply::Cancelled, DialogReply::Cancelled));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogReply {
    /// The user dismissed the dialog without selecting.
    Cancelled,
    /// One or more selected paths.
    Picked(Vec<PathBuf>),
}

/// A native dialog backend.
///
/// Implementations block for the duration of the dialog and return the
/// user's reply. `None` is never returned — a backend that cannot launch
/// should not be constructed (see [`native_backend`]); runtime failures
/// map to [`DialogReply::Cancelled`].
///
/// # Examples
///
/// ```
/// use martensite_dialog_platform::{DialogBackend, DialogReply, DialogSpec};
///
/// struct Null;
/// impl DialogBackend for Null {
///     fn show(&mut self, _s: &DialogSpec) -> DialogReply { DialogReply::Cancelled }
///     fn platform_name(&self) -> &str { "null" }
/// }
/// assert_eq!(Null.platform_name(), "null");
/// ```
pub trait DialogBackend {
    /// Show the dialog described by `spec` and return the user's reply.
    fn show(&mut self, spec: &DialogSpec) -> DialogReply;
    /// Human-readable backend name, e.g. `"macos-osascript"`.
    fn platform_name(&self) -> &str;
}

/// Returns the best available [`DialogBackend`] for the current target.
///
/// | Target | Backend | Availability gate |
/// |--------|---------|-------------------|
/// | `windows` | `windows-forms` | always (PowerShell is in-box) |
/// | `macos` | `macos-osascript` | always (osascript is in-box) |
/// | `linux` | `linux-zenity` → `linux-kdialog` | binary on `PATH` |
/// | other | — | `None` |
///
/// # Examples
///
/// ```
/// use martensite_dialog_platform::native_backend;
///
/// if let Some(b) = native_backend() {
///     assert!(!b.platform_name().is_empty());
/// }
/// ```
pub fn native_backend() -> Option<Box<dyn DialogBackend>> {
    #[cfg(target_os = "macos")]
    {
        Some(Box::new(macos::OsascriptDialog::new()))
    }
    #[cfg(target_os = "windows")]
    {
        Some(Box::new(windows::WinFormsDialog::new()))
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
