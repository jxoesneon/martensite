//! PDF document/render contract plus a procedural test backend — the
//! API surface a rasterizer (`pdfium`, `mutool`, Quartz) implements.
//!
//! The crate itself is pure-safe; real rasterization arrives through
//! the `platform` feature, which adapts
//! `martensite-pdf-platform`'s CLI backends (`pdftoppm`/`pdfinfo`
//! poppler, `mutool` mupdf) onto `CliPdfProvider`. The *contract*
//! is what's reusable:
//!
//! - [`PdfDocument`] — one opened document: metadata, page sizes,
//!   `render_page` → RGBA bitmaps (takes `&self`; backends use
//!   interior mutability so a widget can render inside `paint(&self)`)
//! - [`PdfProvider`] — the factory that opens [`PdfSource`]s
//!   (files or byte streams) into `Box<dyn PdfDocument>`
//! - `CliPdfProvider` — the CLI rasterizer (`platform` feature)
//! - [`NullPdfProvider`] — the stub the factory returns when no
//!   rasterizer is available
//! - [`BlankPdfDocument`] — a procedural document that rasterizes real
//!   pixels, so tests and the facade `PdfView` exercise the full
//!   `PaintList::push_image` path
//!
//! # Examples
//!
//! ```
//! use martensite_pdf::{BlankPdfDocument, PdfDocument, PageSize};
//!
//! let doc = BlankPdfDocument::new(3).with_size(PageSize::A4);
//! assert_eq!(doc.info().page_count, 3);
//!
//! let bmp = doc.render_page(1, 300).unwrap();
//! assert!(bmp.width <= 300 && bmp.height <= 300);
//! ```

#![deny(missing_docs)]

mod blank;
mod doc;
#[cfg(feature = "platform")]
mod platform;

pub use blank::BlankPdfDocument;
pub use doc::{
    default_pdf_provider, NullPdfProvider, PageSize, PdfDocInfo, PdfDocument, PdfError,
    PdfPageBitmap, PdfProvider, PdfSource,
};
#[cfg(feature = "platform")]
pub use platform::CliPdfProvider;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn blank_doc_info_and_sizes() {
        let doc = BlankPdfDocument::new(4).with_title("T").with_author("A");
        let info = doc.info();
        assert_eq!(info.page_count, 4);
        assert_eq!(info.title.as_deref(), Some("T"));
        assert_eq!(doc.page_size(0), Some(PageSize::LETTER));
        assert_eq!(doc.page_size(4), None);
    }

    #[test]
    fn blank_doc_rasterizes_within_max_px() {
        let doc = BlankPdfDocument::new(2);
        let bmp = doc.render_page(0, 256).unwrap();
        assert!(bmp.width <= 256 && bmp.height <= 256);
        assert_eq!(bmp.pixels.len(), (bmp.width * bmp.height * 4) as usize);
        // Aspect preserved: letter is portrait.
        assert!(bmp.height > bmp.width);
        // Not a flat fill — border pixels differ from center.
        let center = ((bmp.height / 2) * bmp.width + bmp.width / 2) as usize * 4;
        assert_ne!(&bmp.pixels[center..center + 4], &bmp.pixels[0..4]);
        assert!(doc.render_page(5, 256).is_none());
    }

    #[test]
    fn pages_rasterize_differently() {
        let doc = BlankPdfDocument::new(3);
        let a = doc.render_page(0, 128).unwrap();
        let b = doc.render_page(1, 128).unwrap();
        assert_ne!(a.pixels, b.pixels);
    }

    #[test]
    #[cfg(not(feature = "platform"))]
    fn null_provider_rejects() {
        let mut p = default_pdf_provider();
        assert_eq!(p.backend_name(), "null");
        assert!(matches!(
            p.open(&PdfSource::File(PathBuf::from("/x.pdf"))),
            Err(PdfError::Unsupported(_))
        ));
    }

    #[test]
    #[cfg(feature = "platform")]
    fn cli_provider_reports_error() {
        let mut p = default_pdf_provider();
        // Rasterizer installed → concrete name; none → the null stub.
        assert!(matches!(p.backend_name(), "poppler" | "mupdf" | "null"));
        // Either way a missing file fails: Unsupported (no CLI) or
        // OpenFailed (CLI present, file absent).
        assert!(matches!(
            p.open(&PdfSource::File(PathBuf::from("/x.pdf"))),
            Err(PdfError::Unsupported(_)) | Err(PdfError::OpenFailed(_))
        ));
    }
}
