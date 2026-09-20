# martensite-share

Share / reveal-in-folder abstraction for Martensite.

`ShareRequest` describes one share — `url` (opened in the platform
handler: `https:`, `mailto:`, `tel:`, custom schemes), `text` (drafted
as a mailto message), `files`, and `subject` — and `ShareService` is
the dispatch contract, with `reveal` for showing files in the platform
file manager. `ScriptedShare` is a deterministic canned-response
backend for tests and headless environments.

The platform share *sheet* (NSSharingService, WinRT DataTransferManager)
needs in-process UI integration; the subprocess backends in
`martensite-share-platform` route shares through URI handlers instead,
behind the `platform` Cargo feature. Without the feature,
`default_platform_share` returns a safe `StubShare`; this crate is
`#![forbid(unsafe_code)]`.
