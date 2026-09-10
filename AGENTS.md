# Martensite — Agent & Contributor Guardrails

This file documents hard-won lessons and mandatory guardrails for working
on the Martensite codebase. Follow these to avoid repeating past issues.

## Repository

- **Path**: `/Users/mey/martensite`
- **Remote**: `https://github.com/jxoesneon/martensite.git`
- **Language**: Rust (workspace, 22+ crates)
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
section from `CHANGELOG.md` and creates a proper GitHub Release.

### 3. Doc examples are required for public API items

docs.rs reports example coverage. Every public struct, enum, and
user-facing function should have a `# Examples` section with a
compilable doctest. This is enforced by `cargo test --doc`.

**What went wrong before**: Only 3 out of 59 public items had examples.

**Fix**: Examples were added to all key public types. Continue this
practice for all new public API items.

**Exemption**: `martensite-cosmic-text` is a vendored upstream fork of
`cosmic-text` and opts out of the doc-example requirement via
`#![allow(missing_docs)]` and `#![allow(rustdoc::broken_intra_doc_links)]`.
It is the only crate in the workspace that does not need compilable
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

### 6. CI must run on tags

The CI workflow triggers on `push` to `main`, on PRs, AND on tag pushes
(`v*.*.*`). This ensures tags get CI coverage before the publish
workflow runs.

## Verification Checklist (run before every release)

```sh
# 1. Formatting
cargo fmt --all -- --check

# 2. Clippy (both feature sets)
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings

# 3. Tests (both feature sets)
cargo test --workspace --bins --lib --tests
cargo test --workspace --all-features

# 4. Doctests
cargo test --doc --workspace --all-features

# 5. Docs build
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features

# 6. Security
cargo audit
cargo deny check

# 7. Benchmarks (test mode)
cargo bench -p bench_suite --bench bench_suite -- --test
```

## Architecture Notes

### Two-level layout

- `LayoutEngine` uses Taffy for arena-level layout.
- Widgets (`Flex`, `Container`, `Stack`) manage their own children
  internally as `Box<dyn Widget>`.
- Widget-internal children are NOT registered in the arena.
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
- The following six crates use `#![allow(unsafe_code)]` for
  platform-specific FFI or vendored upstream code. These are the ONLY
  crates in the workspace that allow unsafe code; all other crates
  maintain `unsafe_code = "deny"`.
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
- `FallbackDecisionCache` caches resolved fallback chains keyed by
  `(script, locale, primary_family)`, invalidated by a font-system
  generation counter.
- `Shaper::shape_with_options` is the single canonical shaping entry
  point used by the production `Text` widget.
- `swash` font-data access is wrapped in `catch_unwind` to guard against
  malformed-font panics.

### Accessibility architecture (v0.11.0)

- AccessKit's platform adapters handle live-region notifications
  natively. The custom `LiveRegionMonitor` was removed.
- `SemanticTreeSync` uses a hybrid strategy: dirty-flag early-exit
  (fast path) + fingerprint validation (slow path).
- `MartensiteAccessBridge` uses `parking_lot::Mutex` (poison-free).
- `relative_luminance` sanitizes NaN/inf color channels before clamping.

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
