# Detailed Design Record: DDR-0017
## Title: `martensite-devtools` Tracing Architecture & F12 HUD

### 1. Architectural Role & Invariants
`martensite-devtools` establishes telemetry and frame profiling without degrading release performance.
* **Invariant 1.1**: All tracing spans compile to no-ops in release mode unless the `tracing` feature flag is explicitly enabled.
* **Invariant 1.2**: CPU spans integrate with `tracing` and `tracy-client`. GPU timestamps use `wgpu::QuerySet`.
* **Invariant 1.3**: The in-app DevTools HUD must render asynchronously or last, never influencing the application's Taffy layout measurements.

### 2. Canonical Spans
The main loop emits strictly 9 high-level hierarchical spans:
1. `poll_events`
2. `reactive_flush`
3. `layout_measure`
4. `layout_place`
5. `paint_list_gen`
6. `vello_scene_build`
7. `vello_encode`
8. `wgpu_submit`
9. `present_swapchain`

### 3. wgpu QuerySet GPU Profiling
```rust
pub struct GpuProfiler {
    query_set: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    destination_buffer: wgpu::Buffer,
}

// Pseudo-usage in rendering:
// encoder.write_timestamp(&query_set, 0);
// vello_render();
// encoder.write_timestamp(&query_set, 1);
// encoder.resolve_query_set(&query_set, 0..2, &resolve_buffer, 0);
```

### 4. F12 HUD Overlay
Toggled via `F12`. Draws a floating overlay directly manipulating the `vello::Scene` *after* the application `PaintList` has been committed. Displays:
- CPU frame time (ms)
- GPU frame time (ms)
- Active Arena Nodes (count)
- Resident Memory (RSS)

### 5. Performance Invariants
- Overhead of DevTools HUD: < 0.5ms.
- Memory overhead: Pre-allocated circular buffers, zero dynamic allocation per frame.
