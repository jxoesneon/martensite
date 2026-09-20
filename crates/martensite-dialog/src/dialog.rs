//! Core file-dialog data model and service trait.
//!
//! This module is platform-agnostic: it defines the request/response wire
//! types every backend consumes ([`FileDialogRequest`], [`FileFilter`],
//! [`DialogKind`], [`DialogOutcome`]), the [`DialogService`] contract, and
//! [`ScriptedDialog`], a deterministic canned-response implementation for
//! tests and headless environments.
//!
//! # Examples
//!
//! ```
//! use martensite_dialog::{DialogService, FileDialogRequest, ScriptedDialog};
//!
//! let mut svc = ScriptedDialog::new();
//! let req = FileDialogRequest::open_file().title("Pick a font");
//! assert!(svc.show(&req).is_cancelled());
//! ```

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// What kind of filesystem selection a dialog performs.
///
/// # Examples
///
/// ```
/// use martensite_dialog::DialogKind;
///
/// assert!(DialogKind::OpenFiles.is_multi());
/// assert!(!DialogKind::SaveFile.is_multi());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DialogKind {
    /// Select one existing file.
    #[default]
    OpenFile,
    /// Select one or more existing files.
    OpenFiles,
    /// Select one existing directory.
    PickFolder,
    /// Choose a destination path for writing (may not exist yet).
    SaveFile,
}

impl DialogKind {
    /// `true` when the dialog may return more than one path.
    ///
    /// ```
    /// use martensite_dialog::DialogKind;
    ///
    /// assert!(DialogKind::OpenFiles.is_multi());
    /// ```
    pub fn is_multi(self) -> bool {
        matches!(self, DialogKind::OpenFiles)
    }
}

/// A named file-type filter (e.g. `"Images"` → `png`, `jpg`, `webp`).
///
/// Extensions are compared case-insensitively and must not contain the
/// leading dot — `"png"`, not `".png"`.
///
/// # Examples
///
/// ```
/// use martensite_dialog::FileFilter;
///
/// let f = FileFilter::new("Images", ["png", "jpg"]);
/// assert_eq!(f.name, "Images");
/// assert!(f.matches("photo.JPG"));
/// assert!(!f.matches("notes.txt"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFilter {
    /// Human-readable group name shown in the dialog.
    pub name: String,
    /// Accepted extensions without the leading dot.
    pub extensions: Vec<String>,
}

impl FileFilter {
    /// A filter named `name` accepting `extensions`.
    ///
    /// ```
    /// use martensite_dialog::FileFilter;
    ///
    /// let f = FileFilter::new("Rust", ["rs"]);
    /// assert_eq!(f.extensions, vec!["rs".to_string()]);
    /// ```
    pub fn new(
        name: impl Into<String>,
        extensions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            extensions: extensions
                .into_iter()
                .map(|e| e.into().trim_start_matches('.').to_lowercase())
                .collect(),
        }
    }

    /// `true` when `path`'s extension is in this filter.
    ///
    /// ```
    /// use martensite_dialog::FileFilter;
    ///
    /// let f = FileFilter::new("Docs", ["md", ".TXT"]);
    /// assert!(f.matches("readme.md"));
    /// assert!(f.matches("NOTES.txt"));
    /// ```
    pub fn matches(&self, path: impl AsRef<Path>) -> bool {
        path.as_ref()
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| self.extensions.iter().any(|x| x == &e.to_lowercase()))
            .unwrap_or(false)
    }
}

/// A request describing one dialog invocation.
///
/// Build with [`FileDialogRequest::open_file`], [`open_files`],
/// [`pick_folder`], or [`save_file`], then refine with the builder methods.
///
/// # Examples
///
/// ```
/// use martensite_dialog::{DialogKind, FileDialogRequest, FileFilter};
///
/// let req = FileDialogRequest::save_file()
///     .title("Export report")
///     .default_name("report.pdf")
///     .filter(FileFilter::new("PDF", ["pdf"]));
/// assert_eq!(req.kind, DialogKind::SaveFile);
/// assert_eq!(req.filters.len(), 1);
/// ```
#[derive(Clone, Debug, Default)]
pub struct FileDialogRequest {
    /// Selection mode.
    pub kind: DialogKind,
    /// Window title override (empty = OS default).
    pub title: String,
    /// Directory the dialog opens in (`None` = OS default / last used).
    pub start_dir: Option<PathBuf>,
    /// File-type filters (empty = all files).
    pub filters: Vec<FileFilter>,
    /// Suggested filename for [`DialogKind::SaveFile`].
    pub default_name: Option<String>,
}

impl FileDialogRequest {
    /// A single-file open request.
    ///
    /// ```
    /// use martensite_dialog::{DialogKind, FileDialogRequest};
    ///
    /// assert_eq!(FileDialogRequest::open_file().kind, DialogKind::OpenFile);
    /// ```
    pub fn open_file() -> Self {
        Self {
            kind: DialogKind::OpenFile,
            ..Default::default()
        }
    }

    /// A multi-file open request.
    ///
    /// ```
    /// use martensite_dialog::{DialogKind, FileDialogRequest};
    ///
    /// assert_eq!(FileDialogRequest::open_files().kind, DialogKind::OpenFiles);
    /// ```
    pub fn open_files() -> Self {
        Self {
            kind: DialogKind::OpenFiles,
            ..Default::default()
        }
    }

    /// A directory-picker request.
    ///
    /// ```
    /// use martensite_dialog::{DialogKind, FileDialogRequest};
    ///
    /// assert_eq!(FileDialogRequest::pick_folder().kind, DialogKind::PickFolder);
    /// ```
    pub fn pick_folder() -> Self {
        Self {
            kind: DialogKind::PickFolder,
            ..Default::default()
        }
    }

    /// A save-destination request.
    ///
    /// ```
    /// use martensite_dialog::{DialogKind, FileDialogRequest};
    ///
    /// assert_eq!(FileDialogRequest::save_file().kind, DialogKind::SaveFile);
    /// ```
    pub fn save_file() -> Self {
        Self {
            kind: DialogKind::SaveFile,
            ..Default::default()
        }
    }

    /// Dialog window title.
    ///
    /// ```
    /// use martensite_dialog::FileDialogRequest;
    ///
    /// assert_eq!(FileDialogRequest::open_file().title("T").title, "T");
    /// ```
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Initial directory.
    ///
    /// ```
    /// use martensite_dialog::FileDialogRequest;
    /// use std::path::PathBuf;
    ///
    /// let r = FileDialogRequest::open_file().start_dir("/tmp");
    /// assert_eq!(r.start_dir, Some(PathBuf::from("/tmp")));
    /// ```
    pub fn start_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.start_dir = Some(dir.into());
        self
    }

    /// Add a file-type filter.
    ///
    /// ```
    /// use martensite_dialog::{FileDialogRequest, FileFilter};
    ///
    /// let r = FileDialogRequest::open_file().filter(FileFilter::new("T", ["t"]));
    /// assert_eq!(r.filters.len(), 1);
    /// ```
    pub fn filter(mut self, filter: FileFilter) -> Self {
        self.filters.push(filter);
        self
    }

    /// Suggested filename for save dialogs.
    ///
    /// ```
    /// use martensite_dialog::FileDialogRequest;
    ///
    /// let r = FileDialogRequest::save_file().default_name("out.txt");
    /// assert_eq!(r.default_name.as_deref(), Some("out.txt"));
    /// ```
    pub fn default_name(mut self, name: impl Into<String>) -> Self {
        self.default_name = Some(name.into());
        self
    }
}

/// The result of a completed dialog invocation.
///
/// # Examples
///
/// ```
/// use martensite_dialog::DialogOutcome;
/// use std::path::PathBuf;
///
/// assert!(DialogOutcome::Cancelled.is_cancelled());
/// let ok = DialogOutcome::Picked(vec![PathBuf::from("/a.txt")]);
/// assert_eq!(ok.path().unwrap().to_str().unwrap(), "/a.txt");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogOutcome {
    /// The user dismissed the dialog without selecting.
    Cancelled,
    /// One or more selected paths (exactly one unless
    /// [`DialogKind::OpenFiles`] was requested).
    Picked(Vec<PathBuf>),
}

impl DialogOutcome {
    /// `true` when the user cancelled.
    ///
    /// ```
    /// use martensite_dialog::DialogOutcome;
    ///
    /// assert!(DialogOutcome::Cancelled.is_cancelled());
    /// ```
    pub fn is_cancelled(&self) -> bool {
        matches!(self, DialogOutcome::Cancelled)
    }

    /// The first picked path, if any.
    ///
    /// ```
    /// use martensite_dialog::DialogOutcome;
    /// use std::path::{Path, PathBuf};
    ///
    /// let o = DialogOutcome::Picked(vec![PathBuf::from("/x")]);
    /// assert_eq!(o.path(), Some(Path::new("/x")));
    /// ```
    pub fn path(&self) -> Option<&Path> {
        match self {
            DialogOutcome::Picked(p) => p.first().map(PathBuf::as_path),
            DialogOutcome::Cancelled => None,
        }
    }

    /// All picked paths (empty when cancelled).
    ///
    /// ```
    /// use martensite_dialog::DialogOutcome;
    /// use std::path::PathBuf;
    ///
    /// let o = DialogOutcome::Picked(vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    /// assert_eq!(o.paths().len(), 2);
    /// ```
    pub fn paths(&self) -> &[PathBuf] {
        match self {
            DialogOutcome::Picked(p) => p,
            DialogOutcome::Cancelled => &[],
        }
    }
}

/// The service contract every dialog backend implements.
///
/// Calls are **blocking**: the method returns after the user confirms or
/// cancels. Hosts that must keep a UI responsive should invoke it from a
/// worker thread (native sheets/portals are dispatched asynchronously by
/// the backend where the platform requires it).
///
/// # Examples
///
/// ```
/// use martensite_dialog::{DialogService, FileDialogRequest, ScriptedDialog};
///
/// let mut svc = ScriptedDialog::new();
/// assert!(svc.show(&FileDialogRequest::open_file()).is_cancelled());
/// ```
pub trait DialogService {
    /// Show the dialog described by `request` and return the outcome.
    fn show(&mut self, request: &FileDialogRequest) -> DialogOutcome;
}

/// A deterministic canned-response backend for tests and headless runs.
///
/// Outcomes are enqueued via [`ScriptedDialog::respond_with`] and popped
/// FIFO per [`DialogService::show`] call; an empty queue yields
/// [`DialogOutcome::Cancelled`]. The most recent request is retained for
/// assertions via [`ScriptedDialog::last_request`].
///
/// # Examples
///
/// ```
/// use martensite_dialog::{DialogOutcome, DialogService, FileDialogRequest,
///     ScriptedDialog};
/// use std::path::PathBuf;
///
/// let mut svc = ScriptedDialog::new();
/// svc.respond_with(DialogOutcome::Picked(vec![PathBuf::from("/tmp/a.png")]));
/// let out = svc.show(&FileDialogRequest::open_file());
/// assert_eq!(out.path().unwrap().to_str().unwrap(), "/tmp/a.png");
/// // Queue now empty → subsequent calls cancel.
/// assert!(svc.show(&FileDialogRequest::open_file()).is_cancelled());
/// assert_eq!(svc.last_request().unwrap().title, "");
/// ```
#[derive(Default, Debug)]
pub struct ScriptedDialog {
    queue: VecDeque<DialogOutcome>,
    last: Option<FileDialogRequest>,
}

impl ScriptedDialog {
    /// An empty scripted dialog (every call cancels).
    ///
    /// ```
    /// use martensite_dialog::{DialogService, FileDialogRequest, ScriptedDialog};
    ///
    /// let mut s = ScriptedDialog::new();
    /// assert!(s.show(&FileDialogRequest::pick_folder()).is_cancelled());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue an outcome returned by the next `show` call.
    ///
    /// ```
    /// use martensite_dialog::{DialogOutcome, DialogService, FileDialogRequest,
    ///     ScriptedDialog};
    /// use std::path::PathBuf;
    ///
    /// let mut s = ScriptedDialog::new();
    /// s.respond_with(DialogOutcome::Picked(vec![PathBuf::from("/f")]));
    /// assert!(!s.show(&FileDialogRequest::open_file()).is_cancelled());
    /// ```
    pub fn respond_with(&mut self, outcome: DialogOutcome) {
        self.queue.push_back(outcome);
    }

    /// The request passed to the most recent `show` call.
    ///
    /// ```
    /// use martensite_dialog::{DialogService, FileDialogRequest, ScriptedDialog};
    ///
    /// let mut s = ScriptedDialog::new();
    /// assert!(s.last_request().is_none());
    /// s.show(&FileDialogRequest::save_file().title("Export"));
    /// assert_eq!(s.last_request().unwrap().title, "Export");
    /// ```
    pub fn last_request(&self) -> Option<&FileDialogRequest> {
        self.last.as_ref()
    }
}

impl DialogService for ScriptedDialog {
    fn show(&mut self, request: &FileDialogRequest) -> DialogOutcome {
        self.last = Some(request.clone());
        self.queue.pop_front().unwrap_or(DialogOutcome::Cancelled)
    }
}
