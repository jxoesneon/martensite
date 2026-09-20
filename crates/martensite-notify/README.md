# martensite-notify

[![Crates.io](https://img.shields.io/crates/v/martensite-notify.svg)](https://crates.io/crates/martensite-notify)
[![Documentation](https://docs.rs/martensite-notify/badge.svg)](https://docs.rs/martensite-notify)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **OS notification abstraction — title, body, subtitle, urgency, sound — with platform backends.**

---

## Overview

`martensite-notify` provides a platform-agnostic notification model so widgets and application code never touch platform APIs directly:

- **`Notification`** describes one delivery (title, body, subtitle, `Urgency`, sound name).
- **`NotifyService`** is the one-shot delivery contract; failures surface as `NotifyError`. `ScriptedNotifier` is a recording implementation for tests and headless environments.
- **`PlatformNotifier`** extends `NotifyService` with a backend name. `default_platform_notifier()` selects the best available backend for the current target.

The entire crate is built under `#![forbid(unsafe_code)]`.

## Platform backends

With the `platform` Cargo feature, `default_platform_notifier()` delegates to `martensite-notify-platform`, which drives the platform's notification facility via subprocess — no FFI required:

| Target | Backend | Mechanism |
|--------|---------|-----------|
| macOS | `macos-osascript` | `display notification` (Notification Center) |
| Windows | `windows-toast` | PowerShell `Windows.UI.Notifications` toast |
| Linux | `linux-notify-send` | freedesktop `notify-send` |

Without the feature, all backends are safe stubs that discard notifications.

## Example

```rust
use martensite_notify::{Notification, NotifyService, ScriptedNotifier, Urgency};

let mut notifier = ScriptedNotifier::new(); // or default_platform_notifier()
notifier
    .notify(&Notification::new("Export done").urgency(Urgency::Normal))
    .unwrap();
assert_eq!(notifier.sent().len(), 1);
```

---

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
