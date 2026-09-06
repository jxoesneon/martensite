# Contributing to Martensite

Thank you for your interest in contributing to **Martensite**, the sovereign native GUI engine for Rust.

## The Iron Invariants
Before submitting any code, review **The Ten Golden Laws** in [`docs/charter/CHARTER.md`](docs/charter/CHARTER.md) and the **Governance Charter** in [`docs/governance/GOVERNANCE.md`](docs/governance/GOVERNANCE.md).

All contributions must strictly adhere to:
1. **The Zero-GC Law**: No allocations in the active interaction and rendering loop.
2. **The Event-Sleep Law**: Absolute 0.00% CPU/GPU utilization during static idle periods.
3. **The Zero C/C++ FFI Law**: Pure Rust supply chain. Never add dependencies that require native C compilers.
4. **Code Quality**: `#![forbid(unsafe_code)]` in all public-facing crates. Any internal `unsafe` requires an explicit `// SAFETY:` proof.

## Submitting Pull Requests
1. All non-trivial feature additions require an approved RFC via the `docs/rfcs/` process.
2. Ensure `cargo fmt --all -- --check` and `cargo clippy --all-targets --all-features` pass cleanly.
3. Ensure `cargo deny check bans` passes without introducing C dependencies.
4. Include reproducible Criterion benchmark comparisons in `benches/` for any changes affecting layout, reactivity, or rendering loops.
