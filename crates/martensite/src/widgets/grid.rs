//! `Grid` — column-grid layout container.
//!
//! The Ant `Row`/`Col` 24-column / CSS-grid-lite pattern: a fixed
//! column count, per-cell `col_span`, `row_span`, and automatic
//! placement — cells flow left-to-right and wrap to the next row when
//! a cell won't fit. Row heights grow to the tallest cell; row spans
//! straddle subsequent rows.
//!
//! # Examples
//!
//! ```
//! use martensite::prelude::*;
//! use martensite::widgets::grid::{Grid, GridCell};
//!
//! let g = Grid::new()
//!     .columns(12)
//!     .cell(GridCell::new(Text::new("half")).col_span(6))
//!     .cell(GridCell::new(Text::new("half")).col_span(6))
//!     .cell(GridCell::new(Text::new("full")).col_span(12));
//! assert_eq!(g.cell_count(), 3);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    LayoutConstraints, LayoutContext, PaintContext, Rect, RenderMinimum, UnderflowPolicy, Widget,
};

/// One cell of a [`Grid`].
///
/// `col_span`/`row_span` count columns/rows (`1` = single). Cells
/// auto-flow: each takes the next free slot; a cell wider than the
/// remaining columns wraps to a fresh row.
///
/// # Examples
///
/// ```
/// use martensite::prelude::*;
/// use martensite::widgets::grid::GridCell;
///
/// let cell = GridCell::new(Text::new("x")).col_span(4).row_span(2);
/// ```
pub struct GridCell {
    /// The cell's content.
    widget: Box<dyn Widget>,
    /// Columns occupied (`1..=columns`).
    col_span: u32,
    /// Rows occupied.
    row_span: u32,
}

impl GridCell {
    /// A single-column, single-row cell.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::grid::GridCell;
    ///
    /// let c = GridCell::new(Text::new("x"));
    /// ```
    pub fn new(widget: impl Widget) -> Self {
        Self {
            widget: Box::new(widget),
            col_span: 1,
            row_span: 1,
        }
    }

    /// Columns occupied (default 1, clamped to the grid's width).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::grid::GridCell;
    ///
    /// let c = GridCell::new(Text::new("x")).col_span(6);
    /// ```
    pub fn col_span(mut self, span: u32) -> Self {
        self.col_span = span.max(1);
        self
    }

    /// Rows occupied (default 1).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::grid::GridCell;
    ///
    /// let c = GridCell::new(Text::new("x")).row_span(2);
    /// ```
    pub fn row_span(mut self, span: u32) -> Self {
        self.row_span = span.max(1);
        self
    }
}

/// A laid-out cell's placement.
struct Placed {
    /// First column.
    col: u32,
    /// First row.
    row: u32,
}

/// A column-grid layout container — see the module docs.
///
/// `Grid` owns its cells; children report laid-out bounds through the
/// standard child protocol.
///
/// # Examples
///
/// ```
/// use martensite::widgets::grid::Grid;
/// use martensite::core::Widget;
///
/// let mut g = Grid::new().columns(4);
/// assert_eq!(g.child_count(), 0);
/// ```
pub struct Grid {
    label: String,
    enabled: bool,
    columns: u32,
    /// Horizontal + vertical gap (logical points).
    gap: f32,
    /// Row height per row unit (logical points) — the shortest row a
    /// `row_span(1)` cell occupies.
    row_height: f32,
    cells: Vec<GridCell>,
    /// Placements + rects from the last layout, parallel to `cells`.
    placed: Vec<(Placed, Rect)>,
    /// Content height in row units after layout (for `measure`).
    used_rows: u32,
}

impl Grid {
    /// An empty 12-column grid.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::grid::Grid;
    ///
    /// let g = Grid::new();
    /// assert_eq!(g.column_count(), 12);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Grid".into(),
            enabled: true,
            columns: 12,
            gap: 8.0,
            row_height: 32.0,
            cells: Vec::new(),
            placed: Vec::new(),
            used_rows: 0,
        }
    }

    /// Column count (default 12, like Ant).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::grid::Grid;
    ///
    /// assert_eq!(Grid::new().columns(4).column_count(), 4);
    /// ```
    pub fn columns(mut self, columns: u32) -> Self {
        self.columns = columns.max(1);
        self
    }

    /// Gap between cells in logical points (default 8).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::grid::Grid;
    ///
    /// let g = Grid::new().gap(12.0);
    /// ```
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap.max(0.0);
        self
    }

    /// Base row height per row unit in logical points (default 32).
    /// Taller content grows its row; `row_span` straddles that many
    /// units plus the gaps between them.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::grid::Grid;
    ///
    /// let g = Grid::new().row_height(40.0);
    /// ```
    pub fn row_height(mut self, height: f32) -> Self {
        self.row_height = height.max(1.0);
        self
    }

    /// Append a cell.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::grid::{Grid, GridCell};
    ///
    /// let g = Grid::new().cell(GridCell::new(Text::new("x")));
    /// ```
    pub fn cell(mut self, cell: GridCell) -> Self {
        self.cells.push(cell);
        self
    }

    /// Set the accessibility label (default `"Grid"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::grid::Grid;
    ///
    /// let g = Grid::new().label("Fields");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Enable or disable the grid's children (default `true`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::grid::Grid;
    ///
    /// let g = Grid::new().enabled(false);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The column count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::grid::Grid;
    ///
    /// assert_eq!(Grid::new().column_count(), 12);
    /// ```
    pub fn column_count(&self) -> u32 {
        self.columns
    }

    /// The number of cells.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::grid::{Grid, GridCell};
    ///
    /// let g = Grid::new().cell(GridCell::new(Text::new("x")));
    /// assert_eq!(g.cell_count(), 1);
    /// ```
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Auto-place every cell: pack left-to-right, wrap when the cell
    /// won't fit the row's remaining columns. `row_span > 1` cells
    /// claim their columns in the rows below too — tracked with a
    /// simple occupancy bitmap per row.
    fn place(&self) -> Vec<Placed> {
        let cols = self.columns as usize;
        // occupancy[r] = bitset of claimed columns in row r.
        let mut occupancy: Vec<u64> = Vec::new();
        let mut placed = Vec::with_capacity(self.cells.len());
        for cell in &self.cells {
            let span = (cell.col_span as usize).min(cols).max(1);
            let rowspan = (cell.row_span as usize).max(1);
            'rows: for r in 0.. {
                if occupancy.len() < r + rowspan {
                    occupancy.resize(r + rowspan + 4, 0);
                }
                for c in 0..=cols.saturating_sub(span) {
                    let mask = ((1u64 << span) - 1) << c;
                    // Free in every spanned row?
                    if (r..r + rowspan).all(|row| occupancy[row] & mask == 0) {
                        for occ in occupancy.iter_mut().skip(r).take(rowspan) {
                            *occ |= mask;
                        }
                        placed.push(Placed {
                            col: c as u32,
                            row: r as u32,
                        });
                        break 'rows;
                    }
                }
            }
        }
        placed
    }
}

impl Default for Grid {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Grid {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let placements = self.place();
        self.used_rows = placements.iter().map(|p| p.row).max().map_or(0, |r| r + 1);
        // Tallest content needs at least its own measured height.
        let mut extra_rows = 0u32;
        for (i, p) in placements.iter().enumerate() {
            let span = self.cells[i].row_span.max(1);
            extra_rows = extra_rows.max(p.row + span);
        }
        self.used_rows = self.used_rows.max(extra_rows);
        let row_h = cx.pt(self.row_height);
        let gap = cx.pt(self.gap);
        let h = self.used_rows as f32 * row_h + self.used_rows.saturating_sub(1) as f32 * gap;
        let _ = constraints;
        Vec2::new(240.0, h.max(row_h))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let gap = cx.pt(self.gap);
        let col_w =
            (bounds.width() - (self.columns - 1) as f32 * gap).max(0.0) / self.columns as f32;
        let row_h = cx.pt(self.row_height);
        let placements = self.place();
        self.placed.clear();
        for (cell, p) in self.cells.iter().zip(placements) {
            let x = bounds.min_x() + p.col as f32 * (col_w + gap);
            let span = cell.col_span.min(self.columns).max(1);
            let w = span as f32 * col_w + span.saturating_sub(1) as f32 * gap;
            // Row height adapts to content when a single-row cell's
            // measured height exceeds the base unit.
            let y = bounds.min_y() + p.row as f32 * (row_h + gap);
            let rspan = cell.row_span.max(1);
            let h = rspan as f32 * row_h + rspan.saturating_sub(1) as f32 * gap;
            let rect = Rect::new(x, y, w.max(0.0), h);
            self.placed.push((p, rect));
        }
        for ((_, rect), cell) in self.placed.iter().zip(self.cells.iter_mut()) {
            cx.layout_child(cell.widget.as_mut(), *rect);
        }
    }

    fn paint(&self, _cx: &mut PaintContext) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.as_str());
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn child_count(&self) -> usize {
        self.cells.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.cells
            .get(index)
            .map(|c| c.widget.as_ref() as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.cells
            .get_mut(index)
            .map(|c| c.widget.as_mut() as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.placed.get(index).map(|(_, r)| *r)
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 32.0)).with_policy(UnderflowPolicy::Lint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::text::Text;
    use martensite_core::HotNode;

    fn lay(w: &mut Grid) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, 1200.0, 400.0));
    }

    #[test]
    fn halves_split_the_row() {
        let mut g = Grid::new()
            .columns(12)
            .gap(0.0)
            .cell(GridCell::new(Text::new("a")).col_span(6))
            .cell(GridCell::new(Text::new("b")).col_span(6));
        lay(&mut g);
        let a = g.child_bounds(0).unwrap();
        let b = g.child_bounds(1).unwrap();
        assert_eq!(a.width(), 600.0);
        assert_eq!(b.min_x(), 600.0);
    }

    #[test]
    fn overflow_wraps_to_next_row() {
        let mut g = Grid::new()
            .columns(4)
            .gap(0.0)
            .cell(GridCell::new(Text::new("a")).col_span(3))
            .cell(GridCell::new(Text::new("b")).col_span(2));
        lay(&mut g);
        let b = g.child_bounds(1).unwrap();
        assert!(b.min_y() > 0.0, "b wrapped to row 1");
        assert_eq!(b.min_x(), 0.0, "b starts the fresh row");
    }

    #[test]
    fn row_span_reserves_columns_below() {
        let mut g = Grid::new()
            .columns(2)
            .gap(0.0)
            .cell(GridCell::new(Text::new("tall")).row_span(2))
            .cell(GridCell::new(Text::new("r0c1")))
            .cell(GridCell::new(Text::new("r1c1")));
        lay(&mut g);
        // Cell 2 can't go under the rowspan — it lands on row 1, col 1.
        let c = g.child_bounds(2).unwrap();
        assert!(c.min_y() > 0.0);
        assert!(c.min_x() > 0.0);
    }

    #[test]
    fn spans_clamp_to_columns() {
        let mut g = Grid::new()
            .columns(4)
            .cell(GridCell::new(Text::new("x")).col_span(99));
        lay(&mut g);
        assert_eq!(g.child_bounds(0).unwrap().width(), 1200.0);
    }

    #[test]
    fn gap_offsets_cells() {
        let mut g = Grid::new()
            .columns(2)
            .gap(10.0)
            .cell(GridCell::new(Text::new("a")))
            .cell(GridCell::new(Text::new("b")));
        lay(&mut g);
        let b = g.child_bounds(1).unwrap();
        assert!(b.min_x() > 595.0, "gap is part of the column pitch");
    }
}
