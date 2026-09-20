# martensite-persist

[![Crates.io](https://img.shields.io/crates/v/martensite-persist.svg)](https://crates.io/crates/martensite-persist)
[![Documentation](https://docs.rs/martensite-persist/badge.svg)](https://docs.rs/martensite-persist)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Key-value settings/state persistence — JSON file store, memory store, per-OS config dirs.**

---

## Overview

`martensite-persist` is the canonical "remember my settings" layer for Martensite applications:

- **`StateStore`** — the `get` / `set` / `remove` / `keys` / `flush` contract over `serde_json::Value`.
- **`MemoryStore`** — volatile `BTreeMap` backend for tests and session-scoped state.
- **`JsonFileStore`** — a single JSON object file, loaded lazily, written atomically (tmp + rename) on `flush`.
- **`paths`** — per-OS config directory resolution (`%APPDATA%` / `Application Support` / `$XDG_CONFIG_HOME`) without a `dirs` dependency.

The entire crate is built under `#![forbid(unsafe_code)]`.

## Example

```rust
use martensite_persist::{JsonFileStore, StateStore, default_store_path};

let path = default_store_path("acme", "app").unwrap_or_else(|| "/tmp/app.json".into());
let mut store = JsonFileStore::open(path).unwrap();
store.set("theme", "dark");
store.flush().unwrap();
```

---

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
