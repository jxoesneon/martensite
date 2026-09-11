//! Zero-allocation regression test for the BSP docking tree.
//!
//! This is an integration test (a separate crate) so that it can install a
//! custom `#[global_allocator]` — which requires `unsafe` — without weakening
//! the `#![forbid(unsafe_code)]` guarantee of the `martensite-blessed` library
//! crate. The library itself remains fully `forbid(unsafe_code)`-safe.
//!
//! The test is `#[ignore]`d by default because it is sensitive to
//! allocations from other threads in the process. CI is the gate of record:
//! run it explicitly with `cargo test -p martensite-blessed --test
//! docking_zero_alloc -- --ignored`.

// The workspace sets `unsafe_code = "deny"` via `[lints] workspace = true`.
// This test crate legitimately needs `unsafe` to implement `GlobalAlloc`, so
// the deny level is overridden here. The library crate stays forbid-safe.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use martensite_blessed::{DockPanel, DockTree, NodeId, SplitDirection};

/// A wrapper around the system allocator that counts `alloc` calls.
struct CountingAllocator;

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        // SAFETY: forwarded to the system allocator with the same layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded to the system allocator with the same layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static A: CountingAllocator = CountingAllocator;

fn make_panel(id: u64) -> DockPanel {
    // An empty title produces a `String::new()`, which does *not* allocate.
    // This keeps panel construction out of the allocation budget so the test
    // measures only the docking tree operations themselves.
    DockPanel::new(id, "")
}

/// Performs 10,000 split/merge cycles within the slab capacity and verifies
/// that no heap allocation occurs.
///
/// CI is the gate of record for this check.
#[test]
#[ignore = "allocation-sensitive; run explicitly in CI with --ignored"]
fn docking_zero_alloc_split_merge() {
    // Use a small, fixed capacity so the slab never needs to grow. Each cycle
    // oscillates between 1 node (leaf) and 3 nodes (split + 2 leaves), well
    // within the 64-slot pre-allocated arena.
    let mut tree = DockTree::with_capacity(64);
    let root: NodeId = tree.insert_root(make_panel(0));

    // Warm up: perform one cycle so any lazy initialization inside the slab
    // happens before we start counting.
    let _ = tree.split_leaf(root, SplitDirection::Vertical, 0.5, make_panel(1));
    tree.merge(root).unwrap();

    let before = ALLOC_COUNT.load(Ordering::SeqCst);
    for _ in 0..10_000 {
        let _ = tree.split_leaf(root, SplitDirection::Vertical, 0.5, make_panel(1));
        tree.merge(root).unwrap();
    }
    let after = ALLOC_COUNT.load(Ordering::SeqCst);

    assert_eq!(
        after,
        before,
        "split/merge cycle allocated {} time(s); expected zero allocation within slab capacity",
        after - before
    );
    // Sanity: the tree ended back as a single leaf.
    assert_eq!(tree.panel_count(), 1);
}
