# Vendored-Fork Maintenance Policy

**Document Identifier:** DOC-VENDORED-FORKS
**Status:** Maintained
**Applies to:** `martensite-vello`, `martensite-cosmic-text`,
`martensite-accesskit-winit` (vendored upstream sources); the
`martensite-bevy` git pin; the `naga` 29/30 duplicate.

Martensite vendors three upstream crates in-tree and pins one git
dependency. This document records why each exists, how it is
maintained, and the conditions under which it is removed — per
milestone v0.18.0 §4.5. Retention with an explicit policy is an
accepted outcome; silent drift is not.

## Inventory

| Crate | Upstream | Vendored version | Why it exists | Exit criterion |
|---|---|---|---|---|
| `martensite-vello` | `netrender-vello` 0.10.0 (byte-compatible republish of Vello 0.10) | `0.10.0-martensite.1` | Official `vello` 0.10 pins `wgpu` 29, incompatible with the workspace's wgpu 30 as a single type; vendoring also removes a single-maintainer republish from the supply chain | Upstream `vello` releases a build against the workspace's wgpu major, or a maintained republish does |
| `martensite-cosmic-text` | `cosmic-text` | `0.19.0-martensite.1` | Updated `fontdb` dependency (0.24) ahead of upstream | Upstream `cosmic-text` release with `fontdb` 0.24 |
| `martensite-accesskit-winit` | `accesskit_winit` 0.34.0 | `0.17.0` | Patched for winit 0.31.0-beta.3 (`&dyn ActiveEventLoop` / `&dyn Window` trait-object signatures) | **Temporary.** Remove once upstream `accesskit_winit` supports winit 0.31 stable |

## Patch discipline

1. **Minimal-diff rule.** Vendored crates carry only the changes the
   workspace needs (dependency rebases, API shims). Feature work and
   refactorings do not land in vendored code — they belong upstream or
   in a first-party crate.
2. **Version suffix.** Vendored releases use the
   `<upstream-version>-martensite.<n>` scheme so the provenance is
   visible in `Cargo.lock` and on crates.io.
3. **Lint exemptions are deliberate and per-crate.**
   `martensite-vello` and `martensite-cosmic-text` carry
   `#![allow(unsafe_code)]`, `#![allow(missing_docs)]`,
   `#![allow(clippy::all)]`, and
   `#![allow(rustdoc::broken_intra_doc_links)]`;
   `martensite-accesskit-winit` carries only `#![allow(unsafe_code)]`
   (for the platform-adapter FFI upstream requires). These are the only
   vendored exemptions — upstream code, not Martensite API surface — and
   no new exemptions may be added.
4. **License retention.** Upstream `LICENSE-*` files ship in each
   vendored crate directory and must not be removed.
5. **Local patches are documented in the crate README** (why the fork
   exists, what was changed relative to upstream).

## Re-sync cadence

- **Tracked upstream releases are reviewed at every minor Martensite
  release** (the version-surface pass documented in
  `docs/VERSION_UPDATE_SURFACE.md`): check whether upstream has shipped
  the change the fork exists for; if yes, plan the exit.
- **Security advisories** against an upstream crate trigger an
  immediate re-sync or backport decision, recorded in the milestone's
  supply-chain notes.
- **No unsolicited rebases.** Re-syncing upstream feature work that the
  exit criterion does not require is deferred — each re-sync is review
  load against code we intend to delete.

## `naga` 29/30 duplication

The workspace uses `naga` 30 (via wgpu 30). `naga` 29.0.4 enters the
graph through `vello_shaders` 0.10.0, a *registry* dependency of
`martensite-vello` (naga 29 is a build-dependency for shader
translation). Two naga majors in the lockfile is accepted interim
state: dedup is **blocked on upstream** — either `vello_shaders`
ships a naga-30 build, or `vello` itself is replaced per the exit
criterion above. Vendoring `vello_shaders` to force the dedup is an
option but expands the vendored surface; it is deferred unless the
duplication produces a concrete failure (e.g. `cargo deny` bans
violation). Re-check at every wgpu upgrade.

## `martensite-bevy` git pin

`martensite-bevy` depends on Bevy **main** at rev
`5036d978a294a3fbb1c42bf005d6a255e2978a74` — the "Upgrade to wgpu 30"
merge, the first Bevy revision whose `bevy_render` builds against the
workspace's wgpu 30.

- **Policy:** a pinned `rev` (never a branch or floating ref). The pin
  moves only when a newer upstream rev is required for a concrete fix
  or wgpu bump; each move is a deliberate, reviewed change.
- **Exit criterion:** switch to a released `bevy` version as soon as
  upstream ships a crates.io release whose render stack targets the
  workspace's wgpu major. Until then `martensite-bevy` stays
  `publish = false` — git dependencies cannot be published.
- **Risk:** tracking main means upstream breaks can appear on any pin
  move; mitigation is to move the pin only when necessary and to run
  the full verification checklist after each move.

## Adding a new vendored fork

A new vendored crate requires all of the following, documented in the
PR that introduces it:

1. The concrete upstream gap it closes (version pin, missing platform
   support, supply-chain concern).
2. A named exit criterion.
3. README provenance + license files + `-martensite.<n>` version
   suffix.
4. The lint-exemption justification (matching §Patch discipline).
5. A `cargo vet`/`cargo deny` note if the vendoring removes a registry
   dependency.
