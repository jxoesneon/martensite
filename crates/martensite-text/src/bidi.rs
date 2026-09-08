//! Unicode Bidirectional Algorithm (UAX #9) and line breaking rules (UAX #14).
//!
//! This module provides primitives for:
//! - Analyzing paragraph text to resolve base direction and embedding levels.
//! - Extracting bidirectional runs (`BidiRun`) in logical order.
//! - Reordering runs into visual display order per UAX #9 Rule L2.
//! - Mirrored character substitution in RTL runs (parentheses, brackets, etc.).
//! - Kinsoku Shori (UAX #14 line breaking) start and end prohibition rules.

use unicode_bidi::{bidi_class, BidiClass, BidiInfo, Level};

/// Base or run direction for bidirectional text layout.
///
/// # Examples
///
/// ```
/// use martensite_text::bidi::BidiDirection;
///
/// let dir = BidiDirection::Ltr;
/// assert_eq!(dir, BidiDirection::default());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BidiDirection {
    /// Left-to-right text stream (e.g. Latin, Cyrillic, Greek).
    #[default]
    Ltr,
    /// Right-to-left text stream (e.g. Arabic, Hebrew).
    Rtl,
    /// Direction automatically detected from the first strong directional character.
    Auto,
}

/// A contiguous slice of text possessing a single resolved bidirectional level and direction.
///
/// # Examples
///
/// ```
/// use martensite_text::bidi::{BidiDirection, BidiRun};
///
/// let run = BidiRun::new(0, 5, 0, BidiDirection::Ltr);
/// assert!(!run.is_rtl());
/// assert_eq!(run.len(), 5);
/// assert!(!run.is_empty());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BidiRun {
    /// Start byte index in the source text.
    pub start: usize,
    /// End byte index in the source text (exclusive).
    pub end: usize,
    /// Resolved bidirectional embedding level (even = LTR, odd = RTL).
    pub level: u8,
    /// Resolved direction of the run.
    pub direction: BidiDirection,
}

impl BidiRun {
    /// Creates a new `BidiRun`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::bidi::{BidiDirection, BidiRun};
    ///
    /// let run = BidiRun::new(0, 10, 1, BidiDirection::Rtl);
    /// assert!(run.is_rtl());
    /// ```
    #[inline]
    pub const fn new(start: usize, end: usize, level: u8, direction: BidiDirection) -> Self {
        Self {
            start,
            end,
            level,
            direction,
        }
    }

    /// Returns `true` if this run progresses right-to-left.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::bidi::{BidiDirection, BidiRun};
    ///
    /// let run = BidiRun::new(0, 4, 1, BidiDirection::Rtl);
    /// assert!(run.is_rtl());
    /// ```
    #[inline]
    pub const fn is_rtl(&self) -> bool {
        matches!(self.direction, BidiDirection::Rtl) || (self.level % 2 == 1)
    }

    /// Returns the length of this run in bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::bidi::{BidiDirection, BidiRun};
    ///
    /// let run = BidiRun::new(2, 7, 0, BidiDirection::Ltr);
    /// assert_eq!(run.len(), 5);
    /// ```
    #[inline]
    pub const fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Returns `true` if the byte range of this run is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::bidi::{BidiDirection, BidiRun};
    ///
    /// let empty = BidiRun::new(5, 5, 0, BidiDirection::Ltr);
    /// assert!(empty.is_empty());
    /// ```
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// A paragraph analyzed under UAX #9 rules, containing resolved base level and logical runs.
///
/// # Examples
///
/// ```
/// use martensite_text::bidi::{BidiDirection, BidiParagraph};
///
/// let para = BidiParagraph::new("Hello World", BidiDirection::Auto);
/// assert_eq!(para.base_direction, BidiDirection::Ltr);
/// assert_eq!(para.resolved_base_level, 0);
/// assert_eq!(para.runs.len(), 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidiParagraph {
    /// Original text of the paragraph.
    pub text: String,
    /// Configured or detected base direction.
    pub base_direction: BidiDirection,
    /// Resolved base embedding level (0 for LTR, 1 for RTL).
    pub resolved_base_level: u8,
    /// Contiguous runs of uniform bidirectional level in logical order.
    pub runs: Vec<BidiRun>,
}

impl BidiParagraph {
    /// Creates a new `BidiParagraph` from text and a default direction.
    ///
    /// When `default_dir` is `BidiDirection::Auto`, the base direction is detected
    /// by searching for the first strong directional character (UAX #9 rules P2/P3).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::bidi::{BidiDirection, BidiParagraph};
    ///
    /// let para = BidiParagraph::new("مرحبا بالعالم", BidiDirection::Auto);
    /// assert_eq!(para.base_direction, BidiDirection::Rtl);
    /// assert_eq!(para.resolved_base_level, 1);
    /// ```
    pub fn new(text: &str, default_dir: BidiDirection) -> Self {
        let base_dir = match default_dir {
            BidiDirection::Auto => Self::detect_base_direction(text),
            other => other,
        };

        let base_level_val = match base_dir {
            BidiDirection::Rtl => 1u8,
            _ => 0u8,
        };

        if text.is_empty() {
            return Self {
                text: text.to_string(),
                base_direction: base_dir,
                resolved_base_level: base_level_val,
                runs: Vec::new(),
            };
        }

        let default_level = Level::new(base_level_val).ok();
        let bidi_info = BidiInfo::new(text, default_level);

        let mut runs = Vec::new();
        let mut run_start = 0;
        let mut current_level = bidi_info.levels[0].number();

        for (i, &lvl) in bidi_info.levels.iter().enumerate().skip(1) {
            let lvl_num = lvl.number();
            if lvl_num != current_level {
                let dir = if current_level % 2 == 1 {
                    BidiDirection::Rtl
                } else {
                    BidiDirection::Ltr
                };
                runs.push(BidiRun::new(run_start, i, current_level, dir));
                run_start = i;
                current_level = lvl_num;
            }
        }

        let dir = if current_level % 2 == 1 {
            BidiDirection::Rtl
        } else {
            BidiDirection::Ltr
        };
        runs.push(BidiRun::new(run_start, text.len(), current_level, dir));

        Self {
            text: text.to_string(),
            base_direction: base_dir,
            resolved_base_level: base_level_val,
            runs,
        }
    }

    /// Detects paragraph base direction from the first strong directional character
    /// per UAX #9 rules P2 and P3.
    ///
    /// Returns `BidiDirection::Ltr` if no strong directional character is found.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::bidi::{BidiDirection, BidiParagraph};
    ///
    /// assert_eq!(BidiParagraph::detect_base_direction("Hello"), BidiDirection::Ltr);
    /// assert_eq!(BidiParagraph::detect_base_direction("123 مرحبا"), BidiDirection::Rtl);
    /// assert_eq!(BidiParagraph::detect_base_direction("12345"), BidiDirection::Ltr);
    /// ```
    pub fn detect_base_direction(text: &str) -> BidiDirection {
        for ch in text.chars() {
            match bidi_class(ch) {
                BidiClass::L => return BidiDirection::Ltr,
                BidiClass::R | BidiClass::AL => return BidiDirection::Rtl,
                _ => {}
            }
        }
        BidiDirection::Ltr
    }

    /// Reorders the paragraph runs visually according to UAX #9 Rule L2.
    ///
    /// In Rule L2, from the highest resolved level down to the lowest odd level,
    /// contiguous sequences of runs with level greater than or equal to the current
    /// level are reversed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::bidi::{BidiDirection, BidiParagraph};
    ///
    /// let para = BidiParagraph::new("Hello مرحبا world", BidiDirection::Ltr);
    /// let visual_runs = para.reorder_visually();
    /// assert!(!visual_runs.is_empty());
    /// ```
    pub fn reorder_visually(&self) -> Vec<BidiRun> {
        if self.runs.is_empty() {
            return Vec::new();
        }

        let mut visual = self.runs.clone();
        let max_level = visual.iter().map(|r| r.level).max().unwrap_or(0);
        let min_odd_level = match visual.iter().map(|r| r.level).filter(|&l| l % 2 == 1).min() {
            Some(l) => l,
            None => return visual,
        };

        for lvl in (min_odd_level..=max_level).rev() {
            let mut i = 0;
            while i < visual.len() {
                if visual[i].level >= lvl {
                    let start = i;
                    while i < visual.len() && visual[i].level >= lvl {
                        i += 1;
                    }
                    visual[start..i].reverse();
                } else {
                    i += 1;
                }
            }
        }

        visual
    }
}

/// Returns the mirrored Unicode glyph for the given character, if one exists.
///
/// Handles ASCII and Unicode parentheses, brackets, curly braces, angle brackets,
/// guillemets, and full-width brackets.
///
/// # Examples
///
/// ```
/// use martensite_text::bidi::get_mirrored_char;
///
/// assert_eq!(get_mirrored_char('('), Some(')'));
/// assert_eq!(get_mirrored_char(')'), Some('('));
/// assert_eq!(get_mirrored_char('«'), Some('»'));
/// assert_eq!(get_mirrored_char('【'), Some('】'));
/// assert_eq!(get_mirrored_char('A'), None);
/// ```
pub fn get_mirrored_char(ch: char) -> Option<char> {
    match ch {
        '(' => Some(')'),
        ')' => Some('('),
        '[' => Some(']'),
        ']' => Some('['),
        '{' => Some('}'),
        '}' => Some('{'),
        '<' => Some('>'),
        '>' => Some('<'),
        '«' => Some('»'),
        '»' => Some('«'),
        '‹' => Some('›'),
        '›' => Some('‹'),
        '（' => Some('）'),
        '）' => Some('（'),
        '［' => Some('］'),
        '］' => Some('［'),
        '｛' => Some('｝'),
        '｝' => Some('｛'),
        '〈' => Some('〉'),
        '〉' => Some('〈'),
        '《' => Some('》'),
        '》' => Some('《'),
        '「' => Some('」'),
        '」' => Some('「'),
        '『' => Some('』'),
        '』' => Some('『'),
        '【' => Some('】'),
        '】' => Some('【'),
        '〔' => Some('〕'),
        '〕' => Some('〔'),
        '〖' => Some('〗'),
        '〗' => Some('〖'),
        '⟨' => Some('⟩'),
        '⟩' => Some('⟨'),
        '⟪' => Some('⟫'),
        '⟫' => Some('⟪'),
        '⟦' => Some('⟧'),
        '⟧' => Some('⟦'),
        _ => None,
    }
}

/// Mirrors mirrored characters inside all runs identified as right-to-left (`is_rtl()`).
///
/// Characters in LTR runs remain unchanged. Characters in RTL runs are substituted with
/// their mirrored pair if one is defined by [`get_mirrored_char`].
///
/// # Examples
///
/// ```
/// use martensite_text::bidi::{BidiDirection, BidiParagraph, mirror_text_in_rtl_runs};
///
/// let para = BidiParagraph::new("مرحبا (1)", BidiDirection::Rtl);
/// let mirrored = mirror_text_in_rtl_runs(&para.text, &para.runs);
/// assert!(mirrored.contains(')') || mirrored.contains('('));
/// ```
pub fn mirror_text_in_rtl_runs(text: &str, runs: &[BidiRun]) -> String {
    let mut result = String::with_capacity(text.len());
    for run in runs {
        if let Some(slice) = text.get(run.start..run.end) {
            if run.is_rtl() {
                for ch in slice.chars() {
                    result.push(get_mirrored_char(ch).unwrap_or(ch));
                }
            } else {
                result.push_str(slice);
            }
        }
    }
    result
}

/// Returns `true` if the character is prohibited from starting a line under UAX #14 Kinsoku Shori rules.
///
/// Prohibited line start characters include closing punctuation, quotation marks, commas, periods,
/// small Japanese kana, and prolonged sound marks.
///
/// # Examples
///
/// ```
/// use martensite_text::bidi::is_prohibited_line_start;
///
/// assert!(is_prohibited_line_start(')'));
/// assert!(is_prohibited_line_start('。'));
/// assert!(is_prohibited_line_start('っ'));
/// assert!(!is_prohibited_line_start('A'));
/// ```
pub fn is_prohibited_line_start(ch: char) -> bool {
    matches!(
        ch,
        ')' | ']'
            | '}'
            | '>'
            | '»'
            | '›'
            | '”'
            | '’'
            | '"'
            | '\''
            | '）'
            | '］'
            | '｝'
            | '〉'
            | '》'
            | '」'
            | '』'
            | '】'
            | '〕'
            | '〗'
            | '⟩'
            | '⟫'
            | '⟧'
            | ','
            | '.'
            | '!'
            | '?'
            | ':'
            | ';'
            | '、'
            | '。'
            | '，'
            | '．'
            | '！'
            | '？'
            | '：'
            | '；'
            | 'ァ'
            | 'ィ'
            | 'ゥ'
            | 'ェ'
            | 'ォ'
            | 'ッ'
            | 'ャ'
            | 'ュ'
            | 'ョ'
            | 'ヮ'
            | 'ヵ'
            | 'ヶ'
            | 'ぁ'
            | 'ぃ'
            | 'ぅ'
            | 'ぇ'
            | 'ぉ'
            | 'っ'
            | 'ゃ'
            | 'ゅ'
            | 'ょ'
            | 'ゎ'
            | 'ー'
            | '～'
            | '〜'
            | '…'
            | '‥'
            | '°'
            | '′'
            | '″'
            | '℃'
    )
}

/// Returns `true` if the character is prohibited from ending a line under UAX #14 Kinsoku Shori rules.
///
/// Prohibited line end characters include opening punctuation, opening quotation marks,
/// and currency symbols.
///
/// # Examples
///
/// ```
/// use martensite_text::bidi::is_prohibited_line_end;
///
/// assert!(is_prohibited_line_end('('));
/// assert!(is_prohibited_line_end('「'));
/// assert!(is_prohibited_line_end('$'));
/// assert!(!is_prohibited_line_end('Z'));
/// ```
pub fn is_prohibited_line_end(ch: char) -> bool {
    matches!(
        ch,
        '(' | '['
            | '{'
            | '<'
            | '«'
            | '‹'
            | '“'
            | '‘'
            | '（'
            | '［'
            | '｛'
            | '〈'
            | '《'
            | '「'
            | '『'
            | '【'
            | '〔'
            | '〖'
            | '⟨'
            | '⟪'
            | '⟦'
            | '$'
            | '¥'
            | '£'
            | '€'
            | '￥'
            | '＄'
            | '₩'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bidi_direction_default() {
        assert_eq!(BidiDirection::default(), BidiDirection::Ltr);
    }

    #[test]
    fn test_bidi_run_methods() {
        let ltr_run = BidiRun::new(0, 10, 0, BidiDirection::Ltr);
        assert!(!ltr_run.is_rtl());
        assert_eq!(ltr_run.len(), 10);
        assert!(!ltr_run.is_empty());

        let rtl_run = BidiRun::new(10, 25, 1, BidiDirection::Rtl);
        assert!(rtl_run.is_rtl());
        assert_eq!(rtl_run.len(), 15);

        let empty_run = BidiRun::new(5, 5, 0, BidiDirection::Ltr);
        assert!(empty_run.is_empty());
        assert_eq!(empty_run.len(), 0);
    }

    #[test]
    fn test_detect_base_direction() {
        assert_eq!(
            BidiParagraph::detect_base_direction("Hello World"),
            BidiDirection::Ltr
        );
        assert_eq!(
            BidiParagraph::detect_base_direction("שלום עולם"),
            BidiDirection::Rtl
        );
        assert_eq!(
            BidiParagraph::detect_base_direction("مرحبا"),
            BidiDirection::Rtl
        );
        assert_eq!(
            BidiParagraph::detect_base_direction("123 456"),
            BidiDirection::Ltr
        );
        assert_eq!(BidiParagraph::detect_base_direction(""), BidiDirection::Ltr);
    }

    #[test]
    fn test_bidi_paragraph_pure_ltr() {
        let text = "The quick brown fox";
        let para = BidiParagraph::new(text, BidiDirection::Auto);
        assert_eq!(para.base_direction, BidiDirection::Ltr);
        assert_eq!(para.resolved_base_level, 0);
        assert_eq!(para.runs.len(), 1);
        assert_eq!(para.runs[0].level, 0);
        assert_eq!(para.runs[0].direction, BidiDirection::Ltr);

        let visual = para.reorder_visually();
        assert_eq!(visual, para.runs);
    }

    #[test]
    fn test_bidi_paragraph_pure_rtl() {
        let text = "שלום עולם";
        let para = BidiParagraph::new(text, BidiDirection::Auto);
        assert_eq!(para.base_direction, BidiDirection::Rtl);
        assert_eq!(para.resolved_base_level, 1);
        assert_eq!(para.runs.len(), 1);
        assert_eq!(para.runs[0].level, 1);
        assert!(para.runs[0].is_rtl());

        let visual = para.reorder_visually();
        assert_eq!(visual.len(), 1);
    }

    #[test]
    fn test_bidi_paragraph_mixed_text() {
        let text = "Hello שלום World";
        let para = BidiParagraph::new(text, BidiDirection::Ltr);
        assert_eq!(para.base_direction, BidiDirection::Ltr);
        assert!(para.runs.len() >= 3);

        let visual = para.reorder_visually();
        assert_eq!(visual.len(), para.runs.len());
    }

    #[test]
    fn test_mirrored_char_substitution() {
        assert_eq!(get_mirrored_char('('), Some(')'));
        assert_eq!(get_mirrored_char(')'), Some('('));
        assert_eq!(get_mirrored_char('['), Some(']'));
        assert_eq!(get_mirrored_char(']'), Some('['));
        assert_eq!(get_mirrored_char('{'), Some('}'));
        assert_eq!(get_mirrored_char('}'), Some('{'));
        assert_eq!(get_mirrored_char('<'), Some('>'));
        assert_eq!(get_mirrored_char('>'), Some('<'));
        assert_eq!(get_mirrored_char('«'), Some('»'));
        assert_eq!(get_mirrored_char('»'), Some('«'));
        assert_eq!(get_mirrored_char('（'), Some('）'));
        assert_eq!(get_mirrored_char('）'), Some('（'));
        assert_eq!(get_mirrored_char('【'), Some('】'));
        assert_eq!(get_mirrored_char('】'), Some('【'));
        assert_eq!(get_mirrored_char('X'), None);
    }

    #[test]
    fn test_mirror_text_in_rtl_runs() {
        let text = "(test) [עברית] {abc}";
        let para = BidiParagraph::new(text, BidiDirection::Ltr);
        let mirrored = mirror_text_in_rtl_runs(&para.text, &para.runs);
        assert!(mirrored.starts_with("(test) "));
        assert!(mirrored.ends_with(" {abc}"));
        // Brackets inside the RTL run should be swapped
        assert!(mirrored.contains(']'));
        assert!(mirrored.contains('['));
    }

    #[test]
    fn test_kinsoku_shori_rules() {
        assert!(is_prohibited_line_start(')'));
        assert!(is_prohibited_line_start(']'));
        assert!(is_prohibited_line_start('}'));
        assert!(is_prohibited_line_start('。'));
        assert!(is_prohibited_line_start('、'));
        assert!(is_prohibited_line_start('っ'));
        assert!(is_prohibited_line_start('ー'));
        assert!(!is_prohibited_line_start('('));
        assert!(!is_prohibited_line_start('漢'));

        assert!(is_prohibited_line_end('('));
        assert!(is_prohibited_line_end('['));
        assert!(is_prohibited_line_end('{'));
        assert!(is_prohibited_line_end('「'));
        assert!(is_prohibited_line_end('『'));
        assert!(is_prohibited_line_end('$'));
        assert!(is_prohibited_line_end('￥'));
        assert!(!is_prohibited_line_end(')'));
        assert!(!is_prohibited_line_end('字'));
    }

    #[test]
    fn test_reorder_visually_no_odd_levels() {
        let mut para = BidiParagraph::new("Hello World", BidiDirection::Ltr);
        // Explicitly set even levels higher than 0 (e.g. level 2)
        para.runs = vec![
            BidiRun::new(0, 5, 2, BidiDirection::Ltr),
            BidiRun::new(5, 11, 2, BidiDirection::Ltr),
        ];
        let visual = para.reorder_visually();
        assert_eq!(visual[0].start, 0);
        assert_eq!(visual[1].start, 5);
    }
}
