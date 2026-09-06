//! Typography and IME candidate projection.
#![forbid(unsafe_code)]

/// Re-exported text layout primitives from `cosmic_text`.
pub use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping};
use winit::dpi::{LogicalPosition, LogicalSize};

/// Computes the IME candidate window bounds from the cursor position and line height.
///
/// Returns a logical position and size describing where the IME candidate window
/// should be anchored relative to the text being composed.
pub fn compute_ime_bounds(x: f64, y: f64, height: f64) -> (LogicalPosition<f64>, LogicalSize<f64>) {
    (LogicalPosition::new(x, y), LogicalSize::new(2.0, height))
}
