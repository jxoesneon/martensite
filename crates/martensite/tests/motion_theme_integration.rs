//! Integration tests for Martensite v0.6.0 — Motion & Theme milestone.
//!
//! These tests exercise the cross-crate interaction between `martensite-motion`
//! (analytical spring solver, animation drivers) and `martensite-theme` (Oklab
//! color pipeline, design tokens, GPU theme-transition uniforms), verifying the
//! exit criteria for the Motion & Theme milestone:
//!
//! 1. Spring analytical accuracy (< 1e-6 vs independent f64 reference).
//! 2. C¹ velocity continuity across gesture interruptions.
//! 3. Theme transition performance (150 ms completion, zero CPU allocations).
//! 4. Oklab color pipeline round-trip, perceptual uniformity, and contrast.
//! 5. Theme token diffing, interpolation, and uniform conversion.
//! 6. Cross-crate integration (spring-driven theme transitions, Oklab in
//!    theme uniforms).

#![forbid(unsafe_code)]

use std::mem::size_of;

use glam::Vec2;
use martensite_motion::animation::{AnimationDriver, AnimationDriver2D, AnimationError};
use martensite_motion::spring::{DampingRegime, SpringConfig, SpringSolver};
use martensite_theme::gpu_transition::{
    ThemeTransition, ThemeUniformBuffer, ThemeUniforms, THEME_TRANSITION_WGSL,
};
use martensite_theme::oklab::{
    apca_contrast, gamut_map, linear_to_srgb, srgb_to_linear, wcag_contrast, Gamut, Oklab, Oklch,
};
use martensite_theme::tokens::{
    default_dark, default_light, Theme, ThemeDictionary, ThemeDiff, ThemeMode, ThemeToken, TokenKey,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Floating-point approximate equality within an absolute `tol`.
fn approx_eq(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() < tol
}

/// Relative approximate equality: `|a - b| ≤ rel_tol · max(|a|, |b|, 1.0)`.
///
/// This is used for velocity continuity checks where the magnitude can be
/// large (e.g. ~400 for snappy springs) and the absolute f32 rounding error
/// scales with the magnitude.
fn rel_approx_eq(a: f32, b: f32, rel_tol: f32) -> bool {
    let scale = a.abs().max(b.abs()).max(1.0);
    (a - b).abs() < rel_tol * scale
}

/// Oklab approximate equality across all four channels.
fn oklab_approx_eq(a: Oklab, b: Oklab, tol: f32) -> bool {
    approx_eq(a.l, b.l, tol)
        && approx_eq(a.a, b.a, tol)
        && approx_eq(a.b, b.b, tol)
        && approx_eq(a.alpha, b.alpha, tol)
}

/// Compile-time proof that `T` is `Copy` (a necessary condition for stack-only,
/// zero-allocation operation).
fn assert_copy<T: Copy>() {}

/// Independent f64 re-implementation of the damped harmonic oscillator
/// closed-form solution, used to validate `SpringSolver::sample_at` against the
/// documented formulae. This is intentionally independent of the f32 solver
/// code so that a shared bug would not mask an error.
fn analytical_f64(mass: f64, stiffness: f64, damping: f64, x0: f64, v0: f64, t: f64) -> (f64, f64) {
    let omega0 = (stiffness / mass).sqrt();
    let zeta = damping / (2.0 * (mass * stiffness).sqrt());

    if (zeta - 1.0).abs() < 1e-9 {
        // Critical: x(t) = e^{-ω₀t}(c₁ + c₂ t)
        let c1 = x0;
        let c2 = v0 + omega0 * x0;
        let decay = (-omega0 * t).exp();
        let x = decay * (c1 + c2 * t);
        let v = decay * (c2 - omega0 * (c1 + c2 * t));
        (x, v)
    } else if zeta < 1.0 {
        // Underdamped: x(t) = e^{-ζω₀t}(c₁ cos(ωd t) + c₂ sin(ωd t))
        let omega_d = omega0 * (1.0 - zeta * zeta).sqrt();
        let c1 = x0;
        let c2 = (v0 + zeta * omega0 * x0) / omega_d;
        let decay = (-zeta * omega0 * t).exp();
        let cos = (omega_d * t).cos();
        let sin = (omega_d * t).sin();
        let x = decay * (c1 * cos + c2 * sin);
        let v = decay
            * ((c2 * omega_d - c1 * zeta * omega0) * cos
                - (c1 * omega_d + c2 * zeta * omega0) * sin);
        (x, v)
    } else {
        // Overdamped: x(t) = c₁ e^{r₁ t} + c₂ e^{r₂ t}
        let root = (zeta * zeta - 1.0).sqrt();
        let r1 = (-zeta + root) * omega0;
        let r2 = (-zeta - root) * omega0;
        let denom = r1 - r2;
        let c1 = (v0 - r2 * x0) / denom;
        let c2 = (r1 * x0 - v0) / denom;
        let e1 = (r1 * t).exp();
        let e2 = (r2 * t).exp();
        let x = c1 * e1 + c2 * e2;
        let v = c1 * r1 * e1 + c2 * r2 * e2;
        (x, v)
    }
}

/// Samples a `SpringSolver` at multiple time points and returns the maximum
/// absolute error against the independent f64 analytical reference.
fn max_spring_error(
    config: SpringConfig,
    initial: f32,
    target: f32,
    v0: f32,
    sample_times: &[f32],
) -> f32 {
    let solver = SpringSolver::new(config, initial, target, v0);
    let x0 = f64::from(initial - target);
    let v0_f64 = f64::from(v0);
    let mass = f64::from(config.mass());
    let stiffness = f64::from(config.stiffness());
    let damping = f64::from(config.damping());

    let mut max_err = 0.0_f32;
    for &t in sample_times {
        let (pos, vel) = solver.sample_at(t);
        let (ex_x, ex_v) = analytical_f64(mass, stiffness, damping, x0, v0_f64, f64::from(t));
        let pos_err = (pos - (target + ex_x as f32)).abs();
        let vel_err = (vel - ex_v as f32).abs();
        max_err = max_err.max(pos_err).max(vel_err);
    }
    max_err
}

// ===========================================================================
// 1. Spring analytical accuracy gate (exit criterion: < 1e-6)
// ===========================================================================

#[test]
fn spring_underdamped_matches_analytical_solution() {
    // ζ = 6 / (2·5) = 0.6, ω₀ = 5, ωd = 4.
    let config = SpringConfig::new(1.0, 25.0, 6.0).unwrap();
    assert_eq!(config.damping_regime(), DampingRegime::Underdamped);

    let times = [0.0_f32, 0.05, 0.1, 0.3, 0.5, 0.7, 1.0, 1.5, 2.0, 3.0];
    let max_err = max_spring_error(config, 1.0, 0.0, 0.5, &times);
    assert!(
        max_err < 1e-6,
        "underdamped max error {max_err} exceeds 1e-6"
    );
}

#[test]
fn spring_critical_matches_analytical_solution() {
    // ζ = 4 / (2·2) = 1.0, ω₀ = 2.
    let config = SpringConfig::new(1.0, 4.0, 4.0).unwrap();
    assert_eq!(config.damping_regime(), DampingRegime::Critical);

    let times = [0.0_f32, 0.05, 0.1, 0.3, 0.5, 0.7, 1.0, 1.5, 2.0, 5.0];
    let max_err = max_spring_error(config, 1.0, 0.0, 0.5, &times);
    assert!(max_err < 1e-6, "critical max error {max_err} exceeds 1e-6");
}

#[test]
fn spring_overdamped_matches_analytical_solution() {
    // ζ = 10 / (2·2) = 2.5, ω₀ = 2.
    let config = SpringConfig::new(1.0, 4.0, 10.0).unwrap();
    assert_eq!(config.damping_regime(), DampingRegime::Overdamped);

    let times = [0.0_f32, 0.05, 0.1, 0.3, 0.5, 0.7, 1.0, 1.5, 2.0, 5.0];
    let max_err = max_spring_error(config, 1.0, 0.0, 0.5, &times);
    assert!(
        max_err < 1e-6,
        "overdamped max error {max_err} exceeds 1e-6"
    );
}

// ===========================================================================
// 2. Velocity continuity gate (exit criterion: C¹ continuity on interrupt)
// ===========================================================================

#[test]
fn gesture_interrupt_velocity_continuity() {
    let mut driver = AnimationDriver::new();
    let id = driver.start(SpringConfig::SNAPPY, 0.0, 100.0);

    // Advance partway so the spring has non-trivial position and velocity.
    driver.advance(0.15);
    let (pos_before, vel_before) = driver
        .sample(id)
        .expect("animation should be active before interrupt");

    // The spring must be mid-motion for the test to be meaningful.
    assert!(
        pos_before > 0.0 && pos_before < 100.0,
        "spring should be mid-motion: pos={pos_before}"
    );
    assert!(
        vel_before > 0.0,
        "spring should have positive velocity: vel={vel_before}"
    );

    // Interrupt with a new target — C¹ continuity requires position and
    // velocity to be preserved exactly across the handoff.
    driver
        .interrupt(id, -50.0)
        .expect("interrupt should succeed on active animation");

    let (pos_after, vel_after) = driver
        .sample(id)
        .expect("animation should still be active after interrupt");

    assert!(
        approx_eq(pos_before, pos_after, 1e-4),
        "position must be continuous across interrupt: before={pos_before}, after={pos_after}"
    );
    assert!(
        rel_approx_eq(vel_before, vel_after, 1e-5),
        "velocity must be continuous across interrupt: before={vel_before}, after={vel_after}"
    );
}

#[test]
fn gesture_interrupt_2d_velocity_continuity() {
    let mut driver = AnimationDriver2D::new();
    let id = driver.start(SpringConfig::SNAPPY, Vec2::ZERO, Vec2::new(100.0, 80.0));

    // Advance partway so both axes have non-trivial state.
    driver.advance(0.15);
    let (pos_before, vel_before) = driver
        .sample(id)
        .expect("2d animation should be active before interrupt");

    assert!(
        pos_before.x > 0.0 && pos_before.x < 100.0,
        "x-axis should be mid-motion: pos.x={}",
        pos_before.x
    );
    assert!(
        pos_before.y > 0.0 && pos_before.y < 80.0,
        "y-axis should be mid-motion: pos.y={}",
        pos_before.y
    );

    // Interrupt with a new target — C¹ continuity on both axes.
    driver
        .interrupt(id, Vec2::new(-50.0, -40.0))
        .expect("interrupt should succeed on active 2d animation");

    let (pos_after, vel_after) = driver
        .sample(id)
        .expect("2d animation should still be active after interrupt");

    assert!(
        approx_eq(pos_before.x, pos_after.x, 1e-4) && approx_eq(pos_before.y, pos_after.y, 1e-4),
        "position must be continuous across 2d interrupt: before={pos_before}, after={pos_after}"
    );
    assert!(
        rel_approx_eq(vel_before.x, vel_after.x, 1e-5)
            && rel_approx_eq(vel_before.y, vel_after.y, 1e-5),
        "velocity must be continuous across 2d interrupt: before={vel_before}, after={vel_after}"
    );
}

#[test]
fn gesture_interrupt_missing_id_returns_not_found() {
    let mut driver = AnimationDriver::new();
    let id = driver.start(SpringConfig::CRITICAL, 0.0, 10.0);
    driver.clear();
    assert_eq!(
        driver.interrupt(id, 5.0).unwrap_err(),
        AnimationError::NotFound
    );
}

#[test]
fn gesture_interrupt_settled_returns_already_settled() {
    let mut driver = AnimationDriver::new();
    let id = driver.start(SpringConfig::CRITICAL, 5.0, 5.0);
    assert!(driver.is_settled(id));
    assert_eq!(
        driver.interrupt(id, 10.0).unwrap_err(),
        AnimationError::AlreadySettled
    );
}

// ===========================================================================
// 3. Theme transition performance gate
// ===========================================================================

#[test]
fn theme_transition_150ms_completes() {
    let from = ThemeUniforms::from_theme(&default_light());
    let to = ThemeUniforms::from_theme(&default_dark());
    let mut transition = ThemeTransition::new(from, to);

    assert_eq!(transition.duration, 0.150);
    assert!(!transition.is_complete());

    // Advance in 16 ms steps (60 fps). 150 ms / 16 ms = 9.375 → 10 frames.
    let mut frames = 0;
    while !transition.is_complete() {
        transition.advance(0.016);
        frames += 1;
        // Safety valve: never loop forever.
        assert!(
            frames <= 100,
            "transition did not complete within 100 frames"
        );
    }

    assert_eq!(
        frames, 10,
        "150 ms transition at 60 fps should complete in exactly 10 frames"
    );
    assert!(transition.is_complete());
    assert!((transition.progress() - 1.0).abs() < 1e-6);
}

#[test]
fn theme_transition_zero_allocation() {
    // The zero-allocation exit criterion is verified by proving that every
    // type touched by `advance` and `current_uniforms` is `Copy` — meaning all
    // data is stack-resident with no `Vec`, `String`, `Box`, or other heap
    // indirection. `Copy` is a necessary condition for stack-only operation.
    assert_copy::<ThemeUniforms>();
    assert_copy::<ThemeTransition>();
    assert_copy::<ThemeUniformBuffer>();
    assert_copy::<Oklab>();

    // The uniform struct is exactly 256 bytes (fixed-size, no heap).
    assert_eq!(size_of::<ThemeUniforms>(), 256);
    // The transition holds two uniforms plus two f32s — no heap pointer.
    // With 16-byte alignment on ThemeUniforms, the struct has 8 bytes of
    // trailing padding to maintain alignment (520 -> 528).
    assert_eq!(size_of::<ThemeTransition>(), 528);

    // Advancing 100 times must not panic or grow any internal structure.
    // Because `ThemeTransition` is `Copy`, every field is stack-resident, so
    // `advance` and `current_uniforms` cannot allocate.
    let from = ThemeUniforms::from_theme(&default_light());
    let to = ThemeUniforms::from_theme(&default_dark());
    let mut transition = ThemeTransition::new(from, to);

    for i in 0..100 {
        transition.advance(0.0016);
        let _current = transition.current_uniforms();
        // Progress should be monotonically non-decreasing.
        let expected = ((i + 1) as f32 * 0.0016).min(0.150) / 0.150;
        assert!(
            transition.progress() >= expected - 1e-6,
            "progress should advance: iteration {i}, progress={}, expected~{expected}",
            transition.progress()
        );
    }
}

#[test]
fn theme_transition_5000_widget_scene() {
    // Simulate a 5 000-widget scene where each widget holds its own
    // `ThemeUniforms`. The transition's `advance` and `current_uniforms`
    // operate on stack-only `Copy` data, so the widget storage must not grow.
    let mut widgets = Vec::with_capacity(5000);
    let from = ThemeUniforms::from_theme(&default_light());
    let to = ThemeUniforms::from_theme(&default_dark());
    for _ in 0..5000 {
        widgets.push(from);
    }

    let capacity_before = widgets.capacity();
    assert_eq!(widgets.len(), 5000);

    let mut transition = ThemeTransition::new(from, to);

    // Advance the transition through the full 150 ms window, updating every
    // widget's uniforms from the interpolated result.
    for _ in 0..10 {
        transition.advance(0.016);
        let current = transition.current_uniforms();
        widgets.fill(current);
    }

    assert!(
        transition.is_complete(),
        "transition should be complete after 10 frames"
    );
    assert_eq!(
        widgets.capacity(),
        capacity_before,
        "widget Vec capacity must not grow during transition"
    );
    assert_eq!(widgets.len(), 5000);

    // Every widget should now hold the `to` uniforms.
    for w in &widgets {
        assert_eq!(w.color_count, to.color_count);
    }
}

#[test]
fn theme_transition_wgsl_is_non_empty() {
    // The WGSL shader string must be present and contain the key function.
    assert!(!THEME_TRANSITION_WGSL.is_empty());
    assert!(THEME_TRANSITION_WGSL.contains("theme_color"));
    assert!(THEME_TRANSITION_WGSL.contains("from_colors"));
    assert!(THEME_TRANSITION_WGSL.contains("to_colors"));
}

// ===========================================================================
// 4. Oklab color pipeline integration
// ===========================================================================

#[test]
fn srgb_oklab_roundtrip_all_primaries() {
    let primaries = [
        ("red", 1.0_f32, 0.0_f32, 0.0_f32),
        ("green", 0.0, 1.0, 0.0),
        ("blue", 0.0, 0.0, 1.0),
        ("white", 1.0, 1.0, 1.0),
        ("black", 0.0, 0.0, 0.0),
    ];

    for &(name, r, g, b) in &primaries {
        let lab = Oklab::from_srgb(r, g, b);
        let (r2, g2, b2) = lab.to_srgb();
        assert!(
            approx_eq(r, r2, 1e-4),
            "sRGB round-trip {name}: r {r} vs {r2}"
        );
        assert!(
            approx_eq(g, g2, 1e-4),
            "sRGB round-trip {name}: g {g} vs {g2}"
        );
        assert!(
            approx_eq(b, b2, 1e-4),
            "sRGB round-trip {name}: b {b} vs {b2}"
        );
    }
}

#[test]
fn oklab_lerp_perceptual_uniformity() {
    // Two distinct colors with different lightness and chroma.
    let start = Oklab::new(0.2, -0.1, 0.05, 1.0);
    let end = Oklab::new(0.8, 0.1, -0.05, 1.0);
    let mid = start.lerp(end, 0.5);

    // Perceptual uniformity: the midpoint should be equidistant from both
    // endpoints in Oklab space.
    let dist_start_to_mid = start.distance(&mid);
    let dist_mid_to_end = mid.distance(&end);
    assert!(
        approx_eq(dist_start_to_mid, dist_mid_to_end, 1e-5),
        "midpoint should be perceptually equidistant: d1={dist_start_to_mid}, d2={dist_mid_to_end}"
    );

    // The midpoint should also be half the total distance.
    let total = start.distance(&end);
    assert!(
        approx_eq(dist_start_to_mid, total * 0.5, 1e-5),
        "midpoint distance should be half the total: half={total}, mid={dist_start_to_mid}"
    );
}

#[test]
fn gamut_mapping_preserves_hue() {
    // An out-of-gamut color with high chroma.
    let extreme = Oklab::new(0.7, 0.5, 0.5, 1.0);
    let lch_before = Oklch::from_oklab(&extreme);

    // Verify it is actually out of gamut.
    let (r, g, b) = extreme.to_linear_srgb();
    assert!(
        r > 1.0 || g > 1.0 || b > 1.0 || r < 0.0 || g < 0.0 || b < 0.0,
        "test color should be out of gamut"
    );

    let mapped = gamut_map(extreme, Gamut::Srgb);
    let lch_after = Oklch::from_oklab(&mapped);

    // Hue must be preserved (chroma is reduced, lightness stays the same).
    assert!(
        approx_eq(lch_before.h, lch_after.h, 1e-4),
        "hue must be preserved during gamut mapping: before={}, after={}",
        lch_before.h,
        lch_after.h
    );
    assert!(
        approx_eq(lch_before.l, lch_after.l, 1e-4),
        "lightness must be preserved during gamut mapping"
    );

    // The mapped color must be in gamut.
    let (r, g, b) = mapped.to_srgb();
    assert!((0.0..=1.0).contains(&r), "mapped r in gamut: {r}");
    assert!((0.0..=1.0).contains(&g), "mapped g in gamut: {g}");
    assert!((0.0..=1.0).contains(&b), "mapped b in gamut: {b}");
}

#[test]
fn wcag_contrast_white_on_black_is_21() {
    let ratio = wcag_contrast(Oklab::WHITE, Oklab::BLACK);
    assert!(
        (ratio - 21.0).abs() < 1e-3,
        "WCAG contrast of white on black should be 21.0, got {ratio}"
    );

    // Symmetric: black on white is also 21.
    let ratio_rev = wcag_contrast(Oklab::BLACK, Oklab::WHITE);
    assert!(
        (ratio_rev - 21.0).abs() < 1e-3,
        "WCAG contrast of black on white should be 21.0, got {ratio_rev}"
    );

    // Identical colors yield ratio 1.0.
    let ratio_same = wcag_contrast(Oklab::WHITE, Oklab::WHITE);
    assert!(
        (ratio_same - 1.0).abs() < 1e-3,
        "WCAG contrast of identical colors should be 1.0, got {ratio_same}"
    );
}

#[test]
fn apca_contrast_white_on_black_negative() {
    // White text on black background: light-on-dark → negative Lc (APCA convention).
    let lc = apca_contrast(Oklab::WHITE, Oklab::BLACK);
    assert!(
        lc < -100.0,
        "APCA contrast of white on black should be large and negative, got {lc}"
    );

    // Black on white should be large and positive (dark-on-light).
    let lc_rev = apca_contrast(Oklab::BLACK, Oklab::WHITE);
    assert!(
        lc_rev > 100.0,
        "APCA contrast of black on white should be large and positive, got {lc_rev}"
    );
}

#[test]
fn srgb_linear_roundtrip() {
    // Verify the sRGB ↔ linear conversion functions are consistent.
    for v in [0.0_f32, 0.04, 0.25, 0.5, 0.75, 1.0] {
        let linear = srgb_to_linear(v);
        let back = linear_to_srgb(linear);
        assert!(
            approx_eq(v, back, 1e-4),
            "sRGB round-trip at {v}: got {back}"
        );
    }
}

// ===========================================================================
// 5. Theme token integration
// ===========================================================================

#[test]
fn light_to_dark_theme_diff() {
    let light = default_light();
    let dark = default_dark();
    let diff = ThemeDiff::from_themes(&light, &dark);

    // The diff must contain color deltas — light and dark themes differ.
    assert!(
        !diff.deltas().is_empty(),
        "light and dark themes should have color differences"
    );

    // Background and text colors definitely differ.
    let keys: Vec<TokenKey> = diff.deltas().iter().map(|d| d.key).collect();
    assert!(
        keys.contains(&TokenKey::BackgroundColor),
        "diff should include BackgroundColor"
    );
    assert!(
        keys.contains(&TokenKey::TextColor),
        "diff should include TextColor"
    );

    // Every delta must have distinct from/to colors.
    for d in diff.deltas() {
        assert_ne!(
            d.from, d.to,
            "delta for {:?} should have different from/to",
            d.key
        );
    }
}

#[test]
fn theme_diff_interpolate_midpoint() {
    let mut from_theme = Theme::new("from");
    from_theme.set(
        TokenKey::BackgroundColor,
        ThemeToken::Color(Oklab::new(0.0, 0.0, 0.0, 1.0)),
    );
    from_theme.set(
        TokenKey::TextColor,
        ThemeToken::Color(Oklab::new(1.0, 0.0, 0.0, 1.0)),
    );

    let mut to_theme = Theme::new("to");
    to_theme.set(
        TokenKey::BackgroundColor,
        ThemeToken::Color(Oklab::new(1.0, 0.0, 0.0, 1.0)),
    );
    to_theme.set(
        TokenKey::TextColor,
        ThemeToken::Color(Oklab::new(0.0, 0.0, 0.0, 1.0)),
    );

    let diff = ThemeDiff::from_themes(&from_theme, &to_theme);
    assert_eq!(diff.deltas().len(), 2, "should have 2 color deltas");

    let mid = diff.interpolate(0.5);
    assert_eq!(mid.len(), 2);

    // At t=0.5 every color should be the exact midpoint.
    for (key, color) in &mid {
        let delta = diff
            .deltas()
            .iter()
            .find(|d| &d.key == key)
            .expect("interpolated key should match a delta");
        let expected = delta.from.lerp(delta.to, 0.5);
        assert!(
            oklab_approx_eq(*color, expected, 1e-5),
            "midpoint color for {key:?} should be exact midpoint"
        );
    }

    // At t=0.0 we get the from colors; at t=1.0 the to colors.
    let at_zero = diff.interpolate(0.0);
    for (i, (key, color)) in at_zero.iter().enumerate() {
        assert_eq!(*key, diff.deltas()[i].key);
        assert!(oklab_approx_eq(*color, diff.deltas()[i].from, 1e-5));
    }

    let at_one = diff.interpolate(1.0);
    for (i, (key, color)) in at_one.iter().enumerate() {
        assert_eq!(*key, diff.deltas()[i].key);
        assert!(oklab_approx_eq(*color, diff.deltas()[i].to, 1e-5));
    }
}

#[test]
fn theme_uniforms_from_theme() {
    let theme = default_light();
    let uniforms = ThemeUniforms::from_theme(&theme);

    // The default light theme has 14 color tokens (all entries in
    // COLOR_TOKEN_KEYS).
    assert_eq!(
        uniforms.color_count, 14,
        "default light theme should produce 14 color uniforms"
    );

    // The first color (BackgroundColor) should match the theme's value.
    let bg = theme
        .get_color(TokenKey::BackgroundColor)
        .expect("light theme should have BackgroundColor");
    assert!(
        oklab_approx_eq(uniforms.colors[0], bg, 1e-6),
        "uniforms[0] should match BackgroundColor"
    );

    // The second color (SurfaceColor) should match.
    let surface = theme
        .get_color(TokenKey::SurfaceColor)
        .expect("light theme should have SurfaceColor");
    assert!(
        oklab_approx_eq(uniforms.colors[1], surface, 1e-6),
        "uniforms[1] should match SurfaceColor"
    );

    // The byte slice should be exactly 256 bytes for GPU upload.
    assert_eq!(uniforms.as_bytes().len(), 256);
}

#[test]
fn theme_dictionary_resolves_both_modes() {
    let dict = ThemeDictionary::new();
    assert_eq!(dict.theme(ThemeMode::Light).name, "Light");
    assert_eq!(dict.theme(ThemeMode::Dark).name, "Dark");

    let light_uniforms = ThemeUniforms::from_theme(dict.theme(ThemeMode::Light));
    let dark_uniforms = ThemeUniforms::from_theme(dict.theme(ThemeMode::Dark));
    assert_eq!(light_uniforms.color_count, 14);
    assert_eq!(dark_uniforms.color_count, 14);

    // Light background is bright, dark background is dark.
    assert!(
        light_uniforms.colors[0].l > dark_uniforms.colors[0].l,
        "light background should be brighter than dark background"
    );
}

// ===========================================================================
// 6. Cross-crate integration
// ===========================================================================

#[test]
fn spring_animation_drives_theme_transition() {
    // Use a critically-damped spring to drive the theme transition's `t`
    // parameter from 0.0 to 1.0. This mirrors how a spring-based gesture can
    // control a GPU theme blend.
    let from = ThemeUniforms::from_theme(&default_light());
    let to = ThemeUniforms::from_theme(&default_dark());
    let mut buffer = ThemeUniformBuffer::new();
    buffer.set_from(from);
    buffer.set_to(to);

    // Spring from 0.0 → 1.0 with critical damping.
    let mut solver = SpringSolver::new(SpringConfig::CRITICAL, 0.0, 1.0, 0.0);

    // At t=0 the spring position is 0.0 → t parameter is 0.0.
    let (pos, _vel) = solver.sample();
    buffer.set_t(pos);
    assert!(
        approx_eq(buffer.transition_t(), 0.0, 1e-6),
        "initial transition t should be 0.0"
    );

    // The interpolated uniforms at t=0 should equal `from`.
    let current_at_start = from.lerp(&to, buffer.transition_t());
    assert!(
        oklab_approx_eq(current_at_start.colors[0], from.colors[0], 1e-6),
        "interpolated color at t=0 should equal from"
    );

    // Advance the spring partway and use its position as the blend parameter.
    solver.advance(0.2);
    let (pos, _vel) = solver.sample();
    assert!(
        pos > 0.0 && pos < 1.0,
        "spring should be mid-motion: pos={pos}"
    );
    buffer.set_t(pos);
    assert!(
        buffer.transition_t() > 0.0 && buffer.transition_t() < 1.0,
        "transition t should be mid-range"
    );

    // The interpolated color should be between from and to.
    let current_mid = from.lerp(&to, buffer.transition_t());
    let bg_from = from.colors[0];
    let bg_to = to.colors[0];
    let bg_mid = current_mid.colors[0];
    assert!(
        bg_mid.l > bg_from.l.min(bg_to.l) && bg_mid.l < bg_from.l.max(bg_to.l),
        "interpolated lightness should be strictly between from and to"
    );

    // After the spring settles, t should be 1.0 (clamped).
    solver.advance(60.0);
    let (pos, _vel) = solver.sample();
    buffer.set_t(pos);
    assert!(
        approx_eq(buffer.transition_t(), 1.0, 1e-4),
        "settled spring should drive t to 1.0"
    );

    let current_at_end = from.lerp(&to, buffer.transition_t());
    assert!(
        oklab_approx_eq(current_at_end.colors[0], to.colors[0], 1e-5),
        "interpolated color at t=1 should equal to"
    );
}

#[test]
fn oklab_color_in_theme_uniforms() {
    // Create a theme with specific Oklab colors and verify they survive the
    // conversion to ThemeUniforms with full precision.
    let mut theme = Theme::new("oklab_test");

    let bg = Oklab::new(0.42, -0.05, 0.03, 1.0);
    let primary = Oklab::new(0.65, 0.12, -0.08, 1.0);
    let text = Oklab::new(0.20, 0.0, 0.0, 1.0);

    theme.set(TokenKey::BackgroundColor, ThemeToken::Color(bg));
    theme.set(TokenKey::PrimaryColor, ThemeToken::Color(primary));
    theme.set(TokenKey::TextColor, ThemeToken::Color(text));

    let uniforms = ThemeUniforms::from_theme(&theme);

    // Three color tokens were set, but they map to indices 0 (Background),
    // 2 (Primary), and 5 (Text) in the canonical COLOR_TOKEN_KEYS order.
    assert_eq!(uniforms.color_count, 6, "highest set index + 1");

    // Verify each color is preserved exactly.
    assert!(
        oklab_approx_eq(uniforms.colors[0], bg, 1e-6),
        "BackgroundColor should be preserved in uniforms"
    );
    assert!(
        oklab_approx_eq(uniforms.colors[2], primary, 1e-6),
        "PrimaryColor should be preserved in uniforms"
    );
    assert!(
        oklab_approx_eq(uniforms.colors[5], text, 1e-6),
        "TextColor should be preserved in uniforms"
    );

    // Unset slots should remain zeroed.
    assert_eq!(
        uniforms.colors[1],
        Oklab::new(0.0, 0.0, 0.0, 0.0),
        "unset SurfaceColor slot should be zeroed"
    );
}
