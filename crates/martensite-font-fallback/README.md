# martensite-font-fallback

OS-native font fallback providers for Martensite.

This crate implements the `FontFallbackProvider` trait from `martensite-text`
using platform-specific native APIs:

- **Windows**: DirectWrite `IDWriteFontFallback::MapCharacters`
- **macOS**: CoreText `CTFontCreateForStringWithLanguage`
- **Linux**: Fontconfig `FcFontSort`

## Safety policy

This crate uses `#![allow(unsafe_code)]` at the crate level because it contains
platform-specific FFI to DirectWrite (Windows COM), CoreText (macOS), and
Fontconfig (Linux). The workspace-level `unsafe_code = "deny"` policy is
preserved for all other crates; this is the narrowly scoped audited exception
described in the v0.11.0 boundary decision.
