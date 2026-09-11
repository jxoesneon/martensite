//! Unicode Vertical Text Layout (UAX #50) and coordinate transformation.
//!
//! This module provides:
//! - Character classification according to UAX #50 (`VerticalOrientation`).
//! - Coordinate and bounding box transposition for `writing-mode: vertical-rl`.
//! - Metrics for vertical typography (`VerticalMetrics`).
//! - OpenType vertical substitution feature tags (`VerticalFeatureTags`).

use martensite_core::Rect;

/// Direction of text flow for the purposes of layout and shaping.
///
/// # Examples
///
/// ```
/// use martensite_text::vertical::WritingMode;
///
/// assert!(WritingMode::HorizontalTb.is_horizontal());
/// assert!(WritingMode::VerticalRl.is_vertical());
/// assert!(WritingMode::VerticalLr.is_vertical());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WritingMode {
    /// Standard horizontal top-to-bottom, left-to-right text.
    #[default]
    HorizontalTb,
    /// Vertical right-to-left text flow (e.g. traditional CJK).
    VerticalRl,
    /// Vertical left-to-right text flow.
    VerticalLr,
}

impl WritingMode {
    /// Returns `true` if the writing mode is vertical.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::vertical::WritingMode;
    ///
    /// assert!(!WritingMode::HorizontalTb.is_vertical());
    /// assert!(WritingMode::VerticalRl.is_vertical());
    /// assert!(WritingMode::VerticalLr.is_vertical());
    /// ```
    #[inline]
    pub const fn is_vertical(&self) -> bool {
        matches!(self, Self::VerticalRl | Self::VerticalLr)
    }

    /// Returns `true` if the writing mode is horizontal.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::vertical::WritingMode;
    ///
    /// assert!(WritingMode::HorizontalTb.is_horizontal());
    /// assert!(!WritingMode::VerticalRl.is_horizontal());
    /// assert!(!WritingMode::VerticalLr.is_horizontal());
    /// ```
    #[inline]
    pub const fn is_horizontal(&self) -> bool {
        matches!(self, Self::HorizontalTb)
    }
}

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

/// A run of characters sharing the same vertical orientation.
///
/// # Examples
///
/// ```
/// use martensite_text::vertical::{VerticalOrientation, VerticalRun};
///
/// let run = VerticalRun::new(0, 6, VerticalOrientation::Upright);
/// assert_eq!(run.orientation, VerticalOrientation::Upright);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerticalRun {
    /// Start byte index in the source text.
    pub start: usize,
    /// End byte index in the source text (exclusive).
    pub end: usize,
    /// Orientation applied to every character in this run.
    pub orientation: VerticalOrientation,
}

impl VerticalRun {
    /// Creates a new `VerticalRun`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::vertical::{VerticalOrientation, VerticalRun};
    ///
    /// let run = VerticalRun::new(0, 6, VerticalOrientation::Upright);
    /// assert_eq!(run.len(), 6);
    /// ```
    #[inline]
    pub const fn new(start: usize, end: usize, orientation: VerticalOrientation) -> Self {
        Self {
            start,
            end,
            orientation,
        }
    }

    /// Returns the length of this run in bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::vertical::{VerticalOrientation, VerticalRun};
    ///
    /// let run = VerticalRun::new(2, 8, VerticalOrientation::Rotated);
    /// assert_eq!(run.len(), 6);
    /// ```
    #[inline]
    pub const fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Returns `true` if this run covers no bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::vertical::{VerticalOrientation, VerticalRun};
    ///
    /// let empty = VerticalRun::new(0, 0, VerticalOrientation::Upright);
    /// assert!(empty.is_empty());
    ///
    /// let nonempty = VerticalRun::new(0, 1, VerticalOrientation::Upright);
    /// assert!(!nonempty.is_empty());
    /// ```
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// Splits `text` into contiguous runs of the same [`VerticalOrientation`].
///
/// # Examples
///
/// ```
/// use martensite_text::vertical::{VerticalOrientation, collect_vertical_runs};
///
/// let runs = collect_vertical_runs("漢字A");
/// assert_eq!(runs[0].orientation, VerticalOrientation::Upright);
/// assert_eq!(runs[1].orientation, VerticalOrientation::Rotated);
/// ```
pub fn collect_vertical_runs(text: &str) -> Vec<VerticalRun> {
    let mut runs: Vec<VerticalRun> = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        let orientation = classify_vertical_orientation(ch);
        let end = chars.peek().map(|(idx, _)| *idx).unwrap_or(text.len());
        if let Some(last) = runs.last_mut() {
            if last.orientation == orientation {
                last.end = end;
                continue;
            }
        }
        runs.push(VerticalRun::new(start, end, orientation));
    }
    runs
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::vertical::VerticalFeatureTags;
    ///
    /// assert_eq!(VerticalFeatureTags::VERT, "vert");
    /// ```
    pub const VERT: &'static str = "vert";

    /// OpenType vertical alternation for dual-orientation glyphs (`vrt2`).
    ///
    /// Replaces roman/latin characters with rotated forms while preserving upright CJK ideographs.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::vertical::VerticalFeatureTags;
    ///
    /// assert_eq!(VerticalFeatureTags::VRT2, "vrt2");
    /// ```
    pub const VRT2: &'static str = "vrt2";

    /// OpenType vertical kerning feature tag (`vkrn`).
    ///
    /// Adjusts inter-glyph spacing along the vertical baseline.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::vertical::VerticalFeatureTags;
    ///
    /// assert_eq!(VerticalFeatureTags::VKRN, "vkrn");
    /// ```
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

/// Returns a [`cosmic_text::FontFeatures`] value with the standard vertical
/// OpenType features enabled.
///
/// This can be passed to [`cosmic_text::Attrs::font_features`] when shaping
/// vertical text so the font system activates `vert`, `vrt2`, and `vkrn`
/// when the current font supports them.
///
/// # Examples
///
/// ```
/// use martensite_text::vertical::vertical_features;
///
/// let features = vertical_features();
/// assert!(!features.features.is_empty());
/// ```
pub fn vertical_features() -> cosmic_text::FontFeatures {
    let mut features = cosmic_text::FontFeatures::new();
    for tag in VerticalFeatureTags::standard_tags() {
        let bytes: &[u8; 4] = tag.as_bytes().try_into().expect("feature tags are 4 bytes");
        let tag = cosmic_text::FeatureTag::new(bytes);
        features.set(tag, 1);
    }
    features
}

/// Applies vertical OpenType features to an [`cosmic_text::Attrs`] value.
///
/// # Examples
///
/// ```
/// use cosmic_text::Attrs;
/// use martensite_text::vertical::apply_vertical_features;
///
/// let attrs = apply_vertical_features(Attrs::new());
/// assert!(!attrs.font_features.features.is_empty());
/// ```
pub fn apply_vertical_features(attrs: cosmic_text::Attrs<'_>) -> cosmic_text::Attrs<'_> {
    attrs.font_features(vertical_features())
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

    #[test]
    fn test_collect_vertical_runs() {
        let runs = collect_vertical_runs("漢字A");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].orientation, VerticalOrientation::Upright);
        assert!(!runs[0].is_empty());
        assert_eq!(runs[1].orientation, VerticalOrientation::Rotated);
    }

    #[test]
    fn test_vertical_features_enabled() {
        let features = vertical_features();
        assert!(!features.features.is_empty());
        let tags: Vec<_> = features
            .features
            .iter()
            .map(|f| std::str::from_utf8(f.tag.as_bytes()).unwrap().to_string())
            .collect();
        assert!(tags.contains(&"vert".to_string()));
        assert!(tags.contains(&"vrt2".to_string()));
        assert!(tags.contains(&"vkrn".to_string()));
    }

    #[test]
    fn test_apply_vertical_features_to_attrs() {
        let attrs = apply_vertical_features(cosmic_text::Attrs::new());
        assert!(!attrs.font_features.features.is_empty());
    }
}
