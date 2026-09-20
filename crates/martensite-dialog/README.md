# martensite-dialog

[![Crates.io](https://img.shields.io/crates/v/martensite-dialog.svg)](https://crates.io/crates/martensite-dialog)
[![Documentation](https://docs.rs/martensite-dialog/badge.svg)](https://docs.rs/martensite-dialog)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Native file/folder dialog abstraction — open, multi-open, save-as, and pick-folder with platform backends.**

---

## Overview

`martensite-dialog` provides a platform-agnostic model for the four canonical filesystem dialogs so widgets and application code never touch platform APIs directly:

- **`FileDialogRequest`** describes one invocation: a `DialogKind` (`OpenFile` / `OpenFiles` / `PickFolder` / `SaveFile`), window title, starting directory, `FileFilter` set, and a suggested filename for save dialogs.
- **`DialogOutcome`** is `Cancelled` or `Picked(paths)`; `path()` / `paths()` accessors cover the common cases.
- **`DialogService`** is the blocking show contract. `ScriptedDialog` is a deterministic canned-response implementation for tests and headless environments.
- **`PlatformDialog`** extends `DialogService` with a backend name. `default_platform_dialog()` selects the best available backend for the current target.

The entire crate is built under `#![forbid(unsafe_code)]`.

## Platform backends

With the `platform` Cargo feature, `default_platform_dialog()` delegates to `martensite-dialog-platform`, which drives the platform's own dialog facility via subprocess — no FFI required:

| Target | Backend | Mechanism |
|--------|---------|-----------|
| macOS | `macos-osascript` | AppleScript `choose file` / `choose folder` / `choose file name` (real `NSOpenPanel`/`NSSavePanel`) |
| Windows | `windows-forms` | PowerShell `System.Windows.Forms` dialogs |
| Linux | `linux-zenity` / `linux-kdialog` | GTK/Qt chooser via `zenity`/`kdialog` on `PATH` |

Without the feature, all backends are safe stubs reporting `DialogOutcome::Cancelled`.

## Example

```rust
use martensite_dialog::{DialogService, FileDialogRequest, FileFilter, ScriptedDialog};
use std::path::PathBuf;

let mut dialogs = ScriptedDialog::new(); // or default_platform_dialog()
dialogs.respond_with(
    martensite_dialog::DialogOutcome::Picked(vec![PathBuf::from("/a.png")]),
);

let req = FileDialogRequest::open_file()
    .title("Choose an image")
    .filter(FileFilter::new("Images", ["png", "jpg"]));
let out = dialogs.show(&req);
assert_eq!(out.path().unwrap().to_str().unwrap(), "/a.png");
```

---

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
