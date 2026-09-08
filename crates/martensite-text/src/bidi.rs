//! Unicode Bidirectional Algorithm (UAX #9) integration for shaping.
//!
//! This module wraps the canonical `unicode-bidi` crate to expose
//! Martensite-level primitives:
//! - Paragraph analysis with automatic base direction detection.
//! - Logical and visual run extraction.
//! - Logical/visual character index mapping.
//! - Mirrored glyph positions for RTL runs without corrupting source text.
//!
//! Mirroring and line-break prohibitions are no longer performed by dead
//! helper functions on raw strings; instead they are produced as metadata
//! that the shaping pipeline can apply to individual glyphs. See
//! [`line_break`](crate::line_break) for UAX #14 support.

use unicode_bidi::{bidi_class, BidiClass, BidiInfo, Level, ParagraphInfo};

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

impl BidiDirection {
    /// Converts a `bool` RTL flag into a [`BidiDirection`].
    #[inline]
    pub const fn from_rtl(rtl: bool) -> Self {
        if rtl {
            Self::Rtl
        } else {
            Self::Ltr
        }
    }
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
    /// Visual display order index (0 is leftmost in the rendered line).
    pub visual_order: usize,
    /// Logical order index (position in the source string).
    pub logical_order: usize,
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
            visual_order: 0,
            logical_order: 0,
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
                let mut run = BidiRun::new(run_start, i, current_level, dir);
                run.logical_order = runs.len();
                runs.push(run);
                run_start = i;
                current_level = lvl_num;
            }
        }

        let dir = if current_level % 2 == 1 {
            BidiDirection::Rtl
        } else {
            BidiDirection::Ltr
        };
        let mut last = BidiRun::new(run_start, text.len(), current_level, dir);
        last.logical_order = runs.len();
        runs.push(last);

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
    ///
    /// Each returned run's [`BidiRun::visual_order`] is populated with its
    /// position in display order; [`BidiRun::logical_order`] retains the
    /// run's position in source order.
    pub fn reorder_visually(&self) -> Vec<BidiRun> {
        if self.runs.is_empty() {
            return Vec::new();
        }

        let mut visual = self.runs.clone();
        let max_level = visual.iter().map(|r| r.level).max().unwrap_or(0);
        let min_odd_level = match visual.iter().map(|r| r.level).filter(|&l| l % 2 == 1).min() {
            Some(l) => l,
            None => {
                for (i, run) in visual.iter_mut().enumerate() {
                    run.visual_order = i;
                }
                return visual;
            }
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

        for (i, run) in visual.iter_mut().enumerate() {
            run.visual_order = i;
        }
        visual
    }
}

/// A fully resolved BiDi paragraph that exposes logical/visual index mapping.
///
/// Unlike [`BidiParagraph`], this type stores the underlying `BidiInfo` so it
/// can produce character-level mappings between logical source positions and
/// visual display positions.
#[derive(Debug)]
pub struct BidiResolved<'text> {
    text: &'text str,
    bidi_info: BidiInfo<'text>,
    para: ParagraphInfo,
    base_direction: BidiDirection,
}

impl<'text> BidiResolved<'text> {
    /// Resolves the BiDi embedding levels for `text`.
    pub fn new(text: &'text str, default_dir: BidiDirection) -> Self {
        let base_level = match default_dir {
            BidiDirection::Auto => match BidiInfo::new(text, None).paragraphs.first() {
                Some(p) => p.level.number(),
                None => 0,
            },
            BidiDirection::Ltr => 0,
            BidiDirection::Rtl => 1,
        };
        let level = Level::new(base_level).ok();
        let bidi_info = BidiInfo::new(text, level);
        let para = bidi_info
            .paragraphs
            .first()
            .cloned()
            .unwrap_or_else(|| ParagraphInfo {
                range: 0..text.len(),
                level: Level::ltr(),
            });
        let base_direction = if base_level % 2 == 1 {
            BidiDirection::Rtl
        } else {
            BidiDirection::Ltr
        };
        Self {
            text,
            bidi_info,
            para,
            base_direction,
        }
    }

    /// Returns the original text.
    #[inline]
    pub fn text(&self) -> &'text str {
        self.text
    }

    /// Returns the base paragraph direction.
    #[inline]
    pub fn base_direction(&self) -> BidiDirection {
        self.base_direction
    }

    /// Returns the resolved base embedding level.
    #[inline]
    pub fn base_level(&self) -> u8 {
        self.para.level.number()
    }

    /// Returns the logical runs of the paragraph in source order.
    pub fn logical_runs(&self) -> Vec<BidiRun> {
        let levels = self
            .bidi_info
            .reordered_levels(&self.para, self.para.range.clone());
        if levels.is_empty() {
            return Vec::new();
        }
        let mut runs = Vec::new();
        let mut start = self.para.range.start;
        let mut current = levels[start].number();
        for (i, lvl) in levels
            .iter()
            .enumerate()
            .skip(self.para.range.start + 1)
            .take(
                self.para
                    .range
                    .end
                    .saturating_sub(self.para.range.start + 1),
            )
        {
            let num = lvl.number();
            if num != current {
                runs.push(self.make_run(start, i, current, runs.len()));
                start = i;
                current = num;
            }
        }
        runs.push(self.make_run(start, self.para.range.end, current, runs.len()));
        runs
    }

    /// Returns the visual runs of the paragraph in display order.
    pub fn visual_runs(&self) -> Vec<BidiRun> {
        let logical = self.logical_runs();
        let (levels, visual) = self
            .bidi_info
            .visual_runs(&self.para, self.para.range.clone());
        let mut result = Vec::with_capacity(visual.len());
        for (visual_order, run) in visual.iter().enumerate() {
            let level = levels[run.start].number();
            let start = run.start;
            let end = run.end;
            let logical_order = logical
                .iter()
                .position(|r| r.start == start && r.end == end)
                .unwrap_or(0);
            let mut r = self.make_run(start, end, level, logical_order);
            r.visual_order = visual_order;
            result.push(r);
        }
        result
    }

    /// Maps a logical character index to its visual display index.
    pub fn logical_to_visual(&self, logical_index: usize) -> Option<usize> {
        let levels = self
            .bidi_info
            .reordered_levels_per_char(&self.para, self.para.range.clone());
        let visual_map = BidiInfo::reorder_visual(&levels);
        // Build the inverse map.
        let mut logical_to_visual = vec![None; visual_map.len()];
        for (visual, &logical) in visual_map.iter().enumerate() {
            if logical < logical_to_visual.len() {
                logical_to_visual[logical] = Some(visual);
            }
        }
        logical_to_visual.get(logical_index).copied().flatten()
    }

    /// Maps a visual display index to its logical source index.
    pub fn visual_to_logical(&self, visual_index: usize) -> Option<usize> {
        let levels = self
            .bidi_info
            .reordered_levels_per_char(&self.para, self.para.range.clone());
        let visual_map = BidiInfo::reorder_visual(&levels);
        visual_map.get(visual_index).copied()
    }

    /// Returns a sequence of (byte_index, mirrored_char) pairs for characters
    /// that should be mirrored in RTL contexts.
    pub fn mirror_map(&self) -> BidiMirrorMap {
        let levels = self
            .bidi_info
            .reordered_levels(&self.para, self.para.range.clone());
        let level_numbers: Vec<u8> = levels.iter().map(|l| l.number()).collect();
        BidiMirrorMap::for_text_and_levels(self.text, &level_numbers)
    }

    fn make_run(&self, start: usize, end: usize, level: u8, logical_order: usize) -> BidiRun {
        let direction = if level % 2 == 1 {
            BidiDirection::Rtl
        } else {
            BidiDirection::Ltr
        };
        BidiRun {
            start,
            end,
            level,
            direction,
            visual_order: 0,
            logical_order,
        }
    }
}

/// Map of character positions to their mirrored glyphs for RTL display.
///
/// This is intended to be applied by the glyph rasterizer, not by mutating
/// the source text before shaping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidiMirrorMap {
    entries: Vec<(usize, char)>,
}

impl BidiMirrorMap {
    /// Creates a mirror map for `text` by resolving its embedding levels
    /// with the default (auto-detected) paragraph direction.
    ///
    /// Equivalent to running [`BidiInfo::new`] on the text and feeding the
    /// resulting levels to [`Self::for_text_and_levels`]. For explicit
    /// control over the base direction use [`BidiResolved::mirror_map`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::bidi::BidiMirrorMap;
    ///
    /// let map = BidiMirrorMap::new("(abc)");
    /// // Auto-detected direction is LTR, so nothing is mirrored.
    /// assert!(map.entries().is_empty());
    /// ```
    pub fn new(text: &str) -> Self {
        if text.is_empty() {
            return Self {
                entries: Vec::new(),
            };
        }
        let bidi_info = BidiInfo::new(text, None);
        let levels: Vec<u8> = bidi_info.levels.iter().map(|l| l.number()).collect();
        Self::for_text_and_levels(text, &levels)
    }

    /// Creates a mirror map for the given text and resolved embedding levels.
    ///
    /// A character is mirrored when it has the `Bidi_Mirrored` property and is
    /// located in an RTL embedding level. The byte index recorded is the start
    /// byte of the character in the source text.
    pub fn for_text_and_levels(text: &str, levels: &[u8]) -> Self {
        let mut entries = Vec::new();
        for (idx, ch) in text.char_indices() {
            if idx >= levels.len() {
                break;
            }
            let rtl = levels[idx] % 2 == 1;
            if rtl && unicode_bidi_mirroring::is_mirroring(ch) {
                if let Some(mirror) = Self::char_mirror(ch) {
                    entries.push((idx, mirror));
                }
            }
        }
        Self { entries }
    }

    fn char_mirror(ch: char) -> Option<char> {
        // The canonical UAX #9 mirroring table.
        unicode_bidi_mirroring::get_mirrored(ch)
    }

    /// Returns the mirrored glyph for the byte position, if any.
    pub fn mirror_at(&self, byte_index: usize) -> Option<char> {
        self.entries
            .iter()
            .find(|(idx, _)| *idx == byte_index)
            .map(|(_, ch)| *ch)
    }

    /// Returns all mirrored positions.
    #[inline]
    pub fn entries(&self) -> &[(usize, char)] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bidi_direction_default() {
        assert_eq!(BidiDirection::default(), BidiDirection::Ltr);
    }

    #[test]
    fn bidi_direction_from_rtl() {
        assert_eq!(BidiDirection::from_rtl(false), BidiDirection::Ltr);
        assert_eq!(BidiDirection::from_rtl(true), BidiDirection::Rtl);
    }

    #[test]
    fn bidi_run_methods() {
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
    fn detect_base_direction() {
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
    fn bidi_paragraph_pure_ltr() {
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
    fn bidi_paragraph_pure_rtl() {
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
    fn bidi_paragraph_mixed_text() {
        let text = "Hello שלום World";
        let para = BidiParagraph::new(text, BidiDirection::Ltr);
        assert_eq!(para.base_direction, BidiDirection::Ltr);
        assert!(para.runs.len() >= 3);

        let visual = para.reorder_visually();
        assert_eq!(visual.len(), para.runs.len());
    }

    #[test]
    fn reorder_visually_no_odd_levels() {
        let mut para = BidiParagraph::new("Hello World", BidiDirection::Ltr);
        para.runs = vec![
            BidiRun::new(0, 5, 2, BidiDirection::Ltr),
            BidiRun::new(5, 11, 2, BidiDirection::Ltr),
        ];
        let visual = para.reorder_visually();
        assert_eq!(visual[0].start, 0);
        assert_eq!(visual[1].start, 5);
    }

    #[test]
    fn resolved_logical_runs_present() {
        let text = "abc שלום 123";
        let resolved = BidiResolved::new(text, BidiDirection::Auto);
        let runs = resolved.logical_runs();
        assert!(
            runs.len() >= 3,
            "expected multiple logical runs for mixed LTR/RTL text, got {}",
            runs.len()
        );
        for (i, run) in runs.iter().enumerate() {
            assert_eq!(run.logical_order, i);
        }
    }

    #[test]
    fn resolved_visual_runs_reordered() {
        let text = "abc שלום 123";
        let resolved = BidiResolved::new(text, BidiDirection::Auto);
        let visual = resolved.visual_runs();
        assert!(!visual.is_empty());
        // Visual order indices must be a permutation of 0..n.
        let orders: Vec<_> = visual.iter().map(|r| r.visual_order).collect();
        let mut sorted = orders.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..orders.len()).collect::<Vec<_>>());
    }

    #[test]
    fn resolved_logical_visual_roundtrip() {
        let text = "Hello مرحبا World";
        let resolved = BidiResolved::new(text, BidiDirection::Auto);
        let char_count = text.chars().count();
        for logical in 0..char_count {
            let visual = resolved
                .logical_to_visual(logical)
                .expect("valid logical index");
            let back = resolved
                .visual_to_logical(visual)
                .expect("valid visual index");
            assert_eq!(
                back, logical,
                "logical<->visual roundtrip failed at logical {logical} -> visual {visual}"
            );
        }
    }

    #[test]
    fn mirror_map_identifies_parens_in_rtl() {
        let text = "(test)";
        let resolved = BidiResolved::new(text, BidiDirection::Rtl);
        let map = resolved.mirror_map();
        assert!(
            map.mirror_at(text.find('(').unwrap()).is_some(),
            "opening paren should be mirrored in RTL"
        );
    }
}
