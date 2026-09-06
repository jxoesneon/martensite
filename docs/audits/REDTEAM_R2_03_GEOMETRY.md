# RED TEAM AUDIT REPORT: Round 2 Geometry & Render Pipeline
**Target:** `martensite-layout`, `martensite-render`, `martensite-wgpu`
**Red Team Specialist:** Saboteur 3
**Date:** 2026-09-06

## 1. `TextShapeCache` Memory Exhaustion & LRU Thrashing

**Status:** VULNERABLE - CATASTROPHIC PERFORMANCE COLLAPSE (THRASHING)

**The Flaw (The 1024-Entry Thrashing Trap):**
The Blue Team implemented an `LruCache` hardcoded to 1024 entries to prevent memory exhaustion. However, they ignored the spatial requirements of standard UI structures (like log viewers, chat feeds, or code editors) which can easily contain >1000 visible text nodes on screen simultaneously. 

Because Taffy performs multi-pass layout, it measures nodes in Pass 1 and then often queries them again in Pass 2 (Placement). If the UI contains 1,500 text nodes, Pass 1 will measure all 1,500. By the time Pass 1 reaches node 1025, node 1 is **evicted**. When Taffy executes Pass 2 and queries node 1 again, it results in a cache miss, triggering a full expensive `cosmic_text` reshape. This causes a permanent cache miss rate of 100% for all text queries during multi-pass layout, resulting in continuous allocator churn and dropping frame rates to single digits. Furthermore, the `scratch_buffer: cosmic_text::Buffer` grows its internal vectors to the maximum length of any observed string and never shrinks, causing a permanent memory high-water mark.

**Remediation:**
- **Per-Frame Arena/Bump Allocation:** Instead of a global bounded LRU, tie the text cache lifetime to the UI tree's layout epoch.
- **Dynamic Sizing:** If an LRU must be used, its capacity must dynamically scale with the number of text nodes currently present in the DOM (e.g., `capacity = text_node_count * 2`).
- **Buffer Compaction:** The `scratch_buffer` must have a compaction heuristic to shrink back down if it was uniquely resized by an anomalously massive string.

---

## 2. Synchronous Modal Resize Deadlock with DWM/OS Compositor

**Status:** VULNERABLE - DEADLOCK & DWM "GHOSTING"

**The Flaw (WM_SIZE Blocking):**
The Blue Team explicitly forces a synchronous sequence of layout, paint, and swapchain `present()` inside the `winit` `WindowEvent::Resized` callback (which on Windows executes within the `WM_SIZE` modal event pump). 

The `frame.present()` call can and will block the thread waiting for the GPU swapchain (especially if V-Sync is on, or the GPU is under heavy load). By blocking the main thread inside the OS message pump, the application stops pulling Windows messages. If the user is vigorously resizing the window, the synchronous blocking rapidly accumulates. If it exceeds 5 seconds, Windows Desktop Window Manager (DWM) assumes the process has deadlocked, marks the application as "Not Responding", and replaces the UI with a frozen ghost window.

**Remediation:**
- **Decoupled Render Thread:** `WM_SIZE` must immediately acknowledge the resize and update an atomic/shared requested-size variable, returning control to the OS instantly. 
- The render/layout engine must run on a separate thread (or asynchronously outside the event pump) and pick up the new surface configuration on its next frame loop, rendering the resized frame without stalling the OS compositor pump.

---

## 3. GPU Resource Resynchronization After TDR

**Status:** VULNERABLE - FATAL PANIC ON DEVICE RESTORE

**The Flaw (Cross-Device Resource Contamination):**
The Blue Team's `GpuState::attempt_reconstitution` successfully requests a new `wgpu::Adapter` and transitions to `GpuState::Recreated`. However, they entirely neglected the fundamental architecture of modern GPU APIs: **GPU resources are permanently bound to the `Device` that created them.**

When the TDR occurs and a new `Device` is initialized, all existing textures, compiled shaders, pipeline layouts, vertex buffers, and the Vello renderer state still belong to the *lost* device. When the engine attempts to submit the next `PaintList` to the new device using the old cached Vello renderer or old font glyph caches, `wgpu` will immediately panic with a cross-device validation error (e.g., "Resource belongs to a different device"). 

**Remediation:**
- **Global Resource Invalidation:** The transition to `GpuState::Recreated` must trigger a recursive invalidation event across the entire UI tree.
- **Renderer Reinitialization:** The Vello renderer must be completely dropped and re-instantiated with the new `Device`.
- **Asset Re-upload:** All textures, font glyph atlases, and cached paths must be marked dirty and re-uploaded to the new GPU device during the first frame of the `Restored` state.
