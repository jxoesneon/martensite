//! Zero-allocation regression test for the theme transition path.
//!
//! This is an integration test (a separate crate) so that it can install a
//! custom `#[global_allocator]` — which requires `unsafe` — without weakening
//! the `#![forbid(unsafe_code)]` guarantee of the `martensite-theme` library
//! crate. The library itself remains fully `forbid(unsafe_code)`-safe.
//!
//! The v0.6.0 milestone claims that a theme switch keeps the 5,000-widget
//! scene at continuous 60/120 fps with **zero CPU allocations**: theme swaps
//! upload two 256-byte [`ThemeUniforms`] snapshots and blend them in the
//! fragment shader, so the CPU-side per-frame path
//! ([`ThemeUniforms::from_theme`], [`ThemeTransition::advance`],
//! [`ThemeTransition::current_uniforms`], and the [`ThemeUniformBuffer`]
//! upload-slice accessors) must never touch the heap.
//!
//! The test is `#[ignore]`d by default because it is sensitive to
//! allocations from other threads in the process. CI is the gate of record:
//! the `performance-gates` job runs it in release mode via `cargo test
//! --release -p martensite-theme --test theme_transition_zero_alloc --
//! --ignored`.

// The workspace sets `unsafe_code = "deny"` via `[lints] workspace = true`.
// This test crate legitimately needs `unsafe` to implement `GlobalAlloc`, so
// the deny level is overridden here. The library crate stays forbid-safe.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

use martensite_theme::gpu_transition::{ThemeTransition, ThemeUniformBuffer, ThemeUniforms};
use martensite_theme::tokens::{default_dark, default_light};

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

/// One full theme-transition frame: advance the transition, produce the
/// interpolated uniforms, and extract the GPU-upload byte slices. This is
/// exactly what the render loop does each frame while a light/dark switch
/// is in flight.
fn transition_frame(
    transition: &mut ThemeTransition,
    uniforms: &mut ThemeUniformBuffer,
    dt: f32,
) -> usize {
    transition.advance(dt);
    let current = transition.current_uniforms();
    uniforms.set_from(current);
    uniforms.set_to(transition.to);
    uniforms.set_t(transition.current_t());
    // Consume the upload slices so the work is not optimized away.
    black_box(uniforms.from_bytes()).len()
        + black_box(uniforms.to_bytes()).len()
        + black_box(current.as_bytes()).len()
}

/// Drives a complete 150 ms transition at 60 fps (9 frames), then restarts
/// a new transition in the opposite direction — the steady-state workload
/// of repeated theme switches.
fn run_theme_switch_cycle(
    from: &ThemeUniforms,
    to: &ThemeUniforms,
    uniforms: &mut ThemeUniformBuffer,
) -> usize {
    let mut transition = ThemeTransition::new(*from, *to);
    let mut acc = 0usize;
    // 0.150s / 9 frames ≈ 16.7ms per frame.
    for _ in 0..9 {
        acc += transition_frame(&mut transition, uniforms, 1.0 / 60.0);
    }
    acc
}

/// Performs theme-transition frame cycles and verifies that no heap
/// allocation occurs in steady state.
///
/// CI is the gate of record for this check: the `performance-gates` job runs
/// it in release mode.
#[test]
#[ignore = "zero-alloc gate, allocation-sensitive; runs in the CI \
           performance-gates job via --release --ignored"]
fn theme_transition_zero_alloc() {
    // Theme construction allocates (HashMap + name String); do it before
    // the measured windows — this mirrors production, where themes are
    // built once and snapshots are re-used across frames.
    let light = default_light();
    let dark = default_dark();
    let from_light = ThemeUniforms::from_theme(&light);
    let from_dark = ThemeUniforms::from_theme(&dark);

    let mut uniforms = ThemeUniformBuffer::new();

    // Warm up: run one full cycle so any lazy initialization happens before
    // we start counting.
    let _ = black_box(run_theme_switch_cycle(
        &from_light,
        &from_dark,
        &mut uniforms,
    ));

    // Two measured windows: the first absorbs any residual one-shot
    // allocations elsewhere in the test process (harness internals, TLS
    // setup on the test thread), the second asserts the steady state.
    // A real regression allocates on every frame, so the second window
    // would show hundreds of allocations, not a handful.
    for _ in 0..1_000 {
        let _ = black_box(run_theme_switch_cycle(
            &from_light,
            &from_dark,
            &mut uniforms,
        ));
        let _ = black_box(run_theme_switch_cycle(
            &from_dark,
            &from_light,
            &mut uniforms,
        ));
    }
    let before = ALLOC_COUNT.load(Ordering::SeqCst);
    for _ in 0..1_000 {
        let _ = black_box(run_theme_switch_cycle(
            &from_light,
            &from_dark,
            &mut uniforms,
        ));
        let _ = black_box(run_theme_switch_cycle(
            &from_dark,
            &from_light,
            &mut uniforms,
        ));
    }
    let after = ALLOC_COUNT.load(Ordering::SeqCst);

    assert_eq!(
        after,
        before,
        "theme transition frames allocated {} time(s); expected zero allocation \
         on the per-frame transition path",
        after - before
    );
}
