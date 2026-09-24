# Spec: `cargo-martensite` CLI Expansion (W2)

**Constraints:** D3 (graceful degradation), D5 (CI-tested scaffolding).
**Crate:** `tools/cargo-martensite`.
**Related:** ADR-0037 (hot reload), ADR-0038 (dev channel),
SCAFFOLDING.md, DEV_LINT.md.

## Goal

Grow the CLI from `dev | build` to the day-one surface developers
expect from a framework toolchain, without inventing commands cargo
already provides.

## Command surface

```text
cargo martensite new <name> [--template app|bare|dashboard]
cargo martensite init [--agents] [--lint]     # add DX files to existing project
cargo martensite dev [--port N] [--no-watch]  # existing: cdylib hot reload
cargo martensite lint [--fix] [--force] [--standard K] [--format text|json]
cargo martensite inspect                      # attach to running dev app
cargo martensite doctor [--fix]               # environment diagnosis
cargo martensite check                        # fast: fmt+clippy+lint in one
cargo martensite build [--release]            # existing: cdylib build
```

Deliberately absent: `bundle`/`installer` (v0.19.0 distribution
milestone owns packaging), `serve` (no web dev-server in scope),
`self-update` (owned by the distribution milestone's signing story).

## Command specs

### `new` / `init` — see SCAFFOLDING.md

### `lint`

Two modes:

1. **Attach mode** (default when a dev session is live): connects to
   the running app's dev channel (ADR-0038), pulls the last `PaintList`
   per window, lints it, prints the standard three-bucket report.
   Flags mirror the design-lint engine: `--fix`, `--force`,
   `--no-recursive`, `--max-recursiveness N`, `--standard`,
   `--severity`, `--filter`.
2. **Offline mode**: `cargo martensite lint --scene dump.bin` replays a
   serialized `PaintList`/`LintScene` (what `MARTENSITE_LINT_DUMP=1`
   writes — see DEV_LINT.md).

Exit codes identical to the existing dashboard bin: nonzero on Warn+,
Info never gates. `--format json` emits the machine-readable report for
CI annotation.

### `inspect`

Connects to the dev channel of a running dev-mode app and prints a
one-shot or `--follow` dump: widget tree, selected-node layout chain,
signal graph fragment. This is the *headless* inspector — same data as
the in-app panel, for CI/SSH/agent use. `cargo martensite inspect
--pick` waits for the user to click in the app and prints the resolved
node.

### `doctor`

Diagnoses the environment; every check prints ✓/✗ plus a remediation
line (never just a failure):

- Toolchain: rustc/cargo version vs `rust-toolchain.toml`, component
  presence (`clippy`, `rustfmt`).
- GPU: `wgpu` adapter probe — backend, `Features`, limits relevant to
  Vello (compute, storage textures); falls back to reporting "CPU
  raster only" rather than failing.
- Text: fontconfig/font provider reachable; system font families
  resolvable; IME backend present (Linux IBus/Fcitx env vars).
- Accessibility: AT-SPI bus (Linux), UI Automation (Windows),
  NSAccessibility (macOS) reachability.
- Dev loop: `cargo-martensite` vs `martensite` version parity — a
  mismatch is a *warning with the exact versions*, following D1's
  version-skew lesson.
- `design-lint.toml` present and parses; stale allows count reported.

`--fix` applies safe remediations (install rustup components, write a
default `design-lint.toml`).

### `check`

The pre-commit composite: `cargo fmt --check` + `clippy -D warnings`
(both feature sets if `--all-features`) + `martensite lint`. Exists
because the three-command habit is exactly what contributors forget;
the exit code is the AND of all legs, and the output preserves each
tool's native format (no reformatting that breaks editor jump-to-error).

## Cross-cutting requirements

- **Version parity (D1).** The CLI embeds `MARTENSITE_VERSION` and the
  dev-channel handshake carries it. `dev`/`inspect`/`lint` warn loudly
  on mismatch; `--allow-version-mismatch` exists for edge cases.
- **Workspace awareness (D3).** `dev` resolves `--package` against
  `cargo metadata` at *watch* time; a file change in an unwatched
  workspace member triggers a full rebuild prompt, never a stale
  partial reload (subsecond #5540 lesson).
- **Config file.** `martensite.toml` at project root (distinct from
  `design-lint.toml`): `[dev] port`, `[lint] config path`,
  `[inspector] enabled`. All keys have CLI-flag overrides; unknown
  keys are warnings, not errors (forward compatibility).
- **Exit codes.** Uniform: 0 ok, 1 lint/doctor findings, 2 usage, 3
  infrastructure failure. Documented in `--help` and tested.

## Acceptance gates

1. `cargo martensite new app && cd app && cargo martensite doctor` is
   the documented 60-second first run, exercised by CI (D5).
2. `lint` in attach mode reports identical findings to the offline
   mode on the same frame (serialization round-trip test).
3. `doctor` on a clean machine exits 0; each removable check produces
   exactly one actionable line when broken (unit tests per check).
4. `check` returns the correct composite exit code for each
   single-leg-failure combination.
5. Version-mismatched `inspect` prints both versions and exits 3 —
   never silently connects.
