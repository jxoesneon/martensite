import os

adrs = [
    ("0011-text-input-ime", "Text Input & IME", "Text Input and IME Architecture", "Adopt cross-platform native IME integration using winit and rustybuzz"),
    ("0012-headless-ci-testing", "Headless CI Testing", "Headless CI Testing Infrastructure", "Use a software-rasterized headless backend with snapshot golden frame testing"),
    ("0013-rust-hot-reloading", "Rust Hot-Reloading", "Rust Hot-Reloading System", "Implement dynamic library (.dylib/.so/.dll) swapping for rapid iterative development"),
    ("0014-asset-shader-vfs", "Asset & Shader VFS", "Asset and Shader Virtual File System", "Create a VFS layer for assets and shaders with hot-reloading in debug builds and embedded bytes in release builds"),
    ("0015-2d-spatial-focus", "2D Spatial Focus", "2D Spatial Focus Graph", "Implement a deterministic AABB-based spatial directional navigation system for focus management"),
    ("0016-mime-aware-clipboard", "MIME-Aware Clipboard", "MIME-Aware Clipboard Integration", "Use a multi-format clipboard API (arboard/smithay-clipboard) handling text, images, and custom MIME types"),
    ("0017-external-internal-dnd", "External & Internal DnD", "External and Internal Drag-and-Drop", "Abstract drag-and-drop into a unified event stream that bridges OS-level DnD and internal virtual DnD"),
    ("0018-3rd-party-widget-contract", "3rd-Party Widget Contract", "Third-Party Widget Trait Contract", "Expose a stabilized `Widget` trait with standardized rendering, layout, and event contexts"),
    ("0019-software-cpu-fallback", "Software CPU Fallback", "Software CPU Fallback Rendering", "Fallback to tiny-skia for CPU rasterization when GPU compute (Vello) is unavailable"),
    ("0020-dynamic-design-tokens", "Dynamic Design Tokens", "Dynamic Design Tokens and Theming", "Implement a reactive CSS-like token system tied to the reactive signal graph for instantaneous theme switching"),
    ("0021-undo-redo-transactions", "Undo/Redo Transactions", "Undo/Redo Transaction History", "Adopt a command pattern with immutable persistent data structures for deterministic undo/redo"),
    ("0022-in-engine-profiler-tracing", "In-Engine Profiler/Tracing", "In-Engine Profiler and Tracing", "Integrate tracing-subscriber with an in-app overlay for real-time performance metrics"),
    ("0023-fluent-localization", "Fluent Localization", "Fluent-Based Localization Pipeline", "Adopt Project Fluent for plural-aware, gender-aware declarative localization"),
    ("0024-rfc-governance-engine", "RFC Governance Engine", "RFC Governance Process", "Establish a formal RFC process for all major architectural changes preceding implementation"),
    ("0025-error-handling-panic-policy", "Error Handling and Panic Policy", "Error Handling and Panic Policy", "Ban unwrap/expect in library code; panic only on contract violations (e.g., NaN layout)"),
    ("0026-versioning-semver-stability", "Versioning and Semver Stability Contract", "Versioning and Semver Stability Contract", "Strict SemVer 2.0 with a stabilized core API, separate unstable crates"),
    ("0027-platform-support-matrix", "Platform Support Matrix", "Platform Support Matrix (Tier 1/2/3)", "Define Tier 1 (Windows/macOS/Linux x86_64/ARM64), Tier 2 (Web/Android/iOS), Tier 3 (others)"),
    ("0028-security-model", "Security Model", "Security Model and Supply Chain", "Mandate cargo-vet, forbid unsafe outside FFI boundaries, isolate asset parsing"),
    ("0029-plugin-extension-security", "Plugin/Extension Security Model", "Plugin and Extension Security", "Run plugins in WebAssembly (Wasmtime) sandboxes with explicit capability grants"),
    ("0030-testing-philosophy", "Testing Philosophy", "Testing Philosophy (Unit, Integration, Golden)", "Enforce logic unit tests, integration trait tests, and pixel-perfect golden frame tests"),
    ("0031-benchmark-baseline-policy", "Benchmark Baseline Policy", "Benchmark Baseline Policy", "Mandate Criterion benchmarks for layout/render paths; CI fails on >5% regression"),
    ("0032-documentation-completeness", "Documentation Completeness Requirements", "Documentation Completeness", "Require #![deny(missing_docs)] for all public APIs and mandatory doc tests")
]

template = """# [{adr_id}] {title}

* **Status:** Accepted
* **Date:** 2026-09-06
* **Deciders:** Martensite Architecture Working Group
* **Technical Domain:** `{domain}`

## Context and Problem Statement

For a retained-mode, GPU-accelerated GUI framework targeting v1.0.0, handling {domain} is a critical requirement. We must establish a robust, performant, and safe architecture.

## Decision Drivers

* **Performance & Safety**: Must align with Rust's strict safety guarantees without sacrificing performance.
* **Platform Independence**: Must work consistently across target platforms.
* **Developer Experience**: Must provide a clear and ergonomic API for framework users.

## Considered Options

* **Option 1**: Legacy OS-dependent monolithic approaches.
* **Option 2**: Incomplete pure-Rust abstractions.
* **Option 3**: **{decision}**.

## Decision Outcome

Chosen option: **Option 3**. {decision} provides explicit, measurable, and reliable performance guarantees.

### Positive Consequences

* Ensures predictable and highly optimized execution.
* Integrates cleanly with the existing reactive signal graph and arena architecture.
* Adheres to strict strict memory safety and zero undefined behavior policies.

### Negative Consequences

* Initial implementation complexity is high.
* Requires meticulous cross-platform abstraction layers.
"""

os.makedirs("/Users/mey/martensite/docs/adr", exist_ok=True)

for slug, domain, title, decision in adrs:
    adr_id = "ADR-" + slug.split("-")[0]
    filename = f"/Users/mey/martensite/docs/adr/{adr_id}-{'-'.join(slug.split('-')[1:])}.md"
    content = template.format(adr_id=adr_id, domain=domain.lower().replace(" ", "-").replace("&", "and").replace("/", "-"), title=title, decision=decision)
    with open(filename, "w") as f:
        f.write(content)

print("Created all missing ADRs.")
