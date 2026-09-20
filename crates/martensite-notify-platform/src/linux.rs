//! Linux backend: `notify-send` (freedesktop notification daemon).
//! `urgency` maps to `--urgency=low|normal|critical`; `sound` maps to the
//! `sound-name` hint.

use std::process::Command;

use crate::{NotifyBackend, NotifySpec, SpecUrgency};

/// `notify-send`-based notifier (`"linux-notify-send"`).
pub struct NotifySend;

/// `Some` when `notify-send` is on `PATH`, else `None`.
pub fn select_backend() -> Option<Box<dyn NotifyBackend>> {
    if Command::new("notify-send")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        Some(Box::new(NotifySend))
    } else {
        None
    }
}

impl NotifyBackend for NotifySend {
    fn notify(&mut self, spec: &NotifySpec) -> Result<(), String> {
        let mut cmd = Command::new("notify-send");
        cmd.arg(match spec.urgency {
            SpecUrgency::Low => "--urgency=low",
            SpecUrgency::Normal => "--urgency=normal",
            SpecUrgency::Critical => "--urgency=critical",
        });
        if let Some(sound) = &spec.sound {
            cmd.arg(format!("--hint=string:sound-name:{sound}"));
        }
        cmd.arg(&spec.title).arg(&spec.body);
        cmd.output().map_err(|e| e.to_string()).and_then(|o| {
            if o.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&o.stderr).into_owned())
            }
        })
    }

    fn platform_name(&self) -> &str {
        "linux-notify-send"
    }
}
