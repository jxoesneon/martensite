//! `layout` namespace — grids, columns, panels, splits, frames.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`layout.*`).
///
/// Every name constant in this module is prefixed `LAYOUT_`
/// (`LAYOUT_FOO` → `"layout.foo"`) so the flattened
/// `builtin::names` re-export cannot collide.
pub mod names {
    /// `"layout.layout-grid"` — four equal tiles.
    pub const LAYOUT_GRID: &str = "layout.layout-grid";
    /// `"layout.layout-list"` — tile plus row lines.
    pub const LAYOUT_LIST: &str = "layout.layout-list";
    /// `"layout.layout-template"` — wide header over two blocks.
    pub const LAYOUT_TEMPLATE: &str = "layout.layout-template";
    /// `"layout.layout-dashboard"` — four asymmetric tiles.
    pub const LAYOUT_DASHBOARD: &str = "layout.layout-dashboard";
    /// `"layout.columns-2"` — frame split into two columns.
    pub const LAYOUT_COLUMNS_2: &str = "layout.columns-2";
    /// `"layout.columns-3"` — frame split into three columns.
    pub const LAYOUT_COLUMNS_3: &str = "layout.columns-3";
    /// `"layout.rows-3"` — frame split into three rows.
    pub const LAYOUT_ROWS_3: &str = "layout.rows-3";
    /// `"layout.panel-left"` — frame with a narrow left panel.
    pub const LAYOUT_PANEL_LEFT: &str = "layout.panel-left";
    /// `"layout.panel-right"` — frame with a narrow right panel.
    pub const LAYOUT_PANEL_RIGHT: &str = "layout.panel-right";
    /// `"layout.panel-top"` — frame with a narrow top panel.
    pub const LAYOUT_PANEL_TOP: &str = "layout.panel-top";
    /// `"layout.panel-bottom"` — frame with a narrow bottom panel.
    pub const LAYOUT_PANEL_BOTTOM: &str = "layout.panel-bottom";
    /// `"layout.sidebar-left"` — left panel plus collapse chevron.
    pub const LAYOUT_SIDEBAR_LEFT: &str = "layout.sidebar-left";
    /// `"layout.sidebar-right"` — right panel plus collapse chevron.
    pub const LAYOUT_SIDEBAR_RIGHT: &str = "layout.sidebar-right";
    /// `"layout.split-horizontal"` — two halves beside a center line.
    pub const LAYOUT_SPLIT_HORIZONTAL: &str = "layout.split-horizontal";
    /// `"layout.split-vertical"` — two halves across a center line.
    pub const LAYOUT_SPLIT_VERTICAL: &str = "layout.split-vertical";
    /// `"layout.frame"` — corner crosshairs / bounds frame.
    pub const LAYOUT_FRAME: &str = "layout.frame";
    /// `"layout.app-window"` — window with a title bar.
    pub const LAYOUT_APP_WINDOW: &str = "layout.app-window";
    /// `"layout.gallery-horizontal"` — centered pane between side rails.
    pub const LAYOUT_GALLERY_HORIZONTAL: &str = "layout.gallery-horizontal";
    /// `"layout.gallery-vertical"` — centered pane between top/bottom rails.
    pub const LAYOUT_GALLERY_VERTICAL: &str = "layout.gallery-vertical";
    /// `"layout.kanban"` — board with hanging card columns.
    pub const LAYOUT_KANBAN: &str = "layout.kanban";
    /// `"layout.table"` — framed rows and a column divider.
    pub const LAYOUT_TABLE: &str = "layout.table";
    /// `"layout.grid-3x3"` — frame split into a 3×3 grid.
    pub const LAYOUT_GRID_3X3: &str = "layout.grid-3x3";
}

/// `"layout.layout-grid"` — four 7×7 rounded tiles.
pub const LAYOUT_GRID: &str = "M4 3h5a1 1 0 011 1v5a1 1 0 01-1 1H4a1 1 0 01-1-1V4a1 1 0 011-1z\
     M15 3h5a1 1 0 011 1v5a1 1 0 01-1 1h-5a1 1 0 01-1-1V4a1 1 0 011-1z\
     M4 14h5a1 1 0 011 1v5a1 1 0 01-1 1H4a1 1 0 01-1-1v-5a1 1 0 011-1z\
     M15 14h5a1 1 0 011 1v5a1 1 0 01-1 1h-5a1 1 0 01-1-1v-5a1 1 0 011-1z";
/// `"layout.layout-list"` — two tiles beside four row lines.
pub const LAYOUT_LIST: &str = "M4 3h5a1 1 0 011 1v5a1 1 0 01-1 1H4a1 1 0 01-1-1V4a1 1 0 011-1z\
     M4 14h5a1 1 0 011 1v5a1 1 0 01-1 1H4a1 1 0 01-1-1v-5a1 1 0 011-1z\
     M14 4h7M14 9h7M14 15h7M14 20h7";
/// `"layout.layout-template"` — header bar over a wide and a small block.
pub const LAYOUT_TEMPLATE: &str =
    "M5 3h14a2 2 0 012 2v3a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2z\
     M5 14h5a2 2 0 012 2v3a2 2 0 01-2 2H5a2 2 0 01-2-2v-3a2 2 0 012-2z\
     M17 14h3a1 1 0 011 1v5a1 1 0 01-1 1h-3a1 1 0 01-1-1v-5a1 1 0 011-1z";
/// `"layout.layout-dashboard"` — staggered tiles, tall and short.
pub const LAYOUT_DASHBOARD: &str =
    "M4 3h5a1 1 0 011 1v7a1 1 0 01-1 1H4a1 1 0 01-1-1V4a1 1 0 011-1z\
     M15 3h5a1 1 0 011 1v3a1 1 0 01-1 1h-5a1 1 0 01-1-1V4a1 1 0 011-1z\
     M15 12h5a1 1 0 011 1v7a1 1 0 01-1 1h-5a1 1 0 01-1-1v-7a1 1 0 011-1z\
     M4 16h5a1 1 0 011 1v3a1 1 0 01-1 1H4a1 1 0 01-1-1v-3a1 1 0 011-1z";
/// `"layout.columns-2"` — framed frame halved vertically.
pub const LAYOUT_COLUMNS_2: &str =
    "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM12 3v18";
/// `"layout.columns-3"` — frame thirded vertically.
pub const LAYOUT_COLUMNS_3: &str =
    "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM9 3v18M15 3v18";
/// `"layout.rows-3"` — frame thirded horizontally.
pub const LAYOUT_ROWS_3: &str =
    "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM3 9h18M3 15h18";
/// `"layout.panel-left"` — frame with a fixed left rail.
pub const LAYOUT_PANEL_LEFT: &str =
    "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM9 3v18";
/// `"layout.panel-right"` — frame with a fixed right rail.
pub const LAYOUT_PANEL_RIGHT: &str =
    "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM15 3v18";
/// `"layout.panel-top"` — frame with a fixed top rail.
pub const LAYOUT_PANEL_TOP: &str =
    "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM3 9h18";
/// `"layout.panel-bottom"` — frame with a fixed bottom rail.
pub const LAYOUT_PANEL_BOTTOM: &str =
    "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM3 15h18";
/// `"layout.sidebar-left"` — left rail plus inward chevron.
pub const LAYOUT_SIDEBAR_LEFT: &str =
    "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM9 3v18M16 9l-3 3 3 3";
/// `"layout.sidebar-right"` — right rail plus inward chevron.
pub const LAYOUT_SIDEBAR_RIGHT: &str =
    "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM15 3v18M8 9l3 3-3 3";
/// `"layout.split-horizontal"` — open halves split by a vertical line.
pub const LAYOUT_SPLIT_HORIZONTAL: &str =
    "M8 19H5a2 2 0 01-2-2V7a2 2 0 012-2h3M16 5h3a2 2 0 012 2v10a2 2 0 01-2 2h-3M12 4v16";
/// `"layout.split-vertical"` — open halves split by a horizontal line.
pub const LAYOUT_SPLIT_VERTICAL: &str =
    "M19 8V5a2 2 0 00-2-2H7a2 2 0 00-2 2v3M5 16v3a2 2 0 002 2h10a2 2 0 002-2v-3M4 12h16";
/// `"layout.frame"` — corner ticks at full bleed.
pub const LAYOUT_FRAME: &str = "M2 6h20M2 18h20M6 2v20M18 2v20";
/// `"layout.app-window"` — window chrome with a title strip.
pub const LAYOUT_APP_WINDOW: &str =
    "M4 4h16a2 2 0 012 2v12a2 2 0 01-2 2H4a2 2 0 01-2-2V6a2 2 0 012-2zM2 8h20M6 4v4M10 4v4";
/// `"layout.gallery-horizontal"` — center pane flanked by rails.
pub const LAYOUT_GALLERY_HORIZONTAL: &str =
    "M2 3v18M22 3v18M8 3h8a2 2 0 012 2v14a2 2 0 01-2 2H8a2 2 0 01-2-2V5a2 2 0 012-2z";
/// `"layout.gallery-vertical"` — center pane between top/bottom rails.
pub const LAYOUT_GALLERY_VERTICAL: &str =
    "M3 2h18M3 22h18M5 6h14a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2z";
/// `"layout.kanban"` — board with columns of hanging cards.
pub const LAYOUT_KANBAN: &str =
    "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM8 7v6M12 7v3M16 7v9";
/// `"layout.table"` — framed rows crossed by one column.
pub const LAYOUT_TABLE: &str =
    "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM3 9h18M3 15h18M12 3v18";
/// `"layout.grid-3x3"` — frame divided into nine cells.
pub const LAYOUT_GRID_3X3: &str = "M5 3h14a2 2 0 012 2v14a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2zM3 9h18M3 15h18M9 3v18M15 3v18";

/// `layout` entries — registered in the pack in this order.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::LAYOUT_GRID, LAYOUT_GRID),
    IconEntry::new(names::LAYOUT_LIST, LAYOUT_LIST),
    IconEntry::new(names::LAYOUT_TEMPLATE, LAYOUT_TEMPLATE),
    IconEntry::new(names::LAYOUT_DASHBOARD, LAYOUT_DASHBOARD),
    IconEntry::new(names::LAYOUT_COLUMNS_2, LAYOUT_COLUMNS_2),
    IconEntry::new(names::LAYOUT_COLUMNS_3, LAYOUT_COLUMNS_3),
    IconEntry::new(names::LAYOUT_ROWS_3, LAYOUT_ROWS_3),
    IconEntry::new(names::LAYOUT_PANEL_LEFT, LAYOUT_PANEL_LEFT),
    IconEntry::new(names::LAYOUT_PANEL_RIGHT, LAYOUT_PANEL_RIGHT),
    IconEntry::new(names::LAYOUT_PANEL_TOP, LAYOUT_PANEL_TOP),
    IconEntry::new(names::LAYOUT_PANEL_BOTTOM, LAYOUT_PANEL_BOTTOM),
    IconEntry::new(names::LAYOUT_SIDEBAR_LEFT, LAYOUT_SIDEBAR_LEFT),
    IconEntry::new(names::LAYOUT_SIDEBAR_RIGHT, LAYOUT_SIDEBAR_RIGHT),
    IconEntry::new(names::LAYOUT_SPLIT_HORIZONTAL, LAYOUT_SPLIT_HORIZONTAL),
    IconEntry::new(names::LAYOUT_SPLIT_VERTICAL, LAYOUT_SPLIT_VERTICAL),
    IconEntry::new(names::LAYOUT_FRAME, LAYOUT_FRAME),
    IconEntry::new(names::LAYOUT_APP_WINDOW, LAYOUT_APP_WINDOW),
    IconEntry::new(names::LAYOUT_GALLERY_HORIZONTAL, LAYOUT_GALLERY_HORIZONTAL),
    IconEntry::new(names::LAYOUT_GALLERY_VERTICAL, LAYOUT_GALLERY_VERTICAL),
    IconEntry::new(names::LAYOUT_KANBAN, LAYOUT_KANBAN),
    IconEntry::new(names::LAYOUT_TABLE, LAYOUT_TABLE),
    IconEntry::new(names::LAYOUT_GRID_3X3, LAYOUT_GRID_3X3),
];

/// `layout` morph pairs.
pub const PAIRS: &[IconPair] = &[IconPair::new(
    names::LAYOUT_SPLIT_HORIZONTAL,
    names::LAYOUT_SPLIT_VERTICAL,
)];
