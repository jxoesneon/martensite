# Detailed Design Record: DDR-0003
## Title: `martensite-wgpu` Device Resurrection & Single-Frame GPU Healing

### 1. Architectural Role & Invariants
`martensite-wgpu` encapsulates all GPU compute pipelines, swapchains, shader validation, and hardware interaction. It treats the GPU as an ephemeral, disposable rasterization cache.
* **Invariant 1.1**: A GPU driver reset (TDR), laptop sleep wake cycle, or eGPU physical disconnection must **never crash the application**.
* **Invariant 1.2**: Swapchain surface reconstitution and VRAM asset re-upload must finalize within a **single frame window ($<16\text{ms}$)** without dropping unsaved UI state.

---

### 2. The Single-Frame GPU Resurrection State Machine

```
   ┌──────────────────────────────────────────────────────────┐
   │                    [HEALTHY_STEADY]                      │
   │  Normal rendering; Vello compute dispatches to swapchain │
   └────────────────────────────┬─────────────────────────────┘
                                │ wgpu::SurfaceError::Lost
                                │ wgpu::SurfaceError::Outdated
                                │ Device::poll(PollError::Dead)
                                ▼
   ┌──────────────────────────────────────────────────────────┐
   │                 [DEVICE_LOSS_DETECTED]                   │
   │  Intercept error; preserve CPU arena & reactive signals  │
   └────────────────────────────┬─────────────────────────────┘
                                │ Halt GPU command submission
                                ▼
   ┌──────────────────────────────────────────────────────────┐
   │                  [HAL_RECONSTITUTION]                    │
   │  Re-request Adapter; create Device & Queue; rebind Surface│
   └────────────────────────────┬─────────────────────────────┘
                                │ Rebuild pipelines & binds
                                ▼
   ┌──────────────────────────────────────────────────────────┐
   │                  [VRAM_REHYDRATION]                      │
   │  Re-upload font atlas, texture cache & Oklab uniforms    │
   └────────────────────────────┬─────────────────────────────┘
                                │ Request immediate redraw
                                ▼
   ┌──────────────────────────────────────────────────────────┐
   │                   [RESURRECTED_PAINT]                    │
   │  Re-encode PaintList; present frame (<16ms total elapsed)│
   └──────────────────────────────────────────────────────────┘
```

---

### 3. Surface Error Interception Pipeline

```rust
pub fn render_frame(
    &mut self,
    arena: &WidgetArena,
    window: &winit::window::Window,
) -> Result<(), Box<dyn std::error::Error>> {
    let surface_texture = match self.surface.get_current_texture() {
        Ok(texture) => texture,
        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
            tracing::warn!("Swapchain lost or outdated. Reconfiguring surface...");
            self.surface.configure(&self.device, &self.config);
            self.surface.get_current_texture()?
        }
        Err(wgpu::SurfaceError::Timeout) => {
            tracing::warn!("GPU surface acquire timed out. Skipping frame.");
            return Ok(());
        }
        Err(wgpu::SurfaceError::OutOfMemory) => {
            tracing::error!("VRAM Out of Memory! Triggering emergency purge...");
            self.purge_caches();
            return Err("GPU VRAM Out Of Memory".into());
        }
    };

    // Encode frame commands...
    Ok(())
}
```
