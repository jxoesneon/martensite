# martensite-notify-platform

[![Crates.io](https://img.shields.io/crates/v/martensite-notify-platform.svg)](https://crates.io/crates/martensite-notify-platform)
[![Documentation](https://docs.rs/martensite-notify-platform/badge.svg)](https://docs.rs/martensite-notify-platform)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **OS-native notification backends (osascript, notify-send, PowerShell toast) for `martensite-notify`.**

---

## Overview

This crate provides the real platform backends for [`martensite-notify`](https://crates.io/crates/martensite-notify). It intentionally does **not** depend on `martensite-notify` — it defines its own `NotifyBackend` trait and wire types (`NotifySpec`, `SpecUrgency`), which the safe crate adapts behind its `PlatformNotifier` trait when the `platform` feature is enabled.

## Backends

| Target | Backend | Mechanism |
|--------|---------|-----------|
| macOS | `OsascriptNotifier` | `osascript` `display notification` (Notification Center) |
| Windows | `ToastNotifier` | PowerShell `Windows.UI.Notifications` `ToastText02` |
| Linux | `NotifySend` | freedesktop `notify-send` (urgency + `sound-name` hint) |

## Safety policy

This crate is `#![forbid(unsafe_code)]`: every backend drives the platform's notification facility through a short-lived subprocess, so no FFI or unsafe code is required — mirroring the `wl-clipboard` subprocess backend in `martensite-clipboard-platform`.

`native_backend()` returns `None` on unsupported targets and on Linux systems without `notify-send` on `PATH`; callers fall back to `martensite_notify::StubNotifier`.

---

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
