# Detailed Design Record: DDR-0022
## Title: Window Resize and Layout Quiescence Protocol

### 1. Architectural Role & Invariants
This document outlines the strict finite state machine (FSM) governing window resize operations. Resizing a window fundamentally alters the GPU surface and requires layout recalculations across the entire scene graph.
* **Invariant 1.1**: The application must never present a frame that contains layout geometry calculated for the old window size to a surface configured for the new window size (preventing 1-frame jitter/stretching).
* **Invariant 1.2**: WGPU command encoders currently in-flight during a resize event must be immediately aborted.

---

### 2. The Resize Finite State Machine (FSM)

The resize protocol is modeled as a 5-state FSM bridging the winit event thread and the render thread.

```
   ┌──────────────────────────────────────────────────────────┐
   │                        [IDLE]                            │
   │  Normal rendering loop; winit event loop polling.        │
   └────────────────────────────┬─────────────────────────────┘
                                │ winit::WindowEvent::Resized
                                ▼
   ┌──────────────────────────────────────────────────────────┐
   │                   [RESIZE_PENDING]                       │
   │  Set LAYOUT_DIRTY flag on root node.                     │
   │  Abort current wgpu encoder (drop `encoder.finish()`).   │
   └────────────────────────────┬─────────────────────────────┘
                                │ Main thread initiates surface update
                                ▼
   ┌──────────────────────────────────────────────────────────┐
   │                [SURFACE_RECONFIGURED]                    │
   │  Call `surface.configure()` with new physical size.      │
   └────────────────────────────┬─────────────────────────────┘
                                │ Next frame initiated
                                ▼
   ┌──────────────────────────────────────────────────────────┐
   │                   [LAYOUT_COMPUTED]                      │
   │  Synchronous layout pass recalculates entire tree sizes. │
   └────────────────────────────┬─────────────────────────────┘
                                │ Layout complete; Render dispatch
                                ▼
   ┌──────────────────────────────────────────────────────────┐
   │                      [PRESENTED]                         │
   │  Submit newly correctly-sized frame to GPU swapchain.    │
   │  Transition back to IDLE.                                │
   └──────────────────────────────────────────────────────────┘
```

---

### 3. Synchronization and Anti-Jitter Guarantees

When a `WindowEvent::Resized` event is popped from the winit event loop, the synchronization point between the OS windowing system and the Martensite render thread engages.

1. **Abort In-Flight Render**: If a render pass is actively encoding commands for the previous size, it is violently aborted by dropping the `wgpu::CommandEncoder` without calling `finish()` or `queue.submit()`. This guarantees no "stale" frames are queued.
2. **Surface Reconfiguration**: The WGPU `Surface` is reconfigured synchronously on the main thread using the exact pixel dimensions provided by winit.
3. **Synchronous Layout**: The root node's bounds are updated to the new surface size, and a complete top-down layout pass is triggered. The render thread is stalled until this layout pass completes.
4. **Presentation**: Only after layout quiescence is reached is the new frame recorded and submitted to the GPU. This guarantees that the first frame drawn after a resize is visually perfect, entirely eliminating the 1-frame scaling jitter common in reactive UI frameworks.
