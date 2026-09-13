//! Windows DirectWrite font fallback provider.
//!
//! Uses `IDWriteFontFallback::MapCharacters` to query the OS for
//! locale-aware font fallback. This is the same API that Windows uses
//! internally for its font fallback cascade.
//!
//! # Safety
//!
//! All `unsafe` blocks in this module call DirectWrite COM methods that
//! are documented to be safe per Microsoft's DirectWrite documentation:
//! - `DWriteCreateFactory`:
//!   <https://learn.microsoft.com/en-us/windows/win32/api/dwrite/nf-dwrite-dwritecreatefactory>
//! - `IDWriteFontFallback::MapCharacters`:
//!   <https://learn.microsoft.com/en-us/windows/win32/api/dwrite_2/nf-dwrite_2-idwritefontfallback-mapcharacters>

use martensite_text::cascade::{FontFallbackProvider, ScriptTag};

use windows::core::{implement, Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::BOOL;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFactory2, IDWriteFont, IDWriteFontCollection,
    IDWriteFontFallback, IDWriteNumberSubstitution, IDWriteTextAnalysisSource,
    IDWriteTextAnalysisSource_Impl, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_READING_DIRECTION_LEFT_TO_RIGHT,
};

/// A DirectWrite-based font fallback provider for Windows.
///
/// This provider queries the Windows DirectWrite font system for
/// locale-aware fallback using `IDWriteFontFallback::MapCharacters`.
/// It returns the system's default fallback family for each script,
/// which respects the user's language preferences and installed fonts.
///
/// # Examples
///
/// ```
/// use martensite_font_fallback::directwrite::DirectWriteFontFallback;
/// use martensite_text::cascade::FontFallbackProvider;
///
/// if let Some(provider) = DirectWriteFontFallback::new() {
///     let fallbacks = provider.script_fallbacks(
///         martensite_text::cascade::ScriptTag::Cjk,
///         "zh-cn",
///     );
///     assert!(!fallbacks.is_empty());
/// }
/// ```
pub struct DirectWriteFontFallback {
    /// The DirectWrite factory, used to create the font fallback.
    factory: IDWriteFactory,
    /// The font fallback object, cached for reuse.
    fallback: Option<IDWriteFontFallback>,
}

impl DirectWriteFontFallback {
    /// Creates a new DirectWrite font fallback provider.
    ///
    /// This initializes a DirectWrite factory and retrieves the
    /// `IDWriteFontFallback` interface from it.
    ///
    /// # Returns
    ///
    /// `Some(Self)` on success, or `None` if `DWriteCreateFactory`
    /// fails (for example, if DirectWrite is unavailable or COM
    /// initialization fails). Callers should fall back to a non-native
    /// provider in that case. Note that a `Some` result may still have
    /// no `IDWriteFontFallback` interface on older Windows versions;
    /// in that case the provider degrades to static fallback lists.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_font_fallback::directwrite::DirectWriteFontFallback;
    /// use martensite_text::cascade::FontFallbackProvider;
    ///
    /// if let Some(provider) = DirectWriteFontFallback::new() {
    ///     let fallbacks = provider.script_fallbacks(
    ///         martensite_text::cascade::ScriptTag::Latin,
    ///         "en-us",
    ///     );
    ///     assert!(!fallbacks.is_empty());
    /// }
    /// ```
    pub fn new() -> Option<Self> {
        // SAFETY: `DWriteCreateFactory` is a stateless OS entry point
        // that is always safe to call. It takes no pointer arguments
        // and returns a COM object (or an `HRESULT` error) through the
        // `windows` crate's checked `Result`, so no raw pointers are
        // involved. COM objects created this way are valid for use on
        // any thread once marshalled; `IDWriteFactory` is an agile
        // (free-threaded) object per the DirectWrite documentation.
        let factory =
            unsafe { DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) }.ok()?;

        // Try to get IDWriteFontFallback from IDWriteFactory2.
        let fallback = factory.cast::<IDWriteFactory2>().ok().and_then(|f2| {
            // SAFETY: `f2` is a valid `IDWriteFactory2` COM object
            // obtained by `cast` from the live `factory`.
            // `GetSystemFontFallback` takes no pointer arguments and
            // returns a COM object or an `HRESULT` error; both are
            // handled by `.ok()`.
            unsafe { f2.GetSystemFontFallback().ok() }
        });

        Some(Self { factory, fallback })
    }

    /// Queries DirectWrite for the fallback family name for the given
    /// text and locale.
    ///
    /// Returns `None` if DirectWrite cannot find a fallback font.
    fn fallback_family_for_text(&self, text: &str, locale: &str) -> Option<String> {
        let fallback = self.fallback.as_ref()?;

        if text.is_empty() {
            return None;
        }

        // Create a simple text analysis source that returns the text
        // and locale. DirectWrite uses this to determine the script
        // and locale for fallback mapping.
        let source = SimpleAnalysisSource::new(text, locale);

        // Convert text to UTF-16 for the text length parameter.
        let text_utf16: Vec<u16> = text.encode_utf16().collect();
        let text_len = text_utf16.len() as u32;

        let mut mapped_font: Option<IDWriteFont> = None;
        let mut mapped_length = 0u32;
        let mut scale = 1.0f32;

        // SAFETY: `fallback` is a valid `IDWriteFontFallback` COM
        // object. `source` is a live `IDWriteTextAnalysisSource`
        // implementation that outlives this call, and `text_len` is the
        // length of its text in UTF-16 code units. A null
        // `IDWriteFontCollection` and `PCWSTR::null()` are documented to
        // select the system collection and the user's default locale.
        // `mapped_length`, `mapped_font`, and `scale` are valid
        // out-pointers to stack locals. Errors are reported via
        // `HRESULT` and ignored here — failure just yields no mapping.
        let _ = unsafe {
            fallback.MapCharacters(
                &source,
                0,
                text_len,
                None::<&IDWriteFontCollection>,
                PCWSTR::null(),
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                &mut mapped_length,
                &mut mapped_font,
                &mut scale,
            )
        };

        let font = mapped_font?;

        // Get the family name from the mapped font.
        // IDWriteFont -> GetFontFamily -> IDWriteFontFamily -> GetFamilyNames
        // -> IDWriteLocalizedStrings -> FindLocaleName + GetString

        // SAFETY: `font` is a valid `IDWriteFont` COM object returned by
        // `MapCharacters`; `GetFontFamily` returns a COM object or an
        // `HRESULT` error, both handled by `.ok()`.
        let family = unsafe { font.GetFontFamily().ok() }?;
        // SAFETY: `family` is a valid `IDWriteFontFamily` COM object;
        // `GetFamilyNames` returns a COM object or an `HRESULT` error,
        // both handled by `.ok()`.
        let names = unsafe { family.GetFamilyNames().ok() }?;

        let locale_str = if locale.is_empty() { "en-us" } else { locale };
        let locale_hstring = HSTRING::from(locale_str);

        let mut index = 0u32;
        let mut exists = BOOL(0);

        // SAFETY: `names` is a valid `IDWriteLocalizedStrings` object,
        // `locale_hstring` is a valid `HSTRING` that outlives the call,
        // and `index`/`exists` are valid out-pointers to stack locals.
        // A failure `HRESULT` is ignored; `exists` stays false and we
        // fall back to index 0 below.
        let _ = unsafe { names.FindLocaleName(&locale_hstring, &mut index, &mut exists) };

        // If the locale doesn't exist, fall back to index 0.
        if !exists.as_bool() {
            index = 0;
        }

        // SAFETY: `names` is valid and `index` is either a locale index
        // confirmed by `FindLocaleName` or 0. An out-of-range index
        // produces an `HRESULT` error (handled by `.ok()?`), not
        // undefined behavior.
        let length = unsafe { names.GetStringLength(index).ok()? } as usize;
        let mut buffer = vec![0u16; length + 1];
        // SAFETY: `buffer` contains `length + 1` elements, where
        // `length` is the value `GetStringLength` reported for the same
        // `index`. `GetString` therefore writes at most `length` code
        // units plus the NUL terminator, all within the allocation.
        unsafe { names.GetString(index, &mut buffer).ok()? };

        // Convert the UTF-16 buffer to a Rust String.
        let name = String::from_utf16_lossy(&buffer[..length]);
        let name = name.trim_end_matches('\0').to_string();

        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }

    /// Returns sample text for the given script tag, used to query
    /// DirectWrite for the fallback family.
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

impl FontFallbackProvider for DirectWriteFontFallback {
    fn common_fallbacks(&self) -> Vec<String> {
        vec!["Segoe UI".to_string(), "Arial".to_string()]
    }

    fn script_fallbacks(&self, script: ScriptTag, locale: &str) -> Vec<String> {
        let sample = Self::sample_text_for_script(script);
        let mut families = Vec::new();

        // Query DirectWrite for the primary fallback.
        if let Some(primary) = self.fallback_family_for_text(sample, locale) {
            families.push(primary);
        }

        // Add locale-specific CJK variants.
        if script == ScriptTag::Cjk {
            let locale_lower = locale.to_lowercase();
            if locale_lower.starts_with("ja") {
                families.push("Yu Gothic".to_string());
                families.push("Meiryo".to_string());
            } else if locale_lower.starts_with("ko") {
                families.push("Malgun Gothic".to_string());
            } else {
                families.push("Microsoft YaHei".to_string());
            }
        }

        // Add the static platform fallbacks as a safety net.
        let static_fallbacks = match script {
            ScriptTag::Latin => &["Segoe UI", "Arial"][..],
            ScriptTag::Cjk => &["Meiryo", "Yu Gothic", "Malgun Gothic", "Microsoft YaHei"][..],
            ScriptTag::Emoji => &["Segoe UI Emoji", "Segoe UI Symbol"][..],
            ScriptTag::Devanagari => &["Nirmala UI", "Mangal"][..],
            ScriptTag::Arabic => &["Segoe UI", "Arabic Typesetting"][..],
            ScriptTag::Hebrew => &["Segoe UI", "David"][..],
            ScriptTag::Math => &["Cambria Math", "Segoe UI Symbol"][..],
            ScriptTag::Other => &["Segoe UI"][..],
        };

        for &f in static_fallbacks {
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

/// A simple `IDWriteTextAnalysisSource` implementation that returns
/// a fixed text and locale for fallback mapping.
///
/// This is used by `MapCharacters` to determine the script and locale
/// of the text being mapped. The text is stored as UTF-16 so that
/// `GetTextAtPosition` can return a stable pointer to the text buffer.
#[allow(non_snake_case)]
#[implement(IDWriteTextAnalysisSource)]
struct SimpleAnalysisSource {
    /// The text in UTF-16 code units, used by `GetTextAtPosition`.
    text_utf16: Vec<u16>,
    /// The locale in UTF-16 code units (null-terminated), used by
    /// `GetLocaleName`.
    locale_utf16: Vec<u16>,
}

impl SimpleAnalysisSource {
    fn new(text: &str, locale: &str) -> Self {
        let text_utf16: Vec<u16> = text.encode_utf16().collect();
        // Store the locale as a null-terminated UTF-16 string so the
        // pointer returned by GetLocaleName is a valid C-style wide
        // string.
        let mut locale_utf16: Vec<u16> = locale.encode_utf16().collect();
        locale_utf16.push(0);
        Self {
            text_utf16,
            locale_utf16,
        }
    }
}

impl IDWriteTextAnalysisSource_Impl for SimpleAnalysisSource {
    fn GetTextAtPosition(
        &self,
        textposition: u32,
        textstring: *mut *mut u16,
        textlength: *mut u32,
    ) -> windows::core::Result<()> {
        let pos = textposition as usize;
        let len = self.text_utf16.len();

        if pos > len {
            return Err(windows::core::Error::from(
                windows::Win32::Foundation::WIN32_ERROR(0x80070057), // E_INVALIDARG
            ));
        }

        // Return a pointer to the text starting at `textposition`.
        // The pointer is valid for the lifetime of this object because
        // `text_utf16` is owned by `self` and not reallocated.
        if !textstring.is_null() {
            // SAFETY: `textstring` is non-null (checked above) and
            // points to a caller-provided out-slot valid for one write.
            // `pos <= len` was checked above, so `.add(pos)` stays
            // within the `text_utf16` allocation (one past the end is
            // permitted). The stored pointer borrows `self`, which
            // DirectWrite only dereferences during `MapCharacters`.
            unsafe {
                *textstring = self.text_utf16.as_ptr().add(pos) as *mut u16;
            }
        }
        if !textlength.is_null() {
            // SAFETY: `textlength` is non-null (checked above) and
            // points to a caller-provided out-slot valid for one write.
            // `pos <= len`, so `len - pos` cannot underflow.
            unsafe {
                *textlength = (len - pos) as u32;
            }
        }
        Ok(())
    }

    fn GetTextBeforePosition(
        &self,
        textposition: u32,
        textstring: *mut *mut u16,
        textlength: *mut u32,
    ) -> windows::core::Result<()> {
        let pos = (textposition as usize).min(self.text_utf16.len());

        // Return a pointer to the text before `textposition`.
        if !textstring.is_null() {
            // SAFETY: `textstring` is non-null (checked above) and
            // points to a caller-provided out-slot valid for one write.
            // The base pointer of `text_utf16` is valid for the `pos`
            // code units reported via `textlength` below (when `pos`
            // is 0 the pointer is never dereferenced). It borrows
            // `self`, which outlives the `MapCharacters` call.
            unsafe {
                *textstring = self.text_utf16.as_ptr() as *mut u16;
            }
        }
        if !textlength.is_null() {
            // SAFETY: `textlength` is non-null (checked above) and
            // points to a caller-provided out-slot valid for one write.
            // `pos` is clamped to the buffer length above.
            unsafe {
                *textlength = pos as u32;
            }
        }
        Ok(())
    }

    fn GetParagraphReadingDirection(
        &self,
    ) -> windows::Win32::Graphics::DirectWrite::DWRITE_READING_DIRECTION {
        DWRITE_READING_DIRECTION_LEFT_TO_RIGHT
    }

    fn GetLocaleName(
        &self,
        _textposition: u32,
        textlength: *mut u32,
        localename: *mut *mut u16,
    ) -> windows::core::Result<()> {
        // Return the locale name for the entire text.
        // The locale is stored as a null-terminated UTF-16 string.
        if !localename.is_null() {
            // SAFETY: `localename` is non-null (checked above) and
            // points to a caller-provided out-slot valid for one write.
            // `locale_utf16` is NUL-terminated and owned by `self`, so
            // the stored pointer is a valid C-style wide string for as
            // long as DirectWrite holds it during `MapCharacters`.
            unsafe {
                *localename = self.locale_utf16.as_ptr() as *mut u16;
            }
        }
        if !textlength.is_null() {
            // SAFETY: `textlength` is non-null (checked above) and
            // points to a caller-provided out-slot valid for one write.
            unsafe {
                // Report the entire remaining text length as having
                // this locale.
                *textlength = self.text_utf16.len() as u32;
            }
        }
        Ok(())
    }

    fn GetNumberSubstitution(
        &self,
        _textposition: u32,
        textlength: *mut u32,
        numbersubstitution: windows::core::OutRef<'_, IDWriteNumberSubstitution>,
    ) -> windows::core::Result<()> {
        // DirectWrite accepts a null number substitution, meaning
        // "use the default". We write None (null pointer) instead of
        // returning an error, which is the correct behavior per the
        // DirectWrite documentation.
        if !textlength.is_null() {
            // SAFETY: `textlength` is non-null (checked above) and
            // points to a caller-provided out-slot valid for one write.
            unsafe {
                *textlength = 0;
            }
        }
        // Write a null (no number substitution) to the out parameter.
        // This is safe because OutRef::write takes ownership of the
        // value and transmutes it to the ABI representation. For COM
        // interfaces, None represents a null pointer.
        numbersubstitution.write(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directwrite_provider_returns_fallbacks() {
        let Some(provider) = DirectWriteFontFallback::new() else {
            // DirectWrite is unavailable on this host (e.g., a
            // non-Windows CI runner or missing COM); nothing to test.
            return;
        };
        let fallbacks = provider.script_fallbacks(ScriptTag::Latin, "en-us");
        assert!(!fallbacks.is_empty());
    }
}
