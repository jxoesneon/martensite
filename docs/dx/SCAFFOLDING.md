# Spec: Scaffolding & Agent-Native Project Context (W3)

**Constraints:** D5 (CI-tested templates, atomic, strict), D6 (drift
guards).
**Crate:** `tools/cargo-martensite` (`new`, `init`), template files in
`tools/cargo-martensite/templates/`.

## Goal

`cargo martensite new my-app` produces a project that compiles, runs,
passes its own lint, and — the differentiator — *teaches the user's AI
assistant how to use Martensite* on first open. `init` brings the same
context to an existing project.

## Templates

Three starting points, all minimal rather than maximal:

- **`app`** (default): single window, `MartensiteApp` scaffold, one
  reactive counter, `design-lint.toml`, `AGENTS.md`, `martensite.toml`,
  a `#[cfg(test)]` smoke test using `martensite-test`.
- **`bare`**: the smallest compiling main — no AGENTS.md, no lint
  config. For people who hate generated files.
- **`dashboard`**: the contextual dashboard skeleton — dock layout,
  one zone, one instrument widget, lint config pre-tuned for HMI.

Templates are *embedded in the CLI binary* (`include_dir!` or
`include_str!`), not fetched from git: no network dependency, no
version skew between template and installed CLI (D1/D5).

## The agent-native layer — the actual differentiator

Rosace and Makepad moved first here; the ecosystem convention
(`AGENTS.md`, `llms.txt`) is now standardizing. Every non-`bare`
template emits:

### `AGENTS.md` (project-local, generated)

Written for the *consumer* of the framework, not our contributors:

- The 10-line mental model: arena + signals + paint list + retained
  widgets. How a Martensite app is structured (app → zones → widgets).
- The widget map: which widgets exist, their `debug_name` conventions,
  and where the catalog example demonstrates each.
- The reactive patterns that are *correct* (signal ownership, when to
  `Memo`, transactional writes) and the ones that look right but aren't
  (per-frame signal creation, side-effects in `paint`).
- The project's own commands: `cargo martensite dev`, `lint`, `check` —
  with the exact flags this template ships.
- Honest capability notes: what's stable vs `unstable-*`, what isn't
  built yet. Generated from the same version of the crate the project
  depends on — no drift.
- Pointer to `design-lint.toml` with the two-line explanation of
  allows/severity.

### `llms.txt`

A terse index for doc-fetching agents: links to docs.rs, the
tutorials, `docs/design-standards/`, and this project's AGENTS.md.

### `design-lint.toml`

Sensible defaults for the template type (`dashboard` gets ISA-101
emphasis); `docs_base` pre-resolved; comments explaining every key.

### `martensite.toml`

CLI config: dev port, lint config path, inspector on in debug.

## Generation mechanics (D5)

- **Atomic scaffold:** render into `.martensite-new-<pid>/` staging,
  then `rename` to the target. Refuse if target exists and is non-
  empty. Partial renders never litter the filesystem.
- **Strict placeholders:** every `{{var}}` must resolve; an undefined
  placeholder is a hard error at *template-validation* time in our CI,
  not silently-empty at the user's machine (cargo-generate pitfall).
- **Name validation:** crate-name-safe check (`[a-z0-9_-]`, no
  keywords) before writing anything.
- **Next steps:** the last thing `new` prints is three commands —
  `cd`, `cargo martensite doctor`, `cargo martensite dev` — never a
  wall of text.
- **Refreshable, not frozen** (thoughtbot lesson): anything that will
  drift — `AGENTS.md`, `design-lint.toml` defaults — can be
  regenerated into an *existing* project via `cargo martensite init
  --agents` / `--lint`, which writes missing files and diffs (not
  clobbers) changed ones.

## CI contract (the drift guard — D5)

A `scaffold_smoke` CI job, per the staratlas pattern:

1. Run `cargo martensite new` for each template into a temp dir.
2. In the *generated* project: `cargo fmt --check`, `clippy
   --all-targets -- -D warnings`, `cargo build`, `cargo test`,
   `cargo martensite lint` (exit 0 or known-info findings only).
3. Assert `AGENTS.md`, `design-lint.toml`, `martensite.toml`,
   `llms.txt` exist and parse.
4. Runs on every PR that touches `tools/` or the facade's public API —
   template rot is caught in the same commit that causes it.

## Acceptance gates

1. `new` for each template → generated project compiles, tests pass,
   lint exits clean — verified by `scaffold_smoke` in CI, not by hand.
2. `new` into an existing non-empty dir fails atomically with zero
   writes (test: dir contents unchanged byte-for-byte).
3. `init --agents` on a project missing `AGENTS.md` writes it; on a
   project with an edited `AGENTS.md` it diffs, never overwrites.
4. The scaffolded `AGENTS.md` names only widgets/APIs that exist in
   the dependency version it pins — CI greps it for `unstable-` claims
   and verifies every referenced symbol resolves.
5. `llms.txt` contains no dead links — CI fetches each.
