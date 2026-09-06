# Martensite Pull Request Review Checklist

This checklist is used for all Pull Requests submitted to the Martensite project. Reviewers verify these constraints before merging.

## 1. Core Principles Verification
- [ ] **Pure-Rust Supply Chain:** `cargo deny check bans` passes. No new C/C++ dependencies (`-sys` crates requiring `cmake` or a C compiler) have been introduced.
- [ ] **Bounded Memory & Zero Runtime Allocation:** No dynamic heap allocations (`Vec::push`, `Box::new`, `String::new`, `.clone()` on heap types) exist in the hot layout, interaction, or rendering loop.
- [ ] **Event-Driven Sleep:** The event loop yields to OS wait states when idle. No continuous polling loops have been added.
- [ ] **Reactive State Model:** State mutations rely on fine-grained signals. No tree-walking diff algorithms have been introduced.

## 2. Memory & Architecture
- [ ] **No `Rc<RefCell<T>>`:** The widget hierarchy remains free of interior mutability wrappers and cycle-prone pointer graphs. References use lightweight generational handles (`WidgetId`).
- [ ] **Two-Pass Layout:** Layout measurement remains strictly decoupled from layout placement. No single-frame layout oscillations are possible.
- [ ] **Error Exhaustiveness:** New error `enum`s are exhaustive or marked with `#[non_exhaustive]` if expected to expand.

## 3. Unsafe & Invariants
- [ ] **No Unsafe in Core:** `#![forbid(unsafe_code)]` remains intact in user-facing core crates (`martensite`, `martensite-core`, `martensite-reactive`).
- [ ] **`// SAFETY:` Comments:** Every `unsafe` block in hardware-level crates (e.g., `martensite-wgpu`) is preceded by an auditable `// SAFETY:` comment documenting prerequisites and invariants.
- [ ] **`// INVARIANT:` Comments:** Internal structural guarantees are documented with `// INVARIANT:`.

## 4. Documentation & API
- [ ] **Rustdoc Completeness:** New public APIs (`pub fn`, `pub struct`, `pub trait`) include complete documentation following `DOCUMENTATION_STANDARDS.md`.
- [ ] **Executable Examples:** New public APIs include at least one executable `# Examples` doctest.
- [ ] **Limitations Documented:** The `# Limitations` section documents explicit boundaries of new APIs.
- [ ] **Objective Language:** Documentation uses objective, measurable language without promotional phrasing ("blazingly fast", "magic").

## 5. Testing & Performance
- [ ] **Unit Tests:** New public APIs have comprehensive unit tests.
- [ ] **Criterion Benchmarks:** Any PR modifying layout, reactivity, text shaping, or rendering loops includes a reproducible Criterion benchmark comparison in `benches/`.
- [ ] **AccessKit Sync:** Interactive widgets implement `accesskit::TreeUpdate` synchronization for accessibility support.

## 6. Process & Tooling
- [ ] **Formatting:** `cargo fmt --all -- --check` passes cleanly.
- [ ] **Clippy:** `cargo clippy --all-targets --all-features -- -D warnings` passes without warnings.
- [ ] **SemVer Contract:** `cargo semver-checks` passes. There are no unintended breaking changes to Tier 1 Core Stable APIs.
- [ ] **Architectural Decisions:** If the PR introduces a major structural change, the relevant RFC or ADR (Architecture Decision Record) is referenced in the PR description.
- [ ] **Commit Messages:** Commits follow Conventional Commits format (`feat:`, `fix:`, `perf:`, `docs:`, `refactor:`).
- [ ] **Ecosystem CI:** The automated CI run against `martensite-blessed` crates passes without regressions.
