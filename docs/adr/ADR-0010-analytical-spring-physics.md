# [ADR-0010] Analytical Damped Harmonic Oscillator Physics (Spring Motion)

* **Status:** Accepted
* **Date:** 2026-09-06
* **Deciders:** Master (Sovereign Architect), Ciel (Systems, Experience & Quality Guilds)
* **Technical Domain:** `martensite-motion`, `martensite-reactive`

## Context and Problem Statement

Animations in web and legacy desktop applications rely predominantly on fixed-duration cubic-bezier curves (`ease-in-out`, CSS transitions). 
* **The Velocity Discontinuity Catastrophe**: When a user intercepts an animating element mid-flight (e.g., dragging an animating card or flinging a scrolling pane), cubic-bezier systems suffer a discontinuous velocity jump: either snapping velocity to zero (causing a visible visual hitch) or recalculating a fixed 300ms duration over a tiny distance (causing an unnatural speed explosion).
* **Numerical Integration Instability**: Naive physics engines use forward Euler or Verlet integration per frame. When frame hitches occur or displays switch refresh rates, numerical integration drifts, oscillates wildly, or diverges to infinity.
* **Continuous Redraw Battery Drain**: Running animations that lack exact analytical settling thresholds continue polling frame loops indefinitely, burning laptop battery life even when visual displacement is imperceptible.

## Decision Drivers

* Continuous C1 velocity preservation across mid-flight user interruptions.
* O(1) constant-time analytical closed-form evaluation (zero numerical drift).
* Frame-rate independence across variable refresh rate displays (60Hz to 240Hz).
* Zero idle CPU/GPU consumption: animations must automatically quench and sleep upon crossing threshold boundaries.

## Considered Options

* **Option 1**: Fixed-duration cubic-bezier curves (CSS-style easing).
* **Option 2**: Numerical Euler/Verlet spring integration.
* **Option 3**: **Analytical Closed-Form Damped Harmonic Oscillator Solvers + Velocity Inheritance + Event-Driven Sleep Quenching**.

## Decision Outcome

Chosen option: **Option 3**, because it provides mathematically exact, display-rate-independent physics with seamless C1 velocity continuity and automatic 0.0% CPU idle sleep.

### Positive Consequences

* **Physical Realism**: Animations behave according to true classical mechanics. Motion feels tactile, organic, and premium.
* **Zero Numerical Drift**: Evaluating x(t) and v(t) uses exact closed-form calculus for any timestamp t >= 0. Frame drops or CPU stalls never cause springs to explode or skip targets.
* **True Quenching & Battery Preservation**: When displacement |x(t) - x_target| < 0.001 and velocity |v(t)| < 0.001, the spring instantly snaps to target, de-registers from the active animation registry, and halts redraws, allowing the application to sleep at **0.00% CPU utilization**.
