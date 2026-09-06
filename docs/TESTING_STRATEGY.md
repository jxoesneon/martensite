# Testing & Quality Assurance Strategy

**Document Identifier:** DOC-TESTING-STRATEGY  
**Status:** Active Core Specification  

## 1. Test Taxonomy

All code within the Martensite project is subject to the following testing domains, designed to run both locally and within the continuous integration (CI) pipeline:

*   **Unit Tests (`#[test]`)**:
    *   **Location**: Inline within per-crate `src/` modules.
    *   **Tooling**: Vanilla `cargo test`.
    *   **CI Trigger**: Runs on every PR and commit via `cargo test --workspace`.
*   **Integration Tests (`tests/`)**:
    *   **Location**: Top-level `tests/` directory within crates.
    *   **Tooling**: `cargo test --test <name>`.
    *   **CI Trigger**: Runs concurrently with unit tests.
*   **Property-Based Tests**:
    *   **Location**: Inline `proptest!` blocks primarily in `martensite-core` and `martensite-reactive`.
    *   **Tooling**: `proptest` crate. Validates invariants of the generational arena and signal DAG.
    *   **CI Trigger**: Executed as part of the standard `cargo test` suite.
*   **Golden Frame Regression Tests**:
    *   **Location**: Driven via `martensite-test` against `tests/snapshots/`.
    *   **Tooling**: Headless `MockWindowBackend` + Lavapipe (Software Vulkan) + `VirtualClock` + YIQ/SSIM diffing engine.
    *   **CI Trigger**: Dedicated `test-suite` CI job on Linux (Mesa Lavapipe).
*   **Benchmark Tests (`benches/`)**:
    *   **Location**: `benches/bench_suite/` at the workspace root.
    *   **Tooling**: `criterion` crate.
    *   **CI Trigger**: Manually invoked or tracked on protected branch merges to monitor performance budgets.
*   **Fuzzing**:
    *   **Location**: `fuzz/` directory in workspace.
    *   **Tooling**: `cargo-fuzz` (libFuzzer).
    *   **CI Trigger**: Run periodically out-of-band via scheduled nightly jobs or local campaigns.
*   **Accessibility Audits**:
    *   **Location**: CI scripts and `martensite-test` simulated assertions.
    *   **Tooling**: Validating `accesskit::TreeUpdate` outputs against WCAG 2.1 AA specifications.
    *   **CI Trigger**: Enforced as a separate automated checklist phase in CI.
*   **Cross-Platform CI Matrix**:
    *   **Location**: `.github/workflows/ci.yml`.
    *   **Tooling**: `cargo check --target <triple>` with zero C/C++ dependencies.
    *   **CI Trigger**: `cross-compile-matrix` job on every PR.

## 2. Criterion Benchmark Specifications

In accordance with CHARTER.md (Mandate III, IV and Anti-Slop 4.3), the following reproducible Criterion benchmarks must execute inside `benches/bench_suite`:

*   **Signal propagation latency**:
    *   **Structure**: Construct a 10,000-node dependency DAG. Mutate a single root source signal.
    *   **Metric**: Measure nanoseconds per iteration (`ns/iter`).
    *   **Budget**: Must complete within sub-millisecond thresholds.
*   **Taffy layout solve time**:
    *   **Structure**: Construct a 5,000-node dynamic grid layout. Invalidate bounds and resolve `Pass 1` and `Pass 2`.
    *   **Metric**: Measure milliseconds per solve (`ms/solve`).
*   **Vello frame encode latency**:
    *   **Structure**: Encode a complex UI frame into a Vello command stream. Evaluate against a cold cache (first frame) and warm cache (steady state).
    *   **Metric**: Measure milliseconds per frame (`ms/frame`).
*   **Cold startup time**:
    *   **Structure**: Instrument `App::build()` from entry point up to the first frame submission.
    *   **Metric**: Measure milliseconds (`ms`).
*   **RSS memory footprint**:
    *   **Structure**: Boot the app, idle the event loop, force a GC/compaction if any, and read `/proc/self/statm` or equivalent.
    *   **Metric**: Measure Megabytes (`MB`). Target is <20 MB.
*   **Baseline comparison (egui, iced)**:
    *   **Methodology**: Identical 10k text-node rendering loop implemented in egui and iced for side-by-side execution.
    *   **Metric**: Frame time (ms) and peak memory (MB) under maximum synthetic load.

## 3. Golden Frame CI Architecture

In accordance with DDR-0009, Martensite enforces 100% bit-exact pixel sovereignty through a headless golden snapshot system.

*   **Mock Surface Initialization**: Runs via `MockWindowBackend` without physical display hardware. CI uses LLVMpipe/Lavapipe (`WGPU_ADAPTER_NAME="llvmpipe"`) to simulate Vulkan natively in software.
*   **Deterministic Virtual Clock Advancement**: Time is governed strictly by `VirtualClock::advance()`. Frame dt is explicitly stepped in exact intervals (e.g., 16ms), guaranteeing reproducible physical animation states without CPU jitter.
*   **YIQ/SSIM Diff Thresholds**: Render targets are read back to CPU and compared per-pixel using YIQ color space deltas and Structural Similarity Index Measure (SSIM). Thresholds: 99.9% structural match required. Minor subpixel anti-aliasing variations are tolerated up to 0.1%.
*   **Snapshot Naming & Storage**: Snapshots are stored as PNGs in `tests/snapshots/<module>_<test_name>_<os>_<arch>.png`. Git LFS is utilized for storage.
*   **Update Process**: If snapshots fail, CI uploads the `target/snapshots/diffs/` directory. Developers review diffs locally, run `UPDATE_EXPECT=1 cargo test` to overwrite local images, and commit the revised golden frames with explicit approval.
*   **CI Matrix**: Executed across `Windows × macOS × Linux` and `x86_64 × aarch64` against `vulkan`, `metal`, and `dx12` backends where available.

## 4. Fuzzing Targets

Using `cargo-fuzz`, the following system invariants must be subjected to continuous randomized input:

*   **Widget Arena**: Random insert, remove, query, and reparent sequences. Validates that stale 64-bit handles resolve safely to `None` without memory corruption or panics.
*   **Signal DAG**: Randomized graph generation and signal connection/disconnection patterns followed by massive parallel mutations. Validates topological sort stability and memory safety.
*   **Layout Solver**: Adversarial constraint inputs (NaN, Inf, conflicting intrinsic widths) fed to the Taffy resolver to ensure zero infinite loops or panics.
*   **Clipboard MIME Parsing**: Malformed and maliciously crafted payloads mimicking system clipboard data.
*   **Asset VFS Path Traversal**: Random unicode byte streams testing virtual file system boundaries and canonicalization constraints.
*   **Fluent Localization Bundle Parsing**: Fuzzing the `.ftl` parser against random syntax mutations.

## 5. v1.0.0 Quality Gate

The following immutable requirements must be fulfilled before the `v1.0.0` release tag is minted:

*   [ ] All Criterion benchmarks documented + within stated budgets
*   [ ] Golden frame suite: 100% pass rate on all 3 platforms
*   [ ] Zero `unsafe` blocks without `SAFETY` comment
*   [ ] docs.rs: 100% public API documented
*   [ ] `cargo deny`: 0 violations
*   [ ] `cargo semver-checks`: 0 breaking changes since last minor
*   [ ] AccessKit: WCAG 2.1 AA compliance verified
*   [ ] Fuzzing: 24h campaign with 0 crashes
*   [ ] Manual testing: all examples run on Windows/macOS/Linux
