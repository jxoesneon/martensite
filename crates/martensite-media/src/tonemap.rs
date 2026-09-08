//! Display-adaptive luminance tone mapping and filmic compression operators.
//!
//! This module provides filmic tone mapping and reference white adaptation for
//! high-dynamic-range video playback on displays with varying luminance capabilities:
//!
//! - **Display Adaptation**: Dynamically queries panel capabilities
//!   (`effective_sdr_white = sdr_white.unwrap_or(203.0).min(max_luminance)`),
//!   preventing SDR UI controls from crushing highlights on budget 150–180 nit screens.
//! - **Hable Filmic Operator**: The Uncharted 2 filmic curve with smooth toe, linear
//!   midtone, and gradual shoulder roll-off.
//! - **Uchimura Filmic Operator**: The Gran Turismo curve offering precise control over
//!   black tightness and highlight compression.

use glam::Vec3;

/// Physical display luminance characteristics.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DisplayProfile {
    /// Configured or reported SDR reference white level in nits (typically 203 nits).
    pub sdr_white_level_nits: Option<f32>,
    /// Minimum panel black level luminance in nits (e.g. 0.005 for OLED).
    pub min_luminance_nits: f32,
    /// Peak panel luminance in nits (e.g. 180 for budget laptops, 1000+ for HDR).
    pub max_luminance_nits: f32,
}

impl DisplayProfile {
    /// Creates a new `DisplayProfile` with the specified luminance limits.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::tonemap::DisplayProfile;
    ///
    /// let profile = DisplayProfile::new(Some(203.0), 0.005, 1000.0);
    /// assert_eq!(profile.effective_sdr_white(), 203.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(
        sdr_white_level_nits: Option<f32>,
        min_luminance_nits: f32,
        max_luminance_nits: f32,
    ) -> Self {
        Self {
            sdr_white_level_nits,
            min_luminance_nits,
            max_luminance_nits,
        }
    }

    /// Default SDR display profile (203 nits reference white, 300 nits peak).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::tonemap::DisplayProfile;
    ///
    /// let profile = DisplayProfile::default_sdr();
    /// assert_eq!(profile.effective_sdr_white(), 203.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn default_sdr() -> Self {
        Self {
            sdr_white_level_nits: Some(203.0),
            min_luminance_nits: 0.1,
            max_luminance_nits: 300.0,
        }
    }

    /// High-end HDR display profile (203 nits reference white, 1000 nits peak).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::tonemap::DisplayProfile;
    ///
    /// let profile = DisplayProfile::default_hdr10();
    /// assert_eq!(profile.effective_sdr_white(), 203.0);
    /// assert!((profile.hdr_headroom() - 1000.0 / 203.0).abs() < 1e-4);
    /// ```
    #[inline]
    #[must_use]
    pub const fn default_hdr10() -> Self {
        Self {
            sdr_white_level_nits: Some(203.0),
            min_luminance_nits: 0.005,
            max_luminance_nits: 1000.0,
        }
    }

    /// Budget low-luminance display profile (e.g. 160 nits peak SDR laptop).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::tonemap::DisplayProfile;
    ///
    /// let profile = DisplayProfile::budget_laptop_sdr();
    /// assert_eq!(profile.effective_sdr_white(), 160.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn budget_laptop_sdr() -> Self {
        Self {
            sdr_white_level_nits: None,
            min_luminance_nits: 0.3,
            max_luminance_nits: 160.0,
        }
    }

    /// Computes the effective SDR reference white level in nits:
    ///
    /// `sdr_white.unwrap_or(203.0).min(max_luminance)`
    ///
    /// On panels with peak brightness below 203 nits, the reference white is clamped
    /// to the maximum physical output of the panel to prevent highlight clipping.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::tonemap::DisplayProfile;
    ///
    /// // Low-nit display clamps to panel max
    /// let low_nit = DisplayProfile::new(Some(203.0), 0.2, 160.0);
    /// assert_eq!(low_nit.effective_sdr_white(), 160.0);
    ///
    /// // High-nit display uses standard 203 nits
    /// let hdr = DisplayProfile::new(Some(203.0), 0.005, 1000.0);
    /// assert_eq!(hdr.effective_sdr_white(), 203.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn effective_sdr_white(&self) -> f32 {
        self.sdr_white_level_nits
            .unwrap_or(203.0)
            .min(self.max_luminance_nits)
    }

    /// Calculates the dynamic range headroom ratio: $\frac{L_{\text{max}}}{L_{\text{sdr\_ref}}}$.
    ///
    /// Returns $1.0$ if the display has no HDR headroom above SDR reference white.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::tonemap::DisplayProfile;
    ///
    /// let hdr = DisplayProfile::new(Some(203.0), 0.005, 1000.0);
    /// assert!((hdr.hdr_headroom() - 1000.0 / 203.0).abs() < 1e-4);
    /// ```
    #[inline]
    #[must_use]
    pub fn hdr_headroom(&self) -> f32 {
        let white = self.effective_sdr_white();
        if white > 0.0 {
            (self.max_luminance_nits / white).max(1.0)
        } else {
            1.0
        }
    }
}

/// Supported tone-mapping operators.
///
/// # Examples
///
/// ```
/// use martensite_media::tonemap::ToneMapOperator;
///
/// assert_eq!(ToneMapOperator::default(), ToneMapOperator::None);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ToneMapOperator {
    /// Direct linear passthrough without tone curve compression.
    #[default]
    None,
    /// Hable (Uncharted 2) filmic tone mapping operator.
    Hable,
    /// Uchimura (Gran Turismo) filmic tone mapping operator.
    Uchimura,
}

impl ToneMapOperator {
    /// Maps a linear color value using the selected operator.
    ///
    /// # Examples
    ///
    /// ```
    /// use glam::Vec3;
    /// use martensite_media::tonemap::ToneMapOperator;
    ///
    /// let hdr_color = Vec3::splat(5.0);
    /// let mapped = ToneMapOperator::Hable.map_rgb(hdr_color);
    /// assert!(mapped.x < 1.0 && mapped.x > 0.0);
    /// ```
    #[must_use]
    pub fn map_rgb(&self, color: Vec3) -> Vec3 {
        match self {
            Self::None => color,
            Self::Hable => Vec3::new(
                hable_tonemap_scalar(color.x),
                hable_tonemap_scalar(color.y),
                hable_tonemap_scalar(color.z),
            ),
            Self::Uchimura => Vec3::new(
                uchimura_tonemap_scalar(color.x),
                uchimura_tonemap_scalar(color.y),
                uchimura_tonemap_scalar(color.z),
            ),
        }
    }
}

/// Evaluates the unnormalized Hable filmic curve:
///
/// $$f(x) = \frac{x(Ax + CB) + DE}{x(Ax + B) + DF} - \frac{E}{F}$$
#[inline]
fn hable_f(x: f32) -> f32 {
    const A: f32 = 0.15; // Shoulder strength
    const B: f32 = 0.50; // Linear strength
    const C: f32 = 0.10; // Linear angle
    const D: f32 = 0.20; // Toe strength
    const E: f32 = 0.02; // Toe numerator
    const F: f32 = 0.30; // Toe denominator

    ((x * (A * x + C * B) + D * E) / (x * (A * x + B) + D * F)) - (E / F)
}

/// Normalized Hable tone-mapping operator for a single scalar channel.
///
/// Output is normalized so that $W = 11.2$ maps to $1.0$.
///
/// # Examples
///
/// ```
/// use martensite_media::tonemap::hable_tonemap_scalar;
///
/// assert_eq!(hable_tonemap_scalar(0.0), 0.0);
/// let val = hable_tonemap_scalar(11.2);
/// assert!((val - 1.0).abs() < 1e-4);
/// ```
#[must_use]
pub fn hable_tonemap_scalar(x: f32) -> f32 {
    if !x.is_finite() || x <= 0.0 {
        return 0.0;
    }
    const W: f32 = 11.20;
    let white_scale = 1.0 / hable_f(W);
    (hable_f(x) * white_scale).clamp(0.0, 1.0)
}

/// Uchimura (Gran Turismo) filmic tone-mapping operator for a single scalar channel.
///
/// Provides a smooth toe, linear midtone, and continuous $C^1$ shoulder compression.
///
/// # Examples
///
/// ```
/// use martensite_media::tonemap::uchimura_tonemap_scalar;
///
/// assert_eq!(uchimura_tonemap_scalar(0.0), 0.0);
/// assert!(uchimura_tonemap_scalar(1.0) > 0.0);
/// ```
#[must_use]
pub fn uchimura_tonemap_scalar(x: f32) -> f32 {
    if !x.is_finite() || x <= 0.0 {
        return 0.0;
    }
    // Uchimura parameters
    const M: f32 = 1.0; // Max luminance
    const A: f32 = 1.0; // Contrast
    const C: f32 = 1.33; // Black tightness
    const L0: f32 = 0.4; // Linear segment start
    const S0: f32 = 0.8; // Linear segment end
    const S1: f32 = 1.2; // Shoulder limit

    if x < L0 {
        M / (1.0 + (x * A).powf(-C))
    } else if x < S0 {
        // Midtone linear region
        let t = (x - L0) / (S0 - L0);
        let y0 = M / (1.0 + (L0 * A).powf(-C));
        let y1 = M * S0 / S1;
        y0 + t * (y1 - y0)
    } else {
        // Shoulder compression
        let s = (x - S0) / (S1 - S0);
        let y1 = M * S0 / S1;
        (y1 + (M - y1) * (1.0 - (-s).exp())).clamp(0.0, M)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hable_monotonicity_and_bounds() {
        let mut prev = -1.0;
        for i in 0..=100 {
            let x = i as f32 * 0.15;
            let mapped = hable_tonemap_scalar(x);
            assert!(
                mapped >= prev,
                "Hable must be strictly monotonic: at x={x}, got {mapped} < {prev}"
            );
            assert!((0.0..=1.0).contains(&mapped));
            prev = mapped;
        }
    }

    #[test]
    fn display_adaptation_effective_white() {
        let budget = DisplayProfile::budget_laptop_sdr();
        assert_eq!(budget.effective_sdr_white(), 160.0);
        assert_eq!(budget.hdr_headroom(), 1.0);

        let hdr = DisplayProfile::default_hdr10();
        assert_eq!(hdr.effective_sdr_white(), 203.0);
        assert!((hdr.hdr_headroom() - 1000.0 / 203.0).abs() < 1e-4);
    }
}
