# martensite-pdf-platform

OS-adjacent PDF rasterizer backends for `martensite-pdf` — real page
bitmaps with **zero unsafe code** and no bundled native libraries.

Rather than FFI into pdfium/mupdf, this crate drives the mature PDF
command-line tools that already exist on the host and decodes their
PPM output in safe Rust — the same subprocess contract as
`martensite-dialog-platform` and `martensite-share-platform`.

## Backends (probed on `$PATH`, poppler preferred)

| Backend | Tools | Install |
| --- | --- | --- |
| `poppler` | `pdfinfo` + `pdftoppm` (PPM is the default format) | `brew install poppler` · `apt install poppler-utils` · `choco install poppler` |
| `mupdf` | `mutool info` + `mutool show` + `mutool draw` | `brew install mupdf-tools` · `apt install mupdf-tools` |

## Usage

```rust,ignore
use martensite_pdf_platform::{CliSource, SubprocessDocument};

let doc = SubprocessDocument::open(&CliSource::File("spec.pdf".into()))?;
let bmp = doc.render_page(0, 1024)?; // RGBA8 bitmap
```

`SubprocessDocument::open` probes `$PATH` for a toolset and returns
`Unsupported` when neither is installed. With `martensite-pdf`'s
`platform` feature enabled, `martensite_pdf::default_pdf_provider()`
returns a `CliPdfProvider` over this crate (or `NullPdfProvider`
when no CLI is found) — callers of the facade don't touch this
crate directly.

`CliSource::Bytes` sources are materialized to a private
(`0600`, `create_new`) temp file and removed on drop — including
when open fails partway.

## Limitations

- Each `page_size`/`render_page` call is a subprocess (bounded by a
  timeout); `SubprocessDocument` caches page sizes, and the facade
  adapter caches rendered bitmaps — but the first call per page
  still pays CLI startup.
- **MuPDF `page_size` probe:** metadata-first — one
  `mutool show -g <file> pages grep` subprocess reads the whole
  object table once per document, resolving each page's effective
  `MediaBox`/`CropBox`/`Rotate` up its `/Parent` chain (matching
  `pdf_lookup_inherited_page_item`) plus the page's own `UserUnit`,
  i.e. exactly what `mutool draw` rasterizes — verified end-to-end
  against mupdf-tools 1.28.4. Pages the object table can't size
  (no `MediaBox` in the ancestor chain, degenerate dims mupdf
  clamps to a unit rect, missing object) fall back to rasterizing
  at 72 dpi (px == pt) and reading only the PPM header; pages whose
  metadata resolves to an absurd size (>100k pt) are reported
  unsized rather than rasterized. `mutool info` is never used for
  sizes — its `Mediaboxes` list is deduplicated and can't be
  mapped to page numbers.
- **Poppler `page_size`:** `pdfinfo -f N -l N` reports the raw
  crop box; the `rot:` line is applied so sizes match the rotated
  `pdftoppm` raster (and the mupdf backend). `pdfinfo` does not
  report `/UserUnit` — a rare UserUnit-scaled page sizes
  differently across backends (poppler ignores it; mupdf scales).
- Pages arrive as P6-PPM pixels — vector-sharp in the CLI's own
  pipeline, but raster by the time this crate sees them.

## License

MIT OR Apache-2.0
