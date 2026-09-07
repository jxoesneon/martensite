//! Typography and text layout for Martensite.
//!
//! This crate provides:
//! - [`font`]: font system abstraction, system font discovery via
//!   `fontdb`, and custom font asset loading.
//! - [`shaping`]: complex text shaping with BiDi, line breaking, and
//!   font fallback via cosmic-text.
//! - [`cache`]: two-tier text measurement and glyph shaping cache
//!   (Tier 1 inline in `ColdNode`, Tier 2 global LRU with 16 MB budget).
//!
//! ## IME candidate projection
//!
//! The [`compute_ime_bounds`] function computes IME candidate window
//! bounds from the cursor position and line height.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Two-tier text cache: `TextShapeCache`, `ShapeCacheKey`.
pub mod cache;
/// Font system abstraction: `FontManager`, `FontId`, `FontSource`.
pub mod font;
/// Complex text shaping: `Shaper`, `TextMetrics`, `ShapedLine`.
pub mod shaping;

pub use cache::{
    CachedShape, FontSizeBits, MaxWidthBits, ShapeCacheKey, TextHash, TextShapeCache,
    DEFAULT_MEMORY_BUDGET,
};
pub use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping};
pub use font::{FontFaceInfo, FontId, FontManager, FontSource, FontStyle};
pub use shaping::{
    measure_text, measure_text_with_attrs, shape_text, ShapedGlyph, ShapedLine, Shaper, TextMetrics,
};

use winit::dpi::{LogicalPosition, LogicalSize};

/// Computes the IME candidate window bounds from the cursor position and line height.
///
/// Returns a logical position and size describing where the IME candidate window
/// should be anchored relative to the text being composed.
pub fn compute_ime_bounds(x: f64, y: f64, height: f64) -> (LogicalPosition<f64>, LogicalSize<f64>) {
    (LogicalPosition::new(x, y), LogicalSize::new(2.0, height))
}

#[cfg(test)]
mod tests {
    use super::compute_ime_bounds;

    #[test]
    fn returns_correct_position_and_size() {
        let (pos, size) = compute_ime_bounds(10.0, 20.0, 30.0);
        assert_eq!(pos.x, 10.0);
        assert_eq!(pos.y, 20.0);
        assert_eq!(size.width, 2.0);
        assert_eq!(size.height, 30.0);
    }

    #[test]
    fn zero_height() {
        let (pos, size) = compute_ime_bounds(5.0, 5.0, 0.0);
        assert_eq!(pos.x, 5.0);
        assert_eq!(pos.y, 5.0);
        assert_eq!(size.width, 2.0);
        assert_eq!(size.height, 0.0);
    }

    #[test]
    fn negative_coordinates() {
        let (pos, size) = compute_ime_bounds(-10.0, -20.0, 30.0);
        assert_eq!(pos.x, -10.0);
        assert_eq!(pos.y, -20.0);
        assert_eq!(size.width, 2.0);
        assert_eq!(size.height, 30.0);
    }
}
