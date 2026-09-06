# Martensite Documentation Standards

Documentation in Martensite must be precise, technically accurate, and focused on clear mechanical descriptions. Marketing language, promotional phrasing, and ambiguous claims are avoided.

## 1. Required Rustdoc Structure for Public Items

Every public item (`pub struct`, `pub trait`, `pub fn`, `pub enum`, `pub const`) must have a comprehensive rustdoc string. The structure follows this section order:

1. **Short Summary**: A single, declarative sentence explaining what the item is or does. Full sentences are preferred.
2. **Extended Description**: Details on behavior, state management, and memory implications.
3. **`# Examples`**: Executable doctests proving usage.
4. **`# Panics`** (If applicable)
5. **`# Errors`** (If applicable)
6. **`# Safety`** (If applicable, required for all `unsafe fn` or `unsafe trait`)
7. **`# Limitations`**: Explicit boundaries and unsupported configurations of the API.

### Example

```rust
/// A generational arena handle referencing an allocated layout node.
///
/// This handle is a lightweight, 64-bit copyable identifier that tracks a layout node
/// within the contiguous `SlotMap`. Stale handles evaluated against the arena will safely
/// resolve to `None` rather than triggering use-after-free conditions.
///
/// # Examples
/// ```
/// # use martensite_arena::{NodeArena, NodeId};
/// let mut arena = NodeArena::new();
/// let node_id = arena.insert(LayoutNode::default());
/// assert!(arena.get(node_id).is_some());
/// ```
///
/// # Panics
/// Panics if the `generation` overflow counter exceeds `u32::MAX`. This is practically
/// impossible in standard applications but could theoretically occur in multi-year
/// continuous mutation loops.
///
/// # Limitations
/// This handle is bound to the exact arena that created it. Attempting to resolve
/// this handle against a different `NodeArena` instance will yield undefined logical
/// behavior (though it will not violate memory safety).
pub struct NodeId {
    // ...
}
```

## 2. Mandatory Sections

*   **`# Examples`**: Every public function, macro, and struct should include an executable example. The example must compile and run via `cargo test --doc`.
*   **`# Panics`**: If a function can panic under any circumstance (e.g., unwrapping an internal `Option`, indexing out of bounds, reaching capacity limits), document the exact conditions that cause the panic.
*   **`# Errors`**: If a function returns a `Result<T, E>`, document all possible variants of `E` that can be returned and the specific circumstances that trigger them.
*   **`# Safety`**: Any `unsafe fn` or `unsafe trait` must have a `# Safety` section outlining the precise contractual invariants the caller must uphold to avoid Undefined Behavior (UB).
*   **`# Limitations`**: Explicitly document what the API does not support (e.g., "This layout engine does not currently support vertical right-to-left CJK rendering").

## 3. Style and Clarity Guidelines

Martensite documentation prioritizes clarity, conciseness, and technical precision.

**Avoid Promotional Language:**
*   "Blazingly fast"
*   "Lightning fast"
*   "Magic" / "Magical"
*   "Just works"
*   "Highly optimized"
*   "Revolutionary"
*   "Next-generation"

**Directives:**
*   **Use measurable claims**: Instead of "fast layout," use "layout computes in $O(N)$ time over the widget hierarchy."
*   **Be objective**: Describe concrete mechanisms, inputs, outputs, and side effects.
*   **Avoid redundant prose**: Do not add filler sentences that simply restate the function signature without providing additional context.

## 4. Benchmark Documentation Format

When documenting performance boundaries (especially in `martensite-core` or `martensite-layout`), reference the explicit Criterion benchmarks.

```markdown
# Performance Characteristics

Measurement via `cargo bench --bench layout_grid_10k`.
* **Hardware:** Apple M3 Max, 14-core CPU, 36GB Unified Memory
* **Cold Layout:** 4.2ms (± 0.1ms)
* **Warm Recalculation (Dirty Node):** 0.8ms (± 0.05ms)
* **Resident Memory Overhead:** 16 bytes per node
```

## 5. Platform-Specific Notes

Martensite abstracts the OS, but leaks are inevitable. Use the `#[cfg_attr(..., doc = "...")]` macro or explicit Markdown callouts to document platform discrepancies.

```markdown
# Platform Differences

* **macOS:** Uses `NSWindow`, automatically handling `NSVisualEffectView` materials.
* **Windows:** Backed by `HWND`. Resizing may block the primary event loop unless handled via the `martensite-window` background thread dispatcher.
```

## 6. Internal Code Comment Standards

Internal code comments are for the maintainers. They must explain *why* something is done, not *what* is done (the code explains the *what*).

### The `// SAFETY:` Format
Every single `unsafe` block must be immediately preceded by a `// SAFETY:` comment proving why the operation is sound. The PR reviewer will explicitly audit this proof.

```rust
// SAFETY: `wgpu_hal::Device::create_buffer` requires `desc.size` to be a multiple of 4.
// We guarantee this on line 114 by applying `align_up(size, 4)`. Furthermore, the raw
// pointer `data` is verified to be valid for reads of `desc.size` bytes because it is
// derived from a contiguous `Vec<u8>` of equal or greater length.
unsafe {
    device.create_buffer_init(&desc, data);
}
```

### The `// INVARIANT:` Format
Use this to document logical data structural invariants that must be maintained across mutations.

```rust
struct SlotMap {
    // INVARIANT: `free_list_head` must always point to a valid slot index where
    // `slots[free_list_head].is_occupied == false`, or `u32::MAX` if full.
    free_list_head: u32,
    // ...
}
```

### The `// PERF:` Format
Use this to justify non-obvious code structures written specifically for optimization (e.g., avoiding branching, maximizing cache locality, hinting the branch predictor).

```rust
// PERF: We manually unroll this loop into batches of 4 to maximize SIMD pipeline utilization
// inside the Vello compute shader preparation phase. This yields a 12% reduction in
// CPU command encoding time on x86_64 architectures (see PR #1042).
for chunk in items.chunks_exact(4) {
    // ...
}
```
