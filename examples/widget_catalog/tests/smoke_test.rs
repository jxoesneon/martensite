//! Smoke tests for the Widget Catalog: every registered page carries
//! complete reference metadata, builds a live widget for its default
//! props, and produces a non-empty snippet; the rail matcher honors
//! canonical names, families, and cross-framework aliases; the
//! composed view instantiates cleanly.

use glam::Vec2;
use martensite::core::paint::{install_ambient_measurer, TextShaper};
use martensite::core::{HotNode, LayoutConstraints, LayoutContext, PaintList, Rect, Widget};
use std::sync::Arc;
use widget_catalog::page::PropValues;
use widget_catalog::{all_pages, CatalogView};

/// Fixed-width stub shaper — enough for real measure passes.
struct FixedShaper;
impl TextShaper for FixedShaper {
    fn paint_shaped_text(&self, _: &mut PaintList, _: kurbo::Point, _: &str, _: f32, _: [u8; 4]) {}
    fn measure_text(&self, text: &str, size_px: f32) -> Option<f32> {
        Some(text.len() as f32 * size_px * 0.5)
    }
}

#[test]
fn catalog_instantiates_all_pages_cleanly() {
    let pages = all_pages();
    assert!(!pages.is_empty(), "catalog must contain pages");

    for page in &pages {
        let meta = page.meta();
        assert!(!meta.name.is_empty(), "page name cannot be empty");
        assert!(!meta.family.is_empty(), "family empty for {}", meta.name);
        assert!(
            !meta.description.is_empty(),
            "description empty for {}",
            meta.name
        );
        assert!(!meta.role.is_empty(), "role empty for {}", meta.name);
        assert!(!meta.aliases.is_empty(), "aliases empty for {}", meta.name);

        // Default props must build a widget and a snippet.
        let props = PropValues::from_specs(page.props());
        let widget = page.build(&props);
        let _ = widget.child_count();
        let snippet = page.snippet(&props);
        assert!(
            snippet.contains(meta.name.split_whitespace().next().unwrap_or(meta.name))
                || !snippet.is_empty(),
            "snippet empty for {}",
            meta.name
        );
        assert!(!snippet.is_empty(), "snippet empty for {}", meta.name);
    }
}

/// Every page's widget must measure and lay out without panic at the
/// three canonical catalog widths (compact / default / wide).
#[test]
fn every_page_measures_and_lays_out_at_three_widths() {
    let _guard = install_ambient_measurer(Arc::new(FixedShaper));
    for page in &all_pages() {
        let props = PropValues::from_specs(page.props());
        for width in [240.0_f32, 600.0, 1200.0] {
            let mut widget = page.build(&props);
            let mut hot = HotNode::default();
            let mut cx = LayoutContext {
                hot: &mut hot,
                scale: 1.0,
            };
            let size = widget.measure(
                &mut cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: Vec2::new(width, 800.0),
                },
            );
            assert!(
                size.x.is_finite() && size.y.is_finite(),
                "{} measured non-finite at {width}px",
                page.meta().name
            );
            widget.layout(&mut cx, Rect::new(0.0, 0.0, width.max(size.x), 800.0));
        }
    }
}

#[test]
fn rail_matcher_finds_names_families_and_aliases() {
    use widget_catalog::view::matches_query;

    let pages = all_pages();
    let by_name = |name: &str| pages.iter().find(|p| p.meta().name == name).unwrap();

    // Canonical names.
    for p in &pages {
        let meta = p.meta();
        assert!(
            matches_query(&meta, &meta.name.to_lowercase()),
            "name query failed for {}",
            meta.name
        );
    }

    // Family names match their members.
    let button = by_name("Button").meta();
    assert!(matches_query(&button, "controls"));

    // Cross-framework aliases.
    assert!(matches_query(&button, "qpushbutton"));
    assert!(matches_query(&button, "gtkbutton"));
    assert!(matches_query(&button, "swiftui"));
    assert!(matches_query(&by_name("TextInput").meta(), "qlineedit"));
    assert!(matches_query(&by_name("Slider").meta(), "gtkscale"));

    // No match.
    assert!(!matches_query(&button, "zzzz-nothing"));
}

#[test]
fn catalog_view_instantiates() {
    let view = CatalogView::new(all_pages());
    assert_eq!(view.selected_name(), "Button");
    assert!(view.child_count() > 5, "view must expose panel children");
}
