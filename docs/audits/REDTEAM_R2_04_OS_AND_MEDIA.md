# Architecture Review 04: OS Boundary & Color Science (Round 2)
**Date:** 2026-09-06
**Specialist:** Architecture Review Team

## 1. Low-Nit SDR Displays (<203 nits)
**Status:** VULNERABLE (Highlight Crushing & Clipping)

**The Flaw:**
Blue Team's color science mathematically fixes the SDR reference white to 203 nits for HDR compositing:
`C_scaled = C_linear_sdr * (203.0 / 10000.0)`
However, budget laptops and uncalibrated SDR monitors frequently have a physical peak luminance of 150-180 nits. 
When the OS/Display receives the composited PQ/scRGB signal, any value mapped above the display's actual peak luminance (e.g., 150 nits) is indiscriminately clipped by the display's internal tonemapper or the OS compositor.

**Concrete Counterexample:**
Assume a monitor with 150 nits peak brightness.
1. The UI draws a subtle gray-to-white gradient for a button (`C_srgb` from 0.9 to 1.0).
2. The framework scales this such that white (1.0) maps to 203 nits.
3. The display clips everything above 150 nits.
4. Any `C_srgb` value that maps to a linear luminance between 150 and 203 nits is clipped to pure white. 
5. The gradient is completely crushed, and subtle UI contrast is destroyed, rendering text or borders invisible.

**Remediation:** 
Do not hardcode 203 nits. Query the OS for the actual `SDR_WHITE_LEVEL` or the display's `MaxFullFrameLuminance`. If the display peak is less than 203 nits, dynamic range compression or a parameterized reference white must be utilized.

---

## 2. Kinetic IME Velocity Discontinuities
**Status:** VULNERABLE (Predictive Overshoot & Jitter)

**The Flaw:**
Blue Team attempts to solve IME detachment via a feed-forward velocity projection:
`P_predicted = P_current + v * t_delay`
This assumes that velocity $v$ is continuously differentiable. However, UI physics regularly exhibit step-function discontinuities in velocity (infinite acceleration).

**Concrete Counterexample:**
1. A user flings a scroll view towards a hard boundary. The velocity $v$ is high (e.g., 2000 pixels/sec).
2. The framework emits the packet with this high velocity.
3. The scroll view hits the boundary and abruptly stops (or rubber-bands backwards).
4. The framework emits a new packet with $v = 0$ (or $v < 0$).
5. Due to asynchronous IPC delay ($t_{delay}$), the OS compositor has already projected the IME window past the boundary using the previous high velocity.
6. The IME candidate window visibly detaches, overshoots the text input, and then violently snaps back a few frames later.

**Remediation:**
Introduce acceleration limits and explicitly flag discontinuity boundaries in the emission packet. When approaching a known physics boundary (like the end of a scroll view), velocity must be clamped in the projection, or the projection must evaluate bounded physics (e.g., stopping at `clip_rect` bounds) rather than simple linear extrapolation.

---

## 3. Non-Rectangular Hit Testing Matrix Inversion
**Status:** VULNERABLE (Singular Matrix Panics / NaNs)

**The Flaw:**
Blue Team's narrow-phase hit-testing computes local coordinates via the inverse of the affine transformation matrix:
`P_local = M^-1 * P`
This fails to account for singular matrices where the determinant $|M| \approx 0$. 

**Concrete Counterexample:**
1. A developer creates a 3D flip animation for a card widget (rotating it around the Y-axis).
2. At exactly the midpoint of the animation (90 degrees), the card is edge-on to the camera. The scale factor along the X-axis is projected to 0.
3. The resulting transformation matrix $M$ has a determinant of 0.
4. A user clicks on the screen, triggering hit-testing.
5. The matrix inversion algorithm attempts to divide by the determinant ($|M|$).
6. This division by zero produces `NaN` values for the hit coordinates, which subsequently corrupts the Point-in-Path distance algorithms, leading to unbounded panics or silent geometry routing failures.

**Remediation:**
Before computing `M^-1`, verify the determinant. If `det(M) < EPSILON`, the matrix is singular (the node is effectively invisible or has 0 area). The hit-test should immediately return `PassThrough` without attempting inversion.
