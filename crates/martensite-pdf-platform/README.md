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
| `mupdf` | `mutool info` + `mutool draw` | `brew install mupdf-tools` · `apt install mupdf-tools` |

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
- **MuPDF `page_size` probe cost:** `mutool info` emits a
  deduplicated `Mediaboxes` list that can't be mapped to page
  numbers, so the mupdf size probe rasterizes the page at 72 dpi
  (px == pt) and reads only the PPM header. Correct but expensive —
  a full raster per first probe per page. The cheaper alternative
  is `mutool pages` (per-page boxes as metadata, no render); it was
  not adopted because its output format varies across mutool
  versions and couldn't be verified in the dev environment. Revisit
  once verified — see the `KNOWN COST` note in
  `src/provider.rs`'s `read_page_size`.
- Pages arrive as P6-PPM pixels — vector-sharp in the CLI's own
  pipeline, but raster by the time this crate sees them.

## License

MIT OR Apache-2.0
