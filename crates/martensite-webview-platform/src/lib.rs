//! OS-adjacent [`WebViewHost`] backends for `martensite-webview` —
//! real platform integration with zero unsafe code.
//!
//! An embedded engine (WKWebView, WebView2, WebKitGTK) needs a shared
//! raster surface and FFI, so this crate provides the two hosts that
//! *are* expressible through subprocess dispatch — the same contract
//! as `martensite-dialog-platform`/`martensite-share-platform`:
//!
//! - [`FetchWebView`] — performs real `curl` HTTP fetches: real load
//!   lifecycle events, extracted `<title>`, retained body. The honest
//!   "content backend" — synchronous, no JS, no raster surface.
//! - [`SystemBrowserWebView`] — dispatches navigations to the OS
//!   default browser (`open`/`xdg-open`/`rundll32`). Real OS
//!   integration; `LoadFinished` means "handed off".
//! - [`default_platform_host`] — `FetchWebView` when `curl` is on
//!   `$PATH`, `SystemBrowserWebView` otherwise.
//!
//! # Dependency direction
//!
//! Unlike the dialog/share/pdf platform crates (which define their
//! own wire types so the *safe* crate can depend on them), this crate
//! depends on `martensite-webview` and implements its [`WebViewHost`]
//! trait directly. That direction is legal because `martensite-webview`
//! never needs to name this crate — the facade selects a host by
//! feature-gating its own dependency. One link instead of a
//! feature-flag cycle.
//!
//! # Examples
//!
//! ```no_run
//! use martensite_webview_platform::default_platform_host;
//! use martensite_webview::WebViewHost;
//!
//! let mut host = default_platform_host();
//! host.navigate("https://example.com");
//! ```

#![deny(missing_docs)]

pub mod browser;
pub mod fetch;

pub use browser::SystemBrowserWebView;
pub use fetch::{extract_title, FetchWebView};

use martensite_webview::WebViewHost;

/// The best available platform host: [`FetchWebView`] when `curl`
/// spawns, [`SystemBrowserWebView`] as the always-available fallback.
///
/// ```
/// use martensite_webview_platform::default_platform_host;
/// use martensite_webview::WebViewHost;
///
/// let host = default_platform_host();
/// assert!(matches!(host.backend_name(), "fetch" | "system-browser"));
/// ```
pub fn default_platform_host() -> Box<dyn WebViewHost> {
    if FetchWebView::available() {
        Box::new(FetchWebView::new())
    } else {
        Box::new(SystemBrowserWebView::new())
    }
}
