# [ADR-0004] In-Register Compute-Centric 2D Vector Rendering

* **Status:** Accepted
* **Date:** 2026-09-06
* **Deciders:** Martensite Architecture Working Group
* **Technical Domain:** `martensite-wgpu`, `martensite-render`

## Context and Problem Statement

Rendering modern user interfaces requires crisp, anti-aliased vector paths, arbitrary rounded corners, complex drop shadows, linear/radial gradients, and high-DPI text rasterization at sustained 120 FPS workstation framerates.
* **Legacy Rasterization (Skia, Cairo)**: Skia relies on extensive CPU tessellation or complex stencil-then-cover GPU pipelines. It requires heavy C++ dependencies, massive binary bloat (~30MB), and frequent PCIe memory bandwidth stalls.
* **Naive Vertex Tessellation (`lyon`, `wgpu` mesh builders)**: Tessellating curves on the CPU into triangle fans generates millions of vertices per frame, saturating vertex buffers and memory buses during animation.

We must decide on a graphics rendering architecture that achieves peak vector fidelity and framerate independence while remaining 100% pure Rust.

## Decision Drivers

* **Compute-Centric Architecture**: Shift vector curve evaluation and anti-aliasing entirely onto GPU compute shaders.
* **Pure-Rust Supply Chain**: Zero C/C++ dependencies (`libskia`, `libharfbuzz`).
* **Sub-Millisecond Frame Encoding**: Render complex workstation interfaces in $<1.0\text{ms}$ GPU compute time.

## Considered Options

* **Option 1**: CPU-based tessellation with hardware vertex shaders (`lyon` + `wgpu`).
* **Option 2**: C++ Skia bindings via FFI (`skia-safe`).
* **Option 3**: **In-Register Compute-Centric 2D Vector Rendering via `Vello` / `wgpu` with SIMD `TinySkia` CPU Fallback**.

## Decision Outcome

Chosen option: **Option 3**, adopting `Vello` (Linebender) as the primary compute rasterizer, backed by pure-Rust `TinySkia` for software fallback on machines without compute support.

### Positive Consequences

* **GPU In-Register Blending**: Curves and paths are evaluated directly in GPU compute shader registers, bypassing CPU tessellation bottlenecks entirely.
* **Hardware Agnostic**: Runs over pure WGPU (Vulkan, DirectX 12, Metal, WebGPU).
* **Zero-Allocation Paint List**: The UI emits a lightweight intermediate command stream (`PaintList`) that serializes directly into Vello compute buffers.
