//! Telemetry and in-app F12 developer HUD.
//!
//! This crate provides two subsystems for the v0.9.0 Developer Experience
//! milestone:
//!
//! - [`inspector`]: In-app widget inspector overlay with Chrome DevTools-style
//!   select mode, lazy tree expansion, layout constraint chain modeling,
//!   and property/marker extraction.
//! - [`tracy`]: Safe, allocation-free profiling span instrumentation that
//!   records region durations into a thread-local ring buffer. The
//!   DevTools overhead gate requires `< 0.1ms` per 60fps frame.
//! - [`event_ledger`]: Preallocated, zero-allocation ring buffer for UI event
//!   dispatch observability, hit-path recording, and diagnostic formatting.
//! - [`hud`]: In-app diagnostic HUD with a rolling 120-frame timing
//!   histogram, dirty rect visualization, and WidgetArena slot
//!   utilization telemetry.
//! - [`tweak`]: Runtime live property tweak registry for dynamically modifying
//!   parameters and reactive signals without a rebuild.
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

#[cfg(feature = "devtools")]
pub mod error_surface;
pub mod event_ledger;
pub mod hud;
#[cfg(feature = "devtools")]
pub mod inspector;
#[cfg(feature = "devtools")]
pub mod lint_bridge;
#[cfg(feature = "devtools-timemachine")]
pub mod timemachine;
pub mod tracy;
#[cfg(feature = "devtools")]
pub mod tweak;

#[cfg(feature = "devtools")]
pub use error_surface::{
    clear_in_flight, current_in_flight, install_dev_panic_hook, is_dev_panic_enabled,
    peek_last_panic, scope_in_flight, set_dev_panic_enabled, set_in_flight, take_last_panic,
    ClippedTextDiagnostic, CrashBundle, DiagnosticEntry, DiagnosticSeverity, ErrorSurface,
    InFlightContext, InlineAnnotation, LayoutDiagnostic, LintDiagnostic, OverflowTape,
    PaintErrorDiagnostic, PanicPhase, RecoveryPolicy, DEFAULT_ERROR_RED,
    DEFAULT_MAX_INLINE_ANNOTATIONS, DEFAULT_STRIPE_BLACK, DEFAULT_STRIPE_YELLOW,
    DEFAULT_WARN_AMBER,
};
#[cfg(feature = "devtools")]
pub use inspector::{
    hit_test_select, Axis, ConstraintStep, ConstraintViolation, DevToolsOptions, HitTestResult,
    InlineMarkers, InspectionMode, InspectorState, InspectorTreeModel, InspectorTreeNode, KeyCode,
    KeyCombo, LayoutInspection, LayoutInspector, LayoutStyleSummary, Modifiers, NodeBadges,
    OverflowInfo, TrackedSignalInfo, WidgetProperties,
};
#[cfg(feature = "devtools")]
pub use lint_bridge::{LintBadgeSummary, LintBridge, LintDump, LintDumpError};
#[cfg(feature = "devtools")]
pub use tweak::{
    format_source_patch, AsSignalId, SourcePatch, SourceSpan, TweakEntry, TweakRegistry,
    TweakValue, Tweakable,
};

pub use event_ledger::{
    is_debug_events_enabled, set_debug_events_enabled, Disposition, Drain, EventFilter, EventKind,
    EventLedger, EventLedgerIter, EventRecord, HitPath, HitPathIter, HitRejection, Point,
    DEFAULT_LEDGER_CAPACITY, HIT_PATH_CAPACITY,
};

#[cfg(test)]
mod tests {
    /// Smoke test verifying the crate compiles with `#![forbid(unsafe_code)]`.
    #[test]
    fn crate_compiles() {
        // Compilation + test execution is the smoke test.
    }
}
