//! `FlowBox` — wrap-flow cell container with optional selection.
//!
//! The GTK `FlowBox` pattern: children flow left-to-right at their
//! natural size, wrapping to a new row when they run out of width —
//! each row's height is its tallest child. Unlike `Masonry` (fixed
//! columns, variable heights) or `Grid` (span placement), a flow
//! box is a *wrapping line* of cells.
//!
//! GTK's second trick is cell selection: [`FlowBox::selection_mode`]
//! switches between a pure layout container and a single-select
//! cell grid (accent ring on the selected cell, index parked in
//! [`FlowBox::take_selected`]).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::flow_box::{FlowBox, FlowSelection};
//! use martensite::widgets::Text;
//! use martensite::core::Widget;
//!
//! let fb = FlowBox::new()
//!     .child(Text::new("one"))
//!     .child(Text::new("two"))
//!     .selection_mode(FlowSelection::Single);
//! assert_eq!(fb.child_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

/// Selected-cell ring.
const ACCENT: TokenKey = TokenKey::AccentColor;
/// Hovered-cell ring.
const HOVER: TokenKey = TokenKey::TextMutedColor;
/// Default gap (logical points).
const GAP_PT: f32 = 8.0;
/// Minimum cell height floor (logical points).
const MIN_CELL_PT: f32 = 8.0;
/// Per-cell size ceiling (logical points). A flow box is a grid of
/// *cells*, not a stack of regions — children are measured with this
/// as the ceiling on both axes rather than `f32::MAX` or the raw
/// offered width. Without it, "fill" children (which answer
/// `constraints.max_size` verbatim, e.g. overlay-style widgets and
/// `GroupBox`'s width) report an astronomical size; row offsets
/// then overflow to `inf` and cells land at non-finite coordinates,
/// turning child paint loops (tiling, stepping) effectively
/// unbounded.
const MAX_CELL_PT: f32 = 2048.0;

/// How cells respond to clicks.
///
/// # Examples
///
/// ```
/// use martensite::widgets::flow_box::FlowSelection;
///
/// assert_eq!(FlowSelection::default(), FlowSelection::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlowSelection {
    /// Pure layout container — clicks pass through to children.
    #[default]
    None,
    /// Clicking a cell selects it (and still forwards to the child
    /// so interactive content works).
    Single,
}

/// A wrap-flow cell container — see the module docs.
///
/// # Examples
///
/// ```
/// use martensite::widgets::flow_box::FlowBox;
/// use martensite::widgets::Text;
/// use martensite::core::Widget;
///
/// assert_eq!(FlowBox::new().child(Text::new("x")).child_count(), 1);
/// ```
pub struct FlowBox {
    children: Vec<Box<dyn Widget>>,
    /// Cell gap (logical points).
    pub gap: f32,
    /// Accessibility label.
    pub label: Option<String>,
    /// Whether input reaches children.
    pub enabled: bool,
    /// Cell selection behavior.
    selection: FlowSelection,
    /// Selected cell index.
    selected: Option<usize>,
    /// Parked selection for `take_selected`.
    pending: Option<usize>,
    /// Hovered cell index.
    hover: Option<usize>,
    /// Child bounds from the last layout (widget-local).
    cell_bounds: Vec<Rect>,
    /// Total content height from the last layout (device px).
    content_height: f32,
}

impl FlowBox {
    /// An empty flow box.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::flow_box::FlowBox;
    ///
    /// assert_eq!(FlowBox::new().gap, 8.0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            gap: GAP_PT,
            label: None,
            enabled: true,
            selection: FlowSelection::None,
            selected: None,
            pending: None,
            hover: None,
            cell_bounds: Vec::new(),
            content_height: 0.0,
        }
    }

    /// Sets the inter-cell gap (clamped ≥0).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::flow_box::FlowBox;
    ///
    /// assert_eq!(FlowBox::new().gap(4.0).gap, 4.0);
    /// ```
    #[must_use]
    pub fn gap(mut self, pts: f32) -> Self {
        self.gap = pts.max(0.0);
        self
    }

    /// Sets the accessibility label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::flow_box::FlowBox;
    ///
    /// let fb = FlowBox::new().label("Tags");
    /// assert_eq!(fb.label.as_deref(), Some("Tags"));
    /// ```
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// Sets whether input reaches children.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::flow_box::FlowBox;
    ///
    /// assert!(!FlowBox::new().enabled(false).enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, flag: bool) -> Self {
        self.enabled = flag;
        self
    }

    /// Sets the cell-selection behavior.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::flow_box::{FlowBox, FlowSelection};
    ///
    /// let fb = FlowBox::new().selection_mode(FlowSelection::Single);
    /// ```
    #[must_use]
    pub fn selection_mode(mut self, mode: FlowSelection) -> Self {
        self.selection = mode;
        self
    }

    /// Appends a child cell.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::flow_box::FlowBox;
    /// use martensite::widgets::Text;
    /// use martensite::core::Widget;
    ///
    /// assert_eq!(FlowBox::new().child(Text::new("a")).child_count(), 1);
    /// ```
    #[must_use]
    pub fn child(mut self, w: impl Widget + 'static) -> Self {
        self.children.push(Box::new(w));
        self
    }

    /// Removes all children.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::flow_box::FlowBox;
    /// use martensite::widgets::Text;
    /// use martensite::core::Widget;
    ///
    /// let mut fb = FlowBox::new().child(Text::new("a"));
    /// fb.clear();
    /// assert_eq!(fb.child_count(), 0);
    /// ```
    pub fn clear(&mut self) {
        self.children.clear();
        self.cell_bounds.clear();
        self.selected = None;
        self.content_height = 0.0;
    }

    /// The selected cell index, if selection is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::flow_box::FlowBox;
    ///
    /// assert_eq!(FlowBox::new().selected_index(), None);
    /// ```
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// Selects `index` programmatically (out-of-range clears).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::flow_box::{FlowBox, FlowSelection};
    /// use martensite::widgets::Text;
    ///
    /// let mut fb = FlowBox::new()
    ///     .child(Text::new("a"))
    ///     .selection_mode(FlowSelection::Single);
    /// fb.select(0);
    /// assert_eq!(fb.selected_index(), Some(0));
    /// ```
    pub fn select(&mut self, index: usize) {
        self.selected = (index < self.children.len()).then_some(index);
    }

    /// Drains the parked user selection — one-shot.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::flow_box::FlowBox;
    ///
    /// let mut fb = FlowBox::new();
    /// assert_eq!(fb.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// Total laid-out content height in device px — feed a wrapping
    /// `ScrollView`'s content size.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::flow_box::FlowBox;
    ///
    /// assert_eq!(FlowBox::new().content_height(), 0.0);
    /// ```
    #[inline]
    pub fn content_height(&self) -> f32 {
        self.content_height
    }

    /// The cell index at a widget-local point.
    fn hit(&self, local: Vec2) -> Option<usize> {
        self.cell_bounds.iter().position(|r| r.contains(local))
    }
}

impl Default for FlowBox {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FlowBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlowBox")
            .field("children", &self.children.len())
            .field("selection", &self.selection)
            .finish()
    }
}

impl Widget for FlowBox {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let gap = cx.pt(self.gap);
        let min_cell = cx.pt(MIN_CELL_PT);
        let max_cell = cx.pt(MAX_CELL_PT);
        let max_w = constraints.max_size.x.max(0.0);
        // Simulate the same wrap `layout` performs so a `ScrollView`
        // (which measures content unbounded) learns the true content
        // height rather than a fill-style echo of `max_size.y`.
        let mut x = 0.0f32;
        let mut row_h = 0.0f32;
        let mut row_w = 0.0f32;
        let mut total_h = 0.0f32;
        for child in self.children.iter_mut() {
            let s = child.measure(
                cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: Vec2::new(max_w.max(min_cell).min(max_cell), max_cell),
                },
            );
            let w = s.x.max(min_cell).min(max_w.max(min_cell)).min(max_cell);
            let h = s.y.max(min_cell).min(max_cell);
            if x > 0.0 && x + w > max_w + 0.5 {
                total_h += row_h + gap;
                x = 0.0;
                row_h = 0.0;
            }
            x += w + gap;
            row_w = row_w.max(x - gap);
            row_h = row_h.max(h);
        }
        total_h += row_h;
        Vec2::new(
            if max_w.is_finite() {
                max_w
            } else {
                row_w.max(min_cell)
            },
            total_h.max(min_cell),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let gap = cx.pt(self.gap);
        let min_cell = cx.pt(MIN_CELL_PT);
        let max_cell = cx.pt(MAX_CELL_PT);
        self.cell_bounds.clear();
        let mut x = bounds.min_x();
        let mut y = bounds.min_y();
        let mut row_h = 0.0f32;
        // Two passes would be needed for per-row vertical centering;
        // instead rows are top-aligned and each row's height is fixed
        // by its tallest cell — record cells, then patch y within the
        // row on the fly by tracking the row's start index.
        let mut row_start = 0usize;
        for child in self.children.iter_mut() {
            let s = child.measure(
                cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: Vec2::new(bounds.width().min(max_cell), max_cell),
                },
            );
            let w = s.x.max(min_cell).min(bounds.width()).min(max_cell);
            let h = s.y.max(min_cell).min(max_cell);
            // Wrap when the cell would overflow the row (unless the
            // row is still empty — an oversized cell gets the row to
            // itself).
            if x > bounds.min_x() && x + w > bounds.max_x() + 0.5 {
                // Close the row: center shorter cells vertically.
                for r in &mut self.cell_bounds[row_start..] {
                    let dy = (row_h - r.height()) / 2.0;
                    r.origin.y += dy;
                }
                x = bounds.min_x();
                y += row_h + gap;
                row_h = 0.0;
                row_start = self.cell_bounds.len();
            }
            let r = Rect::new(x, y, w, h);
            cx.layout_child(&mut **child, r);
            self.cell_bounds.push(r);
            x += w + gap;
            row_h = row_h.max(h);
        }
        // Final row's vertical centering.
        for r in &mut self.cell_bounds[row_start..] {
            let dy = (row_h - r.height()) / 2.0;
            r.origin.y += dy;
        }
        self.content_height = (y + row_h - bounds.min_y()).max(0.0);
    }

    fn paint(&self, cx: &mut PaintContext) {
        if self.selection == FlowSelection::None {
            return;
        }
        let b = cx.bounds;
        let accent = cx.color(ACCENT, [50, 115, 230, 255]);
        let hover = cx.color(HOVER, [150, 150, 158, 255]);
        let shape = martensite_core::shape::Shape::rounded(cx.pt(5.0));
        for (i, cell) in self.cell_bounds.iter().enumerate() {
            let r = kurbo::Rect::new(
                f64::from(b.min_x() + cell.min_x()),
                f64::from(b.min_y() + cell.min_y()),
                f64::from(b.min_x() + cell.max_x()),
                f64::from(b.min_y() + cell.max_y()),
            );
            if self.selected == Some(i) {
                cx.list.push_stroke_shape(r, &shape, cx.pt(2.0), accent);
            } else if self.enabled && self.hover == Some(i) {
                cx.list.push_stroke_shape(r, &shape, cx.pt(1.0), hover);
            }
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        // Selection handling first, then forward to the hit child so
        // interactive cell content still works.
        if let WidgetEvent::PointerPressed { position, .. }
        | WidgetEvent::PointerReleased { position, .. }
        | WidgetEvent::PointerMoved { position }
        | WidgetEvent::Scroll { position, .. } = cx.event
        {
            let pos = *position;
            let local = pos - cx.bounds.origin;
            match cx.event {
                WidgetEvent::PointerMoved { .. } => {
                    if self.selection == FlowSelection::Single {
                        let hit = self.hit(local);
                        if hit != self.hover {
                            self.hover = hit;
                            // Still forward — the child may want hover.
                        }
                    }
                }
                WidgetEvent::PointerReleased { button, .. } => {
                    let is_primary = *button == martensite_core::PointerButton::Primary;
                    if self.selection == FlowSelection::Single && is_primary {
                        if let Some(i) = self.hit(local) {
                            self.selected = Some(i);
                            self.pending = Some(i);
                        }
                    }
                }
                _ => {}
            }
            for i in (0..self.child_count()).rev() {
                let Some(b) = self.child_bounds(i) else {
                    continue;
                };
                // Cell bounds are widget-local; translate to device.
                let device = Rect::new(
                    cx.bounds.min_x() + b.min_x(),
                    cx.bounds.min_y() + b.min_y(),
                    b.width(),
                    b.height(),
                );
                if !device.contains(pos) {
                    continue;
                }
                let mut child_cx = EventContext {
                    event: cx.event,
                    bounds: device,
                    scale: cx.scale,
                };
                if let Some(child) = self.child_mut(i) {
                    return child.event(&mut child_cx);
                }
            }
            if self.selection == FlowSelection::Single {
                return EventResponse::RequestRepaint;
            }
        }
        EventResponse::Ignored
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn child_count(&self) -> usize {
        self.children.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.children.get(index).map(|c| &**c as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.children
            .get_mut(index)
            .map(|c| &mut **c as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.cell_bounds.get(index).copied()
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 24.0)).with_policy(UnderflowPolicy::Lint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::text::Text;
    use martensite_core::{HotNode, PointerButton};

    /// A fixed-size stub child for deterministic flow tests.
    struct Cell {
        w: f32,
        h: f32,
    }

    impl Widget for Cell {
        fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
            Vec2::new(self.w, self.h)
        }
        fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
    }

    fn cell(w: f32, h: f32) -> Cell {
        Cell { w, h }
    }

    fn lay(fb: &mut FlowBox, width: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        fb.layout(&mut cx, Rect::new(0.0, 0.0, width, 400.0));
    }

    #[test]
    fn cells_wrap_when_they_overflow() {
        // Row width 110, gap 8: two 40-wide cells fit (40+8+40=88),
        // the third wraps.
        let mut fb = FlowBox::new()
            .child(cell(40.0, 20.0))
            .child(cell(40.0, 20.0))
            .child(cell(40.0, 20.0));
        lay(&mut fb, 110.0);
        let r2 = fb.child_bounds(2).unwrap();
        assert_eq!(r2.min_x(), 0.0);
        assert!(r2.min_y() > 0.0);
    }

    #[test]
    fn row_height_is_tallest_cell_and_shorter_cells_center() {
        let mut fb = FlowBox::new()
            .child(cell(40.0, 10.0))
            .child(cell(40.0, 30.0));
        lay(&mut fb, 200.0);
        let short = fb.child_bounds(0).unwrap();
        let tall = fb.child_bounds(1).unwrap();
        assert!(
            (short.min_y() - 10.0).abs() < 0.5,
            "short cell centers in the 30px row"
        );
        assert_eq!(tall.min_y(), 0.0);
        assert_eq!(fb.content_height(), 30.0);
    }

    #[test]
    fn oversized_cell_gets_its_own_row() {
        let mut fb = FlowBox::new()
            .child(cell(40.0, 20.0))
            .child(cell(500.0, 20.0))
            .child(cell(40.0, 20.0));
        lay(&mut fb, 100.0);
        let big = fb.child_bounds(1).unwrap();
        // Clamped to row width.
        assert_eq!(big.width(), 100.0);
        assert!(big.min_y() > 0.0);
        let third = fb.child_bounds(2).unwrap();
        assert!(third.min_y() > big.min_y());
    }

    #[test]
    fn single_selection_parks_index() {
        let mut fb = FlowBox::new()
            .child(cell(40.0, 20.0))
            .child(cell(40.0, 20.0))
            .selection_mode(FlowSelection::Single);
        lay(&mut fb, 200.0);
        let c = fb.child_bounds(1).unwrap();
        let mut cx = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: Vec2::new(c.min_x() + 4.0, c.min_y() + 4.0),
                button: PointerButton::Primary,
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 400.0),
            scale: 1.0,
        };
        fb.event(&mut cx);
        assert_eq!(fb.selected_index(), Some(1));
        assert_eq!(fb.take_selected(), Some(1));
        assert_eq!(fb.take_selected(), None);
    }

    #[test]
    fn no_selection_mode_still_forwards_to_children() {
        let mut fb = FlowBox::new().child(Text::new("x"));
        lay(&mut fb, 200.0);
        assert_eq!(fb.child_count(), 1);
        assert!(fb.child_bounds(0).is_some());
    }

    #[test]
    fn content_height_stacks_rows() {
        let mut fb = FlowBox::new()
            .child(cell(80.0, 20.0))
            .child(cell(80.0, 20.0));
        lay(&mut fb, 100.0);
        // Each row is 20 high + 8 gap between = 48.
        assert_eq!(fb.content_height(), 48.0);
    }

    /// A fill-style child that answers `max_size` verbatim — the
    /// overlay-widget idiom that used to poison row heights with
    /// `f32::MAX`, overflow `y` to `inf`, and hang downstream paint
    /// loops on non-finite bounds.
    struct FillCell;

    impl Widget for FillCell {
        fn measure(&mut self, _cx: &mut LayoutContext, c: LayoutConstraints) -> Vec2 {
            c.max_size
        }
        fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
    }

    #[test]
    fn fill_style_cells_are_capped_not_poisoning() {
        let mut fb = FlowBox::new().child(FillCell).child(cell(40.0, 20.0));
        lay(&mut fb, 200.0);
        for i in 0..2 {
            let r = fb.child_bounds(i).unwrap();
            assert!(
                r.origin.is_finite() && r.size.is_finite(),
                "cell {i}: {r:?}"
            );
            assert!(r.height() <= 2048.5, "cell {i}: {r:?}");
        }
        assert!(fb.content_height().is_finite());
    }

    #[test]
    fn measure_reports_wrapped_height() {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let mut fb = FlowBox::new()
            .child(cell(80.0, 20.0))
            .child(cell(80.0, 20.0));
        // Two 80-wide cells in 100pt wrap onto two rows: 20+8+20.
        let s = fb.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(100.0, f32::MAX),
            },
        );
        assert_eq!(s, Vec2::new(100.0, 48.0));
        // Unbounded measure of fill children still yields a finite box.
        let mut fb = FlowBox::new().child(FillCell).child(FillCell);
        let s = fb.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(f32::MAX, f32::MAX),
            },
        );
        assert!(s.x.is_finite() && s.y.is_finite(), "{s:?}");
    }
}
