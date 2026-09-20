# martensite-webview-platform

OS-adjacent `WebViewHost` backends for `martensite-webview` — real
platform integration with **zero unsafe code** and no embedded engine.

A true embedded webview (WKWebView, WebView2, WebKitGTK) requires a
shared raster surface and FFI. This crate provides the two hosts that
*are* expressible through subprocess dispatch — the same contract as
`martensite-dialog-platform` and `martensite-share-platform`.

## Hosts

| Host | Backend | What it does |
| --- | --- | --- |
| `FetchWebView` | `curl -fsSL` | **Real HTTP fetches** — load lifecycle events, extracted `<title>`, retained response body via `body()`. Synchronous; no JS; `has_surface() == false`. |
| `SystemBrowserWebView` | `open` / `xdg-open` / `start` | Hands navigations to the OS default browser. `LoadFinished` means "handed off", not "page loaded". |

`default_platform_host()` picks `FetchWebView` when `curl` is on
`$PATH`, `SystemBrowserWebView` otherwise.

```rust,ignore
use martensite_webview_platform::default_platform_host;
use martensite_webview::WebViewHost;

let mut host = default_platform_host();
host.navigate("https://example.com"); // real fetch
assert_eq!(host.state().title, "Example Domain");
```

## Limits (documented, not hidden)

- `FetchWebView` blocks `command`/`navigate` until curl exits
  (bounded by `--max-time`, default 20s) and caps responses at 10 MiB.
- Only `http:`/`https:` URLs are accepted — other schemes fail fast
  rather than launching files or app handlers.
- `eval_js` always errors; `Stop` is a no-op; no raster surface.

## License

MIT OR Apache-2.0
