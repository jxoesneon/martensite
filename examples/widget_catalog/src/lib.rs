//! Martensite Widget Catalog — interactive API reference and cross-framework migration map.

pub mod entries;
pub mod model;
pub mod view;

pub use entries::catalog_entries;
pub use model::{CatalogModel, WidgetEntry, WidgetFamily, WidgetStateKind};
pub use view::{build_catalog_view, build_widget_card};
