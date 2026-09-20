//! Windows backend: PowerShell printing.
//!
//! Enumeration uses `Get-CimInstance Win32_Printer` (the `Default`
//! property marks the system default queue). Submission:
//!
//! * `SpecSource::Bytes` — the payload is decoded as UTF-8 (lossy) and
//!   piped to `Out-Printer`. Non-text payloads degrade to mojibake;
//!   callers printing binary formats should stage a file and use
//!   `SpecSource::File` instead.
//! * `SpecSource::File` — `Start-Process -Verb Print` hands the file to
//!   the associated application's print verb.
//!
//! `PrintSpec` layout options (copies, page range, duplex, media) are
//! best-effort: `Out-Printer` exposes only `-Name`, and `Print` verbs
//! ignore them entirely — the driver defaults apply.

use std::process::Command;

use crate::{BackendPrinter, PrintBackend, PrintReply, PrintSpec, SpecSource};

/// PowerShell backend (`"windows-powershell"`).
pub struct PowerShellPrinter;

impl PowerShellPrinter {
    /// Creates the backend. PowerShell is in-box on every supported
    /// Windows release.
    pub fn new() -> Self {
        Self
    }
}

impl Default for PowerShellPrinter {
    fn default() -> Self {
        Self::new()
    }
}

/// Escape a string for interpolation inside a PowerShell single-quoted
/// literal (single quotes double up).
fn esc(s: &str) -> String {
    s.replace('\'', "''")
}

fn run(script: &str) -> Result<String, String> {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(script)
        .output()
        .map_err(|e| format!("spawn powershell: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

impl PrintBackend for PowerShellPrinter {
    fn printers(&mut self) -> Vec<BackendPrinter> {
        // Name<tab>Default — parse each line into (name, is_default).
        let script = "Get-CimInstance Win32_Printer | \
                      ForEach-Object { \"$($_.Name)`t$($_.Default)\" }";
        run(script)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let (name, def) = l.split_once('\t')?;
                let name = name.trim();
                (!name.is_empty()).then(|| BackendPrinter {
                    name: name.to_string(),
                    description: String::new(),
                    is_default: def.trim().eq_ignore_ascii_case("true"),
                })
            })
            .collect()
    }

    fn print(&mut self, spec: &PrintSpec) -> PrintReply {
        let script = match &spec.source {
            SpecSource::File(path) => {
                let p = esc(&path.to_string_lossy());
                // `PrintTo` accepts a destination; plain `Print` goes
                // to the default queue when none is named.
                match &spec.printer {
                    Some(d) => format!(
                        "Start-Process -FilePath '{p}' -Verb PrintTo -ArgumentList '{}' -WindowStyle Hidden -Wait",
                        esc(d)
                    ),
                    None => format!(
                        "Start-Process -FilePath '{p}' -Verb Print -WindowStyle Hidden -Wait"
                    ),
                }
            }
            SpecSource::Bytes { data, .. } => {
                let text = String::from_utf8_lossy(data);
                let dest = match &spec.printer {
                    Some(d) => format!(" -Name '{}'", esc(d)),
                    None => String::new(),
                };
                format!("'{}' | Out-Printer{dest}", esc(&text))
            }
        };
        // Out-Printer / Start-Process produce no job id.
        match run(&script) {
            Ok(_) => PrintReply::Submitted { job_id: None },
            Err(e) => PrintReply::Failed(e),
        }
    }

    fn platform_name(&self) -> &str {
        "windows-powershell"
    }
}
