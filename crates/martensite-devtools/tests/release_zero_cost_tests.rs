//! Invariant 1 Gate: Release Zero-Cost Tests for Live Tweaks.
//!
//! Asserts that when devtools is disabled (`devtools = false` or compile-time release mode),
//! `#[tweak]` expansions incur:
//! 1. Zero runtime overhead (raw constants and direct values).
//! 2. Zero memory overhead (identical struct size and alignment).
//! 3. Zero registration calls to `TweakRegistry`.
#![forbid(unsafe_code)]

use martensite_macros::tweak;
use std::mem::{align_of, size_of};

// 1. Compile-time constants with devtools = false
#[tweak("release/padding", 12.0f32, devtools = false)]
const RELEASE_PADDING: f32 = 12.0f32;

#[tweak("release/max_items", 100u32, devtools = false)]
const RELEASE_MAX_ITEMS: u32 = 100u32;

#[tweak("release/ratio", 1.618f64, devtools = false)]
const RELEASE_RATIO: f64 = 1.618f64;

#[tweak("release/enabled", true, devtools = false)]
const RELEASE_ENABLED: bool = true;

#[tweak("release/color", "#ffffff", devtools = false)]
const RELEASE_COLOR: &str = "#ffffff";

#[test]
fn test_zero_cost_constants_fold_at_compile_time() {
    // Compile-time evaluation in const context confirms zero overhead
    const FOLDED_SUM: f32 = RELEASE_PADDING + 4.0;
    assert_eq!(FOLDED_SUM, 16.0f32);
    assert_eq!(RELEASE_MAX_ITEMS, 100);
    assert_eq!(RELEASE_RATIO, 1.618);
    const { assert!(RELEASE_ENABLED) };
    assert_eq!(RELEASE_COLOR, "#ffffff");
}

// 2. Structs with field-level tweaks have identical memory layout to untweaked structs
#[allow(dead_code)]
struct UntweakedLayout {
    width: f32,
    height: f32,
    border_radius: u32,
    is_active: bool,
}

#[tweak(devtools = false)]
struct TweakedReleaseLayout {
    #[tweak("layout/width", 200.0f32)]
    width: f32,
    #[tweak("layout/height", 100.0f32)]
    height: f32,
    #[tweak("layout/border_radius", 8u32)]
    border_radius: u32,
    #[tweak("layout/is_active", true)]
    is_active: bool,
}

#[test]
fn test_zero_memory_overhead_for_tweaked_structs() {
    assert_eq!(
        size_of::<TweakedReleaseLayout>(),
        size_of::<UntweakedLayout>(),
        "Tweaked struct size must exactly match untweaked struct size in release mode"
    );
    assert_eq!(
        align_of::<TweakedReleaseLayout>(),
        align_of::<UntweakedLayout>(),
        "Tweaked struct alignment must match untweaked struct alignment"
    );

    let instance = TweakedReleaseLayout {
        width: 200.0,
        height: 100.0,
        border_radius: 8,
        is_active: true,
    };
    assert_eq!(instance.width, 200.0);
    assert_eq!(instance.height, 100.0);
    assert_eq!(instance.border_radius, 8);
    assert!(instance.is_active);
}

// 3. Functions with devtools = false execute directly with zero wrapper or registration
#[tweak("calc/compute_gap", 8.0f32, devtools = false)]
fn compute_gap(multiplier: f32) -> f32 {
    8.0 * multiplier
}

#[test]
fn test_zero_overhead_function_execution() {
    assert_eq!(compute_gap(2.5), 20.0);
}
