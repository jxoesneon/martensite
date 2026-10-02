//! The built-in sanitization [`Profile`] and its builder.
//!
//! Pipeline order follows CERT IDS11 — normalization runs before any
//! removal so a character that folds to something hostile (a fullwidth
//! control lookalike, a NFKC-produced separator) is still caught by
//! the strip sets rather than sailing through a check that already ran.

use unicode_normalization::UnicodeNormalization;

use crate::tables::{
    in_list, in_ranges, is_plane_noncharacter, ConfusableFold, BASELINE_KEEP_C0, C0, EXOTIC_SPACES,
    FUNCTIONAL_INVISIBLES, LINE_SEPARATORS, PRIVATE_USE, STRIP_ALWAYS, VARIATION_SELECTORS,
};
use crate::{Phase, Sanitize, SanitizeContext};

/// A configurable sanitization profile.
///
/// [`Profile::aggressive`] is the widget default — every stage on
/// except the flags documented as deliberate keeps (functional
/// invisibles, variation selectors, private use). [`Profile::baseline`]
/// is the `sanitize(false)` floor: C0/C1/DEL/noncharacters only — the
/// structural hygiene every field has always enforced, with no
/// normalization and no invisible stripping.
#[derive(Clone, Debug)]
pub struct Profile {
    /// NFKC-normalize before filtering.
    pub normalize_nfkc: bool,
    /// Strip C0 controls except tab/LF/CR (line policy applies after).
    pub strip_c0: bool,
    /// Strip C1 controls + DEL.
    pub strip_c1: bool,
    /// Strip the always-hostile set: soft hyphen, CGJ, bidi
    /// embeddings/overrides/isolates, invisible math operators,
    /// deprecated format chars, interlinear controls, BOM, FDD0..
    /// FDEF, tag block, and plane-final noncharacters.
    pub strip_invisibles: bool,
    /// Also strip the functional invisibles — ZWSP, ZWNJ, ZWJ, LRM,
    /// RLM. Off by default: removing them destroys emoji sequences
    /// and breaks Indic/Persian/Thai shaping.
    pub strip_zw_chars: bool,
    /// Strip variation selectors (emoji/text presentation).
    pub strip_variation_selectors: bool,
    /// Strip private-use-area characters (off by default — icon
    /// fonts and some toolkits use PUA legitimately).
    pub strip_private_use: bool,
    /// Fold ASCII-adjacent confusables (primes, modifier apostrophes,
    /// fullwidth punctuation NFKC misses) to ASCII.
    pub fold_confusables: bool,
    /// Map exotic Unicode whitespace to U+0020.
    pub normalize_spaces: bool,
    /// Commit-phase trim of leading/trailing whitespace.
    pub trim_on_commit: bool,
}

impl Profile {
    /// The aggressive default — what `sanitize(true)` selects.
    pub fn aggressive() -> Self {
        Self {
            normalize_nfkc: true,
            strip_c0: true,
            strip_c1: true,
            strip_invisibles: true,
            strip_zw_chars: false,
            strip_variation_selectors: false,
            strip_private_use: false,
            fold_confusables: true,
            normalize_spaces: true,
            trim_on_commit: true,
        }
    }

    /// The structural floor — what `sanitize(false)` selects.
    pub fn baseline() -> Self {
        Self {
            normalize_nfkc: false,
            strip_c0: true,
            strip_c1: true,
            strip_invisibles: false,
            strip_zw_chars: false,
            strip_variation_selectors: false,
            strip_private_use: false,
            fold_confusables: false,
            normalize_spaces: false,
            trim_on_commit: false,
        }
    }

    /// A builder over the aggressive defaults.
    ///
    /// ```
    /// use martensite_sanitize::Profile;
    ///
    /// let strict = Profile::builder()
    ///     .strip_zw_chars(true) // hostile-input context
    ///     .strip_private_use(true)
    ///     .build();
    /// ```
    pub fn builder() -> ProfileBuilder {
        ProfileBuilder {
            profile: Self::aggressive(),
        }
    }

    /// The shared aggressive instance used by
    /// [`crate::SanitizerConfig::Aggressive`].
    pub(crate) const AGGRESSIVE: Self = Self {
        normalize_nfkc: true,
        strip_c0: true,
        strip_c1: true,
        strip_invisibles: true,
        strip_zw_chars: false,
        strip_variation_selectors: false,
        strip_private_use: false,
        fold_confusables: true,
        normalize_spaces: true,
        trim_on_commit: true,
    };

    /// The shared baseline instance used by
    /// [`crate::SanitizerConfig::Baseline`].
    pub(crate) const BASELINE: Self = Self {
        normalize_nfkc: false,
        strip_c0: true,
        strip_c1: true,
        strip_invisibles: false,
        strip_zw_chars: false,
        strip_variation_selectors: false,
        strip_private_use: false,
        fold_confusables: false,
        normalize_spaces: false,
        trim_on_commit: false,
    };

    /// `true` when `cp` is removed under this profile (before any
    /// line-separator handling, which `ctx.single_line` owns).
    fn strips(&self, cp: u32) -> bool {
        if in_list(LINE_SEPARATORS, cp) {
            // Line separators are context policy, not strip policy —
            // single_line removes them below; multiline keeps them.
            return false;
        }
        if self.strip_c0 && (C0.0..=C0.1).contains(&cp) && !in_list(BASELINE_KEEP_C0, cp) {
            return true;
        }
        if self.strip_invisibles && (in_ranges(STRIP_ALWAYS, cp) || is_plane_noncharacter(cp)) {
            return true;
        }
        if self.strip_zw_chars && in_list(FUNCTIONAL_INVISIBLES, cp) {
            return true;
        }
        if self.strip_variation_selectors && in_ranges(VARIATION_SELECTORS, cp) {
            return true;
        }
        if self.strip_private_use && in_ranges(PRIVATE_USE, cp) {
            return true;
        }
        // Baseline strips C1/DEL even without the invisibles stage.
        self.strip_c1 && (0x7F..=0x9F).contains(&cp)
    }
}

impl Sanitize for Profile {
    fn name(&self) -> &'static str {
        "martensite-sanitize::Profile"
    }

    fn sanitize(&self, input: &str, ctx: &SanitizeContext) -> String {
        // Stage 1 — normalization before removal (CERT IDS11): a
        // code point that folds to something strippable must be
        // caught by the sets below.
        let normalized;
        let input = if self.normalize_nfkc {
            normalized = input.nfkc().collect::<String>();
            normalized.as_str()
        } else {
            input
        };

        // Stage 2 — filter / map code points.
        let mut out = String::with_capacity(input.len());
        for ch in input.chars() {
            let cp = ch as u32;
            if self.strips(cp) {
                continue;
            }
            if self.fold_confusables {
                if let Some(folded) = ConfusableFold::fold(cp) {
                    out.push(folded);
                    continue;
                }
            }
            if self.normalize_spaces && in_list(EXOTIC_SPACES, cp) {
                out.push(' ');
                continue;
            }
            if ctx.single_line && in_list(LINE_SEPARATORS, cp) {
                continue;
            }
            out.push(ch);
        }

        // Stage 3 — line separators already removed above for
        // single-line fields; CRLF collapses to nothing there.

        // Stage 4 — commit-phase finishing.
        if self.trim_on_commit && ctx.phase == Phase::Commit {
            return out.trim().to_string();
        }
        out
    }
}

/// Builder over [`Profile::aggressive`].
#[derive(Clone, Debug)]
pub struct ProfileBuilder {
    profile: Profile,
}

impl ProfileBuilder {
    /// Tightens the profile to strip ZWSP/ZWNJ/ZWJ/LRM/RLM — use for
    /// hostile-input contexts where emoji and Indic/Persian/Thai
    /// shaping are not required.
    pub fn strip_zw_chars(mut self, on: bool) -> Self {
        self.profile.strip_zw_chars = on;
        self
    }

    /// Strips variation selectors (emoji/text presentation).
    pub fn strip_variation_selectors(mut self, on: bool) -> Self {
        self.profile.strip_variation_selectors = on;
        self
    }

    /// Strips private-use-area characters — off by default because
    /// icon fonts use PUA legitimately.
    pub fn strip_private_use(mut self, on: bool) -> Self {
        self.profile.strip_private_use = on;
        self
    }

    /// Disables NFKC normalization.
    pub fn normalize_nfkc(mut self, on: bool) -> Self {
        self.profile.normalize_nfkc = on;
        self
    }

    /// Disables the ASCII-adjacent confusable fold.
    pub fn fold_confusables(mut self, on: bool) -> Self {
        self.profile.fold_confusables = on;
        self
    }

    /// Disables exotic-space → ASCII-space mapping.
    pub fn normalize_spaces(mut self, on: bool) -> Self {
        self.profile.normalize_spaces = on;
        self
    }

    /// Disables the commit-phase trim.
    pub fn trim_on_commit(mut self, on: bool) -> Self {
        self.profile.trim_on_commit = on;
        self
    }

    /// Builds the profile.
    pub fn build(self) -> Profile {
        self.profile
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Phase, SanitizeContext, SanitizerConfig};

    fn ins_single() -> SanitizeContext {
        SanitizeContext::single_line(Phase::Insert)
    }

    fn ins_multi() -> SanitizeContext {
        SanitizeContext::multi_line(Phase::Insert)
    }

    fn commit_single() -> SanitizeContext {
        SanitizeContext::single_line(Phase::Commit)
    }

    #[test]
    fn aggressive_strips_bidi_overrides() {
        // Trojan Source vector — overrides and isolates are always
        // removed, directional marks are kept.
        let p = Profile::aggressive();
        assert_eq!(p.sanitize("a\u{202e}b", &ins_single()), "ab");
        assert_eq!(p.sanitize("a\u{2066}b\u{2069}", &ins_single()), "ab");
        assert_eq!(p.sanitize("a\u{200e}b", &ins_single()), "a\u{200e}b");
    }

    #[test]
    fn aggressive_strips_controls_keeps_whitespace() {
        let p = Profile::aggressive();
        assert_eq!(p.sanitize("a\x00b\x07\x1b", &ins_multi()), "ab");
        assert_eq!(p.sanitize("a\tb\nc", &ins_multi()), "a\tb\nc");
        // DEL and C1 are always removed.
        assert_eq!(p.sanitize("a\x7fb\u{0080}\u{009f}", &ins_multi()), "ab");
    }

    #[test]
    fn single_line_removes_all_line_separators() {
        let p = Profile::aggressive();
        assert_eq!(
            p.sanitize("a\nb\rc\u{85}d\u{2028}e\u{2029}f", &ins_single()),
            "abcdef"
        );
        // Multiline keeps them.
        assert_eq!(p.sanitize("a\nb", &ins_multi()), "a\nb");
    }

    #[test]
    fn aggressive_nfkc_folds_fullwidth_and_ligatures() {
        let p = Profile::aggressive();
        assert_eq!(p.sanitize("\u{ff21}\u{ff42}", &ins_single()), "Ab");
        assert_eq!(p.sanitize("\u{fb01}", &ins_single()), "fi");
        // Superscripts become digits.
        assert_eq!(p.sanitize("x\u{b2}", &ins_single()), "x2");
    }

    #[test]
    fn aggressive_folds_ascii_adjacent_confusables() {
        let p = Profile::aggressive();
        assert_eq!(p.sanitize("it\u{2032}s", &ins_single()), "it's");
        // Scripts are NOT folded — Cyrillic stays Cyrillic.
        assert_eq!(
            p.sanitize("\u{041d}\u{0435}", &ins_single()),
            "\u{041d}\u{0435}"
        );
        // Curly quotes are authored typography — kept.
        assert_eq!(
            p.sanitize("\u{201c}q\u{201d}", &ins_single()),
            "\u{201c}q\u{201d}"
        );
    }

    #[test]
    fn aggressive_keeps_functional_invisibles() {
        let p = Profile::aggressive();
        // Emoji ZWJ sequence survives intact.
        assert_eq!(
            p.sanitize("\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}", &ins_single()),
            "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}"
        );
        // ZWSP kept for Thai-style scripts; tighten via builder.
        assert_eq!(p.sanitize("a\u{200b}b", &ins_single()), "a\u{200b}b");
        let strict = Profile::builder().strip_zw_chars(true).build();
        assert_eq!(strict.sanitize("a\u{200b}b", &ins_single()), "ab");
    }

    #[test]
    fn aggressive_maps_exotic_spaces() {
        let p = Profile::aggressive();
        assert_eq!(
            p.sanitize("a\u{a0}b\u{2009}c\u{3000}d", &ins_single()),
            "a b c d"
        );
    }

    #[test]
    fn aggressive_strips_noncharacters_and_tags() {
        let p = Profile::aggressive();
        assert_eq!(p.sanitize("a\u{fdd0}b\u{fffe}c", &ins_single()), "abc");
        assert_eq!(p.sanitize("a\u{e0001}b", &ins_single()), "ab");
        assert_eq!(p.sanitize("a\u{feff}b", &ins_single()), "ab");
    }

    #[test]
    fn commit_phase_trims() {
        let p = Profile::aggressive();
        assert_eq!(p.sanitize("  hi  ", &commit_single()), "hi");
        // Insert phase never trims — the user is still typing.
        assert_eq!(p.sanitize("  hi  ", &ins_single()), "  hi  ");
    }

    #[test]
    fn baseline_is_structural_only() {
        let p = Profile::baseline();
        // Controls stripped, everything else verbatim.
        assert_eq!(p.sanitize("a\x00b", &ins_single()), "ab");
        assert_eq!(p.sanitize("\u{fb01}", &ins_single()), "\u{fb01}");
        assert_eq!(p.sanitize("a\u{202e}b", &ins_single()), "a\u{202e}b");
        assert_eq!(p.sanitize("  x  ", &commit_single()), "  x  ");
    }

    #[test]
    fn raw_passes_everything() {
        let cfg = SanitizerConfig::Raw;
        let nasty = "a\x00\u{202e}\u{feff}\nb  \u{fb01}";
        assert_eq!(cfg.sanitize(nasty, &ins_single()), nasty);
        assert_eq!(cfg.sanitize(nasty, &commit_single()), nasty);
    }

    #[test]
    fn custom_rule_replaces_profile() {
        struct Upper;
        impl Sanitize for Upper {
            fn name(&self) -> &'static str {
                "upper"
            }
            fn sanitize(&self, input: &str, _ctx: &SanitizeContext) -> String {
                input.to_uppercase()
            }
        }
        let cfg = SanitizerConfig::custom(std::sync::Arc::new(Upper));
        assert_eq!(cfg.sanitize("ab", &ins_single()), "AB");
        let dbg = format!("{cfg:?}");
        assert!(dbg.contains("upper"));
    }

    #[test]
    fn config_variants_dispatch() {
        let ctx = ins_single();
        assert_eq!(
            SanitizerConfig::Aggressive.sanitize("a\u{202e}b", &ctx),
            "ab"
        );
        assert_eq!(
            SanitizerConfig::Baseline.sanitize("a\u{202e}b", &ctx),
            "a\u{202e}b"
        );
        assert!(SanitizerConfig::default().sanitize("a\u{202e}b", &ctx) == "ab");
        assert!(SanitizerConfig::Raw.is_raw());
    }

    #[test]
    fn private_use_and_vs_kept_by_default() {
        let p = Profile::aggressive();
        assert_eq!(p.sanitize("a\u{e000}b", &ins_single()), "a\u{e000}b");
        assert_eq!(
            p.sanitize("\u{2764}\u{fe0f}", &ins_single()),
            "\u{2764}\u{fe0f}"
        );
        let strict = Profile::builder()
            .strip_private_use(true)
            .strip_variation_selectors(true)
            .build();
        assert_eq!(strict.sanitize("a\u{e000}b", &ins_single()), "ab");
        assert_eq!(
            strict.sanitize("\u{2764}\u{fe0f}", &ins_single()),
            "\u{2764}"
        );
    }

    #[test]
    fn empty_and_idempotent() {
        let p = Profile::aggressive();
        assert_eq!(p.sanitize("", &ins_single()), "");
        // Sanitizing twice is a fixed point.
        let once = p.sanitize("a\u{202e}\u{ff21} ", &ins_single());
        assert_eq!(p.sanitize(&once, &ins_single()), once);
    }
}
