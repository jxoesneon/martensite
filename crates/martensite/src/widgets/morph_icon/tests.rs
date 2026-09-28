//! `MorphIcon` + private engine test suite — the ADR-0041 test matrix.
//!
//! Three sections, in order:
//!
//! 1. **Widget surface that does not need the geometry engine** — defaults,
//!    builders, measure/event semantics, rest-state ticking, a11y, and the
//!    arena flag contracts that a `MorphIcon` at rest already satisfies.
//! 2. **Engine-dependent widget tests** — the `d`-parse/cap contract, the
//!    morph lifecycle (`morph_to` → spring → settle), `seek` determinism,
//!    reduced-motion snap, and paint output. Every one is tagged
//!    `// [engine]` on its first line and panics on
//!    `unimplemented!("engine port pending")` until the port lands — they
//!    must run (no `#[ignore]`) so the suite goes green the day it does.
//! 3. **Private-engine unit tests** — `d_to_cubics`, `d_to_sampled`,
//!    `resample`/`build_plan`/`MorphPlan::eval` internals, same status.

use super::*;

use super::engine::{
    bezpath_to_cubics, d_to_cubics, resample, CubicSubpath, MAX_D_BYTES, MAX_SEGMENTS,
    MAX_SUBPATHS, SAMPLE_N,
};
use kurbo::PathEl;
use martensite_core::{
    HotNode, NodeFlags, PaintCommand, PaintList, PointerButton, WidgetArena, WidgetEvent,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A fast overdamped spring — settles inside a bounded number of 16 ms
/// ticks so tests can drive an animation to completion deterministically.
fn fast() -> SpringConfig {
    SpringConfig::new(1.0, 500.0, 60.0).unwrap()
}

/// The icon grid as square bounds (`paint` reads `self.bounds`).
fn bounds_24() -> Rect {
    Rect::new(0.0, 0.0, 24.0, 24.0)
}

/// Lays `icon` out on `bounds` (this is what makes `paint` emit).
fn laid_out(icon: &mut MorphIcon, bounds: Rect) {
    let mut hot = HotNode::default();
    icon.layout(
        &mut LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        },
        bounds,
    );
}

/// Paints an already-laid-out icon into a fresh list.
fn painted(icon: &MorphIcon) -> PaintList {
    let theme = martensite_theme::Theme::new("test");
    let mut list = PaintList::new();
    icon.paint(&mut PaintContext {
        list: &mut list,
        bounds: icon.bounds,
        scale: 1.0,
        theme: &theme,
        text_painter: None,
    });
    list
}

fn a11y_node(icon: &MorphIcon) -> AccessKitNode {
    let mut node = AccessKitNode::new(accesskit::Role::Unknown);
    icon.accessibility(&mut node);
    node
}

/// Ticks at 16 ms until `tick` reports no work; the last true-returning
/// tick is the settle frame itself (the plan drops on it). Capped so a
/// never-settling spring fails loudly instead of hanging.
fn settle(icon: &mut MorphIcon) -> usize {
    let mut frames = 0usize;
    while icon.tick(Duration::from_millis(16)) {
        frames += 1;
        assert!(frames < 4096, "spring did not settle in 4096 frames");
    }
    frames
}

fn sorted(mut pts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    pts.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
    pts
}

fn push_el(el: &PathEl, out: &mut Vec<(f64, f64)>) {
    match *el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => out.push((p.x, p.y)),
        PathEl::QuadTo(a, b) => {
            out.push((a.x, a.y));
            out.push((b.x, b.y));
        }
        PathEl::CurveTo(a, b, c) => {
            out.push((a.x, a.y));
            out.push((b.x, b.y));
            out.push((c.x, c.y));
        }
        PathEl::ClosePath => {}
    }
}

/// All `(x, y)` endpoints/control points of every `StrokePath` command in
/// the list, sorted — a geometric fingerprint insensitive to subpath
/// order, traversal direction, and circular offsets the plan may choose.
fn stroke_points(list: &PaintList) -> Vec<(f64, f64)> {
    let mut pts = Vec::new();
    for c in &list.commands {
        if let PaintCommand::StrokePath(path, ..) = c {
            for el in path.elements() {
                push_el(el, &mut pts);
            }
        }
    }
    sorted(pts)
}

/// Same fingerprint over raw `BezPath`s (e.g. `frame_paths()` output).
fn path_points(paths: &[kurbo::BezPath]) -> Vec<(f64, f64)> {
    let mut pts = Vec::new();
    for p in paths {
        for el in p.elements() {
            push_el(el, &mut pts);
        }
    }
    sorted(pts)
}

/// Point multiset of engine `Sampled` subpaths, sorted.
fn sampled_points(subs: &[Sampled]) -> Vec<(f64, f64)> {
    let mut pts = Vec::new();
    for s in subs {
        for pair in s.pts.as_chunks::<2>().0 {
            pts.push((pair[0], pair[1]));
        }
    }
    sorted(pts)
}

/// Order-insensitive point-set equality within a per-axis epsilon (covers
/// the plan's endpoint recomposition, which is exact only up to f64
/// rounding — the point *count* must still match exactly).
fn assert_pts_eq(actual: &[(f64, f64)], expected: &[(f64, f64)], eps: f64) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "point count differs:\n  actual   {actual:?}\n  expected {expected:?}"
    );
    for (a, e) in actual.iter().zip(expected) {
        assert!(
            (a.0 - e.0).abs() <= eps && (a.1 - e.1).abs() <= eps,
            "point {a:?} vs expected {e:?} (eps {eps})"
        );
    }
}

/// Arena-contract probe recording every `set_reduced_motion` push. A boxed
/// `dyn Widget` cannot be downcast back to `MorphIcon`, so the arena's
/// live-push and insert-seeding contracts are proven through this delegate;
/// the widget-side snap is covered by [`reduced_motion_morph_to_snaps`].
struct FlagProbe(Arc<Mutex<Vec<bool>>>);

impl Widget for FlagProbe {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::ZERO
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}

    fn set_reduced_motion(&mut self, reduced: bool) {
        self.0.lock().unwrap().push(reduced);
    }
}

// ===========================================================================
// 1. Engine-free widget surface
// ===========================================================================

#[test]
fn new_is_empty_at_rest() {
    let icon = MorphIcon::new();
    assert!(!icon.is_animating());
    assert!((icon.progress() - 1.0).abs() < f32::EPSILON);
    assert!(icon.plan.is_none());
    assert!(icon.current.is_empty());
    assert!(icon.frame_paths().is_empty());
    assert!(icon.label.is_empty());
    assert!(!icon.decorative);
    assert!(!icon.reduced_motion);
    assert!(icon.ink.is_none());
}

#[test]
fn default_matches_new() {
    let d = MorphIcon::default();
    assert!((d.size_pt - SIZE_PT).abs() < f32::EPSILON);
    assert!((d.stroke_pt - STROKE_PT).abs() < f32::EPSILON);
}

#[test]
fn builder_knobs_store_values() {
    let icon = MorphIcon::new()
        .label("Menu")
        .decorative(true)
        .size(48.0)
        .stroke_width(3.5)
        .ink([1, 2, 3, 4]);
    assert_eq!(icon.label, "Menu");
    assert!(icon.decorative);
    assert!((icon.size_pt - 48.0).abs() < f32::EPSILON);
    assert!((icon.stroke_pt - 3.5).abs() < f32::EPSILON);
    assert_eq!(icon.ink, Some([1, 2, 3, 4]));
}

#[test]
fn builders_clamp_negative_inputs() {
    let icon = MorphIcon::new().size(-4.0).stroke_width(-1.0);
    assert!((icon.size_pt - 0.0).abs() < f32::EPSILON);
    assert!((icon.stroke_pt - 0.0).abs() < f32::EPSILON);
}

#[test]
fn measure_uses_size_pt_and_respects_constraints() {
    let mut icon = MorphIcon::new();
    let mut big = MorphIcon::new().size(40.0);
    let mut hot = HotNode::default();
    let mut cx = LayoutContext {
        hot: &mut hot,
        scale: 1.0,
    };
    let wide = LayoutConstraints {
        min_size: Vec2::ZERO,
        max_size: Vec2::new(100.0, 100.0),
    };
    assert_eq!(icon.measure(&mut cx, wide), Vec2::new(24.0, 24.0));
    assert_eq!(big.measure(&mut cx, wide), Vec2::new(40.0, 40.0));
    // Tight constraints clamp each axis independently.
    let tight = LayoutConstraints {
        min_size: Vec2::ZERO,
        max_size: Vec2::new(10.0, 6.0),
    };
    assert_eq!(icon.measure(&mut cx, tight), Vec2::new(10.0, 6.0));
    // Logical points convert through the HiDPI scale.
    let mut hidpi = LayoutContext {
        hot: &mut hot,
        scale: 2.0,
    };
    assert_eq!(icon.measure(&mut hidpi, wide), Vec2::new(48.0, 48.0));
}

#[test]
fn event_is_ignored_leaf() {
    // A MorphIcon inside a Button borrows the parent's semantics — it
    // never consumes input itself.
    let mut icon = MorphIcon::new();
    let ev = WidgetEvent::PointerPressed {
        position: Vec2::new(4.0, 4.0),
        button: PointerButton::Primary,
        count: 1,
    };
    let mut cx = EventContext {
        event: &ev,
        bounds: bounds_24(),
        scale: 1.0,
    };
    assert_eq!(icon.event(&mut cx), EventResponse::Ignored);
}

#[test]
fn debug_name_is_morph_icon() {
    assert_eq!(MorphIcon::new().debug_name(), "MorphIcon");
}

#[test]
fn tick_at_rest_reports_no_work() {
    let mut icon = MorphIcon::new();
    assert!(!icon.tick(Duration::from_millis(16)));
    // Declared paint-only: when a tick *does* report work (mid-flight),
    // the arena marks DIRTY_PAINT without DIRTY_A11Y (ADR-0041 §3).
    assert!(icon.tick_paint_only());
}

#[test]
fn seek_without_plan_is_noop() {
    let mut icon = MorphIcon::new();
    icon.seek(0.5);
    icon.seek(2.0); // extrapolation range — still nothing to freeze
    assert!(icon.plan.is_none());
    assert!(!icon.is_animating());
}

#[test]
fn set_reduced_motion_stores_flag() {
    let mut icon = MorphIcon::new();
    Widget::set_reduced_motion(&mut icon, true);
    assert!(icon.reduced_motion);
    Widget::set_reduced_motion(&mut icon, false);
    assert!(!icon.reduced_motion);
}

#[test]
fn a11y_role_image_with_label() {
    let icon = MorphIcon::new().label("Menu");
    let node = a11y_node(&icon);
    assert_eq!(node.role(), accesskit::Role::Image);
    assert_eq!(node.label(), Some("Menu"));
    assert!(!node.is_hidden());
}

#[test]
fn a11y_unlabeled_image_has_no_name() {
    let icon = MorphIcon::new();
    let node = a11y_node(&icon);
    assert_eq!(node.role(), accesskit::Role::Image);
    assert_eq!(node.label(), None);
}

#[test]
fn a11y_decorative_is_hidden() {
    let icon = MorphIcon::new().label("Menu").decorative(true);
    let node = a11y_node(&icon);
    assert!(node.is_hidden());
    // Hidden takes precedence — the surrounding control owns the name.
    assert_eq!(node.label(), None);
}

#[test]
fn set_label_updates_semantic_state() {
    let mut icon = MorphIcon::new().label("Menu");
    icon.set_label("Close");
    let node = a11y_node(&icon);
    assert_eq!(node.label(), Some("Close"));
}

#[test]
fn paint_empty_icon_records_nothing() {
    let mut icon = MorphIcon::new();
    laid_out(&mut icon, bounds_24());
    assert!(painted(&icon).commands.is_empty());
}

#[test]
fn morph_error_display_and_std_error() {
    assert!(MorphError::Parse("bad bits".into())
        .to_string()
        .contains("bad bits"));
    assert_eq!(
        MorphError::TooLarge("segments").to_string(),
        "icon path too large: segments"
    );
    assert_eq!(
        MorphError::Degenerate("nan coord").to_string(),
        "degenerate icon geometry: nan coord"
    );
    assert_eq!(
        MorphError::Empty.to_string(),
        "icon path produced no subpaths"
    );
    fn needs_std_error<E: std::error::Error>(_: &E) {}
    needs_std_error(&MorphError::Empty);
}

#[test]
fn sampled_to_bezpaths_emits_polylines() {
    // `sampled_to_bezpaths` is implemented — exercised directly.
    let subs = vec![
        Sampled {
            pts: vec![0.0, 0.0, 4.0, 0.0, 4.0, 4.0],
            closed: false,
        },
        Sampled {
            pts: vec![8.0, 8.0, 12.0, 8.0, 12.0, 12.0],
            closed: true,
        },
    ];
    let paths = sampled_to_bezpaths(&subs);
    assert_eq!(paths.len(), 2);
    assert!(matches!(paths[0].elements()[0], PathEl::MoveTo(_)));
    assert!(paths[0]
        .elements()
        .iter()
        .all(|e| matches!(e, PathEl::MoveTo(_) | PathEl::LineTo(_))));
    assert!(matches!(
        paths[1].elements().last(),
        Some(PathEl::ClosePath)
    ));
    // Subpaths with fewer than two points are dropped.
    assert!(sampled_to_bezpaths(&[Sampled {
        pts: vec![1.0, 2.0],
        closed: false,
    }])
    .is_empty());
}

#[test]
fn cubic_subpath_seg_count() {
    assert_eq!(
        CubicSubpath {
            pts: vec![0.0; 8],
            closed: false
        }
        .seg_count(),
        1
    );
    assert_eq!(
        CubicSubpath {
            pts: vec![0.0; 14],
            closed: false
        }
        .seg_count(),
        2
    );
    assert_eq!(
        CubicSubpath {
            pts: Vec::new(),
            closed: true
        }
        .seg_count(),
        0
    );
}

// ---- arena integration (rest-state halves) ----

#[test]
fn arena_tick_at_rest_dirties_nothing() {
    let mut arena = WidgetArena::new();
    let id = arena.insert_with_widget(HotNode::default(), Box::new(MorphIcon::new()));
    arena.tick(Duration::from_millis(16));
    let flags = arena.get_hot(id).unwrap().flags;
    assert!(!flags.contains(NodeFlags::DIRTY_PAINT));
    assert!(!flags.contains(NodeFlags::DIRTY_A11Y));
}

#[test]
fn arena_pushes_reduced_motion_live_and_at_insert() {
    // Arena half of the reduced-motion contract (ADR-0041 §3): a live
    // `set_reduced_motion` reaches every widget, and a node inserted while
    // the flag holds is seeded at insert. `Box<dyn Widget>` cannot be
    // downcast to `MorphIcon`, so the push is recorded by `FlagProbe`.
    let mut arena = WidgetArena::new();
    let a = Arc::new(Mutex::new(Vec::new()));
    arena.insert_with_widget(HotNode::default(), Box::new(FlagProbe(Arc::clone(&a))));
    // No preference installed — insert alone must not push.
    assert!(a.lock().unwrap().is_empty());
    // Live push reaches existing widgets.
    arena.set_reduced_motion(true);
    assert!(arena.reduced_motion());
    assert_eq!(*a.lock().unwrap(), vec![true]);
    // Nodes born while the flag holds are seeded at insert.
    let b = Arc::new(Mutex::new(Vec::new()));
    arena.insert_with_widget(HotNode::default(), Box::new(FlagProbe(Arc::clone(&b))));
    assert_eq!(*b.lock().unwrap(), vec![true]);
}

#[test]
fn arena_seeded_morph_icon_ticks_clean() {
    // A MorphIcon inserted while the arena holds the reduced-motion flag
    // takes the same push through the identical call path the probe
    // records above. Nothing further is observable through `dyn Widget`
    // (`morph_to` is inherent — the widget-side snap is asserted in
    // `reduced_motion_morph_to_snaps`), so this stays a smoke check.
    let mut arena = WidgetArena::new();
    arena.set_reduced_motion(true);
    let id = arena.insert_with_widget(HotNode::default(), Box::new(MorphIcon::new().label("Menu")));
    arena.tick(Duration::from_millis(16));
    assert!(arena.reduced_motion());
    let flags = arena.get_hot(id).unwrap().flags;
    assert!(!flags.contains(NodeFlags::DIRTY_A11Y));
}

// ===========================================================================
// 2. Engine-dependent widget tests — each tagged `// [engine]`; they run
//    and panic on `unimplemented!("engine port pending")` until the port
//    lands. No `#[ignore]` — the suite flips green on landing.
// ===========================================================================

// ---- construction / parse contract (ADR-0041 §1 caps) ----

#[test]
fn icon_parses_every_demo_d() {
    // [engine]
    for d in [
        demo::MENU,
        demo::CLOSE,
        demo::PLAY,
        demo::PAUSE,
        demo::CHECK,
        demo::VOLUME_ON,
        demo::VOLUME_OFF,
        demo::EYE_OPEN,
        demo::EYE_CLOSED,
        demo::PLUS,
        demo::MINUS,
        demo::CHEVRON_RIGHT,
        demo::CHEVRON_DOWN,
        demo::LOCK,
        demo::LOCK_OPEN,
    ] {
        assert!(MorphIcon::icon(d).is_ok(), "demo icon failed to parse: {d}");
    }
}

#[test]
fn icon_malformed_d_is_parse_error() {
    // [engine]
    for bad in ["zzz", "M", "M1,2 L3", "M0 0 Q", "M0 0 X5 5"] {
        match MorphIcon::icon(bad) {
            Err(MorphError::Parse(_)) => {}
            Err(e) => panic!("{bad:?}: expected Err(Parse), got Err({e:?})"),
            Ok(_) => panic!("{bad:?}: expected Err(Parse), got Ok"),
        }
    }
}

#[test]
fn icon_oversized_d_is_too_large() {
    // [engine]
    let d = format!("M0 0{}z", " l1 0".repeat(MAX_D_BYTES / 5 + 1));
    assert!(d.len() > MAX_D_BYTES);
    match MorphIcon::icon(&d) {
        Err(MorphError::TooLarge(_)) => {}
        Err(e) => panic!("expected Err(TooLarge), got Err({e:?})"),
        Ok(_) => panic!("expected Err(TooLarge), got Ok"),
    }
}

#[test]
fn icon_too_many_subpaths_errs() {
    // [engine] — > MAX_SUBPATHS subpaths is rejected (cap or degenerate).
    let d = (0..=MAX_SUBPATHS)
        .map(|i| format!("M{i} 0 l1 1"))
        .collect::<Vec<_>>()
        .join("");
    assert!(d.len() <= MAX_D_BYTES, "isolate the subpath cap");
    match MorphIcon::icon(&d) {
        Err(MorphError::TooLarge(_) | MorphError::Degenerate(_)) => {}
        Err(e) => panic!("expected Err(TooLarge|Degenerate), got Err({e:?})"),
        Ok(_) => panic!("expected Err(TooLarge|Degenerate), got Ok"),
    }
}

#[test]
fn icon_too_many_segments_errs() {
    // [engine] — > MAX_SEGMENTS total segments is rejected.
    let d = format!("M0 0{}", " l1 0".repeat(MAX_SEGMENTS + 1));
    assert!(d.len() <= MAX_D_BYTES, "isolate the segment cap");
    assert!(MorphIcon::icon(&d).is_err(), "segment cap not enforced");
}

#[test]
fn icon_empty_or_trivial_d_errs() {
    // [engine] — `""`/whitespace/lone-moveto parse but produce no drawable
    // subpaths; a bare coordinate list parses to an empty path the same way.
    for d in ["", "   ", "M5 5", "10 20 30"] {
        assert!(MorphIcon::icon(d).is_err(), "{d:?} should be Err");
    }
}

#[test]
fn icon_nonfinite_d_errs() {
    // [engine] — non-finite coordinates are rejected, never painted.
    for d in ["M0 0 L1e999 5", "M0 0 L-1e999 5"] {
        assert!(MorphIcon::icon(d).is_err(), "{d:?} should be Err");
    }
}

#[test]
fn set_icon_replaces_rest_state() {
    // [engine]
    let mut icon = MorphIcon::icon(demo::MENU).unwrap();
    icon.set_icon(demo::CLOSE).unwrap();
    assert!(!icon.is_animating());
    assert!(icon.plan.is_none());
    // A failed replacement keeps the previous icon.
    let before = path_points(&icon.frame_paths());
    assert!(icon.set_icon("zzz").is_err());
    assert_eq!(path_points(&icon.frame_paths()), before);
}

// ---- morph lifecycle ----

#[test]
fn morph_to_starts_animation() {
    // [engine]
    let mut icon = MorphIcon::icon(demo::MENU).unwrap();
    icon.morph_to(demo::CLOSE, fast()).unwrap();
    assert!(icon.is_animating());
    assert!(icon.plan.is_some());
    assert!(icon.progress() < 0.5, "spring starts at position 0");
}

#[test]
fn morph_ticks_until_spring_settles() {
    // [engine]
    let mut icon = MorphIcon::icon(demo::MENU).unwrap();
    icon.morph_to(demo::CLOSE, fast()).unwrap();
    assert!(
        icon.tick(Duration::from_millis(16)),
        "mid-flight tick must request a repaint"
    );
    let frames = settle(&mut icon);
    assert!(frames >= 1);
    assert!(!icon.is_animating());
    assert!((icon.progress() - 1.0).abs() < f32::EPSILON);
    assert!(icon.plan.is_none(), "settled morph must drop its plan");
    assert!(
        !icon.tick(Duration::from_millis(16)),
        "settled icon reports no further work"
    );
}

#[test]
fn settled_morph_paints_canonical_target() {
    // [engine] — plan dropped at settle: paint is the canonical `to` shape.
    let mut icon = MorphIcon::icon(demo::MENU).unwrap();
    laid_out(&mut icon, bounds_24());
    icon.morph_to(demo::CLOSE, fast()).unwrap();
    settle(&mut icon);
    let mut canonical = MorphIcon::icon(demo::CLOSE).unwrap();
    laid_out(&mut canonical, bounds_24());
    assert_pts_eq(
        &stroke_points(&painted(&icon)),
        &stroke_points(&painted(&canonical)),
        1e-4,
    );
}

#[test]
fn morph_reentry_midflight_keeps_animating() {
    // [engine] — morphTo-interrupt contract: the interpolated shape becomes
    // the new origin; the latest target still wins, exactly.
    let mut icon = MorphIcon::icon(demo::MENU).unwrap();
    icon.morph_to(demo::CLOSE, fast()).unwrap();
    icon.tick(Duration::from_millis(16));
    icon.morph_to(demo::CHECK, fast()).unwrap();
    assert!(icon.is_animating());
    settle(&mut icon);
    let mut canonical = MorphIcon::icon(demo::CHECK).unwrap();
    laid_out(&mut icon, bounds_24());
    laid_out(&mut canonical, bounds_24());
    assert_pts_eq(
        &stroke_points(&painted(&icon)),
        &stroke_points(&painted(&canonical)),
        1e-4,
    );
}

#[test]
fn morph_to_rejects_bad_d_without_losing_state() {
    // [engine]
    let mut icon = MorphIcon::icon(demo::MENU).unwrap();
    let before = path_points(&icon.frame_paths());
    assert!(matches!(
        icon.morph_to("zzz", fast()),
        Err(MorphError::Parse(_))
    ));
    assert!(!icon.is_animating());
    assert_eq!(path_points(&icon.frame_paths()), before);
}

#[test]
fn morph_to_path_accepts_bezpath_input() {
    // [engine]
    let mut icon = MorphIcon::icon(demo::MENU).unwrap();
    let mut p = kurbo::BezPath::new();
    p.move_to((4.0, 4.0));
    p.line_to((20.0, 20.0));
    icon.morph_to_path(&p, fast()).unwrap();
    assert!(icon.is_animating());
    settle(&mut icon);
    assert!(!icon.is_animating());
}

#[test]
fn morph_from_empty_icon_is_handled() {
    // [engine] — morphing *from* an empty icon may spawn or reject per the
    // engine contract; it must never panic.
    let mut icon = MorphIcon::new();
    let res = icon.morph_to(demo::CLOSE, fast());
    if res.is_ok() {
        assert!(icon.is_animating());
        settle(&mut icon);
    }
}

// ---- seek / determinism (ADR-0041 §5) ----

#[test]
fn seek_endpoints_paint_canonical_shapes() {
    // [engine] — seek(0) paints exactly `from`, seek(1) exactly `to`
    // (same-count pair so point multisets align one-to-one).
    let mut icon = MorphIcon::icon(demo::CHEVRON_RIGHT).unwrap();
    laid_out(&mut icon, bounds_24());
    icon.morph_to(demo::CHEVRON_DOWN, fast()).unwrap();

    icon.seek(0.0);
    let at0 = stroke_points(&painted(&icon));
    icon.seek(1.0);
    let at1 = stroke_points(&painted(&icon));

    let mut from = MorphIcon::icon(demo::CHEVRON_RIGHT).unwrap();
    laid_out(&mut from, bounds_24());
    let mut to = MorphIcon::icon(demo::CHEVRON_DOWN).unwrap();
    laid_out(&mut to, bounds_24());

    assert_pts_eq(&at0, &stroke_points(&painted(&from)), 1e-3);
    assert_pts_eq(&at1, &stroke_points(&painted(&to)), 1e-3);
}

#[test]
fn seeked_frames_are_deterministic() {
    // [engine] — identical constructions seeked to the same t emit
    // bit-identical paths (arc-heavy corpus: kurbo arc slicing coverage).
    let mk = || {
        let mut i = MorphIcon::icon(demo::EYE_OPEN).unwrap();
        i.morph_to(demo::EYE_CLOSED, fast()).unwrap();
        i.seek(0.37);
        i
    };
    let (a, b) = (mk(), mk());
    let (pa, pb) = (a.frame_paths(), b.frame_paths());
    assert_eq!(pa.len(), pb.len());
    for (x, y) in pa.iter().zip(&pb) {
        assert_eq!(
            x.elements(),
            y.elements(),
            "same seek must emit identical paths"
        );
    }
}

#[test]
fn seeked_plan_parks_not_drops() {
    // [engine] — a frozen spring reads settled (`is_animating` false) but
    // the plan stays installed so `frame_paths` keeps evaluating at `t`.
    let mut icon = MorphIcon::icon(demo::CHEVRON_RIGHT).unwrap();
    icon.morph_to(demo::CHEVRON_DOWN, fast()).unwrap();
    icon.seek(0.42);
    assert!(icon.plan.is_some(), "seek must not drop the plan");
    assert!(!icon.is_animating(), "parked spring reads settled");
    assert!(
        !icon.tick(Duration::from_millis(16)),
        "a parked plan produces no tick work"
    );
}

// ---- reduced motion (ADR-0041 §3/§6) ----

#[test]
fn reduced_motion_morph_to_snaps() {
    // [engine]
    let mut icon = MorphIcon::icon(demo::MENU).unwrap();
    laid_out(&mut icon, bounds_24());
    Widget::set_reduced_motion(&mut icon, true);
    icon.morph_to(demo::CLOSE, fast()).unwrap();
    assert!(!icon.is_animating());
    assert!(icon.plan.is_none());
    assert!((icon.progress() - 1.0).abs() < f32::EPSILON);
    // Snapped: the current frame is the canonical target shape.
    let mut canonical = MorphIcon::icon(demo::CLOSE).unwrap();
    laid_out(&mut canonical, bounds_24());
    assert_pts_eq(
        &stroke_points(&painted(&icon)),
        &stroke_points(&painted(&canonical)),
        1e-4,
    );
}

#[test]
fn reduced_motion_morph_to_path_snaps() {
    // [engine]
    let mut icon = MorphIcon::icon(demo::MENU).unwrap();
    Widget::set_reduced_motion(&mut icon, true);
    let mut p = kurbo::BezPath::new();
    p.move_to((4.0, 4.0));
    p.line_to((20.0, 20.0));
    icon.morph_to_path(&p, fast()).unwrap();
    assert!(!icon.is_animating());
    assert!(icon.plan.is_none());
}

#[test]
fn reduced_motion_still_validates_input() {
    // [engine] — `d` validation precedes the snap check: bad input is
    // still Err, never silently swallowed by reduced motion.
    let mut icon = MorphIcon::icon(demo::MENU).unwrap();
    Widget::set_reduced_motion(&mut icon, true);
    assert!(matches!(
        icon.morph_to("zzz", fast()),
        Err(MorphError::Parse(_))
    ));
}

#[test]
fn reduced_motion_push_midflight_does_not_cancel_plan() {
    // [engine] — documents the current contract: the push stores the flag;
    // an in-flight plan still runs out (only *new* morph_to calls snap).
    let mut icon = MorphIcon::icon(demo::MENU).unwrap();
    icon.morph_to(demo::CLOSE, fast()).unwrap();
    Widget::set_reduced_motion(&mut icon, true);
    assert!(icon.reduced_motion);
    assert!(icon.is_animating());
    settle(&mut icon);
}

// ---- a11y transition semantics ----

#[test]
fn a11y_reports_target_state_midflight() {
    // [engine] — the a11y name is the semantic *target* state; the
    // mid-flight shape is never announced (ADR-0041 §4).
    let mut icon = MorphIcon::icon(demo::MENU).unwrap().label("Menu");
    icon.morph_to(demo::CLOSE, fast()).unwrap();
    icon.set_label("Close");
    let node = a11y_node(&icon);
    assert_eq!(node.role(), accesskit::Role::Image);
    assert_eq!(node.label(), Some("Close"));
}

// ---- arena integration during morph ----

#[test]
fn arena_tick_marks_paint_only_during_morph() {
    // [engine] — every frame of the flight, including past settle, must
    // mark DIRTY_PAINT *without* DIRTY_A11Y (flags accumulate, so a single
    // a11y-marking frame would poison the check).
    let mut icon = MorphIcon::icon(demo::MENU).unwrap();
    icon.morph_to(demo::CLOSE, fast()).unwrap();
    let mut arena = WidgetArena::new();
    let id = arena.insert_with_widget(HotNode::default(), Box::new(icon));
    for _ in 0..400 {
        arena.tick(Duration::from_millis(16));
    }
    let flags = arena.get_hot(id).unwrap().flags;
    assert!(flags.contains(NodeFlags::DIRTY_PAINT));
    assert!(!flags.contains(NodeFlags::DIRTY_A11Y));
}

// ---- paint output ----

#[test]
fn paint_emits_one_stroke_per_subpath() {
    // [engine] — MENU is three open subpaths → three StrokePath commands.
    let mut icon = MorphIcon::icon(demo::MENU).unwrap();
    laid_out(&mut icon, bounds_24());
    let strokes = painted(&icon)
        .commands
        .iter()
        .filter(|c| matches!(c, PaintCommand::StrokePath(..)))
        .count();
    assert_eq!(strokes, 3);
}

#[test]
fn paint_stroke_width_and_default_ink() {
    // [engine] — default 2pt stroke at the 24px grid, theme-resolved ink.
    let theme = martensite_theme::Theme::new("test");
    let mut icon = MorphIcon::icon(demo::CLOSE).unwrap();
    laid_out(&mut icon, bounds_24());
    let mut list = PaintList::new();
    icon.paint(&mut PaintContext {
        list: &mut list,
        bounds: icon.bounds,
        scale: 1.0,
        theme: &theme,
        text_painter: None,
    });
    let expected_ink = theme
        .color(TokenKey::TextColor)
        .map_or([230, 230, 235, 255], |c| c.to_srgba8());
    let mut strokes = 0;
    for c in &list.commands {
        let PaintCommand::StrokePath(_, w, color) = c else {
            continue;
        };
        strokes += 1;
        assert!((*w - 2.0).abs() < 1e-3, "2pt stroke at the 24px grid");
        assert_eq!(*color, expected_ink);
    }
    assert!(strokes >= 1);
}

#[test]
fn paint_ink_override_wins() {
    // [engine]
    let mut icon = MorphIcon::icon(demo::CLOSE).unwrap().ink([7, 8, 9, 255]);
    laid_out(&mut icon, bounds_24());
    let list = painted(&icon);
    let mut strokes = 0;
    for c in &list.commands {
        if let PaintCommand::StrokePath(_, _, color) = c {
            strokes += 1;
            assert_eq!(*color, [7, 8, 9, 255]);
        }
    }
    assert!(strokes >= 1);
}

#[test]
fn paint_scales_icon_space_into_bounds() {
    // [engine] — the 24px icon grid scales to fill bounds; MENU spans
    // x∈[4,20] → [8,40] at 2× and the stroke width doubles too.
    let mut icon = MorphIcon::icon(demo::MENU).unwrap();
    laid_out(&mut icon, Rect::new(0.0, 0.0, 48.0, 48.0));
    let list = painted(&icon);
    let pts = stroke_points(&list);
    let max_x = pts.iter().map(|p| p.0).fold(f64::MIN, f64::max);
    assert!((max_x - 40.0).abs() < 0.5, "max x 20·2=40, got {max_x}");
    for c in &list.commands {
        if let PaintCommand::StrokePath(_, w, _) = c {
            assert!((*w - 4.0).abs() < 1e-3, "stroke width rides the scale");
        }
    }
}

#[test]
fn paint_closed_subpath_emits_closepath() {
    // [engine] — PLAY is `z`-terminated; its polyline carries ClosePath.
    let mut icon = MorphIcon::icon(demo::PLAY).unwrap();
    laid_out(&mut icon, bounds_24());
    let list = painted(&icon);
    assert!(list.commands.iter().any(|c| matches!(
        c,
        PaintCommand::StrokePath(p, ..)
            if p.elements().iter().any(|e| matches!(e, PathEl::ClosePath))
    )));
}

#[test]
fn paint_zero_bounds_emits_nothing() {
    // [engine]
    let mut icon = MorphIcon::icon(demo::MENU).unwrap();
    laid_out(&mut icon, Rect::new(5.0, 5.0, 0.0, 0.0));
    assert!(painted(&icon).commands.is_empty());
}

// ===========================================================================
// 3. Private-engine unit tests (ADR-0041 engine matrix) — all `// [engine]`.
// ===========================================================================

#[test]
fn engine_lowers_lines_to_cubic_subpaths() {
    // [engine]
    let cubics = d_to_cubics(demo::MENU).unwrap();
    assert_eq!(cubics.len(), 3);
    assert!(cubics.iter().all(|c| !c.closed && c.seg_count() == 1));
    // `z` marks the subpath closed.
    let play = d_to_cubics(demo::PLAY).unwrap();
    assert_eq!(play.len(), 1);
    assert!(play[0].closed);
}

#[test]
fn engine_bezpath_lowering_splits_on_moveto() {
    // [engine]
    let mut p = kurbo::BezPath::new();
    p.move_to((0.0, 0.0));
    p.line_to((10.0, 0.0));
    p.move_to((0.0, 5.0));
    p.line_to((10.0, 5.0));
    let subs = bezpath_to_cubics(&p).unwrap();
    assert_eq!(subs.len(), 2);
    assert!(subs.iter().all(|s| s.seg_count() == 1 && !s.closed));
}

#[test]
fn engine_resamples_to_sample_n_per_subpath() {
    // [engine]
    let menu = d_to_sampled(demo::MENU).unwrap();
    assert_eq!(menu.len(), 3);
    for s in &menu {
        assert_eq!(s.pts.len(), SAMPLE_N * 2);
        assert!(!s.closed);
    }
    let play = d_to_sampled(demo::PLAY).unwrap();
    assert_eq!(play.len(), 1);
    assert!(play[0].closed);
    assert_eq!(play[0].pts.len(), SAMPLE_N * 2);
}

#[test]
fn engine_resample_anchors_sharp_corners() {
    // [engine] — a 90° corner (>22.5° tangent discontinuity, ADR-0041)
    // must land on an exact sample.
    let subs = d_to_sampled("M0 0 L12 0 L12 12").unwrap();
    let pts = sampled_points(&subs);
    assert!(
        pts.iter()
            .any(|&(x, y)| (x - 12.0).abs() < 1e-9 && y.abs() < 1e-9),
        "corner (12,0) missing from samples: {pts:?}"
    );
}

#[test]
fn engine_entry_points_reject_empty_input() {
    // [engine] — `resample` itself is a pure transform (empty in → empty
    // out); the `Empty` contract lives at the `d`/`BezPath` entry points
    // and at `build_plan`.
    assert!(resample(&[]).unwrap().is_empty());
    assert!(matches!(d_to_sampled(""), Err(MorphError::Empty)));
    assert!(matches!(d_to_sampled("M5 5"), Err(MorphError::Empty)));
    let a = d_to_sampled(demo::MENU).unwrap();
    assert!(matches!(build_plan(&[], &a), Err(MorphError::Empty)));
    assert!(matches!(build_plan(&a, &[]), Err(MorphError::Empty)));
}

#[test]
fn engine_plan_endpoints_exact() {
    // [engine] — interpolant is exact at t=0/1 (ADR-0041 §4); same-count
    // pair so the point multisets align one-to-one.
    let a = d_to_sampled(demo::CHEVRON_RIGHT).unwrap();
    let b = d_to_sampled(demo::CHEVRON_DOWN).unwrap();
    let plan = build_plan(&a, &b).unwrap();
    assert_pts_eq(&sampled_points(&plan.eval(0.0)), &sampled_points(&a), 1e-3);
    assert_pts_eq(&sampled_points(&plan.eval(1.0)), &sampled_points(&b), 1e-3);
}

#[test]
fn engine_plan_eval_is_deterministic() {
    // [engine] — arc-heavy corpus (kurbo slicing divergence coverage).
    let a = d_to_sampled(demo::EYE_OPEN).unwrap();
    let b = d_to_sampled(demo::EYE_CLOSED).unwrap();
    let plan = build_plan(&a, &b).unwrap();
    assert_eq!(plan.eval(0.37), plan.eval(0.37));
}

#[test]
fn engine_plan_extrapolates_past_endpoints() {
    // [engine] — t<0 / t>1 extrapolates (spring overshoot); no clamp.
    let a = d_to_sampled(demo::MENU).unwrap();
    let b = d_to_sampled(demo::CLOSE).unwrap();
    let plan = build_plan(&a, &b).unwrap();
    let over = plan.eval(1.15);
    for s in &over {
        assert!(s.pts.iter().all(|v| v.is_finite()));
    }
    assert_ne!(over, plan.eval(1.0));
    assert_ne!(plan.eval(-0.1), plan.eval(0.0));
}

#[test]
fn engine_plan_handles_subpath_count_mismatch() {
    // [engine] — surjective matching ("cell division") 3→2 subpaths: the
    // plan must build and every evaluated frame stay finite and drawable.
    let a = d_to_sampled(demo::MENU).unwrap(); // 3 subpaths
    let b = d_to_sampled(demo::CLOSE).unwrap(); // 2 subpaths
    let plan = build_plan(&a, &b).unwrap();
    for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let frame = plan.eval(t);
        assert!(!frame.is_empty(), "eval({t}) produced no subpaths");
        for s in &frame {
            assert!(s.pts.len() >= 4);
            assert!(s.pts.iter().all(|v| v.is_finite()));
        }
    }
}

#[test]
fn engine_plan_identical_shapes_hold_geometry() {
    // [engine] — a→a: the congruent ("global hybrid") case must keep the
    // shape at every t rather than spinning parts independently.
    let a = d_to_sampled(demo::CHECK).unwrap();
    let plan = build_plan(&a, &a).unwrap();
    assert_pts_eq(&sampled_points(&plan.eval(0.5)), &sampled_points(&a), 1e-3);
}
