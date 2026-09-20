//! Platform rasterizer adapter — wraps
//! `martensite_pdf_platform::SubprocessDocument` as a [`PdfDocument`]
//! and exposes [`CliPdfProvider`], the [`PdfProvider`] the `platform`
//! feature installs as [`default_pdf_provider`].
//!
//! Only compiled with `feature = "platform"`.
//!
//! # Examples
//!
//! ```no_run
//! use martensite_pdf::{CliPdfProvider, PdfProvider, PdfSource};
//! use std::path::PathBuf;
//!
//! let mut p = CliPdfProvider::new();
//! let doc = p.open(&PdfSource::File(PathBuf::from("spec.pdf")))?;
//! assert!(doc.info().page_count > 0);
//! # Ok::<(), martensite_pdf::PdfError>(())
//! ```

use crate::doc::{
    PageSize, PdfDocInfo, PdfDocument, PdfError, PdfPageBitmap, PdfProvider, PdfSource,
};
use martensite_pdf_platform::{self as cli, CliSource};

/// A [`PdfProvider`] that opens documents via the probed CLI
/// rasterizer (`pdftoppm`/`pdfinfo` or `mutool`).
///
/// ```no_run
/// use martensite_pdf::{CliPdfProvider, PdfProvider};
///
/// let p = CliPdfProvider::new();
/// // "poppler"/"mupdf" when a toolset is installed, "subprocess"
/// // when none was found at construction.
/// let _ = p.backend_name();
/// ```
#[derive(Clone, Debug)]
pub struct CliPdfProvider {
    backend: Option<cli::PdfBackend>,
}

impl Default for CliPdfProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CliPdfProvider {
    /// Creates a new [`CliPdfProvider`], probing `$PATH` for the
    /// rasterizer it will report via `backend_name`.
    ///
    /// ```
    /// use martensite_pdf::{CliPdfProvider, PdfProvider};
    ///
    /// let p = CliPdfProvider::new();
    /// assert!(matches!(p.backend_name(), "poppler" | "mupdf" | "subprocess"));
    /// ```
    pub fn new() -> Self {
        Self {
            backend: cli::probe_backend(),
        }
    }
}

impl PdfProvider for CliPdfProvider {
    fn open(&mut self, source: &PdfSource) -> Result<Box<dyn PdfDocument>, PdfError> {
        let src = match source {
            PdfSource::File(p) => CliSource::File(p.clone()),
            PdfSource::Bytes(b) => CliSource::Bytes(b.clone()),
        };
        let doc = cli::open_document(&src).map_err(|e| match e {
            cli::CliError::OpenFailed(m) => PdfError::OpenFailed(m),
            cli::CliError::Unsupported(m) => PdfError::Unsupported(m),
        })?;
        Ok(Box::new(CliDocumentAdapter(doc)))
    }

    fn backend_name(&self) -> &str {
        self.backend.map_or("subprocess", |b| b.name())
    }
}

/// Adapter: concrete [`cli::SubprocessDocument`] → `dyn PdfDocument`.
#[derive(Debug)]
struct CliDocumentAdapter(cli::SubprocessDocument);

impl PdfDocument for CliDocumentAdapter {
    fn info(&self) -> PdfDocInfo {
        let i = self.0.info();
        PdfDocInfo {
            title: i.title,
            author: i.author,
            page_count: i.page_count,
        }
    }

    fn page_size(&self, page: u32) -> Option<PageSize> {
        self.0.page_size(page).map(|s| PageSize {
            width: s.width,
            height: s.height,
        })
    }

    fn render_page(&self, page: u32, max_px: u32) -> Option<PdfPageBitmap> {
        self.0.render_page(page, max_px).map(|b| PdfPageBitmap {
            width: b.width,
            height: b.height,
            pixels: b.pixels,
        })
    }
}
