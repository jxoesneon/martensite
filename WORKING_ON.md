# Working On — Pending Items

This file tracks work that is **not yet complete** or has known limitations.
It is a living document — items move off this list when they are resolved.

Last updated: v0.11.0 audit remediation cycle.

---

## 1. Test Coverage Gaps (P3 — future work)

The comprehensive audit identified test coverage gaps that require CI
infrastructure changes beyond the current loop. These are documented as
known limitations and tracked here for future work.

### 1.1 GPU tests are `#[ignore]`-gated

**Status:** Not enforced in CI.

Device-loss recovery, surface configure/resize/acquire, theme-transition
pipeline compilation, and Vello↔TinySkia pixel-level parity are all
`#[ignore]`-gated because they require a real GPU adapter. They are not
exercised in normal CI.

**Affected files:**
- `crates/martensite-wgpu/src/resilience.rs:1115` — `harness_full_recovery_recreates_device_and_surface`
- `crates/martensite-wgpu/src/device.rs:242` — `new_high_performance_context_succeeds_or_errors_gracefully`
- `crates/martensite-wgpu/src/device.rs:288` — `recreate_device_and_queue_after_destroy`
- `crates/martensite-wgpu/src/theme_transition.rs:684` — `pipeline_compiles_on_real_device`
- `crates/martensite-wgpu/src/theme_transition.rs:696` — `render_theme_transition_produces_command_buffer`

**Required infrastructure:**
- Software Vulkan adapter (lavapipe/llvmpipe) on CI runners.
- `WGPU_ADAPTER_NAME` environment variable to select the software adapter.
- `cargo test --workspace --ignored` as a separate CI job.

**Recommended action:**
- Add a CI job that runs `cargo test --workspace --ignored` with a software
  Vulkan adapter.
- Add a headless `RenderOrchestrator::render_to_surface` path that can be
  exercised without a real window.
- Assert DSSIM < 1e-4 between `TinySkiaBackend` and `VelloRenderer`
  rasterized to CPU in `crates/martensite-render/tests/parity.rs`.

### 1.2 Media interop is untested

**Status:** No automated coverage.

`VideoProcessor::process_frame`, `import_cpu_memory`, and platform imports
(`IOSurface`, `DXGI`, `DMABUF`) have no automated tests. Only 4 tests exist
in `martensite-media-platform` (format mapping + mock-error dispatch).

**Affected files:**
- `crates/martensite-wgpu/src/interop.rs` — `VideoProcessor::process_frame` (line 589), `import_cpu_memory` (line 730), `import_external_texture` (line 718)
- `crates/martensite-media-platform/src/macos.rs` — `import_iosurface`
- `crates/martensite-media-platform/src/windows.rs` — `import_dxgi_texture`
- `crates/martensite-media-platform/src/linux.rs` — `import_dmabuf`

**Required infrastructure:**
- Headless `wgpu::Device` from a CPU adapter for `process_frame` tests.
- Platform-specific CI runners (macOS, Windows, Linux) for native imports.
- Synthetic NV12/P010 test buffers.

### 1.3 Host dynamic loading is untested

**Status:** Very thin coverage.

`GuestLibrary::reload`, `HostApp::tick`, `GuestLibrary::get_symbol` have no
unit tests. The only real reload test (`tests/hot_reload_latency.rs:88`) is
`#[ignore]`-gated and requires a C toolchain.

**Affected files:**
- `crates/martensite-host/src/lib.rs:169` — `GuestLibrary::get_symbol`
- `crates/martensite-host/src/lib.rs:197` — `GuestLibrary::reload`
- `crates/martensite-host/src/lib.rs:277` — `HostApp::reload`
- `crates/martensite-host/src/lib.rs:298` — `HostApp::tick`

**Recommended action:**
- Build a minimal temp `cdylib` guest in `martensite-host` tests.
- Test `GuestLibrary::load`, `get_symbol`, `reload`, and `HostApp::tick`.

### 1.4 Platform clipboard/DnD only tested on macOS

**Status:** Windows/X11/Linux only have name smoke tests.

`martensite-clipboard-platform` has real clipboard round-trips on macOS, but
`windows.rs` and `x11.rs` only have `platform_name_is_*` smoke tests. No
Linux Wayland backend is visible.

**Required infrastructure:**
- Platform-specific CI runners for Windows and Linux.
- In-memory `ClipboardProvider` trait mock for cross-platform testing.

### 1.5 Performance gates not enforced

**Status:** Tests print timing but do not assert thresholds.

The following performance tests are `#[ignore]`-gated and do not fail if
targets are missed:

- v0.3.0 layout: `<0.5 ms` for 1000 containers, `<0.05 ms` incremental.
  - `crates/martensite-layout/src/engine.rs:974, 1057, 1110, 1180`
  - Comments explicitly state Taffy misses these targets.
- v0.9.0 hot reload: `<350 ms`.
  - `crates/cargo-martensite/tests/hot_reload_latency.rs:88` (ignored).
- v0.6.0 theme-transition: zero allocation not measured.
- v0.9.0 Tracy overhead: `<0.1 ms/frame`.
  - `crates/martensite-devtools/src/tracy.rs:557` (ignored).

**Recommended action:**
- Once Taffy meets targets, change ignored layout tests into release-mode
  assertions.
- Add a headless hot-reload benchmark that fails if reload > 350 ms.
- Enforce Tracy overhead threshold in CI.

### 1.6 Test quality red flags

**Status:** Documented, not yet fixed.

1. **String-contains shader tests** — `theme_transition.rs:635-680` and
   `interop.rs:772-795` assert source substrings, not semantic correctness.
2. **Non-zero-pixel "rendering" tests** — `martensite-render/tests/parity.rs`
   and `tinyskia_backend.rs` use `non_zero_pixels(&backend) > 0` as a success
   signal. Any broken renderer that writes a single stray pixel passes.
3. **Sleeps in tests** — `martensite-assets/src/vfs.rs:923/933/954/963/973`,
   `martensite-clipboard/src/clipboard.rs:738` (`thread::sleep(200ms)`),
   `martensite-devtools/src/tracy.rs:557`. These are environment-dependent
   and can be flaky on slow CI runners.

### 1.7 Official Unicode conformance suites not integrated

**Status:** Custom vectors used, official suites not integrated.

`martensite-text/tests/v0_11_conformance.rs` uses custom vectors for BiDi,
UAX #14, vertical runs, cache keys, and fallback. It does **not** integrate
the official `BidiTest.txt` / `BidiCharactertest.txt` conformance suites.

**Recommended action:**
- Download `BidiTest.txt` and `BidiCharacterTest.txt` from the Unicode
  consortium.
- Add them as test data in `crates/martensite-text/tests/`.
- Parse and run them against the BiDi implementation.

---

## 2. Windows clipboard-platform API drift (P1 — platform-specific)

**Status:** 18 pre-existing compile errors on Windows target.

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

**Status:** Unresolved — requires upstream coordination.

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

**Status:** Unresolved — transitive duplicate.

`wgpu 30.0.1` uses `naga 30.0.1` while `vello_shaders 0.10.0` (transitive
through `netrender-vello`) uses `naga 29.0.4`. If `netrender-vello` /
`martensite-render` ever expose `naga` types or pass shader modules between
the two versions, the build will break. Even if it compiles, carrying two
`naga` copies increases compile time and binary size.

**Required action:**
- Audit whether `vello_shaders` can be updated to use `naga 30`.
- If not, document why the transitive duplicate is unavoidable.

---

## 5. `martensite-blessed` tight coupling (P2 — architecture)

**Status:** Documented design concern.

`martensite-blessed` depends on the top-level `martensite` crate
(`crates/martensite-blessed/Cargo.toml:16`), which is unusual for a "blessed
widget set" and creates a tight coupling. `martensite` does not depend back
on `martensite-blessed`, so there is no cycle, but the dependency direction
is unusual.

**Required action:**
- Evaluate whether `martensite-blessed` should depend on lower-level crates
  (`martensite-core`, `martensite-text`, etc.) instead of the umbrella crate.
- If the current direction is intentional, document the rationale.

---

## 6. `stubs/` directories not documented (P3 — polish)

**Status:** Undocumented.

`stubs/martensite/Cargo.toml` and `stubs/martensite-ui/Cargo.toml` exist but
are **not** workspace members. They are `publish = false` and use version
`0.0.1`. This appears intentional but should be documented.

**Required action:**
- Add a comment in the root `Cargo.toml` or `AGENTS.md` explaining why the
  stubs exist and why they are outside the workspace.

---

## 7. Publish workflow duplication (P2 — CI hygiene)

**Status:** Unresolved.

`publish.yml` duplicates CI logic instead of reusing `ci.yml`. The publish
gates are copy-pasted. If `ci.yml` is updated, `publish.yml` can drift.

**Additional publish issues:**
- No tag/version/changelog validation before publish.
- GitHub Release uses both `body_path` and `generate_release_notes: true`,
  which can produce combined/duplicate release notes.
- `cargo publish --no-verify` bypasses packaging verification.

**Required action:**
- Refactor `publish.yml` to `needs: [ci]` or use a reusable `verify.yml`
  workflow.
- Add explicit tag/version/changelog validation steps.
- Decide between `body_path` and `generate_release_notes`.

---

## 8. `martensite-cosmic-text` documentation exemption (P2 — policy)

**Status:** Documented exception, not resolved.

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

**Required action:**
- Either upstream the documentation improvements to the cosmic-text project,
  or add a formal exemption section to `AGENTS.md` that explicitly lists
  `martensite-cosmic-text` as exempt from the doctest requirement.

---

## 9. `martensite-window` API expansion from doctest work (P3 — process)

**Status:** Code is correct and tested, but exceeds doc-only scope.

During the Wave 2 doctest work, Agent H added new public API to
`martensite-window` beyond pure doctests:
- `DropAction` enum
- `DropEvent` enum
- `convert_drop_event` function (re-exported in `lib.rs`)
- `WindowEventOutcome::Occluded(bool)` variant + `process_window_event` handling
- ~10 new unit tests

The new code is correct, well-tested, and clippy-clean. `WindowEventOutcome`
is not `#[non_exhaustive]`, so adding a variant is technically semver-breaking,
but no workspace consumer matches it exhaustively (verified via grep).

**Required action:**
- Decide whether to keep the new API or revert it to keep Wave 2 doc-only.
- If keeping it, add `#[non_exhaustive]` to `WindowEventOutcome` to prevent
  future semver-breaking variant additions.
- If reverting, remove `DropAction`, `DropEvent`, `convert_drop_event`, and
  the `Occluded` variant.

---

## 10. `RUSTSEC-2026-0192` (ttf-parser) advisory status (P2 — verify)

**Status:** Unverified.

Code comments in `crates/martensite-text/src/font.rs:217` and
`crates/martensite-text/src/cascade.rs:636` reference `RUSTSEC-2026-0192` for
`ttf-parser`. This advisory is **not** in the `audit.toml` or `deny.toml`
ignore lists. If this advisory exists and is triggered by the dependency
tree, `cargo audit` would fail in CI.

**Current status:** `cargo audit` passes (0 vulnerabilities), so either the
advisory does not exist or `ttf-parser` is not in the scanned dependency
tree. This should be verified.

**Required action:**
- Verify whether `RUSTSEC-2026-0192` exists in the RustSec database.
- If it exists and is triggered, either fix the vulnerability or add it to
  both `audit.toml` and `deny.toml` with justification.
- If the code comments reference a non-existent advisory, remove the
  references to avoid confusion.

---

## Summary

| # | Item | Priority | Blocks release? | Requires CI infra? |
|---|------|----------|-----------------|-------------------|
| 1 | Test coverage gaps (GPU, media, host, perf, Unicode) | P3 | No | Yes |
| 2 | Windows clipboard-platform API drift | P1 | No (macOS only) | No |
| 3 | `winit`/`accesskit_winit` version mismatch | P0 | Yes | No |
| 4 | `naga` duplicate version | P2 | No | No |
| 5 | `martensite-blessed` tight coupling | P2 | No | No |
| 6 | `stubs/` directories undocumented | P3 | No | No |
| 7 | Publish workflow duplication | P2 | No | No |
| 8 | `martensite-cosmic-text` doc exemption | P2 | No | No |
| 9 | `martensite-window` API expansion | P3 | No | No |
| 10 | `RUSTSEC-2026-0192` advisory status | P2 | No | No |

**Note:** Item #3 (`winit`/`accesskit_winit` mismatch) is the only true
release blocker. All other items can be resolved incrementally without
blocking development or the native macOS build.
