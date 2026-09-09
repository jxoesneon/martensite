//! macOS CoreText font fallback provider.
//!
//! Uses `CTFontCreateForStringWithLanguage` to query the OS for
//! locale-aware font fallback. This is the same API that macOS uses
//! internally for its font fallback cascade.
//!
//! # Safety
//!
//! All `unsafe` blocks in this module call CoreText C functions that
//! are documented to be safe per Apple's CoreText documentation:
//! - `CTFontCreateForStringWithLanguage`:
//!   <https://developer.apple.com/documentation/coretext/ctfontcreateforstringwithlanguage>
//! - `CTFontCopyFamilyName`:
//!   <https://developer.apple.com/documentation/coretext/ctfontcopyfamilyname>

use std::ffi::c_void;

use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};

use martensite_text::cascade::{FontFallbackProvider, ScriptTag};

/// A CoreText-based font fallback provider for macOS.
///
/// This provider queries the macOS CoreText font system for
/// locale-aware fallback using `CTFontCreateForStringWithLanguage`.
/// It returns the system's default fallback family for each script,
/// which respects the user's language preferences and installed fonts.
///
/// # Thread safety
///
/// `CTFontRef` is a CoreFoundation object that is reference-counted
/// and documented as thread-safe by Apple. We wrap the raw pointer in
/// a `Send + Sync` newtype because the underlying CFType is immutable
/// after creation and the reference count is atomically managed.
///
/// # Examples
///
/// ```
/// use martensite_font_fallback::coretext::CoreTextFontFallback;
/// use martensite_text::cascade::FontFallbackProvider;
///
/// let provider = CoreTextFontFallback::new();
/// let fallbacks = provider.script_fallbacks(
///     martensite_text::cascade::ScriptTag::Cjk,
///     "zh-cn",
/// );
/// assert!(!fallbacks.is_empty());
/// ```
pub struct CoreTextFontFallback {
    /// The system default font descriptor, used as the base for
    /// `CTFontCreateForStringWithLanguage`. Wrapped in a `Send + Sync`
    /// newtype because `CTFontRef` is a thread-safe CoreFoundation
    /// reference-counted object.
    base_font: SendSyncCTFontRef,
}

/// A `Send + Sync` wrapper around `CTFontRef`.
///
/// # Safety
///
/// `CTFontRef` is a CoreFoundation reference-counted object. Apple's
/// documentation states that CoreFoundation objects are thread-safe
/// when their reference counts are managed correctly. We hold a
/// single reference (created via `CTFontCreateUIFontForLanguage` which
/// returns a +1 reference) and release it in `Drop`. The object is
/// immutable after creation, so concurrent reads are safe.
struct SendSyncCTFontRef(CTFontRef);

unsafe impl Send for SendSyncCTFontRef {}
unsafe impl Sync for SendSyncCTFontRef {}

/// Opaque type alias for `CTFontRef`.
type CTFontRef = *mut c_void;

extern "C" {
    fn CTFontCreateUIFontForLanguage(ui_type: u32, size: f64, language: CFStringRef) -> CTFontRef;
    fn CTFontCreateForStringWithLanguage(
        font: CTFontRef,
        string: CFStringRef,
        range: CFRange,
        language: CFStringRef,
    ) -> CTFontRef;
    fn CTFontCopyFamilyName(font: CTFontRef) -> CFStringRef;
}

/// A CoreFoundation range (location, length).
#[repr(C)]
struct CFRange {
    location: isize,
    length: isize,
}

impl CoreTextFontFallback {
    /// Creates a new CoreText font fallback provider.
    ///
    /// This initializes a base system font (the system UI font) that
    /// is used as the starting point for `CTFontCreateForStringWithLanguage`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_font_fallback::coretext::CoreTextFontFallback;
    ///
    /// let provider = CoreTextFontFallback::new();
    /// ```
    pub fn new() -> Self {
        // Use the system UI font (kCTFontUIFontSystem = 0) as the base.
        // Size 0.0 means "default size for the UI type".
        let base_font = unsafe { CTFontCreateUIFontForLanguage(0, 0.0, std::ptr::null()) };
        Self {
            base_font: SendSyncCTFontRef(base_font),
        }
    }

    /// Queries CoreText for the fallback family name for the given
    /// text and locale.
    ///
    /// Returns `None` if CoreText cannot find a fallback font.
    fn fallback_family_for_text(&self, text: &str, locale: &str) -> Option<String> {
        if text.is_empty() {
            return None;
        }

        let cf_text = CFString::new(text);
        let cf_language = if locale.is_empty() {
            std::ptr::null()
        } else {
            let lang = CFString::new(locale);
            lang.as_concrete_TypeRef()
        };

        let range = CFRange {
            location: 0,
            length: cf_text.char_len(),
        };

        let font = unsafe {
            CTFontCreateForStringWithLanguage(
                self.base_font.0,
                cf_text.as_concrete_TypeRef(),
                range,
                cf_language,
            )
        };

        if font.is_null() {
            return None;
        }

        let family_name_ref = unsafe { CTFontCopyFamilyName(font) };
        unsafe { CFRelease(font as CFTypeRef) };

        if family_name_ref.is_null() {
            return None;
        }

        let family = unsafe { CFString::wrap_under_create_rule(family_name_ref) };
        let name = family.to_string();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }

    /// Returns sample text for the given script tag, used to query
    /// CoreText for the fallback family.
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
}

impl Default for CoreTextFontFallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CoreTextFontFallback {
    fn drop(&mut self) {
        if !self.base_font.0.is_null() {
            unsafe { CFRelease(self.base_font.0 as CFTypeRef) };
        }
    }
}

impl FontFallbackProvider for CoreTextFontFallback {
    fn common_fallbacks(&self) -> Vec<String> {
        // The macOS system font covers the widest glyph range.
        // Helvetica Neue is the classic fallback.
        vec!["Helvetica Neue".to_string(), "Arial Unicode MS".to_string()]
    }

    fn script_fallbacks(&self, script: ScriptTag, locale: &str) -> Vec<String> {
        let sample = Self::sample_text_for_script(script);
        let mut families = Vec::new();

        // Query CoreText for the primary fallback.
        if let Some(primary) = self.fallback_family_for_text(sample, locale) {
            families.push(primary);
        }

        // Add locale-specific CJK variants.
        if script == ScriptTag::Cjk {
            let locale_lower = locale.to_lowercase();
            if locale_lower.starts_with("ja") {
                families.push("Hiragino Sans".to_string());
            } else if locale_lower.starts_with("ko") {
                families.push("Apple SD Gothic Neo".to_string());
            } else {
                // Default to Simplified Chinese for zh and other locales.
                families.push("PingFang SC".to_string());
            }
        }

        // Add the static platform fallbacks as a safety net.
        let static_fallbacks = match script {
            ScriptTag::Latin => &["SF Pro", "Helvetica Neue"][..],
            ScriptTag::Cjk => &["PingFang SC", "Hiragino Sans", "Apple SD Gothic Neo"][..],
            ScriptTag::Emoji => &["Apple Color Emoji"][..],
            ScriptTag::Devanagari => &["Devanagari MT", "Kohinoor Devanagari"][..],
            ScriptTag::Arabic => &["Geeza Pro"][..],
            ScriptTag::Hebrew => &["Arial Hebrew", "Lucida Grande"][..],
            ScriptTag::Math => &["STIXGeneral", "Apple Symbols"][..],
            ScriptTag::Other => &["SF Pro"][..],
        };

        for &f in static_fallbacks {
            if !families.iter().any(|x| x.eq_ignore_ascii_case(f)) {
                families.push(f.to_string());
            }
        }

        families
    }

    fn forbidden_fallbacks(&self) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coretext_provider_returns_fallbacks() {
        let provider = CoreTextFontFallback::new();
        let fallbacks = provider.script_fallbacks(ScriptTag::Latin, "en-us");
        assert!(!fallbacks.is_empty());
    }

    #[test]
    fn coretext_provider_cjk_fallbacks() {
        let provider = CoreTextFontFallback::new();
        let fallbacks = provider.script_fallbacks(ScriptTag::Cjk, "zh-cn");
        assert!(!fallbacks.is_empty());
    }

    #[test]
    fn coretext_provider_common_fallbacks() {
        let provider = CoreTextFontFallback::new();
        let common = provider.common_fallbacks();
        assert!(!common.is_empty());
    }
}
