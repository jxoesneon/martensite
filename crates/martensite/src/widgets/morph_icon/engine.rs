//! Private geometry engine for [`MorphIcon`](super::MorphIcon) — a
//! port of morphicons v1.7.1's frozen core (`src/core/*`, MIT License,
//! Copyright (c) 2026 Guillermo) to `kurbo::BezPath` geometry.
//!
//! Pipeline: `d` string/`BezPath` → cubic subpaths ([`to_cubics`]) →
//! arc-length resampled point sets with corner anchoring
//! ([`resample`]) → a correspondence+alignment plan ([`build_plan`])
//! → polar interpolation ([`MorphPlan::eval`]). Plan construction is
//! the expensive leg and runs once per `(from, to)` pair; per-frame
//! evaluation is ~64 fused multiply-adds per subpath.
//!
//! Divergences from upstream v1.7.1 (pinned): arcs are lowered by
//! kurbo's `BezPath::from_svg` rather than the ≤90° center-parametric
//! slicing of `src/core/normalize.ts`; the spring integrator is
//! `martensite-motion`'s closed-form solver (widget side), not the
//! upstream semi-implicit Euler loop; in-flight frames are emitted as
//! `BezPath` polylines instead of serialized `d` strings.
//!
//! All fallible operations return [`MorphError`] — nothing in this
//! module may panic on untrusted path data.

use kurbo::{BezPath, PathEl};

/// Samples per subpath — upstream's fixed N.
pub(crate) const SAMPLE_N: usize = 64;

/// Input caps (ADR-0041 safety contract): a `d` string, a lowered
/// cubic subpath count, or a total segment count beyond these fails
/// `Err` — never a panic, never unbounded work.
pub(crate) const MAX_D_BYTES: usize = 16 * 1024;
/// Maximum number of subpaths accepted per icon.
pub(crate) const MAX_SUBPATHS: usize = 24;
/// Maximum cubic segments across one icon's subpaths.
pub(crate) const MAX_SEGMENTS: usize = 512;

/// A subpath lowered to cubic Béziers, packed as
/// `[p0, c1, c2, p1, c1', c2', p2, …]` — `2·(3m+1)` floats for `m`
/// segments. Consecutive segments share endpoints.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CubicSubpath {
    /// Packed x/y pairs.
    pub pts: Vec<f64>,
    /// Whether the subpath ends in `ClosePath`.
    pub closed: bool,
}

impl CubicSubpath {
    /// Number of cubic segments.
    pub(crate) fn seg_count(&self) -> usize {
        seg_count_of(self.pts.len())
    }
}

/// Packed-length → segment count (`2·(3m+1)` → `m`).
fn seg_count_of(len: usize) -> usize {
    (len / 2).saturating_sub(1) / 3
}

/// A subpath sampled at [`SAMPLE_N`] points by arc length — the
/// currency between [`resample`] and [`build_plan`]. An interrupted
/// mid-flight shape is also a `Vec<Sampled>` (`MorphPlan::eval`
/// output feeds `build_plan` directly for clean re-entry).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Sampled {
    /// `2·N` packed x/y pairs.
    pub pts: Vec<f64>,
    /// Whether the sampled subpath is a closed loop.
    pub closed: bool,
}

/// Why icon input was rejected or a plan could not be built. Every
/// variant is reachable only through validated, non-panicking paths.
#[derive(Debug, Clone, PartialEq)]
pub enum MorphError {
    /// The `d` string failed to parse (`kurbo` error preserved).
    Parse(String),
    /// Input exceeded a cap (`MAX_D_BYTES`/`MAX_SUBPATHS`/`MAX_SEGMENTS`).
    TooLarge(&'static str),
    /// The geometry is degenerate: non-finite coordinates, or a
    /// subpath whose corner-anchored runs exceed the sample budget.
    Degenerate(&'static str),
    /// The icon produced no drawable subpaths.
    Empty,
}

impl core::fmt::Display for MorphError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "icon path parse failed: {e}"),
            Self::TooLarge(what) => write!(f, "icon path too large: {what}"),
            Self::Degenerate(what) => write!(f, "degenerate icon geometry: {what}"),
            Self::Empty => write!(f, "icon path produced no subpaths"),
        }
    }
}

impl std::error::Error for MorphError {}

// ---------------------------------------------------------------------------
// Lowering: BezPath → cubic subpaths (upstream `normalize.ts` builder).
// ---------------------------------------------------------------------------

/// Cubic accumulator for one subpath — the `builder()` tuple of
/// upstream `normalize.ts`. `pts[0..2]` is the start point; the
/// current point is always the last pair.
struct Builder {
    pts: Vec<f64>,
}

impl Builder {
    fn new(x: f64, y: f64) -> Self {
        Self { pts: vec![x, y] }
    }

    /// Current point (the last packed pair).
    fn cur(&self) -> (f64, f64) {
        let n = self.pts.len();
        (self.pts[n - 2], self.pts[n - 1])
    }

    fn seg_count(&self) -> usize {
        seg_count_of(self.pts.len())
    }

    /// `CurveTo` verbatim.
    fn cubic(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64) {
        self.pts.extend_from_slice(&[x1, y1, x2, y2, x, y]);
    }

    /// `LineTo` → degenerate cubic with collinear controls at ⅓ and
    /// ⅔; degenerate endpoints (`|Δ| < 1e-12` per axis) are skipped,
    /// matching upstream `line`.
    fn line(&mut self, x: f64, y: f64) {
        let (cx, cy) = self.cur();
        if (x - cx).abs() < 1e-12 && (y - cy).abs() < 1e-12 {
            return;
        }
        self.cubic(
            cx + (x - cx) / 3.0,
            cy + (y - cy) / 3.0,
            cx + (2.0 * (x - cx)) / 3.0,
            cy + (2.0 * (y - cy)) / 3.0,
            x,
            y,
        );
    }

    /// `QuadTo` → exact degree elevation: `c1 = p0 + ⅔(q−p0)`,
    /// `c2 = p1 + ⅔(q−p1)`.
    fn quad(&mut self, x1: f64, y1: f64, x: f64, y: f64) {
        let (cx, cy) = self.cur();
        self.cubic(
            cx + (2.0 / 3.0) * (x1 - cx),
            cy + (2.0 / 3.0) * (y1 - cy),
            x + (2.0 / 3.0) * (x1 - x),
            y + (2.0 / 3.0) * (y1 - y),
            x,
            y,
        );
    }

    /// Emits the subpath; `closed` first appends the explicit closing
    /// segment back to the start point (skipped when degenerate).
    /// `None` when nothing but the start point was pushed.
    fn finish(mut self, closed: bool) -> Option<CubicSubpath> {
        if closed {
            let (sx, sy) = (self.pts[0], self.pts[1]);
            self.line(sx, sy);
        }
        if self.pts.len() < 8 {
            return None;
        }
        Some(CubicSubpath {
            pts: self.pts,
            closed,
        })
    }
}

/// Rejects non-finite coordinates (NaN/±∞ can arrive via `1e999`
/// scientific-notation overflow or a hand-built `BezPath`).
fn finite(p: kurbo::Point) -> Result<(), MorphError> {
    if p.x.is_finite() && p.y.is_finite() {
        Ok(())
    } else {
        Err(MorphError::Degenerate("non-finite coordinate"))
    }
}

/// Finishes the in-progress subpath into `subs`, enforcing the
/// subpath cap; returns its segment count.
fn push_subpath(
    subs: &mut Vec<CubicSubpath>,
    builder: Builder,
    closed: bool,
) -> Result<usize, MorphError> {
    let Some(sub) = builder.finish(closed) else {
        return Ok(0);
    };
    if subs.len() >= MAX_SUBPATHS {
        return Err(MorphError::TooLarge("subpaths"));
    }
    let segs = sub.seg_count();
    subs.push(sub);
    Ok(segs)
}

/// Parses/lowers an SVG `d` string into cubic subpaths (enforces
/// `MAX_D_BYTES`; rejects non-finite coordinates).
pub(crate) fn d_to_cubics(d: &str) -> Result<Vec<CubicSubpath>, MorphError> {
    if d.len() > MAX_D_BYTES {
        return Err(MorphError::TooLarge("d"));
    }
    let path = BezPath::from_svg(d).map_err(|e| MorphError::Parse(e.to_string()))?;
    bezpath_to_cubics(&path)
}

/// Lowers a `BezPath` into cubic subpaths: `MoveTo` splits,
/// `LineTo`→degenerate cubic, `QuadTo`→degree elevation,
/// `CurveTo` verbatim, `ClosePath` marks closed.
pub(crate) fn bezpath_to_cubics(path: &BezPath) -> Result<Vec<CubicSubpath>, MorphError> {
    let mut subs: Vec<CubicSubpath> = Vec::new();
    let mut segs = 0usize; // segments in finished subpaths
    let mut cur: Option<Builder> = None;
    // Start of the current/most-recent subpath: after a `ClosePath` a
    // drawing command reopens a new subpath at this point — the SVG
    // rule upstream `parse.ts` encodes ("after Z, a drawing command
    // opens a new one at (sx, sy)"). kurbo's parser makes the same
    // move explicit for `d` input.
    let mut start = (0.0, 0.0);
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                finite(p)?;
                if let Some(b) = cur.take() {
                    segs += push_subpath(&mut subs, b, false)?;
                }
                cur = Some(Builder::new(p.x, p.y));
                start = (p.x, p.y);
            }
            PathEl::LineTo(p) => {
                finite(p)?;
                let b = cur.get_or_insert_with(|| Builder::new(start.0, start.1));
                b.line(p.x, p.y);
            }
            PathEl::QuadTo(q, p) => {
                finite(q)?;
                finite(p)?;
                let b = cur.get_or_insert_with(|| Builder::new(start.0, start.1));
                b.quad(q.x, q.y, p.x, p.y);
            }
            PathEl::CurveTo(c1, c2, p) => {
                finite(c1)?;
                finite(c2)?;
                finite(p)?;
                let b = cur.get_or_insert_with(|| Builder::new(start.0, start.1));
                b.cubic(c1.x, c1.y, c2.x, c2.y, p.x, p.y);
            }
            PathEl::ClosePath => {
                if let Some(b) = cur.take() {
                    segs += push_subpath(&mut subs, b, true)?;
                }
            }
        }
        if segs + cur.as_ref().map_or(0, Builder::seg_count) > MAX_SEGMENTS {
            return Err(MorphError::TooLarge("segments"));
        }
    }
    if let Some(b) = cur.take() {
        segs += push_subpath(&mut subs, b, false)?;
        if segs > MAX_SEGMENTS {
            return Err(MorphError::TooLarge("segments"));
        }
    }
    Ok(subs)
}

// ---------------------------------------------------------------------------
// Resample: arc-length sampling with anchored corners (upstream
// `resample.ts`). Cubic length has no closed form: |B′(t)| is
// integrated with 8-point Gauss-Legendre and inverted by safeguarded
// Newton.
// ---------------------------------------------------------------------------

/// Angular threshold for a segment joint to count as a corner (22.5°).
const CORNER_THRESHOLD: f64 = core::f64::consts::PI / 8.0;

// Gauss-Legendre, 8 points on [−1, 1] — symmetric nodes: only half
// is stored (upstream constants, verbatim).
const GX: [f64; 4] = [
    0.18343464249564978,
    0.525532409916329,
    0.7966664774136267,
    0.9602898564975363,
];
const GW: [f64; 4] = [
    0.362683783378362,
    0.31370664587788727,
    0.22238103445337448,
    0.10122853629037626,
];

/// |B′(t)| of segment `k`:
/// `B′(t) = 3(1−t)²(P₁−P₀) + 6(1−t)t(P₂−P₁) + 3t²(P₃−P₂)`.
fn speed(p: &[f64], k: usize, t: f64) -> f64 {
    let i = 6 * k;
    let u = 1.0 - t;
    let c0 = 3.0 * u * u;
    let c1 = 6.0 * u * t;
    let c2 = 3.0 * t * t;
    let dx = c0 * (p[i + 2] - p[i]) + c1 * (p[i + 4] - p[i + 2]) + c2 * (p[i + 6] - p[i + 4]);
    let dy = c0 * (p[i + 3] - p[i + 1]) + c1 * (p[i + 5] - p[i + 3]) + c2 * (p[i + 7] - p[i + 5]);
    dx.hypot(dy)
}

/// ∫₀^t1 |B′| of segment `k` via Gauss-Legendre.
fn seg_len(p: &[f64], k: usize, t1: f64) -> f64 {
    let half = t1 / 2.0;
    let mut s = 0.0;
    for j in 0..4 {
        s += GW[j] * (speed(p, k, half + half * GX[j]) + speed(p, k, half - half * GX[j]));
    }
    s * half
}

/// Bernstein evaluation of segment `k` at `t`.
fn eval_cubic(p: &[f64], k: usize, t: f64) -> (f64, f64) {
    let i = 6 * k;
    let u = 1.0 - t;
    let b0 = u * u * u;
    let b1 = 3.0 * u * u * t;
    let b2 = 3.0 * u * t * t;
    let b3 = t * t * t;
    (
        b0 * p[i] + b1 * p[i + 2] + b2 * p[i + 4] + b3 * p[i + 6],
        b0 * p[i + 1] + b1 * p[i + 3] + b2 * p[i + 5] + b3 * p[i + 7],
    )
}

/// Tangent at an endpoint of segment `k`: `at_end` → outgoing at P₃
/// (P₃−P₂), otherwise incoming at P₀ (P₁−P₀); falls back to the next
/// control point when degenerate.
fn tangent(p: &[f64], k: usize, at_end: bool) -> Option<(f64, f64)> {
    let i = 6 * k;
    let b = if at_end { i + 6 } else { i };
    let s = if at_end { -1.0 } else { 1.0 };
    let js = if at_end { [4usize, 2, 0] } else { [2, 4, 6] };
    for j in js {
        let dx = s * (p[i + j] - p[b]);
        let dy = s * (p[i + j + 1] - p[b + 1]);
        if dx * dx + dy * dy > 1e-18 {
            return Some((dx, dy));
        }
    }
    None
}

/// Segment boundaries (index of the segment starting at the corner)
/// whose tangent discontinuity exceeds the threshold. For closed
/// paths this includes the closing joint (boundary = first active
/// segment).
fn detect_corners(path: &CubicSubpath) -> Vec<usize> {
    let p = &path.pts;
    let m = path.seg_count();
    let mut active = Vec::with_capacity(m);
    for k in 0..m {
        if seg_len(p, k, 1.0) > 1e-9 {
            active.push(k);
        }
    }
    let mut corners: Vec<usize> = Vec::new();
    let mut test = |a: usize, b: usize| {
        if let (Some(u), Some(v)) = (tangent(p, a, true), tangent(p, b, false)) {
            let ang = (u.0 * v.1 - u.1 * v.0).atan2(u.0 * v.0 + u.1 * v.1).abs();
            if ang > CORNER_THRESHOLD {
                corners.push(b);
            }
        }
    };
    for j in 0..active.len().saturating_sub(1) {
        test(active[j], active[j + 1]);
    }
    if path.closed && active.len() > 1 {
        test(active[active.len() - 1], active[0]);
    }
    corners.sort_unstable();
    corners.dedup();
    corners
}

/// Arc-length inversion: `t` such that ∫₀^t |B′| = `s`. Safeguarded
/// Newton with a bisection bracket; |B′| is the exact derivative of
/// the objective.
fn invert(p: &[f64], k: usize, s: f64, ls: f64) -> f64 {
    if s <= 0.0 {
        return 0.0;
    }
    if s >= ls {
        return 1.0;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    let mut t = s / ls;
    for _ in 0..12 {
        let f = seg_len(p, k, t) - s;
        if f.abs() < 1e-10 * ls + 1e-14 {
            break;
        }
        if f > 0.0 {
            hi = t;
        } else {
            lo = t;
        }
        let sp = speed(p, k, t);
        let mut nt = if sp > 1e-12 {
            t - f / sp
        } else {
            (lo + hi) / 2.0
        };
        if !(nt > lo && nt < hi) {
            nt = (lo + hi) / 2.0;
        }
        t = nt;
    }
    t
}

/// Samples a cubic subpath at [`SAMPLE_N`] points equidistant by arc
/// length, anchoring corners and endpoints as exact samples. Closed
/// paths distribute N intervals around the loop without duplicating
/// the first point; the circular start-point freedom is resolved by
/// the plan's circular correspondence.
fn resample_path(path: &CubicSubpath) -> Result<Vec<f64>, MorphError> {
    let p = &path.pts;
    let m = path.seg_count();
    let n = SAMPLE_N;
    // Upstream `fill()`: degenerate input collapses to N copies of p0.
    let fill = || {
        let mut out = Vec::with_capacity(2 * n);
        for _ in 0..n {
            out.push(p[0]);
            out.push(p[1]);
        }
        out
    };
    if m < 1 {
        return Ok(fill());
    }
    let mut lens = vec![0.0; m];
    let mut total_l = 0.0;
    for (k, l) in lens.iter_mut().enumerate() {
        *l = seg_len(p, k, 1.0);
        total_l += *l;
    }
    if total_l < 1e-12 {
        return Ok(fill());
    }

    // Anchors are segment boundaries. For open paths: endpoints +
    // corners. For closed paths: ONLY corners — sampling must be
    // intrinsic to the shape, not to the arbitrary M point, so two
    // congruent loops with different start points produce the same
    // sample set modulo index rotation. With no corners (a circle)
    // the path start is the only possible reference.
    let cs = detect_corners(path);
    let anchors: Vec<usize> = if path.closed {
        if cs.is_empty() {
            vec![0]
        } else {
            cs
        }
    } else {
        let mut a = Vec::with_capacity(cs.len() + 2);
        a.push(0);
        a.extend_from_slice(&cs);
        a.push(m);
        a.sort_unstable();
        a.dedup();
        a
    };
    // Runs between anchors; for closed paths the last wraps to
    // anchors[0] + m.
    let mut runs: Vec<(usize, usize)> = Vec::with_capacity(anchors.len());
    if path.closed {
        for j in 0..anchors.len() {
            let a = anchors[j];
            let b = if j + 1 < anchors.len() {
                anchors[j + 1]
            } else {
                anchors[0] + m
            };
            runs.push((a, b));
        }
    } else {
        for j in 0..anchors.len() - 1 {
            runs.push((anchors[j], anchors[j + 1]));
        }
    }
    let rl: Vec<f64> = runs
        .iter()
        .map(|&(a, b)| (a..b).map(|k| lens[k % m]).sum())
        .collect();
    let intervals = if path.closed { n } else { n - 1 };
    if runs.len() > intervals {
        return Err(MorphError::Degenerate("sample budget"));
    }

    // Largest-remainder apportionment: proportional to length, min 1,
    // exact sum.
    let total = {
        let t: f64 = rl.iter().sum();
        if t > 0.0 {
            t
        } else {
            1.0 // upstream `|| 1`
        }
    };
    let ideal: Vec<f64> = rl.iter().map(|l| intervals as f64 * l / total).collect();
    let mut counts: Vec<usize> = ideal.iter().map(|q| (q.floor() as usize).max(1)).collect();
    let mut rem = intervals as i64 - counts.iter().map(|&c| c as i64).sum::<i64>();
    if rem > 0 {
        // Quantized fraction: quadrature fp noise (~1e-15) must not
        // decide the tie-break — runs congruent under rotation must
        // apportion identically in both icons.
        let mut order: Vec<(i64, usize)> = ideal
            .iter()
            .enumerate()
            .map(|(idx, q)| (((q - q.floor()) * 1e9).round() as i64, idx))
            .collect();
        order.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let n_runs = counts.len();
        for j in 0..rem as usize {
            counts[order[j % n_runs].1] += 1;
        }
    }
    while rem < 0 {
        let mut bi = 0;
        for (idx, &c) in counts.iter().enumerate().skip(1) {
            if c > counts[bi] {
                bi = idx;
            }
        }
        if counts[bi] <= 1 {
            break;
        }
        counts[bi] -= 1;
        rem += 1;
    }

    // Sampling: exact anchor at the start of each run + interiors by
    // arc-length inversion.
    let mut out = Vec::with_capacity(2 * n);
    for (r, &(k0, k1)) in runs.iter().enumerate() {
        let cnt = counts[r];
        let lr = rl[r];
        let vi = 6 * (k0 % m);
        out.push(p[vi]);
        out.push(p[vi + 1]);
        let mut seg = k0;
        let mut acc = 0.0;
        for j in 1..cnt {
            let target = lr * j as f64 / cnt as f64;
            while seg < k1 - 1 && acc + lens[seg % m] < target {
                acc += lens[seg % m];
                seg += 1;
            }
            let k = seg % m;
            let ls = lens[k];
            let t = if ls > 1e-12 {
                invert(p, k, target - acc, ls)
            } else {
                0.0
            };
            let (x, y) = eval_cubic(p, k, t);
            out.push(x);
            out.push(y);
        }
    }
    if !path.closed {
        let vi = 6 * m;
        out.push(p[vi]);
        out.push(p[vi + 1]);
    }
    debug_assert_eq!(out.len(), 2 * n);
    Ok(out)
}

/// Arc-length resamples each subpath to [`SAMPLE_N`] points with
/// corner anchoring and largest-remainder apportionment.
pub(crate) fn resample(cubics: &[CubicSubpath]) -> Result<Vec<Sampled>, MorphError> {
    cubics
        .iter()
        .map(|c| {
            Ok(Sampled {
                pts: resample_path(c)?,
                closed: c.closed,
            })
        })
        .collect()
}

/// `d` string → sampled subpaths (the widget's `icon` entry point).
pub(crate) fn d_to_sampled(d: &str) -> Result<Vec<Sampled>, MorphError> {
    let subs = resample(&d_to_cubics(d)?)?;
    if subs.is_empty() {
        return Err(MorphError::Empty);
    }
    Ok(subs)
}

/// `BezPath` → sampled subpaths.
pub(crate) fn bezpath_to_sampled(path: &BezPath) -> Result<Vec<Sampled>, MorphError> {
    let subs = resample(&bezpath_to_cubics(path)?)?;
    if subs.is_empty() {
        return Err(MorphError::Empty);
    }
    Ok(subs)
}

// ---------------------------------------------------------------------------
// Plan: correspondence + alignment (upstream `plan.ts`). Closed-form
// 2D Procrustes (atan2, no SVD), centroid+length cost matrix,
// surjective "cell division" when counts differ, circular
// correspondence for closed loops, and the global-hybrid block
// transport for congruent icons.
// ---------------------------------------------------------------------------

/// Weight of |ΔL| in the subpath pairing cost.
const LEN_WEIGHT: f64 = 0.35;

/// λ of the minimal-rotation tie-break: `score = res + λ·|θ|/π`.
/// It exists because shapes symmetric under inversion (lines) tie in
/// residual for both traversal orientations yet produce different
/// rotations.
const LAMBDA: f64 = 0.05;

/// Global residual below which the whole icon counts as congruent and
/// the plan shares (θ, σ) across all items (hybrid Procrustes).
const GLOBAL_EPS: f64 = 5e-3;

/// Bounds for exhaustive matching; above them the solvers fall back
/// to greedy with repair. 8! = 40 320 permutations / 1e5 assignments.
const PERM_MAX: usize = 8;
const SURJ_MAX: f64 = 1e5;

/// `(x, y)` point pair used throughout the plan stage.
type Pt = (f64, f64);

/// A similarity transform candidate: rotation `theta`, uniform scale
/// `sigma`, normalized RMS residual `res` (0 → same shape).
#[derive(Debug, Clone, Copy)]
struct Similarity {
    theta: f64,
    sigma: f64,
    res: f64,
}

fn centroid(p: &[f64]) -> Pt {
    let n = p.len() / 2;
    let mut cx = 0.0;
    let mut cy = 0.0;
    for i in 0..n {
        cx += p[2 * i];
        cy += p[2 * i + 1];
    }
    (cx / n as f64, cy / n as f64)
}

/// Polyline length of a sampled point set.
fn poly_len(p: &[f64]) -> f64 {
    let n = p.len() / 2;
    let mut l = 0.0;
    for i in 1..n {
        l += (p[2 * i] - p[2 * i - 2]).hypot(p[2 * i + 1] - p[2 * i - 1]);
    }
    l
}

/// Reversed traversal order of a point cloud.
fn reverse_pts(p: &[f64]) -> Vec<f64> {
    let n = p.len() / 2;
    let mut out = Vec::with_capacity(2 * n);
    for i in (0..n).rev() {
        out.push(p[2 * i]);
        out.push(p[2 * i + 1]);
    }
    out
}

/// Circular re-indexing of a loop: `out[i] = p[(i+off) mod n]`. Same
/// point set, different cut — the circular degree of freedom of
/// closed paths.
fn rotate_pts(p: &[f64], off: usize) -> Vec<f64> {
    let n = p.len() / 2;
    let mut out = Vec::with_capacity(2 * n);
    for i in 0..n {
        let j = (i + off) % n;
        out.push(p[2 * j]);
        out.push(p[2 * j + 1]);
    }
    out
}

/// Optimal similarity (θ, σ) minimizing Σ|σ·R(θ)·(a−c_A) − (b−c_B)|².
/// `θ* = atan2(S_xy − S_yx, S_xx + S_yy)`; σ* by zero derivative;
/// `res` is the RMS residual normalized by b's energy.
fn procrustes(a: &[f64], b: &[f64], ca: Pt, cb: Pt) -> Similarity {
    let n = a.len() / 2;
    let (mut sxx, mut sxy, mut syx, mut syy, mut na, mut nb) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for i in 0..n {
        let ax = a[2 * i] - ca.0;
        let ay = a[2 * i + 1] - ca.1;
        let bx = b[2 * i] - cb.0;
        let by = b[2 * i + 1] - cb.1;
        sxx += ax * bx;
        syy += ay * by;
        sxy += ax * by;
        syx += ay * bx;
        na += ax * ax + ay * ay;
        nb += bx * bx + by * by;
    }
    let theta = (sxy - syx).atan2(sxx + syy);
    let num = theta.cos() * (sxx + syy) + theta.sin() * (sxy - syx);
    let mut sigma = if na > 1e-12 { num / na } else { 1.0 };
    // `!(σ > 1e-6)` — also catches NaN, like upstream's `!(sigma > …)`.
    if sigma.partial_cmp(&1e-6) != Some(core::cmp::Ordering::Greater) {
        sigma = 1e-6;
    }
    let res2 = (sigma * sigma * na - 2.0 * sigma * num + nb).max(0.0);
    let res = if nb > 1e-12 { (res2 / nb).sqrt() } else { 0.0 };
    Similarity { theta, sigma, res }
}

/// The chosen index-to-index correspondence plus its similarity.
struct Alignment {
    sim: Similarity,
    ca: Pt,
    cb: Pt,
    /// A with the chosen correspondence (re-indexed only if A is the
    /// closed loop).
    a: Vec<f64>,
    /// B with the chosen correspondence (orientation + circular
    /// offset).
    b: Vec<f64>,
}

/// Best index-to-index correspondence between `a` and `b`: tries both
/// traversal directions and, if there is a closed loop, its N
/// circular offsets, scoring `res + λ·|θ|/π`. The freedom is applied
/// to ONE cloud — the closed one (b if both are); varying both at
/// once would be redundant.
fn align_pair(a_pts: &[f64], b_pts: &[f64], a_closed: bool, b_closed: bool) -> Alignment {
    let ca = centroid(a_pts);
    let cb = centroid(b_pts);
    let vary_a = a_closed && !b_closed;
    let base: &[f64] = if vary_a { a_pts } else { b_pts };
    let offs = if a_closed || b_closed {
        base.len() / 2
    } else {
        1
    };
    let mut best_score = f64::INFINITY;
    let mut best_dir = 0usize;
    let mut best_off = 0usize;
    let mut sim = Similarity {
        theta: 0.0,
        sigma: 1.0,
        res: 0.0,
    };
    for dir in 0..2usize {
        let walk = if dir == 1 {
            reverse_pts(base)
        } else {
            base.to_vec()
        };
        for off in 0..offs {
            let cand = if off > 0 {
                rotate_pts(&walk, off)
            } else {
                walk.clone()
            };
            let s = if vary_a {
                procrustes(&cand, b_pts, ca, cb)
            } else {
                procrustes(a_pts, &cand, ca, cb)
            };
            let score = s.res + LAMBDA * s.theta.abs() / core::f64::consts::PI;
            if score < best_score {
                best_score = score;
                best_dir = dir;
                best_off = off;
                sim = s;
            }
        }
    }
    let walk = if best_dir == 1 {
        reverse_pts(base)
    } else {
        base.to_vec()
    };
    let best = if best_off > 0 {
        rotate_pts(&walk, best_off)
    } else {
        walk
    };
    if vary_a {
        Alignment {
            sim,
            ca,
            cb,
            a: best,
            b: b_pts.to_vec(),
        }
    } else {
        Alignment {
            sim,
            ca,
            cb,
            a: a_pts.to_vec(),
            b: best,
        }
    }
}

/// Cost matrix `dist(centroids) + LEN_WEIGHT·|ΔL|` between all pairs.
fn cost_matrix(a: &[&[f64]], b: &[&[f64]]) -> Vec<Vec<f64>> {
    let cbs: Vec<Pt> = b.iter().map(|p| centroid(p)).collect();
    let lbs: Vec<f64> = b.iter().map(|p| poly_len(p)).collect();
    a.iter()
        .map(|pa| {
            let ca = centroid(pa);
            let la = poly_len(pa);
            cbs.iter()
                .zip(&lbs)
                .map(|(cb, lb)| (ca.0 - cb.0).hypot(ca.1 - cb.1) + LEN_WEIGHT * (la - lb).abs())
                .collect()
        })
        .collect()
}

/// `p == q`: minimum-cost permutation. Exhaustive with pruning up to
/// [`PERM_MAX`]; greedy (pairs sorted by cost) above it.
fn best_permutation(c: &[Vec<f64>]) -> Vec<usize> {
    let n = c.len();
    if n > PERM_MAX {
        let mut pairs: Vec<(f64, usize, usize)> = Vec::with_capacity(n * n);
        for (i, row) in c.iter().enumerate() {
            for (j, &cost) in row.iter().enumerate() {
                pairs.push((cost, i, j));
            }
        }
        // Stable sort: ties keep (i, j) insertion order like
        // upstream's `Array.prototype.sort`.
        pairs.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(core::cmp::Ordering::Equal));
        let mut out = vec![usize::MAX; n];
        let mut used = vec![false; n];
        for (_, i, j) in pairs {
            if out[i] == usize::MAX && !used[j] {
                out[i] = j;
                used[j] = true;
            }
        }
        return out;
    }
    let mut arr: Vec<usize> = (0..n).collect();
    let mut best = arr.clone();
    let mut bc = f64::INFINITY;
    perm_rec(c, &mut arr, 0, 0.0, &mut bc, &mut best);
    best
}

fn perm_rec(
    c: &[Vec<f64>],
    arr: &mut [usize],
    k: usize,
    acc: f64,
    bc: &mut f64,
    best: &mut Vec<usize>,
) {
    if acc >= *bc {
        return;
    }
    if k == arr.len() {
        *bc = acc;
        *best = arr.to_vec();
        return;
    }
    for i in k..arr.len() {
        arr.swap(k, i);
        perm_rec(c, arr, k + 1, acc + c[k][arr[k]], bc, best);
        arr.swap(k, i);
    }
}

/// `p ≠ q`: surjective assignment from the large side to the small
/// one at minimum cost — enumeration with pruning when `S^B` is
/// small, greedy + coverage repair otherwise. Surjectivity guarantees
/// no subpath appears or vanishes out of nowhere.
fn best_surjection(c: &[Vec<f64>]) -> Result<Vec<usize>, MorphError> {
    let big_b = c.len(); // rows: the larger side
    let s = c.first().map_or(0, Vec::len); // cols: the smaller side
    if s == 0 {
        return Err(MorphError::Degenerate("surjection"));
    }
    if (s as f64).powi(big_b as i32) > SURJ_MAX {
        // Greedy: each row takes its cheapest target.
        let mut f = vec![0usize; big_b];
        for (i, row) in c.iter().enumerate() {
            let mut m = 0;
            for j in 1..row.len() {
                if row[j] < row[m] {
                    m = j;
                }
            }
            f[i] = m;
        }
        // Coverage repair: donate rows from multiply-covered targets
        // to uncovered ones at minimum extra cost.
        let mut mult = vec![0usize; s];
        for &sv in &f {
            mult[sv] += 1;
        }
        for sv in 0..s {
            if mult[sv] > 0 {
                continue;
            }
            let mut bi = usize::MAX;
            let mut bc = f64::INFINITY;
            for i in 0..big_b {
                if mult[f[i]] < 2 {
                    continue; // only donors with multiplicity
                }
                let extra = c[i][sv] - c[i][f[i]];
                if extra < bc {
                    bc = extra;
                    bi = i;
                }
            }
            if bi == usize::MAX {
                break; // unreachable when B ≥ S; stays panic-free
            }
            mult[f[bi]] -= 1;
            f[bi] = sv;
            mult[sv] += 1;
        }
        return Ok(f);
    }
    let mut f = vec![0usize; big_b];
    let mut mult = vec![0usize; s];
    let mut best: Option<Vec<usize>> = None;
    let mut bc = f64::INFINITY;
    surj_rec(c, &mut f, &mut mult, 0, 0.0, 0, &mut bc, &mut best);
    best.ok_or(MorphError::Degenerate("surjection"))
}

#[allow(clippy::too_many_arguments)]
fn surj_rec(
    c: &[Vec<f64>],
    f: &mut [usize],
    mult: &mut [usize],
    i: usize,
    acc: f64,
    covered: usize,
    bc: &mut f64,
    best: &mut Option<Vec<usize>>,
) {
    let big_b = c.len();
    let s = mult.len();
    if acc >= *bc || s - covered > big_b - i {
        return;
    }
    if i == big_b {
        *bc = acc;
        *best = Some(f.to_vec());
        return;
    }
    for sv in 0..s {
        f[i] = sv;
        mult[sv] += 1;
        surj_rec(
            c,
            f,
            mult,
            i + 1,
            acc + c[i][sv],
            covered + usize::from(mult[sv] == 1),
            bc,
            best,
        );
        mult[sv] -= 1;
    }
}

/// Global hybrid: Procrustes over the concatenated clouds with the
/// already-chosen correspondence. If the global residual ≈ 0 the whole
/// icon is congruent and every item shares (θ, σ): coherent block
/// rotation (keeps a symmetric subpath from picking the opposite
/// spin).
fn apply_global(items: &mut [PlanItem], n: usize) {
    let t = items.len() * n;
    let mut ga = Vec::with_capacity(2 * t);
    let mut gb = Vec::with_capacity(2 * t);
    for it in items.iter() {
        ga.extend_from_slice(&it.a);
        gb.extend_from_slice(&it.b_o);
    }
    let gca = centroid(&ga);
    let g = procrustes(&ga, &gb, gca, centroid(&gb));
    if g.res >= GLOBAL_EPS {
        return;
    }
    let cos = (-g.theta).cos();
    let sin = (-g.theta).sin();
    let rc = g.theta.cos();
    let rs = g.theta.sin();
    for it in items.iter_mut() {
        let mut e2 = 0.0;
        let mut nb = 0.0;
        for i in 0..n {
            let bx = it.b_o[2 * i] - it.cb.0;
            let by = it.b_o[2 * i + 1] - it.cb.1;
            it.b_t[2 * i] = (bx * cos - by * sin) / g.sigma;
            it.b_t[2 * i + 1] = (bx * sin + by * cos) / g.sigma;
            let ex = g.sigma * (rc * it.a_c[2 * i] - rs * it.a_c[2 * i + 1]) - bx;
            let ey = g.sigma * (rs * it.a_c[2 * i] + rc * it.a_c[2 * i + 1]) - by;
            e2 += ex * ex + ey * ey;
            nb += bx * bx + by * by;
        }
        it.theta = g.theta;
        it.ln_sigma = g.sigma.ln();
        it.res = if nb > 1e-12 { (e2 / nb).sqrt() } else { 0.0 };
        // Block transport: every part spins with the shared θ, but
        // lerping the centroids would send off-center parts along the
        // chord — inside the arc — and the block would deform
        // mid-flight. The centroid rides the shared similarity around
        // the global centroid instead; drift absorbs the (tiny)
        // global residual so t = 1 stays exact:
        //   c(t) = ca + t·drift + (σᵗR(tθ) − I)·off,  c(1) = cb
        //   ⇒ drift = cb − ca − (σR(θ) − I)·off.
        let s1 = it.ln_sigma.exp();
        let c1 = it.theta.cos() * s1;
        let n1 = it.theta.sin() * s1;
        let ox = it.ca.0 - gca.0;
        let oy = it.ca.1 - gca.1;
        let rx = ox * c1 - oy * n1 - ox;
        let ry = ox * n1 + oy * c1 - oy;
        it.block = Some(Block {
            off: (ox, oy),
            drift: (it.cb.0 - it.ca.0 - rx, it.cb.1 - it.ca.1 - ry),
        });
    }
}

/// Builds the morph plan: cost-matrix matching (exhaustive ≤ 8,
/// greedy above), surjective "cell division" when subpath counts
/// differ, per-pair Procrustes alignment with traversal-direction and
/// circular-offset search, and the global-hybrid block transport when
/// the whole icon is congruent.
pub(crate) fn build_plan(a: &[Sampled], b: &[Sampled]) -> Result<MorphPlan, MorphError> {
    let p = a.len();
    let q = b.len();
    if p == 0 || q == 0 {
        return Err(MorphError::Empty);
    }
    // Upstream assumes a uniform point count (always `SAMPLE_N` here —
    // `Sampled` is only built by `resample`/`eval`). Validated anyway:
    // `procrustes` indexes `b` by `a`'s count and would panic on a
    // mismatched hand-rolled set.
    let n = a[0].pts.len() / 2;
    if n == 0 || !a[0].pts.len().is_multiple_of(2) {
        return Err(MorphError::Degenerate("sample count"));
    }
    if a.iter().chain(b.iter()).any(|s| s.pts.len() != 2 * n) {
        return Err(MorphError::Degenerate("sample count"));
    }
    let a_pts: Vec<&[f64]> = a.iter().map(|s| s.pts.as_slice()).collect();
    let b_pts: Vec<&[f64]> = b.iter().map(|s| s.pts.as_slice()).collect();

    let mut pairs: Vec<(usize, usize)> = Vec::with_capacity(p.max(q));
    if p == q {
        let perm = best_permutation(&cost_matrix(&a_pts, &b_pts));
        for (i, &j) in perm.iter().enumerate() {
            pairs.push((i, j));
        }
    } else if p < q {
        let f = best_surjection(&cost_matrix(&b_pts, &a_pts))?;
        for (j, &si) in f.iter().enumerate() {
            pairs.push((si, j));
        }
    } else {
        let f = best_surjection(&cost_matrix(&a_pts, &b_pts))?;
        for (i, &di) in f.iter().enumerate() {
            pairs.push((i, di));
        }
    }

    let mut items = Vec::with_capacity(pairs.len());
    for (si, di) in pairs {
        let al = align_pair(a_pts[si], b_pts[di], a[si].closed, b[di].closed);
        let mut a_c = vec![0.0; 2 * n];
        let mut b_t = vec![0.0; 2 * n];
        let cos = (-al.sim.theta).cos();
        let sin = (-al.sim.theta).sin();
        for i in 0..n {
            a_c[2 * i] = al.a[2 * i] - al.ca.0;
            a_c[2 * i + 1] = al.a[2 * i + 1] - al.ca.1;
            let bx = al.b[2 * i] - al.cb.0;
            let by = al.b[2 * i + 1] - al.cb.1;
            b_t[2 * i] = (bx * cos - by * sin) / al.sim.sigma;
            b_t[2 * i + 1] = (bx * sin + by * cos) / al.sim.sigma;
        }
        items.push(PlanItem {
            a: al.a,
            a_c,
            b_t,
            b_o: al.b,
            ca: al.ca,
            cb: al.cb,
            theta: al.sim.theta,
            ln_sigma: al.sim.sigma.ln(),
            res: al.sim.res,
            closed: a[si].closed && b[di].closed,
            block: None,
        });
    }
    if items.len() > 1 {
        apply_global(&mut items, n);
    }
    Ok(MorphPlan { items, n })
}

/// Block-transport payload set by the global hybrid: mid-flight the
/// centroid rides the shared similarity around the global centroid
/// instead of lerping — `off = c_A − g_A`, `drift` closes `c(1) = c_B`
/// exactly.
#[derive(Debug, Clone, Copy)]
struct Block {
    off: Pt,
    drift: Pt,
}

/// One matched subpath pair with its similarity decomposition —
/// upstream `PlanItem`.
#[derive(Debug, Clone)]
struct PlanItem {
    /// Points of A with the chosen correspondence (re-indexed if A is
    /// the closed loop); consumed by `apply_global`.
    a: Vec<f64>,
    /// A centered on its centroid.
    a_c: Vec<f64>,
    /// B brought into A's frame: `R(−θ)·(b − c_B)/σ`.
    b_t: Vec<f64>,
    /// B oriented, raw (for the global hybrid and exact `t = 1`).
    b_o: Vec<f64>,
    ca: Pt,
    cb: Pt,
    theta: f64,
    ln_sigma: f64,
    /// Alignment residual (normalized RMS) — diagnostic for parity
    /// tests; the interpolant never reads it.
    #[allow(dead_code)]
    res: f64,
    /// `true` if both endpoints are closed loops: the subpath flies
    /// with Z. Closed → open flies open: the loop opens at the chosen
    /// cut.
    closed: bool,
    /// Block transport from the global hybrid (`None` → lerp
    /// centroids).
    block: Option<Block>,
}

/// A precomputed morph between two sampled icons.
#[derive(Debug, Clone)]
pub(crate) struct MorphPlan {
    items: Vec<PlanItem>,
    /// Sample count per subpath.
    n: usize,
}

impl MorphPlan {
    /// Evaluates the plan at `t` into per-subpath point sets.
    /// Extrapolates for `t < 0` / `t > 1` — spring overshoot must not
    /// be clamped (upstream `interpPolar` semantics).
    ///
    /// Polar interpolation: `P(t) = c(t) + σᵗ·R(tθ)·[(1−t)·aC + t·bT]`
    /// — exact at `t = 0` and `t = 1`.
    pub(crate) fn eval(&self, t: f64) -> Vec<Sampled> {
        let n = self.n;
        self.items
            .iter()
            .map(|it| {
                let s = (it.ln_sigma * t).exp();
                let ang = it.theta * t;
                let cos = ang.cos() * s;
                let sin = ang.sin() * s;
                let (cx, cy) = match it.block {
                    Some(b) => {
                        let (ox, oy) = b.off;
                        let (dx, dy) = b.drift;
                        (
                            it.ca.0 + dx * t + (ox * cos - oy * sin - ox),
                            it.ca.1 + dy * t + (ox * sin + oy * cos - oy),
                        )
                    }
                    None => (
                        it.ca.0 + (it.cb.0 - it.ca.0) * t,
                        it.ca.1 + (it.cb.1 - it.ca.1) * t,
                    ),
                };
                let mut pts = Vec::with_capacity(2 * n);
                for i in 0..n {
                    let px = it.a_c[2 * i] + (it.b_t[2 * i] - it.a_c[2 * i]) * t;
                    let py = it.a_c[2 * i + 1] + (it.b_t[2 * i + 1] - it.a_c[2 * i + 1]) * t;
                    pts.push(cx + px * cos - py * sin);
                    pts.push(cy + px * sin + py * cos);
                }
                Sampled {
                    pts,
                    closed: it.closed,
                }
            })
            .collect()
    }

    /// Evaluates at `t` and emits polyline `BezPath`s ready for
    /// `PaintList::push_stroke_path` (closed subpaths get `ClosePath`).
    pub(crate) fn eval_paths(&self, t: f64) -> Vec<BezPath> {
        sampled_to_bezpaths(&self.eval(t))
    }
}

/// Sampled point sets → polyline `BezPath`s (the per-frame emission).
pub(crate) fn sampled_to_bezpaths(subs: &[Sampled]) -> Vec<BezPath> {
    subs.iter()
        .filter(|s| s.pts.len() >= 4)
        .map(|s| {
            let mut p = BezPath::new();
            p.move_to((s.pts[0], s.pts[1]));
            for i in (2..s.pts.len()).step_by(2) {
                p.line_to((s.pts[i], s.pts[i + 1]));
            }
            if s.closed {
                p.close_path();
            }
            p
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests — ADR-0041 engine matrix: lowering, corner anchoring,
// Procrustes/direction/offset selection, surjective cell division,
// global-hybrid block transport, caps, malformed input, extrapolation.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn points(s: &Sampled) -> Vec<Pt> {
        s.pts
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| (p[0], p[1]))
            .collect()
    }

    fn has_point(s: &Sampled, x: f64, y: f64) -> bool {
        s.pts
            .as_chunks::<2>()
            .0
            .iter()
            .any(|p| p[0] == x && p[1] == y)
    }

    /// Rigidly rotates every sample of every subpath about `c`.
    fn rotated(subs: &[Sampled], c: Pt, ang: f64) -> Vec<Sampled> {
        let (cos, sin) = (ang.cos(), ang.sin());
        subs.iter()
            .map(|s| Sampled {
                pts: s
                    .pts
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .flat_map(|p| {
                        let x = p[0] - c.0;
                        let y = p[1] - c.1;
                        [c.0 + x * cos - y * sin, c.1 + x * sin + y * cos]
                    })
                    .collect(),
                closed: s.closed,
            })
            .collect()
    }

    #[test]
    fn lowering_line_quad_cubic_arc_close() {
        // Line → degenerate cubic, controls at ⅓/⅔.
        let subs = d_to_cubics("M0 0 L9 3").unwrap();
        assert_eq!(subs.len(), 1);
        assert!(!subs[0].closed);
        assert_eq!(subs[0].seg_count(), 1);
        assert_eq!(subs[0].pts, vec![0.0, 0.0, 3.0, 1.0, 6.0, 2.0, 9.0, 3.0]);

        // Degenerate line (|Δ| < 1e-12) skipped.
        let subs = d_to_cubics("M0 0 L0 0 L5 0").unwrap();
        assert_eq!(subs[0].seg_count(), 1);
        assert_eq!(subs[0].pts[6..], [5.0, 0.0]);

        // Quad → degree elevation: c1 = p0 + ⅔(q−p0), c2 = p1 + ⅔(q−p1).
        let subs = d_to_cubics("M0 0 Q3 6 6 0").unwrap();
        assert_eq!(subs[0].pts, vec![0.0, 0.0, 2.0, 4.0, 4.0, 4.0, 6.0, 0.0]);

        // Cubic verbatim.
        let subs = d_to_cubics("M1 2 C3 4 5 6 7 8").unwrap();
        assert_eq!(subs[0].pts, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);

        // Arc → cubics via kurbo slicing; endpoint lands on target.
        let subs = d_to_cubics("M0 0 A5 5 0 0 1 5 5").unwrap();
        assert!(subs[0].seg_count() >= 1);
        let p = &subs[0].pts;
        let n = p.len();
        assert!((p[n - 2] - 5.0).abs() < 1e-6 && (p[n - 1] - 5.0).abs() < 1e-6);

        // ClosePath: marked closed + explicit closing segment to start.
        let subs = d_to_cubics("M0 0 L10 0 L10 10 Z").unwrap();
        assert!(subs[0].closed);
        assert_eq!(subs[0].seg_count(), 3);
        let p = &subs[0].pts;
        assert_eq!((p[p.len() - 2], p[p.len() - 1]), (p[0], p[1]));

        // Already-closed endpoint: Z adds no degenerate segment.
        let subs = d_to_cubics("M0 0 L10 0 L0 0 Z").unwrap();
        assert!(subs[0].closed);
        assert_eq!(subs[0].seg_count(), 2);

        // Multi-subpath split on M; a lone M drops out (no segments).
        let subs = d_to_cubics("M0 0 L1 0 M5 5 M9 9 L8 8").unwrap();
        assert_eq!(subs.len(), 2);
        assert!(d_to_cubics("M3 4").unwrap().is_empty());
    }

    #[test]
    fn caps_and_malformed_input() {
        // Oversized d.
        let big = format!("M0 0{}", " L1 1".repeat(MAX_D_BYTES / 5 + 1));
        assert!(matches!(d_to_cubics(&big), Err(MorphError::TooLarge("d"))));

        // Subpath cap.
        let d = "M0 0L1 1".repeat(MAX_SUBPATHS + 1);
        assert!(matches!(
            d_to_cubics(&d),
            Err(MorphError::TooLarge("subpaths"))
        ));

        // Segment cap (single subpath, > MAX_SEGMENTS segments).
        let mut d = String::from("M0 0");
        for i in 0..=MAX_SEGMENTS {
            d.push_str(&format!(" L{} {}", i % 97 + 1, i % 89 + 1));
        }
        assert!(matches!(
            d_to_cubics(&d),
            Err(MorphError::TooLarge("segments"))
        ));

        // Malformed d → Parse, never a panic.
        assert!(d_to_cubics("").unwrap().is_empty()); // empty parses to no elements
        assert!(matches!(d_to_sampled(""), Err(MorphError::Empty)));
        assert!(matches!(d_to_cubics("M0 0 Q"), Err(MorphError::Parse(_))));
        assert!(matches!(
            d_to_cubics("M0 0 X9 9"),
            Err(MorphError::Parse(_))
        ));
        assert!(matches!(d_to_cubics("L1 1"), Err(MorphError::Parse(_))));

        // Non-finite coordinates → Degenerate (hand-built path and
        // `1e999` scientific-notation overflow through kurbo's lexer).
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((f64::NAN, 1.0));
        assert!(matches!(
            bezpath_to_cubics(&p),
            Err(MorphError::Degenerate(_))
        ));
        assert!(matches!(
            d_to_sampled("M0 0 L1e999 0"),
            Err(MorphError::Degenerate(_))
        ));

        // A lone moveto → no drawable subpaths → Empty at the entry fn.
        assert!(matches!(d_to_sampled("M3 4"), Err(MorphError::Empty)));
    }

    #[test]
    fn resample_anchors_corners_and_endpoints() {
        // Closed square: all four corners are exact sample points.
        let subs = d_to_sampled("M0 0 L10 0 L10 10 L0 10 Z").unwrap();
        assert_eq!(subs.len(), 1);
        let s = &subs[0];
        assert!(s.closed);
        assert_eq!(s.pts.len(), 2 * SAMPLE_N);
        for c in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)] {
            assert!(has_point(s, c.0, c.1), "corner {c:?} not anchored");
        }

        // Open polyline: both endpoints exact, interior corner anchored.
        let subs = d_to_sampled("M1 2 L11 2 L11 8").unwrap();
        let s = &subs[0];
        assert!(!s.closed);
        assert_eq!(&s.pts[..2], &[1.0, 2.0]);
        assert_eq!(&s.pts[2 * SAMPLE_N - 2..], &[11.0, 8.0]);
        assert!(has_point(s, 11.0, 2.0), "interior corner not anchored");
    }

    #[test]
    fn resample_is_uniform_by_arc_length() {
        // Straight line of length 63 → sample i lands at x = i.
        let subs = d_to_sampled("M0 0 L63 0").unwrap();
        for (i, p) in subs[0].pts.as_chunks::<2>().0.iter().enumerate() {
            assert!(
                (p[0] - i as f64).abs() < 1e-6 && p[1].abs() < 1e-12,
                "sample {i} = {p:?} off the uniform grid"
            );
        }
    }

    #[test]
    fn resample_sample_budget_error() {
        // A sawtooth with far more than SAMPLE_N anchored corners
        // (every tooth is a >22.5° tangent discontinuity) cannot be
        // apportioned → Degenerate("sample budget"), not a panic.
        let subs = d_to_cubics(&(0..80).fold(String::from("M0 0"), |mut d, i| {
            d.push_str(&format!(" L{} {}", i + 1, (i % 2) * 5));
            d
        }))
        .unwrap();
        assert!(matches!(
            resample(&subs),
            Err(MorphError::Degenerate("sample budget"))
        ));
    }

    #[test]
    fn procrustes_recovers_known_rotation() {
        let a = d_to_sampled("M0 0 L10 0 L10 10 L0 10 Z").unwrap();
        let ang = core::f64::consts::PI / 6.0; // 30°
        let b = rotated(&a, (5.0, 5.0), ang);
        let plan = build_plan(&a, &b).unwrap();
        assert_eq!(plan.items.len(), 1);
        let it = &plan.items[0];
        assert!(it.res < 1e-4, "residual {}", it.res);
        // Sign is direction-convention dependent; magnitude is the
        // minimal rotation (a square's 90° symmetry yields |θ| = 30°).
        assert!(
            (it.theta.abs() - ang).abs() < 1e-6,
            "theta {} vs {}",
            it.theta,
            ang
        );
    }

    #[test]
    fn surjection_keeps_all_subpaths() {
        // Hamburger (3 lines) → X (2 legs): every source subpath is
        // carried (duplicated dst coverage) — 3 outputs, not 2.
        let a = d_to_sampled("M4 7h16M4 12h16M4 17h16").unwrap();
        assert_eq!(a.len(), 3);
        let b = d_to_sampled("M6 6l12 12M18 6l-12 12").unwrap();
        assert_eq!(b.len(), 2);
        let plan = build_plan(&a, &b).unwrap();
        assert_eq!(plan.items.len(), 3);
        assert_eq!(plan.eval(0.5).len(), 3);
        assert_eq!(plan.eval_paths(1.0).len(), 3);

        // Reverse direction: 2 → 3 duplicates a source line.
        let plan = build_plan(&b, &a).unwrap();
        assert_eq!(plan.items.len(), 3);
    }

    #[test]
    fn circular_offset_invariance() {
        // Same loop, different M cut point: the second cloud is the
        // first circularly shifted by exactly N/4 samples.
        let circle = |phase: f64| Sampled {
            pts: (0..SAMPLE_N)
                .flat_map(|i| {
                    let t = 2.0 * core::f64::consts::PI * i as f64 / SAMPLE_N as f64 + phase;
                    [12.0 + 8.0 * t.cos(), 12.0 + 8.0 * t.sin()]
                })
                .collect(),
            closed: true,
        };
        let a = vec![circle(0.0)];
        let b = vec![circle(core::f64::consts::FRAC_PI_2)];
        let plan = build_plan(&a, &b).unwrap();
        assert!(plan.items[0].res < 1e-9, "residual {}", plan.items[0].res);
        assert!(plan.items[0].theta.abs() < 1e-9);
        // Mid-flight stays on the same loop.
        let mid = plan.eval(0.5);
        let mp = points(&mid[0]);
        for (x, y) in &mp {
            let r = (x - 12.0).hypot(y - 12.0);
            assert!((r - 8.0).abs() < 1e-6, "mid-flight radius {r}");
        }
    }

    #[test]
    fn global_hybrid_engages_block_transport() {
        // Two unequal lines (length term disambiguates the pairing),
        // rigidly rotated 60° about the icon center — congruent icon.
        let a = d_to_sampled("M4 12h16 M18 6l-6 6").unwrap();
        assert_eq!(a.len(), 2);
        let b = rotated(&a, (12.0, 12.0), core::f64::consts::FRAC_PI_3);
        let plan = build_plan(&a, &b).unwrap();
        assert!(
            plan.items.iter().all(|it| it.block.is_some()),
            "congruent icon must share the global similarity"
        );
        // Block transport = rigid mid-flight: the inter-subpath
        // centroid distance is preserved at t = 0.5 (a lerp would pull
        // it inside the arc).
        let mid = plan.eval(0.5);
        let c0 = centroid(&mid[0].pts);
        let c1 = centroid(&mid[1].pts);
        let d_mid = (c0.0 - c1.0).hypot(c0.1 - c1.1);
        let a0 = centroid(&a[0].pts);
        let a1 = centroid(&a[1].pts);
        let d_rest = (a0.0 - a1.0).hypot(a0.1 - a1.1);
        assert!(
            (d_mid - d_rest).abs() < 1e-6,
            "mid-flight centroid distance {d_mid} vs rest {d_rest}"
        );
    }

    #[test]
    fn eval_endpoints_and_overshoot() {
        let a = d_to_sampled("M4 12h16").unwrap();
        let b = d_to_sampled("M6 6l12 12").unwrap();
        let plan = build_plan(&a, &b).unwrap();
        let it = &plan.items[0];

        // Exact endpoints: t=0 yields a (in the aligned indexing),
        // t=1 yields the oriented b.
        let e0 = &plan.eval(0.0)[0];
        for (got, want) in e0.pts.iter().zip(&it.a) {
            assert!((got - want).abs() < 1e-9);
        }
        let e1 = &plan.eval(1.0)[0];
        for (got, want) in e1.pts.iter().zip(&it.b_o) {
            assert!((got - want).abs() < 1e-9);
        }

        // Overshoot extrapolates — never clamped.
        assert_ne!(plan.eval(1.0)[0].pts, plan.eval(1.1)[0].pts);
        assert_ne!(plan.eval(0.0)[0].pts, plan.eval(-0.1)[0].pts);

        // Closed → open flies open; both-closed keeps Z.
        let closed_a = d_to_sampled("M2 2h20v20h-20Z").unwrap();
        let open_b = d_to_sampled("M2 12h20").unwrap();
        let plan = build_plan(&closed_a, &open_b).unwrap();
        assert!(!plan.eval(0.5)[0].closed);
        let plan = build_plan(&closed_a, &closed_a).unwrap();
        assert!(plan.eval(0.5)[0].closed);
    }

    #[test]
    fn plan_rejects_empty_and_mismatched() {
        let a = d_to_sampled("M4 12h16").unwrap();
        assert!(matches!(build_plan(&[], &a), Err(MorphError::Empty)));
        assert!(matches!(build_plan(&a, &[]), Err(MorphError::Empty)));
        let short = Sampled {
            pts: vec![0.0, 0.0, 1.0, 1.0],
            closed: false,
        };
        assert!(matches!(
            build_plan(&a, &[short]),
            Err(MorphError::Degenerate("sample count"))
        ));
    }
}
