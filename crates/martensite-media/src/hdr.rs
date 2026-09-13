//! Typed HDR metadata extracted from decoder side-data.
//!
//! Platform decoders produce [`HdrSideData`] — the untyped numeric carrier —
//! from VideoToolbox `CMFormatDescription` extensions, Media Foundation
//! `MF_MT_*` attributes, VAAPI SEI parsing, or FFmpeg `AV_FRAME_DATA_*`.
//! [`HdrMetadata`] converts that into the typed representation that feeds
//! `martensite-wgpu`'s `VideoPipelineUniforms` alongside a
//! [`crate::tonemap::DisplayProfile`].

use crate::color::ColorSpace;
use crate::decoder::HdrSideData;
use crate::surface::ColorRange;

/// Electro-optical transfer function signalled by the stream.
///
/// # Examples
///
/// ```
/// use martensite_media::hdr::Eotf;
///
/// assert_eq!(Eotf::from_code(16), Eotf::Pq);
/// assert_eq!(Eotf::from_code(18), Eotf::Hlg);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum Eotf {
    /// Standard dynamic range — BT.709/sRGB-style transfer (code 1).
    #[default]
    Sdr,
    /// SMPTE ST 2084 perceptual quantizer (code 16).
    Pq,
    /// Hybrid log-gamma, ARIB STD-B67 (code 18).
    Hlg,
    /// Any other transfer code, preserved for forward compatibility.
    Other(u16),
}

impl Eotf {
    /// Maps an ISO 23001-8 transfer-characteristic code to an [`Eotf`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::hdr::Eotf;
    ///
    /// assert_eq!(Eotf::from_code(1), Eotf::Sdr);
    /// assert_eq!(Eotf::from_code(13), Eotf::Sdr); // sRGB is treated as SDR
    /// assert_eq!(Eotf::from_code(99), Eotf::Other(99));
    /// ```
    #[must_use]
    pub fn from_code(code: u16) -> Self {
        match code {
            1 | 13 => Self::Sdr,
            16 => Self::Pq,
            18 => Self::Hlg,
            other => Self::Other(other),
        }
    }

    /// Returns the ISO 23001-8 transfer-characteristic code.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::hdr::Eotf;
    ///
    /// assert_eq!(Eotf::Pq.code(), 16);
    /// assert_eq!(Eotf::Other(7).code(), 7);
    /// ```
    #[must_use]
    pub fn code(&self) -> u16 {
        match self {
            Self::Sdr => 1,
            Self::Pq => 16,
            Self::Hlg => 18,
            Self::Other(c) => *c,
        }
    }
}

/// Mastering-display colour volume (SMPTE ST 2086 static metadata).
///
/// # Examples
///
/// ```
/// use martensite_media::hdr::MasteringDisplayVolume;
///
/// let md = MasteringDisplayVolume::new(1000.0, 0.005);
/// assert!((md.max_luminance_nits - 1000.0).abs() < f32::EPSILON);
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct MasteringDisplayVolume {
    /// Mastering display peak luminance in nits.
    pub max_luminance_nits: f32,
    /// Mastering display minimum luminance in nits.
    pub min_luminance_nits: f32,
    /// Display primaries as CIE 1931 xy pairs `[R, G, B]`; `None` when
    /// the stream only signalled luminance bounds.
    pub primaries: Option<[(f32, f32); 3]>,
    /// White point as a CIE 1931 xy pair.
    pub white_point: Option<(f32, f32)>,
}

impl MasteringDisplayVolume {
    /// Creates a volume with luminance bounds and no chromaticity data.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::hdr::MasteringDisplayVolume;
    ///
    /// let md = MasteringDisplayVolume::new(4000.0, 0.0001);
    /// assert!(md.primaries.is_none());
    /// ```
    #[must_use]
    pub fn new(max_luminance_nits: f32, min_luminance_nits: f32) -> Self {
        Self {
            max_luminance_nits,
            min_luminance_nits,
            primaries: None,
            white_point: None,
        }
    }
}

/// Content light level metadata (`MaxCLL` / `MaxFALL`, CTA-861.3).
///
/// # Examples
///
/// ```
/// use martensite_media::hdr::ContentLightLevel;
///
/// let cll = ContentLightLevel::new(1000, 400);
/// assert_eq!(cll.max_cll, 1000);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ContentLightLevel {
    /// Maximum content light level in nits.
    pub max_cll: u16,
    /// Maximum frame-average light level in nits.
    pub max_fall: u16,
}

impl ContentLightLevel {
    /// Creates a content light level record.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::hdr::ContentLightLevel;
    ///
    /// let cll = ContentLightLevel::new(500, 180);
    /// assert_eq!(cll.max_fall, 180);
    /// ```
    #[must_use]
    pub fn new(max_cll: u16, max_fall: u16) -> Self {
        Self { max_cll, max_fall }
    }
}

/// Typed HDR metadata for a decoded video stream or frame.
///
/// # Examples
///
/// ```
/// use martensite_media::hdr::{Eotf, HdrMetadata};
/// use martensite_media::color::ColorSpace;
///
/// let hdr = HdrMetadata::new(Eotf::Pq).with_color_space(ColorSpace::Bt2020);
/// assert!(hdr.is_hdr());
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct HdrMetadata {
    /// Signalled transfer function.
    pub eotf: Eotf,
    /// Signalled colour primaries/gamut.
    pub color_space: ColorSpace,
    /// Quantization range.
    pub range: ColorRange,
    /// Mastering-display colour volume, if signalled.
    pub mastering_display: Option<MasteringDisplayVolume>,
    /// Content light level, if signalled.
    pub content_light: Option<ContentLightLevel>,
    /// HDR10+ dynamic metadata payload (SMPTE ST 2094-40), if present.
    pub hdr10_plus: Option<Vec<u8>>,
}

impl HdrMetadata {
    /// Creates metadata for the given EOTF with BT.709 limited-range
    /// defaults and no static/dynamic metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::hdr::{Eotf, HdrMetadata};
    ///
    /// let hdr = HdrMetadata::new(Eotf::Hlg);
    /// assert_eq!(hdr.eotf, Eotf::Hlg);
    /// assert!(!hdr.is_hdr10());
    /// ```
    #[must_use]
    pub fn new(eotf: Eotf) -> Self {
        Self {
            eotf,
            color_space: ColorSpace::Bt709,
            range: ColorRange::Limited,
            mastering_display: None,
            content_light: None,
            hdr10_plus: None,
        }
    }

    /// Builder: set the colour space.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::hdr::{Eotf, HdrMetadata};
    /// use martensite_media::color::ColorSpace;
    ///
    /// let hdr = HdrMetadata::new(Eotf::Pq).with_color_space(ColorSpace::Bt2020);
    /// assert_eq!(hdr.color_space, ColorSpace::Bt2020);
    /// ```
    #[must_use]
    pub fn with_color_space(mut self, color_space: ColorSpace) -> Self {
        self.color_space = color_space;
        self
    }

    /// Builder: set the quantization range.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::hdr::{Eotf, HdrMetadata};
    /// use martensite_media::surface::ColorRange;
    ///
    /// let hdr = HdrMetadata::new(Eotf::Sdr).with_range(ColorRange::Full);
    /// assert_eq!(hdr.range, ColorRange::Full);
    /// ```
    #[must_use]
    pub fn with_range(mut self, range: ColorRange) -> Self {
        self.range = range;
        self
    }

    /// Returns `true` when the signal is HDR (PQ or HLG).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::hdr::{Eotf, HdrMetadata};
    ///
    /// assert!(HdrMetadata::new(Eotf::Pq).is_hdr());
    /// assert!(!HdrMetadata::new(Eotf::Sdr).is_hdr());
    /// ```
    #[must_use]
    pub fn is_hdr(&self) -> bool {
        matches!(self.eotf, Eotf::Pq | Eotf::Hlg)
    }

    /// Returns `true` when the signal is PQ HDR10 (PQ + BT.2020 primaries).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::hdr::{Eotf, HdrMetadata};
    /// use martensite_media::color::ColorSpace;
    ///
    /// let hdr = HdrMetadata::new(Eotf::Pq).with_color_space(ColorSpace::Bt2020);
    /// assert!(hdr.is_hdr10());
    /// ```
    #[must_use]
    pub fn is_hdr10(&self) -> bool {
        self.eotf == Eotf::Pq && self.color_space == ColorSpace::Bt2020
    }

    /// Effective peak content luminance in nits, preferring `MaxCLL` over
    /// the mastering-display peak, defaulting to the PQ reference 10 000.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::hdr::{ContentLightLevel, Eotf, HdrMetadata};
    ///
    /// let hdr = HdrMetadata::new(Eotf::Pq);
    /// assert_eq!(hdr.effective_peak_nits(), 10_000.0);
    ///
    /// let hdr = hdr.with_content_light(ContentLightLevel::new(1200, 400));
    /// assert_eq!(hdr.effective_peak_nits(), 1200.0);
    /// ```
    #[must_use]
    pub fn effective_peak_nits(&self) -> f32 {
        if let Some(cll) = self.content_light {
            return f32::from(cll.max_cll.max(1));
        }
        if let Some(md) = self.mastering_display {
            return md.max_luminance_nits.max(1.0);
        }
        10_000.0
    }

    /// Builder: attach a content light level.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::hdr::{ContentLightLevel, Eotf, HdrMetadata};
    ///
    /// let hdr = HdrMetadata::new(Eotf::Pq).with_content_light(ContentLightLevel::new(800, 200));
    /// assert_eq!(hdr.content_light.unwrap().max_cll, 800);
    /// ```
    #[must_use]
    pub fn with_content_light(mut self, cll: ContentLightLevel) -> Self {
        self.content_light = Some(cll);
        self
    }

    /// Lowers this typed record to the raw side-data carrier produced by
    /// platform decoders. Useful for tests and for re-attaching metadata to
    /// synthesized frames.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_media::hdr::{Eotf, HdrMetadata};
    /// use martensite_media::color::ColorSpace;
    ///
    /// let side = HdrMetadata::new(Eotf::Pq)
    ///     .with_color_space(ColorSpace::Bt2020)
    ///     .to_side_data();
    /// assert_eq!(side.eotf_code, 16);
    /// assert_eq!(side.primaries_code, 9);
    /// ```
    #[must_use]
    pub fn to_side_data(&self) -> HdrSideData {
        HdrSideData {
            eotf_code: self.eotf.code(),
            primaries_code: match self.color_space {
                ColorSpace::Bt2020 => 9,
                _ => 1,
            },
            full_range: self.range == ColorRange::Full,
            max_luminance_nits: self.mastering_display.map(|m| m.max_luminance_nits),
            min_luminance_nits: self.mastering_display.map(|m| m.min_luminance_nits),
            max_cll: self.content_light.map(|c| c.max_cll),
            max_fall: self.content_light.map(|c| c.max_fall),
            dynamic_metadata: self.hdr10_plus.clone(),
        }
    }
}

impl From<&HdrSideData> for HdrMetadata {
    fn from(side: &HdrSideData) -> Self {
        let color_space = match side.primaries_code {
            9 => ColorSpace::Bt2020,
            _ => ColorSpace::Bt709,
        };
        let mastering_display = match (side.max_luminance_nits, side.min_luminance_nits) {
            (Some(max), min) => Some(MasteringDisplayVolume::new(max, min.unwrap_or(0.0))),
            _ => None,
        };
        let content_light = match (side.max_cll, side.max_fall) {
            (Some(cll), fall) => Some(ContentLightLevel::new(cll, fall.unwrap_or(0))),
            _ => None,
        };
        Self {
            eotf: Eotf::from_code(side.eotf_code),
            color_space,
            range: if side.full_range {
                ColorRange::Full
            } else {
                ColorRange::Limited
            },
            mastering_display,
            content_light,
            hdr10_plus: side.dynamic_metadata.clone(),
        }
    }
}

impl From<HdrSideData> for HdrMetadata {
    fn from(side: HdrSideData) -> Self {
        Self::from(&side)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eotf_code_roundtrip() {
        for (code, eotf) in [(1u16, Eotf::Sdr), (16, Eotf::Pq), (18, Eotf::Hlg)] {
            assert_eq!(Eotf::from_code(code), eotf);
            assert_eq!(eotf.code(), code);
        }
    }

    #[test]
    fn side_data_to_metadata_pq() {
        let mut side = HdrSideData::pq_bt2020(4000.0);
        side.max_cll = Some(1500);
        side.max_fall = Some(350);
        let hdr = HdrMetadata::from(&side);
        assert_eq!(hdr.eotf, Eotf::Pq);
        assert_eq!(hdr.color_space, ColorSpace::Bt2020);
        assert!(hdr.is_hdr10());
        assert_eq!(hdr.content_light.unwrap().max_cll, 1500);
        assert_eq!(hdr.effective_peak_nits(), 1500.0);
    }

    #[test]
    fn side_data_to_metadata_hlg() {
        let hdr = HdrMetadata::from(HdrSideData::hlg_bt2020());
        assert_eq!(hdr.eotf, Eotf::Hlg);
        assert!(hdr.is_hdr());
        assert!(hdr.mastering_display.is_none());
    }

    #[test]
    fn metadata_side_data_roundtrip() {
        let hdr = HdrMetadata::new(Eotf::Pq)
            .with_color_space(ColorSpace::Bt2020)
            .with_content_light(ContentLightLevel::new(2000, 500));
        let side = hdr.to_side_data();
        let back = HdrMetadata::from(&side);
        assert_eq!(back.eotf, Eotf::Pq);
        assert_eq!(back.color_space, ColorSpace::Bt2020);
        assert_eq!(back.content_light, hdr.content_light);
    }
}
