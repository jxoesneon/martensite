//! Embedded-webview surface contract.
//!
//! `martensite-webview` defines the model a real engine backend
//! (WKWebView, WebView2, WebKitGTK, CEF) plugs into — so widgets and
//! application code are written once, against the abstraction:
//!
//! * [`WebViewCommand`] — what a widget can ask the engine to do
//!   (navigate, history, reload, stop, zoom).
//! * [`WebViewEvent`] — what the engine reports back (load lifecycle,
//!   title/URL changes, script results).
//! * [`WebViewState`] — the committed snapshot widgets paint from.
//! * [`WebViewHost`] — the `Send + Sync` trait every backend
//!   implements, so a host can be embedded directly in the `WebView`
//!   facade widget.
//! * [`SimulatedWebView`] — a deterministic in-process engine that
//!   runs the full lifecycle without raster content; the default host
//!   for tests, previews, and headless environments.
//!
//! # Architecture
//!
//! A full embedded engine (WKWebView `WKWebView`, WebView2
//! `ICoreWebView2`, WebKitGTK `webkit_web_view_new`) needs in-process
//! FFI plus a shared-GPU surface handoff, which a subprocess cannot
//! provide. Until then, `martensite-webview-platform` supplies the
//! two hosts that *are* expressible through process dispatch —
//! `FetchWebView` (real `curl` fetches, real `<title>` extraction)
//! and `SystemBrowserWebView` (OS browser handoff) — implementing
//! [`WebViewHost`] directly, so the facade `WebView::native()` works
//! against real network/OS integration today.
//!
//! This crate is `#![forbid(unsafe_code)]`.
//!
//! # Examples
//!
//! ```
//! use martensite_webview::{SimulatedWebView, WebViewCommand, WebViewHost};
//!
//! let mut wv = SimulatedWebView::new();
//! wv.command(WebViewCommand::Navigate("https://example.com".into()));
//! let state = wv.state();
//! assert_eq!(state.url, "https://example.com");
//! assert!(!state.loading); // simulated loads finish synchronously
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod simulated;
pub mod state;

pub use simulated::SimulatedWebView;
pub use state::{WebViewCommand, WebViewEvent, WebViewHost, WebViewState};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulated_history_and_events() {
        let mut wv = SimulatedWebView::new();
        wv.navigate("https://a");
        wv.navigate("https://b");
        assert!(wv.state().can_go_back);
        assert!(!wv.state().can_go_forward);
        wv.go_back();
        assert_eq!(wv.state().url, "https://a");
        wv.go_forward();
        assert_eq!(wv.state().url, "https://b");
        // Drain shows the whole lifecycle happened.
        let evs = wv.drain_events();
        assert!(evs
            .iter()
            .any(|e| matches!(e, WebViewEvent::LoadFinished { .. })));
    }

    #[test]
    fn simulated_fail_next_and_stop() {
        let mut wv = SimulatedWebView::new();
        wv.fail_next("no route");
        wv.navigate("https://x");
        assert_eq!(wv.state().error.as_deref(), Some("no route"));
        // Next navigate clears the error.
        wv.navigate("https://y");
        assert!(wv.state().error.is_none());
    }

    #[test]
    fn simulated_eval_js_returns_call_id() {
        let mut wv = SimulatedWebView::new();
        let id = wv.eval_js("40 + 2");
        let evs = wv.drain_events();
        assert!(matches!(
            evs.first(),
            Some(WebViewEvent::ScriptResult { call_id, result: Ok(_) }) if *call_id == id
        ));
    }
}
