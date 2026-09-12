# martensite-wgpu

[![Crates.io](https://img.shields.io/crates/v/martensite-wgpu.svg)](https://crates.io/crates/martensite-wgpu)
[![Documentation](https://docs.rs/martensite-wgpu/badge.svg)](https://docs.rs/martensite-wgpu)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Compute-centric 2D GPU rasterization and self-healing WGPU device resurrection engine for Martensite.**

---

## Overview

`martensite-wgpu` provides the hardware-accelerated compute rendering pipeline for Martensite. Rather than relying on traditional fixed-function triangle tessellation, it coordinates compute-centric vector rendering (via [Vello](https://github.com/linebender/vello)) over [wgpu](https://github.com/gfx-rs/wgpu).

Crucially, `martensite-wgpu` incorporates a **formal typestate resurrection engine** (`RecoveryMachine`). When hardware drivers crash, display cables disconnect, or mobile GPUs enter low-power sleep (triggering GPU device loss), the engine handles reconnection with exponential backoff and can transparently divert frames to the TinySkia CPU software backend without crashing the host application.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **Compute-Centric 2D Rasterization**: High-fidelity vector graphics, gradient fills, and subpixel anti-aliased text rendered directly via GPU compute shaders.
- **Typestate Device Resurrection Engine (`RecoveryMachine`)**:
  - Automatically captures `wgpu::SurfaceError::Lost` and device loss events.
  - Executes exponential backoff recovery attempts within a bounded time budget (`RECOVERY_BUDGET`).
  - Seamlessly transitions `RenderMode::Gpu` to `RenderMode::Cpu` when recovery limits are exceeded.
- **Pipeline Orchestrator (`RenderOrchestrator`)**: Unifies GPU swapchains and CPU pixel presentation behind an atomic frame submission interface.
- **Surface & Swapchain Lifecycle (`SurfaceWrapper`)**: Dynamic swapchain resizing, present mode negotiation (FIFO vs Mailbox), and fractional DPI scaling management.
- **100% Safe Rust**: Zero `unsafe` blocks across all device and surface abstractions.

---

## Quick Start

Add `martensite-wgpu` to your `Cargo.toml`:

```toml
[dependencies]
martensite-wgpu = "0.14.0"
```

Configuring the render orchestrator:

```rust
use martensite_wgpu::{OrchestratorConfig, RenderMode, RenderOrchestrator};

fn main() {
    let config = OrchestratorConfig {
        allow_software_fallback: true,
        prefer_cpu: false,
    };

    let orchestrator =
        RenderOrchestrator::new(800, 600, config).expect("orchestrator init");
    assert_eq!(orchestrator.mode(), RenderMode::Gpu);
}
```

---

## Device Loss State Machine

```text
       +---------------------------------------------+
       |                  Healthy                    |
       |             (RenderMode::Gpu)               |
       +---------------------------------------------+
                              |
                     [Device Lost Event]
                              v
       +---------------------------------------------+
       |                 Recovering                  | <----+ (Retry with Backoff)
       |          (Exponential Backoff FSM)          | -----+
       +---------------------------------------------+
                 |                         |
         [Success within Budget]    [Timeout Exceeded]
                 v                         v
       +--------------------+    +--------------------+
       |      Healthy       |    |    CPU Fallback    |
       | (RenderMode::Gpu)  |    | (RenderMode::Cpu)  |
       +--------------------+    +--------------------+
```

---

## Part of Martensite

This crate provides the GPU hardware layer for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
