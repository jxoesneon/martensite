# ADR-0019 — Software CPU Fallback Rendering

* **Status:** Accepted — Decision Finalised
* **Date:** 2026-09-06
* **Deciders:** Jose Eduardo Rojas Jimenez (Sovereign Architect)
* **Technical Domain:** `martensite-render`, `martensite-wgpu`
* **OQ:** OQ-2 — resolved 2026-09-06

---

## Context

wgpu adapter initialization fails in three known environments:
1. CI runners without GPU hardware or Mesa software Vulkan
2. Headless servers (no display, no GPU)
3. Embedded Linux targets lacking Vulkan drivers

The question is what happens when `App::build().run(...)` is called in these environments: fail loudly, or fall back transparently to a software rasterizer (TinySkia).

## Decision Drivers

- **Law I (Pixel Sovereignty):** Every pixel rendered via Martensite must be produced by the same pipeline. Silently switching backends mid-deployment means the developer never discovers a severe performance regression.
- **Anti-Slop Doctrine §4.2 (Iron Law of Verification):** No capability shall be hidden. A 100× performance gap between GPU and CPU rendering is a capability gap the developer must consciously own.
- **CI/headless validity:** Headless testing is a legitimate requirement. The answer is an explicit flag, not a silent fallback.

## Considered Options

### Option A — Silent automatic fallback
On GPU init failure, quietly fall back to TinySkia. Developer gets a running app. Performance regression may never be noticed in production.

**Rejected.** This violates the Iron Law of Verification. An application unknowingly running on software rasterization in production and shipping to users is a silent defect.

### Option B — Hard error with explicit opt-in (chosen)
On GPU init failure, `App::run()` returns `Err(MartensiteError::NoGpuAdapter)` unless `allow_software_fallback(true)` was set. When the flag is set, TinySkia is used and a `tracing::warn!` is emitted on every startup.

### Option C — Separate headless crate
A `martensite-headless` crate that never attempts GPU init. Rejected: adds crate complexity and splits the test story.

## Decision Outcome

**Option B — Hard error with explicit opt-in.**

```rust
// Production — fails at startup if no GPU adapter found:
App::build().run(|cx| { ... });
// → Err(MartensiteError::NoGpuAdapter { reason: "..." })

// CI / headless / explicit software render:
App::build()
    .allow_software_fallback(true)
    .run(|cx| { ... });
// → Ok(()) — runs via TinySkia, emits tracing::warn! on startup
```

**What TinySkia provides:**
- Same `PaintList` input as the GPU backend — no API difference for the widget author
- SIMD-accelerated CPU rasterization via `tiny-skia` with `simd` feature
- Correct pixel output for golden frame CI tests
- ~50–100× lower throughput than GPU at 4K; acceptable for test harness, unacceptable for production

**What the flag does NOT change:**
- `PresentMode::Immediate` is still honoured (or ignored if `softbuffer` doesn't support it)
- Accessibility, layout, signals, and event routing are unaffected
- The warning is non-suppressible; it fires on every `App::run()` call

## Consequences

**Positive:**
- GPU regressions are impossible to accidentally hide in CI — the flag must be set
- Software render path is a first-class tested code path, not an afterthought
- API is self-documenting: the flag name makes the contract explicit

**Negative:**
- CI pipelines that currently rely on ambient Mesa Lavapipe must add the flag explicitly
- Docker-based CI requires either `--allow_software_fallback` or a software Vulkan layer (Mesa `llvmpipe`)

## Implementation Notes

- ADR-0005 (pure-Rust mandate) is satisfied: `tiny-skia` is pure Rust, no C build deps
- `martensite-wgpu` exposes `GpuBackend::Wgpu(GpuContext)` and `GpuBackend::Software(SoftwareContext)`
- `GpuBackend::Software` wraps `softbuffer` for window surface presentation + `tiny-skia` for rasterization
- The backend is chosen once at startup; no runtime switching
