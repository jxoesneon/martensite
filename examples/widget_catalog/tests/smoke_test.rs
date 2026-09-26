//! Smoke tests for Martensite Widget Catalog instantiation, states, and cross-framework search indexing.

use widget_catalog::{catalog_entries, CatalogModel, WidgetFamily, WidgetStateKind};

#[test]
fn catalog_instantiates_all_required_widgets_and_states() {
    let entries = catalog_entries();
    assert!(!entries.is_empty(), "Catalog must contain entries");

    // Required widget families
    let families: std::collections::HashSet<_> = entries.iter().map(|e| e.family).collect();
    assert!(
        families.contains(&WidgetFamily::Controls),
        "Must include Controls family"
    );
    assert!(
        families.contains(&WidgetFamily::Containers),
        "Must include Containers family"
    );
    assert!(
        families.contains(&WidgetFamily::Data),
        "Must include Data family"
    );
    assert!(
        families.contains(&WidgetFamily::Overlays),
        "Must include Overlays family"
    );
    assert!(
        families.contains(&WidgetFamily::Navigation),
        "Must include Navigation family"
    );

    // Required specific widgets
    let names: std::collections::HashSet<_> = entries.iter().map(|e| e.name).collect();
    let required = [
        "Button",
        "Slider",
        "Toggle",
        "TextInput",
        "Checkbox",
        "Flex",
        "Stack",
        "Container",
        "ScrollView",
        "DataGrid",
        "Table",
        "List",
        "Tooltip",
        "Popover",
        "Popconfirm",
        "Tabs",
        "NavRail",
    ];

    for req in required {
        assert!(
            names.contains(req),
            "Widget catalog missing required widget: '{req}'"
        );
    }

    // Verify each entry has non-empty metadata and instantiates all 4 states cleanly
    for entry in &entries {
        assert!(!entry.name.is_empty(), "Entry name cannot be empty");
        assert!(
            !entry.description.is_empty(),
            "Description cannot be empty for {}",
            entry.name
        );
        assert!(
            !entry.code_snippet.is_empty(),
            "Snippet cannot be empty for {}",
            entry.name
        );
        assert!(
            !entry.aliases.is_empty(),
            "Aliases cannot be empty for {}",
            entry.name
        );

        for state in WidgetStateKind::ALL {
            let widget = (entry.instantiate)(state);
            let _ = widget.child_count();
        }
    }
}

#[test]
fn catalog_search_by_canonical_name_and_cross_framework_aliases() {
    let model = CatalogModel::new(catalog_entries());

    // 1. Search by canonical names
    for canonical in [
        "Button",
        "Slider",
        "Toggle",
        "TextInput",
        "Checkbox",
        "Flex",
        "Table",
        "Tabs",
    ] {
        model.set_search_query(canonical);
        let matches = model.filtered_entries();
        assert!(
            matches
                .iter()
                .any(|e| e.name.eq_ignore_ascii_case(canonical)),
            "Query '{canonical}' should find '{canonical}'"
        );
    }

    // 2. Search by cross-framework aliases:
    // Qt aliases
    let qt_checks = [
        ("QPushButton", "Button"),
        ("QSlider", "Slider"),
        ("QLineEdit", "TextInput"),
        ("QTableView", "DataGrid"),
        ("QTabWidget", "Tabs"),
        ("QListView", "List"),
        ("QToolTip", "Tooltip"),
    ];
    for (alias, expected) in qt_checks {
        model.set_search_query(alias);
        let matches = model.filtered_entries();
        assert!(
            matches.iter().any(|e| e.name == expected),
            "Qt alias '{alias}' should find '{expected}'"
        );
    }

    // GTK aliases
    let gtk_checks = [
        ("GtkButton", "Button"),
        ("GtkScale", "Slider"),
        ("GtkSwitch", "Toggle"),
        ("GtkEntry", "TextInput"),
        ("GtkBox", "Flex"),
        ("GtkNotebook", "Tabs"),
    ];
    for (alias, expected) in gtk_checks {
        model.set_search_query(alias);
        let matches = model.filtered_entries();
        assert!(
            matches.iter().any(|e| e.name == expected),
            "GTK alias '{alias}' should find '{expected}'"
        );
    }

    // SwiftUI aliases
    let swift_checks = [("HStack", "Flex"), ("ZStack", "Stack"), ("TabView", "Tabs")];
    for (alias, expected) in swift_checks {
        model.set_search_query(alias);
        let matches = model.filtered_entries();
        assert!(
            matches.iter().any(|e| e.name == expected),
            "SwiftUI alias '{alias}' should find '{expected}'"
        );
    }

    // React aliases
    let react_checks = [
        ("<button>", "Button"),
        ("<input type=\"range\">", "Slider"),
        ("<input type=\"text\">", "TextInput"),
        ("<table />", "DataGrid"),
        ("<Tabs />", "Tabs"),
    ];
    for (alias, expected) in react_checks {
        model.set_search_query(alias);
        let matches = model.filtered_entries();
        assert!(
            matches.iter().any(|e| e.name == expected),
            "React alias '{alias}' should find '{expected}'"
        );
    }
}

#[test]
fn catalog_view_model_builds_ui_cleanly() {
    let model = CatalogModel::new(catalog_entries());
    let _ui = widget_catalog::build_catalog_view(&model);
}
