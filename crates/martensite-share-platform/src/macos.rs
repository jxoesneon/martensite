//! macOS backend: `open` for URI dispatch, `open -R` for Finder reveal.
//!
//! `open <uri>` hands the URI to its registered handler (browser,
//! Mail, Messages, custom-scheme apps); `open -R <path>` selects the
//! file in Finder. Both return immediately — the handler runs in its
//! own process — so `share` is fire-and-forget: a spawn failure maps
//! to [`ShareReply::Failed`], a clean spawn to [`ShareReply::Shared`]
//! (whether the handler then succeeds is out of band).

use std::process::Command;

use crate::{share_uri, ShareBackend, ShareReply, ShareSpec};

/// `open`-based backend (`"macos-open"`).
pub struct MacosShare;

impl MacosShare {
    /// Creates the backend. `open` is in-box on every macOS release.
    pub fn new() -> Self {
        Self
    }
}

impl Default for MacosShare {
    fn default() -> Self {
        Self::new()
    }
}

fn spawn_open(args: &[&std::ffi::OsStr]) -> ShareReply {
    match Command::new("open").args(args).status() {
        Ok(s) if s.success() => ShareReply::Shared,
        Ok(s) => ShareReply::Failed(format!("open exited {s}")),
        Err(e) => ShareReply::Failed(format!("spawn open: {e}")),
    }
}

impl ShareBackend for MacosShare {
    fn share(&mut self, spec: &ShareSpec) -> ShareReply {
        match share_uri(spec) {
            Ok(uri) => spawn_open(&[std::ffi::OsStr::new(&uri)]),
            Err(r) => r,
        }
    }

    fn reveal(&mut self, path: &std::path::Path) -> ShareReply {
        spawn_open(&[std::ffi::OsStr::new("-R"), path.as_os_str()])
    }

    fn platform_name(&self) -> &str {
        "macos-open"
    }
}
