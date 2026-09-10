//! OS-native font fallback providers for Martensite.
//!
//! This crate implements the [`FontFallbackProvider`] trait from
//! `martensite-text` using platform-specific native APIs:
//!
//! - **Windows**: DirectWrite `IDWriteFontFallback::MapCharacters`
//! - **macOS**: CoreText `CTFontCreateForStringWithLanguage`
//! - **Linux**: Fontconfig `FcFontSort`
//!
//! # Safety policy
//!
//! This crate uses `#![allow(unsafe_code)]` at the crate level because it
//! contains platform-specific FFI to DirectWrite (Windows COM), CoreText
//! (macOS), and Fontconfig (Linux). The workspace-level `unsafe_code =
//! "deny"` policy is preserved for all other crates; this is the narrowly
//! scoped audited exception described in the v0.11.0 boundary decision.
//!
//! All `unsafe` blocks in this crate are confined to platform-specific
//! modules (`directwrite`, `coretext`, `fontconfig`) and are audited
//! against the upstream API documentation:
//! - DirectWrite: <https://learn.microsoft.com/en-us/windows/win32/api/dwrite_2/nf-dwrite_2-idwritefontfallback-mapcharacters>
//! - CoreText: <https://developer.apple.com/documentation/coretext/ctfontcreateforstringwithlanguage>
//! - Fontconfig: <https://freedesktop.org/software/fontconfig/fontconfig-devel/fcfontsort.html>

#![allow(unsafe_code)]
#![forbid(missing_docs)]

use martensite_text::cascade::FontFallbackProvider;

#[cfg(target_os = "windows")]
pub mod directwrite;

#[cfg(target_os = "macos")]
pub mod coretext;

#[cfg(target_os = "linux")]
pub mod fontconfig;

/// Returns the best native [`FontFallbackProvider`] for the current
/// platform, or `None` if no native provider is available.
///
/// On Windows this returns a [`DirectWriteFontFallback`](directwrite::DirectWriteFontFallback).
/// On macOS this returns a [`CoreTextFontFallback`](coretext::CoreTextFontFallback).
/// On Linux this returns a [`FontconfigFontFallback`](fontconfig::FontconfigFontFallback).
///
/// # Examples
///
/// ```
/// use martensite_font_fallback::native_provider;
///
/// if let Some(provider) = native_provider() {
///     let fallbacks = provider.script_fallbacks(
///         martensite_text::cascade::ScriptTag::Cjk,
///         "zh-cn",
///     );
///     assert!(!fallbacks.is_empty());
/// }
/// ```
#[allow(rustdoc::broken_intra_doc_links)]
pub fn native_provider() -> Option<Box<dyn FontFallbackProvider>> {
    #[cfg(target_os = "windows")]
    {
        return Some(Box::new(directwrite::DirectWriteFontFallback::new()));
    }
    #[cfg(target_os = "macos")]
    {
        // `needless_return` would fire on other platforms where only one
        // branch is compiled, but the `return` is required here because
        // multiple cfg-gated branches exist in the source.
        #[allow(clippy::needless_return)]
        return Some(Box::new(coretext::CoreTextFontFallback::new()));
    }
    #[cfg(target_os = "linux")]
    {
        #[allow(clippy::needless_return)]
        return Some(Box::new(fontconfig::FontconfigFontFallback::new()));
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        #[allow(clippy::needless_return)]
        return None;
    }
}
