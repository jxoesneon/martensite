//! v0.11 typography and accessibility conformance test vectors.
//!
//! These tests exercise the Loop 1 remediation requirements for
//! `martensite-text`: proper Unicode BiDi integration, UAX #14 line
//! breaking, vertical writing runs, installed-font fallback, and expanded
//! cache keys/results.

use martensite_text::bidi::{BidiDirection, BidiResolved};
use martensite_text::cache::{CachedShape, ShapeCacheKey, WritingModeBits};
use martensite_text::cascade::{
    FallbackKey, InstalledFontFallbackResolver, PlatformCascadeResolver, ScriptTag,
};
use martensite_text::font::FontManager;
use martensite_text::line_break::{BreakOpportunity, LineBreaker};
use martensite_text::shaping::{Shaper, ShapingOptions, TextMetrics};
use martensite_text::vertical::{
    apply_vertical_features, collect_vertical_runs, VerticalOrientation, WritingMode,
};

use cosmic_text::{Attrs, Metrics};

#[test]
fn bidi_logical_runs_are_extracted() {
    let text = "Hello مرحبا World";
    let resolved = BidiResolved::new(text, BidiDirection::Auto);
    let runs = resolved.logical_runs();
    assert!(
        runs.iter().any(|r| r.is_rtl()),
        "mixed text should contain at least one RTL logical run"
    );
    assert!(
        runs.iter().any(|r| !r.is_rtl()),
        "mixed text should contain LTR logical runs"
    );
}

#[test]
fn bidi_visual_runs_are_reordered() {
    // Three logical runs: LTR "Hello ", RTL "مرحبا", LTR " World".
    // UAX #9 rule L2 reverses the level-1 (RTL) run; since it is a single
    // run the *run* order is unchanged at level granularity, so use a
    // case where run order provably changes: RTL followed by LTR inside
    // an RTL paragraph, plus embedded level-2 digits that flip position.
    let text = "אבג 123 דהו";
    let resolved = BidiResolved::new(text, BidiDirection::Rtl);
    let visual = resolved.visual_runs();
    let logical = resolved.logical_runs();
    assert!(
        logical.len() >= 3,
        "expected at least 3 logical runs, got {}",
        logical.len()
    );
    assert_eq!(visual.len(), logical.len());

    // Under L2, sequences of runs at level >= the lowest odd level are
    // reversed. In an RTL paragraph, the embedded LTR digits (level 2)
    // swap position relative to their neighboring RTL runs: the visual
    // sequence of logical_order indices must NOT be the identity
    // permutation.
    let visual_logical: Vec<usize> = visual.iter().map(|r| r.logical_order).collect();
    let identity: Vec<usize> = (0..logical.len()).collect();
    assert_ne!(
        visual_logical, identity,
        "L2 reordering must change run order for RTL paragraph with embedded LTR digits"
    );

    // visual_order must be the exact display position: a permutation of
    // 0..n, and each run's visual_order equals its index in the visual vec.
    for (i, run) in visual.iter().enumerate() {
        assert_eq!(run.visual_order, i, "visual_order must equal display index");
    }
    let mut sorted: Vec<usize> = visual.iter().map(|r| r.visual_order).collect();
    sorted.sort_unstable();
    assert_eq!(sorted, identity);

    // Concretely: in an RTL paragraph every run has level >= 1, so the
    // level-1 pass reverses the entire run sequence: the first logical
    // run is displayed last and the last logical run is displayed first.
    let n = logical.len();
    assert_eq!(
        visual[0].logical_order,
        n - 1,
        "first visual run should be the last logical run in an RTL paragraph"
    );
    assert_eq!(
        visual[n - 1].logical_order,
        0,
        "last visual run should be the first logical run in an RTL paragraph"
    );

    // And a pure-LTR paragraph must keep logical order.
    let resolved = BidiResolved::new("Hello World", BidiDirection::Ltr);
    let visual = resolved.visual_runs();
    let order: Vec<usize> = visual.iter().map(|r| r.logical_order).collect();
    assert_eq!(order, (0..order.len()).collect::<Vec<_>>());
}

#[test]
fn bidi_logical_visual_roundtrip() {
    let text = "abc שלום 123";
    let resolved = BidiResolved::new(text, BidiDirection::Auto);
    let char_count = text.chars().count();
    for logical in 0..char_count {
        let visual = resolved
            .logical_to_visual(logical)
            .expect("logical index should map to visual");
        let back = resolved
            .visual_to_logical(visual)
            .expect("visual index should map back to logical");
        assert_eq!(back, logical);
    }
}

#[test]
fn bidi_mirror_map_for_rtl_parens() {
    let text = "(hello)";
    let resolved = BidiResolved::new(text, BidiDirection::Rtl);
    let map = resolved.mirror_map();
    let open = text.find('(').unwrap();
    let close = text.find(')').unwrap();
    assert_eq!(map.mirror_at(open), Some(')'));
    assert_eq!(map.mirror_at(close), Some('('));
}

#[test]
fn uax14_opportunities_cover_all_bytes() {
    let text = "Hello, 世界!";
    let ops = LineBreaker::opportunities(text);
    assert_eq!(ops.len(), text.len() + 1);
}

#[test]
fn uax14_no_break_inside_cjk_word() {
    let text = "日本語";
    let ops = LineBreaker::opportunities(text);
    for (i, op) in ops.iter().enumerate().take(text.len()).skip(1) {
        assert!(
            !matches!(op, BreakOpportunity::Allowed | BreakOpportunity::Mandatory),
            "unexpected break opportunity at byte index {i} inside CJK word"
        );
    }
}

#[test]
fn uax14_kinsoku_start_prohibits_closing_punctuation() {
    assert!(LineBreaker::is_kinsoku_start(')'));
    assert!(LineBreaker::is_kinsoku_start('。'));
    assert!(LineBreaker::is_kinsoku_start('、'));
    assert!(!LineBreaker::is_kinsoku_start('A'));
}

#[test]
fn uax14_kinsoku_end_prohibits_opening_punctuation() {
    assert!(LineBreaker::is_kinsoku_end('('));
    assert!(LineBreaker::is_kinsoku_end('「'));
    assert!(LineBreaker::is_kinsoku_end('$'));
    assert!(!LineBreaker::is_kinsoku_end('Z'));
}

#[test]
fn vertical_runs_split_by_orientation() {
    let text = "漢字A";
    let runs = collect_vertical_runs(text);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].orientation, VerticalOrientation::Upright);
    assert_eq!(runs[1].orientation, VerticalOrientation::Rotated);
}

#[test]
fn vertical_features_are_enabled() {
    let attrs = apply_vertical_features(Attrs::new());
    let tags: Vec<String> = attrs
        .font_features
        .features
        .iter()
        .map(|f| String::from_utf8(f.tag.as_bytes().to_vec()).unwrap())
        .collect();
    assert!(tags.iter().any(|t| t == "vert"));
    assert!(tags.iter().any(|t| t == "vrt2"));
    assert!(tags.iter().any(|t| t == "vkrn"));
}

#[test]
fn cache_key_includes_writing_mode_and_direction() {
    let key = ShapeCacheKey::with_options(
        martensite_text::font::FontId::dummy(),
        16.0,
        "Hello",
        None,
        "",
        0.0,
        BidiDirection::Rtl,
        WritingMode::VerticalRl,
        &[],
    );
    assert_eq!(key.writing_mode, WritingModeBits::VerticalRl);
    // Direction must be represented distinctly.
    let key_ltr = ShapeCacheKey::with_options(
        martensite_text::font::FontId::dummy(),
        16.0,
        "Hello",
        None,
        "",
        0.0,
        BidiDirection::Ltr,
        WritingMode::HorizontalTb,
        &[],
    );
    assert_ne!(key, key_ltr);
}

#[test]
fn cache_key_includes_fallback_hash() {
    let key_a = ShapeCacheKey::with_options(
        martensite_text::font::FontId::dummy(),
        16.0,
        "Hello",
        None,
        "",
        0.0,
        BidiDirection::Ltr,
        WritingMode::HorizontalTb,
        &["Noto Sans"],
    );
    let key_b = ShapeCacheKey::with_options(
        martensite_text::font::FontId::dummy(),
        16.0,
        "Hello",
        None,
        "",
        0.0,
        BidiDirection::Ltr,
        WritingMode::HorizontalTb,
        &["Arial"],
    );
    assert_ne!(key_a, key_b);
}

#[test]
fn cached_shape_can_store_v0_11_metadata() {
    let shape = CachedShape::with_metadata(
        Vec::new(),
        TextMetrics::default(),
        1,
        vec![2, 1, 0],
        vec!["Noto Sans".to_string()],
        WritingMode::VerticalRl,
    );
    assert_eq!(shape.base_bidi_level, 1);
    assert_eq!(shape.visual_run_order, vec![2, 1, 0]);
    assert_eq!(shape.fallback_chain, vec!["Noto Sans".to_string()]);
    assert_eq!(shape.writing_mode, WritingMode::VerticalRl);
}

#[test]
fn installed_fallback_resolver_keeps_primary() {
    let manager = FontManager::with_fonts(std::iter::empty());
    let resolver = InstalledFontFallbackResolver::new(manager.system());
    let chain = resolver.resolve_for_text("abc", "Primary");
    assert_eq!(chain[0], "Primary");
}

#[test]
fn installed_fallback_resolver_includes_platform_script_families() {
    let manager = FontManager::with_fonts(std::iter::empty());
    let resolver = InstalledFontFallbackResolver::new(manager.system());
    let chain = resolver.resolve_for_text("漢", "Missing");
    let expected = PlatformCascadeResolver::platform_fallbacks_for_script(ScriptTag::Cjk);
    assert!(
        chain.iter().any(|f| expected.contains(&f.as_str())),
        "CJK chain should include at least one platform fallback family, got {:?}",
        chain
    );
}

#[test]
fn fallback_key_normalizes_locale() {
    let key = FallbackKey::new(ScriptTag::Cjk, "ja-JP");
    assert_eq!(key.locale, "ja-jp");
}

#[test]
fn shaper_base_bidi_level_reflects_options() {
    let mut manager = FontManager::with_fonts(std::iter::empty());
    let mut shaper = Shaper::new_empty(Metrics::new(16.0, 20.0));
    let options = ShapingOptions {
        direction: BidiDirection::Rtl,
        writing_mode: WritingMode::HorizontalTb,
        enable_vertical_features: false,
        fallback_key: None,
    };
    shaper.shape_with_options(manager.system_mut(), "مرحبا", &Attrs::new(), &options);
    assert_eq!(shaper.base_bidi_level(), Some(1));
}
#[test]
fn shaper_fallback_chain_applies_to_cjk_spans() {
    use martensite_text::Family;

    let mut manager = FontManager::new();
    let mut shaper = Shaper::new_empty(Metrics::new(16.0, 20.0));
    let attrs = Attrs::new().family(Family::SansSerif);
    let options = ShapingOptions::default();
    shaper.shape_with_options(manager.system_mut(), "漢字", &attrs, &options);

    // The resolved chain must be non-empty and begin with the requested
    // family, followed by platform CJK candidates.
    let chain = shaper.fallback_chain();
    assert!(
        chain.len() >= 2,
        "resolved fallback chain for CJK text should contain primary + candidates, got {chain:?}"
    );
    assert_eq!(chain[0], "sans-serif");
    let expected = PlatformCascadeResolver::platform_fallbacks_for_script(ScriptTag::Cjk);
    assert!(
        chain.iter().any(|f| expected.contains(&f.as_str())),
        "chain should contain platform CJK families, got {chain:?}"
    );

    // The chain must actually be wired into the buffer: the families
    // applied to the text spans must all come from the resolved chain.
    let applied = shaper.applied_families();
    assert!(
        !applied.is_empty(),
        "applied_families should record the family used per span"
    );
    for name in applied {
        assert!(
            chain.iter().any(|f| f.eq_ignore_ascii_case(name)),
            "applied family {name:?} is not in the resolved chain {chain:?}"
        );
    }
}

#[test]
fn shaper_fallback_chain_applies_per_script_faces() {
    use martensite_text::Family;

    // Mixed Latin + CJK: the CJK run should get a CJK-capable family from
    // the chain when one is installed, while coverage checks remain
    // per-script (a single face is not required to cover everything).
    let mut manager = FontManager::new();
    let mut shaper = Shaper::new_empty(Metrics::new(16.0, 20.0));
    let attrs = Attrs::new().family(Family::SansSerif);
    let options = ShapingOptions::default();
    shaper.shape_with_options(manager.system_mut(), "Hello 漢字", &attrs, &options);

    let chain = shaper.fallback_chain();
    assert!(chain.len() >= 2, "mixed-script chain too small: {chain:?}");
    let applied = shaper.applied_families();
    assert!(!applied.is_empty());
    for name in applied {
        assert!(
            chain.iter().any(|f| f.eq_ignore_ascii_case(name)),
            "applied family {name:?} is not in the resolved chain {chain:?}"
        );
    }
}
