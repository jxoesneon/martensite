# Martensite Architecture Charter

**Document Identifier:** RFC-0000-CHARTER  
**Status:** Approved Architecture Charter  
**Licensing:** MIT License OR Apache License 2.0  

---

## 1. Project Purpose and Scope

### 1.1 Background & Goals
Building native desktop user interfaces in Rust involves distinct architectural tradeoffs. Existing solutions explore several design paradigms:

1. **Immediate-Mode Toolkits**: Re-evaluate and re-render interface trees every frame, which provides rapid prototyping and straightforward state management, but can increase CPU and GPU consumption during idle states and complicates native accessibility synchronization.
2. **Message-Passing Elm Architectures**: Structure state updates via centralized message types and state trees, which ensures functional purity, but can lead to large message enums and per-frame virtual tree allocations in complex layouts.
3. **Embedded Web Runtimes**: Embed browser engines or webviews, which enables rapid cross-platform UI authoring using web technologies, but incurs higher memory overhead and asynchronous IPC serialization boundaries.
4. **Custom DSL Frameworks**: Rely on external domain-specific languages and specialized compilation passes outside the standard Cargo and Rust toolchains.

**Martensite** is engineered to provide a direct systems approach: a pure-Rust, retained-mode framework combining direct GPU rasterization via WGPU compute shaders, a fine-grained push-pull reactive state model, and an arena-based widget hierarchy.

### 1.2 Target Applications
Martensite is designed for performance-conscious desktop applications: workstation software, digital audio and media tools, computer-aided design (CAD/CAM), financial consoles, developer utilities, and general desktop applications across Windows, macOS, Linux, and embedded environments.

---

## 2. Core Requirements

Every subsystem and crate within the Martensite workspace adheres to four foundational operational requirements:

### Requirement I: Pure-Rust Supply Chain
The dependency graph of Martensite should compile cleanly across supported targets using standard `rustc` and `cargo` without requiring external C/C++ build chains or native platform library dependencies (`libfontconfig`, `libX11`, `glib`, `openssl`). Cross-compilation to Windows, macOS, Linux, or WebAssembly should function using standard Rust target tooling.

### Requirement II: Ergonomic Declarative API
The framework aims to provide a clean declarative API in idiomatic Rust while avoiding virtual DOM diffing overhead, hook dependency arrays, or stale closure traps. Interface declaration is verified at compile time by the Rust type system.

### Requirement III: Event-Driven Idle Sleep
When an application's visual state is static and no input events arrive, the event loop yields to operating system wait primitives (`ControlFlow::Wait`), minimizing CPU and GPU consumption while waiting for input.

### Requirement IV: Bounded Memory & Zero Runtime GC
Martensite operates without runtime garbage collection or virtual machines. Per-frame updates and rendering passes execute without dynamic heap allocations. Memory is organized into contiguous, cache-coherent generational arenas.

---

## 3. Core Architectural Principles

These ten principles guide architectural decisions, specifications, code reviews, and RFC evaluations across the project:

### Principle 1: Direct GPU Rendering
Interface elements are rendered directly through hardware compute pipelines (`Vello` / `wgpu`) into the native swapchain rather than wrapping host platform widgets. This ensures consistent layout calculation, visual styling, animations, and typography across supported platforms.

### Principle 2: Bounded Memory & Zero Runtime GC
The core engine does not rely on a background tracing garbage collector or runtime interpreter. The active frame loop (measuring, laying out, and generating draw streams for existing widgets) executes without per-frame dynamic heap allocations. Resident memory (RSS) is kept bounded, and unreferenced pages are returned to the operating system during idle periods.

### Principle 3: Event-Driven Sleep
Continuous polling loops are avoided. The event loop sleeps via native OS wait calls (`epoll`, `kqueue`, `GetMessageW`). Animations are driven by analytical physical solvers that settle cleanly and deregister once motion falls below perceptibility thresholds.

### Principle 4: Generational Arena Hierarchy
Widgets are stored in a contiguous generational slotmap. Relationships are expressed through lightweight 64-bit integer handles (`WidgetId`). Cyclic reference graphs and interior mutability wrappers (`Rc<RefCell<T>>`) are avoided in the public widget hierarchy. Deleting or reparenting a node invalidates its generational slot in $O(1)$ time, allowing stale handles to evaluate safely to `None`.

### Principle 5: Fine-Grained Reactive Signals
Component initialization functions execute once to construct the widget hierarchy in the arena. State changes are managed through fine-grained push-pull signals (`Signal<T>`). When a signal changes, it traverses a dependency DAG, flagging dirty bits on affected leaf nodes without virtual DOM diffing.

### Principle 6: Pure-Rust Implementation
All layout logic, components, modifiers, and state bindings are written in standard Rust syntax compatible with stable `rustc` and `rust-analyzer`. External template languages or custom compiler preprocessors are not required.

### Principle 7: Two-Pass Layout Geometry
Layout evaluation operates in two distinct phases:
1. **Pass 1 (Intrinsic Measurement)**: Nodes calculate minimum and maximum content bounds bottom-up without placement side effects.
2. **Pass 2 (Constraint Placement)**: Taffy resolves CSS Flexbox, Grid, and Block rules top-down, producing final coordinates.

Coordinates are calculated and verified within the frame before draw commands reach the GPU encoder.

### Principle 8: Integrated Accessibility
Accessibility is integrated into the core widget model. Interactive widgets implement accessibility methods, synchronizing state, focus, and bounds with native accessibility systems (Windows UI Automation, macOS NSAccessibility, Linux AT-SPI2) via `accesskit::TreeUpdate`.

### Principle 9: Unicode & Multilingual Typography
All text is processed through `cosmic-text`, utilizing pure-Rust `rustybuzz` for OpenType table evaluation and `unicode-bidi` for bidirectional text mixing (Unicode UAX #9). Fallback fonts are resolved via system font discovery (`fontdb`) to support international scripts and color glyphs.

### Principle 10: Permissive Dual-Licensing
The framework and its standard tooling are dual-licensed under the MIT License and the Apache License (Version 2.0).

---

## 4. Engineering & Quality Standards

### 4.1 Functional Design & Ergonomics
* **Visual Clarity**: Default styles focus on high-contrast, functional design suitable for technical and workstation tools: balanced neutral palettes, crisp boundaries, and clean geometric alignment.
* **Ergonomics & Contrast**: Text tokens adhere to WCAG contrast standards. Motion transitions use damped harmonic curves to provide natural responsiveness without excessive ornamentation.

### 4.2 Verification & Soundness
* **Test-Backed Engineering**: Features must include automated test coverage verifying correct behavior.
* **Operational Public APIs**: Public API surfaces must not expose placeholder stubs (`todo!()` or `unimplemented!()`) as functional capabilities.
* **Technical Documentation**: Documentation must be precise, concise, and factual. Documentation should include clear descriptions of performance characteristics, platform support, and known constraints.

### 4.3 Reproducible Benchmarks
Core crates maintain Criterion benchmark suites tracking:
1. Signal propagation latency across deep dependency DAGs.
2. Layout constraint solving times on multi-node grids.
3. Frame encoding latency across cold and warm render caches.
4. Startup time and resident memory footprint (RSS).

Benchmark results reported in documentation must cite hardware specifications, operating system versions, and git commit hashes.

### 4.4 Memory Safety Policies
* Public-facing crates (`martensite`, `martensite-core`, `martensite-reactive`, `martensite-layout`) enforce `#![forbid(unsafe_code)]`.
* In low-level graphics subsystems (`martensite-wgpu`) where hardware memory mapping is necessary, each `unsafe` block must be accompanied by an explicit `// SAFETY:` comment documenting prerequisites and invariants.

---

## 5. Licensing Guarantees

To support integration across commercial software, open-source projects, and research institutions:

1. **Irrevocable Dual License**: The codebase is licensed under the Apache License, Version 2.0 ([LICENSE-APACHE](http://www.apache.org/licenses/LICENSE-2.0)) and the MIT License ([LICENSE-MIT](http://opensource.org/licenses/MIT)).
2. **Distribution & Linking**: Downstream projects may statically link, dynamically link, modify, embed, or distribute software built upon Martensite without proprietary fees.
3. **Patent Grant**: The Apache 2.0 license provides an explicit patent grant from contributors to users of the framework.
