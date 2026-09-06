# Detailed Design Record: DDR-0011
## Title: `martensite-window` Multi-Window Event Loop Architecture

### 1. Architectural Role & Invariants
`martensite-window` oversees the OS event loop (`winit`), window surfaces, VSync strategies, and hardware device initialization.
* **Invariant 1.1**: The framework must share a single `wgpu::Instance`, `wgpu::Adapter`, and `wgpu::Device` across all open windows.
* **Invariant 1.2**: Each OS window maintains its own independent `wgpu::Surface` and swapchain.
* **Invariant 1.3**: The event loop unconditionally returns to `ControlFlow::Wait` when all windows lack dirty regions and physics are quenched. 0.00% CPU/GPU idle is mandatory.

### 2. Window Lifecycle State Machine
```mermaid
stateDiagram-v2
    [*] --> Suspended: App Launched
    Suspended --> Resumed: Event::Resumed (Create Surfaces)
    Resumed --> Active: Surfaces Configured
    Active --> Suspended: Event::Suspended (Destroy Surfaces)
    Active --> Destroyed: Window Closed
    Suspended --> Destroyed: App Terminated
```

### 3. Core Data Structures & Memory Layout
```rust
use wgpu::{Device, Queue, Instance, Surface, SurfaceConfiguration};
use winit::window::WindowId;
use std::collections::HashMap;

/// Shared global GPU context.
pub struct GpuContext {
    pub instance: Instance,
    pub device: Device,
    pub queue: Queue,
}

/// Per-window rendering state.
pub struct WindowState {
    pub surface: Surface<'static>,
    pub config: SurfaceConfiguration,
    pub scale_factor: f64,
    pub physical_size: (u32, u32),
}

/// The multi-window orchestrator.
pub struct WindowManager {
    pub gpu: GpuContext,
    pub windows: HashMap<WindowId, WindowState>,
}
```

### 4. VSync & Presentation Strategy

Default on all platforms is `PresentMode::Fifo` (compositor-controlled VSync). This satisfies Law III: 0.00% GPU usage at idle.

**`PresentMode::Immediate` opt-in (Sovereign Architect decision, 2026-09-06):**  
Applications with latency-critical rendering requirements (audio workstations, financial UIs, game-adjacent tools) may opt in to `Immediate` presentation via the `App::build()` API:

```rust
App::build()
    .present_mode(PresentMode::Immediate) // developer owns the power regression
    .run(|cx| { ... })
```

This is an **explicit contract**: the caller acknowledges that `Immediate` mode may cause:
- Tearing artifacts on Wayland compositors that do not support it
- GPU spin at rates exceeding display refresh (violates Law III — intentional override)
- Increased power draw on battery-powered devices

`PresentMode::Immediate` is **never the framework default** and is never set implicitly. If the compositor rejects it, `wgpu` falls back to `Fifo` automatically; no panic.

**Platform notes:**
- **macOS (Metal)**: No tearing; `Immediate` maps to `CAMetalLayer.displaySyncEnabled = false`.
- **Windows (DX12)**: Tearing possible on non-G-Sync displays; user accepts this.
- **Linux (Wayland)**: Compositor-dependent. Many reject `Immediate`; graceful `Fifo` fallback applies.
- **Linux (X11)**: `Immediate` supported; tearing on non-TearFree displays.

**Error Condition — Surface Lost**: `SurfaceError::Lost` or `SurfaceError::Outdated` → reconfigure surface before next frame (see DDR-0003 for full resurrection protocol).  
**Error Condition — Out of Memory**: `SurfaceError::OutOfMemory` → fatal panic; environment is irrecoverable.

### 5. DPI Handling
Per-monitor DPI changes emit `WindowEvent::ScaleFactorChanged`. 
Algorithm:
1. Re-calculate logical size based on new `scale_factor`.
2. Resize `wgpu::SurfaceConfiguration` to the new physical size.
3. Mark root `NodeFlags::DIRTY_LAYOUT` in the widget arena.
4. Issue a synchronous re-layout and re-paint.
