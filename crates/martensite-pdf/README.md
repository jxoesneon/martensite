# martensite-pdf

PDF document/render contract for the Martensite GUI framework — the API
surface a real rasterizer backend (pdfium, mupdf, Quartz) implements.

This crate is deliberately **pure-safe**: no FFI, no platform pair.
Real PDF rasterization requires an embedded engine wired to a shared
GPU/CPU surface, which no subprocess provides, so a
`martensite-pdf-platform` crate is deferred until a real backend lands.

## What's here

| Type | Role |
| --- | --- |
| `PdfDocument` | One opened document — `info()`, `page_size()`, `render_page()` → RGBA bitmaps. All `&self` so a widget can render inside `paint(&self)`; backends use interior mutability. |
| `PdfProvider` | Factory that opens `PdfSource::File` / `PdfSource::Bytes` into `Box<dyn PdfDocument>`. |
| `NullPdfProvider` | Stub returned by `default_pdf_provider()` today — every `open` fails `Unsupported`. |
| `BlankPdfDocument` | Procedural N-page document that rasterizes **real pixels** (border, header band, per-page content bars) — exercises the full `push_image` paint path in tests and the facade `PdfView`. |
| `PdfDocInfo` / `PageSize` / `PdfPageBitmap` / `PdfError` / `PdfSource` | Wire types shared by every backend. |

## Usage

```rust
use martensite_pdf::{BlankPdfDocument, PdfDocument, PageSize};

let doc = BlankPdfDocument::new(3).with_size(PageSize::A4);
assert_eq!(doc.info().page_count, 3);

let bmp = doc.render_page(0, 512).unwrap();
assert_eq!(bmp.pixels.len(), (bmp.width * bmp.height * 4) as usize);
```

The facade `martensite::widgets::PdfView` wraps any `PdfDocument` and
defaults to `BlankPdfDocument`, so the scaffold is visible immediately.

## License

MIT OR Apache-2.0
