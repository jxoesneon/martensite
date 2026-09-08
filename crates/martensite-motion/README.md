# martensite-motion

[![Crates.io](https://img.shields.io/crates/v/martensite-motion.svg)](https://crates.io/crates/martensite-motion)
[![Documentation](https://docs.rs/martensite-motion/badge.svg)](https://docs.rs/martensite-motion)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/jxoesneon/martensite#license)

> **Analytical closed-form spring physics solver with $C^1$ velocity continuity and frame-rate-independent motion drivers.**

---

## Overview

`martensite-motion` provides fluid, physically modeled animation physics for the Martensite GUI framework. Unlike heuristic easing curves (e.g. cubic-bezier), physical springs adapt naturally to user interruptions: if a user drags, releases, or redirects an element mid-animation, its momentum is preserved without visual popping or velocity discontinuity.

`martensite-motion` uses **exact closed-form analytical solutions** rather than numerical Euler integration. This guarantees that simulation results are frame-rate independent, numerically stable at variable refresh rates (60 Hz, 120 Hz, 240 Hz), and energy-conserving.

The entire crate is built under `#![forbid(unsafe_code)]`.

---

## Key Features

- **Exact Analytical Closed-Form Solver (`SpringSolver`)**:
  - Closed-form equations for all three damping regimes: **Underdamped**, **Critically Damped**, and **Overdamped** (`DampingRegime`).
  - True frame-rate independence: advancing time by $\Delta t$ computes the exact closed-form state directly without sub-stepping errors.
- **$C^1$ Velocity Continuity on Retargeting**: Retargeting a moving spring to a new destination seamlessly preserves the current instantaneous velocity vector, eliminating unnatural animation hitches.
- **Configurable Spring Parameters (`SpringConfig`)**: Fine-tune mass, stiffness ($k$), and damping ratio ($\zeta$) with automatic rest tolerance detection.
- **Higher-Level Animation Drivers**:
  - `AnimationDriver`: 1D scalar transition coordinator.
  - `AnimationDriver2D`: 2D spatial coordinate motion driver for gestures, sheets, and drag snaps.
- **100% Safe Rust**: Guaranteed memory safety with zero `unsafe` blocks.

---

## Quick Start

Add `martensite-motion` to your `Cargo.toml`:

```toml
[dependencies]
martensite-motion = "0.7.0"
```

Simulating spring motion:

```rust
use martensite_motion::{SpringConfig, SpringSolver};

fn main() {
    // Underdamped spring with subtle bounce
    let config = SpringConfig {
        stiffness: 170.0,
        damping: 14.0,
        mass: 1.0,
        rest_displacement_threshold: 0.001,
        rest_velocity_threshold: 0.001,
    };

    let mut solver = SpringSolver::new(config);
    solver.reset(0.0, 0.0); // Initial position 0.0, velocity 0.0
    solver.set_target(100.0);

    // Advance simulation by 16ms (1 frame at 60 FPS)
    let state = solver.advance(0.016);
    println!("Position: {}, Velocity: {}", state.position, state.velocity);
    assert!(!solver.is_at_rest());
}
```

---

## Part of Martensite

This crate powers animations and physics transitions for the [Martensite](https://github.com/jxoesneon/martensite) GUI framework. For top-level widgets and application integration, see [`martensite`](https://crates.io/crates/martensite).

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/jxoesneon/martensite/blob/main/LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/jxoesneon/martensite/blob/main/LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
