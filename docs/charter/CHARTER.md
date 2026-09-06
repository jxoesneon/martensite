# THE CHARTER OF MARTENSITE

**Document Identifier:** RFC-0000-CHARTER  
**Initial Ratification:** September 2026  
**Status:** Invariant Core Constitution  
**Licensing:** MIT License OR Apache License 2.0 (Permanent Sovereignty Guarantee)

---

## 1. Project Genesis & Purpose

### 1.1 The Ten-Year Void
Since the stabilization of Rust 1.0 in 2015, the language has established categorical supremacy in networking, systems infrastructure, distributed databases, cryptography, operating systems, and command-line interfaces. Despite this profound industrial triumph, graphical client software development within the Cargo ecosystem has remained crippled by a ten-year deadlock.

Developers seeking to author modern client applications in Rust have been forced into an intolerable dilemma:
1. **The Immediate-Mode Trap (`egui`)**: Trading battery life, international typography, and assistive accessibility for procedural development speed, resulting in continuous 60Hz/120Hz GPU redraw loops and 1-frame layout jitter.
2. **The Elm Architecture Trap (`iced`)**: Forcing a functional, message-passing web architecture onto an ownership-based systems language, resulting in catastrophic message enum explosion, encapsulation collapse, and per-frame virtual tree allocations.
3. **The Foreign Language & Commercial Trap (`slint`)**: Forcing developers out of idiomatic Rust into a proprietary domain-specific language (`.slint`) governed by a restrictive dual-license commercial model.
4. **The Webview Wrapper Trap (`tauri` / `dioxus`)**: Abandoning systems programming entirely to embed Chromium or host WebKit runtimes, incurring multi-process memory bloat (100MB+ RAM), asynchronous JSON IPC serialization bottlenecks, and the complete loss of host GPU compute pipelines.

### 1.2 The Genesis of Martensite
**Martensite** is founded to close this void permanently. Named after the hardest, most resilient crystalline microstructure of hardened steel—formed by an instantaneous, diffusionless shear transformation—Martensite is engineered from first principles as an unyielding, high-performance, pure-Rust native GUI engine.

Martensite is not an experimental toy, a temporary wrapper, or an academic prototype. It is an industrial-grade systems platform designed to power mission-critical software: digital audio workstations (DAWs), computer-aided design (CAD/CAM), financial trading consoles, creative image/video suites, developer tools, and consumer desktop applications across Windows, macOS, Linux, and embedded hardware.

---

## 2. Core Mandates

Every subsystem, crate, and algorithm inside the Martensite workspace is bound to four non-negotiable operational mandates:

### Mandate I: 100% Pure-Rust Systems Purity (The Zero C/C++ Law)
The entire dependency graph of Martensite must compile to any supported target using vanilla `rustc` and `cargo` with zero external C/C++ compilers, zero CMake build scripts, zero system shared library dependencies (`libfontconfig`, `libX11`, `glib`, `openssl`), and zero native toolchain friction. Cross-compilation from any host to Windows, macOS, Linux, or WebAssembly must succeed out of the box with zero foreign sysroots.

### Mandate II: Ergonomics Matching or Exceeding Modern Declarative Standards
Developers must never be penalized for choosing a systems language. The developer experience in Martensite must match the declarative elegance of SwiftUI and Jetpack Compose while completely eliminating the Virtual DOM diffing tax, hook dependency array traps, and stale closure bugs. UI declaration is 100% pure, type-safe, compile-time verified Rust.

### Mandate III: Absolute 0.00% Idle Resource Consumption (Event-Driven Sleep)
A user interface that consumes CPU or GPU cycles while sitting static on screen is broken by definition. When no user input arrives, no background tasks dispatch updates, and no physical animations are active, Martensite's event loop drops into deep kernel wait states (`ControlFlow::Wait`). CPU usage must measure strictly **0.00%**, and GPU usage must measure strictly **0.00%**.

### Mandate IV: Complete Eradication of Garbage Collection & Heap Thrashing (Zero-GC)
Martensite operates with zero runtime garbage collectors, zero virtual machines, and zero allocations inside the hot per-frame interaction and rendering loop. Memory is allocated into contiguous, cache-coherent generational arenas. An entire production workstation interface must idle comfortably under **20 MB of resident RAM (RSS)**.

---

## 3. The Ten Golden Laws of Martensite

These ten invariant laws govern all architectural decisions, code reviews, pull requests, and RFC deliberations across the Martensite project. They are immutable and inviolable.

### LAW I: THE PIXEL SOVEREIGNTY LAW
> *Martensite shall render every visual pixel directly through its own hardware compute pipelines. It shall never wrap native OS host widgets.*

* **Invariant 1.1**: The framework relies on hardware-accelerated 2D compute shader pipelines (`Vello` / `wgpu`) rendering directly to native swapchains.
* **Invariant 1.2**: Wrapping OEM platform widgets (`UIButton`, `android.widget.Button`, `HWND`) is strictly forbidden. Pixel sovereignty guarantees that layout math, visual styling, animations, and interaction mechanics behave with 100% bit-exact parity across Windows, macOS, Linux, and embedded displays.

### LAW II: THE ZERO-GC & BOUNDED MEMORY LAW
> *No garbage collector, no virtual machine, and zero dynamic heap allocations inside active frame execution.*

* **Invariant 2.1**: The core engine must never require a background tracing garbage collector or runtime interpreter.
* **Invariant 2.2**: The hot frame loop (measuring, laying out, and generating draw streams for existing widgets) must execute with zero heap allocations (`Vec::push`, `Box::new`, or string allocations).
* **Invariant 2.3**: Application resident memory (RSS) must remain bounded. Burst capacities must compact and purge unreferenced virtual memory pages back to the operating system kernel during idle quiescent periods.

### LAW III: THE EVENT-SLEEP LAW
> *When state is static and no inputs arrive, CPU and GPU utilization must be absolute 0.00%.*

* **Invariant 3.1**: Continuous 60Hz/120Hz polling loops are strictly prohibited. The event loop must sleep via native OS kernel wait calls (`epoll`, `kqueue`, `GetMessageW`).
* **Invariant 3.2**: Animations must be driven by analytical physics solvers that automatically quench and deregister the instant motion settles below imperceptible thresholds, dropping the engine back into deep sleep.

### LAW IV: THE SINGLE-TREE GENERATIONAL ARENA LAW
> *All widgets reside within a flat Generational SlotMap. References are 64-bit copyable handles. Cyclic pointer graphs and `Rc<RefCell<T>>` are banished.*

* **Invariant 4.1**: Widgets are allocated once into a contiguous generational arena. Parent-child and sibling relationships are represented exclusively via lightweight, copyable, 64-bit integer handles (`WidgetId { slot_idx: u32, generation: u32 }`).
* **Invariant 4.2**: Unchecked raw pointers, bidirectional reference trees, and interior mutability wrappers (`Rc<RefCell<T>>`) are prohibited across the public widget hierarchy.
* **Invariant 4.3**: Deleting or reparenting a node invalidates its generational slot in $O(1)$ time; stale handles evaluate safely to `None` without memory corruption, use-after-free, or panics.

### LAW V: THE ZERO-VDOM SIGNAL LAW
> *State mutations shall never diff a Virtual DOM. State shifts update dirty bitsets on exact leaf nodes in $O(1)$ time.*

* **Invariant 5.1**: Component view declarations execute **exactly once** during initialization to forge the node hierarchy within the arena. Component functions must never re-run top-to-bottom on state changes.
* **Invariant 5.2**: State is expressed as fine-grained, push-pull signals (`Signal<T>`). When a signal mutates, it traverses a direct dependency DAG, flagging only the specific leaf nodes observing that signal in a dirty bitset.
* **Invariant 5.3**: Reconciliation diffing trees ($O(N)$ tree-walking) and hook dependency arrays (`useEffect([deps])`) are strictly banished.

### LAW VI: THE PURE-RUST HOMOGENEITY LAW
> *No foreign DSLs, no XML schemas, no MOC preprocessors. UI is 100% compile-time verified, idiomatic Rust.*

* **Invariant 6.1**: Every layout, component, modifier, and state binding must be written in standard, idiomatic Rust syntax recognized by vanilla `rustc` and `rust-analyzer`.
* **Invariant 6.2**: Bespoke template languages (`.slint`, `.qml`, `.xaml`) and custom AST-rewriting preprocessors (`moc`, custom compiler plugins) are forbidden.
* **Invariant 6.3**: All code must compile cleanly on the stable Rust channel within the established MSRV window.

### LAW VII: THE TWO-PASS GEOMETRY LAW
> *Layout measurement is strictly decoupled from layout placement. Single-frame layout popping and oscillation are mathematically forbidden.*

* **Invariant 7.1**: Layout calculation must operate in two distinct, non-destructive phases:
  - **Pass 1 (Intrinsic Measurement)**: Nodes calculate minimum and maximum content bounds bottom-up without placement side effects.
  - **Pass 2 (Constraint Placement)**: Taffy resolves W3C Flexbox, CSS Grid, and Block rules top-down, computing final coordinates.
* **Invariant 7.2**: Layout coordinates must be mathematically finalized and verified in the exact same frame before any draw instructions reach the GPU command encoder, eradicating 1-frame position lag.

### LAW VIII: THE ACCESSIBILITY-FIRST LAW
> *Every interactive primitive must emit native, real-time AccessKit semantic updates from day zero.*

* **Invariant 8.1**: Accessibility is not an opt-in plugin or secondary consideration. Every standard interactive widget must implement the `accessibility` lifecycle method.
* **Invariant 8.2**: State changes, focus shifts, and layout bounds must synchronize incrementally with native OS accessibility daemons (Windows UI Automation, macOS NSAccessibility, Linux AT-SPI2 over pure-Rust `zbus`) via `accesskit::TreeUpdate`.

### LAW IX: THE WORLD TYPOGRAPHY LAW
> *All text rendering must support complex multi-script shaping, bidirectional layout, and font fallbacks without tofu glyphs.*

* **Invariant 9.1**: Primitive, ASCII-only font rasterizers are prohibited. All text passes through `cosmic-text`, utilizing pure-Rust `rustybuzz` (HarfBuzz) for OpenType table evaluation and `unicode-bidi` for Unicode Annex #9 bidirectional mixing.
* **Invariant 9.2**: Missing glyphs must automatically resolve through an indexed system fallback chain (`fontdb`), correctly rendering CJK ideographs, Arabic cursive ligatures, Indic conjuncts, and color emojis (`COLRv1`).

### LAW X: THE PERMISSIVE FREEDOM LAW
> *Permanent dual-licensing under MIT and Apache 2.0. Commercial freedom is absolute and perpetual.*

* **Invariant 10.1**: The core Martensite engine, its official crates, and its build tools shall be dual-licensed under the **MIT License** and the **Apache License (Version 2.0)** in perpetuity.
* **Invariant 10.2**: Copyleft licensing (GPL, AGPL, LGPL), commercial seat taxes, revenue-share royalties, or proprietary dual-license traps are permanently prohibited from entering core repositories.

---

## 4. Anti-Slop Code & Design Doctrine

The modern software landscape is inundated with low-effort, unvetted AI-generated code, fragile webview wrappers, and superficial marketing hype. Martensite repudiates this culture entirely through its **Anti-Slop Doctrine**.

### 4.1 Aesthetic Brutalism & Material Truth
* **Rejection of Glassmorphism**: Blurry frosted backdrops, low-contrast pastel cards, and translucent eye-candy that degrade accessibility and consume GPU compute needlessly are rejected as the default aesthetic.
* **Structural Brutalism**: Martensite champions high-contrast, tactile, industrial visual design inspired by physical workstations: deep carbon and graphite palettes, sharp 1-pixel subpixel-snapped boundaries, crisp geometric bevels, and microsecond visual responsiveness.
* **Legibility & Human Ergonomics**: All text tokens adhere to strict WCAG AAA contrast ratios. Animation curves are strictly physical damped harmonic oscillators; superficial bouncing and decorative easing are discarded.

### 4.2 Zero Hallucinated Features & The Iron Law of Verification
* **Evidence-First Engineering**: No feature shall be declared complete, merged, or announced without verifiable, executable code and automated test coverage.
* **No Stubbed APIs**: Public API surfaces must not contain `todo!()`, `unimplemented!()`, or placeholder mock structs masquerading as functional capabilities.
* **Documentation Brutalism**: Documentation must state technical realities with objective precision. Hyperbolic fluff (*"revolutionary, cutting-edge, blazingly fast magic"*) is banned. Documentation must prominently feature an **Explicit Limitations & Trade-offs Section** detailing unsupported platforms, known performance boundaries, and active constraints.

### 4.3 Verifiable Benchmark Standards
* **Mandatory Criterion Suites**: Every core crate must include reproducible `benches/` tracking:
  1. Signal propagation latency across 10,000-node DAGs.
  2. Taffy constraint-solving time on 5,000-node dynamic grids.
  3. Frame encoding latency in Vello across cold and warm caches.
  4. Cold startup time and resident memory footprint (RSS).
* **Hardware Accountability**: Benchmark results committed to documentation must cite exact hardware specifications, CPU microarchitectures, operating system kernels, and commit hashes. Synthetic claims without reproducible scripts are rejected.

### 4.4 Uncompromising Soundness
* **Public Facade Soundness**: All user-facing crates (`martensite`, `martensite-core`, `martensite-reactive`, `martensite-layout`) enforce `#![forbid(unsafe_code)]`.
* **Hardware HAL Invariants**: In low-level graphics subsystems (`martensite-wgpu`) where raw hardware memory aliasing is required, every single `unsafe` block must be preceded by an explicit, auditable `// SAFETY:` block documenting the exact hardware invariants and memory safety preconditions.

---

## 5. Commercial Sovereignty Guarantee

Martensite is built to serve as permanent, foundational software infrastructure. To ensure commercial enterprises, startups, independent developers, and public research institutions can adopt Martensite with absolute confidence:

1. **Irrevocable Dual License**: The codebase is, and will forever remain, licensed under the Apache License, Version 2.0 ([LICENSE-APACHE](http://www.apache.org/licenses/LICENSE-2.0)) and the MIT License ([LICENSE-MIT](http://opensource.org/licenses/MIT)).
2. **Freedom of Modification & Distribution**: Any entity may statically link, dynamically link, modify, embed, redistribute, or build proprietary, closed-source commercial software upon Martensite with zero licensing fees, zero reporting requirements, and zero threat of license revocation.
3. **Patent Grant**: The Apache 2.0 license provides an explicit, permanent grant of patent rights from all contributors to users of the framework.

*Martensite is forged to endure.*
