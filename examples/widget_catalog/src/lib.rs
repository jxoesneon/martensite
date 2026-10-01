//! Martensite Widget Catalog — interactive per-widget showcase and
//! developer reference: searchable rail, live stage, bespoke props,
//! event log, and dev-channel control.

pub mod app;
pub mod dynamic_column;
pub mod page;
pub mod pages;
pub mod stage;
pub mod view;

pub use app::{run, run_live_headless};
pub use page::{Page, PageMeta, PropSpec, PropValue, PropValues};
pub use pages::all_pages;
pub use stage::{FramePreset, StageHost};
pub use view::CatalogView;
