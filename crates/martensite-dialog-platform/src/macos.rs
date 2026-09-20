//! macOS backend: `osascript` driving AppleScript `choose file` /
//! `choose folder` / `choose file name`, which presents the real
//! `NSOpenPanel` / `NSSavePanel`. User cancellation maps to a non-zero
//! exit status (AppleScript error −128) and becomes
//! [`DialogReply::Cancelled`].

use std::path::PathBuf;
use std::process::Command;

use crate::{DialogBackend, DialogReply, DialogSpec, SpecKind};

/// `osascript`-based dialog backend (`"macos-osascript"`).
pub struct OsascriptDialog;

impl OsascriptDialog {
    /// Creates the backend. `osascript` is in-box on every macOS system.
    pub fn new() -> Self {
        Self
    }
}

impl Default for OsascriptDialog {
    fn default() -> Self {
        Self::new()
    }
}

/// Escape a string for interpolation inside an AppleScript double-quoted
/// literal.
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Build the `of type {…}` / `default location …` / `with prompt …`
/// clause suffix shared by the `choose` verbs.
fn clauses(spec: &DialogSpec, allow_multi: bool) -> String {
    let mut c = String::new();
    if !spec.title.is_empty() {
        c.push_str(&format!(" with prompt \"{}\"", esc(&spec.title)));
    }
    if !spec.filters.is_empty() {
        let exts = spec
            .filters
            .iter()
            .flat_map(|(_, es)| es.iter())
            .map(|e| format!("\"{}\"", esc(e)))
            .collect::<Vec<_>>()
            .join(",");
        c.push_str(&format!(" of type {{{exts}}}"));
    }
    if let Some(dir) = &spec.start_dir {
        c.push_str(&format!(
            " default location POSIX file \"{}\"",
            esc(&dir.to_string_lossy())
        ));
    }
    if allow_multi {
        c.push_str(" with multiple selections allowed");
    }
    c
}

/// Run an osascript script (one `-e` per line); `Some(stdout)` on
/// success, `None` on cancel/error.
fn run(lines: &[String]) -> Option<String> {
    let mut cmd = Command::new("osascript");
    for line in lines {
        cmd.arg("-e").arg(line);
    }
    let out = cmd.output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        None
    }
}

impl DialogBackend for OsascriptDialog {
    fn show(&mut self, spec: &DialogSpec) -> DialogReply {
        match spec.kind {
            SpecKind::OpenFile | SpecKind::PickFolder => {
                let verb = if spec.kind == SpecKind::PickFolder {
                    "choose folder"
                } else {
                    "choose file"
                };
                let line = format!("POSIX path of ({verb}{})", clauses(spec, false));
                match run(&[line]) {
                    Some(out) => {
                        let p = out.trim_end_matches('\n').to_string();
                        if p.is_empty() {
                            DialogReply::Cancelled
                        } else {
                            DialogReply::Picked(vec![PathBuf::from(p)])
                        }
                    }
                    None => DialogReply::Cancelled,
                }
            }
            SpecKind::OpenFiles => {
                let lines = vec![
                    format!("set sel to choose file{}", clauses(spec, true)),
                    "set out to \"\"".to_string(),
                    "repeat with f in sel".to_string(),
                    "set out to out & POSIX path of f & \"\\n\"".to_string(),
                    "end repeat".to_string(),
                    "return out".to_string(),
                ];
                match run(&lines) {
                    Some(out) => {
                        let paths: Vec<PathBuf> = out
                            .lines()
                            .filter(|l| !l.is_empty())
                            .map(PathBuf::from)
                            .collect();
                        if paths.is_empty() {
                            DialogReply::Cancelled
                        } else {
                            DialogReply::Picked(paths)
                        }
                    }
                    None => DialogReply::Cancelled,
                }
            }
            SpecKind::SaveFile => {
                let mut line = format!("choose file name{}", clauses(spec, false));
                if let Some(name) = &spec.default_name {
                    line.push_str(&format!(" default name \"{}\"", esc(name)));
                }
                let script = format!("POSIX path of ({line})");
                match run(&[script]) {
                    Some(out) => {
                        let p = out.trim_end_matches('\n').to_string();
                        if p.is_empty() {
                            DialogReply::Cancelled
                        } else {
                            DialogReply::Picked(vec![PathBuf::from(p)])
                        }
                    }
                    None => DialogReply::Cancelled,
                }
            }
        }
    }

    fn platform_name(&self) -> &str {
        "macos-osascript"
    }
}
