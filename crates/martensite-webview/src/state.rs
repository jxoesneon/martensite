//! Webview state model, command/event vocabulary, and host trait.
//!
//! This module is platform-agnostic: it defines the wire types every
//! webview backend consumes ([`WebViewCommand`], [`WebViewEvent`],
//! [`WebViewState`]) and the [`WebViewHost`] contract a real engine
//! (WKWebView, WebView2, WebKitGTK, CEF) implements. `Widget`-side
//! code talks only to this module; engine details live behind the
//! trait.
//!
//! # Examples
//!
//! ```
//! use martensite_webview::{SimulatedWebView, WebViewHost};
//!
//! let mut wv = SimulatedWebView::new();
//! wv.navigate("https://example.com");
//! assert!(wv.state().loading || wv.state().url.contains("example"));
//! ```

/// A command a widget/host can issue to the engine.
///
/// `EvaluateJs` is *not* part of this enum — it returns a call id, so
/// it is a dedicated [`WebViewHost::eval_js`] method instead.
///
/// # Examples
///
/// ```
/// use martensite_webview::WebViewCommand;
///
/// assert_ne!(WebViewCommand::Reload, WebViewCommand::Stop);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum WebViewCommand {
    /// Navigate to a URL.
    Navigate(String),
    /// History back.
    GoBack,
    /// History forward.
    GoForward,
    /// Reload the current page.
    Reload,
    /// Stop the in-flight load.
    Stop,
    /// Page zoom factor (`1.0` = 100%).
    SetZoom(f32),
}

/// An event the engine emits for the widget/host to consume.
///
/// Events are drained per frame via [`WebViewHost::drain_events`] and
/// mostly mirror state transitions already visible through
/// [`WebViewHost::state`] — they exist so widgets can react
/// (announce, badge, log) rather than only repaint.
///
/// # Examples
///
/// ```
/// use martensite_webview::WebViewEvent;
///
/// let e = WebViewEvent::LoadFinished {
///     url: "https://x".into(),
/// };
/// assert!(matches!(e, WebViewEvent::LoadFinished { .. }));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum WebViewEvent {
    /// A navigation started loading.
    LoadStarted {
        /// The URL being loaded.
        url: String,
    },
    /// Estimated load progress changed (`0.0..=1.0`).
    LoadProgress {
        /// Estimated progress fraction.
        progress: f32,
    },
    /// A load committed and finished.
    LoadFinished {
        /// The loaded URL.
        url: String,
    },
    /// A load failed or was stopped.
    LoadFailed {
        /// The URL that failed.
        url: String,
        /// Engine error description.
        error: String,
    },
    /// The document title changed.
    TitleChanged(String),
    /// The visible URL changed (SPA pushState, redirect).
    UrlChanged(String),
    /// The result of an [`WebViewHost::eval_js`] call.
    ScriptResult {
        /// The call id returned by `eval_js`.
        call_id: u64,
        /// The serialized result or error description.
        result: Result<String, String>,
    },
}

/// A snapshot of the engine's visible state.
///
/// Widgets paint from this — it is a value type so `state()` stays
/// `&self` and cheap.
///
/// # Examples
///
/// ```
/// use martensite_webview::WebViewState;
///
/// let s = WebViewState::default();
/// assert!(!s.loading);
/// assert_eq!(s.progress, 0.0);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebViewState {
    /// The current or pending URL (empty = no page loaded).
    pub url: String,
    /// The document title (empty until the engine reports one).
    pub title: String,
    /// `true` while a load is in flight.
    pub loading: bool,
    /// Estimated load progress `0.0..=1.0`.
    pub progress: f32,
    /// `true` when [`WebViewHost::go_back`] would succeed.
    pub can_go_back: bool,
    /// `true` when [`WebViewHost::go_forward`] would succeed.
    pub can_go_forward: bool,
    /// Zoom factor (`1.0` = 100%).
    pub zoom: f32,
    /// The most recent load error, cleared by the next navigation.
    pub error: Option<String>,
}

/// The contract every embedded-webview backend implements.
///
/// A host owns one engine instance. Commands are non-blocking; state
/// changes arrive as [`WebViewEvent`]s drained per frame, and
/// [`state`](Self::state) always reflects the latest committed
/// snapshot. Implementations must be `Send + Sync` so a host can be
/// embedded in a `Widget` (the facade requires it) — native engines
/// whose real objects are thread-confined expose thread-safe handles
/// here instead.
///
/// # Examples
///
/// ```
/// use martensite_webview::{SimulatedWebView, WebViewHost};
///
/// let mut host = SimulatedWebView::new();
/// host.navigate("https://martensite.dev");
/// assert_eq!(host.backend_name(), "simulated");
/// ```
pub trait WebViewHost: Send + Sync {
    /// Issue a [`WebViewCommand`] to the engine.
    fn command(&mut self, cmd: WebViewCommand);

    /// Navigate to `url` — shorthand for `command(Navigate(..))`.
    ///
    /// ```
    /// use martensite_webview::{SimulatedWebView, WebViewHost};
    ///
    /// let mut h = SimulatedWebView::new();
    /// h.navigate("https://example.com");
    /// ```
    fn navigate(&mut self, url: &str) {
        self.command(WebViewCommand::Navigate(url.to_string()));
    }

    /// History back.
    ///
    /// ```
    /// use martensite_webview::{SimulatedWebView, WebViewHost};
    ///
    /// SimulatedWebView::new().go_back();
    /// ```
    fn go_back(&mut self) {
        self.command(WebViewCommand::GoBack);
    }

    /// History forward.
    ///
    /// ```
    /// use martensite_webview::{SimulatedWebView, WebViewHost};
    ///
    /// SimulatedWebView::new().go_forward();
    /// ```
    fn go_forward(&mut self) {
        self.command(WebViewCommand::GoForward);
    }

    /// Reload the current page.
    ///
    /// ```
    /// use martensite_webview::{SimulatedWebView, WebViewHost};
    ///
    /// SimulatedWebView::new().reload();
    /// ```
    fn reload(&mut self) {
        self.command(WebViewCommand::Reload);
    }

    /// Evaluate JavaScript in the page context. Returns a call id the
    /// matching [`WebViewEvent::ScriptResult`] carries back.
    ///
    /// ```
    /// use martensite_webview::{SimulatedWebView, WebViewHost};
    ///
    /// let mut h = SimulatedWebView::new();
    /// let id = h.eval_js("1 + 1");
    /// assert!(id > 0);
    /// ```
    fn eval_js(&mut self, source: &str) -> u64;

    /// The latest committed state snapshot.
    ///
    /// ```
    /// use martensite_webview::{SimulatedWebView, WebViewHost};
    ///
    /// let h = SimulatedWebView::new();
    /// assert!(!h.state().loading);
    /// ```
    fn state(&self) -> WebViewState;

    /// Drain events emitted since the last call.
    ///
    /// ```
    /// use martensite_webview::{SimulatedWebView, WebViewHost};
    ///
    /// let mut h = SimulatedWebView::new();
    /// assert!(h.drain_events().is_empty());
    /// ```
    fn drain_events(&mut self) -> Vec<WebViewEvent>;

    /// `true` when the engine produces real raster content. Simulated
    /// and stub hosts return `false`; the facade paints a placeholder
    /// in that case.
    ///
    /// ```
    /// use martensite_webview::{SimulatedWebView, WebViewHost};
    ///
    /// assert!(!SimulatedWebView::new().has_surface());
    /// ```
    fn has_surface(&self) -> bool;

    /// Human-readable backend name, e.g. `"wkwebview"`, `"webview2"`,
    /// `"simulated"`.
    ///
    /// ```
    /// use martensite_webview::{SimulatedWebView, WebViewHost};
    ///
    /// assert_eq!(SimulatedWebView::new().backend_name(), "simulated");
    /// ```
    fn backend_name(&self) -> &str;
}
