//! `VirtualRows` — the shared pixel-precise virtualization math
//! behind scrollable row grids (`ListView`, `ScrollView`-hosted
//! collections, icon walls).
//!
//! Pure geometry: it owns an item count, a column count, a fixed row
//! extent, a viewport extent, and a scroll offset, and answers the
//! three questions every virtualized wall needs — which rows/items
//! intersect the viewport (partially visible rows included), where a
//! row lands relative to the viewport top, and how far a scroll delta
//! actually consumed (the chaining boundary for nested scrollers).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::virtualize::VirtualRows;
//!
//! let mut v = VirtualRows::new(100, 3, 20.0, 50.0);
//! assert_eq!(v.visible_rows(), 0..3); // rows 0..=2, bottom row partial
//! v.scroll_by(10.0);
//! assert_eq!(v.visible_rows(), 0..3); // pixel offset: row 0 still partial
//! assert_eq!(v.row_origin(0), -10.0);
//! assert_eq!(v.visible_items(), 0..9);
//! ```

use std::ops::Range;

/// Pixel-precise row-grid virtualization math.
///
/// All extents and the offset are in the same unit — device pixels
/// for widgets that already multiply by the display scale, logical
/// points otherwise. `columns` is clamped to ≥ 1; an empty or
/// degenerate geometry (`row_extent <= 0`, `item_count == 0`,
/// `viewport_extent <= 0`) reports empty ranges and zero scroll.
///
/// # Examples
///
/// ```
/// use martensite::widgets::virtualize::VirtualRows;
///
/// let v = VirtualRows::new(0, 1, 10.0, 100.0);
/// assert_eq!(v.visible_rows(), 0..0);
/// assert_eq!(v.max_offset(), 0.0);
/// ```
#[derive(Debug, Clone)]
pub struct VirtualRows {
    item_count: usize,
    columns: usize,
    /// Row height in the same unit as `offset`/`viewport_extent`.
    row_extent: f32,
    /// Viewport height in the same unit.
    viewport_extent: f32,
    /// Scroll offset in the same unit.
    offset: f32,
}

impl VirtualRows {
    /// A wall of `item_count` items laid out `columns` wide with
    /// `row_extent`-tall rows behind a `viewport_extent`-high window.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::virtualize::VirtualRows;
    ///
    /// let v = VirtualRows::new(10, 2, 30.0, 60.0);
    /// assert_eq!(v.offset(), 0.0);
    /// ```
    #[must_use]
    pub fn new(item_count: usize, columns: usize, row_extent: f32, viewport_extent: f32) -> Self {
        Self {
            item_count,
            columns: columns.max(1),
            row_extent,
            viewport_extent,
            offset: 0.0,
        }
    }

    /// Total row count (`ceil(item_count / columns)`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::virtualize::VirtualRows;
    ///
    /// assert_eq!(VirtualRows::new(5, 2, 10.0, 10.0).row_count(), 3);
    /// ```
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.item_count.div_ceil(self.columns)
    }

    /// Number of columns (≥ 1).
    #[must_use]
    pub fn columns(&self) -> usize {
        self.columns
    }

    /// Current item count.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.item_count
    }

    /// Sets the item count and reclamps the offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::virtualize::VirtualRows;
    ///
    /// let mut v = VirtualRows::new(10, 1, 10.0, 10.0);
    /// v.set_offset(90.0);
    /// v.set_item_count(2);
    /// assert_eq!(v.offset(), 10.0);
    /// ```
    pub fn set_item_count(&mut self, item_count: usize) {
        self.item_count = item_count;
        self.set_offset(self.offset);
    }

    /// Sets the column count (clamped to ≥ 1) and reclamps the offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::virtualize::VirtualRows;
    ///
    /// let mut v = VirtualRows::new(10, 1, 10.0, 10.0);
    /// v.set_columns(3);
    /// assert_eq!(v.row_count(), 4);
    /// ```
    pub fn set_columns(&mut self, columns: usize) {
        self.columns = columns.max(1);
        self.set_offset(self.offset);
    }

    /// Sets the row extent (non-positive disables scrolling) and
    /// reclamps the offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::virtualize::VirtualRows;
    ///
    /// let mut v = VirtualRows::new(10, 1, 10.0, 50.0);
    /// v.set_row_extent(20.0);
    /// assert_eq!(v.max_offset(), 150.0);
    /// ```
    pub fn set_row_extent(&mut self, row_extent: f32) {
        self.row_extent = row_extent;
        self.set_offset(self.offset);
    }

    /// Sets the viewport extent and reclamps the offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::virtualize::VirtualRows;
    ///
    /// let mut v = VirtualRows::new(10, 1, 10.0, 50.0);
    /// v.set_offset(50.0);
    /// v.set_viewport_extent(100.0);
    /// assert_eq!(v.offset(), 0.0);
    /// ```
    pub fn set_viewport_extent(&mut self, viewport_extent: f32) {
        self.viewport_extent = viewport_extent;
        self.set_offset(self.offset);
    }

    /// Maximum scrollable offset: `max(0, content - viewport)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::virtualize::VirtualRows;
    ///
    /// assert_eq!(VirtualRows::new(10, 1, 10.0, 40.0).max_offset(), 60.0);
    /// assert_eq!(VirtualRows::new(4, 1, 10.0, 40.0).max_offset(), 0.0);
    /// ```
    #[must_use]
    pub fn max_offset(&self) -> f32 {
        if self.row_extent <= 0.0 {
            return 0.0;
        }
        (self.row_count() as f32 * self.row_extent - self.viewport_extent).max(0.0)
    }

    /// The clamped scroll offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::virtualize::VirtualRows;
    ///
    /// let mut v = VirtualRows::new(10, 1, 10.0, 40.0);
    /// v.set_offset(25.0);
    /// assert_eq!(v.offset(), 25.0);
    /// ```
    #[must_use]
    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// Sets the scroll offset, clamped to `0..=max_offset()`.
    /// Non-finite input clamps to 0.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::virtualize::VirtualRows;
    ///
    /// let mut v = VirtualRows::new(10, 1, 10.0, 40.0);
    /// v.set_offset(-10.0);
    /// assert_eq!(v.offset(), 0.0);
    /// v.set_offset(f32::NAN);
    /// assert_eq!(v.offset(), 0.0);
    /// ```
    pub fn set_offset(&mut self, offset: f32) {
        let offset = if offset.is_finite() { offset } else { 0.0 };
        self.offset = offset.clamp(0.0, self.max_offset());
    }

    /// Applies a scroll delta; returns the portion actually consumed.
    /// Returns `0.0` at either bound — the chaining boundary a parent
    /// scroller uses to take over.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::virtualize::VirtualRows;
    ///
    /// let mut v = VirtualRows::new(10, 1, 10.0, 40.0);
    /// assert_eq!(v.scroll_by(30.0), 30.0);
    /// assert_eq!(v.scroll_by(-10.0), -10.0);
    /// assert_eq!(v.scroll_by(1e6), 40.0); // clamped at max
    /// assert_eq!(v.scroll_by(5.0), 0.0);  // at bound — chains outward
    /// ```
    pub fn scroll_by(&mut self, delta: f32) -> f32 {
        let before = self.offset;
        self.set_offset(before + delta);
        self.offset - before
    }

    /// Rows intersecting the viewport, including a partially visible
    /// top and bottom row.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::virtualize::VirtualRows;
    ///
    /// let mut v = VirtualRows::new(10, 1, 10.0, 25.0);
    /// assert_eq!(v.visible_rows(), 0..3); // 25/10 → rows 0,1,2 (partial)
    /// v.set_offset(10.0);
    /// assert_eq!(v.visible_rows(), 1..4); // aligned top, partial bottom
    /// ```
    #[must_use]
    pub fn visible_rows(&self) -> Range<usize> {
        if self.row_extent <= 0.0 || self.item_count == 0 || self.viewport_extent <= 0.0 {
            return 0..0;
        }
        let start = (self.offset / self.row_extent).floor() as usize;
        let end = ((self.offset + self.viewport_extent) / self.row_extent).ceil() as usize;
        start..end.min(self.row_count())
    }

    /// Item indices covered by [`visible_rows`](Self::visible_rows) —
    /// `rows.start * columns ..= min(rows.end * columns, item_count)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::virtualize::VirtualRows;
    ///
    /// let mut v = VirtualRows::new(10, 3, 10.0, 15.0);
    /// v.set_offset(10.0);
    /// assert_eq!(v.visible_items(), 3..9); // rows 1..3 → items 3..9
    /// ```
    #[must_use]
    pub fn visible_items(&self) -> Range<usize> {
        let rows = self.visible_rows();
        (rows.start * self.columns)..(rows.end * self.columns).min(self.item_count)
    }

    /// Row `row`'s top edge relative to the viewport top — negative
    /// for a partially scrolled-off top row.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::virtualize::VirtualRows;
    ///
    /// let mut v = VirtualRows::new(10, 1, 10.0, 40.0);
    /// v.set_offset(15.0);
    /// assert_eq!(v.row_origin(1), -5.0);
    /// assert_eq!(v.row_origin(3), 15.0);
    /// ```
    #[must_use]
    pub fn row_origin(&self, row: usize) -> f32 {
        row as f32 * self.row_extent - self.offset
    }

    /// Scrolls the minimum amount that makes `row` fully visible.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::virtualize::VirtualRows;
    ///
    /// let mut v = VirtualRows::new(10, 1, 10.0, 30.0);
    /// v.ensure_row_visible(7);
    /// assert_eq!(v.offset(), 50.0); // row 7 spans 70..80, viewport 30
    /// v.ensure_row_visible(2);
    /// assert_eq!(v.offset(), 20.0); // scroll back up to row 2's top
    /// ```
    pub fn ensure_row_visible(&mut self, row: usize) {
        if self.row_extent <= 0.0 {
            return;
        }
        let top = row as f32 * self.row_extent;
        let bottom = top + self.row_extent;
        if top < self.offset {
            self.set_offset(top);
        } else if bottom > self.offset + self.viewport_extent {
            self.set_offset(bottom - self.viewport_extent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_rows_counted_top_and_bottom() {
        let mut v = VirtualRows::new(10, 1, 10.0, 25.0);
        assert_eq!(v.visible_rows(), 0..3);
        v.set_offset(1.0);
        assert_eq!(v.visible_rows(), 0..3); // row 0 still partial
        v.set_offset(10.0);
        assert_eq!(v.visible_rows(), 1..4); // aligned top, partial bottom
        v.set_offset(15.0);
        assert_eq!(v.visible_rows(), 1..4); // partial top and bottom
    }

    #[test]
    fn scroll_clamps_and_chains() {
        let mut v = VirtualRows::new(4, 1, 10.0, 40.0);
        assert_eq!(v.max_offset(), 0.0);
        assert_eq!(v.scroll_by(50.0), 0.0);
        let mut v = VirtualRows::new(10, 1, 10.0, 40.0);
        assert_eq!(v.scroll_by(1000.0), 60.0);
        assert_eq!(v.scroll_by(1.0), 0.0);
        assert_eq!(v.scroll_by(-1000.0), -60.0);
    }

    #[test]
    fn empty_and_degenerate() {
        assert_eq!(VirtualRows::new(0, 1, 10.0, 50.0).visible_rows(), 0..0);
        assert_eq!(VirtualRows::new(5, 1, 0.0, 50.0).visible_items(), 0..0);
        let v = VirtualRows::new(5, 1, 10.0, 0.0);
        assert_eq!(v.visible_rows(), 0..0);
        assert_eq!(v.max_offset(), 50.0); // still scrollable extent
    }

    #[test]
    fn single_column_matches_list_semantics() {
        let mut v = VirtualRows::new(100, 1, 20.0, 100.0);
        v.set_offset(33.0);
        assert_eq!(v.visible_rows(), 1..7); // floor(33/20)=1, ceil(133/20)=7
        assert_eq!(v.visible_items(), 1..7);
        assert_eq!(v.row_origin(1), -13.0);
    }

    #[test]
    fn fractional_viewport() {
        let v = VirtualRows::new(10, 2, 10.0, 25.5);
        assert_eq!(v.visible_rows(), 0..3);
        assert_eq!(v.visible_items(), 0..6);
    }

    #[test]
    fn multi_column_items_end_clamps() {
        let mut v = VirtualRows::new(7, 3, 10.0, 100.0);
        v.set_offset(1000.0);
        assert_eq!(v.visible_rows(), 0..3);
        assert_eq!(v.visible_items(), 0..7);
    }

    #[test]
    fn ensure_row_visible_noop_when_visible() {
        let mut v = VirtualRows::new(10, 1, 10.0, 30.0);
        v.ensure_row_visible(1);
        assert_eq!(v.offset(), 0.0);
        v.set_offset(70.0); // max = 70
        v.ensure_row_visible(9);
        assert_eq!(v.offset(), 70.0);
    }

    #[test]
    fn setters_reclamp() {
        let mut v = VirtualRows::new(10, 1, 10.0, 40.0);
        v.set_offset(60.0);
        v.set_columns(5);
        assert_eq!(v.offset(), 0.0); // 2 rows fit in 40px
        v.set_row_extent(5.0);
        assert_eq!(v.offset(), 0.0); // 10 items / 5 cols = 2 rows of 5px
        v.set_item_count(50);
        assert_eq!(v.max_offset(), 10.0); // 10 rows * 5px - 40
    }
}
