//! A procedural [`PdfDocument`] — N blank pages that rasterize real
//! RGBA pixels (white page, border, header band, deterministic content
//! bars per page index).
//!
//! Unlike `SimulatedWebView`, which produces no surface, the blank
//! document *does* return bitmaps from `render_page`, so the facade
//! `PdfView` exercises the full `PaintList::push_image` path end-to-end
//! — the same call a pdfium/mupdf backend will make once one lands.
//!
//! # Examples
//!
//! ```
//! use martensite_pdf::{BlankPdfDocument, PdfDocument};
//!
//! let doc = BlankPdfDocument::new(3);
//! let bmp = doc.render_page(0, 512).unwrap();
//! assert_eq!(bmp.pixels.len(), (bmp.width * bmp.height * 4) as usize);
//! ```

use crate::doc::{PageSize, PdfDocInfo, PdfDocument, PdfPageBitmap};

/// A document of `n` procedurally drawn pages.
///
/// # Examples
///
/// ```
/// use martensite_pdf::{BlankPdfDocument, PageSize, PdfDocument};
///
/// let doc = BlankPdfDocument::new(2).with_size(PageSize::A4);
/// assert_eq!(doc.page_size(1), Some(PageSize::A4));
/// ```
#[derive(Clone, Debug)]
pub struct BlankPdfDocument {
    pages: u32,
    size: PageSize,
    title: Option<String>,
    author: Option<String>,
}

impl BlankPdfDocument {
    /// A document with `pages` US-Letter pages.
    ///
    /// ```
    /// use martensite_pdf::{BlankPdfDocument, PdfDocument};
    ///
    /// assert_eq!(BlankPdfDocument::new(7).info().page_count, 7);
    /// ```
    pub fn new(pages: u32) -> Self {
        Self {
            pages,
            size: PageSize::LETTER,
            title: None,
            author: None,
        }
    }

    /// Uniform page size for every page.
    ///
    /// ```
    /// use martensite_pdf::{BlankPdfDocument, PageSize, PdfDocument};
    ///
    /// let doc = BlankPdfDocument::new(1).with_size(PageSize::A4);
    /// assert_eq!(doc.page_size(0).unwrap().width, PageSize::A4.width);
    /// ```
    pub fn with_size(mut self, size: PageSize) -> Self {
        self.size = size;
        self
    }

    /// Document title for `info()`.
    ///
    /// ```
    /// use martensite_pdf::{BlankPdfDocument, PdfDocument};
    ///
    /// let doc = BlankPdfDocument::new(1).with_title("Spec");
    /// assert_eq!(doc.info().title.as_deref(), Some("Spec"));
    /// ```
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Document author for `info()`.
    ///
    /// ```
    /// use martensite_pdf::{BlankPdfDocument, PdfDocument};
    ///
    /// let doc = BlankPdfDocument::new(1).with_author("QA");
    /// assert_eq!(doc.info().author.as_deref(), Some("QA"));
    /// ```
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Fill `pixels` with the procedural page pattern for `page`.
    ///
    /// Layout (fractions of the page): 2% gray border, 10% header band,
    /// then `page`-varying content bars (deterministic pseudo-text).
    fn paint_page(&self, page: u32, w: u32, h: u32, pixels: &mut [u8]) {
        let border = (w.min(h) as f32 * 0.02).max(1.0) as u32;
        let header_h = (h as f32 * 0.10) as u32;
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                // White page, light-gray border, header band, bars.
                let px = if x < border || y < border || x >= w - border || y >= h - border {
                    [176, 180, 188, 255]
                } else if y < border + header_h {
                    [226, 230, 238, 255]
                } else {
                    // Content bars: 6% margins, rows every ~8% of body,
                    // length varied by a page-seeded LCG so pages differ.
                    let body_top = border + header_h + (h as f32 * 0.04) as u32;
                    let rel = y.saturating_sub(body_top);
                    let row_h = (h as f32 * 0.08).max(4.0) as u32;
                    let in_row = rel % row_h < row_h / 3;
                    let row = (rel / row_h) as u64;
                    let margin = (w as f32 * 0.06) as u32;
                    // Per-row pseudo-random bar length (deterministic).
                    let seed = (page as u64 + 1)
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(row.wrapping_mul(1442695040888963407));
                    let frac = 0.35 + (seed >> 33) as f32 / u32::MAX as f32 * 0.6;
                    let bar_end = margin + ((w - margin * 2) as f32 * frac) as u32;
                    if in_row && y >= body_top && x >= margin && x < bar_end {
                        [208, 212, 220, 255]
                    } else {
                        [255, 255, 255, 255]
                    }
                };
                pixels[i..i + 4].copy_from_slice(&px);
            }
        }
    }
}

impl PdfDocument for BlankPdfDocument {
    fn info(&self) -> PdfDocInfo {
        PdfDocInfo {
            title: self.title.clone(),
            author: self.author.clone(),
            page_count: self.pages,
        }
    }

    fn page_size(&self, page: u32) -> Option<PageSize> {
        (page < self.pages).then_some(self.size)
    }

    fn render_page(&self, page: u32, max_px: u32) -> Option<PdfPageBitmap> {
        let size = self.page_size(page)?;
        // Scale so the longer edge hits `max_px` (min 8px per side).
        let scale = max_px as f32 / size.width.max(size.height).max(1.0);
        let w = ((size.width * scale) as u32).clamp(8, max_px.max(8));
        let h = ((size.height * scale) as u32).clamp(8, max_px.max(8));
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        self.paint_page(page, w, h, &mut pixels);
        Some(PdfPageBitmap {
            width: w,
            height: h,
            pixels,
        })
    }
}
