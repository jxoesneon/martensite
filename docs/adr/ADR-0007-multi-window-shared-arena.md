# [ADR-0007] Multi-Window Topology & Shared Generational Arena

* **Status:** Accepted
* **Date:** 2026-09-06
* **Deciders:** Master (Sovereign Architect), Ciel (Systems & Experience Guilds)
* **Technical Domain:** `martensite-window`, `martensite-core`, `martensite-wgpu`

## Context and Problem Statement

Workstation-grade desktop applications (CAD, Digital Audio Workstations, visual node editors, multi-monitor trading terminals) require multiple top-level OS windows, floating utility panels, and detachable tear-off tabs. 
* **State Destruction on Detach**: In existing UI architectures (Chromium/Electron, Qt, Flutter), dragging a tab out of a main window to spawn an independent window destroys the original widget instance and reconstructs an equivalent tree from scratch. This resets internal state, aborts active network/audio pipelines, interrupts undo stacks, and causes perceptible visual lag.
* **Resource Duplication**: Naive multi-window implementations instantiate independent graphics contexts (`wgpu::Device`, `wgpu::Queue`, shader pipelines, and font caches) per window, multiplying VRAM footprint linearly.
* **VSync Starvation**: Blocking sequential presentation loops cause a slow secondary display (e.g., a 60Hz office monitor) to throttle high-refresh primary displays (e.g., a 144Hz gaming/workstation monitor) down to lowest-common-denominator framerates.

## Decision Drivers

* O(1) instantaneous widget reparenting across OS windows without state loss.
* Zero VRAM duplication across multi-window workspaces.
* Completely decoupled per-window presentation framerates and fractional DPI scaling.

## Considered Options

* **Option 1**: Isolated process or isolated WGPU context per OS window (IPC/serialization bridge).
* **Option 2**: Single-window virtual desktop (docking only inside a single OS window envelope; no true native floating windows).
* **Option 3**: **Unified Hardware Context + Shared Generational Arena + Decoupled WGPU Surfaces**.

## Decision Outcome

Chosen option: **Option 3**, because it unifies all windows under a single reactive memory space, allowing pointerless handle reparenting while maintaining hardware efficiency.

### Positive Consequences

* **Instantaneous Tear-Off (O(1))**: A tab pulled from Window A to Window B simply mutates its `window_id` affinity tag. The backing node in the generational slotmap arena is untouched; active video streams, signals, and text buffers persist seamlessly.
* **Zero VRAM Duplication**: A singleton `wgpu::Instance`, `Adapter`, `Device`, and `Queue` power all `wgpu::Surface` instances. Pipelines and texture atlases are uploaded once.
* **Decoupled Refresh Pacing**: Per-window `request_redraw` scheduling coupled with non-blocking presentation (`wgpu::PresentMode::AutoVsync` or `Mailbox`) allows 144Hz and 60Hz displays to run at their physical hardware limits simultaneously.

### Negative Consequences

* **Surface Reconfiguration Matrix**: Resizing or moving windows across fractional DPI boundaries (100% -> 150%) requires independent swapchain reconfigurations without locking the shared device.
* **Complex Scissor Clamping**: Popups extending beyond window borders must spawn dedicated secondary sub-surfaces rather than rendering to parent coordinate spaces.
