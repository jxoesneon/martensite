//! Oklab perceptual color pipeline.
//!
//! This module implements the Oklab perceptual color space as described in
//! Björn Ottosson's paper, along with the cylindrical [`Oklch`] representation,
//! hue-preserving gamut mapping, and WCAG 2.1 / APCA contrast calculations.
//!
//! All color blending in `martensite-theme` is performed in Oklab to avoid the
//! dark, muddy banding that linear sRGB interpolation produces along hue
//! boundaries.

use bytemuck::{Pod, Zeroable};
use core::f32::consts::TAU;

/// Converts a single sRGB channel value in `[0, 1]` to linear-light sRGB
/// using the standard sRGB gamma decoding function.
///
/// # Examples
///
/// ```
/// use martensite_theme::srgb_to_linear;
///
/// // Linear region: small values map almost linearly.
/// assert!((srgb_to_linear(0.04045) - 0.04045 / 12.92).abs() < 1e-6);
/// // White decodes to exactly 1.0.
/// assert_eq!(srgb_to_linear(1.0), 1.0);
/// // Black decodes to exactly 0.0.
/// assert_eq!(srgb_to_linear(0.0), 0.0);
/// ```
#[inline]
pub fn srgb_to_linear(c: f32) -> f32 {
    if !c.is_finite() {
        return 0.0;
    }
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Converts a single linear-light sRGB value to a gamma-encoded sRGB channel
/// in `[0, 1]` using the standard sRGB gamma encoding function.
///
/// The output is clamped to `[0, 1]`.
///
/// # Examples
///
/// ```
/// use martensite_theme::linear_to_srgb;
///
/// // Linear region: small values map almost linearly.
/// assert!((linear_to_srgb(0.0031308) - 0.0031308 * 12.92).abs() < 1e-6);
/// // White encodes to exactly 1.0.
/// assert_eq!(linear_to_srgb(1.0), 1.0);
/// // Black encodes to exactly 0.0.
/// assert_eq!(linear_to_srgb(0.0), 0.0);
/// // Out-of-gamut negative values are clamped to 0.0.
/// assert_eq!(linear_to_srgb(-0.5), 0.0);
/// ```
#[inline]
pub fn linear_to_srgb(c: f32) -> f32 {
    if c.is_nan() || c <= 0.0 {
        return 0.0;
    }
    if c >= 1.0 {
        return 1.0;
    }
    let result = if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    result.clamp(0.0, 1.0)
}

/// A color in the Oklab perceptual color space, stored as a 16-byte
/// C-compatible struct suitable for GPU uniform buffers.
///
/// The `l` channel is lightness (`0.0` = black, `1.0` = white), `a` is the
/// green-red chromaticity axis, and `b` is the blue-yellow chromaticity axis.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct Oklab {
    /// The lightness channel, ranging from `0.0` (black) to `1.0` (white).
    pub l: f32,
    /// The green-red chromaticity axis (`a`).
    pub a: f32,
    /// The blue-yellow chromaticity axis (`b`).
    pub b: f32,
    /// The alpha channel, where `0.0` is fully transparent and `1.0` is opaque.
    pub alpha: f32,
}

impl Oklab {
    /// Pure white in Oklab, equivalent to sRGB `(1, 1, 1, 1)`.
    pub const WHITE: Self = Self {
        l: 1.0,
        a: 0.0,
        b: 0.0,
        alpha: 1.0,
    };

    /// Pure black in Oklab, equivalent to sRGB `(0, 0, 0, 1)`.
    pub const BLACK: Self = Self {
        l: 0.0,
        a: 0.0,
        b: 0.0,
        alpha: 1.0,
    };

    /// Creates a new [`Oklab`] color from the given lightness, `a`, `b`, and
    /// alpha channels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::Oklab;
    ///
    /// let c = Oklab::new(0.5, 0.1, -0.1, 1.0);
    /// assert_eq!(c.l, 0.5);
    /// assert_eq!(c.a, 0.1);
    /// assert_eq!(c.b, -0.1);
    /// assert_eq!(c.alpha, 1.0);
    /// ```
    #[inline]
    pub const fn new(l: f32, a: f32, b: f32, alpha: f32) -> Self {
        Self { l, a, b, alpha }
    }

    /// Linearly interpolates between `self` and `other` by the factor `t`,
    /// performing uniform blending across all four Oklab channels.
    ///
    /// At `t = 0.0` the result is `self`, at `t = 1.0` it is `other`, and at
    /// `t = 0.5` it is the midpoint.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::Oklab;
    ///
    /// let a = Oklab::new(0.0, 0.0, 0.0, 0.0);
    /// let b = Oklab::new(1.0, 1.0, 1.0, 1.0);
    ///
    /// assert_eq!(a.lerp(b, 0.0), a);
    /// assert_eq!(a.lerp(b, 1.0), b);
    /// assert_eq!(a.lerp(b, 0.5), Oklab::new(0.5, 0.5, 0.5, 0.5));
    /// ```
    #[inline(always)]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        // Guard against non-finite t to prevent NaN propagation.
        // NaN → 0.0 (return self), Inf → 1.0 (return other), -Inf → 0.0.
        let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
        Self {
            l: self.l + (other.l - self.l) * t,
            a: self.a + (other.a - self.a) * t,
            b: self.b + (other.b - self.b) * t,
            alpha: self.alpha + (other.alpha - self.alpha) * t,
        }
    }

    /// Converts sRGB channel values in `[0, 1]` to [`Oklab`], first applying
    /// the sRGB gamma decode to obtain linear sRGB.
    ///
    /// Non-finite inputs are handled safely by returning [`Oklab::BLACK`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::Oklab;
    ///
    /// let white = Oklab::from_srgb(1.0, 1.0, 1.0);
    /// assert!((white.l - 1.0).abs() < 1e-4);
    /// assert!(white.a.abs() < 1e-4);
    /// assert!(white.b.abs() < 1e-4);
    ///
    /// let black = Oklab::from_srgb(0.0, 0.0, 0.0);
    /// assert_eq!(black, Oklab::BLACK);
    /// ```
    #[inline]
    pub fn from_srgb(r: f32, g: f32, b: f32) -> Self {
        if !r.is_finite() || !g.is_finite() || !b.is_finite() {
            return Self::BLACK;
        }
        Self::from_linear_srgb(srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b))
    }

    /// Converts linear sRGB channel values in `[0, 1]` to [`Oklab`] using the
    /// linear sRGB → LMS → Oklab transform from Björn Ottosson's paper.
    ///
    /// Non-finite inputs are handled safely by returning [`Oklab::BLACK`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::Oklab;
    ///
    /// let white = Oklab::from_linear_srgb(1.0, 1.0, 1.0);
    /// assert!((white.l - 1.0).abs() < 1e-4);
    /// ```
    #[inline]
    pub fn from_linear_srgb(r: f32, g: f32, b: f32) -> Self {
        if !r.is_finite() || !g.is_finite() || !b.is_finite() {
            return Self::BLACK;
        }
        let l = 0.412_221_46_f32 * r + 0.536_332_55_f32 * g + 0.051_445_995_f32 * b;
        let m = 0.211_903_5_f32 * r + 0.680_699_5_f32 * g + 0.107_396_96_f32 * b;
        let s = 0.088_302_46_f32 * r + 0.281_718_85_f32 * g + 0.629_978_7_f32 * b;

        let l_ = cbrt_safe(l);
        let m_ = cbrt_safe(m);
        let s_ = cbrt_safe(s);

        Self {
            l: 0.210_454_26_f32 * l_ + 0.793_617_8_f32 * m_ - 0.004_072_047_f32 * s_,
            a: 1.977_998_5_f32 * l_ - 2.428_592_2_f32 * m_ + 0.450_593_7_f32 * s_,
            b: 0.025_904_037_f32 * l_ + 0.782_771_77_f32 * m_ - 0.808_675_77_f32 * s_,
            alpha: 1.0,
        }
    }

    /// Converts this [`Oklab`] color back to sRGB channel values in `[0, 1]`,
    /// applying the sRGB gamma encode and clamping each channel to `[0, 1]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::Oklab;
    ///
    /// let (r, g, b) = Oklab::WHITE.to_srgb();
    /// assert!((r - 1.0).abs() < 1e-4);
    /// assert!((g - 1.0).abs() < 1e-4);
    /// assert!((b - 1.0).abs() < 1e-4);
    /// ```
    #[inline]
    pub fn to_srgb(&self) -> (f32, f32, f32) {
        let (r, g, b) = self.to_linear_srgb();
        (linear_to_srgb(r), linear_to_srgb(g), linear_to_srgb(b))
    }

    /// Converts this [`Oklab`] color to linear sRGB channel values using the
    /// inverse Oklab → LMS → linear sRGB transform.
    ///
    /// The returned values are *not* clamped; out-of-gamut colors may produce
    /// values below `0.0` or above `1.0`. Use [`Oklab::to_srgb`] for clamped,
    /// display-ready sRGB.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::Oklab;
    ///
    /// let (r, g, b) = Oklab::from_linear_srgb(0.5, 0.5, 0.5).to_linear_srgb();
    /// assert!((r - 0.5).abs() < 1e-4);
    /// assert!((g - 0.5).abs() < 1e-4);
    /// assert!((b - 0.5).abs() < 1e-4);
    /// ```
    #[inline]
    pub fn to_linear_srgb(&self) -> (f32, f32, f32) {
        let l_ = self.l + 0.396_337_78_f32 * self.a + 0.215_803_76_f32 * self.b;
        let m_ = self.l - 0.105_561_346_f32 * self.a - 0.063_853_174_f32 * self.b;
        let s_ = self.l - 0.089_484_18_f32 * self.a - 1.291_485_5_f32 * self.b;

        let l = l_ * l_ * l_;
        let m = m_ * m_ * m_;
        let s = s_ * s_ * s_;

        let r = 4.076_741_7_f32 * l - 3.307_711_6_f32 * m + 0.230_969_94_f32 * s;
        let g = -1.268_438_f32 * l + 2.609_757_4_f32 * m - 0.341_319_38_f32 * s;
        let b = -0.0041960863_f32 * l - 0.703_418_6_f32 * m + 1.707_614_7_f32 * s;

        (r, g, b)
    }

    /// Computes the Euclidean distance in Oklab space between `self` and
    /// `other`, a perceptual color difference (analogous to ΔE).
    ///
    /// The alpha channel is not included in the distance. The result is always
    /// non-negative and is exactly `0.0` for identical colors.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::Oklab;
    ///
    /// let a = Oklab::new(0.5, 0.1, 0.0, 1.0);
    /// assert_eq!(a.distance(&a), 0.0);
    ///
    /// let b = Oklab::new(0.5, 0.1, 0.1, 1.0);
    /// assert!(b.distance(&a) > 0.0);
    /// ```
    #[inline]
    pub fn distance(&self, other: &Self) -> f32 {
        let dl = self.l - other.l;
        let da = self.a - other.a;
        let db = self.b - other.b;
        (dl * dl + da * da + db * db).sqrt()
    }
}

/// Computes a cube root that safely handles negative and zero values.
///
/// `f32::cbrt` already handles negatives correctly, but this helper guards
/// against `NaN` propagation from non-finite inputs.
#[inline]
fn cbrt_safe(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    x.cbrt()
}

/// The cylindrical form of [`Oklab`], using lightness, chroma, and hue.
///
/// `l` is lightness (`0.0`–`1.0`), `c` is chroma (the magnitude of the `a`/`b`
/// vector; may be negative for mapping purposes), and `h` is the hue angle in
/// radians (`0.0`–`2π`).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Oklch {
    /// The lightness channel, ranging from `0.0` (black) to `1.0` (white).
    pub l: f32,
    /// The chroma, the magnitude of the `a`/`b` chromaticity vector.
    pub c: f32,
    /// The hue angle in radians, in the range `[0, 2π)`.
    pub h: f32,
    /// The alpha channel, where `0.0` is fully transparent and `1.0` is opaque.
    pub alpha: f32,
}

impl Oklch {
    /// Pure white in Oklch, equivalent to [`Oklab::WHITE`].
    pub const WHITE: Self = Self {
        l: 1.0,
        c: 0.0,
        h: 0.0,
        alpha: 1.0,
    };

    /// Pure black in Oklch, equivalent to [`Oklab::BLACK`].
    pub const BLACK: Self = Self {
        l: 0.0,
        c: 0.0,
        h: 0.0,
        alpha: 1.0,
    };

    /// Creates a new [`Oklch`] color from the given lightness, chroma, hue
    /// (in radians), and alpha channels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::Oklch;
    ///
    /// let c = Oklch::new(0.7, 0.1, 1.2, 1.0);
    /// assert_eq!(c.l, 0.7);
    /// assert_eq!(c.c, 0.1);
    /// assert_eq!(c.h, 1.2);
    /// assert_eq!(c.alpha, 1.0);
    /// ```
    #[inline]
    pub const fn new(l: f32, c: f32, h: f32, alpha: f32) -> Self {
        Self { l, c, h, alpha }
    }

    /// Converts an [`Oklab`] color to its cylindrical [`Oklch`] representation.
    ///
    /// Chroma is computed as `√(a² + b²)` and hue as `atan2(b, a)`, normalized
    /// to `[0, 2π)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::{Oklab, Oklch};
    ///
    /// let lab = Oklab::new(1.0, 0.0, 0.0, 1.0);
    /// let lch = Oklch::from_oklab(&lab);
    /// assert!((lch.l - 1.0).abs() < 1e-5);
    /// assert!(lch.c.abs() < 1e-5);
    /// ```
    #[inline]
    pub fn from_oklab(oklab: &Oklab) -> Self {
        let c = (oklab.a * oklab.a + oklab.b * oklab.b).sqrt();
        let h = oklab.b.atan2(oklab.a);
        let h = if h < 0.0 { h + TAU } else { h };
        Self {
            l: oklab.l,
            c,
            h,
            alpha: oklab.alpha,
        }
    }

    /// Converts this [`Oklch`] color back to the Cartesian [`Oklab`] form,
    /// where `a = c·cos(h)` and `b = c·sin(h)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_theme::{Oklab, Oklch};
    ///
    /// let lch = Oklch::new(0.5, 0.0, 0.0, 1.0);
    /// let lab = lch.to_oklab();
    /// assert!((lab.l - 0.5).abs() < 1e-5);
    /// assert!(lab.a.abs() < 1e-5);
    /// assert!(lab.b.abs() < 1e-5);
    /// ```
    #[inline]
    pub fn to_oklab(&self) -> Oklab {
        Oklab {
            l: self.l,
            a: self.c * self.h.cos(),
            b: self.c * self.h.sin(),
            alpha: self.alpha,
        }
    }

    /// Interpolates between `self` and `other` in Oklch space by the factor
    /// `t`, taking the *shortest path* around the hue circle.
    ///
    /// Lightness, chroma, and alpha are interpolated linearly. The hue delta
    /// is wrapped to `[-π, π]` so that, for example, interpolating from `350°`
    /// to `10°` travels forward through `0°` rather than backwards.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::f32::consts::PI;
    /// use martensite_theme::Oklch;
    ///
    /// let a = Oklch::new(0.5, 0.1, 350.0_f32.to_radians(), 1.0);
    /// let b = Oklch::new(0.5, 0.1, 10.0_f32.to_radians(), 1.0);
    /// let mid = a.lerp(b, 0.5);
    /// // Midpoint hue should be near 0° (i.e. 0 radians), not 180°.
    /// assert!(mid.h < 0.1 || mid.h > (2.0 * PI - 0.1));
    /// ```
    #[inline]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        // Guard against non-finite t to prevent NaN propagation.
        // NaN → 0.0 (return self), Inf → 1.0 (return other), -Inf → 0.0.
        let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
        let mut delta = other.h - self.h;
        // Wrap delta into (-π, π] for the shortest path.
        while delta > core::f32::consts::PI {
            delta -= TAU;
        }
        while delta < -core::f32::consts::PI {
            delta += TAU;
        }
        let h = self.h + delta * t;
        // Normalize result into [0, 2π).
        let h = h.rem_euclid(TAU);

        Self {
            l: self.l + (other.l - self.l) * t,
            c: self.c + (other.c - self.c) * t,
            h,
            alpha: self.alpha + (other.alpha - self.alpha) * t,
        }
    }
}

/// The target color gamut for [`gamut_map`].
///
/// Currently only standard sRGB displays are supported, but the enum allows
/// future expansion to wider gamuts (e.g. Display-P3).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Gamut {
    /// Standard sRGB display gamut (the typical monitor target).
    Srgb,
}

/// Returns `true` when the given [`Oklab`] color lies inside `target_gamut`.
///
/// For [`Gamut::Srgb`] this means every linear sRGB channel produced by
/// [`Oklab::to_linear_srgb`] falls within `[0, 1]`.
#[inline]
fn in_gamut(oklab: &Oklab, target_gamut: Gamut) -> bool {
    match target_gamut {
        Gamut::Srgb => {
            let (r, g, b) = oklab.to_linear_srgb();
            (0.0..=1.0).contains(&r) && (0.0..=1.0).contains(&g) && (0.0..=1.0).contains(&b)
        }
    }
}

/// Maps an [`Oklab`] color into `target_gamut` using hue-preserving chroma
/// reduction.
///
/// If the color is already in gamut it is returned unchanged. Otherwise the
/// color is converted to [`Oklch`], and a binary search reduces the chroma
/// while preserving lightness and hue until the resulting color lies inside
/// the gamut. This avoids the hue shifts that naive clamping would introduce.
///
/// # Examples
///
/// ```
/// use martensite_theme::{Gamut, Oklab, gamut_map};
///
/// // An in-gamut color is returned unchanged.
/// let white = Oklab::WHITE;
/// assert_eq!(gamut_map(white, Gamut::Srgb), white);
///
/// // An out-of-gamut color is pulled back inside sRGB.
/// let extreme = Oklab::new(0.7, 0.5, 0.5, 1.0);
/// let mapped = gamut_map(extreme, Gamut::Srgb);
/// let (r, g, b) = mapped.to_srgb();
/// assert!((0.0..=1.0).contains(&r));
/// assert!((0.0..=1.0).contains(&g));
/// assert!((0.0..=1.0).contains(&b));
/// ```
pub fn gamut_map(oklab: Oklab, target_gamut: Gamut) -> Oklab {
    if in_gamut(&oklab, target_gamut) {
        return oklab;
    }

    let lch = Oklch::from_oklab(&oklab);

    // Binary search the largest chroma that stays in gamut.
    let mut lo = 0.0_f32;
    let mut hi = lch.c;
    for _ in 0..32 {
        let mid = (lo + hi) * 0.5;
        let candidate = Oklch::new(lch.l, mid, lch.h, lch.alpha).to_oklab();
        if in_gamut(&candidate, target_gamut) {
            lo = mid;
        } else {
            hi = mid;
        }
    }

    Oklch::new(lch.l, lo, lch.h, lch.alpha).to_oklab()
}

/// Computes the relative luminance of an [`Oklab`] color in linear sRGB using
/// the WCAG 2.1 coefficients.
///
/// Non-finite inputs produce `0.0`.
#[inline]
fn relative_luminance(oklab: &Oklab) -> f32 {
    let (r, g, b) = oklab.to_linear_srgb();
    if !r.is_finite() || !g.is_finite() || !b.is_finite() {
        return 0.0;
    }
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Computes the WCAG 2.1 contrast ratio between a foreground and background
/// color, returning a value in the range `[1.0, 21.0]`.
///
/// The ratio is `(L_light + 0.05) / (L_dark + 0.05)` where `L` is the relative
/// luminance of each color in linear sRGB. Non-finite inputs are treated as
/// black (luminance `0.0`).
///
/// # Examples
///
/// ```
/// use martensite_theme::{Oklab, wcag_contrast};
///
/// // White on black and black on white both yield the maximum ratio.
/// assert!((wcag_contrast(Oklab::WHITE, Oklab::BLACK) - 21.0).abs() < 0.5);
/// assert!((wcag_contrast(Oklab::BLACK, Oklab::WHITE) - 21.0).abs() < 0.5);
/// // Identical colors yield the minimum ratio of 1.0.
/// assert!((wcag_contrast(Oklab::WHITE, Oklab::WHITE) - 1.0).abs() < 1e-3);
/// ```
pub fn wcag_contrast(foreground: Oklab, background: Oklab) -> f32 {
    let l_fg = relative_luminance(&foreground);
    let l_bg = relative_luminance(&background);
    let (light, dark) = if l_fg >= l_bg {
        (l_fg, l_bg)
    } else {
        (l_bg, l_fg)
    };
    let ratio = (light + 0.05) / (dark + 0.05);
    if ratio.is_finite() {
        ratio.clamp(1.0, 21.0)
    } else {
        1.0
    }
}

/// Computes the relative luminance of an [`Oklab`] color using the APCA
/// (Advanced Perceptual Contrast Algorithm) specific sRGB coefficients.
///
/// These differ slightly from the WCAG round-number coefficients to better
/// model perceptual lightness.
#[inline]
fn apca_luminance(oklab: &Oklab) -> f32 {
    let (r, g, b) = oklab.to_linear_srgb();
    if !r.is_finite() || !g.is_finite() || !b.is_finite() {
        return 0.0;
    }
    0.212_672_9_f32 * r + 0.715_152_2_f32 * g + 0.072_175_0_f32 * b
}

/// Applies the APCA soft black clamp: values below `0.022` are raised to the
/// power `1.414` to model the perceptual threshold near black.
#[inline]
fn apca_clamp_y(y: f32) -> f32 {
    if !y.is_finite() || y < 0.0 {
        return 0.0;
    }
    if y < 0.022 {
        y + (0.022 - y).powf(1.414)
    } else {
        y
    }
}

/// Computes the APCA (Advanced Perceptual Contrast Algorithm) lightness
/// contrast `Lc` value between a foreground (text) and background color.
///
/// Implements the APCA-7 formula as specified by the APCA Readability Criterion.
/// The result follows the APCA sign convention:
///
/// - **Positive** `Lc` → text is **darker** than the background (dark-on-light).
/// - **Negative** `Lc` → text is **lighter** than the background (light-on-dark).
///
/// The return value is clamped to `[-108, 108]`. Non-finite inputs produce
/// `0.0`.
///
/// # Examples
///
/// ```
/// use martensite_theme::{Oklab, apca_contrast};
///
/// // Black text on white background: large positive Lc (dark-on-light).
/// let lc = apca_contrast(Oklab::BLACK, Oklab::WHITE);
/// assert!(lc > 100.0, "black on white should be > 100, got {lc}");
///
/// // White text on black background: large negative Lc (light-on-dark).
/// let lc = apca_contrast(Oklab::WHITE, Oklab::BLACK);
/// assert!(lc < -100.0, "white on black should be < -100, got {lc}");
/// ```
pub fn apca_contrast(foreground: Oklab, background: Oklab) -> f32 {
    let y_text = apca_clamp_y(apca_luminance(&foreground));
    let y_bg = apca_clamp_y(apca_luminance(&background));

    // Soft-noise gate: if the difference is below the noise threshold, return 0.
    let diff = y_text - y_bg;
    if diff.abs() < 0.0005 {
        return 0.0;
    }

    // APCA-7 polarity-dependent exponents and scaling.
    // For dark text on light background (y_text < y_bg): positive Lc.
    // For light text on dark background (y_text > y_bg): negative Lc.
    let raw_sapc = if y_text < y_bg {
        // Dark text on light background.
        let y_t = y_text.powf(0.57);
        let y_b = y_bg.powf(0.56);
        (y_b - y_t) * 1.14
    } else {
        // Light text on dark background.
        let y_t = y_text.powf(0.62);
        let y_b = y_bg.powf(0.65);
        (y_t - y_b) * 1.14
    };

    if !raw_sapc.is_finite() {
        return 0.0;
    }

    // Low-contrast clip: raw |sapc| < 0.1 is clamped to 0 before offset.
    if raw_sapc.abs() < 0.1 {
        return 0.0;
    }

    // Apply the offset and scale to Lc (×100), preserving sign.
    // For dark-on-light (positive raw): subtract offset.
    // For light-on-dark (negative raw): the raw is positive here, so we
    // subtract the offset before negating, matching the reference APCA-7
    // which computes raw as negative and adds the offset.
    let lc = if y_text < y_bg {
        (raw_sapc - 0.027) * 100.0
    } else {
        -(raw_sapc - 0.027) * 100.0
    };

    if !lc.is_finite() {
        return 0.0;
    }

    lc.clamp(-108.0, 108.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::TAU;

    fn approx_eq(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn srgb_oklab_round_trip_white() {
        let lab = Oklab::from_srgb(1.0, 1.0, 1.0);
        let (r, g, b) = lab.to_srgb();
        assert!(approx_eq(r, 1.0, 1e-4), "r = {r}");
        assert!(approx_eq(g, 1.0, 1e-4), "g = {g}");
        assert!(approx_eq(b, 1.0, 1e-4), "b = {b}");
    }

    #[test]
    fn srgb_oklab_round_trip_black() {
        let lab = Oklab::from_srgb(0.0, 0.0, 0.0);
        let (r, g, b) = lab.to_srgb();
        assert!(approx_eq(r, 0.0, 1e-4));
        assert!(approx_eq(g, 0.0, 1e-4));
        assert!(approx_eq(b, 0.0, 1e-4));
    }

    #[test]
    fn srgb_oklab_round_trip_midgray() {
        let lab = Oklab::from_srgb(0.5, 0.5, 0.5);
        let (r, g, b) = lab.to_srgb();
        assert!(approx_eq(r, 0.5, 1e-4), "r = {r}");
        assert!(approx_eq(g, 0.5, 1e-4), "g = {g}");
        assert!(approx_eq(b, 0.5, 1e-4), "b = {b}");
    }

    #[test]
    fn srgb_oklab_round_trip_red() {
        let lab = Oklab::from_srgb(1.0, 0.0, 0.0);
        let (r, g, b) = lab.to_srgb();
        assert!(approx_eq(r, 1.0, 1e-4), "r = {r}");
        assert!(approx_eq(g, 0.0, 1e-4), "g = {g}");
        assert!(approx_eq(b, 0.0, 1e-4), "b = {b}");
    }

    #[test]
    fn srgb_oklab_round_trip_green() {
        let lab = Oklab::from_srgb(0.0, 1.0, 0.0);
        let (r, g, b) = lab.to_srgb();
        assert!(approx_eq(r, 0.0, 1e-4));
        assert!(approx_eq(g, 1.0, 1e-4));
        assert!(approx_eq(b, 0.0, 1e-4));
    }

    #[test]
    fn srgb_oklab_round_trip_blue() {
        let lab = Oklab::from_srgb(0.0, 0.0, 1.0);
        let (r, g, b) = lab.to_srgb();
        assert!(approx_eq(r, 0.0, 1e-4));
        assert!(approx_eq(g, 0.0, 1e-4));
        assert!(approx_eq(b, 1.0, 1e-4));
    }

    #[test]
    fn srgb_oklab_round_trip_random() {
        let cases = [
            (0.123, 0.456, 0.789),
            (0.8, 0.2, 0.6),
            (0.33, 0.66, 0.99),
            (0.05, 0.95, 0.5),
            (0.25, 0.25, 0.75),
        ];
        for &(r0, g0, b0) in &cases {
            let lab = Oklab::from_srgb(r0, g0, b0);
            let (r, g, b) = lab.to_srgb();
            assert!(
                approx_eq(r, r0, 1e-4),
                "r round-trip failed: {r0} -> {r} (lab = {lab:?})"
            );
            assert!(
                approx_eq(g, g0, 1e-4),
                "g round-trip failed: {g0} -> {g} (lab = {lab:?})"
            );
            assert!(
                approx_eq(b, b0, 1e-4),
                "b round-trip failed: {b0} -> {b} (lab = {lab:?})"
            );
        }
    }

    #[test]
    fn oklab_oklch_round_trip_preserves_l_and_alpha() {
        let lab = Oklab::new(0.6, 0.1, -0.05, 0.7);
        let lch = Oklch::from_oklab(&lab);
        assert!(approx_eq(lch.l, lab.l, 1e-5));
        assert!(approx_eq(lch.alpha, lab.alpha, 1e-5));
        let back = lch.to_oklab();
        assert!(approx_eq(back.l, lab.l, 1e-5));
        assert!(approx_eq(back.a, lab.a, 1e-5));
        assert!(approx_eq(back.b, lab.b, 1e-5));
        assert!(approx_eq(back.alpha, lab.alpha, 1e-5));
    }

    #[test]
    fn oklab_oklch_round_trip_achromatic() {
        let lab = Oklab::new(0.5, 0.0, 0.0, 1.0);
        let lch = Oklch::from_oklab(&lab);
        assert!(approx_eq(lch.c, 0.0, 1e-6));
        let back = lch.to_oklab();
        assert!(approx_eq(back.a, 0.0, 1e-6));
        assert!(approx_eq(back.b, 0.0, 1e-6));
    }

    #[test]
    fn oklch_hue_in_range() {
        let lab = Oklab::new(0.5, -0.1, -0.1, 1.0);
        let lch = Oklch::from_oklab(&lab);
        assert!((0.0..TAU).contains(&lch.h), "h = {} out of range", lch.h);
    }

    #[test]
    fn oklch_lerp_shortest_path_forward() {
        // From 350° to 10° should go through 0° (forward), not backwards.
        let a = Oklch::new(0.5, 0.1, 350.0_f32.to_radians(), 1.0);
        let b = Oklch::new(0.5, 0.1, 10.0_f32.to_radians(), 1.0);
        let mid = a.lerp(b, 0.5);
        // Midpoint should be near 0° = 0 rad (or very close to 2π).
        assert!(
            mid.h < 0.1 || mid.h > TAU - 0.1,
            "midpoint hue {} should be near 0",
            mid.h
        );
    }

    #[test]
    fn oklch_lerp_shortest_path_backward() {
        // From 10° to 350° should also go through 0°.
        let a = Oklch::new(0.5, 0.1, 10.0_f32.to_radians(), 1.0);
        let b = Oklch::new(0.5, 0.1, 350.0_f32.to_radians(), 1.0);
        let mid = a.lerp(b, 0.5);
        assert!(
            mid.h < 0.1 || mid.h > TAU - 0.1,
            "midpoint hue {} should be near 0",
            mid.h
        );
    }

    #[test]
    fn oklch_lerp_endpoints() {
        let a = Oklch::new(0.3, 0.1, 0.5, 1.0);
        let b = Oklch::new(0.7, 0.2, 1.5, 0.5);
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
    }

    #[test]
    fn oklch_lerp_midpoint_values() {
        let a = Oklch::new(0.0, 0.0, 0.0, 0.0);
        let b = Oklch::new(1.0, 1.0, 0.0, 1.0);
        let mid = a.lerp(b, 0.5);
        assert!(approx_eq(mid.l, 0.5, 1e-6));
        assert!(approx_eq(mid.c, 0.5, 1e-6));
        assert!(approx_eq(mid.alpha, 0.5, 1e-6));
    }

    #[test]
    fn gamut_map_in_gamut_unchanged() {
        let white = Oklab::WHITE;
        assert_eq!(gamut_map(white, Gamut::Srgb), white);
        let gray = Oklab::from_srgb(0.5, 0.5, 0.5);
        assert_eq!(gamut_map(gray, Gamut::Srgb), gray);
    }

    #[test]
    fn gamut_map_out_of_gamut_brought_inside() {
        let extreme = Oklab::new(0.7, 0.5, 0.5, 1.0);
        let mapped = gamut_map(extreme, Gamut::Srgb);
        let (r, g, b) = mapped.to_linear_srgb();
        assert!((-1e-4..=1.0 + 1e-4).contains(&r), "r = {r}");
        assert!((-1e-4..=1.0 + 1e-4).contains(&g), "g = {g}");
        assert!((-1e-4..=1.0 + 1e-4).contains(&b), "b = {b}");
    }

    #[test]
    fn gamut_map_preserves_hue() {
        let extreme = Oklab::new(0.7, 0.5, 0.5, 1.0);
        let original_lch = Oklch::from_oklab(&extreme);
        let mapped = gamut_map(extreme, Gamut::Srgb);
        let mapped_lch = Oklch::from_oklab(&mapped);
        // Hue should be preserved (modulo wrap-around).
        let hue_diff = (original_lch.h - mapped_lch.h).abs();
        assert!(
            !(1e-3..=TAU - 1e-3).contains(&hue_diff),
            "hue changed from {} to {}",
            original_lch.h,
            mapped_lch.h
        );
    }

    #[test]
    fn gamut_map_preserves_lightness() {
        let extreme = Oklab::new(0.7, 0.5, 0.5, 1.0);
        let mapped = gamut_map(extreme, Gamut::Srgb);
        assert!(
            approx_eq(mapped.l, extreme.l, 1e-4),
            "lightness changed from {} to {}",
            extreme.l,
            mapped.l
        );
    }

    #[test]
    fn wcag_contrast_white_on_black() {
        let c = wcag_contrast(Oklab::WHITE, Oklab::BLACK);
        assert!(approx_eq(c, 21.0, 0.5), "white on black = {c}");
    }

    #[test]
    fn wcag_contrast_black_on_white() {
        let c = wcag_contrast(Oklab::BLACK, Oklab::WHITE);
        assert!(approx_eq(c, 21.0, 0.5), "black on white = {c}");
    }

    #[test]
    fn wcag_contrast_same_color() {
        let c = wcag_contrast(Oklab::WHITE, Oklab::WHITE);
        assert!(approx_eq(c, 1.0, 1e-3), "same color = {c}");
        let gray = Oklab::from_srgb(0.3, 0.3, 0.3);
        let c2 = wcag_contrast(gray, gray);
        assert!(approx_eq(c2, 1.0, 1e-3), "same gray = {c2}");
    }

    #[test]
    fn wcag_contrast_range() {
        let c = wcag_contrast(Oklab::WHITE, Oklab::BLACK);
        assert!((1.0..=21.0).contains(&c));
    }

    #[test]
    fn apca_contrast_white_on_black() {
        // White text on black background: light-on-dark → negative Lc.
        let lc = apca_contrast(Oklab::WHITE, Oklab::BLACK);
        assert!(lc < -100.0, "white on black Lc = {lc}");
        assert!(lc >= -108.0);
    }

    #[test]
    fn apca_contrast_black_on_white() {
        // Black text on white background: dark-on-light → positive Lc.
        let lc = apca_contrast(Oklab::BLACK, Oklab::WHITE);
        assert!(lc > 100.0, "black on white Lc = {lc}");
        assert!(lc <= 108.0);
    }

    #[test]
    fn apca_contrast_same_color() {
        let lc = apca_contrast(Oklab::WHITE, Oklab::WHITE);
        assert!(approx_eq(lc, 0.0, 1e-3), "same color Lc = {lc}");
    }

    #[test]
    fn apca_contrast_nan_inputs_are_safe() {
        // NaN Oklab colors are treated as black (luminance 0) by the contrast
        // functions, producing a finite, valid contrast value rather than NaN.
        let nan_color = Oklab::new(f32::NAN, 0.0, 0.0, 1.0);
        let lc = apca_contrast(nan_color, Oklab::WHITE);
        assert!(
            lc.is_finite(),
            "NaN foreground must produce finite Lc, got {lc}"
        );
        let lc2 = apca_contrast(Oklab::WHITE, nan_color);
        assert!(
            lc2.is_finite(),
            "NaN background must produce finite Lc, got {lc2}"
        );
    }

    #[test]
    fn wcag_contrast_nan_inputs_are_safe() {
        // NaN Oklab colors are treated as black (luminance 0), producing a
        // finite contrast ratio rather than NaN/Inf.
        let nan_color = Oklab::new(f32::NAN, 0.0, 0.0, 1.0);
        let c = wcag_contrast(nan_color, Oklab::WHITE);
        assert!(
            c.is_finite(),
            "NaN inputs must produce finite ratio, got {c}"
        );
        assert!(
            (1.0..=21.0).contains(&c),
            "ratio must be in [1, 21], got {c}"
        );
    }

    #[test]
    fn apca_contrast_mid_gray_on_dark_is_moderate_negative() {
        // Non-extreme light-on-dark: mid-gray text on a dark background.
        // This exercises the offset sign without hitting the ±108 clamp.
        let text = Oklab::from_srgb(0.5, 0.5, 0.5);
        let bg = Oklab::from_srgb(0.1, 0.1, 0.1);
        let lc = apca_contrast(text, bg);
        // Light-on-dark should be negative.
        assert!(lc < 0.0, "light-on-dark should be negative, got {lc}");
        // Should be moderate (not extreme): roughly -30 to -80 Lc.
        assert!(
            lc > -90.0 && lc < -20.0,
            "mid-gray on dark should be moderate, got {lc}"
        );
    }

    #[test]
    fn apca_contrast_mid_gray_on_light_is_moderate_positive() {
        // Non-extreme dark-on-light: mid-gray text on a light background.
        let text = Oklab::from_srgb(0.3, 0.3, 0.3);
        let bg = Oklab::from_srgb(0.8, 0.8, 0.8);
        let lc = apca_contrast(text, bg);
        // Dark-on-light should be positive.
        assert!(lc > 0.0, "dark-on-light should be positive, got {lc}");
        // Should be moderate: roughly 20 to 80 Lc.
        assert!(
            lc < 90.0 && lc > 20.0,
            "mid-gray on light should be moderate, got {lc}"
        );
    }

    #[test]
    fn known_oklab_white() {
        let lab = Oklab::from_srgb(1.0, 1.0, 1.0);
        assert!(approx_eq(lab.l, 1.0, 1e-4), "white L = {}", lab.l);
        assert!(lab.a.abs() < 1e-4, "white a = {}", lab.a);
        assert!(lab.b.abs() < 1e-4, "white b = {}", lab.b);
    }

    #[test]
    fn known_oklab_black() {
        let lab = Oklab::from_srgb(0.0, 0.0, 0.0);
        assert!(approx_eq(lab.l, 0.0, 1e-4), "black L = {}", lab.l);
        assert!(lab.a.abs() < 1e-4);
        assert!(lab.b.abs() < 1e-4);
    }

    #[test]
    fn known_oklab_midgray() {
        let lab = Oklab::from_srgb(0.5, 0.5, 0.5);
        assert!(
            approx_eq(lab.l, 0.596, 1e-2),
            "midgray L = {}, expected ~0.596",
            lab.l
        );
    }

    #[test]
    fn distance_non_negative_and_zero_for_identical() {
        let a = Oklab::new(0.5, 0.1, -0.1, 1.0);
        assert_eq!(a.distance(&a), 0.0);
        let b = Oklab::new(0.6, 0.1, -0.1, 1.0);
        assert!(a.distance(&b) > 0.0);
    }

    #[test]
    fn distance_excludes_alpha() {
        let a = Oklab::new(0.5, 0.1, 0.0, 1.0);
        let b = Oklab::new(0.5, 0.1, 0.0, 0.0);
        assert_eq!(a.distance(&b), 0.0);
    }

    #[test]
    fn from_srgb_nan_returns_black() {
        let lab = Oklab::from_srgb(f32::NAN, 0.5, 0.5);
        assert_eq!(lab, Oklab::BLACK);
    }

    #[test]
    fn from_srgb_inf_returns_black() {
        let lab = Oklab::from_srgb(f32::INFINITY, 0.5, 0.5);
        assert_eq!(lab, Oklab::BLACK);
    }

    #[test]
    fn from_linear_srgb_nan_returns_black() {
        let lab = Oklab::from_linear_srgb(f32::NAN, 0.5, 0.5);
        assert_eq!(lab, Oklab::BLACK);
    }

    #[test]
    fn from_linear_srgb_neg_inf_returns_black() {
        let lab = Oklab::from_linear_srgb(f32::NEG_INFINITY, 0.5, 0.5);
        assert_eq!(lab, Oklab::BLACK);
    }

    #[test]
    fn lerp_at_t_zero_returns_self() {
        let a = Oklab::new(0.1, 0.2, 0.3, 0.4);
        let b = Oklab::new(0.5, 0.6, 0.7, 0.8);
        assert_eq!(a.lerp(b, 0.0), a);
    }

    #[test]
    fn lerp_at_t_one_returns_other() {
        let a = Oklab::new(0.1, 0.2, 0.3, 0.4);
        let b = Oklab::new(0.5, 0.6, 0.7, 0.8);
        assert_eq!(a.lerp(b, 1.0), b);
    }

    #[test]
    fn lerp_at_t_half_returns_midpoint() {
        let a = Oklab::new(0.0, 0.0, 0.0, 0.0);
        let b = Oklab::new(1.0, 1.0, 1.0, 1.0);
        let result = a.lerp(b, 0.5);
        assert_eq!(result, Oklab::new(0.5, 0.5, 0.5, 0.5));
    }

    #[test]
    fn lerp_interpolates_all_channels() {
        let a = Oklab::new(0.0, 0.0, 0.0, 0.0);
        let b = Oklab::new(1.0, 2.0, 3.0, 4.0);
        let result = a.lerp(b, 0.25);
        assert_eq!(result.l, 0.25);
        assert_eq!(result.a, 0.5);
        assert_eq!(result.b, 0.75);
        assert_eq!(result.alpha, 1.0);
    }

    #[test]
    fn oklab_is_copy_clone_debug_eq() {
        let a = Oklab::new(0.1, 0.2, 0.3, 0.4);
        let cloned = a;
        assert_eq!(a, cloned);
        let debug_str = format!("{a:?}");
        assert!(debug_str.contains("Oklab"));
    }

    #[test]
    fn oklab_is_pod_and_zeroable() {
        let a = Oklab::new(0.1, 0.2, 0.3, 0.4);
        // bytes_of requires Pod; this call proves the trait is implemented.
        let bytes = bytemuck::bytes_of(&a);
        assert_eq!(bytes.len(), 16);
        // zeroed requires Zeroable; this call proves the trait is implemented.
        let zeroed = Oklab::zeroed();
        assert_eq!(zeroed.l, 0.0);
        assert_eq!(zeroed.a, 0.0);
        assert_eq!(zeroed.b, 0.0);
        assert_eq!(zeroed.alpha, 0.0);
    }

    #[test]
    fn oklab_is_16_bytes() {
        assert_eq!(core::mem::size_of::<Oklab>(), 16);
    }

    #[test]
    fn srgb_to_linear_known_values() {
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert_eq!(srgb_to_linear(1.0), 1.0);
        // Boundary value 0.04045 uses the linear branch.
        assert!(approx_eq(srgb_to_linear(0.04045), 0.04045 / 12.92, 1e-6));
    }

    #[test]
    fn linear_to_srgb_known_values() {
        assert_eq!(linear_to_srgb(0.0), 0.0);
        assert_eq!(linear_to_srgb(1.0), 1.0);
        // Boundary value 0.0031308 uses the linear branch.
        assert!(approx_eq(
            linear_to_srgb(0.0031308),
            0.0031308 * 12.92,
            1e-6
        ));
    }

    #[test]
    fn linear_to_srgb_clamps() {
        assert_eq!(linear_to_srgb(-0.5), 0.0);
        assert_eq!(linear_to_srgb(2.0), 1.0);
    }

    #[test]
    fn srgb_to_linear_nan_returns_zero() {
        assert_eq!(srgb_to_linear(f32::NAN), 0.0);
        assert_eq!(srgb_to_linear(f32::INFINITY), 0.0);
    }

    #[test]
    fn oklch_constants() {
        assert_eq!(Oklch::WHITE.l, 1.0);
        assert_eq!(Oklch::BLACK.l, 0.0);
        assert_eq!(Oklch::WHITE.alpha, 1.0);
    }

    #[test]
    fn oklab_constants() {
        assert_eq!(Oklab::WHITE.l, 1.0);
        assert_eq!(Oklab::WHITE.alpha, 1.0);
        assert_eq!(Oklab::BLACK.l, 0.0);
        assert_eq!(Oklab::BLACK.alpha, 1.0);
    }
}
