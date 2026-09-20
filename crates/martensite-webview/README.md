# martensite-webview

Embedded-webview surface contract for the Martensite GUI framework —
the model a real engine backend (WKWebView, WebView2, WebKitGTK, CEF)
plugs into, so widgets and application code are written once against
the abstraction.

The crate itself is **pure-safe** (`#![forbid(unsafe_code)]`, no FFI)
and dependency-free.

## What's here

| Type | Role |
| --- | --- |
| `WebViewCommand` | What a widget can ask the engine to do — navigate, history back/forward, reload, stop, zoom. |
| `WebViewEvent` | What the engine reports back — load lifecycle, title/URL changes, script results. Drained per frame via `WebViewHost::drain_events`. |
| `WebViewState` | The committed snapshot widgets paint from. |
| `WebViewHost` | The `Send + Sync` trait every backend implements, so a host can be embedded directly in the facade `WebView` widget. |
| `SimulatedWebView` | Deterministic in-process engine that runs the full load lifecycle without raster content — the default host for tests, previews, and headless environments. |

## Usage

```rust
use martensite_webview::{SimulatedWebView, WebViewCommand, WebViewHost};

let mut wv = SimulatedWebView::new();
wv.command(WebViewCommand::Navigate("https://example.com".into()));
let state = wv.state();
assert_eq!(state.url, "https://example.com");
assert!(!state.loading); // simulated loads finish synchronously
```

## Architecture

A full embedded engine (WKWebView `WKWebView`, WebView2
`ICoreWebView2`, WebKitGTK `webkit_web_view_new`) needs in-process FFI
plus a shared-GPU surface handoff, which a subprocess cannot provide.
Until then, `martensite-webview-platform` supplies the two hosts that
*are* expressible through process dispatch — `FetchWebView` (real
`curl` fetches, real `<title>` extraction) and `SystemBrowserWebView`
(OS browser handoff) — implementing `WebViewHost` directly, so the
facade `WebView::native()` works against real network/OS integration
today.

## License

MIT OR Apache-2.0
