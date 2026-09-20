# martensite-share-platform

OS-native share backends for Martensite, driven entirely by
subprocesses (`#![forbid(unsafe_code)]` — no FFI):

- **macOS** — `open <uri>` for URL/mailto dispatch, `open -R` for
  Finder reveal.
- **Linux** — `xdg-open` or `gio open` for dispatch and parent-folder
  reveal, selected by `PATH` probing.
- **Windows** — `cmd /c start` for dispatch, `explorer /select,` for
  Explorer reveal.

The platform share sheet needs in-process UI integration, so `files`
shares report `ShareReply::Unsupported` — use `reveal` for
file-manager presentation. Text shares draft a `mailto:` URI.

This crate defines its own `ShareBackend` trait and wire types
(`ShareSpec`, `ShareReply`) so it does not depend on `martensite-share`
— the safe crate adapts them behind `PlatformShare` when its `platform`
feature is enabled.
