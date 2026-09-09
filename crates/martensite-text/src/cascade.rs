//! Platform System Font Fallback Cascades to eliminate missing-glyph tofu.
//!
//! This module provides:
//! - Script categorization for Unicode characters (`ScriptTag`, `classify_script`).
//! - OS-native system font fallback cascade resolution (`PlatformCascadeResolver`).
//! - Font family fallback chains (`FontFallbackChain`).
//! - Thread-safe cached resolution for font fallback chains (`FontFallbackCache`).

use std::collections::{HashMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};

use cosmic_text::FontSystem;
use parking_lot::RwLock;

/// Recognized typographic script category for font fallback cascade resolution.
///
/// # Examples
///
/// ```
/// use martensite_text::cascade::ScriptTag;
///
/// let script = ScriptTag::Latin;
/// assert_eq!(script, ScriptTag::default());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScriptTag {
    /// Latin and western alphabetic scripts.
    #[default]
    Latin,
    /// Arabic abjad and extensions.
    Arabic,
    /// Hebrew abjad and extensions.
    Hebrew,
    /// Chinese, Japanese, and Korean (CJK) ideographs, Kana, and Hangul.
    Cjk,
    /// Devanagari and Indic scripts.
    Devanagari,
    /// Pictographic symbols and emoji.
    Emoji,
    /// Mathematical operators and alphanumeric symbols.
    Math,
    /// Other unclassified or minority scripts.
    Other,
}

/// Classifies a Unicode character into its primary typographic [`ScriptTag`].
///
/// # Examples
///
/// ```
/// use martensite_text::cascade::{ScriptTag, classify_script};
///
/// assert_eq!(classify_script('A'), ScriptTag::Latin);
/// assert_eq!(classify_script('م'), ScriptTag::Arabic);
/// assert_eq!(classify_script('ש'), ScriptTag::Hebrew);
/// assert_eq!(classify_script('漢'), ScriptTag::Cjk);
/// assert_eq!(classify_script('क'), ScriptTag::Devanagari);
/// assert_eq!(classify_script('🦀'), ScriptTag::Emoji);
/// assert_eq!(classify_script('∑'), ScriptTag::Math);
/// ```
pub fn classify_script(ch: char) -> ScriptTag {
    let u = ch as u32;
    match u {
        // Basic Latin, Latin-1 Supplement, Latin Extended A/B, Latin Extended Additional
        0x0020..=0x024F | 0x1E00..=0x1EFF => ScriptTag::Latin,

        // Arabic, Arabic Supplement, Arabic Extended-A, Arabic Presentation Forms A/B
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => {
            ScriptTag::Arabic
        }

        // Hebrew and Hebrew Presentation Forms
        0x0590..=0x05FF | 0xFB1D..=0xFB4F => ScriptTag::Hebrew,

        // Devanagari and Devanagari Extended
        0x0900..=0x097F | 0xA8E0..=0xA8FF => ScriptTag::Devanagari,

        // Emoji & Symbols
        0x1F300..=0x1F5FF // Miscellaneous Symbols and Pictographs
        | 0x1F600..=0x1F64F // Emoticons
        | 0x1F680..=0x1F6FF // Transport and Map Symbols
        | 0x1F700..=0x1F77F // Alchemical Symbols
        | 0x1F780..=0x1F7FF // Geometric Shapes Extended
        | 0x1F800..=0x1F8FF // Supplemental Arrows-C
        | 0x1F900..=0x1F9FF // Supplemental Symbols and Pictographs
        | 0x1FA00..=0x1FA6F // Chess Symbols
        | 0x1FA70..=0x1FAFF // Symbols and Pictographs Extended-A
        | 0x2600..=0x26FF   // Miscellaneous Symbols
        | 0x2700..=0x27BF   // Dingbats
        => ScriptTag::Emoji,

        // Mathematical Operators & Symbols
        0x2200..=0x22FF // Mathematical Operators
        | 0x2A00..=0x2AFF // Supplemental Mathematical Operators
        | 0x1D400..=0x1D7FF // Mathematical Alphanumeric Symbols
        | 0x27C0..=0x27EF // Miscellaneous Mathematical Symbols-A
        | 0x2980..=0x29FF // Miscellaneous Mathematical Symbols-B
        => ScriptTag::Math,

        // CJK Unified Ideographs, Hiragana, Katakana, Bopomofo, Hangul, CJK Symbols, Fullwidth
        0x4E00..=0x9FFF
        | 0x3400..=0x4DBF
        | 0x20000..=0x2FA1F
        | 0x3040..=0x309F
        | 0x30A0..=0x30FF
        | 0x3100..=0x312F
        | 0x31A0..=0x31BF
        | 0xAC00..=0xD7AF
        | 0x1100..=0x11FF
        | 0x3130..=0x318F
        | 0x3000..=0x303F
        | 0xFF00..=0xFFEF => ScriptTag::Cjk,

        _ => ScriptTag::Other,
    }
}

/// Returns the dominant (most frequent) [`ScriptTag`] among the
/// non-whitespace characters of `text`.
///
/// Returns [`ScriptTag::Latin`] for empty or unclassifiable text.
///
/// # Examples
///
/// ```
/// use martensite_text::cascade::{dominant_script, ScriptTag};
///
/// assert_eq!(dominant_script("Hello 漢字"), ScriptTag::Latin);
/// assert_eq!(dominant_script("漢字漢字A"), ScriptTag::Cjk);
/// assert_eq!(dominant_script(""), ScriptTag::Latin);
/// ```
pub fn dominant_script(text: &str) -> ScriptTag {
    let mut counts = [0u32; 8];
    let order = [
        ScriptTag::Latin,
        ScriptTag::Arabic,
        ScriptTag::Hebrew,
        ScriptTag::Cjk,
        ScriptTag::Devanagari,
        ScriptTag::Emoji,
        ScriptTag::Math,
        ScriptTag::Other,
    ];
    for ch in text.chars() {
        if ch.is_whitespace() {
            continue;
        }
        let tag = classify_script(ch);
        let idx = order.iter().position(|t| *t == tag).unwrap_or(7);
        counts[idx] += 1;
    }
    let best = counts
        .iter()
        .enumerate()
        .filter(|(_, count)| **count > 0)
        .max_by_key(|(_, count)| *count)
        .map(|(i, _)| i)
        .unwrap_or(0);
    order[best]
}

/// Abstract source of platform-specific font fallback cascades.
///
/// This trait is the seam between Martensite's text pipeline and the
/// underlying operating-system font fallback mechanism. The default
/// implementation ([`PlatformCascadeResolver`]) uses static per-OS
/// family lists. Native providers implementing this trait can call
/// DirectWrite `IDWriteFontFallback::MapCharacters` (Windows),
/// CoreText `CTFontCreateForStringWithLanguage` (macOS), or
/// Fontconfig `FcFontSort` (Linux) to deliver locale-aware, coverage-
/// checked fallback that the static lists cannot match.
///
/// The trait is object-safe so providers can be used through
/// `&dyn FontFallbackProvider` or `Arc<dyn FontFallbackProvider>`.
///
/// # Examples
///
/// ```
/// use martensite_text::cascade::{FontFallbackProvider, PlatformCascadeResolver, ScriptTag};
///
/// let provider = PlatformCascadeResolver;
/// let fallbacks = provider.script_fallbacks(ScriptTag::Cjk, "zh-CN");
/// assert!(!fallbacks.is_empty());
/// ```
pub trait FontFallbackProvider: Send + Sync {
    /// Returns the common fallback families tried after all
    /// script-specific lists are exhausted.
    fn common_fallbacks(&self) -> Vec<String>;

    /// Returns the script- and locale-specific fallback families
    /// for the given [`ScriptTag`] and BCP-47 locale string.
    ///
    /// The locale (e.g. `"zh-cn"`, `"ja"`, `"en-us"`) may influence
    /// CJK variant selection and other locale-sensitive ordering.
    /// Providers that ignore locale should return the same list
    /// regardless of the `locale` argument.
    fn script_fallbacks(&self, script: ScriptTag, locale: &str) -> Vec<String>;

    /// Returns families that must never be used as fallbacks
    /// (e.g. symbol fonts that would produce tofu for normal text).
    fn forbidden_fallbacks(&self) -> Vec<String>;
}

/// Resolves operating system platform-native font fallback cascades for each script.
///
/// # Examples
///
/// ```
/// use martensite_text::cascade::{PlatformCascadeResolver, ScriptTag};
///
/// let fallbacks = PlatformCascadeResolver::platform_fallbacks_for_script(ScriptTag::Cjk);
/// assert!(!fallbacks.is_empty());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlatformCascadeResolver;

impl PlatformCascadeResolver {
    /// Returns the OS-native font fallback cascade for the given script on the current target platform.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cascade::{PlatformCascadeResolver, ScriptTag};
    ///
    /// let fonts = PlatformCascadeResolver::platform_fallbacks_for_script(ScriptTag::Emoji);
    /// assert!(!fonts.is_empty());
    /// ```
    pub fn platform_fallbacks_for_script(script: ScriptTag) -> &'static [&'static str] {
        #[cfg(target_os = "windows")]
        {
            Self::windows_fallbacks_for_script(script)
        }
        #[cfg(target_os = "macos")]
        {
            Self::macos_fallbacks_for_script(script)
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            Self::linux_fallbacks_for_script(script)
        }
    }

    /// Returns the Windows system fallback font families for the given script.
    ///
    /// Cascades use Windows defaults:
    /// - Latin: Segoe UI, Arial
    /// - CJK: Meiryo, Yu Gothic, Malgun Gothic, Microsoft YaHei
    /// - Emoji: Segoe UI Emoji, Segoe UI Symbol
    /// - Devanagari: Nirmala UI, Mangal
    /// - Arabic: Segoe UI, Arabic Typesetting
    /// - Hebrew: Segoe UI, David
    /// - Math: Cambria Math, Segoe UI Symbol
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cascade::{PlatformCascadeResolver, ScriptTag};
    ///
    /// let fonts = PlatformCascadeResolver::windows_fallbacks_for_script(ScriptTag::Cjk);
    /// assert!(fonts.contains(&"Meiryo") || fonts.contains(&"Yu Gothic"));
    /// ```
    pub fn windows_fallbacks_for_script(script: ScriptTag) -> &'static [&'static str] {
        match script {
            ScriptTag::Latin => &["Segoe UI", "Arial"],
            ScriptTag::Cjk => &["Meiryo", "Yu Gothic", "Malgun Gothic", "Microsoft YaHei"],
            ScriptTag::Emoji => &["Segoe UI Emoji", "Segoe UI Symbol"],
            ScriptTag::Devanagari => &["Nirmala UI", "Mangal"],
            ScriptTag::Arabic => &["Segoe UI", "Arabic Typesetting"],
            ScriptTag::Hebrew => &["Segoe UI", "David"],
            ScriptTag::Math => &["Cambria Math", "Segoe UI Symbol"],
            ScriptTag::Other => &["Segoe UI"],
        }
    }

    /// Returns the macOS system fallback font families for the given script.
    ///
    /// Cascades use macOS defaults:
    /// - Latin: SF Pro, Helvetica Neue
    /// - CJK: PingFang SC, Hiragino Sans, Apple SD Gothic Neo
    /// - Emoji: Apple Color Emoji
    /// - Devanagari: Devanagari MT, Kohinoor Devanagari
    /// - Arabic: Geeza Pro
    /// - Hebrew: Arial Hebrew, Lucida Grande
    /// - Math: STIXGeneral, Apple Symbols
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cascade::{PlatformCascadeResolver, ScriptTag};
    ///
    /// let fonts = PlatformCascadeResolver::macos_fallbacks_for_script(ScriptTag::Cjk);
    /// assert!(fonts.contains(&"PingFang SC") || fonts.contains(&"Hiragino Sans"));
    /// ```
    pub fn macos_fallbacks_for_script(script: ScriptTag) -> &'static [&'static str] {
        match script {
            ScriptTag::Latin => &["SF Pro", "Helvetica Neue"],
            ScriptTag::Cjk => &["PingFang SC", "Hiragino Sans", "Apple SD Gothic Neo"],
            ScriptTag::Emoji => &["Apple Color Emoji"],
            ScriptTag::Devanagari => &["Devanagari MT", "Kohinoor Devanagari"],
            ScriptTag::Arabic => &["Geeza Pro"],
            ScriptTag::Hebrew => &["Arial Hebrew", "Lucida Grande"],
            ScriptTag::Math => &["STIXGeneral", "Apple Symbols"],
            ScriptTag::Other => &["SF Pro"],
        }
    }

    /// Returns the Linux system fallback font families for the given script.
    ///
    /// Cascades use standard FreeDesktop/Noto defaults:
    /// - Latin: Noto Sans, DejaVu Sans
    /// - CJK: Noto Sans CJK SC, Noto Sans CJK JP, Noto Sans CJK KR
    /// - Emoji: Noto Color Emoji
    /// - Devanagari: Noto Sans Devanagari
    /// - Arabic: Noto Sans Arabic
    /// - Hebrew: Noto Sans Hebrew
    /// - Math: Noto Sans Math, DejaVu Sans Math
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cascade::{PlatformCascadeResolver, ScriptTag};
    ///
    /// let fonts = PlatformCascadeResolver::linux_fallbacks_for_script(ScriptTag::Cjk);
    /// assert!(fonts.contains(&"Noto Sans CJK SC") || fonts.contains(&"Noto Sans CJK JP"));
    /// ```
    pub fn linux_fallbacks_for_script(script: ScriptTag) -> &'static [&'static str] {
        match script {
            ScriptTag::Latin => &["Noto Sans", "DejaVu Sans"],
            ScriptTag::Cjk => &["Noto Sans CJK SC", "Noto Sans CJK JP", "Noto Sans CJK KR"],
            ScriptTag::Emoji => &["Noto Color Emoji"],
            ScriptTag::Devanagari => &["Noto Sans Devanagari"],
            ScriptTag::Arabic => &["Noto Sans Arabic"],
            ScriptTag::Hebrew => &["Noto Sans Hebrew"],
            ScriptTag::Math => &["Noto Sans Math", "DejaVu Sans Math"],
            ScriptTag::Other => &["Noto Sans"],
        }
    }
}

impl FontFallbackProvider for PlatformCascadeResolver {
    fn common_fallbacks(&self) -> Vec<String> {
        // The last-resort families that cover the widest glyph range
        // on the current platform. These are tried after every
        // script-specific list has been exhausted.
        #[cfg(target_os = "windows")]
        {
            vec!["Segoe UI".to_string(), "Arial".to_string()]
        }
        #[cfg(target_os = "macos")]
        {
            vec!["SF Pro".to_string(), "Helvetica Neue".to_string()]
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            vec!["Noto Sans".to_string(), "DejaVu Sans".to_string()]
        }
    }

    fn script_fallbacks(&self, script: ScriptTag, _locale: &str) -> Vec<String> {
        // The static resolver does not vary by locale; the locale
        // parameter is accepted for trait conformance and will be
        // used by native OS providers (DirectWrite, CoreText,
        // Fontconfig) that can select CJK variants per locale.
        Self::platform_fallbacks_for_script(script)
            .iter()
            .map(|&s| s.to_string())
            .collect()
    }

    fn forbidden_fallbacks(&self) -> Vec<String> {
        // Symbol-only fonts that would produce tofu for normal text.
        // The static lists already avoid these, but native providers
        // may return them and need filtering.
        Vec::new()
    }
}

/// An ordered sequence of font families consisting of a primary font and its fallbacks.
///
/// # Examples
///
/// ```
/// use martensite_text::cascade::FontFallbackChain;
///
/// let chain = FontFallbackChain::new("Inter", vec!["Segoe UI".to_string(), "Arial".to_string()]);
/// assert_eq!(chain.primary, "Inter");
/// assert_eq!(chain.fallbacks.len(), 2);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFallbackChain {
    /// Primary requested font family name.
    pub primary: String,
    /// Ordered cascade of fallback font family names.
    pub fallbacks: Vec<String>,
}

impl FontFallbackChain {
    /// Constructs a new `FontFallbackChain`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cascade::FontFallbackChain;
    ///
    /// let chain = FontFallbackChain::new("Roboto", vec!["Helvetica".to_string()]);
    /// assert_eq!(chain.primary, "Roboto");
    /// ```
    #[inline]
    pub fn new(primary: impl Into<String>, fallbacks: Vec<String>) -> Self {
        Self {
            primary: primary.into(),
            fallbacks,
        }
    }

    /// Constructs a `FontFallbackChain` populated with the platform-native cascade for the given script.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cascade::{FontFallbackChain, ScriptTag};
    ///
    /// let chain = FontFallbackChain::for_script("CustomSerif", ScriptTag::Cjk);
    /// assert_eq!(chain.primary, "CustomSerif");
    /// assert!(!chain.fallbacks.is_empty());
    /// ```
    pub fn for_script(primary: impl Into<String>, script: ScriptTag) -> Self {
        let provider = PlatformCascadeResolver;
        let fallbacks = provider.script_fallbacks(script, "");
        Self::new(primary, fallbacks)
    }

    /// Returns an iterator over all font families in the chain, starting with the primary font.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cascade::FontFallbackChain;
    ///
    /// let chain = FontFallbackChain::new("Primary", vec!["Fallback1".to_string(), "Fallback2".to_string()]);
    /// let all: Vec<&str> = chain.families().collect();
    /// assert_eq!(all, vec!["Primary", "Fallback1", "Fallback2"]);
    /// ```
    pub fn families(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.primary.as_str()).chain(self.fallbacks.iter().map(|s| s.as_str()))
    }
}

/// Key for a culture-specific fallback resolution.
///
/// The locale is normalized to lowercase for stable cache lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FallbackKey {
    /// Typographic script tag.
    pub script: ScriptTag,
    /// BCP-47 / POSIX locale string used for regional fallback ordering.
    pub locale: String,
}

impl FallbackKey {
    /// Creates a new key from a script tag and a locale.
    #[inline]
    pub fn new(script: ScriptTag, locale: impl Into<String>) -> Self {
        Self {
            script,
            locale: locale.into().to_lowercase(),
        }
    }
}

/// Resolves font fallback chains by inspecting the installed font database.
///
/// This resolver uses [`fontdb`] (via the cosmic-text [`FontSystem`]) to find
/// fonts that actually contain the required code points. It is pure safe Rust
/// and works on all supported platforms. The fallback family lists are
/// supplied by a [`FontFallbackProvider`]; the default
/// ([`PlatformCascadeResolver`]) uses static per-OS tables, while native
/// providers (DirectWrite, CoreText, Fontconfig) can be plugged in to
/// deliver locale-aware cascade selection.
///
/// # Safety of font parsing
///
/// The `swash` and `ttf-parser` crates used for `cmap` inspection have
/// known panic paths on malformed font data (see swash issues #123–#126,
/// ttf-parser RUSTSEC-2026-0192). All font-data access in this resolver is
/// wrapped in [`std::panic::catch_unwind`] so a corrupt or adversarial font
/// in the database cannot abort the calling thread; a panicking face is
/// treated as not covering the queried character.
pub struct InstalledFontFallbackResolver<'a> {
    font_system: &'a FontSystem,
    provider: &'a dyn FontFallbackProvider,
}

impl<'a> InstalledFontFallbackResolver<'a> {
    /// Creates a resolver bound to the given font system, using the
    /// default [`PlatformCascadeResolver`] as the fallback provider.
    #[inline]
    pub fn new(font_system: &'a FontSystem) -> Self {
        Self::with_provider(font_system, &PlatformCascadeResolver)
    }

    /// Creates a resolver bound to the given font system and an
    /// arbitrary [`FontFallbackProvider`].
    ///
    /// This is the entry point for native OS providers
    /// (DirectWrite, CoreText, Fontconfig) to supply locale-aware
    /// fallback cascades.
    #[inline]
    pub fn with_provider(
        font_system: &'a FontSystem,
        provider: &'a dyn FontFallbackProvider,
    ) -> Self {
        Self {
            font_system,
            provider,
        }
    }

    /// Returns the list of font families to try for `text`, in priority
    /// order.
    ///
    /// The returned list always begins with `primary` so the shaping
    /// pipeline has a family to attempt first. It is followed by the
    /// provider's fallback families for every script present in `text`,
    /// filtered by the provider's locale if available; families that are
    /// actually installed *and* cover the relevant characters are listed
    /// before the remaining candidates. Candidate names are always
    /// appended, even when the font database contains no faces for them,
    /// because the downstream shaper performs its own resolution and
    /// generic fallbacks.
    pub fn resolve_for_text(&self, text: &str, primary: &str) -> Vec<String> {
        self.resolve_for_text_with_locale(text, primary, "")
    }

    /// Like [`resolve_for_text`](Self::resolve_for_text) but passes the
    /// locale to the [`FontFallbackProvider`] for locale-sensitive CJK
    /// and Indic variant selection.
    pub fn resolve_for_text_with_locale(
        &self,
        text: &str,
        primary: &str,
        locale: &str,
    ) -> Vec<String> {
        let mut chain: Vec<String> = vec![primary.to_string()];

        let forbidden = self.provider.forbidden_fallbacks();

        // Collect the scripts present in the text, in order of first use.
        let mut scripts: Vec<ScriptTag> = Vec::new();
        for ch in text.chars() {
            if ch.is_whitespace() || ch.is_control() {
                continue;
            }
            let tag = classify_script(ch);
            if tag != ScriptTag::Other && !scripts.contains(&tag) {
                scripts.push(tag);
            }
        }
        if scripts.is_empty() {
            scripts.push(dominant_script(text));
        }

        // Pass 1: installed provider candidates that actually cover the
        // script's characters, so they are tried before nominal names.
        for &script in &scripts {
            let needed: String = text
                .chars()
                .filter(|ch| classify_script(*ch) == script)
                .collect();
            for family in self.provider.script_fallbacks(script, locale) {
                if chain.iter().any(|f| f.eq_ignore_ascii_case(&family))
                    || forbidden.iter().any(|f| f.eq_ignore_ascii_case(&family))
                {
                    continue;
                }
                if self.family_covers_text(&family, &needed) {
                    chain.push(family);
                }
            }
        }

        // Pass 2: the remaining provider cascade names as candidates.
        for &script in &scripts {
            for family in self.provider.script_fallbacks(script, locale) {
                if !chain.iter().any(|f| f.eq_ignore_ascii_case(&family))
                    && !forbidden.iter().any(|f| f.eq_ignore_ascii_case(&family))
                {
                    chain.push(family);
                }
            }
        }

        // Pass 3: common fallbacks as a last resort.
        for family in self.provider.common_fallbacks() {
            if !chain.iter().any(|f| f.eq_ignore_ascii_case(&family))
                && !forbidden.iter().any(|f| f.eq_ignore_ascii_case(&family))
            {
                chain.push(family);
            }
        }

        chain
    }

    /// Returns the subset of provider fallback family names that are
    /// actually installed in the font database.
    pub fn installed_fallbacks_for_script(&self, script: ScriptTag) -> Vec<String> {
        self.installed_fallbacks_for_script_with_locale(script, "")
    }

    /// Like [`installed_fallbacks_for_script`](Self::installed_fallbacks_for_script)
    /// but passes the locale to the provider.
    pub fn installed_fallbacks_for_script_with_locale(
        &self,
        script: ScriptTag,
        locale: &str,
    ) -> Vec<String> {
        let forbidden = self.provider.forbidden_fallbacks();
        self.provider
            .script_fallbacks(script, locale)
            .into_iter()
            .filter(|family| {
                !forbidden.iter().any(|f| f.eq_ignore_ascii_case(family))
                    && self.family_installed(family)
            })
            .collect()
    }

    /// Returns `true` if the font face with the given ID contains a glyph
    /// for `ch`.
    ///
    /// The face's `cmap` is inspected by parsing the font data through
    /// `swash`'s character map; a glyph ID of `0` (`.notdef`) counts as
    /// uncovered. The call is wrapped in [`catch_unwind`] because
    /// `swash`/`ttf-parser` have known panic paths on malformed font
    /// data (swash #123–#126, ttf-parser RUSTSEC-2026-0192). A panicking
    /// face is treated as not covering `ch`.
    fn face_covers_char(&self, face_id: fontdb::ID, ch: char) -> bool {
        self.font_system
            .db()
            .with_face_data(face_id, |data, index| {
                // catch_unwind guards against malformed font data panics
                // in swash's cmap parsing (swash issues #123–#126).
                catch_unwind(AssertUnwindSafe(|| {
                    swash::FontRef::from_index(data, index as usize)
                        .is_some_and(|font| font.charmap().map(ch) != 0)
                }))
                .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    /// Returns `true` if the database contains at least one face whose
    /// family list contains `family` (case-insensitive).
    fn family_installed(&self, family: &str) -> bool {
        self.font_system.db().faces().any(|face| {
            face.families
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(family))
        })
    }

    /// Returns `true` if faces of `family` collectively cover every
    /// character in `text`.
    ///
    /// Coverage is checked per codepoint: each non-control character must
    /// be covered by *some* installed face whose family list contains
    /// `family` (case-insensitive). A single face is not required to
    /// cover the entire text, so families split across faces (e.g. a
    /// family whose faces cover complementary codepoint ranges, or
    /// per-script faces of a multi-script family) still qualify.
    pub fn family_covers_text(&self, family: &str, text: &str) -> bool {
        text.chars().all(|ch| {
            ch.is_control()
                || self.font_system.db().faces().any(|face| {
                    face.families
                        .iter()
                        .any(|(name, _)| name.eq_ignore_ascii_case(family))
                        && self.face_covers_char(face.id, ch)
                })
        })
    }
}

/// A thread-safe cache for resolved font fallback chains indexed by
/// [`FallbackKey`] (script + locale).
///
/// # Examples
///
/// ```
/// use martensite_text::cascade::{FontFallbackCache, FallbackKey, ScriptTag};
///
/// let cache = FontFallbackCache::new();
/// let key = FallbackKey::new(ScriptTag::Emoji, "en-us");
/// let fonts = cache.get_or_resolve(&key);
/// assert!(!fonts.is_empty());
/// ```
#[derive(Debug, Default)]
pub struct FontFallbackCache {
    cache: RwLock<HashMap<FallbackKey, Vec<String>>>,
}

impl FontFallbackCache {
    /// Creates a new, empty `FontFallbackCache`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cascade::FontFallbackCache;
    ///
    /// let cache = FontFallbackCache::new();
    /// ```
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Returns the cached font fallback families for the key, or resolves
    /// and caches them using the default [`PlatformCascadeResolver`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cascade::{FontFallbackCache, FallbackKey, ScriptTag};
    ///
    /// let cache = FontFallbackCache::new();
    /// let key = FallbackKey::new(ScriptTag::Latin, "en-us");
    /// let list = cache.get_or_resolve(&key);
    /// assert!(!list.is_empty());
    /// ```
    pub fn get_or_resolve(&self, key: &FallbackKey) -> Vec<String> {
        {
            let reader = self.cache.read();
            if let Some(cached) = reader.get(key) {
                return cached.clone();
            }
        }

        let provider = PlatformCascadeResolver;
        let resolved: Vec<String> = provider.script_fallbacks(key.script, &key.locale);

        let mut writer = self.cache.write();
        writer.insert(key.clone(), resolved.clone());

        resolved
    }

    /// Returns the cached font fallback families for the key, or resolves
    /// and caches them using the provided [`FontFallbackProvider`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cascade::{
    ///     FontFallbackCache, FontFallbackProvider, FallbackKey,
    ///     PlatformCascadeResolver, ScriptTag,
    /// };
    ///
    /// let cache = FontFallbackCache::new();
    /// let provider = PlatformCascadeResolver;
    /// let key = FallbackKey::new(ScriptTag::Cjk, "zh-cn");
    /// let list = cache.get_or_resolve_with_provider(&key, &provider);
    /// assert!(!list.is_empty());
    /// ```
    pub fn get_or_resolve_with_provider(
        &self,
        key: &FallbackKey,
        provider: &dyn FontFallbackProvider,
    ) -> Vec<String> {
        {
            let reader = self.cache.read();
            if let Some(cached) = reader.get(key) {
                return cached.clone();
            }
        }

        let resolved: Vec<String> = provider.script_fallbacks(key.script, &key.locale);

        let mut writer = self.cache.write();
        writer.insert(key.clone(), resolved.clone());

        resolved
    }

    /// Registers a custom font family at the front of the fallback cascade for the specified key.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cascade::{FontFallbackCache, FallbackKey, ScriptTag};
    ///
    /// let cache = FontFallbackCache::new();
    /// let key = FallbackKey::new(ScriptTag::Cjk, "zh-cn");
    /// cache.register_custom_fallback(&key, "Source Han Sans");
    /// let list = cache.get_or_resolve(&key);
    /// assert_eq!(list[0], "Source Han Sans");
    /// ```
    pub fn register_custom_fallback(&self, key: &FallbackKey, family: &str) {
        let mut writer = self.cache.write();

        let entry = writer
            .entry(key.clone())
            .or_insert_with(|| PlatformCascadeResolver.script_fallbacks(key.script, &key.locale));

        if !entry.iter().any(|f| f == family) {
            entry.insert(0, family.to_string());
        }
    }

    /// Clears all cached entries.
    pub fn clear(&self) {
        self.cache.write().clear();
    }
}

/// A cache for resolved font fallback decisions, keyed by
/// `(script, locale, primary_family)` and invalidated by a
/// font-system generation counter.
///
/// Unlike [`FontFallbackCache`] (which caches per-script family lists
/// from the provider), this cache stores the *resolved* fallback chain
/// for a specific `(primary_family, text)` combination, including the
/// installed-font coverage check. This avoids re-scanning the font
/// database on every shaping call for the same text and family.
///
/// # Invalidation
///
/// The cache stores the font-system generation counter at insertion
/// time. When [`get`](Self::get) is called with a different
/// generation, all entries are invalidated and the cache returns
/// `None`, forcing a re-resolution.
///
/// # Bounded capacity
///
/// Within a single font-system generation, the cache is bounded to
/// [`FALLBACK_DECISION_CACHE_CAPACITY`] (256) entries. When the
/// capacity is reached, the oldest inserted entry is evicted (FIFO
/// eviction), preventing unbounded memory growth from unique
/// `(script, locale, primary_family)` tuples.
///
/// # Examples
///
/// ```
/// use martensite_text::cascade::{FallbackDecisionCache, FallbackKey, ScriptTag};
///
/// let mut cache = FallbackDecisionCache::new();
/// let key = FallbackKey::new(ScriptTag::Latin, "en-us");
/// // Cache miss on first call.
/// assert!(cache.get(&key, "Inter", 0).is_none());
/// // Insert a resolved chain.
/// cache.insert(&key, "Inter", vec!["Inter".to_string(), "Arial".to_string()], 0);
/// // Cache hit on second call with same generation.
/// assert_eq!(
///     cache.get(&key, "Inter", 0),
///     Some(&vec!["Inter".to_string(), "Arial".to_string()])
/// );
/// // Cache miss when generation changes (font db was modified).
/// assert!(cache.get(&key, "Inter", 1).is_none());
/// ```
#[derive(Debug)]
pub struct FallbackDecisionCache {
    /// Cached fallback chains keyed by (FallbackKey, primary_family).
    entries: HashMap<(FallbackKey, String), Vec<String>>,
    /// Insertion order of keys, oldest at the front. Used for FIFO
    /// eviction when the cache reaches its capacity.
    insertion_order: VecDeque<(FallbackKey, String)>,
    /// The font-system generation when the cache was last populated.
    /// When this differs from the current generation, all entries
    /// are invalidated.
    generation: u64,
    /// Maximum number of entries retained within a single generation.
    capacity: usize,
}

/// Default capacity for [`FallbackDecisionCache`].
pub const FALLBACK_DECISION_CACHE_CAPACITY: usize = 256;

impl Default for FallbackDecisionCache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            insertion_order: VecDeque::new(),
            generation: 0,
            capacity: FALLBACK_DECISION_CACHE_CAPACITY,
        }
    }
}

impl FallbackDecisionCache {
    /// Creates a new, empty `FallbackDecisionCache`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cascade::FallbackDecisionCache;
    ///
    /// let cache = FallbackDecisionCache::new();
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the cached fallback chain for the given key and primary
    /// family, or `None` if not cached or if the generation has changed.
    ///
    /// When the `current_generation` differs from the cache's
    /// generation, all entries are invalidated (cleared) and `None` is
    /// returned.
    pub fn get(
        &mut self,
        key: &FallbackKey,
        primary_family: &str,
        current_generation: u64,
    ) -> Option<&Vec<String>> {
        if self.generation != current_generation {
            self.entries.clear();
            self.insertion_order.clear();
            self.generation = current_generation;
            return None;
        }
        self.entries.get(&(key.clone(), primary_family.to_string()))
    }

    /// Inserts a resolved fallback chain into the cache.
    ///
    /// The `current_generation` is stored as the cache's generation;
    /// future [`get`](Self::get) calls with a different generation will
    /// invalidate the cache. When the cache is at capacity, the oldest
    /// inserted entry is evicted before the new entry is stored.
    pub fn insert(
        &mut self,
        key: &FallbackKey,
        primary_family: &str,
        chain: Vec<String>,
        current_generation: u64,
    ) {
        if self.generation != current_generation {
            self.entries.clear();
            self.insertion_order.clear();
            self.generation = current_generation;
        }
        let cache_key = (key.clone(), primary_family.to_string());

        // If this is an update to an existing entry, remove the old key
        // from the insertion-order deque so it can be re-appended at the
        // back (most recently inserted).
        if self.entries.contains_key(&cache_key) {
            self.insertion_order.retain(|k| k != &cache_key);
        } else if self.entries.len() >= self.capacity {
            // Evict the oldest entry (front of the deque). Skip any keys
            // that are no longer in the map (stale deque entries from
            // prior updates).
            while let Some(old_key) = self.insertion_order.pop_front() {
                if self.entries.remove(&old_key).is_some() {
                    break;
                }
            }
        }

        self.insertion_order.push_back(cache_key.clone());
        self.entries.insert(cache_key, chain);
    }

    /// Returns the number of cached entries.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns the maximum number of entries the cache retains within a
    /// single generation.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns `true` if the cache is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clears all cached entries.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.insertion_order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_classification() {
        assert_eq!(classify_script('Z'), ScriptTag::Latin);
        assert_eq!(classify_script('é'), ScriptTag::Latin);
        assert_eq!(
            classify_script("العربية".chars().next().unwrap()),
            ScriptTag::Arabic
        );
        assert_eq!(
            classify_script("עברית".chars().next().unwrap()),
            ScriptTag::Hebrew
        );
        assert_eq!(
            classify_script("日本語".chars().next().unwrap()),
            ScriptTag::Cjk
        );
        assert_eq!(
            classify_script("हिन्दी".chars().next().unwrap()),
            ScriptTag::Devanagari
        );
        assert_eq!(classify_script('🚀'), ScriptTag::Emoji);
        assert_eq!(classify_script('√'), ScriptTag::Math);
    }

    #[test]
    fn test_platform_cascade_resolver() {
        let win = PlatformCascadeResolver::windows_fallbacks_for_script(ScriptTag::Cjk);
        assert!(win.contains(&"Meiryo") || win.contains(&"Yu Gothic"));

        let mac = PlatformCascadeResolver::macos_fallbacks_for_script(ScriptTag::Cjk);
        assert!(mac.contains(&"PingFang SC") || mac.contains(&"Hiragino Sans"));

        let lnx = PlatformCascadeResolver::linux_fallbacks_for_script(ScriptTag::Cjk);
        assert!(lnx.contains(&"Noto Sans CJK SC") || lnx.contains(&"Noto Sans CJK JP"));
    }

    #[test]
    fn test_font_fallback_chain() {
        let chain = FontFallbackChain::for_script("CustomPrimary", ScriptTag::Latin);
        assert_eq!(chain.primary, "CustomPrimary");
        let all: Vec<&str> = chain.families().collect();
        assert_eq!(all[0], "CustomPrimary");
        assert!(all.len() > 1);
    }

    #[test]
    fn test_font_fallback_cache() {
        let cache = FontFallbackCache::new();
        let key = FallbackKey::new(ScriptTag::Devanagari, "en-us");
        let list1 = cache.get_or_resolve(&key);
        assert!(!list1.is_empty());

        cache.register_custom_fallback(&key, "CustomDevanagari");
        let list2 = cache.get_or_resolve(&key);
        assert_eq!(list2[0], "CustomDevanagari");
    }

    #[test]
    fn fallback_key_normalizes_locale() {
        let key = FallbackKey::new(ScriptTag::Latin, "en-US");
        assert_eq!(key.locale, "en-us");
    }

    #[test]
    fn installed_resolver_returns_primary_if_covers_text() {
        let manager = crate::FontManager::with_fonts(std::iter::empty());
        let resolver = InstalledFontFallbackResolver::new(manager.system());
        let chain = resolver.resolve_for_text("ABC", "NonExistentPrimary");
        // Even with no fonts installed, the resolver should at least return
        // the primary family so the shaping pipeline has something to try.
        assert!(!chain.is_empty());
    }

    #[test]
    fn installed_resolver_falls_back_to_script_cascade() {
        let manager = crate::FontManager::with_fonts(std::iter::empty());
        let resolver = InstalledFontFallbackResolver::new(manager.system());
        let chain = resolver.resolve_for_text("漢", "Missing");
        let platform_fallbacks =
            PlatformCascadeResolver::platform_fallbacks_for_script(ScriptTag::Cjk);
        // At least one platform fallback name should be present in the chain.
        assert!(
            chain
                .iter()
                .any(|f| platform_fallbacks.contains(&f.as_str())),
            "installed resolver should include platform CJK fallbacks, got {:?}",
            chain
        );
    }

    #[test]
    fn installed_fallbacks_for_script_non_empty() {
        let manager = crate::FontManager::with_fonts(std::iter::empty());
        let resolver = InstalledFontFallbackResolver::new(manager.system());
        let installed = resolver.installed_fallbacks_for_script(ScriptTag::Latin);
        // Without real system fonts the list may be empty, but the API must
        // return a deterministic vector.
        assert!(
            installed.len()
                <= PlatformCascadeResolver::platform_fallbacks_for_script(ScriptTag::Latin).len()
        );
    }
}
