//! CUPS backend for macOS and Linux.
//!
//! Printer enumeration uses `lpstat -p` for queue names and
//! `lpstat -d` for the system default. Job submission uses `lp` with
//! `-d` (destination), `-n` (copies), `-t` (title), `-P` (page list)
//! and `-o` option strings for media, duplex, orientation, and color
//! mode. Byte payloads are staged to a temp file whose extension hints
//! the format (`pdf` → `.pdf`, otherwise `.txt`), since `lp` accepts
//! a filename argument more reliably across CUPS versions than stdin.

use std::io::Write;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{BackendPrinter, PrintBackend, PrintReply, PrintSpec, SpecSides, SpecSource};

/// CUPS backend (`"cups"`) — `lp`/`lpstat` subprocesses.
pub struct CupsPrinter;

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

impl CupsPrinter {
    /// `Some` when `lp` is on `PATH` (probed via `lp -h`), else `None`.
    pub fn probe() -> Option<Box<dyn PrintBackend>> {
        let ok = Command::new("lp")
            .arg("-h")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        ok.then(|| Box::new(CupsPrinter) as _)
    }
}

fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd
        .output()
        .map_err(|e| format!("spawn {}: {e}", cmd.get_program().to_string_lossy()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Stage `data` in a temp file; `mime` picks the extension so CUPS
/// auto-detection routes it to the right filter. Removed by the caller.
fn stage_temp(data: &[u8], mime: Option<&str>) -> Result<std::path::PathBuf, String> {
    let ext = match mime {
        Some("application/pdf") => "pdf",
        Some("text/html") => "html",
        _ => "txt",
    };
    let path = std::env::temp_dir().join(format!(
        "martensite-print-{}-{}.{ext}",
        std::process::id(),
        TEMP_SEQ.fetch_add(1, Ordering::Relaxed),
    ));
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(data))
        .map_err(|e| format!("stage temp file: {e}"))?;
    Ok(path)
}

/// Parse the CUPS `request id is <id> (…)` banner for the job id.
fn parse_job_id(stdout: &str) -> Option<String> {
    stdout
        .split("request id is ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .map(str::to_string)
}

impl PrintBackend for CupsPrinter {
    fn printers(&mut self) -> Vec<BackendPrinter> {
        let default = Command::new("lpstat")
            .arg("-d")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                // "system default destination: Office"
                String::from_utf8_lossy(&o.stdout)
                    .split("destination: ")
                    .nth(1)
                    .map(|s| s.trim().to_string())
            });
        let list = Command::new("lpstat")
            .arg("-p")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        // Lines look like: "printer Office is idle.  enabled since …"
        list.lines()
            .filter_map(|l| {
                l.strip_prefix("printer ")
                    .and_then(|r| r.split_whitespace().next())
                    .map(str::to_string)
            })
            .map(|name| BackendPrinter {
                is_default: default.as_deref() == Some(name.as_str()),
                name,
                description: String::new(),
            })
            .collect()
    }

    fn print(&mut self, spec: &PrintSpec) -> PrintReply {
        // Byte payloads go through a staged temp file; removed after
        // `lp` returns (submission copies the file into the spool).
        let (path, staged) = match &spec.source {
            SpecSource::File(p) => (p.clone(), None),
            SpecSource::Bytes { data, mime } => match stage_temp(data, mime.as_deref()) {
                Ok(p) => (p.clone(), Some(p)),
                Err(e) => return PrintReply::Failed(e),
            },
        };

        let mut cmd = Command::new("lp");
        if let Some(dest) = &spec.printer {
            cmd.arg("-d").arg(dest);
        }
        cmd.arg("-n").arg(spec.copies.to_string());
        if !spec.title.is_empty() {
            cmd.arg("-t").arg(&spec.title);
        }
        if let Some((first, last)) = spec.pages {
            let list = if first == last {
                first.to_string()
            } else {
                format!("{first}-{last}")
            };
            cmd.arg("-P").arg(list);
        }
        cmd.arg("-o").arg(format!("media={}", spec.media));
        if spec.landscape {
            cmd.arg("-o").arg("landscape");
        }
        match spec.sides {
            SpecSides::Default => {}
            SpecSides::OneSided => {
                cmd.arg("-o").arg("sides=one-sided");
            }
            SpecSides::LongEdge => {
                cmd.arg("-o").arg("sides=two-sided-long-edge");
            }
            SpecSides::ShortEdge => {
                cmd.arg("-o").arg("sides=two-sided-short-edge");
            }
        }
        if !spec.color {
            cmd.arg("-o").arg("print-color-mode=monochrome");
        }
        cmd.arg(&path);

        let reply = match run(&mut cmd) {
            Ok(stdout) => PrintReply::Submitted {
                job_id: parse_job_id(&stdout),
            },
            Err(e) => PrintReply::Failed(e),
        };
        if let Some(p) = staged {
            let _ = std::fs::remove_file(p);
        }
        reply
    }

    fn platform_name(&self) -> &str {
        "cups"
    }
}
