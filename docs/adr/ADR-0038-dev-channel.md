# [ADR-0038] Dev Channel — Narrow, Read-Only, Version-Handshook IPC

* **Status:** Accepted
* **Date:** 2026-10-08
* **Deciders:** Martensite Architecture Working Group
* **Technical Domain:** `martensite-host`, `tools/cargo-martensite`,
  `martensite-devtools`
* **Amends:** None; companion to ADR-0036 (in-app inspector) — defines
  the only permitted out-of-process surface. Implements audit
  constraint **D1** for the CLI-attach cases.

## Context and Problem Statement

ADR-0036 puts the inspector in-app to eliminate version skew. But some
workflows legitimately cannot use an overlay: headless CI, SSH
sessions, IDE/agent integrations, and `cargo martensite lint`
attaching to a running dev session. These need *some* channel between
the CLI and a live app.

The design question is the channel's shape. The two mistakes to avoid
are well documented: Flutter's DevTools version skew (a too-loose,
version-drift-prone protocol) and the broad, write-capable debug
ports that have caused real vulnerabilities in other ecosystems (the
Chrome `--remote-debugging-port` exposure class — an open control
channel into a running app).

## Decision Drivers

* The channel must never silently accept a version mismatch — the D1
  lesson applied to the one place a wire exists.
* Minimal attack surface: local-only, read-mostly, off by default,
  and absent in release builds.
* The channel serves the *devtools data plane* (tree snapshot, lint
  report, event ledger) — it is not a general app-control API.
* Cheap to implement and stable enough that CI can rely on it.

## Considered Options

* **Option 1**: Full remote-debugging protocol — bidirectional,
  command-execution-capable (CDP-style).
* **Option 2**: No channel at all — in-app inspector only.
* **Option 3**: **Local unix-socket, request/response, read-only-by-
  default, version-handshook dev channel**, enabled only in dev mode.

## Decision Outcome

Chosen option: **Option 3**.

* **Transport:** unix domain socket (named pipe on Windows) at a
  per-app path derived from a build/session id —
  `$XDG_RUNTIME_DIR/martensite/<build_id>.sock` / platform equivalent.
  No TCP listener, ever — no network-exposed debug port (the
  remote-debugging exposure class is declined by construction).
* **Framing:** newline-delimited JSON-RPC-lite requests/responses;
  versioned `protocol` field negotiated in a mandatory `hello`
  handshake carrying `MARTENSITE_VERSION` + protocol version. Mismatch
  ⇒ explicit `version_mismatch` error + both versions printed; the
  CLI exits nonzero (never silent degradation — D1).
* **Surface is read-mostly.** v1 requests:
  `TreeSnapshot` (arena view, lazy/paged), `LintPull`/`LintScene`,
  `EventLedger` (bounded tail), `InspectorSelect` (arm select-mode,
  resolve clicked node), `LintApply` (apply fixes to a *copy* of the
  scene — reports the converged model, never mutates the live arena).
  There is no execute/eval request; there is no arbitrary write.
* **Mutation is a separate, deliberate door.** Live property tweaks
  (W5) go through the in-app `TweakRegistry`, not this channel — so a
  compromised/dev-channel client cannot write the arena by default.
  Any future write request is an explicit opt-in RPC behind a flag,
  justified by ADR, not an ambient capability.
* **Gating:** the channel exists only when `devtools` feature is on
  AND `martensite.toml [dev] channel = true` (default on in dev
  builds, off in release — where the code isn't even compiled). The
  socket is bound 0600/user-only.
* **Lifecycle:** created on `enable_devtools`/`martensite-host` dev
  session start; removed on exit; `cargo martensite inspect`
  discovers it via the runtime-dir path + a `hello` probe — no
  registry, no stale sockets consulted after exit.

### Positive Consequences

* Headless/CI/agent workflows work without an overlay — the same
  data the in-app inspector renders, served read-only.
* The version handshake makes the Flutter skew failure structurally
  impossible on the one wire we do expose.
* Local-only + read-mostly + dev-gated keeps the attack surface near
  zero — no open control port, no remote surface, no write path.
* A stable, narrow protocol is cheap to keep compatible; the data
  plane (tree/lint/events) is already versioned by the same crate
  version the app runs.

### Negative Consequences

* Unix-socket/named-pipe code is platform-specific plumbing to
  maintain (small, well-trodden).
* No remote-device inspection in v1 (a phone running the app can't be
  inspected from the desktop) — deferred as a future opt-in, since
  remote transport reopens the exposure question and deserves its own
  ADR.
* The `LintApply`-to-a-copy semantics must be clearly explained —
  users might expect `--fix` to change the live UI (it intentionally
  does not; W5 owns live mutation).

## Links

* `docs/research/DEVELOPER_EXPERIENCE_AUDIT.md` §4.1
* `docs/dx/CLI.md` (`inspect`, `lint` attach), `docs/dx/DEV_LINT.md`
* ADR-0036 (in-app inspector), ADR-0037 (reload contract)
