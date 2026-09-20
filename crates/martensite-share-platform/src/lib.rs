//! OS-native share backends for Martensite.
//!
//! This crate provides platform-specific share dispatch and
//! file-manager reveal:
//!
//! - **macOS**: `open <uri>` for URL/mailto shares, `open -R <path>`
//!   for Finder reveal
//! - **Linux**: `xdg-open` (or `gio open`) for shares and the parent
//!   directory for reveal, selected automatically by `PATH` probing
//! - **Windows**: `explorer <uri>` for shares, `explorer /select,` for
//!   Explorer reveal
//!
//! # What these backends cannot do
//!
//! The platform share *sheet* (NSSharingService, WinRT
//! `DataTransferManager`, freedesktop share portal) requires in-process
//! UI integration — no subprocess can present it. These backends route
//! shares through URI handlers instead: a `url` opens in its registered
//! app, a bare `text` payload drafts a `mailto:` message, and `files`
//! report [`ShareReply::Unsupported`] (use `reveal` for file-manager
//! presentation).
//!
//! # Architecture
//!
//! This crate intentionally does **not** depend on `martensite-share`
//! to avoid a cyclic dependency. It defines its own [`ShareBackend`]
//! trait, wire types ([`ShareSpec`], [`ShareReply`]), and a
//! [`native_backend`] factory. The `martensite-share` crate wraps this
//! crate's API behind its own `PlatformShare` trait when the `platform`
//! feature is enabled.
//!
//! # Safety policy
//!
//! Like `martensite-dialog-platform`, this crate carries
//! `#![forbid(unsafe_code)]`: every backend drives platform tools
//! through short-lived subprocesses, so no FFI or unsafe code is
//! required at all.
//!
//! # Examples
//!
//! ```
//! use martensite_share_platform::native_backend;
//!
//! // `Some` on desktop targets with a usable dispatch tool on PATH;
//! // `None` elsewhere (e.g. a bare Linux CI box without xdg-open).
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

/// Percent-encode `s` for embedding inside a URI query value.
///
/// Leaves RFC 3986 unreserved characters (`A-Z a-z 0-9 - _ . ~`)
/// intact and percent-encodes everything else, UTF-8 byte by byte.
///
/// # Examples
///
/// ```
/// use martensite_share_platform::percent_encode;
///
/// assert_eq!(percent_encode("a b&c"), "a%20b%26c");
/// ```
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Build a `mailto:` URI drafting `subject`/`body` — the universal
/// subprocess fallback for text shares.
///
/// # Examples
///
/// ```
/// use martensite_share_platform::mailto_uri;
///
/// assert_eq!(mailto_uri(Some("Hi"), "body text"), "mailto:?subject=Hi&body=body%20text");
/// ```
pub fn mailto_uri(subject: Option<&str>, body: &str) -> String {
    let mut params = Vec::new();
    if let Some(s) = subject {
        if !s.is_empty() {
            params.push(format!("subject={}", percent_encode(s)));
        }
    }
    if !body.is_empty() {
        params.push(format!("body={}", percent_encode(body)));
    }
    if params.is_empty() {
        "mailto:".to_string()
    } else {
        format!("mailto:?{}", params.join("&"))
    }
}

/// A platform-agnostic share request (wire type).
///
/// Mirrors `martensite_share::ShareRequest`. Precedence in subprocess
/// backends: `url` first, then `text` (mailto draft); `files` are
/// never dispatched — they report [`ShareReply::Unsupported`].
///
/// # Examples
///
/// ```
/// use martensite_share_platform::ShareSpec;
///
/// let s = ShareSpec {
///     url: Some("https://example.com".into()),
///     ..Default::default()
/// };
/// assert!(s.text.is_none());
/// ```
#[derive(Clone, Debug, Default)]
pub struct ShareSpec {
    /// Plain-text payload.
    pub text: Option<String>,
    /// URI to open in the platform handler.
    pub url: Option<String>,
    /// Subject for mailto-draft shares.
    pub subject: Option<String>,
    /// Files — always [`ShareReply::Unsupported`] on subprocess
    /// backends; kept in the wire so adapters stay total.
    pub files: Vec<PathBuf>,
}

/// The reply a backend produces for a [`ShareSpec`] or reveal call.
///
/// # Examples
///
/// ```
/// use martensite_share_platform::ShareReply;
///
/// assert!(matches!(ShareReply::Shared, ShareReply::Shared));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum ShareReply {
    /// The share was dispatched to a platform handler.
    Shared,
    /// The backend cannot service this request shape.
    Unsupported(String),
    /// Dispatch to the platform handler failed.
    Failed(String),
}

/// A native share backend.
///
/// Implementations dispatch asynchronously — the URI handler or mail
/// draft opens in its own process — so calls return promptly. A
/// backend that cannot launch should not be constructed (see
/// [`native_backend`]); runtime failures map to [`ShareReply::Failed`].
///
/// # Examples
///
/// ```
/// use martensite_share_platform::{ShareBackend, ShareReply, ShareSpec};
///
/// struct Null;
/// impl ShareBackend for Null {
///     fn share(&mut self, _s: &ShareSpec) -> ShareReply { ShareReply::Shared }
///     fn reveal(&mut self, _p: &std::path::Path) -> ShareReply { ShareReply::Shared }
///     fn platform_name(&self) -> &str { "null" }
/// }
/// assert_eq!(Null.platform_name(), "null");
/// ```
pub trait ShareBackend {
    /// Dispatch `spec` and return the outcome.
    fn share(&mut self, spec: &ShareSpec) -> ShareReply;

    /// Reveal `path` in the platform file manager.
    fn reveal(&mut self, path: &std::path::Path) -> ShareReply;

    /// Human-readable backend name, e.g. `"macos-open"`.
    fn platform_name(&self) -> &str;
}

/// Resolve the dispatch URI for `spec`, or report why it cannot be
/// dispatched. Shared by every backend: `url` wins, bare `text` becomes
/// a mailto draft, `files` are unsupported, an empty spec is a no-op
/// failure.
///
/// # Examples
///
/// ```
/// use martensite_share_platform::{share_uri, ShareSpec};
///
/// let uri = share_uri(&ShareSpec {
///     text: Some("hello world".into()),
///     subject: Some("s".into()),
///     ..Default::default()
/// });
/// assert_eq!(uri.unwrap(), "mailto:?subject=s&body=hello%20world");
/// ```
pub fn share_uri(spec: &ShareSpec) -> Result<String, ShareReply> {
    if !spec.files.is_empty() {
        return Err(ShareReply::Unsupported(
            "files: platform share sheets need in-process UI integration; \
             use reveal() for file-manager presentation"
                .to_string(),
        ));
    }
    if let Some(url) = spec.url.as_deref().filter(|u| !u.is_empty()) {
        return Ok(url.to_string());
    }
    match spec.text.as_deref().filter(|t| !t.is_empty()) {
        Some(t) => Ok(mailto_uri(spec.subject.as_deref(), t)),
        None => Err(ShareReply::Failed("empty share request".to_string())),
    }
}

/// Returns the best available [`ShareBackend`] for the current target.
///
/// | Target | Backend | Availability gate |
/// |--------|---------|-------------------|
/// | `macos` | `macos-open` | always (`open` is in-box) |
/// | `linux` | `linux-xdg-open` → `linux-gio` | binary on `PATH` |
/// | `windows` | `windows-explorer` | always (explorer is in-box) |
/// | other | — | `None` |
///
/// # Examples
///
/// ```
/// use martensite_share_platform::native_backend;
///
/// if let Some(b) = native_backend() {
///     assert!(!b.platform_name().is_empty());
/// }
/// ```
pub fn native_backend() -> Option<Box<dyn ShareBackend>> {
    #[cfg(target_os = "macos")]
    {
        Some(Box::new(macos::MacosShare::new()))
    }
    #[cfg(target_os = "windows")]
    {
        Some(Box::new(windows::ExplorerShare::new()))
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
