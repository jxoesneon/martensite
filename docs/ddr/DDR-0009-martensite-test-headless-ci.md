# [DDR-0009] Headless CI Testing Harness & Golden Snapshot Pipeline Specification

* **Subsystem:** `martensite-test`
* **Status:** Approved
* **Authors:** Ciel (Specialist Guilds: Quality, Testing & Infrastructure)
* **Related ADRs:** ADR-0004, ADR-0005

## 1. System Topology & Testing Invariants

`martensite-test` provides an automated headless test harness for continuous integration.
It allows developer suites to test real user interactions, verify layout mathematics, and execute golden pixel snapshots without requiring a physical monitor or GPU hardware display.

### 1.1 Architecture & Mock Backend
```
┌──────────────────────────────────────────────────────────────┐
│                    MARTENSITE TEST HARNESS                   │
├──────────────────────────────────────────────────────────────┤
│  `TestHarness` / `TestDriver`                                │
│    • Synthesizes mouse, keyboard, touch, and resize events   │
│                            │                                 │
│                            ▼                                 │
│  `VirtualClock` (Deterministic Time Stepper)                 │
│    • `clock.step_by(Duration::from_millis(16))`              │
│    • Bypasses wall-clock time; 100% reproducible animations │
│                            │                                 │
│                            ▼                                 │
│  `MockWindowBackend` (Headless Surface)                      │
│    • Offscreen WGPU texture render target                    │
│    • CPU fallback via SIMD `tiny-skia`                       │
│                            │                                 │
│                            ▼                                 │
│  Perceptual Diffing Engine                                   │
│    • 256-byte row-aligned buffer readback                    │
│    • YIQ Color Delta & SSIM perceptual comparison            │
└──────────────────────────────────────────────────────────────┘
```

* **Invariant 1.1 (Zero Flakiness)**: Time is virtualized via `VirtualClock`. Animations advance by exact step intervals, preventing CPU throttling jitter.
* **Invariant 1.2 (Headless Execution)**: All unit and snapshot tests must execute cleanly in headless Docker containers and GitHub Actions runners via Mesa Lavapipe software Vulkan.
