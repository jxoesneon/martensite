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

use crate::resilience::RecoveryMachine;
use martensite_render::{PaintList, RenderBackend, TinySkiaBackend, VelloRenderer};

/// The rendering mode currently active in the orchestrator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// GPU rendering via Vello compute pipeline.
    Gpu,
    /// CPU software rendering via TinySkia.
    Cpu,
}

/// Error returned by orchestrator operations.
#[derive(Debug)]
pub enum OrchestratorError {
    /// The GPU context is not available and CPU fallback is disabled.
    NoBackend,
    /// The TinySkia backend failed to initialize.
    BackendInitFailed,
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoBackend => write!(f, "no rendering backend available"),
            Self::BackendInitFailed => write!(f, "rendering backend initialization failed"),
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
#[derive(Debug, Clone, Default)]
pub struct OrchestratorConfig {
    /// Whether CPU software fallback is allowed.
    pub allow_software_fallback: bool,
    /// Whether to prefer the CPU backend even when a GPU is available.
    pub prefer_cpu: bool,
}

impl OrchestratorConfig {
    /// Creates a new config with the given settings.
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
pub struct RenderOrchestrator {
    /// The Vello GPU renderer (scene builder).
    vello: VelloRenderer,
    /// The TinySkia CPU fallback renderer.
    tinyskia: TinySkiaBackend,
    /// The current rendering mode.
    mode: RenderMode,
    /// The application configuration.
    config: OrchestratorConfig,
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
        })
    }

    /// Creates a new orchestrator with default configuration.
    ///
    /// # Errors
    ///
    /// Returns [`OrchestratorError::BackendInitFailed`] if the TinySkia
    /// backend cannot be initialized.
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
    pub fn force_cpu(&mut self, paint_list: &PaintList) {
        self.mode = RenderMode::Cpu;
        self.tinyskia.render(paint_list);
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
