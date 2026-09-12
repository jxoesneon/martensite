//! Render pipeline orchestrator.
//!
//! This module bridges [`martensite_render`] (the hardware-agnostic paint
//! command stream and backends) with [`crate`] (the WGPU device, surface,
//! and recovery state machine). The [`RenderOrchestrator`] owns a
//! [`martensite_render::VelloRenderer`] for GPU scene composition, a
//! [`crate::GpuContext`] and [`crate::SurfaceWrapper`] for WGPU surface
//! management, and a [`martensite_render::TinySkiaBackend`] for CPU
//! fallback. It switches between GPU and CPU backends based on the
//! [`crate::resilience::RecoveryMachine`] state and the application
//! configuration.
//!
//! When the GPU device is healthy, the orchestrator translates a
//! [`martensite_render::PaintList`] into a Vello `Scene`, then dispatches
//! the scene to the GPU via `vello::Renderer::render_to_texture`. When
//! the recovery machine transitions to
//! [`crate::resilience::DeviceStatus::FallbackCpu`], the orchestrator
//! falls back to the TinySkia CPU rasterizer.

use crate::resilience::{RecoveryMachine, SurfaceError};
use crate::surface::SurfaceWrapper;
use martensite_render::{PaintList, RenderBackend, TinySkiaBackend, VelloRenderer};
use std::borrow::Cow;

/// The rendering mode currently active in the orchestrator.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::orchestrator::RenderMode;
///
/// assert_ne!(RenderMode::Gpu, RenderMode::Cpu);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// GPU rendering via Vello compute pipeline.
    Gpu,
    /// CPU software rendering via TinySkia.
    Cpu,
}

/// Error returned by orchestrator operations.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::orchestrator::OrchestratorError;
/// use std::error::Error;
///
/// let err = OrchestratorError::NoBackend;
/// assert_eq!(err.to_string(), "no rendering backend available");
/// assert!(err.source().is_none());
/// ```
#[derive(Debug)]
pub enum OrchestratorError {
    /// The GPU context is not available and CPU fallback is disabled.
    NoBackend,
    /// The TinySkia backend failed to initialize.
    BackendInitFailed,
    /// An offscreen GPU render or readback failed. The carried string
    /// preserves the underlying wgpu error message for diagnostics.
    GpuReadbackFailed(String),
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoBackend => write!(f, "no rendering backend available"),
            Self::BackendInitFailed => write!(f, "rendering backend initialization failed"),
            Self::GpuReadbackFailed(msg) => {
                write!(f, "offscreen GPU render/readback failed: {msg}")
            }
        }
    }
}

impl std::error::Error for OrchestratorError {}

/// Application configuration consumed by the orchestrator.
///
/// This is a simplified view of `martensite::app::AppConfig` that lives
/// in `martensite-wgpu` to avoid a circular dependency. The umbrella
/// crate's `AppConfig` is converted into this struct before being
/// passed to the orchestrator.
///
/// # Examples
///
/// ```
/// use martensite_wgpu::orchestrator::OrchestratorConfig;
///
/// let config = OrchestratorConfig::new(true, false);
/// assert!(config.allow_software_fallback);
/// assert!(!config.prefer_cpu);
/// ```
#[derive(Debug, Clone, Default)]
pub struct OrchestratorConfig {
    /// Whether CPU software fallback is allowed.
    pub allow_software_fallback: bool,
    /// Whether to prefer the CPU backend even when a GPU is available.
    pub prefer_cpu: bool,
}

impl OrchestratorConfig {
    /// Creates a new config with the given settings.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_wgpu::orchestrator::OrchestratorConfig;
    ///
    /// let config = OrchestratorConfig::new(false, true);
    /// assert!(!config.allow_software_fallback);
    /// assert!(config.prefer_cpu);
    /// ```
    #[must_use]
    pub fn new(allow_software_fallback: bool, prefer_cpu: bool) -> Self {
        Self {
            allow_software_fallback,
            prefer_cpu,
        }
    }
}

/// Orchestrates GPU and CPU rendering backends with automatic fallback.
///
/// The orchestrator owns both a [`VelloRenderer`] (for GPU scene
/// composition) and a [`TinySkiaBackend`] (for CPU fallback). It queries
/// the [`RecoveryMachine`] and [`OrchestratorConfig`] to determine which
/// backend to use for each frame.
///
/// When `prefer_cpu` is set in the config, the CPU backend is always
/// used. When `allow_software_fallback` is set and the recovery machine
/// is in `FallbackCpu` state, the CPU backend is used. Otherwise, the
/// Vello GPU backend is used.
///
/// # Examples
///
/// ```no_run
/// use martensite_wgpu::orchestrator::{OrchestratorConfig, RenderOrchestrator};
///
/// let orchestrator = RenderOrchestrator::new(100, 100, OrchestratorConfig::default());
/// assert!(orchestrator.is_ok());
/// ```
pub struct RenderOrchestrator {
    /// The Vello GPU renderer (scene builder).
    vello: VelloRenderer,
    /// The TinySkia CPU fallback renderer.
    tinyskia: TinySkiaBackend,
    /// The current rendering mode.
    mode: RenderMode,
    /// The application configuration.
    config: OrchestratorConfig,
    /// The current backdrop mode, used to determine whether the
    /// renderer should clear to transparent (for system materials)
    /// or opaque (for solid backgrounds). The render backend already
    /// clears to transparent black by default; this field lets the
    /// orchestrator expose the active mode to the application so it
    /// can skip the background fill rect when `Transparent`.
    backdrop_mode: crate::surface::BackdropMode,
    /// The external-surface composite host (`v0.14.0`). When `Some`,
    /// `PaintCommand::External` markers in a paint list are composited
    /// via [`WgpuHost`] between Vello scene segments, preserving exact
    /// paint ordering.
    external_host: Option<crate::external::WgpuHost>,
    /// Ordered paint segments captured by the last GPU-mode `render`
    /// call. Empty when the last list contained no `External` markers —
    /// the whole-list Vello dispatch is used unchanged.
    pending: Vec<PendingSegment>,
    /// The clear mode the last GPU-mode `render` call configured —
    /// applied by `dispatch_pending`'s phase-2 frame-clear pass.
    #[cfg_attr(not(feature = "vello"), allow(dead_code))]
    gpu_clear: martensite_render::ClearMode,
    /// Offscreen `Rgba8Unorm` textures one per Vello command segment.
    /// Each entry is the texture plus its storage view (the Vello
    /// target); the sRGB-or-not sample view is created per frame by
    /// `dispatch_pending` to match the frame target.
    #[cfg(feature = "vello")]
    seg_pool: Vec<Option<SegEntry>>,
    /// The engine bridge consumed by `PaintCommand::External` markers —
    /// when installed, `dispatch_pending` takes each marker's front
    /// frame from the ring (zero-copy mailbox semantics) instead of
    /// sampling a statically registered texture.
    bridge: Option<martensite_engine_bridge::BridgeHandle>,
    /// Invoked immediately before `SurfaceTexture::present` — the app
    /// installs `winit::window::Window::pre_present_notify` here so the
    /// compositor is notified before the frame lands (milestone pacing
    /// contract).
    pre_present_notify: Option<Box<dyn Fn() + Send>>,
    /// CPU-fallback frame resolver: when a ring's front slot carries no
    /// `CpuFrame`, this asks the owning `Engine` for `to_pixmap(token)`
    /// (wired from `ExternalEngines::cpu_frame_for`).
    cpu_frame_resolver: Option<CpuFrameResolver>,
}

/// One pooled Vello segment texture: storage target + cached
/// target-format sample view and bind group (rebuilt only on resize or
/// host recreation — never per frame).
#[cfg(feature = "vello")]
struct SegEntry {
    texture: wgpu::Texture,
    storage_view: wgpu::TextureView,
    /// `(target_is_srgb, sample_view, bind_group)` — the composite
    /// pipeline's sampling state, keyed by the host's target sRGB-ness.
    sample: Option<(bool, wgpu::TextureView, wgpu::BindGroup)>,
}

/// CPU-fallback frame resolver installed via
/// [`RenderOrchestrator::set_cpu_frame_resolver`]: maps
/// `(surface, frame_token)` to the engine's `to_pixmap` raster.
pub type CpuFrameResolver = Box<
    dyn FnMut(martensite_core::SurfaceId, u64) -> Option<martensite_engine_bridge::CpuFrame> + Send,
>;

/// One element of a segmented frame — mirrors
/// [`martensite_render::PaintSegment`] but owns its command span so it
/// can live past `render` until `render_to_surface`.
#[cfg_attr(not(feature = "vello"), allow(dead_code))]
enum PendingSegment {
    /// Ordinary paint commands for the Vello scene builder.
    Commands(martensite_render::PaintList),
    /// An external-surface composite point.
    External {
        /// The registered external surface identifier.
        surface_id: martensite_core::SurfaceId,
        /// Destination rectangle in physical pixels.
        rect: [f32; 4],
        /// Clip rectangle in physical pixels.
        clip: [f32; 4],
    },
}

/// The frame target [`RenderOrchestrator::dispatch_pending`] draws
/// into — bundles the view, format, size, and Vello-direct flag.
#[cfg(feature = "vello")]
struct DispatchTarget<'a> {
    /// The target texture view (surface texture or offscreen buffer).
    view: &'a wgpu::TextureView,
    /// The target's texture format — the composite pipelines are
    /// built for it.
    format: wgpu::TextureFormat,
    /// Target width in physical pixels.
    width: u32,
    /// Target height in physical pixels.
    height: u32,
    /// Whether Vello can write the target directly (Rgba8Unorm +
    /// STORAGE_BINDING), skipping the offscreen-segment path when no
    /// external markers are present.
    vello_direct: bool,
}

impl RenderOrchestrator {
    /// Creates a new orchestrator with the given target dimensions and
    /// application configuration.
    ///
    /// The TinySkia backend is initialized at the given width and height
    /// for CPU fallback. The Vello renderer starts with an empty scene.
    ///
    /// # Errors
    ///
    /// Returns [`OrchestratorError::BackendInitFailed`] if the TinySkia
    /// backend cannot be initialized (e.g. zero dimensions).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::{OrchestratorConfig, RenderOrchestrator};
    ///
    /// let orchestrator = RenderOrchestrator::new(100, 100, OrchestratorConfig::default());
    /// assert!(orchestrator.is_ok());
    /// ```
    pub fn new(
        width: u32,
        height: u32,
        config: OrchestratorConfig,
    ) -> Result<Self, OrchestratorError> {
        let tinyskia =
            TinySkiaBackend::new(width, height).ok_or(OrchestratorError::BackendInitFailed)?;
        let initial_mode = if config.prefer_cpu {
            RenderMode::Cpu
        } else {
            RenderMode::Gpu
        };
        Ok(Self {
            vello: VelloRenderer::new(),
            tinyskia,
            mode: initial_mode,
            config,
            backdrop_mode: crate::surface::BackdropMode::Opaque,
            external_host: None,
            pending: Vec::new(),
            gpu_clear: martensite_render::ClearMode::Opaque([0.0, 0.0, 0.0, 1.0]),
            #[cfg(feature = "vello")]
            seg_pool: Vec::new(),
            bridge: None,
            pre_present_notify: None,
            cpu_frame_resolver: None,
        })
    }

    /// Creates a new orchestrator with default configuration.
    ///
    /// # Errors
    ///
    /// Returns [`OrchestratorError::BackendInitFailed`] if the TinySkia
    /// backend cannot be initialized.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    ///
    /// let orchestrator = RenderOrchestrator::with_default_config(100, 100);
    /// assert!(orchestrator.is_ok());
    /// ```
    pub fn with_default_config(width: u32, height: u32) -> Result<Self, OrchestratorError> {
        Self::new(width, height, OrchestratorConfig::default())
    }

    /// Renders a [`PaintList`] using the appropriate backend.
    ///
    /// The backend is selected based on the [`OrchestratorConfig`] and
    /// the [`RecoveryMachine`] state:
    /// - If `prefer_cpu` is set, the CPU backend is always used.
    /// - If `allow_software_fallback` is set and the recovery machine is
    ///   in `FallbackCpu` state, the CPU backend is used.
    /// - Otherwise, the Vello GPU backend is used to build a scene.
    ///
    /// The built Vello scene is available via [`vello`](Self::vello) for
    /// the WGPU surface dispatch code to submit to `vello::Renderer`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    /// use martensite_wgpu::resilience::RecoveryMachine;
    /// use martensite_render::PaintList;
    ///
    /// # fn example(orchestrator: &mut RenderOrchestrator) {
    /// let recovery = RecoveryMachine::new();
    /// let list = PaintList::new();
    /// orchestrator.render(&list, &recovery);
    /// # }
    /// ```
    pub fn render(&mut self, paint_list: &PaintList, recovery: &RecoveryMachine) {
        let use_cpu = self.config.prefer_cpu
            || (self.config.allow_software_fallback && recovery.is_fallback_cpu());
        // Derive the clear mode from the active backdrop mode so the
        // renderer clears to transparent black when a system material
        // (Mica/Acrylic/vibrancy) is active, or to opaque black when no
        // system material is in use. This makes `RenderOrchestrator::
        // backdrop_mode` actually drive rendering behavior rather than
        // being advisory-only.
        let clear_mode = match self.backdrop_mode {
            crate::surface::BackdropMode::Opaque => {
                martensite_render::ClearMode::Opaque([0.0, 0.0, 0.0, 1.0])
            }
            crate::surface::BackdropMode::Transparent => martensite_render::ClearMode::Transparent,
        };
        if use_cpu {
            self.mode = RenderMode::Cpu;
            self.pending.clear();
            self.tinyskia.render_with_clear(paint_list, clear_mode);
            // Resolve CPU-fallback payloads: each external marker whose
            // ring slot carries a CpuFrame is composited over the
            // placeholder. The slot is still taken/released so the
            // producer's mailbox keeps cycling — without releases the
            // two-slot ring would stall once both slots went Ready.
            if paint_list.has_external() {
                if let Some(bridge) = &self.bridge {
                    for command in &paint_list.commands {
                        if let martensite_render::PaintCommand::External {
                            surface_id,
                            rect,
                            clip,
                        } = command
                        {
                            let sid = *surface_id;
                            // Ring bookkeeping under a short lock; the
                            // CpuFrame is consumed after the guard drops.
                            let taken = {
                                let mut reg = bridge.lock();
                                match reg.take_front(sid) {
                                    Ok(Some((slot, token))) => {
                                        let cpu = reg.front_cpu_frame(sid).ok().flatten().cloned();
                                        Some((slot, token, cpu))
                                    }
                                    _ => None,
                                }
                            };
                            let Some((slot, token, cpu)) = taken else {
                                continue;
                            };
                            // Prefer the ring-published CpuFrame; if the
                            // producer shipped none, ask the engine for
                            // `to_pixmap(token)` via the resolver.
                            let cpu = cpu.or_else(|| {
                                self.cpu_frame_resolver
                                    .as_mut()
                                    .and_then(|resolve| resolve(sid, token.0))
                            });
                            if let Some(cpu) = cpu {
                                self.tinyskia.composite_rgba_frame(
                                    &cpu.pixels,
                                    cpu.width,
                                    cpu.height,
                                    *rect,
                                    *clip,
                                );
                            }
                            // CPU compositing is synchronous — the
                            // frame is consumed before we release.
                            if let Err(err) = bridge.lock().release(sid, slot) {
                                tracing::warn!(
                                    error = %err,
                                    surface_id = sid.0,
                                    slot,
                                    "external CPU slot release failed"
                                );
                            }
                        }
                    }
                }
            }
        } else {
            self.mode = RenderMode::Gpu;
            // The frame clear is consumed by `dispatch_pending`'s
            // phase-2 clear pass — update it for every GPU frame, not
            // just segmented ones, or it goes stale.
            self.gpu_clear = clear_mode;
            if paint_list.has_external() {
                // Segmented path: capture each span and external marker so
                // `render_to_surface` can interleave Vello dispatches with
                // `WgpuHost` composites in exact paint order. The
                // whole-list scene build is skipped — each segment is
                // translated individually at dispatch time.
                self.pending = paint_list
                    .segments()
                    .into_iter()
                    .map(|seg| match seg {
                        martensite_render::PaintSegment::Commands(cmds) => {
                            PendingSegment::Commands(martensite_render::PaintList {
                                commands: cmds.to_vec(),
                            })
                        }
                        martensite_render::PaintSegment::External {
                            surface_id,
                            rect,
                            clip,
                        } => PendingSegment::External {
                            surface_id,
                            rect,
                            clip,
                        },
                    })
                    .collect();
            } else {
                self.pending.clear();
                self.vello.render_with_clear(paint_list, clear_mode);
            }
        }
    }

    /// Forces the CPU backend for the next frame, regardless of config
    /// or recovery state.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    /// use martensite_render::PaintList;
    ///
    /// # fn example(orchestrator: &mut RenderOrchestrator) {
    /// let list = PaintList::new();
    /// orchestrator.force_cpu(&list);
    /// assert_eq!(orchestrator.mode(), martensite_wgpu::orchestrator::RenderMode::Cpu);
    /// # }
    /// ```
    pub fn force_cpu(&mut self, paint_list: &PaintList) {
        self.mode = RenderMode::Cpu;
        self.tinyskia.render(paint_list);
    }

    /// Returns the active backdrop mode.
    ///
    /// The backdrop mode determines whether the renderer clears to
    /// transparent (for system materials like Mica/Acrylic/vibrancy)
    /// or opaque (for solid backgrounds). The render backend already
    /// clears to transparent black by default; this field lets the
    /// application query the active mode and decide whether to paint
    /// a background fill rect.
    ///
    /// When [`crate::surface::BackdropMode::Transparent`], the application should
    /// skip its background fill rect so the system material shows
    /// through. When [`crate::surface::BackdropMode::Opaque`], the application
    /// should paint a solid background.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    /// use martensite_wgpu::surface::BackdropMode;
    ///
    /// # fn example(orchestrator: &mut RenderOrchestrator) {
    /// assert_eq!(orchestrator.backdrop_mode(), BackdropMode::Opaque);
    /// orchestrator.set_backdrop_mode(BackdropMode::Transparent);
    /// assert_eq!(orchestrator.backdrop_mode(), BackdropMode::Transparent);
    /// # }
    /// ```
    #[must_use]
    pub fn backdrop_mode(&self) -> crate::surface::BackdropMode {
        self.backdrop_mode
    }

    /// Sets the active backdrop mode.
    ///
    /// This should be called whenever the system backdrop material
    /// changes (e.g. via the shell's `BackdropController::mode`). The
    /// orchestrator uses the active mode to choose the clear color for
    /// the next frame: [`crate::surface::BackdropMode::Transparent`]
    /// clears to transparent black so system materials show through;
    /// [`crate::surface::BackdropMode::Opaque`] clears to opaque black.
    ///
    /// To keep the wgpu surface's `alpha_mode` in sync with this mode,
    /// prefer [`configure_surface`](Self::configure_surface) over
    /// calling this method and
    /// [`SurfaceWrapper::configure`](crate::surface::SurfaceWrapper::configure)
    /// separately.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    /// use martensite_wgpu::surface::BackdropMode;
    ///
    /// # fn example(orchestrator: &mut RenderOrchestrator) {
    /// orchestrator.set_backdrop_mode(BackdropMode::Transparent);
    /// assert_eq!(orchestrator.backdrop_mode(), BackdropMode::Transparent);
    /// # }
    /// ```
    pub fn set_backdrop_mode(&mut self, mode: crate::surface::BackdropMode) {
        self.backdrop_mode = mode;
    }

    /// Configures the wgpu surface and updates the orchestrator's
    /// backdrop mode in a single call.
    ///
    /// This is the preferred way to apply a backdrop change: it sets
    /// [`set_backdrop_mode`](Self::set_backdrop_mode) so the next
    /// [`render`](Self::render) clears with the right color, *and*
    /// reconfigures the [`SurfaceWrapper`] so the swapchain uses the
    /// matching `CompositeAlphaMode` (Opaque vs PreMultiplied). Calling
    /// the two separately risks a frame where the clear color and the
    /// surface alpha mode disagree.
    ///
    /// # Errors
    ///
    /// Forwards [`SurfaceWrapper::configure`](crate::surface::SurfaceWrapper::configure)
    /// errors.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    /// use martensite_wgpu::surface::{BackdropMode, SurfaceWrapper};
    /// # use wgpu::Instance;
    /// # fn example(
    /// #     orchestrator: &mut RenderOrchestrator,
    /// #     device: &wgpu::Device,
    /// #     adapter: &wgpu::Adapter,
    /// #     surface: &mut SurfaceWrapper<'_>,
    /// # ) {
    /// orchestrator.configure_surface(device, adapter, surface, 800, 600, BackdropMode::Transparent);
    /// assert_eq!(orchestrator.backdrop_mode(), BackdropMode::Transparent);
    /// # }
    /// ```
    pub fn configure_surface(
        &mut self,
        device: &wgpu::Device,
        adapter: &wgpu::Adapter,
        surface: &mut crate::surface::SurfaceWrapper<'_>,
        width: u32,
        height: u32,
        mode: crate::surface::BackdropMode,
    ) -> Result<(), crate::surface::SurfaceWrapperError> {
        self.backdrop_mode = mode;
        surface.configure(device, adapter, width, height, mode)
    }

    /// Renders the most recently built frame to a WGPU surface and presents it.
    ///
    /// This is the WGPU surface dispatch path that was missing from the v0.2.0
    /// render pipeline. It must be called *after* [`RenderOrchestrator::render`]
    /// (which builds the Vello scene or rasterizes the TinySkia pixel buffer).
    /// It:
    ///
    /// 1. Acquires a frame from `surface` via [`SurfaceWrapper::acquire_frame`].
    /// 2. If the GPU (Vello) mode is active, creates a view of the surface
    ///    texture and dispatches the scene to the GPU compute pipeline via
    ///    [`VelloRenderer::render_to_texture`].
    /// 3. If the CPU (TinySkia) mode is active, uploads the rasterized RGBA8
    ///    pixel buffer to the surface texture via [`wgpu::Queue::write_texture`]
    ///    (which internally stages the data in a buffer), swizzling R/B channels
    ///    when the surface format is `Bgra8Unorm`.
    /// 4. Submits any queued work to `queue` and presents the frame.
    ///
    /// # Errors
    ///
    /// Returns a [`SurfaceError`] when the surface frame could not be acquired
    /// (e.g. `Lost`, `Outdated`, `Timeout`). The caller should feed this error
    /// into the [`RecoveryMachine`] via [`RecoveryMachine::handle_surface_error`]
    /// so the self-healing pipeline can react.
    ///
    /// # Note on the Vello path
    ///
    /// `vello::Renderer::render_to_texture` requires the target texture to use
    /// the `Rgba8Unorm` format with the `STORAGE_BINDING` usage. The surface is
    /// configured by [`SurfaceWrapper`] with the platform-preferred format
    /// (commonly `Bgra8Unorm`) and `RENDER_ATTACHMENT | COPY_DST` usage, so a
    /// direct dispatch to the surface texture may be rejected by Vello at
    /// runtime; in that case the error is logged (see
    /// [`VelloRenderer::render_to_texture`]) and the frame is still presented.
    /// A production deployment that wants the Vello path should configure the
    /// surface with `Rgba8Unorm` + `STORAGE_BINDING`, or render to an
    /// intermediate texture and blit (deferred to a follow-up).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    /// use martensite_wgpu::surface::SurfaceWrapper;
    ///
    /// # fn example(
    /// #     orchestrator: &mut RenderOrchestrator,
    /// #     device: &wgpu::Device,
    /// #     queue: &wgpu::Queue,
    /// #     surface: &mut SurfaceWrapper<'_>,
    /// # ) {
    /// orchestrator.render_to_surface(device, queue, surface).expect("frame presented");
    /// # }
    /// ```
    pub fn render_to_surface(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface: &mut SurfaceWrapper<'_>,
    ) -> Result<(), SurfaceError> {
        let current = surface.acquire_frame();
        if let Some(err) = SurfaceError::from_current_texture(&current) {
            return Err(err);
        }
        // `from_current_texture` returns `None` only for `Success`/`Suboptimal`,
        // so the remaining variants are unreachable here.
        let st = match current {
            wgpu::CurrentSurfaceTexture::Success(st)
            | wgpu::CurrentSurfaceTexture::Suboptimal(st) => st,
            _ => return Err(SurfaceError::Lost),
        };

        let (width, height, format) = surface
            .configuration()
            .map(|c| (c.width, c.height, c.format))
            .ok_or(SurfaceError::Lost)?;

        match self.mode {
            RenderMode::Gpu => {
                #[cfg(feature = "vello")]
                {
                    let view = st
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    // Surface textures are never Vello-compatible
                    // (RENDER_ATTACHMENT | COPY_DST only, typically
                    // sRGB): the offscreen-segment path always applies.
                    self.dispatch_pending(
                        device,
                        queue,
                        DispatchTarget {
                            view: &view,
                            format,
                            width,
                            height,
                            vello_direct: false,
                        },
                    );
                }
                #[cfg(not(feature = "vello"))]
                {
                    let _ = (device, queue, width, height);
                    tracing::warn!(
                        "GPU render mode selected but the `vello` feature is disabled; \
                         presenting an empty frame"
                    );
                }
            }
            RenderMode::Cpu => {
                let pixels = self.tinyskia.pixels();
                let expected = (width as usize)
                    .checked_mul(height as usize)
                    .and_then(|n| n.checked_mul(4));
                if expected != Some(pixels.len()) {
                    tracing::warn!(
                        pixel_len = pixels.len(),
                        expected = expected,
                        width,
                        height,
                        "TinySkia pixel buffer size does not match the surface dimensions; \
                         skipping CPU upload"
                    );
                } else {
                    let bytes_per_row = width.checked_mul(4).ok_or(SurfaceError::Validation)?;
                    // `Queue::write_texture` requires the data to match the
                    // surface's texel format. TinySkia produces RGBA8, so
                    // swizzle R<->B when the surface is BGRA8.
                    let data: Cow<'_, [u8]> = match format {
                        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => {
                            Cow::Borrowed(pixels)
                        }
                        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
                            let mut swizzled = pixels.to_vec();
                            for chunk in swizzled.as_chunks_mut::<4>().0 {
                                chunk.swap(0, 2);
                            }
                            Cow::Owned(swizzled)
                        }
                        _ => {
                            tracing::warn!(
                                ?format,
                                "unsupported surface format for CPU pixel upload; \
                                 writing RGBA8 data as-is"
                            );
                            Cow::Borrowed(pixels)
                        }
                    };
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &st.texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        &data,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(bytes_per_row),
                            rows_per_image: Some(height),
                        },
                        wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                    );
                    // Flush the queued `write_texture` transfer before
                    // presenting so the uploaded pixels are visible this frame.
                    queue.submit(std::iter::empty::<wgpu::CommandBuffer>());
                }
            }
        }

        if let Some(notify) = &self.pre_present_notify {
            notify();
        }
        queue.present(st);
        Ok(())
    }

    /// Renders the most recently built Vello scene to an offscreen texture
    /// and reads back the pixels as premultiplied RGBA8.
    ///
    /// This is the headless GPU readback path: it creates an offscreen
    /// [`wgpu::Texture`] with the [`wgpu::TextureFormat::Rgba8Unorm`] format
    /// and `STORAGE_BINDING | COPY_SRC` usage, dispatches the Vello scene into
    /// it, copies the texture to a staging buffer, maps the buffer, and returns
    /// the RGBA8 bytes. The returned buffer matches the layout of
    /// [`TinySkiaBackend::pixels`]: premultiplied RGBA8, row-major, tightly
    /// packed (no row padding), `width * height * 4` bytes.
    ///
    /// `render_to_buffer` must be called *after* [`RenderOrchestrator::render`]
    /// (which builds the Vello scene in GPU mode). The caller is responsible
    /// for ensuring the GPU mode is active (e.g. via the default config or by
    /// not forcing CPU).
    ///
    /// The readback is synchronous: it uses [`wgpu::Device::poll`] with
    /// [`wgpu::PollType::Wait`] to block until the render and the buffer map
    /// have completed. Row padding required by `wgpu`'s 256-byte
    /// `COPY_BYTES_PER_ROW_ALIGNMENT` is stripped, so the returned buffer is
    /// tightly packed regardless of `width`.
    ///
    /// # Errors
    ///
    /// Returns [`OrchestratorError::GpuReadbackFailed`] if the offscreen
    /// texture, the staging buffer, the Vello dispatch, or the buffer map
    /// fails. This is expected on systems without a usable GPU adapter; the
    /// parity test gates this behind `#[ignore]` and a Lavapipe CI job.
    ///
    /// Only available when the `vello` feature is enabled.
    #[cfg(feature = "vello")]
    pub fn render_to_buffer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, OrchestratorError> {
        use std::sync::mpsc;

        if width == 0 || height == 0 {
            return Err(OrchestratorError::GpuReadbackFailed(
                "zero-sized offscreen target".to_string(),
            ));
        }

        // 1. Offscreen render target: Rgba8Unorm, STORAGE_BINDING for the
        //    Vello compute dispatch, COPY_SRC for the texture→buffer copy.
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("martensite-offscreen-render-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            // RENDER_ATTACHMENT is needed by the external-surface
            // composite pass when the paint list contains `External`
            // markers; STORAGE_BINDING | COPY_SRC cover Vello + readback.
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());

        // 2. Dispatch the scene (segmented when external markers exist).
        // The offscreen target is Rgba8Unorm + STORAGE_BINDING — Vello
        // can write it directly when no external markers are present.
        self.dispatch_pending(
            device,
            queue,
            DispatchTarget {
                view: &view,
                format: wgpu::TextureFormat::Rgba8Unorm,
                width,
                height,
                vello_direct: true,
            },
        );

        // 3. Staging buffer: MAP_READ | COPY_DST. `bytes_per_row` must be a
        //    multiple of wgpu's COPY_BYTES_PER_ROW_ALIGNMENT (256), so the
        //    buffer is padded per row; the padding is stripped on readback.
        const ALIGNMENT: u32 = 256;
        let bytes_per_pixel = 4u32;
        let unpadded_bytes_per_row = width
            .checked_mul(bytes_per_pixel)
            .ok_or_else(|| OrchestratorError::GpuReadbackFailed("row overflow".to_string()))?;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(ALIGNMENT) * ALIGNMENT;
        let buffer_size = (padded_bytes_per_row as u64)
            .checked_mul(height as u64)
            .ok_or_else(|| OrchestratorError::GpuReadbackFailed("buffer overflow".to_string()))?;

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("martensite-offscreen-readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 4. Encode the texture→buffer copy and submit.
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        // 5. Map the staging buffer for read. The callback fires during the
        //    blocking poll below; a channel bridges the async callback to the
        //    synchronous caller.
        let (tx, rx) = mpsc::channel();
        staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| OrchestratorError::GpuReadbackFailed(e.to_string()))?;
        rx.recv()
            .map_err(|e| OrchestratorError::GpuReadbackFailed(e.to_string()))?
            .map_err(|e| OrchestratorError::GpuReadbackFailed(e.to_string()))?;

        // 6. Read the mapped range and strip per-row padding.
        let pixels = {
            let view = staging
                .slice(..)
                .get_mapped_range()
                .map_err(|e| OrchestratorError::GpuReadbackFailed(e.to_string()))?;
            let mut out =
                Vec::with_capacity((width as usize) * (height as usize) * bytes_per_pixel as usize);
            let row_bytes = unpadded_bytes_per_row as usize;
            let padded = padded_bytes_per_row as usize;
            for row in 0..height as usize {
                let start = row * padded;
                out.extend_from_slice(&view[start..start + row_bytes]);
            }
            out
        };
        staging.unmap();
        Ok(pixels)
    }

    /// Returns the current rendering mode.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    ///
    /// let orch = RenderOrchestrator::with_default_config(64, 64).unwrap();
    /// let _mode = orch.mode();
    /// ```
    #[must_use]
    pub fn mode(&self) -> RenderMode {
        self.mode
    }

    /// Returns a reference to the Vello renderer.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    ///
    /// let orch = RenderOrchestrator::with_default_config(64, 64).unwrap();
    /// let _vello = orch.vello();
    /// ```
    #[must_use]
    pub fn vello(&self) -> &VelloRenderer {
        &self.vello
    }

    /// Returns a mutable reference to the Vello renderer.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    ///
    /// let mut orch = RenderOrchestrator::with_default_config(64, 64).unwrap();
    /// let _vello = orch.vello_mut();
    /// ```
    #[must_use]
    pub fn vello_mut(&mut self) -> &mut VelloRenderer {
        &mut self.vello
    }

    /// Returns a reference to the TinySkia backend.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    ///
    /// let orch = RenderOrchestrator::with_default_config(64, 64).unwrap();
    /// let _tinyskia = orch.tinyskia();
    /// ```
    #[must_use]
    pub fn tinyskia(&self) -> &TinySkiaBackend {
        &self.tinyskia
    }

    /// Returns a mutable reference to the TinySkia backend.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    ///
    /// let mut orch = RenderOrchestrator::with_default_config(64, 64).unwrap();
    /// let _tinyskia = orch.tinyskia_mut();
    /// ```
    #[must_use]
    pub fn tinyskia_mut(&mut self) -> &mut TinySkiaBackend {
        &mut self.tinyskia
    }

    /// Returns the CPU pixel buffer from the TinySkia backend.
    ///
    /// This is the RGBA8 buffer that can be presented via
    /// [`martensite_render::present_rgba_to_softbuffer`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    ///
    /// let orch = RenderOrchestrator::with_default_config(64, 64).unwrap();
    /// let _pixels = orch.cpu_pixels();
    /// ```
    #[must_use]
    pub fn cpu_pixels(&self) -> &[u8] {
        self.tinyskia.pixels()
    }

    /// Returns the orchestrator configuration.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    ///
    /// let orch = RenderOrchestrator::with_default_config(64, 64).unwrap();
    /// assert!(!orch.config().prefer_cpu);
    /// ```
    #[must_use]
    pub fn config(&self) -> &OrchestratorConfig {
        &self.config
    }

    /// Installs (or removes) the [`WgpuHost`](crate::external::WgpuHost)
    /// used to composite `PaintCommand::External` markers.
    ///
    /// The host's pipelines are created for a specific target format;
    /// construct it with the surface's configured format before calling
    /// this method.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    /// # fn example(orchestrator: &mut RenderOrchestrator) {
    /// assert!(orchestrator.external_host().is_none());
    /// # }
    /// ```
    pub fn set_external_host(&mut self, host: Option<crate::external::WgpuHost>) {
        self.external_host = host;
    }

    /// Returns the installed external host, if any.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    /// # fn example(orchestrator: &mut RenderOrchestrator) {
    /// assert!(orchestrator.external_host().is_none());
    /// # }
    /// ```
    pub fn external_host(&self) -> Option<&crate::external::WgpuHost> {
        self.external_host.as_ref()
    }

    /// Returns a mutable reference to the external host (e.g. to call
    /// `register_texture` when a producer's ring acquires a slot).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    /// # fn example(orchestrator: &mut RenderOrchestrator) {
    /// assert!(orchestrator.external_host_mut().is_none());
    /// # }
    /// ```
    pub fn external_host_mut(&mut self) -> Option<&mut crate::external::WgpuHost> {
        self.external_host.as_mut()
    }

    /// Installs the engine bridge `PaintCommand::External` markers are
    /// resolved through.
    ///
    /// With a bridge installed, `dispatch_pending` takes each marker's
    /// front frame from the ring — [`BridgeRegistry::take_front`],
    /// composite, then [`BridgeRegistry::release`] after the composite
    /// encoder is submitted — so the producer never writes the texture
    /// the host is sampling. Without a bridge the marker falls back to
    /// the statically registered [`WgpuHost::register_texture`] entry.
    ///
    /// [`BridgeRegistry::take_front`]: martensite_engine_bridge::BridgeRegistry::take_front
    /// [`BridgeRegistry::release`]: martensite_engine_bridge::BridgeRegistry::release
    /// [`WgpuHost::register_texture`]: crate::external::WgpuHost::register_texture
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    /// use martensite_engine_bridge::BridgeHandle;
    /// # fn example(orchestrator: &mut RenderOrchestrator) {
    /// orchestrator.set_bridge(BridgeHandle::new());
    /// # }
    /// ```
    pub fn set_bridge(&mut self, bridge: martensite_engine_bridge::BridgeHandle) {
        self.bridge = Some(bridge);
    }

    /// The installed bridge handle, if any.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    /// # fn example(orchestrator: &mut RenderOrchestrator) {
    /// assert!(orchestrator.bridge_handle().is_none());
    /// # }
    /// ```
    pub fn bridge_handle(&self) -> Option<&martensite_engine_bridge::BridgeHandle> {
        self.bridge.as_ref()
    }

    /// Installs the callback invoked immediately before
    /// `SurfaceTexture::present` — the milestone's `pre_present_notify`
    /// pacing contract.
    ///
    /// The application typically wires `winit`'s
    /// `Window::pre_present_notify` here so the platform compositor is
    /// notified before the frame lands:
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    /// # fn example(orchestrator: &mut RenderOrchestrator) {
    /// // Inside the app, wire winit's `Window::pre_present_notify`:
    /// // `orchestrator.set_pre_present_notify(Some(Box::new(move || window.pre_present_notify())))`.
    /// orchestrator.set_pre_present_notify(Some(Box::new(|| {})));
    /// # }
    /// ```
    pub fn set_pre_present_notify(&mut self, notify: Option<Box<dyn Fn() + Send>>) {
        self.pre_present_notify = notify;
    }

    /// Installs the CPU-fallback frame resolver used when a ring's
    /// front slot carries no `CpuFrame`.
    ///
    /// Wire `martensite::widgets::external::ExternalEngines::cpu_frame_for`
    /// here (usually behind a shared `Mutex`) so the TinySkia path can
    /// ask each bound `Engine` for `to_pixmap(token)` on demand — the
    /// spec's `Engine::to_pixmap` fallback contract.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_wgpu::orchestrator::RenderOrchestrator;
    /// # fn example(orchestrator: &mut RenderOrchestrator) {
    /// orchestrator.set_cpu_frame_resolver(Some(Box::new(|_surface, _token| None)));
    /// # }
    /// ```
    pub fn set_cpu_frame_resolver(&mut self, resolver: Option<CpuFrameResolver>) {
        self.cpu_frame_resolver = resolver;
    }

    /// Ensures the installed [`WgpuHost`] exists and was built for
    /// `target_format`, lazily creating or recreating it.
    #[cfg(feature = "vello")]
    fn ensure_host(&mut self, device: &wgpu::Device, target_format: wgpu::TextureFormat) {
        match &self.external_host {
            Some(host) if host.target_format() == target_format => {}
            _ => {
                if self.external_host.is_some() {
                    tracing::warn!(
                        "surface format changed; recreating WgpuHost (registered surfaces dropped)"
                    );
                }
                self.external_host = Some(crate::external::WgpuHost::new(device, target_format));
                // Cached segment bind groups reference the old host's
                // bind-group layout — rebuild the pool.
                #[cfg(feature = "vello")]
                self.seg_pool.clear();
            }
        }
    }

    /// Ensures segment texture `i` exists at `width`×`height`, growing
    /// the pool and recreating textures when the size changes.
    ///
    /// Segment textures are `Rgba8Unorm` — Vello's storage format —
    /// with `Rgba8UnormSrgb` in `view_formats` so the composite pass
    /// can sample through a view whose sRGB-ness matches the frame
    /// target.
    #[cfg(feature = "vello")]
    fn ensure_segment(&mut self, device: &wgpu::Device, i: usize, width: u32, height: u32) {
        while self.seg_pool.len() <= i {
            self.seg_pool.push(None);
        }
        let stale = match &self.seg_pool[i] {
            Some(entry) => entry.texture.width() != width || entry.texture.height() != height,
            None => true,
        };
        if stale {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("vello-segment-texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
            });
            let storage_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.seg_pool[i] = Some(SegEntry {
                texture,
                storage_view,
                sample: None,
            });
        }
    }

    /// Dispatches the pending paint work into `target`.
    ///
    /// `vello::Renderer::render_to_texture` overwrites its whole target
    /// with the base color (a compute store, not a blend), so segments
    /// cannot be composited by sequential Vello dispatches into one
    /// target. Instead each command span renders into its own offscreen
    /// `Rgba8Unorm` texture with a transparent base, and all textures
    /// (Vello segments *and* external surfaces) are composited in one
    /// encoder — preceded by an explicit frame-clear pass honoring the
    /// configured clear mode — in exact paint order, via [`WgpuHost`].
    ///
    /// `vello_direct` marks targets that Vello can write directly
    /// (`Rgba8Unorm` + `STORAGE_BINDING`, e.g. the offscreen readback
    /// texture): when the frame contains no `External` markers the
    /// historical single dispatch is used and no blit pass is needed.
    #[cfg(feature = "vello")]
    fn dispatch_pending(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: DispatchTarget<'_>,
    ) {
        let DispatchTarget {
            view: target,
            format: target_format,
            width,
            height,
            vello_direct,
        } = target;
        if self.pending.is_empty() && vello_direct {
            self.vello
                .render_to_texture(device, queue, target, width, height);
            return;
        }

        self.ensure_host(device, target_format);
        let target_is_srgb = self
            .external_host
            .as_ref()
            .expect("host just ensured")
            .target_is_srgb();
        let pending = std::mem::take(&mut self.pending);

        // Phase 1 — render every command span into its own segment
        // texture. Vello submits each dispatch internally; the writes
        // land on the queue before the composite encoder submitted in
        // phase 2, so the textures are complete when sampled.
        let single_scene = pending.is_empty();
        let work: Vec<PendingSegment> = if single_scene {
            vec![PendingSegment::Commands(martensite_render::PaintList::new())]
        } else {
            pending
        };
        let mut seg_views: Vec<Option<usize>> = Vec::with_capacity(work.len());
        let mut seg_index = 0usize;
        for segment in &work {
            if let PendingSegment::Commands(list) = segment {
                self.ensure_segment(device, seg_index, width, height);
                {
                    let storage_view = &self.seg_pool[seg_index]
                        .as_ref()
                        .expect("segment just created")
                        .storage_view;
                    if single_scene {
                        // The scene is already built — dispatch as-is.
                        self.vello
                            .render_to_texture(device, queue, storage_view, width, height);
                    } else {
                        // Every segment uses a transparent base: the
                        // frame itself is cleared in phase 2, and
                        // transparent bases let lower content (cleared
                        // backdrop, earlier segments, external
                        // surfaces) show through correctly.
                        self.vello
                            .render_with_clear(list, martensite_render::ClearMode::Transparent);
                        self.vello
                            .render_to_texture(device, queue, storage_view, width, height);
                    }
                }
                // Sample view + bind group are cached on the entry:
                // decode-on-sample when the target encodes on store
                // (sRGB), raw pass-through otherwise — either way the
                // bytes round-trip. Rebuilt only when the sRGB-ness of
                // the host's target changes.
                let entry = self.seg_pool[seg_index]
                    .as_mut()
                    .expect("segment just created");
                if entry.sample.as_ref().map(|(s, _, _)| *s) != Some(target_is_srgb) {
                    let sample_format = if target_is_srgb {
                        wgpu::TextureFormat::Rgba8UnormSrgb
                    } else {
                        wgpu::TextureFormat::Rgba8Unorm
                    };
                    let sample_view = entry.texture.create_view(&wgpu::TextureViewDescriptor {
                        format: Some(sample_format),
                        ..Default::default()
                    });
                    let bind_group = self
                        .external_host
                        .as_ref()
                        .expect("host just ensured")
                        .segment_bind_group(device, &sample_view);
                    entry.sample = Some((target_is_srgb, sample_view, bind_group));
                }
                seg_views.push(Some(seg_index));
                seg_index += 1;
            } else {
                seg_views.push(None);
            }
        }

        // Phase 2 — one encoder, one ordered sequence of composites.
        // The frame starts with an explicit clear: acquired surface
        // textures have undefined contents, so a transparent-clear
        // frame would otherwise composite over garbage.
        self.external_host
            .as_ref()
            .expect("host just ensured")
            .begin_frame();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("external-composite-encoder"),
        });
        let clear_color = match self.gpu_clear {
            martensite_render::ClearMode::Opaque(c) => wgpu::Color {
                r: f64::from(c[0]),
                g: f64::from(c[1]),
                b: f64::from(c[2]),
                a: f64::from(c[3]),
            },
            martensite_render::ClearMode::Transparent => wgpu::Color::TRANSPARENT,
        };
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("frame-clear-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
        // The registry lock is taken per-call inside `composite_front`
        // (ring bookkeeping only) — producers are never blocked for the
        // duration of the composite pass.
        let mut releases: Vec<(martensite_engine_bridge::SurfaceId, u8)> = Vec::new();
        for (i, segment) in work.iter().enumerate() {
            match segment {
                PendingSegment::Commands(_) => {
                    let Some(seg_i) = seg_views[i] else {
                        tracing::warn!("segment {i} has no view — skipped");
                        continue;
                    };
                    let Some(entry) = self.seg_pool[seg_i].as_ref() else {
                        tracing::warn!("segment {i} missing pool entry — skipped");
                        continue;
                    };
                    let Some((_, _, bind_group)) = entry.sample.as_ref() else {
                        tracing::warn!("segment {i} has no cached bind group — skipped");
                        continue;
                    };
                    match &self.external_host {
                        Some(host) => {
                            match host.record_segment_composite(
                                crate::external::CompositeTarget {
                                    encoder: &mut encoder,
                                    queue,
                                    view: target,
                                    size: (width, height),
                                },
                                bind_group,
                                [0.0, 0.0, width as f32, height as f32],
                                [0.0, 0.0, width as f32, height as f32],
                            ) {
                                Ok(()) => {}
                                Err(err) => {
                                    tracing::warn!(error = %err, "segment blit skipped");
                                }
                            }
                        }
                        None => tracing::warn!("no external host — segment skipped"),
                    }
                }
                PendingSegment::External {
                    surface_id,
                    rect,
                    clip,
                } => {
                    let Some(host) = &self.external_host else {
                        tracing::warn!("no external host — marker skipped");
                        continue;
                    };
                    let sid = *surface_id;
                    // Prefer the ring: take the front frame and release
                    // after submit. Fall back to the statically
                    // registered texture when the ring is absent or has
                    // no front frame.
                    let mut took = false;
                    if let Some(bridge) = &self.bridge {
                        match host.composite_front(
                            device,
                            bridge,
                            crate::external::CompositeTarget {
                                encoder: &mut encoder,
                                queue,
                                view: target,
                                size: (width, height),
                            },
                            sid,
                            *rect,
                            *clip,
                        ) {
                            Ok(Some(taken)) => {
                                releases.push((sid, taken.slot));
                                took = true;
                            }
                            Ok(None) => {}
                            Err(err) => {
                                tracing::warn!(
                                    error = %err,
                                    surface_id = surface_id.0,
                                    "external ring composite skipped"
                                );
                            }
                        }
                    }
                    if !took {
                        match host.composite(
                            crate::external::CompositeTarget {
                                encoder: &mut encoder,
                                queue,
                                view: target,
                                size: (width, height),
                            },
                            sid,
                            *rect,
                            *clip,
                        ) {
                            Ok(()) => {}
                            Err(err) => {
                                tracing::warn!(
                                    error = %err,
                                    surface_id = surface_id.0,
                                    "external composite skipped"
                                );
                            }
                        }
                    }
                }
            }
        }
        // Always submitted — the initial clear pass alone already
        // requires it.
        queue.submit([encoder.finish()]);
        // Release composited slots AFTER the submit: same-queue
        // ordering guarantees the producer's next write to the freed
        // texture lands after this frame's composite sample.
        if !releases.is_empty() {
            if let Some(bridge) = &self.bridge {
                let mut reg = bridge.lock();
                for (sid, slot) in releases {
                    if let Err(err) = reg.release(sid, slot) {
                        tracing::warn!(
                            error = %err,
                            surface_id = sid.0,
                            slot,
                            "external slot release failed"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_render::Rect;
    use std::time::{Duration, Instant};

    #[test]
    fn new_with_default_config_uses_gpu() {
        let orchestrator = RenderOrchestrator::with_default_config(100, 100).expect("init");
        assert_eq!(orchestrator.mode(), RenderMode::Gpu);
    }

    #[test]
    fn new_with_prefer_cpu_uses_cpu() {
        let config = OrchestratorConfig {
            prefer_cpu: true,
            allow_software_fallback: false,
        };
        let orchestrator = RenderOrchestrator::new(100, 100, config).expect("init");
        assert_eq!(orchestrator.mode(), RenderMode::Cpu);
    }

    #[test]
    fn new_with_zero_dims_fails() {
        let result = RenderOrchestrator::with_default_config(0, 0);
        assert!(result.is_err());
    }

    #[test]
    fn render_uses_gpu_when_healthy() {
        let orchestrator = RenderOrchestrator::with_default_config(100, 100).expect("init");
        let recovery = RecoveryMachine::new();
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 50.0, 50.0), [255, 0, 0, 255]);
        let mut orchestrator = orchestrator;
        orchestrator.render(&list, &recovery);
        assert_eq!(orchestrator.mode(), RenderMode::Gpu);
    }

    #[test]
    fn render_uses_cpu_when_fallback_and_allowed() {
        let config = OrchestratorConfig {
            allow_software_fallback: true,
            prefer_cpu: false,
        };
        let mut orchestrator = RenderOrchestrator::new(100, 100, config).expect("init");
        let mut recovery = RecoveryMachine::with_policy(1, Duration::from_secs(60));
        use crate::resilience::SurfaceError;
        let t0 = Instant::now();
        recovery.handle_surface_error_at(SurfaceError::Lost, t0);
        recovery.begin_retry_at(t0);
        recovery.retry_failed_at(t0);
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 50.0, 50.0), [255, 0, 0, 255]);
        orchestrator.render(&list, &recovery);
        assert_eq!(orchestrator.mode(), RenderMode::Cpu);
    }

    #[test]
    fn render_does_not_use_cpu_when_fallback_not_allowed() {
        let config = OrchestratorConfig {
            allow_software_fallback: false,
            prefer_cpu: false,
        };
        let mut orchestrator = RenderOrchestrator::new(100, 100, config).expect("init");
        let mut recovery = RecoveryMachine::with_policy(1, Duration::from_secs(60));
        use crate::resilience::SurfaceError;
        let t0 = Instant::now();
        recovery.handle_surface_error_at(SurfaceError::Lost, t0);
        recovery.begin_retry_at(t0);
        recovery.retry_failed_at(t0);
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 50.0, 50.0), [255, 0, 0, 255]);
        orchestrator.render(&list, &recovery);
        // Without allow_software_fallback, stays on GPU even during recovery
        assert_eq!(orchestrator.mode(), RenderMode::Gpu);
    }

    #[test]
    fn render_uses_cpu_when_prefer_cpu_set() {
        let config = OrchestratorConfig {
            prefer_cpu: true,
            allow_software_fallback: false,
        };
        let mut orchestrator = RenderOrchestrator::new(100, 100, config).expect("init");
        let recovery = RecoveryMachine::new();
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 50.0, 50.0), [255, 0, 0, 255]);
        orchestrator.render(&list, &recovery);
        assert_eq!(orchestrator.mode(), RenderMode::Cpu);
    }

    #[test]
    fn force_cpu_overrides_config_and_recovery() {
        let mut orchestrator = RenderOrchestrator::with_default_config(100, 100).expect("init");
        let recovery = RecoveryMachine::new();
        let _ = recovery.status();
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 50.0, 50.0), [255, 0, 0, 255]);
        orchestrator.force_cpu(&list);
        assert_eq!(orchestrator.mode(), RenderMode::Cpu);
    }

    #[test]
    fn cpu_pixels_returns_tinyskia_buffer() {
        let orchestrator = RenderOrchestrator::with_default_config(10, 10).expect("init");
        let pixels = orchestrator.cpu_pixels();
        assert_eq!(pixels.len(), 400);
    }

    #[test]
    fn config_accessor_returns_config() {
        let config = OrchestratorConfig {
            allow_software_fallback: true,
            prefer_cpu: true,
        };
        let orchestrator = RenderOrchestrator::new(100, 100, config.clone()).expect("init");
        assert!(orchestrator.config().allow_software_fallback);
        assert!(orchestrator.config().prefer_cpu);
    }

    #[test]
    fn vello_accessors_return_renderer() {
        let orchestrator = RenderOrchestrator::with_default_config(100, 100).expect("init");
        let _ = orchestrator.vello().last_command_count();
    }

    #[test]
    fn tinyskia_accessors_return_backend() {
        let orchestrator = RenderOrchestrator::with_default_config(100, 100).expect("init");
        let _ = orchestrator.tinyskia().width();
    }

    #[test]
    fn render_empty_list_works() {
        let mut orchestrator = RenderOrchestrator::with_default_config(100, 100).expect("init");
        let recovery = RecoveryMachine::new();
        let list = PaintList::new();
        orchestrator.render(&list, &recovery);
    }

    #[test]
    fn orchestrator_error_display() {
        assert_eq!(
            OrchestratorError::NoBackend.to_string(),
            "no rendering backend available"
        );
        assert_eq!(
            OrchestratorError::BackendInitFailed.to_string(),
            "rendering backend initialization failed"
        );
    }

    #[test]
    fn orchestrator_config_default() {
        let config = OrchestratorConfig::default();
        assert!(!config.allow_software_fallback);
        assert!(!config.prefer_cpu);
    }
}
