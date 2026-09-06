//! WGPU compute rasterization and GPU resurrection engine.

/// GPU renderer encapsulating the core `wgpu` resources required for
/// compute-based rasterization and GPU resurrection workflows.
pub struct GpuRenderer {
    /// The `wgpu` instance used to enumerate and create adapters and surfaces.
    pub instance: wgpu::Instance,
    /// The physical GPU adapter selected for high-performance compute work.
    pub adapter: wgpu::Adapter,
    /// The logical device used to allocate resources and submit commands.
    pub device: wgpu::Device,
    /// The command queue used to submit work to the GPU.
    pub queue: wgpu::Queue,
}

impl GpuRenderer {
    /// Creates a new [`GpuRenderer`] by requesting a high-performance adapter
    /// and its associated device and queue.
    ///
    /// # Errors
    ///
    /// Returns an error if no suitable adapter or device could be acquired.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: true,
        }))?;

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::GpuRenderer;

    #[test]
    #[ignore = "requires a wgpu backend feature enabled and a GPU available"]
    fn new_does_not_panic() {
        // A GPU may not be available in CI; we only verify that new() does not panic
        // when a backend is actually available.
        let _ = GpuRenderer::new();
    }

    #[test]
    #[ignore = "requires a wgpu backend feature enabled and a GPU available"]
    fn new_handles_missing_gpu_gracefully() {
        // The error path must be handled gracefully when no GPU is available.
        match GpuRenderer::new() {
            Ok(renderer) => {
                // When construction succeeds, the core wgpu resources must be present.
                let _ = &renderer.instance;
                let _ = &renderer.adapter;
                let _ = &renderer.device;
                let _ = &renderer.queue;
            }
            Err(_) => {
                // No suitable adapter/device available (e.g. headless CI).
            }
        }
    }
}
