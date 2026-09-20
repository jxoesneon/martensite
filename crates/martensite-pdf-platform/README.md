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
| `poppler` | `pdfinfo` + `pdftoppm -ppm` | `brew install poppler` · `apt install poppler-utils` · `choco install poppler` |
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

## License

MIT OR Apache-2.0
