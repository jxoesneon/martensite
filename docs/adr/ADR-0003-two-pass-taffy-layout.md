# [ADR-0003] Decoupled Two-Pass Taffy Layout Integration

* **Status:** Accepted
* **Date:** 2026-09-06
* **Deciders:** Master (Sovereign Architect), Ciel (Layout & Systems Guild)
* **Technical Domain:** `martensite-layout`, `martensite-core`

## Context and Problem Statement

Desktop user interfaces must dynamically adapt to varying window dimensions, localized string lengths, fractional display scaling, and dynamic content resizing.
* **The Single-Pass Immediate-Mode Failure (`egui`)**: Immediate-mode UI engines calculate layout during the same procedural pass that handles events and renders draw calls. This results in **one-frame layout lag** where dynamic content (e.g., expanding panels, auto-sizing windows) jitters visually for one frame before settling.
* **The Web Layout Engine Bloat (Blink/WebKit)**: Web browsers implement massive, monolithic layout engines with complex cascading specificity wars and non-deterministic reflow performance.

We must establish a deterministic, high-performance layout architecture that provides full support for industry-standard W3C Flexbox and CSS Grid layouts while guaranteeing zero visual lag.

## Decision Drivers

* **Zero Single-Frame Layout Popping**: Mathematical guarantee that measurement and placement are fully finalized within the current frame before rendering.
* **W3C Standards Compliance**: Native support for Flexbox (CSS Flexible Box Module Level 1) and CSS Grid layout algorithms.
* **Zero C/C++ FFI Dependencies**: The layout solver must be 100% pure Rust.

## Considered Options

* **Option 1**: Immediate-mode single-pass layout calculation.
* **Option 2**: Custom procedural layout trait engine.
* **Option 3**: **Decoupled Two-Pass Layout Bridge via Pure-Rust `Taffy`**.

## Decision Outcome

Chosen option: **Option 3**, integrating the pure-Rust `Taffy` crate directly over Martensite's generational arena.

### Positive Consequences

* **Two-Pass Decoupling**:
  - **Pass 1 (Intrinsic Measure)**: Bottom-up evaluation of text bounds, image aspect ratios, and custom widget sizes (`MeasureFunc`).
  - **Pass 2 (Constraint Placement)**: Top-down resolution of flexbox and grid coordinates, placing elements into final pixel bounds.
* **Bit-Exact Standard Behavior**: Developers utilize familiar Flexbox and CSS Grid mental models without web engine baggage.
* **Elimination of Visual Popping**: Layout coordinates are guaranteed final before any GPU drawing commands are recorded.
