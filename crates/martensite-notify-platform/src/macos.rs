//! macOS backend: `osascript` `display notification`, which posts to
//! Notification Center. `subtitle` and `sound name` clauses map the
//! macOS-only fields.

use std::process::Command;

use crate::{NotifyBackend, NotifySpec};

/// `osascript`-based notifier (`"macos-osascript"`).
pub struct OsascriptNotifier;

impl OsascriptNotifier {
    /// Creates the backend. `osascript` is in-box on every macOS system.
    pub fn new() -> Self {
        Self
    }
}

impl Default for OsascriptNotifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Escape a string for interpolation inside an AppleScript double-quoted
/// literal.
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

impl NotifyBackend for OsascriptNotifier {
    fn notify(&mut self, spec: &NotifySpec) -> Result<(), String> {
        let mut line = format!(
            "display notification \"{}\" with title \"{}\"",
            esc(&spec.body),
            esc(&spec.title)
        );
        if !spec.subtitle.is_empty() {
            line.push_str(&format!(" subtitle \"{}\"", esc(&spec.subtitle)));
        }
        if let Some(sound) = &spec.sound {
            line.push_str(&format!(" sound name \"{}\"", esc(sound)));
        }
        Command::new("osascript")
            .arg("-e")
            .arg(line)
            .output()
            .map_err(|e| e.to_string())
            .and_then(|o| {
                if o.status.success() {
                    Ok(())
                } else {
                    Err(String::from_utf8_lossy(&o.stderr).into_owned())
                }
            })
    }

    fn platform_name(&self) -> &str {
        "macos-osascript"
    }
}
