# martensite-mcp

First-party Model Context Protocol (MCP) server for AI-assisted Martensite development.

`martensite-mcp` exposes native, semantic observability and developer toolchain capabilities directly to autonomous AI agents (such as Devin, Claude Code, Cursor, and Windsurf) via the standard Model Context Protocol.

Rather than relying on lossy computer-vision screenshots or blind static code generation, `martensite-mcp` connects directly to the Martensite engine runtime over local Unix domain socket IPC (`$XDG_RUNTIME_DIR/martensite/<build_id>.sock`) or runs in offline mode for static analysis, design linting, and AST scaffolding.

## Architectural Principles

1. **Semantic Honesty (D9):** Never synthesizes or approximates layout or accessibility state. Queries the live `WidgetArena`, Taffy layout chain, reactive signal DAG, and AccessKit trees directly.
2. **Version Lock (D1):** Enforces strict version matching between agent, MCP server, and running application to prevent skew.
3. **Zero Production Overhead (D8):** Dev-channel IPC hooks compile out completely in release builds (`--release`).
4. **Guarded Source Synchronization:** Live parameter tweaks support safe source code write-back with dry-run previews by default, mandatory confirmation gates, and strict Cargo workspace `src/` boundary enforcement.
5. **Local-Only Stdio Transport:** Standard MCP JSON-RPC over `stdio` without exposing unauthenticated network ports or remote attack surfaces.

## Tool Suite (28 Semantic Tools)

### Category A: Widget Arena & Hierarchy Inspection
- `martensite_inspect_tree`: Hierarchical widget tree inspection with horizontal breadth clamping and pagination.
- `martensite_get_node`: Complete single-node details (bounds, kind, markers, ARIA roles, source spans).

### Category B: Layout & Constraint Diagnostics
- `martensite_diagnose_layout`: Inbound Taffy constraints, measure closure results, and layout step chains.
- `martensite_explain_overflow`: Exact mathematical causes and clipping lineages of layout overflows with suggested remediations.

### Category C: Reactive State & Signal DAG
- `martensite_inspect_signals`: Push-pull reactive signal DAG inspection, subscriber counts, and dirty bitsets.
- `martensite_trigger_signal`: Type-safe live signal mutation for state transition testing.
- `martensite_set_signal`: JSON signal writes through registered `SignalAdapter`s (type-checked by the app).

### Category D: Accessibility & Semantic Contract
- `martensite_inspect_a11y_tree`: Hierarchical AccessKit accessibility tree and W3C APG pattern conformance checks.
- `martensite_invoke_accessibility_action`: Dispatch a `SemanticAction` (click, focus, set_value, scroll_to_point, ...) to a widget or the focused node.

### Category E: Design Standards & Lint Enforcement
- `martensite_lint_scene`: 51-rule design standards engine evaluation (WCAG 2.2, ISA-101, Gestalt, Hick-Fitts).
- `martensite_apply_lint_fix`: Applies safe or risky fixes to the in-memory `LintScene` preview model or emits git diff patches.
- `martensite_audit_paint`: Text ink descender overlap checks, WCAG contrast audits, and 3:1 non-text keyline verification.

### Category F: Live Tweaks & Source Synchronization
- `martensite_list_tweaks`: Discovers active tweakable variables registered via `#[tweak]` or `TweakRegistry`.
- `martensite_set_tweak`: Sub-millisecond runtime tweak mutations.
- `martensite_set_theme`: Dark/light mode switching and dynamic design token palette overrides.
- `martensite_sync_tweaks_to_source`: Workspace-confined, audited source code write-back (`dry_run: true` default, `confirmed: true` gate).

### Category G: Interaction, Replay & Visual Verification
- `martensite_dispatch_event`: Synthetic pointer, keyboard, and scroll event routing.
- `martensite_get_event_ledger`: Event routing ledger and hit-test rejection diagnostics.
- `martensite_step_timemachine`: Deterministic time-travel debugging (step, pause, checkpoint restore).
- `martensite_capture_node`: Direct offscreen widget subtree image rendering to disk artifacts.
- `martensite_render_headless`: Headless test harness rendering under `VirtualClock`.

### Category H: Toolchain, Lifecycle & Scaffolding
- `martensite_doctor`: Graphic adapter, display server, and compiler toolchain diagnostics.
- `martensite_reload_status`: Hot-reload cdylib lifecycle and compiler diagnostic telemetry.
- `martensite_hot_reload`: Requests a guest-code rebuild+reload via the dev coordinator's reload-request marker.
- `martensite_scaffold_widget`: Idiomatic, standards-compliant Martensite widget source code generation.

### Category I: Runtime Diagnostics
- `martensite_runtime_errors`: Captured panics, `ErrorSurface` diagnostics, and reactive evaluation errors with severity filtering and clear-after-read.
- `martensite_logs`: Bounded tracing ring-buffer drain with level, target-prefix, and message-substring filters.

### Category J: Loading-State Forcing (ADR-0040)
- `martensite_set_loading`: Forces or clears a node's loading state (placeholder chrome, `busy`/`disabled` semantics) for verification loops.

## Running & Client Setup

The server speaks MCP over `stdio`; register it in any MCP client:

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

`cargo-martensite` is installed via `cargo install --path tools/cargo-martensite`.

**Flags:** `--socket <path>` pins a dev channel (default: auto-discover via `MARTENSITE_DEV_SOCKET` then `$XDG_RUNTIME_DIR/martensite/*.sock`), `--scene <path>` enables offline scene analysis, `--workspace <path>` confines scaffolding/lint writes, `--allow-version-mismatch` skips the version handshake check (not recommended).

**Live mode:** the target app opts in by building with the `dev-channel` feature and running with `MARTENSITE_DEV_CHANNEL=1` (debug builds only — see "App-Side Integration" in `docs/dx/MCP.md`). With no live app the server stays useful offline: static lint, scaffolding, doctor, and headless rendering.

## Resources & Prompt Templates

- **Resources:** `martensite://app/tree/summary`, `martensite://app/tree/{root_id}`, `martensite://app/a11y`, `martensite://app/lint`, `martensite://app/signals`, `martensite://app/events/ledger`, `martensite://app/tweaks`, `martensite://theme/tokens`.
- **Prompts:** `audit_screen`, `fix_overflow`, `refactor_reactive_widget`, `implement_accessible_pattern`.

## Specification & References

- [ADR-0039: First-Party Model Context Protocol Server](../../docs/adr/ADR-0039-mcp-server-for-ai-assisted-development.md)
- [MCP Specification (W10)](../../docs/dx/MCP.md)
