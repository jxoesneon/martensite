# THE GOVERNANCE CHARTER OF MARTENSITE

**Document Identifier:** RFC-0000-GOVERNANCE  
**Initial Ratification:** September 2026  
**Status:** Invariant Operating Constitution  
**Authority:** The Sovereign Architect & The Working Groups

---

## 1. Organizational Model

The governance of Martensite is structured to prevent both autocratic single-maintainer burnout and bureaucratic, design-by-committee stagnation. Martensite adopts a **Federated Working Group Model** steered by the Sovereign Architect.

```
┌────────────────────────────────────────────────────────────────────────┐
│                      MARTENSITE STEERING TOPOLOGY                      │
├────────────────────────────────────────────────────────────────────────┤
│                                                                        │
│                 SOVEREIGN ARCHITECT (Master / Lead)                    │
│     * Final veto on core architecture, licensing, and Golden Laws      │
│                                    │                                   │
│            ┌───────────────────────┴───────────────────────┐            │
│            ▼                                               ▼            │
│  [ WORKING GROUPS ]                               [ RFC ASSEMBLY ]      │
│  Domain Sovereigns                                Public Consensus Gate │
│  • Core & Layout WG                               • Community Reviews   │
│  • Graphics & Compute WG                          • Final Comment Period│
│  • Platform & Shell WG                            • Tracking Issues     │
│  • Text & Accessibility WG                                              │
│  • Tooling & Ecosystem WG                                               │
│                                                                        │
└────────────────────────────────────────────────────────────────────────┘
```

### 1.1 The Sovereign Architect (Master)
* **Role**: The project founder and executive custodian of the Martensite vision.
* **Responsibilities**:
  - Enforces absolute adherence to **The Ten Golden Laws of Martensite** (`CHARTER.md`).
  - Holds final determination on breaking architectural forks, project licensing, and foundational trait revisions.
  - Appoints and confirms Working Group leads.

### 1.2 The Five Federated Working Groups (WGs)
Operational authority is decentralized into five autonomous Working Groups. Each Working Group possesses technical sovereignty within its domain:

1. **Core & Layout Working Group (`wg-core`)**:
   - *Domain*: `martensite-core`, `martensite-reactive`, `martensite-layout`, `martensite-arena`, `martensite-history`.
   - *Mandate*: Generational SlotMap integrity, push-pull signal DAG performance, Taffy two-pass layout integration, memory compaction, and undo/redo transactional journals.
2. **Graphics & Compute Working Group (`wg-graphics`)**:
   - *Domain*: `martensite-wgpu`, `martensite-render`, `martensite-media`, `martensite-theme`.
   - *Mandate*: Vello compute shader integration, WGPU swapchain management, single-frame GPU loss self-healing, pure-Rust SIMD software rasterizer fallback (`tiny-skia`), zero-copy media handles, and Oklab shader uniforms.
3. **Platform & Shell Working Group (`wg-platform`)**:
   - *Domain*: `martensite-window`, `martensite-clipboard`, `martensite-dnd`, `martensite-focus`.
   - *Mandate*: Winit multi-window management, per-monitor fractional DPI scaling, multi-MIME delayed clipboard rendering, native cross-OS drag-and-drop, and 2D spatial focus engines.
4. **Text & Accessibility Working Group (`wg-a11y`)**:
   - *Domain*: `martensite-text`, `martensite-access`, `martensite-l10n`.
   - *Mandate*: `cosmic-text` HarfBuzz multi-script shaping, sub-pixel IME candidate window positioning, native AccessKit screen-reader synchronization, and Mozilla Fluent grammatical localization.
5. **Tooling & Ecosystem Working Group (`wg-tooling`)**:
   - *Domain*: `martensite-devtools`, `martensite-macros`, `martensite-assets`, `cargo-martensite`, and `martensite-blessed`.
   - *Mandate*: Sub-second hot-reloading toolchains, Tracy/Chrome profiling spans, in-app F12 developer HUDs, `naga` AOT shader compilers, documentation, and Crater-CI ecosystem testing.

---

## 2. The Formal Martensite RFC Process

Major changes to Martensite cannot be merged via unilateral pull requests. Any change affecting public APIs, crate architectures, or execution semantics must pass through the **Martensite Request for Comments (RFC) Process**.

```
┌────────────────────────────────────────────────────────────────────────┐
│                        THE 6-PHASE RFC LIFECYCLE                       │
├────────────────────────────────────────────────────────────────────────┤
│                                                                        │
│   [Phase 1: Pre-RFC Exploration] ──► Idea pitch & feasibility draft    │
│                                           │                            │
│   [Phase 2: Formal RFC Submission] ─► PR to `martensite-rfcs`          │
│                                           │                            │
│   [Phase 3: Working Group Review] ──► Technical critique & iteration   │
│                                           │                            │
│   [Phase 4: Final Comment Period] ──► 10-Day public community clock    │
│                                           │                            │
│   [Phase 5: Unstable Incubator] ────► Merged under feature flag        │
│                                           │                            │
│   [Phase 6: Formal Stabilization] ──► Promoted to Tier 1 Core API      │
│                                           │                            │
└────────────────────────────────────────────────────────────────────────┘
```

### 2.1 When an RFC is Mandatory
An RFC is strictly required for:
* Any modification to public traits (`Widget`, `Signal`, `RenderBackend`, `ChangeOp`).
* Adding, altering, or deprecating standard widget primitives.
* Changing the memory layout or indexing algorithm of the Generational SlotMap.
* Altering reactive scheduling order or signal propagation rules.
* Introducing any new workspace sub-crate.
* Modifying the Minimum Supported Rust Version (MSRV) or licensing.

### 2.2 The Six Phases of an RFC
* **Phase 1 (Pre-RFC Exploration)**: The author shares a lightweight design pitch in Discussions or the community chat to gather preliminary feedback, establish problem severity, and identify existing prior art.
* **Phase 2 (Formal RFC Submission)**: The author submits a pull request to `martensite-rfcs/text/0000-my-feature.md` using the standardized RFC template.
* **Phase 3 (Working Group Review)**: The designated Working Group reviews the RFC. Team members raise technical critiques regarding memory allocation, cache locality, API ergonomics, and alignment with the Ten Golden Laws. The author iterates on the proposal.
* **Phase 4 (Final Comment Period - FCP)**: Once consensus is reached, the Working Group lead declares a **10-day Final Comment Period (FCP)** with a disposition to **Merge**, **Close**, or **Postpone**. The FCP is announced publicly. Any community member may raise a substantive technical objection. If no blocking flaws are identified within 10 days, the RFC is formally approved.
* **Phase 5 (Unstable Incubator)**: The approved RFC is implemented and merged into the main codebase, gated strictly behind the `#[cfg(martensite_unstable)]` compilation flag and an opt-in Cargo feature.
* **Phase 6 (Formal Stabilization)**: After real-world battle-testing and automated Criterion benchmark verification, the Working Group files a Stabilization Report, graduating the capability into the Tier 1 Core Stable API.

---

## 3. Stability Tiers & The Semantic Versioning Contract

To ensure commercial enterprises and studio engineering teams can build mission-critical products on Martensite without fear of breaking changes, the framework establishes three strict stability tiers.

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

### 3.1 Tier 1: Core Stable API (The SemVer 1.0 Invariant)
* **The Zero-Breakage Guarantee**: Once `martensite v1.0.0` is published, **zero breaking changes are permitted within the 1.x release series**. Code that compiles against `1.0.0` is guaranteed to compile and run with identical semantics on `1.99.0`.
* **Automated CI Gate (`cargo-semver-checks`)**:
  - Every single pull request in the repository automatically executes `cargo semver-checks` against the latest published crates.io release.
  - If a pull request accidentally changes a trait signature, removes a method, narrows visibility, or alters generic trait bounds on a stable API, **the CI pipeline terminates with an immediate fatal error**.

### 3.2 Tier 2: The Experimental Incubator (`martensite_unstable`)
* **Rapid Innovation Without Instability**: Experimental features (such as bleeding-edge GPU compute passes, novel docking algorithms, or specialized media decoders) live in the main tree but are isolated behind an explicit compiler flag:
  ```rust
  #[cfg(martensite_unstable)]
  pub mod experimental_viewport;
  ```
* **Explicit Opt-In**: To consume unstable APIs, developers must explicitly pass:
  ```bash
  RUSTFLAGS="--cfg martensite_unstable" cargo build
  ```
* Unstable APIs may be adjusted or iterated across minor version releases based on developer feedback.

### 3.3 Tier 3: Deprecation Ledger & The 6-Month Grace Window
When an existing API is superseded by a superior abstraction, it enters a structured three-stage deprecation lifecycle:
1. **Soft Deprecation**: The item is marked with `#[deprecated(since = "1.X.0", note = "...")]`. Compiler warnings provide explicit, copy-paste migration steps and, where applicable, machine-executable `rustfix` patches.
2. **The 6-Month Grace Window**: Deprecated APIs remain fully operational, tested, and actively maintained for **a minimum of two minor releases or six calendar months** (whichever duration is longer). Under no circumstances are deprecated APIs broken or removed during this window.
3. **Major Boundary Removal**: Deprecated APIs are expunged **only upon major version boundaries (`2.0.0`)**, accompanied by an automated CLI migration tool (`cargo martensite-migrate`).

---

## 4. The `martensite-blessed` Ecosystem & Automated Crater-CI

A GUI engine cannot exist in isolation; it thrives through its ecosystem of community libraries: charting suites, audio faders, CAD viewports, code editors, markdown renderers, and theme packs.

To prevent the ecosystem decay that plagued predecessors—where core library updates continually broke community packages—Martensite introduces **The Blessed Tier and Crater-CI**.

### 4.1 The `martensite-blessed` Curated Tier
`martensite-blessed` is an officially audited catalog of high-prestige community crates that adhere to the highest engineering standards:
* **Quality Criteria for Blessed Status**:
  1. Complete adherence to the Ten Golden Laws (0% idle CPU, zero unmanaged memory leaks).
  2. `#![forbid(unsafe_code)]` or 100% verified, auditable `// SAFETY:` invariants.
  3. Minimum 80% automated unit and visual regression test coverage.
  4. Dual-licensing under MIT and Apache 2.0.
  5. Active maintenance and participation in Working Group testing.

### 4.2 Automated Crater-CI Ecosystem Matrix
* **The Continuous Shield**: Every pull request targeting `martensite` core and every nightly build triggers a specialized GitHub Actions matrix that clones, builds, and runs the complete test suites of **all crates in the `martensite-blessed` catalog**.
* **The Non-Breaking Ecosystem Mandate**: If a proposed change in core passes internal tests but causes compilation errors or test failures in any `martensite-blessed` package, **the pull request is blocked from merging**. The core team must resolve the regression or coordinate with the ecosystem maintainer prior to integration.

---

## 5. Minimum Supported Rust Version (MSRV) Policy

To provide absolute predictability for enterprise production deployments, industrial toolchains, and distribution package maintainers:

1. **Rolling Stable-3 Guarantee**: Martensite guarantees support for the **current stable Rust compiler release and the preceding three stable releases** (a rolling $\approx 4.5$-month baseline window).
2. **Explicit Bump Protocol**: Bumping the MSRV requires:
   - A formal proposal and approval by the Core & Layout Working Group.
   - Demonstration of substantial compiler capabilities, safety improvements, or language features required by the engine.
   - An explicit notice published in the release notes of at least one minor version preceding the bump.
---

## 6. Stewardship & Post-v1.0 Foundation Succession

To guarantee long-term institutional stability, mitigate single-point-of-failure risks (Bus Factor = 1), and provide sovereign assurances for enterprise adoption:

### 6.1 Pre-v1.0 Stewardship
During the active bootstrapping and stabilization phases leading up to `v1.0.0`, executive authority remains vested in the Sovereign Architect. This structure preserves conceptual integrity, maintains development velocity, and prevents dilution of the Ten Golden Laws during core engine synthesis.

### 6.2 Post-v1.0 Foundation Transition
Upon the successful stabilization and publication of `martensite v1.0.0`:
1. **Foundation Stewardship**: Project stewardship, trademarks, and domain assets (`martensite.dev`) will be transferred to a recognized, vendor-neutral open-source foundation (such as the **Rust Foundation** or **Linux Foundation**).
2. **Multi-Steward Governance**: Governance will formally transition to a multi-steward council composed of elected Working Group representatives, core contributors, and institutional stakeholders, mirroring the governance topologies of foundational Rust infrastructure (e.g., Cargo, rustfmt, Clippy).
3. **Constitution Immutability**: The core tenets of `CHARTER.md`—including permanent dual-licensing (MIT/Apache 2.0), 100% Pure-Rust homogeneity, and Pixel Sovereignty—are codified as irrevocable constitutional invariants.

### 6.3 Inactivity & Emergency Contingency
In the pre-1.0 era, should the Sovereign Architect experience prolonged, unannounced inactivity exceeding **90 consecutive calendar days** without appointing a designated delegate:
* The five Federated Working Group leads shall automatically convene as an **Emergency Governance Council**.
* By a supermajority vote ($\ge 4$ of 5 leads), the Council is empowered to appoint an interim Sovereign Architect or initiate immediate foundation transfer to ensure project survival and continuity.

---

*This Governance Charter stands as the operational law of Martensite, ensuring an enduring, stable, and sovereign engineering platform.*

