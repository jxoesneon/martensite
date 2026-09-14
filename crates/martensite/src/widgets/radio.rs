//! `RadioGroup` widget: an ARIA APG radio group.
//!
//! Implements the [APG radio group pattern](https://www.w3.org/WAI/ARIA/apg/patterns/radio/):
//!
//! - The group emits `Role::RadioGroup`; each option emits
//!   `Role::RadioButton` with `Toggled` checked state.
//! - **Roving tabindex**: the group is a single tab stop — the selected
//!   (or, before first selection, the focused) option carries the
//!   `Focus` action.
//! - Arrow keys move the focus indicator *and* select the newly
//!   focused option; `Space` selects the focused option.
//! - The single-checked invariant is enforced by `select` — exactly
//!   one option is checked at a time.
//! - `SemanticAction::Click` on an option selects it.
//!
//! # Accessibility note: roving tabindex and virtual nodes
//!
//! Each option is an internal child emitted as a virtual
//! `RadioButton` node; the option carrying the roving tabindex
//! advertises `Action::Focus` so assistive technologies can direct
//! focus. Platform focus itself is a single stop on the **group's
//! arena node** — `TreeUpdate.focus` names the group, and an AT
//! `Focus` request on an option moves the group's roving index rather
//! than producing a separate focus target.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::RadioGroup;
//!
//! let group = RadioGroup::new(["Small", "Medium", "Large"]);
//! assert_eq!(group.selected(), 0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use kurbo::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect};

use crate::widgets::FlexDirection;

/// Option row height in logical pixels.
const ROW_H: f32 = 24.0;
/// Radio circle diameter.
const DOT: f32 = 14.0;
/// Gap between the circle and the label.
const LABEL_GAP: f32 = 8.0;
/// Circle edge colour.
const EDGE: [u8; 4] = [120, 125, 135, 255];
/// Checked dot colour.
const CHECKED: [u8; 4] = [60, 110, 220, 255];
/// Label ink colour.
const INK: [u8; 4] = [20, 20, 25, 255];
/// Focus ring colour.
const FOCUS_RING: [u8; 4] = [60, 110, 220, 128];

/// One option inside a [`RadioGroup`].
///
/// Emitted as an internal child with `Role::RadioButton`; its checked
/// and focused flags mirror group state via
/// [`RadioGroup::poll_pending`].
pub struct RadioOption {
    /// The option's accessible label.
    label: String,
    /// Whether this option is checked (mirrored from the group).
    checked: bool,
    /// Whether this option carries the roving tabindex.
    focused: bool,
    /// A `SemanticAction::Click` or pointer press received but not yet
    /// applied by the owning group (see [`RadioGroup::poll_pending`]).
    activation_pending: bool,
    /// A `SemanticAction::Focus` received but not yet applied by the
    /// owning group — moves the roving tabindex without selecting.
    focus_pending: bool,
    /// Whether the group is enabled.
    enabled: bool,
}

impl RadioOption {
    fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            checked: false,
            focused: false,
            activation_pending: false,
            focus_pending: false,
            enabled: true,
        }
    }
}

impl Widget for RadioOption {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Approximate label width — real text shaping lives in the
        // `martensite-text` pipeline; this is a coarse per-grapheme
        // estimate sufficient for row layout.
        let w = DOT + LABEL_GAP + 8.0 * self.label.chars().count() as f32;
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            ROW_H.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::RadioButton);
        node.set_label(self.label.as_str());
        node.set_toggled(accesskit::Toggled::from(self.checked));
        node.add_action(accesskit::Action::Click);
        // Roving tabindex: only the option holding the tab stop
        // advertises Focus — the group is a single tab stop.
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
            } => {
                // Park the activation; the owning group applies it via
                // `poll_pending` so the single-checked invariant stays
                // centralized.
                self.activation_pending = true;
                EventResponse::CaptureFocus
            }
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                self.activation_pending = true;
                EventResponse::Handled
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => {
                // Move the group's roving tabindex here and request
                // platform focus on the owning group node.
                self.focus_pending = true;
                EventResponse::CaptureFocus
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let cy = f64::from(b.min_y() + b.height() / 2.0);
        let cxm = f64::from(b.min_x() + DOT / 2.0);
        let circle = kurbo::Circle::new(kurbo::Point::new(cxm, cy), f64::from(DOT / 2.0));
        cx.list.push_stroke_path(circle.to_path(0.1), 1.5, EDGE);
        if self.focused {
            let ring = kurbo::Circle::new(kurbo::Point::new(cxm, cy), f64::from(DOT / 2.0 + 3.0));
            cx.list.push_stroke_path(ring.to_path(0.1), 2.0, FOCUS_RING);
        }
        if self.checked {
            let dot = kurbo::Circle::new(kurbo::Point::new(cxm, cy), f64::from(DOT / 2.0 - 4.0));
            cx.list.push_path(dot.to_path(0.1), CHECKED);
        }
        cx.list.push_text(
            kurbo::Point::new(f64::from(b.min_x() + DOT + LABEL_GAP), cy + 5.0),
            self.label.clone(),
            14.0,
            INK,
        );
    }
}

/// A radio group widget implementing the ARIA APG radio contract.
///
/// The group is a single tab stop with a roving tabindex over its
/// options. Exactly one option is checked at all times; [`select`]
/// enforces the invariant.
///
/// [`select`]: RadioGroup::select
///
/// # Examples
///
/// ```
/// use martensite::widgets::RadioGroup;
///
/// let mut group = RadioGroup::new(["S", "M", "L"]);
/// group.select(2);
/// assert_eq!(group.selected(), 2);
/// ```
pub struct RadioGroup {
    /// Optional accessible label for the group.
    pub label: Option<String>,
    /// Whether the group accepts input.
    pub enabled: bool,
    /// Layout direction for the option list.
    pub direction: FlexDirection,
    /// The options.
    options: Vec<RadioOption>,
    /// Index of the checked option.
    selected: usize,
    /// Index of the option holding the roving tabindex.
    focused: usize,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Option bounds from the last layout pass.
    option_bounds: Vec<Rect>,
}

impl RadioGroup {
    /// Creates a group from option labels; the first option is
    /// selected and focused. An empty group is tolerated — selection
    /// operations become no-ops until options exist (consistent with
    /// `Dropdown::new`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RadioGroup;
    ///
    /// let g = RadioGroup::new(["A", "B"]);
    /// assert_eq!(g.selected(), 0);
    /// assert_eq!(g.focused_index(), 0);
    /// ```
    pub fn new(labels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let options: Vec<RadioOption> = labels.into_iter().map(RadioOption::new).collect();
        let mut group = Self {
            label: None,
            enabled: true,
            direction: FlexDirection::Column,
            options,
            selected: 0,
            focused: 0,
            cached_bounds: Rect::default(),
            option_bounds: Vec::new(),
        };
        group.sync_options();
        group
    }

    /// Sets the group's accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RadioGroup;
    ///
    /// let g = RadioGroup::new(["S", "M"]).label("Size");
    /// assert_eq!(g.label.as_deref(), Some("Size"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the option layout direction.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{FlexDirection, RadioGroup};
    ///
    /// let g = RadioGroup::new(["S", "M"]).direction(FlexDirection::Row);
    /// assert_eq!(g.direction, FlexDirection::Row);
    /// ```
    #[inline]
    #[must_use]
    pub fn direction(mut self, direction: FlexDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Sets whether the group is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RadioGroup;
    ///
    /// let g = RadioGroup::new(["S"]).enabled(false);
    /// assert!(!g.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self.sync_options();
        self
    }

    /// The index of the checked option.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RadioGroup;
    ///
    /// let g = RadioGroup::new(["A", "B"]);
    /// assert_eq!(g.selected(), 0);
    /// ```
    #[inline]
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The index of the option carrying the roving tabindex.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RadioGroup;
    ///
    /// let g = RadioGroup::new(["A", "B"]);
    /// assert_eq!(g.focused_index(), 0);
    /// ```
    #[inline]
    pub fn focused_index(&self) -> usize {
        self.focused
    }

    /// Selects `index` — moves the checked state and, per APG arrow-key
    /// behaviour, the focus indicator with it. Out-of-range indices are
    /// ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RadioGroup;
    ///
    /// let mut g = RadioGroup::new(["A", "B", "C"]);
    /// g.select(1);
    /// assert_eq!(g.selected(), 1);
    /// g.select(9);
    /// assert_eq!(g.selected(), 1); // out of range ignored
    /// ```
    pub fn select(&mut self, index: usize) {
        if index < self.options.len() {
            self.selected = index;
            self.focused = index;
            self.sync_options();
        }
    }

    /// Moves the roving tabindex by `delta` positions with wraparound
    /// and selects the newly focused option (APG arrow-key semantics).
    fn move_focus(&mut self, delta: i64) {
        let n = self.options.len() as i64;
        if n == 0 {
            return;
        }
        self.focused = ((self.focused as i64 + delta).rem_euclid(n)) as usize;
        self.selected = self.focused;
        self.sync_options();
    }

    /// Applies pending activations and focus moves recorded by option
    /// children.
    ///
    /// AT actions dispatched to internal option widgets (through
    /// `WidgetArena::internal_widget_mut`) can only mark the option —
    /// this pulls those marks into group state. Called automatically
    /// from `event`, `layout`, and `a11y_prepare`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::RadioGroup;
    /// use martensite_core::widget::Widget;
    ///
    /// let mut g = RadioGroup::new(["A", "B"]);
    /// // Simulate an AT Click on option 1 through the child protocol.
    /// {
    ///     let option = g.child_mut(1).unwrap();
    ///     let event = martensite_core::WidgetEvent::SemanticAction(
    ///         martensite_core::SemanticAction::Click,
    ///     );
    ///     let mut cx = martensite_core::EventContext {
    ///         event: &event,
    ///         bounds: martensite_core::Rect::default(),
    ///     };
    ///     option.event(&mut cx);
    /// }
    /// g.poll_pending();
    /// assert_eq!(g.selected(), 1);
    /// ```
    pub fn poll_pending(&mut self) {
        let mut activate = None;
        let mut focus = None;
        for (i, option) in self.options.iter_mut().enumerate() {
            if option.activation_pending {
                option.activation_pending = false;
                activate = Some(i);
            }
            if option.focus_pending {
                option.focus_pending = false;
                focus = Some(i);
            }
        }
        if let Some(index) = activate {
            self.select(index);
        } else if let Some(index) = focus {
            // Focus without selection (activation also moves focus).
            self.focused = index;
            self.sync_options();
        }
    }

    /// Mirrors group state onto the option children so their emitted
    /// `RadioButton` nodes are accurate.
    fn sync_options(&mut self) {
        for (i, option) in self.options.iter_mut().enumerate() {
            option.checked = i == self.selected;
            option.focused = i == self.focused;
            option.enabled = self.enabled;
        }
    }
}

impl Widget for RadioGroup {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut total = Vec2::ZERO;
        for (i, option) in self.options.iter_mut().enumerate() {
            let size = option.measure(cx, constraints);
            if self.direction.is_row() {
                total.x += size.x + if i > 0 { 16.0 } else { 0.0 };
                total.y = total.y.max(size.y);
            } else {
                total.y += size.y;
                total.x = total.x.max(size.x);
            }
        }
        total
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        // Declare keyboard focusability on the arena node.
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
        self.option_bounds.clear();
        let n = self.options.len();
        let row_gap = 16.0f32;
        for (i, option) in self.options.iter_mut().enumerate() {
            let option_bounds = if self.direction.is_row() {
                let w = if n > 0 {
                    (bounds.width() - row_gap * n.saturating_sub(1) as f32) / n as f32
                } else {
                    0.0
                };
                Rect::new(
                    bounds.min_x() + i as f32 * (w + row_gap),
                    bounds.min_y(),
                    w.max(0.0),
                    bounds.height(),
                )
            } else {
                let h = if n > 0 {
                    bounds.height() / n as f32
                } else {
                    0.0
                };
                Rect::new(
                    bounds.min_x(),
                    bounds.min_y() + i as f32 * h,
                    bounds.width(),
                    h.max(0.0),
                )
            };
            self.option_bounds.push(option_bounds);
            option.layout(cx, option_bounds);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::RadioGroup);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
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
            } => {
                // Forward to the option under the press via the child
                // protocol; the option parks an activation, applied
                // below so selection stays centralized.
                let mut response = EventResponse::Ignored;
                for i in (0..self.options.len()).rev() {
                    let Some(b) = self.child_bounds(i) else {
                        continue;
                    };
                    if !b.contains(*position) {
                        continue;
                    }
                    let mut child_cx = EventContext {
                        event: cx.event,
                        bounds: b,
                    };
                    if let Some(option) = self.options.get_mut(i) {
                        response = option.event(&mut child_cx);
                    }
                    break;
                }
                self.poll_pending();
                response
            }
            WidgetEvent::KeyPressed { key, .. } => {
                // APG: all four arrows cycle the group in either layout.
                let forward = key == "ArrowDown" || key == "ArrowRight";
                let backward = key == "ArrowUp" || key == "ArrowLeft";
                if forward {
                    self.move_focus(1);
                    EventResponse::RequestRepaint
                } else if backward {
                    self.move_focus(-1);
                    EventResponse::RequestRepaint
                } else if key == " " || key == "Space" {
                    self.select(self.focused);
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            // A Click on the group selects the roving option.
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                self.select(self.focused);
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => EventResponse::CaptureFocus,
            _ => EventResponse::Ignored,
        }
    }

    fn child_count(&self) -> usize {
        self.options.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.options.get(index).map(|o| o as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.options.get_mut(index).map(|o| o as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.option_bounds.get(index).copied()
    }
}

impl std::fmt::Debug for RadioGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RadioGroup")
            .field("options", &self.options.len())
            .field("selected", &self.selected)
            .field("focused", &self.focused)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(group: &mut RadioGroup, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext { hot: &mut hot };
        group.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn event(group: &mut RadioGroup, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: group.cached_bounds,
        };
        group.event(&mut cx)
    }

    #[test]
    fn single_checked_invariant() {
        let mut g = RadioGroup::new(["A", "B", "C"]);
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        for i in 0..3 {
            g.select(i);
            let checked: Vec<bool> = (0..3)
                .map(|j| {
                    let mut n = AccessKitNode::new(accesskit::Role::Unknown);
                    g.child(j).unwrap().accessibility(&mut n);
                    n.toggled() == Some(accesskit::Toggled::True)
                })
                .collect();
            assert_eq!(checked.iter().filter(|c| **c).count(), 1);
            assert!(checked[i]);
        }
        let _ = &mut node;
    }

    #[test]
    fn arrows_move_and_select() {
        let mut g = RadioGroup::new(["A", "B", "C"]);
        laid_out(&mut g, 200.0, 72.0);
        event(&mut g, &key("ArrowDown"));
        assert_eq!(g.focused_index(), 1);
        assert_eq!(g.selected(), 1);
        event(&mut g, &key("ArrowDown"));
        assert_eq!(g.selected(), 2);
        // Wraparound.
        event(&mut g, &key("ArrowDown"));
        assert_eq!(g.selected(), 0);
        event(&mut g, &key("ArrowUp"));
        assert_eq!(g.selected(), 2);
    }

    #[test]
    fn space_selects_focused() {
        let mut g = RadioGroup::new(["A", "B", "C"]);
        laid_out(&mut g, 200.0, 72.0);
        g.focused = 1;
        g.sync_options();
        event(&mut g, &key("Space"));
        assert_eq!(g.selected(), 1);
    }

    #[test]
    fn roving_tabindex_single_focus_action() {
        let g = RadioGroup::new(["A", "B", "C"]);
        let focus_actions: Vec<bool> = (0..3)
            .map(|i| {
                let mut n = AccessKitNode::new(accesskit::Role::Unknown);
                g.child(i).unwrap().accessibility(&mut n);
                n.supports_action(accesskit::Action::Focus)
            })
            .collect();
        // Exactly the roved option carries the Focus action.
        assert_eq!(focus_actions, vec![true, false, false]);
    }

    #[test]
    fn click_selects_option() {
        let mut g = RadioGroup::new(["A", "B", "C"]);
        laid_out(&mut g, 200.0, 72.0);
        // Option rows are 24px tall; press in the third row.
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(10.0, 60.0),
            button: PointerButton::Primary,
        };
        assert_eq!(event(&mut g, &press), EventResponse::CaptureFocus);
        assert_eq!(g.selected(), 2);
    }

    #[test]
    fn pending_activation_from_child_click() {
        let mut g = RadioGroup::new(["A", "B", "C"]);
        laid_out(&mut g, 200.0, 72.0);
        let ev = WidgetEvent::SemanticAction(SemanticAction::Click);
        let option = g.child_mut(2).unwrap();
        let mut cx = EventContext {
            event: &ev,
            bounds: Rect::default(),
        };
        assert_eq!(option.event(&mut cx), EventResponse::Handled);
        g.poll_pending();
        assert_eq!(g.selected(), 2);
    }

    #[test]
    fn group_accessibility_role() {
        let g = RadioGroup::new(["A"]).label("Choice");
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        g.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::RadioGroup);
        assert_eq!(node.label(), Some("Choice"));
    }
}
