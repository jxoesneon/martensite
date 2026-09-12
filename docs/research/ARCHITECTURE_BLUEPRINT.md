# Martensite — Architectural Specification & Ecosystem Roadmap

**Status:** Architectural Synthesis (Post-v0.10.0)  
**Target Domain:** High-Performance, Low-Overhead Native Desktop GUI Framework in Rust  
**Methodology:** Comprehensive Investigation Across 10 Native Desktop Domains  

---

## Executive Overview

Martensite v0.10.0 established a verified foundation: a 22-crate workspace providing direct GPU compute vector rasterization (via Vello and WGPU), a fine-grained push-pull topological reactive DAG (`martensite-reactive`), a generational arena-backed widget hierarchy, a non-linear LCA branching state ledger (`martensite-history`), and deterministic headless CI testing with virtual clocks (`martensite-test`).

To address modern native desktop requirements comprehensively across consumer, creative, and enterprise environments, Martensite systematically develops ten foundational domains. This document synthesizes the architectural specifications, mathematical formulations, platform APIs, and implementation roadmaps across each domain.

```
+──────────────────────────────────────────────────────────────────────────────────────────+
|                               MARTENSITE ARCHITECTURAL SCOPE                             |
+──────────────────────────────────────────────────────────────────────────────────────────+
|  1. ACCESSIBILITY & A11Y      |  2. BLESSED WIDGET TIER       |  3. PACKAGING & UPDATES  |
|  - AccessKit Node Tree        |  - 1M-Row Virtualized DataGrid|  - WiX v5 / MSIX Sparse  |
|  - NSAccessibility / UIA      |  - TreeGrid & Docking BSP Tree|  - Notarized macOS DMG   |
|  - Dynamic Live Regions       |  - Fuzzy Command Palette (K-D)|  - AppImage & Flatpak    |
|  - Section 508 VPAT Compliance|  - High-Throughput Telemetry  |  - Ed25519 Delta Updates |
+───────────────────────────────+───────────────────────────────+──────────────────────────+
|  4. ADVANCED TYPOGRAPHY       |  5. OS WINDOWING & SHELL      |  6. INPUTS & KINEMATICS  |
|  - HarfBuzz / Swash Complex   |  - Client-Side Decoration(CSD)|  - 0.55 Rubber-Banding   |
|  - Unicode BiDi (UAX #9)      |  - Win11 Mica / Mica Alt      |  - 6-DoF Stylus & Kalman |
|  - System Fallback Cascades   |  - macOS Liquid Glass Vibrancy|  - Damped IME Projection |
|  - 4-Tier Shaper/Glyph Cache  |  - Wayland Fractional Scale   |  - W3C Spatial Navigation|
+───────────────────────────────+───────────────────────────────+──────────────────────────+
|  7. GPU COMPUTE & HDR         |  8. DEVELOPER EXPERIENCE      |  9. STATE & CONCURRENCY  |
|  - Vello Sort-Middle Pipeline |  - Sub-350ms cdylib Hot Reload|  - Push-Pull Topo DAG    |
|  - Closed-Form erf Drop Shadow|  - In-App HUD (F12) & Tracy   |  - Dial's Bucket Queue   |
|  - SMPTE ST 2084 PQ EOTF      |  - TokenStream Proc-Macros    |  - Dynamic Epoch Pruning |
|  - 5-State Device Loss FSM    |  - Copyable Generational IDs  |  - 100k msg/s Conflation |
+───────────────────────────────+───────────────────────────────+──────────────────────────+
| 10. ENGINE EMBEDDING & STREAMING                                                         |
|  - In-Pass Swapchain Overlay (Bevy ECS & Godot 4 GDExtension RenderingDevice)            |
|  - Zero-Copy DXGI Shared NT Handles / Apple IOSurface / Linux DRM dma-buf                |
|  - Headless Deterministic CI with VirtualClock and DSSIM Perceptual Diffing              |
|  - Off-Screen Pixel Streaming via NVENC / WebRTC RTP / SCTP DataChannels                |
+──────────────────────────────────────────────────────────────────────────────────────────+
```

---

## Domain 1: Accessibility (a11y) & Assistive Technology

### 1.1 Architectural Principle
Accessibility is a structural phase of the frame pipeline. Martensite integrates **AccessKit** directly into `martensite-core` and the layout pipeline.

### 1.2 The Frame Synchronization Pipeline
```
[Event Loop / Input] ──> [Reactive Signal Flush] ──> [Taffy Layout Pass]
                                                            │
                 ┌──────────────────────────────────────────┴────────────────────────┐
                 ▼                                                                   ▼
       [PaintList Command Stream]                                           [AccessKit Node Tree Update]
                 │                                                                   │
       [GPU Rasterization]                                                  [OS Accessibility Engine]
  (Vello Compute / RenderPass)                                              (UIA, NSAccessibility, AT-SPI)
```

1. **Incremental Tree Mutation**: Every widget registers an `accesskit::NodeId` mapped from its generational `WidgetId`.
2. **Layout Projection**: During the post-layout phase, computed world-space bounding boxes (`kurbo::Rect`) are copied into `accesskit::Node::set_bounds()`.
3. **Platform Adapters**:
   - **Windows**: `accesskit_windows` maps nodes to UI Automation (UIA) providers (`IRawElementProviderSimple`, `ITextProvider`).
   - **macOS**: `accesskit_macos` translates nodes into `NSAccessibility` protocols and elements.
   - **Linux**: `accesskit_unix` communicates with AT-SPI2 over D-Bus (`org.a11y.Bus`).

### 1.3 Assistive Technology Requirements
- **Live Regions**: Essential for real-time applications (telemetry, progress indicators). Signals bound to live regions emit `accesskit::Live::Polite` or `accesskit::Live::Assertive` alerts when text mutates.
- **Dynamic Action Handlers**: Exposes standard AccessKit actions: `Click`, `Focus`, `Blur`, `ScrollIntoView`, `SetValue`, and `Increment`/`Decrement` on sliders.
- **Screen Reader Caret Tracking**: Text selection and cursor movements emit `accesskit::ActionData::SetTextSelection` so screen readers (NVDA, JAWS, VoiceOver, Orca) speak individual glyphs or words as the cursor navigates.

---

## Domain 2: The Blessed Widget Tier

A robust desktop framework provides complex, high-throughput components built-in to ensure predictable performance and uniform accessibility.

### 2.1 Virtualized 1,000,000-Row DataGrid
- **Viewport Virtualization**: For $N = 1,000,000$ rows, only $M \approx 30$ visible row containers are allocated.
- **Geometry Calculation**:
  $$\text{Visible Range} = \left[ \left\lfloor \frac{y_{\text{scroll}}}{h_{\text{row}}} \right\rfloor, \; \min\left(N - 1, \left\lceil \frac{y_{\text{scroll}} + h_{\text{viewport}}}{h_{\text{row}}} \right\rceil \right) \right]$$
- **Column Operations**: Reordering via drag-and-drop, interactive column resizing with cursor snapping, column freezing (sticky left/right pinning), multi-column stable sort ($O(K \log N)$), and multi-tier column filtering.
- **Memory Footprint**: Strict zero-allocation during scrolling; cells update via signal recycling.

### 2.2 Hierarchical TreeGrid
- Asynchronous lazy expansion of child nodes with breadcrumb memory.
- Multi-column tree hierarchy with indent guides and expansion disclosure controls.

### 2.3 Binary Space Partitioning (BSP) Docking System
- Modular docking system: panels can be docked to viewport edges (North, South, East, West, Center) or floated into detached OS windows with independent swapchains.
- Dynamic layout serialization to JSON/RON for saving and restoring user workspace configurations.

### 2.4 Command Palette (Fuzzy Matcher)
- Quick-open command launcher (`Ctrl+K` / `Cmd+K`) powered by a Smith-Waterman / Nucleo fuzzy matching algorithm.
- Keybinding registry, action grouping, MRU history, and contextual filtering.

### 2.5 High-Throughput Financial & Scientific Visualizations
- Real-time time-series line charts, candlesticks, and heatmaps supporting $> 500,000$ data points at 120 FPS.
- Level-of-Detail (LOD) downsampling via the **Largest-Triangle-Three-Buckets (LTTB)** algorithm to decimate massive series to display pixel resolution on worker threads.

---

## Domain 3: Packaging, Native Distribution & Auto-Updates

### 3.1 OS Packaging Matrix
| Platform | Target Package | Installer & Bundle Pipeline |
| :--- | :--- | :--- |
| **Windows** | `.msix` & `.msi` | WiX v5 toolset with per-user MSI installation (no UAC escalation) and MSIX sparse bundles for Windows Store. |
| **macOS** | `.dmg` & `.app` | Universal binary (`x86_64` + `arm64`), hardened runtime, code signing with Apple Developer ID, and automated `notarytool` stapling. |
| **Linux** | AppImage & Flatpak | Standalone `.AppImage` (glibc 2.31 compatibility base) and Flathub-ready Flatpak manifests with Wayland portal sandboxing. |

### 3.2 Cryptographically Secure Delta Updates
- **Update Architecture**: Background update polling via HTTPS; delta binary patching using **bsdiff** or **zstd chunked dictionaries**.
- **Cryptographic Verification**: Update metadata and binaries are verified using **Ed25519 signatures** before disk staging.
- **Atomic Swap**: Staged updates are swapped atomically on process restart via transactional filesystem renames (`MoveFileExW(MOVEFILE_DELAY_UNTIL_REBOOT)` on Windows, atomic directory symlink swapping on macOS/Linux).

---

## Domain 4: Typography, Shaping & Multilingual I18n

### 4.1 Shaping Engine Architecture
Martensite employs **Swash** and **HarfBuzz** for complex script shaping, adhering to:
- **Unicode BiDi (UAX #9)**: Automatic directionality detection (LTR / RTL), directional run splitting, and mirrored glyph substitution for Arabic, Hebrew, and Persian.
- **Unicode Line Breaking (UAX #14)**: Context-sensitive line breaking rules for CJK scripts (Kinsoku Shori).
- **Vertical Text Layout (UAX #50)**: Native vertical text rendering (`writing-mode: vertical-rl`) using OpenType `vert` and `vrt2` features for East Asian scripts.

### 4.2 System Font Fallback Cascades
To avoid missing glyph placeholders (`☐`):
- **Windows**: Query DirectWrite font fallback list (`IDWriteFontFallback`), resolving missing glyphs through Meiryo, Yu Gothic, Segoe UI Emoji, and Nirmala UI.
- **macOS**: CoreText cascade resolution via `CTFontCreateForString`.
- **Linux**: Fontconfig XML cascade query matching character code points (`FcFontSort`).

### 4.3 Four-Tier Typography Cache Architecture
```
[Text Request]
      │
      ▼
[Tier 1: Inline Flexbox Cache (ColdNode)] ──(Hit: 0ns)──> Return Width/Height
      │ (Miss)
      ▼
[Tier 2: Global LRU TextShapeCache (16MB)] ──(Hit: <50ns)─> Return Shaped Glyph Runs
      │ (Miss)
      ▼
[Tier 3: Complex Shaper (Swash/HarfBuzz)] ──(Exec: ~2-15μs)─> Emit Glyphs & Advance
      │
      ▼
[Tier 4: Dynamic GPU Glyph Texture Atlas] ──(Upload to VRAM)─> Render Quad
```

---

## Domain 5: OS Windowing, Multi-Display & Shell Integration

### 5.1 Modern Backdrop Materials & Client-Side Decoration (CSD)
- **Windows 11**: Direct invocation of `DwmSetWindowAttribute`:
  - `DWMWA_SYSTEMBACKDROP_TYPE = 2` (Mica), `3` (Acrylic), `4` (Mica Alt).
  - Custom caption rendering via `WM_NCCALCSIZE` and native Snap Layouts menu integration via `WM_NCHITTEST` returning `HTMAXBUTTON`.
- **macOS**: AppKit window backing with `NSVisualEffectView`, `NSVisualEffectMaterialHeaderView`, and full-size content view (`NSWindowStyleMaskFullSizeContentView`).
- **Linux / Wayland**: Client-Side Decoration via `libdecor`, supporting `wp_fractional_scale_v1` for crisp fractional DPI scaling without bilinear blurring, and `xdg_wm_dialog_v1` for modal window attachment.

### 5.2 Shell & System Tray Integration
- **System Tray**:
  - Windows: `Shell_NotifyIconW` with `NOTIFYICON_VERSION_4` and taskbar jump lists (`ICustomDestinationList`).
  - macOS: `NSStatusItem` in the system menu bar.
  - Linux: FreeDesktop `StatusNotifierItem` (SNI) over D-Bus with `com.canonical.dbusmenu`.
- **Taskbar Progress & Badges**:
  - Windows: `ITaskbarList3::SetProgressValue` and `SetOverlayIcon`.
  - macOS: `[NSApp dockTile]` with dynamic badges.

---

## Domain 6: Modern Input Systems, Precision Physics & Gestures

### 6.1 Touchpad Kinematics & Apple 0.55 Rubber-Banding
- **Inertial Momentum**: Physical fling kinematics modeled with exponential decay:
  $$v(t) = v_0 \cdot e^{-\lambda t}$$
- **Boundary Stretch & Rubber-Banding**: When a scrollable container reaches its boundary, viewport offset follows the Apple UIScrollView non-linear damping formulation:
  $$d_{\text{clamped}} = \frac{x \cdot d \cdot c}{d + c \cdot x}$$
  where $d$ is the viewport dimension, $x$ is the overscroll distance, and $c = 0.55$ is the empirical elasticity constant.

### 6.2 Stylus, Pen & Spatial Navigation
- **6-DoF Stylus Tracking**: Pressure sensitivity, tilt angle (azimuth and altitude), rotation, and barrel button events.
- **Latency Prediction**: Pen stroke latency compensation using **1D/2D Kalman Filters** to extrapolate the pen tip position forward by one display frame interval ($\Delta t = 16.6\text{ms}$).
- **W3C Spatial Navigation**: 2D directional arrow/gamepad navigation scoring:
  $$\text{Score}(u, v) = w_{\text{dist}} \cdot d(u, v) + w_{\text{angle}} \cdot \theta(u, v)$$
  providing structured keyboard-only and controller accessibility.

### 6.3 Velocity-Damped IME Cursor Tracking
When typing in a scrolling container, the IME candidate window tracks the text caret smoothly. Martensite applies a damped projection formula:
$$P_{\text{ime}} = P_{\text{caret}} + v_{\text{viewport}} \cdot \Delta t \cdot e^{-\lambda \Delta t}$$
preventing visual candidate window separation during kinetic scroll events.

---

## Domain 7: GPU Vector Rasterization, Compute Shaders & HDR

### 7.1 Compute-Centric Vector Graphics (Vello)
Martensite leverages **Vello** for GPU-accelerated 2D vector graphics:
1. **Path Flattening**: GPU compute pass flattening cubic Bézier curves using **Wang's formula**:
   $$n = \left\lceil \sqrt{\frac{3}{4} \cdot \frac{\max(|p_0 - 2p_1 + p_2|, |p_1 - 2p_2 + p_3|)}{\text{tol}}} \right\rceil$$
2. **Sort-Middle Coarse Rasterization**: Assigning path segments into $16 \times 16$ pixel tiles on the GPU.
3. **Fine Rasterization**: Sub-pixel accurate area calculation executed entirely in compute shaders.

### 7.2 Closed-Form Analytical Drop Shadows
Rather than multi-tap texture convolutions that consume significant bandwidth, Martensite implements **Evan Wallace's analytical closed-form erf shadow shader** for rounded rectangles:
$$I(x, y) = \frac{1}{4} \left[ \text{erf}\left(\frac{x - x_0}{\sigma \sqrt{2}}\right) - \text{erf}\left(\frac{x - x_1}{\sigma \sqrt{2}}\right) \right] \left[ \text{erf}\left(\frac{y - y_0}{\sigma \sqrt{2}}\right) - \text{erf}\left(\frac{y - y_1}{\sigma \sqrt{2}}\right) \right]$$
evaluating drop shadows and inner shadows in a single compute pass with mathematical precision.

### 7.3 High Dynamic Range (HDR) Color Architecture
- **Swapchain Formats**: Negotiating `wgpu::TextureFormat::Rgba16Float` (scRGB on Windows, Extended Linear sRGB on macOS) or `Bgra8UnormSrgb`.
- **EOTF Transformations**: Built-in WGSL shaders for SMPTE ST 2084 (PQ) and BT.709/BT.2020 matrix conversions.
- **Robust Device Loss FSM**: Five-state finite state machine (`Active` $\to$ `Suspended` $\to$ `Recreating` $\to$ `Restoring` $\to$ `Failed`) ensuring structured recovery when GPUs are disconnected, displays sleep, or drivers reset.

---

## Domain 8: Developer Experience (DX), Tooling & Ergonomics

### 8.1 Sub-350ms Dynamic Hot Reloading (`cdylib` Splitting)
- **Host Binary**: Owns OS window, event loop, GPU context, `WidgetArena`, and reactive `SignalStore`.
- **Guest Library**: Compiled as a `cdylib` containing UI view builders and callbacks.
- **Turnaround Latency**: Using Mold/LLD with Cranelift/opt-level=0, recompilation and dynamic library hot-swap execute in $< 350\text{ ms}$, preserving live application state and input signals.

### 8.2 In-App Observability HUD & Tracy Profiling
- **F12 Diagnostic HUD**: A zero-allocation diagnostic overlay rendering:
  - 120-frame rolling execution timing histogram (layout time, paint time, GPU wait time).
  - Real-time dirty rect flash visualizer tracking partial repaint regions.
  - Generational arena slot utilization and compaction telemetry.
- **Tracy Profiler Hooks**: Sub-50ns tracing spans across all frame stages with native GPU timestamp query integration.

### 8.3 Ergonomic API Design
1. **Copyable Handles**: `WidgetId: Copy` and `Signal<T>: Copy`. Closures capture handles with `move |_| count.update(...)` without `.clone()` boilerplate or borrow-checker collisions.
2. **Orthogonal Vocabulary**: Universal property builders across all components (`.padding()`, `.gap()`, `.bg()`, `.on_click()`).
3. **Permissive `IntoProp<T>`**: Builder methods accept string slices, `String`, or `Signal<String>` transparently.
4. **Machine-Readable Guides**: A standardized `llms.txt` file at the repository root describing architectural invariants and canonical patterns.

---

## Domain 9: State Architecture, Push-Pull Reactivity & Concurrency

### 9.1 Glitch-Free Push-Pull Topological DAG
- **Phase 1 (Push)**: Zero-allocation BFS dirty bit propagation marking dependent memos and effects as dirty.
- **Phase 2 (Pull)**: Strictly ordered evaluation driven by **Dial's Bucket Queue** indexed by topological depth rank $\lambda(v)$:
  $$\lambda(v) = \max_{u \in \text{deps}(v)} \lambda(u) + 1$$
  preventing diamond dependency glitches.
- **Dynamic Dependency Pruning**: Uses monotonic evaluation epoch counters (`eval_epoch: u32`) to drop obsolete subscriptions in $O(1)$ without allocations.
- **Cycle Circuit Breaker**: 3-color DFS cycle detection isolating and poisoning rogue circular feedback loops before they block thread execution.

### 9.2 Non-Linear Branching History Ledger (LCA)
- Replaces naive undo/redo linear stacks with a **directed history tree** (`HistoryTree`).
- Navigates between arbitrary history points via the **Lowest Common Ancestor (LCA)** algorithm, calculating minimal revert/apply paths:
  $$\Delta(S \to T) = \sum_{n = S}^{L.child} \text{revert}(n) + \sum_{n = L.child}^{T} \text{apply}(n)$$
- LRU leaf pruning protects active branch paths while enforcing bounded memory budgets.

### 9.3 100,000 msgs/sec High-Throughput Concurrency
- **The Streaming Ingestion Challenge**: Direct `signal.set()` on 100k events/sec causes high lock contentions and DAG traversals, starving the UI thread.
- **Triple-Buffered Conflation**: An atomic triple-buffer decouples producer and consumer. Incoming events arriving during a single 16.6ms frame are conflated into **one single reactive evaluation** on the VSync boundary.
- **Bounded Lossless Draining**: High-speed telemetry feeds are drained in fixed-size batches from SPSC lock-free ring buffers into virtualized data grids.

---

## Domain 10: External Engine Embedding, Zero-Copy Media & Cloud Streaming

### 10.1 3D Engine Viewport Embedding (Bevy & Godot 4)

> **Amendment (v0.14.0 re-scope, ADR-0033):** The original direction below
> describes Martensite rendering *into* the host engine's render graph
> (guest mode). The confirmed architecture is the inverse — **host mode**:
> Martensite owns the window, event loop, wgpu device, and compositing;
> external engines produce textures Martensite consumes via
> `martensite-engine-bridge` (v0.14.0). For Bevy this is
> `RenderCreation::Manual` device injection + `RenderTarget::TextureView`.
> For Godot, deep research (v0.15.0 spec) established that Godot's render
> targets are not exportable and GDExtension exposes no fence/semaphore
> APIs — the shipped path is `texture_get_data_async` readback; true
> zero-copy requires upstream engine patches.

- **Bevy Engine**: Integrates into Bevy’s Render World as a custom `RenderGraph` node executing after `Core3dSystems::MainPass`, rendering directly to the camera's `ViewTarget` color attachment with zero blit copies. *(Superseded by host-mode embed — see amendment.)*
- **Godot 4**: GDExtension integration registering native Vulkan `VkImage` or Direct3D 12 texture handles with Godot’s `RenderingDevice` via `texture_create_from_extension`, or directly injecting passes via `CompositorEffect`. *(Superseded — see amendment; `texture_create_from_extension` remains the mechanism for the one-copy experimental path.)*
- **Diegetic World-Space UI**: Offscreen GUI textures mapped onto 3D in-game meshes, using camera raycasting to map 3D intersection coordinates into 2D UI input events.

### 10.2 Zero-Copy Hardware Video Playback
- **Direct GPU Memory Sharing**: Hardware video decoders (DXVA, VideoToolbox, VA-API) share memory handles directly with the GUI renderer:
  - Windows: Direct3D 11/12 DXGI Shared NT Handles (`IDXGIResource1::CreateSharedHandle`).
  - macOS: CoreVideo `CVPixelBuffer` backed by `IOSurfaceRef`.
  - Linux: DRM `dma-buf` file descriptors imported into Vulkan memory.
- **Performance**: 4K 60fps 10-bit HDR video runs inside the widget tree with $< 1\%$ CPU utilization.

### 10.3 Headless CI Verification & Cloud Pixel Streaming
- **Deterministic CI Testing**: Driven by `VirtualClock` and `HeadlessHarness` using the perceptual **DSSIM** metric, achieving reproducible test runs with zero timing jitter.
- **Cloud Pixel Streaming**: Headless GUI framebuffers registered directly with hardware encoders (NVENC) via `NvEncRegisterResource`. Zero-copy encoded H.264/AV1 NAL units stream over WebRTC RTP/SRTP with $< 35\text{ ms}$ glass-to-glass latency, supported by client-side local hardware cursor tracking.

---

## Strategic Implementation Roadmap

```
                                      MARTENSITE ROADMAP MILESTONES
                                      
  [v0.10.0] -> VERIFIED BASELINE (Core Engine, Vello Compute, Taffy Layout, Reactive DAG, LCA History)
      │
      ├─► [v0.11.0] Typography & Accessibility Expansion
      │   ├── Full AccessKit integration across all base widgets (UIA, NSAccessibility, AT-SPI2).
      │   ├── Swash/HarfBuzz BiDi (UAX #9) & vertical-rl (UAX #50) layout pipelines.
      │   └── System font fallback cascades for Windows, macOS, and Linux.
      │
      ├─► [v0.12.0] Blessed Widgets & Kinematics
      │   ├── 1,000,000-Row virtualized DataGrid with multi-column sorting and filtering.
      │   ├── BSP Docking Tree with floating window multi-swapchains.
      │   └── Apple 0.55 rubber-banding and 6-DoF Kalman stylus prediction.
      │
      ├─► [v0.13.0] Modern Shell & Platform Integration
      │   ├── Windows 11 Mica / Mica Alt / Acrylic & Snap Layouts integration.
      │   ├── macOS Liquid Glass / NSVisualEffectView vibrancy.
      │   └── Wayland wp_fractional_scale_v1 & StatusNotifierItem shell menus.
      │
      ├─► [v0.14.0] External Surface Foundation
      │   ├── Generic external-texture widget + martensite-engine-bridge protocol.
      │   ├── Same-device zero-copy composite (direct TextureView sampling).
      │   └── Damage-driven redraw + CPU fallback contract.
      │       (DXGI/IOSurface/dma-buf import & BT.2408 scaling shipped in v0.8.0.)
      │
      ├─► [v0.15.0] Engine Showcase
      │   ├── Bevy host-mode viewport (shared wgpu device, RenderTarget::TextureView).
      │   └── Godot 4 GDExtension (async readback; zero-copy needs upstream patches).
      │
      ├─► [v0.16.0] Hardware Media Pipeline
      │   ├── Platform decoders: VideoToolbox / Media Foundation / VAAPI.
      │   ├── Multi-plane NV12/P010 import fix + HDR metadata flow.
      │   └── 4K 120fps gate: <0.1% drops, <1% CPU dispatch.
      │
      ├─► [v0.17.0] Platform Expansion
      │   ├── Widget breadth: slider, radio, dropdown, scrollview, tabs, tooltip.
      │   ├── Web (wasm32/WebGPU), iOS (Metal), Android (GameActivity/Vulkan).
      │   └── Hybrid command-ledger + snapshot time-travel debugger.
      │
      └─► [v1.0.0] Production Stability & Distribution Release
          ├── Enterprise packaging: WiX v5 MSI/MSIX, notarized DMG, AppImage/Flatpak.
          ├── Cryptographic Ed25519 delta auto-updates.
          ├── Cloud rendering WebRTC / NVENC headless streaming runtime.
          └── Comprehensive benchmark verification across performance, memory, and responsiveness.
```

---

## Conclusion

By combining low-level systems engineering (compute shaders, lock-free ring buffers, platform memory handles) with high-level developer ergonomics (copyable signals, sub-350ms hot reload, consistent builder APIs), Martensite establishes a predictable, high-performance foundation. It fulfills the functional and non-functional requirements demanded by enterprise software, creative tooling, and desktop environments.
