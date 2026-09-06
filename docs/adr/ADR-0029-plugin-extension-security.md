# ADR-0029 — Plugin and Extension Security (Wasmtime Sandbox)

* **Status:** Accepted — Decision Finalised
* **Date:** 2026-09-06
* **Deciders:** Jose Eduardo Rojas Jimenez (Sovereign Architect)
* **Technical Domain:** `martensite-plugin` (new crate, Phase 6), `martensite`
* **OQ:** OQ-4 — resolved 2026-09-06

---

## Context

The Widget trait (ADR-0018) defines how trusted, same-process Rust code extends the widget hierarchy. That covers first-party crates and application widgets. It does not cover **runtime-loaded, third-party plugins** — code that arrives as a binary artifact (from a marketplace, plugin directory, or user install) and must be executed without trusting the author.

Use cases that require this:
- **DAW plugins** (VST/CLAP-style GUI extensions, instrument UIs)
- **CAD tool extensions** (geometry import plugins, analysis panels)
- **Developer tool addons** (language server UI panels, diff view plugins)

The risk: a malicious or buggy plugin can read the host process's memory, crash it, exfiltrate data, or corrupt the widget arena.

## Decision Drivers

- **Safety:** A plugin must not be able to corrupt the host process or the widget arena.
- **Performance:** Plugin IPC overhead must not affect the host frame rate.
- **Scope commitment:** Deferring to v1.1 would exclude Martensite from DAW/CAD markets at launch.
- **Pure-Rust mandate (ADR-0005):** The sandbox runtime must not require C build dependencies.

## Considered Options

### Option A — Defer to v1.1
Ship v1.0 with only the Widget trait (trusted, same-process). Sandboxed plugins come later.

**Rejected.** The Sovereign Architect confirmed: ship at v1.0. The industrial workstation use cases (DAW, CAD, trading terminal) are first-class targets, not future work.

### Option B — OS process isolation
Each plugin runs in a separate OS process; host communicates via IPC (pipes, sockets, shared memory). Heavy: plugin startup time is 50–200ms, IPC latency is 1–5ms per frame call.

**Rejected.** Per-frame IPC latency at 60Hz = 6ms budget per frame, not viable for complex plugin UIs.

### Option C — Wasmtime sandbox with capability grants (chosen)
Plugins are compiled to `wasm32-wasi` and executed inside a per-plugin `wasmtime::Store`. The host exposes a capability interface (WASI + Martensite Plugin ABI) that the plugin calls via `wasmtime::Func` trampolines. Memory is isolated: each plugin has its own linear memory, cannot address host memory.

## Decision Outcome

**Option C — Wasmtime sandbox with explicit capability grants at v1.0.**

### Plugin ABI Contract

Plugins implement a fixed ABI exported from their wasm module:

```rust
// Plugin side (compiled to wasm32-wasi):
#[no_mangle]
pub extern "C" fn plugin_init(ctx: u32) -> u32;       // returns plugin handle
#[no_mangle]
pub extern "C" fn plugin_build(handle: u32, cx: u32); // build widget tree
#[no_mangle]
pub extern "C" fn plugin_event(handle: u32, ev: u32); // deliver event
#[no_mangle]
pub extern "C" fn plugin_destroy(handle: u32);
```

### Host Capability Model

Capabilities are granted explicitly at load time — no ambient authority:

```rust
Plugin::load("path/to/plugin.wasm")
    .grant(Capability::SignalRead)      // may read named signals
    .grant(Capability::SignalWrite)     // may set named signals
    .grant(Capability::FileRead("/project/assets/")) // scoped fs read
    .grant(Capability::Network)         // outbound HTTP (off by default)
    .mount(cx, widget_slot);
```

### Performance Invariants

- Wasmtime JIT compilation on first load: 50–500ms (one-time cost, cached)
- Per-call trampoline overhead: ~50ns (acceptable within frame budget)
- Plugin linear memory: isolated, max 256MB per plugin by default
- Plugin render output: submitted as a `PaintList` substream — same path as any widget

### Security Invariants

- Plugins cannot address host memory (wasm linear memory isolation)
- Plugins cannot call arbitrary host functions (only capability-granted imports)
- Plugins cannot spawn threads (WASI thread proposal not enabled)
- `wasmtime` is a pure-Rust crate; no C build dependencies (ADR-0005 satisfied)

### New Crate: `martensite-plugin`

This functionality lives in a new workspace crate added at Phase 6:
- `crates/martensite-plugin/` — `wasmtime` dependency, Plugin loader, ABI trampoline layer
- Not part of the `martensite` facade by default — opt-in via feature flag `plugins`
- Pure-Rust: `wasmtime` pulls no C build system dependencies

## Consequences

**Positive:**
- Martensite is viable for DAW, CAD, and trading terminal markets at v1.0
- Plugin authors can ship pre-compiled `.wasm` artifacts — no Rust toolchain required
- Host process is safe from malicious or buggy plugins

**Negative:**
- Adds `martensite-plugin` as a new crate (not in current workspace — must be scaffolded in Phase 6)
- `wasmtime` is a large dependency (~10MB compiled); gated behind the `plugins` feature
- Plugin debugging is harder than native code (wasm stack traces, no native debugger attach)
- Adds 2–3 months to v1.0 timeline — acknowledged and accepted by Sovereign Architect

## Implementation Notes

- Phase 6 milestone: `martensite-plugin` crate, Plugin ABI v1 definition, capability model
- The Widget trait (ADR-0018) remains the path for trusted in-process extensions — plugins are a distinct, higher-isolation layer
- Plugin wasm modules are compiled offline with `cargo build --target wasm32-wasi`; no runtime Rust compilation
