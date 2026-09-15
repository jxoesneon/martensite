//! `Dropdown` widget: an ARIA APG select-only combobox.
//!
//! Implements the [APG select-only combobox pattern](https://www.w3.org/WAI/ARIA/apg/patterns/combobox/examples/combobox-select-only/):
//!
//! - The widget emits `Role::ComboBox` with `aria-haspopup="listbox"`,
//!   `aria-expanded`, the selected option's text as its value, and
//!   `aria-activedescendant` + `aria-controls` wired to the popup
//!   (via [`Widget::a11y_fixup`](martensite_core::Widget::a11y_fixup) against overlay node ids).
//! - The popup is a `Role::ListBox` of `Role::ListBoxOption` children
//!   living in the [`OverlayLayer`](martensite_core::overlay::OverlayLayer),
//!   placed below the combobox (flipping above near the bottom edge)
//!   and clamped to the window.
//! - **Closed**: `Enter`/`Space`/`ArrowDown`/`ArrowUp` opens the list;
//!   printable characters typeahead-select an option.
//! - **Open**: arrows move the highlight (`Home`/`End` jump), `Enter`
//!   commits, `Escape` or an outside press dismisses without
//!   committing — the overlay handles both and the widget reconciles
//!   in [`Dropdown::sync_overlay`].
//! - Popup options share their state with the owner through an
//!   `Arc<Mutex<_>>`, so hover/click/commit inside the popup reaches
//!   the combobox without the adapter preparing popup widgets.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::Dropdown;
//!
//! let dd = Dropdown::new(["Small", "Medium", "Large"]);
//! assert_eq!(dd.selected(), 0);
//! assert!(!dd.is_open());
//! ```

use std::sync::{Arc, Mutex};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
use martensite_core::widget::{
    A11yEmittedNode, EventContext, EventResponse, LayoutConstraints, LayoutContext, OverlayA11yRef,
    PaintContext, PointerButton, SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect};

use crate::widgets::scrollview::ScrollView;

/// Option row height in the popup, in logical pixels.
const ROW_H: f32 = 28.0;
/// Maximum popup rows before the list scrolls.
const MAX_VISIBLE_ROWS: f32 = 10.0;
/// Combobox face height.
const FACE_H: f32 = 32.0;
/// Combobox face chrome colours.
const FACE_BG: [u8; 4] = [250, 250, 252, 255];
/// Face border.
const FACE_BORDER: [u8; 4] = [150, 155, 165, 255];
/// Label ink.
const INK: [u8; 4] = [30, 30, 36, 255];
/// Disabled ink.
const INK_DISABLED: [u8; 4] = [150, 150, 158, 255];
/// Popup background.
const POPUP_BG: [u8; 4] = [252, 252, 254, 255];
/// Popup border.
const POPUP_BORDER: [u8; 4] = [140, 145, 155, 255];
/// Highlighted option background.
const HIGHLIGHT_BG: [u8; 4] = [60, 110, 220, 255];
/// Highlighted option ink.
const HIGHLIGHT_INK: [u8; 4] = [255, 255, 255, 255];
/// Selected-option check colour.
const CHECK: [u8; 4] = [60, 110, 220, 255];

/// State shared between a [`Dropdown`] and its popup widgets.
///
/// The popup content reads `options`/`highlighted`/`selected` to paint
/// and emit accessibility; option hover and commit write back so the
/// owner can apply them on the next `sync_overlay`.
#[derive(Debug)]
struct PopupState {
    /// Option labels.
    options: Vec<String>,
    /// Index with the visual highlight (aria-activedescendant target).
    highlighted: usize,
    /// Committed index.
    selected: usize,
    /// Set by a popup option when it is activated (click or AT Click).
    committed: Option<usize>,
}

/// One option inside the popup listbox — a stateless view over the
/// shared [`PopupState`], emitted as `Role::ListBoxOption`.
struct OptionItem {
    /// Index into `shared.options`.
    index: usize,
    /// Shared state with the owning combobox.
    shared: Arc<Mutex<PopupState>>,
}

impl Widget for OptionItem {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(0.0),
            ROW_H.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListBoxOption);
        let state = self.shared.lock().expect("popup state poisoned");
        if let Some(label) = state.options.get(self.index) {
            node.set_label(label.as_str());
        }
        node.set_selected(self.index == state.selected);
        node.set_position_in_set(self.index + 1);
        node.set_size_of_set(state.options.len());
        node.add_action(accesskit::Action::Click);
        // No `Action::Focus`: options are not focusable — the combobox
        // owns focus and tracks the highlight via
        // `aria-activedescendant` (the select-only APG pattern).
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { .. } | WidgetEvent::PointerEnter => {
                let mut state = self.shared.lock().expect("popup state poisoned");
                if state.highlighted != self.index {
                    state.highlighted = self.index;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                ..
            } => {
                self.shared.lock().expect("popup state poisoned").committed = Some(self.index);
                EventResponse::Handled
            }
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                self.shared.lock().expect("popup state poisoned").committed = Some(self.index);
                EventResponse::Handled
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => {
                self.shared
                    .lock()
                    .expect("popup state poisoned")
                    .highlighted = self.index;
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let state = self.shared.lock().expect("popup state poisoned");
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let highlighted = self.index == state.highlighted;
        if highlighted {
            cx.list.push_fill_rect(rect, HIGHLIGHT_BG);
        }
        if self.index == state.selected && !highlighted {
            // Selected but not highlighted: draw a check glyph.
            cx.list.push_text(
                kurbo::Point::new(f64::from(b.min_x() + 6.0), f64::from(b.min_y() + 19.0)),
                "✓".to_string(),
                14.0,
                CHECK,
            );
        }
        let ink = if highlighted { HIGHLIGHT_INK } else { INK };
        if let Some(label) = state.options.get(self.index) {
            cx.list.push_text(
                kurbo::Point::new(
                    f64::from(b.min_x() + 24.0),
                    f64::from(b.min_y() + b.height() / 2.0 + 5.0),
                ),
                label.clone(),
                14.0,
                ink,
            );
        }
    }
}

/// Vertical column of popup options — the scroll view's content.
struct OptionColumn {
    /// Options in order.
    items: Vec<OptionItem>,
    /// Row bounds from the last layout pass.
    row_bounds: Vec<Rect>,
}

impl Widget for OptionColumn {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(0.0),
            (self.items.len() as f32 * ROW_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.row_bounds.clear();
        for (i, item) in self.items.iter_mut().enumerate() {
            let rect = Rect::new(
                bounds.min_x(),
                bounds.min_y() + i as f32 * ROW_H,
                bounds.width(),
                ROW_H,
            );
            self.row_bounds.push(rect);
            item.layout(cx, rect);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
    }

    fn child_count(&self) -> usize {
        self.items.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.items.get(index).map(|i| i as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.items.get_mut(index).map(|i| i as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.row_bounds.get(index).copied()
    }
}

/// The popup surface — a `Role::ListBox` wrapping a [`ScrollView`] of
/// [`OptionItem`]s. Opened in the overlay by [`Dropdown::sync_overlay`].
struct ListBoxPopup {
    /// Scrolling option list (internal child 0).
    scroll: ScrollView,
    /// Shared state with the owning combobox.
    shared: Arc<Mutex<PopupState>>,
    /// Popup bounds from the last layout pass (the scroll view fills
    /// the whole popup inside its 1px border).
    bounds: Option<Rect>,
}

impl ListBoxPopup {
    fn new(shared: Arc<Mutex<PopupState>>) -> Self {
        let items = {
            let state = shared.lock().expect("popup state poisoned");
            (0..state.options.len())
                .map(|index| OptionItem {
                    index,
                    shared: Arc::clone(&shared),
                })
                .collect()
        };
        Self {
            scroll: ScrollView::new(OptionColumn {
                items,
                row_bounds: Vec::new(),
            }),
            shared,
            bounds: None,
        }
    }
}

impl Widget for ListBoxPopup {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut s = self.scroll.measure(cx, constraints);
        s.y = s.y.min(MAX_VISIBLE_ROWS * ROW_H + 2.0);
        s.x = s.x.max(120.0).min(constraints.max_size.x.max(0.0));
        s
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = Some(bounds);
        self.scroll.layout(cx, bounds);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListBox);
        node.set_orientation(accesskit::Orientation::Vertical);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Keyboard on the popup itself: move the highlight / commit.
        if let WidgetEvent::KeyPressed { key, .. } = cx.event {
            let mut state = self.shared.lock().expect("popup state poisoned");
            let n = state.options.len();
            if n > 0 {
                match key.as_str() {
                    "ArrowDown" => {
                        state.highlighted = (state.highlighted + 1).min(n - 1);
                        return EventResponse::RequestRepaint;
                    }
                    "ArrowUp" => {
                        state.highlighted = state.highlighted.saturating_sub(1);
                        return EventResponse::RequestRepaint;
                    }
                    "Home" => {
                        state.highlighted = 0;
                        return EventResponse::RequestRepaint;
                    }
                    "End" => {
                        state.highlighted = n - 1;
                        return EventResponse::RequestRepaint;
                    }
                    "Enter" | " " | "Space" => {
                        state.committed = Some(state.highlighted);
                        return EventResponse::Handled;
                    }
                    _ => {}
                }
            }
            drop(state);
        }
        // Forward to the scroll view / options.
        self.scroll.event(cx)
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        cx.list.push_fill_rect(rect, POPUP_BG);
        cx.list.push_stroke_rect(rect, 1.0, POPUP_BORDER);
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, _index: usize) -> Option<&dyn Widget> {
        Some(&self.scroll)
    }

    fn child_mut(&mut self, _index: usize) -> Option<&mut dyn Widget> {
        Some(&mut self.scroll)
    }

    fn child_bounds(&self, _index: usize) -> Option<Rect> {
        // The scroll view fills the whole popup inside its 1px border.
        self.bounds
    }
}

/// A select-only dropdown combobox implementing the ARIA APG contract.
///
/// Owns no popup widget itself — [`sync_overlay`](Self::sync_overlay)
/// reconciles an overlay entry each frame so the popup paints above
/// window content and is emitted into the accessibility tree.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Dropdown;
///
/// let mut dd = Dropdown::new(["One", "Two"]).placeholder("Pick…");
/// dd.open();
/// assert!(dd.is_open());
/// dd.commit(1);
/// assert_eq!(dd.selected(), 1);
/// ```
pub struct Dropdown {
    /// Optional accessible label for the combobox.
    pub label: Option<String>,
    /// Text shown when no option is selected.
    pub placeholder: String,
    /// Whether the combobox accepts input.
    pub enabled: bool,
    /// Option labels.
    options: Vec<String>,
    /// Committed index.
    selected: usize,
    /// Whether the popup is logically open.
    open: bool,
    /// Highlighted option (aria-activedescendant target).
    highlighted: usize,
    /// Overlay entry id of the open popup.
    popup_id: Option<u64>,
    /// State shared with popup widgets.
    shared: Arc<Mutex<PopupState>>,
    /// Typeahead buffer (printable characters typed while open).
    typeahead: String,
    /// Combobox bounds from the last layout pass.
    cached_bounds: Rect,
}

impl Dropdown {
    /// Creates a dropdown from option labels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Dropdown;
    ///
    /// let dd = Dropdown::new(["A", "B", "C"]);
    /// assert_eq!(dd.option_count(), 3);
    /// ```
    pub fn new(options: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let options: Vec<String> = options.into_iter().map(Into::into).collect();
        let shared = Arc::new(Mutex::new(PopupState {
            options: options.clone(),
            highlighted: 0,
            selected: 0,
            committed: None,
        }));
        Self {
            label: None,
            placeholder: String::new(),
            enabled: true,
            options,
            selected: 0,
            open: false,
            highlighted: 0,
            popup_id: None,
            shared,
            typeahead: String::new(),
            cached_bounds: Rect::default(),
        }
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Dropdown;
    ///
    /// let dd = Dropdown::new(["A"]).label("Size");
    /// assert_eq!(dd.label.as_deref(), Some("Size"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets placeholder text shown when no option is selected.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Dropdown;
    ///
    /// let dd = Dropdown::new(Vec::<String>::new()).placeholder("Pick…");
    /// assert_eq!(dd.placeholder, "Pick…");
    /// ```
    #[inline]
    #[must_use]
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Sets whether the combobox is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Dropdown;
    ///
    /// let dd = Dropdown::new(["A"]).enabled(false);
    /// assert!(!dd.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Number of options.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Dropdown;
    ///
    /// assert_eq!(Dropdown::new(["A", "B"]).option_count(), 2);
    /// ```
    #[inline]
    pub fn option_count(&self) -> usize {
        self.options.len()
    }

    /// Committed option index.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Dropdown;
    ///
    /// assert_eq!(Dropdown::new(["A", "B"]).selected(), 0);
    /// ```
    #[inline]
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The selected option's text, or `None` when the list is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Dropdown;
    ///
    /// let dd = Dropdown::new(["Alpha", "Beta"]);
    /// assert_eq!(dd.selected_text(), Some("Alpha"));
    /// ```
    pub fn selected_text(&self) -> Option<&str> {
        self.options.get(self.selected).map(String::as_str)
    }

    /// Whether the popup is logically open.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Dropdown;
    ///
    /// let mut dd = Dropdown::new(["A"]);
    /// dd.open();
    /// assert!(dd.is_open());
    /// ```
    #[inline]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The highlighted option index (the `aria-activedescendant`
    /// target while open).
    #[inline]
    pub fn highlighted(&self) -> usize {
        self.highlighted
    }

    /// The overlay entry id of the open popup, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Dropdown;
    ///
    /// let dd = Dropdown::new(["A"]);
    /// assert_eq!(dd.popup_id(), None);
    /// ```
    #[inline]
    pub fn popup_id(&self) -> Option<u64> {
        self.popup_id
    }

    /// Opens the popup with the current selection highlighted.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Dropdown;
    ///
    /// let mut dd = Dropdown::new(["A", "B"]);
    /// dd.open();
    /// assert_eq!(dd.highlighted(), dd.selected());
    /// ```
    pub fn open(&mut self) {
        if self.options.is_empty() {
            return;
        }
        self.open = true;
        self.highlighted = self.selected.min(self.options.len() - 1);
        self.typeahead.clear();
        self.push_shared();
    }

    /// Closes the popup without committing.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Dropdown;
    ///
    /// let mut dd = Dropdown::new(["A", "B"]);
    /// dd.open();
    /// dd.close();
    /// assert!(!dd.is_open());
    /// ```
    pub fn close(&mut self) {
        self.open = false;
        self.typeahead.clear();
    }

    /// Selects `index` and closes the popup.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Dropdown;
    ///
    /// let mut dd = Dropdown::new(["A", "B", "C"]);
    /// dd.open();
    /// dd.commit(2);
    /// assert_eq!(dd.selected(), 2);
    /// assert!(!dd.is_open());
    /// ```
    pub fn commit(&mut self, index: usize) {
        if index < self.options.len() {
            self.selected = index;
        }
        self.close();
        self.push_shared();
    }

    /// Moves the popup highlight by `delta` with clamping (APG keeps
    /// the highlight inside the list).
    pub fn move_highlight(&mut self, delta: i64) {
        let n = self.options.len() as i64;
        if n == 0 {
            return;
        }
        self.highlighted = (self.highlighted as i64 + delta).clamp(0, n - 1) as usize;
        self.push_shared();
    }

    /// Typeahead: searches options for `buffer`, starting after the
    /// current highlight and wrapping (APG "type to select").
    fn typeahead_select(&mut self, c: char) -> bool {
        self.typeahead.push(c.to_ascii_lowercase());
        let n = self.options.len();
        if n == 0 {
            return false;
        }
        let buffer = self.typeahead.clone();
        // First try the accumulated buffer; fall back to the single
        // character, scanning from just after the highlight so
        // repeating a character cycles through its matches (APG
        // typeahead semantics).
        for (probe, start) in [
            (buffer.as_str(), 0usize),
            (&buffer[buffer.len() - 1..], 1usize),
        ] {
            for offset in start..start + n {
                let i = (self.highlighted + offset) % n;
                if self.options[i].to_ascii_lowercase().starts_with(probe) {
                    self.highlighted = i;
                    if !self.open {
                        self.selected = i;
                    }
                    self.push_shared();
                    return true;
                }
            }
        }
        false
    }

    /// Reconciles the overlay with the combobox's open state.
    ///
    /// Call once per frame before `OverlayLayer::layout_pass`:
    ///
    /// - applies an option commit made inside the popup;
    /// - opens/closes the popup entry to match [`is_open`](Self::is_open);
    /// - notices overlay-level dismissal (outside press, `Escape`) and
    ///   resets `open`/`popup_id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Dropdown;
    /// use martensite_core::overlay::OverlayLayer;
    /// use martensite_core::{HotNode, LayoutContext, Rect, Widget};
    ///
    /// let mut dd = Dropdown::new(["A", "B"]);
    /// let mut hot = HotNode::default();
    /// let mut cx = LayoutContext { hot: &mut hot };
    /// dd.layout(&mut cx, Rect::new(10.0, 10.0, 160.0, 32.0));
    ///
    /// let mut overlay = OverlayLayer::new();
    /// overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// dd.open();
    /// dd.sync_overlay(&mut overlay);
    /// overlay.layout_pass();
    /// assert_eq!(overlay.len(), 1);
    /// ```
    pub fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        self.drain_shared_state();
        // The layer dismissed our popup (outside press / Escape).
        if let Some(id) = self.popup_id {
            if !overlay.is_open(id) {
                self.popup_id = None;
                self.open = false;
            }
        }
        if self.open && self.popup_id.is_none() {
            self.push_shared();
            let popup = ListBoxPopup::new(Arc::clone(&self.shared));
            self.popup_id =
                Some(overlay.open(Box::new(popup), OverlayAnchor::Bounds(self.cached_bounds)));
        } else if !self.open {
            if let Some(id) = self.popup_id.take() {
                overlay.close(id);
            }
        }
    }

    /// Mirrors owner state into the shared popup state.
    fn push_shared(&self) {
        let mut state = self.shared.lock().expect("popup state poisoned");
        state.options = self.options.clone();
        state.selected = self.selected;
        state.highlighted = self.highlighted;
    }

    /// Applies state the popup wrote into the shared slot: a committed
    /// selection (click or AT `Click` on an option) is committed here,
    /// and popup-side highlight changes are mirrored so
    /// `aria-activedescendant` follows the pointer.
    fn drain_shared_state(&mut self) {
        // A popup option committed a selection (click or AT Click).
        let committed = self
            .shared
            .lock()
            .expect("popup state poisoned")
            .committed
            .take();
        if let Some(index) = committed {
            self.commit(index);
        }
        // Popup-side hover moved the highlight — mirror it so
        // aria-activedescendant follows the pointer.
        let popup_highlight = self
            .shared
            .lock()
            .expect("popup state poisoned")
            .highlighted;
        self.highlighted = popup_highlight.min(self.options.len().saturating_sub(1));
    }
}

impl Widget for Dropdown {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Approximate face width — real shaping lives in the
        // `martensite-text` pipeline.
        let widest = self
            .options
            .iter()
            .map(|o| o.chars().count())
            .max()
            .unwrap_or_else(|| self.placeholder.chars().count()) as f32;
        let w = widest * 7.0 + 48.0;
        Vec2::new(
            w.clamp(80.0, constraints.max_size.x.max(0.0)),
            FACE_H.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        // Declare keyboard focusability on the arena node.
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ComboBox);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        node.set_value(
            self.selected_text()
                .unwrap_or(self.placeholder.as_str())
                .to_string(),
        );
        node.set_has_popup(accesskit::HasPopup::Listbox);
        node.set_expanded(self.open);
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Expand);
        node.add_action(accesskit::Action::Collapse);
        // `SetValue` selects an option by label.
        node.add_action(accesskit::Action::SetValue);
        if self.enabled {
            node.add_action(accesskit::Action::Focus);
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn a11y_prepare(&mut self) {
        // AT activations delivered to popup options (an `Action::Click`
        // on a ListBoxOption virtual node) write into the shared slot;
        // drain them here so the emitted tree reflects the commit even
        // when the action bypassed `sync_overlay`.
        self.drain_shared_state();
    }

    fn a11y_fixup(
        &self,
        _emitted: &mut Vec<A11yEmittedNode>,
        overlay_nodes: &[OverlayA11yRef],
        this_node: &mut AccessKitNode,
    ) {
        let Some(popup) = self.popup_id else {
            return;
        };
        // aria-controls → the popup's ListBox root.
        if let Some(listbox_id) = overlay_nodes
            .iter()
            .find(|r| r.entry == popup && r.path.is_empty())
            .map(|r| r.id)
        {
            this_node.set_controls(vec![listbox_id]);
        }
        // aria-activedescendant → the highlighted option. Options live
        // at path [0, 0, i]: ListBoxPopup → ScrollView → OptionColumn.
        let option_path = [0u32, 0, self.highlighted as u32];
        if let Some(option_id) = overlay_nodes
            .iter()
            .find(|r| r.entry == popup && r.path.as_slice() == option_path)
            .map(|r| r.id)
        {
            this_node.set_active_descendant(option_id);
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
                if self.open {
                    self.close();
                } else {
                    self.open();
                }
                EventResponse::CaptureFocus
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                // While the popup is open this branch is normally
                // unreachable: the arena-owned OverlayLayer consumes
                // `Escape` first (dismissing the topmost popup), and
                // `sync_overlay` reconciles `self.open` via the
                // dismissal queue. It's kept for ownerless-embedded use
                // — a Dropdown driven without arena overlay routing —
                // where no layer intercepts the key.
                "Escape" => {
                    if self.open {
                        self.close();
                        return EventResponse::RequestRepaint;
                    }
                    EventResponse::Ignored
                }
                "Enter" | " " | "Space" => {
                    if self.open {
                        self.commit(self.highlighted);
                    } else {
                        self.open();
                    }
                    EventResponse::RequestRepaint
                }
                "ArrowDown" => {
                    if self.open {
                        self.move_highlight(1);
                    } else {
                        self.open();
                    }
                    EventResponse::RequestRepaint
                }
                "ArrowUp" => {
                    if self.open {
                        self.move_highlight(-1);
                    } else {
                        self.open();
                    }
                    EventResponse::RequestRepaint
                }
                "Home" if self.open => {
                    self.highlighted = 0;
                    self.push_shared();
                    EventResponse::RequestRepaint
                }
                "End" if self.open => {
                    self.highlighted = self.options.len().saturating_sub(1);
                    self.push_shared();
                    EventResponse::RequestRepaint
                }
                k if k.chars().count() == 1 => {
                    let c = k.chars().next().unwrap_or_default();
                    if c.is_ascii_alphanumeric() && self.typeahead_select(c) {
                        EventResponse::RequestRepaint
                    } else {
                        EventResponse::Ignored
                    }
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::Expand => {
                    self.open();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Collapse => {
                    self.close();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Click => {
                    if self.open {
                        self.close();
                    } else {
                        self.open();
                    }
                    EventResponse::RequestRepaint
                }
                SemanticAction::SetValue(text) => {
                    let probe = text.to_ascii_lowercase();
                    if let Some(i) = self
                        .options
                        .iter()
                        .position(|o| o.to_ascii_lowercase() == probe)
                    {
                        self.commit(i);
                        return EventResponse::RequestRepaint;
                    }
                    EventResponse::Ignored
                }
                SemanticAction::Focus => EventResponse::CaptureFocus,
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // Delegate to the inherent method so `Dropdown::sync_overlay`
        // and the `Widget` trait seam stay in lock-step.
        Dropdown::sync_overlay(self, overlay);
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        cx.list.push_fill_rect(rect, FACE_BG);
        cx.list.push_stroke_rect(rect, 1.0, FACE_BORDER);
        let ink = if self.enabled { INK } else { INK_DISABLED };
        let text = self
            .selected_text()
            .unwrap_or(self.placeholder.as_str())
            .to_string();
        cx.list.push_text(
            kurbo::Point::new(
                f64::from(b.min_x() + 10.0),
                f64::from(b.min_y() + b.height() / 2.0 + 5.0),
            ),
            text,
            14.0,
            ink,
        );
        // Disclosure triangle.
        let cx_mid = f64::from(b.max_x() - 16.0);
        let cy = f64::from(b.min_y() + b.height() / 2.0);
        let tri = kurbo::BezPath::from_vec(vec![
            kurbo::PathEl::MoveTo(kurbo::Point::new(cx_mid - 5.0, cy - 2.5)),
            kurbo::PathEl::LineTo(kurbo::Point::new(cx_mid + 5.0, cy - 2.5)),
            kurbo::PathEl::LineTo(kurbo::Point::new(cx_mid, cy + 3.5)),
            kurbo::PathEl::ClosePath,
        ]);
        cx.list.push_path(tri, INK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(dd: &mut Dropdown) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext { hot: &mut hot };
        dd.layout(&mut cx, Rect::new(10.0, 10.0, 160.0, 32.0));
    }

    fn overlay() -> OverlayLayer {
        let mut o = OverlayLayer::new();
        o.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
        o
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn event(dd: &mut Dropdown, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: dd.cached_bounds,
        };
        dd.event(&mut cx)
    }

    #[test]
    fn open_close_keys() {
        let mut dd = Dropdown::new(["A", "B", "C"]);
        laid_out(&mut dd);
        event(&mut dd, &key("ArrowDown"));
        assert!(dd.is_open());
        event(&mut dd, &key("Escape"));
        assert!(!dd.is_open());
        event(&mut dd, &key("Enter"));
        assert!(dd.is_open());
    }

    #[test]
    fn arrow_moves_highlight_clamped() {
        let mut dd = Dropdown::new(["A", "B", "C"]);
        laid_out(&mut dd);
        dd.open();
        event(&mut dd, &key("ArrowUp")); // at top, clamped
        assert_eq!(dd.highlighted(), 0);
        event(&mut dd, &key("ArrowDown"));
        event(&mut dd, &key("ArrowDown"));
        event(&mut dd, &key("ArrowDown")); // clamps at last
        assert_eq!(dd.highlighted(), 2);
        event(&mut dd, &key("Home"));
        assert_eq!(dd.highlighted(), 0);
        event(&mut dd, &key("End"));
        assert_eq!(dd.highlighted(), 2);
    }

    #[test]
    fn enter_commits_highlight() {
        let mut dd = Dropdown::new(["A", "B", "C"]);
        laid_out(&mut dd);
        event(&mut dd, &key("ArrowDown")); // open, highlight 0
        event(&mut dd, &key("ArrowDown")); // highlight 1
        event(&mut dd, &key("Enter"));
        assert_eq!(dd.selected(), 1);
        assert!(!dd.is_open());
    }

    #[test]
    fn typeahead_selects_when_closed() {
        let mut dd = Dropdown::new(["Alpha", "Beta", "Gamma"]);
        laid_out(&mut dd);
        event(&mut dd, &key("g"));
        assert_eq!(dd.selected(), 2);
    }

    #[test]
    fn typeahead_cycles_same_letter() {
        let mut dd = Dropdown::new(["Apple", "Avocado", "Banana"]);
        laid_out(&mut dd);
        dd.open();
        dd.typeahead.clear();
        // Single 'a' typed twice should cycle Apple → Avocado.
        dd.typeahead = "a".to_string();
        assert!(dd.typeahead_select('a'));
        assert_eq!(dd.highlighted(), 1);
    }

    #[test]
    fn overlay_opens_listbox_below() {
        let mut dd = Dropdown::new(["A", "B"]);
        laid_out(&mut dd);
        let mut o = overlay();
        dd.open();
        dd.sync_overlay(&mut o);
        o.layout_pass();
        assert_eq!(o.len(), 1);
        let b = o.entry_bounds(dd.popup_id.unwrap()).unwrap();
        // Below the combobox face.
        assert!(b.min_y() >= 42.0);
    }

    #[test]
    fn outside_press_dismisses_and_reconciles() {
        let mut dd = Dropdown::new(["A", "B"]);
        laid_out(&mut dd);
        let mut o = overlay();
        dd.open();
        dd.sync_overlay(&mut o);
        o.layout_pass();
        // Press outside the popup → overlay dismisses all.
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
        };
        assert_eq!(o.dispatch_event(&press), EventResponse::Ignored);
        dd.sync_overlay(&mut o);
        assert!(!dd.is_open());
        assert_eq!(dd.popup_id, None);
    }

    #[test]
    fn popup_option_click_commits() {
        let mut dd = Dropdown::new(["A", "B", "C"]);
        laid_out(&mut dd);
        let mut o = overlay();
        dd.open();
        dd.sync_overlay(&mut o);
        o.layout_pass();
        let id = dd.popup_id.unwrap();
        // Reach the option at path [0, 0, 1] inside the popup and
        // press it.
        let option = o.widget_at_mut(id, &[0, 0, 1]).expect("option widget");
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(5.0, 5.0),
            button: PointerButton::Primary,
        };
        let mut cx = EventContext {
            event: &press,
            bounds: Rect::default(),
        };
        assert_eq!(option.event(&mut cx), EventResponse::Handled);
        dd.sync_overlay(&mut o);
        assert_eq!(dd.selected(), 1);
        assert!(!dd.is_open());
    }

    #[test]
    fn combobox_accessibility() {
        let mut dd = Dropdown::new(["A", "B"]).label("Size");
        dd.open();
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        dd.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::ComboBox);
        assert_eq!(node.label(), Some("Size"));
        assert_eq!(node.value(), Some("A"));
        assert_eq!(node.is_expanded(), Some(true));
        assert!(node.supports_action(accesskit::Action::Expand));
        assert!(node.supports_action(accesskit::Action::Collapse));
    }

    #[test]
    fn semantic_expand_collapse() {
        let mut dd = Dropdown::new(["A", "B"]);
        laid_out(&mut dd);
        event(
            &mut dd,
            &WidgetEvent::SemanticAction(SemanticAction::Expand),
        );
        assert!(dd.is_open());
        event(
            &mut dd,
            &WidgetEvent::SemanticAction(SemanticAction::Collapse),
        );
        assert!(!dd.is_open());
    }

    #[test]
    fn semantic_set_value_selects() {
        let mut dd = Dropdown::new(["Small", "Medium", "Large"]);
        laid_out(&mut dd);
        event(
            &mut dd,
            &WidgetEvent::SemanticAction(SemanticAction::SetValue("Large".to_string())),
        );
        assert_eq!(dd.selected(), 2);
    }
}
