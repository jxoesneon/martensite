# Detailed Design Record: DDR-0018
## Title: `martensite-motion` Spring Physics & Quench Architecture

### 1. Architectural Role & Invariants
`martensite-motion` governs UI animation. It rejects fixed-duration easing curves in favor of interruptible spring physics.
* **Invariant 1.1**: Animations must inherit velocity seamlessly upon interruption (target change).
* **Invariant 1.2**: All spring solvers operate deterministically using the frame's elapsed delta time.
* **Invariant 1.3**: Springs must "quench" (deactivate) automatically when velocity and displacement fall below an imperceptible epsilon, enabling OS sleep (`ControlFlow::Wait`).

### 2. Spring Equations
Based on a damped harmonic oscillator:
$$ m \frac{d^2x}{dt^2} + c \frac{dx}{dt} + k(x - x_{target}) = 0 $$
Parameterized by `stiffness` ($k$) and `damping` ($c$).

```rust
#[derive(Clone, Copy, Debug)]
pub struct SpringConfig {
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
}

pub struct SpringState {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
}

pub fn advance_spring(state: &mut SpringState, config: &SpringConfig, dt: f32) -> bool {
    let displacement = state.value - state.target;
    let spring_force = -config.stiffness * displacement;
    let damping_force = -config.damping * state.velocity;
    let acceleration = (spring_force + damping_force) / config.mass;
    
    state.velocity += acceleration * dt;
    state.value += state.velocity * dt;
    
    // Quench condition
    state.velocity.abs() < 0.001 && displacement.abs() < 0.001
}
```

### 3. CPU Sleep Integration
The Event Loop maintains an `active_animations` counter.
- Frame starts: Compute `advance_spring` for all active nodes.
- If `advance_spring` returns `true` (quenched), decrement counter.
- If counter == 0, `martensite-window` yields to `ControlFlow::Wait`.

### 4. Performance Invariants
- Computation: Closed-form or RK4 numeric integration depending on non-linear modifiers. RK4 overhead is strictly $O(1)$ per spring, target < 50ns per node.
