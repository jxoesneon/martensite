# [ADR-0005] The Zero C/C++ FFI Cross-Compilation Mandate

* **Status:** Accepted
* **Date:** 2026-09-06
* **Deciders:** Master (Sovereign Architect), Ciel (Infrastructure & Security Guild)
* **Technical Domain:** Workspace Supply Chain, `deny.toml`, CI Matrix

## Context and Problem Statement

The defining failure of enterprise C++ GUI toolkits (Qt, wxWidgets) and multi-language hybrids is build-time fragility. Introducing native C/C++ dependencies into a Rust dependency tree forces every downstream user to install CMake, Python, LLVM/Clang, Perl, MSVC build tools, or platform sysroots (`libasound2-dev`, `libfontconfig1-dev`, `libx11-dev`).

This causes immediate failure modes:
1. **Cross-Compilation Impossibility**: Building a Windows binary from Linux or macOS fails because native C dependencies cannot link without cross-compilation toolchains and headers.
2. **Build Non-Determinism**: Minor differences in host operating system header versions cause obscure compile errors or security vulnerabilities.
3. **Supply Chain Vulnerabilities**: C dependencies bypass Rust's memory safety guarantees, introducing memory corruption and buffer overflow risks.

We must decide whether to permit convenient C/C++ dependencies in our supply chain or mandate absolute 100% pure Rust.

## Decision Drivers

* **Instant Zero-Config Cross-Compilation**: `cargo build --target x86_64-pc-windows-gnu` must succeed on a clean Linux machine without installing foreign sysroots.
* **100% Memory Safety**: All code in the dependency tree must be governed by Rust's borrow checker.
* **Deterministic Hermetic Builds**: Continuous integration must guarantee bit-for-bit reproducible compilation.

## Considered Options

* **Option 1**: Allow system C libraries for font handling (`freetype`, `fontconfig`) and windowing (`X11`, `Wayland-client`).
* **Option 2**: Compile bundled C/C++ sources via `cc` and `cmake-rs`.
* **Option 3**: **Strict Pure-Rust Cross-Compilation Mandate Enforced by `cargo-deny`**.

## Decision Outcome

Chosen option: **Option 3**, enforcing an absolute ban on native C/C++ build tools and FFI sys crates across the entire repository.

### Positive Consequences

* **Automated CI Enforcement**: A specialized `deny.toml` configuration strictly bans `cc`, `cmake`, `pkg-config`, `openssl-sys`, `freetype-sys`, and `fontconfig-sys`. Any PR introducing a C build tool fails CI immediately.
* **Pure-Rust Substitutions**:
  - Fonts: `cosmic-text` (`rustybuzz` + `swash` + `fontdb`).
  - Graphics: `wgpu` + `vello` (pure Rust shading and compute).
  - Accessibility: `accesskit` + `zbus` (pure D-Bus IPC without `libatspi.so`).
* **Universal Hermetic Packaging**: Applications built on Martensite can be compiled for any desktop or WebAssembly target with standard `cargo build`.
