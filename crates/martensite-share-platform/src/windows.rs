//! Windows backend: `cmd /c start` for URI dispatch,
//! `explorer /select,` for Explorer reveal.
//!
//! `start "" "<uri>"` hands the URI to its registered handler (browser,
//! Mail, custom-scheme apps). `explorer /select,<path>` selects the
//! file in Explorer — `explorer`'s exit code is unreliable (it often
//! reports 1 on success), so reveal treats a clean *spawn* as
//! [`ShareReply::Shared`]. Both return immediately: the handler runs
//! in its own process.

use std::process::Command;

use crate::{share_uri, ShareBackend, ShareReply, ShareSpec};

/// `cmd`/`explorer`-based backend (`"windows-explorer"`).
pub struct ExplorerShare;

impl ExplorerShare {
    /// Creates the backend. `cmd` and `explorer` are in-box on every
    /// supported Windows release.
    pub fn new() -> Self {
        Self
    }
}

impl Default for ExplorerShare {
    fn default() -> Self {
        Self::new()
    }
}

impl ShareBackend for ExplorerShare {
    fn share(&mut self, spec: &ShareSpec) -> ShareReply {
        let uri = match share_uri(spec) {
            Ok(u) => u,
            Err(r) => return r,
        };
        // `start` is a cmd builtin; the first quoted arg is the window
        // title, so an empty "" must precede the URI.
        match Command::new("cmd").args(["/c", "start", "", &uri]).status() {
            Ok(s) if s.success() => ShareReply::Shared,
            Ok(s) => ShareReply::Failed(format!("cmd start exited {s}")),
            Err(e) => ShareReply::Failed(format!("spawn cmd: {e}")),
        }
    }

    fn reveal(&mut self, path: &std::path::Path) -> ShareReply {
        match Command::new("explorer")
            .arg(format!("/select,{}", path.to_string_lossy()))
            .status()
        {
            // Explorer's exit code is unreliable — a clean spawn is
            // the best signal available.
            Ok(_) => ShareReply::Shared,
            Err(e) => ShareReply::Failed(format!("spawn explorer: {e}")),
        }
    }

    fn platform_name(&self) -> &str {
        "windows-explorer"
    }
}
