//! Unicode Vertical Text Layout (UAX #50) and coordinate transformation.
//!
//! This module provides:
//! - Character classification according to UAX #50 (`VerticalOrientation`).
//! - Coordinate and bounding box transposition for `writing-mode: vertical-rl`.
//! - Metrics for vertical typography (`VerticalMetrics`).
//! - OpenType vertical substitution feature tags (`VerticalFeatureTags`).

use martensite_core::Rect;

/// Vertical text glyph orientation per Unicode Standard Annex #50 (UAX #50).
///
/// Indicates whether a character is displayed upright (unrotated), rotated 90 degrees
/// clockwise, or substituted with a transformed glyph variant designed for vertical layout.
///
/// # Examples
///
/// ```
/// use martensite_text::vertical::VerticalOrientation;
///
/// let orientation = VerticalOrientation::Upright;
/// assert_eq!(orientation, VerticalOrientation::default());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VerticalOrientation {
    /// Glyph remains upright and unrotated (e.g. CJK ideographs, Kana, full-width punctuation).
    #[default]
    Upright,
    /// Glyph is rotated 90 degrees clockwise (e.g. Latin letters, digits, ASCII punctuation).
    Rotated,
    /// Glyph requires a transformed vertical alternate or 90-degree glyph rotation
    /// (e.g. em-dashes, brackets, parentheses).
    Transformed,
}

/// Classifies a character into its vertical orientation according to UAX #50 rules.
///
/// - **Upright**: CJK ideographs, Kana (Hiragana/Katakana), Hangul syllables, and full-width punctuation.
/// - **Transformed**: Em-dashes, parentheses, brackets, braces, angle brackets, and Japanese quotation marks.
/// - **Rotated**: Latin script, Arabic, Hebrew, ASCII digits, and general ASCII punctuation.
///
/// # Examples
///
/// ```
/// use martensite_text::vertical::{VerticalOrientation, classify_vertical_orientation};
///
/// assert_eq!(classify_vertical_orientation('漢'), VerticalOrientation::Upright);
/// assert_eq!(classify_vertical_orientation('あ'), VerticalOrientation::Upright);
/// assert_eq!(classify_vertical_orientation('A'), VerticalOrientation::Rotated);
/// assert_eq!(classify_vertical_orientation('1'), VerticalOrientation::Rotated);
/// assert_eq!(classify_vertical_orientation('—'), VerticalOrientation::Transformed);
/// assert_eq!(classify_vertical_orientation('（'), VerticalOrientation::Transformed);
/// ```
pub fn classify_vertical_orientation(ch: char) -> VerticalOrientation {
    // Check Transformed characters first (brackets, dashes, tildes, quotation brackets)
    if is_transformed_char(ch) {
        return VerticalOrientation::Transformed;
    }

    // Check Upright CJK and East Asian scripts
    let u = ch as u32;
    match u {
        // CJK Unified Ideographs & Extensions
        0x4E00..=0x9FFF
        | 0x3400..=0x4DBF
        | 0x20000..=0x2FA1F
        | 0x2E80..=0x2EFF
        | 0x2F00..=0x2FDF
        | 0x3005 // Ideographic iteration mark
        | 0x3006 // Ideographic closing mark
        // Hiragana & Katakana
        | 0x3041..=0x3096
        | 0x3099..=0x309F
        | 0x30A1..=0x30FA
        | 0x30FC..=0x30FF
        | 0x31F0..=0x31FF
        // Bopomofo
        | 0x3105..=0x312F
        | 0x31A0..=0x31BF
        // Hangul Syllables & Jamo
        | 0xAC00..=0xD7AF
        | 0x1100..=0x11FF
        | 0x3130..=0x318F
        // Fullwidth forms & punctuation (excluding transformed brackets handled above)
        | 0x3001..=0x3003 // 、。〃
        | 0xFF01..=0xFF07 // ！＂＃＄％＆＇
        | 0xFF0C..=0xFF0F // ，－．／
        | 0xFF1A..=0xFF1F // ：；＜＝＞？
        => VerticalOrientation::Upright,

        // Everything else defaults to Rotated (Latin, digits, ASCII punctuation, other scripts)
        _ => VerticalOrientation::Rotated,
    }
}

/// Helper function to check for vertical transformed characters.
fn is_transformed_char(ch: char) -> bool {
    matches!(
        ch,
        // Em-dash, en-dash, horizontal bars, wave dashes, leaders
        '—' | '–'
            | '―'
            | '‒'
            | '⸺'
            | '⸻'
            | '〜'
            | '～'
            | '…'
            | '‥'
            | '⋯'
            // ASCII and Unicode brackets, parentheses, braces, quotes
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '<'
            | '>'
            | '«'
            | '»'
            | '‹'
            | '›'
            | '“'
            | '”'
            | '‘'
            | '’'
            // Full-width brackets and East Asian quotation marks
            | '（'
            | '）'
            | '［'
            | '］'
            | '｛'
            | '｝'
            | '〈'
            | '〉'
            | '《'
            | '》'
            | '「'
            | '」'
            | '『'
            | '』'
            | '【'
            | '】'
            | '〔'
            | '〕'
            | '〖'
            | '〗'
            | '⟨'
            | '⟩'
            | '⟪'
            | '⟫'
            | '⟦'
            | '⟧'
    )
}

/// Coordinate transposition operations for vertical glyphs and bounding boxes.
///
/// # Examples
///
/// ```
/// use martensite_text::vertical::VerticalGlyphTransform;
/// use martensite_core::Rect;
///
/// let bounds = Rect::new(10.0, 20.0, 30.0, 40.0);
/// let transformed = VerticalGlyphTransform::transform_rect(bounds, 200.0);
/// assert_eq!(transformed.min_x(), 140.0); // 200 - (20 + 40)
/// assert_eq!(transformed.min_y(), 10.0);  // 10
/// assert_eq!(transformed.width(), 40.0);  // original height
/// assert_eq!(transformed.height(), 30.0); // original width
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VerticalGlyphTransform;

impl VerticalGlyphTransform {
    /// Transforms a horizontal bounding rectangle into `vertical-rl` coordinate space
    /// within a container of the specified width.
    ///
    /// The transposition performs:
    /// - `x' = container_width - (bounds.min_y() + bounds.height())`
    /// - `y' = bounds.min_x()`
    /// - `width' = bounds.height()`
    /// - `height' = bounds.width()`
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::vertical::VerticalGlyphTransform;
    /// use martensite_core::Rect;
    ///
    /// let r = Rect::new(0.0, 0.0, 10.0, 20.0);
    /// let v = VerticalGlyphTransform::transform_rect(r, 100.0);
    /// assert_eq!(v.min_x(), 80.0);
    /// assert_eq!(v.min_y(), 0.0);
    /// assert_eq!(v.width(), 20.0);
    /// assert_eq!(v.height(), 10.0);
    /// ```
    #[inline]
    pub fn transform_rect(bounds: Rect, container_width: f32) -> Rect {
        let x = container_width - (bounds.min_y() + bounds.height());
        let y = bounds.min_x();
        let width = bounds.height();
        let height = bounds.width();
        Rect::new(x, y, width, height)
    }
}

/// Metrics describing vertical typographic layout parameters.
///
/// # Examples
///
/// ```
/// use martensite_text::vertical::VerticalMetrics;
///
/// let metrics = VerticalMetrics::new(16.0, 8.0, 24.0);
/// assert_eq!(metrics.advance, 16.0);
/// assert_eq!(metrics.central_baseline_offset, 8.0);
/// assert_eq!(metrics.line_spacing, 24.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VerticalMetrics {
    /// Vertical advance increment along the block flow axis (downwards).
    pub advance: f32,
    /// Offset from the glyph origin to the central baseline.
    pub central_baseline_offset: f32,
    /// Line spacing between adjacent vertical columns (right to left).
    pub line_spacing: f32,
}

impl VerticalMetrics {
    /// Creates a new `VerticalMetrics` instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::vertical::VerticalMetrics;
    ///
    /// let metrics = VerticalMetrics::new(20.0, 10.0, 28.0);
    /// assert_eq!(metrics.advance, 20.0);
    /// ```
    #[inline]
    pub const fn new(advance: f32, central_baseline_offset: f32, line_spacing: f32) -> Self {
        Self {
            advance,
            central_baseline_offset,
            line_spacing,
        }
    }
}

/// OpenType layout feature tags for vertical typography.
///
/// # Examples
///
/// ```
/// use martensite_text::vertical::VerticalFeatureTags;
///
/// assert_eq!(VerticalFeatureTags::VERT, "vert");
/// assert_eq!(VerticalFeatureTags::VRT2, "vrt2");
/// assert_eq!(VerticalFeatureTags::VKRN, "vkrn");
/// assert_eq!(VerticalFeatureTags::standard_tags().len(), 3);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VerticalFeatureTags;

impl VerticalFeatureTags {
    /// OpenType vertical substitution feature tag (`vert`).
    ///
    /// Substitutes horizontal glyph forms with vertical forms (e.g. rotated punctuation,
    /// centered vertical commas, colon repositioning).
    pub const VERT: &'static str = "vert";

    /// OpenType vertical alternation for dual-orientation glyphs (`vrt2`).
    ///
    /// Replaces roman/latin characters with rotated forms while preserving upright CJK ideographs.
    pub const VRT2: &'static str = "vrt2";

    /// OpenType vertical kerning feature tag (`vkrn`).
    ///
    /// Adjusts inter-glyph spacing along the vertical baseline.
    pub const VKRN: &'static str = "vkrn";

    /// Returns the standard list of OpenType vertical feature tags.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::vertical::VerticalFeatureTags;
    ///
    /// let tags = VerticalFeatureTags::standard_tags();
    /// assert!(tags.contains(&"vert"));
    /// assert!(tags.contains(&"vrt2"));
    /// assert!(tags.contains(&"vkrn"));
    /// ```
    #[inline]
    pub fn standard_tags() -> &'static [&'static str] {
        &[Self::VERT, Self::VRT2, Self::VKRN]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_cjk_kana_upright() {
        assert_eq!(
            classify_vertical_orientation('漢'),
            VerticalOrientation::Upright
        );
        assert_eq!(
            classify_vertical_orientation('字'),
            VerticalOrientation::Upright
        );
        assert_eq!(
            classify_vertical_orientation('あ'),
            VerticalOrientation::Upright
        );
        assert_eq!(
            classify_vertical_orientation('カ'),
            VerticalOrientation::Upright
        );
        assert_eq!(
            classify_vertical_orientation('、'),
            VerticalOrientation::Upright
        );
        assert_eq!(
            classify_vertical_orientation('。'),
            VerticalOrientation::Upright
        );
    }

    #[test]
    fn test_classify_latin_rotated() {
        assert_eq!(
            classify_vertical_orientation('A'),
            VerticalOrientation::Rotated
        );
        assert_eq!(
            classify_vertical_orientation('z'),
            VerticalOrientation::Rotated
        );
        assert_eq!(
            classify_vertical_orientation('0'),
            VerticalOrientation::Rotated
        );
        assert_eq!(
            classify_vertical_orientation('9'),
            VerticalOrientation::Rotated
        );
        assert_eq!(
            classify_vertical_orientation(';'),
            VerticalOrientation::Rotated
        );
    }

    #[test]
    fn test_classify_transformed() {
        assert_eq!(
            classify_vertical_orientation('—'),
            VerticalOrientation::Transformed
        );
        assert_eq!(
            classify_vertical_orientation('–'),
            VerticalOrientation::Transformed
        );
        assert_eq!(
            classify_vertical_orientation('―'),
            VerticalOrientation::Transformed
        );
        assert_eq!(
            classify_vertical_orientation('('),
            VerticalOrientation::Transformed
        );
        assert_eq!(
            classify_vertical_orientation(')'),
            VerticalOrientation::Transformed
        );
        assert_eq!(
            classify_vertical_orientation('（'),
            VerticalOrientation::Transformed
        );
        assert_eq!(
            classify_vertical_orientation('）'),
            VerticalOrientation::Transformed
        );
        assert_eq!(
            classify_vertical_orientation('「'),
            VerticalOrientation::Transformed
        );
        assert_eq!(
            classify_vertical_orientation('」'),
            VerticalOrientation::Transformed
        );
        assert_eq!(
            classify_vertical_orientation('…'),
            VerticalOrientation::Transformed
        );
    }

    #[test]
    fn test_transform_rect() {
        let original = Rect::new(10.0, 20.0, 30.0, 40.0);
        let container_width = 300.0;
        let transformed = VerticalGlyphTransform::transform_rect(original, container_width);

        // x' = 300 - (20 + 40) = 240
        // y' = 10
        // width = 40 (original height)
        // height = 30 (original width)
        assert_eq!(transformed.min_x(), 240.0);
        assert_eq!(transformed.min_y(), 10.0);
        assert_eq!(transformed.width(), 40.0);
        assert_eq!(transformed.height(), 30.0);
    }

    #[test]
    fn test_vertical_metrics() {
        let m = VerticalMetrics::new(18.0, 9.0, 26.0);
        assert_eq!(m.advance, 18.0);
        assert_eq!(m.central_baseline_offset, 9.0);
        assert_eq!(m.line_spacing, 26.0);
    }

    #[test]
    fn test_vertical_feature_tags() {
        let tags = VerticalFeatureTags::standard_tags();
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0], "vert");
        assert_eq!(tags[1], "vrt2");
        assert_eq!(tags[2], "vkrn");
    }
}
