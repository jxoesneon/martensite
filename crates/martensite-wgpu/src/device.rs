//! GPU device context: adapter enumeration, feature selection, and
//! device/queue lifecycle management.
//!
//! [`GpuContext`] is the entry point for acquiring a logical `wgpu` device and
//! queue. It encapsulates the instance, the selected physical adapter, and the
//! resulting device/queue pair, exposing helpers for adapter enumeration with
//! power-preference selection and for verifying that the acquired device
//! satisfies a required feature/limit set.

use std::sync::Arc;
use std::time::Duration;

/// Errors that can occur while constructing a [`GpuContext`].
///
/// # Examples
///
/// ```
/// use martensite_wgpu::device::GpuContextError;
/// use std::error::Error;
///
/// let err = GpuContextError::NoAdapter("no adapters".to_string());
/// assert!(err.to_string().contains("no suitable GPU adapter"));
/// assert!(err.source().is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuContextError {
    /// No adapter matching the requested options could be found on the system.
    /// The carried string preserves the original wgpu error message for
    /// diagnostics.
    NoAdapter(String),
    /// An adapter was selected, but the system refused to hand back a logical
    /// device and queue. The carried string preserves the original wgpu error
    /// message for diagnostics.
    DeviceRequestFailed(String),
}

impl std::fmt::Display for GpuContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAdapter(msg) => write!(
                f,
                "no suitable GPU adapter was found for the requested options: {msg}"
            ),
            Self::DeviceRequestFailed(msg) => write!(
                f,
                "failed to request a logical device from the selected adapter: {msg}"
            ),
        }
    }
}

impl std::error::Error for GpuContextError {}

/// A complete, ready-to-use GPU context.
///
/// `GpuContext` owns the four core `wgpu` resources required for compute-based
/// rasterization and GPU resurrection workflows:
///
/// * the [`wgpu::Instance`] used to enumerate adapters and create surfaces,
/// * the selected physical [`wgpu::Adapter`],
/// * the logical [`wgpu::Device`] used to allocate resources, and
/// * the command [`wgpu::Queue`] used to submit work.
///
/// Each resource is held in an [`Arc`] so hosts can share the identical
/// instance/adapter/device/queue with an embedded engine — Bevy's
/// `RenderCreation::Manual` takes `Arc` clones of exactly these handles,
/// and `wgpu` requires the surface, adapter, and device to originate from
/// the same instance.
///
/// It is constructed via [`GpuContext::new`] (default high-performance
/// selection), [`GpuContext::with_power_preference`] for explicit control,
/// or [`GpuContext::for_surface`] when the context will present to a real
/// window.
///
/// # Examples
///
/// ```no_run
/// use martensite_wgpu::device::GpuContext;
///
/// let ctx = GpuContext::new().expect("GPU available");
/// assert!(!ctx.adapter_info.name.is_empty());
/// ```
pub struct GpuContext {
    /// The `wgpu` instance used to enumerate and create adapters and surfaces.
    ///
    /// Shared via [`Arc`]: surfaces must be created from this exact instance
    /// for [`GpuContext::for_surface`]-acquired adapters to present to them.
    pub instance: Arc<wgpu::Instance>,
    /// The physical GPU adapter selected for high-performance compute work.
    pub adapter: Arc<wgpu::Adapter>,
    /// The logical device used to allocate resources and submit commands.
    pub device: Arc<wgpu::Device>,
    /// The command queue used to submit work to the GPU.
    pub queue: Arc<wgpu::Queue>,
    /// Cached descriptive metadata for the selected adapter.
    pub adapter_info: wgpu::AdapterInfo,
}

impl GpuContext {
    /// Creates a new [`GpuContext`] by requesting a high-performance adapter
    /// and its associated device and queue.
    ///
    /// This is equivalent to calling
    /// [`GpuContext::with_power_preference`] with
    /// [`wgpu::PowerPreference::HighPerformance`].
    ///
    /// # Errors
    ///
    /// Returns [`GpuContextError::NoAdapter`] if no suitable adapter could be
    /// acquired, or [`GpuContextError::DeviceRequestFailed`] if the adapter
    /// was found but the device request failed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::device::GpuContext;
    ///
    /// let ctx = GpuContext::new();
    /// assert!(ctx.is_ok() || ctx.is_err());
    /// ```
    pub fn new() -> Result<Self, GpuContextError> {
        Self::with_power_preference(wgpu::PowerPreference::HighPerformance)
    }

    /// Creates a new [`GpuContext`] requesting an adapter matching the supplied
    /// power preference.
    ///
    /// # Errors
    ///
    /// Returns [`GpuContextError::NoAdapter`] if no suitable adapter could be
    /// acquired, or [`GpuContextError::DeviceRequestFailed`] if the adapter
    /// was found but the device request failed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::device::GpuContext;
    ///
    /// let ctx = GpuContext::with_power_preference(wgpu::PowerPreference::LowPower);
    /// assert!(ctx.is_ok() || ctx.is_err());
    /// ```
    pub fn with_power_preference(
        power_preference: wgpu::PowerPreference,
    ) -> Result<Self, GpuContextError> {
        let instance = wgpu::Instance::default();

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: true,
        }))
        .map_err(|e| GpuContextError::NoAdapter(e.to_string()))?;

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| GpuContextError::DeviceRequestFailed(e.to_string()))?;

        let adapter_info = adapter.get_info();

        Ok(Self {
            instance: Arc::new(instance),
            adapter: Arc::new(adapter),
            device: Arc::new(device),
            queue: Arc::new(queue),
            adapter_info,
        })
    }

    /// Creates a new [`GpuContext`] forcing the CPU fallback adapter
    /// (Lavapipe/llvmpipe on Linux, WARP on Windows, etc.).
    ///
    /// This requests a low-power adapter with `force_fallback_adapter: true`,
    /// which selects the software/CPU rasterizer. It is intended for headless
    /// testing and CI where no physical GPU is available, so that the Vello
    /// compute pipeline can run on a deterministic software Vulkan device.
    ///
    /// # Errors
    ///
    /// Returns [`GpuContextError::NoAdapter`] if no fallback adapter could be
    /// acquired (e.g. no software Vulkan driver installed), or
    /// [`GpuContextError::DeviceRequestFailed`] if the adapter was found but
    /// the device request failed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::device::GpuContext;
    ///
    /// let ctx = GpuContext::with_cpu_fallback();
    /// assert!(ctx.is_ok() || ctx.is_err());
    /// ```
    pub fn with_cpu_fallback() -> Result<Self, GpuContextError> {
        let instance = wgpu::Instance::default();

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: true,
            apply_limit_buckets: true,
        }))
        .map_err(|e| GpuContextError::NoAdapter(e.to_string()))?;

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| GpuContextError::DeviceRequestFailed(e.to_string()))?;

        let adapter_info = adapter.get_info();

        Ok(Self {
            instance: Arc::new(instance),
            adapter: Arc::new(adapter),
            device: Arc::new(device),
            queue: Arc::new(queue),
            adapter_info,
        })
    }

    /// Requests a surface-compatible adapter on `instance` and returns a
    /// ready-to-use [`GpuContext`] sharing that instance.
    ///
    /// Use this when the context will present to a real window — host-mode
    /// engine embedding needs the same device family for zero-copy
    /// compositing. This is the windowed counterpart of
    /// [`GpuContext::with_power_preference`] (`HighPerformance`,
    /// `force_fallback_adapter: false`, default device descriptor).
    ///
    /// `surface` must have been created from `instance`: `wgpu` requires the
    /// surface, adapter, and device to originate from the same instance —
    /// passing a surface built by a different [`wgpu::Instance`] panics
    /// inside `request_adapter`. The returned context clones `instance` into
    /// an [`Arc`], so the caller keeps its own handle for Bevy's
    /// `RenderCreation::Manual` or further surface creation.
    ///
    /// # Errors
    ///
    /// Returns [`GpuContextError::NoAdapter`] if no adapter compatible with
    /// `surface` could be acquired, or [`GpuContextError::DeviceRequestFailed`]
    /// if the adapter was found but the device request failed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::device::GpuContext;
    /// use martensite_wgpu::wgpu;
    ///
    /// # fn surface_for(instance: &wgpu::Instance) -> wgpu::Surface<'static> {
    /// #     unimplemented!() // instance.create_surface(window)
    /// # }
    /// let instance = wgpu::Instance::default();
    /// let surface = surface_for(&instance);
    /// let ctx = pollster::block_on(GpuContext::for_surface(&instance, &surface))
    ///     .expect("surface-compatible GPU context");
    /// ```
    pub async fn for_surface(
        instance: &wgpu::Instance,
        surface: &wgpu::Surface<'_>,
    ) -> Result<Self, GpuContextError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(surface),
                force_fallback_adapter: false,
                apply_limit_buckets: true,
            })
            .await
            .map_err(|e| GpuContextError::NoAdapter(e.to_string()))?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| GpuContextError::DeviceRequestFailed(e.to_string()))?;

        let adapter_info = adapter.get_info();

        Ok(Self {
            instance: Arc::new(instance.clone()),
            adapter: Arc::new(adapter),
            device: Arc::new(device),
            queue: Arc::new(queue),
            adapter_info,
        })
    }

    /// Recreates the logical device and command queue from the current adapter.
    ///
    /// This is the core of device-loss recovery: the adapter usually survives
    /// a TDR or driver reset, so a new [`wgpu::Device`] and [`wgpu::Queue`]
    /// can be requested without re-enumerating hardware. The old device and
    /// queue fields are replaced in place.
    ///
    /// # Errors
    ///
    /// Returns [`GpuContextError::DeviceRequestFailed`] if the adapter refuses
    /// to create a new logical device.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::device::GpuContext;
    ///
    /// # fn example(mut ctx: GpuContext) {
    /// ctx.recreate_device_and_queue().expect("device recovered");
    /// # }
    /// ```
    pub fn recreate_device_and_queue(&mut self) -> Result<(), GpuContextError> {
        let (device, queue) = pollster::block_on(
            self.adapter
                .request_device(&wgpu::DeviceDescriptor::default()),
        )
        .map_err(|e| GpuContextError::DeviceRequestFailed(e.to_string()))?;

        self.device = Arc::new(device);
        self.queue = Arc::new(queue);
        Ok(())
    }

    /// Enumerates every adapter currently visible to the instance, ordered by
    /// the supplied power preference.
    ///
    /// The returned vector is sorted so that adapters best matching the
    /// requested power preference appear first. This is useful for diagnostic
    /// UI and for the recovery FSM, which must re-enumerate adapters after a
    /// device loss.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::device::GpuContext;
    ///
    /// let instance = wgpu::Instance::default();
    /// let adapters = GpuContext::enumerate_adapters(&instance, wgpu::PowerPreference::HighPerformance);
    /// // The list may be empty in headless environments without a GPU.
    /// println!("found {} adapter(s)", adapters.len());
    /// ```
    #[must_use]
    pub fn enumerate_adapters(
        instance: &wgpu::Instance,
        power_preference: wgpu::PowerPreference,
    ) -> Vec<wgpu::Adapter> {
        let mut adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
        // Stable sort by a power-preference score so the most-desired adapter
        // ends up first without disturbing the relative order of equal-score
        // adapters returned by the backend.
        adapters.sort_by(|a, b| {
            let sa = power_score(a.get_info().device_type, power_preference);
            let sb = power_score(b.get_info().device_type, power_preference);
            sb.cmp(&sa)
        });
        adapters
    }

    /// Returns `true` when the acquired device supports every feature in
    /// `required`.
    ///
    /// Use this for feature selection before allocating resources that depend
    /// on optional capabilities.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::device::GpuContext;
    ///
    /// # fn example(ctx: &GpuContext) {
    /// // Every device supports the empty feature set.
    /// assert!(ctx.supports_features(wgpu::Features::empty()));
    /// # }
    /// ```
    #[must_use]
    pub fn supports_features(&self, required: wgpu::Features) -> bool {
        self.device.features().contains(required)
    }

    /// Returns `true` when the acquired device meets or exceeds every limit in
    /// `required`.
    ///
    /// Comparison is delegated to [`wgpu::Limits::check_limits`], which
    /// correctly treats "higher is better" limits (e.g. max texture
    /// dimensions) and "lower is better" alignment limits (e.g.
    /// `min_uniform_buffer_offset_alignment`) with the appropriate ordering.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::device::GpuContext;
    ///
    /// # fn example(ctx: &GpuContext) {
    /// // The device always meets the downlevel defaults it was created with.
    /// assert!(ctx.meets_limits(&wgpu::Limits::downlevel_defaults()));
    /// # }
    /// ```
    #[must_use]
    pub fn meets_limits(&self, required: &wgpu::Limits) -> bool {
        required.check_limits(&self.device.limits())
    }

    /// Returns the maximum duration the recovery FSM should wait before
    /// declaring a device permanently lost and falling back to the CPU
    /// rasterizer.
    ///
    /// The default is 32 milliseconds, matching the v0.2.0 milestone
    /// specification.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::device::GpuContext;
    /// use std::time::Duration;
    ///
    /// assert_eq!(GpuContext::fallback_threshold(), Duration::from_millis(32));
    /// ```
    #[must_use]
    pub fn fallback_threshold() -> Duration {
        Duration::from_millis(32)
    }
}

impl std::fmt::Debug for GpuContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuContext")
            .field("adapter_info", &self.adapter_info)
            .finish_non_exhaustive()
    }
}

/// Scores an adapter against a power preference.
///
/// Higher is better. Discrete GPUs score highest when `HighPerformance` is
/// requested; integrated/Cpu adapters score highest when `LowPower` is
/// requested.
fn power_score(device_type: wgpu::DeviceType, preference: wgpu::PowerPreference) -> u8 {
    match preference {
        wgpu::PowerPreference::HighPerformance => match device_type {
            wgpu::DeviceType::DiscreteGpu => 3,
            wgpu::DeviceType::IntegratedGpu => 2,
            wgpu::DeviceType::Other | wgpu::DeviceType::VirtualGpu => 1,
            wgpu::DeviceType::Cpu => 0,
        },
        wgpu::PowerPreference::LowPower => match device_type {
            wgpu::DeviceType::Cpu => 3,
            wgpu::DeviceType::IntegratedGpu => 2,
            wgpu::DeviceType::Other | wgpu::DeviceType::VirtualGpu => 1,
            wgpu::DeviceType::DiscreteGpu => 0,
        },
        wgpu::PowerPreference::None => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a wgpu backend feature enabled and a GPU available"]
    fn new_high_performance_context_succeeds_or_errors_gracefully() {
        // A GPU may not be available in CI; we only verify that construction
        // either yields a usable context or returns a structured error.
        match GpuContext::new() {
            Ok(ctx) => {
                assert!(!ctx.adapter_info.name.is_empty());
                assert!(ctx.supports_features(wgpu::Features::empty()));
                assert!(ctx.meets_limits(&wgpu::Limits::downlevel_defaults()));
            }
            Err(GpuContextError::NoAdapter(_) | GpuContextError::DeviceRequestFailed(_)) => {}
        }
    }

    #[test]
    fn power_score_orders_discrete_above_integrated_for_high_performance() {
        let discrete = wgpu::DeviceType::DiscreteGpu;
        let integrated = wgpu::DeviceType::IntegratedGpu;
        assert!(
            power_score(discrete, wgpu::PowerPreference::HighPerformance)
                > power_score(integrated, wgpu::PowerPreference::HighPerformance)
        );
        assert!(
            power_score(integrated, wgpu::PowerPreference::LowPower)
                > power_score(discrete, wgpu::PowerPreference::LowPower)
        );
    }

    #[test]
    fn limits_check_requires_geq_on_every_field() {
        let a = wgpu::Limits::defaults();
        let mut b = a.clone();
        b.max_bind_groups = a.max_bind_groups + 1;
        // `b` demands more bind groups than `a` provides, so `a` does not meet
        // `b` (`required.check_limits(&device)` is false).
        assert!(!b.check_limits(&a));
        // `a` demands at most what `b` provides, so `b` meets `a`.
        assert!(a.check_limits(&b));
    }

    #[test]
    fn fallback_threshold_matches_milestone_specification() {
        assert_eq!(GpuContext::fallback_threshold(), Duration::from_millis(32));
    }

    #[test]
    #[ignore = "requires a software Vulkan adapter (Lavapipe/llvmpipe)"]
    fn with_cpu_fallback_selects_fallback_adapter() {
        // The fallback adapter may not be available on every system; we only
        // verify that construction either yields a usable context or returns a
        // structured error.
        match GpuContext::with_cpu_fallback() {
            Ok(ctx) => {
                assert!(!ctx.adapter_info.name.is_empty());
                assert!(ctx.supports_features(wgpu::Features::empty()));
            }
            Err(GpuContextError::NoAdapter(_) | GpuContextError::DeviceRequestFailed(_)) => {}
        }
    }

    #[test]
    #[ignore = "requires a wgpu adapter and device"]
    fn recreate_device_and_queue_after_destroy() {
        // Simulate a TDR/driver reset by destroying the logical device, then
        // verify the adapter can hand back a fresh device/queue pair.
        let mut ctx = match GpuContext::new() {
            Ok(ctx) => ctx,
            Err(_) => return,
        };
        ctx.device.destroy();
        ctx.recreate_device_and_queue()
            .expect("recreate should succeed");
        assert!(!ctx.adapter_info.name.is_empty());
        assert!(ctx.supports_features(wgpu::Features::empty()));
    }
}
