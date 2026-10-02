//! Code-point tables for the sanitization pipeline.
//!
//! Ranges are numeric rather than literal characters so the source
//! stays pure ASCII and auditable — the invisible characters being
//! filtered must never appear in the filter's own source.

/// Inclusive code-point ranges stripped by the aggressive profile
/// regardless of flags — the always-hostile set. Whitespace and the
/// functional invisibles are NOT here; they are policy-flagged.
///
/// References: Unicode TR36/TR39 (Identifier_Type, Default_Ignorable),
/// OWASP LLM prompt-injection cheat sheet, Trojan Source (CVE-2021-42574).
pub(crate) const STRIP_ALWAYS: &[(u32, u32)] = &[
    // C1 controls (0x80..=0x9F) — includes NEL.
    (0x0080, 0x009F),
    // DEL is in the C0 table; 0x7F.
    (0x007F, 0x007F),
    // Soft hyphen — invisible in most renderers, keyword-evasion vector.
    (0x00AD, 0x00AD),
    // Combining grapheme joiner — invisible modifier.
    (0x034F, 0x034F),
    // Stateful bidi embeddings and overrides — Trojan Source.
    (0x202A, 0x202E),
    // Invisible math operators and word joiner U+2060..=0x2064.
    (0x2060, 0x2064),
    // Bidi isolates U+2066..=0x2069.
    (0x2066, 0x2069),
    // Deprecated format characters U+206A..=0x206F.
    (0x206A, 0x206F),
    // Interlinear annotation controls.
    (0xFFF9, 0xFFFC),
    // BOM / zero-width no-break space.
    (0xFEFF, 0xFEFF),
    // Noncharacters U+FDD0..=0xFDEF.
    (0xFDD0, 0xFDEF),
    // Tag block (Used for invisible tagging in early Unicode).
    (0xE0000, 0xE007F),
];

/// C0 controls kept even under the aggressive profile: tab, LF, CR.
/// `SanitizeContext::single_line` removes LF/CR (and the Unicode line
/// separators) downstream — the table itself keeps them so multiline
/// fields keep them.
pub(crate) const BASELINE_KEEP_C0: &[u32] = &[0x09, 0x0A, 0x0D];

/// C0 range (strip all except `BASELINE_KEEP_C0`).
pub(crate) const C0: (u32, u32) = (0x00, 0x1F);

/// Unicode line separators removed when `single_line` is set —
/// beyond `\n`/`\r`: NEL, LS, PS.
pub(crate) const LINE_SEPARATORS: &[u32] = &[0x0A, 0x0D, 0x0085, 0x2028, 0x2029];

/// Exotic Unicode whitespace mapped to ASCII space — visible-space
/// lookalikes that break token matching and column alignment:
/// NBSP, the U+2000..=U+200A family (en quad … hair space), narrow
/// NBSP, medium math space, ideographic space, Ogham space mark, and
/// the Hangul "blank" fillers which render as empty glyphs.
pub(crate) const EXOTIC_SPACES: &[u32] = &[
    0x00A0, // NO-BREAK SPACE
    0x115F, 0x1160, // Hangul Jungseong/Choseong filler
    0x1680, // Ogham space mark
    0x180E, // Mongolian vowel separator
    // U+2000..=U+200A — en quad through hair space.
    0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200A,
    0x202F, // narrow no-break space
    0x205F, // medium mathematical space
    0x3000, // ideographic space
    0x3164, // Hangul filler
    0xFFA0, // halfwidth Hangul filler
];

/// Functional invisible characters — legitimate typographic and
/// script-shaping semantics, preserved by the aggressive profile
/// unless `strip_zw_chars` is set:
///
/// * U+200B ZWSP — break hints in Thai/Khmer/Lao (no spaces).
/// * U+200C ZWNJ — required by Persian and Indic orthography.
/// * U+200D ZWJ — emoji ZWJ sequences and Indic conjuncts.
/// * U+200E/U+200F LRM/RLM — needed by legitimate bidi text.
pub(crate) const FUNCTIONAL_INVISIBLES: &[u32] = &[0x200B, 0x200C, 0x200D, 0x200E, 0x200F];

/// Emoji/text variation selectors U+FE00..=U+FE0F and the
/// supplementary selectors — stripped only under
/// `strip_variation_selectors`.
pub(crate) const VARIATION_SELECTORS: &[(u32, u32)] = &[(0xFE00, 0xFE0F), (0xE0100, 0xE01EF)];

/// Private Use Areas — kept by default (icon fonts use them
/// legitimately); `strip_private_use` removes them.
pub(crate) const PRIVATE_USE: &[(u32, u32)] =
    &[(0xE000, 0xF8FF), (0xF0000, 0xFFFFD), (0x100000, 0x10FFFD)];

/// `true` when `cp` is inside any inclusive range of `table`.
pub(crate) fn in_ranges(table: &[(u32, u32)], cp: u32) -> bool {
    table.iter().any(|&(lo, hi)| (lo..=hi).contains(&cp))
}

/// `true` when `cp` equals any listed code point.
pub(crate) fn in_list(table: &[u32], cp: u32) -> bool {
    table.contains(&cp)
}

/// `true` for plane-final noncharacters `U+xFFFE`/`U+xFFFF` (the
/// BMP pair is also covered, alongside U+FDD0..FDEF in
/// `STRIP_ALWAYS`).
pub(crate) fn is_plane_noncharacter(cp: u32) -> bool {
    cp & 0xFFFE == 0xFFFE
}

/// ASCII-adjacent confusables NFKC does NOT fold — a small curated
/// map of code points that render as ASCII punctuation. Deliberately
/// excludes scripts (Cyrillic/Greek lookalikes are legitimate text)
/// and typographic quotes (curly quotes are authored content, not
/// deception).
pub struct ConfusableFold;

impl ConfusableFold {
    /// Folds one code point to its ASCII-adjacent form, or `None`
    /// when the point is not a confusable.
    pub(crate) fn fold(cp: u32) -> Option<char> {
        Some(match cp {
            0x02B9 | 0x02BB | 0x02BC | 0x02BD | 0x2032 | 0xFF07 => '\'', // primes, modifier apostrophes
            0x02BA | 0x2033 | 0x2034 | 0xFF02 => '"',                    // double primes
            0x02CB | 0x2035 | 0xFF40 => '`',                             // graves / reversed prime
            0x2212 | 0x2010 | 0x2011 | 0xFE58 | 0xFE63 | 0xFF0D => '-',  // minus/dash lookalikes
            0x2215 | 0xFF0F => '/',                                      // division slash
            0x2216 | 0xFF3C => '\\',                                     // set minus
            0xFF0C => ',',
            0xFF0E => '.',
            0xFF1A => ':',
            0xFF1B => ';',
            0xFF01 => '!',
            0xFF1F => '?',
            0xFF08 | 0x207D | 0x208D => '(',
            0xFF09 | 0x207E | 0x208E => ')',
            0xFF3B => '[',
            0xFF3D => ']',
            0xFF5B => '{',
            0xFF5D => '}',
            _ => return None,
        })
    }
}
