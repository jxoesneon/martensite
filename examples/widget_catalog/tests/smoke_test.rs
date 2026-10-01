//! Smoke tests for the Widget Catalog: every registered page carries
//! complete reference metadata, builds a live widget for its default
//! props, and produces a non-empty snippet; the rail matcher honors
//! canonical names, families, and cross-framework aliases; the
//! composed view instantiates cleanly.

use martensite::core::Widget;
use widget_catalog::page::PropValues;
use widget_catalog::{all_pages, CatalogView};

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
