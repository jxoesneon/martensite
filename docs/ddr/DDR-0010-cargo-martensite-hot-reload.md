# [DDR-0010] Sub-Second Hot-Reloading & Dynamic Toolchain Engine Specification

* **Subsystem:** `cargo-martensite`
* **Status:** Approved
* **Authors:** Martensite Architecture Working Group
* **Related ADRs:** ADR-0001, ADR-0002

## 1. System Topology & Host-Guest cdylib Architecture

`cargo-martensite` delivers sub-second hot-reloading for client UI development without sacrificing native binary speed or Rust's compile-time type safety.
It partitions the running application into two layers:
1. **The Persistent Host Executable**: Manages OS windowing (`winit`), GPU device contexts (`wgpu`), audio pipelines, and the root reactive signal memory store.
2. **The Dynamic Guest Dynamic Library (`cdylib`)**: Compiles component view declarations, styling rules, and layout structures.

### 1.1 Architecture & Shadow Copying
```
┌──────────────────────────────────────────────────────────────┐
│                 CARGO-MARTENSITE HOT-RELOAD                  │
├──────────────────────────────────────────────────────────────┤
│  `cargo-martensite watch` (Background Compiler Daemon)       │
│    • Watches filesystem for `.rs` mutations                  │
│    • Incremental compilation via Cranelift & `mold` / `lld`  │
│                            │                                 │
│                            ▼                                 │
│  Shadow Copy Generation (`target/hot/app_vN.dll`)            │
│    • Bypasses OS file locking (crucial on Windows)           │
│                            │                                 │
│                            ▼                                 │
│  Host Dynamic Reload Pipeline                                │
│    • Pause event loop frame dispatch                         │
│    • Unload previous guest `cdylib` handle via `libloading`  │
│    • Load new guest shadow copy                              │
│    • Re-attach Host Signal Store & re-execute view tree      │
│    • Resume frame loop (<350ms total elapsed time)           │
└──────────────────────────────────────────────────────────────┘
```

* **Invariant 1.1 (State Preservation)**: Hot-reloading the guest library must never reset existing reactive signal values or active user input buffers.
* **Invariant 1.2 (Sub-Second Turnaround)**: Incremental component rebuilds must finalize in $<500\text{ms}$ on developer workstations.
* **Invariant 1.3 (Zero Production Overhead)**: Release builds compile down to a single, monolithic, statically linked executable with zero dynamic library loading overhead.
