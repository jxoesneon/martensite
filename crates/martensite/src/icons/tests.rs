//! `icons` test suite — pack-data well-formedness, the engine
//! contract on every builtin `d` (parse + declared-pair morphs), the
//! `IconSet` resolution chain (overlay order, builtin fallback,
//! pairs), and the `MorphIcon::named`/`set_named`/`morph_to_named`
//! surface including clean errors on unknown names and bad data.

use super::*;
use crate::widgets::{MorphError, MorphIcon};
use martensite_core::widget::{LayoutContext, PaintContext, Widget};
use martensite_core::{HotNode, PaintCommand, PaintList, Rect};
use martensite_motion::SpringConfig;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fast() -> SpringConfig {
    SpringConfig::new(1.0, 500.0, 60.0).unwrap()
}

fn bounds_24() -> Rect {
    Rect::new(0.0, 0.0, 24.0, 24.0)
}

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

fn painted(icon: &MorphIcon) -> PaintList {
    let theme = martensite_theme::Theme::new("test");
    let mut list = PaintList::new();
    icon.paint(&mut PaintContext {
        list: &mut list,
        bounds: bounds_24(),
        scale: 1.0,
        theme: &theme,
        text_painter: None,
    });
    list
}

fn sorted(mut pts: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    pts.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
    pts
}

/// Sorted (x, y) point multiset over every StrokePath in the list —
/// order-insensitive geometry fingerprint.
fn stroke_points(list: &PaintList) -> Vec<(f64, f64)> {
    let mut pts = Vec::new();
    for c in &list.commands {
        if let PaintCommand::StrokePath(path, ..) = c {
            for el in path.elements() {
                match *el {
                    kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => pts.push((p.x, p.y)),
                    kurbo::PathEl::QuadTo(a, b) => {
                        pts.push((a.x, a.y));
                        pts.push((b.x, b.y));
                    }
                    kurbo::PathEl::CurveTo(a, b, d) => {
                        pts.push((a.x, a.y));
                        pts.push((b.x, b.y));
                        pts.push((d.x, d.y));
                    }
                    kurbo::PathEl::ClosePath => {}
                }
            }
        }
    }
    sorted(pts)
}

fn settle(icon: &mut MorphIcon) -> usize {
    let mut frames = 0usize;
    while icon.tick(Duration::from_millis(16)) {
        frames += 1;
        assert!(frames < 4096, "spring did not settle in 4096 frames");
    }
    frames
}

fn overlay(entries: &[(&'static str, &'static str)]) -> IconPack {
    IconPack::from_entries("overlay", entries.iter().copied())
}

// ---------------------------------------------------------------------------
// Pack data well-formedness
// ---------------------------------------------------------------------------

#[test]
fn builtin_pack_is_well_formed() {
    assert_eq!(BUILTIN.name(), "martensite");
    assert!(std::ptr::eq(builtin(), &*BUILTIN));
    assert!(BUILTIN.len() >= 40, "pack size {}", BUILTIN.len());
    // Unique names, all in the `namespace.kebab-name` scheme over the
    // twenty documented namespaces.
    let mut names: Vec<&str> = BUILTIN.names().collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), BUILTIN.len(), "duplicate icon names");
    for e in BUILTIN.entries() {
        let (ns, local) = e.name.split_once('.').expect("qualified name");
        assert!(
            matches!(
                ns,
                "nav"
                    | "arrow"
                    | "layout"
                    | "media"
                    | "device"
                    | "status"
                    | "security"
                    | "system"
                    | "data"
                    | "file"
                    | "edit"
                    | "text"
                    | "comms"
                    | "people"
                    | "time"
                    | "weather"
                    | "map"
                    | "tool"
                    | "dev"
                    | "misc"
            ),
            "unknown namespace {ns:?} in {:?}",
            e.name
        );
        assert!(
            !local.is_empty()
                && local
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-' || c.is_ascii_digit()),
            "bad kebab name {:?}",
            e.name
        );
        assert!(!e.d.is_empty(), "empty d for {:?}", e.name);
        // Lookup is exact-match only — the qualified form is the name.
        assert_eq!(BUILTIN.lookup(e.name.as_ref()), Some(e.d.as_ref()));
    }
}

#[test]
fn pair_endpoints_resolve_and_are_unique() {
    for p in BUILTIN.pairs() {
        assert!(
            BUILTIN.contains(p.name.as_ref()),
            "pair endpoint {:?} missing",
            p.name
        );
        assert!(
            BUILTIN.contains(p.alternate.as_ref()),
            "pair endpoint {:?} missing",
            p.alternate
        );
        assert_ne!(p.name, p.alternate, "self-pair {:?}", p.name);
    }
}

// ---------------------------------------------------------------------------
// Engine contract: every icon parses, every pair morphs
// ---------------------------------------------------------------------------

#[test]
fn every_builtin_icon_parses() {
    for e in BUILTIN.entries() {
        assert!(
            MorphIcon::icon(e.d.as_ref()).is_ok(),
            "{:?} failed to parse: {:?}",
            e.name,
            e.d
        );
    }
}

#[test]
fn every_declared_pair_morphs() {
    let set = IconSet::new();
    for p in BUILTIN.pairs() {
        let (a, b) = set
            .resolve_pair(p.name.as_ref())
            .expect("pair must resolve");
        let mut icon = MorphIcon::icon(a).unwrap();
        icon.morph_to(b, fast())
            .unwrap_or_else(|e| panic!("{} -> {}: {e:?}", p.name, p.alternate));
        assert!(
            icon.is_animating(),
            "{} -> {} produced no flight",
            p.name,
            p.alternate
        );
        assert!(icon.progress() < 0.5);
        assert!(settle(&mut icon) >= 1, "spring never ran");
        assert!(!icon.is_animating());

        // The relation is symmetric: reverse direction morphs too.
        let mut back = MorphIcon::icon(b).unwrap();
        back.morph_to(a, fast()).unwrap();
        assert!(back.is_animating(), "reverse pair produced no flight");
        assert!(settle(&mut back) >= 1);
    }
}

#[test]
fn builtin_icon_geometry_stays_on_canvas() {
    // DESIGN.md contract: centered 2px strokes must keep path
    // coordinates inside [0.5, 23.5] so the stroke never clips the
    // 24px canvas, and subpaths stay under the engine's cap (24).
    for e in BUILTIN.entries() {
        let path = kurbo::BezPath::from_svg(e.d.as_ref())
            .unwrap_or_else(|err| panic!("{}: parse failed: {err}", e.name));
        let mut subs = 0usize;
        let mut over = Vec::new();
        for el in path.iter() {
            if matches!(el, kurbo::PathEl::MoveTo(_)) {
                subs += 1;
            }
            let pts: Vec<kurbo::Point> = match el {
                kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => vec![p],
                kurbo::PathEl::QuadTo(a, b) => vec![a, b],
                kurbo::PathEl::CurveTo(a, b, c) => vec![a, b, c],
                kurbo::PathEl::ClosePath => vec![],
            };
            for p in pts {
                if !(0.5..=23.5).contains(&p.x) || !(0.5..=23.5).contains(&p.y) {
                    over.push(p);
                }
            }
        }
        assert!(subs <= 24, "{}: {subs} subpaths exceed engine cap", e.name);
        assert!(
            over.is_empty(),
            "{}: coordinates outside [0.5, 23.5]: {over:?}",
            e.name
        );
    }
}

#[test]
fn selected_pip_variants_parse_and_morph() {
    // The pip convention produces valid morph input for every icon it
    // is applied to — the extra subpath dissolves against the base
    // shape both ways. Covers the differing-subpath-count morphs the
    // dashboard drives (base ↔ base+pip on selection edges).
    for d in [
        builtin::data::DATA_GRID,
        builtin::data::DATA_ACTIVITY,
        builtin::data::DATA_FLATLINE,
        builtin::edit::EDIT_PEN,
        builtin::media::MEDIA_PLAY,
        builtin::media::MEDIA_PAUSE,
    ] {
        let pip = builtin::selected_pip(d);
        let mut icon = MorphIcon::icon(&pip).unwrap_or_else(|e| panic!("{d}+pip: {e:?}"));
        icon.morph_to(d, fast())
            .unwrap_or_else(|e| panic!("{d}+pip -> {d}: {e:?}"));
        assert!(icon.is_animating(), "{d}: pip drop produced no flight");
        assert!(settle(&mut icon) >= 1);
        icon.morph_to(&pip, fast())
            .unwrap_or_else(|e| panic!("{d} -> {d}+pip: {e:?}"));
        assert!(icon.is_animating(), "{d}: pip add produced no flight");
        assert!(settle(&mut icon) >= 1);
    }
}

#[test]
fn pair_morph_midflight_geometry_differs() {
    // Intermediate geometry, not just animation state: the seeked
    // mid-frame is a real blend of the endpoints.
    for (a, b) in [
        (builtin::nav::NAV_MENU, builtin::status::STATUS_CLOSE),
        (builtin::media::MEDIA_PLAY, builtin::media::MEDIA_PAUSE),
    ] {
        let mut icon = MorphIcon::icon(a).unwrap();
        laid_out(&mut icon, bounds_24());
        icon.morph_to(b, fast()).unwrap();
        icon.seek(0.0);
        let at0 = stroke_points(&painted(&icon));
        icon.seek(0.5);
        let mid = stroke_points(&painted(&icon));
        icon.seek(1.0);
        let at1 = stroke_points(&painted(&icon));
        assert_ne!(mid, at0, "mid-flight equals t=0 for {a}");
        assert_ne!(mid, at1, "mid-flight equals t=1 for {a}");
    }
}

// ---------------------------------------------------------------------------
// IconPack surface
// ---------------------------------------------------------------------------

#[test]
fn pack_lookup_is_exact_match_only() {
    assert_eq!(BUILTIN.lookup("nav.menu"), Some(builtin::nav::NAV_MENU));
    // No partial/case/namespace-normalized matching — misses are None.
    for miss in ["menu", "NAV.MENU", "Nav.menu", "nav.", ".menu", "nav.menu "] {
        assert_eq!(BUILTIN.lookup(miss), None, "{miss:?} should miss");
    }
}

#[test]
fn pack_from_entries_accepts_owned_and_borrowed() {
    let owned = IconPack::from_entries(
        "owned",
        vec![(String::from("app.star"), String::from("M0 0L1 1"))],
    );
    assert_eq!(owned.lookup("app.star"), Some("M0 0L1 1"));

    let borrowed = IconPack::from_entries("b", [("x.y", "M1 1L2 2")]);
    assert_eq!(borrowed.lookup("x.y"), Some("M1 1L2 2"));
}

#[test]
fn pack_with_pairs_declares_symmetric_alternates() {
    let pack =
        overlay(&[("app.a", "M0 0L1 1"), ("app.b", "M2 2L3 3")]).with_pairs([("app.a", "app.b")]);
    assert_eq!(pack.paired("app.a"), Some("app.b"));
    assert_eq!(pack.paired("app.b"), Some("app.a"));
    assert_eq!(pack.paired("app.c"), None);
}

#[test]
fn pack_iterators_report_order_and_size() {
    let names: Vec<&str> = BUILTIN.names().collect();
    assert_eq!(names.len(), BUILTIN.len());
    assert_eq!(names[0], "nav.menu");
    assert!(IconPack::new("e", &[]).names().next().is_none());
}

// ---------------------------------------------------------------------------
// IconSet resolution chain
// ---------------------------------------------------------------------------

#[test]
fn resolve_falls_back_to_builtin() {
    let set = IconSet::new();
    assert!(set.packs().is_empty());
    assert_eq!(
        set.resolve("status.lock"),
        Some(builtin::status::STATUS_LOCK)
    );
    assert_eq!(set.resolve("bogus"), None);
    assert!(!set.contains("bogus"));
    assert!(IconSet::default().contains("status.lock"));
}

#[test]
fn overlay_shadows_builtin_and_extends() {
    let pack = overlay(&[("nav.menu", "M1 1h2M1 5h2"), ("app.logo", "M2 2l4 4-4 4")]);
    let set = IconSet::new().with_pack(pack);
    assert_eq!(set.resolve("nav.menu"), Some("M1 1h2M1 5h2"));
    assert_eq!(set.resolve("app.logo"), Some("M2 2l4 4-4 4"));
    // Untouched names still reach the builtin tail.
    assert_eq!(
        set.resolve("status.lock"),
        Some(builtin::status::STATUS_LOCK)
    );
}

#[test]
fn earlier_overlay_wins_over_later() {
    let first = overlay(&[("x", "M0 0L1 1")]);
    let second = IconPack::from_entries("second", [("x", "M2 2L3 3")]);
    let mut set = IconSet::new().with_pack(first);
    set.push(second);
    assert_eq!(set.resolve("x"), Some("M0 0L1 1"));
    assert_eq!(set.packs()[1].name(), "second");
}

#[test]
fn paired_resolves_through_the_chain() {
    let set = IconSet::new();
    assert_eq!(set.paired("status.lock"), Some("status.lock-open"));
    assert_eq!(set.paired("status.lock-open"), Some("status.lock"));
    assert_eq!(set.paired("nav.home"), None);
    assert_eq!(set.paired("bogus"), None);

    // An overlay pack can declare its own pair for its own names.
    let pack = overlay(&[("app.on", "M0 0L1 1"), ("app.off", "M1 1L2 2")])
        .with_pairs([("app.on", "app.off")]);
    let set = IconSet::new().with_pack(pack);
    let (a, b) = set.resolve_pair("app.on").unwrap();
    assert_eq!((a, b), ("M0 0L1 1", "M1 1L2 2"));
    assert_eq!(set.resolve_pair("app.off"), Some(("M1 1L2 2", "M0 0L1 1")));
    assert_eq!(set.resolve_pair("nav.home"), None);
}

#[test]
fn overlay_pairs_take_precedence() {
    // The first pack that declares a pair for `name` decides — an
    // overlay can repoint a builtin alternate.
    let pack = overlay(&[("nav.menu", "M0 0L1 1"), ("nav.menu.alt", "M2 2L3 3")])
        .with_pairs([("nav.menu", "nav.menu.alt")]);
    let set = IconSet::new().with_pack(pack);
    assert_eq!(set.paired("nav.menu"), Some("nav.menu.alt"));
}

#[test]
fn names_lists_chain_in_order_deduped() {
    let pack = overlay(&[("nav.menu", "M0 0L1 1"), ("app.x", "M2 2L3 3")]);
    let set = IconSet::new().with_pack(pack);
    let names = set.names();
    // Overlay entries lead; the shadowed "nav.menu" appears once.
    assert_eq!(names[0], "nav.menu");
    assert_eq!(names.iter().filter(|&&n| n == "nav.menu").count(), 1);
    assert!(names.contains(&"app.x"));
    assert!(names.contains(&"misc.gauge"));
    assert_eq!(names.len(), BUILTIN.len() + 1);
}

// ---------------------------------------------------------------------------
// MorphIcon::named surface
// ---------------------------------------------------------------------------

#[test]
fn named_builds_builtin_icon() {
    // The resolved widget paints the builtin shape — compare via the
    // canonical `icon(d)` construction.
    let mut named = MorphIcon::named("nav.menu").unwrap();
    let mut direct = MorphIcon::icon(builtin::nav::NAV_MENU).unwrap();
    laid_out(&mut named, bounds_24());
    laid_out(&mut direct, bounds_24());
    assert_eq!(
        stroke_points(&painted(&named)),
        stroke_points(&painted(&direct))
    );
    assert!(MorphIcon::named("status.lock").is_ok());
}

#[test]
fn named_unknown_name_is_unknown_error() {
    for bad in ["bogus", "nav.nope", "", "menu"] {
        match MorphIcon::named(bad) {
            Err(IconError::Unknown { name }) => assert_eq!(name, bad),
            Err(e) => panic!("{bad:?}: expected Unknown, got {e:?}"),
            Ok(_) => panic!("{bad:?}: expected Err, got Ok"),
        }
    }
}

#[test]
fn named_in_resolves_overlay_first() {
    let pack = overlay(&[("nav.menu", "M1 1h20M1 20h20")]);
    let set = IconSet::new().with_pack(pack);
    let mut named = MorphIcon::named_in("nav.menu", &set).unwrap();
    let mut direct = MorphIcon::icon("M1 1h20M1 20h20").unwrap();
    laid_out(&mut named, bounds_24());
    laid_out(&mut direct, bounds_24());
    assert_eq!(
        stroke_points(&painted(&named)),
        stroke_points(&painted(&direct))
    );
}

#[test]
fn named_in_bad_overlay_path_is_invalid_error() {
    let pack = overlay(&[("app.broken", "zzz")]);
    let set = IconSet::new().with_pack(pack);
    match MorphIcon::named_in("app.broken", &set) {
        Err(IconError::Invalid { name, source }) => {
            assert_eq!(name, "app.broken");
            assert!(matches!(source, MorphError::Parse(_)));
        }
        Err(e) => panic!("expected Err(Invalid), got Err({e:?})"),
        Ok(_) => panic!("expected Err(Invalid), got Ok"),
    }
}

#[test]
fn set_named_and_morph_to_named_drive_state() {
    let set = IconSet::new();
    let mut icon = MorphIcon::named("status.lock").unwrap();
    // set_named jumps — no flight.
    icon.set_named("status.lock-open", &set).unwrap();
    assert!(!icon.is_animating());
    // morph_to_named flies the declared pair.
    icon.morph_to_named("status.lock", &set, fast()).unwrap();
    assert!(icon.is_animating());
    assert!(settle(&mut icon) >= 1);
    // Misses error without disturbing state.
    let before = stroke_points(&{
        laid_out(&mut icon, bounds_24());
        painted(&icon)
    });
    assert!(matches!(
        icon.morph_to_named("status.nope", &set, fast()),
        Err(IconError::Unknown { .. })
    ));
    assert!(matches!(
        icon.set_named("status.nope", &set),
        Err(IconError::Unknown { .. })
    ));
    assert_eq!(stroke_points(&painted(&icon)), before);
}

#[test]
fn icon_error_display_and_source() {
    let unk = IconError::Unknown {
        name: "nav.nope".into(),
    };
    assert_eq!(unk.to_string(), "unknown icon name: nav.nope");
    assert!(std::error::Error::source(&unk).is_none());

    let inv = IconError::Invalid {
        name: "x".into(),
        source: MorphError::Empty,
    };
    assert!(inv.to_string().contains("\"x\""));
    assert!(std::error::Error::source(&inv).is_some());
    fn needs_std_error<E: std::error::Error>(_: &E) {}
    needs_std_error(&unk);
}
