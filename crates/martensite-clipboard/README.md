# martensite-clipboard

[![Crates.io](https://img.shields.io/crates/v/martensite-clipboard.svg)](https://crates.io/crates/martensite-clipboard)
[![Documentation](https://docs.rs/martensite-clipboard/badge.svg)](https://docs.rs/martensite-clipboard)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Multi-MIME delayed-rendering clipboard engine supporting text, HTML, images, and lazy payload evaluation.**

---

## Overview

`martensite-clipboard` provides a platform-agnostic clipboard data architecture for the Martensite GUI framework. In modern operating systems (Windows OLE, macOS Cocoa `NSPasteboard`, Linux Wayland `wl_data_device`, and X11), copying complex content requires offering multiple representations of the same logical data (e.g. plain text, HTML, and rich images simultaneously).

Furthermore, copying massive datasets (e.g., gigabyte spreadsheets or high-resolution images) should not allocate or encode formats eagerly. `martensite-clipboard` implements **lazy delayed rendering**, deferring payload encoding until a target application actually requests that specific MIME format upon paste, with built-in deadline timeouts to guard against unresponsive producers.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **Multi-MIME Clipboard Items (`ClipboardItem`)**: Declare multiple simultaneous formats for a single copy action (`offer_text`, `offer_html`, `offer_rtf`, `offer_png`, `offer_custom`).
- **Lazy Delayed Rendering (`ClipboardPayload::Lazy`)**: Defers expensive data encoding closures until the moment of paste.
- **Deadline Guardrails (`with_deadline`)**: Protects the clipboard event loop from hanging by bounding lazy generation times.
- **Pure-Rust Testing Service (`InMemoryClipboard`)**: Mockable, thread-safe clipboard backend ideal for unit tests and headless continuous integration.
- **100% Safe Rust**: `#![forbid(unsafe_code)]` strictly enforced.

---

## Quick Start

Add `martensite-clipboard` to your `Cargo.toml`:

```toml
[dependencies]
martensite-clipboard = "0.7.0"
```

Offering multiple formats with lazy generation:

```rust
use martensite_clipboard::{
    ClipboardItem, ClipboardPayload, ClipboardService, InMemoryClipboard,
};

fn main() {
    let mut clipboard = InMemoryClipboard::new();

    // Copy rich content with multiple representations
    let item = ClipboardItem::new()
        .offer_text("Hello, World!")
        .offer_html("<b>Hello, World!</b>")
        .offer_custom("application/x-custom", ClipboardPayload::lazy(|| {
            // Expensive encoding work only executes if requested
            b"custom-payload-bytes".to_vec()
        }));

    clipboard.set_contents(&item);

    // Paste plain text
    let pasted_text = clipboard.get_text().expect("plain text available");
    assert_eq!(pasted_text, "Hello, World!");
}
```

---

## Cargo Feature Flags

| Feature | Description | Default |
| :--- | :--- | :--- |
| `default` | Standard clipboard service implementations. | Yes |
| `wayland` | Enables Linux Wayland data device protocol integration. | Yes |

---

## Part of Martensite

This crate provides system clipboard management for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
