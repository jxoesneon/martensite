# Martensite — Agent & Contributor Guardrails

This file documents hard-won lessons and mandatory guardrails for working
on the Martensite codebase. Follow these to avoid repeating past issues.

## Repository

- **Path**: `/Users/mey/martensite`
- **Remote**: `https://github.com/jxoesneon/martensite.git`
- **Language**: Rust (workspace, 35+ crates)
- **Milestone docs**: `docs/milestones/`

## Release Pipeline — Critical Rules

### 1. Publishing is the LAST step, gated on ALL CI green

The `publish.yml` workflow runs ALL CI checks (fmt, clippy, tests, audit,
deny, docs, benchmarks) as prerequisite jobs. The `publish` job only runs
after all of them pass. **Never publish crates manually from a local
machine** unless you have verified every gate locally first.

**What went wrong before**: Crates were published to crates.io while CI
was failing on GitHub. The publish workflow ran independently of CI and
did not check CI status.

**Fix**: The publish workflow now embeds all CI gates as `needs:`
dependencies. Publishing cannot start unless every gate passes.

### 2. GitHub Releases must have detailed notes

Every version tag (`v*.*.*`) must have a corresponding GitHub Release with
changelog-derived notes. The `publish.yml` workflow now creates a GitHub
Release automatically after publishing, extracting notes from
`CHANGELOG.md`.

**What went wrong before**: Tags were pushed without release notes,
showing only commit messages on the GitHub tags page.

**Fix**: The `github-release` job in `publish.yml` extracts the relevant
section from `CHANGELOG.md` and creates a proper GitHub Release in `draft: true`
mode. Following binary compilation and cryptographic asset verification by
`verify-release-assets`, the release is automatically converted to public
(`draft: false`). This guarantees users never observe half-uploaded releases.

### 3. Doc examples are required for public API items

docs.rs reports example coverage. Every public struct, enum, and
user-facing function should have a `# Examples` section with a
compilable doctest. This is enforced by `cargo test --doc`.

**What went wrong before**: Only 3 out of 59 public items had examples.

**Fix**: Examples were added to all key public types. Continue this
practice for all new public API items.

**Exemption**: `martensite-cosmic-text` and `martensite-vello` are vendored
upstream forks (of `cosmic-text` and `netrender-vello`/Vello 0.10
respectively) and opt out of the doc-example requirement via
`#![allow(missing_docs)]` and `#![allow(rustdoc::broken_intra_doc_links)]`.
They are the only crates in the workspace that do not need compilable
doctest examples for every public item.

## CI — Known Pitfalls

### 4. Test with BOTH default and all features

CI runs tests with default features AND with `--all-features`. Code that
compiles with `--all-features` may fail without it (e.g., feature-gated
imports in test modules).

**What went wrong before**: `vello_backend.rs` test module imported
`GlyphInstance`, `GlyphRun`, `GradientStop`, `GradientStops`, and
`Point` unconditionally, but these are only used under the `vello`
feature. CI's default-features test run failed with `unused_imports`.

**Fix**: Gate test-only imports behind `#[cfg(feature = "...")]` to
match the feature gates of the code that uses them.

**Rule**: Always run both:
```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --bins --lib --tests
cargo test --workspace --all-features
```

### 5. Benchmark CLI flags must be valid

The benchmark job uses `cargo bench -- --test`. Do not pass flags that
criterion doesn't support (e.g., `--quick` was never a valid criterion
flag).

**What went wrong before**: CI benchmark step used `--quick --test` but
`--quick` is not a criterion argument, causing the benchmark job to
fail on every run.

**Fix**: Use only `--test` for benchmark exit-gate verification.

### 6. CI must run on tags — via publish.yml, not a tag trigger

`ci.yml` does NOT trigger on tag pushes directly: `publish.yml` invokes
the same pipeline through `workflow_call`, and a direct tag trigger
would run the entire suite twice per release. Tag CI coverage comes
from the publish workflow's `ci` job.

### 7. Crate enumerations are generated, never hand-listed

The publish order, the publishable set, the semver-checks matrix, and
doctest shards all derive from `cargo metadata` via
`scripts/workspace-matrix.py`. There is no hand-maintained crate list
anywhere — a previous hand-list silently skipped newly added crates and
shipped an inverted publish order that aborted a release mid-upload.

- Adding a publishable crate requires **no** list edits.
- A publishable crate must be named `martensite-*` or carry
  `package.metadata.ci.publishable = true`; `version-consistency` CI
  enforces the predicate and the generated order on every PR.
- `release-binaries`/`verify-release-assets` must never run when
  `github-release` was skipped on a tag push — softprops upserts to a
  bare auto-created release otherwise.

### 8. Tests run under cargo-nextest in CI — built once, sharded

CI compiles test binaries ONCE per feature leg (`build-tests` job →
`cargo nextest archive` → workflow artifact), then 12 `test-suite`
shards (6 hash-partitions × {default, all-features}) download the
archive and run only their slice — no per-shard compile. The parity
assertion (libtest `-- --list` count == archived test count) lives in
the build job where compilation already happened. Doctests run in a
separate 4-shard `doc-tests` job plus an 8-shard `doc-tests-facade` job
(the `martensite` facade alone carries ~4000 doctests — one crate was a
61-minute critical path — so it is split at file granularity via
`facade-doctest-group`; nextest never runs doctests; mold links them
faster). `test-suite` and `ignored-tests` use `if: ${{ !cancelled() }}`
so one failed archive leg cannot silently skip an entire test surface —
the missing artifact fails that leg's shards loudly while the healthy
leg still runs. Ignored GPU/hardware tests run from the default archive
via `--run-ignored ignored-only` (no second compile). Per-test process
isolation is mostly safer, but tests sharing an OS-level singleton —
the real system clipboard in `martensite-clipboard-platform` — are
serialized via the `system-clipboard` test-group in
`.config/nextest.toml`. Add any new OS-singleton test there.
`--ignored`/`--nocapture` libtest flags need `--run-ignored`/
`--no-capture` equivalents under nextest. Tests that need an OS service
that may be absent must serve a stub in-process (see
`status_notifier_item_*` in martensite-shell) or probe-and-skip with a
printed reason — never `continue-on-error`.

### 9. Unified Status Gate (`ci-gate`) for Branch Protection

All 21 matrix and verification jobs roll up into a single terminal
`ci-gate` aggregator. It runs with `if: always()` and programmatically
inspects all upstream job states:
- Failed or cancelled upstream jobs cause `ci-gate` to exit with an error.
- Permissibly skipped jobs (such as heavy adapters on PRs) are ignored.
This provides a single, unambiguous status check for GitHub branch
protection rules without brittle per-shard job roster matching.

### 10. Monorepo Blast-Radius Closure with Saturation Fallback

PR verification uses `scripts/workspace-matrix.py blast-radius` to
compute the reverse-dependency closure via BFS over `cargo metadata`.
Modifications to global surfaces (`Cargo.lock`, `Cargo.toml`, `.cargo/*`,
`.github/*`, `scripts/*`, `benches/*`, or `crates/martensite/*`) or any
change impacting over 35% of workspace crates automatically triggers a
saturation fallback, scheduling the full workspace test suite.

### 11. Hermetic Debug Symbols & Linker Tuning

Test compilation uses `[profile.test]` and `[profile.ci]` with:
- `debug = 1` (line tables only, cutting binary sizes by 80%).
- `split-debuginfo = "off"` (self-contained binaries so distributed
  nextest shards retain symbolicated panic backtraces without `.o` trees).
- `codegen-units = 32` (prevents thread contention on 4-vCPU runners).
- `opt-level = 2` for `package."*"` dependencies (ensures layout and
  compute shaders run at native performance in test runs).
- Target-isolated linkers: `mold` on Linux GNU and `lld-link` on Windows MSVC.

## Verification Checklist (run before every release)

```sh
# 1. Formatting
cargo fmt --all -- --check

# 2. Clippy (both feature sets)
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings

# 3. Tests (both feature sets — cargo test locally; CI uses nextest)
cargo test --workspace --bins --lib --tests
cargo test --workspace --all-features
# nextest equivalent (what CI runs, per shard):
#   cargo nextest run --workspace [--all-features]

# 4. Doctests (CI shards: 4 crate groups + 8 facade file-groups via
#    workspace-matrix.py)
cargo test --doc --workspace --all-features

# 5. Docs build
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features

# 6. Security — ZERO-VULNERABILITY policy (matches CI)
cargo audit --deny warnings
cargo deny check advisories -D warnings
cargo deny check bans licenses sources
#    Advisory ignore lists stay EMPTY: an unmaintained/unsound dep must
#    be upgraded, substituted with a maintained crate, or vendored in.

# 7. Benchmarks (test mode)
cargo bench -p bench_suite --bench bench_suite -- --test
```

## Architecture Notes

### Two-level layout

- `LayoutEngine` uses Taffy for arena-level layout.
- Widgets (`Flex`, `Container`, `Stack`) manage their own children
  internally as `Box<dyn Widget>`.
- Widget-internal children are NOT registered in the arena; the
  framework reaches them through the `Widget::child_count`/`child`/
  `child_mut`/`child_bounds` protocol:
  - `Widget::event` forwards into internal children by default
    (bounds-gated, topmost-first).
  - `WidgetArena::dispatch_event` bubbles up arena ancestors on
    `EventResponse::Ignored`; `RequestRepaint` sets `DIRTY_PAINT`.
  - `WidgetArena::build_paint_list` records each visible widget's
    `paint` chrome plus internal children in document order.
  - The AccessKit adapter emits internal children as virtual nodes
    (generation-0 `NodeId` space) — see `martensite-access`'s
    `AccessKitAdapter::resolve_internal`.
- `EventRouter::dispatch_pointer_event`/`dispatch_keyboard_event`/
  `dispatch_scroll_event` in `martensite-window` are the production
  entry points combining routing and delivery.
- This is documented in `crates/martensite/src/widgets/mod.rs`.

### Text caching

- Tier 1: `InlineTextCache` in `ColdNode` — fast constraint probing.
- Tier 2: `TextShapeCache` — global LRU with 16 MB budget.
- Cache keys include: `FontId`, `FontSizeBits`, `TextHash`,
  `MaxWidthBits`, `family_hash`, `line_height_bits`.

### Documented limitations

These are explicitly documented in code, not hidden:
- `MinContent` → `0.0` in engine measure closure.
- `compute_ime_bounds` uses fixed `2.0` pixel width.
- Performance tests are `#[ignore]` by default.
- `Text::content` is public; call `invalidate_cache()` after direct mutation.

### Font fallback architecture (v0.11.0)

- `FontFallbackProvider` trait in `martensite-text::cascade` abstracts the
  source of fallback families.
- `PlatformCascadeResolver` (default) uses static per-OS family lists.
- `martensite-font-fallback` crate implements native OS providers:
  - `DirectWriteFontFallback` (Windows, `IDWriteFontFallback::MapCharacters`)
  - `CoreTextFontFallback` (macOS, `CTFontCreateForStringWithLanguage`)
  - `FontconfigFontFallback` (Linux, `FcFontSort`)
- The following ten crates use `#![allow(unsafe_code)]` for
  platform-specific FFI or vendored upstream code. These are the ONLY
  crates in the workspace that allow unsafe code; all other crates
  maintain `unsafe_code = "deny"`.
  - `martensite-access-platform` — mobile accessibility FFI boundary
    (UIKit `accesskit_ios` adapter on iOS, JNI/`accesskit_android`
    injection on Android).
  - `martensite-font-fallback` — platform FFI (DirectWrite on Windows,
    CoreText on macOS, Fontconfig on Linux).
  - `martensite-clipboard-platform` — OS clipboard FFI (macOS
    NSPasteboard, Windows Win32, X11).
  - `martensite-media-platform` — hardware video surface import FFI
    (IOSurface on macOS, DXGI on Windows, dmabuf on Linux).
  - `martensite-host` — dynamic library loading (`libloading`/`dlopen`
    on Unix, `LoadLibrary` on Windows).
  - `martensite-cosmic-text` — vendored upstream fork (cosmic-text),
    exempt with `#![allow(missing_docs)]` and `#![allow(clippy::all)]`.
  - `martensite-accesskit-winit` — vendored upstream fork of
    `accesskit_winit` 0.34.0 patched for winit 0.31.0-beta.3. Temporary;
    remove once upstream supports winit 0.31.
  - `martensite-vello` — vendored copy of `netrender-vello` 0.10 (a
    republish of Vello 0.10 built against wgpu 30), with local fixes:
    coverage-driven bump buffers scale with target tile/bin count
    (upstream's fixed estimates silently overflow on dense HiDPI
    scenes → `bump.failed` → empty frame), and the async path always
    reads back `BumpAllocators` + polls the device for diagnostics.
    Upstream uses `unsafe` for trusted shader module creation; exempt
    with `#![allow(missing_docs)]` and `#![allow(clippy::all)]`.
  - `martensite-shell` — platform FFI for system backdrops (DWM on
    Windows, NSVisualEffectView/Liquid Glass on macOS). Wayland CSD
    is safe Rust.
  - `martensite-godot` — GDExtension FFI boundary via the `godot`
    (gdext) crate. Unsafe is confined to the extension crate; it is
    `publish = false` and excluded from the default workspace build.
- `FallbackDecisionCache` caches resolved fallback chains keyed by
  `(script, locale, primary_family)`, invalidated by a font-system
  generation counter.
- `Shaper::shape_with_options` is the single canonical shaping entry
  point used by the production `Text` widget.
- `swash` font-data access is wrapped in `catch_unwind` to guard against
  malformed-font panics.

### Media pipeline architecture (v0.16.0)

- Decoder **wire types** (`EncodedPacket`, `DecodedFrame`, `DecoderConfig`,
  `DecodeStats`, `HdrSideData`, `DecodeError`, `VideoCodec`) live in
  `martensite-media-platform::decoder` — the same cycle-break pattern as
  `surface` (media → platform direction, so backend impls in the FFI crate
  cannot name media-crate types). `martensite-media::decoder` re-exports
  them and defines the `VideoDecoder` trait, implementing it for each
  platform backend type (local trait on foreign type).
- `VideoDecoder` requires `Send + Sync`; all mutation goes through
  `&mut self` methods. `end_of_stream` drains reorder-buffered frames —
  tests MUST call it before draining or reorder-delayed frames never emit.
  `flush` re-arms the keyframe gate.
- `HardwareHandle::DmaBuf` is multi-plane: `objects: Vec<fd>` + `planes:
  Vec<DmaBufPlane{object_index, offset, stride}>` + a surface-wide
  `modifier`. `import_external_planes` imports NV12/P010 as a
  `VideoTexture{y, uv}` pair via `ImportTextureDescriptor::plane_index`.
- Windows zero-copy: `import_dxgi_texture` works only on a **Vulkan-backend**
  wgpu device with `Features::VULKAN_EXTERNAL_MEMORY_WIN32` — wgpu-hal's
  `texture_from_d3d11_shared_handle` binds the whole allocation to one
  image, so bi-planar NV12/P010 cannot be split (they return
  `UnsupportedFormat`; callers fall back to `import_cpu_memory`). The DX12
  backend cannot import D3D11 shared handles at all.
- `ffmpeg-next` 9 API notes: `Packet::copy` + `set_flags(Flags::KEY)`
  (no `set_key`), side data via `side_data::Type::MasteringDisplayMetadata`
  / `ContentLightLevel` / `DYNAMIC_HDR_PLUS`, and
  `set_packet_time_base(1/1e9)` for nanosecond PTS.
- New decoder deps are feature-gated (`decoder-videotoolbox`, `decoder-mf`,
  `decoder-vaapi`, `decoder-ffmpeg`); default build is unaffected. CI
  installs `libva-dev` + ffmpeg `-dev` packages for `--all-features` jobs.

### Accessibility architecture (v0.11.0)

- AccessKit's platform adapters handle live-region notifications
  natively. The custom `LiveRegionMonitor` was removed.
- `SemanticTreeSync` uses a hybrid strategy: dirty-flag early-exit
  (fast path) + fingerprint validation (slow path).
- `MartensiteAccessBridge` uses `parking_lot::Mutex` (poison-free).
- `relative_luminance` sanitizes NaN/inf color channels before clamping.

### Paint audit (v0.18.0)

- `martensite-access::paint_audit` checks text clipping/overlap,
  widget overflow, occlusion, WCAG 4.5:1 text contrast and 3:1
  non-text (stroke) contrast, and target size.
- Dashboard gate:
  `cargo test -p industrial_dashboard dump_zone_lints -- --nocapture`
  audits every page at widths 700–2400; it must report zero findings.
  `dump_widget_tree` regenerates `WIDGET_TREE.txt` (redirect stderr).
- Always paint and audit at the **same** scale factor — a mismatch
  halves reported font sizes.
- Token semantics: `DividerColor`/`BorderColor` are **stroke** tokens
  (3:1 vs surface). `RaisedColor` is the chrome-band **fill** token
  (hosts text at 4.5:1) — never fill a text-hosting band with a stroke
  token.
- `kurbo::Rect::inset(positive)` **expands** — use negative values to
  shrink a keyline inside a fill.
- Keyline idiom: stroke *inside* a chromatic fill with `better_ink` so
  the edge is judged against the fill, not the backdrop it floats on.
- `ScrollView::horizontal` is the strip/toolbar idiom (unbounded X
  measure); `ScrollView::new` is the document idiom (unbounded Y).
- Glyph ink extends ~1.15×font-size **below** the text origin — line
  advances must be ≥1.3×fs to avoid overlap.

### Design lint (martensite-design-lint)

- Replays a `PaintList`'s `PushScope`/`PopScope` provenance into a
  `LintScene` (widget tree + geometry + text sizes + colors), then
  evaluates standards-backed rules — WCAG 2.2, ISA-101, ISA-18.2 alarm
  analogs, Hick/Fitts, Tufte/Few, perception research. Facade:
  `martensite::design_lint`.
- Findings cite their standard and link
  `docs/design-standards/rules/<rule-id>.md`.
- All standards are default-on. `design-lint.toml` (project config)
  sets `standards`, `[rules.<id>]` severity/params, `[classify]`, and
  `[[allow]]` path globs (`*` = within a segment, `**` = any depth).
- Inline control: `debug_name` suffix `@lint:rule-id|all|standard:<key>`
  suppresses that subtree; `@level:1..4` declares an ISA-101 level;
  semantic markers (`@alarm`, `@priority:N`, `@kpi`, `@destructive`)
  feed domain rules and are node-local (rules needing ancestor context
  use `scene.by_path()`/`marker_in_lineage`).
- Suppressed findings land in `LintReport::suppressed` — reported, not
  dropped; allows matching nothing go to `unused_allows`; `Severity::
  Forbid` cannot be suppressed by either mechanism.
- Autofix: findings may carry `LintFix` (`Safe` ops = spacing/alignment
  nudges; `Risky` = recolors/font/bounds — gated on `--force`).
  `autofix(&mut scene, cfg, &FixOptions)` loops lint→fix→re-lint until
  convergence; `recursive:false` = one pass, `max_depth` caps passes.
  Fixes mutate the `LintScene` model — a convergence proof/preview,
  not source rewriting.
- Rule modules: `rules/mod.rs` (original 14 + shared helpers:
  `surface_nodes`, `kind_of`, `interactive_leaves`, `sibling_gaps`,
  `background_at`, `contrast_ratio`, `desaturate`, `is_alarm_red`,
  `marker_in_lineage`) plus `rules/{wcag,hmi,gestalt,consistency_ext,
  frame}.rs` — each exports `pub(crate) fn rules()`.
- Dashboard harness:
  `cargo test -p industrial_dashboard dump_design_lints -- --nocapture`;
  `PAGE_FILTER=<substr>` narrows pages (same convention as
  `dump_zone_lints`). CLI:
  `cargo run -p industrial_dashboard --bin design_lint -- [--fix]
  [--force] [--no-recursive] [--max-recursiveness N] [--filter STR]
  [--list-rules]` — exits 1 on Warn+ findings (Info never gates; CI-
  usable). The sweep machinery is shared in `lint_sweep.rs`.
- Fix-op semantics worth knowing: path-anchored ops hit ALL same-path
  siblings (`nodes_mut`); `SetGap`/`SnapGapsToGrid`/`AlignSiblings`
  only move band-adjacent children (off-row/grid children untouched —
  a global-extreme align collapses grids); recolors keep a color that
  other fills/texts still use; `autofix` fingerprints the scene each
  pass and stops on a repeated state (oscillating fixes).

### Live-window verification (ultramac MCP)

- `MARTENSITE_CPU=1` forces the TinySkia surface path — use it when the
  GPU (Vello→composite→`wgpu::Surface`) path presents black.
- `gpu_readback_real_frame` (`#[ignore]`d) renders the real dashboard
  paint list through the offscreen composite and counts non-black
  pixels — isolates scene/composite health from surface presentation.
- Black-frame bisect recipe: `MARTENSITE_CPU` (surface+present OK?) →
  hardcode the frame clear color (present+clear OK?) → hardcode the
  composite fragment to a solid color (draw/geometry/blend OK?) →
  sample-raw (segment texture empty?) → `vello_bump_stats` (which Vello
  buffer overflowed?). Each step halves the pipeline.
- wgpu validation errors go through the **`log`** crate, not `tracing`
  — an app that only installs `tracing_subscriber` sees nothing. The
  dashboard installs `device.on_uncaptured_error` → stderr; tests can
  `tracing_subscriber::fmt::try_init()` (its `tracing-log` feature
  captures `log` records).
- Vello 0.10's `render_to_texture` is **non-robust**: fixed bump
  buffers, and overflow sets `bump.failed` then silently early-outs —
  empty frame, `Ok(())`, no error. `bump.blend` (per-tile blend-stack
  spill) scales with clip layers × coverage; the dashboard's clip-heavy
  scene exhausted the 1<<20 default at ~2600×1500+. `martensite-vello`
  now scales coverage-driven buffers by tile/bin count; check
  `RenderOrchestrator::vello_bump_stats` if a frame ever goes empty
  again.
- **Coordinates**: `screenshot` images are ~1.037× the screen's logical
  points — clicking image pixels drifts ~10px low near the bottom.
  Prefer `click_in_window` (window-relative logical points) or divide
  image coords by ~1.037 before `mouseClick`.
- `MenuStack::take_activated` closes the stack — owners drain it in
  `tick`, which runs before `sync`, so the drain itself is the
  dismissal signal (commit 1557192).

## Workflow

### Santa Method (adversarial review)

For quality-sensitive changes, run two independent reviewer subagents.
Both must return PASS before proceeding. See the `santa-method` skill.

### Dependency upgrades

- Upgrade to latest available versions.
- Avoid versions published less than 7 days ago.
- Do not use floating ranges (`latest`, `*`, unbounded `>=`).
- Run `cargo audit` and `cargo deny check` after upgrades.

### crates.io rate limits

The publish workflow implements the leaky bucket rate limits documented at
<https://crates.io/docs/rate-limits>:

- **New crates** (not yet on crates.io): burst of 5, then 1 per 600s (10 min)
- **New versions** (existing crate): burst of 30, then 1 per 60s (1 min)

The workflow tracks token buckets for each type, refills based on elapsed
time, and sleeps only when the bucket is empty. A 10s index propagation
delay is applied between all publishes. 429 responses are parsed for the
next-allowed timestamp to compute the exact wait time.
