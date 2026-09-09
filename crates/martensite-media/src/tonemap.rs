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

    /// Creates a [`DisplayProfile`] from a `wgpu` [`wgpu::DisplayHdrInfo`],
    /// falling back to [`DisplayProfile::default_sdr`] when the platform does
    /// not report absolute-nit luminance values.
    ///
    /// This is the v0.8.0 dynamic display-capability query. On Windows (DXGI)
    /// the `DisplayHdrInfo::luminance` field carries `max_nits`, `min_nits`,
    /// and `sdr_white_nits`; on macOS (EDR) and the web only a relative
    /// headroom multiplier is available, so the SDR fallback is used for the
    /// absolute-nit fields and the headroom is applied via
    /// [`DisplayProfile::with_headroom`].
    ///
    /// # Arguments
    ///
    /// * `hdr_info` — The `wgpu::DisplayHdrInfo` queried from
    ///   `wgpu::Surface::display_hdr_info(adapter)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::tonemap::DisplayProfile;
    ///
    /// // A headless/unknown display falls back to SDR defaults.
    /// let profile = DisplayProfile::from_display_hdr_info(&wgpu::DisplayHdrInfo::default());
    /// assert_eq!(profile.effective_sdr_white(), 203.0);
    /// ```
    #[must_use]
    pub fn from_display_hdr_info(hdr_info: &wgpu::DisplayHdrInfo) -> Self {
        // If the platform reports absolute nits (Windows/DXGI), use them
        // directly. `sdr_white_nits` may be `None` even when `max_nits` is
        // known; in that case we fall back to the standard 203-nit reference.
        if let Some(lum) = hdr_info.luminance {
            let max_nits = lum.max_nits.unwrap_or(300.0);
            let min_nits = lum.min_nits.unwrap_or(0.1);
            let sdr_white = lum.sdr_white_nits.unwrap_or(203.0).min(max_nits);
            return Self {
                sdr_white_level_nits: Some(sdr_white),
                min_luminance_nits: min_nits,
                max_luminance_nits: max_nits,
            };
        }

        // On Apple EDR, a headroom multiplier is available. We can't convert
        // it to absolute nits, but we can scale the SDR fallback by the
        // headroom to approximate the peak luminance.
        if let Some(headroom) = hdr_info
            .headroom
            .and_then(|h| h.current)
            .filter(|h| h.is_finite() && *h > 0.0)
        {
            let sdr_white = 203.0;
            let max_nits = sdr_white * headroom;
            return Self {
                sdr_white_level_nits: Some(sdr_white),
                min_luminance_nits: 0.005,
                max_luminance_nits: max_nits,
            };
        }

        // Use the coarse HDR flag as a last resort: if the display is
        // definitively SDR, use the budget-laptop fallback; otherwise use the
        // default SDR profile.
        match hdr_info.coarse.and_then(|c| c.high_dynamic_range) {
            Some(false) => Self::budget_laptop_sdr(),
            _ => Self::default_sdr(),
        }
    }

    /// Returns a copy of this profile with the peak luminance scaled by
    /// `headroom`, clamped to a minimum of `1.0`.
    ///
    /// This is useful when the platform reports a relative EDR headroom
    /// multiplier (Apple) rather than absolute nits.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::tonemap::DisplayProfile;
    ///
    /// let sdr = DisplayProfile::default_sdr();
    /// let hdr = sdr.with_headroom(3.0);
    /// assert!((hdr.max_luminance_nits - 300.0 * 3.0).abs() < 1e-4);
    /// ```
    #[must_use]
    pub fn with_headroom(&self, headroom: f32) -> Self {
        let headroom = if headroom.is_finite() && headroom > 0.0 {
            headroom
        } else {
            1.0
        };
        Self {
            sdr_white_level_nits: self.sdr_white_level_nits,
            min_luminance_nits: self.min_luminance_nits,
            max_luminance_nits: self.max_luminance_nits * headroom,
        }
    }

    /// Returns `true` when this profile represents an HDR-capable display
    /// (peak luminance exceeds 400 nits, the typical HDR entry point).
    ///
    /// SDR displays commonly have peak luminance in the 250–350 nit range
    /// with a 203-nit SDR reference white; HDR displays start at ~400 nits.
    /// The [`DisplayCapabilities::hdr_capable`] field, which checks the
    /// surface's color-space support, is a more precise signal.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::tonemap::DisplayProfile;
    ///
    /// assert!(!DisplayProfile::default_sdr().is_hdr_capable());
    /// assert!(DisplayProfile::default_hdr10().is_hdr_capable());
    /// ```
    #[must_use]
    pub fn is_hdr_capable(&self) -> bool {
        // 400 nits is the typical HDR entry point; SDR displays peak at
        // ~300–350 nits.
        self.max_luminance_nits >= 400.0
    }
}

/// A snapshot of the surface and display capabilities reported by `wgpu` for
/// a given adapter + surface pair.
///
/// This is the v0.8.0 dynamic display-capability query result. It combines
/// the surface format/present-mode capabilities (from
/// [`wgpu::Surface::get_capabilities`]) with the display's HDR luminance
/// characteristics (from [`wgpu::Surface::display_hdr_info`]) into a single
/// value, with static fallbacks for headless CI where no surface or adapter
/// is available.
///
/// # Examples
///
/// ```
/// use martensite_media::tonemap::{DisplayCapabilities, DisplayProfile};
///
/// // The static fallback is always available, even in headless CI.
/// let fallback = DisplayCapabilities::fallback();
/// assert!(fallback.supported_formats.is_empty());
/// assert!(!fallback.hdr_capable);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayCapabilities {
    /// The texture formats supported by the surface, in preference order.
    /// Empty when no surface is available (headless fallback).
    pub supported_formats: Vec<wgpu::TextureFormat>,
    /// The present modes supported by the surface.
    /// Empty when no surface is available (headless fallback).
    pub supported_present_modes: Vec<wgpu::PresentMode>,
    /// The display luminance profile derived from the adapter's HDR info.
    pub profile: DisplayProfile,
    /// `true` when the surface supports an HDR-capable color space (e.g.
    /// `ExtendedSrgbLinear`, `Bt2100Pq`). `false` for the headless fallback.
    pub hdr_capable: bool,
    /// The output bit depth per color channel, if reported by the platform.
    pub bits_per_color: Option<u8>,
}

impl DisplayCapabilities {
    /// Creates a `DisplayCapabilities` from `wgpu` surface capabilities and
    /// display HDR info.
    ///
    /// This is the production entry point: call
    /// `surface.get_capabilities(adapter)` and
    /// `surface.display_hdr_info(adapter)`, then pass the results here.
    ///
    /// # Arguments
    ///
    /// * `caps` — The `wgpu::SurfaceCapabilities` from
    ///   `wgpu::Surface::get_capabilities`.
    /// * `hdr_info` — The `wgpu::DisplayHdrInfo` from
    ///   `wgpu::Surface::display_hdr_info`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::tonemap::DisplayCapabilities;
    ///
    /// let caps = DisplayCapabilities::from_wgpu(
    ///     &wgpu::SurfaceCapabilities::default(),
    ///     &wgpu::DisplayHdrInfo::default(),
    /// );
    /// assert!(caps.supported_formats.is_empty());
    /// ```
    #[must_use]
    pub fn from_wgpu(caps: &wgpu::SurfaceCapabilities, hdr_info: &wgpu::DisplayHdrInfo) -> Self {
        let profile = DisplayProfile::from_display_hdr_info(hdr_info);

        // Check whether any format supports an HDR color space.
        let hdr_capable = caps.format_capabilities.iter().any(|fc| {
            fc.color_spaces
                .contains(wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR)
                || fc
                    .color_spaces
                    .contains(wgpu::SurfaceColorSpaces::BT2100_PQ)
                || fc
                    .color_spaces
                    .contains(wgpu::SurfaceColorSpaces::EXTENDED_SRGB)
                || fc
                    .color_spaces
                    .contains(wgpu::SurfaceColorSpaces::EXTENDED_DISPLAY_P3)
        });

        Self {
            supported_formats: caps.formats.clone(),
            supported_present_modes: caps.present_modes.clone(),
            profile,
            hdr_capable,
            bits_per_color: hdr_info.bits_per_color,
        }
    }

    /// Returns the static fallback capabilities for headless CI and
    /// environments without a GPU surface.
    ///
    /// The fallback uses [`DisplayProfile::default_sdr`] and reports no
    /// supported formats or present modes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::tonemap::DisplayCapabilities;
    ///
    /// let fallback = DisplayCapabilities::fallback();
    /// assert!(!fallback.hdr_capable);
    /// assert!(fallback.supported_formats.is_empty());
    /// ```
    #[must_use]
    pub fn fallback() -> Self {
        Self {
            supported_formats: Vec::new(),
            supported_present_modes: Vec::new(),
            profile: DisplayProfile::default_sdr(),
            hdr_capable: false,
            bits_per_color: None,
        }
    }

    /// Returns the best swapchain format from the supported formats, preferring
    /// HDR formats when the display is HDR-capable.
    ///
    /// Falls back to [`wgpu::TextureFormat::Bgra8Unorm`] when no formats are
    /// reported (headless).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::tonemap::DisplayCapabilities;
    ///
    /// let fallback = DisplayCapabilities::fallback();
    /// assert_eq!(fallback.best_format(), wgpu::TextureFormat::Bgra8Unorm);
    /// ```
    #[must_use]
    pub fn best_format(&self) -> wgpu::TextureFormat {
        if self.hdr_capable {
            if self
                .supported_formats
                .contains(&wgpu::TextureFormat::Rgba16Float)
            {
                return wgpu::TextureFormat::Rgba16Float;
            }
            if self
                .supported_formats
                .contains(&wgpu::TextureFormat::Rgb10a2Unorm)
            {
                return wgpu::TextureFormat::Rgb10a2Unorm;
            }
        }
        if self
            .supported_formats
            .contains(&wgpu::TextureFormat::Bgra8Unorm)
        {
            return wgpu::TextureFormat::Bgra8Unorm;
        }
        if self
            .supported_formats
            .contains(&wgpu::TextureFormat::Rgba8Unorm)
        {
            return wgpu::TextureFormat::Rgba8Unorm;
        }
        self.supported_formats
            .first()
            .copied()
            .unwrap_or(wgpu::TextureFormat::Bgra8Unorm)
    }

    /// Returns `true` when the surface supports the given present mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::tonemap::DisplayCapabilities;
    /// use wgpu::PresentMode;
    ///
    /// let fallback = DisplayCapabilities::fallback();
    /// assert!(!fallback.supports_present_mode(PresentMode::Mailbox));
    /// ```
    #[must_use]
    pub fn supports_present_mode(&self, mode: wgpu::PresentMode) -> bool {
        self.supported_present_modes.contains(&mode)
    }
}

impl Default for DisplayCapabilities {
    fn default() -> Self {
        Self::fallback()
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

    #[test]
    fn from_display_hdr_info_default_falls_back_to_sdr() {
        let profile = DisplayProfile::from_display_hdr_info(&wgpu::DisplayHdrInfo::default());
        assert_eq!(profile.effective_sdr_white(), 203.0);
        assert!(!profile.is_hdr_capable());
    }

    #[test]
    fn from_display_hdr_info_with_absolute_nits() {
        let hdr_info = wgpu::DisplayHdrInfo {
            luminance: Some(wgpu::DisplayLuminance {
                max_nits: Some(1000.0),
                max_full_frame_nits: Some(600.0),
                min_nits: Some(0.005),
                sdr_white_nits: Some(203.0),
            }),
            headroom: None,
            chromaticity: None,
            coarse: None,
            bits_per_color: Some(10),
        };
        let profile = DisplayProfile::from_display_hdr_info(&hdr_info);
        assert_eq!(profile.max_luminance_nits, 1000.0);
        assert_eq!(profile.min_luminance_nits, 0.005);
        assert_eq!(profile.sdr_white_level_nits, Some(203.0));
        assert!(profile.is_hdr_capable());
    }

    #[test]
    fn from_display_hdr_info_with_edr_headroom() {
        let hdr_info = wgpu::DisplayHdrInfo {
            luminance: None,
            headroom: Some(wgpu::DisplayHeadroom {
                current: Some(3.0),
                potential: Some(4.0),
                reference: None,
            }),
            chromaticity: None,
            coarse: None,
            bits_per_color: None,
        };
        let profile = DisplayProfile::from_display_hdr_info(&hdr_info);
        // max_nits = sdr_white * headroom = 203 * 3 = 609
        assert!((profile.max_luminance_nits - 609.0).abs() < 1e-4);
        assert!(profile.is_hdr_capable());
    }

    #[test]
    fn from_display_hdr_info_coarse_sdr_uses_budget_fallback() {
        let hdr_info = wgpu::DisplayHdrInfo {
            luminance: None,
            headroom: None,
            chromaticity: None,
            coarse: Some(wgpu::DisplayCoarseRange {
                high_dynamic_range: Some(false),
                gamut: None,
            }),
            bits_per_color: None,
        };
        let profile = DisplayProfile::from_display_hdr_info(&hdr_info);
        // Budget laptop fallback has max 160 nits.
        assert_eq!(profile.max_luminance_nits, 160.0);
    }

    #[test]
    fn with_headroom_scales_max_luminance() {
        let sdr = DisplayProfile::default_sdr();
        let hdr = sdr.with_headroom(3.0);
        assert!((hdr.max_luminance_nits - 300.0 * 3.0).abs() < 1e-4);
    }

    #[test]
    fn with_headroom_clamps_non_finite() {
        let sdr = DisplayProfile::default_sdr();
        let hdr = sdr.with_headroom(f32::NAN);
        assert_eq!(hdr.max_luminance_nits, sdr.max_luminance_nits);
    }

    #[test]
    fn is_hdr_capable_distinguishes_sdr_and_hdr() {
        assert!(!DisplayProfile::default_sdr().is_hdr_capable());
        assert!(DisplayProfile::default_hdr10().is_hdr_capable());
    }

    #[test]
    fn display_capabilities_fallback_is_sdr() {
        let fallback = DisplayCapabilities::fallback();
        assert!(fallback.supported_formats.is_empty());
        assert!(fallback.supported_present_modes.is_empty());
        assert!(!fallback.hdr_capable);
        assert_eq!(fallback.bits_per_color, None);
        assert!(!fallback.profile.is_hdr_capable());
    }

    #[test]
    fn display_capabilities_default_is_fallback() {
        assert_eq!(
            DisplayCapabilities::default(),
            DisplayCapabilities::fallback()
        );
    }

    #[test]
    fn display_capabilities_from_wgpu_default() {
        let caps = DisplayCapabilities::from_wgpu(
            &wgpu::SurfaceCapabilities::default(),
            &wgpu::DisplayHdrInfo::default(),
        );
        assert!(caps.supported_formats.is_empty());
        assert!(!caps.hdr_capable);
    }

    #[test]
    fn display_capabilities_from_wgpu_with_hdr_formats() {
        let caps = wgpu::SurfaceCapabilities {
            formats: vec![wgpu::TextureFormat::Rgba16Float],
            format_capabilities: vec![wgpu::SurfaceFormatCapabilities {
                format: wgpu::TextureFormat::Rgba16Float,
                color_spaces: wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR,
            }],
            present_modes: vec![wgpu::PresentMode::Fifo],
            alpha_modes: vec![wgpu::CompositeAlphaMode::Opaque],
            usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
        };
        let hdr_info = wgpu::DisplayHdrInfo {
            luminance: Some(wgpu::DisplayLuminance {
                max_nits: Some(1000.0),
                max_full_frame_nits: None,
                min_nits: Some(0.005),
                sdr_white_nits: Some(203.0),
            }),
            headroom: None,
            chromaticity: None,
            coarse: None,
            bits_per_color: Some(10),
        };
        let display = DisplayCapabilities::from_wgpu(&caps, &hdr_info);
        assert!(display.hdr_capable);
        assert!(display
            .supported_formats
            .contains(&wgpu::TextureFormat::Rgba16Float));
        assert_eq!(display.bits_per_color, Some(10));
        assert!(display.profile.is_hdr_capable());
    }

    #[test]
    fn display_capabilities_best_format_fallback() {
        let fallback = DisplayCapabilities::fallback();
        assert_eq!(fallback.best_format(), wgpu::TextureFormat::Bgra8Unorm);
    }

    #[test]
    fn display_capabilities_best_format_prefers_hdr() {
        let caps = wgpu::SurfaceCapabilities {
            formats: vec![
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::TextureFormat::Rgba16Float,
            ],
            format_capabilities: vec![wgpu::SurfaceFormatCapabilities {
                format: wgpu::TextureFormat::Rgba16Float,
                color_spaces: wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR,
            }],
            present_modes: vec![wgpu::PresentMode::Fifo],
            alpha_modes: vec![wgpu::CompositeAlphaMode::Opaque],
            usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
        };
        let display = DisplayCapabilities::from_wgpu(&caps, &wgpu::DisplayHdrInfo::default());
        // HDR-capable, so Rgba16Float should be preferred.
        assert_eq!(display.best_format(), wgpu::TextureFormat::Rgba16Float);
    }

    #[test]
    fn display_capabilities_supports_present_mode() {
        let caps = wgpu::SurfaceCapabilities {
            formats: vec![],
            format_capabilities: vec![],
            present_modes: vec![wgpu::PresentMode::Fifo, wgpu::PresentMode::Mailbox],
            alpha_modes: vec![wgpu::CompositeAlphaMode::Opaque],
            usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
        };
        let display = DisplayCapabilities::from_wgpu(&caps, &wgpu::DisplayHdrInfo::default());
        assert!(display.supports_present_mode(wgpu::PresentMode::Fifo));
        assert!(display.supports_present_mode(wgpu::PresentMode::Mailbox));
        assert!(!display.supports_present_mode(wgpu::PresentMode::AutoVsync));
    }
}
