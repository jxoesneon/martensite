# martensite-pdf

PDF document/render contract for the Martensite GUI framework — plus
real rasterization via the `platform` feature.

The crate itself is **pure-safe** (no FFI). Real page bitmaps arrive
through `martensite-pdf-platform`, which drives the PDF CLIs already
on the host — `pdftoppm`/`pdfinfo` (poppler) or `mutool` (mupdf) —
and is adapted onto `CliPdfProvider` when `platform` is enabled.

## What's here

| Type | Role |
| --- | --- |
| `PdfDocument` | One opened document — `info()`, `page_size()`, `render_page()` → RGBA bitmaps. All `&self` so a widget can render inside `paint(&self)`; backends use interior mutability. |
| `PdfProvider` | Factory that opens `PdfSource::File` / `PdfSource::Bytes` into `Box<dyn PdfDocument>`. |
| `CliPdfProvider` | *(feature `platform`)* Real rasterizer backed by poppler/mupdf CLIs. |
| `NullPdfProvider` | Stub `default_pdf_provider()` returns when no rasterizer is available — `open` reports `Unsupported`. |
| `BlankPdfDocument` | Procedural N-page document that rasterizes **real pixels** — exercises the full `push_image` path in tests and the facade `PdfView`. |
| `PdfDocInfo` / `PageSize` / `PdfPageBitmap` / `PdfError` / `PdfSource` | Wire types shared by every backend. |

## Usage

```rust
use martensite_pdf::{BlankPdfDocument, PdfDocument, PageSize};

let doc = BlankPdfDocument::new(3).with_size(PageSize::A4);
assert_eq!(doc.info().page_count, 3);

let bmp = doc.render_page(0, 512).unwrap();
assert_eq!(bmp.pixels.len(), (bmp.width * bmp.height * 4) as usize);
```

With the `platform` feature:

```rust,ignore
use martensite_pdf::{default_pdf_provider, PdfProvider, PdfSource};

let mut provider = default_pdf_provider(); // poppler or mupdf
let doc = provider.open(&PdfSource::File("spec.pdf".into()))?;
```

The facade `martensite::widgets::PdfView` wraps any `PdfDocument`,
defaults to `BlankPdfDocument`, and offers `PdfView::open` for the
provider path — so the scaffold is visible immediately and real
documents work when a CLI toolset is installed.

## License

MIT OR Apache-2.0
