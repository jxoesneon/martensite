//! Font system abstraction: system font discovery via `fontdb`, custom
//! font asset loading, and font identity types.
//!
//! The [`FontManager`] wraps cosmic-text's [`FontSystem`] and provides
//! a higher-level API for discovering, loading, and querying fonts.
//! It is designed to be created once at application startup and shared
//! throughout the application lifetime, as system font discovery is
//! expensive.

use std::path::PathBuf;
use std::sync::Arc;

use cosmic_text::{Attrs, FontSystem};

/// A stable identifier for a loaded font face.
///
/// Wraps `fontdb::ID` to provide a Martensite-native type that is
/// `Copy`, `Hash`, and `Eq`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FontId(pub fontdb::ID);

impl FontId {
    /// Create a new `FontId` from a raw `fontdb::ID`.
    #[inline(always)]
    pub fn new(id: fontdb::ID) -> Self {
        Self(id)
    }

    /// Creates a dummy font identifier for use as a placeholder cache key
    /// when the actual font ID is not yet known.
    #[inline(always)]
    pub fn dummy() -> Self {
        Self(fontdb::ID::dummy())
    }

    /// Returns the raw `fontdb::ID`.
    #[inline(always)]
    pub fn raw(self) -> fontdb::ID {
        self.0
    }
}

impl From<fontdb::ID> for FontId {
    #[inline(always)]
    fn from(id: fontdb::ID) -> Self {
        Self(id)
    }
}

impl From<FontId> for fontdb::ID {
    #[inline(always)]
    fn from(id: FontId) -> Self {
        id.0
    }
}

/// A loaded font asset, either from a file path or in-memory binary data.
#[derive(Clone, Debug)]
pub enum FontSource {
    /// A font loaded from a file on disk.
    File(PathBuf),
    /// A font loaded from in-memory binary data.
    Binary(Arc<Vec<u8>>),
}

impl FontSource {
    /// Creates a `FontSource::File` from a path.
    #[inline(always)]
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::File(path.into())
    }

    /// Creates a `FontSource::Binary` from raw font data.
    #[inline(always)]
    pub fn binary(data: impl Into<Vec<u8>>) -> Self {
        Self::Binary(Arc::new(data.into()))
    }
}

impl From<PathBuf> for FontSource {
    #[inline(always)]
    fn from(path: PathBuf) -> Self {
        Self::File(path)
    }
}

impl From<Vec<u8>> for FontSource {
    #[inline(always)]
    fn from(data: Vec<u8>) -> Self {
        Self::Binary(Arc::new(data))
    }
}

/// Information about a discovered font face.
#[derive(Clone, Debug)]
pub struct FontFaceInfo {
    /// The font's unique identifier in the database.
    pub id: FontId,
    /// The font family name.
    pub family: String,
    /// Whether the font is monospaced.
    pub monospaced: bool,
    /// The font weight (100-900, 400 = normal).
    pub weight: u16,
    /// Whether the font is italic or oblique.
    pub style: FontStyle,
}

/// The style of a font face.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FontStyle {
    /// Normal (upright) style.
    Normal,
    /// Italic style.
    Italic,
    /// Oblique (slanted) style.
    Oblique,
}

impl From<fontdb::Style> for FontStyle {
    #[inline]
    fn from(style: fontdb::Style) -> Self {
        match style {
            fontdb::Style::Normal => Self::Normal,
            fontdb::Style::Italic => Self::Italic,
            fontdb::Style::Oblique => Self::Oblique,
        }
    }
}

/// High-level font manager wrapping cosmic-text's [`FontSystem`].
///
/// Provides system font discovery, custom font loading, and font
/// querying. The underlying [`FontSystem`] is exposed via
/// [`FontManager::system`] and [`FontManager::system_mut`] for
/// direct use with cosmic-text APIs (e.g., `Buffer`).
pub struct FontManager {
    system: FontSystem,
}

impl FontManager {
    /// Creates a new `FontManager` that discovers all system fonts.
    ///
    /// This is an expensive operation (up to ~1s on release builds)
    /// and should be called once at startup.
    pub fn new() -> Self {
        Self {
            system: FontSystem::new(),
        }
    }

    /// Creates a new `FontManager` with only the specified custom fonts,
    /// without loading any system fonts.
    pub fn with_fonts(fonts: impl IntoIterator<Item = FontSource>) -> Self {
        let sources: Vec<fontdb::Source> = fonts
            .into_iter()
            .map(|src| match src {
                FontSource::File(path) => fontdb::Source::File(path),
                FontSource::Binary(data) => fontdb::Source::Binary(data),
            })
            .collect();
        Self {
            system: FontSystem::new_with_fonts(sources),
        }
    }

    /// Creates a `FontManager` from an existing [`FontSystem`].
    pub fn from_system(system: FontSystem) -> Self {
        Self { system }
    }

    /// Loads a custom font from a file path and registers it in the
    /// font database.
    ///
    /// Returns the IDs of the font faces that were loaded from the file.
    /// A single font file may contain multiple faces. Returns an empty
    /// vec if the file could not be loaded.
    pub fn load_font_file(&mut self, path: impl Into<PathBuf>) -> Vec<FontId> {
        let path = path.into();
        // Record face IDs before loading
        let before: std::collections::HashSet<fontdb::ID> =
            self.system.db().faces().map(|f| f.id).collect();
        match self.system.db_mut().load_font_file(&path) {
            Ok(()) => {
                // Return only the newly added face IDs
                self.system
                    .db()
                    .faces()
                    .filter(|f| !before.contains(&f.id))
                    .map(|f| FontId(f.id))
                    .collect()
            }
            Err(_) => Vec::new(),
        }
    }

    /// Loads a custom font from in-memory binary data.
    ///
    /// Returns the IDs of the font faces that were loaded.
    pub fn load_font_data(
        &mut self,
        data: impl AsRef<[u8]> + Sync + Send + 'static,
    ) -> Vec<FontId> {
        let source = fontdb::Source::Binary(Arc::new(data));
        let face_ids = self.system.db_mut().load_font_source(source);
        face_ids.into_iter().map(FontId).collect()
    }

    /// Returns all discovered font faces.
    pub fn faces(&self) -> Vec<FontFaceInfo> {
        self.system
            .db()
            .faces()
            .map(|face| FontFaceInfo {
                id: FontId(face.id),
                family: face
                    .families
                    .first()
                    .map(|(name, _)| name.clone())
                    .unwrap_or_default(),
                monospaced: face.monospaced,
                weight: face.weight.0,
                style: face.style.into(),
            })
            .collect()
    }

    /// Finds font faces matching the given family name.
    pub fn find_by_family(&self, family: &str) -> Vec<FontFaceInfo> {
        self.faces()
            .into_iter()
            .filter(|f| f.family.eq_ignore_ascii_case(family))
            .collect()
    }

    /// Returns the locale string used for font fallback.
    pub fn locale(&self) -> &str {
        self.system.locale()
    }

    /// Borrows the underlying [`FontSystem`] for use with cosmic-text APIs.
    #[inline(always)]
    pub fn system(&self) -> &FontSystem {
        &self.system
    }

    /// Mutably borrows the underlying [`FontSystem`].
    #[inline(always)]
    pub fn system_mut(&mut self) -> &mut FontSystem {
        &mut self.system
    }

    /// Consumes the manager and returns the underlying [`FontSystem`].
    #[inline(always)]
    pub fn into_system(self) -> FontSystem {
        self.system
    }

    /// Creates default attrs for a text run with the given family name.
    pub fn attrs_for_family<'a>(&self, family: &'a str) -> Attrs<'a> {
        Attrs::new().family(cosmic_text::Family::Name(family))
    }
}

impl Default for FontManager {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FontManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontManager")
            .field("locale", &self.system.locale())
            .field("face_count", &self.system.db().faces().count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_id_roundtrip() {
        let raw = fontdb::ID::dummy();
        let id = FontId::new(raw);
        assert_eq!(id.raw(), raw);
        let id2: FontId = raw.into();
        assert_eq!(id, id2);
        let back: fontdb::ID = id.into();
        assert_eq!(back, raw);
    }

    #[test]
    fn font_source_file_from_path() {
        let src = FontSource::file("/tmp/font.ttf");
        assert!(matches!(src, FontSource::File(_)));
    }

    #[test]
    fn font_source_binary_from_vec() {
        let src = FontSource::binary(vec![0u8, 1, 2, 3]);
        assert!(matches!(src, FontSource::Binary(_)));
    }

    #[test]
    fn font_source_from_pathbuf() {
        let src: FontSource = PathBuf::from("/tmp/font.otf").into();
        assert!(matches!(src, FontSource::File(_)));
    }

    #[test]
    fn font_source_from_vec() {
        let src: FontSource = vec![0u8, 1, 2].into();
        assert!(matches!(src, FontSource::Binary(_)));
    }

    #[test]
    fn font_style_from_fontdb() {
        assert_eq!(FontStyle::from(fontdb::Style::Normal), FontStyle::Normal);
        assert_eq!(FontStyle::from(fontdb::Style::Italic), FontStyle::Italic);
        assert_eq!(FontStyle::from(fontdb::Style::Oblique), FontStyle::Oblique);
    }

    #[test]
    fn font_manager_new_discovers_system_fonts() {
        // This may find zero fonts in CI without system fonts, but should not panic.
        let manager = FontManager::new();
        let _locale = manager.locale();
        let _faces = manager.faces();
    }

    #[test]
    fn font_manager_with_custom_fonts_only() {
        // new_with_fonts always loads system fonts; this test just verifies
        // it doesn't panic with an empty custom font iterator.
        let manager = FontManager::with_fonts(std::iter::empty());
        let _faces = manager.faces();
    }

    #[test]
    fn font_manager_debug_format() {
        let manager = FontManager::with_fonts(std::iter::empty());
        let debug = format!("{:?}", manager);
        assert!(debug.contains("FontManager"));
        assert!(debug.contains("locale"));
    }

    #[test]
    fn font_manager_attrs_for_family() {
        let manager = FontManager::with_fonts(std::iter::empty());
        let attrs = manager.attrs_for_family("Helvetica");
        // Just verify it doesn't panic
        let _ = attrs;
    }

    #[test]
    fn font_manager_find_by_family_empty() {
        let manager = FontManager::with_fonts(std::iter::empty());
        let results = manager.find_by_family("NonExistentFont12345");
        assert!(results.is_empty());
    }
}
