//! Native file/folder dialog abstraction.
//!
//! `martensite-dialog` provides a platform-agnostic model for the four
//! canonical filesystem dialogs — open-file, multi-open, pick-folder, and
//! save-as — so widgets and application code never touch platform APIs
//! directly.
//!
//! # Architecture
//!
//! * [`FileDialogRequest`] describes one invocation: a [`DialogKind`],
//!   window title, starting directory, [`FileFilter`] set, and a suggested
//!   filename for save dialogs.
//! * [`DialogOutcome`] is `Cancelled` or `Picked(paths)`; [`path`] /
//!   [`paths`] accessors cover the common cases.
//! * [`DialogService`] is the blocking show contract implemented by
//!   backends. [`ScriptedDialog`] is a deterministic canned-response
//!   implementation for tests and headless environments.
//! * [`PlatformDialog`] extends [`DialogService`] with a backend name.
//!   [`default_platform_dialog`] selects the best available backend for
//!   the current target. Because this crate is `#![forbid(unsafe_code)]`,
//!   native dialogs are delegated to `martensite-dialog-platform` behind
//!   the `platform` Cargo feature; without it, every backend is a safe
//!   stub that reports `Cancelled` (see the [`platform`] module docs).
//!
//! [`path`]: DialogOutcome::path
//! [`paths`]: DialogOutcome::paths
//!
//! # Examples
//!
//! ```
//! use martensite_dialog::{DialogOutcome, DialogService, FileDialogRequest,
//!     FileFilter, ScriptedDialog};
//! use std::path::PathBuf;
//!
//! let mut dialogs = ScriptedDialog::new();
//! dialogs.respond_with(DialogOutcome::Picked(vec![PathBuf::from("/a.png")]));
//!
//! let req = FileDialogRequest::open_file()
//!     .title("Choose an image")
//!     .filter(FileFilter::new("Images", ["png", "jpg"]));
//! let out = dialogs.show(&req);
//! assert_eq!(out.path().unwrap().to_str().unwrap(), "/a.png");
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod dialog;
pub mod platform;

pub use dialog::{
    DialogKind, DialogOutcome, DialogService, FileDialogRequest, FileFilter, ScriptedDialog,
};
pub use platform::{default_platform_dialog, PlatformDialog, StubDialog};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn re_exported_scripted_round_trip() {
        let mut d = ScriptedDialog::new();
        d.respond_with(DialogOutcome::Picked(vec![PathBuf::from("/f.txt")]));
        let out = d.show(&FileDialogRequest::open_file());
        assert_eq!(out.path().unwrap().to_str().unwrap(), "/f.txt");
    }

    #[test]
    fn re_exported_default_platform_dialog_is_usable() {
        let d = default_platform_dialog();
        assert!(!d.platform_name().is_empty());
    }

    #[test]
    #[cfg(not(feature = "platform"))]
    fn default_platform_dialog_cancels_without_backend() {
        let mut d = default_platform_dialog();
        assert!(d.show(&FileDialogRequest::open_file()).is_cancelled());
    }

    #[test]
    fn re_exported_stub_cancels() {
        let mut d = StubDialog::new();
        assert!(d.show(&FileDialogRequest::save_file()).is_cancelled());
    }

    #[test]
    fn filter_normalizes_extensions() {
        let f = FileFilter::new("Docs", [".MD", "txt"]);
        assert_eq!(f.extensions, vec!["md".to_string(), "txt".to_string()]);
        assert!(f.matches("README.MD"));
    }
}
