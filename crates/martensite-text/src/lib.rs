//! Typography and IME candidate projection.
#![forbid(unsafe_code)]

use winit::dpi::{LogicalPosition, LogicalSize};

pub fn compute_ime_bounds(x: f64, y: f64, height: f64) -> (LogicalPosition<f64>, LogicalSize<f64>) {
    (LogicalPosition::new(x, y), LogicalSize::new(2.0, height))
}
