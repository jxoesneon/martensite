//! Core share data model and service trait.
//!
//! This module is platform-agnostic: it defines the request/response
//! wire types every backend consumes ([`ShareRequest`],
//! [`ShareOutcome`]), the [`ShareService`] contract, and
//! [`ScriptedShare`], a deterministic canned-response implementation
//! for tests and headless environments.
//!
//! # What subprocess backends can and cannot do
//!
//! The native share *sheet* (NSSharingService, Windows
//! `DataTransferManager`, portal share) requires in-process UI
//! integration; the subprocess backends this crate ships instead route
//! shares through URI handlers and the file manager:
//!
//! * `url` — opened in the platform default handler (`open`,
//!   `xdg-open`, `explorer`): `https:`, `mailto:`, `tel:`, `sms:` and
//!   custom schemes all work.
//! * `text` alone — drafted as a `mailto:?body=…` message, the
//!   universal share-to-email path.
//! * `files` — reported [`ShareOutcome::Unsupported`] by `share`;
//!   use [`ShareService::reveal`] to show them in the file manager.
//!
//! # Examples
//!
//! ```
//! use martensite_share::{ShareRequest, ShareService, ScriptedShare};
//!
//! let mut svc = ScriptedShare::new();
//! let req = ShareRequest::url("https://example.com/report.pdf");
//! assert!(svc.share(&req).is_shared());
//! ```

use std::path::{Path, PathBuf};

/// One share action.
///
/// Build with [`ShareRequest::url`], [`ShareRequest::text`], or
/// [`ShareRequest::files`], then refine with the builder methods.
/// Precedence in subprocess backends: `url` first, then `text`
/// (mailto draft); `files` are only handled by
/// [`ShareService::reveal`].
///
/// # Examples
///
/// ```
/// use martensite_share::ShareRequest;
///
/// let req = ShareRequest::text("check this out").subject("FYI");
/// assert_eq!(req.subject.as_deref(), Some("FYI"));
/// ```
#[derive(Clone, Debug, Default)]
pub struct ShareRequest {
    /// Plain-text payload.
    pub text: Option<String>,
    /// A URI to open in the platform handler (`https:`, `mailto:`,
    /// `tel:`, custom schemes).
    pub url: Option<String>,
    /// Files to share — subprocess backends report `Unsupported`;
    /// reveal them via [`ShareService::reveal`] instead.
    pub files: Vec<PathBuf>,
    /// Subject line for mailto-draft shares.
    pub subject: Option<String>,
}

impl ShareRequest {
    /// Share a URI via the platform's default handler.
    ///
    /// ```
    /// use martensite_share::ShareRequest;
    ///
    /// assert_eq!(ShareRequest::url("tel:+15551234").url.as_deref(), Some("tel:+15551234"));
    /// ```
    pub fn url(url: impl Into<String>) -> Self {
        Self {
            url: Some(url.into()),
            ..Default::default()
        }
    }

    /// Share plain text (drafted as an email body by subprocess
    /// backends).
    ///
    /// ```
    /// use martensite_share::ShareRequest;
    ///
    /// assert_eq!(ShareRequest::text("hi").text.as_deref(), Some("hi"));
    /// ```
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Default::default()
        }
    }

    /// Share files — subprocess backends report `Unsupported`; prefer
    /// [`ShareService::reveal`] for file-manager presentation.
    ///
    /// ```
    /// use martensite_share::ShareRequest;
    ///
    /// assert_eq!(ShareRequest::files(["/a.png"]).files.len(), 1);
    /// ```
    pub fn files(paths: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        Self {
            files: paths.into_iter().map(Into::into).collect(),
            ..Default::default()
        }
    }

    /// Subject for mailto-draft shares.
    ///
    /// ```
    /// use martensite_share::ShareRequest;
    ///
    /// assert_eq!(ShareRequest::text("b").subject("s").subject.as_deref(), Some("s"));
    /// ```
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }
}

/// The result of a [`ShareService::share`] or [`ShareService::reveal`]
/// call.
///
/// # Examples
///
/// ```
/// use martensite_share::ShareOutcome;
///
/// assert!(ShareOutcome::Shared.is_shared());
/// assert!(ShareOutcome::Unsupported("files".into()).is_unsupported());
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum ShareOutcome {
    /// The share was dispatched to a platform handler.
    Shared,
    /// The backend cannot service this request shape — the payload is
    /// named in the message (e.g. `"files"`).
    Unsupported(String),
    /// Dispatch to the platform handler failed.
    Failed(String),
}

impl ShareOutcome {
    /// `true` when the share was dispatched.
    ///
    /// ```
    /// use martensite_share::ShareOutcome;
    ///
    /// assert!(ShareOutcome::Shared.is_shared());
    /// ```
    pub fn is_shared(&self) -> bool {
        matches!(self, ShareOutcome::Shared)
    }

    /// `true` when the backend cannot service this request shape.
    ///
    /// ```
    /// use martensite_share::ShareOutcome;
    ///
    /// assert!(ShareOutcome::Unsupported("x".into()).is_unsupported());
    /// ```
    pub fn is_unsupported(&self) -> bool {
        matches!(self, ShareOutcome::Unsupported(_))
    }
}

/// The service contract every share backend implements.
///
/// `share` dispatches asynchronously on every backend — the URI handler
/// or mail draft opens in its own process — so calls return promptly.
///
/// # Examples
///
/// ```
/// use martensite_share::{ShareRequest, ShareService, ScriptedShare};
///
/// let mut svc = ScriptedShare::new();
/// assert!(svc.share(&ShareRequest::text("hi")).is_shared());
/// ```
pub trait ShareService {
    /// Dispatch `request` and return the outcome.
    fn share(&mut self, request: &ShareRequest) -> ShareOutcome;

    /// Reveal `path` in the platform file manager (Finder, Explorer,
    /// the freedesktop default). Default: `Unsupported`.
    ///
    /// ```
    /// use martensite_share::{ShareOutcome, ShareRequest, ShareService};
    ///
    /// struct S;
    /// impl ShareService for S {
    ///     fn share(&mut self, _: &ShareRequest) -> ShareOutcome {
    ///         ShareOutcome::Shared
    ///     }
    /// }
    /// assert!(S.reveal(std::path::Path::new("/x")).is_unsupported());
    /// ```
    fn reveal(&mut self, _path: &Path) -> ShareOutcome {
        ShareOutcome::Unsupported("reveal not supported by this backend".to_string())
    }
}

/// A deterministic canned-response backend for tests and headless runs.
///
/// Outcomes are enqueued via [`ScriptedShare::respond_with`] and popped
/// FIFO per [`share`](ShareService::share) call; an empty queue yields
/// [`ShareOutcome::Shared`]. The most recent request is retained for
/// assertions via [`ScriptedShare::last_request`].
///
/// # Examples
///
/// ```
/// use martensite_share::{ShareOutcome, ShareRequest, ShareService,
///     ScriptedShare};
///
/// let mut svc = ScriptedShare::new();
/// svc.respond_with(ShareOutcome::Failed("no handler".into()));
/// assert!(!svc.share(&ShareRequest::url("https://x")).is_shared());
/// assert_eq!(svc.last_request().unwrap().url.as_deref(), Some("https://x"));
/// ```
#[derive(Default, Debug)]
pub struct ScriptedShare {
    queue: std::collections::VecDeque<ShareOutcome>,
    last: Option<ShareRequest>,
    revealed: Vec<PathBuf>,
}

impl ScriptedShare {
    /// An empty scripted share (every `share` reports `Shared`).
    ///
    /// ```
    /// use martensite_share::{ShareRequest, ShareService, ScriptedShare};
    ///
    /// let mut s = ScriptedShare::new();
    /// assert!(s.share(&ShareRequest::text("t")).is_shared());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue an outcome returned by the next `share` call.
    ///
    /// ```
    /// use martensite_share::{ShareOutcome, ShareRequest, ShareService,
    ///     ScriptedShare};
    ///
    /// let mut s = ScriptedShare::new();
    /// s.respond_with(ShareOutcome::Unsupported("files".into()));
    /// assert!(s.share(&ShareRequest::files(["/f"])).is_unsupported());
    /// ```
    pub fn respond_with(&mut self, outcome: ShareOutcome) {
        self.queue.push_back(outcome);
    }

    /// The request passed to the most recent `share` call.
    ///
    /// ```
    /// use martensite_share::{ShareRequest, ShareService, ScriptedShare};
    ///
    /// let mut s = ScriptedShare::new();
    /// assert!(s.last_request().is_none());
    /// s.share(&ShareRequest::text("body").subject("s"));
    /// assert_eq!(s.last_request().unwrap().subject.as_deref(), Some("s"));
    /// ```
    pub fn last_request(&self) -> Option<&ShareRequest> {
        self.last.as_ref()
    }

    /// Paths passed to [`reveal`](ShareService::reveal) so far.
    ///
    /// ```
    /// use martensite_share::{ShareService, ScriptedShare};
    /// use std::path::Path;
    ///
    /// let mut s = ScriptedShare::new();
    /// s.reveal(Path::new("/a"));
    /// assert_eq!(s.revealed().len(), 1);
    /// ```
    pub fn revealed(&self) -> &[PathBuf] {
        &self.revealed
    }
}

impl ShareService for ScriptedShare {
    fn share(&mut self, request: &ShareRequest) -> ShareOutcome {
        self.last = Some(request.clone());
        self.queue.pop_front().unwrap_or(ShareOutcome::Shared)
    }

    fn reveal(&mut self, path: &Path) -> ShareOutcome {
        self.revealed.push(path.to_path_buf());
        ShareOutcome::Shared
    }
}
