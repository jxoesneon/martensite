# Martensite Migration Guide (0.x to 1.x)

**Document Identifier:** DOC-MIGRATE-0x-1x

This guide documents the API deltas across the `0.x` series through
`v0.17.0` and what remains deliberately unstable until the `1.0.0` API
freeze (milestone v0.18.0 → v1.0.0-rc). It describes the API as it
actually exists in the crate sources; where an earlier revision of this
document named APIs that never shipped, those entries are corrected in
[Corrections to the previous revision](#corrections-to-the-previous-revision).

## Scope and stability contract

- The `0.x` series makes no SemVer stability promise; each minor release
  may break API. `cargo-semver-checks` runs in CI from v0.18.0 onward to
  make the remaining breaks visible.
- `1.0.0` freezes the public surface audited in
  `docs/API_FREEZE_AUDIT.md`. Nothing in this guide is a guarantee about
  pre-`rc` releases.

## Architectural invariants

These have been stable since the early `0.x` series and will not change
at 1.0.0:

- **Generational arena** ([ADR-0001](adr/ADR-0001-generational-slotmap-arena.md)).
  Widgets live in `martensite_core::WidgetArena` behind 64-bit
  `WidgetId` handles. There is no shared ownership of widget nodes —
  state lives in `Signal<T>` or inside the widget itself.
- **Push-pull reactivity** ([ADR-0002](adr/ADR-0002-push-pull-reactive-signals.md)).
  `Signal::set` marks dependents dirty; `Memo`/`Effect` re-evaluate on
  demand. Custom widgets read signals via `.get()` inside
  `measure`/`paint` (see `docs/tutorials/02-reactive-state.md`).
- **Two-pass layout finality** ([ADR-0003](adr/ADR-0003-two-pass-taffy-layout.md)).
  `Widget::measure` proposes a size under `LayoutConstraints`;
  `Widget::layout` receives the final `Rect`. Geometry is fixed for the
  frame after `layout` returns.

## Breaking changes by release

### v0.17.0 — Platform expansion

- **winit 0.31 trait-object APIs.** `martensite-accesskit-winit` adapter
  constructors now take `&dyn ActiveEventLoop` and `&dyn Window` (winit
  0.31 hands out trait objects through `ApplicationHandler`). If you
  construct adapters directly, pass `&*event_loop` / `&*window` as
  trait-object references.
- **Pointer events.** `martensite_window::event::PointerId` is now a
  `u64` newtype (was `u32`), and `PointerEvent` carries a
  `kind: PointerKind` field (`Mouse`/`Touch`/`Tablet`/`Unknown`). Touch ids are
  offset so they cannot collide with the mouse's primary id. Match
  sites constructing `PointerEvent` literally need the new field.
- **Mobile accessibility FFI moved.** iOS/Android adapter glue moved
  out of `martensite-access` into the new
  **`martensite-access-platform`** crate (`ios::IosAdapter`,
  `android::AndroidAdapter`). It is one of the whitelisted
  `unsafe_code` crates — `martensite-access` itself remains
  `#![forbid(unsafe_code)]`-clean. Depend on `martensite-access-platform`
  only when targeting mobile.
- **Overlay layer and APG widgets.** `martensite_core::OverlayLayer`
  (also re-exported in `martensite::prelude`) is the arena-owned popup
  layer. Widget hooks: `Widget::sync_overlay(&mut OverlayLayer)` to
  publish popups, `Widget::tick(Duration) -> bool` for time-dependent
  state; `WidgetArena::tick` and `MartensiteAccessBridge::tick` are the
  per-frame entry points. AT actions can now target internal children
  and popup nodes via `ActionTarget::{Arena, Internal, Overlay}`.
- **New widgets.** `Slider::new(min, max)`,
  `RadioGroup::new(labels)`, `Dropdown::new(options)`,
  `ScrollView::new(content)`, `Tabs::new()`,
  `Tooltip::new(trigger, text)` (+ `TooltipBubble::new(text)` for the
  popup chrome) — all in `martensite::widgets` / `martensite::prelude`.
- **Focus transitions.** `FocusManager::apply_focus_request(&mut arena,
  WidgetId)` performs a focus transition with `FocusLost`/`FocusGained`
  dispatch; widgets request focus by returning
  `EventResponse::CaptureFocus`.
- **wasm32 target.** `martensite-window`, `-wgpu`, `-text`,
  `-clipboard`, `-dnd`, `-access`, and the umbrella crate compile for
  `wasm32-unknown-unknown`. `martensite_access::web::WebA11yBridge` is
  the minimal hidden-DOM/ARIA mirror — it is not a full AccessKit
  adapter. Compile-verified; browser runtime status is tracked in
  `docs/PLATFORM_SUPPORT.md`.
- **Desktop platforms.** `accesskit_winit` adapter construction moved
  to `&dyn` references (first bullet) — this is the only desktop-facing
  signature change in v0.17.0.

### v0.16.0 — Hardware media pipeline

- **Decoder wire types moved.** `VideoCodec`, `EncodedPacket`,
  `DecodedFrame`, `DecoderConfig`, `DecodeStats`, `HdrSideData`, and
  `DecodeError` are defined in **`martensite-media-platform::decoder`**
  and re-exported by `martensite-media::decoder`. Update imports if you
  referenced an earlier location; prefer the `martensite-media`
  re-export path.
- **`VideoDecoder` trait.** Producer-side contract
  (`init`/`send_packet`/`try_recv_frame`/`end_of_stream`/`flush`/
  `negotiated_format`/`hdr_metadata`/`stats`), `Send + Sync`,
  object-safe. `end_of_stream` must be called before draining or
  reorder-buffered frames never emit; `flush` re-arms the keyframe
  gate. A `MockDecoder` with configurable reorder latency ships for
  tests.
- **Platform backends are feature-gated.** `decoder-videotoolbox`
  (macOS), `decoder-mf` (Windows), `decoder-vaapi` (Linux),
  `decoder-ffmpeg` (anywhere FFmpeg is installed). The default build is
  unaffected.
- **`HardwareHandle::DmaBuf` is multi-plane.** The variant now carries
  `objects: Vec<fd>`, `planes: Vec<DmaBufPlane { object_index, offset,
  stride }>`, and a surface-wide `modifier`. Code matching on the old
  single-fd shape must be updated.
- **`MediaView` decoder wiring.** `MediaView::new().with_decoder(..)`,
  `feed_packet`, `advance(now_nanos)`, `next_wait_nanos`,
  `end_of_stream`, `drop_rate_pct`, `queued_frames`.
- **Windows zero-copy limits (documented, not a regression).**
  `import_dxgi_texture` requires a Vulkan-backend wgpu device with
  `Features::VULKAN_EXTERNAL_MEMORY_WIN32`; NV12/P010 return
  `UnsupportedFormat` (callers fall back to `import_cpu_memory`), and
  the DX12 backend cannot import D3D11 shared handles at all.

### Vendored-fork package renames

Three upstream crates are vendored in-tree under `martensite-*` names.
If your `Cargo.toml` or code references the upstreams, substitute:

| Upstream | Martensite crate | Reason |
|---|---|---|
| `accesskit_winit` 0.34 | `martensite-accesskit-winit` | Patched for winit 0.31.0-beta.3; temporary — removed once upstream supports winit 0.31. |
| `netrender-vello` / `vello` 0.10 | `martensite-vello` | Byte-compatible republish built against wgpu 30. |
| `cosmic-text` | `martensite-cosmic-text` | Vendored fork with `fontdb` 0.24 ahead of upstream; patch discipline in `docs/VENDORED_FORKS.md`. |

`martensite-vello` and `martensite-cosmic-text` opt out of the
doc-example requirement via
`#![allow(missing_docs)]`/`#![allow(rustdoc::broken_intra_doc_links)]` —
they are upstream code, not Martensite API surface.
`martensite-accesskit-winit` is internal-only (do not depend on it
outside the workspace). The full maintenance policy is in
`docs/VENDORED_FORKS.md`.

## Corrections to the previous revision

The previous revision of this document listed "1.x" APIs that were
aspirational and never shipped. The current API is:

- **App bootstrapping.** There is no `martensite::run(main_widget)` and
  no `App::run`. `App::build()` returns an `AppBuilder` whose
  `build()` produces `AppConfig` (convertible to
  `martensite_wgpu::OrchestratorConfig`). The event loop is wired
  explicitly through `winit::application::ApplicationHandler` plus
  `martensite_window::WindowManager` — see
  `docs/tutorials/01-project-setup.md` §3 and `examples/engine_embed`.
- **No `WidgetExt` / free `text()` constructors.** Widgets are plain
  structs constructed with `new(...)` plus chained configurator methods
  (e.g. `Container::new().padding_uniform(16.0)`,
  `Text::new("Hello")`). There is no `.padding(10.0)` universal
  modifier trait.
- **No `cx.signal()` / `cx.spawn()`.** Reactive state is created with
  `martensite_reactive::{Signal::new, create_signal, create_memo,
  create_effect}`. There is no async-spawn context API in v0.17.0;
  bridging an executor to the repaint loop is open work.
- **No `Theme::builder` / `cx.theme()`.** Theming is
  `martensite_theme::{Theme, ThemeDictionary, ThemeMode, ThemeToken,
  TokenKey}`: `Theme::new(name)`, `set`/`get`/`merge` on `TokenKey`s,
  `ThemeDictionary::theme(mode)` for light/dark selection, plus the
  Oklab pipeline (`Oklab`, `apca_contrast`, `wcag_contrast`) and the
  `ThemeTransition` GPU uniform path.
- **Trait rename.** `Widget::draw` → `Widget::paint(&self, cx: &mut
  PaintContext)` recording into `cx.list` (a `PaintList`), alongside the
  mandatory `measure`/`layout` pair — see
  `docs/tutorials/03-custom-widget.md`.

## Migrating a 0.x application — checklist

1. Pin `martensite = "0.18.0"` (and `winit = "0.31.0-beta.3"` if you wire
   the event loop yourself — it must match the workspace pin).
2. Replace `Rc<RefCell<...>>` widget graphs with `WidgetArena` +
   `WidgetId`; move shared state into `Signal<T>`.
3. Update `Widget` impls: `measure`/`layout` are mandatory; paint via
   `PaintContext`; add `accessibility(&self, node: &mut accesskit::Node)`
   for AT-visible widgets.
4. If you handle `PointerEvent`, add `kind: PointerKind`; `PointerId`
   is `u64`.
5. If you construct AccessKit winit adapters, pass `&dyn
   ActiveEventLoop` / `&dyn Window`.
6. If you use media decode, import wire types from
   `martensite_media::decoder` and enable the matching `decoder-*`
   feature; call `end_of_stream` before draining.
7. On mobile, add `martensite-access-platform` for the native AT
   adapters (iOS/Android only).
8. Follow compiler diagnostics; all non-whitelisted crates are
   `unsafe_code = "deny"`.
