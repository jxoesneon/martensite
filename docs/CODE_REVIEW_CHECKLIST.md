# Martensite Pull Request Review Checklist

This checklist is mandatory for all Pull Requests submitted to the Martensite project. Reviewers are required to verify these constraints before merging. If a PR violates any of these rules without an explicit, documented exemption, it must be rejected.

## 1. The Golden Laws Verification
- [ ] **Zero C/C++ Law:** `cargo deny check bans` passes. No new C/C++ dependencies (`-sys` crates requiring `cmake` or a C compiler) have been introduced.
- [ ] **Zero-GC & Allocation Law:** No heap allocations (`Vec::push`, `Box::new`, `String::new`, `.clone()` on heap types) exist in the hot layout, interaction, or rendering loop.
- [ ] **Event-Sleep Law:** The application still drops to 0.00% CPU/GPU usage when idle. No new continuous polling loops have been added.
- [ ] **No Virtual DOM Law:** State mutations rely on fine-grained signals. No tree-walking diff algorithms have been introduced.

## 2. Memory & Architecture
- [ ] **No `Rc<RefCell<T>>`:** The widget hierarchy remains free of interior mutability wrappers and cycle-prone pointer graphs. All references use lightweight generational handles (`WidgetId`).
- [ ] **Two-Pass Layout:** Layout measurement remains strictly decoupled from layout placement. No single-frame layout oscillations are possible.
- [ ] **Error Exhaustiveness:** New error `enum`s are exhaustive. If an error enum is expected to grow in the future, it is marked with `#[non_exhaustive]`.

## 3. Unsafe & Invariants
- [ ] **No Unsafe in Core:** `#![forbid(unsafe_code)]` remains intact in user-facing core crates (`martensite`, `martensite-core`, `martensite-reactive`).
- [ ] **`// SAFETY:` Comments:** Every single `unsafe` block in hardware-level crates (e.g., `martensite-wgpu`) is immediately preceded by an auditable `// SAFETY:` comment proving its memory safety invariants.
- [ ] **`// INVARIANT:` Comments:** New internal structural guarantees are documented with `// INVARIANT:`.

## 4. Documentation & API
- [ ] **Rustdoc Completeness:** All new public APIs (`pub fn`, `pub struct`, `pub trait`) possess complete documentation following `DOCUMENTATION_STANDARDS.md`.
- [ ] **Executable Examples:** All new public APIs include at least one executable `# Examples` doctest.
- [ ] **Limitations Documented:** The `# Limitations` section is populated for new APIs, honestly assessing what it cannot do.
- [ ] **No AI Slop / Hype:** Documentation uses objective, measurable language. Banned phrases ("blazingly fast", "magic") are absent.

## 5. Testing & Performance
- [ ] **Unit Tests:** New public APIs have comprehensive unit tests.
- [ ] **Criterion Benchmarks:** Any PR modifying layout, reactivity, text shaping, or rendering loops includes a reproducible Criterion benchmark comparison in `benches/`.
- [ ] **AccessKit Sync:** If a new interactive widget was added, its `accesskit::TreeUpdate` lifecycle method is fully implemented for native screen reader support.

## 6. Process & Tooling
- [ ] **Formatting:** `cargo fmt --all -- --check` passes cleanly.
- [ ] **Clippy:** `cargo clippy --all-targets --all-features -- -D warnings` passes without emitting warnings.
- [ ] **SemVer Contract:** `cargo semver-checks` passes. There are no accidental breaking changes to Tier 1 Core Stable APIs. (If breaking changes are necessary, an approved RFC is required).
- [ ] **Architectural Decisions:** If the PR introduces a major structural change, the relevant RFC or ADR (Architecture Decision Record) is referenced in the PR description.
- [ ] **Commit Messages:** Commits follow Conventional Commits format (`feat:`, `fix:`, `perf:`, `docs:`, `refactor:`).
- [ ] **Ecosystem Shield:** The automated Crater-CI run against `martensite-blessed` crates passes without regressions.
