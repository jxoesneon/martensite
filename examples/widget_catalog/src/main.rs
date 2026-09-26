//! Martensite Widget Catalog executable.

use widget_catalog::{catalog_entries, CatalogModel};

fn main() {
    let entries = catalog_entries();
    let model = CatalogModel::new(entries);
    println!(
        "Martensite Widget Catalog initialized with {} widgets across 5 families.",
        model.entries.len()
    );

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--filter" {
            if let Some(query) = args.next() {
                model.set_search_query(&query);
                let matches = model.filtered_entries();
                println!("Filter \"{}\" ({} matches):", query, matches.len());
                for m in matches {
                    println!(
                        "  - {} [{:?}] (Role: {:?})",
                        m.name, m.family, m.accesskit_role
                    );
                }
            }
        }
    }
}
