//! Script directionality resolution for BiDi layout mirroring.
//!
//! Martensite resolves the horizontal writing direction of a locale from its
//! BCP47 language identifier so that the layout engine can mirror the widget
//! tree for right-to-left scripts without rebuilding structure. Resolution
//! follows the Unicode script direction tables: an explicit script subtag wins
//! over the language code, and a curated set of RTL language codes covers
//! locales that omit the script subtag (e.g. `ar`, `he`, `fa`).
//!
//! The resolution is pure, allocation-free, and operates entirely on
//! `&str` subtags, keeping it suitable for hot locale-switch paths.

use crate::LanguageIdentifier;

/// Horizontal writing direction of a script.
///
/// Martensite models only the two horizontal directions used by the layout
/// mirroring pass; vertical scripts (e.g. Mongolian) are out of scope for this
/// milestone and default to [`ScriptDirection::Ltr`].
///
/// # Examples
///
/// ```
/// use martensite_l10n::direction::{ScriptDirection, direction_for_locale};
/// use martensite_l10n::LanguageIdentifier;
///
/// let en: LanguageIdentifier = "en".parse().unwrap();
/// assert_eq!(direction_for_locale(&en), ScriptDirection::Ltr);
///
/// let ar: LanguageIdentifier = "ar".parse().unwrap();
/// assert_eq!(direction_for_locale(&ar), ScriptDirection::Rtl);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScriptDirection {
    /// Left-to-right script direction (e.g. Latin, Cyrillic, Han).
    Ltr,
    /// Right-to-left script direction (e.g. Arabic, Hebrew, N'Ko).
    Rtl,
}

impl ScriptDirection {
    /// Returns `true` if this direction is right-to-left.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::direction::ScriptDirection;
    ///
    /// assert!(ScriptDirection::Rtl.is_rtl());
    /// assert!(!ScriptDirection::Ltr.is_rtl());
    /// ```
    #[inline]
    pub const fn is_rtl(self) -> bool {
        matches!(self, Self::Rtl)
    }

    /// Returns `true` if this direction is left-to-right.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_l10n::direction::ScriptDirection;
    ///
    /// assert!(ScriptDirection::Ltr.is_ltr());
    /// assert!(!ScriptDirection::Rtl.is_ltr());
    /// ```
    #[inline]
    pub const fn is_ltr(self) -> bool {
        matches!(self, Self::Ltr)
    }
}

impl Default for ScriptDirection {
    /// Defaults to left-to-right, the dominant direction for the default
    /// (`und`) locale.
    fn default() -> Self {
        Self::Ltr
    }
}

impl std::fmt::Display for ScriptDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ltr => f.write_str("ltr"),
            Self::Rtl => f.write_str("rtl"),
        }
    }
}

/// Scripts whose default horizontal direction is right-to-left.
///
/// The list follows the Unicode script direction tables and includes all
/// RTL scripts enumerated by the v0.7.0 milestone specification. Persian,
/// Urdu, Sindhi and Uyghur are covered by the `Arab` entry since they all
/// use the Arabic script.
const RTL_SCRIPTS: &[&str] = &[
    "Arab", "Hebr", "Thaa", "Syrc", "Nkoo", "Armi", "Avst", "Cprt", "Khar", "Sarb", "Phnx", "Lydi",
    "Mand", "Mani", "Mend", "Orkh", "Sogd", "Hatr", "Nbat", "Palm", "Phlp",
];

/// Language subtags whose default direction is right-to-left when no explicit
/// script subtag is present.
///
/// This covers locales such as `ar`, `he`, `fa`, `ur`, `dv`, `yi` and the
/// other codes enumerated by the v0.7.0 milestone specification. Ambiguous
/// macrolanguages that are written RTL only in specific scripts (e.g.
/// Azerbaijani in `az-Arab`) are disambiguated by the explicit script subtag
/// path in [`direction_for_locale`].
const RTL_LANGUAGES: &[&str] = &[
    "ar", "arc", "azb", "bcc", "bqi", "ckb", "dv", "fa", "glk", "he", "ku", "mzn", "nqo", "pnb",
    "ps", "sd", "sdh", "ug", "ur", "yi",
];

/// Resolves the script direction from a 4-letter Unicode script code.
///
/// The script code is matched case-sensitively against the Unicode title-cased
/// form (e.g. `Arab`, `Hebr`, `Latn`). Unknown scripts default to
/// left-to-right.
///
/// # Examples
///
/// ```
/// use martensite_l10n::direction::{ScriptDirection, direction_for_script};
///
/// assert_eq!(direction_for_script("Arab"), ScriptDirection::Rtl);
/// assert_eq!(direction_for_script("Hebr"), ScriptDirection::Rtl);
/// assert_eq!(direction_for_script("Latn"), ScriptDirection::Ltr);
/// assert_eq!(direction_for_script("Unknown"), ScriptDirection::Ltr);
/// ```
#[inline]
pub fn direction_for_script(script: &str) -> ScriptDirection {
    if RTL_SCRIPTS.contains(&script) {
        ScriptDirection::Rtl
    } else {
        ScriptDirection::Ltr
    }
}

/// Resolves the script direction for a locale's [`LanguageIdentifier`].
///
/// Resolution order:
///
/// 1. If the identifier carries an explicit script subtag, the direction is
///    resolved from the script via [`direction_for_script`].
/// 2. Otherwise the primary language subtag is matched against the curated RTL
///    language list.
/// 3. Anything else defaults to left-to-right.
///
/// # Examples
///
/// ```
/// use martensite_l10n::direction::{ScriptDirection, direction_for_locale};
/// use martensite_l10n::LanguageIdentifier;
///
/// let ar: LanguageIdentifier = "ar".parse().unwrap();
/// assert_eq!(direction_for_locale(&ar), ScriptDirection::Rtl);
///
/// let az_arab: LanguageIdentifier = "az-Arab".parse().unwrap();
/// assert_eq!(direction_for_locale(&az_arab), ScriptDirection::Rtl);
///
/// let az_latn: LanguageIdentifier = "az-Latn".parse().unwrap();
/// assert_eq!(direction_for_locale(&az_latn), ScriptDirection::Ltr);
///
/// let en_us: LanguageIdentifier = "en-US".parse().unwrap();
/// assert_eq!(direction_for_locale(&en_us), ScriptDirection::Ltr);
/// ```
pub fn direction_for_locale(locale: &LanguageIdentifier) -> ScriptDirection {
    if let Some(script) = locale.script.as_ref() {
        return direction_for_script(script.as_str());
    }
    let language = locale.language.as_str();
    if RTL_LANGUAGES.contains(&language) {
        ScriptDirection::Rtl
    } else {
        ScriptDirection::Ltr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn ltr_scripts_resolve_ltr() {
        assert_eq!(direction_for_script("Latn"), ScriptDirection::Ltr);
        assert_eq!(direction_for_script("Cyrl"), ScriptDirection::Ltr);
        assert_eq!(direction_for_script("Hans"), ScriptDirection::Ltr);
        assert_eq!(direction_for_script("Hant"), ScriptDirection::Ltr);
        assert_eq!(direction_for_script("Hang"), ScriptDirection::Ltr);
        assert_eq!(direction_for_script("Deva"), ScriptDirection::Ltr);
        assert_eq!(direction_for_script("Grek"), ScriptDirection::Ltr);
    }

    #[test]
    fn all_rtl_scripts_resolve_rtl() {
        for script in RTL_SCRIPTS {
            assert_eq!(
                direction_for_script(script),
                ScriptDirection::Rtl,
                "script {script} should be RTL"
            );
        }
    }

    #[test]
    fn unknown_script_defaults_ltr() {
        assert_eq!(direction_for_script("Zzzz"), ScriptDirection::Ltr);
        assert_eq!(direction_for_script(""), ScriptDirection::Ltr);
    }

    #[test]
    fn rtl_languages_without_script_resolve_rtl() {
        for code in RTL_LANGUAGES {
            let li = LanguageIdentifier::from_str(code).unwrap_or_else(|_| {
                panic!("language code {code} should parse as a LanguageIdentifier")
            });
            assert_eq!(
                direction_for_locale(&li),
                ScriptDirection::Rtl,
                "language {code} should be RTL"
            );
        }
    }

    #[test]
    fn rtl_languages_with_region_resolve_rtl() {
        let ar = LanguageIdentifier::from_str("ar-EG").unwrap();
        assert_eq!(direction_for_locale(&ar), ScriptDirection::Rtl);

        let he = LanguageIdentifier::from_str("he-IL").unwrap();
        assert_eq!(direction_for_locale(&he), ScriptDirection::Rtl);

        let fa = LanguageIdentifier::from_str("fa-IR").unwrap();
        assert_eq!(direction_for_locale(&fa), ScriptDirection::Rtl);

        let ur = LanguageIdentifier::from_str("ur-PK").unwrap();
        assert_eq!(direction_for_locale(&ur), ScriptDirection::Rtl);
    }

    #[test]
    fn explicit_script_overrides_language() {
        let az_arab = LanguageIdentifier::from_str("az-Arab").unwrap();
        assert_eq!(direction_for_locale(&az_arab), ScriptDirection::Rtl);

        let az_latn = LanguageIdentifier::from_str("az-Latn").unwrap();
        assert_eq!(direction_for_locale(&az_latn), ScriptDirection::Ltr);

        let ku_arab = LanguageIdentifier::from_str("ku-Arab").unwrap();
        assert_eq!(direction_for_locale(&ku_arab), ScriptDirection::Rtl);

        let en_arab = LanguageIdentifier::from_str("en-Arab").unwrap();
        assert_eq!(direction_for_locale(&en_arab), ScriptDirection::Rtl);
    }

    #[test]
    fn common_ltr_locales_resolve_ltr() {
        let en = LanguageIdentifier::from_str("en").unwrap();
        assert_eq!(direction_for_locale(&en), ScriptDirection::Ltr);

        let en_us = LanguageIdentifier::from_str("en-US").unwrap();
        assert_eq!(direction_for_locale(&en_us), ScriptDirection::Ltr);

        let zh = LanguageIdentifier::from_str("zh-Hans").unwrap();
        assert_eq!(direction_for_locale(&zh), ScriptDirection::Ltr);

        let ja = LanguageIdentifier::from_str("ja-JP").unwrap();
        assert_eq!(direction_for_locale(&ja), ScriptDirection::Ltr);

        let ru = LanguageIdentifier::from_str("ru").unwrap();
        assert_eq!(direction_for_locale(&ru), ScriptDirection::Ltr);
    }

    #[test]
    fn und_locale_defaults_ltr() {
        let und = LanguageIdentifier::default();
        assert_eq!(direction_for_locale(&und), ScriptDirection::Ltr);
    }

    #[test]
    fn is_rtl_and_is_ltr_are_complementary() {
        assert!(ScriptDirection::Rtl.is_rtl());
        assert!(!ScriptDirection::Rtl.is_ltr());
        assert!(ScriptDirection::Ltr.is_ltr());
        assert!(!ScriptDirection::Ltr.is_rtl());
    }

    #[test]
    fn default_is_ltr() {
        assert_eq!(ScriptDirection::default(), ScriptDirection::Ltr);
    }

    #[test]
    fn display_renders_lowercase_token() {
        assert_eq!(ScriptDirection::Ltr.to_string(), "ltr");
        assert_eq!(ScriptDirection::Rtl.to_string(), "rtl");
    }

    #[test]
    fn equality_and_copy() {
        let a = ScriptDirection::Rtl;
        let b = a;
        assert_eq!(a, b);
        assert_ne!(a, ScriptDirection::Ltr);
    }
}
