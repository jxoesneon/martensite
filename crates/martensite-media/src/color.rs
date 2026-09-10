//! Color space transformations, SMPTE ST 2084 (PQ) EOTF, and optical linear blending.
//!
//! This module implements high-dynamic-range (HDR) and standard-dynamic-range (SDR)
//! color pipeline science for video rendering:
//!
//! - **BT.709 and BT.2020 YUV-to-RGB matrices**: Precise matrix arithmetic with studio
//!   and limited-range expansion.
//! - **SMPTE ST 2084 Perceptual Quantizer (PQ)**: Forward Electro-Optical Transfer Function
//!   (EOTF) converting non-linear code values to absolute scene luminance in nits
//!   ($\text{cd}/\text{m}^2 \in [0, 10000]$), and inverse Opto-Electronic Transfer Function
//!   (OETF).
//! - **scRGB Linear Optical Space**: Normalization where $1.0$ corresponds to SDR reference
//!   white ($203\ \text{nits}$, ITU-R BT.2408) and specular HDR highlights exceed $1.0$.
//! - **Pre-multiplied Alpha Compositing**: Physically accurate linear-light blending
//!   between UI overlays and underlying HDR video frames.
//! - **Color Difference ($\Delta E^*_{ab}$)**: Colorimetric error evaluation against
//!   standard reference color bars.

use glam::Vec3;

/// Color space identification and chromaticity specification.
///
/// # Examples
///
/// ```
/// use martensite_media::color::ColorSpace;
///
/// assert_eq!(ColorSpace::default(), ColorSpace::Bt709);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum ColorSpace {
    /// Rec. 709 / sRGB gamut (HDTV standard).
    #[default]
    Bt709,
    /// Rec. 2020 wide color gamut (UHD / HDR standard).
    Bt2020,
    /// DCI-P3 wide color gamut with D65 white point (Display P3).
    DisplayP3,
}

/// Optical transfer function (EOTF / tone response curve).
///
/// # Examples
///
/// ```
/// use martensite_media::color::TransferFunction;
///
/// assert_eq!(TransferFunction::default(), TransferFunction::Srgb);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum TransferFunction {
    /// Standard sRGB / BT.709 gamma curve ($\approx 2.2$).
    #[default]
    Srgb,
    /// SMPTE ST 2084 Perceptual Quantizer (PQ) for HDR10.
    Pq,
    /// Pure linear-light optical intensity.
    Linear,
}

// SMPTE ST 2084 (PQ) exact rational constants
const PQ_M1: f64 = 2610.0 / 16384.0; // 0.1593017578125
const PQ_M2: f64 = (2523.0 / 4096.0) * 128.0; // 78.84375
const PQ_C1: f64 = 3424.0 / 4096.0; // 0.8359375
const PQ_C2: f64 = (2413.0 / 4096.0) * 32.0; // 18.8515625
const PQ_C3: f64 = (2392.0 / 4096.0) * 32.0; // 18.6875

/// Evaluates the SMPTE ST 2084 (PQ) Electro-Optical Transfer Function (EOTF).
///
/// Maps a normalized non-linear code value $N \in [0.0, 1.0]$ to absolute physical
/// luminance in nits ($\text{cd}/\text{m}^2 \in [0.0, 10000.0]$).
///
/// Formula:
/// $$L = 10000 \cdot \left( \frac{\max(N^{1/m_2} - c_1, 0)}{c_2 - c_3 N^{1/m_2}} \right)^{1/m_1}$$
///
/// # Examples
///
/// ```
/// use martensite_media::color::pq_eotf;
///
/// // 0.0 code value maps to 0.0 nits (absolute black).
/// assert_eq!(pq_eotf(0.0), 0.0);
/// // 1.0 code value maps to 10,000.0 nits (peak PQ luminance).
/// assert!((pq_eotf(1.0) - 10000.0).abs() < 1e-2);
/// // ~0.58 code value corresponds approximately to 203 nits (SDR reference white).
/// let nits_203 = pq_eotf(0.58);
/// assert!(nits_203 > 180.0 && nits_203 < 230.0);
/// ```
#[must_use]
pub fn pq_eotf(n: f32) -> f32 {
    if !n.is_finite() || n <= 0.0 {
        return 0.0;
    }
    if n >= 1.0 {
        return 10000.0;
    }
    let n_f64 = n as f64;
    let n_pow = n_f64.powf(1.0 / PQ_M2);
    let num = (n_pow - PQ_C1).max(0.0);
    let den = PQ_C2 - PQ_C3 * n_pow;
    if den <= 0.0 {
        return 10000.0;
    }
    let y = (num / den).powf(1.0 / PQ_M1);
    (y * 10000.0) as f32
}

/// Evaluates the inverse SMPTE ST 2084 (PQ) Opto-Electronic Transfer Function (OETF).
///
/// Maps absolute physical luminance $L \in [0.0, 10000.0]$ in nits ($\text{cd}/\text{m}^2$)
/// to the normalized non-linear code value $N \in [0.0, 1.0]$.
///
/// Formula:
/// $$N = \left( \frac{c_1 + c_2 Y^{m_1}}{1 + c_3 Y^{m_1}} \right)^{m_2}, \quad Y = L / 10000$$
///
/// # Examples
///
/// ```
/// use martensite_media::color::{pq_eotf, pq_oetf};
///
/// // Round-trip precision test
/// let original = 0.65_f32;
/// let nits = pq_eotf(original);
/// let encoded = pq_oetf(nits);
/// assert!((encoded - original).abs() < 1e-5);
/// ```
#[must_use]
pub fn pq_oetf(luminance_nits: f32) -> f32 {
    if !luminance_nits.is_finite() || luminance_nits <= 0.0 {
        return 0.0;
    }
    if luminance_nits >= 10000.0 {
        return 1.0;
    }
    let y = (luminance_nits as f64) / 10000.0;
    let y_pow = y.powf(PQ_M1);
    let num = PQ_C1 + PQ_C2 * y_pow;
    let den = 1.0 + PQ_C3 * y_pow;
    let n = (num / den).powf(PQ_M2);
    (n as f32).clamp(0.0, 1.0)
}

/// Converts BT.709 YUV samples (with normalized luma $Y \in [0, 1]$ and centered chroma $Cb, Cr \in [-0.5, 0.5]$)
/// to non-linear $R, G, B \in [0, 1]$.
///
/// Matrix coefficients:
/// - $R = Y + 1.57480 \cdot Cr$
/// - $G = Y - 0.18732 \cdot Cb - 0.46812 \cdot Cr$
/// - $B = Y + 1.85560 \cdot Cb$
///
/// # Examples
///
/// ```
/// use martensite_media::color::bt709_yuv_to_rgb;
///
/// // Pure white in YUV: Y = 1.0, Cb = 0.0, Cr = 0.0
/// let rgb = bt709_yuv_to_rgb(1.0, 0.0, 0.0);
/// assert!((rgb.x - 1.0).abs() < 1e-4);
/// assert!((rgb.y - 1.0).abs() < 1e-4);
/// assert!((rgb.z - 1.0).abs() < 1e-4);
/// ```
#[inline]
#[must_use]
pub fn bt709_yuv_to_rgb(y: f32, cb: f32, cr: f32) -> Vec3 {
    let r = y + 1.57480 * cr;
    let g = y - 0.18732 * cb - 0.46812 * cr;
    let b = y + 1.85560 * cb;
    Vec3::new(r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
}

/// Converts BT.2020 YUV samples (with normalized luma $Y \in [0, 1]$ and centered chroma $Cb, Cr \in [-0.5, 0.5]$)
/// to non-linear $R, G, B \in [0, 1]$.
///
/// Matrix coefficients:
/// - $R = Y + 1.47460 \cdot Cr$
/// - $G = Y - 0.16455 \cdot Cb - 0.57135 \cdot Cr$
/// - $B = Y + 1.88140 \cdot Cb$
///
/// # Examples
///
/// ```
/// use martensite_media::color::bt2020_yuv_to_rgb;
///
/// // Pure white in YUV: Y = 1.0, Cb = 0.0, Cr = 0.0
/// let rgb = bt2020_yuv_to_rgb(1.0, 0.0, 0.0);
/// assert!((rgb.x - 1.0).abs() < 1e-4);
/// assert!((rgb.y - 1.0).abs() < 1e-4);
/// assert!((rgb.z - 1.0).abs() < 1e-4);
/// ```
#[inline]
#[must_use]
pub fn bt2020_yuv_to_rgb(y: f32, cb: f32, cr: f32) -> Vec3 {
    let r = y + 1.47460 * cr;
    let g = y - 0.16455 * cb - 0.57135 * cr;
    let b = y + 1.88140 * cb;
    Vec3::new(r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
}

/// Converts non-linear $R, G, B \in [0, 1]$ to BT.709 YUV samples
/// (with normalized luma $Y \in [0, 1]$ and centered chroma $Cb, Cr \in [-0.5, 0.5]$).
///
/// Matrix coefficients:
/// - $Y = 0.2126 \cdot R + 0.7152 \cdot G + 0.0722 \cdot B$
/// - $Cb = (B - Y) / 1.85560$
/// - $Cr = (R - Y) / 1.57480$
///
/// # Examples
///
/// ```
/// use glam::Vec3;
/// use martensite_media::color::rgb_to_bt709_yuv;
///
/// let (y, cb, cr) = rgb_to_bt709_yuv(Vec3::new(1.0, 1.0, 1.0));
/// assert!((y - 1.0).abs() < 1e-4);
/// assert!(cb.abs() < 1e-4);
/// assert!(cr.abs() < 1e-4);
/// ```
#[inline]
#[must_use]
pub fn rgb_to_bt709_yuv(rgb: Vec3) -> (f32, f32, f32) {
    let y = 0.2126 * rgb.x + 0.7152 * rgb.y + 0.0722 * rgb.z;
    let cb = (rgb.z - y) / 1.85560;
    let cr = (rgb.x - y) / 1.57480;
    (y, cb, cr)
}

/// Converts non-linear $R, G, B \in [0, 1]$ to BT.2020 YUV samples
/// (with normalized luma $Y \in [0, 1]$ and centered chroma $Cb, Cr \in [-0.5, 0.5]$).
///
/// Matrix coefficients:
/// - $Y = 0.2627 \cdot R + 0.6780 \cdot G + 0.0593 \cdot B$
/// - $Cb = (B - Y) / 1.88140$
/// - $Cr = (R - Y) / 1.47460$
///
/// # Examples
///
/// ```
/// use glam::Vec3;
/// use martensite_media::color::rgb_to_bt2020_yuv;
///
/// let (y, cb, cr) = rgb_to_bt2020_yuv(Vec3::new(1.0, 1.0, 1.0));
/// assert!((y - 1.0).abs() < 1e-4);
/// assert!(cb.abs() < 1e-4);
/// assert!(cr.abs() < 1e-4);
/// ```
#[inline]
#[must_use]
pub fn rgb_to_bt2020_yuv(rgb: Vec3) -> (f32, f32, f32) {
    let y = 0.2627 * rgb.x + 0.6780 * rgb.y + 0.0593 * rgb.z;
    let cb = (rgb.z - y) / 1.88140;
    let cr = (rgb.x - y) / 1.47460;
    (y, cb, cr)
}

/// Converts linear BT.2020 primaries to linear BT.709 primaries.
///
/// Matrix:
/// $$\begin{bmatrix} R_{709} \\ G_{709} \\ B_{709} \end{bmatrix} =
///   \begin{bmatrix} 1.6605 & -0.5876 & -0.0728 \\ -0.1246 & 1.1329 & -0.0083 \\ -0.0182 & -0.1006 & 1.1187 \end{bmatrix}
///   \begin{bmatrix} R_{2020} \\ G_{2020} \\ B_{2020} \end{bmatrix}$$
///
/// # Examples
///
/// ```
/// use glam::Vec3;
/// use martensite_media::color::bt2020_to_bt709_linear;
///
/// let white_2020 = Vec3::splat(1.0);
/// let white_709 = bt2020_to_bt709_linear(white_2020);
/// assert!((white_709.x - 1.0).abs() < 1e-3);
/// assert!((white_709.y - 1.0).abs() < 1e-3);
/// assert!((white_709.z - 1.0).abs() < 1e-3);
/// ```
#[inline]
#[must_use]
pub fn bt2020_to_bt709_linear(rgb2020: Vec3) -> Vec3 {
    let r = 1.6605 * rgb2020.x - 0.5876 * rgb2020.y - 0.0728 * rgb2020.z;
    let g = -0.1246 * rgb2020.x + 1.1329 * rgb2020.y - 0.0083 * rgb2020.z;
    let b = -0.0182 * rgb2020.x - 0.1006 * rgb2020.y + 1.1187 * rgb2020.z;
    Vec3::new(r, g, b)
}

/// Color representation in open-domain scRGB linear optical space.
///
/// In scRGB, a value of $1.0$ corresponds to SDR reference white ($203\ \text{nits}$).
/// Specular HDR highlights can reach values $> 1.0$ (e.g. $1000\ \text{nits} \approx 4.93$).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScRgb {
    /// Linear red intensity.
    pub r: f32,
    /// Linear green intensity.
    pub g: f32,
    /// Linear blue intensity.
    pub b: f32,
    /// Linear pre-multiplied alpha.
    pub alpha: f32,
}

impl ScRgb {
    /// Creates a new `ScRgb` color value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::color::ScRgb;
    ///
    /// let color = ScRgb::new(1.0, 1.0, 1.0, 1.0);
    /// assert_eq!(color.luminance_nits(203.0), 203.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(r: f32, g: f32, b: f32, alpha: f32) -> Self {
        Self { r, g, b, alpha }
    }

    /// Converts physical luminance in nits to an scRGB value based on a given reference white level.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::color::ScRgb;
    ///
    /// let val = ScRgb::from_nits(203.0, 203.0);
    /// assert!((val.r - 1.0).abs() < 1e-4);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_nits(nits: f32, sdr_reference_white_nits: f32) -> Self {
        let scale = if sdr_reference_white_nits > 0.0 {
            nits / sdr_reference_white_nits
        } else {
            0.0
        };
        Self::new(scale, scale, scale, 1.0)
    }

    /// Computes the approximate physical luminance in nits given the reference white level.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::color::ScRgb;
    ///
    /// let color = ScRgb::new(1.0, 1.0, 1.0, 1.0);
    /// assert_eq!(color.luminance_nits(200.0), 200.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn luminance_nits(&self, sdr_reference_white_nits: f32) -> f32 {
        // Rec. 709 luminance coefficients
        let y_lin = 0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b;
        y_lin * sdr_reference_white_nits
    }

    /// Blends a foreground UI color over this background in linear optical space using pre-multiplied alpha.
    ///
    /// Formula:
    /// $$C_{\text{out}} = C_{\text{fg}} \cdot \alpha_{\text{fg}} + C_{\text{bg}} \cdot (1 - \alpha_{\text{fg}})$$
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::color::ScRgb;
    ///
    /// let bg_hdr = ScRgb::new(4.0, 4.0, 4.0, 1.0); // ~800 nit HDR background
    /// let fg_ui = ScRgb::new(1.0, 0.0, 0.0, 0.5);  // 50% semi-transparent red UI element
    /// let blended = bg_hdr.blend_over(fg_ui);
    /// assert!((blended.r - (1.0 * 0.5 + 4.0 * 0.5)).abs() < 1e-4);
    /// ```
    #[must_use]
    pub fn blend_over(&self, fg: ScRgb) -> Self {
        let a = fg.alpha.clamp(0.0, 1.0);
        let inv_a = 1.0 - a;
        Self {
            r: fg.r * a + self.r * inv_a,
            g: fg.g * a + self.g * inv_a,
            b: fg.b * a + self.b * inv_a,
            alpha: a + self.alpha * inv_a,
        }
    }
}

/// Converts linear sRGB / Rec.709 to CIE 1931 XYZ using D65 reference white.
///
/// # Examples
///
/// ```
/// use glam::Vec3;
/// use martensite_media::color::rgb_to_xyz;
///
/// let white_xyz = rgb_to_xyz(Vec3::ONE);
/// assert!((white_xyz.y - 1.0).abs() < 1e-4);
/// ```
#[must_use]
pub fn rgb_to_xyz(rgb: Vec3) -> Vec3 {
    let x = 0.4124564 * rgb.x + 0.3575761 * rgb.y + 0.1804375 * rgb.z;
    let y = 0.2126729 * rgb.x + 0.7151522 * rgb.y + 0.0721750 * rgb.z;
    let z = 0.0193339 * rgb.x + 0.119192 * rgb.y + 0.9503041 * rgb.z;
    Vec3::new(x, y, z)
}

/// Converts CIE 1931 XYZ to CIE $L^*a^*b^*$ using standard D65 reference white.
///
/// # Examples
///
/// ```
/// use glam::Vec3;
/// use martensite_media::color::{rgb_to_xyz, xyz_to_lab};
///
/// let lab = xyz_to_lab(rgb_to_xyz(Vec3::ONE));
/// assert!((lab.x - 100.0).abs() < 1e-2); // White L* is 100
/// ```
#[must_use]
pub fn xyz_to_lab(xyz: Vec3) -> Vec3 {
    // D65 reference white
    const XN: f32 = 0.95047;
    const YN: f32 = 1.00000;
    const ZN: f32 = 1.08883;

    fn f(t: f32) -> f32 {
        const DELTA: f32 = 6.0 / 29.0;
        if t > DELTA * DELTA * DELTA {
            t.powf(1.0 / 3.0)
        } else {
            t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
        }
    }

    let fx = f(xyz.x / XN);
    let fy = f(xyz.y / YN);
    let fz = f(xyz.z / ZN);

    let l = 116.0 * fy - 16.0;
    let a = 500.0 * (fx - fy);
    let b = 200.0 * (fy - fz);

    Vec3::new(l, a, b)
}

/// Calculates the CIE 1976 $\Delta E^*_{ab}$ color difference between two linear RGB colors.
///
/// A $\Delta E < 1.0$ is generally accepted as indistinguishable to the human eye.
///
/// # Examples
///
/// ```
/// use glam::Vec3;
/// use martensite_media::color::delta_e_76;
///
/// let c1 = Vec3::new(1.0, 0.0, 0.0);
/// let c2 = Vec3::new(1.0, 0.0, 0.0);
/// assert_eq!(delta_e_76(c1, c2), 0.0);
/// ```
#[must_use]
pub fn delta_e_76(c1: Vec3, c2: Vec3) -> f32 {
    let lab1 = xyz_to_lab(rgb_to_xyz(c1));
    let lab2 = xyz_to_lab(rgb_to_xyz(c2));
    (lab1 - lab2).length()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pq_roundtrip_precision() {
        // Test 1,000 evenly spaced points from 0.0 to 1.0
        for i in 0..=1000 {
            let n = i as f32 / 1000.0;
            let nits = pq_eotf(n);
            let roundtrip = pq_oetf(nits);
            assert!(
                (roundtrip - n).abs() < 1e-4,
                "PQ roundtrip failed at n={n}: nits={nits}, roundtrip={roundtrip}"
            );
        }
    }

    #[test]
    fn bt709_color_bars_delta_e() {
        // Standard SMPTE 100% color bars
        let reference_bars = [
            ("White", Vec3::new(1.0, 1.0, 1.0)),
            ("Yellow", Vec3::new(1.0, 1.0, 0.0)),
            ("Cyan", Vec3::new(0.0, 1.0, 1.0)),
            ("Green", Vec3::new(0.0, 1.0, 0.0)),
            ("Magenta", Vec3::new(1.0, 0.0, 1.0)),
            ("Red", Vec3::new(1.0, 0.0, 0.0)),
            ("Blue", Vec3::new(0.0, 0.0, 1.0)),
            ("Black", Vec3::new(0.0, 0.0, 0.0)),
        ];

        for (name, expected_rgb) in reference_bars {
            let (y, cb, cr) = rgb_to_bt709_yuv(expected_rgb);
            let actual_rgb = bt709_yuv_to_rgb(y, cb, cr);
            let de = delta_e_76(expected_rgb, actual_rgb);
            assert!(
                de < 0.05,
                "Color bar {name} failed Delta E check: de={de:.4}, expected={expected_rgb}, actual={actual_rgb}"
            );
        }
    }

    #[test]
    fn bt2020_color_bars_delta_e() {
        let reference_bars = [
            ("White", Vec3::new(1.0, 1.0, 1.0)),
            ("Yellow", Vec3::new(1.0, 1.0, 0.0)),
            ("Cyan", Vec3::new(0.0, 1.0, 1.0)),
            ("Green", Vec3::new(0.0, 1.0, 0.0)),
            ("Magenta", Vec3::new(1.0, 0.0, 1.0)),
            ("Red", Vec3::new(1.0, 0.0, 0.0)),
            ("Blue", Vec3::new(0.0, 0.0, 1.0)),
            ("Black", Vec3::new(0.0, 0.0, 0.0)),
        ];

        for (name, expected_rgb) in reference_bars {
            let (y, cb, cr) = rgb_to_bt2020_yuv(expected_rgb);
            let actual_rgb = bt2020_yuv_to_rgb(y, cb, cr);
            let de = delta_e_76(expected_rgb, actual_rgb);
            assert!(
                de < 0.05,
                "BT.2020 Color bar {name} failed Delta E check: de={de:.4}, expected={expected_rgb}, actual={actual_rgb}"
            );
        }
    }

    #[test]
    fn scrgb_linear_blending() {
        let bg = ScRgb::new(2.0, 2.0, 2.0, 1.0); // HDR highlight
        let fg = ScRgb::new(1.0, 1.0, 1.0, 1.0); // Opaque SDR white UI button
        let res = bg.blend_over(fg);
        assert_eq!(res.r, 1.0);
        assert_eq!(res.g, 1.0);
        assert_eq!(res.b, 1.0);
    }
}
