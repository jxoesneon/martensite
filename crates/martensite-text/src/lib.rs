//! Typography and text layout for Martensite.
//!
//! This crate provides:
//! - [`font`]: font system abstraction, system font discovery via
//!   `fontdb`, and custom font asset loading.
//! - [`shaping`]: complex text shaping with BiDi, line breaking, and
//!   font fallback via cosmic-text.
//! - [`cache`]: two-tier text measurement and glyph shaping cache
//!   (Tier 1 inline in `ColdNode`, Tier 2 global LRU with 16 MB budget).
//! - [`ime`]: velocity-damped kinetic IME candidate positioning that
//!   tracks the caret during active scrolling.
//!
//! ## IME candidate projection
//!
//! The `compute_ime_bounds` function computes IME candidate window
//! bounds from the cursor position and line height. For scrolling
//! containers, prefer the [`ime`] module's [`ImePositioner`], which
//! applies velocity-damped projection and viewport clamping.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Unicode Bidirectional Algorithm (UAX #9) integration for shaping.
pub mod bidi;
/// Two-tier text cache: `TextShapeCache`, `ShapeCacheKey`.
pub mod cache;
/// System font fallback cascade and script classification.
pub mod cascade;
/// Font system abstraction: `FontManager`, `FontId`, `FontSource`.
pub mod font;
/// Grapheme cluster break evaluation (UAX #29) for cursor positioning
/// and text selection.
pub mod grapheme;
/// Velocity-damped kinetic IME candidate positioning.
pub mod ime;
/// Unicode line breaking (UAX #14) and Kinsoku Shori.
pub mod line_break;
/// Complex text shaping: `Shaper`, `TextMetrics`, `ShapedLine`.
pub mod shaping;
/// Unicode vertical text layout (UAX #50) and coordinate transformation.
pub mod vertical;

pub use bidi::{BidiDirection, BidiMirrorMap, BidiParagraph, BidiResolved, BidiRun};
pub use cache::{
    CachedShape, DirectionBits, FallbackHash, FontSizeBits, LineHeightBits, MaxWidthBits,
    ShapeCacheKey, TextHash, TextShapeCache, WritingModeBits, DEFAULT_MEMORY_BUDGET,
};
pub use cascade::{
    classify_script, FallbackDecisionCache, FallbackKey, FontFallbackCache, FontFallbackChain,
    FontFallbackProvider, InstalledFontFallbackResolver, PlatformCascadeResolver, ScriptTag,
};
pub use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping};
pub use font::{FontFaceInfo, FontId, FontManager, FontSource, FontStyle};
pub use grapheme::{
    grapheme_at, grapheme_boundary_before, grapheme_byte_offset, grapheme_clusters, grapheme_count,
    next_grapheme_boundary, prev_grapheme_boundary, GraphemeBreaker,
};
pub use ime::{ImePositioner, ScrollKinematics, Viewport};
pub use line_break::{BreakOpportunity, LineBreaker};
pub use shaping::{
    measure_text, measure_text_with_attrs, shape_text, ShapedGlyph, ShapedLine, Shaper,
    ShapingOptions, TextMetrics,
};
pub use vertical::{
    apply_vertical_features, classify_vertical_orientation, collect_vertical_runs,
    VerticalFeatureTags, VerticalGlyphTransform, VerticalMetrics, VerticalOrientation, VerticalRun,
    WritingMode,
};

#[cfg(test)]
use winit::dpi::{LogicalPosition, LogicalSize};

/// Computes the IME candidate window bounds from the cursor position and line height.
///
/// Returns a logical position and size describing where the IME candidate window
/// should be anchored relative to the text being composed.
///
/// # Limitation
///
/// The width is a minimal placeholder (`2.0` logical pixels) because the
/// actual IME candidate window width depends on platform-specific IME
/// APIs and the composing text content, which are not available at the
/// text-shaping layer. A future milestone will integrate platform IME
/// APIs to compute the real candidate window width. The position and
/// height are accurate.
///
/// For scrolling containers, prefer [`ImePositioner::compute_bounds`],
/// which applies velocity-damped projection and viewport clamping.
#[deprecated(
    since = "0.5.0",
    note = "use `ImePositioner::compute_bounds` for velocity-damped, viewport-clamped IME bounds"
)]
#[cfg(test)]
fn compute_ime_bounds(x: f64, y: f64, height: f64) -> (LogicalPosition<f64>, LogicalSize<f64>) {
    (LogicalPosition::new(x, y), LogicalSize::new(2.0, height))
}

#[cfg(test)]
mod tests {
    #![allow(deprecated)]
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
