# [ADR-0036] In-App Inspector — DevTools Ships Inside the App Process

* **Status:** Accepted
* **Date:** 2026-10-08
* **Deciders:** Martensite Architecture Working Group
* **Technical Domain:** `martensite-devtools`, `tools/cargo-martensite`,
  `martensite-design-lint`
* **Amends:** None; extends ADR-0022 (in-engine profiler tracing).
  Implements audit constraint **D1** from
  `docs/research/DEVELOPER_EXPERIENCE_AUDIT.md`.

## Context and Problem Statement

The DX audit identified the widget inspector as the single largest DX
gap — Chrome DevTools select-mode and Flutter's widget inspector are
the ecosystem's reference experience. The architectural question is
where the inspector lives: in the app process, or as an external
tool over a wire protocol.

Flutter DevTools is the cautionary reference for external hosting:

- `flutter/devtools#8822` — a service-worker-cached web DevTools
  served the *wrong version* across SDK channel switches.
- `flutter/flutter#100247` — three divergent version strings for one
  tool.
- `flutter/devtools#9728` — DevTools auto-updating its pinned Flutter
  broke the app↔tool pairing; their own postmortem notes only
  integration-testing the *combination* would have caught it.
- `flutter/devtools#7477` — a Perfetto upgrade caused a severe
  DevTools-side performance regression (P1).

The failure mode is structural: an externally-hosted tool and the app
are versioned and shipped on different schedules, so skew is not an
edge case — it is the steady state, and every user pays the pairing
tax.

## Decision Drivers

* The inspector must see ground truth — the real `WidgetArena`, the
  same hit-test path, the same `PaintList` — not a serialized
  approximation that can drift.
* Zero version skew by construction, not by discipline.
* No foreign UI stack and no bundled webview (supply-chain and
  dependency-weight limits; Perfetto lesson — D2).
* Still support headless/CI/SSH/agent workflows where an overlay is
  unreachable.
* Dev-only cost: absent from release builds.

## Considered Options

* **Option 1**: External DevTools process — a separate app (or web
  app) attaching over a socket, Flutter/Chrome-DevTools style.
* **Option 2**: In-app only — inspector overlay compiled into the app,
  no external surface at all.
* **Option 3**: **In-app primary, read-only CLI attach secondary** —
  the inspector is a Martensite widget overlay in the app process;
  the CLI connects over a narrow dev channel (ADR-0038) for the
  headless cases, read-only.

## Decision Outcome

Chosen option: **Option 3**.

* **Primary surface is in-app.** `window.enable_devtools()` mounts an
  `OverlayLayer` inspector built from Martensite widgets, compiled
  from the same crate version as the framework it inspects. Version
  skew is impossible by construction — there is nothing to skew.
* **Dogfooding (D2).** The inspector uses `Tree`, `DataGrid`, `Tabs`,
  `OverlayLayer`. No foreign UI toolkit, no embedded webview, no
  service worker. Tooling pain is our bug report.
* **Headless access stays possible.** `cargo martensite inspect` and
  `lint` attach to a running dev-mode app through the dev channel —
  but as *read-only request/response* against the same in-app data
  (arena view, LintReport), with a protocol-version handshake that
  fails loudly on mismatch (the D1 lesson applied where a channel
  does exist).
* **The inspector subtree is invisible to the app.** Excluded from
  hit-testing, focus, the AccessKit tree, and `LintScene` — DevTools
  chrome is not part of the document it inspects.
* **Zero-cost when off.** Overlay not built, arena not walked, ledger
  not touched; feature-gated out of release entirely.

### Positive Consequences

* Version-skew bug class eliminated structurally — the class of issue
  that produced three separate Flutter DevTools bugs cannot occur.
* Inspector correctness is the framework's correctness: it exercises
  the production hit-test, arena, and paint paths, so it cannot
  silently drift from them.
* Every improvement to Martensite's widgets improves the inspector;
  every inspector limitation is real user-facing feedback.
* One UI codebase for tooling — no second stack to secure, bundle, or
  keep current.

### Negative Consequences

* The inspector shares the app's event loop; a wedged app cannot host
  its own inspector. Mitigated by the CLI's read-only attach and by
  the dev-channel dump (`--scene`) working post-mortem on serialized
  frames.
* In-app overlay cannot inspect apps on devices where the overlay
  itself can't render (a fallback for that is remote attach — noted
  as a future possibility, not v1 scope).
* Inspector widgets must be carefully excluded from app-level systems
  (a11y, lint, focus) — an explicit exclusion contract to maintain.

## Links

* `docs/research/DEVELOPER_EXPERIENCE_AUDIT.md` §4.1, §4.2
* `docs/dx/INSPECTOR.md`
* ADR-0038 (dev-channel transport)
