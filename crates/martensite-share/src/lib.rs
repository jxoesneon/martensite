//! Share / reveal-in-folder abstraction.
//!
//! `martensite-share` provides a platform-agnostic model for sharing
//! content — a [`ShareRequest`] (text, URI, files, subject) dispatched
//! through a [`ShareService`] — so widgets and application code never
//! touch platform APIs directly.
//!
//! # Architecture
//!
//! * [`ShareRequest`] describes one share: `url` (opened in the
//!   platform handler — `https:`, `mailto:`, `tel:`, custom schemes),
//!   `text` (drafted as a mailto message), `files`, and `subject`.
//! * [`ShareOutcome`] is `Shared`, `Unsupported(payload)`, or
//!   `Failed(reason)`.
//! * [`ShareService`] is the dispatch contract; it also exposes
//!   [`reveal`](ShareService::reveal) for showing files in the platform
//!   file manager. [`ScriptedShare`] is a deterministic canned-response
//!   implementation for tests and headless environments.
//! * [`PlatformShare`] extends [`ShareService`] with a backend name.
//!   [`default_platform_share`] selects the best available backend for
//!   the current target. Because this crate is `#![forbid(unsafe_code)]`,
//!   native dispatch is delegated to `martensite-share-platform` behind
//!   the `platform` Cargo feature; without it, every backend is a safe
//!   stub (see the [`platform`] module docs).
//!
//! # Examples
//!
//! ```
//! use martensite_share::{ShareRequest, ShareService, ScriptedShare};
//!
//! let mut share = ScriptedShare::new();
//! let req = ShareRequest::url("mailto:team@example.com?subject=Hi");
//! assert!(share.share(&req).is_shared());
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod platform;
pub mod share;

pub use platform::{default_platform_share, PlatformShare, StubShare};
pub use share::{ScriptedShare, ShareOutcome, ShareRequest, ShareService};

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(feature = "platform"))]
    use std::path::Path;

    #[test]
    fn re_exported_scripted_round_trip() {
        let mut s = ScriptedShare::new();
        s.respond_with(ShareOutcome::Unsupported("files".into()));
        assert!(s.share(&ShareRequest::files(["/a"])).is_unsupported());
        assert!(s.share(&ShareRequest::url("https://x")).is_shared());
    }

    #[test]
    fn re_exported_default_platform_share_is_usable() {
        let s = default_platform_share();
        assert!(!s.platform_name().is_empty());
    }

    #[test]
    #[cfg(not(feature = "platform"))]
    fn default_platform_share_unsupported_without_backend() {
        let mut s = default_platform_share();
        assert!(s.share(&ShareRequest::text("t")).is_unsupported());
        assert!(s.reveal(Path::new("/x")).is_unsupported());
    }

    #[test]
    fn re_exported_stub_is_unsupported() {
        let mut s = StubShare::new();
        assert!(s.share(&ShareRequest::url("https://x")).is_unsupported());
    }
}
