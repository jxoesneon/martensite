# [ADR-0039] First-Party Model Context Protocol (MCP) Server for Comprehensive AI-Assisted UI Development

* **Status:** Accepted
* **Date:** 2026-10-15
* **Deciders:** Martensite Architecture Working Group
* **Technical Domain:** `tools/cargo-martensite`, `martensite-mcp`, `martensite-devtools`, `martensite-host`, `martensite-design-lint`
* **Amends:** Companion to ADR-0038 (dev channel) and ADR-0036 (in-app inspector); implements DX audit constraint **D9** (Semantic Honesty & Comprehensive Agent Parity).

---

## Context and Problem Statement

Autonomous AI coding agents (Devin, Claude Code, Cursor, Windsurf) have become core contributors and paired engineering partners in modern software development. While these agents excel at CLI tools and algorithmic logic, developing and maintaining graphical user interfaces (GUIs) in Rust remains challenging for AI systems due to the lack of direct semantic observability into the running UI engine.

Presently, agents attempting to develop or debug Martensite applications face two inadequate choices:
1. **Blind static generation:** Generating widget code without any runtime confirmation of layout bounds, Taffy flex distribution, clipping geometry, or reactive signal cascades.
2. **Black-box computer vision:** Relying on external OS-level desktop automation MCPs (such as `ultranix-mcp` or `ultramac`). While capable of taking screenshots and clicking coordinates, OS-level vision cannot inspect Taffy layout chains, explain why a widget overflowed by 14 pixels, observe reactive signal dependencies, query AccessKit semantic states, or inspect 51-rule design lint violations.

The architectural challenge is: **How can Martensite provide autonomous agents with rich, safe, real-time, semantic access to the live framework runtime without violating our constitutional principles of zero-cost release binaries, version-locked stability, and defense-in-depth security?**

---

## Decision Drivers

* **Semantic Honesty (D9):** Agents must observe the exact same ground truth that the engine computes (Taffy layout constraints, generational arena bounds, reactive dependency DAGs, design lint violations) rather than brittle, inferred pixel heuristics.
* **Version Lock (D1):** The MCP server must never silently accept a version mismatch with the target application, avoiding Flutter's documented DevTools skew failures (Flutter #8822, #100247).
* **Zero Release Overhead (D8):** All runtime inspection, dev-channel sockets, and live mutation mechanisms must compile out completely in release mode.
* **Standardized Agent Protocol:** The server must implement the standard Model Context Protocol (MCP) strictly over `stdio` and local Unix domain socket IPC, providing integration with Antigravity, Devin, Claude Code, Cursor, and the broader agent ecosystem while eliminating remote attack surfaces.
* **Offline Resilience:** The MCP server must function constructively in offline mode (when no live app is running), providing static design linting, code scaffolding, and toolchain diagnostics.
* **Controlled Mutation Boundary:** Live tweaks and source write-backs must be explicit, audited, and strictly sandboxed (`dry_run: true` by default, explicit `confirmed: true` required, workspace `src/` path-confined), preventing unauthorized arbitrary memory or code modification.

---

## Considered Options

* **Option 1: In-Process HTTP/Websocket Server Inside Every Martensite App**
  * *Description:* Compile a full MCP HTTP/Websocket listener directly into every Martensite binary.
  * *Rejection:* Violates ADR-0038 and Martensite security posture. Opening network ports in user binaries creates remote debugging attack surfaces and inflates dependency trees in release builds.
* **Option 2: Rely Solely on Desktop Vision MCPs (`ultranix-mcp` / `ultramac`)**
  * *Description:* Direct agents to use OS-level screenshots and OCR.
  * *Rejection:* Blind to layout constraints, flex resolution, reactive state, and design standards; high token consumption; no bidirectional source synchronization.
* **Option 3: Thin CLI-Only Wrapper over `cargo martensite inspect`**
  * *Description:* Wrap CLI subcommands as simple shell tools.
  * *Rejection:* High process spawning overhead; lacks interactive session context, resource subscription streams, and rich schema validation.
* **Option 4: First-Party Native MCP Server (`martensite-mcp` / `cargo martensite mcp`) Bridging to ADR-0038 Dev Channel**
  * *Description:* A dedicated crate (`martensite-mcp`) built on the standard `rmcp` library, exposing a comprehensive suite of semantic tools, resources, and prompt templates. In live mode, it connects to the running application via ADR-0038's version-handshook local Unix domain socket (`$XDG_RUNTIME_DIR/martensite/<build_id>.sock`). In offline mode, it falls back to static AST scaffolding, design linting, and toolchain diagnostics.

---

## Decision Outcome

Chosen option: **Option 4**.

Martensite will ship a first-party, native Model Context Protocol server integrated into the developer toolchain:
1. **Packaging & Entry Point:**
   - Shipped as a library crate `martensite-mcp` and exposed via the unified developer toolchain CLI as `cargo martensite mcp [--socket <path>]`.
   - Built using the high-performance pure-Rust `rmcp` SDK (matching the transport foundation of `ultranix-mcp`).
   - Transport is strictly local: `stdio` for agent pairing and local 0600 Unix domain sockets. In strict accordance with ADR-0038, **no TCP/HTTP listeners are ever exposed**.
2. **Runtime Transport & Bridge:**
   - Connects to running Martensite applications exclusively over the local, read-mostly Unix domain socket defined in ADR-0038 (`$XDG_RUNTIME_DIR/martensite/<build_id>.sock`).
   - Mandatory version-locked `hello` handshake enforcing matching `protocol` and `MARTENSITE_VERSION`. Mismatches exit immediately with explicit diagnostic errors.
3. **Comprehensive Capability Surface (28 Semantic Tools):**
   - **Inspection & Layout:** `martensite_inspect_tree` (with horizontal `child_limit` & pagination), `martensite_get_node`, `martensite_diagnose_layout`, `martensite_explain_overflow`, `martensite_inspect_signals`, `martensite_trigger_signal` (type-safe, tweak-gated), `martensite_set_signal` (adapter-gated JSON writes).
   - **Accessibility & Semantics:** `martensite_inspect_a11y_tree` (full ARIA & APG tree exploration), `martensite_invoke_accessibility_action` (real `SemanticAction` dispatch).
   - **Standards & Design Lint:** `martensite_lint_scene`, `martensite_apply_lint_fix` (clarified: mutates in-memory `LintScene` preview only; never mutates live `WidgetArena`), `martensite_audit_paint`.
   - **Live Tweaks & Source Sync:** `martensite_list_tweaks`, `martensite_set_tweak`, `martensite_sync_tweaks_to_source` (`dry_run: true` default, explicit `confirmed: true` gate, workspace `src/` confinement, optimistic revision locking), `martensite_set_theme`.
   - **Interaction & Replay:** `martensite_dispatch_event`, `martensite_get_event_ledger`, `martensite_step_timemachine`.
   - **Visual Verification:** `martensite_capture_node` (artifact file path default, opt-in base64), `martensite_render_headless` (sanitized alphanumeric arguments, direct `Command` execution without shell).
   - **Environment, Lifecycle & Scaffolding:** `martensite_doctor`, `martensite_reload_status`, `martensite_hot_reload` (coordinator marker contract), `martensite_scaffold_widget` (strictly workspace-path confined).
   - **Runtime Diagnostics:** `martensite_runtime_errors` (panic bundle + `ErrorSurface` + reactive errors), `martensite_logs` (bounded tracing ring).
   - **Loading-State Forcing (ADR-0040):** `martensite_set_loading` (forces/clears a node's loading state for placeholder verification).
4. **Resources & Prompts:**
   - Exposes dynamic MCP resources with notification invalidation pings: `martensite://app/tree/summary`, `martensite://app/tree/{root_id}`, `martensite://app/a11y`, `martensite://app/lint`, `martensite://app/signals`, `martensite://app/events/ledger`, `martensite://theme/tokens`.
   - Bundles standardized prompts (`audit_screen`, `fix_overflow`, `refactor_reactive_widget`, `implement_accessible_pattern`).
5. **Offline Fallback Mode:**
   - When no application socket is active, tool calls automatically fall back to static analysis, scene dump evaluation, template scaffolding, and environment diagnostics.
6. **Future Extensibility & Platform Bridges:**
   - Mobile and web runtime bridging (v0.17.0 parity) is roadmap-sequenced via authenticated local port forwarders (`adb forward`, `usbmuxd`, loopback WebSocket proxy) translating to the internal dev-channel socket without altering the application security profile.

---

## Positive Consequences

* **Unprecedented Agent Productivity:** AI agents can diagnose layout bugs, fix WCAG contrast issues, and tweak interactive spring physics in sub-millisecond feedback loops.
* **Deterministic Layout Ground Truth:** Eliminates guesswork; agents read the exact Taffy constraints, flex parameters, and generational node bounds.
* **Zero Production Overhead:** Dev-channel hooks and tweak registries compile out completely in non-dev builds; release binaries contain zero dev symbols.
* **Native Toolchain Interoperability:** Immediate out-of-the-box support for Antigravity, Devin, Claude Code, Cursor, and any MCP-compliant harness.

---

## Negative Consequences

* **Crate Surface Maintenance:** Adds `crates/martensite-mcp` to workspace maintenance and CI testing suites.
* **Schema Versioning:** The MCP tool schemas must be maintained and versioned alongside `martensite-devtools` and `martensite-design-lint`.

---

## Implementation Status (as landed)

The shipped surface exceeds the originally scoped 21 tools — **28 tools** are registered, and all tools advertise MCP `ToolAnnotations` (`readOnlyHint`, destructive/artifact hints) plus a recommended-workflow `instructions` payload on `initialize`. Decisions that emerged during implementation:

- **Layout push-model (`LayoutStore`):** `LayoutEngine` is `!Send` (Taffy compact-length internals carry `*const ()`), so it can never live behind the probe's socket-thread mutex. Apps feed a shared `LayoutStore` once per layout pass (`store.update_from_engine(&engine)`) — the same app-thread-push model as `DevSession::on_frame`/`absorb_events`/`record_reload`. `layout_chain` degrades to arena bounds + ancestry (`"layout_engine": null`) when no store is attached.
- **`SignalAdapter` registry:** `Signal::set` is generic over `T`, so JSON writes need a per-signal codec the app registers via `DevSession::register_signal_adapter`. Unregistered signals stay inspectable but honestly read-only — never fabricated as writable.
- **`LogRing` + `LogRingLayer`:** bounded tracing ring buffer; the app composes `ring.layer()` into its `tracing_subscriber` once at startup. `martensite_logs` reports `not_implemented` without an attached ring.
- **`martensite_runtime_errors`:** merges the dev-panic-hook bundle, the attached `ErrorSurface`, and ambient `ReactiveRuntime` errors; `clear: true` drains after snapshot.
- **`martensite_hot_reload` marker contract:** the session drops `<socket>.reload-request` next to its bound socket; `cargo martensite dev` polls discovered sessions' markers and runs a `reload_cycle`. Sessions not served via `serve_dev_session` report `unavailable:` (operational), not `not_implemented:` (capability).
- **Env auto-enable (D8):** `serve_dev_session_from_env` in `martensite-host` (`martensite::dev_channel` facade) binds the channel when `MARTENSITE_DEV_CHANNEL` is truthy, in debug builds only; release builds compile to a no-op returning `Ok(None)`.
- **Real rendering path:** `capture_node` rasterizes the node's paint list through `TinySkiaBackend` → PNG (base64 or artifact path); `audit_paint` reports real per-command stats from `WidgetArena::build_paint_list`. `a11y_action` dispatches real `SemanticAction`s; `event_dispatch` performs real bounds hit-testing and `WidgetArena::dispatch_event` routing (focus-aware keyboard routing when `with_focus_manager` is attached).

All remaining `not_implemented:` responses denote genuine capability gaps requiring an app-provided bridge (interactive inspector pick overlay, `theme_apply` `system` mode, node-scoped signal enumeration without probe support) — never fabricated state (D9).

## Links

* `docs/research/AI_ASSISTED_DEVELOPMENT_RESEARCH.md`
* `docs/dx/MCP.md` (Workstream W10 Specification)
* ADR-0036 (In-App Inspector), ADR-0037 (Hot-Reload Contract), ADR-0038 (Dev Channel)
* `docs/milestones/vNEXT-developer-experience.md`
