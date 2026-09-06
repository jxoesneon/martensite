# Martensite Governance Charter

**Document Identifier:** RFC-0000-GOVERNANCE  
**Status:** Approved Governance Charter  
**Authority:** The Lead Architect & The Working Groups  

---

## 1. Organizational Model

The governance of Martensite is structured to balance focused architectural direction with distributed, domain-specific ownership. Martensite adopts a **Federated Working Group Model** coordinated by the Lead Architect.

```
┌────────────────────────────────────────────────────────────────────────┐
│                      MARTENSITE STEERING TOPOLOGY                      │
├────────────────────────────────────────────────────────────────────────┤
│                                                                        │
│                       LEAD ARCHITECT / MAINTAINERS                     │
│     * Core architecture guidance, licensing, and design principles     │
│                                    │                                   │
│            ┌───────────────────────┴───────────────────────┐            │
│            ▼                                               ▼            │
│  [ WORKING GROUPS ]                               [ RFC PROCESS ]       │
│  Domain Leads                                     Public Review Gate    │
│  • Core & Layout WG                               • Community Reviews   │
│  • Graphics & Compute WG                          • Final Comment Period│
│  • Platform & Shell WG                            • Tracking Issues     │
│  • Text & Accessibility WG                                              │
│  • Tooling & Ecosystem WG                                               │
│                                                                        │
└────────────────────────────────────────────────────────────────────────┘
```

### 1.1 The Lead Architect
* **Role**: Custodian of the Martensite technical roadmap and architectural coherence.
* **Responsibilities**:
  - Ensures alignment with the **Core Architectural Principles** (`CHARTER.md`).
  - Guides determinations on foundational trait designs, public API changes, and project licensing.
  - Confirms Working Group leads and facilitates cross-group coordination.

### 1.2 The Five Federated Working Groups (WGs)
Operational responsibility is organized into five domain-focused Working Groups:

1. **Core & Layout Working Group (`wg-core`)**:
   - *Domain*: `martensite-core`, `martensite-reactive`, `martensite-layout`, `martensite-arena`, `martensite-history`.
   - *Mandate*: Generational slotmap integrity, push-pull signal DAG performance, Taffy two-pass layout integration, memory compaction, and undo/redo transactional journals.
2. **Graphics & Compute Working Group (`wg-graphics`)**:
   - *Domain*: `martensite-wgpu`, `martensite-render`, `martensite-media`, `martensite-theme`.
   - *Mandate*: Vello compute shader integration, WGPU swapchain management, device loss recovery, SIMD software rasterizer fallback (`tiny-skia`), zero-copy media handles, and color pipeline uniforms.
3. **Platform & Shell Working Group (`wg-platform`)**:
   - *Domain*: `martensite-window`, `martensite-clipboard`, `martensite-dnd`, `martensite-focus`.
   - *Mandate*: Winit multi-window management, per-monitor DPI scaling, multi-format clipboard integration, platform drag-and-drop, and 2D spatial focus engines.
4. **Text & Accessibility Working Group (`wg-a11y`)**:
   - *Domain*: `martensite-text`, `martensite-access`, `martensite-l10n`.
   - *Mandate*: `cosmic-text` HarfBuzz multi-script shaping, IME candidate window positioning, native AccessKit screen-reader synchronization, and Fluent localization.
5. **Tooling & Ecosystem Working Group (`wg-tooling`)**:
   - *Domain*: `martensite-devtools`, `martensite-macros`, `martensite-assets`, `cargo-martensite`, and `martensite-blessed`.
   - *Mandate*: Hot-reloading toolchains, tracing integrations, developer diagnostic overlays, `naga` shader compilation, documentation, and ecosystem CI testing.

---

## 2. The Formal Martensite RFC Process

Major changes to Martensite follow the **Martensite Request for Comments (RFC) Process** to ensure thorough review of public APIs, crate architectures, and runtime semantics.

```
┌────────────────────────────────────────────────────────────────────────┐
│                        THE 6-PHASE RFC LIFECYCLE                       │
├────────────────────────────────────────────────────────────────────────┤
│                                                                        │
│   [Phase 1: Pre-RFC Exploration] ──► Discussion & initial draft        │
│                                           │                            │
│   [Phase 2: Formal RFC Submission] ─► PR to `martensite-rfcs`          │
│                                           │                            │
│   [Phase 3: Working Group Review] ──► Technical feedback & iteration   │
│                                           │                            │
│   [Phase 4: Final Comment Period] ──► 10-Day public review window      │
│                                           │                            │
│   [Phase 5: Unstable Incubator] ────► Merged under feature flag        │
│                                           │                            │
│   [Phase 6: Formal Stabilization] ──► Promoted to Core Stable API      │
│                                           │                            │
└────────────────────────────────────────────────────────────────────────┘
```

### 2.1 When an RFC is Mandatory
An RFC is required for:
* Modifications to public traits (`Widget`, `Signal`, `RenderBackend`, `ChangeOp`).
* Adding, altering, or deprecating standard widget primitives.
* Changing the memory layout or indexing algorithm of the generational slotmap.
* Altering reactive scheduling order or signal propagation rules.
* Introducing a new workspace sub-crate.
* Modifying the Minimum Supported Rust Version (MSRV) or licensing.

### 2.2 The Six Phases of an RFC
* **Phase 1 (Pre-RFC Exploration)**: The author shares a lightweight design pitch in Discussions to gather preliminary feedback and explore prior art.
* **Phase 2 (Formal RFC Submission)**: The author submits a pull request to `martensite-rfcs/text/0000-my-feature.md` using the standardized RFC template.
* **Phase 3 (Working Group Review)**: The designated Working Group reviews the RFC, evaluating performance, memory characteristics, API ergonomics, and alignment with core principles.
* **Phase 4 (Final Comment Period - FCP)**: Once consensus is reached, the Working Group lead announces a **10-day Final Comment Period (FCP)** with a disposition to **Merge**, **Close**, or **Postpone**. If no blocking issues emerge, the RFC is formally approved.
* **Phase 5 (Unstable Incubator)**: The feature is implemented and merged into the main codebase behind the `#[cfg(martensite_unstable)]` flag or an opt-in Cargo feature.
* **Phase 6: (Formal Stabilization)**: Following testing and benchmark verification, the Working Group files a Stabilization Report, promoting the feature to the Core Stable API.

---

## 3. Stability Tiers & The Semantic Versioning Contract

To provide reliability for production deployments, Martensite establishes three stability tiers:

```
┌────────────────────────────────────────────────────────────────────────┐
│                        MARTENSITE STABILITY TIERS                      │
├─────────────────┬──────────────────────────────────┬───────────────────┤
│ Stability Tier  │ Consumer Configuration           │ Breaking Policy   │
├─────────────────┼──────────────────────────────────┼───────────────────┤
│ Tier 1: Core    │ Standard `Cargo.toml` dependency │ SemVer 1.0 (Zero) │
│ Tier 2: Unstable│ `RUSTFLAGS="--cfg martensite_.."`│ May alter in minor│
│ Tier 3: Deprec. │ Emits `#[deprecated]` compiler w.│ Min 2 minor vers. │
└─────────────────┴──────────────────────────────────┴───────────────────┘
```

### 3.1 Tier 1: Core Stable API (SemVer 1.0 Guarantee)
* **API Compatibility**: Following the release of `v1.0.0`, breaking changes are not permitted within the 1.x release series. Code compiling against `1.0.0` will compile against subsequent 1.x releases.
* **Automated CI Gate (`cargo-semver-checks`)**:
  - Pull requests execute `cargo semver-checks` against the latest published release.
  - Unintended breaking changes to public trait signatures or visibility will fail CI checks.

### 3.2 Tier 2: The Experimental Incubator (`martensite_unstable`)
* **Iterative Features**: Experimental capabilities live in the main tree behind a compilation flag:
  ```rust
  #[cfg(martensite_unstable)]
  pub mod experimental_viewport;
  ```
* **Explicit Opt-In**:
  ```bash
  RUSTFLAGS="--cfg martensite_unstable" cargo build
  ```
* Unstable APIs may be adjusted across minor releases based on developer feedback.

### 3.3 Tier 3: Deprecation Policy
When an API is superseded, it follows a structured deprecation process:
1. **Soft Deprecation**: The item is marked with `#[deprecated(since = "1.X.0", note = "...")]` with guidance on alternative APIs.
2. **Grace Window**: Deprecated APIs remain operational and tested for **a minimum of two minor releases or six calendar months** (whichever is longer).
3. **Major Boundary Removal**: Deprecated APIs are removed only upon major version boundaries (`2.0.0`), accompanied by migration documentation.

---

## 4. The Curated Ecosystem Tier & Crater-CI

To encourage a healthy ecosystem of community extensions (charting, audio controls, editors, and widgets), Martensite maintains the `martensite-blessed` catalog.

### 4.1 Curated Ecosystem Tier
`martensite-blessed` is a curated list of high-quality community crates:
* **Criteria**:
  1. Alignment with Core Architectural Principles (efficient idle resource usage, bounded memory).
  2. `#![forbid(unsafe_code)]` or documented `// SAFETY:` invariants.
  3. Automated unit and visual test coverage.
  4. Permissive dual-licensing (MIT/Apache 2.0).
  5. Active maintenance.

### 4.2 Automated Ecosystem CI
* Pull requests to core crates run CI builds and test suites across crates in the `martensite-blessed` catalog.
* If a proposed core change causes compilation or test failures in a blessed package, maintainers coordinate to resolve the regression before merging.

---

## 5. Minimum Supported Rust Version (MSRV) Policy

To provide predictability for production deployments:

1. **Rolling Stable-3 Baseline**: Martensite supports the **current stable Rust compiler release and the preceding three stable releases** (approximately a 4.5-month baseline window).
2. **Bump Protocol**: Increasing the MSRV requires:
   - Approval by the Core & Layout Working Group.
   - Demonstration of substantial compiler capabilities, safety improvements, or language features required by the engine.
   - Advance notice in release notes prior to the bump.

---

## 6. Stewardship & Foundation Transition

To ensure long-term stability and continuity:

### 6.1 Pre-v1.0 Stewardship
During the development and stabilization phases leading to `v1.0.0`, technical direction is coordinated by the Lead Architect and Working Group leads to maintain architectural coherence and development velocity.

### 6.2 Post-v1.0 Foundation Transition
Upon publication of `martensite v1.0.0`:
1. **Foundation Stewardship**: Project stewardship, trademarks, and domain assets may be transferred to a neutral open-source foundation (such as the **Rust Foundation** or **Linux Foundation**).
2. **Multi-Steward Governance**: Governance will formally transition to a multi-steward council composed of elected Working Group representatives and core contributors, consistent with standard open-source governance models.
3. **Core Tenets**: Permissive dual-licensing (MIT/Apache 2.0) and core architecture principles remain foundational commitments.

### 6.3 Continuity Contingency
Should the Lead Architect experience extended inactivity exceeding **90 consecutive calendar days** without appointing a delegate:
* The Working Group leads convene to appoint an interim lead or initiate foundation transition to ensure project continuity.

---

*This Governance Charter defines the operational structure of Martensite, ensuring an enduring and transparent engineering process.*

