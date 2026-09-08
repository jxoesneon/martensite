//! Telemetry and in-app F12 developer HUD.
//!
//! This crate provides two subsystems for the v0.9.0 Developer Experience
//! milestone:
//!
//! - [`tracy`]: Safe, allocation-free profiling span instrumentation that
//!   records region durations into a thread-local ring buffer. The
//!   DevTools overhead gate requires `< 0.1ms` per 60fps frame.
//! - [`hud`]: In-app diagnostic HUD with a rolling 120-frame timing
//!   histogram, dirty rect visualization, and WidgetArena slot
//!   utilization telemetry.
//!
//! # Example
//!
//! ```
//! use martensite_devtools::{tracy, hud::DiagnosticHud};
//!
//! // Profile a layout pass.
//! let _guard = tracy::span("layout_pass");
//!
//! // Record frame timing for the HUD.
//! let mut hud = DiagnosticHud::new();
//! hud.record_frame(martensite_devtools::hud::FrameTiming {
//!     layout_time_ns: 500_000,
//!     paint_time_ns: 300_000,
//!     gpu_wait_time_ns: 100_000,
//!     total_time_ns: 900_000,
//! });
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod hud;
pub mod tracy;

#[cfg(test)]
mod tests {
    /// Smoke test verifying the crate compiles with `#![forbid(unsafe_code)]`.
    #[test]
    fn crate_compiles() {
        // Compilation + test execution is the smoke test.
    }
}
