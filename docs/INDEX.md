# Martensite — Documentation Index

**Repository:** https://github.com/jxoesneon/martensite  
**Build:** `cargo check --workspace` ✅ (Rust 1.98.1, 0 errors)  
**Spec State:** Complete — all ADRs, DDRs, and process docs written for v1.0.0

---

## Quick Navigation

| Need | Go to |
|------|-------|
| Understand the project | [Charter](charter/CHARTER.md) |
| Understand decisions | [ADR Index](#adrs) |
| Understand crate internals | [DDR Index](#ddrs) |
| Write code against Martensite | [Public API Design](PUBLIC_API_DESIGN.md) |
| Contribute code | [CONTRIBUTING.md](../CONTRIBUTING.md) |
| Review a PR | [Code Review Checklist](CODE_REVIEW_CHECKLIST.md) |
| Understand the architecture | [Architecture Overview](ARCHITECTURE_OVERVIEW.md) |
| Plan implementation | [Roadmap](ROADMAP.md) · [Milestone Specifications](milestones/INDEX.md) · [Implementation Checklist](IMPLEMENTATION_CHECKLIST.md) |
| Understand test strategy | [Testing Strategy](TESTING_STRATEGY.md) |
| Understand platform coverage | [Platform Support](PLATFORM_SUPPORT.md) |

---

## Constitutional Documents

- [CHARTER.md](charter/CHARTER.md) — Foundation charter, core architectural principles, engineering standards
- [GOVERNANCE.md](governance/GOVERNANCE.md) — Working groups, RFC lifecycle, governance model

---

## ADRs — Architectural Decision Records {#adrs}

Each ADR records a binding architectural decision in MADR 3.0.0 format.

### Founding Architecture (ADR 0001–0010)
- [ADR-0001](adr/ADR-0001-generational-slotmap-arena.md) — Generational SlotMap Arena
- [ADR-0002](adr/ADR-0002-push-pull-reactive-signals.md) — Push-Pull Reactive Signal DAG
- [ADR-0003](adr/ADR-0003-two-pass-taffy-layout.md) — Two-Pass Taffy Layout
- [ADR-0004](adr/ADR-0004-vello-compute-rendering.md) — Vello Compute 2D Rendering
- [ADR-0005](adr/ADR-0005-pure-rust-cross-compilation.md) — Pure-Rust Cross-Compilation Mandate
- [ADR-0006](adr/ADR-0006-cosmic-text-accesskit.md) — Cosmic-Text + AccessKit Day-Zero
- [ADR-0007](adr/ADR-0007-multi-window-shared-arena.md) — Multi-Window Shared Arena
- [ADR-0008](adr/ADR-0008-dense-sparse-arena-compaction.md) — Dense-Sparse Arena Compaction
- [ADR-0009](adr/ADR-0009-zero-copy-hardware-media.md) — Zero-Copy Hardware Media Surfaces
- [ADR-0010](adr/ADR-0010-analytical-spring-physics.md) — Analytical Spring Physics

### Comprehensive Coverage (ADR 0011–0032)
- [ADR-0011](adr/ADR-0011-text-input-ime.md) — Text Input and IME Architecture
- [ADR-0012](adr/ADR-0012-headless-ci-testing.md) — Headless CI Testing Infrastructure
- [ADR-0013](adr/ADR-0013-rust-hot-reloading.md) — Rust Hot-Reloading System
- [ADR-0014](adr/ADR-0014-asset-shader-vfs.md) — Asset and Shader VFS
- [ADR-0015](adr/ADR-0015-2d-spatial-focus.md) — 2D Spatial Focus Graph
- [ADR-0016](adr/ADR-0016-mime-aware-clipboard.md) — MIME-Aware Clipboard
- [ADR-0017](adr/ADR-0017-external-internal-dnd.md) — External + Internal DnD
- [ADR-0018](adr/ADR-0018-3rd-party-widget-contract.md) — Third-Party Widget Contract
- [ADR-0019](adr/ADR-0019-software-cpu-fallback.md) — Software CPU Fallback (TinySkia)
- [ADR-0020](adr/ADR-0020-dynamic-design-tokens.md) — Dynamic Design Tokens
- [ADR-0021](adr/ADR-0021-undo-redo-transactions.md) — Undo/Redo Transactions
- [ADR-0022](adr/ADR-0022-in-engine-profiler-tracing.md) — In-Engine Profiler + Tracing
- [ADR-0023](adr/ADR-0023-fluent-localization.md) — Fluent Localization Pipeline
- [ADR-0024](adr/ADR-0024-rfc-governance-engine.md) — RFC Governance Engine
- [ADR-0025](adr/ADR-0025-error-handling-panic-policy.md) — Error Handling + Panic Policy
- [ADR-0026](adr/ADR-0026-versioning-semver-stability.md) — Versioning + SemVer Stability
- [ADR-0027](adr/ADR-0027-platform-support-matrix.md) — Platform Support Matrix
- [ADR-0028](adr/ADR-0028-security-model.md) — Security Model + Supply Chain
- [ADR-0029](adr/ADR-0029-plugin-extension-security.md) — Plugin + Extension Security
- [ADR-0030](adr/ADR-0030-testing-philosophy.md) — Testing Philosophy
- [ADR-0031](adr/ADR-0031-benchmark-baseline-policy.md) — Benchmark Baseline Policy
- [ADR-0032](adr/ADR-0032-documentation-completeness.md) — Documentation Completeness

---

## DDRs — Detailed Design Records {#ddrs}

Each DDR specifies a crate's internal algorithms, data structures, and invariants.

### Founding Specs (DDR 0001–0010)
- [DDR-0001](ddr/DDR-0001-martensite-core-arena-spec.md) — `martensite-core` Arena Spec
- [DDR-0002](ddr/DDR-0002-martensite-reactive-scheduler.md) — `martensite-reactive` Scheduler
- [DDR-0003](ddr/DDR-0003-martensite-wgpu-resilience.md) — `martensite-wgpu` Device Resilience
- [DDR-0004](ddr/DDR-0004-martensite-focus-spatial-beam.md) — `martensite-focus` Spatial Beam
- [DDR-0005](ddr/DDR-0005-martensite-clipboard-dnd.md) — `martensite-clipboard` + DnD
- [DDR-0006](ddr/DDR-0006-martensite-text-ime.md) — `martensite-text` + IME
- [DDR-0007](ddr/DDR-0007-martensite-theme-oklab.md) — `martensite-theme` Oklab
- [DDR-0008](ddr/DDR-0008-martensite-history-lca-tree.md) — `martensite-history` LCA Tree
- [DDR-0009](ddr/DDR-0009-martensite-test-headless-ci.md) — `martensite-test` Headless CI
- [DDR-0010](ddr/DDR-0010-cargo-martensite-hot-reload.md) — `cargo-martensite` Hot-Reload

### Comprehensive Specs (DDR 0011–0024)
- [DDR-0011](ddr/DDR-0011-martensite-window.md) — `martensite-window` Event Loop FSM
- [DDR-0012](ddr/DDR-0012-martensite-layout.md) — `martensite-layout` Taffy Integration
- [DDR-0013](ddr/DDR-0013-martensite-access.md) — `martensite-access` Incremental A11y
- [DDR-0014](ddr/DDR-0014-martensite-render.md) — `martensite-render` PaintList → Vello
- [DDR-0015](ddr/DDR-0015-martensite-assets.md) — `martensite-assets` Dual-Mode VFS
- [DDR-0016](ddr/DDR-0016-martensite-l10n.md) — `martensite-l10n` Fluent Integration
- [DDR-0017](ddr/DDR-0017-martensite-devtools.md) — `martensite-devtools` Tracing + HUD
- [DDR-0018](ddr/DDR-0018-martensite-motion.md) — `martensite-motion` Full Spring Physics
- [DDR-0019](ddr/DDR-0019-martensite-media.md) — `martensite-media` Zero-Copy Surfaces
- [DDR-0020](ddr/DDR-0020-martensite-macros.md) — `martensite-macros` widget! Design
- [DDR-0021](ddr/DDR-0021-arena-concurrent-reads.md) — FrameFence Concurrent Reads
- [DDR-0022](ddr/DDR-0022-resize-layout-quiescence.md) — Resize Layout Quiescence FSM
- [DDR-0023](ddr/DDR-0023-event-routing-hit-testing.md) — Event Routing + Hit-Testing
- [DDR-0024](ddr/DDR-0024-martensite-blessed-tier.md) — `martensite-blessed` Tier Spec

---

## Engineering Specifications

- [specs/CRATE_API_SPECIFICATIONS.md](specs/CRATE_API_SPECIFICATIONS.md) — Full API surface, all 22 crates
- [specs/DEPENDENCY_GRAPH.md](specs/DEPENDENCY_GRAPH.md) — Topological dependency DAG
- [PUBLIC_API_DESIGN.md](PUBLIC_API_DESIGN.md) — End-user facing API contract
- [PLATFORM_SUPPORT.md](PLATFORM_SUPPORT.md) — OS/GPU/A11y/IME support matrix
- [ERROR_HANDLING.md](ERROR_HANDLING.md) — Error types, panic policy, recovery
- [SECURITY.md](SECURITY.md) — Threat model, supply chain, unsafe policy

---

## Process Documents

- [ROADMAP.md](ROADMAP.md) — v0.1.0 → v1.0.0 milestones
- [IMPLEMENTATION_CHECKLIST.md](IMPLEMENTATION_CHECKLIST.md) — Every implementation task
- [TESTING_STRATEGY.md](TESTING_STRATEGY.md) — Test taxonomy, benchmarks, quality gate
- [RELEASE_PROCESS.md](RELEASE_PROCESS.md) — Publication order, SemVer, changelog
- [MIGRATION_GUIDE_0x_to_1x.md](MIGRATION_GUIDE_0x_to_1x.md) — 0.x → 1.0 migration

---

## Contributing Infrastructure

- [ARCHITECTURE_OVERVIEW.md](ARCHITECTURE_OVERVIEW.md) — Frame lifecycle, arena, signals, glossary
- [DOCUMENTATION_STANDARDS.md](DOCUMENTATION_STANDARDS.md) — Rustdoc format, quality guidelines
- [CODE_REVIEW_CHECKLIST.md](CODE_REVIEW_CHECKLIST.md) — PR review gate
- [rfcs/0000-template.md](rfcs/0000-template.md) — RFC submission template
- [../CONTRIBUTING.md](../CONTRIBUTING.md) — Full contributor guide

---

## Architecture Hardening & Verification

Comprehensive architecture review and hardening plan across all subsystem domains:

- [HARDENING_PLAN.md](HARDENING_PLAN.md) — Architecture Review Synthesis & Hardening Plan (Memory, Reactivity, Geometry, Media, and Public API Invariants)

---

## Milestones & Release Engineering

Detailed per-milestone architectural specifications, entry/exit criteria, and verification gates:

- [milestones/INDEX.md](milestones/INDEX.md) — Master Milestone Progression & Dependency Graph
- [milestones/v0.1.0-foundation.md](milestones/v0.1.0-foundation.md) — v0.1.0 Foundation (Core Arena & Reactive DAG)
- [milestones/v0.2.0-render-pipeline.md](milestones/v0.2.0-render-pipeline.md) — v0.2.0 Render Pipeline (WGPU, Vello, TinySkia)
- [milestones/v0.3.0-text-layout.md](milestones/v0.3.0-text-layout.md) — v0.3.0 Text & Layout (Cosmic-Text, Taffy Two-Pass)
- [milestones/v0.4.0-accessibility-focus.md](milestones/v0.4.0-accessibility-focus.md) — v0.4.0 Accessibility & Focus (AccessKit, 2D Spatial Focus)
- [milestones/v0.5.0-input-platform.md](milestones/v0.5.0-input-platform.md) — v0.5.0 Input & Platform (Clipboard, DnD, IME)
- [milestones/v0.6.0-motion-theme.md](milestones/v0.6.0-motion-theme.md) — v0.6.0 Motion & Theme (Spring Physics, Oklab)
- [milestones/v0.7.0-subsystems.md](milestones/v0.7.0-subsystems.md) — v0.7.0 Advanced Subsystems (Undo/Redo LCA, VFS, Fluent)
- [milestones/v0.8.0-media-hdr.md](milestones/v0.8.0-media-hdr.md) — v0.8.0 Media & Advanced GPU (NV12/P010 Zero-Copy, Display EOTF)
- [milestones/v0.9.0-developer-experience.md](milestones/v0.9.0-developer-experience.md) — v0.9.0 Developer Experience (Tracy, Hot-Reload, Headless CI)
- [milestones/v0.10.0-plugins-ecosystem.md](milestones/v0.10.0-plugins-ecosystem.md) — v0.10.0 Hardening, Plugins & Ecosystem (Wasmtime, Blessed Widgets)
- [milestones/v1.0.0-production-release.md](milestones/v1.0.0-production-release.md) — v1.0.0 Production Stability (API Freeze, docs.rs, Crates.io)

---

## Architectural Decisions Status

All 5 core architectural open questions have been evaluated and codified:
- **OQ-1**: Wayland `Immediate` mode allowed via explicit opt-in (`App::build().present_mode(PresentMode::Immediate)`).
- **OQ-2**: Software CPU fallback (`tiny-skia`) requires explicit opt-in (`App::build().allow_software_fallback(true)`).
- **OQ-3**: NV12 (8-bit SDR) and P010 (10-bit HDR) both Tier-1 at v1.0.
- **OQ-4**: Wasmtime plugin sandbox shipped at v1.0 (`crates/martensite-plugin`).
- **OQ-5**: Core maintainer authority pre-1.0; Foundation transfer (Rust / Linux Foundation) post-v1.0.


