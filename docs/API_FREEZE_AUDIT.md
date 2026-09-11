# API Freeze Audit — v0.11.0

## Status: FROZEN for v0.11.0

This document records the API freeze audit performed after the v0.11.0
release. All public APIs in the workspace have been reviewed for stability,
documentation coverage, and safety.

## Lint Enforcement

### Workspace-level lints (`[workspace.lints.rust]`)

```toml
[workspace.lints.rust]
unsafe_code = "deny"
missing_docs = "deny"
```

All workspace crates inherit these lints via `[lints] workspace = true`.

### Crate-level overrides

#### `#![forbid(unsafe_code)]` (strictest — cannot be overridden)

All crates except the six audited FFI exceptions below use
`#![forbid(unsafe_code)]` at the crate level.

#### `#![allow(unsafe_code)]` (audited FFI exceptions)

Six crates use `#![allow(unsafe_code)]` for platform-specific FFI or
vendored upstream code. These are the ONLY crates in the workspace that
allow unsafe code:

1. **`martensite-font-fallback`** — platform FFI (DirectWrite, CoreText, Fontconfig)
2. **`martensite-clipboard-platform`** — OS clipboard FFI (NSPasteboard, Win32, X11)
3. **`martensite-media-platform`** — hardware video surface import FFI (IOSurface, DXGI, dmabuf)
4. **`martensite-host`** — dynamic library loading (libloading/LoadLibrary)
5. **`martensite-cosmic-text`** — vendored upstream fork (cosmic-text), also has `#![allow(missing_docs)]`
6. **`martensite-accesskit-winit`** — vendored upstream fork of accesskit_winit 0.34.0

### `#![deny(missing_docs)]` enforcement

All publishable crates now enforce `#![deny(missing_docs)]` either at the
crate level or via the workspace lint. The only exception is
`martensite-cosmic-text` which has `#![allow(missing_docs)]` due to its
vendored upstream fork status.

## `#[non_exhaustive]` Usage

The following crates use `#[non_exhaustive]` on public enums/structs to
prevent breaking changes when new variants/fields are added:

- `martensite-host` (1 item)
- `martensite-wgpu` (4 items across resilience, surface, device)
- `martensite-dnd` (7 items across platform, bridge)
- `martensite-clipboard` (2 items)
- `martensite-window` (4 items across manager, event)
- `martensite-assets` (1 item)
- `martensite-plugin` (1 item)

## Feature Gates

### Existing feature gates

- `martensite-render`: `vello` (enables Vello GPU backend)
- `martensite-media`: `test-noop` (enables wgpu noop backend for tests)
- `martensite-clipboard`: `platform` (enables platform clipboard integration)
- `martensite`: `docs-rs` (metadata feature for docs.rs builds)
- Various crates: `docs-rs` (metadata feature)

### Unstable API gating

No `unstable-*` feature gates are currently needed. All public APIs in
v0.11.0 are considered stable for the 0.x semver range. APIs that may
change in future releases are:

1. **`martensite-wgpu` recovery/resilience APIs** — `RecoveryMachine`,
   `RecoveryHarness`, `RecoveryOutcome` are new in v0.11.0 and may evolve
   based on real-world device-loss recovery feedback. These are marked
   `#[non_exhaustive]` where applicable.

2. **`martensite-media-platform` import functions** — `import_iosurface`,
   `import_dxgi_texture`, `import_dmabuf` are platform-specific and may
   change as the wgpu HAL API evolves.

3. **`martensite-plugin` runtime APIs** — `PluginRuntime`, `PluginInstance`
   are new in v0.11.0 and may evolve as the plugin ecosystem matures.

4. **`martensite-blessed` widget APIs** — `AudioWaveform`, `CodeEditor`,
   `DataTable`, `Chart` are new in v0.11.0 and may evolve based on usage.

These APIs will be stabilized in v1.0.0 after real-world validation. No
`unstable-*` feature gates are added in v0.11.0 because the entire 0.x
series is semver-unstable by definition.

## Semver Policy

- **0.x releases**: Breaking changes are allowed with minor version bumps.
  All public APIs are subject to change.
- **1.0.0 and beyond**: Breaking changes require major version bumps.
  `#[non_exhaustive]` is used to allow non-breaking additions.

## Audit Date

2026-09-10
