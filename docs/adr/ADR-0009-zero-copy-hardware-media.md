# [ADR-0009] Zero-Copy Hardware Media Surfaces & External 3D Interop

* **Status:** Accepted
* **Date:** 2026-09-06
* **Deciders:** Master (Sovereign Architect), Ciel (Systems, Spatial & Graphics Guilds)
* **Technical Domain:** `martensite-media`, `martensite-wgpu`

## Context and Problem Statement

Integrating high-framerate (60–120 FPS) 4K video streams, hardware-accelerated WebRTC pipelines, and external 3D engines (e.g., Bevy simulation viewports, CAD modelers) within UI containers is a fundamental requirement of industrial software. 
* **The PCIe Bus Saturation Disaster**: Transferring decoded 4K frames through host RAM via CPU-mediated copies requires ~3.98 GB/s bandwidth. Mapping framebuffers to CPU memory, copying pixels, and re-uploading via `wgpu::Queue::write_texture` saturates memory buses, introduces severe thermal throttling, and triggers catastrophic frame drops.
* **YUV Color Conversion CPU Overhead**: Video decoders emit multi-planar NV12 or P010 YUV streams. Converting YUV to RGB on the CPU wastes significant processor cycles.
* **Lack of Container Integration**: Traditional video integrations render via OS overlay child windows that sit on top of the UI, preventing UI elements from casting shadows, applying rounded corner clips, or drawing translucent glass/acrylic overlays across the video surface.

## Decision Drivers

* Absolute zero CPU memory copies for video and 3D rendering pipelines.
* First-class UI integration: external video and 3D surfaces must support rounded clipping, anti-aliased borders, and drop shadows.
* Universal hardware API interop (DirectX 12, Metal, Vulkan).

## Considered Options

* **Option 1**: CPU memory buffer streaming (`queue.write_texture`).
* **Option 2**: Native OS child window overlays (`HWND`, `NSView`, `wl_subsurface`).
* **Option 3**: **Platform Zero-Copy Hardware Handle Import + WGPU In-VRAM YUV-to-RGB & SDF Compositing Pipeline**.

## Decision Outcome

Chosen option: **Option 3**, because it preserves 100% of PCIe and CPU bandwidth while giving the UI compositor full pixel-level control over clipping, borders, and visual effects.

### Positive Consequences

* **Zero PCIe Bandwidth Cost**: Decoded video frames and 3D viewports remain resident in VRAM. Textures are imported via shared platform handles (Windows DXGI NT Shared Handles, macOS `IOSurface`, Linux `dma-buf`).
* **Single-Pass GPU Conversion**: An analytical WGSL shader converts BT.709/BT.2020 YUV planes to linear sRGB while simultaneously evaluating an analytical Signed Distance Field (SDF) for sub-pixel anti-aliased rounded corners and metallic borders.
* **First-Class Bevy/CAD Integration**: External engines render directly into an offscreen WGPU texture that aliases directly into a Martensite widget container.
