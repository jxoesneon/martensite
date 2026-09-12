# [ADR-0033] Host-Mode External Surface Embedding

* **Status:** Accepted
* **Date:** 2026-09-14
* **Deciders:** Martensite Architecture Working Group
* **Technical Domain:** `martensite-engine-bridge`, `martensite-wgpu`,
  `martensite` (widget layer)
* **Amends:** ARCHITECTURE_BLUEPRINT §10.1 (which described guest-mode
  embedding); consistent with ADR-0009 (zero-copy handle import).

## Context and Problem Statement

External renderers — Bevy scenes, Godot viewports, CAD kernels, hardware
video decoders — must appear inside Martensite's widget tree with correct
layout, clipping, and compositing. Two architectural directions exist:

* **Direction A (host mode):** Martensite owns the window, winit event
  loop, wgpu device, swapchain, accessibility tree, and compositing pass.
  External engines render into textures Martensite imports or directly
  owns. External content is a widget.
* **Direction B (guest mode):** The engine owns the application loop and
  render graph; Martensite UI is injected as a render pass (e.g., a
  Bevy `RenderGraph` node after `Core3dSystems::MainPass`, a Godot
  `CompositorEffect`).

The blueprint originally described Direction B. Competitive analysis
(egui `PaintCallback`, iced `widget::shader`, Slint wgpu interop) shows
the durable ecosystem pattern is Direction A: "hand the framework a
texture or callback" is what every embeddable-GUI consumer actually uses.
Guest mode additionally bypasses Martensite's shell, focus, and
accessibility ownership — inverting the framework's core value.

Deep research on Godot (v0.15.0 spec) established that true zero-copy
*from* Godot is impossible without upstream engine patches: Godot render
targets lack export flags (`D3D12_HEAP_FLAG_SHARED`,
`VkExportMemoryAllocateInfo`, IOSurface backing) and GDExtension exposes
no fence/semaphore APIs. This holds regardless of direction chosen.

## Decision Drivers

* Martensite retains ownership of windowing, input, focus, and
  AccessKit — the framework's differentiating features.
* Zero GPU copies on the same-device path (the dominant in-process
  embedding case, e.g. Bevy).
* Extensibility to cross-device/cross-process handles (video decoders,
  out-of-process engines) without redesign.
* Graceful CPU-fallback behavior consistent with the TinySkia backend.

## Considered Options

* **Option 1**: Guest mode — Martensite as a render-graph node inside
  Bevy/Godot.
* **Option 2**: Host mode, same-device only — producers share the
  `wgpu::Device`; composite via direct `TextureView` sampling.
* **Option 3**: **Host mode, two-tier** — same-device path now; declared
  `FrameSync`/`NativeFrame` cross-device surface for platform-handle
  producers later.

## Decision Outcome

Chosen option: **Option 3**.

`martensite-engine-bridge` defines `Engine`/`Frame`/`FrameSync`/
`Viewport`; the `ExternalEngine` widget is a Taffy replaced-element leaf
emitting `PaintCommand::External`; `WgpuHost` composites via direct
texture sampling (fullscreen triangle), never Vello's `COPY_SRC` atlas
path. Same-queue `Queue::submit` ordering provides synchronization;
`Queue::add_wait_semaphore`/`add_wait_fence`/`add_wait_event` (wgpu 30)
back the declared cross-device variants.

### Positive Consequences

* One primitive serves Bevy viewports, video, and any future producer.
* Full UI compositing over external content: rounded clips, shadows,
  translucent overlays — impossible with OS child-window embedding.
* CPU fallback contract is explicit (`Engine::to_pixmap` or placeholder).

### Negative Consequences

* Godot integration is limited to the readback path (one CPU round-trip)
  or a one-GPU-copy shared-texture blit until upstream exposes exportable
  targets and sync primitives — documented in the v0.15.0 spec.
* Applications wanting Martensite-inside-Bevy (Direction B) are
  unsupported; judged the smaller market.
