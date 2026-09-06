# Contributing to Martensite

Thank you for your interest in contributing to **Martensite**, a retained-mode, GPU-accelerated GUI framework for Rust. This document outlines the technical standards, workflows, and processes required to contribute to the framework.

## 1. Core Principles
Before submitting any code, please review our foundational architecture documents:
* **Core Architectural Principles** in [`docs/charter/CHARTER.md`](docs/charter/CHARTER.md)
* **Governance Charter** in [`docs/governance/GOVERNANCE.md`](docs/governance/GOVERNANCE.md)

All contributions should adhere to the following guidelines:
1. **Bounded Memory & Zero Runtime GC**: No dynamic heap allocations in the active interaction and rendering loop.
2. **Event-Driven Sleep**: The event loop yields to OS wait states during idle periods.
3. **Pure-Rust Toolchain**: Pure Rust supply chain. Avoid dependencies requiring external C/C++ compilers or foreign build scripts.
4. **Code Quality**: `#![forbid(unsafe_code)]` in all public-facing crates. Any internal `unsafe` requires an explicit `// SAFETY:` invariant explanation.

## 2. Development Environment Setup

Martensite targets a reproducible pure-Rust toolchain.

### Prerequisites
1. **Rust Toolchain**: Install via [rustup](https://rustup.rs/). We track the stable channel.
   ```bash
   rustup default stable
   ```
2. **Cargo Deny** (for supply chain auditing):
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

Before opening a PR, your branch must pass the test and lint pipeline executed by CI.

### Running Tests
All unit tests and doctests must pass without warnings:
```bash
cargo test --all-targets --all-features
```

### Running Benchmarks
If you touch `martensite-layout`, `martensite-reactive`, or the rendering pipeline, verify performance using Criterion:
```bash
cargo bench
```
*Note: Any PR modifying performance-sensitive code should include benchmark results in the PR description.*

### Static Analysis
```bash
# Ensure formatting
cargo fmt --all -- --check

# Lints must pass with zero warnings
cargo clippy --all-targets --all-features -- -D warnings

# Ensure no unvetted C/C++ dependencies have been introduced
cargo deny check bans

# Ensure no unintended breaking changes to public APIs
cargo semver-checks
```

## 4. Submitting an RFC

Major architectural changes, new widgets, or API alterations require an RFC.

1. **Pre-RFC**: Discuss the idea in Discussions or chat.
2. **Draft**: Copy [`docs/rfcs/0000-template.md`](docs/rfcs/0000-template.md) to `docs/rfcs/text/0000-my-feature.md`.
3. **Write**: Fill out the technical specification following our documentation standards (concise, factual, with verifiable mechanisms).
4. **Submit**: Open a PR to the `martensite-rfcs` repository.
5. **Review**: The relevant Working Group will review the proposal.
6. **Final Comment Period**: If approved, a 10-day FCP period begins before final ratification.

## 5. Commit and PR Standards

### Commit Message Format
We follow [Conventional Commits](https://www.conventionalcommits.org/) for automated changelogs and release notes.

```text
<type>(<optional scope>): <description>

[optional body]

[optional footer(s)]
```

* **Types**: `feat` (new feature), `fix` (bug fix), `perf` (performance), `docs` (documentation), `refactor` (code restructuring), `chore` (maintenance).
* **Example**: `perf(layout): batch taffy constraint resolution`

### Pull Request Title Format
The PR title should match the primary commit message format (e.g., `feat(button): add physical press animation curve`).

## 6. Code Review Process & Timelines

1. **Self-Review**: Verify your PR against [`docs/CODE_REVIEW_CHECKLIST.md`](docs/CODE_REVIEW_CHECKLIST.md) before requesting review.
2. **Assigning**: Mention the relevant Working Group (e.g., `@martensite/wg-graphics`) in your PR description.
3. **Turnaround**: Reviewers operate asynchronously. An initial technical review will typically occur within 48 to 72 hours.
4. **Ecosystem Testing**: PRs run regression checks against curated ecosystem crates in `martensite-blessed`. If a change breaks a downstream crate, coordinate with maintainers before merging.
