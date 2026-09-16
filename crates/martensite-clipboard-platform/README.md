# martensite-clipboard-platform

OS-native clipboard backends for Martensite.

This crate implements the `PlatformClipboard` trait from `martensite-clipboard`
using platform-specific native APIs:

- **macOS**: `NSPasteboard` via the Objective-C runtime
- **Windows**: Win32 clipboard API (`OpenClipboard`, `SetClipboardData`, …)
- **Linux**: X11 `CLIPBOARD` selection via raw `libX11` FFI, plus a Wayland
  backend implemented via the `wl-clipboard` command-line utility
  (`wl-copy`/`wl-paste`); it is selected automatically when
  `WAYLAND_DISPLAY` is set and the utility is installed, otherwise the
  factory falls back to X11 (which also covers XWayland).

The Wayland backend delegates to `wl-clipboard` rather than implementing
the `wl_data_device` protocol inline: doing so would require driving a
`wl_display` roundtrip and an event queue inside a stateless library crate,
pulling in `wayland-client` + `calloop` and duplicating connection state
already owned by the application's windowing layer. `wl-clipboard` is the
standard lightweight mechanism used by toolkits in the same position
(`wl-clipboard-rs`, Alacritty's `copypasta`, etc. use the same protocol
through a persistent helper). If `wl-clipboard` is absent, X11/XWayland
clipboard remains functional.

## Safety policy

This crate uses `#![allow(unsafe_code)]` at the crate level because it contains
platform-specific FFI. The workspace-level `unsafe_code = "deny"` policy is
preserved for all other crates; this is the narrowly scoped audited exception,
mirroring the `martensite-font-fallback` crate's boundary decision.
