# martensite-clipboard-platform

OS-native clipboard backends for Martensite.

This crate implements the `PlatformClipboard` trait from `martensite-clipboard`
using platform-specific native APIs:

- **macOS**: `NSPasteboard` via the Objective-C runtime
- **Windows**: Win32 clipboard API (`OpenClipboard`, `SetClipboardData`, …)
- **Linux**: X11 `CLIPBOARD` selection via raw `libX11` FFI

## Safety policy

This crate uses `#![allow(unsafe_code)]` at the crate level because it contains
platform-specific FFI. The workspace-level `unsafe_code = "deny"` policy is
preserved for all other crates; this is the narrowly scoped audited exception,
mirroring the `martensite-font-fallback` crate's boundary decision.
