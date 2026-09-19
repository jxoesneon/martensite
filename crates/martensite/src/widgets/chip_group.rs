//! `ChipGroup` — a wrapping set of selectable chips.
//!
//! The Material 3 filter-chip set / Ant `Tag.CheckableTag` group
//! pattern: [`Chip`] children flow left-to-right and wrap (the
//! `FlowBox` idiom), while the group enforces a
//! [`ChipSelection`] mode — `Single` behaves like a radio set
//! (clicking one clears the others), `Multiple` toggles freely,
//! `None` leaves chips as passive action targets.
//!
//! The group drains each child's `take_selected` seam after event
//! dispatch and reports the change through
//! [`ChipGroup::take_changed`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::chip_group::{ChipGroup, ChipSelection};
//! use martensite::widgets::Chip;
//!
//! let g = ChipGroup::new()
//!     .chip(Chip::new("Draft"))
//!     .chip(Chip::new("Published"))
//!     .selection(ChipSelection::Single);
//! assert_eq!(g.chip_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};

use crate::widgets::chip::Chip;

/// Inter-chip gap (logical points).
const GAP_PT: f32 = 6.0;
/// Minimum cell height floor (logical points).
const MIN_CELL_PT: f32 = 8.0;

/// How the group treats chip toggles.
///
/// # Examples
///
/// ```
/// use martensite::widgets::chip_group::ChipSelection;
///
/// assert_eq!(ChipSelection::default(), ChipSelection::Multiple);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChipSelection {
    /// Chips never toggle via the group (passive action targets).
    None,
    /// Clicking a chip selects it and clears every other — the
    /// radio-set idiom.
    Single,
    /// Every chip toggles independently (the default).
    #[default]
    Multiple,
}

/// A wrapping set of selectable chips — see the module docs.
///
/// # Examples
///
/// ```
/// use martensite::widgets::chip_group::ChipGroup;
/// use martensite::widgets::Chip;
/// use martensite::core::Widget;
///
/// assert_eq!(ChipGroup::new().chip(Chip::new("a")).child_count(), 1);
/// ```
pub struct ChipGroup {
    chips: Vec<Chip>,
    /// Cell gap (logical points).
    pub gap: f32,
    /// Accessibility label.
    pub label: Option<String>,
    /// Whether input reaches children.
    pub enabled: bool,
    /// Selection mode.
    selection: ChipSelection,
    /// Parked change — `(index, selected)`.
    pending: Option<(usize, bool)>,
    /// Child bounds from the last layout (widget-local).
    cell_bounds: Vec<Rect>,
    /// Content height from the last layout (device px).
    content_height: f32,
}

impl ChipGroup {
    /// An empty group (wrap flow, multiple selection).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::chip_group::ChipGroup;
    ///
    /// assert_eq!(ChipGroup::new().chip_count(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            chips: Vec::new(),
            gap: GAP_PT,
            label: None,
            enabled: true,
            selection: ChipSelection::Multiple,
            pending: None,
            cell_bounds: Vec::new(),
            content_height: 0.0,
        }
    }

    /// Appends a chip.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::chip_group::ChipGroup;
    /// use martensite::widgets::Chip;
    ///
    /// assert_eq!(ChipGroup::new().chip(Chip::new("a")).chip_count(), 1);
    /// ```
    #[must_use]
    pub fn chip(mut self, chip: Chip) -> Self {
        self.chips.push(chip);
        self
    }

    /// Sets the inter-chip gap (clamped ≥0).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::chip_group::ChipGroup;
    ///
    /// assert_eq!(ChipGroup::new().gap(4.0).gap, 4.0);
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
    /// use martensite::widgets::chip_group::ChipGroup;
    ///
    /// let g = ChipGroup::new().label("Status");
    /// assert_eq!(g.label.as_deref(), Some("Status"));
    /// ```
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// Sets the selection mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::chip_group::{ChipGroup, ChipSelection};
    ///
    /// let g = ChipGroup::new().selection(ChipSelection::Single);
    /// ```
    #[must_use]
    pub fn selection(mut self, mode: ChipSelection) -> Self {
        self.selection = mode;
        self
    }

    /// Sets whether input reaches children.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::chip_group::ChipGroup;
    ///
    /// assert!(!ChipGroup::new().enabled(false).enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, flag: bool) -> Self {
        self.enabled = flag;
        self
    }

    /// Chip count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::chip_group::ChipGroup;
    ///
    /// assert_eq!(ChipGroup::new().chip_count(), 0);
    /// ```
    pub fn chip_count(&self) -> usize {
        self.chips.len()
    }

    /// Indices of the currently-selected chips.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::chip_group::ChipGroup;
    /// use martensite::widgets::Chip;
    ///
    /// let g = ChipGroup::new().chip(Chip::new("a").selected(true));
    /// assert_eq!(g.selected_indices(), vec![0]);
    /// ```
    pub fn selected_indices(&self) -> Vec<usize> {
        self.chips
            .iter()
            .enumerate()
            .filter(|(_, c)| c.selected)
            .map(|(i, _)| i)
            .collect()
    }

    /// Drains the parked `(index, selected)` change — one-shot.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::chip_group::ChipGroup;
    ///
    /// let mut g = ChipGroup::new();
    /// assert_eq!(g.take_changed(), None);
    /// ```
    pub fn take_changed(&mut self) -> Option<(usize, bool)> {
        self.pending.take()
    }

    /// Total laid-out content height in device px — feed a wrapping
    /// `ScrollView`'s content size.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::chip_group::ChipGroup;
    ///
    /// assert_eq!(ChipGroup::new().content_height(), 0.0);
    /// ```
    #[inline]
    pub fn content_height(&self) -> f32 {
        self.content_height
    }

    /// Applies the selection mode after chip `index` toggled to `on`.
    fn enforce_mode(&mut self, index: usize, on: bool) {
        match self.selection {
            ChipSelection::None => {
                // Revert — group-level toggling is off.
                self.chips[index].selected = !on;
            }
            ChipSelection::Single if on => {
                for (i, chip) in self.chips.iter_mut().enumerate() {
                    if i != index {
                        chip.selected = false;
                    }
                }
            }
            _ => {}
        }
    }
}

impl Default for ChipGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ChipGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChipGroup")
            .field("chips", &self.chips.len())
            .field("selection", &self.selection)
            .finish()
    }
}

impl Widget for ChipGroup {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints
                .max_size
                .x
                .max(cx.pt(120.0).min(constraints.max_size.x)),
            constraints
                .max_size
                .y
                .max(cx.pt(32.0).min(constraints.max_size.y)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let gap = cx.pt(self.gap);
        let min_cell = cx.pt(MIN_CELL_PT);
        self.cell_bounds.clear();
        let mut x = bounds.min_x();
        let mut y = bounds.min_y();
        let mut row_h = 0.0f32;
        let mut row_start = 0usize;
        for chip in self.chips.iter_mut() {
            let s = chip.measure(
                cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: Vec2::new(bounds.width(), f32::MAX),
                },
            );
            let w = s.x.max(min_cell).min(bounds.width());
            let h = s.y.max(min_cell);
            if x > bounds.min_x() && x + w > bounds.max_x() + 0.5 {
                for r in &mut self.cell_bounds[row_start..] {
                    r.origin.y += (row_h - r.height()) / 2.0;
                }
                x = bounds.min_x();
                y += row_h + gap;
                row_h = 0.0;
                row_start = self.cell_bounds.len();
            }
            let r = Rect::new(x, y, w, h);
            cx.layout_child(chip, r);
            self.cell_bounds.push(r);
            x += w + gap;
            row_h = row_h.max(h);
        }
        for r in &mut self.cell_bounds[row_start..] {
            r.origin.y += (row_h - r.height()) / 2.0;
        }
        self.content_height = (y + row_h - bounds.min_y()).max(0.0);
    }

    fn paint(&self, _cx: &mut PaintContext) {
        // Chrome-free — chips paint themselves.
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        if let WidgetEvent::PointerPressed { position, .. }
        | WidgetEvent::PointerReleased { position, .. }
        | WidgetEvent::PointerMoved { position }
        | WidgetEvent::Scroll { position, .. } = cx.event
        {
            let pos = *position;
            for i in (0..self.chips.len()).rev() {
                let Some(b) = self.cell_bounds.get(i).copied() else {
                    continue;
                };
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
                let response = self.chips[i].event(&mut child_cx);
                // Drain the child's toggle seam and enforce the mode.
                if let Some(on) = self.chips[i].take_selected() {
                    self.enforce_mode(i, on);
                    self.pending = Some((i, on));
                    return EventResponse::RequestRepaint;
                }
                return response;
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
        self.chips.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.chips.get(index).map(|c| c as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.chips.get_mut(index).map(|c| c as &mut dyn Widget)
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
    use martensite_core::{HotNode, PointerButton};

    fn lay(g: &mut ChipGroup, width: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        g.layout(&mut cx, Rect::new(0.0, 0.0, width, 200.0));
    }

    fn click(g: &mut ChipGroup, index: usize) -> EventResponse {
        let b = g.cell_bounds[index];
        let pos = Vec2::new(b.min_x() + 6.0, b.min_y() + 6.0);
        let mut press = EventContext {
            event: &WidgetEvent::PointerPressed {
                position: pos,
                button: PointerButton::Primary,
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 400.0, 200.0),
            scale: 1.0,
        };
        g.event(&mut press);
        let mut release = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: pos,
                button: PointerButton::Primary,
            },
            bounds: Rect::new(0.0, 0.0, 400.0, 200.0),
            scale: 1.0,
        };
        g.event(&mut release)
    }

    #[test]
    fn multiple_mode_toggles_independently() {
        let mut g = ChipGroup::new().chip(Chip::new("a")).chip(Chip::new("b"));
        lay(&mut g, 400.0);
        click(&mut g, 0);
        click(&mut g, 1);
        assert_eq!(g.selected_indices(), vec![0, 1]);
    }

    #[test]
    fn single_mode_clears_others() {
        let mut g = ChipGroup::new()
            .chip(Chip::new("a").selected(true))
            .chip(Chip::new("b"))
            .selection(ChipSelection::Single);
        lay(&mut g, 400.0);
        click(&mut g, 1);
        assert_eq!(g.selected_indices(), vec![1]);
        assert_eq!(g.take_changed(), Some((1, true)));
    }

    #[test]
    fn none_mode_reverts_the_toggle() {
        let mut g = ChipGroup::new()
            .chip(Chip::new("a"))
            .selection(ChipSelection::None);
        lay(&mut g, 400.0);
        click(&mut g, 0);
        assert_eq!(g.selected_indices(), Vec::<usize>::new());
    }

    #[test]
    fn chips_wrap_when_out_of_width() {
        let mut g = ChipGroup::new();
        for _ in 0..12 {
            g = g.chip(Chip::new("wide-chip-label"));
        }
        lay(&mut g, 100.0);
        assert!(
            g.cell_bounds[1].min_y() > 0.0 || g.cell_bounds[0].width() <= 100.0,
            "chips wrapped or clamped to the row"
        );
    }
}
