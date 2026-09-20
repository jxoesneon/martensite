//! A deterministic in-process webview host for tests, previews, and
//! headless environments.
//!
//! [`SimulatedWebView`] applies commands to its state synchronously and
//! queues the corresponding [`WebViewEvent`]s for `drain_events`:
//! navigation updates the history stack and completes the load
//! immediately (`progress` settles at `1.0`), `eval_js` echoes the
//! source back as its result, and `has_surface` is always `false` — it
//! produces no raster content, so the facade paints its placeholder.
//!
//! # Examples
//!
//! ```
//! use martensite_webview::{SimulatedWebView, WebViewHost};
//!
//! let mut wv = SimulatedWebView::new();
//! wv.navigate("https://a.example");
//! wv.navigate("https://b.example");
//! assert!(wv.state().can_go_back);
//! wv.go_back();
//! assert_eq!(wv.state().url, "https://a.example");
//! ```

use crate::state::{WebViewCommand, WebViewEvent, WebViewHost, WebViewState};
use std::collections::VecDeque;

/// A simulated [`WebViewHost`] — full lifecycle, no real engine.
///
/// # Examples
///
/// ```
/// use martensite_webview::{SimulatedWebView, WebViewEvent, WebViewHost};
///
/// let mut wv = SimulatedWebView::new();
/// wv.navigate("https://example.com");
/// let events = wv.drain_events();
/// assert!(events.iter().any(|e| matches!(e, WebViewEvent::LoadFinished { .. })));
/// ```
#[derive(Debug)]
pub struct SimulatedWebView {
    history: Vec<String>,
    index: usize,
    title: String,
    progress: f32,
    loading: bool,
    zoom: f32,
    error: Option<String>,
    next_call: u64,
    events: VecDeque<WebViewEvent>,
    /// Pending failure injected by [`fail_next`](Self::fail_next).
    fail_next_error: Option<String>,
}

impl Default for SimulatedWebView {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulatedWebView {
    /// An empty simulated webview (no page loaded).
    ///
    /// ```
    /// use martensite_webview::{SimulatedWebView, WebViewHost};
    ///
    /// let wv = SimulatedWebView::new();
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
            fail_next_error: None,
        }
    }

    /// Force the next load to fail with `error` — test seam for the
    /// [`WebViewEvent::LoadFailed`] path.
    ///
    /// ```
    /// use martensite_webview::{SimulatedWebView, WebViewEvent, WebViewHost};
    ///
    /// let mut wv = SimulatedWebView::new();
    /// wv.fail_next("dns");
    /// wv.navigate("https://gone");
    /// assert!(wv.state().error.is_some());
    /// ```
    pub fn fail_next(&mut self, error: impl Into<String>) {
        self.fail_next_error = Some(error.into());
    }

    fn url(&self) -> String {
        self.history.get(self.index).cloned().unwrap_or_default()
    }

    fn commit_load(&mut self) {
        let url = self.url();
        self.loading = true;
        self.progress = 0.0;
        self.events
            .push_back(WebViewEvent::LoadStarted { url: url.clone() });
        if let Some(e) = self.fail_next_error.take() {
            self.loading = false;
            self.error = Some(e.clone());
            self.events
                .push_back(WebViewEvent::LoadFailed { url, error: e });
            return;
        }
        self.error = None;
        self.progress = 1.0;
        self.loading = false;
        self.events
            .push_back(WebViewEvent::LoadProgress { progress: 1.0 });
        self.events.push_back(WebViewEvent::LoadFinished { url });
        let title = format!("Simulated — {}", self.url());
        if title != self.title {
            self.title = title.clone();
            self.events.push_back(WebViewEvent::TitleChanged(title));
        }
    }
}

impl WebViewHost for SimulatedWebView {
    fn command(&mut self, cmd: WebViewCommand) {
        match cmd {
            WebViewCommand::Navigate(url) => {
                // Drop forward history, push the new entry.
                self.history.truncate(self.index + 1);
                self.history.push(url.clone());
                self.index = self.history.len() - 1;
                self.events.push_back(WebViewEvent::UrlChanged(url));
                self.commit_load();
            }
            WebViewCommand::GoBack if self.index > 0 => {
                self.index -= 1;
                self.commit_load();
            }
            WebViewCommand::GoForward if self.index + 1 < self.history.len() => {
                self.index += 1;
                self.commit_load();
            }
            WebViewCommand::Reload if !self.history.is_empty() => self.commit_load(),
            WebViewCommand::Stop if self.loading => {
                self.loading = false;
                let url = self.url();
                self.events.push_back(WebViewEvent::LoadFailed {
                    url,
                    error: "stopped".to_string(),
                });
            }
            WebViewCommand::SetZoom(z) => {
                self.zoom = z.clamp(0.25, 5.0);
            }
            _ => {}
        }
    }

    fn eval_js(&mut self, source: &str) -> u64 {
        let call_id = self.next_call;
        self.next_call += 1;
        self.events.push_back(WebViewEvent::ScriptResult {
            call_id,
            result: Ok(format!("undefined /* {source} */")),
        });
        call_id
    }

    fn state(&self) -> WebViewState {
        WebViewState {
            url: self.url(),
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
        "simulated"
    }
}
