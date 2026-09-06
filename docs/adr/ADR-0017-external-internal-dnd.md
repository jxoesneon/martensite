# [ADR-0017] External and Internal Drag-and-Drop

* **Status:** Accepted
* **Date:** 2026-09-06
* **Deciders:** Master (Sovereign Architect), Ciel (Systems Architecture Guild)
* **Technical Domain:** `external-and-internal-dnd`

## Context and Problem Statement

For a sovereign, retained-mode, GPU-accelerated GUI engine targeting v1.0.0, handling external-and-internal-dnd is a critical requirement. We must establish a robust, performant, and safe architecture.

## Decision Drivers

* **Performance & Safety**: Must align with Rust's strict safety guarantees without sacrificing performance.
* **Platform Independence**: Must work consistently across target platforms.
* **Developer Experience**: Must provide a clear and ergonomic API for framework users.

## Considered Options

* **Option 1**: Legacy OS-dependent monolithic approaches.
* **Option 2**: Incomplete pure-Rust abstractions.
* **Option 3**: **Abstract drag-and-drop into a unified event stream that bridges OS-level DnD and internal virtual DnD**.

## Decision Outcome

Chosen option: **Option 3**. Abstract drag-and-drop into a unified event stream that bridges OS-level DnD and internal virtual DnD perfectly aligns with the Anti-Slop Doctrine, providing explicit, measurable, and high-performance guarantees.

### Positive Consequences

* Ensures predictable and highly optimized execution.
* Integrates cleanly with the existing reactive signal graph and arena architecture.
* Adheres to strict strict memory safety and zero undefined behavior policies.

### Negative Consequences

* Initial implementation complexity is high.
* Requires meticulous cross-platform abstraction layers.
