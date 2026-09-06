# [ADR-0016] MIME-Aware Clipboard Integration

* **Status:** Accepted
* **Date:** 2026-09-06
* **Deciders:** Martensite Architecture Working Group
* **Technical Domain:** `mime-aware-clipboard`

## Context and Problem Statement

For a retained-mode, GPU-accelerated GUI framework targeting v1.0.0, handling mime-aware-clipboard is a critical requirement. We must establish a robust, performant, and safe architecture.

## Decision Drivers

* **Performance & Safety**: Must align with Rust's strict safety guarantees without sacrificing performance.
* **Platform Independence**: Must work consistently across target platforms.
* **Developer Experience**: Must provide a clear and ergonomic API for framework users.

## Considered Options

* **Option 1**: Legacy OS-dependent monolithic approaches.
* **Option 2**: Incomplete pure-Rust abstractions.
* **Option 3**: **Use a multi-format clipboard API (arboard/smithay-clipboard) handling text, images, and custom MIME types**.

## Decision Outcome

Chosen option: **Option 3**. Use a multi-format clipboard API (arboard/smithay-clipboard) handling text, images, and custom MIME types perfectly provides reliable performance guarantees, providing explicit, measurable, and high-performance guarantees.

### Positive Consequences

* Ensures predictable and highly optimized execution.
* Integrates cleanly with the existing reactive signal graph and arena architecture.
* Adheres to strict strict memory safety and zero undefined behavior policies.

### Negative Consequences

* Initial implementation complexity is high.
* Requires meticulous cross-platform abstraction layers.
