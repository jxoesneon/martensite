//! `FetchWebView` — a [`WebViewHost`] that performs real HTTP fetches
//! through `curl` and reports a real load lifecycle.
//!
//! This is the honest "native backend" short of an embedded engine:
//! navigation actually hits the network (via `curl -fsSL`, present
//! on macOS, virtually all Linux distros, and Windows 10+), extracts
//! the document `<title>`, retains the response body for
//! [`body`](FetchWebView::body), and emits `LoadStarted`/`LoadProgress`/
//! `TitleChanged`/`LoadFinished`/`LoadFailed` against real results.
//!
//! Deliberate scaffold limits, documented rather than hidden:
//!
//! - Fetches are **synchronous** — `command`/`navigate`/`reload`
//!   block until curl exits (bounded by `--max-time`). A streaming
//!   engine backend would report progress asynchronously.
//! - `Stop` is a no-op (nothing can be in flight when `command`
//!   returns).
//! - `eval_js` always returns `Err` — there is no JS engine.
//! - `has_surface` is `false`; fetched content is text, not pixels.
//! - Only `http:`/`https:` URLs are fetched — other schemes
//!   (`file:`, `javascript:`, …) fail fast without spawning.
//!
//! # Examples
//!
//! ```no_run
//! use martensite_webview_platform::FetchWebView;
//! use martensite_webview::WebViewHost;
//!
//! let mut wv = FetchWebView::new();
//! wv.navigate("https://example.com");
//! assert_eq!(wv.state().title, "Example Domain");
//! assert!(wv.body().is_some());
//! ```

use martensite_webview::{WebViewCommand, WebViewEvent, WebViewHost, WebViewState};
use std::collections::VecDeque;
use std::process::Command;

const DEFAULT_TIMEOUT_SECS: u32 = 20;
const MAX_BODY_BYTES: u64 = 10 * 1024 * 1024;
const USER_AGENT: &str = "martensite-webview-platform/0.18 (+https://martensite.dev)";

/// A real-fetch [`WebViewHost`] — see the module docs.
///
/// ```no_run
/// use martensite_webview_platform::FetchWebView;
/// use martensite_webview::WebViewHost;
///
/// assert_eq!(FetchWebView::new().backend_name(), "fetch");
/// ```
pub struct FetchWebView {
    history: Vec<String>,
    index: usize,
    title: String,
    progress: f32,
    loading: bool,
    zoom: f32,
    error: Option<String>,
    next_call: u64,
    events: VecDeque<WebViewEvent>,
    /// The last successfully fetched body (UTF-8 lossy).
    body: Option<String>,
    timeout_secs: u32,
}

impl std::fmt::Debug for FetchWebView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FetchWebView")
            .field("url", &self.current_url())
            .field("title", &self.title)
            .field("loading", &self.loading)
            .finish()
    }
}

impl Default for FetchWebView {
    fn default() -> Self {
        Self::new()
    }
}

impl FetchWebView {
    /// A fetch host with a 20s per-request timeout.
    ///
    /// ```no_run
    /// use martensite_webview_platform::FetchWebView;
    /// use martensite_webview::WebViewHost;
    ///
    /// let wv = FetchWebView::new();
    /// assert_eq!(wv.state().url, "");
    /// ```
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            index: 0,
            title: String::new(),
            progress: 0.0,
            loading: false,
            zoom: 1.0,
            error: None,
            next_call: 1,
            events: VecDeque::new(),
            body: None,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    /// Per-request timeout in seconds (default 20, clamped ≥1 —
    /// `--max-time 0` would *disable* curl's timeout entirely).
    ///
    /// ```
    /// use martensite_webview_platform::FetchWebView;
    ///
    /// let wv = FetchWebView::new().with_timeout(5);
    /// # let _ = wv;
    /// ```
    pub fn with_timeout(mut self, secs: u32) -> Self {
        self.timeout_secs = secs.max(1);
        self
    }

    /// `true` when `curl` spawns on this machine.
    ///
    /// ```
    /// use martensite_webview_platform::FetchWebView;
    ///
    /// // Curl ships on macOS, most Linux distros, and Windows 10+ —
    /// // but callers must still handle `false`.
    /// let _ = FetchWebView::available();
    /// ```
    pub fn available() -> bool {
        // Bounded like the fetch path — a hung `curl` impostor on
        // `$PATH` must not stall probing forever.
        let Ok(mut child) = Command::new("curl")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        else {
            return false;
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return status.success(),
                Ok(None) if std::time::Instant::now() > deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(_) => return false,
            }
        }
    }

    /// The last fetched response body (UTF-8 lossy), cleared on the
    /// next navigation.
    ///
    /// ```no_run
    /// use martensite_webview_platform::FetchWebView;
    ///
    /// assert!(FetchWebView::new().body().is_none());
    /// ```
    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }

    fn current_url(&self) -> String {
        self.history.get(self.index).cloned().unwrap_or_default()
    }

    /// Drive one real navigation: update history, fetch, emit events.
    fn load(&mut self, url: String, push_history: bool) {
        if push_history {
            self.history.truncate(self.index + 1);
            self.history.push(url.clone());
            self.index = self.history.len() - 1;
        }
        self.error = None;
        self.body = None;
        self.loading = true;
        if push_history {
            self.events.push_back(WebViewEvent::UrlChanged(url.clone()));
        }
        self.events
            .push_back(WebViewEvent::LoadStarted { url: url.clone() });
        self.progress = 0.3;
        self.events.push_back(WebViewEvent::LoadProgress {
            progress: self.progress,
        });

        match fetch(&url, self.timeout_secs) {
            Ok(bytes) => {
                self.progress = 1.0;
                self.loading = false;
                self.events
                    .push_back(WebViewEvent::LoadProgress { progress: 1.0 });
                let text = String::from_utf8_lossy(&bytes).into_owned();
                // A page without <title> falls back to its URL —
                // never leaves a stale title from the previous page.
                let title = extract_title(&text).unwrap_or_else(|| url.clone());
                self.body = Some(text);
                self.events.push_back(WebViewEvent::LoadFinished { url });
                if title != self.title {
                    self.title = title.clone();
                    self.events.push_back(WebViewEvent::TitleChanged(title));
                }
            }
            Err(e) => {
                self.progress = 0.0;
                self.loading = false;
                self.error = Some(e.clone());
                self.events
                    .push_back(WebViewEvent::LoadFailed { url, error: e });
            }
        }
    }
}

impl WebViewHost for FetchWebView {
    fn command(&mut self, cmd: WebViewCommand) {
        match cmd {
            WebViewCommand::Navigate(url) => self.load(url, true),
            WebViewCommand::GoBack if self.index > 0 => {
                self.index -= 1;
                let url = self.current_url();
                self.load(url, false);
            }
            WebViewCommand::GoForward if self.index + 1 < self.history.len() => {
                self.index += 1;
                let url = self.current_url();
                self.load(url, false);
            }
            WebViewCommand::Reload if !self.history.is_empty() => {
                let url = self.current_url();
                self.load(url, false);
            }
            WebViewCommand::Stop => {
                // Synchronous fetch — nothing can be in flight.
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
            result: Err("no JS engine (fetch backend)".to_string()),
        });
        call_id
    }

    fn state(&self) -> WebViewState {
        WebViewState {
            url: self.current_url(),
            title: self.title.clone(),
            loading: self.loading,
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
        "fetch"
    }
}

/// `curl -fsSL` fetch of `url` — http/https only, size-capped.
///
/// The body streams to a temp file rather than a pipe: a chunked or
/// Content-Length-free response can exceed `--max-filesize`, and
/// buffering it in memory (as `Command::output` would) is itself the
/// unbounded-allocation vector. Writing to disk + checking the file
/// size bounds memory regardless of what the server sends.
fn fetch(url: &str, timeout_secs: u32) -> Result<Vec<u8>, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("unsupported scheme (http/https only): {url}"));
    }
    let out_path =
        claim_out_path().ok_or_else(|| "temp create: exhausted path candidates".to_string())?;
    let out = Command::new("curl")
        .args([
            "-q", // first arg: skip ~/.curlrc — no ambient config
            "-fsSL",
            "-g", // globoff — `{a,b}`/`[1-9]` in a URL must not fan out
            "--max-time",
            &timeout_secs.to_string(),
            "--max-filesize",
            &MAX_BODY_BYTES.to_string(),
            // The URL scheme gate must survive redirects too.
            "--proto-redir",
            "=http,https",
            "-A",
            USER_AGENT,
            "-o",
        ])
        .arg(&out_path)
        .arg(url)
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) => {
            // The claimed placeholder exists even though curl
            // never ran.
            let _ = std::fs::remove_file(&out_path);
            return Err(format!("curl spawn: {e}"));
        }
    };
    let result = if out.status.success() {
        // `--max-filesize` only aborts when Content-Length is
        // declared; chunked/streamed responses can exceed it — so
        // the cap is enforced on the file size too.
        match std::fs::metadata(&out_path) {
            Ok(m) if m.len() <= MAX_BODY_BYTES => {
                std::fs::read(&out_path).map_err(|e| format!("read body: {e}"))
            }
            _ => Err("response exceeded 10 MiB cap".to_string()),
        }
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        Err(format!(
            "curl exit {:?}: {}",
            out.status.code(),
            stderr.lines().next().unwrap_or("").trim()
        ))
    };
    // A failed fetch can still leave a partial file — remove on
    // every path, not just success.
    let _ = std::fs::remove_file(&out_path);
    result
}

/// Claim a fresh, private output file for the curl body.
///
/// Created with `create_new` + `0o600` *before* curl runs: a
/// pre-planted symlink at a predictable path can't make curl follow
/// it and overwrite an unrelated file, and curl happily truncates
/// our empty placeholder. Retried on `AlreadyExists`.
fn claim_out_path() -> Option<std::path::PathBuf> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    for _ in 0..32 {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "martensite-webview-{}-{n}.html",
            std::process::id()
        ));
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        match opts.open(&path) {
            Ok(_) => return Some(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// First `<title>…</title>` in `html` (case-insensitive), whitespace-
/// trimmed. Byte-scan — no HTML parser dependency.
///
/// ```
/// use martensite_webview_platform::fetch::extract_title;
///
/// assert_eq!(
///     extract_title("<html><TITLE> Hi </TITLE></html>"),
///     Some("Hi".to_string())
/// );
/// assert_eq!(extract_title("<html><body>x"), None);
/// ```
pub fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    // Scan every `<title` candidate — a `<titlefoo`/` — the boundary check
    // must reject it and keep scanning, not lose the real title.
    for (start, _) in lower.match_indices("<title") {
        // The char after `<title` must be `>` or whitespace —
        // otherwise this is `<titlefoo>`/`<titlex`, not an element.
        let Some(after) = lower.as_bytes().get(start + 6).copied() else {
            continue;
        };
        if after != b'>' && !after.is_ascii_whitespace() {
            continue;
        }
        let Some(rel) = lower[start..].find('>') else {
            continue;
        };
        let gt = start + rel;
        // The closer is `</title` + optional whitespace + `>` —
        // `</title >`/`</title\n>` are legal, `</titlex` is not.
        let mut search = gt;
        let end = loop {
            let Some(rel) = lower[search..].find("</title") else {
                break None;
            };
            let close = search + rel;
            let mut j = close + 7;
            while lower
                .as_bytes()
                .get(j)
                .is_some_and(|b| b.is_ascii_whitespace())
            {
                j += 1;
            }
            if lower.as_bytes().get(j) == Some(&b'>') {
                break Some(close);
            }
            search = close + 7;
        };
        let Some(end) = end else {
            continue;
        };
        let t = html[gt + 1..end].trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_extraction() {
        assert_eq!(
            extract_title("<html><head><title>Example Domain</title></head>"),
            Some("Example Domain".to_string())
        );
        assert_eq!(
            extract_title("<TITLE>A</TITLE><title>B</title>"),
            Some("A".to_string())
        );
        assert_eq!(extract_title("<title>  </title>"), None);
        assert_eq!(extract_title("no tags"), None);
    }

    #[test]
    fn rejects_non_http_schemes() {
        assert!(fetch("file:///etc/passwd", 5).is_err());
        assert!(fetch("javascript:alert(1)", 5).is_err());
        assert!(fetch("ftp://x", 5).is_err());
    }

    #[test]
    fn scheme_gate_is_case_sensitive_prefix() {
        // Deliberately strict: only lowercase http(s) prefixes pass —
        // curl itself rejects/garbles other casing anyway, and the
        // gate exists to stop scheme smuggling, not to parse URLs.
        assert!(fetch("HTTP://example.com", 5).is_err());
    }

    #[test]
    fn eval_js_reports_no_engine() {
        let mut wv = FetchWebView::new();
        let id = wv.eval_js("1+1");
        let events = wv.drain_events();
        assert!(events.iter().any(|e| matches!(
            e,
            WebViewEvent::ScriptResult { call_id, result } if *call_id == id && result.is_err()
        )));
    }

    #[test]
    fn history_and_zoom_state() {
        let mut wv = FetchWebView::new();
        wv.command(WebViewCommand::SetZoom(1.5));
        assert_eq!(wv.state().zoom, 1.5);
        assert!(!wv.state().can_go_back);
        assert_eq!(wv.backend_name(), "fetch");
        assert!(!wv.has_surface());
    }
}
