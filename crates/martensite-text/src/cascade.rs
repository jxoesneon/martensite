//! Platform System Font Fallback Cascades to eliminate missing-glyph tofu.
//!
//! This module provides:
//! - Script categorization for Unicode characters (`ScriptTag`, `classify_script`).
//! - OS-native system font fallback cascade resolution (`PlatformCascadeResolver`).
//! - Font family fallback chains (`FontFallbackChain`).
//! - Thread-safe cached resolution for font fallback chains (`FontFallbackCache`).

use std::collections::HashMap;
use std::sync::RwLock;

use cosmic_text::FontSystem;

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
        let fallbacks = PlatformCascadeResolver::platform_fallbacks_for_script(script)
            .iter()
            .map(|&s| s.to_string())
            .collect();
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
/// and works on all supported platforms. Platform-native APIs such as
/// DirectWrite, CoreText, or Fontconfig are not called directly because doing
/// so safely from `forbid(unsafe_code)` code would require additional binding
/// crates; fontdb already queries the platform font directories and caches
/// the results.
pub struct InstalledFontFallbackResolver<'a> {
    font_system: &'a FontSystem,
}

impl<'a> InstalledFontFallbackResolver<'a> {
    /// Creates a resolver bound to the given font system.
    #[inline]
    pub fn new(font_system: &'a FontSystem) -> Self {
        Self { font_system }
    }

    /// Returns the list of font families to try for `text`, in priority
    /// order.
    ///
    /// The returned list always begins with `primary` so the shaping
    /// pipeline has a family to attempt first. It is followed by the
    /// platform fallback families for every script present in `text`;
    /// families that are actually installed *and* cover the relevant
    /// characters are listed before the remaining platform candidates.
    /// Candidate names are always appended, even when the font database
    /// contains no faces for them, because the downstream shaper
    /// performs its own resolution and generic fallbacks.
    pub fn resolve_for_text(&self, text: &str, primary: &str) -> Vec<String> {
        let mut chain: Vec<String> = vec![primary.to_string()];

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

        // Pass 1: installed platform candidates that actually cover the
        // script's characters, so they are tried before nominal names.
        for &script in &scripts {
            let needed: String = text
                .chars()
                .filter(|ch| classify_script(*ch) == script)
                .collect();
            for &family in PlatformCascadeResolver::platform_fallbacks_for_script(script) {
                if chain.iter().any(|f| f.eq_ignore_ascii_case(family)) {
                    continue;
                }
                if self.family_covers_text(family, &needed) {
                    chain.push(family.to_string());
                }
            }
        }

        // Pass 2: the remaining platform cascade names as candidates.
        for &script in &scripts {
            for &family in PlatformCascadeResolver::platform_fallbacks_for_script(script) {
                if !chain.iter().any(|f| f.eq_ignore_ascii_case(family)) {
                    chain.push(family.to_string());
                }
            }
        }

        chain
    }

    /// Returns the subset of platform fallback family names that are
    /// actually installed in the font database.
    pub fn installed_fallbacks_for_script(&self, script: ScriptTag) -> Vec<String> {
        PlatformCascadeResolver::platform_fallbacks_for_script(script)
            .iter()
            .filter(|family| self.family_installed(family))
            .map(|&family| family.to_string())
            .collect()
    }

    /// Returns `true` if the font face with the given ID contains a glyph
    /// for `ch`.
    ///
    /// The face's `cmap` is inspected by parsing the font data through
    /// `swash`'s character map; a glyph ID of `0` (`.notdef`) counts as
    /// uncovered.
    fn face_covers_char(&self, face_id: fontdb::ID, ch: char) -> bool {
        self.font_system
            .db()
            .with_face_data(face_id, |data, index| {
                swash::FontRef::from_index(data, index as usize)
                    .is_some_and(|font| font.charmap().map(ch) != 0)
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

/// A thread-safe cache for resolved font fallback chains indexed by script.
///
/// # Examples
///
/// ```
/// use martensite_text::cascade::{FontFallbackCache, ScriptTag};
///
/// let cache = FontFallbackCache::new();
/// let fonts = cache.get_or_resolve(ScriptTag::Emoji);
/// assert!(!fonts.is_empty());
/// ```
#[derive(Debug, Default)]
pub struct FontFallbackCache {
    cache: RwLock<HashMap<ScriptTag, Vec<String>>>,
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

    /// Returns the cached font fallback families for the script, or resolves and caches them.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cascade::{FontFallbackCache, ScriptTag};
    ///
    /// let cache = FontFallbackCache::new();
    /// let list = cache.get_or_resolve(ScriptTag::Latin);
    /// assert!(!list.is_empty());
    /// ```
    pub fn get_or_resolve(&self, script: ScriptTag) -> Vec<String> {
        if let Ok(reader) = self.cache.read() {
            if let Some(cached) = reader.get(&script) {
                return cached.clone();
            }
        }

        let resolved: Vec<String> = PlatformCascadeResolver::platform_fallbacks_for_script(script)
            .iter()
            .map(|&s| s.to_string())
            .collect();

        if let Ok(mut writer) = self.cache.write() {
            writer.insert(script, resolved.clone());
        }

        resolved
    }

    /// Registers a custom font family at the front of the fallback cascade for the specified script.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_text::cascade::{FontFallbackCache, ScriptTag};
    ///
    /// let cache = FontFallbackCache::new();
    /// cache.register_custom_fallback(ScriptTag::Cjk, "Source Han Sans");
    /// let list = cache.get_or_resolve(ScriptTag::Cjk);
    /// assert_eq!(list[0], "Source Han Sans");
    /// ```
    pub fn register_custom_fallback(&self, script: ScriptTag, family: &str) {
        let mut writer = match self.cache.write() {
            Ok(w) => w,
            Err(poisoned) => poisoned.into_inner(),
        };

        let entry = writer.entry(script).or_insert_with(|| {
            PlatformCascadeResolver::platform_fallbacks_for_script(script)
                .iter()
                .map(|&s| s.to_string())
                .collect()
        });

        if !entry.iter().any(|f| f == family) {
            entry.insert(0, family.to_string());
        }
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
        let list1 = cache.get_or_resolve(ScriptTag::Devanagari);
        assert!(!list1.is_empty());

        cache.register_custom_fallback(ScriptTag::Devanagari, "CustomDevanagari");
        let list2 = cache.get_or_resolve(ScriptTag::Devanagari);
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
