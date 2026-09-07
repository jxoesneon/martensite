//! Render pipeline orchestrator.
//!
//! This module bridges [`martensite_render`] (the hardware-agnostic paint
//! command stream and backends) with [`crate`] (the WGPU device, surface,
//! and recovery state machine). The [`RenderOrchestrator`] owns a
//! [`martensite_render::VelloRenderer`] for GPU scene composition and a
//! [`martensite_render::TinySkiaBackend`] for CPU fallback, switching
//! between them based on the [`crate::resilience::RecoveryMachine`] state.
//!
//! When the GPU device is healthy, the orchestrator translates a
//! [`martensite_render::PaintList`] into a Vello `Scene` via the Vello
//! backend. When the recovery machine transitions to
//! [`crate::resilience::DeviceStatus::FallbackCpu`], the orchestrator
//! falls back to the TinySkia CPU rasterizer and presents via the
//! softbuffer presentation path.

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

/// Orchestrates GPU and CPU rendering backends with automatic fallback.
///
/// The orchestrator owns both a [`VelloRenderer`] (for GPU rendering) and
/// a [`TinySkiaBackend`] (for CPU fallback). It queries the
/// [`RecoveryMachine`] to determine which backend to use for each frame.
///
/// # Example
///
/// ```ignore
/// use martensite_wgpu::orchestrator::RenderOrchestrator;
/// use martensite_wgpu::resilience::RecoveryMachine;
/// use martensite_render::PaintList;
///
/// let mut recovery = RecoveryMachine::new();
/// let mut orchestrator = RenderOrchestrator::new(100, 100);
/// let paint_list = PaintList::new();
///
/// orchestrator.render(&paint_list, &recovery);
/// assert_eq!(orchestrator.mode(), martensite_wgpu::orchestrator::RenderMode::Gpu);
/// ```
pub struct RenderOrchestrator {
    /// The Vello GPU renderer.
    vello: VelloRenderer,
    /// The TinySkia CPU fallback renderer.
    tinyskia: TinySkiaBackend,
    /// The current rendering mode.
    mode: RenderMode,
}

impl RenderOrchestrator {
    /// Creates a new orchestrator with the given target dimensions.
    ///
    /// The TinySkia backend is initialized at the given width and height
    /// for CPU fallback. The Vello renderer starts with an empty scene.
    ///
    /// # Errors
    ///
    /// Returns [`OrchestratorError::BackendInitFailed`] if the TinySkia
    /// backend cannot be initialized (e.g. zero dimensions).
    pub fn new(width: u32, height: u32) -> Result<Self, OrchestratorError> {
        let tinyskia =
            TinySkiaBackend::new(width, height).ok_or(OrchestratorError::BackendInitFailed)?;
        Ok(Self {
            vello: VelloRenderer::new(),
            tinyskia,
            mode: RenderMode::Gpu,
        })
    }

    /// Renders a [`PaintList`] using the appropriate backend.
    ///
    /// If the recovery machine is in [`crate::resilience::DeviceStatus::FallbackCpu`], the
    /// TinySkia CPU backend is used. Otherwise, the Vello GPU backend
    /// is used.
    pub fn render(&mut self, paint_list: &PaintList, recovery: &RecoveryMachine) {
        if recovery.is_fallback_cpu() {
            self.mode = RenderMode::Cpu;
            self.tinyskia.render(paint_list);
        } else {
            self.mode = RenderMode::Gpu;
            self.vello.render(paint_list);
        }
    }

    /// Forces the CPU backend for the next frame, regardless of recovery state.
    ///
    /// This is used when `AppConfig::prefer_cpu()` is set.
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_render::Rect;

    #[test]
    fn new_creates_with_gpu_mode() {
        let orchestrator = RenderOrchestrator::new(100, 100).expect("init");
        assert_eq!(orchestrator.mode(), RenderMode::Gpu);
    }

    #[test]
    fn new_with_zero_dims_fails() {
        let result = RenderOrchestrator::new(0, 0);
        assert!(result.is_err());
    }

    #[test]
    fn render_uses_gpu_when_healthy() {
        let mut orchestrator = RenderOrchestrator::new(100, 100).expect("init");
        let recovery = RecoveryMachine::new();
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 50.0, 50.0), [255, 0, 0, 255]);
        orchestrator.render(&list, &recovery);
        assert_eq!(orchestrator.mode(), RenderMode::Gpu);
    }

    #[test]
    fn render_uses_cpu_when_fallback_active() {
        let mut orchestrator = RenderOrchestrator::new(100, 100).expect("init");
        let mut recovery = RecoveryMachine::with_policy(1, std::time::Duration::from_secs(60));
        use crate::resilience::SurfaceError;
        use std::time::Instant;
        let t0 = Instant::now();
        recovery.handle_surface_error_at(SurfaceError::Lost, t0);
        recovery.begin_retry_at(t0);
        recovery.retry_failed_at(t0); // exceeds max_retries=1 -> FallbackCpu
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 50.0, 50.0), [255, 0, 0, 255]);
        orchestrator.render(&list, &recovery);
        assert_eq!(orchestrator.mode(), RenderMode::Cpu);
    }

    #[test]
    fn force_cpu_overrides_recovery_state() {
        let mut orchestrator = RenderOrchestrator::new(100, 100).expect("init");
        let recovery = RecoveryMachine::new();
        let _ = recovery.status();
        let mut list = PaintList::new();
        list.push_fill_rect(Rect::new(0.0, 0.0, 50.0, 50.0), [255, 0, 0, 255]);
        orchestrator.force_cpu(&list);
        assert_eq!(orchestrator.mode(), RenderMode::Cpu);
    }

    #[test]
    fn cpu_pixels_returns_tinyskia_buffer() {
        let orchestrator = RenderOrchestrator::new(10, 10).expect("init");
        let pixels = orchestrator.cpu_pixels();
        // 10x10 RGBA8 = 400 bytes
        assert_eq!(pixels.len(), 400);
    }

    #[test]
    fn vello_accessors_return_renderer() {
        let orchestrator = RenderOrchestrator::new(100, 100).expect("init");
        let _ = orchestrator.vello().last_command_count();
    }

    #[test]
    fn tinyskia_accessors_return_backend() {
        let orchestrator = RenderOrchestrator::new(100, 100).expect("init");
        let _ = orchestrator.tinyskia().width();
    }

    #[test]
    fn render_empty_list_works() {
        let mut orchestrator = RenderOrchestrator::new(100, 100).expect("init");
        let recovery = RecoveryMachine::new();
        let list = PaintList::new();
        orchestrator.render(&list, &recovery);
    }

    #[test]
    fn orchestrator_error_display() {
        assert_eq!(
            format!("{}", OrchestratorError::NoBackend),
            "no rendering backend available"
        );
        assert_eq!(
            format!("{}", OrchestratorError::BackendInitFailed),
            "rendering backend initialization failed"
        );
    }
}
