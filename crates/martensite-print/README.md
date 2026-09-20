# martensite-print

Printer enumeration and print-job submission abstraction for Martensite.

`PrintJob` describes one submission — payload (`PrintSource::File` or
`Bytes` + MIME hint), destination printer, copies, page range, paper
size, orientation, duplex, and color mode — and `PrinterService` is the
blocking submit/enumerate contract. `ScriptedPrinter` is a deterministic
canned-response backend for tests and headless environments.

Native printing is delegated to `martensite-print-platform` behind the
`platform` Cargo feature (CUPS `lp`/`lpstat` on macOS and Linux,
PowerShell on Windows). Without the feature, `default_platform_printer`
returns a safe `StubPrinter`; this crate is `#![forbid(unsafe_code)]`.
