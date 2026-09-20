# martensite-dialog-platform

[![Crates.io](https://img.shields.io/crates/v/martensite-dialog-platform.svg)](https://crates.io/crates/martensite-dialog-platform)
[![Documentation](https://docs.rs/martensite-dialog-platform/badge.svg)](https://docs.rs/martensite-dialog-platform)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **OS-native file dialog backends (osascript, PowerShell WinForms, zenity/kdialog) for `martensite-dialog`.**

---

## Overview

This crate provides the real platform backends for [`martensite-dialog`](https://crates.io/crates/martensite-dialog). It intentionally does **not** depend on `martensite-dialog` — it defines its own `DialogBackend` trait and wire types (`DialogSpec`, `SpecKind`, `DialogReply`), which the safe crate adapts behind its `PlatformDialog` trait when the `platform` feature is enabled.

## Backends

| Target | Backend | Mechanism |
|--------|---------|-----------|
| macOS | `OsascriptDialog` | `osascript` — AppleScript `choose file` / `choose folder` / `choose file name` (real `NSOpenPanel`/`NSSavePanel`) |
| Windows | `WinFormsDialog` | PowerShell `System.Windows.Forms` — `OpenFileDialog` / `SaveFileDialog` / `FolderBrowserDialog` |
| Linux | `ZenityDialog` / `KdialogDialog` | `zenity --file-selection` (GTK) or `kdialog` (Qt), selected by `PATH` probe |

## Safety policy

Unlike the other `martensite-*-platform` crates, this crate is `#![forbid(unsafe_code)]`: every backend drives the platform's dialog facility through a short-lived subprocess, so no FFI or unsafe code is required — mirroring the `wl-clipboard` subprocess backend in `martensite-clipboard-platform`.

`native_backend()` returns `None` on unsupported targets and on Linux systems with neither `zenity` nor `kdialog` on `PATH`; callers fall back to `martensite_dialog::StubDialog`.

---

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
