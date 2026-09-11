//! A virtualized data table.
//!
//! [`DataTable`] stores rows in random-access storage while materializing only
//! the visible range. Scrolling and range calculation are constant-time and
//! iterating visible rows allocates nothing in the steady state. The table also
//! supports column configuration, sorting, filtering, range selection, and
//! keyboard navigation.

use std::cmp::Ordering;
use std::ops::Range;

use smallvec::{smallvec, SmallVec};

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
    sort_index: Vec<usize>,
    sort_inverse: Vec<usize>,
    sort_active: bool,
    sort_column: Option<usize>,
    sort_direction: ColumnSort,
    filtered_index: Vec<usize>,
    filtered_inverse: Vec<usize>,
    filter_active: bool,
    selection: SelectionModel,
    focused_row: Option<usize>,
    columns: Vec<ColumnConfig>,
}

impl<T> DataTable<T>
where
    T: AsRef<[T::Item]>,
    T: TableStorage,
{
    /// Creates a table from random-access row storage and a logical row height.
    pub fn new(rows: T, row_height: f32) -> Self {
        let row_count = rows.len();
        let sort_index: Vec<usize> = (0..row_count).collect();
        let sort_inverse: Vec<usize> = (0..row_count).collect();
        Self {
            rows,
            row_count,
            row_height: valid_nonnegative(f64::from(row_height)),
            viewport_height: 0.0,
            scroll_offset: 0.0,
            sort_index,
            sort_inverse,
            sort_active: false,
            sort_column: None,
            sort_direction: ColumnSort::None,
            filtered_index: Vec::new(),
            filtered_inverse: Vec::new(),
            filter_active: false,
            selection: SelectionModel::default(),
            focused_row: None,
            columns: Vec::new(),
        }
    }

    /// Returns the total number of rows in storage.
    pub const fn row_count(&self) -> usize {
        self.row_count
    }

    /// Returns the number of rows currently on display.
    ///
    /// This is the row count after filtering has been applied. When no filter
    /// is active it is identical to [`row_count`](Self::row_count).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DataTable;
    /// use martensite_blessed::data_table::RowFilter;
    /// let mut table = DataTable::new(vec![1_i32, 2, 3], 20.0);
    /// assert_eq!(table.display_row_count(), 3);
    /// table.set_filter(RowFilter::new(|r: &i32| *r == 2));
    /// assert_eq!(table.display_row_count(), 1);
    /// ```
    pub fn display_row_count(&self) -> usize {
        self.display_count()
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
    ///
    /// This is a single field assignment with no allocation; out-of-range
    /// offsets are clamped lazily by [`visible_range`](Self::visible_range).
    pub fn set_scroll_offset(&mut self, offset: f32) {
        self.scroll_offset = valid_nonnegative(f64::from(offset));
    }

    /// Returns the current logical-pixel scroll offset.
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_offset as f32
    }

    /// Returns the half-open range of display positions intersecting the
    /// viewport.
    ///
    /// When no filter is active display positions coincide with row indices.
    pub fn visible_range(&self) -> Range<usize> {
        if self.row_height == 0.0 || self.viewport_height == 0.0 {
            return 0..0;
        }
        let count = self.display_count();
        if count == 0 {
            return 0..0;
        }
        let start = (self.scroll_offset / self.row_height).floor() as usize;
        let end = ((self.scroll_offset + self.viewport_height) / self.row_height).ceil() as usize;
        start.min(count)..end.min(count)
    }

    /// Iterates visible `(row_index, row)` pairs without allocating.
    ///
    /// The returned iterator borrows the table directly: scrolling never
    /// rebuilds an index, so steady-state iteration is allocation-free.
    pub fn visible_rows(&self) -> impl ExactSizeIterator<Item = (usize, &T::Item)> {
        let range = self.visible_range();
        let rows = self.rows.as_ref();
        let index = self.display_slice();
        index[range]
            .iter()
            .map(move |&row_idx| (row_idx, &rows[row_idx]))
    }

    /// Iterates visible `(row_index, row)` pairs for column-aware rendering.
    ///
    /// This is the column-aware counterpart to [`visible_rows`](Self::visible_rows).
    /// Callers consult [`columns`](Self::columns) to decide which fields of each
    /// row to render. Like `visible_rows`, this allocates nothing while
    /// scrolling.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DataTable;
    /// let mut table = DataTable::new(vec![10_i32, 20, 30, 40], 20.0);
    /// table.set_viewport_height(80.0);
    /// let collected: Vec<(usize, i32)> =
    ///     table.visible_rows_with_columns().map(|(i, r)| (i, *r)).collect();
    /// assert_eq!(collected, vec![(0, 10), (1, 20), (2, 30), (3, 40)]);
    /// ```
    pub fn visible_rows_with_columns(&self) -> impl ExactSizeIterator<Item = (usize, &T::Item)> {
        self.visible_rows()
    }

    /// Sorts the table by `column` using `comparator` to order rows.
    ///
    /// The sort is performed with [`slice::sort_unstable_by`] over an index
    /// vector that is rebuilt only when the sort changes. When a filter is
    /// active the filtered index is reordered to match.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DataTable;
    /// use martensite_blessed::data_table::ColumnSort;
    /// let mut table = DataTable::new(vec![3_i32, 1, 4, 2, 5], 20.0);
    /// table.set_viewport_height(200.0);
    /// table.sort_by(0, ColumnSort::Ascending, |a, b| a.cmp(b));
    /// let values: Vec<i32> = table.visible_rows().map(|(_, r)| *r).collect();
    /// assert_eq!(values, vec![1, 2, 3, 4, 5]);
    /// ```
    pub fn sort_by<F>(&mut self, column: usize, direction: ColumnSort, comparator: F)
    where
        F: Fn(&T::Item, &T::Item) -> Ordering,
    {
        let rows = self.rows.as_ref();
        let n = self.row_count;
        let mut index: Vec<usize> = (0..n).collect();
        match direction {
            ColumnSort::None => {}
            ColumnSort::Ascending => {
                index.sort_unstable_by(|a, b| comparator(&rows[*a], &rows[*b]));
            }
            ColumnSort::Descending => {
                index.sort_unstable_by(|a, b| comparator(&rows[*a], &rows[*b]).reverse());
            }
        }
        let mut inverse = vec![0_usize; n];
        for (pos, &row) in index.iter().enumerate() {
            inverse[row] = pos;
        }
        self.sort_index = index;
        self.sort_inverse = inverse;
        self.sort_column = Some(column);
        self.sort_direction = direction;
        self.sort_active = matches!(direction, ColumnSort::Ascending | ColumnSort::Descending);
        if self.filter_active {
            self.filtered_index
                .sort_unstable_by_key(|&row| self.sort_inverse[row]);
            self.rebuild_filtered_inverse();
        }
    }

    /// Clears any active column sort, restoring natural row order.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DataTable;
    /// use martensite_blessed::data_table::ColumnSort;
    /// let mut table = DataTable::new(vec![3_i32, 1, 2], 20.0);
    /// table.set_viewport_height(200.0);
    /// table.sort_by(0, ColumnSort::Descending, |a, b| a.cmp(b));
    /// table.clear_sort();
    /// let values: Vec<i32> = table.visible_rows().map(|(_, r)| *r).collect();
    /// assert_eq!(values, vec![3, 1, 2]);
    /// ```
    pub fn clear_sort(&mut self) {
        let n = self.row_count;
        self.sort_index = (0..n).collect();
        self.sort_inverse = (0..n).collect();
        self.sort_active = false;
        self.sort_column = None;
        self.sort_direction = ColumnSort::None;
        if self.filter_active {
            self.filtered_index.sort_unstable_by_key(|&row| row);
            self.rebuild_filtered_inverse();
        }
    }

    /// Returns the column index currently driving the sort, if any.
    pub fn sort_column(&self) -> Option<usize> {
        self.sort_column
    }

    /// Returns the active sort direction.
    pub fn sort_direction(&self) -> ColumnSort {
        self.sort_direction
    }

    /// Applies `filter`, keeping only rows for which the predicate returns
    /// `true`.
    ///
    /// The filtered index is rebuilt only when the filter changes. When a sort
    /// is active the filtered rows are kept in sorted order.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DataTable;
    /// use martensite_blessed::data_table::RowFilter;
    /// let mut table = DataTable::new(vec![1_i32, 2, 3, 4, 5], 20.0);
    /// table.set_viewport_height(200.0);
    /// table.set_filter(RowFilter::new(|r: &i32| *r >= 3));
    /// let values: Vec<i32> = table.visible_rows().map(|(_, r)| *r).collect();
    /// assert_eq!(values, vec![3, 4, 5]);
    /// ```
    pub fn set_filter(&mut self, filter: RowFilter<T::Item>) {
        let rows = self.rows.as_ref();
        let n = self.row_count;
        let mut filtered: Vec<usize> = (0..n).filter(|&i| filter.test(&rows[i])).collect();
        if self.sort_active {
            filtered.sort_unstable_by_key(|&row| self.sort_inverse[row]);
        }
        self.filtered_index = filtered;
        self.filter_active = true;
        self.rebuild_filtered_inverse();
        self.clamp_scroll();
        if let Some(row) = self.focused_row {
            if row >= self.row_count
                || self
                    .filtered_inverse
                    .get(row)
                    .copied()
                    .unwrap_or(usize::MAX)
                    == usize::MAX
            {
                self.focused_row = self.display_slice().first().copied();
            }
        }
    }

    /// Clears the active filter, restoring all rows to the display.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DataTable;
    /// use martensite_blessed::data_table::RowFilter;
    /// let mut table = DataTable::new(vec![1_i32, 2, 3], 20.0);
    /// table.set_filter(RowFilter::new(|r: &i32| *r == 2));
    /// assert_eq!(table.display_row_count(), 1);
    /// table.clear_filter();
    /// assert_eq!(table.display_row_count(), 3);
    /// ```
    pub fn clear_filter(&mut self) {
        self.filtered_index.clear();
        self.filtered_inverse.clear();
        self.filter_active = false;
        self.clamp_scroll();
    }

    /// Returns whether a filter is currently active.
    pub fn filter_active(&self) -> bool {
        self.filter_active
    }

    /// Returns the selection model by shared reference.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DataTable;
    /// let mut table = DataTable::new(vec![0_u32; 5], 20.0);
    /// table.selection_mut().select(2);
    /// assert!(table.selection().is_selected(2));
    /// ```
    pub fn selection(&self) -> &SelectionModel {
        &self.selection
    }

    /// Returns the selection model by mutable reference.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DataTable;
    /// let mut table = DataTable::new(vec![0_u32; 5], 20.0);
    /// table.selection_mut().select_range(1..3);
    /// assert_eq!(table.selection().selected_count(), 2);
    /// ```
    pub fn selection_mut(&mut self) -> &mut SelectionModel {
        &mut self.selection
    }

    /// Returns the currently focused row, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DataTable;
    /// use martensite_blessed::data_table::KeyAction;
    /// let mut table = DataTable::new(vec![0_u32; 10], 20.0);
    /// table.set_viewport_height(40.0);
    /// assert!(table.handle_key(KeyAction::End));
    /// assert_eq!(table.focused_row(), Some(9));
    /// ```
    pub fn focused_row(&self) -> Option<usize> {
        self.focused_row
    }

    /// Sets the focused row.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DataTable;
    /// let mut table = DataTable::new(vec![0_u32; 10], 20.0);
    /// table.set_focused_row(Some(3));
    /// assert_eq!(table.focused_row(), Some(3));
    /// ```
    pub fn set_focused_row(&mut self, row: Option<usize>) {
        self.focused_row = row;
    }

    /// Handles a keyboard navigation action, updating the focused row and
    /// auto-scrolling to keep it visible.
    ///
    /// Returns `true` when the action was applied and `false` when the table
    /// has no displayable rows. Shift actions extend the current selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DataTable;
    /// use martensite_blessed::data_table::KeyAction;
    /// let mut table = DataTable::new(vec![0_u32; 100], 20.0);
    /// table.set_viewport_height(80.0);
    /// assert!(table.handle_key(KeyAction::Down));
    /// assert_eq!(table.focused_row(), Some(1));
    /// ```
    pub fn handle_key(&mut self, action: KeyAction) -> bool {
        let count = self.display_count();
        if count == 0 {
            self.focused_row = None;
            return false;
        }
        let page = self.page_size();
        let current_pos = match self.focused_row {
            Some(row) if row < self.row_count => {
                let p = self.display_position_of(row);
                if p >= count {
                    0
                } else {
                    p
                }
            }
            _ => 0,
        };
        let new_pos = match action {
            KeyAction::Up | KeyAction::ShiftUp => current_pos.saturating_sub(1),
            KeyAction::Down | KeyAction::ShiftDown => (current_pos + 1).min(count - 1),
            KeyAction::PageUp => current_pos.saturating_sub(page),
            KeyAction::PageDown => (current_pos + page).min(count - 1),
            KeyAction::Home | KeyAction::CtrlHome => 0,
            KeyAction::End | KeyAction::CtrlEnd => count - 1,
        };
        let new_pos = new_pos.min(count - 1);
        let new_row = self.display_slice()[new_pos];
        if matches!(action, KeyAction::ShiftUp | KeyAction::ShiftDown) {
            if self.selection.is_empty() {
                if let Some(cur) = self.focused_row {
                    self.selection.select(cur);
                }
            }
            self.selection.extend_to(new_row);
        }
        self.focused_row = Some(new_row);
        self.scroll_to_display_pos(new_pos);
        true
    }

    /// Returns the column configurations by shared reference.
    pub fn columns(&self) -> &[ColumnConfig] {
        &self.columns
    }

    /// Replaces the column configurations.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DataTable;
    /// use martensite_blessed::data_table::ColumnConfig;
    /// let mut table = DataTable::new(vec![0_u32; 3], 20.0);
    /// table.set_columns(vec![ColumnConfig::new(80.0), ColumnConfig::new(120.0)]);
    /// assert_eq!(table.column_count(), 2);
    /// ```
    pub fn set_columns(&mut self, columns: Vec<ColumnConfig>) {
        self.columns = columns;
    }

    /// Returns the number of configured columns.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DataTable;
    /// use martensite_blessed::data_table::ColumnConfig;
    /// let mut table = DataTable::new(vec![0_u32; 3], 20.0);
    /// table.set_columns(vec![ColumnConfig::new(80.0), ColumnConfig::new(40.0)]);
    /// assert_eq!(table.column_count(), 2);
    /// ```
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Sets the width of column `column` in logical pixels.
    ///
    /// Does nothing if `column` is out of range.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DataTable;
    /// use martensite_blessed::data_table::ColumnConfig;
    /// let mut table = DataTable::new(vec![0_u32; 3], 20.0);
    /// table.set_columns(vec![ColumnConfig::new(80.0)]);
    /// table.set_column_width(0, 200.0);
    /// assert_eq!(table.columns()[0].width(), 200.0);
    /// ```
    pub fn set_column_width(&mut self, column: usize, width: f32) {
        if let Some(col) = self.columns.get_mut(column) {
            col.set_width(width);
        }
    }

    /// Shows or hides column `column`.
    ///
    /// Does nothing if `column` is out of range.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::DataTable;
    /// use martensite_blessed::data_table::ColumnConfig;
    /// let mut table = DataTable::new(vec![0_u32; 3], 20.0);
    /// table.set_columns(vec![ColumnConfig::new(80.0)]);
    /// table.set_column_visible(0, false);
    /// assert!(!table.columns()[0].visible());
    /// ```
    pub fn set_column_visible(&mut self, column: usize, visible: bool) {
        if let Some(col) = self.columns.get_mut(column) {
            col.set_visible(visible);
        }
    }

    fn display_count(&self) -> usize {
        if self.filter_active {
            self.filtered_index.len()
        } else {
            self.row_count
        }
    }

    fn display_slice(&self) -> &[usize] {
        if self.filter_active {
            &self.filtered_index
        } else {
            &self.sort_index
        }
    }

    fn display_position_of(&self, row: usize) -> usize {
        if self.filter_active {
            self.filtered_inverse
                .get(row)
                .copied()
                .unwrap_or(usize::MAX)
        } else {
            self.sort_inverse.get(row).copied().unwrap_or(usize::MAX)
        }
    }

    fn rebuild_filtered_inverse(&mut self) {
        self.filtered_inverse = vec![usize::MAX; self.row_count];
        for (pos, &row) in self.filtered_index.iter().enumerate() {
            self.filtered_inverse[row] = pos;
        }
    }

    fn page_size(&self) -> usize {
        if self.row_height == 0.0 {
            return 1;
        }
        ((self.viewport_height / self.row_height).floor() as usize).max(1)
    }

    fn scroll_to_display_pos(&mut self, pos: usize) {
        if self.row_height == 0.0 || self.viewport_height == 0.0 {
            return;
        }
        let row_top = pos as f64 * self.row_height;
        let row_bottom = row_top + self.row_height;
        if row_top < self.scroll_offset {
            self.scroll_offset = row_top;
        } else if row_bottom > self.scroll_offset + self.viewport_height {
            self.scroll_offset = (row_bottom - self.viewport_height).max(0.0);
        }
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let count = self.display_count();
        let content_height = count as f64 * self.row_height;
        let maximum = (content_height - self.viewport_height).max(0.0);
        self.scroll_offset = self.scroll_offset.clamp(0.0, maximum);
    }
}

/// Sort direction for a [`DataTable`] column.
///
/// # Examples
///
/// ```
/// use martensite_blessed::data_table::ColumnSort;
/// let direction = ColumnSort::Ascending;
/// assert_ne!(direction, ColumnSort::None);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColumnSort {
    /// No sort applied; rows keep their natural order.
    #[default]
    None,
    /// Ascending order.
    Ascending,
    /// Descending order.
    Descending,
}

/// A closure-based predicate used to filter rows of a [`DataTable`].
///
/// Construct a `RowFilter` with [`RowFilter::new`] and pass it to
/// [`DataTable::set_filter`]. The predicate receives a borrowed row and
/// returns `true` to keep it.
///
/// # Examples
///
/// ```
/// use martensite_blessed::DataTable;
/// use martensite_blessed::data_table::RowFilter;
/// let mut table = DataTable::new(vec![1_i32, 2, 3, 4], 20.0);
/// table.set_viewport_height(200.0);
/// table.set_filter(RowFilter::new(|r: &i32| *r % 2 == 0));
/// assert_eq!(table.display_row_count(), 2);
/// ```
pub struct RowFilter<R> {
    predicate: Box<dyn Fn(&R) -> bool + Send + Sync>,
}

impl<R> RowFilter<R> {
    /// Creates a filter wrapping `predicate`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::RowFilter;
    /// let filter = RowFilter::new(|r: &i32| *r > 0);
    /// assert!(filter.test(&5));
    /// assert!(!filter.test(&-1));
    /// ```
    pub fn new<F>(predicate: F) -> Self
    where
        F: Fn(&R) -> bool + Send + Sync + 'static,
    {
        Self {
            predicate: Box::new(predicate),
        }
    }

    /// Evaluates the predicate against `row`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::RowFilter;
    /// let filter = RowFilter::new(|r: &i32| *r == 7);
    /// assert!(filter.test(&7));
    /// assert!(!filter.test(&8));
    /// ```
    pub fn test(&self, row: &R) -> bool {
        (self.predicate)(row)
    }
}

impl<R> Default for RowFilter<R> {
    fn default() -> Self {
        Self::new(|_row: &R| true)
    }
}

impl<R> std::fmt::Debug for RowFilter<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RowFilter").finish_non_exhaustive()
    }
}

/// A range-based row selection model with inline storage for small selections.
///
/// Selections are stored as a [`SmallVec`] of [`Range<usize>`] so that up to
/// four disjoint ranges live inline without heap allocation.
///
/// # Examples
///
/// ```
/// use martensite_blessed::data_table::SelectionModel;
/// let mut selection = SelectionModel::new();
/// selection.select_range(2..5);
/// selection.extend_to(7);
/// assert_eq!(selection.selected_count(), 6);
/// assert!(selection.is_selected(7));
/// assert!(!selection.is_selected(1));
/// ```
#[derive(Clone, Debug, Default)]
pub struct SelectionModel {
    ranges: SmallVec<[Range<usize>; 4]>,
    anchor: Option<usize>,
}

impl SelectionModel {
    /// Creates an empty selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::SelectionModel;
    /// let selection = SelectionModel::new();
    /// assert!(selection.is_empty());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the selection with a single row.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::SelectionModel;
    /// let mut selection = SelectionModel::new();
    /// selection.select(3);
    /// assert!(selection.is_selected(3));
    /// assert_eq!(selection.selected_count(), 1);
    /// ```
    pub fn select(&mut self, row: usize) {
        self.ranges = smallvec![row..row + 1];
        self.anchor = Some(row);
    }

    /// Replaces the selection with `range`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::SelectionModel;
    /// let mut selection = SelectionModel::new();
    /// selection.select_range(2..5);
    /// assert_eq!(selection.selected_count(), 3);
    /// ```
    pub fn select_range(&mut self, range: Range<usize>) {
        self.anchor = Some(range.start);
        self.ranges = smallvec![range];
    }

    /// Extends the selection from its anchor to `row` inclusive.
    ///
    /// If no anchor is set this is equivalent to [`select`](Self::select).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::SelectionModel;
    /// let mut selection = SelectionModel::new();
    /// selection.select(4);
    /// selection.extend_to(1);
    /// assert!(selection.is_selected(1));
    /// assert!(selection.is_selected(4));
    /// assert_eq!(selection.selected_count(), 4);
    /// ```
    pub fn extend_to(&mut self, row: usize) {
        let anchor = match self.anchor {
            Some(a) => a,
            None => {
                self.select(row);
                return;
            }
        };
        let start = anchor.min(row);
        let end = anchor.max(row) + 1;
        self.ranges = smallvec![start..end];
    }

    /// Clears the selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::SelectionModel;
    /// let mut selection = SelectionModel::new();
    /// selection.select(1);
    /// selection.clear();
    /// assert!(selection.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.ranges.clear();
        self.anchor = None;
    }

    /// Returns whether `row` is selected.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::SelectionModel;
    /// let mut selection = SelectionModel::new();
    /// selection.select_range(2..4);
    /// assert!(selection.is_selected(2));
    /// assert!(!selection.is_selected(4));
    /// ```
    pub fn is_selected(&self, row: usize) -> bool {
        self.ranges.iter().any(|r| r.contains(&row))
    }

    /// Returns the total number of selected rows.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::SelectionModel;
    /// let mut selection = SelectionModel::new();
    /// selection.select_range(0..10);
    /// assert_eq!(selection.selected_count(), 10);
    /// ```
    pub fn selected_count(&self) -> usize {
        self.ranges.iter().map(|r| r.len()).sum()
    }

    /// Returns whether no rows are selected.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::SelectionModel;
    /// let selection = SelectionModel::new();
    /// assert!(selection.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// Returns the ranges composing this selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::SelectionModel;
    /// let mut selection = SelectionModel::new();
    /// selection.select_range(2..5);
    /// assert_eq!(selection.ranges().len(), 1);
    /// ```
    pub fn ranges(&self) -> &SmallVec<[Range<usize>; 4]> {
        &self.ranges
    }
}

/// A keyboard navigation action understood by [`DataTable::handle_key`].
///
/// # Examples
///
/// ```
/// use martensite_blessed::DataTable;
/// use martensite_blessed::data_table::KeyAction;
/// let mut table = DataTable::new(vec![0_u32; 50], 20.0);
/// table.set_viewport_height(80.0);
/// assert!(table.handle_key(KeyAction::PageDown));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    /// Move focus up one row.
    Up,
    /// Move focus down one row.
    Down,
    /// Move focus up by one viewport page.
    PageUp,
    /// Move focus down by one viewport page.
    PageDown,
    /// Move focus to the first visible row.
    Home,
    /// Move focus to the last visible row.
    End,
    /// Move focus up one row and extend the selection.
    ShiftUp,
    /// Move focus down one row and extend the selection.
    ShiftDown,
    /// Move focus to the first row regardless of scroll position.
    CtrlHome,
    /// Move focus to the last row regardless of scroll position.
    CtrlEnd,
}

/// Configuration for a single [`DataTable`] column.
///
/// # Examples
///
/// ```
/// use martensite_blessed::data_table::ColumnConfig;
/// let mut col = ColumnConfig::new(120.0);
/// assert!(col.visible());
/// col.set_visible(false);
/// assert!(!col.visible());
/// assert_eq!(col.width(), 120.0);
/// ```
#[derive(Clone, Debug)]
pub struct ColumnConfig {
    width: f64,
    visible: bool,
    sortable: bool,
    resizable: bool,
}

impl ColumnConfig {
    /// Creates a column `width` logical pixels wide, visible, sortable, and
    /// resizable by default.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::ColumnConfig;
    /// let col = ColumnConfig::new(100.0);
    /// assert_eq!(col.width(), 100.0);
    /// assert!(col.sortable());
    /// ```
    pub fn new(width: f32) -> Self {
        Self {
            width: valid_nonnegative(f64::from(width)),
            visible: true,
            sortable: true,
            resizable: true,
        }
    }

    /// Returns the column width in logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::ColumnConfig;
    /// let col = ColumnConfig::new(64.0);
    /// assert_eq!(col.width(), 64.0);
    /// ```
    pub fn width(&self) -> f32 {
        self.width as f32
    }

    /// Returns whether the column is visible.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::ColumnConfig;
    /// let col = ColumnConfig::new(64.0);
    /// assert!(col.visible());
    /// ```
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Returns whether the column is sortable.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::ColumnConfig;
    /// let col = ColumnConfig::new(64.0);
    /// assert!(col.sortable());
    /// ```
    pub fn sortable(&self) -> bool {
        self.sortable
    }

    /// Returns whether the column is resizable.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::ColumnConfig;
    /// let col = ColumnConfig::new(64.0);
    /// assert!(col.resizable());
    /// ```
    pub fn resizable(&self) -> bool {
        self.resizable
    }

    /// Sets the column width in logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::ColumnConfig;
    /// let mut col = ColumnConfig::new(64.0);
    /// col.set_width(200.0);
    /// assert_eq!(col.width(), 200.0);
    /// ```
    pub fn set_width(&mut self, width: f32) {
        self.width = valid_nonnegative(f64::from(width));
    }

    /// Shows or hides the column.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::ColumnConfig;
    /// let mut col = ColumnConfig::new(64.0);
    /// col.set_visible(false);
    /// assert!(!col.visible());
    /// ```
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Enables or disables sorting for the column.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::ColumnConfig;
    /// let mut col = ColumnConfig::new(64.0);
    /// col.set_sortable(false);
    /// assert!(!col.sortable());
    /// ```
    pub fn set_sortable(&mut self, sortable: bool) {
        self.sortable = sortable;
    }

    /// Enables or disables resizing for the column.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_blessed::data_table::ColumnConfig;
    /// let mut col = ColumnConfig::new(64.0);
    /// col.set_resizable(false);
    /// assert!(!col.resizable());
    /// ```
    pub fn set_resizable(&mut self, resizable: bool) {
        self.resizable = resizable;
    }
}

impl Default for ColumnConfig {
    fn default() -> Self {
        Self::new(0.0)
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
    use super::*;

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

    #[test]
    fn sort_then_filter_compose() {
        let mut table = DataTable::new(vec![5_i32, 3, 8, 1, 6], 20.0);
        table.set_viewport_height(200.0);
        table.sort_by(0, ColumnSort::Ascending, |a, b| a.cmp(b));
        table.set_filter(RowFilter::new(|r: &i32| *r >= 3));
        let values: Vec<i32> = table.visible_rows().map(|(_, r)| *r).collect();
        assert_eq!(values, vec![3, 5, 6, 8]);
    }

    #[test]
    fn filter_then_sort_compose() {
        let mut table = DataTable::new(vec![5_i32, 3, 8, 1, 6], 20.0);
        table.set_viewport_height(200.0);
        table.set_filter(RowFilter::new(|r: &i32| *r >= 3));
        table.sort_by(0, ColumnSort::Descending, |a, b| a.cmp(b));
        let values: Vec<i32> = table.visible_rows().map(|(_, r)| *r).collect();
        assert_eq!(values, vec![8, 6, 5, 3]);
    }

    #[test]
    fn selection_extends_across_ranges() {
        let mut sel = SelectionModel::new();
        sel.select(5);
        sel.extend_to(2);
        assert!(sel.is_selected(2));
        assert!(sel.is_selected(5));
        assert!(!sel.is_selected(1));
        assert!(!sel.is_selected(6));
        assert_eq!(sel.selected_count(), 4);
        sel.clear();
        assert!(sel.is_empty());
    }

    #[test]
    fn keyboard_navigation_scrolls() {
        let mut table = DataTable::new(vec![0_u32; 50], 20.0);
        table.set_viewport_height(80.0);
        assert!(table.handle_key(KeyAction::Down));
        assert_eq!(table.focused_row(), Some(1));
        assert!(table.handle_key(KeyAction::CtrlEnd));
        assert_eq!(table.focused_row(), Some(49));
        assert!(table.handle_key(KeyAction::CtrlHome));
        assert_eq!(table.focused_row(), Some(0));
    }

    #[test]
    fn shift_navigation_extends_selection() {
        let mut table = DataTable::new(vec![0_u32; 20], 20.0);
        table.set_viewport_height(80.0);
        table.handle_key(KeyAction::Down);
        table.handle_key(KeyAction::ShiftDown);
        table.handle_key(KeyAction::ShiftDown);
        assert!(table.selection().is_selected(1));
        assert!(table.selection().is_selected(3));
        assert_eq!(table.selection().selected_count(), 3);
    }

    #[test]
    fn column_configuration_round_trips() {
        let mut table = DataTable::new(vec![0_u32; 3], 20.0);
        table.set_columns(vec![ColumnConfig::new(80.0), ColumnConfig::new(40.0)]);
        assert_eq!(table.column_count(), 2);
        table.set_column_width(1, 200.0);
        assert_eq!(table.columns()[1].width(), 200.0);
        table.set_column_visible(0, false);
        assert!(!table.columns()[0].visible());
    }

    /// Steady-state scroll budget: each `visible_rows()` call must complete in
    /// under 8.3 ms (120 fps) while scrolling through one thousand consecutive
    /// offsets on a one-million-row table.
    ///
    /// CI runners are the gate of record for this budget; the test is
    /// `#[ignore]`d by default to avoid flakiness on shared developer
    /// hardware.
    #[test]
    #[ignore]
    fn data_table_1m_steady_scroll() {
        use std::time::Instant;

        let mut table = DataTable::new(vec![0_u32; 1_000_000], 20.0);
        table.set_viewport_height(400.0);
        for i in 0..1000 {
            let offset = i as f32 * 20.0;
            table.set_scroll_offset(offset);
            let start = Instant::now();
            let _ = table.visible_rows().count();
            let elapsed = start.elapsed();
            assert!(
                elapsed.as_secs_f64() < 0.0083,
                "visible_rows took {:?} at offset {} (budget 8.3ms)",
                elapsed,
                offset
            );
        }
    }
}
