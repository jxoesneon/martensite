//! Web (`wasm32-unknown-unknown`) GPU backend selection.
//!
//! On the web there is exactly one "display server": the browser. `wgpu`
//! exposes two browser rendering paths behind a single
//! [`wgpu::Instance`]:
//!
//! * [`wgpu::Backends::BROWSER_WEBGPU`] — the native WebGPU API
//!   (`navigator.gpu`). Full compute support; the Vello renderer works here.
//! * [`wgpu::Backends::GL`] — the WebGL2 downlevel backend (`wgpu-hal`'s
//!   `glow` path, enabled by wgpu's `webgl` feature). **No compute
//!   shaders.** Vello cannot run here; see [`WebBackend::WebGl2`].
//!
//! [`create_instance`] requests both backends and defers to
//! [`wgpu::util::new_instance_with_webgpu_detection`], which performs a real
//! `navigator.gpu.requestAdapter()` probe before deciding whether the
//! instance may create WebGPU adapters. This matters because some browsers
//! (Chrome on Linux at the time of writing) expose `navigator.gpu` yet fail
//! every adapter request — the naive `Instance::new` presence-check would
//! then lock the instance to WebGPU and produce no adapters at all.
//!
//! # TinySkia fallback decision boundary
//!
//! The renderer selection is a real, enforced boundary, not a documented
//! wish:
//!
//! | Adapter backend | Rasterizer | Reason |
//! |-----------------|------------|--------|
//! | [`wgpu::Backend::BrowserWebGpu`] | Vello compute → wgpu | compute available |
//! | [`wgpu::Backend::Gl`] | TinySkia CPU → `queue.write_texture` | WebGL2 has no compute; Vello's pipeline cannot be expressed |
//! | no adapter | TinySkia CPU → `softbuffer`-style copy | no wgpu device at all |
//!
//! [`WebBackend::supports_vello`] encodes the compute gate;
//! [`RenderOrchestrator`](crate::orchestrator::RenderOrchestrator) already
//! implements the TinySkia CPU upload path used in the second and third
//! rows — the web port reuses it rather than adding a parallel path.
//!
//! # `fragile-send-sync-non-atomic-wasm`
//!
//! The workspace `wgpu` dependency enables the
//! `fragile-send-sync-non-atomic-wasm` feature. On `wasm32` without the
//! `atomics` target-feature, wgpu's handle types internally use `Rc`/
//! `RefCell` and are therefore `!Send`/`!Sync`; the feature installs wgpu's
//! sanctioned always-`Send`/`Sync` impls so the crate's existing
//! `Send + Sync` bounds hold unchanged. The feature is a no-op on every
//! native target (its cfg requires `target_arch = "wasm32"` and
//! `not(target_feature = "atomics")`). It is *unsound* only if wasm
//! objects actually cross a thread boundary; Martensite's web target is
//! single-threaded (no `atomics`, no `SharedArrayBuffer`), which is
//! precisely the configuration the feature guards on.
//!
//! # COOP/COEP
//!
//! Cross-Origin-Opener-Policy / Cross-Origin-Embedder-Policy headers
//! (`Cross-Origin-Opener-Policy: same-origin` + `Cross-Origin-Embedder-
//! Policy: require-corp`) are required only for `crossOriginIsolated`
//! contexts — i.e. `SharedArrayBuffer` and wasm threading. Martensite's
//! web target does not require them; they are documented here because a
//! future multithreaded wasm build (atomics + `rayon`-style workers)
//! would need the serving page to set both headers, and would then also
//! need to *drop* `fragile-send-sync-non-atomic-wasm` in favor of wgpu's
//! real shared-state backend.

use std::sync::Arc;

use crate::device::{GpuContext, GpuContextError};

/// The GPU backend `wgpu` selected on the web, with the rasterizer it
/// implies.
///
/// See the [module-level decision table](self) for the TinySkia fallback
/// boundary this enum encodes.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::web::WebBackend;
///
/// assert!(WebBackend::WebGpu.supports_vello());
/// assert!(!WebBackend::WebGl2.supports_vello());
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum WebBackend {
    /// The browser's native WebGPU API (`navigator.gpu`). Compute shaders
    /// are available, so the Vello renderer can run.
    WebGpu,
    /// The WebGL2 downlevel backend. WebGL2 has no compute stage, so Vello
    /// cannot run; callers must select the TinySkia CPU raster path.
    WebGl2,
    /// No GPU adapter is available at all. All rasterization runs on the
    /// CPU via TinySkia; presentation is a `queue.write_texture`/
    /// `softbuffer`-style pixel upload performed by the caller.
    CpuRaster,
}

impl WebBackend {
    /// Returns `true` when the Vello compute pipeline can run on this
    /// backend.
    ///
    /// Vello is compute-only: it requires storage buffers and compute
    /// dispatch, which exist on WebGPU but not on WebGL2. On `WebGl2` and
    /// `CpuRaster` the answer is unconditionally `false` — there is no
    /// WebGL2 Vello path to gate.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::web::WebBackend;
    ///
    /// assert_eq!(WebBackend::WebGpu.supports_vello(), true);
    /// assert_eq!(WebBackend::WebGl2.supports_vello(), false);
    /// assert_eq!(WebBackend::CpuRaster.supports_vello(), false);
    /// ```
    #[must_use]
    pub fn supports_vello(self) -> bool {
        matches!(self, Self::WebGpu)
    }

    /// Returns `true` when rasterization must run on the CPU (TinySkia).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::web::WebBackend;
    ///
    /// assert!(!WebBackend::WebGpu.requires_cpu_raster());
    /// assert!(WebBackend::WebGl2.requires_cpu_raster());
    /// assert!(WebBackend::CpuRaster.requires_cpu_raster());
    /// ```
    #[must_use]
    pub fn requires_cpu_raster(self) -> bool {
        !self.supports_vello()
    }
}

/// Returns the backend flags requested for the web instance:
/// `BROWSER_WEBGPU | GL`.
///
/// `BROWSER_WEBGPU` is tried first via
/// [`wgpu::util::new_instance_with_webgpu_detection`]; if the browser's
/// WebGPU probe fails, the flag is removed and only the WebGL2 downlevel
/// backend remains.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::web;
/// use martensite_wgpu::wgpu;
///
/// let backends = web::web_backends();
/// assert!(backends.contains(wgpu::Backends::BROWSER_WEBGPU));
/// assert!(backends.contains(wgpu::Backends::GL));
/// ```
#[must_use]
pub fn web_backends() -> wgpu::Backends {
    wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL
}

/// Creates a [`wgpu::Instance`] for the web target.
///
/// Requests [`web_backends`] and runs wgpu's real WebGPU support probe
/// (`navigator.gpu.requestAdapter()`), so a browser that exposes
/// `navigator.gpu` but cannot create adapters still falls through to the
/// WebGL2 backend instead of yielding an empty instance.
///
/// The call is `async` because the probe awaits a JS promise; callers on
/// the wasm main thread should drive it with
/// `wasm_bindgen_futures::spawn_local` or from an `async` entry point.
///
/// # Examples
///
/// ```no_run
/// async fn init() {
///     let instance = martensite_wgpu::web::create_instance().await;
///     let _ = instance;
/// }
/// ```
pub async fn create_instance() -> wgpu::Instance {
    let desc = wgpu::InstanceDescriptor {
        backends: web_backends(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    };
    wgpu::util::new_instance_with_webgpu_detection(desc).await
}

/// Classifies an adapter's [`wgpu::AdapterInfo`] into a [`WebBackend`].
///
/// `wgpu::Backend::BrowserWebGpu` maps to [`WebBackend::WebGpu`]. Any GL
/// adapter (`Backend::Gl`, the only other backend reachable on
/// `wasm32-unknown-unknown`) maps to [`WebBackend::WebGl2`]. Any other
/// value is defensive: it cannot be produced by a web instance today, and
/// is mapped to [`WebBackend::CpuRaster`] rather than pretending compute
/// is available.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::web::{classify_adapter, WebBackend};
/// use martensite_wgpu::wgpu;
///
/// let info = wgpu::AdapterInfo {
///     name: String::new(),
///     vendor: 0,
///     device: 0,
///     device_type: wgpu::DeviceType::Other,
///     device_pci_bus_id: String::new(),
///     driver: String::new(),
///     driver_info: String::new(),
///     backend: wgpu::Backend::BrowserWebGpu,
///     subgroup_min_size: 0,
///     subgroup_max_size: 0,
///     transient_saves_memory: None,
///     limit_bucket: None,
/// };
/// assert_eq!(classify_adapter(&info), WebBackend::WebGpu);
/// ```
#[must_use]
pub fn classify_adapter(info: &wgpu::AdapterInfo) -> WebBackend {
    match info.backend {
        wgpu::Backend::BrowserWebGpu => WebBackend::WebGpu,
        wgpu::Backend::Gl => WebBackend::WebGl2,
        // Defensive: no other backend is reachable on wasm32-unknown-unknown
        // today. Treat unknown adapters as CPU-raster-only rather than
        // assuming compute exists.
        _ => WebBackend::CpuRaster,
    }
}

/// Returns the [`wgpu::DeviceDescriptor`] appropriate for `backend`.
///
/// On [`WebBackend::WebGl2`] the device is requested with
/// [`wgpu::Limits::downlevel_webgl2_defaults`], the limit set wgpu
/// guarantees fits inside WebGL2's mandatory minimums (no compute, reduced
/// texture/binding counts). On [`WebBackend::WebGpu`] the default
/// descriptor is used; on [`WebBackend::CpuRaster`] there is no device and
/// the returned descriptor is unused by convention.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::web::{device_descriptor_for, WebBackend};
/// use martensite_wgpu::wgpu;
///
/// let gl = device_descriptor_for(WebBackend::WebGl2);
/// assert_eq!(
///     gl.required_limits,
///     wgpu::Limits::downlevel_webgl2_defaults()
/// );
/// let webgpu = device_descriptor_for(WebBackend::WebGpu);
/// assert_eq!(webgpu.required_limits, wgpu::Limits::default());
/// ```
#[must_use]
pub fn device_descriptor_for(backend: WebBackend) -> wgpu::DeviceDescriptor<'static> {
    wgpu::DeviceDescriptor {
        required_limits: match backend {
            WebBackend::WebGl2 => wgpu::Limits::downlevel_webgl2_defaults(),
            WebBackend::WebGpu | WebBackend::CpuRaster => wgpu::Limits::default(),
        },
        ..wgpu::DeviceDescriptor::default()
    }
}

/// Requests a surface-compatible (or bare) adapter on a web instance and
/// returns the [`GpuContext`] together with the [`WebBackend`] that was
/// actually selected.
///
/// The caller inspects the returned `WebBackend` to pick the rasterizer —
/// Vello for [`WebBackend::WebGpu`], TinySkia for the rest — per the
/// [module-level decision boundary](self).
///
/// `surface` may be `None` for offscreen/headless probing.
///
/// # Errors
///
/// Returns [`GpuContextError::NoAdapter`] when the browser exposes neither
/// a WebGPU nor a WebGL2 adapter — the [`WebBackend::CpuRaster`] case,
/// which the caller handles by rasterizing with TinySkia and uploading
/// pixels itself. Returns [`GpuContextError::DeviceRequestFailed`] when an
/// adapter was found but refused a device.
///
/// # Examples
///
/// ```no_run
/// async fn init() {
///     let instance = martensite_wgpu::web::create_instance().await;
///     match martensite_wgpu::web::gpu_context_for_web(&instance, None).await {
///         Ok((_ctx, backend)) => { /* pick rasterizer from `backend` */ }
///         Err(_) => { /* WebBackend::CpuRaster — TinySkia only */ }
///     }
/// }
/// ```
pub async fn gpu_context_for_web(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<(GpuContext, WebBackend), GpuContextError> {
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: surface,
            force_fallback_adapter: false,
            apply_limit_buckets: true,
        })
        .await
        .map_err(|e| GpuContextError::NoAdapter(e.to_string()))?;

    let adapter_info = adapter.get_info();
    let backend = classify_adapter(&adapter_info);

    let (device, queue) = adapter
        .request_device(&device_descriptor_for(backend))
        .await
        .map_err(|e| GpuContextError::DeviceRequestFailed(e.to_string()))?;

    let context = GpuContext {
        instance: Arc::new(instance.clone()),
        adapter: Arc::new(adapter),
        device: Arc::new(device),
        queue: Arc::new(queue),
        adapter_info,
    };
    Ok((context, backend))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_backends_requests_webgpu_and_gl() {
        let backends = web_backends();
        assert!(backends.contains(wgpu::Backends::BROWSER_WEBGPU));
        assert!(backends.contains(wgpu::Backends::GL));
        // No native backend is requested on the web.
        assert!(!backends.contains(wgpu::Backends::VULKAN));
        assert!(!backends.contains(wgpu::Backends::METAL));
    }

    #[test]
    fn classify_maps_wgpu_backend_variants() {
        let mut info = wgpu::AdapterInfo {
            name: String::new(),
            vendor: 0,
            device: 0,
            device_type: wgpu::DeviceType::Other,
            device_pci_bus_id: String::new(),
            driver: String::new(),
            driver_info: String::new(),
            backend: wgpu::Backend::BrowserWebGpu,
            subgroup_min_size: 0,
            subgroup_max_size: 0,
            transient_saves_memory: None,
            limit_bucket: None,
        };
        assert_eq!(classify_adapter(&info), WebBackend::WebGpu);
        info.backend = wgpu::Backend::Gl;
        assert_eq!(classify_adapter(&info), WebBackend::WebGl2);
        info.backend = wgpu::Backend::Vulkan;
        assert_eq!(classify_adapter(&info), WebBackend::CpuRaster);
    }

    #[test]
    fn vello_gate_is_compute_only() {
        assert!(WebBackend::WebGpu.supports_vello());
        assert!(!WebBackend::WebGl2.supports_vello());
        assert!(!WebBackend::CpuRaster.supports_vello());
    }

    #[test]
    fn webgl2_descriptor_uses_downlevel_limits() {
        let desc = device_descriptor_for(WebBackend::WebGl2);
        assert_eq!(
            desc.required_limits,
            wgpu::Limits::downlevel_webgl2_defaults()
        );
    }
}
