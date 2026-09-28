# [ADR-0041] Native Icon Morphing — Facade-Private Engine, `MorphIcon` Widget

* **Status:** Accepted
* **Date:** 2026-09-27
* **Deciders:** Martensite Architecture Working Group
* **Technical Domain:** `martensite` (widget library — private geometry engine + `MorphIcon`), `martensite-motion` (`SpringSolver` reuse), `martensite-core` (`PaintContext` reduced-motion seam, paint-only tick), `kurbo` (`BezPath::from_svg`), `martensite-design-lint` (a11y lint interplay)
* **Amends:** None. Companion to ADR-0040 (shares the reduced-motion and paint-only-dirty machinery the loading work introduced).

---

## Context and Problem Statement

Martensite has no vector-icon infrastructure at all. Roughly thirty facade widgets take `icon: String` emoji/char glyphs painted through the text painter (`nav_rail`, `settings_row`, `dock`, `app_grid`, `empty_state`, `alert_dialog`, `command_palette`, …), and several hand-draw state-paired glyphs — `volume.rs` draws a speaker and a muted ✕ that are literally a morph pair. Nothing can animate between them: `martensite-motion` animates `f32`/`Vec2` only, and there is no path-correspondence machinery anywhere in the workspace.

The upstream reference is `guillermolg00/morphicons` v1.7.1 — **MIT license**, ~7 kB gzip zero-dependency TypeScript core — which morphs *arbitrary* stroke-icon pairs. Its value is not per-pair pre-baked animation but a general pipeline:

1. `parse` — full SVG `d` grammar (relatives, `H/V/S/T` shorthands, packed arc flags, scientific notation).
2. `normalize` — everything lowered to cubic-Bézier subpaths (lines → degenerate cubics, quads → degree elevation, arcs → SVG F.6 center-parametrization in ≤90° slices; `circle/ellipse/rect/polyline/polygon` element lowering).
3. `resample` — arc-length resampling to N=64 points per subpath via 8-point Gauss-Legendre quadrature + safeguarded Newton inversion; **corner anchoring** (tangent discontinuities > 22.5° become exact samples); largest-remainder apportionment; closed loops sampled intrinsically (corner-anchored only) so congruent loops produce congruent sample sets modulo rotation.
4. `plan` — the correspondence solver: centroid-distance + `0.35·|ΔL|` cost matrix; exhaustive permutation ≤ 8 subpaths / greedy above; **surjective matching** when subpath counts differ (extra strokes duplicate — "cell division"); per-pair closed-form Procrustes (θ via `atan2`, σ closed-form, no SVD); both traversal directions + all N circular offsets on closed loops with a `λ·|θ|/π` minimal-rotation tie-break; **global hybrid** — when the whole icon is congruent (residual < 5e-3) every subpath shares one (θ, σ), yielding coherent rigid rotation mid-flight rather than per-part spinning.
5. `interpolate` — polar interpolation `P(t) = c(t) + σᵗ·R(tθ)·[(1−t)·aC + t·bT]`; exact at t=0/1; **extrapolates** under spring overshoot — essential for spring character.
6. `spring` — semi-implicit Euler @ 240 Hz substeps, interruptible with velocity carry.
7. `serialize` + DOM controller — polyline `d` emission per frame, lazy driver, controlled `from/to/progress` seek, reduced-motion modes.

Pointwise coordinate lerp (the naive alternative) produces visibly degenerate morphs — shape-mush with no rotation intent. The resample/correspondence/Procrustes machinery is precisely what makes arbitrary pairs look intentional; a cheap port ships the failure mode.

## Decision Drivers

* **Reuse over port:** `kurbo 0.13.1` (already a workspace dep, ungated) provides `BezPath::from_svg` — full `d` parsing *and* arc→cubic lowering — replacing ~450 of upstream's ~1200 LOC. `martensite_motion::SpringSolver` is a closed-form analytical damped-spring — strictly better than upstream's Euler integrator (frame-rate independent, C¹-continuous interruption, deterministic `sample_at`). `PaintList::push_stroke_path(BezPath, …)` is the established stroke idiom on both render backends. Serialization is unnecessary — paths go straight to the paint list.
* **v1.0 freeze proximity:** the release spec admits no new API between `v1.0.0-rc.1` and v1.0.0 and rates freezing unsoaked API High/High. A new published crate would freeze ~15 public items with zero dogfooding. A facade-private engine freezes only `MorphIcon`'s builder surface — small and hard to misdesign — while internals iterate freely through the RC soak.
* **Promotion asymmetry:** facade-private → leaf crate is a cheap additive move (publish `martensite-morph`, widget delegates); the reverse is impossible — published crates cannot be unpublished.
* **Facade weight:** the facade is already the ~4000-doctest CI critical path; private items add no doc surface (doc-example requirements apply to public API only), and ~900 LOC is within widget-file norms (`table.rs` ≈ 2900).
* **Consumer reality:** no godot/engine-bridge/headless consumer requests path morphs today; the "second consumer" argument for a leaf crate is speculative (YAGNI) — matching the ADR-0040 extraction convention: engines are promoted when shared, not born shared.
* **Icon data:** upstream's own frozen-core/adapter doctrine — core speaks `d` strings + element lists, foreign formats enter via adapters. Zero icon data is vendored today; input-only keeps binary weight and NOTICE obligations at zero.

## Decision

### 1. Placement — facade-private engine + public widget

One module, `crates/martensite/src/widgets/morph_icon.rs` (~900 LOC total), containing:

- **Private engine items** (not pub): `to_cubics` (PathEl→cubic-subpath lowering — `from_svg` output is MoveTo/LineTo/QuadTo/CurveTo/ClosePath; lines→degenerate cubics, quads→degree elevation, MoveTo splits subpaths, ClosePath marks closed), `resample` (Gauss-Legendre + Newton + corner anchoring + apportionment), `plan` (cost matrix, permutation/surjection, Procrustes, circular offsets, global hybrid), `interp_polar`/`interp_linear`, `MorphPlan` cache keyed by `(from_hash, to_hash)`.
- **Public widget:** `MorphIcon` — `icon(d)`/`morph_to(d, spring)`/`set(d)`/`seek(t)` controlled mode; `spring: SpringConfig` (reuses `martensite-motion` — **no second solver is ported**); stroke width/color builder knobs; accessibility label.
- **Demo pairs module:** ~8–12 canonical stroke-icon `d` constants (menu↔back, play↔pause, check↔x, volume on↔off, eye open↔closed, plus↔minus, arrow↔chevron) as doc/test fixtures — not a vendored icon set.

### 2. Icon data — input-only

The widget accepts `&str` (`d` via `BezPath::from_svg`) and `BezPath` directly. No vendored icon set. A `martensite-assets` VFS icon-pack adapter is a deliberately-deferred, non-breaking post-1.0 add. Stroke-only contract: filled-geometry `d` input is documented (and linted) — morphs operate on stroke paths.

### 3. Animation plumbing — two small core additions

The loading work already proved both mechanisms; morphing generalizes them:

- **Paint-only tick:** `Widget::tick` returning true currently routes `mark_dirty` = `DIRTY_PAINT | DIRTY_A11Y` — per-frame a11y re-emission during every morph. `mark_dirty_paint` exists but is loading-scoped; it becomes the generic "visual-only" tick path (morph progress never changes semantics).
- **Reduced-motion delivery:** `PaintContext` carries no `reduced_motion` — the flag is arena-internal. `MorphIcon` gets `set_reduced_motion(bool)` wired from `martensite_window::prefs::apply_platform_preferences` (same seam the loading shimmer uses); when set, `morph_to` snaps to target.

### 4. Semantics

- `accessibility()`: `Role::Image` + label = the icon's **semantic state** ("Pause", "Muted"), updated on state transition — never mid-flight; decorative icons `hidden`. Satisfies the `icon-only-control` lint when nested in controls.
- Loading: `is_loading`/`paint_loading` free via ADR-0040 defaults (block shimmer — correct shape for a glyph).

### 5. Licensing

MIT port: the morphicons copyright notice + license text is carried in the crate files that derive from upstream (per the `martensite-cosmic-text`/`martensite-vello` vendored-notice precedent). The ported algorithm is pinned to **upstream v1.7.1** with a documented-divergence list (kurbo arc slicing vs upstream ≤90° center-param; `SpringSolver` vs Euler; polyline BezPath vs serialized `d`). `deny.toml` already allowlists MIT/ISC/Apache-2.0 — future vendored icon data (lucide ISC / tabler MIT) re-opens per-set attribution at that time, not now.

## Safety conditions (binding)

1. Every fallible upstream `throw` maps to `Result` — zero panic paths from untrusted `d` strings into `paint`. Input caps: `d` byte length, segment count, subpath count.
2. Degenerate-geometry NaN guards (zero-length subpaths, coincident points, non-finite coords) — same sanitization idiom as `SpringSolver::new`; malformed input is `Err`, never NaN reaching paint.
3. Morph plans cached per `(from,to)` — data-bound icon changes must not force per-frame resampling.
4. Interpolant extrapolates under `t > 1` spring overshoot — no clamp to `[0,1]`.
5. `seek(t)` deterministic for golden-frame tests; arc-heavy icons regression-tested (kurbo arc slicing diverges from upstream); mid-morph frames tested only via fixed-t seeks, never wall clock.
6. Reduced-motion snap and paint-only tick per §3 — both are prerequisites, not options.

## Rejected alternatives

- **`martensite-morph` leaf crate now** — premature publication of an unsoaked API into the 1.0 freeze; one real consumer; extraction stays available post-1.0 (the promotion is a publish + delegate).
- **`martensite-motion::morph` module** — the crate is analytically scoped (spring physics, glam-only); a geometry engine + kurbo dep violates its identity and doubles its weight.
- **Lerp-only widget** — ships the visible failure mode; the correspondence machinery is the capability.
- **Vendored icon set** — first icon dependency, ISC/MIT NOTICE weight, binary cost, for marginal convenience; `d` input + demo pairs cover dogfooding.
- **Lottie/full-SVG animation** — heavyweight runtime for a problem already solved at ~800 LOC.

## Phasing

- **Phase 1 (pre-rc.1):** facade-private engine + `MorphIcon` + demo pairs + reduced-motion/paint-only-tick core seams + upstream-parity test suite.
- **Phase 2 (post-1.0, additive):** promote engine to `martensite-morph` if a second consumer materializes; VFS icon-pack adapter via `martensite-assets`; optional retrofit of `icon: String` slots to accept morph pairs.
- **Out of scope:** filled-icon morphing, full SVG document animation, Lottie compat, icon-set distribution.

## Test matrix

- Engine: resample corner anchoring; Procrustes both-orientation and circular-offset selection; surjective cell-division counts; global-hybrid block transport; malformed `d` → `Err`; caps enforced; overshoot extrapolation.
- Golden frames at fixed `seek(t)` across a fixture corpus incl. arc-heavy icons (kurbo slicing divergence coverage).
- Widget: `tick`→paint-only dirty; reduced-motion snap; a11y label transition semantics; `icon-only-control` lint pass inside a `Button`.
- Dev-channel/MCP: morph state inspectable read-only; no mutation tool needed (icons are app state).
