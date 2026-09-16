# Deprecation & MSRV Policy

**Status:** Active from v0.18.0
**Applies to:** all publishable workspace crates (see
`docs/API_SURFACE_AUDIT.md` for the enumerated surface and stability tiers)

This document defines how Martensite deprecates and removes public API, and
how the Minimum Supported Rust Version (MSRV) is managed. It complements
`docs/RELEASE_PROCESS.md` (versioning, release mechanics) and
`docs/API_FREEZE_AUDIT.md` (lint enforcement, unsafe exceptions).

## 1. Marking a Deprecation

A public item (function, type, trait, module, constant, or feature) is
deprecated by applying `#[deprecated]` with **both** fields populated:

```rust
#[deprecated(
    since = "0.19.0",
    note = "use `widget::DataGrid` instead; DataTable is removed in v1.0.0"
)]
```

Rules:

- `since` is the version **in which the deprecation first ships** (the next
  release, not the current development version).
- `note` always names the replacement API, or states explicitly that the
  item is removed without replacement and why.
- The deprecation is recorded in `CHANGELOG.md` under `Deprecated` in the
  release that introduces it — this starts the notice-period clock.
- Items classified EXPERIMENTAL in `docs/API_SURFACE_AUDIT.md` may change
  without a deprecation cycle while the workspace is pre-1.0 (see §3), but
  a `Deprecated` changelog entry is still required so downstream users get
  notice.
- Deprecating a whole **feature flag**: mark it in the crate's feature
  documentation and changelog; keep the flag compiling (possibly as an
  empty alias) until removal.

## 2. Minimum Notice Period

The notice period is the time between the release that introduces the
deprecation and the release that may remove the item.

- **Pre-1.0 (current, 0.x):** one full minor release cycle. An item
  deprecated in `0.x.0` may be removed no earlier than `0.(x+1).0`.
  Because 0.x minor releases may contain breaking changes per SemVer
  §4, this is a courtesy window, not a hard guarantee — but it is the
  project default and exceptions require a changelog `Changed` entry
  calling out the early removal.
- **Post-1.0:** two full minor release cycles, with a floor of 6 months of
  calendar time, whichever is longer. An item deprecated in `1.x.0` may be
  removed no earlier than `1.(x+2).0` and no sooner than 6 months after the
  `1.x.0` release date. Removal never happens in a patch release.

## 3. Removal Rules

### Pre-1.0

- Removing a STABLE-classified item without a prior deprecation cycle is
  allowed (0.x SemVer) but must be called out under `Removed` in the
  changelog with a migration note when a replacement exists.
- EXPERIMENTAL items (vendored-fork surface, feature-gated subsystems, and
  the areas flagged in `docs/API_SURFACE_AUDIT.md`) may be changed or
  removed in any minor release; the changelog entry is the only required
  notice.
- `#[non_exhaustive]` additions (new enum variants, new struct fields) are
  not breaking and need no deprecation.

### Post-1.0

- A STABLE item may only be removed after completing the §2 notice period,
  and only in a release that also updates `docs/MIGRATION_GUIDE_0x_to_1x.md`
  or the successor migration document.
- EXPERIMENTAL items added post-1.0 must ship behind an `unstable-*`
  feature flag (see `docs/API_FREEZE_AUDIT.md` §3) and carry
  the same removal freedom as pre-1.0 experimental API.
- `cargo-semver-checks` (the `semver-checks` CI job) must confirm that the
  version under release satisfies the removal — i.e., removal of a
  non-deprecated stable item post-1.0 requires a major bump and never
  ships on the 1.x line.

## 4. MSRV Policy

- **Current MSRV:** `1.89.0` — declared once in
  `[workspace.package].rust-version` in the root `Cargo.toml` and enforced
  by CI toolchains resolving at or above it.
- **Bump policy** (per `docs/RELEASE_PROCESS.md` §6): MSRV bumps are
  treated as **minor version bumps** — an MSRV increase may only ship in
  an `0.x.0` release (post-1.0: `1.x.0`), never in a patch release.
- **Notice:** a 6-month public notice in `CHANGELOG.md` precedes any
  planned MSRV bump, per `docs/RELEASE_PROCESS.md` §6.
- **Release notes:** GitHub Release notes must list the MSRV explicitly
  (`docs/RELEASE_PROCESS.md` §5).
- New crates added to the workspace inherit the workspace MSRV; they may
  not declare a higher `rust-version` without following the bump policy.

## 5. Audit Trail

Every deprecation, removal, and MSRV bump is verifiable in three places:

1. `CHANGELOG.md` — `Deprecated`/`Removed` sections and the MSRV notice.
2. `docs/API_SURFACE_AUDIT.md` — regenerated per release; deprecated items
   are listed under their crate (rustdoc `deprecation` markers).
3. Git history — the commit introducing `#[deprecated]` and the commit
   removing the item.
