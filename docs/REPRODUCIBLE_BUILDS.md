# Reproducible Builds

**Document Identifier:** DOC-REPRO-BUILDS
**Status:** Documented procedure; cross-runner bit-for-bit verification
not yet implemented (milestone v0.18.0 §4.7).

This document records what Martensite does and does not guarantee about
build reproducibility, and how to verify it locally.

## What is pinned

- **Dependency graph.** `Cargo.lock` is committed. `cargo build
  --locked` refuses to build against any other resolution, so every
  build of a given commit uses byte-identical crate sources.
- **Toolchain channel.** `rust-toolchain.toml` pins `channel =
  "stable"` with `rustfmt`/`clippy`/`rust-src` components and the
  cross-compile targets. Caveat: `stable` is a *moving* channel — two
  builds on different dates may use different rustc point releases. For
  bit-for-bit comparison, pin a specific version
  (e.g. `channel = "1.xx.y"`) or record `rustc --version` alongside the
  artifact.
- **Release profile.** `[profile.release]` sets `opt-level = 3`,
  `codegen-units = 1`, `lto = "thin"`, `strip = true`. Single
  codegen-unit + thin LTO removes codegen-unit parallelism as a source
  of binary variance, and `strip` removes symbol/debug sections that
  embed absolute paths.
- **Linkers.** `.cargo/config.toml` pins mold (Linux x86_64), lld
  (Windows MSVC), and the platform linker on macOS — the linker choice
  affects the binary, so it is part of the pinned configuration.

## What still varies

Bit-for-bit reproducibility is **not currently claimed**. Known sources
of variance between otherwise identical builds:

- **Linker build-id.** ELF linkers may embed a build-id whose scheme
  differs by linker and flags (mold's default is content-derived; GNU
  ld/lld defaults differ). Compare with the build-id excluded or
  normalized (`readelf -n`, `ld64`'s `-no_uuid` on macOS) before
  hashing.
- **Embedded paths.** `panic!`/`file!` messages embed paths relative to
  the workspace root, so the *checkout location* can appear in
  binaries. `--remap-path-prefix` is not set; builds from different
  checkout paths are not expected to match.
- **Proc-macro and env inputs.** `env!`, `vergen`-style build-time
  env reads, and proc-macro output can embed time/host data. No
  Martensite crate embeds a build timestamp, but this is asserted by
  inspection, not a lint.
- **Platform toolchains.** The same commit built on Linux vs macOS vs
  Windows produces different binaries by construction — reproducibility
  is only meaningful per-target.

## CI's approach

There is **no CI binary-artifact workflow**. The project publishes
source to crates.io via `publish.yml`; consumers compile from source.
CI therefore verifies *buildability* per platform (`target-checks` for
wasm32/ios-sim/android, the platform matrix in `ci.yml`), not binary
equivalence across runners. The v0.18.0 §4.7 cross-runner
bit-for-bit check is not yet implemented; when it lands it should live
in a dedicated workflow that builds `cargo build --release --locked` on
each OS runner and compares post-normalization SHA-256 digests.

## Verifying locally

```sh
# Two clean builds from the same commit and toolchain:
cargo clean && cargo build --release --locked -p martensite
sha256sum target/release/libmartensite.rlib > /tmp/build1.sha
cargo clean && cargo build --release --locked -p martensite
sha256sum target/release/libmartensite.rlib > /tmp/build2.sha
diff /tmp/build1.sha /tmp/build2.sha
```

Expected result: archives are **not** guaranteed identical today —
archive member timestamps and object ordering can differ even when
codegen is deterministic. A mismatch is not by itself a defect; a match
is stronger evidence. For executables, strip or normalize the build-id
before comparing.

## Supply-chain relationship

Reproducibility complements (not replaces) the existing supply-chain
gates: `cargo vet` audits, `cargo deny` policy, and the vendored-fork
policy in `docs/VENDORED_FORKS.md` establish *what* code ships; this
document covers whether two builds of that code produce the same
bytes.
