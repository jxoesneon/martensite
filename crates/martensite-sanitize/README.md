# martensite-sanitize

[![Crates.io](https://img.shields.io/crates/v/martensite-sanitize.svg)](https://crates.io/crates/martensite-sanitize)
[![Documentation](https://docs.rs/martensite-sanitize/badge.svg)](https://docs.rs/martensite-sanitize)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Configurable input sanitization engine — Unicode hygiene, normalization, and confusable folding for editable widgets.**

---

## Overview

`martensite-sanitize` is the input-hygiene layer for every editable text surface in the Martensite GUI framework. Pasted and typed text routinely carries invisible hazards — bidirectional overrides (the Trojan-Source vector), zero-width payloads, noncharacters, exotic space codepoints, and compatibility forms that defeat validation and equality checks. This crate funnels all text ingestion through a single configurable pipeline so widgets sanitize consistently at both insertion (typing, IME commit, paste, programmatic `set_value`) and commit (submit, send, token creation) boundaries.

---

## Key Features

- **Tri-state widget API**: `sanitize(true)` selects the `Aggressive` profile (the default), `sanitize(false)` selects `Baseline` structural hygiene, and `raw()` disables rewriting entirely.
- **Custom rules**: `with_sanitizer(Arc<dyn Sanitize>)` replaces the built-in profile with a caller-supplied trait object; `SanitizerConfig` exposes `is_raw()`/`is_custom()` for introspection.
- **Phase-aware pipeline**: `Phase::Insert` rewrites incoming text; `Phase::Commit` additionally trims. Single-line contexts collapse separators per `SanitizeContext`.
- **Aggressive profile**: strips C0/C1 controls, bidi overrides/isolates, noncharacters, invisible math operators, tag-block characters, and the BOM; applies NFKC; folds ASCII-adjacent confusables (primes, modifier apostrophes); maps exotic Unicode spaces to `U+0020`. Functional invisibles (ZWJ, ZWNJ, ZWSP) are preserved by default to keep emoji-family sequences and Indic/Persian shaping intact; the `Profile` builder can tighten this.
- **Idempotent**: `sanitize(sanitize(x)) == sanitize(x)` — safe to run at both ingestion seams.

---

## Example

```rust
use std::sync::Arc;
use martensite_sanitize::{SanitizerConfig, SanitizeContext, Phase};

let ctx = SanitizeContext { phase: Phase::Commit, single_line: true };

// Default: comprehensive profile.
let aggressive = SanitizerConfig::default();
assert_eq!(aggressive.sanitize("  a\u{202e}b  ", &ctx), "ab");

// Baseline keeps text verbatim except control characters.
let baseline = SanitizerConfig::Baseline;
assert_eq!(baseline.sanitize("ＡＢＣ", &ctx), "ＡＢＣ");

// Raw passes everything through untouched.
let raw = SanitizerConfig::Raw;
assert_eq!(raw.sanitize(" \u{202e} ", &ctx), " \u{202e} ");
```

---

## Part of Martensite

This crate provides input sanitization for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
