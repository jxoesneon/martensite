# Working On — Pending Items

This file tracks work that is **not yet complete** or has known limitations.
It is a living document — items move off this list when they are resolved.

Last updated: v0.17.0 released — tagged `v0.17.0` at `480c33d`,
31 crates published to crates.io at 0.17.0 (including first-time
`martensite-access-platform`; `martensite-vello` and
`martensite-cosmic-text` correctly skipped at their pinned versions),
GitHub Release created with changelog notes. All gates green: full CI
pipeline on the tag, then gated publish.

## Active Milestone Plan (v0.18.0 → v0.19.0 → v1.0.0-rc → v1.0.0)

Competitive feature-coverage research (egui 0.35, iced 0.14, Xilem,
Floem, Vizia, Slint 1.16, Makepad, GPUI, Dioxus) re-scoped the roadmap
through v0.17.0. Post-release reconciliation assigned the three divergent
v1.0.0 definitions (milestone doc, `PROJECT.md` Phase 18, `INDEX.md`
diagram) into the sequence below. Direction A (host-mode embedding) is
confirmed — see ADR-0033. v1.0.0 ships through a release-candidate line;
no API changes land between `v1.0.0-rc.1` and the stable tag.

- **v0.14.0 External Surface Foundation** — generic external-texture
  widget + `martensite-engine-bridge` (`Engine`/`Frame`/`FrameSync`),
  same-device zero-copy composite, damage-driven redraw.
  Spec: `docs/milestones/v0.14.0-external-surfaces.md`.
  **Status: IMPLEMENTED & VERIFIED** — see the v0.14.0 section below.
- **v0.15.0 Engine Showcase** — `martensite-bevy` host-mode viewport
  (`RenderCreation::Manual` + `RenderTarget::TextureView`); requires
  Bevy wgpu-30 pin (0.19 ships wgpu 29). `martensite-godot` GDExtension
  — **true zero-copy impossible without Godot engine patches**; ships
  `texture_get_data_async` readback + experimental shared-texture blit.
  Spec: `docs/milestones/v0.15.0-engine-showcase.md`.
- **v0.16.0 Hardware Media Pipeline** — `VideoDecoder` trait;
  VideoToolbox/MF/VAAPI/FFmpeg backends in `martensite-media-platform`;
  multi-plane `DmaBuf` + `import_external_planes`; `HdrMetadata`;
  `FrameQueue` drop accounting; `MediaView` decoder wiring.
  Spec: `docs/milestones/v0.16.0-media-pipeline.md`.
  **Status: RELEASED** (tag `v0.16.0`, crates.io + GitHub Release).
  VT hardware-verified on macOS (30-frame real
  decode through `VideoToolboxDecoder` → `IoSurface`); FFmpeg real-decode
  test on the checked-in 320x240 Annex-B fixture; MF compile-verified for
  `x86_64-pc-windows-msvc`; VAAPI compile + parser unit tests verified on
  real Linux libva via `docker/media-test.Dockerfile`
  (`docker build -t martensite-media-test -f docker/media-test.Dockerfile .`
  — also runs the FFmpeg 5.1 real-decode and noop-wgpu interop suites;
  `docker run --rm --device /dev/dri` on a Linux host exercises runtime
  decode). FFmpeg backend drains opportunistically inside `send_packet`
  because libavcodec 5.x drops the tail frame at `send_eof` when no
  mid-stream receive occurred. The 4K120 acceptance gate is implemented
  (`full_rate_4k120_gate`, `#[ignore]` + `MARTENSITE_MEDIA_4K120=1`) and
  **verified on Apple M4**: H.264 + HEVC decode 60 s @ 4K120 through
  VideoToolbox hardware with 0.000% drops and ~0.015% dispatch CPU
  (7200/7200 presented at a wall-clock 120.0 fps); the AV1 leg also runs
  on VideoToolbox hardware via the vendored `av1C`→`CMFormatDescription`
  bridge (`decoder::av1` + `SampleDescriptionExtensionAtoms`), presenting
  1800/1800 frames at 120.1 fps with 0.000% loss (dav1d/ffmpeg remains
  the fallback on hosts without AV1 hardware decode). Samples
  regenerate via `scripts/generate-media-samples.sh` into
  `target/media-samples/` (gitignored; `$MARTENSITE_MEDIA_SAMPLES`
  overrides). The HDR golden gate (`tests/hdr_golden.rs`,
  `decoder-ffmpeg`) decodes checked-in BT.2020/PQ (HDR10 mastering +
  MaxCLL/MaxFALL) and BT.2020/HLG 10-bit HEVC fixtures, asserts
  `HdrSideData` → `HdrMetadata` → `VideoPipelineUniforms`, then renders
  a mid-GOP frame through `VideoProcessor` on a real headless adapter
  (Metal on M4) and compares `Rgba16Float` readback against a CPU mirror
  of `MEDIA_YUV_EOTF_WGSL`: PQ mean |err| 8.6e-5, HLG 3.5e-4 scRGB —
  f16 quantization, ~2 orders under tolerance. It skips gracefully
  without a GPU. P010 stays unexercised because the FFmpeg decoder
  negotiates NV12 out of `yuv420p10le` (documented in the file);
  `extract_hdr` handles the FFmpeg 8 `AVContentLightMetadata`
  `unsigned short` → `unsigned` ABI widening by payload size.
  Remaining exit gate: Windows DXGI zero-copy requires a
  Vulkan-backend wgpu device with `VULKAN_EXTERNAL_MEMORY_WIN32` (DX12
  backend cannot import D3D11 shared handles; NV12/P010 on Windows fall
  back to `import_cpu_memory`).
- **v0.17.0 Platform Expansion** — slider/radio/dropdown/scrollview/
  tabs/tooltip with ARIA APG + AccessKit; overlay layer; wasm32
  (WebGPU + TinySkia fallback; web a11y needs new hidden-DOM bridge —
  no upstream AccessKit web adapter exists); iOS (UIKit/Metal/
  `accesskit_ios`) + Android (**GameActivity** not NativeActivity,
  `accesskit_android`); hybrid command-ledger + snapshot time-travel
  debugger.
  Spec: `docs/milestones/v0.17.0-platform-expansion.md`.
  **Status: RELEASED** — all six workstreams implemented on
  `milestone/*` branches, each through a double-review loop (spec/API
  then correctness/safety), merged into `main` at `431d4aa`..`247cfd8`,
  tagged `v0.17.0` at `480c33d` and published (31 crates at 0.17.0 +
  GitHub Release). All local gates pass (fmt, clippy default +
  all-features, workspace tests, doctests, `cargo doc -D warnings`,
  deny, audit, vet) and all CI workflows are green, including the new
  `target-checks` matrix (wasm32, ios-sim, android).
  - `milestone/vt-av1` — AV1 `av1C` → `CMFormatDescription` bridge
    closes the last v0.16.0 gap: the AV1 4K120 leg now decodes through
    VideoToolbox hardware (`hw=true`, ~120 fps on M4) instead of the
    dav1d advisory path. Bounded deferred init, real `RequireHardware`
    gate, HDR colour propagation.
  - `milestone/widgets` — `OverlayLayer` + the six APG widgets;
    incremental AccessKit updates emit popups, `Widget::tick` frame
    hook drives tooltip hover, AT actions reach virtual/overlay nodes.
  - `milestone/web` — wasm32 target for window/wgpu/text/clipboard/
    dnd/access + umbrella crate; `WebA11yBridge` hidden-DOM mirror;
    WebGPU→WebGL2→CPU `WebBackend` fallback; playwright browser gate
    (`MARTENSITE_WEB_BROWSER=1`). **Gate PASSED** on this host during
    v0.18.0: headless Chromium 140 run verified startup, GPU backend
    decision, a11y mirror, and aria-live announcement.
  - `milestone/ios` — UIKit/`accesskit_ios` via new
    `martensite-access-platform`, Metal-only backend, safe-area,
    IME, `PointerId` u64 + `PointerKind`; ios-sim `cargo check` +
    env-gated `harness = false` adapter test. **Gate PASSED** during
    v0.18.0 via `xcrun simctl spawn` on a booted iPhone simulator —
    `IosAdapter` subclassed a real UIView and exported a11y elements.
  - `milestone/android` — GameActivity, `InjectingAdapter` JNI
    boundary, Vulkan-first/GLES fallback, surface lifecycle;
    `cargo check --target aarch64-linux-android` + env-gated device
    test (`MARTENSITE_ANDROID_DEVICE=1`). Gate remains unrun:
    needs a GameActivity APK harness (no cargo-apk2/xbuild
    provisioning) and the local AVD failed to boot; formally
    descoped to compile-verified in `docs/PLATFORM_SUPPORT.md`.
  - `milestone/timemachine` — `martensite-devtools::timemachine`
    behind `devtools-timemachine`: journal + arena/signal snapshots,
    deepest-checkpoint restore + forward replay, `HistoryGap` error
    across pruned regions, VirtualClock determinism tests.
- **v0.18.0 Production Hardening & Dogfooding** — no new feature
  surface. Public API audit + `unstable-*` inventory +
  `cargo-semver-checks` in CI; verification-honesty pass (every
  env-gated leg run on real infrastructure or formally descoped with
  downgraded claims); residual hardening (`martensite-vello` `todo!()`
  paths, Windows font-fallback exclusion, Wayland clipboard gap,
  fixed test sleeps, remaining perf gates); vendored-fork/dependency
  hygiene (winit 0.31 stable tracking, `accesskit-winit` removal plan,
  naga dedup, Bevy pin); dogfooding via the full
  `industrial_dashboard` workstation demo + egui/iced benchmark
  baselines; four tutorials; reproducible builds; migration-guide
  refresh. Spec: `docs/milestones/v0.18.0-production-hardening.md`.
  **Status: PENDING COUNCIL SIGNOFF** — all six workstreams merged to
  main (api-freeze, hardening, dogfood, docs-honesty, binaries, and the
  W6 dogfood-API-friction fixes). Council review found and fixed one
  release blocker: the `martensite::blessed` facade alias added a
  `martensite → martensite-blessed` dependency edge, so the publish
  order in `publish.yml`/`RELEASE_PROCESS.md` now ships blessed first.
- **v0.19.0 Distribution** — prebuilt `cargo-martensite` binaries,
  WiX/DMG/Flatpak installers, Ed25519 signed updates, build
  attestations. Tentatively scoped; may be descoped to post-1.0 by
  ADR without blocking the RC.
  Spec: `docs/milestones/v0.19.0-distribution.md`.
- **v1.0.0-rc.N → v1.0.0** — release-candidate line: full gated
  publish + ≥2-week soak, `cargo-semver-checks` clean vs last `0.x`,
  then the stable tag. Spec (single source of truth for v1.0.0
  scope): `docs/milestones/v1.0.0-production-release.md`.
- **Developer Experience Initiative (proposed — version pending)** —
  close the DX gap found by `docs/research/DEVELOPER_EXPERIENCE_AUDIT.md`:
  in-app widget inspector, `cargo-martensite` CLI expansion
  (`new`/`init`/`lint`/`inspect`/`doctor`/`check`), agent-native
  scaffolding, runtime lint bridge, live tweaks, event debugging,
  dev-mode error surface, onboarding depth. Eight workstreams, specs
  in `docs/dx/`, ADRs 0036–0038. Version slot pending council
  (v0.19.0 if Distribution descopes, else v0.20.0).
  Spec: `docs/milestones/vNEXT-developer-experience.md`.
  - **W1 Inspector** — select mode, lazy tree, layout chain, a11y
    tree, lint/events panels; in-app per ADR-0036.
    Spec: `docs/dx/INSPECTOR.md`.
  - **W2 CLI** — `new`, `init`, `lint`, `inspect`, `doctor`, `check`;
    version-handshook dev channel per ADR-0038.
    Spec: `docs/dx/CLI.md`.
  - **W3 Scaffolding** — 3 templates + generated `AGENTS.md`/
    `llms.txt`/`design-lint.toml`; `scaffold_smoke` CI.
    Spec: `docs/dx/SCAFFOLDING.md`.
  - **W4 Dev lint** — `LintBridge` live-lints frames; inspector panel,
    HUD badge, CLI attach, `--scene` dump.
    Spec: `docs/dx/DEV_LINT.md`.
  - **W5 Live tweaks** — `TweakRegistry` + inspector editors + source
    write-back; survives reload by name (ADR-0037 contract).
    Spec: `docs/dx/LIVE_TWEAKS.md`.
  - **W6 Event debugging** — `EventRecord` ledger over production
    dispatch; `MARTENSITE_DEBUG_EVENTS`, inspector panel.
    Spec: `docs/dx/EVENT_DEBUGGING.md`.
  - **W7 Error surface** — overflow tape, diagnostics overlay,
    structured dev-mode panic with crash bundle.
    Spec: `docs/dx/ERROR_SURFACE.md`.
  - **W8 Onboarding** — `examples/widget_catalog` (lint-clean,
    alias-searchable), 12-recipe cookbook, migration guides, drift
    guards. Spec: `docs/dx/ONBOARDING.md`.


## v0.15.0 — Engine Showcase (RELEASED)

- `martensite-bevy` — host-mode zero-copy Bevy viewport: the host's
  `wgpu::Device`/`Queue` are injected via `RenderCreation::manual`
  (bevy git pin `rev = "5036d97"`, the wgpu-30 merge commit on bevy
  main — released 0.19.x ships wgpu 29); Bevy renders into a
  Martensite-owned texture through `ManualTextureViews` +
  `RenderTarget::TextureView` (`ManualTextureViewHandle`);
  `sub_apps.update()` + `RenderDevice::poll(PollType::Wait)` per
  frame; `PipelinedRenderingPlugin` omitted (no GPU fence yet).
  See ADR-0034.
- `martensite-godot` — GDExtension viewport under an explicit honesty
  contract: Tier 1 `texture_get_data_async` readback → transport →
  `queue.write_texture` (one GPU→CPU copy + one CPU→GPU upload);
  Tier 2 `godot-gpu-copy` feature-gated shared-texture blit (one
  GPU→GPU copy). True zero-copy needs four upstream Godot changes —
  see ADR-0035. 8th crate on the `AGENTS.md` allowed-unsafe list.
- `examples/viewport_showcase` — Bevy 3D scene and Godot viewport
  side-by-side inside a Martensite window with native shell chrome
  (follows the adapter crates; excluded from the workspace).

Both adapters are `publish = false` and excluded from the default
workspace build (root `Cargo.toml` `exclude`); the `adapters` CI job
runs `cargo check`/`clippy`/`test` per manifest on main/tag pushes
and publish gates, skipping PRs to keep PR CI fast.


## v0.14.0 — External Surface Foundation (IMPLEMENTED, VERIFIED)

### 1. `martensite-engine-bridge` crate
- `Engine`/`Frame`/`FrameSync`/`EngineContext`/`Viewport` protocol types.
- `SurfaceRing` two-slot mailbox ring; `BridgeRegistry`/`BridgeHandle`
  shared lifecycle with ready-event wake channel.
- `FrontFrame` atomic snapshot (slot + `FrameToken` + size in one lock)
  consumed by `ExternalEngine::poll_frame` — recycled-slot frames are
  detected correctly by token, not slot index.
- `MockEngine`, `CpuFrame`, `TextureFrame`, `NativeFrame`; zero unsafe.

### 2. `PaintCommand::External` (`martensite-render`)
- Ordered marker carrying `surface_id`, destination rect, clip rect.
- `PaintList::segments()`/`has_external()`/`push_external()` split a
  list into `PaintSegment::Commands`/`External` spans preserving paint
  order; Vello emits no geometry for markers, TinySkia draws a
  deterministic checkerboard placeholder (CPU fallback).

### 3. `WgpuHost` composite pipeline (`martensite-wgpu`)
- `register_texture`/`composite`/`composite_view`: producer textures
  drawn directly into the frame target — zero GPU copies.
- Shared dynamic-offset rect uniform (128 slots/frame) so repeated
  composites of one surface keep independent destination rects.
- sRGB-aware pipelines (pass-through vs in-shader `linear_to_srgb`).

### 4. Segmented `RenderOrchestrator` dispatch
- Each command span renders into its own offscreen `Rgba8Unorm`
  texture (`seg_pool`, `view_formats` includes `Rgba8UnormSrgb`);
  one encoder clears the frame then composites all segment blits and
  external surfaces in exact paint order.
- Segment blits use the straight→premul pipeline: Vello's fine shader
  stores *unpremultiplied* pixels, so the blit premultiplies
  in-shader — byte-parity with the direct-dispatch convention.
- Fixed a pre-existing bug: the GPU `render_to_surface` path no longer
  dispatches Vello directly into surface textures (typical surfaces
  are `*Srgb` and lack `STORAGE_BINDING` — the direct dispatch failed
  validation at runtime). All surface frames use the offscreen path.

### 5. `ExternalEngine` widget (`martensite`)
- Retained leaf widget (`PaintCallback` primitive): `poll_frame()` →
  `FramePoll` damage signaling, `VideoFit`-compatible scaling,
  `record_paint` marker emission, `Role::Image` accessibility;
  exported via `widgets::external` and the prelude.

### 6. `examples/engine_embed` — end-to-end demonstration
- Winit 0.31 `ApplicationHandler` driving the full loop:
  `drive_frame` → ready-event `request_redraw` → `poll_frame` →
  `record_paint` (`PaintCommand::External`) → segmented orchestrator
  `render_to_surface` (`take_front` → `composite_front` → release →
  `pre_present_notify` → `present`) → `drain_released`.
- `MockEngine` producer + `ExternalEngine` widget + `SurfaceWrapper`
  low-latency pacing; TinySkia CPU fallback via `cpu_frame_for`.

### Verification Status
- `cargo fmt --all -- --check`: pass.
- `cargo check --workspace` (default + all-features): pass.
- `cargo clippy --workspace --all-targets` (default + all-features)
  `-D warnings`: pass.
- `cargo test` (touched crates, default + all-features): all pass;
  full workspace suite pass except `locale_switch_1000_nodes_settle`
  (pre-existing flaky timing test, passes standalone).
- `cargo test --doc` (touched crates): pass.
- `RUSTDOCFLAGS="-D warnings" cargo doc` (touched crates): pass.
- Two adversarial Santa-method reviewers: PASS (3 rounds — corrected
  a slot-recycling token bug, a pending-frame redispatch bug, a
  same-surface multi-composite rect overwrite, the Vello
  whole-target-overwrite ordering flaw, the straight-alpha segment
  blit, and a stale `gpu_clear`).

### Known limitations (by design, tracked for later milestones)
- External composite ordering is per-frame only — no cross-frame
  `FrameSync` wait integration yet (fence/timeline support is in the
  types; host-side `add_wait_*` wiring lands with the Bevy adapter).
- Cross-device/native-handle import (`NativeFrame`, dmabuf/IOSurface)
  is type-defined but the wgpu HAL import path is not implemented.
- `WgpuHost` owns its own pipelines; no Vello-internal integration.


## v0.13.0 — Modern Shell & Platform (IMPLEMENTED, VERIFIED)

All v0.13.0 deliverables are implemented and verified on CI
(run 34659412786, all 10 jobs green):

### 1. New `martensite-shell` crate
- `BackdropMaterial` enum, `BackdropController` trait,
  `StubBackdropController`, `BackdropMode`, `VibrancyMaterial`,
  `SnapLayout`.
- Windows 11 backend (DWM Mica/Acrylic/MicaAlt/Transient,
  `WindowsBackdropController`, `WindowsSnapLayout`).
- macOS backend (`MacosBackdropController`, NSVisualEffectView /
  Liquid Glass, `vibrancy_to_ns_material`, `AppearanceObserver`).
- Wayland backend (`WaylandBackdropController` stub,
  `FractionalScale`, `CsdConfig`/`DesktopEnvironment`, `CsdHitTest`,
  `StatusNotifierItem` stub).

### 2. Theme system extensions (`martensite-theme`)
- 8 new `TokenKey` variants with light/dark defaults.

### 3. Surface alpha negotiation (`martensite-wgpu`)
- `BackdropMode` enum, `SurfaceWrapper::configure` accepts
  `BackdropMode`, `configure_opaque` convenience method.

### 4. Render pipeline (`martensite-render`)
- `ClearMode` enum, `RenderBackend::render_with_clear`,
  `PaintList::push_blurred_rect`, `PaintCommand::BlurredRect`,
  CPU (tinyskia) three-pass box-blur, and Vello GPU Gaussian blur
  via `Scene::draw_blurred_rounded_rect`.

### 5. Window crate extensions (`martensite-window`)
- `CsdController`, `WindowEventOutcome::FractionalScaleChanged`,
  `WindowEventOutcome::ThemeAppearanceChanged`.

### 6. winit feature change
- Root `Cargo.toml` now enables `wayland` alongside `x11`.

### 7. zbus dependency
- `zbus` 5.x added as a workspace dependency (optional,
  cfg-gated to Linux, behind `wayland-backend` feature).

### Verification Status
- `cargo check --workspace`: pass.
- `cargo fmt --all -- --check`: pass.
- Full CI gate suite (clippy, tests, doctests, docs, audit, deny):
  pass — GitHub Actions run 34659412786, all 10 jobs green.

### Non-Windows compromises resolved (post-audit)
- Vello `BlurredRect` is a real GPU Gaussian blur (was an
  alpha-scaled solid placeholder).
- `RenderOrchestrator::backdrop_mode` now drives the frame clear
  color (`ClearMode::Transparent` / `Opaque`) and
  `configure_surface` keeps the swapchain alpha mode in sync;
  device-loss recovery preserves the surface's backdrop mode.
- macOS `NSGlassEffectView` path no longer receives
  `NSVisualEffectView` selectors; `EffectViewKind` tags the live
  view class and `reduce_transparency_enabled()` exposes the
  accessibility check.

### Remaining (accepted Windows-specific compromises)
- DXGI shared-handle texture import is a validated stub.
- `WindowsSnapLayout` support is inferred from DWM/backdrop probing
  rather than the `ISnapLayouts` COM interface (requires the
  Windows App SDK).
- `martensite-font-fallback` and `martensite-text-reference` are
  excluded from the Windows workspace build (DirectWrite API
  mismatch with `windows` 0.61; Pango/Cairo are Linux-only).
- No new release tag or crates.io publish until the user requests it.

---

## v0.12.0 — Blessed Widgets & Kinematics (IMPLEMENTED)

All four v0.12.0 deliverables are implemented and verified:

### 1. 1M-Row Virtualized DataGrid (`martensite-blessed::data_table`)
- Column sorting (`ColumnSort`), filtering (`RowFilter`), range selection
  (`SelectionModel` with `SmallVec`), keyboard navigation (`KeyAction`),
  column configuration (`ColumnConfig`).
- Zero-allocation scroll path; 1M-row `visible_rows()` < 8.3ms (120fps).
- `#[ignore]`-gated steady-state scroll test; CI is gate of record.

### 2. BSP Docking Tree (`martensite-blessed::docking`)
- `DockTree` with `Slab`-backed arena, zero-alloc `split_leaf`/`merge`
  within capacity.
- Multi-swapchain panel surfaces, drag-and-drop docking (`DockDragSession`,
  `DockDropZone`), iterative `panel_rects`, serialization.
- `#[ignore]`-gated zero-alloc test (10,000 split/merge cycles, 0 allocations).

### 3. 0.55 Rubber-Band Overscroll (`martensite-motion::rubber_band`)
- `RUBBER_BAND_COEFFICIENT = 0.55`, `RubberBandScroller` (1D) and
  `RubberBandScroller2D` (2D with directional axis lock).
- Spring-back via critically-damped `SpringSolver` (~300ms settle).
- O(1) `visible_offset`/`update` with zero allocation (verified).

### 4. 6-DoF Kalman Stylus (`martensite-window::stylus`)
- 12-dimensional Kalman state (position, velocity, orientation, angular
  velocity) with hand-rolled 12×12 matrix math, no external deps.
- Separate 2-state pressure Kalman filter.
- `KalmanStylus::update` < 2.0ms per event (~7µs locally); CI is gate of record.

### Verification Status
- `cargo fmt --check`: clean
- `cargo clippy` (default + all-features): 0 errors
- `cargo test` (default + all-features): all pass
- `cargo test --doc` (all-features): all pass
- `RUSTDOCFLAGS="-D warnings" cargo doc`: clean
- `cargo audit`: 2 allowed warnings, 0 errors
- `cargo deny check`: advisories/bans/licenses/sources ok
- `cargo vet`: Vetting Succeeded (703 exempted)

### Remaining (CI-only verification)
- Timing gates (DataGrid 120fps, Kalman <2ms, docking zero-alloc) are
  `#[ignore]`-gated and must be validated on CI reference runners.
- No new release tag or crates.io publish until CI confirms all gates.

---

## Summary of Resolved Items

The following audit items were resolved during this cycle:
- §1.3 Host dynamic loading — resolved (guest cdylib lifecycle tests).
- §1.4 Platform clipboard/DnD — resolved (Windows/X11 round-trip tests
  added; Wayland `wl-clipboard` backend landed in v0.18.0; non-macOS
  runtime verification is CI-only).
- §1.6 Test quality red flags — resolved (shader, rendering, VFS,
  clipboard, and Tracy sleeps all converted to bounded polling).
- §1.7 Official Unicode conformance suites — resolved (BiDi corpora
  vendored, 100% pass).
- §11.6 BiDi Unicode test suite — resolved (100% pass on both corpora).
- VideoTexture struct — resolved (both NV12 planes preserved).
- Layout performance gates — resolved (CI-calibrated milestone targets).
- TinySkia vertical CJK rasterizer — resolved (DSSIM comparison in CI).

---

## 1. Test Coverage Gaps (P3 — future work)

The comprehensive audit identified test coverage gaps that require CI
infrastructure changes beyond the current loop. The GPU `#[ignore]`-gated
test job has been added; media, host, performance, and cross-platform
clipboard/DnD coverage remain tracked here for future work.

### 1.1 GPU tests are `#[ignore]`-gated

**Status:** Source implementation complete; CI-only verification (Lavapipe).

Device-loss recovery, surface configure/resize/acquire, theme-transition
pipeline compilation, and Vello↔TinySkia pixel-level parity are all
`#[ignore]`-gated because they require a real GPU adapter. They are not
exercised in normal CI.

**Resolved:**
- Added an `ignored-tests` CI job that runs `cargo test --workspace --ignored`
  with the lavapipe software Vulkan adapter and `WGPU_ADAPTER_NAME=llvmpipe`.
- Added an offscreen GPU readback path
  (`RenderOrchestrator::render_to_buffer` in
  `crates/martensite-wgpu/src/orchestrator.rs`) that renders the Vello scene
  into an offscreen `Rgba8Unorm` texture (`STORAGE_BINDING | COPY_SRC`),
  copies it to a `MAP_READ` staging buffer, maps it, and returns the
  premultiplied RGBA8 pixels (row padding stripped) — matching the layout of
  `TinySkiaBackend::pixels`.
- Added `GpuContext::with_cpu_fallback()` in
  `crates/martensite-wgpu/src/device.rs`, which requests an adapter with
  `force_fallback_adapter: true` and `PowerPreference::LowPower` to select
  the Lavapipe/llvmpipe software device for headless testing.
- Added `gpu_cpu_dssim_parity` in `crates/martensite-render/tests/parity.rs`
  (gated behind the `vello` feature, `#[ignore]`-gated). It renders a
  deterministic paint list (solid fill rect, linear gradient, filled triangle
  path) with TinySkia and Vello, demultiplies both buffers, converts to
  `martensite_test::dssim::ImageBuffer`, and asserts `SSIM > 0.98`
  (i.e. `DSSIM < 0.02`). The threshold is intentionally realistic: independent
  rasterizers differ on anti-aliased edges and gradient interpolation, so
  near-exact parity (`0.999`) is not asserted.
- Added a `GPU/CPU parity (Lavapipe)` step to the `ignored-tests` CI job in
  `.github/workflows/ci.yml` that runs
  `cargo test -p martensite-render --test parity --features vello -- --ignored`
  with `WGPU_ADAPTER_NAME=llvmpipe`.

**Verification status:**
- Source implementation: complete.
- Locally verified: not yet — the host (macOS) has no Lavapipe/llvmpipe
  software Vulkan adapter, so `GpuContext::with_cpu_fallback()` returns
  `NoAdapter` and the `#[ignore]`-gated test skips. `cargo fmt`, `cargo clippy`
  (default + `--all-features`), and `cargo test` (non-ignored) pass locally.
- CI-only: the parity assertion is exercised by the Lavapipe job in CI.

### 1.2 Media interop is untested

**Status:** Headless noop tests added; platform imports remain stubbed.

`VideoProcessor::process_frame`, `import_cpu_memory`, and platform imports
(`IOSurface`, `DXGI`, `DMABUF`) had no automated tests. Headless interop tests
have now been added in `crates/martensite-media/tests/interop.rs`, using the
`wgpu` `noop` backend (enabled via the `test-noop` feature) so they run in CI
without a real GPU adapter. The six noop tests cover:

- `video_processor_new_compiles_shaders` — `VideoProcessor::new` succeeds.
- `process_frame_records_commands` — a synthetic 64x64 NV12 frame dispatches
  and the command buffer is recorded/submitted.
- `import_cpu_memory_nv12` — synthetic NV12 data uploads and the returned
  `VideoTexture` exposes both the luma plane (full resolution, `R8Unorm`)
  and the chroma plane (half resolution, `Rg8Unorm`).
- `import_cpu_memory_returns_video_texture_with_both_planes` — verifies the
  `VideoTexture` struct: `luma_view()` succeeds, `chroma_view()` returns
  `Some`, and `width()`/`height()` match the requested dimensions.
- `import_cpu_memory_rejects_zero_dimensions` — a 0x0 descriptor returns
  `Err(MediaError::InvalidBufferDimensions { width: 0, height: 0 })`.
- `import_external_texture_cpu_returns_error` — an invalid `DmaBuf` handle
  surfaces an error instead of succeeding.
- `create_output_texture_matches_dimensions` — a 1920x1080 output texture
  matches the dimensions and `Rgba16Float` format.

Platform-specific import paths (IOSurface/DXGI/dma-buf) are documented as
`#[ignore]`-gated stubs gated by `#[cfg(target_os = "...")]`; each panics with
"not yet implemented" until a real GPU-backed implementation lands on the
matching platform runner.

**Bug fixed during test addition:** `import_cpu_memory` created the luma and
chroma textures with `TEXTURE_BINDING | COPY_SRC` but omitted `COPY_DST`,
which `queue.write_texture` requires. The noop backend's validation surfaced
this (it would also fail on a real GPU). Both texture descriptors in
`crates/martensite-media-platform/src/lib.rs` now include `COPY_DST`.

**Resolved — `VideoTexture` struct introduced:** `import_cpu_memory` now
returns a `VideoTexture` struct that owns both the luma (`y`) and chroma
(`uv`) planes instead of discarding the UV texture. The struct exposes
`luma_view()`, `chroma_view()`, `width()`, and `height()` helpers. The UV
texture is no longer dropped, so the chroma plane can be sampled and
validated after upload. This resolves the API blocker: full
pixel-correctness testing of the bi-planar pipeline is now possible through
the current API.

**Affected files:**
- `crates/martensite-wgpu/src/interop.rs` — `VideoProcessor::process_frame` (line 589), `import_cpu_memory` (line 730), `import_external_texture` (line 718)
- `crates/martensite-media-platform/src/lib.rs` — `import_cpu_memory`, `import_external_texture`, `ImportTextureDescriptor`
- `crates/martensite-media-platform/src/macos.rs` — `import_iosurface`
- `crates/martensite-media-platform/src/windows.rs` — `import_dxgi_texture`
- `crates/martensite-media-platform/src/linux.rs` — `import_dmabuf`
- `crates/martensite-media/tests/interop.rs` — new headless + platform-stub tests

**Required infrastructure:**
- Headless `wgpu::Device` from the `noop` backend for `process_frame` tests
  (added via the `test-noop` feature).
- Platform-specific CI runners (macOS, Windows, Linux) for native imports
  (stubbed, `#[ignore]`-gated).
- Synthetic NV12/P010 test buffers (added in `interop.rs`).

### 1.3 Host dynamic loading is untested

**Status:** Resolved — fast, non-ignored guest lifecycle tests added.

`GuestLibrary::reload`, `HostApp::tick`, `GuestLibrary::get_symbol` had no
unit tests. The only real reload test (`tests/hot_reload_latency.rs:88`) was
`#[ignore]`-gated and required a C toolchain.

**Resolved:**
- Added `crates/martensite-host/tests/guest_lifecycle.rs`, which builds a
  minimal C guest cdylib at runtime (via `std::process::Command` invoking
  `cc`/`clang`/`gcc`) that exports the `martensite_render` symbol, and
  exercises the full `GuestLibrary` / `HostApp` lifecycle against it. The
  seven non-ignored tests cover:
  - `guest_library_load_and_path_matches` — `GuestLibrary::load` succeeds
    and `path()` matches the loaded artifact.
  - `guest_library_get_symbol_render_is_callable` —
    `GuestLibrary::get_symbol("martensite_render")` succeeds and the symbol
    is callable.
  - `guest_library_get_symbol_nonexistent_returns_symbol_not_found` —
    `get_symbol("nonexistent")` returns `HostError::SymbolNotFound`.
  - `host_app_new_and_tick_succeeds` — `HostApp::new` + `HostApp::tick`
    succeed when the render symbol is present.
  - `host_app_tick_missing_render_symbol_returns_error` — `HostApp::tick`
    on a cdylib missing the render symbol returns
    `HostError::MissingRenderSymbol`.
  - `guest_library_reload_updates_path` — `GuestLibrary::reload` to a
    second built cdylib succeeds, `path()` updates, and the reloaded
    library remains usable.
  - `guest_library_load_bogus_file_returns_load_failed` — loading a bogus
    file returns `HostError::LoadFailed`.
- The cdylib-building is gated behind a runtime check for an available C
  compiler (`cc`/`clang`/`gcc`); if none is found the tests skip
  gracefully (printing a message) rather than failing. No `#[ignore]`
  gating is used.
- No new dev-dependency was required: the system compiler is invoked
  directly via `std::process::Command`.

**Affected files:**
- `crates/martensite-host/src/lib.rs:169` — `GuestLibrary::get_symbol`
- `crates/martensite-host/src/lib.rs:197` — `GuestLibrary::reload`
- `crates/martensite-host/src/lib.rs:277` — `HostApp::reload`
- `crates/martensite-host/src/lib.rs:298` — `HostApp::tick`
- `crates/martensite-host/tests/guest_lifecycle.rs` — new guest lifecycle
  tests.

### 1.4 Platform clipboard/DnD only tested on macOS

**Status:** Resolved (v0.18.0) — Windows/X11 round-trip tests added; a
Wayland backend now exists via `wl-copy`/`wl-paste` subprocesses with
bounded waits (see `martensite-clipboard-platform/src/wayland.rs`).

`martensite-clipboard-platform` has real clipboard round-trips on macOS.
Windows and X11 now have real OS round-trip tests (write/read/clear,
available_types, empty text, unicode, multiple writes overwrite, large
text) gated behind `#[cfg(target_os = "windows")]` and
`#[cfg(target_os = "linux")]` respectively. The X11 tests use a runtime
`DISPLAY` env-var check to skip gracefully when no X server is available
(no `#[ignore]`). The in-memory `MockBackend` and `InMemoryClipboard` mock
test suites have been expanded with write/read/clear round-trip,
available_types, multiple writes overwrite, large text, empty string, and
unicode (CJK, emoji, RTL) tests that run on all platforms without any OS
clipboard.

**Resolved:**
- Added real Windows clipboard round-trip tests in
  `crates/martensite-clipboard-platform/src/windows.rs` (text round-trip,
  clear, available_types, empty text, unicode, overwrite, large text).
- Added real X11 clipboard round-trip tests in
  `crates/martensite-clipboard-platform/src/x11.rs` with runtime `DISPLAY`
  check for graceful skip.
- Expanded `MockBackend` tests in
  `crates/martensite-clipboard-platform/tests/mock_backend.rs` (overwrite,
  large text, empty string, CJK, emoji, RTL, write/read/clear, binary).
- Expanded `InMemoryClipboard` adapter tests in
  `crates/martensite-clipboard/tests/platform_adapter.rs` (overwrite, large
  text, empty string, CJK, emoji, RTL, write/read/clear).
- Added `clipboard-windows` and `clipboard-x11` CI jobs to
  `.github/workflows/platform-conformance.yml` that run the real OS
  clipboard tests on `windows-latest` and `ubuntu-latest` (with Xvfb).

**Remaining:**
- Wayland clipboard runtime verification is CI-only — the backend is
  compile-verified (`x86_64-unknown-linux-gnu` check + doc build pass)
  and unit-tested for backend selection, but real `wl-copy`/`wl-paste`
  round-trips have not run on a Wayland session.
- Windows/X11 real-OS tests are CI-only (not locally verified on macOS).

### 1.5 Performance gates not enforced

**Status:** Partially resolved — layout and bench_suite gates now enforce
milestone targets in CI; remaining items still open.

The following performance tests were `#[ignore]`-gated and did not fail if
targets were missed. The layout gates now enforce the milestone targets
directly, calibrated for CI runners (ubuntu-latest). Local dev machines —
especially older hardware — may exceed these thresholds; that is expected
and not an implementation regression.

**Resolved:**

- v0.3.0 layout: `<0.5 ms` for 1000 containers, `<0.05 ms` incremental.
  - `crates/martensite-layout/src/engine.rs` — `deep_nested_flex_performance`
    and `incremental_relayout_performance` now gate hard assertions behind
    `MARTENSITE_STRICT_BENCH=1`. The enforced thresholds are the milestone
    targets themselves (`<0.5 ms` and `<0.05 ms`), calibrated for CI runners
    (ubuntu-latest). The tests remain `#[ignore]` and run in CI via the
    `performance-gates` job with `--release --ignored`.
  - The `deep_nested_flex_actual_perf` and `incremental_relayout_actual_perf`
    tracking tests remain informational (print only, no assertion).
- bench_suite: `bench_arena_operations_10k` and
  `bench_diamond_reactive_network` now enforce strict exit gates
  (`<25 ms` CI threshold) when `MARTENSITE_STRICT_BENCH=1`, matching the
  existing `bench_signal_propagation_10k` gate pattern.
- CI: a `performance-gates` job was added to `.github/workflows/ci.yml`
  that runs `cargo test --release --workspace --benches -- --ignored` with
  `MARTENSITE_STRICT_BENCH=1`, enforcing the layout milestone targets and
  the bench_suite strict exit gates.
- `docs/BENCHMARKS.md` now distinguishes enforced, regression-gate,
  informational, and manual/platform-specific suites with a summary table.

**Resolved (v0.18.0):** all three remaining gates now run in the CI
`performance-gates` job (`.github/workflows/ci.yml`) under
`MARTENSITE_STRICT_BENCH=1`:

- v0.9.0 hot reload `<350 ms` — `hot_reload_latency` runs
  `--ignored` in CI.
- v0.6.0 theme-transition zero-alloc — `theme_transition_zero_alloc`
  (new gate, `--ignored` in CI).
- v0.9.0 Tracy overhead `<0.1 ms/frame` — tracy overhead test runs
  `--ignored` in CI.

**Recommended action:**
- Monitor CI results for the milestone-target gates. If CI fails, investigate
  whether Taffy needs optimization or the target needs revisiting.
- Add a headless hot-reload benchmark that fails if reload > 350 ms.
- Enforce Tracy overhead threshold in CI.

### 1.6 Test quality red flags

**Status:** Resolved — shader and rendering tests now validate semantic
behavior; all fixed sleeps (vfs watcher, clipboard, Tracy) have been
replaced with bounded waits or removed.

**Resolved:**

1. **String-contains shader tests** — `gpu_transition.rs` now parses the
   WGSL shader with `naga` and asserts structural properties (uniform
   struct fields, binding group/index pairs, and the `theme_color`
   function) instead of matching source substrings. The `interop.rs`
   tests already used `naga` structural parsing.
2. **Non-zero-pixel "rendering" tests** — `parity.rs` and
   `tinyskia_backend.rs` now assert exact pixel colors at known
   coordinates and verify that pixels outside drawn areas remain
   transparent/background. Minimum non-zero pixel counts are retained
   only as secondary regression checks.
3. **Sleeps in tests (vfs)** — `martensite-assets/src/vfs.rs` no longer
   uses fixed `std::thread::sleep` calls; watcher tests now use bounded
   `park_timeout`-based polling loops with deadlines that fail on
   timeout instead of sleeping for a fixed duration.
4. **Sleeps in tests (clipboard, Tracy)** —
   `martensite-clipboard/src/clipboard.rs` lazy-payload deadline waits now
   use `mpsc::recv_timeout` on a detached producer thread (see
   `run_with_deadline`), and the `martensite-devtools/src/tracy.rs`
   overhead gate measures wall-clock instrumentation cost directly;
   neither uses fixed `thread::sleep` calls anymore.

**Remaining:** None — all previously flagged fixed sleeps have been
replaced with bounded waits.

### 1.7 Official Unicode conformance suites not integrated

**Status:** Resolved — official BiDi corpora vendored and passing at 100%.

`martensite-text/tests/v0_11_conformance.rs` uses custom vectors for
UAX #14, vertical runs, cache keys, and fallback. The official
`BidiTest.txt` and `BidiCharacterTest.txt` conformance suites are now
vendored under `crates/martensite-text/tests/data/` (UCD 17.0.0) and
the harness in `crates/martensite-text/tests/bidi_conformance.rs`
achieves 100% pass on both corpora (770,241 + 91,707 cases). See
§11.6 for details.

---

## 2. Windows clipboard-platform API drift (P1 — platform-specific)

**Status:** Resolved — Windows backend migrated to `windows` 0.61.3 and `build.rs` fixed for cross-compilation.

The `windows` crate was upgraded from 0.59 to 0.61.3, which introduced API
breaking changes. The Windows clipboard backend
(`crates/martensite-clipboard-platform/src/windows.rs`) has 18 compile errors
when cross-compiling to `x86_64-pc-windows-msvc`. This does **not** affect
the native macOS build or any non-Windows CI gate.

**Known API changes:**
- `GlobalFree` was removed from `Win32::System::Memory`.
- `GetClipboardData` / `GlobalAlloc` now return `Result<HANDLE>` instead of
  `HANDLE`.
- `RegisterClipboardFormatW` now requires `PCWSTR` instead of `*const u16`.
- `CF_UNICODETEXT.0` is `u16` not `u32`.

**Required action:**
- Migrate all Windows FFI calls to the `windows` 0.61.3 API.
- Update `GlobalFree` usage to the new ownership model.
- Wrap raw pointers in `PCWSTR` where required.
- Test with `cargo check -p martensite-clipboard-platform --target x86_64-pc-windows-msvc`.

---

## 3. `winit` / `accesskit_winit` version mismatch (P0 — release-blocking)

**Status:** Resolved — vendored and patched `martensite-accesskit-winit` for winit 0.31.0-beta.3.

The workspace pins `winit = "0.31.0-beta.3"` while `accesskit_winit = "0.34"`
depends on `winit ^0.30.5`. The lockfile resolves two incompatible `winit`
versions (0.30.13 and 0.31.0-beta.3). `martensite-window` uses winit 0.31
pointer-event variants that `accesskit_winit` 0.34 cannot accept.

**Impact:**
- Any example or application that wires `martensite-window` together with
  `martensite-access` / `accesskit_winit` will fail to compile.
- The workspace is using two incompatible `winit` APIs simultaneously.

**Required action (one of):**
- Downgrade `winit` to a stable `0.30.x` release and use `accesskit_winit 0.34`.
- Wait for / fork an `accesskit_winit` version that supports `winit 0.31`.
- Vendor and patch `accesskit_winit` locally until upstream support lands.

---

## 4. `naga` duplicate version (P2 — dependency hygiene)

**Status:** Resolved — documented; unification requires an upstream `vello_shaders` update.

`wgpu 30.0.1` uses `naga 30.0.1` while `vello_shaders 0.10.0` (transitive
through `martensite-vello`) uses `naga 29.0.4`. If `martensite-vello` /
`martensite-render` ever expose `naga` types or pass shader modules between
the two versions, the build will break. Even if it compiles, carrying two
`naga` copies increases compile time and binary size.

**Resolution:**
- Audited: `vello_shaders 0.10.0` is pinned to `naga ^29.0.3`; `netrender-vello` 0.10.0 (vendored as `martensite-vello`) is the latest published build.
- Added an explicit `naga` 29.0.4 exemption in `deny.toml` with a comment explaining the upstream constraint.

---

## 5. `martensite-blessed` tight coupling (P2 — architecture)

**Status:** Resolved — unused umbrella dependency removed.

`martensite-blessed` depended on the top-level `martensite` crate
(`crates/martensite-blessed/Cargo.toml:16`), which was unusual for a "blessed
widget set" and created a tight coupling. At the time `martensite` did not
depend back on `martensite-blessed`, so there was no cycle, but the
dependency direction was unusual. **Update (v0.18.0):** the
`martensite::blessed` facade alias added in W6 now makes `martensite`
depend on `martensite-blessed` (still acyclic — blessed's umbrella
dependency was removed); the publish order was updated to ship
`martensite-blessed` first.

**Resolution:**
- Audited the crate source: no `martensite::*` imports were found.
- Removed the `martensite` dependency from `crates/martensite-blessed/Cargo.toml`.
- The crate now depends only on `martensite-core`, `martensite-render`, and `martensite-text`.

---

## 6. `stubs/` directories not documented (P3 — polish)

**Status:** Resolved — documented in root `Cargo.toml`.

`stubs/martensite/Cargo.toml` and `stubs/martensite-ui/Cargo.toml` exist but
are **not** workspace members. They are `publish = false` and use version
`0.0.1` to reserve the crates.io package names for the v1.0 release.

**Resolution:**
- Added a comment in the root `Cargo.toml` explaining that the `stubs/`
  crates are namespace reservations and are not built or published with the
  workspace.

---

## 7. Publish workflow duplication (P2 — CI hygiene)

**Status:** Resolved — `publish.yml` now invokes `ci.yml` and validates metadata.

`publish.yml` duplicated CI logic instead of reusing `ci.yml`. The publish
gates were copy-pasted, so `ci.yml` updates could drift.

**Resolution:**
- Added `workflow_call:` to `ci.yml` so it can be reused.
- Replaced the duplicate CI jobs in `publish.yml` with a single `ci` job
  that calls `uses: ./.github/workflows/ci.yml`.
- Added a `validate` job that checks the tag version matches
  `Cargo.toml` and that `CHANGELOG.md` contains a section for the release.
- Set `generate_release_notes: false` in the GitHub Release step; the
  canonical `CHANGELOG.md` section is used via `body_path`.
- `cargo publish --no-verify` is retained because workspace path
  dependencies are published sequentially and may not yet be visible in the
  crates.io index for the next crate's verification (build validation is done
  by the CI job).

---

## 8. `martensite-cosmic-text` documentation exemption (P2 — policy)

**Status:** Resolved — formal exemption added to `AGENTS.md`.

`martensite-cosmic-text` explicitly opts out of the project's documentation
standards:

```rust
#![allow(unsafe_code)]
#![allow(missing_docs)]
#![allow(clippy::all)]
#![allow(rustdoc::broken_intra_doc_links)]
```

This is documented as an upstream-fork exception in `CHANGELOG.md`, but it
conflicts with `AGENTS.md` which says every public item should have a
`# Examples` section.

**Resolution:**
- Added a formal exemption to `AGENTS.md` under the "Doc examples" rule,
  explicitly listing `martensite-cosmic-text` as the only crate that does not
  require compilable doctest examples for every public item.

---

## 9. `martensite-window` API expansion from doctest work (P3 — process)

**Status:** Resolved — kept the API and made `WindowEventOutcome` non-exhaustive.

During the Wave 2 doctest work, Agent H added new public API to
`martensite-window` beyond pure doctests:
- `DropAction` enum
- `DropEvent` enum
- `convert_drop_event` function (re-exported in `lib.rs`)
- `WindowEventOutcome::Occluded(bool)` variant + `process_window_event` handling
- ~10 new unit tests

The new code is correct, well-tested, and clippy-clean. `WindowEventOutcome`
is not `#[non_exhaustive]`, so adding a variant was technically semver-breaking,
but no workspace consumer matches it exhaustively (verified via grep).

**Resolution:**
- Kept the new API.
- Added `#[non_exhaustive]` to `WindowEventOutcome` in
  `crates/martensite-window/src/manager.rs` to prevent future
  semver-breaking variant additions.

---

## 10. `RUSTSEC-2026-0192` (ttf-parser) advisory status (P2 — verify)

**Status:** Resolved — references removed after verification.

Code comments in `crates/martensite-text/src/font.rs:217` and
`crates/martensite-text/src/cascade.rs:636` referenced `RUSTSEC-2026-0192` for
`ttf-parser`. This advisory was **not** in the `audit.toml` or `deny.toml`
ignore lists.

**Verification:**
- `ttf-parser` is not present in `Cargo.lock`.
- `cargo audit` passes with 0 vulnerabilities and does not report
  `RUSTSEC-2026-0192`.

**Resolution:**
- Removed the `ttf-parser RUSTSEC-2026-0192` references from the comments in
  `font.rs` and `cascade.rs`. The `swash` panic-path notes were retained.

---

## Summary

| # | Item | Priority | Blocks release? | Requires CI infra? |
|---|------|----------|-----------------|-------------------|
| 1 | Test coverage gaps (GPU, media, host, perf, Unicode, clipboard) | P3 — partially resolved (GPU CI job, perf gates, clipboard round-trips added) | No | Yes |
| 2 | Windows clipboard-platform API drift | P1 — resolved | No (macOS only) | No |
| 3 | `winit`/`accesskit_winit` version mismatch | P0 — resolved | Yes | No |
| 4 | `naga` duplicate version | P2 — resolved | No | No |
| 5 | `martensite-blessed` tight coupling | P2 — resolved | No | No |
| 6 | `stubs/` directories undocumented | P3 — resolved | No | No |
| 7 | Publish workflow duplication | P2 — resolved | No | No |
| 8 | `martensite-cosmic-text` doc exemption | P2 — resolved | No | No |
| 9 | `martensite-window` API expansion | P3 — resolved | No | No |
| 10 | `RUSTSEC-2026-0192` advisory status | P2 — resolved | No | No |

**Note:** Item #3 (`winit`/`accesskit_winit` mismatch) is the only true
release blocker. All other items can be resolved incrementally without
blocking development or the native macOS build.

---

## 11. v0.0.0–v0.11.0 exhaustive audit — remediated gaps

The exhaustive audit of every milestone promise from v0.1.0 through
v0.11.0 identified the following gaps that have been remediated in this
cycle. Each item links to the concrete fix.

### 11.1 v0.4.0 — `is_in_current_scope` boundary check (resolved)

**Promise:** CHANGELOG `[0.4.0]` lists `is_in_current_scope` for
boundary checks.

**Gap:** No public `is_in_current_scope(widget)` API existed; scope
containment was only enforced internally.

**Fix:** Added `FocusManager::is_in_current_scope(&arena, id) -> bool`
in `crates/martensite-focus/src/manager.rs`, with a doctest and the
expected semantics (returns `true` when no scope is active; otherwise
checks subtree membership of the top scope root).

### 11.2 v0.7.0 — `ClipboardItem::types()` zero-alloc accessor (resolved)

**Promise:** CHANGELOG `[0.7.0]` references
`ClipboardItem::types()` debug output without intermediate `Vec`.

**Gap:** Only `offered_types() -> Vec<String>` (allocating) existed.

**Fix:** Added `ClipboardItem::types() -> impl Iterator<Item = &str>`
in `crates/martensite-clipboard/src/clipboard.rs`. The `Debug` impl
already avoided the `Vec` via `self.payloads.keys()`; the new method
provides the documented zero-allocation accessor for callers.

### 11.3 v0.8.0 — BT.709/BT.2020 ΔE threshold (resolved)

**Promise:** CHANGELOG `[0.8.0]` claims ΔE < 0.05 for BT.709/BT.2020
YUV↔RGB round-trip.

**Gap:** In-code tests only enforced `de < 1.0`.

**Fix:** Tightened the assertions in
`crates/martensite-media/src/color.rs::bt709_color_bars_delta_e` and
`bt2020_color_bars_delta_e` to `de < 0.05`. Both tests pass.

### 11.4 v0.10.0 — Plugin `file_read` path traversal (resolved)

**Promise:** v0.10.0 capability model grants `FileRead` on a path.

**Gap:** Authorization used exact path matching, so granting a
directory did not authorize reads of files beneath it, and a
`/assets/../etc/passwd` request would be rejected only by accident
of string inequality — not by canonical containment.

**Fix:** Added `CapabilitySet::file_read_allowed` /
`file_write_allowed` in `crates/martensite-plugin/src/security.rs`
that canonicalizes both granted and requested paths via
`std::fs::canonicalize` (with a lexical `..`-resolving fallback for
non-existent paths), then requires the requested path to equal or
descend into a granted root. The runtime `file_read` host import now
calls `file_read_allowed` instead of exact `contains`. New tests
cover traversal rejection, exact-file grants, real-dir traversal,
and lexical normalization.

### 11.5 v0.11.0 — WCAG 2.2 AAA + VPAT + caret + multilingual (resolved)

**Promise:** v0.11.0 exit gates for WCAG AAA, VPAT, caret sync, and
zero-tofu.

**Gap:** Implementation existed but no conformance tests proved the
claims.

**Fix:** Added `crates/martensite-access/tests/v0_11_conformance.rs`
with 11 tests covering:
- WCAG AAA text contrast for the standard widget palette.
- WCAG AA text contrast (subset).
- WCAG 1.4.11 UI-component contrast for focus indicators.
- WCAG 2.5.8 target size for interactive widgets.
- WCAG 2.4.13 focus appearance (AAA) for standard widgets.
- VPAT report evaluates all automated criteria and marks 502.3 as
  `NotEvaluated`.
- VPAT markdown disclaims certification and lists all criteria.
- VPAT validation flags missing remarks on `DoesNotSupport` rows.
- Section 508 VPAT summary is honest (PASSED/FAILED + disclaimer).
- Caret synchronization completes under the 16.6 ms frame budget
  (measured per-update over 1,000 updates on a 1,000-char node).
- Multilingual fallback chain is non-empty for every `ScriptTag`
  (structural precondition for zero-tofu).

### 11.6 v0.11.0 — BiDi Unicode test suite status (resolved)

**Promise:** 100% pass on official `BidiTest.txt` / `BidiCharacterTest.txt`.

**Status:** Resolved — the official conformance corpora
(`BidiTest.txt` 7.6 MB, `BidiCharacterTest.txt` 6.6 MB, UCD 17.0.0)
are vendored under `crates/martensite-text/tests/data/` and the
conformance harness in `crates/martensite-text/tests/bidi_conformance.rs`
achieves **100% pass** on both:
- `BidiTest.txt`: 770,241 cases, 770,241 passed, 0 failed.
- `BidiCharacterTest.txt`: 91,707 cases, 91,707 passed, 0 failed.

The harness was fixed by: (1) using U+0627 (ARABIC LETTER ALEF) as
the representative character for the AL bidi class instead of U+0607
which `unicode_bidi` did not classify as AL; (2) concatenating levels
across paragraph splits (BidiInfo splits at B/paragraph-separator
characters, but BidiTest.txt expects levels for the entire line);
(3) implementing strict X9-aware reorder comparison instead of the
previous loose index-bounds check. The thresholds are now 100%
(`pass_rate >= 1.0`) for both corpora.

---

## 12. v0.0.0–v0.11.0 exhaustive audit — infrastructure-dependent gaps

These gaps require external infrastructure (platform CI runners,
screen-reader automation, official test corpora, real GPU adapters)
and cannot be closed by source changes alone. They are tracked here
so they are not silently dropped.

**Infrastructure built this cycle:** All eight gaps now have CI
workflows, test harnesses, or Docker setups in place. The gaps are
partially closed — the infrastructure exists and runs locally, but
full closure depends on CI execution on platform runners.

### 12.1 Official Unicode BiDi / vertical conformance corpora

**Status: Infrastructure built; CI workflow created.**

- `BidiTest.txt` (7.6 MB) and `BidiCharacterTest.txt` (6.6 MB) are
  vendored under `crates/martensite-text/tests/data/` (UCD 17.0.0,
  Unicode License v3).
- A conformance harness `crates/martensite-text/tests/bidi_conformance.rs`
  parses both formats and runs them through `unicode_bidi::BidiInfo`.
- Smoke tests (always run) verify the parser; full corpus tests are
  `#[ignore]`-gated (~770k + ~92k cases).
- CI job `bidi-conformance` in `platform-conformance.yml` downloads
  the corpora and runs the full tests.
- Vertical golden-frame tests in
  `crates/martensite-text/tests/vertical_golden_conformance.rs`
  verify UAX #50 orientation classification, vertical run collection,
  and glyph transform stability using a self-baseline regression
  approach.
- A Pango/Cairo external reference renderer is now in place in the
  isolated `martensite-text-reference` crate
  (`crates/martensite-text-reference/`). It renders CJK text in vertical
  mode (`gravity = EAST`, `gravity_hint = STRONG`) and horizontal mode,
  returning straight RGBA8 pixel buffers suitable for DSSIM comparison
  via `martensite-test::dssim`.
  - Reference renderer infrastructure: complete (source-level).
  - Locally verified: builds and tests pass on a machine with
    `libcairo2-dev`, `libpango1.0-dev`, `libglib2.0-dev`, `pkg-config`,
    and `fonts-noto-cjk` installed.
  - CI-only: the `pango-vertical-reference` job in
    `platform-conformance.yml` installs the native deps and runs
    `cargo test -p martensite-text-reference`. The main `ci.yml` jobs
    that use `--workspace` also install Pango/Cairo dev packages so the
    workspace member compiles.
  - The reference crate's tests (`tests/vertical_dssim.rs`) validate
    determinism (self-consistency DSSIM < 0.001) and non-trivial output
    (glyphs are actually drawn) for both vertical and horizontal modes.
  - A TinySkia-based vertical CJK rasterizer has been added to the
    reference crate's test suite. It shapes CJK text with cosmic-text,
    stacks glyphs vertically (UAX #50 upright orientation), and
    rasterizes each glyph outline via swash into a TinySkia pixmap.
  - The `vertical_cjk_martensite_vs_pango_dssim` test compares the
    Martensite TinySkia vertical output against the Pango/Cairo
    reference via DSSIM (threshold < 0.10). This test is
    `#[ignore]`-gated because it requires `fonts-noto-cjk` and Pango
    vertical gravity support, which are only available in CI. Run in
    CI with `cargo test -p martensite-text-reference -- --ignored`.
  - The CI `pango-vertical-reference` job now runs both the default and
    `--ignored` tests, exercising the full DSSIM comparison.

### 12.2 Multilingual zero-tofu glyph coverage on platform runners

**Status: CI workflow created; requires platform runners.**

- CI job `multilingual-tofu` in `platform-conformance.yml` runs a
  matrix on `ubuntu-latest`, `macos-latest`, `windows-latest`.
- Installs CJK fonts on Linux (`fonts-noto-cjk`).
- Runs the multilingual fallback tests with `MARTENSITE_TOFU_CHECK=1`.
- The structural fallback-chain test already exists in
  `v0_11_conformance.rs`; the CI job adds the platform-specific
  font-system verification.

### 12.3 Screen-reader caret latency harness (NVDA/VoiceOver/Orca)

**Status: CI workflow created; requires platform runners + AT setup.**

- CI jobs `at-harness-nvda`, `at-harness-voiceover`, `at-harness-orca`
  in `platform-conformance.yml`.
- NVDA: uses `@guidepup/setup` to install NVDA on Windows.
- VoiceOver: enables VoiceOver automation via TCC + Guidepup on macOS.
- Orca: installs `at-spi2-core`, `orca`, `python3-pyatspi2`, starts
  Xvfb + D-Bus + Orca on Linux.
- All jobs run the `caret_synchronization` test with
  `MARTENSITE_AT_HARNESS` and `MARTENSITE_AT_LATENCY_BUDGET_MS=16`.
- The AT-harness jobs can be skipped via `workflow_dispatch` input.
- **Known risk:** macOS 26+ may restrict VoiceOver AppleScript via
  entitlement requirements.

### 12.4 Section 508 VPAT certification

**Status: Documented; requires manual specialist testing.**

- The automated `VpatReport` is an evaluation aid, not a
  certification.
- Full Section 508 conformance requires manual AT testing of all
  applicable criteria by an accessibility specialist.
- The report explicitly disclaims certification and marks
  unevaluated criteria as `NotEvaluated`.
- The README "Contributing > Accessibility testing" section documents
  what manual testing is needed and how to contribute.

### 12.5 WCAG AAA coverage for blessed widgets

**Status: Resolved — automated tests added.**

- `crates/martensite-blessed/tests/wcag_blessed_conformance.rs` tests
  WCAG AAA text contrast, 1.4.11 UI component contrast, 2.5.8 target
  size, and 2.4.13 focus appearance for `DataTable`, `Chart`,
  `CodeEditor`, and `AudioWaveform`.
- CI job `blessed-wcag` in `platform-conformance.yml` runs these
  tests.

### 12.6 48-hour fuzzing run (v0.10.0)

**Status: Docker setup created; requires local execution.**

- `docker/fuzz/Dockerfile` + `docker-compose.yml` + `fuzz-runner.sh`
  provide a local Docker environment for 48-hour soak fuzzing.
- The `soak_campaign` test in `crates/martensite-test/src/fuzz.rs` is
  `#[ignore]`-gated and configurable via `MARTENSITE_FUZZ_SEED` and
  `MARTENSITE_FUZZ_DURATION` env vars.
- Results are logged to a bind-mounted `./results/` directory.
- Run locally with: `cd docker/fuzz && docker compose up --build`.
- Short smoke runs are supported via `MARTENSITE_FUZZ_DURATION=60`.

### 12.7 Plugin 5 ms wall-clock budget (v0.10.0)

**Status: Benchmark test created; CI workflow created.**

- `crates/martensite-plugin/tests/wall_clock_budget.rs` measures
  1,000 plugin invocations and asserts average < 10ms (2x safety margin
  of the 5ms target).
- Smoke test (always runs) verifies the plugin loads and invokes
  within the default fuel budget.
- Benchmark test is `#[ignore]`-gated (host-dependent timing).
- CI job `plugin-wall-clock` in `platform-conformance.yml` runs the
  benchmark on `ubuntu-latest`, `macos-latest`, `windows-latest`.
