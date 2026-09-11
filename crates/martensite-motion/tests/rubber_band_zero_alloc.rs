//! Zero-allocation verification for `RubberBandScroller` per-frame methods.
//!
//! This is an integration test (a separate compilation unit) so it can install
//! a counting global allocator, which requires `unsafe`. The library crate
//! itself remains `#![forbid(unsafe_code)]`; only this test binary opts in via
//! `#![allow(unsafe_code)]`, which overrides the workspace-level
//! `unsafe_code = "deny"` lint.

#![allow(unsafe_code)]

use martensite_motion::{RubberBandScroller, RubberBandScroller2D};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Counting allocator wrapping the system allocator.
struct Counting;

/// Number of live allocations observed by [`Counting`].
static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        // SAFETY: `layout` is a valid layout forwarded unchanged to the system
        // allocator, whose `alloc` has the same safety contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` was returned by `System::alloc` with `layout`, matching
        // `System::dealloc`'s safety contract.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// `visible_offset()` and `update()` must perform zero heap allocations.
///
/// This is `#[ignore]` because installing a global allocator is process-global
/// and incompatible with running alongside other tests that may rely on the
/// default allocator's behaviour; run explicitly with
/// `cargo test --test rubber_band_zero_alloc -- --ignored`.
#[test]
#[ignore]
fn rubber_band_zero_alloc() {
    let mut scroller = RubberBandScroller::new(1000.0, 500.0);
    // Establish an overscrolled state with an active spring.
    scroller.drag(-100.0);
    scroller.release(320.0);
    assert!(scroller.spring().is_some());

    // Snapshot the allocator after setup; the measured calls must not allocate.
    let baseline = ALLOC_COUNT.load(Ordering::SeqCst);
    let _ = scroller.visible_offset();
    scroller.update(1.0 / 60.0);
    let _ = scroller.visible_offset();
    scroller.update(1.0 / 60.0);
    let after = ALLOC_COUNT.load(Ordering::SeqCst);

    assert_eq!(
        after,
        baseline,
        "visible_offset()/update() allocated {} byte(s); expected zero",
        after - baseline
    );

    // Also verify the 2D per-frame methods allocate nothing.
    let mut scroller2d = RubberBandScroller2D::new((1000.0, 1000.0), (500.0, 500.0));
    scroller2d.drag((-100.0, -80.0));
    scroller2d.release((200.0, -150.0));

    let baseline2d = ALLOC_COUNT.load(Ordering::SeqCst);
    let _ = scroller2d.visible_offset();
    scroller2d.update(1.0 / 60.0);
    let after2d = ALLOC_COUNT.load(Ordering::SeqCst);

    assert_eq!(
        after2d,
        baseline2d,
        "2D visible_offset()/update() allocated {} byte(s); expected zero",
        after2d - baseline2d
    );
}
