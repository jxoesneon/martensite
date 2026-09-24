# Version Update Surface

Every location that carries a Martensite version number, classified by what
must happen on each release. Automation lives in `scripts/`:

- **`scripts/bump-version.sh [X.Y.Z]`** — with an argument, sets
  `[workspace.package].version` then syncs every *auto-bump* surface below;
  without an argument, repairs drift by syncing all surfaces to the current
  workspace version.
- **`scripts/check-version-consistency.sh`** — verifies all *auto-bump* and
  *verify* surfaces; exits non-zero on drift. Runs in CI on every push.

The invariant both scripts enforce: **every recorded version for a
first-party crate equals that crate's effective `[package].version`** —
literal, or the workspace version when `version.workspace = true`. Pinned
vendored forks are therefore handled correctly by the same rule.

---

## 1. Auto-bump surfaces (synced by `bump-version.sh`)

| Surface | Pattern | Notes |
| :--- | :--- | :--- |
| `Cargo.toml` `[workspace.package]` | `version = "X.Y.Z"` | Canonical version. Only rewritten when the script gets an explicit argument. |
| `Cargo.toml` `[workspace.dependencies]` | `name = { version = "V", path = "P" }` | Each `V` synced to `P`'s `[package].version`. Covers `accesskit_winit` (package-aliased to `martensite-accesskit-winit`, workspace-tracked) and the pinned `vello`/`cosmic-text` entries (synced to their independent pins — no-ops). |
| `crates/martensite-bevy/Cargo.toml` | `[package].version` + `martensite-*` dep versions | Excluded from default workspace; literal versions. |
| `crates/martensite-godot/Cargo.toml` | same | Excluded; literal versions. |
| `examples/viewport_showcase/Cargo.toml` | same | Excluded; literal versions. |
| `README.md` | `martensite = "0.19.0"` install snippet | Root install instructions. |
| `crates/*/README.md` | `martensite-* = "X.Y.Z"` install snippets | Publish to crates.io — the most visible drift surface (was 0.7.0/0.14.0 at 0.17.0). |
| `docs/*.md` | `martensite* = "V"` snippets inside code fences | e.g. `android-packaging.md`. Provenance text ("implemented in v0.17.0") is *not* matched — see §3. |
| `Cargo.lock` | `[[package]] version` | Regenerated via `cargo metadata`. |

## 2. Verify surfaces (checked, never rewritten)

| Surface | Check |
| :--- | :--- |
| `CHANGELOG.md` | `## [X.Y.Z]` section must exist for the workspace version. `publish.yml` also validates this on tags. |
| Git tag | `v{workspace version}` — `publish.yml` `validate` job. |

## 3. Permanent provenance (never bump)

Version strings that record *when* something shipped — historical record,
not current-version state:

- `CHANGELOG.md` — all past sections (append-only).
- `docs/milestones/*`, `docs/adr/*`, `docs/ddr/*` — milestone/ADR numbers.
- Provenance mentions: "shipped in v0.17.0" (`PLATFORM_SUPPORT.md`),
  `martensite-access-platform (v0.17.0)` (`CRATE_API_SPECIFICATIONS.md`),
  crate-table `*(vX.Y.Z)*` introduced-in markers (`README.md`), code comments
  referencing milestone versions (`src/*.rs`, tests), `WORKING_ON.md`
  historical sections, `docs/research/ARCHITECTURE_BLUEPRINT.md` timeline,
  `docs/android-packaging.md` status line and `version_name = "0.1.0"`
  (illustrative app config, not the framework version).
- `stubs/martensite/Cargo.toml`, `stubs/martensite-ui/Cargo.toml` — pinned
  `0.0.1` crates.io name reservations; intentionally never bumped.
- `martensite-vello` (`0.10.0-martensite.1`), `martensite-cosmic-text`
  (`0.19.0-martensite.2`), `martensite-text-reference` (`0.11.0`,
  `publish = false`) — independent pins that track upstream/base versions,
  not the workspace. The scripts sync *references* to them but never set
  them to the workspace version.
- `supply-chain/config.toml` — third-party exemption versions (e.g. `glow
  0.17.0` is a coincidence, not our version).
- `rust-version = "1.89.0"` MSRV — separate cadence, governed by
  `docs/RELEASE_PROCESS.md` (minor-bump policy + 6-month notice).
- Third-party dependency versions (`winit`, `accesskit`, `wgpu`, …).

## 4. Local-only surfaces (gitignored, not CI-checked)

| Surface | Action on release |
| :--- | :--- |
| `.ciel/PROJECT.md` | `Workspace version` / `Latest released` fields. |
| `.ciel/state/STATE.md` | Phase tracker release-tag column. |
| `WORKING_ON.md` | "Last updated" header — refreshed on release. |

## 5. Review surfaces (judgment, not mechanical)

| Surface | Why manual |
| :--- | :--- |
| `README.md` capability table header `Martensite (v0.13.0)` | "As-of" snapshot against competitor versions — refresh deliberately when the comparison is re-verified, not on every bump. |

---

## Release procedure

```sh
scripts/bump-version.sh 0.18.0        # set + sync everything
# write the new CHANGELOG.md section (manual — release notes)
scripts/check-version-consistency.sh  # verify all surfaces
git add -A && git commit -m "release: v0.18.0 ..."
git tag v0.18.0 && git push --tags    # publish.yml validates + gates
```
