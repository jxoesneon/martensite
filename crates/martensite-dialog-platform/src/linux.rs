//! Linux backends: `zenity --file-selection` (GTK chooser) or `kdialog`
//! (Qt chooser), whichever is on `PATH`. Both return the selected path(s)
//! on stdout and exit non-zero on cancel, which maps to
//! [`DialogReply::Cancelled`].

use std::path::PathBuf;
use std::process::Command;

use crate::{DialogBackend, DialogReply, DialogSpec, SpecKind};

/// `zenity`-based backend (`"linux-zenity"`).
pub struct ZenityDialog;

/// `kdialog`-based backend (`"linux-kdialog"`).
pub struct KdialogDialog;

/// Returns the first usable Linux backend: `zenity` if present, else
/// `kdialog`, else `None`.
pub fn select_backend() -> Option<Box<dyn DialogBackend>> {
    if tool_on_path("zenity") {
        Some(Box::new(ZenityDialog))
    } else if tool_on_path("kdialog") {
        Some(Box::new(KdialogDialog))
    } else {
        None
    }
}

/// `true` when `tool` is found on `PATH` (probed via `tool --version`).
fn tool_on_path(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run(mut cmd: Command) -> Option<String> {
    let out = cmd.output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        None
    }
}

fn to_reply(out: Option<String>, sep: char) -> DialogReply {
    let paths: Vec<PathBuf> = out
        .unwrap_or_default()
        .split(sep)
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect();
    if paths.is_empty() {
        DialogReply::Cancelled
    } else {
        DialogReply::Picked(paths)
    }
}

impl DialogBackend for ZenityDialog {
    fn show(&mut self, spec: &DialogSpec) -> DialogReply {
        let mut cmd = Command::new("zenity");
        cmd.arg("--file-selection");
        if !spec.title.is_empty() {
            cmd.arg(format!("--title={}", spec.title));
        }
        match spec.kind {
            SpecKind::OpenFile => {}
            SpecKind::OpenFiles => {
                cmd.arg("--multiple").arg("--separator=\n");
            }
            SpecKind::PickFolder => {
                cmd.arg("--directory");
            }
            SpecKind::SaveFile => {
                cmd.arg("--save").arg("--confirm-overwrite");
            }
        }
        // --filename sets both the initial directory and the suggested
        // save name (a trailing "/" forces directory mode).
        let filename = match spec.kind {
            SpecKind::SaveFile => spec
                .start_dir
                .as_ref()
                .map(|d| d.join(spec.default_name.as_deref().unwrap_or("")))
                .or_else(|| spec.default_name.as_ref().map(PathBuf::from)),
            _ => spec.start_dir.clone().map(|d| d.join("")),
        };
        if let Some(f) = filename {
            cmd.arg(format!("--filename={}", f.to_string_lossy()));
        }
        for (name, exts) in &spec.filters {
            let pats = exts
                .iter()
                .map(|e| format!("*.{e}"))
                .collect::<Vec<_>>()
                .join(" ");
            cmd.arg(format!("--file-filter={name} | {pats}"));
        }
        to_reply(run(cmd), '\n')
    }

    fn platform_name(&self) -> &str {
        "linux-zenity"
    }
}

impl DialogBackend for KdialogDialog {
    fn show(&mut self, spec: &DialogSpec) -> DialogReply {
        let mut cmd = Command::new("kdialog");
        let start = spec
            .start_dir
            .as_ref()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        let filter = spec
            .filters
            .iter()
            .map(|(n, es)| {
                format!(
                    "{}|{}",
                    es.iter()
                        .map(|e| format!("*.{e}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    n
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        match spec.kind {
            SpecKind::OpenFile => {
                cmd.arg("--getopenfilename").arg(&start).arg(&filter);
            }
            SpecKind::OpenFiles => {
                cmd.arg("--getopenfilename")
                    .arg(&start)
                    .arg(&filter)
                    .arg("--multiple")
                    .arg("--separate-output");
            }
            SpecKind::PickFolder => {
                cmd.arg("--getexistingdirectory").arg(&start);
            }
            SpecKind::SaveFile => {
                let mut dir = start;
                if let Some(name) = &spec.default_name {
                    dir = format!("{dir}/{name}");
                }
                cmd.arg("--getsavefilename").arg(dir).arg(&filter);
            }
        }
        if !spec.title.is_empty() {
            cmd.arg("--title").arg(&spec.title);
        }
        to_reply(run(cmd), '\n')
    }

    fn platform_name(&self) -> &str {
        "linux-kdialog"
    }
}
