//! `Table` widget: a virtualized data grid with a pinned, sortable,
//! resizable column header.
//!
//! The data counterpart to [`ListView`](crate::widgets::ListView) —
//! the `QTableView` / GTK `ColumnView` / WinUI `DataGrid` / Ant
//! `Table` slot in the widget set:
//!
//! - `Role::Table` on the widget. Visible rows are emitted as pooled
//!   internal `Role::Row` children (each with `Role::Cell`
//!   grandchildren carrying `column_index`), and column headers as
//!   internal `Role::ColumnHeader` children carrying the AccessKit
//!   `sort_direction` — the same windowed-emission contract `ListView`
//!   uses, so the table stays a single keyboard-focus stop.
//! - **Virtualized**: only the display rows intersecting the body
//!   viewport are materialized into the child pool, painted, and
//!   emitted into the accessibility tree. Scrolling is owned (wheel,
//!   keyboard, and the shared `VScrollBar` strip) because row
//!   virtualization needs the offset at paint time — `ScrollView`
//!   cannot be reused here.
//! - **Sorting is a view concern**: clicking a sortable column header
//!   cycles ascending → descending → none. The widget owns the display
//!   permutation ([`Table::sorted_rows`]) and repaints in sorted order;
//!   reordering the app's backing data is the app's job — it observes
//!   changes through [`Table::take_sort`]/[`Table::sort_state`] and may
//!   apply the same permutation to its own storage.
//! - **Resizing**: pressing within ~4pt of a header column separator
//!   captures the pointer and drags that column's width (clamped to
//!   its `min_width`).
//! - Keyboard: `ArrowUp`/`ArrowDown`/`PageUp`/`PageDown`/`Home`/`End`
//!   move focus+selection through the *display* order with
//!   scroll-into-view; `Enter` activates the focused row (drained via
//!   [`Table::take_activated`]). Pointer: click selects, double-click
//!   activates, drag moves the selection, hover highlights.
//!
//! # Documented limitations
//!
//! - **Single selection only** — `selected()`/`select()` address one
//!   *storage* row. Range and disjoint multi-selection are left to a
//!   future revision.
//! - **Index space**: `selected()`, `select()`, `take_activated()`,
//!   and `scroll_row_into_view()` use *storage* indices (positions in
//!   the data as added); `visible_range()` and the row-child
//!   `row_index` use *display* positions after sorting.
//! - **Cells are strings**, not delegates — per-cell rich content and
//!   horizontal scrolling are out of scope; cells clip instead.
//! - Cells compare **numeric-aware**: when both cells parse as `f64`
//!   they order numerically, otherwise lexicographically (byte order).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{Table, TableColumn};
//!
//! let table = Table::new()
//!     .columns([
//!         TableColumn::new("name", "Name"),
//!         TableColumn::new("age", "Age").width(60.0),
//!     ])
//!     .row(["Ada", "36"])
//!     .row(["Grace", "85"]);
//! assert_eq!(table.row_count(), 2);
//! ```

use std::cmp::Ordering;
use std::ops::Range;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use kurbo::BezPath;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, RenderMinimum, TokenKey, UnderflowPolicy};

use crate::widgets::list_view::{BarRequest, VScrollBar};

/// Default body row height in logical points.
const ROW_H: f32 = 28.0;
/// Default header row height in logical points.
const HEADER_H: f32 = 28.0;
/// Maximum body rows the widget asks for in `measure` before scrolling.
const MAX_VISIBLE_ROWS: f32 = 10.0;
/// Scrollbar thickness in logical pixels.
const BAR: f32 = 10.0;
/// Minimum scrollbar thumb length.
const MIN_THUMB: f32 = 24.0;
/// Half-width of the header-separator resize hit zone in logical points.
const EDGE_PT: f32 = 4.0;
/// Horizontal cell text padding in logical points.
const CELL_PAD: f32 = 8.0;
/// Default column width in logical points.
const DEFAULT_COL_W: f32 = 120.0;
/// Default column minimum width in logical points.
const DEFAULT_MIN_COL_W: f32 = 40.0;
/// Cell font size in logical points.
const CELL_FONT: f32 = 13.0;
/// Header font size in logical points.
const HEADER_FONT: f32 = 13.0;

/// Track colour.
const TRACK_COLOR: [u8; 4] = [235, 237, 240, 255];
/// Header face background.
const HEADER_BG: [u8; 4] = [245, 246, 249, 255];
/// Table face background.
const SURFACE_BG: [u8; 4] = [250, 250, 252, 255];
/// Table border.
const BORDER: [u8; 4] = [150, 155, 165, 255];
/// Label ink.
const INK: [u8; 4] = [30, 30, 36, 255];
/// Disabled label ink.
const INK_DISABLED: [u8; 4] = [150, 150, 158, 255];
/// Selection accent.
const ACCENT: [u8; 4] = [60, 110, 220, 255];
/// Hover wash alpha.
const HOVER_ALPHA: u8 = 32;
/// Selected-row wash alpha.
const SELECTED_ALPHA: u8 = 56;
/// Alternating-row stripe alpha.
const STRIPE_ALPHA: u8 = 12;
/// Focus ring alpha (the accent colour at 50%).
const FOCUS_ALPHA: u8 = 128;

/// Horizontal text alignment inside a [`Table`] cell or header.
///
/// # Examples
///
/// ```
/// use martensite::widgets::TableAlign;
///
/// assert_eq!(TableAlign::default(), TableAlign::Start);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum TableAlign {
    /// Align to the leading edge (left in LTR layouts).
    #[default]
    Start,
    /// Centre horizontally inside the cell.
    Center,
    /// Align to the trailing edge (right in LTR layouts).
    End,
}

/// Direction of a [`Table`] column sort — the `take_sort` payload.
///
/// # Examples
///
/// ```
/// use martensite::widgets::SortDir;
///
/// assert_eq!(SortDir::default(), SortDir::Ascending);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum SortDir {
    /// Ascending order (numeric-aware cell comparison).
    #[default]
    Ascending,
    /// Descending order.
    Descending,
}

/// One column of a [`Table`] — identity, header title, width, and
/// per-column options.
///
/// `id` is a stable app-level key (the widget never interprets it);
/// `title` is the painted header text. `width` and `min_width` are
/// logical points.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{TableAlign, TableColumn};
///
/// let col = TableColumn::new("price", "Price")
///     .width(90.0)
///     .min_width(48.0)
///     .sortable(true)
///     .align(TableAlign::End);
/// assert_eq!(col.id, "price");
/// assert_eq!(col.width, 90.0);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct TableColumn {
    /// Stable application-level column key.
    pub id: String,
    /// Header text.
    pub title: String,
    /// Column width in logical points.
    pub width: f32,
    /// Minimum width a resize drag may shrink the column to (logical
    /// points).
    pub min_width: f32,
    /// Whether this column participates in sorting (ANDed with the
    /// table-wide [`Table::sortable`] flag).
    pub sortable: bool,
    /// Cell and header text alignment.
    pub align: TableAlign,
}

impl TableColumn {
    /// Creates a column with the default width ([`DEFAULT_COL_W`] =
    /// 120pt), minimum width (40pt), `sortable: true`, and
    /// [`TableAlign::Start`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TableColumn;
    ///
    /// let col = TableColumn::new("name", "Name");
    /// assert_eq!(col.title, "Name");
    /// assert!(col.sortable);
    /// ```
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            width: DEFAULT_COL_W,
            min_width: DEFAULT_MIN_COL_W,
            sortable: true,
            align: TableAlign::Start,
        }
    }

    /// Sets the column width in logical points.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TableColumn;
    ///
    /// let col = TableColumn::new("a", "A").width(200.0);
    /// assert_eq!(col.width, 200.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Sets the minimum width a resize drag may shrink the column to,
    /// in logical points.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TableColumn;
    ///
    /// let col = TableColumn::new("a", "A").min_width(24.0);
    /// assert_eq!(col.min_width, 24.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = width;
        self
    }

    /// Sets whether this column is sortable (ANDed with
    /// [`Table::sortable`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TableColumn;
    ///
    /// let col = TableColumn::new("a", "A").sortable(false);
    /// assert!(!col.sortable);
    /// ```
    #[inline]
    #[must_use]
    pub fn sortable(mut self, sortable: bool) -> Self {
        self.sortable = sortable;
        self
    }

    /// Sets the cell/header text alignment.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TableAlign, TableColumn};
    ///
    /// let col = TableColumn::new("a", "A").align(TableAlign::End);
    /// assert_eq!(col.align, TableAlign::End);
    /// ```
    #[inline]
    #[must_use]
    pub fn align(mut self, align: TableAlign) -> Self {
        self.align = align;
        self
    }
}

/// One cell of an emitted AT row — `Role::Cell` leaf carrying the
/// cell text and its `column_index`. Cells never paint (the owning
/// [`Table`] draws the whole grid) and never receive events (the
/// table's `event` owns all pointer routing).
struct TableCellChild {
    /// Column index this cell belongs to.
    column_index: usize,
    /// The cell text.
    label: String,
    /// Whether the owning row is selected.
    selected: bool,
    /// Cached bounds (mirrored by `Table::sync_rows`).
    rect: Rect,
    /// Whether the owner is enabled.
    enabled: bool,
}

impl Widget for TableCellChild {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        constraints.max_size.max(Vec2::ZERO)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Cell);
        node.set_label(self.label.as_str());
        node.set_column_index(self.column_index);
        node.set_selected(self.selected);
        if !self.enabled {
            node.set_disabled();
        }
    }
}

/// One column header emitted into the accessibility tree —
/// `Role::ColumnHeader` carrying the title, `column_index`, and the
/// active `sort_direction` when the column drives the sort. Header
/// clicks are handled on the [`Table`] node itself, so the header
/// child advertises no actions.
struct TableHeaderChild {
    /// Column index this header presents.
    column_index: usize,
    /// The header title.
    label: String,
    /// The column's sort state on the table, if it drives the sort.
    sort: Option<SortDir>,
    /// Whether the owner is enabled.
    enabled: bool,
}

impl Widget for TableHeaderChild {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        constraints.max_size.max(Vec2::ZERO)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ColumnHeader);
        node.set_label(self.label.as_str());
        node.set_column_index(self.column_index);
        match self.sort {
            Some(SortDir::Ascending) => {
                node.set_sort_direction(accesskit::SortDirection::Ascending);
            }
            Some(SortDir::Descending) => {
                node.set_sort_direction(accesskit::SortDirection::Descending);
            }
            None => {}
        }
        if !self.enabled {
            node.set_disabled();
        }
    }
}

/// One visible row inside a [`Table`] — the pooled AT child.
///
/// Rows are *pooled* like `ListView`'s: the widget keeps enough to
/// cover the viewport and re-points them at display positions as the
/// body scrolls. Each emits `Role::Row` with `row_index` (the display
/// position), `selected`, and `Role::Cell` grandchildren. AT actions
/// are parked and applied by the owner via [`Table::poll_pending`];
/// painting is done wholesale by `Table::paint` so cells clip to the
/// body viewport.
struct TableRowChild {
    /// Display position this row presents (post-sort index).
    display_index: usize,
    /// Storage index (position in the app's data) this row presents.
    storage_index: usize,
    /// Joined cell text for the row's accessible label.
    label: String,
    /// Whether the row is selected.
    selected: bool,
    /// Whether this row carries the roving focus indicator.
    focused: bool,
    /// Whether the owner is enabled.
    enabled: bool,
    /// Parked `SemanticAction::Click` / pointer press for the owner.
    press_pending: bool,
    /// Parked `SemanticAction::Focus` for the owner.
    focus_pending: bool,
    /// Cell children (one per column).
    cells: Vec<TableCellChild>,
}

impl TableRowChild {
    fn new() -> Self {
        Self {
            display_index: 0,
            storage_index: 0,
            label: String::new(),
            selected: false,
            focused: false,
            enabled: true,
            press_pending: false,
            focus_pending: false,
            cells: Vec::new(),
        }
    }
}

impl Widget for TableRowChild {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(0.0),
            cx.pt(ROW_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Row);
        node.set_label(self.label.as_str());
        node.set_row_index(self.display_index);
        node.set_selected(self.selected);
        node.add_action(accesskit::Action::Click);
        // Roving tabindex: only the row holding the indicator
        // advertises Focus — the table is a single tab stop.
        if self.focused {
            node.add_action(accesskit::Action::Focus);
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                ..
            }
            | WidgetEvent::SemanticAction(SemanticAction::Click) => {
                self.press_pending = true;
                EventResponse::CaptureFocus
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => {
                self.focus_pending = true;
                EventResponse::CaptureFocus
            }
            _ => EventResponse::Ignored,
        }
    }

    fn child_count(&self) -> usize {
        self.cells.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.cells.get(index).map(|c| c as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.cells.get_mut(index).map(|c| c as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.cells.get(index).map(|c| c.rect)
    }
}

/// In-flight header-separator drag: the column being resized and the
/// pointer's grab offset left of the separator.
#[derive(Copy, Clone, Debug)]
struct ResizeDrag {
    /// Index of the column whose right edge is being dragged.
    column: usize,
    /// `position.x - separator_x` captured at press time.
    grab: f32,
}

/// A virtualized data grid: pinned sortable header over a scrollable,
/// striped, selectable body of string cells.
///
/// Column order is fixed; column *widths* are user-resizable by
/// dragging header separators. Sorting reorders the *display* —
/// `sorted_rows()` exposes the display→storage permutation and
/// `take_sort()` the change seam; the app's backing data stays the
/// app's problem.
///
/// All index parameters on the public API are *storage* indices
/// unless noted — see the module-level index-space note.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Table, TableColumn};
///
/// let mut table = Table::new()
///     .columns([TableColumn::new("n", "Name")])
///     .row(["Ada"])
///     .row(["Grace"]);
/// table.select(1);
/// assert_eq!(table.selected(), Some(1));
/// ```
pub struct Table {
    /// Whether the table accepts input.
    pub enabled: bool,
    /// Optional accessible label.
    pub label: Option<String>,
    /// Whether odd display rows paint the alternating stripe.
    pub striped: bool,
    /// Whether row/column grid lines are painted.
    pub grid_lines: bool,
    /// Table-wide sort gate — ANDed with each column's `sortable`.
    pub sortable: bool,
    /// Body row height in logical points.
    pub row_height: f32,
    /// Pinned header height in logical points.
    pub header_height: f32,
    /// Column definitions.
    columns: Vec<TableColumn>,
    /// Row storage: each row is a cell vector indexed by column.
    rows: Vec<Vec<String>>,
    /// Display→storage permutation (identity when unsorted).
    order: Vec<usize>,
    /// Column driving the sort, if any.
    sort_column: Option<usize>,
    /// Direction of the active sort.
    sort_dir: SortDir,
    /// Sort change parked for `take_sort`.
    pending_sort: Option<(usize, SortDir)>,
    /// Selected storage index.
    selected: Option<usize>,
    /// Storage index of the row carrying the roving focus indicator.
    focused: usize,
    /// Whether the widget holds keyboard focus (for the focus ring).
    has_focus: bool,
    /// Hovered storage index.
    hovered: Option<usize>,
    /// Vertical scroll offset in device pixels.
    scroll_y: f32,
    /// Pooled visible-window row children.
    row_children: Vec<TableRowChild>,
    /// Emitted column-header children (one per column).
    header_children: Vec<TableHeaderChild>,
    /// The vertical scrollbar (last internal child).
    vbar: VScrollBar,
    /// Storage index activated since the last `take_activated`.
    activated: Option<usize>,
    /// Cached widget bounds.
    cached_bounds: Rect,
    /// Pinned header strip.
    header_rect: Rect,
    /// Body viewport (widget bounds minus header and shown bar).
    viewport: Rect,
    /// Vertical bar rect when shown.
    vbar_rect: Option<Rect>,
    /// Thumb-drag state: grab offset inside the thumb.
    thumb_drag: Option<f32>,
    /// Header-separator drag state.
    resize_drag: Option<ResizeDrag>,
    /// Whether a press-drag on rows is in flight.
    dragging: bool,
    /// Display scale from `layout` — sizes are logical pt.
    scale: f32,
    /// Shared shaped-text painter for cell/header text.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Table {
    /// Creates an empty table; add columns with
    /// [`columns`](Self::columns) and rows with [`row`](Self::row).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let t = Table::new();
    /// assert_eq!(t.row_count(), 0);
    /// assert_eq!(t.column_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            enabled: true,
            label: None,
            striped: false,
            grid_lines: false,
            sortable: true,
            row_height: ROW_H,
            header_height: HEADER_H,
            columns: Vec::new(),
            rows: Vec::new(),
            order: Vec::new(),
            sort_column: None,
            sort_dir: SortDir::Ascending,
            pending_sort: None,
            selected: None,
            focused: 0,
            has_focus: false,
            hovered: None,
            scroll_y: 0.0,
            row_children: Vec::new(),
            header_children: Vec::new(),
            vbar: VScrollBar::new(),
            activated: None,
            cached_bounds: Rect::default(),
            header_rect: Rect::default(),
            viewport: Rect::default(),
            vbar_rect: None,
            thumb_drag: None,
            resize_drag: None,
            dragging: false,
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Sets the column definitions (builder form of
    /// [`set_columns`](Self::set_columns)).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Table, TableColumn};
    ///
    /// let t = Table::new().columns([TableColumn::new("a", "A")]);
    /// assert_eq!(t.column_count(), 1);
    /// ```
    #[must_use]
    pub fn columns(mut self, columns: impl IntoIterator<Item = TableColumn>) -> Self {
        self.set_columns(columns);
        self
    }

    /// Replaces the column definitions, sanitizing widths to their
    /// minimums and dropping a sort that referenced a vanished column.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Table, TableColumn};
    ///
    /// let mut t = Table::new();
    /// t.set_columns([TableColumn::new("a", "A"), TableColumn::new("b", "B")]);
    /// assert_eq!(t.column_count(), 2);
    /// ```
    pub fn set_columns(&mut self, columns: impl IntoIterator<Item = TableColumn>) {
        self.columns = columns
            .into_iter()
            .map(|mut c| {
                c.width = c.width.max(c.min_width.max(1.0));
                c
            })
            .collect();
        if self.sort_column.is_some_and(|c| c >= self.columns.len()) {
            self.sort_column = None;
            self.recompute_order();
        }
        self.sync_children();
        self.sync_bars();
    }

    /// The column definition at `index`, if in range.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Table, TableColumn};
    ///
    /// let t = Table::new().columns([TableColumn::new("a", "A")]);
    /// assert_eq!(t.column(0).map(|c| c.title.as_str()), Some("A"));
    /// assert!(t.column(9).is_none());
    /// ```
    #[inline]
    pub fn column(&self, index: usize) -> Option<&TableColumn> {
        self.columns.get(index)
    }

    /// The number of columns.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Table, TableColumn};
    ///
    /// assert_eq!(Table::new().columns([TableColumn::new("a", "A")]).column_count(), 1);
    /// ```
    #[inline]
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Appends a row of cells (builder form of
    /// [`add_row`](Self::add_row)). Extra cells beyond the column count
    /// are ignored at paint time; missing cells paint empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Table, TableColumn};
    ///
    /// let t = Table::new()
    ///     .columns([TableColumn::new("a", "A")])
    ///     .row(["one"])
    ///     .row(["two"]);
    /// assert_eq!(t.row_count(), 2);
    /// ```
    #[must_use]
    pub fn row(mut self, cells: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.add_row(cells);
        self
    }

    /// Appends a row of cells (mutating form).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let mut t = Table::new();
    /// t.add_row(["x", "y"]);
    /// assert_eq!(t.cell(0, 1), Some("y"));
    /// ```
    pub fn add_row(&mut self, cells: impl IntoIterator<Item = impl Into<String>>) {
        self.rows.push(cells.into_iter().map(Into::into).collect());
        self.recompute_order();
        self.sync_children();
    }

    /// Replaces all rows. Selection, focus, and scroll offset clamp
    /// into the new range.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let mut t = Table::new().row(["a"]);
    /// t.set_rows([vec!["b".to_string()], vec!["c".to_string()]]);
    /// assert_eq!(t.row_count(), 2);
    /// ```
    pub fn set_rows(&mut self, rows: impl IntoIterator<Item = Vec<String>>) {
        self.rows = rows.into_iter().collect();
        self.recompute_order();
        let n = self.rows.len();
        self.focused = self.focused.min(n.saturating_sub(1));
        self.selected = self.selected.filter(|&i| i < n);
        self.hovered = self.hovered.filter(|&i| i < n);
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());
        self.sync_children();
        self.sync_bars();
    }

    /// The number of rows.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// assert_eq!(Table::new().row(["a"]).row_count(), 1);
    /// ```
    #[inline]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// The cell text at storage `row`, column `col`, if in range.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let t = Table::new().row(["a", "b"]);
    /// assert_eq!(t.cell(0, 0), Some("a"));
    /// assert_eq!(t.cell(0, 9), None);
    /// ```
    #[inline]
    pub fn cell(&self, row: usize, col: usize) -> Option<&str> {
        self.rows.get(row)?.get(col).map(String::as_str)
    }

    /// Sets whether odd display rows paint the alternating stripe.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let t = Table::new().striped(true);
    /// assert!(t.striped);
    /// ```
    #[inline]
    #[must_use]
    pub fn striped(mut self, striped: bool) -> Self {
        self.striped = striped;
        self
    }

    /// Sets whether row/column grid lines are painted.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let t = Table::new().grid_lines(true);
    /// assert!(t.grid_lines);
    /// ```
    #[inline]
    #[must_use]
    pub fn grid_lines(mut self, grid: bool) -> Self {
        self.grid_lines = grid;
        self
    }

    /// Sets the table-wide sort gate (ANDed with each column's
    /// `sortable` flag).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let t = Table::new().sortable(false);
    /// assert!(!t.sortable);
    /// ```
    #[inline]
    #[must_use]
    pub fn sortable(mut self, sortable: bool) -> Self {
        self.sortable = sortable;
        self
    }

    /// Sets whether the table accepts input.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let t = Table::new().enabled(false);
    /// assert!(!t.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self.sync_children();
        self
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let t = Table::new().label("People");
    /// assert_eq!(t.label.as_deref(), Some("People"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the body row height in logical points.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let t = Table::new().row_height(32.0);
    /// assert_eq!(t.row_height, 32.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn row_height(mut self, height: f32) -> Self {
        self.row_height = height.max(1.0);
        self
    }

    /// Sets the pinned header height in logical points.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let t = Table::new().header_height(36.0);
    /// assert_eq!(t.header_height, 36.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn header_height(mut self, height: f32) -> Self {
        self.header_height = height.max(0.0);
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so cells emit real
    /// glyph runs instead of `DrawText` placeholder boxes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let t = Table::new().with_text_painter(shared_painter());
    /// ```
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The selected storage index, or `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let mut t = Table::new().row(["a"]).row(["b"]);
    /// assert_eq!(t.selected(), None);
    /// t.select(1);
    /// assert_eq!(t.selected(), Some(1));
    /// ```
    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Whether storage `index` is selected.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let mut t = Table::new().row(["a"]);
    /// t.select(0);
    /// assert!(t.is_selected(0));
    /// ```
    #[inline]
    pub fn is_selected(&self, index: usize) -> bool {
        self.selected == Some(index)
    }

    /// Selects storage `index` and moves the roving focus to it,
    /// scrolling it into view. Out-of-range indices are ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let mut t = Table::new().row(["a"]).row(["b"]);
    /// t.select(9);
    /// assert_eq!(t.selected(), None); // out of range ignored
    /// t.select(0);
    /// assert_eq!(t.selected(), Some(0));
    /// ```
    pub fn select(&mut self, index: usize) {
        if index < self.rows.len() {
            self.focused = index;
            self.selected = Some(index);
            if let Some(d) = self.display_of(index) {
                self.ensure_visible(d);
            }
            self.sync_children();
        }
    }

    /// Clears the selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let mut t = Table::new().row(["a"]);
    /// t.select(0);
    /// t.clear_selection();
    /// assert_eq!(t.selected(), None);
    /// ```
    pub fn clear_selection(&mut self) {
        self.selected = None;
        self.sync_children();
    }

    /// The storage index of the row carrying the roving focus
    /// indicator.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// assert_eq!(Table::new().row(["a"]).focused_index(), 0);
    /// ```
    #[inline]
    pub fn focused_index(&self) -> usize {
        self.focused
    }

    /// The activated storage index once, if any — the
    /// `Dialog::take_response` out-seam for `Enter` and double-click
    /// activations.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let mut t = Table::new().row(["a"]);
    /// assert_eq!(t.take_activated(), None);
    /// ```
    pub fn take_activated(&mut self) -> Option<usize> {
        self.activated.take()
    }

    /// The pending sort change once, if any — `(column, direction)`.
    /// Cycling a column back to "none" produces no payload; observe
    /// that state through [`sort_state`](Self::sort_state).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{SortDir, Table, TableColumn};
    ///
    /// let mut t = Table::new().columns([TableColumn::new("a", "A")]);
    /// t.set_sort(0, SortDir::Descending);
    /// assert_eq!(t.take_sort(), Some((0, SortDir::Descending)));
    /// assert_eq!(t.take_sort(), None);
    /// ```
    pub fn take_sort(&mut self) -> Option<(usize, SortDir)> {
        self.pending_sort.take()
    }

    /// Sets the sort to `column`/`dir` (mutating form). Out-of-range
    /// columns are ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{SortDir, Table, TableColumn};
    ///
    /// let mut t = Table::new()
    ///     .columns([TableColumn::new("a", "A")])
    ///     .row(["b"])
    ///     .row(["a"]);
    /// t.set_sort(0, SortDir::Ascending);
    /// assert_eq!(t.sorted_rows(), vec![1, 0]);
    /// ```
    pub fn set_sort(&mut self, column: usize, dir: SortDir) {
        if column >= self.columns.len() {
            return;
        }
        self.sort_column = Some(column);
        self.sort_dir = dir;
        self.pending_sort = Some((column, dir));
        self.recompute_order();
        self.sync_children();
    }

    /// Clears the sort, restoring natural (storage) display order.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{SortDir, Table, TableColumn};
    ///
    /// let mut t = Table::new().columns([TableColumn::new("a", "A")]);
    /// t.set_sort(0, SortDir::Ascending);
    /// t.clear_sort();
    /// assert_eq!(t.sort_state(), None);
    /// ```
    pub fn clear_sort(&mut self) {
        self.sort_column = None;
        self.recompute_order();
        self.sync_children();
    }

    /// The active sort as `(column, direction)`, or `None` when
    /// display order matches storage order.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{SortDir, Table, TableColumn};
    ///
    /// let mut t = Table::new().columns([TableColumn::new("a", "A")]);
    /// assert_eq!(t.sort_state(), None);
    /// t.set_sort(0, SortDir::Ascending);
    /// assert_eq!(t.sort_state(), Some((0, SortDir::Ascending)));
    /// ```
    #[inline]
    pub fn sort_state(&self) -> Option<(usize, SortDir)> {
        self.sort_column.map(|c| (c, self.sort_dir))
    }

    /// The display→storage permutation the body paints: the storage
    /// index shown at each display position. With no active sort this
    /// is the identity map. Apps that keep their own backing store
    /// sorted apply the same permutation to it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{SortDir, Table, TableColumn};
    ///
    /// let mut t = Table::new()
    ///     .columns([TableColumn::new("a", "A")])
    ///     .row(["b"])
    ///     .row(["a"])
    ///     .row(["c"]);
    /// assert_eq!(t.sorted_rows(), vec![0, 1, 2]); // identity unsorted
    /// t.set_sort(0, SortDir::Ascending);
    /// assert_eq!(t.sorted_rows(), vec![1, 0, 2]);
    /// ```
    pub fn sorted_rows(&self) -> Vec<usize> {
        self.order.clone()
    }

    /// The current clamped vertical scroll offset in device pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// assert_eq!(Table::new().scroll_offset(), 0.0);
    /// ```
    #[inline]
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_y
    }

    /// Maximum scroll offset: `max(0, content - viewport)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// assert_eq!(Table::new().max_scroll_offset(), 0.0);
    /// ```
    #[inline]
    pub fn max_scroll_offset(&self) -> f32 {
        self.max_scroll()
    }

    /// Sets the scroll offset in device pixels, clamped to
    /// `0..=max_scroll_offset`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let mut t = Table::new();
    /// t.set_scroll_offset(-10.0);
    /// assert_eq!(t.scroll_offset(), 0.0); // clamped
    /// ```
    pub fn set_scroll_offset(&mut self, offset: f32) {
        self.set_scroll(offset);
    }

    /// Scrolls by `delta` device pixels, clamped. Returns the
    /// actually-applied delta — `0.0` means nothing was consumed
    /// (chaining boundary for an ancestor scroll region).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let mut t = Table::new();
    /// assert_eq!(t.scroll_by(10.0), 0.0); // nothing to scroll
    /// ```
    pub fn scroll_by(&mut self, delta: f32) -> f32 {
        let old = self.scroll_y;
        self.set_scroll(old + delta);
        self.scroll_y - old
    }

    /// Scrolls the minimum amount that makes storage row `index`
    /// fully visible.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let mut t = Table::new().row(["a"]).row(["b"]);
    /// t.scroll_row_into_view(1);
    /// ```
    pub fn scroll_row_into_view(&mut self, index: usize) {
        if let Some(d) = self.display_of(index) {
            self.ensure_visible(d);
        }
    }

    /// The range of *display positions* intersecting the viewport —
    /// the window the row pool materializes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let t = Table::new().row(["a"]);
    /// assert_eq!(t.visible_range(), 0..0); // not laid out yet
    /// ```
    pub fn visible_range(&self) -> Range<usize> {
        let row_px = self.row_px();
        if row_px <= 0.0 || self.rows.is_empty() || self.viewport.height() <= 0.0 {
            return 0..0;
        }
        let start = self.first_visible();
        let end = ((self.scroll_y + self.viewport.height()) / row_px).ceil() as usize;
        start..end.min(self.rows.len())
    }

    /// Applies pending presses/focus moves recorded by row children
    /// and scroll requests parked by the scrollbar (AT actions
    /// delivered through `WidgetArena::internal_widget_mut`).
    ///
    /// Called automatically from `event`, `layout`, and `a11y_prepare`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Table;
    ///
    /// let mut t = Table::new().row(["a", "b"]);
    /// t.poll_pending();
    /// assert_eq!(t.selected(), None);
    /// ```
    pub fn poll_pending(&mut self) {
        let mut press = None;
        let mut focus = None;
        for row in &mut self.row_children {
            if row.press_pending {
                row.press_pending = false;
                press = Some(row.storage_index);
            } else if row.focus_pending {
                row.focus_pending = false;
                focus = Some(row.storage_index);
            }
        }
        if let Some(i) = press {
            self.select(i);
        } else if let Some(i) = focus {
            // Focus without selection (a press also moves focus).
            self.focused = i.min(self.rows.len().saturating_sub(1));
            if let Some(d) = self.display_of(self.focused) {
                self.ensure_visible(d);
            }
            self.sync_children();
        }
        if let Some(req) = self.vbar.pending.take() {
            match req {
                BarRequest::By(d) => {
                    self.scroll_by(d);
                }
                BarRequest::To(o) => {
                    self.set_scroll(o);
                }
            }
        }
    }

    /// Row height in device pixels at the cached display scale.
    fn row_px(&self) -> f32 {
        self.row_height * self.scale
    }

    /// Full body content height in device pixels.
    fn content_height(&self) -> f32 {
        self.rows.len() as f32 * self.row_px()
    }

    /// Maximum scroll offset.
    fn max_scroll(&self) -> f32 {
        (self.content_height() - self.viewport.height()).max(0.0)
    }

    /// First fully-or-partially visible display position.
    fn first_visible(&self) -> usize {
        let row_px = self.row_px();
        if row_px <= 0.0 || self.rows.is_empty() {
            return 0;
        }
        ((self.scroll_y / row_px).floor() as usize).min(self.rows.len() - 1)
    }

    /// How many pooled rows cover the viewport (plus one partial row).
    fn visible_capacity(&self) -> usize {
        let row_px = self.row_px();
        if row_px <= 0.0 || self.viewport.height() <= 0.0 {
            return 0;
        }
        (self.viewport.height() / row_px).ceil() as usize + 1
    }

    /// Absolute device-pixel x positions of column edges within the
    /// viewport: `len == columns + 1`, `edges[0] == viewport.min_x`.
    fn col_edges(&self) -> Vec<f32> {
        let mut edges = Vec::with_capacity(self.columns.len() + 1);
        let mut x = self.viewport.min_x();
        edges.push(x);
        for col in &self.columns {
            x += col.width * self.scale;
            edges.push(x);
        }
        edges
    }

    /// The screen-space rect of display position `d` (may be
    /// partially outside the viewport).
    fn row_rect(&self, d: usize) -> Rect {
        let row_px = self.row_px();
        Rect::new(
            self.viewport.min_x(),
            self.viewport.min_y() + d as f32 * row_px - self.scroll_y,
            self.viewport.width(),
            row_px,
        )
    }

    /// Display position under `position` (body hit test).
    fn row_display_at(&self, position: Vec2) -> Option<usize> {
        let row_px = self.row_px();
        if row_px <= 0.0 || !self.viewport.contains(position) {
            return None;
        }
        let d = ((position.y - self.viewport.min_y() + self.scroll_y) / row_px) as usize;
        (d < self.rows.len()).then_some(d)
    }

    /// Display position of storage `index`, if any.
    fn display_of(&self, storage: usize) -> Option<usize> {
        self.order.iter().position(|&s| s == storage)
    }

    /// Column index whose header cell interior contains `x`, if any.
    fn header_col_at(&self, x: f32) -> Option<usize> {
        let edges = self.col_edges();
        (0..self.columns.len()).find(|&i| x >= edges[i] && x < edges[i + 1])
    }

    /// Column whose right-edge separator is within the resize hit
    /// zone of `position`, if any.
    fn resize_edge_at(&self, position: Vec2) -> Option<usize> {
        let zone = EDGE_PT * self.scale;
        let edges = self.col_edges();
        edges
            .iter()
            .enumerate()
            .take(self.columns.len())
            .skip(1)
            .find(|&(_, &e)| (position.x - e).abs() <= zone)
            .map(|(i, _)| i - 1)
    }

    /// Whether `column` may drive sorting: the table gate AND the
    /// column flag.
    fn effective_sortable(&self, column: usize) -> bool {
        self.sortable && self.columns.get(column).is_some_and(|c| c.sortable)
    }

    /// Numeric-aware cell comparison for the sort permutation.
    fn cell_cmp(a: Option<&String>, b: Option<&String>) -> Ordering {
        let a = a.map_or("", String::as_str);
        let b = b.map_or("", String::as_str);
        match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
            (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            _ => a.cmp(b),
        }
    }

    /// Rebuilds the display→storage permutation from the current sort
    /// state. Stable: equal keys keep storage order.
    fn recompute_order(&mut self) {
        let mut order: Vec<usize> = (0..self.rows.len()).collect();
        if let Some(col) = self.sort_column {
            let dir = self.sort_dir;
            order.sort_by(|&a, &b| {
                let ord = Self::cell_cmp(self.rows[a].get(col), self.rows[b].get(col));
                match dir {
                    SortDir::Ascending => ord,
                    SortDir::Descending => ord.reverse(),
                }
            });
        }
        self.order = order;
    }

    /// Header click on a sortable column: asc → desc → none.
    fn cycle_sort(&mut self, column: usize) {
        if self.sort_column == Some(column) {
            match self.sort_dir {
                SortDir::Ascending => {
                    self.sort_dir = SortDir::Descending;
                }
                SortDir::Descending => {
                    self.sort_column = None;
                }
            }
        } else {
            self.sort_column = Some(column);
            self.sort_dir = SortDir::Ascending;
        }
        self.pending_sort = self.sort_column.map(|c| (c, self.sort_dir));
        self.recompute_order();
        self.sync_children();
    }

    /// Sets the scroll offset, clamped; mirrors state onto
    /// children/bars.
    fn set_scroll(&mut self, offset: f32) {
        let clamped = if offset.is_finite() { offset } else { 0.0 };
        self.scroll_y = clamped.clamp(0.0, self.max_scroll());
        self.sync_children();
        self.sync_bars();
    }

    /// Scrolls the minimum amount that makes display position `d`
    /// fully visible.
    fn ensure_visible(&mut self, d: usize) {
        let row_px = self.row_px();
        if row_px <= 0.0 {
            return;
        }
        let top = d as f32 * row_px;
        let bottom = top + row_px;
        if top < self.scroll_y {
            self.set_scroll(top);
        } else if bottom > self.scroll_y + self.viewport.height() {
            self.set_scroll(bottom - self.viewport.height());
        }
    }

    /// Moves focus+selection to display position `d` (clamped).
    fn move_focus_to(&mut self, d: usize) {
        let n = self.rows.len();
        if n == 0 {
            return;
        }
        let d = d.min(n - 1);
        let storage = self.order[d];
        self.focused = storage;
        self.selected = Some(storage);
        self.ensure_visible(d);
        self.sync_children();
    }

    /// Display position of the focus indicator, if rows exist.
    fn focused_display(&self) -> usize {
        self.display_of(self.focused).unwrap_or(0)
    }

    /// Rows moved by `PageUp`/`PageDown`.
    fn page_size(&self) -> usize {
        let row_px = self.row_px();
        if row_px <= 0.0 {
            return 1;
        }
        ((self.viewport.height() / row_px).floor() as usize).max(1)
    }

    /// Records an activation for [`take_activated`](Self::take_activated).
    fn activate(&mut self, storage: usize) {
        if storage < self.rows.len() {
            self.activated = Some(storage);
        }
    }

    /// The vertical scrollbar thumb rect, if the bar is shown.
    fn vbar_thumb(&self) -> Option<Rect> {
        let track = self.vbar_rect?;
        let content = self.content_height();
        if content <= 0.0 {
            return None;
        }
        let track_len = track.height();
        let frac = (self.viewport.height() / content).clamp(0.0, 1.0);
        let thumb_len = (track_len * frac)
            .max(MIN_THUMB * self.scale)
            .min(track_len);
        let max = self.max_scroll();
        let t = if max > 0.0 { self.scroll_y / max } else { 0.0 };
        let top = track.min_y() + t * (track_len - thumb_len);
        Some(Rect::new(track.min_x(), top, track.width(), thumb_len))
    }

    /// Maps a pointer position inside the bar track to a thumb grab
    /// or a page scroll. Returns `true` if the press was consumed.
    fn press_bar(&mut self, position: Vec2) -> bool {
        let Some(thumb) = self.vbar_thumb() else {
            return false;
        };
        if thumb.contains(position) {
            self.thumb_drag = Some(position.y - thumb.min_y());
        } else {
            // Track press: page toward the click.
            let sign = if position.y < thumb.min_y() {
                -1.0
            } else {
                1.0
            };
            self.scroll_by(sign * self.viewport.height() * 0.9);
        }
        true
    }

    /// Continues an active thumb drag.
    fn drag_thumb(&mut self, position: Vec2) {
        let Some(grab) = self.thumb_drag else {
            return;
        };
        let Some(track) = self.vbar_rect else {
            return;
        };
        let thumb_len = self.vbar_thumb().map(|t| t.height()).unwrap_or(0.0);
        let usable = (track.height() - thumb_len).max(f32::EPSILON);
        let frac = ((position.y - track.min_y() - grab) / usable).clamp(0.0, 1.0);
        self.set_scroll(frac * self.max_scroll());
    }

    /// Continues an active header-separator drag.
    fn drag_resize(&mut self, position: Vec2) {
        let Some(drag) = self.resize_drag else {
            return;
        };
        let edges = self.col_edges();
        let Some(&left) = edges.get(drag.column) else {
            return;
        };
        let new_px = position.x - drag.grab - left;
        let min = self.columns[drag.column].min_width.max(1.0);
        self.columns[drag.column].width = (new_px / self.scale.max(f32::EPSILON)).max(min);
    }

    /// Resizes/re-points the pooled row children at the visible
    /// window and mirrors owner state onto them and the headers.
    fn sync_children(&mut self) {
        let first = self.first_visible();
        let count = self
            .rows
            .len()
            .saturating_sub(first)
            .min(self.visible_capacity());
        self.row_children.resize_with(count, TableRowChild::new);
        let ncols = self.columns.len();
        let edges = self.col_edges();
        let row_px = self.row_px();
        let vp_min_x = self.viewport.min_x();
        let vp_min_y = self.viewport.min_y();
        let vp_w = self.viewport.width();
        let scroll_y = self.scroll_y;
        let selected = self.selected;
        let focused = self.focused;
        let enabled = self.enabled;
        for (k, row) in self.row_children.iter_mut().enumerate() {
            let d = first + k;
            let s = self.order.get(d).copied().unwrap_or(d);
            let cells = self.rows.get(s);
            row.display_index = d;
            row.storage_index = s;
            row.label = cells.map_or_else(String::new, |c| c.join(", "));
            row.selected = selected == Some(s);
            row.focused = s == focused;
            row.enabled = enabled;
            let rect = Rect::new(
                vp_min_x,
                vp_min_y + d as f32 * row_px - scroll_y,
                vp_w,
                row_px,
            );
            row.cells.resize_with(ncols, || TableCellChild {
                column_index: 0,
                label: String::new(),
                selected: false,
                rect: Rect::default(),
                enabled: true,
            });
            for (i, cell) in row.cells.iter_mut().enumerate() {
                cell.column_index = i;
                cell.label = cells.and_then(|c| c.get(i)).cloned().unwrap_or_default();
                cell.selected = row.selected;
                cell.enabled = enabled;
                let x0 = edges.get(i).copied().unwrap_or(rect.min_x());
                let x1 = edges.get(i + 1).copied().unwrap_or(rect.max_x());
                cell.rect = Rect::new(x0, rect.min_y(), (x1 - x0).max(0.0), row_px);
            }
        }
        self.header_children
            .resize_with(ncols, || TableHeaderChild {
                column_index: 0,
                label: String::new(),
                sort: None,
                enabled: true,
            });
        for (i, h) in self.header_children.iter_mut().enumerate() {
            h.column_index = i;
            h.label = self.columns[i].title.clone();
            h.sort = (self.sort_column == Some(i)).then_some(self.sort_dir);
            h.enabled = self.enabled;
        }
    }

    /// Mirrors scroll state onto the scrollbar child for its emitted
    /// `ScrollBar` node and thumb painting.
    fn sync_bars(&mut self) {
        self.vbar.offset = self.scroll_y;
        self.vbar.max_offset = self.max_scroll();
        self.vbar.thumb = self.vbar_thumb();
        self.vbar.active = self.thumb_drag.is_some();
    }
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Table {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let row_px = cx.pt(self.row_height);
        let content_h = self.rows.len() as f32 * row_px;
        let h = content_h.min(MAX_VISIBLE_ROWS * row_px) + cx.pt(self.header_height);
        // Width follows the declared column widths plus the potential
        // scrollbar; empty tables get a sane default.
        let w = self
            .columns
            .iter()
            .map(|c| c.width.max(c.min_width))
            .sum::<f32>()
            .max(120.0)
            * cx.scale
            + cx.pt(BAR);
        let max_w = constraints.max_size.x.max(0.0);
        let max_h = constraints.max_size.y.max(0.0);
        Vec2::new(
            w.clamp(cx.pt(96.0).min(max_w), max_w),
            h.clamp(cx.pt(64.0).min(max_h), max_h),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        // Header plus a couple of rows and the scrollbar strip.
        RenderMinimum::new(Vec2::new(96.0, 84.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.scale = cx.scale;
        self.vbar.scale = cx.scale;
        // Single keyboard-focus stop (roving tabindex over the rows).
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
        self.poll_pending();
        let header_px = cx.pt(self.header_height).min(bounds.height());
        let bar = cx.pt(BAR);
        self.header_rect = Rect::new(bounds.min_x(), bounds.min_y(), bounds.width(), header_px);
        let body_h = (bounds.height() - header_px).max(0.0);
        // Smart scrollbar: shown only when the body overflows.
        let show_v = self.content_height() > body_h;
        let mut viewport = Rect::new(
            bounds.min_x(),
            bounds.min_y() + header_px,
            bounds.width(),
            body_h,
        );
        self.vbar_rect = None;
        if show_v {
            viewport.size.x = (viewport.width() - bar).max(0.0);
            self.vbar_rect = Some(Rect::new(
                bounds.max_x() - bar,
                viewport.min_y(),
                bar,
                body_h,
            ));
        }
        self.viewport = viewport;
        self.vbar.shown = show_v;
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());
        self.sync_children();
        // Lay out pooled children so their layout pass runs with
        // current bounds (`child_bounds` derives them arithmetically).
        let first = self.first_visible();
        let nrows = self.row_children.len();
        for k in 0..nrows {
            let rect = self.row_rect(first + k);
            cx.layout_child(&mut self.row_children[k], rect);
        }
        let ncols = self.header_children.len();
        let edges = self.col_edges();
        for i in 0..ncols {
            let rect = Rect::new(
                edges[i],
                self.header_rect.min_y(),
                (edges[i + 1] - edges[i]).max(0.0),
                self.header_rect.height(),
            );
            cx.layout_child(&mut self.header_children[i], rect);
        }
        if let Some(rect) = self.vbar_rect {
            cx.layout_child(&mut self.vbar, rect);
        }
        self.sync_bars();
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Table);
        node.set_row_count(self.rows.len());
        node.set_column_count(self.columns.len());
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        let max = self.max_scroll();
        node.set_scroll_y(f64::from(self.scroll_y));
        node.set_scroll_y_min(0.0);
        node.set_scroll_y_max(f64::from(max));
        node.add_action(accesskit::Action::ScrollUp);
        node.add_action(accesskit::Action::ScrollDown);
        node.add_action(accesskit::Action::SetScrollOffset);
        node.add_action(accesskit::Action::Focus);
        node.add_child_action(accesskit::Action::ScrollIntoView);
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn a11y_prepare(&mut self) {
        self.poll_pending();
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        self.poll_pending();
        match cx.event {
            WidgetEvent::PointerPressed {
                position,
                button: PointerButton::Primary,
                count,
            } => {
                if self.vbar_rect.is_some_and(|r| r.contains(*position)) {
                    self.press_bar(*position);
                    return EventResponse::CapturePointer;
                }
                if self.header_rect.contains(*position) {
                    // Separator edge zones win over sort clicks.
                    if let Some(col) = self.resize_edge_at(*position) {
                        let edges = self.col_edges();
                        self.resize_drag = Some(ResizeDrag {
                            column: col,
                            grab: position.x - edges[col + 1],
                        });
                        return EventResponse::CapturePointer;
                    }
                    if let Some(col) = self.header_col_at(position.x) {
                        if self.effective_sortable(col) {
                            self.cycle_sort(col);
                            return EventResponse::RequestRepaint;
                        }
                    }
                    return EventResponse::Handled;
                }
                if let Some(d) = self.row_display_at(*position) {
                    let storage = self.order[d];
                    if *count >= 2 {
                        self.select(storage);
                        self.activate(storage);
                    } else {
                        self.select(storage);
                    }
                    // Press-drags move the selection — capture so
                    // tracking continues outside the bounds.
                    self.dragging = true;
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if self.resize_drag.is_some() {
                    self.drag_resize(*position);
                    return EventResponse::RequestRepaint;
                }
                if self.thumb_drag.is_some() {
                    self.drag_thumb(*position);
                    return EventResponse::RequestRepaint;
                }
                if self.dragging {
                    if let Some(d) = self.row_display_at(*position) {
                        self.select(self.order[d]);
                    }
                    return EventResponse::RequestRepaint;
                }
                let hov = self.row_display_at(*position).map(|d| self.order[d]);
                if hov != self.hovered {
                    self.hovered = hov;
                    self.sync_children();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.resize_drag.take().is_some() {
                    return EventResponse::ReleasePointer;
                }
                if self.thumb_drag.take().is_some() {
                    self.sync_bars();
                    return EventResponse::ReleasePointer;
                }
                if self.dragging {
                    self.dragging = false;
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
                    self.sync_children();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::Scroll { delta, .. } => {
                // Nested chaining: an unconsumed delta returns
                // `Ignored` so an ancestor scroll region can take it.
                let applied = self.scroll_by(delta.y);
                if applied != 0.0 {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowUp" => {
                    let d = self.focused_display().saturating_sub(1);
                    self.move_focus_to(d);
                    EventResponse::RequestRepaint
                }
                "ArrowDown" => {
                    let d = self.focused_display().saturating_add(1);
                    self.move_focus_to(d);
                    EventResponse::RequestRepaint
                }
                "PageUp" => {
                    let d = self.focused_display().saturating_sub(self.page_size());
                    self.move_focus_to(d);
                    EventResponse::RequestRepaint
                }
                "PageDown" => {
                    let d = self.focused_display().saturating_add(self.page_size());
                    self.move_focus_to(d);
                    EventResponse::RequestRepaint
                }
                "Home" => {
                    self.move_focus_to(0);
                    EventResponse::RequestRepaint
                }
                "End" => {
                    self.move_focus_to(self.rows.len().saturating_sub(1));
                    EventResponse::RequestRepaint
                }
                "Enter" => {
                    self.activate(self.focused);
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::FocusGained => {
                self.has_focus = true;
                self.sync_children();
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.has_focus = false;
                // Don't leak a stuck drag into the next focus session.
                self.dragging = false;
                self.resize_drag = None;
                self.sync_children();
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::Focus => EventResponse::CaptureFocus,
                SemanticAction::Click => {
                    self.activate(self.focused);
                    EventResponse::Handled
                }
                SemanticAction::ScrollUp => {
                    self.scroll_by(-self.row_px());
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollDown => {
                    self.scroll_by(self.row_px());
                    EventResponse::RequestRepaint
                }
                SemanticAction::SetScrollOffset(offset) => {
                    self.set_scroll(offset.y);
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollToPoint(point) => {
                    // Point is in this widget's coordinate space.
                    let d = ((point.y - self.viewport.min_y() + self.scroll_y)
                        / self.row_px().max(f32::EPSILON))
                    .max(0.0) as usize;
                    self.ensure_visible(d.min(self.rows.len().saturating_sub(1)));
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollIntoView => {
                    self.ensure_visible(self.focused_display());
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let widget_rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let surface = cx.color(TokenKey::SurfaceColor, SURFACE_BG);
        let border = cx.color(TokenKey::BorderColor, BORDER);
        let divider = cx.color(TokenKey::DividerColor, TRACK_COLOR);
        let accent = cx.color(TokenKey::AccentColor, ACCENT);
        let ink = if self.enabled {
            cx.color(TokenKey::TextColor, INK)
        } else {
            cx.color(TokenKey::TextMutedColor, INK_DISABLED)
        };
        let muted = cx.color(TokenKey::TextMutedColor, INK_DISABLED);
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let font_px = cx.pt(CELL_FONT);
        let pad = f64::from(cx.pt(CELL_PAD));

        // The table face.
        cx.list.push_fill_rect(widget_rect, surface);

        // --- Body rows, clipped to the viewport so a partially
        // scrolled row never bleeds into the pinned header. ---
        let vp = kurbo::Rect::new(
            f64::from(self.viewport.min_x()),
            f64::from(self.viewport.min_y()),
            f64::from(self.viewport.max_x()),
            f64::from(self.viewport.max_y()),
        );
        cx.list.push_clip(vp);
        let edges = self.col_edges();
        let ncols = self.columns.len();
        for d in self.visible_range() {
            let s = self.order[d];
            let r = self.row_rect(d);
            let rect = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            if self.selected == Some(s) {
                cx.list
                    .push_fill_rect(rect, [accent[0], accent[1], accent[2], SELECTED_ALPHA]);
            } else if self.hovered == Some(s) {
                cx.list
                    .push_fill_rect(rect, [accent[0], accent[1], accent[2], HOVER_ALPHA]);
            } else if self.striped && d % 2 == 1 {
                cx.list
                    .push_fill_rect(rect, [muted[0], muted[1], muted[2], STRIPE_ALPHA]);
            }
            if self.grid_lines {
                cx.list.push_stroke_rect(
                    kurbo::Rect::new(rect.x0, rect.y1, rect.x1, rect.y1),
                    cx.pt(1.0),
                    divider,
                );
            }
            // Cells.
            for (i, col) in self.columns.iter().enumerate() {
                let cell = kurbo::Rect::new(
                    f64::from(edges[i]),
                    f64::from(r.min_y()),
                    f64::from(edges[i + 1]),
                    f64::from(r.max_y()),
                );
                if self.grid_lines && i + 1 < ncols {
                    cx.list.push_stroke_rect(
                        kurbo::Rect::new(cell.x1, cell.y0, cell.x1, cell.y1),
                        cx.pt(1.0),
                        divider,
                    );
                }
                let text = self.rows[s].get(i).map_or("", String::as_str);
                if text.is_empty() {
                    continue;
                }
                let text_w = painter
                    .and_then(|p| p.measure_text(text, font_px))
                    .unwrap_or_else(|| text.chars().count() as f32 * cx.pt(7.0))
                    .max(0.0);
                let x0 = match col.align {
                    TableAlign::Start => cell.x0 + pad,
                    TableAlign::End => cell.x1 - pad - f64::from(text_w),
                    TableAlign::Center => cell.x0 + ((cell.x1 - cell.x0) - f64::from(text_w)) / 2.0,
                };
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    cell,
                    kurbo::Point::new(
                        x0,
                        f64::from(r.min_y()) + (rect.height() - f64::from(font_px)) / 2.0,
                    ),
                    text,
                    font_px,
                    ink,
                );
            }
            // Roving focus indicator.
            if s == self.focused && self.has_focus {
                cx.list.push_stroke_rect(
                    rect,
                    cx.pt(2.0),
                    [accent[0], accent[1], accent[2], FOCUS_ALPHA],
                );
            }
        }
        cx.list.pop_clip();

        // --- Pinned header. ---
        let hr = kurbo::Rect::new(
            f64::from(self.header_rect.min_x()),
            f64::from(self.header_rect.min_y()),
            f64::from(self.header_rect.max_x()),
            f64::from(self.header_rect.max_y()),
        );
        cx.list
            .push_fill_rect(hr, cx.color(TokenKey::SurfaceColor, HEADER_BG));
        // Header underline separating it from the body.
        cx.list.push_stroke_rect(
            kurbo::Rect::new(hr.x0, hr.y1, hr.x1, hr.y1),
            cx.pt(1.0),
            divider,
        );
        let header_font = cx.pt(HEADER_FONT);
        for (i, col) in self.columns.iter().enumerate() {
            let cell = kurbo::Rect::new(
                f64::from(edges[i]),
                f64::from(self.header_rect.min_y()),
                f64::from(edges[i + 1]),
                f64::from(self.header_rect.max_y()),
            );
            // Sort indicator: a small triangle at the trailing edge.
            let sorted = self.sort_column == Some(i);
            if sorted {
                let w = f64::from(cx.pt(9.0));
                let h = f64::from(cx.pt(5.0));
                let mid_x = cell.x1 - f64::from(cx.pt(6.0)) - w / 2.0;
                let mid_y = (cell.y0 + cell.y1) / 2.0;
                let mut tri = BezPath::new();
                match self.sort_dir {
                    SortDir::Ascending => {
                        tri.move_to((mid_x - w / 2.0, mid_y + h / 2.0));
                        tri.line_to((mid_x, mid_y - h / 2.0));
                        tri.line_to((mid_x + w / 2.0, mid_y + h / 2.0));
                    }
                    SortDir::Descending => {
                        tri.move_to((mid_x - w / 2.0, mid_y - h / 2.0));
                        tri.line_to((mid_x, mid_y + h / 2.0));
                        tri.line_to((mid_x + w / 2.0, mid_y - h / 2.0));
                    }
                }
                tri.close_path();
                cx.list.push_path(tri, accent);
            }
            // Reserve room for the arrow in the text clip.
            let clip = if sorted {
                kurbo::Rect::new(cell.x0, cell.y0, cell.x1 - f64::from(cx.pt(18.0)), cell.y1)
            } else {
                cell
            };
            let title_w = painter
                .and_then(|p| p.measure_text(&col.title, header_font))
                .unwrap_or_else(|| col.title.chars().count() as f32 * cx.pt(7.0))
                .max(0.0);
            let x0 = match col.align {
                TableAlign::Start => cell.x0 + pad,
                TableAlign::End => cell.x1 - pad - f64::from(title_w),
                TableAlign::Center => cell.x0 + ((cell.x1 - cell.x0) - f64::from(title_w)) / 2.0,
            };
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(
                    x0,
                    f64::from(self.header_rect.min_y())
                        + (hr.height() - f64::from(header_font)) / 2.0,
                ),
                &col.title,
                header_font,
                ink,
            );
            // Column separator (skip the leftmost edge — the border
            // already delimits it).
            if i > 0 {
                let sep = kurbo::Rect::new(cell.x0, cell.y0, cell.x0, cell.y1);
                let color = if self.resize_drag.is_some_and(|d| d.column == i - 1) {
                    accent
                } else {
                    divider
                };
                cx.list.push_stroke_rect(sep, cx.pt(1.0), color);
            }
        }

        // Focus ring around the table when it holds focus and the
        // focused row is scrolled out (rows draw their own ring).
        cx.list.push_stroke_rect(widget_rect, cx.pt(1.0), border);
    }

    fn child_count(&self) -> usize {
        self.header_children.len() + self.row_children.len() + 1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        let ncols = self.header_children.len();
        let nrows = self.row_children.len();
        if index < ncols {
            self.header_children.get(index).map(|h| h as &dyn Widget)
        } else if index < ncols + nrows {
            self.row_children
                .get(index - ncols)
                .map(|r| r as &dyn Widget)
        } else if index == ncols + nrows {
            Some(&self.vbar)
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        let ncols = self.header_children.len();
        let nrows = self.row_children.len();
        if index < ncols {
            self.header_children
                .get_mut(index)
                .map(|h| h as &mut dyn Widget)
        } else if index < ncols + nrows {
            self.row_children
                .get_mut(index - ncols)
                .map(|r| r as &mut dyn Widget)
        } else if index == ncols + nrows {
            Some(&mut self.vbar)
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        let ncols = self.header_children.len();
        let nrows = self.row_children.len();
        if index < ncols {
            let edges = self.col_edges();
            let (Some(&x0), Some(&x1)) = (edges.get(index), edges.get(index + 1)) else {
                return None;
            };
            Some(Rect::new(
                x0,
                self.header_rect.min_y(),
                (x1 - x0).max(0.0),
                self.header_rect.height(),
            ))
        } else if index < ncols + nrows {
            self.row_children
                .get(index - ncols)
                .map(|r| self.row_rect(r.display_index))
        } else if index == ncols + nrows {
            self.vbar_rect
        } else {
            None
        }
    }
}

impl std::fmt::Debug for Table {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Table")
            .field("columns", &self.columns.len())
            .field("rows", &self.rows.len())
            .field("selected", &self.selected)
            .field("focused", &self.focused)
            .field("sort", &self.sort_state())
            .field("scroll_y", &self.scroll_y)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(t: &mut Table, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        t.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn make_table(rows: usize, w: f32, h: f32) -> Table {
        let mut t = Table::new()
            .columns([
                TableColumn::new("name", "Name").width(120.0),
                TableColumn::new("num", "Num").width(80.0),
            ])
            .striped(true)
            .grid_lines(true);
        for i in 0..rows {
            t.add_row([format!("Item {i}"), format!("{i}")]);
        }
        laid_out(&mut t, w, h);
        t
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn event(t: &mut Table, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: t.cached_bounds,
            scale: 1.0,
        };
        t.event(&mut cx)
    }

    fn press(x: f32, y: f32, count: u8) -> WidgetEvent {
        WidgetEvent::PointerPressed {
            position: Vec2::new(x, y),
            button: PointerButton::Primary,
            count,
        }
    }

    #[test]
    fn builder_columns_and_rows() {
        let t = Table::new()
            .columns([
                TableColumn::new("a", "Alpha").width(100.0).min_width(30.0),
                TableColumn::new("b", "Beta")
                    .sortable(false)
                    .align(TableAlign::End),
            ])
            .row(["x", "1"])
            .row(["y", "2"])
            .label("Grid")
            .striped(true)
            .grid_lines(true);
        assert_eq!(t.column_count(), 2);
        assert_eq!(t.row_count(), 2);
        assert_eq!(t.column(0).unwrap().id, "a");
        assert_eq!(t.column(1).unwrap().align, TableAlign::End);
        assert!(!t.column(1).unwrap().sortable);
        assert_eq!(t.cell(1, 0), Some("y"));
        assert!(t.striped && t.grid_lines);
        assert_eq!(t.label.as_deref(), Some("Grid"));
    }

    #[test]
    fn header_click_cycles_sort() {
        let mut t = make_table(4, 400.0, 300.0);
        assert_eq!(t.sort_state(), None);
        // Header is the top 28pt; column 0 spans x in 0..120.
        assert_eq!(
            event(&mut t, &press(40.0, 14.0, 1)),
            EventResponse::RequestRepaint
        );
        assert_eq!(t.sort_state(), Some((0, SortDir::Ascending)));
        assert_eq!(t.take_sort(), Some((0, SortDir::Ascending)));
        assert_eq!(t.take_sort(), None);
        event(&mut t, &press(40.0, 14.0, 1));
        assert_eq!(t.sort_state(), Some((0, SortDir::Descending)));
        event(&mut t, &press(40.0, 14.0, 1));
        assert_eq!(t.sort_state(), None); // third click clears
    }

    #[test]
    fn sorted_rows_permutation() {
        let mut t = Table::new()
            .columns([TableColumn::new("a", "A")])
            .row(["b"])
            .row(["a"])
            .row(["c"]);
        laid_out(&mut t, 200.0, 200.0);
        assert_eq!(t.sorted_rows(), vec![0, 1, 2]);
        t.set_sort(0, SortDir::Ascending);
        assert_eq!(t.sorted_rows(), vec![1, 0, 2]);
        t.set_sort(0, SortDir::Descending);
        assert_eq!(t.sorted_rows(), vec![2, 0, 1]);
        t.clear_sort();
        assert_eq!(t.sorted_rows(), vec![0, 1, 2]);
        // Out-of-range column is ignored.
        t.set_sort(9, SortDir::Ascending);
        assert_eq!(t.sort_state(), None);
    }

    #[test]
    fn sort_is_numeric_aware() {
        let mut t = Table::new()
            .columns([TableColumn::new("n", "N")])
            .row(["10"])
            .row(["9"])
            .row(["100"]);
        laid_out(&mut t, 200.0, 200.0);
        t.set_sort(0, SortDir::Ascending);
        assert_eq!(t.sorted_rows(), vec![1, 0, 2]); // 9 < 10 < 100
    }

    #[test]
    fn unsortable_column_click_is_inert() {
        let mut t = Table::new()
            .columns([TableColumn::new("a", "A").sortable(false)])
            .row(["x"]);
        laid_out(&mut t, 200.0, 200.0);
        assert_eq!(event(&mut t, &press(40.0, 14.0, 1)), EventResponse::Handled);
        assert_eq!(t.sort_state(), None);
        // The table-wide gate also wins.
        let mut t = Table::new()
            .columns([TableColumn::new("a", "A")])
            .sortable(false)
            .row(["x"]);
        laid_out(&mut t, 200.0, 200.0);
        event(&mut t, &press(40.0, 14.0, 1));
        assert_eq!(t.sort_state(), None);
    }

    #[test]
    fn click_selects_storage_row() {
        let mut t = make_table(10, 300.0, 200.0);
        // Body starts at y=28; row height 28 → click at y=70 is
        // display row 1.
        assert_eq!(
            event(&mut t, &press(30.0, 70.0, 1)),
            EventResponse::CapturePointer
        );
        assert_eq!(t.selected(), Some(1));
        assert_eq!(t.focused_index(), 1);
    }

    #[test]
    fn click_maps_through_sort() {
        let mut t = Table::new()
            .columns([TableColumn::new("a", "A")])
            .row(["b"])
            .row(["a"])
            .row(["c"]);
        laid_out(&mut t, 200.0, 200.0);
        t.set_sort(0, SortDir::Ascending);
        // Display order: a(1), b(0), c(2). Click display row 0 →
        // storage 1.
        event(&mut t, &press(30.0, 30.0, 1));
        assert_eq!(t.selected(), Some(1));
    }

    #[test]
    fn keyboard_nav_moves_selection() {
        let mut t = make_table(20, 300.0, 140.0); // 4 visible rows (112/28)
        event(&mut t, &key("ArrowDown"));
        assert_eq!(t.selected(), Some(1));
        event(&mut t, &key("ArrowDown"));
        assert_eq!(t.selected(), Some(2));
        event(&mut t, &key("ArrowUp"));
        assert_eq!(t.selected(), Some(1));
        event(&mut t, &key("ArrowUp"));
        event(&mut t, &key("ArrowUp")); // clamps at 0
        assert_eq!(t.selected(), Some(0));
        event(&mut t, &key("End"));
        assert_eq!(t.selected(), Some(19));
        event(&mut t, &key("Home"));
        assert_eq!(t.selected(), Some(0));
        event(&mut t, &key("PageDown"));
        assert_eq!(t.selected(), Some(4));
        event(&mut t, &key("PageUp"));
        assert_eq!(t.selected(), Some(0));
    }

    #[test]
    fn keyboard_nav_keeps_row_visible() {
        let mut t = make_table(100, 300.0, 140.0);
        for _ in 0..6 {
            event(&mut t, &key("ArrowDown"));
        }
        assert_eq!(t.selected(), Some(6));
        assert!(t.visible_range().contains(&6));
        assert!(t.scroll_offset() > 0.0);
    }

    #[test]
    fn enter_and_double_click_activate() {
        let mut t = make_table(10, 300.0, 200.0);
        t.select(3);
        event(&mut t, &key("Enter"));
        assert_eq!(t.take_activated(), Some(3));
        assert_eq!(t.take_activated(), None);
        // Double-click on display row 1.
        assert_eq!(
            event(&mut t, &press(30.0, 60.0, 2)),
            EventResponse::CapturePointer
        );
        assert_eq!(t.take_activated(), Some(1));
    }

    #[test]
    fn separator_drag_resizes_column() {
        let mut t = make_table(5, 300.0, 200.0);
        assert_eq!(t.column(0).unwrap().width, 120.0);
        // Separator between col 0 (120pt) and col 1 sits at x=120.
        assert_eq!(
            event(&mut t, &press(121.0, 14.0, 1)),
            EventResponse::CapturePointer
        );
        let moved = WidgetEvent::PointerMoved {
            position: Vec2::new(170.0, 14.0),
        };
        assert_eq!(event(&mut t, &moved), EventResponse::RequestRepaint);
        // Grab offset (1px right of the edge) is preserved: 170-1.
        assert_eq!(t.column(0).unwrap().width, 169.0);
        let release = WidgetEvent::PointerReleased {
            position: Vec2::new(170.0, 14.0),
            button: PointerButton::Primary,
        };
        assert_eq!(event(&mut t, &release), EventResponse::ReleasePointer);
    }

    #[test]
    fn resize_clamps_to_min_width() {
        let mut t = Table::new()
            .columns([TableColumn::new("a", "A").width(100.0).min_width(60.0)])
            .row(["x"]);
        laid_out(&mut t, 300.0, 200.0);
        // Only one column → no internal separator. Add a second col.
        t.set_columns([
            TableColumn::new("a", "A").width(100.0).min_width(60.0),
            TableColumn::new("b", "B"),
        ]);
        laid_out(&mut t, 300.0, 200.0);
        event(&mut t, &press(101.0, 14.0, 1));
        let moved = WidgetEvent::PointerMoved {
            position: Vec2::new(10.0, 14.0),
        };
        event(&mut t, &moved);
        assert_eq!(t.column(0).unwrap().width, 60.0);
    }

    #[test]
    fn wheel_scrolls_and_clamps() {
        let mut t = make_table(100, 300.0, 140.0);
        let body = 140.0 - 28.0;
        assert_eq!(t.max_scroll_offset(), 100.0 * 28.0 - body);
        let ev = WidgetEvent::Scroll {
            position: Vec2::new(10.0, 60.0),
            delta: Vec2::new(0.0, 56.0),
        };
        assert_eq!(event(&mut t, &ev), EventResponse::RequestRepaint);
        assert_eq!(t.scroll_offset(), 56.0);
        assert_eq!(t.visible_range().start, 2);
        // Unconsumed at the bottom → chains to an ancestor.
        t.set_scroll_offset(t.max_scroll_offset());
        assert_eq!(event(&mut t, &ev), EventResponse::Ignored);
    }

    #[test]
    fn rows_virtualize_to_viewport() {
        let t = make_table(10_000, 300.0, 140.0);
        assert_eq!(t.visible_range(), 0..4); // 112pt body / 28pt rows
        assert_eq!(t.row_children.len(), 5); // capacity + 1 partial
        assert_eq!(t.row_count(), 10_000);
    }

    #[test]
    fn disabled_ignores_input() {
        let mut t = make_table(10, 300.0, 200.0).enabled(false);
        assert_eq!(event(&mut t, &press(30.0, 60.0, 1)), EventResponse::Ignored);
        assert_eq!(event(&mut t, &press(40.0, 14.0, 1)), EventResponse::Ignored);
        assert_eq!(event(&mut t, &key("ArrowDown")), EventResponse::Ignored);
        assert_eq!(t.selected(), None);
        assert_eq!(t.sort_state(), None);
    }

    #[test]
    fn set_rows_clamps_selection() {
        let mut t = make_table(10, 300.0, 200.0);
        t.select(8);
        t.set_rows([vec!["only".to_string()]]);
        assert_eq!(t.selected(), None);
        assert_eq!(t.row_count(), 1);
        assert_eq!(t.focused_index(), 0);
    }

    #[test]
    fn table_accessibility_contract() {
        let mut t = make_table(10, 300.0, 140.0).label("People");
        t.select(1);
        t.set_sort(0, SortDir::Ascending);
        laid_out(&mut t, 300.0, 140.0);
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        t.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Table);
        assert_eq!(node.label(), Some("People"));
        assert_eq!(node.row_count(), Some(10));
        assert_eq!(node.column_count(), Some(2));
        assert!(node.supports_action(accesskit::Action::SetScrollOffset));

        // Children: 2 headers + pooled rows + scrollbar.
        let ncols = 2;
        let mut h = AccessKitNode::new(accesskit::Role::Unknown);
        t.child(0).unwrap().accessibility(&mut h);
        assert_eq!(h.role(), accesskit::Role::ColumnHeader);
        assert_eq!(h.label(), Some("Name"));
        assert_eq!(
            h.sort_direction(),
            Some(accesskit::SortDirection::Ascending)
        );

        // First row child: Role::Row with row_index, cells inside.
        let row = t.child(ncols).unwrap();
        let mut r = AccessKitNode::new(accesskit::Role::Unknown);
        row.accessibility(&mut r);
        assert_eq!(r.role(), accesskit::Role::Row);
        assert_eq!(r.row_index(), Some(0));
        let cell = row.child(0).unwrap();
        let mut c = AccessKitNode::new(accesskit::Role::Unknown);
        cell.accessibility(&mut c);
        assert_eq!(c.role(), accesskit::Role::Cell);
        assert_eq!(c.column_index(), Some(0));

        // Scrollbar is the last child.
        let last = t.child(t.child_count() - 1).unwrap();
        let mut bar = AccessKitNode::new(accesskit::Role::Unknown);
        last.accessibility(&mut bar);
        assert_eq!(bar.role(), accesskit::Role::ScrollBar);
    }

    #[test]
    fn pending_press_from_row_child() {
        let mut t = make_table(10, 300.0, 140.0);
        let ev = WidgetEvent::SemanticAction(SemanticAction::Click);
        let ncols = 2;
        let row = t.child_mut(ncols + 2).unwrap();
        let mut cx = EventContext {
            event: &ev,
            bounds: Rect::default(),
            scale: 1.0,
        };
        assert_eq!(row.event(&mut cx), EventResponse::CaptureFocus);
        t.poll_pending();
        assert_eq!(t.selected(), Some(2));
    }

    #[test]
    fn semantic_scroll_actions() {
        let mut t = make_table(100, 300.0, 140.0);
        let ev = WidgetEvent::SemanticAction(SemanticAction::ScrollDown);
        event(&mut t, &ev);
        assert_eq!(t.scroll_offset(), 28.0);
        let ev = WidgetEvent::SemanticAction(SemanticAction::SetScrollOffset(Vec2::new(0.0, 84.0)));
        event(&mut t, &ev);
        assert_eq!(t.scroll_offset(), 84.0);
    }

    #[test]
    fn scrollbar_shows_on_overflow_and_drags() {
        let mut t = make_table(100, 300.0, 140.0);
        let track = t.vbar_rect.expect("overflow shows the bar");
        // Short table: no bar.
        let short = make_table(2, 300.0, 140.0);
        assert!(short.vbar_rect.is_none());
        // Grab the thumb and drag to the bottom → max offset.
        let thumb = t.vbar_thumb().unwrap();
        assert_eq!(
            event(&mut t, &press(track.min_x() + 5.0, thumb.min_y() + 5.0, 1)),
            EventResponse::CapturePointer
        );
        let moved = WidgetEvent::PointerMoved {
            position: Vec2::new(track.min_x() + 5.0, track.max_y() - 1.0),
        };
        event(&mut t, &moved);
        assert_eq!(t.scroll_offset(), t.max_scroll_offset());
    }
}
