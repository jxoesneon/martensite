//! 6-DoF Kalman smoothing for stylus/pen input.
//!
//! Raw tablet events arrive with sub-pixel jitter on position, quantization
//! noise on pressure, and angular quantization on tilt/twist/azimuth. Feeding
//! those values directly into stroke tessellation produces wobbly strokes and
//! visible pressure banding. This module implements a fixed-size, zero-allocation
//! Kalman filter that smooths all six degrees of freedom in `O(1)` per event.
//!
//! # State layout
//!
//! The main filter tracks a 12-dimensional state vector with a
//! constant-velocity (for position) / constant-angular-velocity (for
//! orientation) model:
//!
//! | index | meaning            |
//! |-------|--------------------|
//! | 0..2  | position x, y, z   |
//! | 3..5  | velocity vx, vy, vz|
//! | 6..8  | tilt, twist, azimuth (rad) |
//! | 9..11 | angular velocity ω_tilt, ω_twist, ω_azimuth (rad/s) |
//!
//! A separate 2-state Kalman filter (value + rate) smooths pressure. The
//! depth coordinate `z` is derived from the smoothed pressure so that
//! pressure-sensitive stroke width and z-depth stay consistent.
//!
//! # Performance
//!
//! Every matrix operation is hand-rolled over stack-allocated `f32` arrays.
//! The dominant cost is the 12×12 covariance predict (`1728` multiplies) plus
//! a 5×5 innovation-matrix inversion. The whole `update` is well under the
//! 2 ms per-event budget on commodity hardware — see the
//! `kalman_stylus_latency` test.

/// Initial diagonal covariance for a freshly reset filter.
///
/// A large value expresses high initial uncertainty so the first few
/// measurements are trusted over the (zero) prior.
const INIT_COV: f32 = 100.0;

/// State indices observed by the 5-measurement correction step
/// `(x, y, tilt, twist, azimuth)`. The measurement matrix `H` is the
/// 5×12 selector that picks exactly these rows out of the 12-state vector.
const H_IDX: [usize; 5] = [0, 1, 6, 7, 8];

/// A raw, unfiltered stylus measurement from the tablet driver.
///
/// All angles are in radians. `azimuth` is the direction the pen is leaning
/// towards (rotation about the surface normal); `tilt` is the angle between
/// the pen barrel and the surface normal (`0` = vertical).
///
/// # Examples
///
/// ```
/// use martensite_window::stylus::StylusState;
/// let s = StylusState {
///     x: 120.0,
///     y: 340.0,
///     pressure: 0.75,
///     tilt: 0.3,
///     twist: 0.0,
///     azimuth: 1.57,
/// };
/// assert_eq!(s.x, 120.0);
/// assert_eq!(s.pressure, 0.75);
/// assert_eq!(s.azimuth, 1.57);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StylusState {
    /// Surface-space X position in logical pixels.
    pub x: f32,
    /// Surface-space Y position in logical pixels.
    pub y: f32,
    /// Normalized pressure in `[0, 1]` as reported by the tablet.
    pub pressure: f32,
    /// Tilt angle in radians (`0` = perpendicular to the surface).
    pub tilt: f32,
    /// Barrel rotation (twist) in radians.
    pub twist: f32,
    /// Azimuth (tilt direction) in radians, derived from the tilt direction
    /// or the tablet's azimuth report.
    pub azimuth: f32,
}

/// Smoothed stylus state produced by [`KalmanStylus::update`].
///
/// In addition to the measured degrees of freedom this carries the filter's
/// velocity estimates (useful for predictive stroke rendering) and a
/// pressure-derived depth `z`.
///
/// # Examples
///
/// ```
/// use martensite_window::stylus::{FilteredStylusState, KalmanStylus, StylusState};
///
/// let mut kf = KalmanStylus::new();
/// let m = StylusState {
///     x: 10.0, y: 20.0, pressure: 0.5,
///     tilt: 0.2, twist: 0.0, azimuth: 0.0,
/// };
/// let out: FilteredStylusState = kf.update(m, 0.016);
/// assert!(out.x >= 9.0 && out.x <= 11.0);
/// assert!(out.pressure >= 0.0 && out.pressure <= 1.0);
/// assert!(out.vx.is_finite());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilteredStylusState {
    /// Smoothed X position in logical pixels.
    pub x: f32,
    /// Smoothed Y position in logical pixels.
    pub y: f32,
    /// Pressure-derived depth coordinate (equals smoothed pressure).
    pub z: f32,
    /// Smoothed X velocity in pixels per second.
    pub vx: f32,
    /// Smoothed Y velocity in pixels per second.
    pub vy: f32,
    /// Pressure rate (z velocity) in pressure units per second.
    pub vz: f32,
    /// Smoothed tilt in radians.
    pub tilt: f32,
    /// Smoothed twist in radians.
    pub twist: f32,
    /// Smoothed azimuth in radians.
    pub azimuth: f32,
    /// Smoothed pressure in `[0, 1]`.
    pub pressure: f32,
}

/// A 6-DoF Kalman filter for stylus/pen input smoothing.
///
/// The filter owns a 12-element state vector, a 12×12 covariance matrix
/// (row-major, `144` elements), and a separate 2-state pressure Kalman
/// filter. All operations are `O(1)` and perform zero heap allocation — every
/// matrix is a stack-allocated `f32` array.
///
/// # Examples
///
/// ```
/// use martensite_window::stylus::{KalmanStylus, StylusState};
///
/// let mut kf = KalmanStylus::new();
/// let m = StylusState {
///     x: 5.0, y: 5.0, pressure: 0.5,
///     tilt: 0.1, twist: 0.0, azimuth: 0.0,
/// };
/// // Feed a stream of events; each call is O(1) with no allocation.
/// for i in 0..10 {
///     let m = StylusState { x: 5.0 + i as f32, y: 5.0, pressure: 0.5, tilt: 0.1, twist: 0.0, azimuth: 0.0 };
///     let _ = kf.update(m, 0.016);
/// }
/// // Read the current smoothed state without advancing the filter.
/// let snap = kf.state();
/// assert!(snap.x > 5.0);
/// ```
#[derive(Debug, Clone)]
pub struct KalmanStylus {
    /// 12-element state vector `x` (position, velocity, orientation, angular velocity).
    state: [f32; 12],
    /// 12×12 covariance matrix `P` in row-major order.
    covariance: [f32; 144],
    /// 2-element pressure state `[value, rate]`.
    pressure_state: [f32; 2],
    /// 2×2 pressure covariance in row-major order.
    pressure_covariance: [f32; 4],
    /// Process-noise tuning scalar `Q` added to the covariance diagonal each predict.
    process_noise: f32,
    /// Measurement-noise tuning scalar `R` added to the innovation covariance.
    measurement_noise: f32,
    /// Accumulated timestamp (seconds) of the most recent update, or `None` if
    /// the filter has never been updated.
    last_update: Option<f32>,
}

impl KalmanStylus {
    /// Creates a new filter with default tuning suited to typical stylus
    /// input (~60-120 Hz tablets, sub-pixel position jitter).
    ///
    /// The defaults favour smoothing: a small process noise (`0.01`) relative
    /// to the measurement noise (`1.0`) gives a low-bandwidth response that
    /// suppresses jitter without introducing noticeable lag.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::stylus::KalmanStylus;
    ///
    /// let kf = KalmanStylus::new();
    /// // A fresh filter has not converged yet.
    /// assert!(!kf.is_settled());
    /// ```
    pub fn new() -> Self {
        Self::with_tuning(0.01, 1.0)
    }

    /// Creates a new filter with caller-supplied process and measurement
    /// noise tuning.
    ///
    /// Larger `process_noise` makes the filter more responsive (it trusts new
    /// measurements more); larger `measurement_noise` makes it smoother (it
    /// trusts the constant-velocity model more). Both values must be finite
    /// and non-negative; non-finite values fall back to the defaults.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::stylus::KalmanStylus;
    ///
    /// // Aggressive smoothing for a noisy sensor.
    /// let kf = KalmanStylus::with_tuning(0.001, 4.0);
    /// assert!(!kf.is_settled());
    /// ```
    pub fn with_tuning(process_noise: f32, measurement_noise: f32) -> Self {
        let pn = if process_noise.is_finite() && process_noise >= 0.0 {
            process_noise
        } else {
            0.01
        };
        let mn = if measurement_noise.is_finite() && measurement_noise >= 0.0 {
            measurement_noise
        } else {
            1.0
        };
        let mut kf = Self {
            state: [0.0; 12],
            covariance: [0.0; 144],
            pressure_state: [0.0; 2],
            pressure_covariance: [0.0; 4],
            process_noise: pn,
            measurement_noise: mn,
            last_update: None,
        };
        kf.reset_internal();
        kf
    }

    /// Runs one full predict + correct cycle and returns the smoothed state.
    ///
    /// `dt` is the elapsed time in seconds since the previous update. The
    /// call is `O(1)` and performs zero heap allocation — all matrix math runs
    /// over stack-allocated fixed-size arrays.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::stylus::{KalmanStylus, StylusState};
    ///
    /// let mut kf = KalmanStylus::new();
    /// let m = StylusState {
    ///     x: 12.0, y: 7.0, pressure: 0.6,
    ///     tilt: 0.05, twist: 0.0, azimuth: 0.2,
    /// };
    /// let out = kf.update(m, 0.016);
    /// assert!(out.x.is_finite());
    /// assert!(out.pressure.is_finite());
    /// ```
    pub fn update(&mut self, measurement: StylusState, dt: f32) -> FilteredStylusState {
        // Sanitize dt: a non-finite or negative delta collapses to zero so the
        // predict step becomes a no-op (the filter just corrects).
        let dt = if dt.is_finite() && dt > 0.0 { dt } else { 0.0 };

        // --- Predict (12-state) -------------------------------------------------
        let f = build_state_transition(dt);
        // x = F * x
        self.state = mat12_vec_mul(&f, &self.state);
        // P = F * P * F^T
        let fp = mat12_mul(&f, &self.covariance);
        let fpt = mat12_transpose(&f);
        let mut new_p = mat12_mul(&fp, &fpt);
        // Add process noise Q (diagonal).
        for i in 0..12 {
            new_p[i * 12 + i] += self.process_noise;
        }
        self.covariance = new_p;

        // --- Pressure sub-filter (2-state) -------------------------------------
        self.predict_pressure(dt);
        self.correct_pressure(measurement.pressure);

        // --- Derive z / vz from the smoothed pressure --------------------------
        // The depth coordinate z mirrors the normalized pressure so that
        // pressure-driven stroke width and z-depth stay consistent. The z/vz
        // covariance block is pinned to the pressure covariance so the
        // settled-state check reflects the real uncertainty.
        self.state[2] = self.pressure_state[0];
        self.state[5] = self.pressure_state[1];
        self.covariance[2 * 12 + 2] = self.pressure_covariance[0];
        self.covariance[2 * 12 + 5] = self.pressure_covariance[1];
        self.covariance[5 * 12 + 2] = self.pressure_covariance[2];
        self.covariance[5 * 12 + 5] = self.pressure_covariance[3];

        // --- Correct (5-measurement main filter) -------------------------------
        self.correct_main(measurement);

        // Symmetrize P to fight accumulated floating-point asymmetry.
        symmetrize12(&mut self.covariance);

        // Track the accumulated timestamp.
        self.last_update = Some(self.last_update.unwrap_or(0.0) + dt);

        self.state()
    }

    /// Returns the current smoothed state without advancing the filter.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::stylus::{KalmanStylus, StylusState};
    ///
    /// let mut kf = KalmanStylus::new();
    /// let m = StylusState { x: 1.0, y: 2.0, pressure: 0.5, tilt: 0.0, twist: 0.0, azimuth: 0.0 };
    /// kf.update(m, 0.016);
    /// let snap = kf.state();
    /// // The first update pulls the estimate most of the way toward the
    /// // measurement (Kalman gain 100/101 ≈ 0.99 with the default prior).
    /// assert!((snap.x - 1.0).abs() < 0.05);
    /// assert!((snap.pressure - 0.5).abs() < 0.05);
    /// ```
    pub fn state(&self) -> FilteredStylusState {
        FilteredStylusState {
            x: self.state[0],
            y: self.state[1],
            z: self.state[2],
            vx: self.state[3],
            vy: self.state[4],
            vz: self.state[5],
            tilt: self.state[6],
            twist: self.state[7],
            azimuth: self.state[8],
            pressure: self.pressure_state[0],
        }
    }

    /// Resets the filter to its initial (unconverged) state, forgetting all
    /// prior measurements.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::stylus::{KalmanStylus, StylusState};
    ///
    /// let mut kf = KalmanStylus::new();
    /// let m = StylusState { x: 100.0, y: 100.0, pressure: 0.9, tilt: 0.0, twist: 0.0, azimuth: 0.0 };
    /// kf.update(m, 0.016);
    /// kf.reset();
    /// let snap = kf.state();
    /// assert_eq!(snap.x, 0.0);
    /// assert!(!kf.is_settled());
    /// ```
    pub fn reset(&mut self) {
        self.reset_internal();
    }

    /// Returns `true` once the filter's position covariance has fallen below
    /// the settled threshold, indicating it has converged onto the input
    /// signal and is producing trustworthy smoothed output.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::stylus::{KalmanStylus, StylusState};
    ///
    /// let mut kf = KalmanStylus::new();
    /// assert!(!kf.is_settled());
    /// for i in 0..200 {
    ///     let m = StylusState { x: i as f32, y: 0.0, pressure: 0.5, tilt: 0.0, twist: 0.0, azimuth: 0.0 };
    ///     kf.update(m, 0.016);
    /// }
    /// assert!(kf.is_settled());
    /// ```
    pub fn is_settled(&self) -> bool {
        // Trace of the covariance matrix. With the default INIT_COV=100 the
        // initial trace is 1200; the threshold is scaled by the measurement
        // noise so it adapts to the chosen tuning.
        let mut trace = 0.0f32;
        for i in 0..12 {
            trace += self.covariance[i * 12 + i];
        }
        let threshold = self.measurement_noise * 24.0;
        trace.is_finite() && trace < threshold
    }

    /// Shared body of [`new`](Self::new)/[`with_tuning`](Self::with_tuning)
    /// and [`reset`](Self::reset).
    fn reset_internal(&mut self) {
        self.state = [0.0; 12];
        self.covariance = mat12_identity();
        for i in 0..12 {
            self.covariance[i * 12 + i] = INIT_COV;
        }
        self.pressure_state = [0.0, 0.0];
        self.pressure_covariance = [INIT_COV, 0.0, 0.0, INIT_COV];
        self.last_update = None;
    }

    /// Pressure sub-filter predict step (2-state constant-rate model).
    fn predict_pressure(&mut self, dt: f32) {
        // x = F * x with F = [[1, dt],[0,1]].
        self.pressure_state[0] += self.pressure_state[1] * dt;

        // P = F * P * F^T + Q.
        let [a, b, c, d] = self.pressure_covariance;
        let predicted = [a + dt * c + dt * b + dt * dt * d, b + dt * d, c + dt * d, d];
        self.pressure_covariance = [
            predicted[0] + self.process_noise,
            predicted[1],
            predicted[2],
            predicted[3] + self.process_noise,
        ];
    }

    /// Pressure sub-filter correct step. `measured` is the raw pressure.
    fn correct_pressure(&mut self, measured: f32) {
        let r = self.measurement_noise;
        let innov = measured - self.pressure_state[0];
        let s = self.pressure_covariance[0] + r;
        if !s.is_finite() || s <= 0.0 {
            return;
        }
        let k0 = self.pressure_covariance[0] / s;
        let k1 = self.pressure_covariance[2] / s;
        self.pressure_state[0] += k0 * innov;
        self.pressure_state[1] += k1 * innov;
        let [p0, p1, p2, p3] = self.pressure_covariance;
        // P = (I - K*H) * P, H = [1, 0], K = [k0, k1]^T.
        self.pressure_covariance = [(1.0 - k0) * p0, (1.0 - k0) * p1, p2 - k1 * p0, p3 - k1 * p1];
    }

    /// Main 5-measurement correction step using the sparse measurement
    /// matrix `H` that selects `(x, y, tilt, twist, azimuth)`.
    fn correct_main(&mut self, measurement: StylusState) {
        let z_meas: [f32; 5] = [
            measurement.x,
            measurement.y,
            measurement.tilt,
            measurement.twist,
            measurement.azimuth,
        ];

        // S = H * P * H^T + R  (5x5). Because H is a selector, S[i][j] is just
        // P[H_IDX[i]][H_IDX[j]].
        let mut s = [0.0f32; 25];
        for i in 0..5 {
            for j in 0..5 {
                s[i * 5 + j] = self.covariance[H_IDX[i] * 12 + H_IDX[j]];
            }
            s[i * 5 + i] += self.measurement_noise;
        }
        let s_inv = mat5_inverse(&s);

        // K = P * H^T * S^-1  (12x5). PHt[i][k] = P[i][H_IDX[k]].
        let mut gain = [0.0f32; 60]; // 12x5 row-major
        for i in 0..12 {
            for j in 0..5 {
                let mut sum = 0.0;
                for k in 0..5 {
                    let ph = self.covariance[i * 12 + H_IDX[k]];
                    sum += ph * s_inv[k * 5 + j];
                }
                gain[i * 5 + j] = sum;
            }
        }

        // Innovation y = z - H * x.
        let mut innov = [0.0f32; 5];
        for j in 0..5 {
            innov[j] = z_meas[j] - self.state[H_IDX[j]];
        }

        // x = x + K * y.
        for i in 0..12 {
            let mut sum = 0.0;
            for j in 0..5 {
                sum += gain[i * 5 + j] * innov[j];
            }
            self.state[i] += sum;
        }

        // P = (I - K * H) * P = P - K * (H * P). HP[k][j] = P[H_IDX[k]][j].
        let mut khp = [0.0f32; 144];
        for i in 0..12 {
            for j in 0..12 {
                let mut sum = 0.0;
                for k in 0..5 {
                    sum += gain[i * 5 + k] * self.covariance[H_IDX[k] * 12 + j];
                }
                khp[i * 12 + j] = sum;
            }
        }
        for (cov, k) in self.covariance.iter_mut().zip(khp.iter()) {
            *cov -= k;
        }
    }
}

impl Default for KalmanStylus {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Private fixed-size matrix helpers (12x12 / 12x1 / 5x5).
// ---------------------------------------------------------------------------

/// Builds the 12×12 constant-velocity state-transition matrix `F(dt)`.
fn build_state_transition(dt: f32) -> [f32; 144] {
    let mut f = mat12_identity();
    // position += velocity * dt  (row-major index = r*12 + c)
    f[idx12(0, 3)] = dt;
    f[idx12(1, 4)] = dt;
    f[idx12(2, 5)] = dt;
    // orientation += angular_velocity * dt
    f[idx12(6, 9)] = dt;
    f[idx12(7, 10)] = dt;
    f[idx12(8, 11)] = dt;
    f
}

/// Row-major index into a 12×12 matrix: `r * 12 + c`.
fn idx12(r: usize, c: usize) -> usize {
    r * 12 + c
}

/// 12×12 matrix multiply: returns `a * b` (row-major).
fn mat12_mul(a: &[f32; 144], b: &[f32; 144]) -> [f32; 144] {
    let mut out = [0.0f32; 144];
    for i in 0..12 {
        for k in 0..12 {
            let aik = a[i * 12 + k];
            if aik == 0.0 {
                continue;
            }
            for j in 0..12 {
                out[i * 12 + j] += aik * b[k * 12 + j];
            }
        }
    }
    out
}

/// 12×12 * 12×1 matrix-vector multiply.
fn mat12_vec_mul(m: &[f32; 144], v: &[f32; 12]) -> [f32; 12] {
    let mut out = [0.0f32; 12];
    for i in 0..12 {
        let mut sum = 0.0;
        for j in 0..12 {
            sum += m[i * 12 + j] * v[j];
        }
        out[i] = sum;
    }
    out
}

/// 12×12 matrix addition.
#[allow(dead_code)]
fn mat12_add(a: &[f32; 144], b: &[f32; 144]) -> [f32; 144] {
    let mut out = [0.0f32; 144];
    for i in 0..144 {
        out[i] = a[i] + b[i];
    }
    out
}

/// 12×12 transpose.
fn mat12_transpose(m: &[f32; 144]) -> [f32; 144] {
    let mut out = [0.0f32; 144];
    for i in 0..12 {
        for j in 0..12 {
            out[j * 12 + i] = m[i * 12 + j];
        }
    }
    out
}

/// 12×12 identity matrix.
fn mat12_identity() -> [f32; 144] {
    let mut out = [0.0f32; 144];
    for i in 0..12 {
        out[i * 12 + i] = 1.0;
    }
    out
}

/// 12×1 vector addition.
#[allow(dead_code)]
fn vec12_add(a: &[f32; 12], b: &[f32; 12]) -> [f32; 12] {
    let mut out = [0.0f32; 12];
    for i in 0..12 {
        out[i] = a[i] + b[i];
    }
    out
}

/// Symmetrize a 12×12 matrix in place: `P = (P + P^T) / 2`.
fn symmetrize12(p: &mut [f32; 144]) {
    for i in 0..12 {
        for j in (i + 1)..12 {
            let avg = (p[i * 12 + j] + p[j * 12 + i]) * 0.5;
            p[i * 12 + j] = avg;
            p[j * 12 + i] = avg;
        }
    }
}

/// Inverts a 5×5 matrix via Gauss-Jordan elimination with partial pivoting.
///
/// The innovation covariance `S` is symmetric positive definite in normal
/// operation, so the inverse always exists. If a pivot is numerically zero
/// (degenerate input), the corresponding output row is left at zero so the
/// filter degrades gracefully rather than producing `NaN`.
fn mat5_inverse(m: &[f32; 25]) -> [f32; 25] {
    // Augmented matrix [m | I] stored as 5 rows of 10 columns.
    let mut aug = [[0.0f32; 10]; 5];
    for i in 0..5 {
        for j in 0..5 {
            aug[i][j] = m[i * 5 + j];
        }
        aug[i][5 + i] = 1.0;
    }

    for col in 0..5 {
        // Partial pivot: find the largest-magnitude entry in this column.
        let mut piv = col;
        let mut maxv = aug[col][col].abs();
        for (r, row) in aug.iter().enumerate().skip(col + 1) {
            let v = row[col].abs();
            if v > maxv {
                maxv = v;
                piv = r;
            }
        }
        if piv != col {
            aug.swap(piv, col);
        }
        let pivot = aug[col][col];
        if !pivot.is_finite() || pivot.abs() < 1e-12 {
            // Singular column; leave this row's inverse half as zeros.
            continue;
        }
        // Normalize the pivot row.
        for cell in aug[col].iter_mut() {
            *cell /= pivot;
        }
        // Eliminate the column from every other row. Copying the pivot row
        // (10 f32) lets us mutate `aug[r]` while reading the pivot row
        // without an aliasing borrow.
        let pivot_row = aug[col];
        for (r, row) in aug.iter_mut().enumerate() {
            if r == col {
                continue;
            }
            let factor = row[col];
            if factor == 0.0 {
                continue;
            }
            for (ar, &ac) in row.iter_mut().zip(pivot_row.iter()) {
                *ar -= factor * ac;
            }
        }
    }

    let mut inv = [0.0f32; 25];
    for i in 0..5 {
        for j in 0..5 {
            inv[i * 5 + j] = aug[i][5 + j];
        }
    }
    inv
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Sample variance of a slice of `f32` samples.
    fn variance(data: &[f32]) -> f32 {
        if data.is_empty() {
            return 0.0;
        }
        let mean = data.iter().sum::<f32>() / data.len() as f32;
        let sum_sq = data
            .iter()
            .map(|v| {
                let d = v - mean;
                d * d
            })
            .sum::<f32>();
        sum_sq / data.len() as f32
    }

    /// Deterministic LCG pseudo-random generator (no external dependencies).
    struct Lcg {
        state: u32,
    }
    impl Lcg {
        fn new(seed: u32) -> Self {
            Self { state: seed }
        }
        /// Uniform float in `[-0.5, 0.5)`.
        fn next_f32(&mut self) -> f32 {
            // Numerical Recipes constants.
            self.state = self.state.wrapping_mul(1664525).wrapping_add(1013904223);
            ((self.state >> 8) as f32) / 16777216.0 - 0.5
        }
    }

    #[test]
    fn kalman_stylus_smooths_noise() {
        let mut kf = KalmanStylus::new();
        let mut rng = Lcg::new(1234567);
        let true_x = 50.0f32;
        let n = 400usize;
        let mut inputs = vec![0.0f32; n];
        let mut outputs = vec![0.0f32; n];
        for i in 0..n {
            let noise = rng.next_f32() * 6.0;
            let m = StylusState {
                x: true_x + noise,
                y: 0.0,
                pressure: 0.5,
                tilt: 0.0,
                twist: 0.0,
                azimuth: 0.0,
            };
            inputs[i] = m.x;
            outputs[i] = kf.update(m, 0.016).x;
        }
        // Discard the warm-up transient before comparing variances.
        let warm = 80;
        let in_var = variance(&inputs[warm..]);
        let out_var = variance(&outputs[warm..]);
        assert!(
            out_var < in_var,
            "filtered variance {out_var:.4} should be < input variance {in_var:.4}"
        );
        // Smoothing should be substantial, not merely marginal.
        assert!(out_var < in_var * 0.25);
    }

    #[test]
    fn kalman_stylus_pressure_smoothing() {
        let mut kf = KalmanStylus::new();
        let mut rng = Lcg::new(7654321);
        let true_p = 0.5f32;
        let n = 400usize;
        let mut inputs = vec![0.0f32; n];
        let mut outputs = vec![0.0f32; n];
        for i in 0..n {
            let noise = rng.next_f32() * 0.4;
            let m = StylusState {
                x: 0.0,
                y: 0.0,
                pressure: true_p + noise,
                tilt: 0.0,
                twist: 0.0,
                azimuth: 0.0,
            };
            inputs[i] = m.pressure;
            outputs[i] = kf.update(m, 0.016).pressure;
        }
        let warm = 80;
        let in_var = variance(&inputs[warm..]);
        let out_var = variance(&outputs[warm..]);
        assert!(
            out_var < in_var,
            "pressure not smoothed: out {out_var:.6} >= in {in_var:.6}"
        );
    }

    #[test]
    fn kalman_stylus_state_snap_matches_update() {
        let mut kf = KalmanStylus::new();
        let m = StylusState {
            x: 3.0,
            y: 4.0,
            pressure: 0.5,
            tilt: 0.1,
            twist: 0.0,
            azimuth: 0.2,
        };
        let out = kf.update(m, 0.016);
        let snap = kf.state();
        assert_eq!(out.x, snap.x);
        assert_eq!(out.pressure, snap.pressure);
        assert_eq!(out.tilt, snap.tilt);
    }

    #[test]
    fn kalman_stylus_reset_clears_state() {
        let mut kf = KalmanStylus::new();
        let m = StylusState {
            x: 42.0,
            y: 17.0,
            pressure: 0.8,
            tilt: 0.3,
            twist: 0.1,
            azimuth: 1.0,
        };
        kf.update(m, 0.016);
        assert_ne!(kf.state().x, 0.0);
        kf.reset();
        let snap = kf.state();
        assert_eq!(snap.x, 0.0);
        assert_eq!(snap.y, 0.0);
        assert_eq!(snap.pressure, 0.0);
        assert!(!kf.is_settled());
    }

    #[test]
    fn kalman_stylus_settles_after_stream() {
        let mut kf = KalmanStylus::new();
        assert!(!kf.is_settled());
        for i in 0..400 {
            let m = StylusState {
                x: 10.0 + i as f32 * 0.1,
                y: 5.0,
                pressure: 0.5,
                tilt: 0.05,
                twist: 0.0,
                azimuth: 0.1,
            };
            kf.update(m, 0.016);
        }
        assert!(kf.is_settled(), "filter should converge after 400 events");
    }

    #[test]
    fn kalman_stylus_handles_non_finite_dt() {
        let mut kf = KalmanStylus::new();
        let m = StylusState {
            x: 1.0,
            y: 1.0,
            pressure: 0.5,
            tilt: 0.0,
            twist: 0.0,
            azimuth: 0.0,
        };
        // Should not panic or produce NaN.
        let out = kf.update(m, f32::NAN);
        assert!(out.x.is_finite());
        let out = kf.update(m, -1.0);
        assert!(out.x.is_finite());
    }

    /// Per-event latency gate.
    ///
    /// Processes 10,000 synthetic stylus events and asserts the average
    /// per-event time is under 2.0 ms. **CI runners are the gate of record**
    /// for absolute latency numbers — local machines vary, so this test is
    /// `#[ignore]` by default to avoid flakiness on shared/loaded hardware.
    /// Run it explicitly with `cargo test -p martensite-window --lib
    /// kalman_stylus_latency -- --ignored`.
    #[test]
    #[ignore]
    fn kalman_stylus_latency() {
        use std::time::Instant;

        let mut kf = KalmanStylus::new();
        let n: usize = 10_000;
        let mut total = std::time::Duration::ZERO;
        for i in 0..n {
            let m = StylusState {
                x: i as f32 * 0.001,
                y: ((i as f32) * 0.05).sin() * 5.0,
                pressure: 0.5 + ((i as f32) * 0.01).sin() * 0.2,
                tilt: 0.1 + ((i as f32) * 0.003).cos() * 0.05,
                twist: (i as f32) * 0.0001,
                azimuth: (i as f32) * 0.0002,
            };
            let t0 = Instant::now();
            let out = kf.update(m, 0.016);
            total += t0.elapsed();
            // Prevent the optimizer from eliding the work.
            std::hint::black_box(out);
        }
        let avg_ms = total.as_secs_f64() * 1000.0 / n as f64;
        assert!(
            avg_ms < 2.0,
            "avg per-event latency {avg_ms:.4} ms >= 2.0 ms budget"
        );
    }

    // Sanity-check the private matrix helpers.

    #[test]
    fn mat12_identity_roundtrip() {
        let id = mat12_identity();
        let v = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0,
        ];
        let out = mat12_vec_mul(&id, &v);
        assert_eq!(out, v);
        let id2 = mat12_mul(&id, &id);
        assert_eq!(id2, id);
    }

    #[test]
    fn mat12_transpose_double_is_identity() {
        let mut m = [0.0f32; 144];
        for (i, v) in m.iter_mut().enumerate() {
            *v = i as f32 * 0.1;
        }
        let tt = mat12_transpose(&mat12_transpose(&m));
        for i in 0..144 {
            assert!((tt[i] - m[i]).abs() < 1e-5);
        }
    }

    #[test]
    fn mat12_add_and_vec12_add() {
        let a = mat12_identity();
        let b = mat12_identity();
        let c = mat12_add(&a, &b);
        assert_eq!(c[0], 2.0);
        assert_eq!(c[1], 0.0);
        let v1 = [1.0; 12];
        let v2 = [2.0; 12];
        assert_eq!(vec12_add(&v1, &v2), [3.0; 12]);
    }

    #[test]
    fn mat5_inverse_roundtrip() {
        // A small invertible 5x5 matrix.
        let mut m = [0.0f32; 25];
        for i in 0..5 {
            m[i * 5 + i] = 4.0;
            if i + 1 < 5 {
                m[i * 5 + i + 1] = 1.0;
                m[(i + 1) * 5 + i] = 1.0;
            }
        }
        let inv = mat5_inverse(&m);
        // m * inv should be the identity.
        let mut prod = [0.0f32; 25];
        for i in 0..5 {
            for j in 0..5 {
                let mut s = 0.0;
                for k in 0..5 {
                    s += m[i * 5 + k] * inv[k * 5 + j];
                }
                prod[i * 5 + j] = s;
            }
        }
        for i in 0..5 {
            assert!((prod[i * 5 + i] - 1.0).abs() < 1e-3, "diag {i} not 1.0");
            for j in 0..5 {
                if i != j {
                    assert!(prod[i * 5 + j].abs() < 1e-3, "off-diag ({i},{j}) not 0");
                }
            }
        }
    }
}
