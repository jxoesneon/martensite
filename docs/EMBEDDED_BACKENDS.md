# Martensite — Embedded Backends: Native Engine Integration Design

**Status:** Design proposal — pre-RFC
**Scope:** `martensite-webview`, `martensite-webview-platform`, `martensite-pdf`,
`martensite-pdf-platform`, `martensite-media-platform`, `martensite-engine-bridge`,
`martensite-wgpu`, `martensite` (facade widgets)
**Related:** ADR-0009 (zero-copy hardware media), ADR-0033 (host-mode external
surface embedding), `docs/milestones/v0.14.0-external-surfaces.md`,
`docs/milestones/v0.16.0-media-pipeline.md`

---

## 1. Current state — what the subprocess backends can and cannot do

### 1.1 The contracts

Two trait contracts already define the seam a real engine plugs into:

- **`WebViewHost`** (`crates/martensite-webview/src/state.rs:155`) — `Send + Sync`
  object owning one engine instance. Surface: `command(&mut self, WebViewCommand)`
  (L157), `eval_js(&mut self, &str) -> u64` (L214), `state(&self) -> WebViewState`
  (L224), `drain_events(&mut self) -> Vec<WebViewEvent>` (L234),
  `has_surface(&self) -> bool` (L245), `backend_name(&self) -> &str` (L255).
  `WebViewCommand` (L33-46) covers Navigate/GoBack/GoForward/Reload/Stop/SetZoom;
  `WebViewEvent` (L66-100) covers the load lifecycle, TitleChanged/UrlChanged,
  and `ScriptResult { call_id, result }`. The trait docs already anticipate
  thread-confined native engines: "native engines whose real objects are
  thread-confined expose thread-safe handles here instead" (L140-144).
- **`PdfDocument` / `PdfProvider`** (`crates/martensite-pdf/src/doc.rs:191`,
  L224) — `Send + Sync`; every `PdfDocument` method takes `&self` explicitly so
  real rasterizers "use interior mutability internally" and the widget can call
  `render_page` from `paint(&self)` (L176-181). `render_page(&self, page,
  max_px) -> Option<PdfPageBitmap>` (L210) returns tightly-packed RGBA8;
  `None` means "paint a placeholder". `PdfProvider::open` returns
  `Result<Box<dyn PdfDocument>, PdfError>` (L226).

The crate-level docs state the blocker plainly: a real embedded engine "needs
in-process FFI plus a shared-GPU surface handoff, which a subprocess cannot
provide" (`crates/martensite-webview/src/lib.rs:21-24`).

### 1.2 What the scaffolds actually deliver

**`FetchWebView`** (`crates/martensite-webview-platform/src/fetch.rs`) — real
`curl -fsSL` fetches, real `<title>` extraction, retained body, real load
lifecycle events. Documented limits (L11-22):

- **Synchronous** — `command`/`navigate`/`reload` block until curl exits
  (bounded by `--max-time`, default 20 s, L39/L14-15). `Stop` is a no-op
  because nothing can be in flight when `command` returns (L16-17).
- **No JS** — `eval_js` always reports `Err` (L18, L247-255).
- **No surface** — `has_surface()` is `false`; fetched content is text, not
  pixels (L19, L274-276).
- **http/https only** — other schemes fail fast (L20-21, L291-293).
- 10 MiB body cap (L40), temp-file body buffering with `create_new`+`0o600`
  hardening (L348-376).

**`SystemBrowserWebView`** (`crates/martensite-webview-platform/src/browser.rs`) —
dispatches to `open`/`xdg-open`/`rundll32`. `LoadFinished` means "handed to the
OS", not "page loaded" (L6-9); the title is the URL because the OS gives no
document feedback (L40-42). `has_surface` is `false` (L177-179); `eval_js`
errors (L150-158).

**`default_platform_host()`** picks `FetchWebView` when `curl` probes,
`SystemBrowserWebView` otherwise (`webview-platform/src/lib.rs:58-64`). Both
implement `WebViewHost` directly — the platform crate depends on
`martensite-webview`, not vice versa (lib.rs:18-26).

**`SubprocessDocument`** (`crates/martensite-pdf-platform/src/provider.rs:231`) —
drives `pdfinfo`/`pdftoppm` (poppler) or `mutool` (mupdf). Real page rasters,
but:

- **A subprocess per page render** — `render_page` computes a DPI from `max_px`
  (L454-460), spawns the rasterizer via `render_ppm`→`render_ppm_to`
  (L1245-1300), decodes its P6 PPM (L461). No raster cache; every repaint
  re-rasterizes through a fork/exec.
- **Metadata-first page sizes (mupdf)** — one `mutool show -g … pages grep`
  subprocess dumps the whole object table; `/Parent`-chain resolution covers
  inherited MediaBox/CropBox/Rotate, page-local UserUnit scales, and pages the
  metadata can't cover fall back to a 72-dpi render probe. An *insane*
  resolved size skips the probe rather than rasterizing gigabytes
  (L375-433, L571-621 + `parse_mutool_show`).
- **Synchronous** — `run()` blocks until the child exits or a deadline kills
  it: 60 s for metadata/info/page-size probes (L420, L633, L813), 120 s for
  page renders (L1268, L1285); `run` itself at L571-621.
- **`CliSource::Bytes` is materialized to a temp file** (L297, via
  `write_temp_pdf` L502-535) because the CLIs only read paths — an in-memory
  backend has no such constraint.
- Page sizes probed lazily and cached behind a `Mutex` (L247, L375-399) —
  the interior-mutability pattern a real backend will reuse.
- `PdfBackend` is probed once (`probe_backend`, L187-198); missing tools →
  `CliError::Unsupported` → `NullPdfProvider` (`martensite-pdf/src/doc.rs:290-309`).

### 1.3 What the facade does with it

- `WebView::native()` injects `default_platform_host()`
  (`crates/martensite/src/widgets/webview.rs:100-102`); `WebView::with_host`
  (L114) is the documented seam "a native engine backend plugs into". When
  `has_surface()` is `false` the widget paints a "preview — N engine paints no
  surface" placeholder (L486-501). Input is currently inert: `event()` returns
  `EventResponse::Ignored` (L401-403).
- `PdfView::open` goes through `default_pdf_provider()` and errors honestly
  rather than silently falling back (`pdf_view.rs:122-125`). `paint` calls
  `doc.render_page(self.page, RENDER_MAX_PX)` — a fixed 1024 px long-edge cap
  (L35) — and hands the bitmap to `PaintList::push_image` via
  `ImageData::from_rgba` (L455-459).

**Summary of the gap:** the subprocess tier proves the contracts end-to-end but
can never produce (a) asynchronous load progress, (b) JS execution, (c) pixels
for web content, (d) sub-100 ms page rasterization, or (e) scrolled/tiled PDF
rendering. Those require in-process engines and a way to get engine pixels into
Martensite's paint tree.

---

## 2. Target architecture — WebView

### 2.1 The compositing problem is the design's center

Every webview engine renders in its own compositor — WKWebView in the
WebContent/WindowServer pipeline, WebView2 in a DComp visual tree, WebKitGTK
in a GTK render-node graph. Getting those pixels into Martensite's paint tree
is the hard part; mapping `WebViewHost` onto each engine's delegate surface is
mechanical by comparison.

Martensite already owns the machinery for this, built for external GPU
producers (v0.14.0, ADR-0033):

- **`PaintCommand::External { surface_id, rect, clip }`**
  (`crates/martensite-core/src/paint.rs:784-791`) is a paint-order marker;
  `PaintList::segments()` (L1681) splits the list so the orchestrator
  composites each external surface at its exact z-position
  (`PaintSegment` enum, L845-857).
- **`martensite-engine-bridge`** defines the producer side: `Engine::render`
  publishes frames into a two-slot `SurfaceRing`; `BridgeRegistry` tracks
  readiness and damage (`engine-bridge/src/lib.rs:30-42`). `FrameSync`
  (`frame.rs:48-93`) already declares the cross-device sync vocabulary —
  `FenceValue` (D3D12 fence), `VkSemaphoreFd`, `MetalSharedEvent`,
  `DxgiKeyedMutex` — and `NativeFrame` (`frame.rs:127-150`) declares the
  cross-process handle descriptors: `DmaBuf{fd,modifier,stride,offset}`,
  `IoSurface{surface_id}`, `SharedHandle{handle}`.
- **`martensite-wgpu`'s `WgpuHost`** composites a producer's `wgpu::Texture`
  into the frame target with a six-vertex quad pass — zero GPU copies on the
  same-device path (`martensite-wgpu/src/external.rs:1-8`,
  `composite_front` L863). `SourceAlpha` straight/premul pipelines already
  exist (L92-114).
- **`martensite-media-platform`** is the cross-device import half:
  `import_external_texture` (lib.rs:345-408) maps `HardwareHandle::IoSurface` /
  `DxgiSharedHandle` / `DmaBuf` (surface.rs:165-210) onto wgpu-hal's
  Metal/Vulkan import paths, with `import_cpu_memory` (L486+) as the
  documented fallback.
- **`ExternalEngine`** (`crates/martensite/src/widgets/external.rs:114`) is
  the retained widget that displays a `SurfaceId` — the precedent for how a
  surface-bearing `WebView` would paint.

So a webview backend has two raster paths, mirroring the video decoder tier:

| Path | Mechanism | Status today |
|------|-----------|--------------|
| CPU raster | Engine snapshot → `CpuFrame` (`frame.rs:169+`) → `import_cpu_memory` or `PaintList::push_image` | Fully implementable with existing types |
| GPU surface | Engine export → `NativeFrame`/`HardwareHandle` → `import_external_texture` → ring slot → `WgpuHost::composite_front` | Import functions exist; the *producer* (engine-side export) is platform-specific and partially uncertain |

The `WebViewHost` trait needs **one additive contract** for this: a way to
expose the engine's `SurfaceId`/`BridgeHandle` (or a CPU pixmap for the
fallback) so the facade can emit `PaintCommand::External` instead of the
placeholder when `has_surface()` is `true`. `has_surface()` (state.rs:245)
is already the gate; the widget already skips the placeholder on that flag
(webview.rs:486-501). The addition must be a defaulted method or a separate
capability trait to stay semver-compatible — `WebViewHost` is public API.

### 2.2 WKWebView (macOS)

**Engine object:** `WKWebView` (WebKit.framework), driven through `objc2` /
`objc2-web-kit` bindings. *Needs verification:* current `objc2-web-kit`
coverage of `WKNavigationDelegate`, `WKUIDelegate`, snapshot APIs.

**Command/event mapping:**

- `Navigate` → `load(_ request:)`; `GoBack`/`GoForward` → `goBack()`/`goForward()`;
  `Reload` → `reload()`; `Stop` → `stopLoading()`; `SetZoom` →
  `pageZoom`/`magnification` (verify exact API on macOS vs iOS).
- `WKNavigationDelegate` callbacks → `WebViewEvent`s:
  `didStartProvisionalNavigation` → `LoadStarted`; `estimatedProgress` KVO →
  `LoadProgress`; `didFinish` → `LoadFinished`; `didFail(Provisional)Navigation`
  → `LoadFailed`; `title`/`URL` KVO → `TitleChanged`/`UrlChanged`.
- `eval_js` → `evaluateJavaScript(_:completionHandler:)`; the returned call id
  indexes a pending-map drained into `ScriptResult`. All callbacks arrive on
  the main runloop, so the host object is a `Send + Sync` proxy holding a
  main-thread-confined inner behind `Mutex` + a `VecDeque<WebViewEvent>` —
  exactly the pattern state.rs:140-144 anticipates and `SubprocessDocument`
  already uses (`provider.rs:247`).

**Pixels — two tiers:**

1. **Snapshot fallback:** `WKWebView.takeSnapshot(with:completionHandler:)`
   delivers an `NSImage`/bitmap per call. Convert to RGBA8 → `CpuFrame` →
   `push_image`/`import_cpu_memory`. Works today as a public API; frame-rate
   limited (per-frame allocations, main-thread bounce). Good enough for
   v1-of-the-backend correctness; not for video/scroll.
2. **Surface sharing:** WKWebView content arrives from the WebContent process
   as `CALayer`/`IOSurface` content composited by WindowServer. There is **no
   public API** to obtain the backing `IOSurface` of a live `WKWebView`.
   Candidate routes — *all need verification*: (a) render the view's layer
   into an `IOSurface`-backed `CARenderer` (private-ish API, App Store risk);
   (b) `CAMetalLayer`-host the view and capture its drawable — not supported
   for a `WKWebView` subtree; (c) accept the snapshot path. If a legal
   `IOSurfaceID` is obtained, the import half already exists:
   `HardwareHandle::IoSurface` → `import_iosurface` → `wgpu::Texture`
   (media-platform lib.rs:11-13, 351-355).

**Threading:** `WKWebView` must be created and driven on the main thread —
*needs verification* against current WebKit docs (AppKit view classes are
main-thread-bound by convention; whether `WKWebView` specifically hard-requires
it vs. merely its event delivery should be confirmed). The
proxy host therefore needs a main-thread executor channel; `command(&mut self)`
enqueues, `drain_events` dequeues. This is the same discipline
`martensite-access-platform` applies for UIKit FFI (AGENTS.md:182-184).

### 2.3 WebView2 (Windows)

**Engine object:** `ICoreWebView2` via `CoreWebView2Environment` (COM), using
the `webview2-com`/`windows` crate bindings. Runtime is the Evergreen
WebView2 loader (`WebView2Loader.dll`) — distribution question in §6.

**Command/event mapping** — analogous to WKWebView:

- `Navigate` → `ICoreWebView2::Navigate`; history → `GoBack`/`GoForward`;
  `Reload`; `Stop` → `Stop`; `SetZoom` → `ZoomFactor`.
- Events: `NavigationStarting` → `LoadStarted`; `NavigationCompleted` →
  `LoadFinished`/`LoadFailed` (split on `IsSuccess`/`WebErrorStatus`);
  `DocumentTitleChanged` → `TitleChanged`; `SourceChanged` → `UrlChanged`;
  progress is not a first-class WebView2 event — derive from
  `NavigationStarting`→`ContentLoading`→`NavigationCompleted` stages or poll.
- `eval_js` → `ExecuteScript` (or `PostWebMessageAsJson` for structured
  channels); completion handler → `ScriptResult`.

**Pixels — two tiers:**

1. **Readback fallback:** `ICoreWebView2::CapturePreview` streams a PNG of the
   current viewport — a supported public API. Decode → `CpuFrame`. Cheaper
   than repeated snapshots is not guaranteed; usable for correctness-first v1.
   *Needs verification:* `CapturePreview` requires the controller's HWND-mode
   view; behavior under the composition controller differs by SDK version.
2. **Surface sharing:** `ICoreWebView2CompositionController` presents into a
   caller-owned `IDCompositionVisual` tree. Embedding into a DComp visual
   *that Martensite also reads back* requires either (a)
   `DCompositionGetSurfaceId`/DWM redirection surface tricks (fragile,
   version-sensitive), or (b) letting WebView2 own a real visual tree and
   interposing a swapchain — the approach used by community WebView2
   integrations, *needs verification per SDK version*. Any produced
   `IDXGIResource1::CreateSharedHandle` NT handle maps onto
   `HardwareHandle::DxgiSharedHandle` (surface.rs:167-169).

**Hard constraint discovered in-tree:** `import_dxgi_texture` only works on a
**Vulkan-backend** wgpu device created with
`Features::VULKAN_EXTERNAL_MEMORY_WIN32` — wgpu-hal's DX12 backend *cannot
import D3D11 shared handles* (`martensite-media-platform/src/windows.rs:18-24`;
AGENTS.md:233-238). WebView2's composition output is D3D11 — *needs
verification*; if WebView2 produces D3D12-shared textures on newer SDKs this
constraint relaxes. Therefore Windows zero-copy requires either running
Martensite's device on the Vulkan backend
when a webview surface is active (a device/backend selection decision — see
§6), or `D3D11On12` plumbing wgpu-hal does not expose, or the CPU readback
path. This is the single largest technical risk in the tier.

**Threading:** WebView2 objects are thread-agile-ish but the controller's
event pump is apartment-bound; the proxy-host pattern applies as on macOS.

### 2.4 WebKitGTK / WPE (Linux)

Two engines, one trade-off:

- **WebKitGTK** (`WebKitWebView`) is a GTK widget. Embedding into a winit
  window means initializing GTK/GDK in-process, owning a `GtkWidget` subtree,
  and either (a) parenting it to a native window — a floating child window,
  the same non-solution as a raw HWND on Windows — or (b) rasterizing via
  `gtk_widget_snapshot`/GL render nodes into a texture we own. GTK4 render
  nodes can paint to a `GdkTexture`; extracting a DMA-BUF or GL texture from
  that path is *uncertain* and version-dependent. GTK can coexist with a
  winit loop (iterate `g_main_context` manually per frame) but it is the most
  invasive engine init of the three.
- **WPE WebKit + `wpebackend-fdo`** is purpose-built for embedders: the view
  backend exports each frame as a **DMA-BUF fd** (`wpe_fdo_exportable`) — a
  direct fit for `HardwareHandle::DmaBuf` (surface.rs:185-193) →
  `import_dmabuf` → Vulkan texture → `WgpuHost`. This is the only engine whose
  native output format is exactly the transport `NativeFrame::DmaBuf` already
  declares (frame.rs:129-140). *Needs verification:* current fdo API surface
  (`wpe_view_backend_exportable_fdo` vs the newer buffer-pool API), and
  distro packaging of WPE vs WebKitGTK.

Event/eval mapping: `webkit_web_view_load_uri`, `load-changed` signal →
LoadStarted/Finished/Failed; `notify::title`/`notify::uri` →
TitleChanged/UrlChanged; `notify::estimated-load-progress` → LoadProgress;
`webkit_web_view_evaluate_javascript` (GTK4 API) / `run_javascript` →
`ScriptResult`. GLib signals marshal through a `g_main_context` pump — the
host proxy polls or is woken per frame by `drain_events`.

### 2.5 wry/tao as an alternative to direct FFI

`wry` (Tauri) wraps exactly these three engines behind one API. Assessment:

**For:**
- Months of per-platform delegate/COM/GTK glue already written and
  battle-tested; instant `WebViewHost` adapter surface.

**Against:**
- **Window ownership conflict.** wry renders webviews as child
  windows/layers attached to a window it expects to own (via `tao`, a winit
  fork pinned to an older winit line — *needs verification* of the current
  tao/winit version alignment). Martensite owns its winit 0.31-beta
  event loop and already carries a patched `accesskit-winit` for it
  (AGENTS.md:195-197). A floating child NSView/HWND is *not* composited into
  the paint tree — it hovers above the surface, breaks `PaintCommand` z-order
  and clipping, and bypasses the `External` segment machinery ADR-0033
  established. It reintroduces Direction-B-style "the engine owns a window"
  inside Direction-A.
- **Input/focus/a11y:** child-window webviews own their own focus and
  accessibility trees, conflicting with Martensite's AccessKit ownership —
  the explicit reason host mode was chosen (ADR-0033).
- **Offscreen mode:** wry has no public "render to texture/IOSurface" mode as
  of its current API — *needs verification per version*; without it, wry
  solves FFI but not the compositing problem, which is the actual hard part.
- **Unsafe budget:** wry's FFI lives in the dependency, but Martensite's
  vendored/patched-fork precedent (`martensite-vello`, `-cosmic-text`,
  `-accesskit-winit`, `martensite-godot` as `publish=false`) shows the
  workspace absorbs or quarantines unsafe rather than pretending a dep makes
  it disappear; supply-chain policy (`deny.toml`, `supply-chain/`) adds
  review cost either way.

**Recommendation:** hand-rolled thin FFI per engine inside a whitelisted
`-engine` crate, with wry as the documented fallback if hand-rolled slips.
The adapter surface (`WebViewHost`) is small — six methods — so the glue
saved by wry is modest next to its window-ownership mismatch.

### 2.6 Input forwarding

`WebView::event` currently returns `Ignored` (webview.rs:401-403). A real
engine needs pointer/scroll/key/IME injection — `EngineEvent` already
defines exactly this vocabulary (engine-bridge `engine.rs:96-136`):
`PointerMove`/`PointerButton`/`Scroll`/`Key`/`TextInput`/`Focus` in
surface-local physical pixels. The webview host should consume the same
event type so the `ExternalEngine` input path and the webview input path
share one adapter.

---

## 3. Target architecture — PDF

The `PdfDocument` contract is already engine-ready: `&self` methods with
interior mutability, `Option` raster returns, `Send + Sync` (doc.rs:176-211).
Three candidate engines:

### 3.1 pdfium (via `pdfium-render` or direct FFI)

- **API fit:** `FPDF_LoadDocument`/`FPDF_GetPageCount`/`FPDF_GetPageSize`/
  `FPDF_RenderPageBitmap` map 1:1 onto `open`/`info`/`page_size`/`render_page`.
  `PdfSource::Bytes` maps onto `FPDF_LoadMemDocument` — no temp file (kills
  the `write_temp_pdf` path at provider.rs:502-535 for that backend).
- **`pdfium-render` crate** provides maintained safe-ish bindings; it still
  needs a libpdfium binary (see §6 — binary distribution/licensing).
- **Direct FFI** is also viable: the used surface is ~15 C functions, a
  tractable hand-rolled shim.
- **Async:** pdfium supports progressive render (`FPDF_RenderPageBitmap_Start`/
  `Continue`) — enabling the tiled/async strategy in §3.4 without threads
  blocking the paint path. Interior mutability (`Mutex<RenderState>`)
  satisfies the `&self` contract.

### 3.2 MuPDF library embedding

- `fz_open_document`/`fz_new_pixmap_from_page` are equally direct fits and
  `mutool` is already the proven CLI rasterizer.
- **Licensing blocker:** libmupdf is **AGPL-3.0** (or commercial Artifex
  license). Linking it into an Apache-2.0/MIT workspace — even behind FFI —
  propagates AGPL obligations to every Martensite application. The CLI path
  avoids this because a subprocess is not a derivative work at link time.
  Library embedding almost certainly requires the commercial license.
  **Recommendation: exclude mupdf-as-library**; keep `mutool` as the
  subprocess fallback. Flagged in §6 for human confirmation.

### 3.3 Quartz / PDFKit (macOS)

- `PDFDocument`/`PDFPage` via `objc2-foundation`/`objc2-quartz-core`-family
  bindings: `PDFPage.bounds(for:)` → `page_size`; render into a
  `CGContext`-backed RGBA buffer → `PdfPageBitmap`. No binary distribution
  problem (system framework), decent fidelity.
- macOS-only coverage means pdfium is still needed for Windows/Linux;
  value is as a zero-dependency macOS backend. Lower priority — ship only if
  pdfium binary distribution stalls.

### 3.4 Sync render vs tiled/async; DPI scaling

Current semantics: `render_page(page, max_px)` synchronously returns ≤`max_px`
RGBA (doc.rs:199-210). The facade renders at a fixed `RENDER_MAX_PX = 1024`
(pdf_view.rs:35).

A real backend should evolve toward:

1. **Exact-size rasterization** — render at `bounds * scale_factor` rather
   than a fixed cap; the contract's `max_px` stays the budget, the widget
   passes its real size.
2. **Bitmap cache** — `Mutex<HashMap<(page, max_px), PdfPageBitmap>>`, the
   same lazy-cache pattern `SubprocessDocument::sizes` uses
   (provider.rs:247). Zoom changes must not re-rasterize identically.
3. **Tiled/async rendering** — for print-resolution zoom (`ZoomMode::Zoom`
   up to 4×, pdf_view.rs:36-38): render visible tiles on a worker pool;
   `render_page` returns the cached composite or `None` → the widget paints
   its existing placeholder (pdf_view.rs:462-465) and re-requests repaint on
   completion. The contract supports this *today* because `Option` +
   placeholder is already the failure path — no API change needed for v1 of
   async, though a readiness signal (event channel) is a future additive
   improvement.
4. **DPI scaling** — `max_px` already encodes resolution independence; a real
   backend renders at device pixels and the `Viewport::scale_factor`
   convention (engine-bridge `engine.rs:31-49`) applies for Retina.

---

## 4. Unsafe whitelisting plan

Workspace policy: `unsafe_code = "deny"` at `Cargo.toml:216-217`; exactly ten
crates carry `#![allow(unsafe_code)]` and AGENTS.md:178-207 names each with
its justification. Any new FFI crate must be added to that list with the same
justification text — the list is the audit surface.

### 4.1 New crates needed

| Crate | Unsafe contents | Whitelist justification (AGENTS.md entry draft) |
|-------|-----------------|--------------------------------------------------|
| `martensite-webview-engine` | ObjC msgSend/delegate impls (WKWebView), COM `ICoreWebView2` vtables, GLib/GTK or WPE-fdo C API, IOSurface/DXGI/dmabuf producer-side export | "embedded webview engine FFI (WKWebView/WebKit on macOS, WebView2 COM on Windows, WebKitGTK/WPE-fdo on Linux) plus producer-side surface export handles" |
| `martensite-pdf-engine` | pdfium C API (`pdfium-render` internally holds its own unsafe; direct FFI needs ours) — mupdf excluded per §3.2 | "PDF rasterizer FFI (pdfium `fpdf_*.h` C API); Quartz PDFKit via objc2 on macOS" |

No new unsafe is needed in `martensite-media-platform` (import side already
exists) or in the safe contract crates — `martensite-webview` stays
`#![forbid(unsafe_code)]` (lib.rs:44) and `martensite-pdf` stays pure.

### 4.2 Minimize-unsafe patterns (established in-tree)

1. **Thin FFI shim + safe wrapper split.** Follow `martensite-media-platform`'s
   structure: all `unsafe` in cfg-gated `macos.rs`/`windows.rs`/`linux.rs`
   modules (lib.rs:46-53), exposing safe `import_*` functions. For the webview
   crate: per-OS `wkwebview.rs`, `webview2.rs`, `wpe.rs` modules behind
   `#[cfg(target_os)]`, each exposing a safe `*WebView` type implementing
   `WebViewHost`.
2. **Local trait on foreign type.** The decoder tier's cycle-break rule:
   implement `WebViewHost`/`PdfProvider`/`PdfDocument` for the concrete engine
   types inside the engine crate (AGENTS.md:220-224 pattern). The engine crate
   depends on the contract crate, never the reverse — matching
   `martensite-webview-platform`'s dependency direction (lib.rs:18-26).
3. **Send + Sync via proxy, not transmute.** Thread-confined engine objects
   live on their owning thread; the `WebViewHost` impl is a proxy holding
   command/event channels — the state.rs:140-144 documented escape hatch.
   No `unsafe impl Send`.
4. **Panic guards at the FFI seam.** `swash` precedent: wrap engine calls in
   `catch_unwind` where C++ exceptions/ObjC exceptions can cross (AGENTS.md
   :213-214). Engine callbacks into Rust must be `extern "C"` shims that never
   unwind out — panic → log + quarantine, mirroring `ExternalEngines`' engine
   quarantine (engine-bridge `engine.rs:184-193`).
5. **Wire types stay in the safe crate.** No new wire-type duplication:
   `WebViewEvent`/`WebViewState`/`PdfPageBitmap` are already defined in the
   contract crates, so the engine crates need no `Cli*`-style mirror types
   (the `Cli*` wire-type duplication at provider.rs:39-150 exists only because of
   the platform→safe dependency direction).
6. **Audited-unsafe surface accounting.** Keep `unsafe` blocks countable:
   FFI call sites + ObjC `declare_class!`/COM impl macros only; every
   `unsafe fn` gets a `# Safety` contract comment — the existing convention
   (`import_external_texture`, media-platform lib.rs:319-324).

---

## 5. Feature-gating & platform matrix

### 5.1 Cargo wiring

Follow the decoder-tier precedent (`crates/martensite/Cargo.toml:75-85`):
one umbrella feature plus per-backend features, all off by default.

```toml
# martensite facade (proposed)
webview-engine        = ["dep:martensite-webview-engine"]   # all engines for target
webview-engine-wkwebview / -webview2 / -wpe / -webkitgtk    # per-engine
#   (-wpe is the primary Linux engine per §2.4/§5.3; -webkitgtk the alternate)
pdf-engine            = ["dep:martensite-pdf-engine"]
pdf-engine-pdfium / -quartz                                 # per-engine
```

```toml
# martensite-pdf (proposed, mirrors `platform`)
engine = ["dep:martensite-pdf-engine"]
```

### 5.2 Backend selection order (graceful degradation)

Extend the existing probe-then-stub functions rather than replacing them:

- `martensite_webview_platform::default_platform_host()` (lib.rs:58-64)
  becomes a three-tier select — or better, a sibling
  `default_embedded_host()` in the engine crate so the platform crate's
  zero-unsafe property is preserved:
  `WkWebViewHost/WebView2Host/WpeWebViewHost` (engine init succeeds) →
  `FetchWebView` (curl present) → `SystemBrowserWebView` (always).
  Engine-init failure (missing WebView2 runtime, no GTK) must degrade
  in-process, not panic — probe at `open`/construction and return the next
  tier, the `CliPdfProvider`-style honest `backend_name` convention
  (pdf/platform.rs:77-79).
- `martensite_pdf::default_pdf_provider()` (doc.rs:290-309): engine provider
  (`"pdfium"`/`"quartz"`) → `CliPdfProvider` (`"poppler"`/`"mupdf"`) →
  `NullPdfProvider` (`"null"`).

### 5.3 Platform matrix

| OS | WebView engine | Raster transport | PDF engine | Fallback today |
|----|----------------|------------------|------------|----------------|
| macOS | WKWebView | snapshot → `CpuFrame`; IOSurface *if* obtainable (§2.2) | pdfium; Quartz as alt | fetch/system-browser; poppler/mutool CLI |
| Windows | WebView2 | `CapturePreview` → `CpuFrame`; DXGI share needs **Vulkan-backend device** (windows.rs:18-24) | pdfium | same |
| Linux | WebKitGTK (GTK snapshot) or **WPE-fdo** (DMA-BUF — best zero-copy fit) | `HardwareHandle::DmaBuf` → `import_dmabuf` (Vulkan) | pdfium | same |
| iOS/Android | WKWebView (iOS) / Android WebView (Android) | future tier — mobile shells not in scope | pdfium | — |
| headless/CI | `SimulatedWebView` (state.rs:155 impl at simulated.rs:130) | n/a (`has_surface=false`) | `BlankPdfDocument`/`NullPdfProvider` | — |

The default build (no engine features) compiles exactly as today: zero unsafe
added, zero new system deps — critical because `cargo test --workspace` and
`--all-features` CI must still pass (AGENTS.md CI rules; decoder features set
the precedent that `-dev` system packages get installed for all-features jobs).

---

## 6. Sequencing

Recommended order, with rationale:

**Phase 1 — `martensite-pdf-engine` (pdfium).**
- Highest unlock-to-risk ratio: real PDF documents with no compositing
  problem — `render_page` already returns CPU bitmaps the facade paints via
  `push_image` (pdf_view.rs:455-459). Zero surface machinery required.
- Validates the whole machinery this doc proposes at smallest blast radius:
  new unsafe-whitelisted crate, AGENTS.md entry, FFI-shim style, engine
  feature flag, provider-chain extension.
- Delivers the `Bytes`-without-temp-file win (provider.rs:297).

**Phase 2 — WebView contract extension + WKWebView snapshot path (macOS).**
- Add the additive surface contract to `WebViewHost` (§2.1) and teach
  `WebView::paint` to emit `PaintCommand::External`/`push_image` when
  `has_surface()`. This is public-API surface — needs its own review.
- WKWebView first because `objc2` tooling is the most mature Rust FFI of the
  three and the snapshot path is a public API — real navigation events +
  `eval_js` + actual pixels land even before surface sharing is solved.
- Input forwarding via `EngineEvent` (§2.6) lands here.

**Phase 3 — WPE-fdo WebKit (Linux).**
- Counterintuitively ahead of WebView2: WPE's DMA-BUF export is the *only*
  clean zero-copy fit (`NativeFrame::DmaBuf`, `import_dmabuf` on the Vulkan
  backend, surface.rs:185-193). It exercises the full
  `Engine`→ring→`WgpuHost::composite_front` path end-to-end and becomes the
  reference implementation for engine-side surface production.
- If WPE packaging proves hostile, fall back to WebKitGTK snapshot raster.

**Phase 4 — WebView2 (Windows).**
- Start on `CapturePreview` CPU path (supported API, no backend constraint).
- Zero-copy is gated on the DXGI→Vulkan import constraint
  (windows.rs:18-24): ship it only alongside a device-backend story
  (§6.3 open question). Largest risk, sequenced last.

**Phase 5 — GPU surface upgrade for WKWebView (macOS).**
- Only if a legal IOSurface export route verifies (§2.2); otherwise the
  snapshot path remains the macOS ceiling. Independent of Phases 3-4.

Dependencies honored: Phase 1 precedes all WebView work because it proves the
`-engine` crate + whitelist pattern. The `PaintCommand::External` consumer
path (`WgpuHost`, `PaintList::segments`) already exists, so no render-pipeline
work blocks Phase 2-3 — only the contract addition in Phase 2 gates them.

---

## 7. Open questions — decisions needing a human

1. **wry/tao vs hand-rolled FFI.** §2.5 recommends hand-rolled; confirm —
   wry would cut engine-glue cost substantially but reintroduces child-window
   compositing and a second winit lineage. *Verify current wry version for
   any offscreen/texture-output API before deciding.*
2. **pdfium binary distribution & licensing.** pdfium is BSD-3 (fine), but a
   ~10-30 MB `libpdfium` per-OS artifact needs a distribution story:
   `pdfium-render` + user/system lib? `pdfium-prebuilt`-style crate? Vendored
   binaries in `packages/`? This interacts with `deny.toml`/`supply-chain/`
   policy and `cargo audit`. Building pdfium from source is a Chromium
   toolchain — effectively ruled out. **Confirm mupdf AGPL exclusion** (§3.2).
3. **wgpu device backend on Windows.** Zero-copy WebView2 requires a
   Vulkan-backend device (`VULKAN_EXTERNAL_MEMORY_WIN32`,
   windows.rs:18-24). Options: (a) request Vulkan when webview-engine is
   active — a device-init decision touching `martensite-wgpu::device`; (b)
   accept CPU readback on Windows; (c) investigate `D3D11On12`/D3D12-shared
   paths wgpu-hal doesn't currently expose. *Needs a spike.*
4. **WKWebView legal IOSurface access.** No public API exports the live
   backing surface. Is `CARenderer`-into-IOSurface or layer-copying
   acceptable (App Store rules, private SPI)? If not, macOS stays on
   snapshots — confirm acceptance.
5. **WebKitGTK vs WPE-fdo on Linux.** WPE gives DMA-BUF but thin distro
   packaging; WebKitGTK is everywhere but forces GTK-in-process + a
   snapshot raster path. Could ship both behind per-engine features (§5.1).
6. **`WebViewHost` surface contract shape.** Additive method
   (`fn surface(&self) -> Option<(BridgeHandle, SurfaceId)>`), a second
   capability trait (`SurfaceHost`), or `Engine`-impl-per-webview? Must stay
   semver-compatible — this trait is published public API. The regenerated
   `docs/API_SURFACE_AUDIT.md` now covers `martensite-webview`/`martensite-pdf`
   and can serve as the baseline for this contract change.
   Also: does `WebViewEvent` need a
   `FrameReady`-style variant, or is the bridge's ready-event queue enough?
7. **Engine process isolation.** `Engine` impls run in-process and trusted.
   Two distinct failure modes: (a) a *Rust* panic inside the engine —
   `catch_unwind` can quarantine it under `panic = "unwind"`, but
   Martensite's release profile uses `panic = "abort"`, which aborts the
   process uncatchably (engine-bridge `engine.rs:184-193`); and (b) a
   *native* crash — a segfault or `abort()` inside web-engine C++ — which
   kills the host under **any** panic profile; `catch_unwind` is irrelevant
   there. WKWebView/WPE already sandbox rendering in helper processes, but
   WebView2 composition + our import path keep the GPU handoff in-process.
   Accept, or spec a helper-process transport via `NativeFrame` handles
   (which exist precisely for cross-process, frame.rs:127-150)?
8. **Mobile tier.** WKWebView-on-iOS and Android WebView share the contract
   but add surface machinery (GLESTexture/ SurfaceTexture). In scope for
   this design or a follow-up doc? Currently out of scope per §5.3.
9. **WebView2 runtime distribution.** Evergreen runtime presence probe +
   bootstrapper bundling vs `WebView2LoaderStatic.lib` static link — a
   packaging/licensing decision parallel to pdfium's.

---

## Appendix A — Verified in-tree facts used above

| Claim | Source |
|-------|--------|
| `WebViewHost` is `Send + Sync`; thread-confined engines expose thread-safe handles | `crates/martensite-webview/src/state.rs:140-155` |
| Fetch backend is synchronous; `Stop` no-op; `eval_js`→Err; `has_surface`→false | `crates/martensite-webview-platform/src/fetch.rs:11-22,247-255,274-276` |
| System-browser `LoadFinished` means "handed off" | `crates/martensite-webview-platform/src/browser.rs:6-9` |
| PDF contract is `&self` + interior-mutability + `Send + Sync` | `crates/martensite-pdf/src/doc.rs:176-211` |
| CLI rasterizer spawns per page; temp-file for `Bytes`; 60/120 s run caps; metadata-first mupdf page sizes | `crates/martensite-pdf-platform/src/provider.rs:287-298,375-433,571-621,1245-1300` |
| `HardwareHandle::{IoSurface,DxgiSharedHandle,DmaBuf}` + `import_external_texture`/`import_cpu_memory` exist | `crates/martensite-media-platform/src/{surface.rs:165-210,lib.rs:345-408,486}` |
| DXGI import requires Vulkan-backend wgpu device; DX12 cannot import D3D11 shared handles | `crates/martensite-media-platform/src/windows.rs:18-24` |
| `PaintCommand::External` + `PaintList::segments` paint-order compositing | `crates/martensite-core/src/paint.rs:784-791,845-857,1681` |
| `Engine`/`SurfaceRing`/`FrameSync`/`NativeFrame`/`CpuFrame` producer protocol | `crates/martensite-engine-bridge/src/{lib.rs:30-46,engine.rs,frame.rs:48-190}` |
| `WgpuHost::composite_front` zero-copy same-device composite | `crates/martensite-wgpu/src/external.rs:1-8,863` |
| `ExternalEngine` widget precedent | `crates/martensite/src/widgets/external.rs:95-156` |
| Facade paints placeholder when `!has_surface`; `event` is `Ignored` | `crates/martensite/src/widgets/webview.rs:401-403,486-501` |
| `PdfView` renders via `render_page`→`push_image` at fixed 1024 px | `crates/martensite/src/widgets/pdf_view.rs:35,455-459` |
| Ten-crate unsafe whitelist is the audit surface | `AGENTS.md:178-207`; `Cargo.toml:216-217` |
| Host-mode (Direction A) embedding is the binding decision | `docs/adr/ADR-0033-host-mode-external-surface-embedding.md` |

## Appendix B — External-API claims flagged for verification

Not derivable from this codebase; verify before implementation:

- WKWebView `takeSnapshot`, `pageZoom`, KVO keys; absence of a public
  compositor-surface export (§2.2).
- `ICoreWebView2CompositionController` + `IDCompositionVisual` integration,
  `CapturePreview` behavior, DXGI surface extraction routes (§2.3).
- `wpebackend-fdo` exportable API current shape; WebKitGTK `gtk_widget_snapshot`
  → texture path (§2.4).
- `pdfium-render` crate API/version status; `FPDF_RenderPageBitmap_*`
  progressive render (§3.1).
- `objc2-web-kit` / `objc2-quartz-core` binding coverage (§2.2, §3.3).
- wry/tao current offscreen-rendering capability (§2.5).
- `WKWebView` main-thread requirement — hard requirement vs. convention
  (§2.2 Threading).
- WebView2 composition output pixel/API family — the "D3D11" premise behind
  the biggest technical risk; a D3D12-shared output on newer SDKs would
  relax the Vulkan-backend constraint (§2.3).
- tao↔winit version alignment — whether wry's window layer tracks a winit
  lineage compatible with Martensite's winit 0.31-beta event loop (§2.5).
