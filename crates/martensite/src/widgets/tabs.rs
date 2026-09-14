//! `Tabs` widget: an ARIA APG tab set.
//!
//! Implements the [APG tabs pattern](https://www.w3.org/WAI/ARIA/apg/patterns/tabpanel/):
//!
//! - A `Role::TabList` strip of `Role::Tab` children, each with
//!   `aria-selected` and `aria-controls` pointing at its
//!   `Role::TabPanel`.
//! - Roving tabindex: the tab list is one tab stop; arrow keys cycle
//!   the focus indicator, `Home`/`End` jump to the ends.
//! - [`TabActivation::Automatic`] selects on focus move;
//!   [`TabActivation::Manual`] requires `Enter`/`Space` to activate.
//! - Panel visibility follows the selection — only the selected
//!   `TabPanel` is visible; the rest are emitted hidden.
//! - `SemanticAction::Click` on a tab activates it.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{Tabs, Text};
//!
//! let tabs = Tabs::new()
//!     .tab("General", Text::new("general panel"))
//!     .tab("Advanced", Text::new("advanced panel"));
//! assert_eq!(tabs.selected(), 0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    A11yEmittedNode, EventContext, EventResponse, LayoutConstraints, LayoutContext, OverlayA11yRef,
    PaintContext, PointerButton, SemanticAction, Widget, WidgetEvent,
};
use martensite_core::Rect;

/// Tab strip height in logical pixels.
const STRIP_H: f32 = 32.0;
/// Tab label ink.
const INK: [u8; 4] = [30, 30, 36, 255];
/// Selected tab underline.
const ACCENT: [u8; 4] = [60, 110, 220, 255];
/// Focus ring colour.
const FOCUS_RING: [u8; 4] = [60, 110, 220, 128];
/// Tab face hover/selected background.
const TAB_BG: [u8; 4] = [240, 242, 246, 255];

/// Whether selecting a tab happens automatically on focus or manually
/// via `Enter`/`Space`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::TabActivation;
///
/// assert_eq!(TabActivation::default(), TabActivation::Automatic);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum TabActivation {
    /// Moving the focus indicator also selects the tab (default).
    #[default]
    Automatic,
    /// Moving only focuses; `Enter`/`Space` activates.
    Manual,
}

/// One tab inside the strip — an internal child emitted with
/// `Role::Tab`.
pub struct TabItem {
    /// The tab's accessible label.
    label: String,
    /// Whether this tab is selected (mirrored from the owner).
    selected: bool,
    /// Whether this tab carries the roving tabindex.
    focused: bool,
    /// 1-based position in the tab set.
    pos_in_set: usize,
    /// Total tab count.
    set_size: usize,
    /// Whether the owner is enabled.
    enabled: bool,
    /// Parked `SemanticAction::Click` for the owner to apply.
    activation_pending: bool,
    /// Parked `SemanticAction::Focus` for the owner to apply.
    focus_pending: bool,
}

impl TabItem {
    fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            selected: false,
            focused: false,
            pos_in_set: 0,
            set_size: 0,
            enabled: true,
            activation_pending: false,
            focus_pending: false,
        }
    }
}

impl Widget for TabItem {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let w = 24.0 + 8.0 * self.label.len() as f32;
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            STRIP_H.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Tab);
        node.set_label(self.label.as_str());
        node.set_selected(self.selected);
        node.set_position_in_set(self.pos_in_set);
        node.set_size_of_set(self.set_size);
        node.add_action(accesskit::Action::Click);
        // Roving tabindex: only the tab carrying the tab stop
        // advertises Focus.
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
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                self.activation_pending = true;
                EventResponse::Handled
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => {
                self.focus_pending = true;
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        if self.selected {
            cx.list.push_fill_rect(rect, TAB_BG);
            let underline = kurbo::Rect::new(rect.x0, rect.y1 - 2.0, rect.x1, rect.y1);
            cx.list.push_fill_rect(underline, ACCENT);
        }
        if self.focused {
            cx.list.push_stroke_rect(rect, 2.0, FOCUS_RING);
        }
        cx.list.push_text(
            kurbo::Point::new(
                f64::from(b.min_x() + 12.0),
                f64::from(b.min_y() + b.height() / 2.0 + 5.0),
            ),
            self.label.clone(),
            14.0,
            INK,
        );
    }
}

/// The tab strip — an internal child emitted with `Role::TabList`
/// containing the [`TabItem`] children.
struct TabStrip {
    /// The tabs.
    tabs: Vec<TabItem>,
    /// Tab bounds from the last layout pass.
    tab_bounds: Vec<Rect>,
}

impl TabStrip {
    fn new(labels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            tabs: labels.into_iter().map(TabItem::new).collect(),
            tab_bounds: Vec::new(),
        }
    }
}

impl Widget for TabStrip {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(0.0),
            STRIP_H.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let n = self.tabs.len();
        self.tab_bounds.clear();
        if n == 0 {
            return;
        }
        let w = bounds.width() / n as f32;
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            let rect = Rect::new(
                bounds.min_x() + i as f32 * w,
                bounds.min_y(),
                w,
                bounds.height(),
            );
            self.tab_bounds.push(rect);
            tab.layout(cx, rect);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::TabList);
        node.set_orientation(accesskit::Orientation::Horizontal);
    }

    fn child_count(&self) -> usize {
        self.tabs.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.tabs.get(index).map(|t| t as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.tabs.get_mut(index).map(|t| t as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.tab_bounds.get(index).copied()
    }
}

/// One tab panel — wraps user content so it is emitted as
/// `Role::TabPanel`.
struct TabPanelChild {
    /// The panel content.
    content: Box<dyn Widget>,
    /// Whether this panel is currently selected/visible.
    shown: bool,
    /// Content bounds from the last layout pass.
    bounds: Option<Rect>,
}

impl Widget for TabPanelChild {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        self.content.measure(cx, constraints)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = Some(bounds);
        self.content.layout(cx, bounds);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::TabPanel);
        if !self.shown {
            node.set_hidden();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.shown {
            return EventResponse::Ignored;
        }
        self.content.event(cx)
    }

    fn paint(&self, _cx: &mut PaintContext) {}

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, _index: usize) -> Option<&dyn Widget> {
        Some(&*self.content)
    }

    fn child_mut(&mut self, _index: usize) -> Option<&mut dyn Widget> {
        Some(&mut *self.content)
    }

    fn child_bounds(&self, _index: usize) -> Option<Rect> {
        self.bounds
    }
}

/// The panel stack — an internal child grouping the `Role::TabPanel`
/// children beneath the widget node, keeping panels out of the
/// `TabList` subtree.
struct PanelSet {
    /// Panels in tab order.
    panels: Vec<TabPanelChild>,
    /// Panel region bounds from the last layout pass.
    region: Option<Rect>,
}

impl Widget for PanelSet {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut size = Vec2::ZERO;
        for panel in &mut self.panels {
            let s = panel.measure(cx, constraints);
            size = size.max(s);
        }
        size
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.region = Some(bounds);
        for panel in &mut self.panels {
            panel.layout(cx, bounds);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
    }

    fn child_count(&self) -> usize {
        self.panels.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.panels.get(index).map(|p| p as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.panels.get_mut(index).map(|p| p as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        let panel = self.panels.get(index)?;
        if panel.shown {
            self.region
        } else {
            None
        }
    }
}

/// A tab set implementing the ARIA APG tabs contract.
///
/// Child protocol: `child(0)` is the `Role::TabList` strip (whose
/// children are the `Role::Tab`s), `child(1)` is the panel set (whose
/// children are the `Role::TabPanel`s), so the emitted accessibility
/// tree matches the APG structure exactly.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{TabActivation, Tabs, Text};
///
/// let mut tabs = Tabs::new()
///     .tab("One", Text::new("first"))
///     .tab("Two", Text::new("second"))
///     .activation(TabActivation::Manual);
/// tabs.activate(1);
/// assert_eq!(tabs.selected(), 1);
/// ```
pub struct Tabs {
    /// Optional accessible label.
    pub label: Option<String>,
    /// Whether the set accepts input.
    pub enabled: bool,
    /// Activation mode.
    pub activation: TabActivation,
    /// The tab strip (internal child 0).
    strip: TabStrip,
    /// The panels (internal child 1).
    panels: PanelSet,
    /// Selected tab index.
    selected: usize,
    /// Roving focus index.
    focused: usize,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Strip bounds from the last layout pass.
    strip_bounds: Option<Rect>,
    /// Panel region bounds from the last layout pass.
    panel_bounds: Option<Rect>,
}

impl Tabs {
    /// Creates an empty tab set; add tabs with [`tab`](Self::tab).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tabs;
    ///
    /// let tabs = Tabs::new();
    /// assert_eq!(tabs.tab_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            activation: TabActivation::Automatic,
            strip: TabStrip::new(std::iter::empty::<String>()),
            panels: PanelSet {
                panels: Vec::new(),
                region: None,
            },
            selected: 0,
            focused: 0,
            cached_bounds: Rect::default(),
            strip_bounds: None,
            panel_bounds: None,
        }
    }

    /// Creates a tab set from labels with empty panels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tabs;
    ///
    /// let tabs = Tabs::with_labels(["A", "B"]);
    /// assert_eq!(tabs.tab_count(), 2);
    /// ```
    pub fn with_labels(labels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut tabs = Self::new();
        for label in labels {
            tabs = tabs.tab(label, crate::widgets::text::Text::new(""));
        }
        tabs
    }

    /// Appends a tab with its panel content.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Tabs, Text};
    ///
    /// let tabs = Tabs::new().tab("Home", Text::new("home panel"));
    /// assert_eq!(tabs.tab_count(), 1);
    /// ```
    #[must_use]
    pub fn tab(mut self, label: impl Into<String>, panel: impl Widget + 'static) -> Self {
        self.strip.tabs.push(TabItem::new(label));
        self.panels.panels.push(TabPanelChild {
            content: Box::new(panel),
            shown: self.panels.panels.is_empty(),
            bounds: None,
        });
        self.sync_children();
        self
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tabs;
    ///
    /// let t = Tabs::new().label("Settings");
    /// assert_eq!(t.label.as_deref(), Some("Settings"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the activation mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TabActivation, Tabs};
    ///
    /// let t = Tabs::new().activation(TabActivation::Manual);
    /// assert_eq!(t.activation, TabActivation::Manual);
    /// ```
    #[inline]
    #[must_use]
    pub fn activation(mut self, activation: TabActivation) -> Self {
        self.activation = activation;
        self
    }

    /// Sets whether the set is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tabs;
    ///
    /// let t = Tabs::new().enabled(false);
    /// assert!(!t.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self.sync_children();
        self
    }

    /// Number of tabs.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tabs;
    ///
    /// assert_eq!(Tabs::with_labels(["A", "B", "C"]).tab_count(), 3);
    /// ```
    #[inline]
    pub fn tab_count(&self) -> usize {
        self.strip.tabs.len()
    }

    /// Selected tab index.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tabs;
    ///
    /// assert_eq!(Tabs::with_labels(["A", "B"]).selected(), 0);
    /// ```
    #[inline]
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Roving focus index.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tabs;
    ///
    /// assert_eq!(Tabs::with_labels(["A", "B"]).focused_index(), 0);
    /// ```
    #[inline]
    pub fn focused_index(&self) -> usize {
        self.focused
    }

    /// Activates `index`: selects it and moves the roving tabindex.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tabs;
    ///
    /// let mut t = Tabs::with_labels(["A", "B", "C"]);
    /// t.activate(2);
    /// assert_eq!(t.selected(), 2);
    /// ```
    pub fn activate(&mut self, index: usize) {
        if index < self.strip.tabs.len() {
            self.focused = index;
            self.selected = index;
            self.sync_children();
        }
    }

    /// Moves the roving tabindex by `delta` with wraparound, honouring
    /// the activation mode.
    pub fn move_focus(&mut self, delta: i64) {
        let n = self.strip.tabs.len() as i64;
        if n == 0 {
            return;
        }
        self.focused = ((self.focused as i64 + delta).rem_euclid(n)) as usize;
        if self.activation == TabActivation::Automatic {
            self.selected = self.focused;
        }
        self.sync_children();
    }

    /// Moves the roving tabindex to `index`, honouring the activation
    /// mode.
    pub fn focus_tab(&mut self, index: usize) {
        if index >= self.strip.tabs.len() {
            return;
        }
        self.focused = index;
        if self.activation == TabActivation::Automatic {
            self.selected = index;
        }
        self.sync_children();
    }

    /// Applies pending activations parked by tab children (AT actions
    /// delivered through `WidgetArena::internal_widget_mut`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Tabs;
    /// use martensite_core::widget::Widget;
    /// use martensite_core::{EventContext, EventResponse, Rect, SemanticAction, WidgetEvent};
    ///
    /// let mut t = Tabs::with_labels(["A", "B"]);
    /// {
    ///     // Tab children live under the strip at path [0, i].
    ///     let strip = t.child_mut(0).unwrap();
    ///     let tab = strip.child_mut(1).unwrap();
    ///     let ev = WidgetEvent::SemanticAction(SemanticAction::Click);
    ///     let mut cx = EventContext { event: &ev, bounds: Rect::default() };
    ///     assert_eq!(tab.event(&mut cx), EventResponse::Handled);
    /// }
    /// t.poll_pending();
    /// assert_eq!(t.selected(), 1);
    /// ```
    pub fn poll_pending(&mut self) {
        let mut activate = None;
        let mut focus = None;
        for (i, tab) in self.strip.tabs.iter_mut().enumerate() {
            if tab.activation_pending {
                tab.activation_pending = false;
                activate = Some(i);
            } else if tab.focus_pending {
                tab.focus_pending = false;
                focus = Some(i);
            }
        }
        if let Some(i) = activate {
            self.activate(i);
        } else if let Some(i) = focus {
            self.focus_tab(i);
        }
    }

    /// Mirrors owner state onto the strip/panel children.
    fn sync_children(&mut self) {
        let n = self.strip.tabs.len();
        for (i, tab) in self.strip.tabs.iter_mut().enumerate() {
            tab.selected = i == self.selected;
            tab.focused = i == self.focused;
            tab.pos_in_set = i + 1;
            tab.set_size = n;
            tab.enabled = self.enabled;
        }
        for (i, panel) in self.panels.panels.iter_mut().enumerate() {
            panel.shown = i == self.selected;
        }
    }
}

impl Default for Tabs {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Tabs {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let panels = self.panels.measure(cx, constraints);
        Vec2::new(
            panels.x.clamp(80.0, constraints.max_size.x.max(0.0)),
            (panels.y + STRIP_H).clamp(80.0, constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        let strip = Rect::new(
            bounds.min_x(),
            bounds.min_y(),
            bounds.width(),
            STRIP_H.min(bounds.height()),
        );
        let panels = Rect::new(
            bounds.min_x(),
            bounds.min_y() + strip.height(),
            bounds.width(),
            (bounds.height() - strip.height()).max(0.0),
        );
        self.strip_bounds = Some(strip);
        self.panel_bounds = Some(panels);
        self.strip.layout(cx, strip);
        self.panels.layout(cx, panels);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
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

    fn a11y_fixup(
        &self,
        emitted: &mut Vec<A11yEmittedNode>,
        _overlay_nodes: &[OverlayA11yRef],
        _this_node: &mut AccessKitNode,
    ) {
        // Wire aria-controls: tab at path [0, i] controls panel at
        // path [1, i]; panel is labelled by its tab.
        let n = self.strip.tabs.len();
        for i in 0..n {
            let tab_path = [0, i as u32];
            let panel_path = [1, i as u32];
            let panel_id = emitted
                .iter()
                .find(|e| e.path.as_slice() == panel_path)
                .map(|e| e.id);
            let tab_id = emitted
                .iter()
                .find(|e| e.path.as_slice() == tab_path)
                .map(|e| e.id);
            if let Some(panel_id) = panel_id {
                if let Some(tab) = emitted.iter_mut().find(|e| e.path.as_slice() == tab_path) {
                    tab.node.set_controls(vec![panel_id]);
                }
            }
            if let Some(tab_id) = tab_id {
                if let Some(panel) = emitted.iter_mut().find(|e| e.path.as_slice() == panel_path) {
                    panel.node.set_labelled_by(vec![tab_id]);
                }
            }
        }
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
                if let Some(strip) = self.strip_bounds.filter(|s| s.contains(*position)) {
                    let _ = strip;
                    for (i, tab_bounds) in self.strip.tab_bounds.iter().enumerate() {
                        if tab_bounds.contains(*position) {
                            self.activate(i);
                            return EventResponse::CaptureFocus;
                        }
                    }
                    return EventResponse::Handled;
                }
                // Inside the panel region: forward to the visible panel.
                if let Some(region) = self.panel_bounds.filter(|r| r.contains(*position)) {
                    if let Some(panel) = self.panels.panels.get_mut(self.selected) {
                        let mut panel_cx = EventContext {
                            event: cx.event,
                            bounds: region,
                        };
                        return panel.event(&mut panel_cx);
                    }
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowRight" => {
                    self.move_focus(1);
                    EventResponse::RequestRepaint
                }
                "ArrowLeft" => {
                    self.move_focus(-1);
                    EventResponse::RequestRepaint
                }
                "Home" => {
                    self.focus_tab(0);
                    EventResponse::RequestRepaint
                }
                "End" => {
                    let last = self.strip.tabs.len().saturating_sub(1);
                    self.focus_tab(last);
                    EventResponse::RequestRepaint
                }
                "Enter" | " " | "Space" => {
                    self.activate(self.focused);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                self.activate(self.focused);
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn child_count(&self) -> usize {
        2
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        match index {
            0 => Some(&self.strip),
            1 => Some(&self.panels),
            _ => None,
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        match index {
            0 => Some(&mut self.strip),
            1 => Some(&mut self.panels),
            _ => None,
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        match index {
            0 => self.strip_bounds,
            1 => self.panel_bounds,
            _ => None,
        }
    }
}

impl std::fmt::Debug for Tabs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tabs")
            .field("tabs", &self.strip.tabs.len())
            .field("selected", &self.selected)
            .field("focused", &self.focused)
            .field("activation", &self.activation)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(tabs: &mut Tabs, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext { hot: &mut hot };
        tabs.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn event(t: &mut Tabs, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: t.cached_bounds,
        };
        t.event(&mut cx)
    }

    #[test]
    fn automatic_activation_selects_on_move() {
        let mut t = Tabs::with_labels(["A", "B", "C"]);
        laid_out(&mut t, 300.0, 200.0);
        event(&mut t, &key("ArrowRight"));
        assert_eq!(t.focused_index(), 1);
        assert_eq!(t.selected(), 1);
        // Wraparound.
        event(&mut t, &key("ArrowRight"));
        event(&mut t, &key("ArrowRight"));
        assert_eq!(t.selected(), 0);
        event(&mut t, &key("ArrowLeft"));
        assert_eq!(t.selected(), 2);
    }

    #[test]
    fn manual_activation_requires_enter() {
        let mut t = Tabs::with_labels(["A", "B", "C"]).activation(TabActivation::Manual);
        laid_out(&mut t, 300.0, 200.0);
        event(&mut t, &key("ArrowRight"));
        assert_eq!(t.focused_index(), 1);
        assert_eq!(t.selected(), 0); // moved focus only
        event(&mut t, &key("Enter"));
        assert_eq!(t.selected(), 1);
    }

    #[test]
    fn home_end_jump() {
        let mut t = Tabs::with_labels(["A", "B", "C"]);
        laid_out(&mut t, 300.0, 200.0);
        event(&mut t, &key("End"));
        assert_eq!(t.focused_index(), 2);
        event(&mut t, &key("Home"));
        assert_eq!(t.focused_index(), 0);
    }

    #[test]
    fn roving_tabindex_single_focus_action() {
        let mut t = Tabs::with_labels(["A", "B", "C"]);
        t.focus_tab(1);
        let focus: Vec<bool> = (0..3)
            .map(|i| {
                let mut n = AccessKitNode::new(accesskit::Role::Unknown);
                t.strip.child(i).unwrap().accessibility(&mut n);
                n.supports_action(accesskit::Action::Focus)
            })
            .collect();
        assert_eq!(focus, vec![false, true, false]);
    }

    #[test]
    fn panel_visibility_follows_selection() {
        let mut t = Tabs::with_labels(["A", "B"]);
        t.activate(1);
        let hidden: Vec<bool> = (0..2)
            .map(|i| {
                let mut n = AccessKitNode::new(accesskit::Role::Unknown);
                t.panels.child(i).unwrap().accessibility(&mut n);
                n.is_hidden()
            })
            .collect();
        assert_eq!(hidden, vec![true, false]);
    }

    #[test]
    fn click_activates_tab() {
        let mut t = Tabs::with_labels(["A", "B", "C"]);
        laid_out(&mut t, 300.0, 200.0);
        // Third tab occupies the last third of the strip.
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(250.0, 16.0),
            button: PointerButton::Primary,
        };
        assert_eq!(event(&mut t, &press), EventResponse::CaptureFocus);
        assert_eq!(t.selected(), 2);
    }

    #[test]
    fn accessibility_roles() {
        let t = Tabs::with_labels(["A", "B"]);
        let mut strip_node = AccessKitNode::new(accesskit::Role::Unknown);
        t.child(0).unwrap().accessibility(&mut strip_node);
        assert_eq!(strip_node.role(), accesskit::Role::TabList);

        let mut tab_node = AccessKitNode::new(accesskit::Role::Unknown);
        t.child(0)
            .unwrap()
            .child(0)
            .unwrap()
            .accessibility(&mut tab_node);
        assert_eq!(tab_node.role(), accesskit::Role::Tab);
        assert_eq!(tab_node.is_selected(), Some(true));

        let mut panel_node = AccessKitNode::new(accesskit::Role::Unknown);
        t.child(1)
            .unwrap()
            .child(1)
            .unwrap()
            .accessibility(&mut panel_node);
        assert_eq!(panel_node.role(), accesskit::Role::TabPanel);
        assert!(panel_node.is_hidden());
    }

    #[test]
    fn pending_activation_from_tab_child() {
        let mut t = Tabs::with_labels(["A", "B", "C"]);
        let ev = WidgetEvent::SemanticAction(SemanticAction::Click);
        let tab = t.child_mut(0).unwrap().child_mut(2).unwrap();
        let mut cx = EventContext {
            event: &ev,
            bounds: Rect::default(),
        };
        assert_eq!(tab.event(&mut cx), EventResponse::Handled);
        t.poll_pending();
        assert_eq!(t.selected(), 2);
    }
}
