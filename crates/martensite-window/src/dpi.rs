//! Physical-to-logical coordinate scaling with fractional DPI support.
//!
//! Modern displays report a *scale factor* relating physical pixels (the
//! device resolution that the GPU scans out) to logical pixels (the
//! resolution that the application reasons about). A scale factor of `1.0`
//! means one logical pixel maps to exactly one physical pixel; a factor of
//! `1.5` means each logical pixel occupies `1.5` physical pixels in each
//! dimension.
//!
//! [`DpiScale`] encapsulates a single scale factor and provides the two
//! fundamental conversions required throughout the rendering pipeline:
//!
//! - [`DpiScale::to_logical`] divides a physical measurement by the scale
//!   factor, yielding the logical (UI) size.
//! - [`DpiScale::to_physical`] multiplies a logical measurement by the scale
//!   factor, yielding the size that must be fed to the swapchain / GPU.
//!
//! The scale factor can be updated at runtime via [`DpiScale::update_scale`]
//! to react to [`WindowEvent::ScaleFactorChanged`] events emitted when a
//! window is dragged onto a monitor with a different DPI, or when the user
//! changes the system scaling settings.
//!
//! [`WindowEvent::ScaleFactorChanged`]: winit::event::WindowEvent::ScaleFactorChanged

/// Tracks the DPI scale factor for a single window and converts between
/// physical and logical coordinates.
///
/// Physical coordinates correspond to actual device pixels. Logical
/// coordinates are the resolution the application layout works in. The
/// relationship is:
///
/// ```text
/// logical = physical / scale_factor
/// physical = logical * scale_factor
/// ```
///
/// Fractional scale factors such as `1.25`, `1.5` and `1.75` are fully
/// supported — no integer rounding is applied, so callers retain full
/// sub-pixel precision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DpiScale {
    /// The ratio of physical pixels to logical pixels.
    ///
    /// Always strictly positive. A value of `1.0` indicates a 1:1 mapping.
    scale_factor: f64,
}

impl DpiScale {
    /// Creates a new [`DpiScale`] with the given scale factor.
    ///
    /// # Panics
    ///
    /// Panics if `scale_factor` is not finite or is not strictly positive
    /// (`<= 0.0`). A non-positive scale factor would make coordinate
    /// conversion divide by zero or flip the coordinate space, which is
    /// never meaningful for a real display.
    #[must_use]
    pub fn new(scale_factor: f64) -> Self {
        assert!(
            scale_factor.is_finite() && scale_factor > 0.0,
            "DPI scale factor must be finite and strictly positive, got {scale_factor}",
        );
        Self { scale_factor }
    }

    /// Converts a physical measurement into logical pixels.
    ///
    /// This divides `physical` by the stored scale factor. For example, with
    /// a scale factor of `2.0`, a `1920` physical-pixel-wide surface is
    /// `960` logical pixels wide.
    #[must_use]
    pub fn to_logical(&self, physical: f64) -> f64 {
        physical / self.scale_factor
    }

    /// Converts a logical measurement into physical pixels.
    ///
    /// This multiplies `logical` by the stored scale factor. For example,
    /// with a scale factor of `1.5`, a `100` logical-pixel-wide element is
    /// rasterized at `150` physical pixels.
    #[must_use]
    pub fn to_physical(&self, logical: f64) -> f64 {
        logical * self.scale_factor
    }

    /// Updates the scale factor in response to a monitor DPI change.
    ///
    /// # Panics
    ///
    /// Panics if `new_scale` is not finite or is not strictly positive
    /// (`<= 0.0`), for the same reasons as [`DpiScale::new`].
    pub fn update_scale(&mut self, new_scale: f64) {
        assert!(
            new_scale.is_finite() && new_scale > 0.0,
            "DPI scale factor must be finite and strictly positive, got {new_scale}",
        );
        self.scale_factor = new_scale;
    }

    /// Returns the current scale factor.
    ///
    /// This is the ratio of physical pixels to logical pixels currently in
    /// effect for the owning window.
    #[must_use]
    pub fn scale_factor(&self) -> f64 {
        self.scale_factor
    }
}

impl Default for DpiScale {
    /// Returns a [`DpiScale`] with a scale factor of `1.0`.
    ///
    /// This is the correct default for unscaled displays where physical and
    /// logical pixels coincide.
    fn default() -> Self {
        Self { scale_factor: 1.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::DpiScale;

    #[test]
    fn default_is_identity_scale() {
        let scale = DpiScale::default();
        assert_eq!(scale.scale_factor(), 1.0);
        // Identity scaling leaves coordinates unchanged.
        assert_eq!(scale.to_logical(800.0), 800.0);
        assert_eq!(scale.to_physical(800.0), 800.0);
    }

    #[test]
    fn new_stores_scale_factor() {
        let scale = DpiScale::new(2.0);
        assert_eq!(scale.scale_factor(), 2.0);
    }

    #[test]
    fn to_logical_divides_by_scale() {
        let scale = DpiScale::new(2.0);
        assert_eq!(scale.to_logical(1920.0), 960.0);
        assert_eq!(scale.to_logical(1080.0), 540.0);
    }

    #[test]
    fn to_physical_multiplies_by_scale() {
        let scale = DpiScale::new(2.0);
        assert_eq!(scale.to_physical(960.0), 1920.0);
        assert_eq!(scale.to_physical(540.0), 1080.0);
    }

    #[test]
    fn conversions_are_inverse() {
        let scale = DpiScale::new(2.5);
        for value in [0.0, 1.0, 100.0, 1234.5678, 4096.0] {
            let roundtrip = scale.to_logical(scale.to_physical(value));
            assert!(
                (roundtrip - value).abs() < f64::EPSILON,
                "logical(physical({value})) = {roundtrip} should equal {value}",
            );
            let roundtrip = scale.to_physical(scale.to_logical(value));
            assert!(
                (roundtrip - value).abs() < f64::EPSILON,
                "physical(logical({value})) = {roundtrip} should equal {value}",
            );
        }
    }

    #[test]
    fn fractional_scale_1_25() {
        let scale = DpiScale::new(1.25);
        // 1.25x: 1000 logical -> 1250 physical.
        assert_eq!(scale.to_physical(1000.0), 1250.0);
        // 1250 physical -> 1000 logical.
        assert_eq!(scale.to_logical(1250.0), 1000.0);
    }

    #[test]
    fn fractional_scale_1_5() {
        let scale = DpiScale::new(1.5);
        assert_eq!(scale.to_physical(100.0), 150.0);
        assert_eq!(scale.to_logical(150.0), 100.0);
    }

    #[test]
    fn fractional_scale_1_75() {
        let scale = DpiScale::new(1.75);
        assert_eq!(scale.to_physical(100.0), 175.0);
        assert_eq!(scale.to_logical(175.0), 100.0);
    }

    #[test]
    fn fractional_scale_3x_retina() {
        let scale = DpiScale::new(3.0);
        assert_eq!(scale.to_physical(640.0), 1920.0);
        assert_eq!(scale.to_logical(1920.0), 640.0);
    }

    #[test]
    fn update_scale_changes_conversions() {
        let mut scale = DpiScale::new(1.0);
        assert_eq!(scale.to_physical(100.0), 100.0);

        scale.update_scale(2.0);
        assert_eq!(scale.scale_factor(), 2.0);
        assert_eq!(scale.to_physical(100.0), 200.0);
        assert_eq!(scale.to_logical(200.0), 100.0);
    }

    #[test]
    fn update_scale_to_fractional() {
        let mut scale = DpiScale::new(1.0);
        scale.update_scale(1.5);
        assert_eq!(scale.scale_factor(), 1.5);
        assert_eq!(scale.to_physical(200.0), 300.0);
    }

    #[test]
    fn zero_physical_maps_to_zero_logical() {
        let scale = DpiScale::new(2.0);
        assert_eq!(scale.to_logical(0.0), 0.0);
        assert_eq!(scale.to_physical(0.0), 0.0);
    }

    #[test]
    fn negative_coordinates_are_preserved() {
        // Layouts can use negative offsets; the sign must be retained.
        let scale = DpiScale::new(2.0);
        assert_eq!(scale.to_logical(-100.0), -50.0);
        assert_eq!(scale.to_physical(-50.0), -100.0);
    }

    #[test]
    fn subpixel_precision_is_retained() {
        let scale = DpiScale::new(1.5);
        // 1 logical px = 1.5 physical px; no rounding should occur.
        assert_eq!(scale.to_physical(1.0), 1.5);
        assert_eq!(scale.to_logical(1.5), 1.0);
    }

    #[test]
    #[should_panic(expected = "strictly positive")]
    fn new_rejects_zero() {
        let _ = DpiScale::new(0.0);
    }

    #[test]
    #[should_panic(expected = "strictly positive")]
    fn new_rejects_negative() {
        let _ = DpiScale::new(-1.0);
    }

    #[test]
    #[should_panic(expected = "finite")]
    fn new_rejects_nan() {
        let _ = DpiScale::new(f64::NAN);
    }

    #[test]
    #[should_panic(expected = "finite")]
    fn new_rejects_infinity() {
        let _ = DpiScale::new(f64::INFINITY);
    }

    #[test]
    #[should_panic(expected = "strictly positive")]
    fn update_scale_rejects_zero() {
        let mut scale = DpiScale::new(1.0);
        scale.update_scale(0.0);
    }

    #[test]
    #[should_panic(expected = "finite")]
    fn update_scale_rejects_nan() {
        let mut scale = DpiScale::new(1.0);
        scale.update_scale(f64::NAN);
    }

    #[test]
    fn copy_and_eq_semantics() {
        let a = DpiScale::new(1.5);
        let b = a;
        assert_eq!(a, b);
        let c = DpiScale::new(2.0);
        assert_ne!(a, c);
    }
}
