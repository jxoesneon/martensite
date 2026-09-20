//! Platform backend abstraction.
//!
//! This module defines the [`PlatformShare`] trait, which extends
//! [`ShareService`] with a [`PlatformShare::platform_name`] accessor,
//! together with a [`StubShare`] no-op implementation and a
//! [`default_platform_share`] factory that selects the best available
//! backend for the current target.
//!
//! # Platform mechanisms
//!
//! Native share dispatch is driven by `martensite-share-platform` when
//! the `platform` Cargo feature is enabled. That crate shells out to
//! the platform's own URI/file-manager facilities — no FFI is required:
//!
//! * **macOS** — `open <uri>` for URL/mailto shares, `open -R` for
//!   Finder reveal.
//! * **Linux** — `xdg-open <uri>` (or `gio open`) for shares and the
//!   parent directory for reveal.
//! * **Windows** — `explorer <uri>` for shares, `explorer /select,`
//!   for reveal.
//!
//! Without the `platform` feature every backend is a **safe stub** that
//! reports [`ShareOutcome::Unsupported`]; this crate is
//! `#![forbid(unsafe_code)]` and stays a safe, auditable dependency
//! either way.
//!
//! # Examples
//!
//! ```no_run
//! use martensite_share::{PlatformShare, ShareRequest, ShareService,
//!     default_platform_share};
//!
//! let mut s = default_platform_share();
//! // The factory always returns a usable (possibly stub) service.
//! assert!(!s.platform_name().is_empty());
//! let _ = s.share(&ShareRequest::url("https://example.com"));
//! ```

use crate::share::{ShareOutcome, ShareRequest, ShareService};

/// A [`ShareService`] backed by a specific platform share facility.
///
/// Implementations identify themselves via
/// [`platform_name`](PlatformShare::platform_name) so callers and
/// diagnostics can report which backend is active.
///
/// # Examples
///
/// ```
/// use martensite_share::{PlatformShare, StubShare};
///
/// assert_eq!(StubShare::new().platform_name(), "stub");
/// ```
pub trait PlatformShare: ShareService {
    /// Returns a human-readable backend name, e.g. `"macos-open"`,
    /// `"linux-xdg-open"`, `"windows-explorer"`, or `"stub"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_share::{PlatformShare, StubShare};
    ///
    /// assert_eq!(StubShare::new().platform_name(), "stub");
    /// ```
    fn platform_name(&self) -> &str;
}

/// A no-op share backend for environments without OS share support.
///
/// Every `share`/`reveal` returns [`ShareOutcome::Unsupported`]. This is
/// the fallback used by [`default_platform_share`] when no backend is
/// compiled in, and is useful for headless tests and CI.
///
/// # Examples
///
/// ```
/// use martensite_share::{ShareRequest, ShareService, StubShare};
///
/// let mut s = StubShare::new();
/// assert!(s.share(&ShareRequest::text("hi")).is_unsupported());
/// ```
#[derive(Default, Clone, Debug)]
pub struct StubShare;

impl StubShare {
    /// Creates a new [`StubShare`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_share::{PlatformShare, StubShare};
    ///
    /// assert_eq!(StubShare::new().platform_name(), "stub");
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

impl ShareService for StubShare {
    fn share(&mut self, _request: &ShareRequest) -> ShareOutcome {
        ShareOutcome::Unsupported("no share backend available".to_string())
    }
}

impl PlatformShare for StubShare {
    #[inline]
    fn platform_name(&self) -> &str {
        "stub"
    }
}

/// Returns the best available [`PlatformShare`] for the current target.
///
/// The selection rules are:
///
/// | Target | Backend |
/// |--------|---------|
/// | `macos` | `macos-open` (`open`/`open -R`) |
/// | `linux` | `linux-xdg-open` or `linux-gio` |
/// | `windows` | `windows-explorer` |
/// | other | [`StubShare`] |
///
/// Without the `platform` Cargo feature this returns [`StubShare`];
/// with it, the function delegates to
/// `martensite_share_platform::native_backend` and only falls back to
/// [`StubShare`] when no usable backend exists (e.g. no `xdg-open` on
/// `PATH`). [`ScriptedShare`](crate::ScriptedShare) remains the choice
/// for tests. The returned service is always usable and never panics.
///
/// # Examples
///
/// ```
/// use martensite_share::{PlatformShare, default_platform_share};
///
/// let s = default_platform_share();
/// assert!(!s.platform_name().is_empty());
/// ```
pub fn default_platform_share() -> Box<dyn PlatformShare> {
    cfg_default_platform_share()
}

#[cfg(feature = "platform")]
fn cfg_default_platform_share() -> Box<dyn PlatformShare> {
    if let Some(backend) = martensite_share_platform::native_backend() {
        return Box::new(PlatformBackendAdapter(backend));
    }
    Box::new(StubShare::new())
}

#[cfg(not(feature = "platform"))]
fn cfg_default_platform_share() -> Box<dyn PlatformShare> {
    Box::new(StubShare::new())
}

/// Adapter wrapping a `martensite_share_platform::ShareBackend` as a
/// [`PlatformShare`].
///
/// The subprocess backend trait lives in the platform crate to avoid a
/// cyclic dependency; this adapter maps the safe crate's
/// [`ShareRequest`]/[`ShareOutcome`] onto the platform crate's
/// `ShareSpec`/`ShareReply` wire types.
#[cfg(feature = "platform")]
struct PlatformBackendAdapter(Box<dyn martensite_share_platform::ShareBackend>);

#[cfg(feature = "platform")]
impl ShareService for PlatformBackendAdapter {
    fn share(&mut self, request: &ShareRequest) -> ShareOutcome {
        use martensite_share_platform::{ShareReply, ShareSpec};
        let spec = ShareSpec {
            text: request.text.clone(),
            url: request.url.clone(),
            subject: request.subject.clone(),
            files: request.files.clone(),
        };
        match self.0.share(&spec) {
            ShareReply::Shared => ShareOutcome::Shared,
            ShareReply::Unsupported(r) => ShareOutcome::Unsupported(r),
            ShareReply::Failed(e) => ShareOutcome::Failed(e),
        }
    }

    fn reveal(&mut self, path: &std::path::Path) -> ShareOutcome {
        use martensite_share_platform::ShareReply;
        match self.0.reveal(path) {
            ShareReply::Shared => ShareOutcome::Shared,
            ShareReply::Unsupported(r) => ShareOutcome::Unsupported(r),
            ShareReply::Failed(e) => ShareOutcome::Failed(e),
        }
    }
}

#[cfg(feature = "platform")]
impl PlatformShare for PlatformBackendAdapter {
    fn platform_name(&self) -> &str {
        self.0.platform_name()
    }
}
