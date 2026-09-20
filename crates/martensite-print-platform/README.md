# martensite-print-platform

OS-native print backends for Martensite, driven entirely by subprocesses
(`#![forbid(unsafe_code)]` — no FFI):

- **macOS / Linux** — CUPS: `lpstat` for queue enumeration, `lp` for
  submission (`-d`/`-n`/`-t`/`-P`/`-o`).
- **Windows** — PowerShell: `Get-CimInstance Win32_Printer` for
  enumeration, `Out-Printer` for text payloads, `Start-Process -Verb
  Print` for file payloads.

This crate defines its own `PrintBackend` trait and wire types
(`PrintSpec`, `SpecSource`, `SpecSides`, `PrintReply`, `BackendPrinter`)
so it does not depend on `martensite-print` — the safe crate adapts them
behind `PlatformPrinter` when its `platform` feature is enabled.
