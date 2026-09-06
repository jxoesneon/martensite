//! Industrial dashboard example demonstrating Martensite arena and signal primitives.
use martensite::prelude::*;

fn main() {
    println!("Initializing Martensite Industrial Workstation...");
    let _arena = WidgetArena::new();
    let signal = Signal::new(42);
    println!("Active Reactive Signal Value: {}", signal.get());
    println!("Arena + Signal primitives: operational.");
}

/// Smoke test verifying the example crate compiles and links correctly.
#[cfg(test)]
#[test]
fn example_smoke_test() {
    let arena = WidgetArena::new();
    assert!(arena.is_empty());
    let signal = Signal::new(42);
    assert_eq!(signal.get(), 42);
}
