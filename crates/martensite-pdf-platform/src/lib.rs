//! OS-adjacent PDF rasterizer backends — real page bitmaps with zero
//! unsafe code and no bundled native libraries.
//!
//! Rather than FFI into pdfium/mupdf, this crate drives the mature
//! PDF command-line tools that already exist on the host —
//! `pdfinfo`/`pdftoppm` (poppler) or `mutool` (mupdf) — and decodes
//! their PPM output in safe Rust, the same subprocess contract as
//! `martensite-dialog-platform` and `martensite-share-platform`.
//!
//! Like its siblings this crate defines its own wire types
//! ([`CliDocInfo`], [`CliPageSize`], [`CliBitmap`], [`CliSource`],
//! [`CliError`]) so `martensite-pdf` can depend on it without a
//! dependency cycle; the safe crate adapts them onto `PdfDocument`
//! when its `platform` feature is enabled.
//!
//! - [`probe_backend`] — find poppler/mupdf on `$PATH`
//! - [`open_document`] / [`SubprocessDocument`] — info, page sizes,
//!   and `render_page` → RGBA bitmaps
//! - [`ppm`] — the `P6` decoder shared by both rasterizers
//!
//! # Examples
//!
//! ```no_run
//! use martensite_pdf_platform::{open_document, CliSource};
//! use std::path::PathBuf;
//!
//! let doc = open_document(&CliSource::File(PathBuf::from("spec.pdf")))?;
//! assert!(doc.info().page_count > 0);
//! let bmp = doc.render_page(0, 512).unwrap();
//! assert_eq!(bmp.pixels.len(), (bmp.width * bmp.height * 4) as usize);
//! # Ok::<(), martensite_pdf_platform::CliError>(())
//! ```

#![deny(missing_docs)]

pub mod ppm;
mod provider;

pub use provider::{
    open_document, probe_backend, CliBitmap, CliDocInfo, CliError, CliPageSize, CliSource,
    PdfBackend, SubprocessDocument,
};
