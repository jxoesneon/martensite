# Martensite Architecture Overview

Welcome to the core engine of Martensite. This document is the definitive guide to how Martensite converts operating system events into pixels on the screen, maintaining zero idle CPU utilization when quiescent and deterministic memory management without garbage collection.

## 1. The Frame Lifecycle: Event to Pixel

Martensite operates on a reactive, event-driven architecture. Unlike immediate-mode GUIs (`egui`) or polling engines, Martensite sleeps in the OS kernel until a stimulus arrives. When an event fires, a strict sequential pipeline executes:

1. **OS Event Ingress (`martensite-window`)**: `winit` intercepts a hardware interrupt (e.g., mouse click, key press, window resize).
2. **Signal Mutation (`martensite-reactive`)**: The event handler executes, mutating one or more `Signal<T>` values.
3. **Dirty Propagation (The DAG)**: Mutated signals immediately notify their dependent observers. Specific UI nodes in the generational arena are flagged as `DIRTY_LAYOUT`, `DIRTY_PAINT`, or `DIRTY_A11Y`.
4. **Pass 1: Layout Measurement (`martensite-layout`)**: The engine traverses only the `DIRTY_LAYOUT` nodes bottom-up, computing intrinsic minimum and maximum sizes.
5. **Pass 2: Layout Placement (`Taffy`)**: The Taffy engine runs top-down, solving Flexbox/Grid constraints and assigning final floating-point $X/Y$ coordinates to the nodes.
6. **Accessibility Sync (`martensite-access`)**: `DIRTY_A11Y` nodes push their new spatial bounds and state semantics into the `accesskit::TreeUpdate` queue, synchronizing with the OS screen reader daemon.
7. **Display List Generation**: The engine walks the `DIRTY_PAINT` nodes, generating a localized stream of 2D vector drawing commands (rectangles, text glyphs, paths).
8. **Compute Shader Execution (`martensite-wgpu` / `Vello`)**: The vector commands are uploaded to the GPU. `Vello`'s compute shaders evaluate the paths and rasterize the geometry into a high-performance tile buffer.
9. **Swapchain Presentation**: The final texture is blitted to the OS window swapchain, appearing on the physical monitor.
10. **Kernel Sleep**: The engine issues a kernel wait command (`epoll`/`GetMessageW`) and sleeps until the next interrupt.

## 2. The Generational Arena

Martensite banishes `Rc<RefCell<T>>`, memory leaks, and Virtual DOM diffing by storing the entire UI widget tree in a flat, contiguous array called a Generational Arena (`SlotMap`).

```text
┌─────────────────────────────────────────────────────────────┐
│                      THE SLOTMAP ARENA                      │
├──────┬──────┬──────┬──────┬──────┬──────┬──────┬────────────┤
│ IDX  │  0   │  1   │  2   │  3   │  4   │  5   │ ...        │
├──────┼──────┼──────┼──────┼──────┼──────┼──────┼────────────┤
│ GEN  │  1   │  1   │  2   │  1   │  3   │  1   │ ...        │
├──────┼──────┼──────┼──────┼──────┼──────┼──────┼────────────┤
│ DATA │ Node │ Node │ Node │ Free │ Node │ Free │ ...        │
│      │(Root)│(Btn) │(Text)│      │(Icon)│      │            │
└──────┴──────┴──────┴──────┴──────┴──────┴──────┴────────────┘
```

* **Allocation**: When a widget is created, it takes a slot in the array. The `WidgetId` is a 64-bit integer containing the `index` and the current `generation`.
* **Deallocation**: When a widget is destroyed, its slot is marked `Free` and its `generation` counter increments.
* **Safety**: If an old `WidgetId` (e.g., `Index: 2, Gen: 1`) tries to access the slot after it was reused (now at `Gen: 2`), the arena returns `None`. This guarantees no Use-After-Free or memory corruption without requiring garbage collection or reference counting.

## 3. Signal Propagation

State in Martensite is fine-grained. There is no `render()` function that re-runs top-to-bottom.

```text
       [ Signal: user_name ]             [ Signal: theme_color ]
               │                                   │
       ┌───────┴───────┐                           │
       ▼               ▼                           ▼
[ Text Node ]    [ Input Node ]              [ Panel Node ]
 (Observer)       (Observer)                  (Observer)
```

When `user_name` changes:
1. The `Signal` directly holds `WidgetId`s of its observers.
2. It pushes a dirty flag *only* to the `Text Node` and `Input Node`.
3. The `Panel Node` remains entirely untouched. Time complexity is $O(1)$ relative to the size of the whole tree.

## 4. Taffy Layout Integration

Martensite strictly enforces the **Two-Pass Geometry Law**.
* **Intrinsic Measurement**: Nodes report how big they *want* to be based on their content (e.g., text shaping via `cosmic-text`).
* **Placement**: We hand these constraints to `Taffy`, a zero-allocation Rust layout engine. Taffy resolves standard CSS Flexbox and Grid rules, outputting exact pixel coordinates.
* **Result**: UI elements never jump, pop, or oscillate on the first frame because layout is mathematically finalized before the GPU ever sees the draw commands.

## 5. Vello Rendering

Martensite does not use traditional immediate-mode triangles or native OS widgets. It uses **Vello**, a 2D compute-shader rasterizer built on `wgpu`.
* Instead of uploading thousands of vertices, Martensite uploads high-level vector commands (Bezier curves, gradients, glyph runs).
* Vello uses the GPU's compute pipelines to resolve these shapes into tiles, bypassing the traditional rasterization pipeline bottlenecks.
* This is what gives Martensite its 1-pixel subpixel-snapped boundaries and physical rendering precision.

## 6. AccessKit Synchronization

Accessibility is mandatory. `AccessKit` provides a cross-platform semantic tree (bridging to Windows UIAutomation, macOS NSAccessibility, Linux AT-SPI2).
* Every interactive node implements the `accessibility` lifecycle method.
* When Taffy computes layout, the spatial bounds are synced to AccessKit.
* When a `Signal` mutates a button's disabled state, that state is synced to AccessKit.
* Native screen readers read the Martensite UI indistinguishably from a native OS application.

## 7. Common Contributor Mistakes

1. **Allocating in the Paint Loop**: Calling `Vec::new()` or `.clone()` inside a widget's `paint` method. **Fix**: Pre-allocate in the widget's state or use temporary bump allocators provided by the `PaintCtx`.
2. **Polling for Animations**: Spawning a thread to update a value 60 times a second. **Fix**: Use Martensite's analytical physics solvers (`SpringAnimation`) which automatically sleep when visually settled.
3. **Wrapping `Rc<RefCell<T>>`**: Trying to share state by wrapping widgets in reference-counted pointers. **Fix**: Use `Signal<T>` or store state external to the widget tree, passing only `WidgetId`s.
4. **Forgetting `accesskit` roles**: Adding a custom interactive widget but failing to emit `accesskit::TreeUpdate`. The widget will be invisible to screen readers.

## 8. Glossary

* **Arena**: The flat, generational `SlotMap` array holding all UI nodes.
* **Signal**: A fine-grained, push-pull reactive state primitive.
* **Vello**: The compute-shader 2D graphics rasterizer backing Martensite.
* **Taffy**: The zero-allocation Flexbox/Grid layout engine.
* **AccessKit**: The cross-platform accessibility synchronization bridge.
* **Zero-GC**: The absence of runtime garbage collection and zero dynamic allocations in the hot interaction path.
* **Direct GPU Rendering**: Direct rendering through GPU compute pipelines rather than wrapping native OS controls, ensuring cross-platform visual consistency.
