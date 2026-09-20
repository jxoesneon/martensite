//! PDF document model, render contract, and provider trait.
//!
//! This module is platform-agnostic: it defines the wire types every
//! PDF backend produces ([`PdfDocInfo`], [`PageSize`],
//! [`PdfPageBitmap`], [`PdfSource`], [`PdfError`]) and the two traits
//! in the pipeline — [`PdfDocument`] (one opened document) and
//! [`PdfProvider`] (the factory that opens them). `Widget`-side code
//! talks only to this module; rasterizer details live behind the
//! traits.
//!
//! # Examples
//!
//! ```
//! use martensite_pdf::{BlankPdfDocument, PdfDocument};
//!
//! let doc = BlankPdfDocument::new(5);
//! assert_eq!(doc.info().page_count, 5);
//! ```

use std::path::PathBuf;

/// Document-level metadata.
///
/// # Examples
///
/// ```
/// use martensite_pdf::PdfDocInfo;
///
/// let i = PdfDocInfo::default();
/// assert_eq!(i.page_count, 0);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PdfDocInfo {
    /// `Title` from the document info dictionary, when present.
    pub title: Option<String>,
    /// `Author` from the document info dictionary, when present.
    pub author: Option<String>,
    /// Number of pages (`0` for a failed/empty document).
    pub page_count: u32,
}

/// A page's media-box size in PDF points (1/72 inch).
///
/// # Examples
///
/// ```
/// use martensite_pdf::PageSize;
///
/// let s = PageSize::LETTER;
/// assert_eq!(s.width, 612.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSize {
    /// Width in points.
    pub width: f32,
    /// Height in points.
    pub height: f32,
}

impl PageSize {
    /// US Letter — 8.5 × 11 in.
    pub const LETTER: PageSize = PageSize {
        width: 612.0,
        height: 792.0,
    };

    /// ISO A4 — 210 × 297 mm ≈ 595.3 × 841.9 pt.
    pub const A4: PageSize = PageSize {
        width: 595.3,
        height: 841.9,
    };

    /// Aspect ratio `width / height` (`1.0` for degenerate sizes).
    ///
    /// ```
    /// use martensite_pdf::PageSize;
    ///
    /// assert!((PageSize::LETTER.aspect() - 612.0 / 792.0).abs() < 1e-6);
    /// ```
    pub fn aspect(self) -> f32 {
        if self.height > 0.0 {
            self.width / self.height
        } else {
            1.0
        }
    }
}

/// One rasterized page: tightly-packed straight-alpha RGBA8 pixels.
///
/// Matches `martensite_core::ImageData`'s layout so the facade widget
/// can hand it to `PaintList::push_image` without conversion.
///
/// # Examples
///
/// ```
/// use martensite_pdf::PdfPageBitmap;
///
/// let b = PdfPageBitmap {
///     width: 1,
///     height: 1,
///     pixels: vec![255, 255, 255, 255],
/// };
/// assert_eq!(b.pixels.len(), 4);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct PdfPageBitmap {
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// `width * height * 4` RGBA bytes.
    pub pixels: Vec<u8>,
}

/// Where a [`PdfProvider`] reads a document from.
///
/// # Examples
///
/// ```
/// use martensite_pdf::PdfSource;
/// use std::path::PathBuf;
///
/// let s = PdfSource::File(PathBuf::from("/doc.pdf"));
/// assert!(matches!(s, PdfSource::File(_)));
/// ```
#[derive(Clone, Debug)]
pub enum PdfSource {
    /// A file on disk.
    File(PathBuf),
    /// An in-memory PDF byte stream.
    Bytes(Vec<u8>),
}

/// An open/parse/render failure.
///
/// # Examples
///
/// ```
/// use martensite_pdf::PdfError;
///
/// let e = PdfError::Unsupported("no backend".into());
/// assert!(matches!(e, PdfError::Unsupported(_)));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum PdfError {
    /// The document could not be opened or parsed.
    OpenFailed(String),
    /// The backend cannot service this operation.
    Unsupported(String),
    /// A specific page failed to rasterize.
    RenderFailed {
        /// The 0-based page index.
        page: u32,
        /// Error description.
        error: String,
    },
}

impl std::fmt::Display for PdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PdfError::OpenFailed(e) => write!(f, "open failed: {e}"),
            PdfError::Unsupported(e) => write!(f, "unsupported: {e}"),
            PdfError::RenderFailed { page, error } => {
                write!(f, "render failed on page {page}: {error}")
            }
        }
    }
}

impl std::error::Error for PdfError {}

/// One opened PDF document.
///
/// A document owns its page table. All methods take `&self` — real
/// rasterizers (pdfium, mupdf) use interior mutability internally —
/// so the facade widget can call [`render_page`](Self::render_page)
/// from `paint(&self)`. Implementations must be `Send + Sync` to embed
/// in a `Widget`.
///
/// # Examples
///
/// ```
/// use martensite_pdf::{BlankPdfDocument, PdfDocument};
///
/// let doc = BlankPdfDocument::new(2);
/// assert_eq!(doc.info().page_count, 2);
/// assert!(doc.page_size(0).is_some());
/// ```
pub trait PdfDocument: Send + Sync {
    /// Document metadata.
    fn info(&self) -> PdfDocInfo;

    /// The media-box size of `page` (0-based), `None` when out of
    /// range.
    fn page_size(&self, page: u32) -> Option<PageSize>;

    /// Rasterize `page` (0-based) so its longer edge is at most
    /// `max_px` pixels, preserving aspect. `None` means "no raster for
    /// this page" — the caller paints a placeholder.
    ///
    /// ```
    /// use martensite_pdf::{BlankPdfDocument, PdfDocument};
    ///
    /// let doc = BlankPdfDocument::new(1);
    /// let bmp = doc.render_page(0, 256).unwrap();
    /// assert!(bmp.width <= 256 && bmp.height <= 256);
    /// ```
    fn render_page(&self, page: u32, max_px: u32) -> Option<PdfPageBitmap>;
}

/// The factory that opens [`PdfDocument`]s — one per rasterizer
/// backend (pdfium, mupdf, quartz).
///
/// # Examples
///
/// ```
/// use martensite_pdf::{NullPdfProvider, PdfProvider, PdfSource};
///
/// let mut p = NullPdfProvider::new();
/// assert!(p.open(&PdfSource::Bytes(vec![])).is_err());
/// ```
pub trait PdfProvider: Send + Sync {
    /// Open `source` as a document.
    fn open(&mut self, source: &PdfSource) -> Result<Box<dyn PdfDocument>, PdfError>;

    /// Human-readable backend name, e.g. `"pdfium"`, `"null"`.
    fn backend_name(&self) -> &str;
}

/// The provider used when no rasterizer backend is compiled in — every
/// `open` returns [`PdfError::Unsupported`].
///
/// # Examples
///
/// ```
/// use martensite_pdf::{NullPdfProvider, PdfProvider, PdfSource};
///
/// let mut p = NullPdfProvider::new();
/// assert_eq!(p.backend_name(), "null");
/// ```
#[derive(Default, Clone, Debug)]
pub struct NullPdfProvider;

impl NullPdfProvider {
    /// Creates a new [`NullPdfProvider`].
    ///
    /// ```
    /// use martensite_pdf::{NullPdfProvider, PdfProvider};
    ///
    /// assert_eq!(NullPdfProvider::new().backend_name(), "null");
    /// ```
    pub fn new() -> Self {
        Self
    }
}

impl PdfProvider for NullPdfProvider {
    fn open(&mut self, _source: &PdfSource) -> Result<Box<dyn PdfDocument>, PdfError> {
        Err(PdfError::Unsupported(
            "no PDF rasterizer backend available".to_string(),
        ))
    }

    fn backend_name(&self) -> &str {
        "null"
    }
}

/// Returns the best available [`PdfProvider`] for the current build.
///
/// There is deliberately no `martensite-pdf-platform` crate yet — real
/// rasterization needs an FFI engine (pdfium, mupdf) wired to a shared
/// GPU/CPU surface, which no subprocess provides. Until that backend
/// lands this returns [`NullPdfProvider`]; applications get real pages
/// by injecting a provider/document directly (the facade `PdfView`
/// defaults to [`BlankPdfDocument`](crate::BlankPdfDocument) — see the
/// crate docs).
///
/// # Examples
///
/// ```
/// use martensite_pdf::{default_pdf_provider, PdfProvider};
///
/// assert_eq!(default_pdf_provider().backend_name(), "null");
/// ```
pub fn default_pdf_provider() -> Box<dyn PdfProvider> {
    Box::new(NullPdfProvider::new())
}
