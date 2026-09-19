//! `AutoComplete` widget: a text input with a filtered suggestion
//! popup — the Qt `QCompleter` / WinUI `AutoSuggestBox` / Ant
//! `AutoComplete` equivalent.
//!
//! Implements the [APG editable-combobox pattern](https://www.w3.org/WAI/ARIA/apg/patterns/combobox/):
//!
//! - The widget embeds a [`TextInput`] internal child and emits
//!   `Role::ComboBox` with `aria-haspopup="listbox"`, `aria-expanded`,
//!   `aria-controls`, and `aria-activedescendant` wired to the popup
//!   (via [`Widget::a11y_fixup`](martensite_core::Widget::a11y_fixup)
//!   against overlay node ids). The inner field keeps its own
//!   `Role::TextInput` node as a virtual child.
//! - The popup is a `Role::ListBox` of `Role::ListBoxOption` children
//!   living in the [`OverlayLayer`](martensite_core::overlay::OverlayLayer),
//!   placed below the field (flipping above near the bottom edge) —
//!   the same architecture [`Dropdown`](crate::widgets::Dropdown) uses,
//!   including `Arc<Mutex<_>>`-shared state so hover/click inside the
//!   popup reaches the owner.
//! - Typing filters [`suggestions`](AutoComplete::suggestions) by
//!   case-insensitive substring match ([`FilterMode::Prefix`] switches
//!   to prefix matching). The popup opens on focus-with-text and on
//!   edits once the field holds at least
//!   [`min_chars`](AutoComplete::min_chars) characters, and closes on
//!   commit, `Escape`, blur, or text that falls below `min_chars` /
//!   yields no matches.
//! - `ArrowDown`/`ArrowUp` move the highlight and preview the
//!   suggestion in the field; `Enter`/`Tab` commits it into the field,
//!   `Escape` closes and restores the pre-popup text. A live popup's
//!   option list is rebuilt in place (`OverlayLayer::replace_content`)
//!   as filtering narrows the matches, keeping its entry id and
//!   z-position stable.
//! - Out-seams: [`take_committed`](AutoComplete::take_committed)
//!   (explicit pick or Enter-submit) and
//!   [`take_edited`](AutoComplete::take_edited) (forwarded from the
//!   embedded field).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::AutoComplete;
//!
//! let ac = AutoComplete::new().suggestions(["Apple", "Banana", "Cherry"]);
//! assert!(!ac.is_open());
//! assert_eq!(ac.value(), "");
//! ```

use std::sync::Arc;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
use martensite_core::shape::Shape;
use martensite_core::widget::{
    A11yEmittedNode, EventContext, EventResponse, LayoutConstraints, LayoutContext, OverlayA11yRef,
    PaintContext, PointerButton, SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, RenderMinimum, TokenKey, UnderflowPolicy};
use parking_lot::Mutex;

use crate::widgets::scrollview::ScrollView;
use crate::widgets::text_input::TextInput;

/// Option row height in the popup, in logical pixels.
const ROW_H: f32 = 28.0;
/// Default maximum popup rows before the list scrolls.
const DEFAULT_MAX_VISIBLE: usize = 8;
/// Default field characters required before the popup may open.
const DEFAULT_MIN_CHARS: usize = 1;
/// Field measurement floor in logical points (the embedded
/// `TextInput`'s one-line height).
const FIELD_W: f32 = 160.0;
const FIELD_H: f32 = 24.0;
/// Label ink.
const INK: [u8; 4] = [30, 30, 36, 255];
/// Popup background.
const POPUP_BG: [u8; 4] = [252, 252, 254, 255];
/// Popup border.
const POPUP_BORDER: [u8; 4] = [140, 145, 155, 255];
/// Highlighted option background.
const HIGHLIGHT_BG: [u8; 4] = [60, 110, 220, 255];
/// Highlighted option ink.
const HIGHLIGHT_INK: [u8; 4] = [255, 255, 255, 255];
/// Fallback accessible name when no [`AutoComplete::label`] is set.
const DEFAULT_LABEL: &str = "Autocomplete";

/// How the typed text filters an [`AutoComplete`]'s suggestion list.
///
/// # Examples
///
/// ```
/// use martensite::widgets::auto_complete::FilterMode;
///
/// assert_eq!(FilterMode::default(), FilterMode::Substring);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum FilterMode {
    /// A suggestion matches when the needle appears anywhere in it
    /// (the default — `QCompleter`'s `MatchContains`).
    #[default]
    Substring,
    /// A suggestion matches only when it starts with the needle.
    Prefix,
}

/// State shared between an [`AutoComplete`] and its popup widgets.
///
/// The popup content reads `items`/`highlighted` to paint and emit
/// accessibility; option hover and commit write back so the owner can
/// apply them on the next `sync_overlay`.
#[derive(Debug)]
struct PopupState {
    /// Filtered suggestion labels in display order.
    items: Vec<String>,
    /// Row with the visual highlight (the `aria-activedescendant`
    /// target); `None` before keyboard navigation begins.
    highlighted: Option<usize>,
    /// Set by a popup option when it is activated (click or AT Click).
    committed: Option<usize>,
}

/// One suggestion inside the popup listbox — a stateless view over
/// the shared [`PopupState`], emitted as `Role::ListBoxOption`.
struct SuggestionItem {
    /// Index into `shared.items`.
    index: usize,
    /// Shared state with the owning combobox.
    shared: Arc<Mutex<PopupState>>,
    /// Shared shaped-text painter from the owning `AutoComplete`.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Widget for SuggestionItem {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Content width (label approx + gutters) — reporting
        // `max_size.x` here makes the ScrollView think the column
        // overflows horizontally and shows a phantom hbar.
        let label_w = self
            .shared
            .lock()
            .items
            .get(self.index)
            .map(|o| o.chars().count() as f32 * cx.pt(7.0) + cx.pt(32.0))
            .unwrap_or(0.0);
        Vec2::new(
            label_w.min(constraints.max_size.x.max(0.0)),
            cx.pt(ROW_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListBoxOption);
        let state = self.shared.lock();
        if let Some(label) = state.items.get(self.index) {
            node.set_label(label.as_str());
        }
        node.set_position_in_set(self.index + 1);
        node.set_size_of_set(state.items.len());
        node.add_action(accesskit::Action::Click);
        // No `Action::Focus`: options are not focusable — the combobox
        // owns focus and tracks the highlight via
        // `aria-activedescendant` (the editable APG combobox pattern).
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { .. } | WidgetEvent::PointerEnter => {
                let mut state = self.shared.lock();
                if state.highlighted != Some(self.index) {
                    state.highlighted = Some(self.index);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                ..
            } => {
                self.shared.lock().committed = Some(self.index);
                EventResponse::Handled
            }
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                self.shared.lock().committed = Some(self.index);
                EventResponse::Handled
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => {
                self.shared.lock().highlighted = Some(self.index);
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let state = self.shared.lock();
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let highlighted = state.highlighted == Some(self.index);
        if highlighted {
            cx.list
                .push_fill_rect(rect, cx.color(TokenKey::AccentColor, HIGHLIGHT_BG));
        }
        // `DrawText` positions by the run's top edge — centre the 14 pt
        // font box inside the row.
        let font_px = cx.pt(14.0);
        let text_y = b.min_y() + (b.height() - font_px) / 2.0;
        let ink = if highlighted {
            cx.color(TokenKey::TextInverseColor, HIGHLIGHT_INK)
        } else {
            cx.color(TokenKey::TextColor, INK)
        };
        if let Some(label) = state.items.get(self.index) {
            // Clip the option label to the row — a long suggestion
            // can't spill past the popup's right edge.
            let text_x = b.min_x() + cx.pt(10.0);
            crate::text_paint::paint_label_clipped(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Rect::new(
                    f64::from(text_x),
                    f64::from(b.min_y()),
                    f64::from(b.max_x() - cx.pt(6.0)),
                    f64::from(b.max_y()),
                ),
                kurbo::Point::new(f64::from(text_x), f64::from(text_y)),
                label.as_str(),
                font_px,
                ink,
            );
        }
    }
}

/// Vertical column of popup suggestions — the scroll view's content.
struct SuggestionColumn {
    /// Suggestions in order.
    items: Vec<SuggestionItem>,
    /// Row bounds from the last layout pass.
    row_bounds: Vec<Rect>,
}

impl Widget for SuggestionColumn {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Widest item — `SuggestionItem::measure` reports content
        // width so the ScrollView does not see a phantom horizontal
        // overflow.
        let w = self
            .items
            .iter_mut()
            .map(|i| i.measure(cx, constraints).x)
            .fold(0.0f32, f32::max);
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            (self.items.len() as f32 * cx.pt(ROW_H)).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.row_bounds.clear();
        for (i, item) in self.items.iter_mut().enumerate() {
            let rect = Rect::new(
                bounds.min_x(),
                bounds.min_y() + i as f32 * cx.pt(ROW_H),
                bounds.width(),
                cx.pt(ROW_H),
            );
            self.row_bounds.push(rect);
            cx.layout_child(item, rect);
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
/// [`SuggestionItem`]s. Opened in the overlay by
/// [`AutoComplete::sync_overlay`].
struct SuggestionPopup {
    /// Scrolling suggestion list (internal child 0).
    scroll: ScrollView,
    /// Shared state with the owning combobox.
    shared: Arc<Mutex<PopupState>>,
    /// Maximum rows the popup measures before scrolling.
    max_rows: usize,
    /// Popup bounds from the last layout pass (the scroll view fills
    /// the whole popup inside its 1px border).
    bounds: Option<Rect>,
    /// The silhouette painted last frame — the single source of truth
    /// for `clip_shape`/`hit_shape` so clipping and hit-testing can
    /// never diverge from the visible outline.
    painted_shape: Mutex<Shape>,
}

impl SuggestionPopup {
    fn new(
        shared: Arc<Mutex<PopupState>>,
        text_painter: Option<crate::text_paint::SharedTextPainter>,
        max_rows: usize,
    ) -> Self {
        let items = {
            let state = shared.lock();
            (0..state.items.len())
                .map(|index| SuggestionItem {
                    index,
                    shared: Arc::clone(&shared),
                    text_painter: text_painter.clone(),
                })
                .collect()
        };
        Self {
            scroll: ScrollView::new(SuggestionColumn {
                items,
                row_bounds: Vec::new(),
            }),
            shared,
            max_rows,
            bounds: None,
            painted_shape: Mutex::new(Shape::RECT),
        }
    }
}

impl Widget for SuggestionPopup {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut s = self.scroll.measure(cx, constraints);
        s.y = s.y.min(self.max_rows as f32 * cx.pt(ROW_H) + cx.pt(2.0));
        // Options measure as fill-width (`max_size.x`), so `s.x` would
        // report the whole viewport — size to the widest suggestion's
        // text plus gutters instead.
        let widest = self
            .shared
            .lock()
            .items
            .iter()
            .map(|o| o.chars().count())
            .max()
            .unwrap_or(0) as f32;
        s.x = (widest * cx.pt(7.0) + cx.pt(32.0)).clamp(
            cx.pt(80.0).min(constraints.max_size.x.max(0.0)),
            constraints.max_size.x.max(0.0),
        );
        s
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = Some(bounds);
        cx.layout_child(&mut self.scroll, bounds);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListBox);
        node.set_orientation(accesskit::Orientation::Vertical);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Keyboard on the popup itself: move the highlight / commit.
        // Production keyboard input stays with the owning field —
        // this path covers ownerless-embedded popups, mirroring
        // `Dropdown`'s popup.
        if let WidgetEvent::KeyPressed { key, .. } = cx.event {
            let mut state = self.shared.lock();
            let n = state.items.len();
            if n > 0 {
                match key.as_str() {
                    "ArrowDown" => {
                        state.highlighted =
                            Some(state.highlighted.map_or(0, |h| (h + 1).min(n - 1)));
                        return EventResponse::RequestRepaint;
                    }
                    "ArrowUp" => {
                        state.highlighted =
                            Some(state.highlighted.map_or(n - 1, |h| h.saturating_sub(1)));
                        return EventResponse::RequestRepaint;
                    }
                    "Home" => {
                        state.highlighted = Some(0);
                        return EventResponse::RequestRepaint;
                    }
                    "End" => {
                        state.highlighted = Some(n - 1);
                        return EventResponse::RequestRepaint;
                    }
                    "Enter" | " " | "Space" => {
                        if let Some(h) = state.highlighted {
                            state.committed = Some(h);
                        }
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
        let popup_shape = Shape::rounded(cx.dim(TokenKey::BorderRadius, 6.0));
        *self.painted_shape.lock() = popup_shape.clone();
        cx.list.push_fill_shape(
            rect,
            &popup_shape,
            cx.color(TokenKey::SurfaceColor, POPUP_BG),
        );
        cx.list.push_stroke_shape(
            rect,
            &popup_shape,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, POPUP_BORDER),
        );
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn clip_shape(&self) -> Option<Shape> {
        Some(self.painted_shape.lock().clone())
    }

    fn hit_shape(&self) -> Option<Shape> {
        Some(self.painted_shape.lock().clone())
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

/// A text input with a filtered suggestion popup — an editable
/// combobox.
///
/// Owns no popup widget itself — [`sync_overlay`](Self::sync_overlay)
/// reconciles an overlay entry each frame so the suggestion list
/// paints above window content and is emitted into the accessibility
/// tree.
///
/// # Examples
///
/// ```
/// use martensite::widgets::AutoComplete;
///
/// let ac = AutoComplete::new()
///     .suggestions(["Apple", "Avocado", "Banana"])
///     .placeholder("Fruit…");
/// assert_eq!(ac.suggestion_count(), 3);
/// ```
pub struct AutoComplete {
    /// Optional accessible label for the combobox.
    pub label: Option<String>,
    /// Whether the widget accepts input.
    pub enabled: bool,
    /// Placeholder text shown while the field is empty.
    pub placeholder: String,
    /// The embedded text field (internal child).
    field: TextInput,
    /// Field bounds assigned in `layout` (fills the widget).
    field_rect: Rect,
    /// Full candidate list.
    suggestions: Vec<String>,
    /// Candidates matching the current field text.
    filtered: Vec<String>,
    /// Substring vs prefix matching.
    filter: FilterMode,
    /// Minimum field characters before the popup may open.
    min_chars: usize,
    /// Maximum popup rows before the list scrolls.
    max_visible: usize,
    /// Whether the popup is logically open.
    open: bool,
    /// Highlighted filtered index (the `aria-activedescendant`
    /// target while open).
    highlighted: Option<usize>,
    /// Overlay entry id of the open popup.
    popup_id: Option<u64>,
    /// State shared with popup widgets.
    shared: Arc<Mutex<PopupState>>,
    /// The typed text that produced `filtered` — what `Escape`
    /// restores when a keyboard-previewed suggestion sits in the
    /// field.
    baseline: String,
    /// Whether the field currently shows a keyboard-previewed
    /// suggestion rather than `baseline` text.
    previewed: bool,
    /// `take_committed` out-seam.
    committed: Option<String>,
    /// `take_edited` out-seam — buffered from the inner field's flag.
    edited: bool,
    /// Whether the widget holds keyboard focus.
    focused: bool,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// The bounds the live popup was last anchored to — `sync_overlay`
    /// re-anchors when `cached_bounds` moves so an open listbox tracks
    /// its field instead of detaching.
    last_anchor: Option<Rect>,
    /// Bumped every [`refilter`](Self::refilter) — the live popup is
    /// rebuilt via `OverlayLayer::replace_content` when its baked
    /// items go stale.
    generation: u64,
    /// The `generation` the live popup's content was built from.
    built_generation: u64,
    /// Shared shaped-text painter — `paint` emits real `GlyphRun`s
    /// when set. Propagated to the field and popup rows.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl AutoComplete {
    /// Creates an autocomplete with an empty suggestion list.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// let ac = AutoComplete::new();
    /// assert_eq!(ac.suggestion_count(), 0);
    /// assert_eq!(ac.value(), "");
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            placeholder: String::new(),
            field: TextInput::new(DEFAULT_LABEL).clearable(true),
            field_rect: Rect::default(),
            suggestions: Vec::new(),
            filtered: Vec::new(),
            filter: FilterMode::Substring,
            min_chars: DEFAULT_MIN_CHARS,
            max_visible: DEFAULT_MAX_VISIBLE,
            open: false,
            highlighted: None,
            popup_id: None,
            shared: Arc::new(Mutex::new(PopupState {
                items: Vec::new(),
                highlighted: None,
                committed: None,
            })),
            baseline: String::new(),
            previewed: false,
            committed: None,
            edited: false,
            focused: false,
            cached_bounds: Rect::default(),
            last_anchor: None,
            generation: 0,
            built_generation: 0,
            text_painter: None,
        }
    }

    /// Sets the full suggestion list (builder version).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// let ac = AutoComplete::new().suggestions(["A", "B"]);
    /// assert_eq!(ac.suggestion_count(), 2);
    /// ```
    #[must_use]
    pub fn suggestions(mut self, suggestions: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.set_suggestions(suggestions);
        self
    }

    /// Replaces the suggestion list and refilters against the current
    /// field text (mutable version).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// let mut ac = AutoComplete::new();
    /// ac.set_suggestions(["one", "two"]);
    /// assert_eq!(ac.suggestion_count(), 2);
    /// ```
    pub fn set_suggestions(&mut self, suggestions: impl IntoIterator<Item = impl Into<String>>) {
        self.suggestions = suggestions.into_iter().map(Into::into).collect();
        self.refilter();
        self.refresh_open();
        self.push_shared();
    }

    /// Sets the filter mode — [`FilterMode::Substring`] (default) or
    /// [`FilterMode::Prefix`]. Refilters immediately.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::auto_complete::{AutoComplete, FilterMode};
    ///
    /// let ac = AutoComplete::new()
    ///     .suggestions(["Apple", "Pineapple"])
    ///     .with_value("apple")
    ///     .filter_mode(FilterMode::Prefix);
    /// assert_eq!(ac.filtered().join(", "), "Apple");
    /// ```
    #[must_use]
    pub fn filter_mode(mut self, mode: FilterMode) -> Self {
        self.filter = mode;
        self.refilter();
        self
    }

    /// Sets the minimum field characters before the popup may open
    /// (default 1).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// let ac = AutoComplete::new().min_chars(3);
    /// assert_eq!(ac.minimum_chars(), 3);
    /// ```
    #[must_use]
    pub fn min_chars(mut self, min_chars: usize) -> Self {
        self.min_chars = min_chars;
        self.refresh_open();
        self
    }

    /// The minimum field characters before the popup may open.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// assert_eq!(AutoComplete::new().minimum_chars(), 1);
    /// ```
    #[inline]
    pub fn minimum_chars(&self) -> usize {
        self.min_chars
    }

    /// Sets the maximum popup rows before the suggestion list scrolls
    /// (default 8).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// let ac = AutoComplete::new().max_visible(5);
    /// assert_eq!(ac.maximum_visible(), 5);
    /// ```
    #[must_use]
    pub fn max_visible(mut self, max_visible: usize) -> Self {
        self.max_visible = max_visible.max(1);
        self
    }

    /// The maximum popup rows before the suggestion list scrolls.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// assert_eq!(AutoComplete::new().maximum_visible(), 8);
    /// ```
    #[inline]
    pub fn maximum_visible(&self) -> usize {
        self.max_visible
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// let ac = AutoComplete::new().label("City");
    /// assert_eq!(ac.label.as_deref(), Some("City"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the placeholder text shown while the field is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// let ac = AutoComplete::new().placeholder("Type to filter…");
    /// assert_eq!(ac.placeholder, "Type to filter…");
    /// ```
    #[inline]
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets the current field text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// let ac = AutoComplete::new().with_value("ap");
    /// assert_eq!(ac.value(), "ap");
    /// ```
    #[inline]
    #[must_use]
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.set_value(value);
        self
    }

    /// Sets the field text (mutable version — does not set the
    /// [`take_edited`](Self::take_edited) flag, matching
    /// `TextInput::set_value`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// let mut ac = AutoComplete::new();
    /// ac.set_value("ap");
    /// assert_eq!(ac.value(), "ap");
    /// ```
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.field.set_value(value);
        self.baseline.clone_from(&self.field.value);
        self.previewed = false;
        self.refilter();
        self.refresh_open();
        self.push_shared();
    }

    /// The current field text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// assert_eq!(AutoComplete::new().with_value("x").value(), "x");
    /// ```
    #[inline]
    pub fn value(&self) -> &str {
        &self.field.value
    }

    /// Sets whether the widget is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// let ac = AutoComplete::new().enabled(false);
    /// assert!(!ac.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so the field and
    /// popup rows emit real glyph runs instead of `DrawText`
    /// placeholder boxes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::shared_painter;
    /// use martensite::widgets::AutoComplete;
    ///
    /// let ac = AutoComplete::new().with_text_painter(shared_painter());
    /// ```
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.field = self.field.clone().with_text_painter(painter.clone());
        self.text_painter = Some(painter);
        self
    }

    /// Number of suggestions in the full candidate list.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// assert_eq!(AutoComplete::new().suggestions(["A", "B"]).suggestion_count(), 2);
    /// ```
    #[inline]
    pub fn suggestion_count(&self) -> usize {
        self.suggestions.len()
    }

    /// The suggestions matching the current field text, in display
    /// order — the rows the popup would show.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// let ac = AutoComplete::new()
    ///     .suggestions(["Apple", "Avocado", "Banana"])
    ///     .with_value("a");
    /// assert_eq!(ac.filtered().join(", "), "Apple, Avocado, Banana");
    /// ```
    #[inline]
    pub fn filtered(&self) -> &[String] {
        &self.filtered
    }

    /// Whether the popup is logically open.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// assert!(!AutoComplete::new().is_open());
    /// ```
    #[inline]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The highlighted filtered index (the `aria-activedescendant`
    /// target while open), or `None` before keyboard navigation
    /// begins.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// assert_eq!(AutoComplete::new().highlighted(), None);
    /// ```
    #[inline]
    pub fn highlighted(&self) -> Option<usize> {
        self.highlighted
    }

    /// The overlay entry id of the open popup, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// assert_eq!(AutoComplete::new().popup_id(), None);
    /// ```
    #[inline]
    pub fn popup_id(&self) -> Option<u64> {
        self.popup_id
    }

    /// The committed text — an explicit suggestion pick (popup click,
    /// `Enter`/`Tab` on a highlight) or an Enter-submit of the raw
    /// field text — or `None` when nothing has committed since the
    /// last call. The widget's commit out-seam (mirrors
    /// `Pagination::take_selected`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// let mut ac = AutoComplete::new();
    /// assert_eq!(ac.take_committed(), None);
    /// ```
    #[inline]
    pub fn take_committed(&mut self) -> Option<String> {
        self.committed.take()
    }

    /// `true` when a user-driven edit mutated the field text since
    /// the last call — forwards the embedded field's
    /// [`take_edited`](TextInput::take_edited) flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    ///
    /// let mut ac = AutoComplete::new();
    /// assert!(!ac.take_edited());
    /// ```
    #[inline]
    pub fn take_edited(&mut self) -> bool {
        std::mem::take(&mut self.edited)
    }

    /// Recomputes `filtered` from `suggestions` and the current field
    /// text, honouring [`FilterMode`]. Bumps `generation` so a live
    /// popup notices its baked items went stale.
    fn refilter(&mut self) {
        let needle = self.field.value.to_lowercase();
        self.filtered = self
            .suggestions
            .iter()
            .filter(|s| {
                let hay = s.to_lowercase();
                match self.filter {
                    FilterMode::Substring => hay.contains(needle.as_str()),
                    FilterMode::Prefix => hay.starts_with(needle.as_str()),
                }
            })
            .cloned()
            .collect();
        self.generation = self.generation.wrapping_add(1);
    }

    /// Whether the popup may show: enough field characters and at
    /// least one match.
    fn eligible(&self) -> bool {
        self.field.value.chars().count() >= self.min_chars && !self.filtered.is_empty()
    }

    /// Reconciles `open` with the eligibility rules — called after
    /// edits, focus changes, and list/value writes. The automatic
    /// open paths (focus-with-text, edits) require focus: a
    /// programmatic write while unfocused must not pop a suggestion
    /// list over unrelated content. Explicit requests use
    /// [`expand`](Self::expand) instead.
    fn refresh_open(&mut self) {
        self.open = self.enabled && self.focused && self.eligible();
        if !self.open {
            self.highlighted = None;
        }
    }

    /// Opens the popup when eligible, regardless of focus — the
    /// explicit AT `Expand` path (an expand request implies the
    /// combobox is the user's current context).
    fn expand(&mut self) {
        self.refilter();
        self.open = self.enabled && self.eligible();
        if self.open {
            self.push_shared();
        } else {
            self.highlighted = None;
        }
    }

    /// The user edited the field text: the new text becomes the
    /// `baseline` `Escape` restores, the preview (if any) is dropped,
    /// and the popup refilters.
    fn on_edit(&mut self) {
        self.baseline = self.field.value.clone();
        self.previewed = false;
        self.refilter();
        self.highlighted = None;
        self.refresh_open();
        self.push_shared();
    }

    /// Moves the highlight by `delta` with clamping (APG keeps the
    /// highlight inside the list) and previews the newly highlighted
    /// suggestion in the field — `Escape` restores `baseline`, commit
    /// keeps it.
    fn move_highlight(&mut self, delta: i64) {
        let n = self.filtered.len() as i64;
        if n == 0 {
            return;
        }
        let next = match self.highlighted {
            Some(h) => (h as i64 + delta).clamp(0, n - 1) as usize,
            None if delta >= 0 => 0,
            None => (n - 1) as usize,
        };
        self.highlighted = Some(next);
        // Preview the suggestion in the field — the platform
        // AutoSuggestBox behavior. `set_value` is a programmatic
        // write: no `edited` flag, no refilter.
        self.field.set_value(self.filtered[next].clone());
        self.previewed = true;
        self.push_shared();
    }

    /// Highlights `index` directly (`Home`/`End`) with the same
    /// preview semantics as [`move_highlight`](Self::move_highlight).
    fn highlight(&mut self, index: usize) {
        if self.filtered.is_empty() {
            return;
        }
        let next = index.min(self.filtered.len() - 1);
        self.highlighted = Some(next);
        self.field.set_value(self.filtered[next].clone());
        self.previewed = true;
        self.push_shared();
    }

    /// Commits `text` into the field and closes the popup — the
    /// shared path for explicit picks and popup commits.
    fn commit_text(&mut self, text: String) {
        self.field.set_value(text.clone());
        self.baseline.clone_from(&text);
        self.committed = Some(text);
        self.close();
    }

    /// Commits the highlighted suggestion, if any. Returns whether a
    /// suggestion was committed.
    fn commit_highlighted(&mut self) -> bool {
        if let Some(text) = self.highlighted.and_then(|h| self.filtered.get(h)).cloned() {
            self.commit_text(text);
            return true;
        }
        false
    }

    /// Closes the popup, keeping the field text — the commit path.
    fn close(&mut self) {
        self.open = false;
        self.highlighted = None;
        self.previewed = false;
    }

    /// Dismisses the popup, restoring `baseline` when a previewed
    /// suggestion sits in the field — the `Escape`/blur/outside-press
    /// path (the same reconciliation `Dropdown` performs when the
    /// layer dismisses its popup).
    fn dismiss(&mut self) {
        self.open = false;
        self.highlighted = None;
        if self.previewed {
            self.field.set_value(self.baseline.clone());
            self.previewed = false;
        }
    }

    /// Mirrors widget state into the shared popup state.
    fn push_shared(&self) {
        let mut state = self.shared.lock();
        state.items = self.filtered.clone();
        state.highlighted = self.highlighted;
    }

    /// Applies state the popup wrote into the shared slot: a
    /// committed pick (click or AT `Click` on an option) is committed
    /// here, and popup-side highlight changes are mirrored so
    /// `aria-activedescendant` follows the pointer.
    fn drain_shared_state(&mut self) {
        let committed = self.shared.lock().committed.take();
        if let Some(index) = committed {
            if let Some(text) = self.filtered.get(index).cloned() {
                self.commit_text(text);
            }
        }
        // Popup-side hover moved the highlight — mirror it so
        // aria-activedescendant follows the pointer.
        let popup_highlight = self.shared.lock().highlighted;
        self.highlighted = popup_highlight
            .map(|h| h.min(self.filtered.len().saturating_sub(1)))
            .filter(|_| self.open);
    }

    /// Mirrors widget state onto the embedded field.
    fn sync_field(&mut self) {
        self.field.enabled = self.enabled;
        self.field.label = self
            .label
            .clone()
            .unwrap_or_else(|| DEFAULT_LABEL.to_string());
        self.field.placeholder.clone_from(&self.placeholder);
    }

    /// Forwards an event to the embedded field the way the default
    /// `Widget::event` child walk would: positional events only inside
    /// the field's bounds, except moves and releases which always
    /// reach it so a captured drag keeps tracking outside.
    fn forward(&mut self, cx: &mut EventContext) -> EventResponse {
        if let Some(pos) = cx.event.position() {
            let drag = matches!(
                cx.event,
                WidgetEvent::PointerMoved { .. } | WidgetEvent::PointerReleased { .. }
            );
            if !drag && !self.field_rect.contains(pos) {
                return EventResponse::Ignored;
            }
        }
        let mut child_cx = EventContext {
            event: cx.event,
            bounds: self.field_rect,
            scale: cx.scale,
        };
        self.field.event(&mut child_cx)
    }

    /// Forwards an event to the field, then applies the edit protocol:
    /// a user-driven mutation refilters the popup and propagates the
    /// `edited` flag into the widget's own out-seam.
    fn forward_then_edit(&mut self, cx: &mut EventContext) -> EventResponse {
        let response = self.forward(cx);
        if self.field.take_edited() {
            self.edited = true;
            self.on_edit();
            EventResponse::RequestRepaint
        } else {
            response
        }
    }

    /// Reconciles the overlay with the combobox's open state.
    ///
    /// Call once per frame before `OverlayLayer::layout_pass`:
    ///
    /// - applies a suggestion commit made inside the popup;
    /// - opens/closes the popup entry to match
    ///   [`is_open`](Self::is_open);
    /// - rebuilds the live popup's content in place when the filtered
    ///   list changed under it (`OverlayLayer::replace_content`);
    /// - notices overlay-level dismissal (outside press, `Escape`) and
    ///   runs [`dismiss`](Self::dismiss) so previewed text restores.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::AutoComplete;
    /// use martensite_core::overlay::OverlayLayer;
    /// use martensite_core::widget::Widget;
    /// use martensite_core::{EventContext, HotNode, LayoutContext, Rect, WidgetEvent};
    ///
    /// let mut ac = AutoComplete::new().suggestions(["Apple", "Banana"]);
    /// let mut hot = HotNode::default();
    /// let mut cx = LayoutContext {
    ///     hot: &mut hot,
    ///     scale: 1.0,
    /// };
    /// ac.layout(&mut cx, Rect::new(10.0, 10.0, 160.0, 24.0));
    ///
    /// // Focus with matching text opens the popup.
    /// ac.set_value("a");
    /// let focus = WidgetEvent::FocusGained;
    /// let mut ecx = EventContext {
    ///     event: &focus,
    ///     bounds: Rect::new(10.0, 10.0, 160.0, 24.0),
    ///     scale: 1.0,
    /// };
    /// ac.event(&mut ecx);
    /// assert!(ac.is_open());
    ///
    /// let mut overlay = OverlayLayer::new();
    /// overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// ac.sync_overlay(&mut overlay);
    /// overlay.layout_pass();
    /// assert_eq!(overlay.len(), 1);
    /// ```
    pub fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        self.drain_shared_state();
        // The layer dismissed our popup (outside press / Escape).
        if let Some(id) = self.popup_id {
            if !overlay.is_open(id) {
                self.popup_id = None;
                self.last_anchor = None;
                self.dismiss();
            }
        }
        if self.open && self.popup_id.is_none() {
            self.push_shared();
            let popup = SuggestionPopup::new(
                Arc::clone(&self.shared),
                self.text_painter.clone(),
                self.max_visible,
            );
            self.popup_id =
                Some(overlay.open(Box::new(popup), OverlayAnchor::Bounds(self.cached_bounds)));
            self.built_generation = self.generation;
            self.last_anchor = Some(self.cached_bounds);
        } else if !self.open {
            if let Some(id) = self.popup_id.take() {
                overlay.close(id);
            }
            self.last_anchor = None;
        } else if let Some(id) = self.popup_id {
            // The filtered list changed under the live popup — rebuild
            // its content in place so the entry keeps its id and
            // z-position.
            if self.built_generation != self.generation {
                self.push_shared();
                let popup = SuggestionPopup::new(
                    Arc::clone(&self.shared),
                    self.text_painter.clone(),
                    self.max_visible,
                );
                overlay.replace_content(id, Box::new(popup));
                self.built_generation = self.generation;
            }
            // The field moved while open (resize, scale change,
            // relayout) — re-anchor so the listbox tracks it. Guarded
            // on change so a settled popup doesn't re-mark layout
            // every tick.
            if self.last_anchor != Some(self.cached_bounds) {
                overlay.set_anchor(id, OverlayAnchor::Bounds(self.cached_bounds));
                self.last_anchor = Some(self.cached_bounds);
            }
        }
    }
}

impl Default for AutoComplete {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for AutoComplete {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // A one-line editable field — the same 160x24 logical pt
        // request a `TextInput`-family face makes.
        Vec2::new(
            cx.pt(FIELD_W).min(constraints.max_size.x.max(0.0)),
            cx.pt(FIELD_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        // A narrower or shorter face cannot render the text lane
        // legibly.
        RenderMinimum::new(Vec2::new(120.0, FIELD_H)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        // Declare keyboard focusability on the arena node so
        // press-to-focus applies; key/IME input then forwards into
        // the field child.
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
        self.sync_field();
        self.field_rect = bounds;
        cx.layout_child(&mut self.field, self.field_rect);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ComboBox);
        node.set_label(self.label.as_deref().unwrap_or(DEFAULT_LABEL));
        node.set_value(self.field.value.as_str());
        node.set_has_popup(accesskit::HasPopup::Listbox);
        node.set_auto_complete(accesskit::AutoComplete::List);
        node.set_expanded(self.open);
        node.add_action(accesskit::Action::SetValue);
        node.add_action(accesskit::Action::Expand);
        node.add_action(accesskit::Action::Collapse);
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
        // at path [0, 0, i]: SuggestionPopup → ScrollView →
        // SuggestionColumn.
        if let Some(highlighted) = self.highlighted {
            let option_path = [0u32, 0, highlighted as u32];
            if let Some(option_id) = overlay_nodes
                .iter()
                .find(|r| r.entry == popup && r.path.as_slice() == option_path)
                .map(|r| r.id)
            {
                this_node.set_active_descendant(option_id);
            }
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                // While the popup is open this branch is normally
                // unreachable for `Escape`: the arena-owned
                // OverlayLayer consumes it first (dismissing the
                // topmost popup), and `sync_overlay` reconciles via
                // `dismiss`. It's kept for ownerless-embedded use —
                // an AutoComplete driven without arena overlay
                // routing — where no layer intercepts the key.
                "Escape" => {
                    if self.open {
                        self.dismiss();
                        EventResponse::RequestRepaint
                    } else {
                        // Let the field collapse a selection.
                        self.forward(cx)
                    }
                }
                "ArrowDown" => {
                    if self.open {
                        self.move_highlight(1);
                        EventResponse::RequestRepaint
                    } else {
                        self.forward_then_edit(cx)
                    }
                }
                "ArrowUp" => {
                    if self.open {
                        self.move_highlight(-1);
                        EventResponse::RequestRepaint
                    } else {
                        self.forward_then_edit(cx)
                    }
                }
                "Home" if self.open => {
                    self.highlight(0);
                    EventResponse::RequestRepaint
                }
                "End" if self.open => {
                    self.highlight(self.filtered.len().saturating_sub(1));
                    EventResponse::RequestRepaint
                }
                "Enter" => {
                    // Enter-submit: a highlighted suggestion wins;
                    // otherwise the raw field text commits.
                    if !(self.open && self.commit_highlighted()) {
                        self.committed = Some(self.field.value.clone());
                        self.baseline.clone_from(&self.field.value);
                        self.close();
                    }
                    EventResponse::RequestRepaint
                }
                "Tab" => {
                    // Commit any highlight into the field, then let
                    // the key fall through so focus traversal moves
                    // on (the `FocusLost` that follows closes the
                    // popup for real).
                    if self.open {
                        self.commit_highlighted();
                        self.close();
                    }
                    EventResponse::Ignored
                }
                _ => self.forward_then_edit(cx),
            },
            WidgetEvent::FocusGained => {
                self.focused = true;
                // The text the popup opens on — `Escape` restores it.
                self.baseline.clone_from(&self.field.value);
                let response = self.forward(cx);
                self.refilter();
                self.refresh_open();
                self.push_shared();
                response
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                // Blur dismisses the popup and restores any preview.
                self.dismiss();
                self.forward(cx)
            }
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::SetValue(text) => {
                    self.field.set_value(text.clone());
                    self.baseline.clone_from(&self.field.value);
                    self.previewed = false;
                    self.refilter();
                    self.highlighted = None;
                    self.refresh_open();
                    self.push_shared();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Expand => {
                    self.expand();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Collapse => {
                    self.dismiss();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Focus => EventResponse::CaptureFocus,
                _ => EventResponse::Ignored,
            },
            _ => self.forward_then_edit(cx),
        }
    }

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // Delegate to the inherent method so
        // `AutoComplete::sync_overlay` and the `Widget` trait seam
        // stay in lock-step.
        AutoComplete::sync_overlay(self, overlay);
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&self.field as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut self.field as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.field_rect)
    }
}

impl std::fmt::Debug for AutoComplete {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutoComplete")
            .field("label", &self.label)
            .field("value", &self.field.value)
            .field("enabled", &self.enabled)
            .field("open", &self.open)
            .field("filtered", &self.filtered.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(ac: &mut AutoComplete) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        ac.layout(&mut cx, Rect::new(10.0, 10.0, 160.0, 24.0));
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

    fn event(ac: &mut AutoComplete, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: ac.cached_bounds,
            scale: 1.0,
        };
        ac.event(&mut cx)
    }

    fn focus(ac: &mut AutoComplete) {
        event(ac, &WidgetEvent::FocusGained);
    }

    fn type_text(ac: &mut AutoComplete, text: &str) {
        event(
            ac,
            &WidgetEvent::ImeCommitted {
                text: text.to_string(),
            },
        );
    }

    #[test]
    fn substring_filter_is_case_insensitive() {
        let ac = AutoComplete::new()
            .suggestions(["Apple", "Pineapple", "Banana"])
            .with_value("APP");
        assert_eq!(ac.filtered().join(", "), "Apple, Pineapple");
    }

    #[test]
    fn prefix_filter_mode() {
        let ac = AutoComplete::new()
            .suggestions(["Apple", "Pineapple"])
            .with_value("app")
            .filter_mode(FilterMode::Prefix);
        assert_eq!(ac.filtered().join(", "), "Apple");
    }

    #[test]
    fn focus_with_text_opens_popup() {
        let mut ac = AutoComplete::new().suggestions(["Apple"]).with_value("a");
        laid_out(&mut ac);
        assert!(!ac.is_open());
        focus(&mut ac);
        assert!(ac.is_open());
    }

    #[test]
    fn min_chars_gates_open() {
        let mut ac = AutoComplete::new()
            .suggestions(["Apple", "Avocado"])
            .min_chars(2);
        laid_out(&mut ac);
        focus(&mut ac);
        type_text(&mut ac, "a");
        // One char below the 2-char minimum — still closed.
        assert!(!ac.is_open());
        type_text(&mut ac, "p");
        assert!(ac.is_open());
    }

    #[test]
    fn edits_open_and_refilter() {
        let mut ac = AutoComplete::new().suggestions(["Apple", "Avocado", "Banana"]);
        laid_out(&mut ac);
        focus(&mut ac);
        type_text(&mut ac, "a");
        assert!(ac.is_open());
        assert_eq!(ac.filtered().len(), 3);
        type_text(&mut ac, "p");
        assert_eq!(ac.filtered().join(", "), "Apple");
        assert!(ac.take_edited());
    }

    #[test]
    fn text_below_min_chars_closes() {
        let mut ac = AutoComplete::new().suggestions(["Apple"]);
        laid_out(&mut ac);
        focus(&mut ac);
        type_text(&mut ac, "a");
        assert!(ac.is_open());
        event(&mut ac, &key("Backspace"));
        assert!(!ac.is_open());
    }

    #[test]
    fn no_matches_closes() {
        let mut ac = AutoComplete::new().suggestions(["Apple"]);
        laid_out(&mut ac);
        focus(&mut ac);
        type_text(&mut ac, "z");
        assert!(!ac.is_open());
    }

    #[test]
    fn arrows_preview_and_clamp() {
        let mut ac = AutoComplete::new()
            .suggestions(["Apple", "Apricot", "Maple"])
            .with_value("ap");
        laid_out(&mut ac);
        focus(&mut ac);
        assert_eq!(ac.highlighted(), None);
        event(&mut ac, &key("ArrowDown"));
        assert_eq!(ac.highlighted(), Some(0));
        // Preview fills the field with the highlighted suggestion.
        assert_eq!(ac.value(), "Apple");
        event(&mut ac, &key("ArrowDown"));
        event(&mut ac, &key("ArrowDown"));
        event(&mut ac, &key("ArrowDown")); // clamps at last
        assert_eq!(ac.highlighted(), Some(2));
        assert_eq!(ac.value(), "Maple");
        event(&mut ac, &key("ArrowUp"));
        event(&mut ac, &key("ArrowUp"));
        event(&mut ac, &key("ArrowUp")); // clamps at first
        assert_eq!(ac.highlighted(), Some(0));
    }

    #[test]
    fn enter_commits_highlighted() {
        let mut ac = AutoComplete::new()
            .suggestions(["Apple", "Avocado"])
            .with_value("a");
        laid_out(&mut ac);
        focus(&mut ac);
        event(&mut ac, &key("ArrowDown"));
        event(&mut ac, &key("ArrowDown"));
        event(&mut ac, &key("Enter"));
        assert_eq!(ac.take_committed(), Some("Avocado".to_string()));
        assert_eq!(ac.value(), "Avocado");
        assert!(!ac.is_open());
    }

    #[test]
    fn enter_submits_raw_text() {
        let mut ac = AutoComplete::new().suggestions(["Apple"]).with_value("xyz");
        laid_out(&mut ac);
        focus(&mut ac);
        // "xyz" matches nothing — popup closed; Enter still submits.
        event(&mut ac, &key("Enter"));
        assert_eq!(ac.take_committed(), Some("xyz".to_string()));
    }

    #[test]
    fn tab_commits_and_falls_through() {
        let mut ac = AutoComplete::new()
            .suggestions(["Apple", "Avocado"])
            .with_value("a");
        laid_out(&mut ac);
        focus(&mut ac);
        event(&mut ac, &key("ArrowDown"));
        // Tab commits the highlight but stays `Ignored` so focus
        // traversal can move on.
        assert_eq!(event(&mut ac, &key("Tab")), EventResponse::Ignored);
        assert_eq!(ac.take_committed(), Some("Apple".to_string()));
        assert_eq!(ac.value(), "Apple");
    }

    #[test]
    fn escape_restores_preview_text() {
        let mut ac = AutoComplete::new()
            .suggestions(["Apple", "Avocado"])
            .with_value("av");
        laid_out(&mut ac);
        focus(&mut ac);
        event(&mut ac, &key("ArrowDown"));
        assert_eq!(ac.value(), "Avocado"); // previewed
        event(&mut ac, &key("Escape"));
        assert!(!ac.is_open());
        assert_eq!(ac.value(), "av"); // baseline restored
    }

    #[test]
    fn blur_closes_popup() {
        let mut ac = AutoComplete::new().suggestions(["Apple"]).with_value("a");
        laid_out(&mut ac);
        focus(&mut ac);
        assert!(ac.is_open());
        event(&mut ac, &WidgetEvent::FocusLost);
        assert!(!ac.is_open());
    }

    #[test]
    fn overlay_opens_listbox_below() {
        let mut ac = AutoComplete::new().suggestions(["Apple"]).with_value("a");
        laid_out(&mut ac);
        focus(&mut ac);
        let mut o = overlay();
        ac.sync_overlay(&mut o);
        o.layout_pass();
        assert_eq!(o.len(), 1);
        let b = o.entry_bounds(ac.popup_id.unwrap()).unwrap();
        // Below the field face.
        assert!(b.min_y() >= 34.0);
    }

    #[test]
    fn popup_option_click_commits() {
        let mut ac = AutoComplete::new()
            .suggestions(["Apple", "Avocado"])
            .with_value("a");
        laid_out(&mut ac);
        focus(&mut ac);
        let mut o = overlay();
        ac.sync_overlay(&mut o);
        o.layout_pass();
        let id = ac.popup_id.unwrap();
        // Reach the option at path [0, 0, 1] inside the popup and
        // press it.
        let option = o.widget_at_mut(id, &[0, 0, 1]).expect("option widget");
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(5.0, 5.0),
            button: PointerButton::Primary,
            count: 1,
        };
        let mut cx = EventContext {
            event: &press,
            bounds: Rect::default(),
            scale: 1.0,
        };
        assert_eq!(option.event(&mut cx), EventResponse::Handled);
        ac.sync_overlay(&mut o);
        assert_eq!(ac.take_committed(), Some("Avocado".to_string()));
        assert_eq!(ac.value(), "Avocado");
        assert!(!ac.is_open());
    }

    #[test]
    fn outside_press_dismisses_and_reconciles() {
        let mut ac = AutoComplete::new().suggestions(["Apple"]).with_value("a");
        laid_out(&mut ac);
        focus(&mut ac);
        let mut o = overlay();
        ac.sync_overlay(&mut o);
        o.layout_pass();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(o.dispatch_event(&press), EventResponse::Ignored);
        ac.sync_overlay(&mut o);
        assert!(!ac.is_open());
        assert_eq!(ac.popup_id, None);
    }

    #[test]
    fn refilter_rebuilds_live_popup() {
        let mut ac = AutoComplete::new()
            .suggestions(["Apple", "Apricot", "Banana"])
            .with_value("a");
        laid_out(&mut ac);
        focus(&mut ac);
        let mut o = overlay();
        ac.sync_overlay(&mut o);
        o.layout_pass();
        let id = ac.popup_id.unwrap();
        // Narrow the filter — the same entry id serves the shorter
        // list (replace_content, not close/reopen).
        type_text(&mut ac, "p");
        ac.sync_overlay(&mut o);
        assert_eq!(ac.popup_id, Some(id));
        o.layout_pass();
        // The rebuilt popup exposes exactly the filtered items.
        let column = o.widget_at_mut(id, &[0, 0]).expect("column");
        assert_eq!(column.child_count(), 2);
    }

    #[test]
    fn layer_escape_dismissal_restores_preview() {
        let mut ac = AutoComplete::new()
            .suggestions(["Apple", "Avocado"])
            .with_value("a");
        laid_out(&mut ac);
        focus(&mut ac);
        let mut o = overlay();
        ac.sync_overlay(&mut o);
        o.layout_pass();
        event(&mut ac, &key("ArrowDown"));
        assert_eq!(ac.value(), "Apple");
        // The layer consumes Escape and drops the entry; the next
        // sync reconciles and restores the baseline text.
        assert_eq!(o.dispatch_event(&key("Escape")), EventResponse::Handled);
        ac.sync_overlay(&mut o);
        assert!(!ac.is_open());
        assert_eq!(ac.value(), "a");
    }

    #[test]
    fn combobox_accessibility() {
        let mut ac = AutoComplete::new()
            .suggestions(["A"])
            .label("City")
            .with_value("a");
        laid_out(&mut ac);
        focus(&mut ac);
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        ac.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::ComboBox);
        assert_eq!(node.label(), Some("City"));
        assert_eq!(node.value(), Some("a"));
        assert_eq!(node.has_popup(), Some(accesskit::HasPopup::Listbox));
        assert_eq!(node.is_expanded(), Some(true));
        assert!(node.supports_action(accesskit::Action::Expand));
        assert!(node.supports_action(accesskit::Action::Collapse));
        assert_eq!(ac.child_count(), 1);
    }

    #[test]
    fn semantic_expand_collapse_set_value() {
        let mut ac = AutoComplete::new().suggestions(["Apple"]).with_value("a");
        laid_out(&mut ac);
        focus(&mut ac);
        event(
            &mut ac,
            &WidgetEvent::SemanticAction(SemanticAction::Expand),
        );
        assert!(ac.is_open());
        event(
            &mut ac,
            &WidgetEvent::SemanticAction(SemanticAction::Collapse),
        );
        assert!(!ac.is_open());
        event(
            &mut ac,
            &WidgetEvent::SemanticAction(SemanticAction::SetValue("ap".to_string())),
        );
        assert_eq!(ac.value(), "ap");
        assert!(ac.is_open());
    }

    #[test]
    fn disabled_ignores_events() {
        let mut ac = AutoComplete::new()
            .suggestions(["Apple"])
            .with_value("a")
            .enabled(false);
        laid_out(&mut ac);
        assert_eq!(event(&mut ac, &key("Enter")), EventResponse::Ignored);
        assert_eq!(
            event(&mut ac, &WidgetEvent::FocusGained),
            EventResponse::Ignored
        );
        assert!(!ac.is_open());
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        ac.accessibility(&mut node);
        assert!(node.is_disabled());
    }
}
