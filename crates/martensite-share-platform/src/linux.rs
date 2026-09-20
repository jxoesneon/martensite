//! Linux backends: `xdg-open` (freedesktop URI dispatch) or `gio open`
//! (GLib/GIO), whichever is on `PATH`. Both hand the URI to its
//! registered handler; reveal opens the parent directory since neither
//! tool supports select-in-folder.

use std::process::Command;

use crate::{share_uri, ShareBackend, ShareReply, ShareSpec};

/// `xdg-open`-based backend (`"linux-xdg-open"`).
pub struct XdgShare;

/// `gio open`-based backend (`"linux-gio"`).
pub struct GioShare;

/// Returns the first usable Linux backend: `xdg-open` if present, else
/// `gio`, else `None`.
pub fn select_backend() -> Option<Box<dyn ShareBackend>> {
    if tool_on_path("xdg-open") {
        Some(Box::new(XdgShare))
    } else if tool_on_path("gio") {
        Some(Box::new(GioShare))
    } else {
        None
    }
}

/// `true` when `tool` is found on `PATH` (probed via `tool --version`;
/// `gio` answers `--version`, `xdg-open` answers `--version` since
/// xdg-utils 1.1 — an older xdg-open still passes the probe when its
/// usage text exits 0).
fn tool_on_path(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn spawn(tool: &str, args: &[&std::ffi::OsStr]) -> ShareReply {
    match Command::new(tool).args(args).status() {
        Ok(s) if s.success() => ShareReply::Shared,
        Ok(s) => ShareReply::Failed(format!("{tool} exited {s}")),
        Err(e) => ShareReply::Failed(format!("spawn {tool}: {e}")),
    }
}

impl ShareBackend for XdgShare {
    fn share(&mut self, spec: &ShareSpec) -> ShareReply {
        match share_uri(spec) {
            Ok(uri) => spawn("xdg-open", &[std::ffi::OsStr::new(&uri)]),
            Err(r) => r,
        }
    }

    fn reveal(&mut self, path: &std::path::Path) -> ShareReply {
        // No select-in-folder support — open the containing directory.
        let dir = path.parent().unwrap_or(path);
        spawn("xdg-open", &[dir.as_os_str()])
    }

    fn platform_name(&self) -> &str {
        "linux-xdg-open"
    }
}

impl ShareBackend for GioShare {
    fn share(&mut self, spec: &ShareSpec) -> ShareReply {
        match share_uri(spec) {
            Ok(uri) => spawn(
                "gio",
                &[std::ffi::OsStr::new("open"), std::ffi::OsStr::new(&uri)],
            ),
            Err(r) => r,
        }
    }

    fn reveal(&mut self, path: &std::path::Path) -> ShareReply {
        let dir = path.parent().unwrap_or(path);
        spawn("gio", &[std::ffi::OsStr::new("open"), dir.as_os_str()])
    }

    fn platform_name(&self) -> &str {
        "linux-gio"
    }
}
