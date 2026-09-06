# Adversarial Audit Report: API Ergonomics & Ten Golden Laws (Red Team 05)

**Target:** Martensite v1.0.0 Public API, Constitutional Documents, Wasmtime Plugin ABI
**Auditor:** Swarm Member 5 (The API Ergonomics & Ten Golden Laws Inquisitor)
**Date:** September 6, 2026

## 1. The Borrow Checker Trap in Context (`cx`) & Event Handlers

### The Flaw
The public API specifies `Signal<T>` as a lightweight 64-bit copyable handle. However, when defining event handlers using the modifier API, we encounter severe ergonomic friction because of the interaction between `Copy` closures and capturing state.

```rust
let my_signal = cx.signal(0);
let my_other_state = cx.signal("hello");

button("Click Me")
    .on_click(move |cx| {
        // We move the copyable signals into this closure.
        my_signal.set(my_signal.get() + 1);
    })
    .on_hover(move |cx, hovered| {
        // ERROR: If my_signal or my_other_state were moved into the first closure,
        // we might run into capture issues if we aren't careful about how Rust closures 
        // capture `Copy` vs `Clone` types when mixed with non-copy environments.
    });
```
Moreover, `on_click` provides `&mut EventContext`. But what if a developer wants to spawn an async task on click? The `Context` API provides `cx.spawn(...)`, but `EventContext` does not document a `spawn` method in the specification! If they try to capture `&mut Context` from the outer scope, the borrow checker will vehemently reject it, forcing developers to use `Rc<RefCell<Context>>`—a direct violation of Law IV (The Single-Tree Generational Arena Law).

### Remediation
1. Ensure `EventContext` and `LayoutContext` can escalate to or deref into a standard `Context` capable of `.spawn()` and `.signal()`.
2. Introduce a `Clone!` or `capture!` macro standard (like `glib::clone!`) to prevent the developer from battling the borrow checker when capturing dozens of signals across multiple event handler closures.

---

## 2. Hidden Heap Allocations & The "Static View" Trap in Widget Builders

### The Flaw (Critical Architectural Contradiction)
Law V (Invariant 5.1) states: *"Component view declarations execute exactly once during initialization to forge the node hierarchy. Component functions must never re-run top-to-bottom on state changes."*

Yet, the `WidgetExt` trait is defined as taking raw primitive values:
```rust
fn padding(self, points: f32) -> Self;
fn background(self, color: impl Into<Color>) -> Self;
fn text(content: impl Into<String>) -> TextWidget;
```
If the view only executes *exactly once*, how does the UI update?
If a developer writes:
```rust
let padding_sig = cx.signal(10.0);
column().padding(padding_sig.get()) // reads 10.0 ONCE
```
When `padding_sig` changes, the padding will **never update** because the builder function never re-runs, and `.padding()` consumed the primitive `f32` by value, not the signal! 

Furthermore, `WidgetExt` methods consume `self` and return `Self`. This means `TextWidget::padding` must return `TextWidget`. This implies `TextWidget` is a monolithic "fat struct" containing fields for padding, margin, borders, tooltips, etc. This contradicts the 64-byte `HotNode` requirement and requires boxing or heap allocations, violating Law II.

### Remediation
1. **Reactive Modifiers:** Modifier methods must accept `IntoValue<T>`, an enum that can be either a static value or a `Signal<T>` / `Memo<T>`. 
```rust
pub enum Value<T> { Static(T), Reactive(Signal<T>), Computed(Memo<T>) }
pub trait IntoValue<T> { ... }

fn padding(self, points: impl IntoValue<f32>) -> Self;
```
2. **Type Erasure via Node IDs:** `WidgetExt` should not return `Self` as a fat struct. It should return a lightweight wrapper or operate on a `WidgetId` builder wrapper. Alternatively, modifiers must instantly append to the `WidgetArena` and return a chained `ArenaHandle` to prevent boxing huge modifier chains on the heap.

---

## 3. Wasmtime Plugin Overhead & The 60Hz/120Hz Budget

### The Flaw
ADR-0029 dictates that third-party plugins will run in a Wasmtime sandbox and submit output via a `PaintList` substream. For a synthesizer displaying 1,000 points at 60Hz:
- If the plugin makes 1,000 host function calls via Wasmtime trampolines per frame (`fill_rect` x 1000), at ~50ns per call, that is 50μs.
- At 120Hz, a frame is 8.33ms. 50μs is negligible (0.6% of the frame). 
- **However**, if those calls serialize data across the wasm boundary via shared linear memory, the overhead of memory boundary checks, string serialization (for colors, text, etc.), and host-side deserialization for 1,000 objects will spike to 1-2ms, consuming up to 25% of the frame budget.

### Remediation
The ABI specified in ADR-0029 (`plugin_event`, `plugin_build`) is insufficient for real-time 120Hz rendering. 
1. **Shared Memory Ring Buffer:** The ABI must expose a `SharedArrayBuffer` for geometry data. The plugin writes raw vertices (e.g., `[x, y, r, g, b, a]`) directly into a memory-mapped slice that the host GPU backend (`martensite-wgpu`) reads directly, completely bypassing Wasmtime trampoline host calls per-primitive.

---

## 4. Constitutional & Architectural Inconsistencies

### 1. Inconsistent "Zero C/C++" Law (Mandate I vs Phase 4)
The CHARTER states: *"zero external C/C++ compilers, zero CMake build scripts...".*
Yet, `martensite-text` relies on `fontdb` and HarfBuzz logic (`rustybuzz`). While `rustybuzz` is pure Rust, `martensite-window` relies on `winit`, which relies on `objc2` and potentially Cocoa C-bindings. If building for Linux Wayland/X11, `winit` relies on `x11-dl` or `wayland-client`, which historically require `libx11` headers or `pkg-config` unless completely statically rewritten. 

### 2. Multi-Window Limitations vs Charter Claims
SPEC-0001-API notes under `martensite` (Phase 1): *"Limitations v0.x: Single window only; multi-window support delayed to v1.0."*
Yet `wg-platform` in GOVERNANCE is mandated to handle *"Winit multi-window management"*. This is a minor misalignment but creates confusion regarding the v1.0.0 target scope versus current limitations.

### 3. Idle 0.00% Resource Conflict
The Event-Sleep Law (Law III) states CPU/GPU must be at 0.00% when idle. 
ADR-0029 allows Wasmtime plugins with no constraints on background polling within the wasm module. If a plugin continuously spins or polls within its sandbox, it will violate the host's 0.00% CPU mandate.
*Remediation:* The Wasmtime engine must strictly pause execution or block the wasm instance unless explicitly awoken by a host-routed event or registered timeout via the host capability interface.
