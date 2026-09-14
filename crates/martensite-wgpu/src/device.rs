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

/// Returns the `wgpu` backends used on this platform.
///
/// On Android this is `VULKAN | GL` — Vulkan is first-class and OpenGL
/// ES is the downlevel fallback for devices whose drivers lack usable
/// Vulkan support. On every other platform all compiled-in backends are
/// used, matching `wgpu::Instance::default()`.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::device::platform_backends;
/// use martensite_wgpu::wgpu;
///
/// assert!(platform_backends().contains(wgpu::Backends::VULKAN));
/// ```
#[must_use]
pub fn platform_backends() -> wgpu::Backends {
    if cfg!(target_os = "android") {
        wgpu::Backends::VULKAN | wgpu::Backends::GL
    } else if cfg!(target_os = "ios") {
        wgpu::Backends::METAL
    } else {
        wgpu::Backends::all()
    }
}

/// Creates a `wgpu::Instance` restricted to [`platform_backends`].
///
/// On Android this limits the instance to Vulkan + GLES; elsewhere it is
/// equivalent to `wgpu::Instance::default()`.
///
/// # Examples
///
/// ```no_run
/// use martensite_wgpu::device::new_instance;
///
/// let instance = new_instance();
/// ```
#[must_use]
pub fn new_instance() -> wgpu::Instance {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = platform_backends();
    wgpu::Instance::new(descriptor)
}

/// Requests an adapter honoring the platform's backend preference.
///
/// On Android, adapters are enumerated across [`platform_backends`] and
/// Vulkan adapters are ranked above GLES (Vulkan first-class, GLES
/// downlevel fallback), then by [`power_score`]. `compatible_surface`
/// is honored by dropping adapters that cannot present to it. On other
/// platforms this is equivalent to `Instance::request_adapter`.
async fn request_platform_adapter(
    instance: &wgpu::Instance,
    power_preference: wgpu::PowerPreference,
    compatible_surface: Option<&wgpu::Surface<'_>>,
    force_fallback_adapter: bool,
) -> Result<wgpu::Adapter, GpuContextError> {
    if cfg!(target_os = "android") {
        let mut adapters = instance.enumerate_adapters(platform_backends()).await;
        if let Some(surface) = compatible_surface {
            adapters.retain(|adapter| adapter.is_surface_supported(surface));
        }
        if force_fallback_adapter {
            adapters.retain(|adapter| adapter.get_info().device_type == wgpu::DeviceType::Cpu);
        }
        rank_adapters(&mut adapters, power_preference);
        return adapters.into_iter().next().ok_or_else(|| {
            GpuContextError::NoAdapter(
                "no Vulkan or GLES adapter was enumerated on Android".to_string(),
            )
        });
    }

    instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference,
            compatible_surface,
            force_fallback_adapter,
            apply_limit_buckets: true,
        })
        .await
        .map_err(|e| GpuContextError::NoAdapter(e.to_string()))
}

/// Stable-sorts `adapters` by platform preference, most-desired first.
///
/// On Android, Vulkan adapters rank above every other backend (Vulkan is
/// first-class; GLES is the downlevel fallback). Everywhere — and within
/// a backend tier — the requested [`wgpu::PowerPreference`] breaks ties
/// via [`power_score`]. Shared by [`request_platform_adapter`] and
/// [`GpuContext::enumerate_adapters`] so the diagnostic listing reflects
/// the same ordering the constructor picks from.
fn rank_adapters(adapters: &mut [wgpu::Adapter], power_preference: wgpu::PowerPreference) {
    adapters.sort_by(|a, b| {
        let score = |adapter: &wgpu::Adapter| {
            let info = adapter.get_info();
            let backend_rank =
                u8::from(cfg!(target_os = "android") && info.backend == wgpu::Backend::Vulkan);
            (
                backend_rank,
                power_score(info.device_type, power_preference),
            )
        };
        score(b).cmp(&score(a))
    });
}

/// Returns the device descriptor used to request a logical device from
/// `adapter`.
///
/// GLES adapters get [`wgpu::Limits::downlevel_defaults`]: the GLES
/// backend cannot honor the desktop default limits, so the downlevel
/// limit set is the portable choice for the Android GLES fallback.
///
/// Other adapters on Android request exactly `adapter.limits()`: the
/// enumerate-then-pick path does not apply limit buckets (unlike
/// `request_adapter` with `apply_limit_buckets`), and low-end Vulkan
/// adapters can sit below wgpu's defaults — a default `required_limits`
/// request would fail `request_device` on hardware that is otherwise
/// usable. Requesting the advertised limits grants full capability and
/// always validates. Non-Android adapters keep wgpu's defaults, matching
/// the desktop `request_adapter` + `apply_limit_buckets` behavior.
fn device_descriptor_for(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    let mut descriptor = wgpu::DeviceDescriptor::default();
    match adapter.get_info().backend {
        wgpu::Backend::Gl => {
            descriptor.required_limits = wgpu::Limits::downlevel_defaults();
        }
        _ if cfg!(target_os = "android") => {
            descriptor.required_limits = adapter.limits();
        }
        _ => {}
    }
    descriptor
}

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
    /// Returns the [`wgpu::Backends`] set appropriate for the target
    /// platform.
    ///
    /// - **iOS:** [`wgpu::Backends::METAL`] — UIKit windows are backed by
    ///   `CAMetalLayer`, and Metal is the only GPU API available on the
    ///   platform. Restricting the instance avoids enumerating backends
    ///   (Vulkan, GLES) that can never surface an adapter there.
    /// - **All other platforms:** [`wgpu::Backends::all`] — adapter
    ///   enumeration picks whichever compiled-in backend is present.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::device::GpuContext;
    ///
    /// // On iOS this is `Backends::METAL`, on Android Vulkan|GL;
    /// // elsewhere `Backends::all()`.
    /// let backends = GpuContext::platform_backends();
    /// assert!(!backends.is_empty());
    /// ```
    #[must_use]
    pub fn platform_backends() -> wgpu::Backends {
        platform_backends()
    }

    /// Builds a [`wgpu::InstanceDescriptor`] using
    /// [`platform_backends`](Self::platform_backends) for the backend set.
    ///
    /// This is the iOS-correct counterpart of
    /// `wgpu::InstanceDescriptor::new_without_display_handle()`: identical
    /// except `backends` is `METAL` instead of `all()` when targeting
    /// `aarch64-apple-ios(-sim)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::device::GpuContext;
    ///
    /// let desc = GpuContext::instance_descriptor();
    /// assert_eq!(desc.backends, GpuContext::platform_backends());
    /// ```
    #[must_use]
    pub fn instance_descriptor() -> wgpu::InstanceDescriptor {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = platform_backends();
        desc
    }

    /// Creates a [`wgpu::Instance`] restricted to
    /// [`platform_backends`](Self::platform_backends).
    ///
    /// On iOS the instance only enables the Metal backend; elsewhere this
    /// is equivalent to [`wgpu::Instance::default`]. Surfaces created from
    /// the returned instance work with UIKit `CAMetalLayer`-backed winit
    /// windows on iOS.
    ///
    /// # Panics
    ///
    /// Panics if no backend feature for the active target platform is
    /// enabled in the `wgpu` build — the workspace manifest enables the
    /// `metal` backend unconditionally, so this cannot happen on iOS.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::device::GpuContext;
    ///
    /// let instance = GpuContext::create_instance();
    /// ```
    #[must_use]
    pub fn create_instance() -> wgpu::Instance {
        wgpu::Instance::new(Self::instance_descriptor())
    }

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
        let instance = new_instance();

        let adapter = pollster::block_on(request_platform_adapter(
            &instance,
            power_preference,
            None,
            false,
        ))?;

        let (device, queue) =
            pollster::block_on(adapter.request_device(&device_descriptor_for(&adapter)))
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
        let instance = new_instance();

        let adapter = pollster::block_on(request_platform_adapter(
            &instance,
            wgpu::PowerPreference::LowPower,
            None,
            true,
        ))?;

        let (device, queue) =
            pollster::block_on(adapter.request_device(&device_descriptor_for(&adapter)))
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
        let adapter = request_platform_adapter(
            instance,
            wgpu::PowerPreference::HighPerformance,
            Some(surface),
            false,
        )
        .await?;

        let (device, queue) = adapter
            .request_device(&device_descriptor_for(&adapter))
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

    /// Creates a `wgpu` presentation surface for a window target.
    ///
    /// This is a thin pass-through to [`wgpu::Instance::create_surface`]
    /// on this context's instance, so the surface shares the instance the
    /// adapter and device came from — required by `wgpu` for
    /// `for_surface`-compatible adapters to present to it.
    ///
    /// On Android the winit window's `RawWindowHandle::AndroidNdk`
    /// (`ANativeWindow*`) is consumed directly: the Vulkan backend
    /// creates a `VK_KHR_android_surface` and the GLES backend an
    /// `EGLSurface`. Create the surface inside
    /// `ApplicationHandler::can_create_surfaces` — the `ANativeWindow`
    /// does not exist before then — and drop it before the window in
    /// `destroy_surfaces`.
    ///
    /// `ApplicationHandler::can_create_surfaces`: winit::application::ApplicationHandler::can_create_surfaces
    /// `destroy_surfaces`: winit::application::ApplicationHandler::destroy_surfaces
    ///
    /// # Errors
    ///
    /// Returns [`wgpu::CreateSurfaceError`] if the target's window or
    /// display handle cannot be consumed by any enabled backend.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::device::GpuContext;
    /// use martensite_wgpu::wgpu;
    ///
    /// # fn example(ctx: &GpuContext, target: wgpu::SurfaceTarget<'static>) {
    /// let surface = ctx.create_surface(target).expect("surface");
    /// # }
    /// ```
    pub fn create_surface<'window>(
        &self,
        target: impl Into<wgpu::SurfaceTarget<'window>>,
    ) -> Result<wgpu::Surface<'window>, wgpu::CreateSurfaceError> {
        self.instance.create_surface(target)
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
                .request_device(&device_descriptor_for(&self.adapter)),
        )
        .map_err(|e| GpuContextError::DeviceRequestFailed(e.to_string()))?;

        self.device = Arc::new(device);
        self.queue = Arc::new(queue);
        Ok(())
    }

    /// Enumerates every adapter currently visible to the instance across
    /// [`platform_backends`], in the same order [`request_platform_adapter`]
    /// would pick from.
    ///
    /// The returned vector is sorted so that the most-desired adapters
    /// appear first: on Android, Vulkan ranks above GLES; everywhere, the
    /// requested power preference breaks ties. This is useful for
    /// diagnostic UI and for the recovery FSM, which must re-enumerate
    /// adapters after a device loss.
    ///
    /// Enumeration is restricted to [`platform_backends`](Self::platform_backends)
    /// (Metal-only on iOS), consistent with [`create_instance`](Self::create_instance).
    /// Pass an instance created by [`create_instance`](Self::create_instance).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::device::GpuContext;
    ///
    /// let instance = GpuContext::create_instance();
    /// let adapters = GpuContext::enumerate_adapters(&instance, wgpu::PowerPreference::HighPerformance);
    /// // The list may be empty in headless environments without a GPU.
    /// println!("found {} adapter(s)", adapters.len());
    /// ```
    #[must_use]
    pub fn enumerate_adapters(
        instance: &wgpu::Instance,
        power_preference: wgpu::PowerPreference,
    ) -> Vec<wgpu::Adapter> {
        let mut adapters = pollster::block_on(instance.enumerate_adapters(platform_backends()));
        // Shared ranking: Vulkan-first on Android, power preference within
        // a backend tier — identical to the order request_platform_adapter
        // picks from.
        rank_adapters(&mut adapters, power_preference);
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
