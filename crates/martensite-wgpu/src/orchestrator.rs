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
        if use_cpu {
            self.mode = RenderMode::Cpu;
            self.tinyskia.render(paint_list);
        } else {
            self.mode = RenderMode::Gpu;
            self.vello.render(paint_list);
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
    /// changes (e.g. via the shell's `BackdropController::mode`).
    /// The orchestrator stores the mode and exposes it via
    /// [`backdrop_mode`](Self::backdrop_mode) so the application can
    /// decide whether to paint a background fill rect.
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
                    self.vello
                        .render_to_texture(device, queue, &view, width, height);
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
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());

        // 2. Dispatch the Vello scene into the offscreen texture.
        self.vello
            .render_to_texture(device, queue, &view, width, height);

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
    #[must_use]
    pub fn mode(&self) -> RenderMode {
        self.mode
    }

    /// Returns a reference to the Vello renderer.
    #[must_use]
    pub fn vello(&self) -> &VelloRenderer {
        &self.vello
    }

    /// Returns a mutable reference to the Vello renderer.
    #[must_use]
    pub fn vello_mut(&mut self) -> &mut VelloRenderer {
        &mut self.vello
    }

    /// Returns a reference to the TinySkia backend.
    #[must_use]
    pub fn tinyskia(&self) -> &TinySkiaBackend {
        &self.tinyskia
    }

    /// Returns a mutable reference to the TinySkia backend.
    #[must_use]
    pub fn tinyskia_mut(&mut self) -> &mut TinySkiaBackend {
        &mut self.tinyskia
    }

    /// Returns the CPU pixel buffer from the TinySkia backend.
    ///
    /// This is the RGBA8 buffer that can be presented via
    /// [`martensite_render::present_rgba_to_softbuffer`].
    #[must_use]
    pub fn cpu_pixels(&self) -> &[u8] {
        self.tinyskia.pixels()
    }

    /// Returns the orchestrator configuration.
    #[must_use]
    pub fn config(&self) -> &OrchestratorConfig {
        &self.config
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
