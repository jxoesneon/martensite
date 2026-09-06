# Release Engineering Process

**Document Identifier:** DOC-0002-RELEASE
**Status:** Maintained
**Target:** v1.0.0

## 1. Versioning Scheme

Martensite strictly adheres to **Semantic Versioning 2.0.0**. 
Prior to `1.0.0`, minor versions (`0.x.0`) may contain breaking changes, though we adhere to the stability tiers defined in `GOVERNANCE.md`.

* **Pre-release Tags:** Releases are staged using standard suffixes (`-alpha.N`, `-beta.N`, `-rc.N`).
* **v1.0.0 Stability Guarantee:** Once 1.0.0 is released, **zero breaking changes** are permitted to public APIs, trait signatures, or reactive semantics until 2.0.0. `cargo-semver-checks` enforces this in CI.

## 2. Branching Strategy

* `main`: The eternal trunk. All PRs target `main`. Must pass CI and remain usable.
* `release/0.x`: Stabilization branches for minor releases. Only bugfixes and documentation are cherry-picked here.
* `hotfix/*`: Emergency patches for severe regressions or security issues on published versions.

## 3. Topologically Sorted Publication Sequence

Due to the strict workspace constraints and inter-crate dependencies, the 24 internal crates (21 core crates, 1 CLI tool, 1 example, 1 bench suite) must be published in a strict bottom-up leaf-first topological order. `examples` and `benches` are excluded from crates.io.

1. `martensite-macros`
2. `martensite-core`
3. `martensite-reactive`
4. `martensite-layout`
5. `martensite-theme`
6. `martensite-motion`
7. `martensite-history`
8. `martensite-assets`
9. `martensite-l10n`
10. `martensite-text`
11. `martensite-access`
12. `martensite-focus`
13. `martensite-clipboard`
14. `martensite-dnd`
15. `martensite-media`
16. `martensite-render`
17. `martensite-wgpu`
18. `martensite-window`
19. `martensite-test`
20. `martensite-devtools`
21. `cargo-martensite`
22. `martensite` (The facade)
*(Not published: `examples/industrial_dashboard`, `benches/bench_suite`)*

## 4. Changelog & GitHub Releases

Martensite uses the **Keep a Changelog** format.
* Before release, the Release Manager updates `CHANGELOG.md` with explicit categories: `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`.
* The GitHub Release notes are generated from the changelog and must explicitly list the `MSRV`.

## 5. MSRV Bump Policy

* MSRV (Minimum Supported Rust Version) bumps are treated as **minor version bumps**.
* A 6-month public notice must be provided in the changelog before a planned MSRV bump.

## 6. Yanking Policy

Crates will only be yanked from crates.io in two scenarios:
1. **Critical Security Vulnerability:** (See `SECURITY.md`).
2. **Fatal Regression:** A bug that irreversibly corrupts user data or causes widespread immediate compilation failure across all supported platforms.
*Yanking is never used for minor bugs or aesthetic API regrets.*
