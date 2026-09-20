//! Windows backend: PowerShell `System.Windows.Forms` — `OpenFileDialog`,
//! `SaveFileDialog`, and `FolderBrowserDialog`. The script writes the
//! selected path(s) to stdout (newline-separated for multi-select); a
//! non-OK result exits silently and maps to [`DialogReply::Cancelled`].

use std::path::PathBuf;
use std::process::Command;

use crate::{DialogBackend, DialogReply, DialogSpec, SpecKind};

/// PowerShell `System.Windows.Forms` backend (`"windows-forms"`).
pub struct WinFormsDialog;

impl WinFormsDialog {
    /// Creates the backend. PowerShell is in-box on every supported
    /// Windows release.
    pub fn new() -> Self {
        Self
    }
}

impl Default for WinFormsDialog {
    fn default() -> Self {
        Self::new()
    }
}

/// Escape a string for interpolation inside a PowerShell single-quoted
/// literal (single quotes double up).
fn esc(s: &str) -> String {
    s.replace('\'', "''")
}

/// Build the WinForms `Filter` string: `"Name (*.a;*.b)|*.a;*.b"`.
fn filter_string(spec: &DialogSpec) -> String {
    let mut parts: Vec<String> = spec
        .filters
        .iter()
        .map(|(n, es)| {
            let pats = es
                .iter()
                .map(|e| format!("*.{e}"))
                .collect::<Vec<_>>()
                .join(";");
            format!("{n} ({pats})|{pats}")
        })
        .collect();
    parts.push("All files (*.*)|*.*".to_string());
    parts.join("|")
}

fn build_script(spec: &DialogSpec) -> String {
    let mut s = String::from("Add-Type -AssemblyName System.Windows.Forms;");
    match spec.kind {
        SpecKind::PickFolder => {
            s.push_str("$d = New-Object System.Windows.Forms.FolderBrowserDialog;");
            if !spec.title.is_empty() {
                s.push_str(&format!("$d.Description = '{}';", esc(&spec.title)));
            }
            if let Some(dir) = &spec.start_dir {
                s.push_str(&format!(
                    "$d.SelectedPath = '{}';",
                    esc(&dir.to_string_lossy())
                ));
            }
            s.push_str(
                "if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) \
                 { $d.SelectedPath }",
            );
        }
        SpecKind::OpenFile | SpecKind::OpenFiles | SpecKind::SaveFile => {
            let class = if spec.kind == SpecKind::SaveFile {
                "SaveFileDialog"
            } else {
                "OpenFileDialog"
            };
            s.push_str(&format!("$d = New-Object System.Windows.Forms.{class};"));
            if !spec.title.is_empty() {
                s.push_str(&format!("$d.Title = '{}';", esc(&spec.title)));
            }
            if let Some(dir) = &spec.start_dir {
                s.push_str(&format!(
                    "$d.InitialDirectory = '{}';",
                    esc(&dir.to_string_lossy())
                ));
            }
            s.push_str(&format!("$d.Filter = '{}';", esc(&filter_string(spec))));
            match spec.kind {
                SpecKind::OpenFiles => s.push_str("$d.Multiselect = $true;"),
                SpecKind::SaveFile => {
                    s.push_str("$d.OverwritePrompt = $true;");
                    if let Some(name) = &spec.default_name {
                        s.push_str(&format!("$d.FileName = '{}';", esc(name)));
                    }
                }
                _ => {}
            }
            if spec.kind == SpecKind::OpenFiles {
                s.push_str(
                    "if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) \
                     { $d.FileNames -join \"`n\" }",
                );
            } else {
                s.push_str(
                    "if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) \
                     { $d.FileName }",
                );
            }
        }
    }
    s
}

impl DialogBackend for WinFormsDialog {
    fn show(&mut self, spec: &DialogSpec) -> DialogReply {
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(build_script(spec))
            .output();
        let paths: Vec<PathBuf> = out
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
            .lines()
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

    fn platform_name(&self) -> &str {
        "windows-forms"
    }
}
