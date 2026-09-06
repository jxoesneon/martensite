# Red Team Audit 04: OS Boundary & Hardware Media Saboteur
**Target:** OS window integration, hardware media pipeline, IME positioning, clipboard/DnD, event routing.
**Date:** 2026-09-06
**Auditor:** Swarm Member 4

## 1. Wayland Immediate Present Mode Protocol Violation & Spin-Lock
**Vulnerability:** Under `DDR-0011`, `PresentMode::Immediate` allows unbounded rendering for latency-critical apps. However, on Wayland, the core protocol operates on frame callbacks (`wl_surface.frame`). If a compositor does not support `wp_tearing_control_v1`, submitting buffers without waiting for frame callbacks is a protocol violation.
**Exploit Scenario:** 
1. The app forces `Immediate` mode.
2. The WGPU Vulkan backend maps this to `VK_PRESENT_MODE_IMMEDIATE_KHR`.
3. If Wayland fallback occurs, WGPU/Winit might continuously pump buffers to the compositor. The compositor throttles the client by refusing to release `wl_buffer`s.
4. The application exhausts its swapchain image pool, blocking in `get_current_texture()`, causing a main-thread deadlock or a panic due to `SurfaceTimeout`.
**Remediation:** 
Implement explicit fallback sensing. Before allowing `PresentMode::Immediate` on Linux, the framework MUST query Winit for `wp_tearing_control_v1` support. If unsupported, the framework must forcibly override the developer's request to `Mailbox` or `Fifo`, logging a `tracing::warn!`. The event loop must respect Wayland's frame callbacks to prevent swapchain exhaustion.

## 2. 10-Bit P010 HDR Compositing vs SDR UI Washing Out
**Vulnerability:** `DDR-0019` specifies that P010 video is mapped to linear scene-referred light via the PQ EOTF (nits / 10000). However, the standard UI widgets are rendered in sRGB. When alpha-blending a translucent UI widget (e.g., glass overlay) over the HDR video in the linear swapchain, the UI's relative white level is mismatched.
**Exploit Scenario:** 
1. An sRGB UI white pixel (1.0, 1.0, 1.0) is converted to linear space, representing SDR peak white (~80-100 nits).
2. The HDR video behind it outputs a specular highlight at 1.0 in PQ space (10000 nits).
3. Blending these in linear space results in the UI appearing almost invisible or aggressively washed out because 100 nits + (alpha * 10000 nits) overwhelms the SDR color bounds.
**Remediation:**
Introduce an SDR Reference White Level (e.g., 203 nits as per ITU-R BT.2408). All UI sRGB colors must be multiplied by this reference level when composited into the linear HDR swapchain. The WGSL compositing shader must perform color-space aware alpha blending:
`ui_color_linear = srgb_to_linear(ui_color) * (SDR_WHITE_NITS / 10000.0)`.

## 3. Kinetic IME Candidate Window Detachment
**Vulnerability:** `DDR-0006` states the IME composition window is anchored to the screen-space bounding box of the caret. However, OS window managers process IME bounds asynchronously via IPC.
**Exploit Scenario:**
1. A text field with active IME composition is placed inside a kinetic scroll view.
2. The user flicks the scroll view. The text field moves 20-30 pixels per frame.
3. The framework sends `set_ime_cursor_area` every frame, but the Wayland/macOS IME server updates with a 1-2 frame delay, causing the candidate popup to visually trail the text field.
4. If the text field scrolls out of the `CLIPS_CHILDREN` bounding box (DDR-0023), it visually disappears, but the OS still renders the IME candidate window floating outside the scroll area.
**Remediation:**
When a text node with an active IME session undergoes kinetic scrolling or affine transformation, the IME bounds must be clamped to the intersection of the text node's AABB and all parent clipping rects. If the intersection area is zero (fully occluded), the framework must send an empty bounds rect or explicitly suspend the IME window projection to hide the candidate list.

## 4. Multi-Window Drag-and-Drop Lazy Promise Use-After-Free
**Vulnerability:** `DDR-0005` implements zero-allocation delayed rendering (lazy promises) for clipboard/DnD. `ADR-0007` allows instantaneous widget reparenting and window destruction.
**Exploit Scenario:**
1. User starts a drag operation from Window A with a heavy payload (e.g., 4K image), registering a `Lazy` closure.
2. Window A is closed by the user or OS. 
3. The destination application requests the drag data. The OS invokes the lazy closure.
4. The closure captures references to Window A's local state or specific `Wgpu` surface textures that have been destroyed, resulting in a panic, dangling pointer, or `wgpu` validation error.
**Remediation:**
Lazy promises must capture `Weak` handles to the generative arena nodes, not strong references to Window or Surface state. If the source window is closed, the underlying data should either be instantly materialized (eagerly rendered) before the window drops, or the promise must gracefully return `None` (canceling the drop) if the source data is irrecoverable.

## 5. False Positives in Non-Rectangular & Transformed Hit-Testing
**Vulnerability:** `DDR-0023` specifies Axis-Aligned Bounding Box (AABB) intersection for hit-testing. This is mathematically inadequate for transformed or clipped nodes.
**Exploit Scenario:**
1. A UI node is rotated by 45 degrees. Its AABB in parent space expands significantly to enclose the rotated corners.
2. A user clicks in the "empty" space within the AABB but outside the actual rotated node.
3. The router falsely returns a `Consumed` hit on the rotated node, swallowing the click intended for the background widget beneath it.
4. The same applies to `BorderRadius` (rounded corners). Clicks in the transparent corner pixels trigger button presses.
**Remediation:**
Enhance the `HitTestResult` and routing pipeline. 
1. **Transform Inverse Routing:** The hit point must be multiplied by the node's inverse affine transform matrix *before* bounds checking.
2. **Path/Clip Masking:** If a node has rounded corners or a custom path clip, the hit-test must evaluate the mathematical distance function (e.g., SDF) of the clip. If the local point is outside the rounded corner, it must return `PassThrough` and continue reverse Z-order traversal.
