# cargo-martensite

[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Developer CLI toolchain for sub-second hot-reloading, shader validation, and asset compilation for Martensite.**

---

## Overview

`cargo-martensite` is the official developer CLI toolchain for the Martensite GUI framework. It streamlines the developer feedback loop by orchestrating live asset hot-reloading, ahead-of-time WGSL shader compilation, and cross-platform native bundle distribution.

---

## Toolchain Capabilities

- **Live Code & Asset Reloading (`run --watch`)**: Monitors project directories, asset manifests, and WGSL shader files to trigger sub-second live state updates during UI development.
- **AOT Shader Compilation & Naga Reflection**: Validates all embedded WGSL compute/render shaders against target platform GPU limitations at build time.
- **Asset Bundle Packaging (`bundle`)**: Packs textures, fonts, and Fluent localization files into compact zero-copy `EmbeddedVfs` byte archives for production release binaries.
- **Project Scaffolding (`new`)**: Generates optimized Martensite application templates configured with recommended rendering, reactive, and layout presets.

---

## Part of Martensite

This tool is part of the [Martensite](https://github.com/jxoesneon/martensite) GUI framework workspace.

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
