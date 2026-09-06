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
- **macOS/Windows**: Default to `PresentMode::Fifo` (VSync). For latency-critical apps, allow fallback to `Mailbox`.
- **Linux (Wayland/X11)**: `PresentMode::Fifo`. `Immediate` is disabled by default to prevent tearing.
- **Error Condition - Surface Lost**: If `SurfaceError::Lost` or `SurfaceError::Outdated` is returned during `surface.get_current_texture()`, the swapchain must be immediately reconfigured via `surface.configure` before the next frame.
- **Error Condition - Out of Memory**: If `SurfaceError::OutOfMemory` is returned, a critical panic is executed as the process environment is irrecoverable.

### 5. DPI Handling
Per-monitor DPI changes emit `WindowEvent::ScaleFactorChanged`. 
Algorithm:
1. Re-calculate logical size based on new `scale_factor`.
2. Resize `wgpu::SurfaceConfiguration` to the new physical size.
3. Mark root `NodeFlags::DIRTY_LAYOUT` in the widget arena.
4. Issue a synchronous re-layout and re-paint.
