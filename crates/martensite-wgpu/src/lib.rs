//! WGPU compute rasterization and GPU resurrection engine.
//!
//! `martensite-wgpu` provides four modules covering the v0.2.0 rendering
//! pipeline milestone:
//!
//! * [`device`] — [`device::GpuContext`] encapsulates the instance, adapter,
//!   device, and queue, with adapter enumeration, power-preference selection,
//!   and feature/limit verification.
//! * [`surface`] — [`surface::SurfaceWrapper`] manages a `wgpu` surface and its
//!   swapchain configuration, with present-mode negotiation and resize
//!   re-creation.
//! * [`resilience`] — [`resilience::RecoveryMachine`] implements the formal
//!   typestate GPU device-loss recovery FSM with exponential backoff and a
//!   CPU fallback path.
//! * [`orchestrator`] — [`orchestrator::RenderOrchestrator`] bridges
//!   `martensite-render` with the WGPU device pipeline, switching between
//!   Vello GPU rendering and TinySkia CPU fallback based on recovery state.
//!
//! # Safety
//!
//! This crate contains no `unsafe` code (`#![forbid(unsafe_code)]`).

#![forbid(unsafe_code)]

/// GPU device context: adapter enumeration, feature selection, device/queue
/// lifecycle.
pub mod device;
/// External video surface memory import, texture format negotiation, and compute EOTF shaders.
pub mod interop;
/// Render pipeline orchestrator bridging GPU and CPU backends.
pub mod orchestrator;
/// GPU device-loss recovery finite state machine.
pub mod resilience;
/// Surface and swapchain management.
pub mod surface;

pub use device::{GpuContext, GpuContextError};
pub use interop::{FormatNegotiator, VideoPipelineUniforms, MEDIA_YUV_EOTF_WGSL};
pub use orchestrator::{OrchestratorConfig, OrchestratorError, RenderMode, RenderOrchestrator};
pub use resilience::{
    backoff_duration, DeviceStatus, RecoveryMachine, SurfaceError, DEFAULT_FALLBACK_THRESHOLD,
    DEFAULT_MAX_RETRIES, RECOVERY_BUDGET,
};
pub use surface::{SurfaceWrapper, SurfaceWrapperError};
pub use wgpu;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn re_exports_are_constructible() {
        // The re-exported recovery machine must be usable directly from the
        // crate root without importing submodules.
        let m = RecoveryMachine::new();
        assert!(m.is_active());
    }

    #[test]
    fn surface_wrapper_error_alias_matches_inner_type() {
        // The crate-root alias must be the same type as the surface module's
        // error so callers can use either path interchangeably.
        let a: SurfaceWrapperError = surface::SurfaceWrapperError::NotConfigured;
        let b: surface::SurfaceWrapperError = SurfaceWrapperError::NotConfigured;
        assert_eq!(a, b);
    }
}
