# RED TEAM AUDIT: Geometry & Render Pipeline Assessment
**Target:** `martensite-layout`, `martensite-render`, `martensite-wgpu`
**Auditor:** Swarm Member 3 (Adversarial Systems Auditor)
**Date:** 2026-09-06

## 1. Two-Pass Layout vs Intrinsic Text Measurement Recursion

**Vulnerability: $O(N^2)$ Text Measurement Explosion**
In `DDR-0012`, `martensite-layout` implements `taffy::TraversePartialTree` to enforce a strict decoupled two-pass layout. However, when placing dynamically sized, multi-line text (`cosmic-text`) inside a flex or grid container, Taffy must query the text node's `MeasureFunc` multiple times during Pass 1 (Intrinsic Measurement) to ascertain min-content, max-content, and fit-content bounds under varying `AvailableSpace` constraints.

If `cosmic-text` re-evaluates glyph shaping, line-breaking, and bidi-resolution on every query, a deeply nested flex/grid hierarchy will trigger an $O(N^2)$ or worse exponential cascade of text shaping passes within a single frame. The DDR mentions returning "cached metrics" if a node is not dirty, but this completely ignores that Taffy will poll the *same* dirty node multiple times with *different* available widths during its constraint resolution phase.

**Concrete Remediation:**
You must implement a local, width-keyed memoization layer within the text node's `MeasureFunc`.
```rust
struct TextMeasureCache {
    /// Maps AvailableSpace (width constraint) to resulting shaped text bounds
    cache: std::collections::HashMap<taffy::AvailableSpace, taffy::Size<f32>>,
    /// The underlying cosmic-text buffer, reused to avoid reallocation
    buffer: cosmic_text::Buffer,
}
```
During layout computation, if the text content itself hasn't changed (only constraints have), `cosmic-text::Buffer::set_size` should only be called if the specific `AvailableSpace` hasn't been evaluated in the current layout frame.

## 2. Window Resize Quiescence & 1-Frame Jitter Deadlock

**Vulnerability: OS Modal Loop Deadlock & Frame Skipping**
`DDR-0022` defines a 5-state resize FSM meant to eliminate 1-frame jitter by aborting in-flight WGPU encoders and forcing a synchronous layout pass before presenting. 
However, on Windows (`WM_SIZE`) and macOS (`viewWillStartLiveResize`), resizing is handled by an OS-level modal tracking loop that blocks the primary thread.

If winit's event loop drops into the modal tracker, it only pumps specific resize/redraw events. If your FSM stalls the winit event handler waiting for the render thread to achieve "quiescence," you will deadlock the OS modal loop. Alternatively, if you drop the command encoder and defer presentation to a standard event loop tick that the OS modal loop starves, the OS will paint the window background (resulting in a blank white/black flash), causing the exact 1-frame jitter you sought to eradicate.

**Concrete Remediation:**
The FSM must not span asynchronous thread boundaries during a live resize. Upon receiving `WindowEvent::Resized`:
1. The main thread must synchronously invoke `surface.configure`.
2. The main thread must execute the entire `Taffy` layout pass (Pass 1 & Pass 2).
3. The main thread must encode the `PaintList` and submit the WGPU compute pass.
4. Call `surface.get_current_texture().present()` **synchronously** inside the winit `Resized` handler before returning control to the OS modal loop.

## 3. GPU Device Loss Resurrection Edge Cases

**Vulnerability: OS TDR (Timeout Detection and Recovery) Crash**
`DDR-0003` asserts that GPU device loss is intercepted on `surface.get_current_texture()` and entirely remediated within a `<16ms` frame window. 

This assumption is catastrophically false on Windows (DX12) and Linux (Vulkan). A driver crash (e.g., TDR) typically stalls the entire GPU subsystem for 1,000ms to 3,000ms. If you attempt to re-request a `wgpu::Adapter` immediately inside the error handler block, the OS will return `None` because the hardware has not re-initialized. The application will panic. 

Furthermore, device loss can surface asynchronously during `queue.submit()` or `device.poll()`, not just during surface acquisition.

**Concrete Remediation:**
1. Register `wgpu::Device::on_uncaptured_error` to catch asynchronous validation/OOM failures.
2. Implement an exponential backoff FSM for the `[HAL_RECONSTITUTION]` phase. Do not assume 16ms. If `request_adapter` fails, drop to a blocked `std::thread::sleep` retry loop (e.g., 100ms, 500ms, 1s) while rendering a static CPU-painted window background, or simply stall the winit event loop safely until the adapter is yielded by the OS.

## 4. TinySkia CPU Fallback Pixel-Parity & Anti-Aliasing Drift

**Vulnerability: CI Golden Frame SSIM Failures**
`ADR-0004` and `ADR-0019` establish `vello` (compute shader) and `tiny-skia` (CPU scanline) as dual-rendering backends, requiring strict golden frame parity for CI testing.

Vello uses hardware fine-rasterization compute shaders. TinySkia uses analytical CPU geometry coverage. The subpixel anti-aliasing (especially on curved primitives and font glyphs) will permanently diverge. If CI relies on a 99.9% structural similarity (SSIM) or strict pixel diffing, your tests will constantly flake because edge coverage will differ by subtle alpha values across the two rasterizers.

**Concrete Remediation:**
1. CI golden tests must be backend-specific (e.g., `button_test_vello.png` vs `button_test_tinyskia.png`). You cannot cross-compare them.
2. If cross-comparison is mandated by the Sovereign Architect, you must explicitly disable subpixel anti-aliasing in both pipelines during CI execution, and apply a spatial blur perceptual diff algorithm (e.g., dssim) rather than strict absolute-error pixel matching.
