//! `SystemBrowserWebView` — a [`WebViewHost`] that dispatches
//! navigations to the OS default browser (`open`, `xdg-open`,
//! `rundll32 url.dll,FileProtocolHandler`).
//!
//! It owns a real history stack and emits `LoadStarted`→`LoadFinished`
//! immediately — dispatch is fire-and-forget, so "finished" means
//! "handed to the OS", not "page loaded". The title is the URL (the
//! OS gives no document feedback). This is the honest fallback when
//! `curl` is unavailable — real OS integration, no fake lifecycle.
//!
//! # Examples
//!
//! ```no_run
//! use martensite_webview_platform::SystemBrowserWebView;
//! use martensite_webview::WebViewHost;
//!
//! let mut wv = SystemBrowserWebView::new();
//! wv.navigate("https://example.com"); // opens in the default browser
//! assert_eq!(wv.state().url, "https://example.com");
//! ```

use martensite_webview::{WebViewCommand, WebViewEvent, WebViewHost, WebViewState};
use std::collections::VecDeque;
#[cfg(any(unix, target_os = "windows"))]
use std::process::Command;

/// A system-browser [`WebViewHost`] — see the module docs.
///
/// ```no_run
/// use martensite_webview_platform::SystemBrowserWebView;
/// use martensite_webview::WebViewHost;
///
/// assert_eq!(SystemBrowserWebView::new().backend_name(), "system-browser");
/// ```
pub struct SystemBrowserWebView {
    history: Vec<String>,
    index: usize,
    zoom: f32,
    /// `1.0` after a successful dispatch, `0.0` before any.
    progress: f32,
    /// The OS gives no document feedback — the title is the URL of
    /// the last successful dispatch, tracked for `TitleChanged`.
    title: String,
    next_call: u64,
    events: VecDeque<WebViewEvent>,
    /// Dispatch failures surface as `LoadFailed` + `state().error`.
    error: Option<String>,
}

impl std::fmt::Debug for SystemBrowserWebView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemBrowserWebView")
            .field("url", &self.current_url())
            .finish()
    }
}

impl Default for SystemBrowserWebView {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemBrowserWebView {
    /// A browser host with empty history.
    ///
    /// ```no_run
    /// use martensite_webview_platform::SystemBrowserWebView;
    /// use martensite_webview::WebViewHost;
    ///
    /// assert_eq!(SystemBrowserWebView::new().state().url, "");
    /// ```
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            index: 0,
            zoom: 1.0,
            progress: 0.0,
            title: String::new(),
            next_call: 1,
            events: VecDeque::new(),
            error: None,
        }
    }

    fn current_url(&self) -> String {
        self.history.get(self.index).cloned().unwrap_or_default()
    }

    fn dispatch(&mut self, url: String, push_history: bool) {
        if push_history {
            self.history.truncate(self.index + 1);
            self.history.push(url.clone());
            self.index = self.history.len() - 1;
        }
        self.error = None;
        // Event order matches the simulated engine's `commit_load`:
        // UrlChanged (nav only) → LoadStarted → LoadProgress{1.0} →
        // LoadFinished → TitleChanged.
        if push_history {
            self.events.push_back(WebViewEvent::UrlChanged(url.clone()));
        }
        self.events
            .push_back(WebViewEvent::LoadStarted { url: url.clone() });
        match open_url(&url) {
            Ok(()) => {
                self.progress = 1.0;
                self.events
                    .push_back(WebViewEvent::LoadProgress { progress: 1.0 });
                self.events
                    .push_back(WebViewEvent::LoadFinished { url: url.clone() });
                if url != self.title {
                    self.title = url;
                    self.events
                        .push_back(WebViewEvent::TitleChanged(self.title.clone()));
                }
            }
            Err(e) => {
                self.progress = 0.0;
                self.error = Some(e.clone());
                self.events
                    .push_back(WebViewEvent::LoadFailed { url, error: e });
            }
        }
    }
}

impl WebViewHost for SystemBrowserWebView {
    fn command(&mut self, cmd: WebViewCommand) {
        match cmd {
            WebViewCommand::Navigate(url) => self.dispatch(url, true),
            WebViewCommand::GoBack if self.index > 0 => {
                self.index -= 1;
                let url = self.current_url();
                self.dispatch(url, false);
            }
            WebViewCommand::GoForward if self.index + 1 < self.history.len() => {
                self.index += 1;
                let url = self.current_url();
                self.dispatch(url, false);
            }
            WebViewCommand::Reload if !self.history.is_empty() => {
                let url = self.current_url();
                self.dispatch(url, false);
            }
            WebViewCommand::SetZoom(z) => self.zoom = z.clamp(0.25, 5.0),
            _ => {}
        }
    }

    fn eval_js(&mut self, _source: &str) -> u64 {
        let call_id = self.next_call;
        self.next_call += 1;
        self.events.push_back(WebViewEvent::ScriptResult {
            call_id,
            result: Err("no JS engine (system-browser backend)".to_string()),
        });
        call_id
    }

    fn state(&self) -> WebViewState {
        WebViewState {
            url: self.current_url(),
            title: self.title.clone(),
            loading: false,
            progress: self.progress,
            can_go_back: self.index > 0,
            can_go_forward: self.index + 1 < self.history.len(),
            zoom: self.zoom,
            error: self.error.clone(),
        }
    }

    fn drain_events(&mut self) -> Vec<WebViewEvent> {
        self.events.drain(..).collect()
    }

    fn has_surface(&self) -> bool {
        false
    }

    fn backend_name(&self) -> &str {
        "system-browser"
    }
}

/// Hand `url` to the OS browser — http/https only (same scheme gate
/// as the fetch backend; `open`/`xdg-open` happily launch `file://`
/// or app-scheme URLs, which a "webview" must not smuggle).
fn open_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("unsupported scheme (http/https only): {url}"));
    }
    #[cfg(target_os = "macos")]
    {
        spawn_wait("open", &[url])
    }
    #[cfg(target_os = "windows")]
    {
        // rundll32 takes pure argv — `cmd /c start` would re-parse
        // the line and let `&`/`|` in a URL smuggle commands.
        spawn_wait("rundll32", &["url.dll,FileProtocolHandler", url])
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        spawn_wait("xdg-open", &[url])
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        Err("no browser opener on this platform".to_string())
    }
}

/// Spawn `prog args`, wait for exit, and map a non-zero status to
/// `Err` — a fire-and-forget `spawn` would leave zombies and report
/// "spawned" as "succeeded" (`xdg-open` exits non-zero on failure).
/// The wait is bounded: `open`/`xdg-open`/`rundll32` all return
/// promptly, so a hung one is killed rather than blocking `command`.
#[cfg(any(unix, target_os = "windows"))]
fn spawn_wait(prog: &str, args: &[&str]) -> Result<(), String> {
    let mut child = Command::new(prog)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("{prog}: {e}"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("{prog}: exit {:?}", status.code()))
                };
            }
            Ok(None) if std::time::Instant::now() > deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{prog}: timed out"));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(e) => return Err(format!("{prog}: wait: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_http_schemes() {
        assert!(open_url("file:///etc/passwd").is_err());
        assert!(open_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn history_and_state() {
        let mut wv = SystemBrowserWebView::new();
        wv.command(WebViewCommand::SetZoom(2.0));
        let s = wv.state();
        assert_eq!(s.zoom, 2.0);
        assert_eq!(s.url, "");
        assert!(!s.can_go_back);
        assert_eq!(wv.backend_name(), "system-browser");
        assert!(!wv.has_surface());
    }

    #[test]
    fn eval_js_reports_no_engine() {
        let mut wv = SystemBrowserWebView::new();
        let id = wv.eval_js("x");
        assert!(wv
            .drain_events()
            .iter()
            .any(|e| matches!(e, WebViewEvent::ScriptResult { call_id, .. } if *call_id == id)));
    }
}
