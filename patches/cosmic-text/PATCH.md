# Patched `cosmic-text` 0.19.0

This is a local copy of `cosmic-text` 0.19.0 with one change: the `fontdb`
dependency has been bumped from `0.23` to `0.24`. `fontdb` 0.24 removes its
transitive dependency on the unmaintained `ttf-parser` crate (RUSTSEC-2026-0192).

Upstream `cosmic-text` 0.19.0 pins `fontdb` to `^0.23`, so this patch is used
via `[patch.crates-io]` in the workspace root until a new `cosmic-text` release
is available.

## Changes from upstream

- `Cargo.toml`: `fontdb` version `0.23` → `0.24`
- Removed non-library files (tests, benches, examples, bundled fonts, scripts,
  screenshots) to keep the patch minimal.
