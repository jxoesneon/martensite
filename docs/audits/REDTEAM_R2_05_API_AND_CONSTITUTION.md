# Red Team Attack Report 05: API & Constitutional Sabotage (Round 2)

**Target:** Martensite v1.0.0 Public API, Constitutional Documents, Wasmtime Plugin ABI
**Author:** Red Team Saboteur 5
**Date:** 2026-09-06

This document completely dismantles the defenses established in the Blue Team's Phase 1 report. The proposed "zero-cost" architectural solutions exhibit catastrophic flaws ranging from exponential compiler bloat to host OS panics and untracked zombie tasks.

## 1. `IntoValue<T>` Binary Bloat: The Monomorphization Explosion

**The Defense:** Using `impl IntoValue<T>` on every modifier in a builder-pattern `WidgetExt` trait returning `Self`.
**The Flaw:** By taking `impl IntoValue<T>` and returning `Self` on a generic trait implemented for *every* widget type, you have created a combinatoric monomorphization bomb. 

Every modifier used triggers the compiler to generate a unique function for:
`Widget Type` × `Modifier` × `PropValue Variant (Static or Dynamic)`.
If a developer chains `.padding(10.0).background(Color::RED)` on a `Button`, and later `.padding(my_sig).background(Color::BLUE)` on a `Stack`, the compiler generates entirely separate binary code for `Button::padding<f32>`, `Button::padding<Signal<f32>>`, `Stack::padding<f32>`, etc. In a complex application with 50 widgets and 30 modifiers, this forces the instantiation of thousands of redundant modifier functions, destroying compile times and causing massive binary bloat.

**Rigorous Remediation:**
Modifiers should not be monomorphized per widget type or per value type. 
1. Use concrete `PropValue<T>` in signatures, forcing conversion at the call site: `fn padding(self, points: PropValue<f32>) -> Self`. (Developers can use `.into()` at the call site).
2. Better yet, since the UI is backed by a Generational Arena, modifiers should operate on a type-erased handle (e.g., `WidgetId`) rather than returning a strongly-typed `Self` builder that requires infinite monomorphization.

---

## 2. Wasmtime Shared-Memory Host Security: Undefined Behavior & GPU Exploitation

**The Defense:** Direct-streaming a lock-free shared memory ring buffer from a Wasm plugin to the GPU pipeline to achieve <0.1ms latency, bypassing Wasmtime trampolines entirely.
**The Flaw:** By blindly streaming a "raw byte slice" of `PaintCmd` packets from untrusted Wasm memory to the host/GPU, you open the host to immediate Undefined Behavior and GPU crashes.

1. **Invalid Discriminants:** `PaintCmd` is a Rust enum. If a compromised Wasm plugin writes an invalid discriminant (e.g., a byte value not matching `FillRect` or `DrawPath`), and the host reads this memory as a `&[PaintCmd]`, it is **instant Undefined Behavior** in Rust.
2. **Buffer Overruns via `points_offset`:** If `DrawPath { points_offset, points_count, ... }` points outside the Wasm linear memory, and the host GPU pipeline blindly executes this command, the GPU will perform an out-of-bounds memory access, likely crashing the graphics driver or the compositor.
3. **NaN Poisoning:** Untrusted floats (NaNs or infinities) streamed directly into compute shaders can trigger infinite loops or undefined rendering states in the GPU pipeline.

**Rigorous Remediation:**
You cannot stream untrusted memory directly to execution. 
1. The packet structure must be strictly plain-old-data (POD) via crates like `bytemuck` to prevent enum discriminant UB. 
2. The host *must* perform a linear validation pass over the commands before dispatching to the GPU. Ensure `points_offset + points_count` is clamped strictly within the allowed Wasm memory bounds. SIMD can be used to validate bounds and floats across 1,000 packets in single-digit microseconds, easily preserving the <0.1ms budget.

---

## 3. EventContext Async Cancellation: The Zombie Task Leak

**The Defense:** Spawning background tasks in localized closures via `cx_event.spawn(async move { ... })` with `'static` futures, bypassing the borrow checker.
**The Flaw:** The spawned tasks are completely detached from the lifecycle of the widget that spawned them. 

**The Counterexample:**
```rust
button("Fetch")
    .on_click(move |cx| {
        cx.spawn(async move {
            let data = network_fetch().await; // Takes 5 seconds
            my_signal.set(data);
        });
    })
```
If the user clicks "Fetch", and the button is immediately unmounted from the tree (e.g., by navigating to a different view), the future continues executing in the background. 
1. **Resource Leak:** The network fetch and task consume CPU and memory long after the view is dead.
2. **Dangling Handle Panic:** When the task completes, it calls `my_signal.set(data)`. Since `my_signal` is a 64-bit generational index, its backing slot in the arena has been dropped. If `set` unwraps the arena lookup, the application will panic and crash. If it ignores the missing slot, the task was purely a zombie resource leak.

**Rigorous Remediation:**
`EventContext::spawn` must intrinsically bind the spawned task to the widget's lifetime.
The `spawn` method should tie the future to the executing `WidgetId`. The runtime executor must automatically cancel (drop) all pending futures associated with a `WidgetId` when that node is removed from the Generational Arena. This prevents both resource leaks and dangling signal panics.
