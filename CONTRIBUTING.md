# Contributing to Martensite

Thank you for your interest in contributing to **Martensite**, the sovereign native GUI engine for Rust. This document outlines the technical standards, workflows, and processes required to contribute to the framework.

## 1. The Iron Invariants
Before submitting any code, you must read and understand our constitution:
* **The Ten Golden Laws** in [`docs/charter/CHARTER.md`](docs/charter/CHARTER.md)
* **The Governance Charter** in [`docs/governance/GOVERNANCE.md`](docs/governance/GOVERNANCE.md)

All contributions must strictly adhere to:
1. **The Zero-GC Law**: No allocations in the active interaction and rendering loop.
2. **The Event-Sleep Law**: Absolute 0.00% CPU/GPU utilization during static idle periods.
3. **The Zero C/C++ FFI Law**: Pure Rust supply chain. Never add dependencies that require native C compilers.
4. **Code Quality**: `#![forbid(unsafe_code)]` in all public-facing crates. Any internal `unsafe` requires an explicit `// SAFETY:` proof.

## 2. Development Environment Setup

Martensite mandates a strict, reproducible pure-Rust toolchain.

### Prerequisites
1. **Rust Toolchain**: Install via [rustup](https://rustup.rs/). We strictly track the stable channel.
   ```bash
   rustup default stable
   ```
2. **Cargo Deny** (for C/C++ supply chain auditing):
   ```bash
   cargo install cargo-deny
   ```
3. **Cargo Semver Checks** (for ABI/API stability verification):
   ```bash
   cargo install cargo-semver-checks
   ```

### Building the Project
```bash
git clone https://github.com/martensite/martensite.git
cd martensite
cargo build --all-targets --all-features
```

## 3. Testing and Verification

Before opening a PR, your branch must pass the exact pipeline executed by CI.

### Running Tests
All unit tests and doctests must pass without warnings:
```bash
cargo test --all-targets --all-features
```

### Running Benchmarks
If you touch `martensite-layout`, `martensite-reactive`, or the rendering pipeline, you must verify performance regressions using Criterion:
```bash
cargo bench
```
*Note: Any PR modifying performance-sensitive code must include the benchmark output in the PR description.*

### Static Analysis
```bash
# Ensure strict formatting
cargo fmt --all -- --check

# Lints must pass with zero warnings
cargo clippy --all-targets --all-features -- -D warnings

# Ensure no foreign C/C++ dependencies have leaked in
cargo deny check bans

# Ensure no Tier 1 breaking changes
cargo semver-checks
```

## 4. Submitting an RFC

Major architectural changes, new widgets, or API alterations cannot be merged via unilateral PRs. They must traverse the 6-Phase RFC Process.

1. **Pre-RFC**: Pitch your idea in Discussions.
2. **Draft**: Copy [`docs/rfcs/0000-template.md`](docs/rfcs/0000-template.md) to `docs/rfcs/text/0000-my-feature.md`.
3. **Write**: Fill out the technical specification following the Anti-Slop Doctrine (no hype, just verifiable mechanisms).
4. **Submit**: Open a PR to the `martensite-rfcs` repository.
5. **Review**: The relevant Working Group will audit the design.
6. **Final Comment Period**: If approved, a 10-day FCP clock starts before final ratification.

## 5. Commit and PR Standards

### Commit Message Format
We strictly follow [Conventional Commits](https://www.conventionalcommits.org/). This automates our SemVer generation and changelogs.

```text
<type>(<optional scope>): <description>

[optional body]

[optional footer(s)]
```

* **Types**: `feat` (new feature), `fix` (bug fix), `perf` (performance), `docs` (documentation), `refactor` (code restructuring), `chore` (maintenance).
* **Example**: `perf(layout): batch taffy constraint resolution`

### Pull Request Title Format
The PR title must match the primary commit message format (e.g., `feat(button): add physical press animation curve`).

## 6. Code Review Process & Timelines

1. **Self-Review**: You must verify your PR against [`docs/CODE_REVIEW_CHECKLIST.md`](docs/CODE_REVIEW_CHECKLIST.md) before requesting review.
2. **Assigning**: Mention the relevant Working Group (e.g., `@martensite/wg-graphics`) in your PR description.
3. **Turnaround**: Reviewers operate asynchronously. Expect an initial technical review within 48 to 72 hours.
4. **Ecosystem Shield**: The PR will trigger Crater-CI against the `martensite-blessed` ecosystem. If your PR breaks a blessed downstream package, you must coordinate a fix with the maintainers before your PR can be merged.
