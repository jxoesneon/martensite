//! `ListView` widget: a virtualized, selectable list of string rows.
//!
//! Implements the core of the [APG listbox pattern](https://www.w3.org/WAI/ARIA/apg/patterns/listbox/):
//!
//! - `Role::List` on the widget; visible rows are emitted as internal
//!   `Role::ListItem` children with `selected`/`posinset` metadata —
//!   the same roving-tabindex contract `RadioGroup` uses, so platform
//!   focus stays a single stop on the list node.
//! - **Virtualized**: only the rows intersecting the viewport are
//!   pooled, laid out, painted, and emitted into the accessibility
//!   tree — a million-item list costs a viewport's worth of row
//!   widgets. `position_in_set`/`size_of_set` carry each item's real
//!   index, so AT sees honest positions inside the windowed emission.
//! - Scrolling is owned (wheel, keyboard, and a smart vertical
//!   scrollbar child) the way `ScrollView` manages its internals —
//!   `ScrollView` itself cannot be reused here because its content
//!   child is painted whole; row virtualization needs the offset at
//!   paint time.
//! - Keyboard: `ArrowUp`/`ArrowDown` move focus+selection,
//!   `PageUp`/`PageDown`/`Home`/`End` jump, `Enter` activates
//!   (drained via [`ListView::take_activated`]), printable characters
//!   typeahead-select, `Shift`+arrows extend the range in
//!   [`SelectionMode::Multiple`].
//! - Pointer: click selects, double-click activates, hover
//!   highlights, dragging extends the range in `Multiple` mode.
//!   With [`ListView::reorderable`], grabbing the selected row in
//!   `Single` mode starts a reorder drag (accent drop line; the
//!   completed `(from, to)` parks in [`ListView::take_moved`]).
//!
//! # Documented limitations
//!
//! - **Modifiers**: `WidgetEvent::KeyPressed` carries no modifier
//!   field; `Shift` arrives as its own key event and is tracked
//!   (`shift_held`, the `TextInput` pattern), so `Shift`+arrows/click
//!   extend the range in `Multiple` mode. `Ctrl`-click disjoint
//!   toggling is not supported — the range-based
//!   [`SelectionModel`] stores contiguous ranges, matching what the
//!   event vocabulary can express.
//! - **Items are strings**, not delegates — per-row rich content is
//!   intentionally out of scope.
//! - **AT windowing**: rows outside the viewport are not emitted;
//!   `posinset`/`setsize` give AT the full coordinate space.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::ListView;
//!
//! let list = ListView::new().items(["Apple", "Banana", "Cherry"]);
//! assert_eq!(list.item_count(), 3);
//! ```

use std::ops::Range;
use std::sync::{Arc, Mutex};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
pub use martensite_blessed::data_table::SelectionModel;
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, RenderMinimum, TokenKey, UnderflowPolicy};

/// Default row height in logical pixels.
const ROW_H: f32 = 24.0;
/// Maximum rows the widget asks for in `measure` before scrolling.
const MAX_VISIBLE_ROWS: f32 = 12.0;
/// Scrollbar thickness in logical pixels.
const BAR: f32 = 10.0;
/// Minimum scrollbar thumb length.
const MIN_THUMB: f32 = 24.0;
/// Track colour.
const TRACK_COLOR: [u8; 4] = [235, 237, 240, 255];
/// Thumb colour.
const THUMB_COLOR: [u8; 4] = [160, 166, 176, 255];
/// Thumb colour while dragged.
const THUMB_ACTIVE: [u8; 4] = [120, 126, 138, 255];
/// List face background.
const SURFACE_BG: [u8; 4] = [250, 250, 252, 255];
/// List border.
const BORDER: [u8; 4] = [150, 155, 165, 255];
/// Label ink.
const INK: [u8; 4] = [30, 30, 36, 255];
/// Disabled label ink.
const INK_DISABLED: [u8; 4] = [150, 150, 158, 255];
/// Selected-row ink (on the accent fill).
const INK_SELECTED: [u8; 4] = [255, 255, 255, 255];
/// Selection accent.
const ACCENT: [u8; 4] = [60, 110, 220, 255];
/// Focus ring alpha (the accent colour at 50%).
const FOCUS_RING: [u8; 4] = [60, 110, 220, 128];
/// Hover wash alpha.
const HOVER_ALPHA: u8 = 32;
/// Alternating-row stripe alpha.
const STRIPE_ALPHA: u8 = 12;

/// How a [`ListView`] manages its selection.
///
/// # Examples
///
/// ```
/// use martensite::widgets::SelectionMode;
///
/// assert_eq!(SelectionMode::default(), SelectionMode::Single);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum SelectionMode {
    /// Exactly one row is selected; selection follows the focus
    /// indicator (APG single-select listbox).
    #[default]
    Single,
    /// A contiguous range of rows may be selected: `Shift`+arrows,
    /// `Shift`+click, and pointer drags extend the range from its
    /// anchor. See the module-level note on modifiers — `Ctrl`-style
    /// disjoint multi-selection is not expressible today.
    Multiple,
}

/// A scroll request parked by a [`VScrollBar`] for the owner to apply.
#[derive(Copy, Clone, Debug)]
pub(crate) enum BarRequest {
    /// Scroll by this delta (logical pixels).
    By(f32),
    /// Scroll to this absolute offset.
    To(f32),
}

/// A vertical scrollbar strip — the internal child of the facade's
/// virtualized views.
///
/// Mirrors `scrollview::ScrollBarWidget` (whose constructor and
/// mirrored fields are private to that module). Its fields are mirrors
/// of the owner's scroll state, refreshed on every change; AT actions
/// it receives are parked in `pending` and applied by the owner via
/// its `poll_pending`.
pub(crate) struct VScrollBar {
    /// Whether the bar is shown (content overflows the viewport).
    pub(crate) shown: bool,
    /// Current vertical offset.
    pub(crate) offset: f32,
    /// Maximum scrollable offset.
    pub(crate) max_offset: f32,
    /// Parked AT action for the owner to apply.
    pub(crate) pending: Option<BarRequest>,
    /// Thumb rect within the bar, mirrored by the owner for painting.
    pub(crate) thumb: Option<Rect>,
    /// Whether the thumb is being dragged (mirrored from the owner).
    pub(crate) active: bool,
    /// Display scale from `layout` — scroll steps are logical pt.
    pub(crate) scale: f32,
}

impl VScrollBar {
    pub(crate) fn new() -> Self {
        Self {
            shown: false,
            offset: 0.0,
            max_offset: 0.0,
            pending: None,
            thumb: None,
            active: false,
            scale: 1.0,
        }
    }
}

impl Widget for VScrollBar {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(cx.pt(BAR), 0.0).min(constraints.max_size.max(Vec2::ZERO))
    }

    fn layout(&mut self, cx: &mut LayoutContext, _bounds: Rect) {
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ScrollBar);
        node.set_orientation(accesskit::Orientation::Vertical);
        node.set_numeric_value(f64::from(self.offset));
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(f64::from(self.max_offset));
        node.set_numeric_value_step(f64::from(ROW_H * self.scale));
        node.add_action(accesskit::Action::ScrollUp);
        node.add_action(accesskit::Action::ScrollDown);
        node.add_action(accesskit::Action::SetScrollOffset);
        if !self.shown {
            node.set_hidden();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        let WidgetEvent::SemanticAction(action) = cx.event else {
            return EventResponse::Ignored;
        };
        let line = ROW_H * self.scale;
        let request = match action {
            SemanticAction::ScrollUp => BarRequest::By(-line),
            SemanticAction::ScrollDown => BarRequest::By(line),
            SemanticAction::SetScrollOffset(offset) => BarRequest::To(offset.y),
            _ => return EventResponse::Ignored,
        };
        self.pending = Some(request);
        EventResponse::Handled
    }

    fn paint(&self, cx: &mut PaintContext) {
        if !self.shown {
            return;
        }
        let b = cx.bounds;
        let track = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        cx.list
            .push_fill_rect(track, cx.color(TokenKey::DividerColor, TRACK_COLOR));
        if let Some(thumb) = self.thumb {
            let t = kurbo::Rect::new(
                f64::from(thumb.min_x()),
                f64::from(thumb.min_y()),
                f64::from(thumb.max_x()),
                f64::from(thumb.max_y()),
            );
            cx.list.push_fill_shape(
                t,
                &Shape::PILL,
                if self.active {
                    cx.color(TokenKey::TextMutedColor, THUMB_ACTIVE)
                } else {
                    cx.color(TokenKey::BorderColor, THUMB_COLOR)
                },
            );
        }
    }
}

/// One visible row inside a [`ListView`].
///
/// Rows are *pooled*: the widget keeps exactly enough `ListItemRow`s to
/// cover the viewport and re-points them at items as the list scrolls.
/// Each is emitted as an internal child with `Role::ListItem`; its
/// fields mirror list state via [`ListView::sync_rows`]. AT actions it
/// receives are parked and applied by the owner via
/// [`ListView::poll_pending`].
struct ListItemRow {
    /// Item index this row currently presents.
    item_index: usize,
    /// The item's accessible label.
    label: String,
    /// Whether the item is selected (mirrored from the owner).
    selected: bool,
    /// Whether this row carries the roving tabindex.
    focused: bool,
    /// Whether the owning list holds keyboard focus.
    has_focus: bool,
    /// Whether the pointer is over this row.
    hovered: bool,
    /// Whether this row paints the alternating stripe.
    alternate: bool,
    /// Whether the owner is enabled.
    enabled: bool,
    /// Parked `SemanticAction::Click` / pointer press for the owner.
    press_pending: bool,
    /// Parked `SemanticAction::Focus` for the owner.
    focus_pending: bool,
    /// Shared shaped-text painter from the owning `ListView`.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl ListItemRow {
    fn new() -> Self {
        Self {
            item_index: 0,
            label: String::new(),
            selected: false,
            focused: false,
            has_focus: false,
            hovered: false,
            alternate: false,
            enabled: true,
            press_pending: false,
            focus_pending: false,
            text_painter: None,
        }
    }
}

impl Widget for ListItemRow {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(0.0),
            cx.pt(ROW_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListItem);
        node.set_label(self.label.as_str());
        node.set_selected(self.selected);
        // `position_in_set` is zero-based (unlike ARIA's one-based
        // `aria-posinset`); `size_of_set` lives on the List container.
        node.set_position_in_set(self.item_index);
        node.add_action(accesskit::Action::Click);
        // Roving tabindex: only the row holding the tab stop
        // advertises Focus — the list is a single tab stop.
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
            }
            | WidgetEvent::SemanticAction(SemanticAction::Click) => {
                // Park the press; the owning list applies it via
                // `poll_pending` so selection stays centralized.
                self.press_pending = true;
                EventResponse::CaptureFocus
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => {
                self.focus_pending = true;
                EventResponse::CaptureFocus
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
        let accent = cx.color(TokenKey::AccentColor, ACCENT);
        if self.selected {
            cx.list.push_fill_rect(rect, accent);
        } else if self.hovered {
            // Translucent wash of the accent colour.
            let wash = [accent[0], accent[1], accent[2], HOVER_ALPHA];
            cx.list.push_fill_rect(rect, wash);
        } else if self.alternate {
            let muted = cx.color(TokenKey::TextMutedColor, INK_DISABLED);
            cx.list
                .push_fill_rect(rect, [muted[0], muted[1], muted[2], STRIPE_ALPHA]);
        }
        if self.focused && self.has_focus {
            let wash = [accent[0], accent[1], accent[2], FOCUS_RING[3]];
            cx.list.push_stroke_rect(rect, cx.pt(2.0), wash);
        }
        // `DrawText` positions by the run's top edge — centre the font
        // box inside the row, clipped so a long label can't spill.
        let font_px = cx.pt(14.0);
        let ink = if self.selected {
            cx.color(TokenKey::TextInverseColor, INK_SELECTED)
        } else if self.enabled {
            cx.color(TokenKey::TextColor, INK)
        } else {
            cx.color(TokenKey::TextMutedColor, INK_DISABLED)
        };
        let text_x = b.min_x() + cx.pt(8.0);
        crate::text_paint::paint_label_clipped(
            crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
            cx.list,
            kurbo::Rect::new(
                f64::from(text_x),
                f64::from(b.min_y()),
                f64::from(b.max_x() - cx.pt(8.0)),
                f64::from(b.max_y()),
            ),
            kurbo::Point::new(
                f64::from(text_x),
                f64::from(b.min_y() + (b.height() - font_px) / 2.0),
            ),
            &self.label,
            font_px,
            ink,
        );
    }
}

/// A virtualized, selectable list of string rows.
///
/// The widget owns its scroll state and a smart vertical scrollbar;
/// the row pool (internal `Role::ListItem` children) covers only the
/// visible window, so item count does not bound the per-frame cost.
///
/// Selection out-seams mirror the codebase's conventions: state is
/// read back through [`selected`](Self::selected)/
/// [`selection`](Self::selection), and activations (`Enter`,
/// double-click) are drained via [`take_activated`](Self::take_activated)
/// or mirrored into an [`activated_sink`](Self::activated_sink) shared
/// cell — the same `Dialog::take_response`/`response_sink` pattern.
///
/// # Examples
///
/// ```
/// use martensite::widgets::ListView;
///
/// let mut list = ListView::new().items(["A", "B", "C"]);
/// list.set_selected(1);
/// assert_eq!(list.selected(), Some(1));
/// ```
pub struct ListView {
    /// Whether the list accepts input.
    pub enabled: bool,
    /// Optional accessible label for the list.
    pub label: Option<String>,
    /// Whether odd rows paint the alternating stripe.
    pub alternating_rows: bool,
    /// The selection mode (`Single` or range `Multiple`).
    pub selection_mode: SelectionMode,
    /// The item labels.
    items: Vec<String>,
    /// Row height in logical points. Prefer
    /// [`set_row_height`](Self::set_row_height) — it rescales the
    /// scroll offset so the top row is preserved; direct assignment
    /// simply clamps the offset at the next layout.
    pub row_height: f32,
    /// The range-based selection model (shared with `DataTable`).
    selection: SelectionModel,
    /// Index of the row carrying the roving tabindex.
    focused: usize,
    /// Whether the widget holds keyboard focus (for the focus ring).
    has_focus: bool,
    /// Whether `Shift` is currently held (tracked via key events).
    shift_held: bool,
    /// Hovered item index.
    hovered: Option<usize>,
    /// Vertical scroll offset in device pixels.
    scroll_y: f32,
    /// Pooled visible-window row children.
    rows: Vec<ListItemRow>,
    /// The vertical scrollbar (last internal child).
    vbar: VScrollBar,
    /// Item index activated since the last `take_activated`.
    activated: Option<usize>,
    /// Shared cell also receiving activations — the observation seam.
    activated_sink: Option<Arc<Mutex<Option<usize>>>>,
    /// Typeahead buffer (printable characters typed while focused).
    typeahead: String,
    /// Cached widget bounds.
    cached_bounds: Rect,
    /// Row viewport (widget bounds minus the shown bar).
    viewport: Rect,
    /// Vertical bar rect when shown.
    vbar_rect: Option<Rect>,
    /// Thumb-drag state: grab offset inside the thumb.
    thumb_drag: Option<f32>,
    /// Whether a press-drag on rows is in flight (drag selection).
    dragging: bool,
    /// Whether grabbing the *selected* row starts a reorder drag —
    /// the Qt `InternalMove` / macOS row-drag idiom (`Single`
    /// selection mode only; `Multiple` drags extend the range).
    /// Completed moves park `(from, to)` in [`take_moved`](Self::take_moved).
    pub reorderable: bool,
    /// Reorder drag in flight: `(from, drop_index)` where
    /// `drop_index` is the pre-removal insertion boundary `0..=len`.
    reorder_drag: Option<(usize, usize)>,
    /// Parked `(from, to)` reorder for `take_moved`.
    moved: Option<(usize, usize)>,
    /// Display scale from `layout` — row height, bar, thumbs are
    /// logical pt.
    scale: f32,
    /// Shared shaped-text painter — propagated to rows in `sync_rows`.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl ListView {
    /// Creates an empty list; add items with [`items`](Self::items) or
    /// [`set_items`](Self::set_items).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let list = ListView::new();
    /// assert_eq!(list.item_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            enabled: true,
            label: None,
            alternating_rows: false,
            selection_mode: SelectionMode::Single,
            items: Vec::new(),
            row_height: ROW_H,
            selection: SelectionModel::new(),
            focused: 0,
            has_focus: false,
            shift_held: false,
            hovered: None,
            scroll_y: 0.0,
            rows: Vec::new(),
            vbar: VScrollBar::new(),
            activated: None,
            activated_sink: None,
            typeahead: String::new(),
            cached_bounds: Rect::default(),
            viewport: Rect::default(),
            vbar_rect: None,
            thumb_drag: None,
            dragging: false,
            reorderable: false,
            reorder_drag: None,
            moved: None,
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Sets the item labels (builder form of
    /// [`set_items`](Self::set_items)).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let list = ListView::new().items(["One", "Two"]);
    /// assert_eq!(list.item_count(), 2);
    /// ```
    #[must_use]
    pub fn items(mut self, items: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.set_items(items);
        self
    }

    /// Replaces the item labels. Selection, focus, and scroll offset
    /// are clamped into the new range.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let mut list = ListView::new().items(["A", "B"]);
    /// list.set_items(["C"]);
    /// assert_eq!(list.item_count(), 1);
    /// ```
    pub fn set_items(&mut self, items: impl IntoIterator<Item = impl Into<String>>) {
        self.items = items.into_iter().map(Into::into).collect();
        let n = self.items.len();
        self.focused = self.focused.min(n.saturating_sub(1));
        self.hovered = self.hovered.filter(|&i| i < n);
        // Clamp stored ranges into the new bounds — the model holds
        // storage indices, so a shrink could leave stale selections.
        let clamped: Vec<Range<usize>> = self
            .selection
            .ranges()
            .iter()
            .map(|r| r.start.min(n)..r.end.min(n))
            .filter(|r| !r.is_empty())
            .collect();
        self.selection.clear();
        for range in clamped {
            self.selection.select_range(range);
        }
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());
        self.sync_rows();
        self.sync_bars();
    }

    /// The number of items.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// assert_eq!(ListView::new().items(["A", "B"]).item_count(), 2);
    /// ```
    #[inline]
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// The label of item `index`, if in range.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let list = ListView::new().items(["A", "B"]);
    /// assert_eq!(list.item(1), Some("B"));
    /// assert_eq!(list.item(9), None);
    /// ```
    #[inline]
    pub fn item(&self, index: usize) -> Option<&str> {
        self.items.get(index).map(String::as_str)
    }

    /// Sets the row height in logical points.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let list = ListView::new().row_height(32.0);
    /// assert_eq!(list.row_height, 32.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn row_height(mut self, height: f32) -> Self {
        self.set_row_height(height);
        self
    }

    /// Sets the row height in logical points (mutating form).
    /// Rescales the scroll offset so the top row is preserved across
    /// the change; assigning `row_height` directly skips the rescale
    /// (the offset simply clamps at the next layout).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let mut list = ListView::new();
    /// list.set_row_height(18.0);
    /// assert_eq!(list.row_height, 18.0);
    /// ```
    pub fn set_row_height(&mut self, height: f32) {
        // Preserve the top row across the change (the DataTable rule:
        // offsets are pixels, so rescale on a row-height change).
        let new = if height.is_finite() {
            height.max(1.0)
        } else {
            ROW_H
        };
        let old_px = self.row_height * self.scale;
        let new_px = new * self.scale;
        if old_px > 0.0 && new_px > 0.0 && new_px != old_px {
            self.scroll_y *= new_px / old_px;
        }
        self.row_height = new;
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());
        self.sync_rows();
        self.sync_bars();
    }

    /// Sets the selection mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ListView, SelectionMode};
    ///
    /// let list = ListView::new().selection_mode(SelectionMode::Multiple);
    /// assert_eq!(list.selection_mode, SelectionMode::Multiple);
    /// ```
    #[inline]
    #[must_use]
    pub fn selection_mode(mut self, mode: SelectionMode) -> Self {
        self.selection_mode = mode;
        self
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let l = ListView::new().label("Fruit");
    /// assert_eq!(l.label.as_deref(), Some("Fruit"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets whether the list accepts input.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let l = ListView::new().enabled(false);
    /// assert!(!l.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self.sync_rows();
        self
    }

    /// Sets whether odd rows paint the alternating stripe.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let l = ListView::new().alternating_rows(true);
    /// assert!(l.alternating_rows);
    /// ```
    #[inline]
    #[must_use]
    pub fn alternating_rows(mut self, alternating: bool) -> Self {
        self.alternating_rows = alternating;
        self.sync_rows();
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so row labels emit
    /// real glyph runs instead of `DrawText` placeholder boxes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let l = ListView::new().with_text_painter(shared_painter());
    /// ```
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self.sync_rows();
        self
    }

    /// Wires a shared cell that receives each activated index — the
    /// observation seam for hosts that cannot poll
    /// [`take_activated`](Self::take_activated) (mirrors
    /// `Dialog::response_sink`).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::{Arc, Mutex};
    /// use martensite::widgets::ListView;
    ///
    /// let sink = Arc::new(Mutex::new(None));
    /// let l = ListView::new().activated_sink(sink.clone());
    /// assert!(sink.lock().unwrap().is_none());
    /// ```
    #[must_use]
    pub fn activated_sink(mut self, sink: Arc<Mutex<Option<usize>>>) -> Self {
        self.activated_sink = Some(sink);
        self
    }

    /// The "active end" of the selection — the last index selected or
    /// extended to — or `None` when nothing is selected.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let mut l = ListView::new().items(["A", "B"]);
    /// assert_eq!(l.selected(), None);
    /// l.set_selected(1);
    /// assert_eq!(l.selected(), Some(1));
    /// ```
    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.selection.ranges().last().map(|r| r.end - 1)
    }

    /// The range-based selection model.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let l = ListView::new().items(["A", "B"]);
    /// assert!(l.selection().is_empty());
    /// ```
    #[inline]
    pub fn selection(&self) -> &SelectionModel {
        &self.selection
    }

    /// Whether `index` is selected.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let mut l = ListView::new().items(["A", "B"]);
    /// l.set_selected(0);
    /// assert!(l.is_selected(0));
    /// assert!(!l.is_selected(1));
    /// ```
    #[inline]
    pub fn is_selected(&self, index: usize) -> bool {
        self.selection.is_selected(index)
    }

    /// Every selected index, in order.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let mut l = ListView::new().items(["A", "B", "C"]);
    /// l.set_selected(1);
    /// assert_eq!(l.selected_indices(), vec![1]);
    /// ```
    pub fn selected_indices(&self) -> Vec<usize> {
        self.selection
            .ranges()
            .iter()
            .flat_map(|r| r.clone())
            .collect()
    }

    /// Selects `index` (single-select semantics — the selection's
    /// anchor moves too). Out-of-range indices are ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let mut l = ListView::new().items(["A", "B", "C"]);
    /// l.set_selected(2);
    /// assert_eq!(l.selected(), Some(2));
    /// l.set_selected(9);
    /// assert_eq!(l.selected(), Some(2));
    /// ```
    pub fn set_selected(&mut self, index: usize) {
        self.select_index(index);
    }

    /// Clears the selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let mut l = ListView::new().items(["A"]);
    /// l.set_selected(0);
    /// l.clear_selection();
    /// assert_eq!(l.selected(), None);
    /// ```
    pub fn clear_selection(&mut self) {
        self.selection.clear();
        self.sync_rows();
    }

    /// The index of the row carrying the roving tabindex.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// assert_eq!(ListView::new().items(["A"]).focused_index(), 0);
    /// ```
    #[inline]
    pub fn focused_index(&self) -> usize {
        self.focused
    }

    /// The current clamped vertical scroll offset in device pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// assert_eq!(ListView::new().scroll_offset(), 0.0);
    /// ```
    #[inline]
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_y
    }

    /// Maximum scroll offset: `max(0, content - viewport)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// assert_eq!(ListView::new().max_scroll_offset(), 0.0);
    /// ```
    #[inline]
    pub fn max_scroll_offset(&self) -> f32 {
        self.max_scroll()
    }

    /// Sets the scroll offset in device pixels, clamped to
    /// `0..=max_scroll_offset`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let mut l = ListView::new();
    /// l.set_scroll_offset(-10.0);
    /// assert_eq!(l.scroll_offset(), 0.0); // clamped
    /// ```
    pub fn set_scroll_offset(&mut self, offset: f32) {
        self.set_scroll(offset);
    }

    /// Scrolls by `delta` device pixels, clamped. Returns the
    /// actually-applied delta — `0.0` means nothing was consumed
    /// (chaining boundary for an ancestor scroll region).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let mut l = ListView::new();
    /// assert_eq!(l.scroll_by(10.0), 0.0); // nothing to scroll
    /// ```
    pub fn scroll_by(&mut self, delta: f32) -> f32 {
        let old = self.scroll_y;
        self.set_scroll(old + delta);
        self.scroll_y - old
    }

    /// Scrolls the minimum amount that makes row `index` fully visible.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let mut l = ListView::new().items(["A", "B"]);
    /// l.scroll_row_into_view(1);
    /// ```
    pub fn scroll_row_into_view(&mut self, index: usize) {
        self.ensure_visible(index);
    }

    /// The range of item indices intersecting the viewport — the
    /// window the row pool materializes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let l = ListView::new().items(["A", "B"]);
    /// assert_eq!(l.visible_range(), 0..0); // not laid out yet
    /// ```
    pub fn visible_range(&self) -> Range<usize> {
        let row_px = self.row_px();
        if row_px <= 0.0 || self.items.is_empty() || self.viewport.height() <= 0.0 {
            return 0..0;
        }
        let start = self.first_visible();
        let end = ((self.scroll_y + self.viewport.height()) / row_px).ceil() as usize;
        start..end.min(self.items.len())
    }

    /// Returns the activated item index once, if any — the
    /// `Dialog::take_response` out-seam for `Enter` and double-click
    /// activations.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let mut l = ListView::new().items(["A"]);
    /// assert_eq!(l.take_activated(), None);
    /// ```
    pub fn take_activated(&mut self) -> Option<usize> {
        self.activated.take()
    }

    /// Drain the parked `(from, to)` reorder — one-shot (see
    /// [`reorderable`](Self::reorderable)).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    ///
    /// let mut l = ListView::new().items(["A", "B"]);
    /// assert_eq!(l.take_moved(), None);
    /// ```
    pub fn take_moved(&mut self) -> Option<(usize, usize)> {
        self.moved.take()
    }

    /// Applies pending presses/focus moves recorded by row children and
    /// scroll requests parked by the scrollbar (AT actions delivered
    /// through `WidgetArena::internal_widget_mut`).
    ///
    /// Called automatically from `event`, `layout`, and `a11y_prepare`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ListView;
    /// use martensite_core::widget::Widget;
    ///
    /// let mut l = ListView::new().items(["A", "B"]);
    /// l.poll_pending();
    /// assert_eq!(l.selected(), None);
    /// ```
    pub fn poll_pending(&mut self) {
        let mut press = None;
        let mut focus = None;
        for row in &mut self.rows {
            if row.press_pending {
                row.press_pending = false;
                press = Some(row.item_index);
            } else if row.focus_pending {
                row.focus_pending = false;
                focus = Some(row.item_index);
            }
        }
        if let Some(i) = press {
            self.select_index(i);
        } else if let Some(i) = focus {
            // Focus without selection (a press also moves focus).
            self.focused = i.min(self.items.len().saturating_sub(1));
            self.ensure_visible(self.focused);
            self.sync_rows();
        }
        if let Some(req) = self.vbar.pending.take() {
            match req {
                BarRequest::By(d) => {
                    self.scroll_by(d);
                }
                BarRequest::To(o) => {
                    self.set_scroll(o);
                }
            }
        }
    }

    /// Row height in device pixels at the cached display scale.
    fn row_px(&self) -> f32 {
        self.row_height * self.scale
    }

    /// Full content height in device pixels.
    fn content_height(&self) -> f32 {
        self.items.len() as f32 * self.row_px()
    }

    /// Maximum scroll offset.
    fn max_scroll(&self) -> f32 {
        (self.content_height() - self.viewport.height()).max(0.0)
    }

    /// First fully-or-partially visible item index.
    fn first_visible(&self) -> usize {
        let row_px = self.row_px();
        if row_px <= 0.0 || self.items.is_empty() {
            return 0;
        }
        ((self.scroll_y / row_px).floor() as usize).min(self.items.len() - 1)
    }

    /// How many pooled rows cover the viewport (plus one partial row).
    fn visible_capacity(&self) -> usize {
        let row_px = self.row_px();
        if row_px <= 0.0 || self.viewport.height() <= 0.0 {
            return 0;
        }
        (self.viewport.height() / row_px).ceil() as usize + 1
    }

    /// The screen-space rect of item `index` (may be partially or
    /// fully outside the viewport).
    fn row_rect(&self, index: usize) -> Rect {
        let row_px = self.row_px();
        Rect::new(
            self.viewport.min_x(),
            self.viewport.min_y() + index as f32 * row_px - self.scroll_y,
            self.viewport.width(),
            row_px,
        )
    }

    /// Item index under `position` (viewport-space hit test).
    fn row_at(&self, position: Vec2) -> Option<usize> {
        let row_px = self.row_px();
        if row_px <= 0.0 || !self.viewport.contains(position) {
            return None;
        }
        let i = ((position.y - self.viewport.min_y() + self.scroll_y) / row_px) as usize;
        (i < self.items.len()).then_some(i)
    }

    /// Insertion boundary under `position` for a reorder drag —
    /// `0..=items.len()`. The top half of a row drops *before* it,
    /// the bottom half *after* it; outside the viewport clamps to
    /// the nearest end.
    fn drop_index_at(&self, position: Vec2) -> usize {
        let row_px = self.row_px();
        if row_px <= 0.0 || self.items.is_empty() {
            return 0;
        }
        let raw = (position.y - self.viewport.min_y() + self.scroll_y) / row_px;
        let i = if raw < 0.0 {
            0.0
        } else {
            raw.floor() + if raw.fract() >= 0.5 { 1.0 } else { 0.0 }
        };
        (i as usize).min(self.items.len())
    }

    /// Sets the scroll offset, clamped; mirrors state onto rows/bars.
    fn set_scroll(&mut self, offset: f32) {
        let clamped = if offset.is_finite() { offset } else { 0.0 };
        self.scroll_y = clamped.clamp(0.0, self.max_scroll());
        self.sync_rows();
        self.sync_bars();
    }

    /// Scrolls the minimum amount that makes row `index` fully visible.
    fn ensure_visible(&mut self, index: usize) {
        let row_px = self.row_px();
        if row_px <= 0.0 {
            return;
        }
        let top = index as f32 * row_px;
        let bottom = top + row_px;
        if top < self.scroll_y {
            self.set_scroll(top);
        } else if bottom > self.scroll_y + self.viewport.height() {
            self.set_scroll(bottom - self.viewport.height());
        }
    }

    /// Single-select `index` and move the roving tabindex to it.
    fn select_index(&mut self, index: usize) {
        if index < self.items.len() {
            self.focused = index;
            self.selection.select(index);
            self.ensure_visible(index);
            self.sync_rows();
        }
    }

    /// Extends the selection range from its anchor to `index` and
    /// moves the roving tabindex (the `Multiple`-mode extend gesture).
    fn extend_selection_to(&mut self, index: usize) {
        if index < self.items.len() {
            self.focused = index;
            self.selection.extend_to(index);
            self.ensure_visible(index);
            self.sync_rows();
        }
    }

    /// Keyboard navigation: move the roving tabindex to `index`
    /// (clamped), honouring the selection mode and `Shift` state, and
    /// keep it visible.
    fn move_focus_to(&mut self, index: usize) {
        let n = self.items.len();
        if n == 0 {
            return;
        }
        self.focused = index.min(n - 1);
        if self.selection_mode == SelectionMode::Multiple && self.shift_held {
            self.selection.extend_to(self.focused);
        } else {
            self.selection.select(self.focused);
        }
        self.ensure_visible(self.focused);
        self.sync_rows();
    }

    /// Rows moved by `PageUp`/`PageDown`.
    fn page_size(&self) -> usize {
        let row_px = self.row_px();
        if row_px <= 0.0 {
            return 1;
        }
        ((self.viewport.height() / row_px).floor() as usize).max(1)
    }

    /// Records an activation for [`take_activated`](Self::take_activated)
    /// and the optional sink.
    fn activate(&mut self, index: usize) {
        if index < self.items.len() {
            self.activated = Some(index);
            if let Some(ref sink) = self.activated_sink {
                *sink.lock().expect("activated sink poisoned") = Some(index);
            }
        }
    }

    /// Typeahead: searches items for the accumulated buffer, starting
    /// after the focus indicator and wrapping (APG "type to select"),
    /// falling back to the last character so repeats cycle.
    fn typeahead_select(&mut self, c: char) -> bool {
        self.typeahead.push(c.to_ascii_lowercase());
        let n = self.items.len();
        if n == 0 {
            return false;
        }
        let buffer = self.typeahead.clone();
        // A multi-char buffer may legitimately stay on the current
        // match (offset 0); a single char must start after it so
        // repeated presses cycle through matches (APG typeahead).
        let first_start = usize::from(buffer.len() == 1);
        for (probe, start) in [
            (buffer.as_str(), first_start),
            (&buffer[buffer.len() - 1..], 1usize),
        ] {
            for offset in start..start + n {
                let i = (self.focused + offset) % n;
                if self.items[i].to_ascii_lowercase().starts_with(probe) {
                    self.focused = i;
                    self.selection.select(i);
                    self.ensure_visible(i);
                    self.sync_rows();
                    return true;
                }
            }
        }
        false
    }

    /// The vertical scrollbar thumb rect, if the bar is shown.
    fn vbar_thumb(&self) -> Option<Rect> {
        let track = self.vbar_rect?;
        let content = self.content_height();
        if content <= 0.0 {
            return None;
        }
        let track_len = track.height();
        let frac = (self.viewport.height() / content).clamp(0.0, 1.0);
        let thumb_len = (track_len * frac)
            .max(MIN_THUMB * self.scale)
            .min(track_len);
        let max = self.max_scroll();
        let t = if max > 0.0 { self.scroll_y / max } else { 0.0 };
        let top = track.min_y() + t * (track_len - thumb_len);
        Some(Rect::new(track.min_x(), top, track.width(), thumb_len))
    }

    /// Maps a pointer position inside the bar track to a thumb grab or
    /// a page scroll. Returns `true` if the press was consumed.
    fn press_bar(&mut self, position: Vec2) -> bool {
        let Some(thumb) = self.vbar_thumb() else {
            return false;
        };
        if thumb.contains(position) {
            self.thumb_drag = Some(position.y - thumb.min_y());
        } else {
            // Track press: page toward the click.
            let sign = if position.y < thumb.min_y() {
                -1.0
            } else {
                1.0
            };
            self.scroll_by(sign * self.viewport.height() * 0.9);
        }
        true
    }

    /// Continues an active thumb drag.
    fn drag_thumb(&mut self, position: Vec2) {
        let Some(grab) = self.thumb_drag else {
            return;
        };
        let Some(track) = self.vbar_rect else {
            return;
        };
        let thumb_len = self.vbar_thumb().map(|t| t.height()).unwrap_or(0.0);
        let usable = (track.height() - thumb_len).max(f32::EPSILON);
        let frac = ((position.y - track.min_y() - grab) / usable).clamp(0.0, 1.0);
        self.set_scroll(frac * self.max_scroll());
    }

    /// Resizes/re-points the pooled row children at the visible window
    /// and mirrors owner state onto them.
    fn sync_rows(&mut self) {
        let first = self.first_visible();
        let count = self
            .items
            .len()
            .saturating_sub(first)
            .min(self.visible_capacity());
        self.rows.resize_with(count, ListItemRow::new);
        for (k, row) in self.rows.iter_mut().enumerate() {
            let i = first + k;
            row.item_index = i;
            row.label = self.items.get(i).cloned().unwrap_or_default();
            row.selected = self.selection.is_selected(i);
            row.focused = i == self.focused;
            row.has_focus = self.has_focus;
            row.hovered = self.hovered == Some(i);
            row.alternate = self.alternating_rows && i % 2 == 1;
            row.enabled = self.enabled;
            row.text_painter = self.text_painter.clone();
        }
    }

    /// Mirrors scroll state onto the scrollbar child for its emitted
    /// `ScrollBar` node and thumb painting.
    fn sync_bars(&mut self) {
        self.vbar.offset = self.scroll_y;
        self.vbar.max_offset = self.max_scroll();
        self.vbar.thumb = self.vbar_thumb();
        self.vbar.active = self.thumb_drag.is_some();
    }
}

impl Default for ListView {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ListView {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let row_px = cx.pt(self.row_height);
        let content_h = self.items.len() as f32 * row_px;
        let h = content_h.min(MAX_VISIBLE_ROWS * row_px);
        // Width follows the widest label (the dropdown `7pt/char`
        // estimate) plus row gutters and the potential scrollbar.
        let widest = self
            .items
            .iter()
            .map(|i| i.chars().count())
            .max()
            .unwrap_or(0) as f32;
        let w = widest * cx.pt(7.0) + cx.pt(16.0) + cx.pt(BAR);
        // `clamp` panics when min > max — cap the preferred minimum at
        // the constraint max so zero-constraint probes stay safe.
        let max_w = constraints.max_size.x.max(0.0);
        let max_h = constraints.max_size.y.max(0.0);
        Vec2::new(
            w.clamp(cx.pt(80.0).min(max_w), max_w),
            h.clamp(cx.pt(48.0).min(max_h), max_h),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        // The 48×48pt floor `measure` requests — a couple of rows plus
        // the scrollbar strip.
        RenderMinimum::new(Vec2::new(48.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.scale = cx.scale;
        self.vbar.scale = cx.scale;
        // Declare keyboard focusability on the arena node — the list
        // is a single tab stop (roving tabindex over the rows).
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
        self.poll_pending();
        // Smart scrollbar: shown only when content overflows.
        let bar = cx.pt(BAR);
        let show_v = self.content_height() > bounds.height();
        let mut viewport = bounds;
        self.vbar_rect = None;
        if show_v {
            viewport.size.x = (viewport.width() - bar).max(0.0);
            self.vbar_rect = Some(Rect::new(
                bounds.max_x() - bar,
                bounds.min_y(),
                bar,
                bounds.height(),
            ));
        }
        self.viewport = viewport;
        self.vbar.shown = show_v;
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());
        self.sync_rows();
        // Lay out the pooled rows so the child's layout pass runs with
        // current bounds (`child_bounds` derives them arithmetically).
        let row_px = cx.pt(self.row_height);
        let first = self.first_visible();
        for (k, row) in self.rows.iter_mut().enumerate() {
            let rect = Rect::new(
                viewport.min_x(),
                viewport.min_y() + (first + k) as f32 * row_px - self.scroll_y,
                viewport.width(),
                row_px,
            );
            cx.layout_child(row, rect);
        }
        if let Some(rect) = self.vbar_rect {
            cx.layout_child(&mut self.vbar, rect);
        }
        self.sync_bars();
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        node.set_orientation(accesskit::Orientation::Vertical);
        // `size_of_set` lives on the container (unlike ARIA's
        // per-item `aria-setsize`); items carry `position_in_set`.
        node.set_size_of_set(self.items.len());
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        if self.selection_mode == SelectionMode::Multiple {
            node.set_multiselectable();
        }
        let max = self.max_scroll();
        node.set_scroll_y(f64::from(self.scroll_y));
        node.set_scroll_y_min(0.0);
        node.set_scroll_y_max(f64::from(max));
        node.add_action(accesskit::Action::ScrollUp);
        node.add_action(accesskit::Action::ScrollDown);
        node.add_action(accesskit::Action::SetScrollOffset);
        node.add_action(accesskit::Action::Focus);
        node.add_child_action(accesskit::Action::ScrollIntoView);
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
                count,
            } => {
                if self.vbar_rect.is_some_and(|r| r.contains(*position)) {
                    self.press_bar(*position);
                    return EventResponse::CapturePointer;
                }
                if let Some(i) = self.row_at(*position) {
                    if *count >= 2 {
                        self.select_index(i);
                        self.activate(i);
                    } else if self.reorderable
                        && self.selection_mode == SelectionMode::Single
                        && self.selected() == Some(i)
                    {
                        // Grabbing the already-selected row starts a
                        // reorder drag instead of re-selecting.
                        self.reorder_drag = Some((i, i));
                        return EventResponse::CapturePointer;
                    } else if self.selection_mode == SelectionMode::Multiple && self.shift_held {
                        self.extend_selection_to(i);
                    } else {
                        self.select_index(i);
                    }
                    // Press-drags extend the range (Multiple) or move
                    // the selection (Single) — capture so tracking
                    // continues outside the bounds.
                    self.dragging = true;
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if self.thumb_drag.is_some() {
                    self.drag_thumb(*position);
                    return EventResponse::RequestRepaint;
                }
                if let Some((from, drop)) = self.reorder_drag {
                    let next = self.drop_index_at(*position);
                    if next != drop {
                        self.reorder_drag = Some((from, next));
                        self.sync_rows();
                    }
                    return EventResponse::RequestRepaint;
                }
                if self.dragging {
                    if let Some(i) = self.row_at(*position) {
                        if self.selection_mode == SelectionMode::Multiple {
                            self.extend_selection_to(i);
                        } else {
                            self.select_index(i);
                        }
                    }
                    return EventResponse::RequestRepaint;
                }
                let hov = self.row_at(*position);
                if hov != self.hovered {
                    self.hovered = hov;
                    self.sync_rows();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.thumb_drag.is_some() {
                    self.thumb_drag = None;
                    self.sync_bars();
                    return EventResponse::ReleasePointer;
                }
                if let Some((from, drop)) = self.reorder_drag.take() {
                    // `drop` counts positions in the *pre-removal*
                    // list — after `remove(from)` every boundary past
                    // `from` shifts down one.
                    let to = if drop > from { drop - 1 } else { drop };
                    if to != from && to <= self.items.len() {
                        let item = self.items.remove(from);
                        let to = to.min(self.items.len());
                        self.items.insert(to, item);
                        self.focused = to;
                        self.selection.select(to);
                        self.moved = Some((from, to));
                        self.sync_rows();
                    }
                    return EventResponse::ReleasePointer;
                }
                if self.dragging {
                    self.dragging = false;
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
                    self.sync_rows();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::Scroll { delta, .. } => {
                // Nested chaining: an unconsumed delta returns `Ignored`
                // so an ancestor scroll region can take it.
                let applied = self.scroll_by(delta.y);
                if applied != 0.0 {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "Shift" => {
                    // `KeyPressed` carries no modifier field — `Shift`
                    // arrives as its own key event (the `TextInput`
                    // pattern), tracked here for range extension.
                    self.shift_held = true;
                    EventResponse::Handled
                }
                "ArrowUp" => {
                    self.move_focus_to(self.focused.saturating_sub(1));
                    EventResponse::RequestRepaint
                }
                "ArrowDown" => {
                    self.move_focus_to(self.focused.saturating_add(1));
                    EventResponse::RequestRepaint
                }
                "PageUp" => {
                    self.move_focus_to(self.focused.saturating_sub(self.page_size()));
                    EventResponse::RequestRepaint
                }
                "PageDown" => {
                    self.move_focus_to(self.focused.saturating_add(self.page_size()));
                    EventResponse::RequestRepaint
                }
                "Home" => {
                    self.move_focus_to(0);
                    EventResponse::RequestRepaint
                }
                "End" => {
                    self.move_focus_to(self.items.len().saturating_sub(1));
                    EventResponse::RequestRepaint
                }
                "Enter" => {
                    self.activate(self.focused);
                    EventResponse::Handled
                }
                " " | "Space" => {
                    self.select_index(self.focused);
                    EventResponse::RequestRepaint
                }
                _ => {
                    if key.chars().count() == 1 {
                        let c = key.chars().next().unwrap_or('\0');
                        if c.is_ascii_alphanumeric() && self.typeahead_select(c) {
                            return EventResponse::RequestRepaint;
                        }
                    }
                    EventResponse::Ignored
                }
            },
            WidgetEvent::KeyReleased { key } if key == "Shift" => {
                self.shift_held = false;
                EventResponse::Handled
            }
            WidgetEvent::FocusGained => {
                self.has_focus = true;
                self.sync_rows();
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.has_focus = false;
                // The OS can swallow key releases on focus transitions —
                // don't leak a stuck Shift or drag into the next focus.
                self.shift_held = false;
                self.dragging = false;
                self.sync_rows();
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::Focus => EventResponse::CaptureFocus,
                SemanticAction::Click => {
                    self.activate(self.focused);
                    EventResponse::Handled
                }
                SemanticAction::ScrollUp => {
                    self.scroll_by(-self.row_px());
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollDown => {
                    self.scroll_by(self.row_px());
                    EventResponse::RequestRepaint
                }
                SemanticAction::SetScrollOffset(offset) => {
                    self.set_scroll(offset.y);
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollToPoint(point) => {
                    // Point is in this widget's coordinate space.
                    let i = ((point.y - self.viewport.min_y() + self.scroll_y)
                        / self.row_px().max(f32::EPSILON))
                    .max(0.0) as usize;
                    self.ensure_visible(i.min(self.items.len().saturating_sub(1)));
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollIntoView => {
                    self.ensure_visible(self.focused);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        // Chrome: the list face. Rows and the scrollbar paint through
        // the child walk, clipped to the widget bounds by
        // `clips_children`.
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        cx.list
            .push_fill_rect(rect, cx.color(TokenKey::SurfaceColor, SURFACE_BG));
        cx.list
            .push_stroke_rect(rect, cx.pt(1.0), cx.color(TokenKey::BorderColor, BORDER));
        // Reorder drop indicator — an accent line at the insertion
        // boundary (skipped for the no-op boundaries adjacent to the
        // dragged row).
        if let Some((from, drop)) = self.reorder_drag {
            if drop != from && drop != from + 1 {
                let row_px = self.row_px();
                let y = self.viewport.min_y() + drop as f32 * row_px - self.scroll_y;
                let t = cx.pt(1.0);
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(self.viewport.min_x()),
                        f64::from(y - t),
                        f64::from(self.viewport.max_x()),
                        f64::from(y + t),
                    ),
                    cx.color(TokenKey::AccentColor, [50, 115, 230, 255]),
                );
            }
        }
    }

    fn child_count(&self) -> usize {
        self.rows.len() + 1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index < self.rows.len() {
            self.rows.get(index).map(|r| r as &dyn Widget)
        } else if index == self.rows.len() {
            Some(&self.vbar)
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index < self.rows.len() {
            self.rows.get_mut(index).map(|r| r as &mut dyn Widget)
        } else if index == self.rows.len() {
            Some(&mut self.vbar)
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index < self.rows.len() {
            self.rows.get(index).map(|r| self.row_rect(r.item_index))
        } else if index == self.rows.len() {
            self.vbar_rect
        } else {
            None
        }
    }
}

impl std::fmt::Debug for ListView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListView")
            .field("items", &self.items.len())
            .field("selected", &self.selected())
            .field("focused", &self.focused)
            .field("scroll_y", &self.scroll_y)
            .field("selection_mode", &self.selection_mode)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(list: &mut ListView, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        list.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn make_view(items: usize, w: f32, h: f32) -> ListView {
        let mut l = ListView::new().items((0..items).map(|i| format!("Item {i}")));
        laid_out(&mut l, w, h);
        l
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn event(l: &mut ListView, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: l.cached_bounds,
            scale: 1.0,
        };
        l.event(&mut cx)
    }

    #[test]
    fn rows_virtualize_to_viewport() {
        // 10-row viewport: pool holds capacity rows only, not 10_000.
        let l = make_view(10_000, 200.0, 240.0);
        assert_eq!(l.visible_range(), 0..10);
        assert_eq!(l.rows.len(), 11); // capacity = ceil(240/24) + 1
        assert_eq!(l.item_count(), 10_000);
    }

    #[test]
    fn wheel_scrolls_and_clamps() {
        let mut l = make_view(100, 200.0, 96.0);
        assert_eq!(l.max_scroll_offset(), 100.0 * 24.0 - 96.0);
        let ev = WidgetEvent::Scroll {
            position: Vec2::new(10.0, 10.0),
            delta: Vec2::new(0.0, 48.0),
        };
        assert_eq!(event(&mut l, &ev), EventResponse::RequestRepaint);
        assert_eq!(l.scroll_offset(), 48.0);
        assert_eq!(l.visible_range().start, 2);
        // Unconsumed at the bottom → chains to an ancestor.
        l.set_scroll_offset(l.max_scroll_offset());
        assert_eq!(event(&mut l, &ev), EventResponse::Ignored);
    }

    #[test]
    fn arrows_move_focus_and_select() {
        let mut l = make_view(10, 200.0, 96.0);
        event(&mut l, &key("ArrowDown"));
        assert_eq!(l.focused_index(), 1);
        assert_eq!(l.selected(), Some(1));
        event(&mut l, &key("ArrowUp"));
        assert_eq!(l.selected(), Some(0));
        // No wraparound — the list clamps (APG listbox).
        event(&mut l, &key("ArrowUp"));
        assert_eq!(l.selected(), Some(0));
        event(&mut l, &key("End"));
        assert_eq!(l.selected(), Some(9));
        event(&mut l, &key("Home"));
        assert_eq!(l.selected(), Some(0));
    }

    #[test]
    fn page_keys_jump_by_viewport() {
        let mut l = make_view(100, 200.0, 96.0);
        event(&mut l, &key("PageDown"));
        assert_eq!(l.focused_index(), 4); // floor(96/24) rows per page
        event(&mut l, &key("PageUp"));
        assert_eq!(l.focused_index(), 0);
    }

    #[test]
    fn keyboard_nav_keeps_row_visible() {
        let mut l = make_view(100, 200.0, 96.0);
        event(&mut l, &key("PageDown"));
        event(&mut l, &key("PageDown"));
        assert_eq!(l.focused_index(), 8);
        // Focused row 8 must be inside the visible window.
        assert!(l.visible_range().contains(&8));
        assert!(l.scroll_offset() > 0.0);
    }

    #[test]
    fn enter_and_double_click_activate() {
        let mut l = make_view(10, 200.0, 96.0);
        l.set_selected(3);
        event(&mut l, &key("Enter"));
        assert_eq!(l.take_activated(), Some(3));
        assert_eq!(l.take_activated(), None);

        let press = |y: f32, count: u8| WidgetEvent::PointerPressed {
            position: Vec2::new(10.0, y),
            button: PointerButton::Primary,
            count,
        };
        assert_eq!(
            event(&mut l, &press(30.0, 2)),
            EventResponse::CapturePointer
        );
        assert_eq!(l.selected(), Some(1));
        assert_eq!(l.take_activated(), Some(1));
    }

    #[test]
    fn click_selects_row() {
        let mut l = make_view(10, 200.0, 96.0);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(10.0, 60.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(event(&mut l, &press), EventResponse::CapturePointer);
        assert_eq!(l.selected(), Some(2));
        assert_eq!(l.focused_index(), 2);
    }

    #[test]
    fn shift_arrows_extend_range_in_multiple() {
        let mut l = make_view(10, 200.0, 96.0).selection_mode(SelectionMode::Multiple);
        l.set_selected(2);
        event(&mut l, &key("Shift"));
        event(&mut l, &key("ArrowDown"));
        event(&mut l, &key("ArrowDown"));
        let release = WidgetEvent::KeyReleased {
            key: "Shift".to_string(),
        };
        event(&mut l, &release);
        assert_eq!(l.selected_indices(), vec![2, 3, 4]);
        assert_eq!(l.focused_index(), 4);
    }

    #[test]
    fn drag_extends_range_in_multiple() {
        let mut l = make_view(10, 200.0, 96.0).selection_mode(SelectionMode::Multiple);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(10.0, 10.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(event(&mut l, &press), EventResponse::CapturePointer);
        let moved = WidgetEvent::PointerMoved {
            position: Vec2::new(10.0, 70.0),
        };
        event(&mut l, &moved);
        assert_eq!(l.selected_indices(), vec![0, 1, 2]);
        let release_ev = WidgetEvent::PointerReleased {
            position: Vec2::new(10.0, 70.0),
            button: PointerButton::Primary,
        };
        assert_eq!(event(&mut l, &release_ev), EventResponse::ReleasePointer);
    }

    #[test]
    fn shift_click_extends_from_anchor() {
        let mut l = make_view(10, 200.0, 96.0).selection_mode(SelectionMode::Multiple);
        l.set_selected(1);
        event(&mut l, &key("Shift"));
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(10.0, 80.0), // row 3
            button: PointerButton::Primary,
            count: 1,
        };
        event(&mut l, &press);
        assert_eq!(l.selected_indices(), vec![1, 2, 3]);
    }

    #[test]
    fn typeahead_selects_item() {
        let mut l = ListView::new().items(["Apple", "Banana", "Cherry"]);
        laid_out(&mut l, 200.0, 96.0);
        event(&mut l, &key("c"));
        assert_eq!(l.selected(), Some(2));
        // Repeating the same letter cycles matches.
        let mut l = ListView::new().items(["Ant", "Ape", "Bat"]);
        laid_out(&mut l, 200.0, 96.0);
        l.set_selected(0);
        event(&mut l, &key("a"));
        assert_eq!(l.selected(), Some(1));
    }

    #[test]
    fn hover_highlights_row() {
        let mut l = make_view(10, 200.0, 96.0);
        let moved = WidgetEvent::PointerMoved {
            position: Vec2::new(10.0, 36.0),
        };
        assert_eq!(event(&mut l, &moved), EventResponse::RequestRepaint);
        assert_eq!(l.hovered, Some(1));
        let leave = WidgetEvent::PointerLeave;
        event(&mut l, &leave);
        assert_eq!(l.hovered, None);
    }

    #[test]
    fn scrollbar_shows_on_overflow_and_drags() {
        let mut l = make_view(100, 200.0, 96.0);
        let track = l.vbar_rect.expect("overflow shows the bar");
        // Short list: no bar.
        let short = make_view(2, 200.0, 96.0);
        assert!(short.vbar_rect.is_none());
        // Grab the thumb and drag to the bottom → max offset.
        let thumb = l.vbar_thumb().unwrap();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(track.min_x() + 5.0, thumb.min_y() + 5.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(event(&mut l, &press), EventResponse::CapturePointer);
        let moved = WidgetEvent::PointerMoved {
            position: Vec2::new(track.min_x() + 5.0, track.max_y() - 1.0),
        };
        event(&mut l, &moved);
        assert_eq!(l.scroll_offset(), l.max_scroll_offset());
    }

    #[test]
    fn list_accessibility_contract() {
        let mut l = make_view(10, 200.0, 96.0).label("Fruit");
        l.set_selected(1);
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        l.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::List);
        assert_eq!(node.label(), Some("Fruit"));
        assert_eq!(node.size_of_set(), Some(10));
        assert_eq!(node.scroll_y_max(), Some(240.0 - 96.0));
        assert!(node.supports_action(accesskit::Action::SetScrollOffset));

        // Row children emit Role::ListItem with selected/posinset.
        let mut item = AccessKitNode::new(accesskit::Role::Unknown);
        l.child(1).unwrap().accessibility(&mut item);
        assert_eq!(item.role(), accesskit::Role::ListItem);
        assert_eq!(item.is_selected(), Some(true));
        assert_eq!(item.position_in_set(), Some(1));

        // The scrollbar child emits Role::ScrollBar with the range.
        let bar_child = l.child(l.child_count() - 1).unwrap();
        let mut bar = AccessKitNode::new(accesskit::Role::Unknown);
        bar_child.accessibility(&mut bar);
        assert_eq!(bar.role(), accesskit::Role::ScrollBar);
        assert_eq!(bar.max_numeric_value(), Some(240.0 - 96.0));
    }

    #[test]
    fn roving_tabindex_single_focus_action() {
        let mut l = make_view(10, 200.0, 96.0);
        l.set_selected(2);
        let focus: Vec<bool> = (0..l.rows.len())
            .map(|i| {
                let mut n = AccessKitNode::new(accesskit::Role::Unknown);
                l.child(i).unwrap().accessibility(&mut n);
                n.supports_action(accesskit::Action::Focus)
            })
            .collect();
        assert_eq!(focus.iter().filter(|f| **f).count(), 1);
        assert!(focus[2]);
    }

    #[test]
    fn pending_press_from_row_child() {
        let mut l = make_view(10, 200.0, 96.0);
        let ev = WidgetEvent::SemanticAction(SemanticAction::Click);
        let row = l.child_mut(2).unwrap();
        let mut cx = EventContext {
            event: &ev,
            bounds: Rect::default(),
            scale: 1.0,
        };
        assert_eq!(row.event(&mut cx), EventResponse::CaptureFocus);
        l.poll_pending();
        assert_eq!(l.selected(), Some(2));
    }

    #[test]
    fn semantic_scroll_actions() {
        let mut l = make_view(100, 200.0, 96.0);
        let ev = WidgetEvent::SemanticAction(SemanticAction::ScrollDown);
        event(&mut l, &ev);
        assert_eq!(l.scroll_offset(), 24.0);
        let ev =
            WidgetEvent::SemanticAction(SemanticAction::SetScrollOffset(Vec2::new(0.0, 120.0)));
        event(&mut l, &ev);
        assert_eq!(l.scroll_offset(), 120.0);
    }

    #[test]
    fn set_items_clamps_selection_and_scroll() {
        let mut l = make_view(100, 200.0, 96.0);
        l.set_selected(50);
        l.set_scroll_offset(200.0);
        l.set_items(["Only"]);
        assert_eq!(l.selected(), None); // range clamped out of existence
        assert_eq!(l.scroll_offset(), 0.0);
        assert_eq!(l.focused_index(), 0);
    }

    #[test]
    fn disabled_ignores_input() {
        let mut l = make_view(10, 200.0, 96.0).enabled(false);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(10.0, 10.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(event(&mut l, &press), EventResponse::Ignored);
        assert_eq!(event(&mut l, &key("ArrowDown")), EventResponse::Ignored);
    }

    fn press_at(y: f32) -> WidgetEvent {
        WidgetEvent::PointerPressed {
            position: Vec2::new(10.0, y),
            button: PointerButton::Primary,
            count: 1,
        }
    }

    fn release_at(y: f32) -> WidgetEvent {
        WidgetEvent::PointerReleased {
            position: Vec2::new(10.0, y),
            button: PointerButton::Primary,
        }
    }

    #[test]
    fn reorder_drag_moves_selected_row_down() {
        let mut l = make_view(5, 200.0, 200.0);
        l.reorderable = true;
        l.set_selected(0);
        // Grab the selected row (y=12 → row 0).
        assert_eq!(
            event(&mut l, &press_at(12.0)),
            EventResponse::CapturePointer
        );
        // Bottom half of row 2 → drop boundary 3 → lands at index 2.
        event(
            &mut l,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(10.0, 2.0 * 24.0 + 18.0),
            },
        );
        assert_eq!(
            event(&mut l, &release_at(66.0)),
            EventResponse::ReleasePointer
        );
        assert_eq!(l.take_moved(), Some((0, 2)));
        assert_eq!(l.item(2), Some("Item 0"));
        assert_eq!(l.selected(), Some(2));
    }

    #[test]
    fn reorder_drag_moves_row_up() {
        let mut l = make_view(5, 200.0, 200.0);
        l.reorderable = true;
        l.set_selected(3);
        // Grab row 3 (y = 3*24 + 12).
        event(&mut l, &press_at(3.0 * 24.0 + 12.0));
        // Top half of row 0 → drop boundary 0.
        event(
            &mut l,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(10.0, 6.0),
            },
        );
        event(&mut l, &release_at(6.0));
        assert_eq!(l.take_moved(), Some((3, 0)));
        assert_eq!(l.item(0), Some("Item 3"));
    }

    #[test]
    fn reorder_press_on_unselected_row_selects_instead() {
        let mut l = make_view(5, 200.0, 200.0);
        l.reorderable = true;
        l.set_selected(0);
        // Press row 2 — not selected → normal selection, no reorder.
        event(&mut l, &press_at(2.0 * 24.0 + 12.0));
        assert_eq!(l.selected(), Some(2));
        event(&mut l, &release_at(2.0 * 24.0 + 12.0));
        assert_eq!(l.take_moved(), None);
    }

    #[test]
    fn reorder_noop_drop_keeps_order() {
        let mut l = make_view(5, 200.0, 200.0);
        l.reorderable = true;
        l.set_selected(1);
        event(&mut l, &press_at(24.0 + 12.0));
        // Release without moving — drop boundary is the row itself.
        event(&mut l, &release_at(24.0 + 12.0));
        assert_eq!(l.take_moved(), None);
        assert_eq!(l.item(1), Some("Item 1"));
    }

    #[test]
    fn multiple_mode_drag_still_extends_not_reorders() {
        let mut l = make_view(5, 200.0, 200.0).selection_mode(SelectionMode::Multiple);
        l.reorderable = true;
        l.set_selected(1);
        event(&mut l, &press_at(24.0 + 12.0));
        event(
            &mut l,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(10.0, 3.0 * 24.0 + 12.0),
            },
        );
        event(&mut l, &release_at(3.0 * 24.0 + 12.0));
        assert_eq!(l.take_moved(), None);
        assert_eq!(l.item(1), Some("Item 1"));
    }
}
