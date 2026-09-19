//! `Pagination` widget: a page switcher — prev/next arrows plus a
//! windowed run of page cells with ellipsis gaps (Ant `Pagination`,
//! Carbon `Pagination`, WinUI community `PaginationControl`).
//!
//! Cells are painted hit targets, not child widgets — the control is a
//! flat strip. Poll [`Pagination::take_selected`] for activations.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::pagination::Pagination;
//!
//! let p = Pagination::new().total_pages(20).current(7);
//! assert_eq!(p.current_page(), 7);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

/// Cell text ink.
const INK: [u8; 4] = [40, 42, 48, 255];
/// Disabled arrow ink.
const DIM_INK: [u8; 4] = [170, 173, 180, 255];
/// Selected cell ink.
const SELECTED_INK: [u8; 4] = [255, 255, 255, 255];
/// Selected cell face.
const SELECTED_FACE: [u8; 4] = [70, 110, 200, 255];
/// Hover/focus cell tint.
const HIGHLIGHT: [u8; 4] = [70, 110, 200, 22];
/// Cell size, logical points.
const CELL_PT: f32 = 28.0;
/// Cell gap, logical points.
const GAP_PT: f32 = 4.0;
/// Font size, logical points.
const FONT_PT: f32 = 13.0;
/// Cell corner radius, logical points.
const RADIUS: f32 = 5.0;

/// One cell in the strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    /// ‹ previous page.
    Prev,
    /// A numbered page (1-based).
    Page(usize),
    /// An ellipsis gap (non-interactive).
    Ellipsis,
    /// › next page.
    Next,
}

/// A windowed pagination strip.
///
/// The cell sequence always shows the first and last page, up to
/// `sibling_count` neighbours on each side of the current page, and
/// "…" for the gaps — the standard Ant/Carbon windowing:
/// `‹ 1 … 5 6 [7] 8 9 … 20 ›`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::pagination::Pagination;
///
/// let p = Pagination::new().total_pages(10);
/// assert_eq!(p.total(), 10);
/// ```
pub struct Pagination {
    /// Total page count (≥1).
    total: usize,
    /// Current page, 1-based, clamped into range.
    current: usize,
    /// Numbered cells shown on each side of the current page.
    sibling_count: usize,
    /// Cell index under hover/keyboard focus.
    highlighted: Option<usize>,
    /// Pending selection — drained by `take_selected`.
    selected: Option<usize>,
    /// The resolved cell list from the last `layout`.
    cells: Vec<Cell>,
    /// Hit rect per interactive cell (`cells` and `rects` align;
    /// ellipsis cells get a zero rect).
    rects: Vec<Rect>,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter. See [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Pagination {
    /// Creates a pager with one page.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::pagination::Pagination;
    ///
    /// let p = Pagination::new();
    /// assert_eq!(p.current_page(), 1);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            total: 1,
            current: 1,
            sibling_count: 1,
            highlighted: None,
            selected: None,
            cells: Vec::new(),
            rects: Vec::new(),
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the total page count (clamped to ≥1).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::pagination::Pagination;
    ///
    /// let p = Pagination::new().total_pages(42);
    /// assert_eq!(p.total(), 42);
    /// ```
    #[must_use]
    pub fn total_pages(mut self, total: usize) -> Self {
        self.total = total.max(1);
        self.current = self.current.min(self.total);
        self
    }

    /// Sets the current page (1-based, clamped into range).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::pagination::Pagination;
    ///
    /// let p = Pagination::new().total_pages(9).current(4);
    /// assert_eq!(p.current_page(), 4);
    /// ```
    #[must_use]
    pub fn current(mut self, page: usize) -> Self {
        self.current = page.clamp(1, self.total);
        self
    }

    /// How many numbered cells appear on each side of the current
    /// page (Ant `defaultCurrent` siblings — 1 is typical).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::pagination::Pagination;
    ///
    /// let p = Pagination::new().sibling_count(2);
    /// ```
    #[must_use]
    pub fn sibling_count(mut self, n: usize) -> Self {
        self.sibling_count = n.max(1);
        self
    }

    /// The total page count.
    #[inline]
    #[must_use]
    pub fn total(&self) -> usize {
        self.total
    }

    /// The current page (1-based).
    #[inline]
    #[must_use]
    pub fn current_page(&self) -> usize {
        self.current
    }

    /// Programmatic page change — does not report through
    /// `take_selected`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::pagination::Pagination;
    ///
    /// let mut p = Pagination::new().total_pages(9);
    /// p.set_current(5);
    /// assert_eq!(p.current_page(), 5);
    /// ```
    pub fn set_current(&mut self, page: usize) {
        self.current = page.clamp(1, self.total);
    }

    /// Drains the page the user activated — `Some(page)` once per
    /// selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::pagination::Pagination;
    ///
    /// let mut p = Pagination::new().total_pages(9);
    /// assert_eq!(p.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.selected.take()
    }

    /// Installs a shared shaped-text painter for the cells.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The windowed cell sequence for the current state — pure, so it
    /// is directly testable.
    fn build_cells(&self) -> Vec<Cell> {
        let mut cells = vec![Cell::Prev];
        let mut pages: Vec<Option<usize>> = Vec::with_capacity(self.total.min(11));
        // Window: first, last, and current ± siblings.
        let lo = self.current.saturating_sub(self.sibling_count).max(1);
        let hi = (self.current + self.sibling_count).min(self.total);
        let mut page = 1;
        while page <= self.total {
            if page == 1 || page == self.total || (page >= lo && page <= hi) {
                pages.push(Some(page));
            } else if pages.last() != Some(&None) {
                pages.push(None);
            }
            page += 1;
        }
        for p in pages {
            cells.push(match p {
                Some(n) => Cell::Page(n),
                None => Cell::Ellipsis,
            });
        }
        cells.push(Cell::Next);
        cells
    }

    /// The page a cell activates (None for ellipses/disabled arrows).
    fn cell_page(&self, cell: Cell) -> Option<usize> {
        match cell {
            Cell::Prev if self.current > 1 => Some(self.current - 1),
            Cell::Next if self.current < self.total => Some(self.current + 1),
            Cell::Page(n) => Some(n),
            _ => None,
        }
    }

    /// Selects the cell at `index`, if it activates a page.
    fn activate(&mut self, index: usize) {
        if let Some(&cell) = self.cells.get(index) {
            if let Some(page) = self.cell_page(cell) {
                if page != self.current {
                    self.current = page;
                    self.selected = Some(page);
                }
            }
        }
    }
}

impl Default for Pagination {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Pagination {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let cells = self.build_cells().len() as f32;
        let w = cx.pt(CELL_PT * cells + GAP_PT * (cells - 1.0).max(0.0));
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            cx.pt(CELL_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.cells = self.build_cells();
        self.rects.clear();
        let cell = cx.pt(CELL_PT);
        let gap = cx.pt(GAP_PT);
        let total_w = self.cells.len() as f32 * cell + (self.cells.len() - 1) as f32 * gap;
        let mut x = bounds.origin.x + (bounds.size.x - total_w).max(0.0) / 2.0;
        let y = bounds.origin.y + (bounds.size.y - cell).max(0.0) / 2.0;
        for c in &self.cells {
            // Ellipses get a display slot but no hit target.
            let interactive = !matches!(c, Cell::Ellipsis);
            self.rects.push(if interactive {
                Rect::new(x, y, cell, cell)
            } else {
                Rect::default()
            });
            x += cell + gap;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Navigation);
        node.set_label("Pagination");
        node.add_action(accesskit::Action::Focus);
        node.set_value(format!("Page {} of {}", self.current, self.total));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.rects.iter().position(|r| r.contains(*position));
                if hit != self.highlighted {
                    self.highlighted = hit;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerLeave => {
                self.highlighted = None;
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self.rects.iter().position(|r| r.contains(*position)) {
                    self.activate(i);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowLeft" | "ArrowRight" => {
                    let interactive: Vec<usize> = self
                        .cells
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| !matches!(c, Cell::Ellipsis))
                        .map(|(i, _)| i)
                        .collect();
                    if interactive.is_empty() {
                        return EventResponse::Ignored;
                    }
                    let pos = interactive
                        .iter()
                        .position(|i| Some(*i) == self.highlighted)
                        .map(|p| p as isize)
                        .unwrap_or(-1);
                    let next = match key.as_str() {
                        "ArrowRight" => (pos + 1).min(interactive.len() as isize - 1),
                        _ => (pos - 1).max(0),
                    };
                    self.highlighted = Some(interactive[next as usize]);
                    EventResponse::RequestRepaint
                }
                "Enter" | "Space" | " " => {
                    if let Some(i) = self.highlighted {
                        self.activate(i);
                    }
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::SemanticAction(SemanticAction::Increment) => {
                if self.current < self.total {
                    self.current += 1;
                    self.selected = Some(self.current);
                }
                EventResponse::Handled
            }
            WidgetEvent::SemanticAction(SemanticAction::Decrement) => {
                if self.current > 1 {
                    self.current -= 1;
                    self.selected = Some(self.current);
                }
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let font = cx.pt(FONT_PT);
        for (i, cell) in self.cells.iter().enumerate() {
            let r = self.rects.get(i).copied().unwrap_or_default();
            let page = self.cell_page(*cell);
            let is_current = matches!(cell, Cell::Page(n) if *n == self.current);
            let enabled = page.is_some() && !is_current;

            if is_current {
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(r.min_x()),
                        f64::from(r.min_y()),
                        f64::from(r.max_x()),
                        f64::from(r.max_y()),
                    ),
                    &martensite_core::shape::Shape::rounded(cx.pt(RADIUS)),
                    cx.color(TokenKey::AccentColor, SELECTED_FACE),
                );
            } else if self.highlighted == Some(i) && enabled {
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(r.min_x()),
                        f64::from(r.min_y()),
                        f64::from(r.max_x()),
                        f64::from(r.max_y()),
                    ),
                    &martensite_core::shape::Shape::rounded(cx.pt(RADIUS)),
                    HIGHLIGHT,
                );
            }

            let label = match cell {
                Cell::Prev => "‹",
                Cell::Next => "›",
                Cell::Ellipsis => "…",
                Cell::Page(n) => {
                    let s = n.to_string();
                    let w = painter
                        .and_then(|p| p.measure_text(&s, font))
                        .unwrap_or(font * s.len() as f32 * 0.55);
                    let x = r.origin.x + (r.size.x - w.min(r.size.x)) / 2.0;
                    let y = r.origin.y + (r.size.y - font) / 2.0;
                    crate::text_paint::paint_label_clipped(
                        painter,
                        cx.list,
                        kurbo::Rect::new(
                            f64::from(r.min_x()),
                            f64::from(r.min_y()),
                            f64::from(r.max_x()),
                            f64::from(r.max_y()),
                        ),
                        kurbo::Point::new(f64::from(x), f64::from(y)),
                        &s,
                        font,
                        if is_current {
                            cx.color(TokenKey::TextInverseColor, SELECTED_INK)
                        } else {
                            cx.color(TokenKey::TextColor, INK)
                        },
                    );
                    continue;
                }
            };
            let w = painter
                .and_then(|p| p.measure_text(label, font))
                .unwrap_or(font * 0.6);
            // Ellipses center in their slot; arrows in their cells.
            let slot_x = if matches!(cell, Cell::Ellipsis) {
                r.origin.x + r.size.x / 2.0 - w / 2.0
            } else {
                r.origin.x + (r.size.x - w) / 2.0
            };
            let y = r.origin.y + (r.size.y - font) / 2.0;
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(r.min_x()),
                    f64::from(r.min_y()),
                    f64::from(r.max_x()),
                    f64::from(r.max_y()),
                ),
                kurbo::Point::new(f64::from(slot_x), f64::from(y)),
                label,
                font,
                if enabled {
                    cx.color(TokenKey::TextColor, INK)
                } else {
                    cx.color(TokenKey::TextMutedColor, DIM_INK)
                },
            );
        }
    }
}

impl std::fmt::Debug for Pagination {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pagination")
            .field("current", &self.current)
            .field("total", &self.total)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 600.0, 40.0),
            scale: 1.0,
        }
    }

    fn laid_out(total: usize, current: usize) -> Pagination {
        let mut p = Pagination::new().total_pages(total).current(current);
        let mut hot = HotNode::default();
        p.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 600.0, 40.0));
        p
    }

    fn cell_pages(p: &Pagination) -> Vec<Option<usize>> {
        p.cells
            .iter()
            .map(|c| match c {
                Cell::Page(n) => Some(*n),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn small_total_no_ellipsis() {
        let p = laid_out(5, 3);
        assert_eq!(
            cell_pages(&p),
            vec![None, Some(1), Some(2), Some(3), Some(4), Some(5), None]
        );
        assert!(p.cells.iter().all(|c| !matches!(c, Cell::Ellipsis)));
    }

    #[test]
    fn large_total_windows_with_ellipses() {
        let p = laid_out(20, 10);
        // ‹ 1 … 9 10 11 … 20 ›
        assert_eq!(
            cell_pages(&p),
            vec![
                None,
                Some(1),
                None,
                Some(9),
                Some(10),
                Some(11),
                None,
                Some(20),
                None
            ]
        );
        assert_eq!(
            p.cells
                .iter()
                .filter(|c| matches!(c, Cell::Ellipsis))
                .count(),
            2
        );
    }

    #[test]
    fn near_start_shows_leading_run() {
        let p = laid_out(20, 2);
        // ‹ 1 2 3 … 20 ›
        assert_eq!(
            cell_pages(&p),
            vec![None, Some(1), Some(2), Some(3), None, Some(20), None]
        );
    }

    #[test]
    fn click_page_selects() {
        let mut p = laid_out(9, 2);
        // Cells for current=2: ‹,1,2,3,…,9,› — page 3 sits at index 3.
        let r = p.rects[3];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(r.origin.x + 2.0, r.origin.y + 2.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(p.event(&mut ev(&press)), EventResponse::Handled);
        assert_eq!(p.take_selected(), Some(3));
        assert_eq!(p.current_page(), 3);
    }

    #[test]
    fn next_arrow_advances() {
        let mut p = laid_out(9, 4);
        let next = *p.rects.last().unwrap();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(next.origin.x + 2.0, next.origin.y + 2.0),
            button: PointerButton::Primary,
            count: 1,
        };
        p.event(&mut ev(&press));
        assert_eq!(p.take_selected(), Some(5));
    }

    #[test]
    fn prev_disabled_on_first_page() {
        let mut p = laid_out(9, 1);
        let prev = p.rects[0];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(prev.origin.x + 2.0, prev.origin.y + 2.0),
            button: PointerButton::Primary,
            count: 1,
        };
        p.event(&mut ev(&press));
        assert_eq!(p.take_selected(), None);
        assert_eq!(p.current_page(), 1);
    }

    #[test]
    fn semantic_increment_decrement() {
        let mut p = laid_out(9, 5);
        let inc = WidgetEvent::SemanticAction(SemanticAction::Increment);
        p.event(&mut ev(&inc));
        assert_eq!(p.take_selected(), Some(6));
        let dec = WidgetEvent::SemanticAction(SemanticAction::Decrement);
        p.event(&mut ev(&dec));
        p.event(&mut ev(&dec));
        assert_eq!(p.take_selected(), Some(4));
    }

    #[test]
    fn current_clamped_into_range() {
        let p = Pagination::new().total_pages(5).current(99);
        assert_eq!(p.current_page(), 5);
        let mut p2 = Pagination::new().current(4);
        p2.total = 3;
        p2.set_current(0);
        assert_eq!(p2.current_page(), 1);
    }
}
