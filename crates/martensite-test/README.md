# martensite-test

[![Crates.io](https://img.shields.io/crates/v/martensite-test.svg)](https://crates.io/crates/martensite-test)
[![Documentation](https://docs.rs/martensite-test/badge.svg)](https://docs.rs/martensite-test)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Headless CI test harness, deterministic virtual clock, and synthetic event injection for Martensite.**

---

## Overview

`martensite-test` provides automated testing infrastructure for applications built with the Martensite GUI framework. Testing graphical user interfaces in Continuous Integration (CI) environments is traditionally plagued by non-deterministic timing, flaky test failures caused by `thread::sleep`, and the absence of physical GPU displays on server runners.

`martensite-test` solves these challenges by providing:
- **`VirtualClock`**: A deterministic time simulation engine that advances discrete time increments without wall-clock blocking.
- **Headless Pipeline**: Executes widget lifecycle passes (measure, layout, reactive propagation, and paint command recording) without requiring an OS window or active GPU context.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **Deterministic Virtual Time (`VirtualClock`)**: Step simulations frame-by-frame with exact microsecond precision, completely eliminating race conditions and flaky timing tests.
- **Headless Test Execution**: Run entire integration suites on standard Linux/macOS/Windows headless CI workers.
- **Zero Platform Dependencies**: No requirement for X11, Wayland, or desktop display servers during test runs.
- **100% Safe Rust**: `#![forbid(unsafe_code)]` enforced.

---

## Quick Start

Add `martensite-test` to your `[dev-dependencies]`:

```toml
[dev-dependencies]
martensite-test = "0.7.0"
```

Testing animations and time-dependent logic with `VirtualClock`:

```rust
use martensite_test::VirtualClock;
use std::time::Duration;

fn main() {
    let mut clock = VirtualClock::new();
    assert_eq!(clock.elapsed, Duration::ZERO);

    // Simulate 3 frames of animation at 60 FPS (16.6ms per frame)
    let frame_time = Duration::from_micros(16_666);
    clock.advance(frame_time);
    clock.advance(frame_time);
    clock.advance(frame_time);

    assert_eq!(clock.elapsed, frame_time * 3);
}
```

---

## Part of Martensite

This crate provides testing utilities for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
