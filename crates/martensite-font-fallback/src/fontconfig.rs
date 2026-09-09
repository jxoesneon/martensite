//! Linux Fontconfig font fallback provider.
//!
//! Uses `FcFontSort` to query the OS for locale-aware font fallback.
//! This is the same API that Linux desktop environments use internally
//! for font fallback.
//!
//! # Safety
//!
//! All `unsafe` blocks in this module call Fontconfig C functions that
//! are documented to be safe per the Fontconfig documentation:
//! - `FcFontSort`:
//!   <https://freedesktop.org/software/fontconfig/fontconfig-devel/fcfontsort.html>
//! - `FcPatternGet`:
//!   <https://freedesktop.org/software/fontconfig/fontconfig-devel/fcpatternget.html>
//!
//! The `fontconfig` crate (`fontconfig = "0.11"`) provides safe Rust
//! wrappers around these C functions. This module uses those wrappers
//! rather than calling the C FFI directly, so no `unsafe` blocks are
//! required here; the safety boundary is encapsulated inside the
//! `fontconfig` crate itself.

use fontconfig::{CharSet, Fontconfig, Pattern, UnicodeCoverage};
use martensite_text::cascade::{FontFallbackProvider, ScriptTag};
use tracing::warn;

/// A Fontconfig-based font fallback provider for Linux.
///
/// This provider queries the Linux Fontconfig system for
/// locale-aware fallback using `FcFontSort`. It returns the system's
/// default fallback families for each script, which respects the
/// user's language preferences and installed fonts.
///
/// The provider holds a [`Fontconfig`] handle obtained from
/// [`Fontconfig::new`]. If Fontconfig could not be initialised (e.g.
/// `libfontconfig` is not installed), the handle is `None` and the
/// provider falls back to the static per-OS family lists, identical
/// to [`PlatformCascadeResolver`](martensite_text::cascade::PlatformCascadeResolver).
///
/// # Thread safety
///
/// The [`Fontconfig`] handle is a zero-sized marker that signals the
/// Fontconfig library has been initialised. Fontconfig's C API is
/// documented as thread-safe after `FcInit` has been called, so the
/// handle is safe to share across threads.
///
/// # Examples
///
/// ```
/// use martensite_font_fallback::fontconfig::FontconfigFontFallback;
/// use martensite_text::cascade::FontFallbackProvider;
///
/// let provider = FontconfigFontFallback::new();
/// let fallbacks = provider.script_fallbacks(
///     martensite_text::cascade::ScriptTag::Cjk,
///     "zh-cn",
/// );
/// assert!(!fallbacks.is_empty());
/// ```
pub struct FontconfigFontFallback {
    /// The Fontconfig handle, or `None` if Fontconfig could not be
    /// initialised (in which case static fallback lists are used).
    fc: Option<Fontconfig>,
}

impl FontconfigFontFallback {
    /// Creates a new Fontconfig font fallback provider.
    ///
    /// This initialises the Fontconfig library via `FcInit`. If
    /// Fontconfig is unavailable, the provider degrades to the
    /// static fallback lists.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_font_fallback::fontconfig::FontconfigFontFallback;
    ///
    /// let provider = FontconfigFontFallback::new();
    /// ```
    pub fn new() -> Self {
        Self {
            fc: Fontconfig::new(),
        }
    }

    /// Returns sample text for the given script tag, used to query
    /// Fontconfig for the fallback family.
    fn sample_text_for_script(script: ScriptTag) -> &'static str {
        match script {
            ScriptTag::Latin => "Aa Bb Cc",
            ScriptTag::Arabic => "مرحبا",
            ScriptTag::Hebrew => "שלום",
            ScriptTag::Cjk => "漢字",
            ScriptTag::Devanagari => "नमस्ते",
            ScriptTag::Emoji => "🦀",
            ScriptTag::Math => "∑∫√",
            ScriptTag::Other => "A",
        }
    }

    /// Queries Fontconfig (`FcFontSort`) for the fallback family
    /// names that cover the characters of `sample_text`.
    ///
    /// Returns an empty vector if Fontconfig is unavailable or the
    /// query yields no results.
    fn fontconfig_fallbacks(&self, sample_text: &str) -> Vec<String> {
        let fc = match &self.fc {
            Some(fc) => fc,
            None => return Vec::new(),
        };

        // Build a charset from the sample text characters so that
        // FcFontSort returns fonts that actually cover the script.
        let mut charset = match CharSet::new(fc) {
            Ok(cs) => cs,
            Err(_) => return Vec::new(),
        };
        for ch in sample_text.chars() {
            // Whitespace and control characters are not useful for
            // coverage matching; skip them.
            if ch.is_whitespace() || ch.is_control() {
                continue;
            }
            // If Fontconfig cannot add a character to the charset (e.g.
            // allocation failure), the query would proceed with an
            // incomplete charset and could return fonts that do not
            // actually cover the requested script. Fail safe: log the
            // failure and abort with an empty fallback vector so the
            // caller falls back to the static per-OS family lists.
            if let Err(err) = charset.add_char(ch) {
                warn!(
                    char = ?ch,
                    error = ?err,
                    "fontconfig charset add_char failed; aborting fallback query"
                );
                return Vec::new();
            }
        }

        // Create a pattern with the charset. We do not specify a
        // family so Fontconfig returns the default fallback cascade
        // for the requested characters.
        let mut pattern = match Pattern::new(fc) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };
        if let Err(_) = pattern.add_charset(charset) {
            return Vec::new();
        }

        // FcFontSort returns fonts sorted by closeness to the pattern.
        // Using `NoTrim` keeps all candidates so we can extract a
        // broader fallback chain.
        let font_set = match pattern.sort_fonts(UnicodeCoverage::NoTrim) {
            Ok(set) => set,
            Err(_) => return Vec::new(),
        };

        // Extract family names from the sorted patterns. Each pattern
        // may expose a "fullname" (FC_FULLNAME) or a "family" (FC_FAMILY);
        // we prefer the family name for cascade purposes.
        let mut families = Vec::new();
        for pat in font_set.iter() {
            // Prefer FC_FAMILY for the cascade family name.
            let family = pat
                .get_string(fontconfig::FC_FAMILY)
                .ok()
                .map(|s| s.to_string());
            if let Some(name) = family {
                if !name.is_empty()
                    && !families
                        .iter()
                        .any(|f: &String| f.eq_ignore_ascii_case(&name))
                {
                    families.push(name);
                }
            }
        }

        families
    }

    /// Returns the static fallback list for `script`, used as a safety
    /// net when Fontconfig is unavailable or returns no results.
    fn static_fallbacks(script: ScriptTag) -> &'static [&'static str] {
        match script {
            ScriptTag::Latin => &["Noto Sans", "DejaVu Sans"][..],
            ScriptTag::Cjk => &["Noto Sans CJK SC", "Noto Sans CJK JP", "Noto Sans CJK KR"][..],
            ScriptTag::Emoji => &["Noto Color Emoji"][..],
            ScriptTag::Devanagari => &["Noto Sans Devanagari"][..],
            ScriptTag::Arabic => &["Noto Sans Arabic"][..],
            ScriptTag::Hebrew => &["Noto Sans Hebrew"][..],
            ScriptTag::Math => &["Noto Sans Math", "DejaVu Sans Math"][..],
            ScriptTag::Other => &["Noto Sans"][..],
        }
    }
}

impl Default for FontconfigFontFallback {
    fn default() -> Self {
        Self::new()
    }
}

impl FontFallbackProvider for FontconfigFontFallback {
    fn common_fallbacks(&self) -> Vec<String> {
        vec!["Noto Sans".to_string(), "DejaVu Sans".to_string()]
    }

    fn script_fallbacks(&self, script: ScriptTag, locale: &str) -> Vec<String> {
        let mut families = Vec::new();

        // Query Fontconfig for the primary fallback families that
        // cover the script's sample characters.
        let sample = Self::sample_text_for_script(script);
        for family in self.fontconfig_fallbacks(sample) {
            if !families
                .iter()
                .any(|f: &String| f.eq_ignore_ascii_case(&family))
            {
                families.push(family);
            }
        }

        // Add locale-specific CJK variants.
        if script == ScriptTag::Cjk {
            let locale_lower = locale.to_lowercase();
            if locale_lower.starts_with("ja") {
                push_unique(&mut families, "Noto Sans CJK JP");
            } else if locale_lower.starts_with("ko") {
                push_unique(&mut families, "Noto Sans CJK KR");
            } else {
                push_unique(&mut families, "Noto Sans CJK SC");
            }
        }

        // Add the static platform fallbacks as a safety net.
        for &f in Self::static_fallbacks(script) {
            if !families.iter().any(|x: &String| x.eq_ignore_ascii_case(f)) {
                families.push(f.to_string());
            }
        }

        families
    }

    fn forbidden_fallbacks(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Pushes `name` into `families` if it is not already present
/// (case-insensitive).
fn push_unique(families: &mut Vec<String>, name: &str) {
    if !families
        .iter()
        .any(|f: &String| f.eq_ignore_ascii_case(name))
    {
        families.push(name.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fontconfig_provider_returns_fallbacks() {
        let provider = FontconfigFontFallback::new();
        let fallbacks = provider.script_fallbacks(ScriptTag::Latin, "en-us");
        assert!(!fallbacks.is_empty());
    }

    #[test]
    fn fontconfig_provider_cjk_fallbacks() {
        let provider = FontconfigFontFallback::new();
        let fallbacks = provider.script_fallbacks(ScriptTag::Cjk, "zh-cn");
        assert!(!fallbacks.is_empty());
    }

    #[test]
    fn fontconfig_provider_falls_back_to_static() {
        // Even if Fontconfig is unavailable, the static fallbacks
        // ensure a non-empty result.
        let provider = FontconfigFontFallback { fc: None };
        let fallbacks = provider.script_fallbacks(ScriptTag::Emoji, "en-us");
        assert!(fallbacks.iter().any(|f| f == "Noto Color Emoji"));
    }

    #[test]
    fn fontconfig_provider_common_fallbacks() {
        let provider = FontconfigFontFallback::new();
        let common = provider.common_fallbacks();
        assert!(!common.is_empty());
    }
}
