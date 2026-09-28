# Spec: First-Party Model Context Protocol (MCP) Server for AI-Assisted Development (W10)

**Constraints:** D1 (version-locked handshake), D3 (cdylib reload fallback), D5 (CI-tested scaffolding), D7 (lazy tree, horizontal breadth clamping, node-level findings, no scores), D8 (dev-only mutation channel, compiled out in release), **D9 (Semantic Honesty & Comprehensive Agent Parity)**.  
**Crates:** `crates/martensite-mcp` (NEW library crate), `tools/cargo-martensite` (CLI entry point `cargo martensite mcp`), `martensite-devtools`, `martensite-design-lint`, `martensite-reactive`, `martensite-access`.  
**ADR:** ADR-0039 (First-Party Model Context Protocol Server for Comprehensive AI-Assisted Development).  
**Parent Workstream:** Milestone v0.20.0 Developer Experience Initiative (Workstream W10).

---

## 1. Goal & Architectural Mission

Provide autonomous AI agents (such as Devin, Claude Code, Cursor, and Windsurf) with direct semantic access to the live Martensite application runtime and developer toolchain.

Rather than treating the running application as an opaque black box of pixels (requiring brittle computer-vision models or lossy OCR heuristics), the Martensite MCP server enables agents to:
1. **Query runtime layout truth:** Inspect exact Taffy input constraints, flex parameters, measure closure outputs, and generational arena bounds.
2. **Diagnose and autofix design violations:** Pull structured, cited findings from Martensite's 51-rule design linter (WCAG 2.2, ISA-101, Gestalt, Hick-Fitts) and apply safe or risky fixes.
3. **Trace reactive state:** Inspect active signal DAGs, subscriber sets, and dirty bitsets to pinpoint invalidation bugs.
4. **Mutate live properties with safe source write-back:** Live-tweak numerical, color, and spring values in the running app and safely commit converged values back to original Rust source files under strict confirmation gates and workspace confinement.
5. **Simulate events & test deterministically:** Inject pointer, keyboard, and focus interactions into the `EventRouter` and verify state transitions against the `VirtualClock`.

---

## 2. System Architecture & Wire Communication

```text
┌────────────────────────── AI Agent (Host) ──────────────────────────┐
│ Devin / Claude Code / Cursor / Windsurf                             │
└──────────────────────────────────┬───────────────────────────────────┘
                                   │ Standard MCP over stdio (JSON-RPC)
┌──────────────────────────────────▼───────────────────────────────────┐
│ cargo martensite mcp (martensite-mcp crate)                         │
│                                                                      │
│ ┌──────────────────────┐  ┌───────────────────────┐ ┌──────────────┐ │
│ │  MCP Tool Registry   │  │ MCP Resource Provider │ │ MCP Prompts  │ │
│ │  (28 Semantic Tools) │  │  (Dynamic URI Trees)  │ │ (Templates)  │ │
│ └──────────┬───────────┘  └───────────┬───────────┘ └──────┬───────┘ │
└────────────┼──────────────────────────┼────────────────────┼─────────┘
             │                          │                    │
             ├──────────────────────────┴────────────────────┤
             │                                               │
             ▼ [If Live App Running]                         ▼ [If Offline Mode]
┌──────────────────────────────────────────────┐ ┌──────────────────────────────┐
│ ADR-0038 Dev Channel IPC (Local Unix Socket) │ │ Offline Toolchain Engine     │
│ $XDG_RUNTIME_DIR/martensite/<build_id>.sock  │ │                              │
│                                              │ │ • Static AST Scaffolder      │
│ ┌──────────────── user application ────────┐ │ │ • Offline Scene Analyzer     │
│ │ martensite-devtools (feature: devtools)  │ │ │ • File-based Design Linter   │
│ │  • Generational Arena (WidgetArena)      │ │ │ • Toolchain Doctor           │
│ │  • Taffy Layout Chain                    │ │ │ • Headless Test Harness      │
│ │  • Reactive Signal DAG                   │ │ └──────────────────────────────┘
│ │  • 51-Rule Design Lint Engine            │ │
│ │  • AccessKit Semantic Tree               │ │
│ │  • Live Tweak Registry                   │ │
│ │  • Event Record Ledger                   │ │
│ │  • TimeMachine Checkpoints               │ │
│ └──────────────────────────────────────────┘ │
└──────────────────────────────────────────────┘
```

### Transport & Security Invariant:
- **Local-Only Stdio Transport:** `cargo martensite mcp` communicates with the calling agent strictly via `stdio` JSON-RPC. In compliance with ADR-0038, **no TCP listeners, HTTP endpoints, or unauthenticated ports are exposed**.
- **Local 0600 IPC Socket:** In live mode, it attaches to the application's dev channel at `$XDG_RUNTIME_DIR/martensite/<build_id>.sock` (bound `0600` user-only).
- **Mandatory Version Handshake (D1):** Connects with mandatory `hello` handshake enforcing matching `protocol: 1` and `MARTENSITE_VERSION`. Mismatches exit immediately with a diagnostic error.
- **Offline Project Mode:** Automatically activates when no live application is detected. Provides offline static scene evaluation (`--scene <path>`), AST template scaffolding, environment diagnosis, and headless regression testing.

---

## 3. Comprehensive MCP Tool Suite Specification

The server registers 28 tools categorized into ten functional domains. Every tool advertises MCP `ToolAnnotations` — read-only tools carry `readOnlyHint: true`, mutations (`set_tweak`, `set_theme`, `set_signal`, `trigger_signal`, `set_loading`, `dispatch_event`, `invoke_accessibility_action`, `apply_lint_fix`, `sync_tweaks_to_source`, `hot_reload`) carry `readOnlyHint: false`, and artifact producers (`capture_node`, `render_headless`, `scaffold_widget`) carry artifact hints. The server's `initialize` response embeds a recommended workflow in `instructions` so agents can bootstrap without prior Martensite knowledge.

### Category A: Widget Arena & Hierarchy Inspection

#### 1. `martensite_inspect_tree`
Queries the hierarchical widget tree of the running application with horizontal breadth clamping and lazy pagination.
- **Parameters:**
  - `root_id` (optional, string): Target `WidgetId` to scope subtree inspection. Defaults to the arena root.
  - `max_depth` (optional, integer): Maximum vertical recursion depth (default: `4`, max: `32`).
  - `child_limit` (optional, integer): Maximum direct child elements returned per parent (default: `50`, max: `200`) to prevent token saturation on large container hierarchies.
  - `offset` (optional, integer): Child element pagination offset (default: `0`).
  - `filter_marker` (optional, string): Filter nodes by semantic marker (e.g., `@alarm`, `@kpi`, `@level:1`).
  - `include_internal` (optional, boolean): If `true`, expands widget-internal child protocol nodes (`child_count`/`child`).
- **Returns:** JSON hierarchy containing `debug_name`, `WidgetId`, logical bounds `[x, y, w, h]`, total child count, active pagination token, and state badges (`has_lint_warnings`, `signal_dirty`). Virtualized containers (such as `DataGrid` or virtual `ScrollView`) emit a `+N virtualized rows` placeholder node per constraint D7.

#### 2. `martensite_get_node`
Retrieves comprehensive details for a single target widget node.
- **Parameters:**
  - `node_id` (required, string): The unique `WidgetId` or `debug_name` path.
- **Returns:** Complete node record:
  - Generational index and node kind (`Flex`, `Container`, `Text`, `Button`, etc.).
  - Computed logical and physical bounds, clip rects, z-index.
  - Semantic annotations (`debug_name`, markers, ARIA role).
  - Source code span (`file`, `line`, `col`) when compiled with `devtools-source-spans`.
  - Registered AccessKit properties (accessible name, description, role, enabled/disabled state).

### Category B: Layout & Constraint Diagnostics

#### 3. `martensite_diagnose_layout`
Returns the exact Taffy layout chain and constraint resolution history for a widget.
- **Parameters:**
  - `node_id` (required, string): Target `WidgetId`.
- **Returns:**
  - Inbound constraints received from parent (`MinContent`, `MaxContent`, `Definite(w, h)`).
  - Taffy style definitions (flex-direction, flex-grow, flex-shrink, flex-basis, align-items, justify-content, padding, margin, gap).
  - Intrinsic measure closure result (`min_content`, `max_content`, aspect ratio).
  - Final resolved layout bounds and ancestry constraint propagation chain.

#### 4. `martensite_explain_overflow`
Pinpoints the exact mathematical cause and clipping lineage of a layout overflow.
- **Parameters:**
  - `node_id` (optional, string): Target widget to diagnose. If omitted, scans the entire scene for active overflows.
- **Returns:**
  - Array of overflow diagnostics:
    - `offending_node`: The node whose content exceeds allocated bounds.
    - `overflow_axis`: `"horizontal"` | `"vertical"` | `"both"`.
    - `overflow_pixels`: Exact floating-point pixel magnitude of the overflow.
    - `clipping_ancestor`: The parent container enforcing the bounds and its scroll capability.
    - `suggested_remediations`: Array of actionable code changes (e.g., wrap in `ScrollView`, set `flex_shrink(1.0)`, or clamp font size).

### Category C: Reactive State & Signal DAG

#### 5. `martensite_inspect_signals`
Introspects the push-pull reactive signal DAG with token-safe pagination.
- **Parameters:**
  - `signal_id` (optional, string): Specific signal ID to inspect.
  - `node_id` (optional, string): Return only signals subscribed to by this widget node.
  - `only_dirty` (optional, boolean): If `true`, returns only signals flagged dirty in the current evaluation tick (default: `false`).
  - `limit` (optional, integer): Maximum signals returned (default: `50`, max: `200`).
  - `offset` (optional, integer): Pagination offset (default: `0`).
- **Returns:**
  - Array of signal descriptors: ID, current serialized value, subscriber count, dependency nodes (producers and consumers), and topological scheduler rank.

#### 6. `martensite_trigger_signal`
Mutates a registered tweakable reactive signal value in dev mode to test dynamic UI state transitions.
- **Parameters:**
  - `signal_id` (required, string): ID of the signal to update.
  - `new_value_json` (required, string): Serialized JSON value to assign.
- **Safety Gate:** Gated to signals registered in `TweakRegistry` or annotated with `#[tweak]`. Deserialization is strictly type-checked against the registered schema. Mismatches return structured validation errors, preventing application panics.
- **Returns:** Status confirmation and list of invalidated widgets marked dirty for the next frame.

#### 7. `martensite_set_signal`
Writes a JSON value into a reactive signal through a registered `SignalAdapter`.
- **Parameters:**
  - `signal_id` (required, string): Registered adapter name or numeric signal id.
  - `value` (required, JSON): New value; type-checked by the app's adapter codec.
- **Safety Gate:** `Signal::set` is generic over `T`, so an arbitrary signal cannot be written from JSON without a per-type codec. Only signals the app explicitly registered via `DevSession::register_signal_adapter` are writable; unregistered signals return an actionable error naming the registration call rather than a fabricated success.
- **Returns:** `{applied, signal, signal_id, revision}` on success.

### Category D: Accessibility & Semantic Contract

#### 8. `martensite_inspect_a11y_tree`
Evaluates the hierarchical AccessKit accessibility tree mirror independently of the widget layout hierarchy.
- **Parameters:**
  - `root_id` (optional, string): Target `NodeId` or widget anchor.
  - `role_filter` (optional, string): Filter by ARIA role (`"button"`, `"heading"`, `"list"`, `"dialog"`, etc.).
  - `include_ignored` (optional, boolean): Include presentation-only / unmapped accessibility nodes (default: `false`).
- **Returns:** Complete semantic accessibility tree: accessible names, descriptions, roles, states (expanded, selected, checked), live region attributes, and APG pattern conformance findings.

#### 9. `martensite_invoke_accessibility_action`
Dispatches a `SemanticAction` into the running application — the same actions assistive technologies emit.
- **Parameters:**
  - `node_id` (optional, string): Target `WidgetId`. When omitted and the app attached its `FocusManager` (`with_focus_manager`), the action targets the focused node.
  - `action` (required, string): `"click"`, `"focus"`, `"blur"`, `"set_value"`, `"increment"`, `"decrement"`, `"expand"`, `"collapse"`, `"show_tooltip"`, `"hide_tooltip"`, `"show_context_menu"`, `"scroll_up"`, `"scroll_down"`, `"scroll_left"`, `"scroll_right"`, `"scroll_into_view"`, `"scroll_to_point"`, `"set_scroll_offset"`.
  - `value` (optional, string): Required for `set_value`.
  - `point` (optional, `[x, y]` array): Required for `scroll_to_point`/`set_scroll_offset`.
- **Returns:** `EventResponse` and the resolved target/hit path.

### Category E: Design Standards & Lint Enforcement

#### 10. `martensite_lint_scene`
Evaluates the running frame or offline scene against Martensite's 51 design standards with token-safe output limits.
- **Parameters:**
  - `standard` (optional, string): Filter by standard (`"wcag"`, `"isa-101"`, `"isa-18.2"`, `"gestalt"`, `"fitts"`, `"tufte"`, `"all"`).
  - `min_severity` (optional, string): Minimum severity threshold (`"info"`, `"warn"`, `"forbid"`).
  - `node_path_filter` (optional, string): Substring filter on widget `debug_name` lineage.
  - `limit` (optional, integer): Maximum findings returned (default: `50`, max: `250`).
  - `offset` (optional, integer): Pagination offset (default: `0`).
- **Returns:** Structured lint report:
  - `findings`: Array of violations, each containing `rule_id`, `standard`, `severity`, `node_id`, `node_path`, `message`, `citation_url`, and optional `autofix` payload.
  - `summary`: Counts of active, suppressed, and forbid findings across the entire scene.

#### 11. `martensite_apply_lint_fix`
Applies a safe or risky `LintFix` to the live scene preview model or generates a git diff patch.
- **Parameters:**
  - `finding_id` (required, string): Finding identifier returned by `martensite_lint_scene`.
  - `force` (optional, boolean): If `true`, permits applying `Risky` fixes (such as recoloring or bounds restructuring).
  - `target` (optional, string): `"scene"` (updates in-memory preview model) or `"patch"` (returns unified git diff for source code).
- **Live Arena Invariant:** Specifying `target: "scene"` mutates **solely the ephemeral in-memory `LintScene` preview model** to mathematically prove convergence and preview visual changes. In accordance with ADR-0038, it **never mutates the authoritative live `WidgetArena`**.
- **Returns:** Convergence proof, modified properties, or formatted git patch text.

#### 12. `martensite_audit_paint`
Performs a deep visual audit on text rendering, contrast, and keylines.
- **Parameters:**
  - `node_id` (optional, string): Subtree to audit.
- **Returns:**
  - Text ink descender clearance audit (detects baseline-to-bottom overlaps).
  - Measured WCAG contrast ratios of all visible text against actual blended background pixels.
  - Non-text keyline contrast verification (3:1 stroke ratio enforcement).

### Category F: Live Tweaks & Source Synchronization

#### 13. `martensite_list_tweaks`
Lists all active runtime tweakable parameters registered via `#[tweak]` or `TweakRegistry`.
- **Returns:** Array of registered tweaks: `name`, `type` (`f32`, `Color`, `Spring`, `String`), `current_value`, `default_value`, `source_file`, `source_line`, and current `revision_token`.

#### 14. `martensite_set_tweak`
Updates a tweak parameter in the running application in real-time.
- **Parameters:**
  - `name` (required, string): Tweak identifier.
  - `value` (required, string/number): New value to assign.
- **Returns:** Confirmation of live application and latency of the update (target: `< 1.0ms`).

#### 15. `martensite_set_theme`
Toggles dark/light theme mode or sets dynamic design token palette overrides.
- **Parameters:**
  - `mode` (optional, string): `"light"`, `"dark"`, `"system"`.
  - `token_overrides` (optional, object): Map of token names to Oklab/Hex color values.
- **Returns:** Updated theme status, active token values, and repaint confirmation.

#### 16. `martensite_sync_tweaks_to_source`
Safely commits converged live tweak values back to the underlying Rust source files.
- **Parameters:**
  - `tweak_names` (optional, array of strings): Names of tweaks to commit. If omitted, targets all modified tweaks.
  - `dry_run` (optional, boolean): **Defaults to `true` (preview-only by default)**.
  - `confirmed` (optional, boolean): Mandatory confirmation gate. Disk writes only proceed when `dry_run: false` AND `confirmed: true`.
  - `expected_revision` (optional, string): Optimistic concurrency token preventing multi-agent race conditions.
- **Safety Boundaries:**
  - **Workspace Confinement:** Target files must resolve strictly within the discovered Cargo workspace `src/` directory; any attempt to write outside is rejected.
  - **Audit Logging:** Every applied disk write emits a structured audit record stamped with timestamp, file path, span line numbers, and applied diff.
- **Returns:** Unified git diff of changes, modified file paths, and audit status.

### Category G: Interaction, Replay & Visual Verification

#### 17. `martensite_dispatch_event`
Dispatches a synthetic input event into the running application's `EventRouter`.
- **Parameters:**
  - `event_type` (required, string): `"pointer_click"`, `"pointer_move"`, `"scroll"`, `"key_press"`, `"text_input"`.
  - `position` (optional, array of two floats): `[x, y]` logical coordinates.
  - `key` (optional, string): Virtual key code or text string.
  - `delta` (optional, array of two floats): Scroll delta `[dx, dy]`.
- **Returns:** `EventResponse` (`Handled`, `Ignored`, `RequestFocus`, `RequestRepaint`) and resolved hit-test path.

#### 18. `martensite_get_event_ledger`
Retrieves the recent history of routed events and routing diagnostics.
- **Parameters:**
  - `limit` (optional, integer): Maximum ledger entries to return (default: `50`, max: `200`).
- **Returns:** Array of `EventRecord`s: timestamp, event type, hit-test target `WidgetId`, bubbling ancestry, and rejection reasons if dropped.

#### 19. `martensite_step_timemachine`
Controls the deterministic TimeMachine debugger.
- **Parameters:**
  - `action` (required, string): `"pause"`, `"resume"`, `"step_forward"`, `"step_backward"`, `"restore_checkpoint"`.
  - `checkpoint_id` (optional, integer): Specific frame checkpoint to restore.
- **Returns:** Current frame timestamp, active arena fingerprint, and signal state hash.

#### 20. `martensite_capture_node`
Renders an individual widget subtree directly to an image without desktop window chrome.
- **Parameters:**
  - `node_id` (required, string): Target `WidgetId` or `debug_name`.
  - `format` (optional, string): `"png"` (default) or `"jpeg"`.
  - `scale` (optional, float): DPI scale multiplier (default: `1.0`).
  - `include_base64` (optional, boolean): **Defaults to `false`**.
- **Efficiency Invariant:** Defaults to writing the rasterized frame to a local disk artifact path (e.g. `$XDG_RUNTIME_DIR/martensite/captures/<node_id>.png`) and returning its file URI, preventing massive LLM token waste. Inline Base64 is returned only when `include_base64: true` is explicitly requested.
- **Returns:** `artifact_uri`, logical dimensions, and physical pixel dimensions.

#### 21. `martensite_render_headless`
Spins up a headless Martensite test harness and renders a widget to a golden frame under `VirtualClock`.
- **Parameters:**
  - `crate_target` (required, string): Target package or example name. Must match alphanumeric regex `^[a-zA-Z0-9_-]+$`.
  - `viewport_size` (required, array of two integers): `[width, height]`.
  - `virtual_time_ms` (optional, integer): Advance virtual clock by N milliseconds before capture.
- **Safety Gate:** Invoked directly via `std::process::Command` with segregated arguments; never executed through a shell interpreter.
- **Returns:** Golden frame image artifact path, paint audit report, and perceptual diff comparison if a reference frame exists.

### Category H: Toolchain, Lifecycle & Scaffolding

#### 22. `martensite_doctor`
Diagnoses host toolchains, graphic adapters, display servers, and feature flags.
- **Returns:** Diagnostic status of Rust compiler, LLD/mold linkers, Vulkan/wgpu physical devices, Wayland/X11/macOS display interfaces, and required dev tools.

#### 23. `martensite_reload_status`
Queries the live hot-reload cdylib lifecycle and compiler status.
- **Returns:**
  - `active_build_id`: Unique identifier of the currently mapped cdylib.
  - `last_reload_timestamp`: ISO 8601 timestamp of last reload.
  - `total_reloads`: Total successful hot swaps in the active session.
  - `compiler_diagnostics`: Array of active rustc warnings/errors from background file watching.
  - `state_preservation_report`: Status of `on_hot_reload` state restoration hooks.

#### 24. `martensite_hot_reload`
Requests a guest-code rebuild+reload from the running app's dev coordinator.
- **Parameters:**
  - `reason` (optional, string): Free-form reason recorded in the reload-request marker.
- **Mechanism:** The dev session drops a `<socket>.reload-request` JSON marker next to its socket; `cargo martensite dev` polls discovered sessions' markers each tick and runs a `reload_cycle` when one appears, then removes the marker. A session not served through `serve_dev_session` has no marker path and reports `unavailable` (an operational error, not a capability gap).
- **Returns:** `{requested, mechanism, marker}`; confirm the cycle landed via `martensite_reload_status` (`total_reloads` increments).

#### 25. `martensite_scaffold_widget`
Generates idiomatic, standards-compliant Martensite widget source code.
- **Parameters:**
  - `widget_name` (required, string): PascalCase widget name.
  - `parent_crate_path` (required, string): Target crate directory.
  - `widget_type` (required, string): `"leaf"` (custom painting), `"container"` (two-level internal children protocol), or `"composite"` (macro-based component).
  - `has_reactive_state` (optional, boolean): Include `Signal` bindings.
  - `has_a11y` (optional, boolean): Include AccessKit contract implementation.
- **Safety Gate:** Path canonicalization ensures `parent_crate_path` resides strictly within the Cargo workspace root. Any attempt to write outside is rejected with an authorization error.
- **Returns:** Generated Rust file paths and registered module updates.

### Category I: Runtime Diagnostics

#### 26. `martensite_runtime_errors`
Aggregates every in-process error source the dev session can observe into one severity-tagged record list: the global dev-panic hook bundle (`install_dev_panic_hook`), the attached `ErrorSurface` (tier-2 diagnostics + captured crash), and the ambient `ReactiveRuntime` evaluation-error log.
- **Parameters:**
  - `limit` (optional, integer): Maximum records returned (default: `50`, max: `500`).
  - `severity` (optional, string): Exact severity match (`"info"`, `"warning"`/`"warn"`, `"error"`, `"critical"`, `"fatal"`).
  - `clear` (optional, boolean): Drain every source after snapshotting (default: `false`).
- **Returns:** `{errors, total, panic_active, surface_attached, surface_diagnostic_count}` — an empty list is an honest answer, never a capability gap.

#### 27. `martensite_logs`
Tails the app's attached `LogRing` — a bounded tracing ring buffer.
- **Parameters:**
  - `limit` (optional, integer): Maximum records returned (default: `100`, max: `1000`).
  - `level` (optional, string): Minimum severity (`trace`<`debug`<`info`<`warn`<`error`).
  - `target_prefix` (optional, string): Filter to `tracing` targets with this prefix.
  - `contains` (optional, string): Message substring filter.
- **App Contract:** The app attaches a shared ring via `DevSession::set_log_ring` and feeds it by composing `ring.layer()` (a `tracing_subscriber::Layer`) into its subscriber at startup. Without an attached ring the tool reports `not_implemented` rather than an empty list that would pretend the app logged nothing.
- **Returns:** `{logs: [{seq, level, target, message, file, line, timestamp}], total_buffered, capacity}` — records are newest-first, bounded by ring capacity.

### Category J: Loading-State Forcing (ADR-0040)

#### 28. `martensite_set_loading`
Forces or clears a widget node's `NodeFlags::LOADING` override in the live `WidgetArena` — the node-level half of ADR-0040 phase 3, letting agents drive the skeleton-placeholder path for *any* node, including widgets that never implemented `is_loading`.
- **Parameters:**
  - `node_id` (required, string): Target `WidgetId` (numeric id as returned by `martensite_inspect_tree`/`martensite_get_node`).
  - `loading` (required, boolean): `true` forces the skeleton placeholder; `false` clears the instance override (a widget's own `is_loading` declaration still applies).
- **Returns:** `{applied, node_id, loading, revision}` — `loading` reports the *effective* state (`WidgetArena::node_loading`, i.e. override OR widget-declared), and `revision` is the session's mutation counter.
- **Verify loop:** `martensite_inspect_a11y_tree` shows the sanitized node (`name: "Loading"`, `busy`/`disabled` states, descendants pruned — mirroring the AccessKit adapter), `martensite_get_node` reports `loading: true` plus the `loading` badge and `accesskit.busy`, and `martensite_capture_node` shows the painted placeholder.
- **Live-only:** the override mutates a running arena, so there is no offline equivalent — without a dev session the tool returns the `requires a live Martensite dev session` IPC error rather than fabricating state (D9).

---

## 3.1 App-Side Integration

The dev channel is opt-in and per-app — the `WidgetArena` and `EventRouter` live inside whatever binary the app owns, so `cargo martensite dev` (an external build coordinator) cannot inject the channel. Integration is a small, explicit callsite:

```rust
use std::sync::{Arc, Mutex};

// One-liner: serves the dev channel only when the environment opts in —
// a no-op returning `Ok(None)` in release builds (ADR-0039 D8).
let _server = martensite::dev_channel::serve_dev_session_from_env(arena.clone())?;
```

`serve_dev_session_from_env` (in `martensite-host`, re-exported as `martensite::dev_channel` under the facade's `dev-channel` feature):
- **`MARTENSITE_DEV_CHANNEL=1`** (also `true`/`on`/`yes`, case-insensitive) opts the process in — debug builds only; release builds always return `Ok(None)`.
- **`MARTENSITE_BUILD_ID`** names the socket (`<build_id>.sock`); defaults to `dev-<pid>` so parallel sessions never collide.
- **`MARTENSITE_DEV_SOCKET`** is honored by dev-channel *clients* (MCP, `cargo martensite inspect`) during discovery, not by the server.

For manual wiring — or when the app owns additional runtimes — build the session directly:

```rust
let session = Arc::new(DevSession::with_arena(arena.clone()));
// Optional attachments, each unlocking real data for a channel surface:
let layout_store = Arc::new(Mutex::new(LayoutStore::new()));
let probe = WidgetArenaProbe::new(arena.clone())
    .with_layout_store(layout_store.clone()) // real Taffy `layout_chain`
    .with_focus_manager(focus.clone());      // keyboard/text event_dispatch, focused-node a11y_action
let session = Arc::new(DevSession::new(Box::new(probe)));
session.set_error_surface(error_surface);   // feeds runtime_errors
session.set_log_ring(log_ring.clone());     // feeds logs — compose ring.layer() into the subscriber
session.register_signal_adapter(adapter);   // makes a named signal writable via set_signal
let _server = serve_dev_session(session.clone(), "dev")?;
```

Per-frame/per-event feeds on the app thread keep the session current:

```rust
layout_store.lock().unwrap().update_from_engine(&layout_engine); // !Send engine → Send snapshot
session.on_frame(&paint_list);          // lint_pull/lint_apply scene capture
session.absorb_events(router.ledger()); // merges real routed input into event_ledger
session.record_reload(build_id);        // feeds reload_status after each hot swap
```

`LayoutEngine` is `!Send` (Taffy compact-length internals), so it can never sit behind the probe's socket-thread mutex — the `LayoutStore` push model is the same pattern as `on_frame`/`absorb_events`: the app extracts the snapshot on its own thread and the channel reads it lock-free. Capabilities the runtime genuinely lacks (interactive inspector pick overlay, `theme_apply` `system` mode, node-scoped signals without a probe) return honest `not_implemented:` errors rather than fabricated state (D9).

## 3.2 MCP Client Configuration

Any stdio-capable MCP client attaches via `cargo martensite mcp`:

```jsonc
// .devin/mcp_config.json (Devin), .mcp.json (Claude Code),
// or the client's equivalent mcpServers block:
{
  "mcpServers": {
    "martensite": {
      "command": "cargo-martensite",
      "args": ["mcp", "--workspace", "/path/to/workspace"]
    }
  }
}
```

Flags: `--socket <path>` pins a specific dev channel (otherwise auto-discovery scans `$XDG_RUNTIME_DIR/martensite/` and `MARTENSITE_DEV_SOCKET`), `--scene <path>` supplies an offline scene dump, `--workspace <path>` sets the workspace root for scaffolding/lint confinement, `--allow-version-mismatch` bypasses the D1 handshake check (not recommended), `--offline` disables dev-session attach entirely so live-only tools report the offline hint (hermetic test/CI runs).

## 4. MCP Dynamic Resources & Event Streams

The server exposes scoped, low-overhead MCP resources:

| Resource URI | Description | Notification Invalidation Trigger |
| :--- | :--- | :--- |
| `martensite://app/tree/summary` | Lightweight structural summary (node count, depth, root IDs) | Layout pass completion ping |
| `martensite://app/tree/{root_id}` | Scoped subtree JSON hierarchy for target node | Scoped dirty update |
| `martensite://app/a11y` | Full hierarchical AccessKit semantic tree | Semantic tree sync ping |
| `martensite://app/lint` | Latest design lint report across all standards | Scene update / frame commit |
| `martensite://app/signals` | Active signal registry summary and dirty bitset | Signal scheduler tick |
| `martensite://app/events/ledger` | Tail of the last 100 routed events | Event dispatch |
| `martensite://app/tweaks` | Current state of all registered live tweaks | Tweak mutation |
| `martensite://theme/tokens` | Active theme palette, Oklab colors, and token bindings | Theme switch |
| `martensite://standards/{standard}` | Markdown documentation and citations for a design standard | Static |

### Proactive Notification Streams:
The server emits proactive server-sent MCP event notifications for critical runtime anomalies:
- `martensite/overflow`: Emitted when layout computation detects clipping content exceeding parent bounds.
- `martensite/reload`: Emitted when `martensite-host` swaps a new cdylib or when compilation fails.
- `martensite/frame_budget`: Emitted when a frame exceeds the 16.6ms (60fps) or 8.3ms (120fps) deadline.

---

## 5. Standard MCP Prompt Templates

Pre-packaged prompt workflows guide AI agents through complex multi-step UI engineering tasks:

1. `audit_screen`: Evaluates the current screen against WCAG 2.2 AAA, checks for text clipping, audits color contrast, and lists prioritized remediations.
2. `fix_overflow`: Takes a reported overflow, diagnoses the Taffy constraint failure, proposes minimal code edits, and applies a live tweak preview to verify the fix.
3. `refactor_reactive_widget`: Inspects an imperative widget, identifies static state bottlenecks, and refactors it into fine-grained `Signal` and `Memo` primitives.
4. `implement_accessible_pattern`: Validates a composite widget against the W3C ARIA Authoring Practices Guide (APG) and emits the necessary AccessKit node bindings.

---

## 6. Verified Invariants & Safety Constraints

1. **Version-Lock Enforcement (D1):** The MCP server validates protocol and crate versions on connect. Skew terminates the session with an actionable diagnostic.
2. **Zero-Cost Release Boundary (D8):** Dev-channel IPC, tweak registers, and live inspector hooks compile out in release builds.
3. **Semantic Honesty (D9):** The MCP server never synthesizes or approximates layout or accessibility state. Every response derives directly from the authoritative `WidgetArena`, `LayoutEngine`, or `AccessKit` state.
4. **Guarded Disk Mutation:** Source writes via `martensite_sync_tweaks_to_source` are preview-only (`dry_run: true`) by default, require `confirmed: true`, are strictly workspace-confined, and maintain an audit log.
5. **Fail-Safe Degradation:** If the live application closes or crashes, the MCP server automatically transitions to Offline Mode without aborting the host agent's connection.

---

## 7. Exit Criteria & Verification Gates

1. **Integration Test Suite:** Automated headless test (`tests/mcp_stdio_smoke.rs`) boots a mock Martensite dev app, launches `cargo martensite mcp`, and exercises all 28 tools over stdio with 100% pass rate; `tests/dev_session_socket_e2e.rs` additionally proves the full MCP → socket → `DevSession` → `WidgetArenaProbe` → live `WidgetArena` path over a real bound socket.
2. **Sub-Millisecond Query Gate:** `martensite_inspect_tree` and `martensite_diagnose_layout` execute in `< 1.0ms` for a 10,000-node scene.
3. **Bidirectional Tweak Verification:** A live tweak mutation applied via MCP updates the running scene, survives a hot reload by name, and successfully writes back to source code.
4. **Agent Conformance:** Verified interoperability with Antigravity, Devin, and Claude Code stdio configurations.
