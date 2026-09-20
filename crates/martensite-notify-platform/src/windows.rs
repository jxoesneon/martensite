//! Windows backend: PowerShell `Windows.UI.Notifications` toast via the
//! `ToastText02` XML template (title + body).

use std::process::Command;

use crate::{NotifyBackend, NotifySpec};

/// PowerShell toast notifier (`"windows-toast"`).
pub struct ToastNotifier;

impl ToastNotifier {
    /// Creates the backend. PowerShell and the toast manager are in-box
    /// on Windows 10+.
    pub fn new() -> Self {
        Self
    }
}

impl Default for ToastNotifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Escape a string for interpolation inside a PowerShell single-quoted
/// literal (single quotes double up).
fn esc(s: &str) -> String {
    s.replace('\'', "''")
}

fn build_script(spec: &NotifySpec) -> String {
    format!(
        "[Windows.UI.Notifications.ToastNotificationManager, \
         Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null;\
         $t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent(\
         [Windows.UI.Notifications.ToastTemplateType]::ToastText02);\
         $t.GetElementsByTagName('text').Item(0).AppendChild(\
         $t.CreateTextNode('{}')) | Out-Null;\
         $t.GetElementsByTagName('text').Item(1).AppendChild(\
         $t.CreateTextNode('{}')) | Out-Null;\
         $n = New-Object Windows.UI.Notifications.ToastNotification $t;\
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier(\
         'Martensite').Show($n)",
        esc(&spec.title),
        esc(&spec.body)
    )
}

impl NotifyBackend for ToastNotifier {
    fn notify(&mut self, spec: &NotifySpec) -> Result<(), String> {
        Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(build_script(spec))
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
        "windows-toast"
    }
}
