use martensite::prelude::*;

fn main() {
    println!("Initializing Martensite Industrial Workstation...");
    let _arena = WidgetArena::new();
    let signal = Signal::new(42);
    println!("Active Reactive Signal Value: {}", signal.get());
    println!("Arena + Signal primitives: operational.");
}
