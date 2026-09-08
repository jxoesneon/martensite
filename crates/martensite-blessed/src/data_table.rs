//! A virtualized data table.

use std::ops::Range;

/// A table that stores rows while materializing only the visible range.
///
/// Scrolling and range calculation are constant-time. Iterating visible rows
/// allocates no intermediate row collection.
///
/// # Examples
///
/// ```
/// use martensite_blessed::DataTable;
/// let mut table = DataTable::new(vec![0_u8; 1_000_000], 20.0);
/// table.set_viewport_height(100.0);
/// assert_eq!(table.visible_range(), 0..5);
/// ```
#[derive(Clone, Debug)]
pub struct DataTable<T> {
    rows: T,
    row_count: usize,
    row_height: f64,
    viewport_height: f64,
    scroll_offset: f64,
}

impl<T> DataTable<T>
where
    T: AsRef<[T::Item]>,
    T: TableStorage,
{
    /// Creates a table from random-access row storage and a logical row height.
    pub fn new(rows: T, row_height: f32) -> Self {
        let row_count = rows.len();
        Self {
            rows,
            row_count,
            row_height: valid_nonnegative(f64::from(row_height)),
            viewport_height: 0.0,
            scroll_offset: 0.0,
        }
    }

    /// Returns the total number of rows.
    pub const fn row_count(&self) -> usize {
        self.row_count
    }

    /// Sets the viewport height in logical pixels.
    pub fn set_viewport_height(&mut self, height: f32) {
        self.viewport_height = valid_nonnegative(f64::from(height));
        self.clamp_scroll();
    }

    /// Scrolls by a logical-pixel delta and returns the resulting offset.
    pub fn scroll_by(&mut self, delta: f32) -> f32 {
        let delta = f64::from(delta);
        if delta.is_finite() {
            self.scroll_offset += delta;
        }
        self.clamp_scroll();
        self.scroll_offset as f32
    }

    /// Sets the absolute logical-pixel scroll offset.
    pub fn set_scroll_offset(&mut self, offset: f32) {
        self.scroll_offset = valid_nonnegative(f64::from(offset));
        self.clamp_scroll();
    }

    /// Returns the current logical-pixel scroll offset.
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_offset as f32
    }

    /// Returns the half-open range of rows intersecting the viewport.
    pub fn visible_range(&self) -> Range<usize> {
        if self.row_height == 0.0 || self.viewport_height == 0.0 {
            return 0..0;
        }
        let start = (self.scroll_offset / self.row_height).floor() as usize;
        let end = ((self.scroll_offset + self.viewport_height) / self.row_height).ceil() as usize;
        start.min(self.row_count)..end.min(self.row_count)
    }

    /// Iterates visible `(row_index, row)` pairs without allocating.
    pub fn visible_rows(&self) -> impl ExactSizeIterator<Item = (usize, &T::Item)> {
        let range = self.visible_range();
        self.rows.as_ref()[range.clone()]
            .iter()
            .enumerate()
            .map(move |(offset, row)| (range.start + offset, row))
    }

    fn clamp_scroll(&mut self) {
        let content_height = self.row_count as f64 * self.row_height;
        let maximum = (content_height - self.viewport_height).max(0.0);
        self.scroll_offset = self.scroll_offset.clamp(0.0, maximum);
    }
}

/// Random-access storage accepted by [`DataTable`].
///
/// # Examples
///
/// ```
/// use martensite_blessed::{DataTable, TableStorage};
/// let rows = vec![1, 2, 3];
/// assert_eq!(TableStorage::len(&rows), 3);
/// let table = DataTable::new(rows, 18.0);
/// assert_eq!(table.row_count(), 3);
/// ```
pub trait TableStorage {
    /// The row type.
    type Item;
    /// Returns the number of rows.
    fn len(&self) -> usize;
    /// Returns whether there are no rows.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<R> TableStorage for Vec<R> {
    type Item = R;
    fn len(&self) -> usize {
        Vec::len(self)
    }
}

fn valid_nonnegative(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::DataTable;

    #[test]
    fn virtualizes_a_million_rows() {
        let mut table = DataTable::new(vec![0_u32; 1_000_000], 10.0);
        table.set_viewport_height(95.0);
        table.set_scroll_offset(500_003.0);
        assert_eq!(table.visible_range(), 50_000..50_010);
        assert_eq!(table.visible_rows().len(), 10);
    }

    #[test]
    fn clamps_scrolling() {
        let mut table = DataTable::new(vec![0; 10], 10.0);
        table.set_viewport_height(30.0);
        assert_eq!(table.scroll_by(f32::MAX), 70.0);
        assert_eq!(table.scroll_by(-100.0), 0.0);
    }
}
