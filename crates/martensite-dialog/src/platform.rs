//! Platform backend abstraction.
//!
//! This module defines the [`PlatformDialog`] trait, which extends
//! [`DialogService`] with a [`PlatformDialog::platform_name`] accessor,
//! together with a [`StubDialog`] no-op implementation and a
//! [`default_platform_dialog`] factory that selects the best available
//! backend for the current target.
//!
//! # Platform mechanisms
//!
//! Native file dialogs are driven by `martensite-dialog-platform` when the
//! `platform` Cargo feature is enabled. That crate shells out to the
//! platform's own dialog facility — no FFI is required:
//!
//! * **Windows** — PowerShell `System.Windows.Forms` (`OpenFileDialog`,
//!   `SaveFileDialog`, `FolderBrowserDialog`).
//! * **macOS** — `osascript` driving AppleScript `choose file` /
//!   `choose folder` / `choose file name`, which surfaces the real
//!   `NSOpenPanel` / `NSSavePanel`.
//! * **Linux** — `zenity --file-selection` or `kdialog` (whichever is on
//!   `PATH`); both render the toolkit-native chooser.
//!
//! Without the `platform` feature every backend is a **safe stub** that
//! reports [`DialogOutcome::Cancelled`]; this crate is
//! `#![forbid(unsafe_code)]` and stays a safe, auditable dependency either
//! way.
//!
//! # Examples
//!
//! ```no_run
//! use martensite_dialog::{DialogService, FileDialogRequest, PlatformDialog,
//!     default_platform_dialog};
//!
//! let mut d = default_platform_dialog();
//! // The factory always returns a usable (possibly stub) service.
//! assert!(!d.platform_name().is_empty());
//! let _ = d.show(&FileDialogRequest::open_file());
//! ```

use crate::dialog::{DialogOutcome, DialogService, FileDialogRequest};

/// A [`DialogService`] backed by a specific platform dialog facility.
///
/// Implementations identify themselves via [`platform_name`](PlatformDialog::platform_name)
/// so callers and diagnostics can report which backend is active.
///
/// # Examples
///
/// ```
/// use martensite_dialog::{DialogService, FileDialogRequest, PlatformDialog,
///     StubDialog};
///
/// let mut d = StubDialog::new();
/// assert_eq!(d.platform_name(), "stub");
/// assert!(d.show(&FileDialogRequest::open_file()).is_cancelled());
/// ```
pub trait PlatformDialog: DialogService {
    /// Returns a human-readable backend name, e.g. `"windows-forms"`,
    /// `"macos-osascript"`, `"linux-zenity"`, or `"stub"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dialog::{PlatformDialog, StubDialog};
    ///
    /// assert_eq!(StubDialog::new().platform_name(), "stub");
    /// ```
    fn platform_name(&self) -> &str;
}

/// A no-op platform dialog for environments without OS dialog support.
///
/// Every invocation returns [`DialogOutcome::Cancelled`]. This is the
/// fallback used by [`default_platform_dialog`] when no backend is
/// compiled in, and is useful for headless tests and CI.
///
/// # Examples
///
/// ```
/// use martensite_dialog::{DialogService, FileDialogRequest, PlatformDialog,
///     StubDialog};
///
/// let mut d = StubDialog::new();
/// assert!(d.show(&FileDialogRequest::save_file()).is_cancelled());
/// ```
#[derive(Default, Clone, Debug)]
pub struct StubDialog;

impl StubDialog {
    /// Creates a new [`StubDialog`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_dialog::{PlatformDialog, StubDialog};
    ///
    /// assert_eq!(StubDialog::new().platform_name(), "stub");
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

impl DialogService for StubDialog {
    fn show(&mut self, _request: &FileDialogRequest) -> DialogOutcome {
        DialogOutcome::Cancelled
    }
}

impl PlatformDialog for StubDialog {
    #[inline]
    fn platform_name(&self) -> &str {
        "stub"
    }
}

/// Returns the best available [`PlatformDialog`] for the current target.
///
/// The selection rules are:
///
/// | Target | Backend |
/// |--------|---------|
/// | `windows` | `windows-forms` (PowerShell `System.Windows.Forms`) |
/// | `macos` | `macos-osascript` (`choose file`/`choose folder`) |
/// | `linux` | `linux-zenity` or `linux-kdialog` |
/// | other | [`StubDialog`] |
///
/// Without the `platform` Cargo feature this returns [`StubDialog`];
/// with it, the function delegates to
/// `martensite_dialog_platform::native_backend` and only falls back to
/// [`StubDialog`] when no usable backend exists (e.g. no `zenity` on
/// `PATH`). [`ScriptedDialog`] remains the choice for tests. The returned
/// service is always usable and never panics.
///
/// [`ScriptedDialog`]: crate::ScriptedDialog
///
/// # Examples
///
/// ```
/// use martensite_dialog::{PlatformDialog, default_platform_dialog};
///
/// let d = default_platform_dialog();
/// assert!(!d.platform_name().is_empty());
/// ```
pub fn default_platform_dialog() -> Box<dyn PlatformDialog> {
    cfg_default_platform_dialog()
}

#[cfg(feature = "platform")]
fn cfg_default_platform_dialog() -> Box<dyn PlatformDialog> {
    if let Some(backend) = martensite_dialog_platform::native_backend() {
        return Box::new(PlatformBackendAdapter(backend));
    }
    Box::new(StubDialog::new())
}

#[cfg(not(feature = "platform"))]
fn cfg_default_platform_dialog() -> Box<dyn PlatformDialog> {
    Box::new(StubDialog::new())
}

/// Adapter wrapping a `martensite_dialog_platform::DialogBackend` as a
/// [`PlatformDialog`].
///
/// The FFI/subprocess backend trait lives in the platform crate to avoid
/// a cyclic dependency; this adapter maps the safe crate's
/// [`FileDialogRequest`]/[`DialogOutcome`] onto the platform crate's
/// `DialogSpec`/`DialogReply` wire types.
#[cfg(feature = "platform")]
struct PlatformBackendAdapter(Box<dyn martensite_dialog_platform::DialogBackend>);

#[cfg(feature = "platform")]
impl DialogService for PlatformBackendAdapter {
    fn show(&mut self, request: &FileDialogRequest) -> DialogOutcome {
        use martensite_dialog_platform::{DialogReply, DialogSpec, SpecKind};
        let kind = match request.kind {
            crate::DialogKind::OpenFile => SpecKind::OpenFile,
            crate::DialogKind::OpenFiles => SpecKind::OpenFiles,
            crate::DialogKind::PickFolder => SpecKind::PickFolder,
            crate::DialogKind::SaveFile => SpecKind::SaveFile,
        };
        let spec = DialogSpec {
            kind,
            title: request.title.clone(),
            start_dir: request.start_dir.clone(),
            filters: request
                .filters
                .iter()
                .map(|f| (f.name.clone(), f.extensions.clone()))
                .collect(),
            default_name: request.default_name.clone(),
        };
        match self.0.show(&spec) {
            DialogReply::Cancelled => DialogOutcome::Cancelled,
            DialogReply::Picked(paths) => DialogOutcome::Picked(paths),
        }
    }
}

#[cfg(feature = "platform")]
impl PlatformDialog for PlatformBackendAdapter {
    fn platform_name(&self) -> &str {
        self.0.platform_name()
    }
}
