//! Unicode line breaking (UAX #14) with Kinsoku Shori integration.
//!
//! This module provides a real UAX #14 implementation via the
//! `unicode-linebreak` crate, plus East-Asian typography rules that
//! prohibit specific characters from starting or ending a line.

/// Classification of a line-break opportunity between two adjacent
/// characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BreakOpportunity {
    /// A line break is allowed at this position.
    Allowed,
    /// A line break is mandatory (e.g. after LF).
    Mandatory,
    /// A line break is prohibited at this position.
    Prohibited,
}

/// A line breaker that resolves UAX #14 break opportunities and applies
/// Kinsoku Shori adjustments.
///
/// In addition to the raw UAX #14 opportunities reported by
/// `unicode-linebreak`, this breaker applies two East-Asian typography
/// refinements:
///
/// - **Kinsoku Shori**: characters that may not start a line
///   ([`Self::is_kinsoku_start`]) or end a line
///   ([`Self::is_kinsoku_end`]) suppress the adjacent break
///   opportunity, even when UAX #14 would allow it.
/// - **Keep-all CJK**: breaks between two adjacent CJK characters are
///   suppressed so that ideographic words stay together (equivalent to
///   CSS `word-break: keep-all`).
#[derive(Debug, Clone, Copy, Default)]
pub struct LineBreaker;

impl LineBreaker {
    /// Returns a `BreakOpportunity` for every byte offset in `text`.
    ///
    /// The returned vector has length `text.len() + 1`. Index `i`
    /// describes the break opportunity *before* byte `i` (or after the
    /// final byte for the last entry).
    pub fn opportunities(text: &str) -> Vec<BreakOpportunity> {
        let mut ops = vec![BreakOpportunity::Prohibited; text.len() + 1];

        // Base UAX #14 opportunities. Offsets reported here denote a
        // break *before* that byte index.
        for (offset, opportunity) in unicode_linebreak::linebreaks(text) {
            if offset > text.len() {
                continue;
            }
            ops[offset] = match opportunity {
                unicode_linebreak::BreakOpportunity::Mandatory => BreakOpportunity::Mandatory,
                unicode_linebreak::BreakOpportunity::Allowed => BreakOpportunity::Allowed,
                #[allow(unreachable_patterns)]
                _ => BreakOpportunity::Prohibited,
            };
        }

        // Apply Kinsoku Shori and keep-all CJK suppression.
        for (offset, next) in text.char_indices() {
            if offset == 0 {
                continue;
            }
            // Kinsoku Shori and keep-all suppression may only downgrade an
            // `Allowed` break opportunity. `Mandatory` breaks (e.g. after a
            // hard line break such as LF) are required by UAX #14 and must
            // never be suppressed.
            if ops[offset] != BreakOpportunity::Allowed {
                continue;
            }
            let prev = text[..offset].chars().next_back().unwrap_or('\0');

            // A kinsoku-start character must not begin a line.
            if Self::is_kinsoku_start(next) {
                ops[offset] = BreakOpportunity::Prohibited;
                continue;
            }
            // A kinsoku-end character must not be stranded at line end.
            if Self::is_kinsoku_end(prev) {
                ops[offset] = BreakOpportunity::Prohibited;
                continue;
            }
            // Keep adjacent CJK characters in the same word
            // (`word-break: keep-all` semantics).
            if Self::is_cjk(prev) && Self::is_cjk(next) {
                ops[offset] = BreakOpportunity::Prohibited;
            }
        }

        ops
    }

    /// Returns the byte offsets where a line break is allowed or required,
    /// after applying Kinsoku Shori restrictions.
    ///
    /// The returned offsets are in strictly increasing order. Offset `0`
    /// is never included; the end of text is included when the UAX #14
    /// tail break is present.
    pub fn break_points(text: &str) -> Vec<usize> {
        Self::opportunities(text)
            .iter()
            .enumerate()
            .skip(1)
            .filter_map(|(offset, opportunity)| {
                matches!(
                    opportunity,
                    BreakOpportunity::Allowed | BreakOpportunity::Mandatory
                )
                .then_some(offset)
            })
            .collect()
    }

    /// Returns `true` if `ch` may not appear at the start of a line.
    ///
    /// Covers closing punctuation, iteration/prolonged-sound marks, small
    /// kana, and sentence punctuation per JIS X 4051 / Kinsoku Shori.
    pub fn is_kinsoku_start(ch: char) -> bool {
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

    /// Returns `true` if `ch` may not appear at the end of a line.
    ///
    /// Covers opening brackets/quotes and currency symbols per
    /// JIS X 4051 / Kinsoku Shori.
    pub fn is_kinsoku_end(ch: char) -> bool {
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

    /// Returns `true` if `ch` belongs to a CJK script for the purposes of
    /// keep-all word breaking.
    fn is_cjk(ch: char) -> bool {
        crate::cascade::classify_script(ch) == crate::cascade::ScriptTag::Cjk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_break_opportunities_exist() {
        let text = "hello world";
        let ops = LineBreaker::opportunities(text);
        // One entry per byte plus a trailing entry.
        assert_eq!(ops.len(), text.len() + 1);
        // A break between the two words should be allowed.
        let space_index = text.find(' ').unwrap();
        assert_eq!(ops[space_index + 1], BreakOpportunity::Allowed);
    }

    #[test]
    fn cjk_no_break_inside_word() {
        let text = "日本語";
        let ops = LineBreaker::opportunities(text);
        assert_eq!(ops.len(), text.len() + 1);
        // No breaks are allowed in the middle of the CJK word.
        for (i, op) in ops.iter().enumerate().take(text.len()).skip(1) {
            assert!(
                !matches!(op, BreakOpportunity::Allowed | BreakOpportunity::Mandatory),
                "unexpected break at byte index {i}"
            );
        }
    }

    #[test]
    fn kinsoku_start_prohibits_closing_punctuation() {
        assert!(LineBreaker::is_kinsoku_start(')'));
        assert!(LineBreaker::is_kinsoku_start('。'));
        assert!(LineBreaker::is_kinsoku_start('、'));
        assert!(!LineBreaker::is_kinsoku_start('A'));
    }

    #[test]
    fn kinsoku_end_prohibits_opening_punctuation() {
        assert!(LineBreaker::is_kinsoku_end('('));
        assert!(LineBreaker::is_kinsoku_end('「'));
        assert!(LineBreaker::is_kinsoku_end('$'));
        assert!(!LineBreaker::is_kinsoku_end('Z'));
    }

    #[test]
    fn break_points_honor_kinsoku() {
        // "A（B" — ideally we should be able to break after "A", but not
        // leave the opening bracket at the end of a line by itself.
        let text = "A（B";
        let points = LineBreaker::break_points(text);
        assert!(
            !points.contains(&1),
            "breaking before U+FF08 opening paren would strand it at line end"
        );
    }

    #[test]
    fn mandatory_break_not_suppressed_by_kinsoku_or_cjk() {
        // A hard line break (LF) produces a Mandatory break opportunity
        // after it. Kinsoku-start suppression must not downgrade it to
        // Prohibited.
        let text = "a\n。b";
        let ops = LineBreaker::opportunities(text);
        let kinsoku_index = text.find('。').unwrap();
        assert_eq!(
            ops[kinsoku_index],
            BreakOpportunity::Mandatory,
            "mandatory break before a kinsoku-start character must be preserved"
        );

        // The same holds for the keep-all CJK rule: a mandatory break
        // between two CJK characters stays mandatory.
        let text = "日\n本";
        let ops = LineBreaker::opportunities(text);
        let after_lf = text.find('本').unwrap();
        assert_eq!(
            ops[after_lf],
            BreakOpportunity::Mandatory,
            "mandatory break between CJK characters must be preserved"
        );

        // Sanity check: the suppression still applies to Allowed breaks.
        let text = "a。b";
        let ops = LineBreaker::opportunities(text);
        let kinsoku_index = text.find('。').unwrap();
        assert_eq!(
            ops[kinsoku_index],
            BreakOpportunity::Prohibited,
            "allowed break before a kinsoku-start character is suppressed"
        );
    }
}
