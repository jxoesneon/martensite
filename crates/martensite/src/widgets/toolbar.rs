//! `Toolbar` widget: a horizontal strip of action items — QToolBar /
//! NSToolbar / WinUI `CommandBar`.
//!
//! A toolbar packs action items left-to-right on a single row:
//! [`Button`]s added with [`item`](Toolbar::item), arbitrary widgets
//! with [`item_widget`](Toolbar::item_widget), visual
//! [`separator`](Toolbar::separator)s, and flexible
//! [`spacer`](Toolbar::spacer)s that push the following items to the
//! right edge (Qt `addStretch` / NSToolbar flexible space).
//!
//! When the allocated width cannot fit every item, the trailing items
//! collapse into a `»` overflow affordance pinned to the right edge.
//! Pressing it sets the one-shot flag drained by
//! [`take_overflow_open`](Toolbar::take_overflow_open) — the host then
//! opens a menu or popover listing the hidden items, which
//! [`overflowed_indices`](Toolbar::overflowed_indices) reports.
//! Overflowed items leave the child protocol entirely: they are not
//! painted, not hit-tested, and not emitted to the accessibility tree
//! (they belong to the popup the host shows).
//!
//! Item activation travels through [`take_activated`](Toolbar::take_activated),
//! which reports the item index of whichever embedded [`Button`]
//! fired — the same `take_*` signal seam `Button::take_activated`
//! exposes, polled per item. Items added via
//! [`item_widget`](Toolbar::item_widget) keep their own state seams
//! (a `Switch`'s `on`, a `Slider`'s `value`) — only `Button` items
//! report through `take_activated`.
//!
//! Items are real widget children: events, focus, and accessibility
//! nodes flow through the framework's internal-child protocol.
//! Pointer presses pick an internal key target, so non-positional
//! events (keys, IME) reach only the last-pressed item — the same
//! internal-focus model the dashboard's bespoke toolbar uses.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{Button, Toolbar};
//!
//! let bar = Toolbar::new()
//!     .item(Button::new("New"))
//!     .item(Button::new("Open"))
//!     .separator()
//!     .item(Button::new("Save"))
//!     .spacer()
//!     .item(Button::new("About"));
//! assert_eq!(bar.item_count(), 6);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, SemanticAction,
    Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

use crate::widgets::button::Button;
use crate::widgets::separator::Separator;

/// Horizontal padding between the strip edge and the first/last item.
const PAD_X: f32 = 8.0;
/// Vertical padding around the items.
const PAD_Y: f32 = 4.0;
/// Gap between adjacent items.
const GAP: f32 = 6.0;
/// Width of the `»` overflow affordance.
const OVERFLOW_W: f32 = 28.0;
/// Minimum strip height in logical points.
const MIN_H: f32 = 28.0;
/// Strip background fallback when the theme lacks a surface token.
const FACE: [u8; 4] = [238, 240, 244, 255];
/// Bottom hairline fallback.
const HAIRLINE: [u8; 4] = [140, 145, 155, 255];
/// Arena-focus ring fallback (translucent accent).
const FOCUS_RING: [u8; 4] = [60, 110, 220, 200];

/// One entry in a [`Toolbar`] row — a real widget child (button,
/// generic widget, or separator), or a flexible spacer with no child
/// of its own.
enum ToolbarEntry {
    /// A [`Button`] added via [`Toolbar::item`] — its activation is
    /// polled by [`Toolbar::take_activated`].
    Button(Button),
    /// An arbitrary widget added via [`Toolbar::item_widget`].
    Widget(Box<dyn Widget>),
    /// A [`Separator::vertical`] added via [`Toolbar::separator`].
    Separator(Separator),
    /// A flexible gap added via [`Toolbar::spacer`] — consumes an
    /// equal share of the leftover width; no widget child.
    Spacer,
}

impl ToolbarEntry {
    /// The widget this entry carries, if any.
    fn as_widget(&self) -> Option<&dyn Widget> {
        match self {
            Self::Button(b) => Some(b),
            Self::Widget(w) => Some(w.as_ref()),
            Self::Separator(s) => Some(s),
            Self::Spacer => None,
        }
    }

    /// Mutable form of [`as_widget`](Self::as_widget).
    fn as_widget_mut(&mut self) -> Option<&mut dyn Widget> {
        match self {
            Self::Button(b) => Some(b),
            Self::Widget(w) => Some(w.as_mut()),
            Self::Separator(s) => Some(s),
            Self::Spacer => None,
        }
    }

    /// Whether the entry consumes leftover width rather than having a
    /// fixed measured size.
    fn is_spacer(&self) -> bool {
        matches!(self, Self::Spacer)
    }
}

/// A horizontal strip of action items — QToolBar / NSToolbar / WinUI
/// `CommandBar`.
///
/// See the [module documentation](self) for the item model, overflow
/// collapse, and the activation seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Button, Toolbar};
///
/// let bar = Toolbar::new().item(Button::new("Run")).spacer().item(Button::new("Stop"));
/// assert_eq!(bar.item_count(), 3);
/// ```
pub struct Toolbar {
    /// Optional accessible label for the strip.
    pub label: Option<String>,
    /// Whether the bar accepts input.
    pub enabled: bool,
    /// The row entries in declaration order.
    entries: Vec<ToolbarEntry>,
    /// Cached per-entry measured sizes from the last measure pass.
    item_sizes: Vec<Vec2>,
    /// Per-entry rects from the last layout pass (zeroed for
    /// overflowed entries).
    item_bounds: Vec<Rect>,
    /// Item indices exposed through the child protocol — every
    /// widget-bearing entry that is currently visible, in order.
    visible_children: Vec<usize>,
    /// Item indices currently collapsed into the overflow affordance.
    overflowed: Vec<usize>,
    /// The `»` overflow affordance — a real [`Button`] surfaced as the
    /// last child while any item is overflowed, so it inherits press
    /// handling and an AT node for free.
    overflow: Button,
    /// Bounds assigned to the overflow affordance.
    overflow_rect: Rect,
    /// One-shot flag set when the overflow affordance activates —
    /// drained by [`take_overflow_open`](Self::take_overflow_open).
    overflow_open: bool,
    /// The last button activation not yet drained by
    /// [`take_activated`](Self::take_activated) — the signal-out seam.
    activated: Option<usize>,
    /// The child (in `child` index space) that receives
    /// non-positional events — set by the last pointer press, the
    /// internal-focus model.
    key_target: Option<usize>,
    /// Child currently holding a pointer press; while set, positional
    /// events forward to it regardless of hit position so captured
    /// drags (a slider thumb leaving its rect) are not dropped.
    press_target: Option<usize>,
    /// Whether arena focus currently rests on the strip — paints the
    /// focus ring.
    focused: bool,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter propagated to item labels. See
    /// [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Toolbar {
    /// Creates an empty strip; add entries with [`item`](Self::item),
    /// [`item_widget`](Self::item_widget), [`separator`](Self::separator),
    /// and [`spacer`](Self::spacer).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Toolbar;
    ///
    /// let bar = Toolbar::new();
    /// assert_eq!(bar.item_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            entries: Vec::new(),
            item_sizes: Vec::new(),
            item_bounds: Vec::new(),
            visible_children: Vec::new(),
            overflowed: Vec::new(),
            overflow: Button::new("»").tooltip("More items"),
            overflow_rect: Rect::default(),
            overflow_open: false,
            activated: None,
            key_target: None,
            press_target: None,
            focused: false,
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Appends a [`Button`] item. Its activation is reported through
    /// [`take_activated`](Self::take_activated) with this entry's item
    /// index.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Button, Toolbar};
    ///
    /// let bar = Toolbar::new().item(Button::new("Cut")).item(Button::new("Copy"));
    /// assert_eq!(bar.item_count(), 2);
    /// ```
    #[must_use]
    pub fn item(mut self, button: Button) -> Self {
        let button = match &self.text_painter {
            Some(p) => button.with_text_painter(p.clone()),
            None => button,
        };
        self.entries.push(ToolbarEntry::Button(button));
        self
    }

    /// Appends an arbitrary widget item — a `Switch`, `Slider`,
    /// `Dropdown`, or any other [`Widget`]. It is laid out at its
    /// measured size and rides the child protocol like any other
    /// item, but its activation does *not* flow through
    /// [`take_activated`](Self::take_activated) (which polls `Button`
    /// items only).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Switch, Toolbar};
    ///
    /// let bar = Toolbar::new().item_widget(Switch::new("live"));
    /// assert_eq!(bar.item_count(), 1);
    /// ```
    #[must_use]
    pub fn item_widget(mut self, widget: impl Widget + 'static) -> Self {
        self.entries.push(ToolbarEntry::Widget(Box::new(widget)));
        self
    }

    /// Inserts a vertical hairline separator between items — the same
    /// [`Separator`] widget used standalone, riding the child
    /// protocol.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Button, Toolbar};
    ///
    /// let bar = Toolbar::new().item(Button::new("A")).separator().item(Button::new("B"));
    /// assert_eq!(bar.item_count(), 3);
    /// ```
    #[must_use]
    pub fn separator(mut self) -> Self {
        self.entries
            .push(ToolbarEntry::Separator(Separator::vertical()));
        self
    }

    /// Inserts a flexible spacer that consumes an equal share of the
    /// leftover strip width with any other spacers, pushing following
    /// items toward the right edge — Qt `addStretch` / NSToolbar
    /// flexible space. A spacer carries no widget child.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Button, Toolbar};
    ///
    /// let bar = Toolbar::new().item(Button::new("Left")).spacer().item(Button::new("Right"));
    /// assert_eq!(bar.item_count(), 3);
    /// ```
    #[must_use]
    pub fn spacer(mut self) -> Self {
        self.entries.push(ToolbarEntry::Spacer);
        self
    }

    /// Sets the strip's accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Toolbar;
    ///
    /// let bar = Toolbar::new().label("Editor actions");
    /// assert_eq!(bar.label.as_deref(), Some("Editor actions"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets whether the strip accepts input.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Toolbar;
    ///
    /// let bar = Toolbar::new().enabled(false);
    /// assert!(!bar.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Number of entries in the strip — buttons, generic widgets,
    /// separators, and spacers alike.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Button, Toolbar};
    ///
    /// let bar = Toolbar::new().item(Button::new("A")).separator();
    /// assert_eq!(bar.item_count(), 2);
    /// ```
    #[inline]
    pub fn item_count(&self) -> usize {
        self.entries.len()
    }

    /// Item indices currently collapsed into the overflow affordance —
    /// the items the host should list in the popup it opens when
    /// [`take_overflow_open`](Self::take_overflow_open) reports a
    /// press. Empty while every item fits.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Button, Toolbar};
    ///
    /// let bar = Toolbar::new().item(Button::new("A"));
    /// assert!(bar.overflowed_indices().is_empty());
    /// ```
    #[inline]
    pub fn overflowed_indices(&self) -> &[usize] {
        &self.overflowed
    }

    /// Whether any item is currently collapsed into the overflow
    /// affordance.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Toolbar;
    ///
    /// assert!(!Toolbar::new().is_overflowing());
    /// ```
    #[inline]
    pub fn is_overflowing(&self) -> bool {
        !self.overflowed.is_empty()
    }

    /// Drains the one-shot overflow-open flag: `true` once per press
    /// of the `»` affordance. The host should respond by opening a
    /// menu or popover listing
    /// [`overflowed_indices`](Self::overflowed_indices).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Toolbar;
    ///
    /// let mut bar = Toolbar::new();
    /// assert!(!bar.take_overflow_open());
    /// ```
    #[inline]
    pub fn take_overflow_open(&mut self) -> bool {
        std::mem::take(&mut self.overflow_open)
    }

    /// Drains the last item activation: the *item index* (position in
    /// declaration order, separators and spacers included) of the
    /// [`Button`] that fired since the previous call. Poll per frame
    /// or after dispatching input — the same `take_*` seam
    /// `Dialog::take_response` uses.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Button, Toolbar};
    ///
    /// let mut bar = Toolbar::new().item(Button::new("Go"));
    /// assert_eq!(bar.take_activated(), None);
    /// ```
    #[inline]
    pub fn take_activated(&mut self) -> Option<usize> {
        self.activated.take()
    }

    /// Shares a [`crate::text_paint::TextPainter`] so item labels and
    /// the overflow glyph emit real glyph runs instead of `DrawText`
    /// placeholder boxes.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter.clone());
        self.overflow = Button::new("»")
            .tooltip("More items")
            .with_text_painter(painter.clone());
        self.entries = std::mem::take(&mut self.entries)
            .into_iter()
            .map(|e| match e {
                ToolbarEntry::Button(b) => {
                    ToolbarEntry::Button(b.with_text_painter(painter.clone()))
                }
                other => other,
            })
            .collect();
        self
    }

    /// Drains embedded-`Button` activation flags into
    /// [`take_activated`](Self::take_activated), and the overflow
    /// affordance into [`take_overflow_open`](Self::take_overflow_open).
    /// Called automatically from `event`, `layout`, and `a11y_prepare`
    /// — public so hosts that reach children through
    /// `child_mut` directly can fold the marks in afterwards, the same
    /// contract [`Segmented::poll_pending`](crate::widgets::Segmented)
    /// documents.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Button, Toolbar};
    /// use martensite_core::widget::Widget;
    ///
    /// let mut bar = Toolbar::new().item(Button::new("Go"));
    /// // Simulate an AT Click on the button through the child protocol.
    /// {
    ///     let mut hot = martensite_core::HotNode::default();
    ///     let mut lcx = martensite_core::LayoutContext { hot: &mut hot, scale: 1.0 };
    ///     bar.layout(&mut lcx, martensite_core::Rect::new(0.0, 0.0, 400.0, 40.0));
    ///     let ev = martensite_core::WidgetEvent::SemanticAction(
    ///         martensite_core::SemanticAction::Click,
    ///     );
    ///     let button = bar.child_mut(0).unwrap();
    ///     let mut cx = martensite_core::EventContext {
    ///         event: &ev,
    ///         bounds: martensite_core::Rect::new(0.0, 0.0, 80.0, 32.0),
    ///         scale: 1.0,
    ///     };
    ///     button.event(&mut cx);
    /// }
    /// bar.poll_signals();
    /// assert_eq!(bar.take_activated(), Some(0));
    /// ```
    pub fn poll_signals(&mut self) {
        for (i, entry) in self.entries.iter_mut().enumerate() {
            if let ToolbarEntry::Button(button) = entry {
                if button.take_activated() {
                    self.activated = Some(i);
                }
            }
        }
        if self.overflow.take_activated() {
            self.overflow_open = true;
        }
    }

    /// Refreshes `item_sizes` when the entry list changed since the
    /// last measure — layout tolerates being called without a prior
    /// measure pass (the test-harness path).
    fn ensure_sizes(&mut self, cx: &mut LayoutContext) {
        if self.item_sizes.len() == self.entries.len() {
            return;
        }
        self.item_sizes.clear();
        for entry in &mut self.entries {
            let size = match entry.as_widget_mut() {
                Some(w) => w.measure(
                    cx,
                    LayoutConstraints {
                        min_size: Vec2::ZERO,
                        max_size: Vec2::new(f32::INFINITY, f32::INFINITY),
                    },
                ),
                None => Vec2::ZERO,
            };
            self.item_sizes.push(size);
        }
    }

    /// Width an entry occupies in the fit pass — spacers measure as
    /// zero and absorb leftover space afterwards.
    fn entry_width(&self, index: usize) -> f32 {
        if self.entries[index].is_spacer() {
            0.0
        } else {
            self.item_sizes.get(index).copied().unwrap_or(Vec2::ZERO).x
        }
    }

    /// The index of the first entry that does not fit within `limit`
    /// (an absolute x coordinate), or `entries.len()` when everything
    /// fits. Spacers never trigger a cut themselves.
    fn fit_cut(&self, inner_min_x: f32, limit: f32, gap: f32) -> usize {
        let mut cursor = inner_min_x;
        let mut placed = false;
        for i in 0..self.entries.len() {
            let w = self.entry_width(i);
            let advance = w + if placed { gap } else { 0.0 };
            if !self.entries[i].is_spacer() && cursor + advance > limit + 0.5 {
                return i;
            }
            cursor += advance;
            placed = true;
        }
        self.entries.len()
    }

    /// Forwards a non-positional event to the current key target only —
    /// the internal-focus contract (default forwarding would broadcast
    /// keys to every item, arming every button at once).
    fn forward_key(&mut self, cx: &mut EventContext) -> EventResponse {
        let Some(i) = self.key_target else {
            return EventResponse::Ignored;
        };
        let Some(bounds) = self.child_bounds(i) else {
            return EventResponse::Ignored;
        };
        let Some(child) = self.child_mut(i) else {
            return EventResponse::Ignored;
        };
        let mut child_cx = EventContext {
            event: cx.event,
            bounds,
            scale: cx.scale,
        };
        child.event(&mut child_cx)
    }
}

impl Default for Toolbar {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Toolbar {
    fn debug_name(&self) -> &'static str {
        "Toolbar"
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let pad_x = cx.pt(PAD_X);
        let pad_y = cx.pt(PAD_Y);
        let gap = cx.pt(GAP);
        let tight = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: constraints.max_size,
        };
        self.item_sizes.clear();
        let mut content_w = pad_x * 2.0;
        let mut max_h = 0.0f32;
        let mut visible_w = 0usize;
        for entry in &mut self.entries {
            let size = match entry.as_widget_mut() {
                Some(w) => w.measure(cx, tight),
                None => Vec2::ZERO,
            };
            self.item_sizes.push(size);
            if !entry.is_spacer() {
                if visible_w > 0 {
                    content_w += gap;
                }
                content_w += size.x;
                max_h = max_h.max(size.y);
                visible_w += 1;
            }
        }
        let h = (max_h + 2.0 * pad_y)
            .max(cx.pt(MIN_H))
            .min(constraints.max_size.y.max(0.0));
        // Fill the offered width when it is bounded (the docked-strip
        // convention); an unbounded width hugs the content.
        let w = if constraints.max_size.x.is_finite() {
            constraints.max_size.x.max(0.0)
        } else {
            content_w
        };
        Vec2::new(w, h)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.poll_signals();
        self.cached_bounds = bounds;
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
        self.ensure_sizes(cx);
        self.item_bounds.clear();
        self.item_bounds.resize(self.entries.len(), Rect::default());
        self.visible_children.clear();
        self.overflowed.clear();
        self.overflow_rect = Rect::default();
        if self.entries.is_empty() || bounds.size.x <= 0.0 {
            return;
        }

        let pad_x = cx.pt(PAD_X);
        let pad_y = cx.pt(PAD_Y);
        let gap = cx.pt(GAP);
        let overflow_w = cx.pt(OVERFLOW_W);
        let inner = Rect::new(
            bounds.min_x() + pad_x,
            bounds.min_y() + pad_y,
            (bounds.size.x - 2.0 * pad_x).max(0.0),
            (bounds.size.y - 2.0 * pad_y).max(0.0),
        );

        // Fit pass: the first entry whose right edge would cross the
        // limit starts the overflow set — everything after it is
        // hidden too (the overflow set is always a suffix). When
        // anything overflows, reserve the `»` affordance at the right
        // edge and refit; the smaller limit can only grow the set.
        let mut limit = inner.max_x();
        let mut cut = self.fit_cut(inner.min_x(), limit, gap);
        if cut < self.entries.len() {
            limit = inner.max_x() - overflow_w - gap;
            cut = self.fit_cut(inner.min_x(), limit, gap);
            self.overflowed.extend(cut..self.entries.len());
        }
        let has_overflow = !self.overflowed.is_empty();

        // Leftover width after the visible fixed items is shared
        // equally among the visible spacers.
        let spacer_count = self.entries[..cut].iter().filter(|e| e.is_spacer()).count();
        let fixed_end = {
            let mut cursor = inner.min_x();
            let mut placed = false;
            for i in 0..cut {
                if placed {
                    cursor += gap;
                }
                cursor += self.entry_width(i);
                placed = true;
            }
            cursor
        };
        let leftover = (limit - fixed_end).max(0.0);
        let spacer_w = if spacer_count > 0 {
            leftover / spacer_count as f32
        } else {
            0.0
        };

        // Placement pass: assign each visible entry its rect and lay
        // out its widget child; overflowed entries keep zeroed rects
        // and stay out of the child protocol entirely.
        let mut cursor = inner.min_x();
        let mut placed = false;
        for (i, entry) in self.entries.iter_mut().enumerate().take(cut) {
            if placed {
                cursor += gap;
            }
            let w = if entry.is_spacer() {
                spacer_w
            } else {
                self.item_sizes.get(i).copied().unwrap_or(Vec2::ZERO).x
            };
            let rect = Rect::new(cursor, inner.min_y(), w.max(0.0), inner.size.y);
            self.item_bounds[i] = rect;
            if let Some(widget) = entry.as_widget_mut() {
                self.visible_children.push(i);
                cx.layout_child(widget, rect);
            }
            cursor += w.max(0.0);
            placed = true;
        }

        if has_overflow {
            self.overflow_rect = Rect::new(
                inner.max_x() - overflow_w,
                inner.min_y(),
                overflow_w,
                inner.size.y,
            );
            cx.layout_child(&mut self.overflow, self.overflow_rect);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Toolbar);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        node.set_orientation(accesskit::Orientation::Horizontal);
        node.add_action(accesskit::Action::Focus);
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn a11y_prepare(&mut self) {
        self.poll_signals();
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        let response = match cx.event {
            WidgetEvent::SemanticAction(SemanticAction::Focus | SemanticAction::Click) => {
                EventResponse::CaptureFocus
            }
            WidgetEvent::FocusGained => {
                self.focused = true;
                // Let the internal key target observe the focus move —
                // a text field shows its caret only while it holds
                // real focus.
                self.forward_key(cx);
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                self.forward_key(cx);
                EventResponse::RequestRepaint
            }
            _ if cx.event.position().is_some() => {
                let pos = cx.event.position().expect("checked");
                // A held press keeps the event stream: drag moves and
                // the release forward to `press_target` even when the
                // pointer leaves every item rect. Everything else
                // hit-tests topmost-first over the visible children.
                let n = self.child_count();
                let hit = if matches!(cx.event, WidgetEvent::PointerPressed { .. }) {
                    (0..n)
                        .rev()
                        .find(|&i| self.child_bounds(i).is_some_and(|b| b.contains(pos)))
                } else {
                    self.press_target.or_else(|| {
                        (0..n)
                            .rev()
                            .find(|&i| self.child_bounds(i).is_some_and(|b| b.contains(pos)))
                    })
                };
                let Some(i) = hit else {
                    return EventResponse::Ignored;
                };
                if matches!(cx.event, WidgetEvent::PointerPressed { .. }) {
                    self.key_target = Some(i);
                    self.press_target = Some(i);
                }
                let bounds = self.child_bounds(i).unwrap_or_default();
                let response = match self.child_mut(i) {
                    Some(child) => {
                        let mut child_cx = EventContext {
                            event: cx.event,
                            bounds,
                            scale: cx.scale,
                        };
                        child.event(&mut child_cx)
                    }
                    None => EventResponse::Ignored,
                };
                // The release ends the hold after the child sees it —
                // the child needs it to answer `ReleasePointer`.
                if matches!(cx.event, WidgetEvent::PointerReleased { .. }) {
                    self.press_target = None;
                }
                response
            }
            // Non-positional: internal focus decides.
            _ => self.forward_key(cx),
        };
        self.poll_signals();
        response
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        cx.list
            .push_fill_rect(rect, cx.color(TokenKey::SurfaceColor, FACE));
        // Bottom hairline separating the strip from the content below.
        let y = f64::from(b.max_y()) - 1.0;
        cx.list.push_fill_rect(
            kurbo::Rect::new(f64::from(b.min_x()), y, f64::from(b.max_x()), y + 1.0),
            cx.color(TokenKey::DividerColor, HAIRLINE),
        );
        // Arena-focus ring on the strip itself (internal items carry
        // their own indicators).
        if self.focused {
            let accent = cx.color(TokenKey::AccentColor, FOCUS_RING);
            cx.list.push_stroke_rect(
                kurbo::Rect::new(rect.x0 + 1.0, rect.y0 + 1.0, rect.x1 - 1.0, rect.y1 - 1.0),
                cx.pt(1.5),
                [accent[0], accent[1], accent[2], FOCUS_RING[3]],
            );
        }
    }

    fn child_count(&self) -> usize {
        self.visible_children.len() + usize::from(!self.overflowed.is_empty())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index < self.visible_children.len() {
            let item = self.visible_children[index];
            self.entries.get(item).and_then(ToolbarEntry::as_widget)
        } else if index == self.visible_children.len() && !self.overflowed.is_empty() {
            Some(&self.overflow)
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index < self.visible_children.len() {
            let item = self.visible_children[index];
            self.entries
                .get_mut(item)
                .and_then(ToolbarEntry::as_widget_mut)
        } else if index == self.visible_children.len() && !self.overflowed.is_empty() {
            Some(&mut self.overflow)
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index < self.visible_children.len() {
            let item = self.visible_children[index];
            self.item_bounds.get(item).copied()
        } else if index == self.visible_children.len() && !self.overflowed.is_empty() {
            Some(self.overflow_rect)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for Toolbar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Toolbar")
            .field("items", &self.entries.len())
            .field("overflowed", &self.overflowed)
            .field("enabled", &self.enabled)
            .field("focused", &self.focused)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;
    use martensite_core::PointerButton;

    fn bar() -> Toolbar {
        Toolbar::new()
            .item(Button::new("New"))
            .item(Button::new("Open"))
            .separator()
            .item(Button::new("Save"))
    }

    fn laid_out(bar: &mut Toolbar, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        bar.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn measured(bar: &mut Toolbar, w: f32, h: f32) -> Vec2 {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        bar.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        )
    }

    fn event(bar: &mut Toolbar, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: bar.cached_bounds,
            scale: 1.0,
        };
        bar.event(&mut cx)
    }

    fn press(x: f32, y: f32) -> WidgetEvent {
        WidgetEvent::PointerPressed {
            position: Vec2::new(x, y),
            button: PointerButton::Primary,
            count: 1,
        }
    }

    fn release(x: f32, y: f32) -> WidgetEvent {
        WidgetEvent::PointerReleased {
            position: Vec2::new(x, y),
            button: PointerButton::Primary,
        }
    }

    #[test]
    fn builder_collects_entries() {
        let bar = bar().spacer().item(Button::new("About"));
        assert_eq!(bar.item_count(), 6);
        assert_eq!(bar.child_count(), 0); // not laid out yet
    }

    #[test]
    fn measure_fills_width_and_floors_height() {
        let mut bar = bar();
        let size = measured(&mut bar, 600.0, 40.0);
        assert_eq!(size.x, 600.0);
        assert!(size.y >= 28.0);
    }

    #[test]
    fn layout_packs_items_left_to_right() {
        let mut bar = bar();
        measured(&mut bar, 600.0, 40.0);
        laid_out(&mut bar, 600.0, 40.0);
        let b0 = bar.child_bounds(0).unwrap();
        let b1 = bar.child_bounds(1).unwrap();
        assert_eq!(b0.min_x(), 8.0);
        assert!(b1.min_x() > b0.max_x());
        assert_eq!(bar.child_count(), 4);
    }

    #[test]
    fn spacer_pushes_following_items_right() {
        let mut bar = Toolbar::new()
            .item(Button::new("Left"))
            .spacer()
            .item(Button::new("Right"));
        measured(&mut bar, 600.0, 40.0);
        laid_out(&mut bar, 600.0, 40.0);
        // The last button hugs the right padding edge.
        let last = bar.item_bounds[2];
        assert!((last.max_x() - (600.0 - 8.0)).abs() < 0.5);
        // The spacer consumed the middle.
        let spacer_rect = bar.item_bounds[1];
        assert!(spacer_rect.size.x > 300.0);
        // Spacer carries no child — only the two buttons are visible.
        assert_eq!(bar.child_count(), 2);
    }

    #[test]
    fn overflow_collapses_trailing_items() {
        let mut bar = bar(); // 3 buttons (80pt each) + separator (9pt)
        measured(&mut bar, 140.0, 40.0);
        laid_out(&mut bar, 140.0, 40.0);
        assert!(bar.is_overflowing());
        // The overflow set is a suffix.
        let idx = bar.overflowed_indices();
        assert!(!idx.is_empty());
        assert_eq!(*idx.last().unwrap(), bar.item_count() - 1);
        // Overflowed items leave the child protocol; the » button is
        // appended as the last child.
        let last_child = bar.child(bar.child_count() - 1).unwrap();
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        last_child.accessibility(&mut node);
        assert_eq!(node.label(), Some("»"));
    }

    #[test]
    fn overflow_press_sets_open_flag() {
        let mut bar = bar();
        measured(&mut bar, 140.0, 40.0);
        laid_out(&mut bar, 140.0, 40.0);
        assert!(bar.is_overflowing());
        let r = bar.overflow_rect;
        assert!(r.size.x > 0.0);
        let cx_pt = Vec2::new(r.min_x() + r.size.x / 2.0, r.min_y() + r.size.y / 2.0);
        event(&mut bar, &press(cx_pt.x, cx_pt.y));
        event(&mut bar, &release(cx_pt.x, cx_pt.y));
        assert!(bar.take_overflow_open());
        assert!(!bar.take_overflow_open()); // drained
    }

    #[test]
    fn take_activated_reports_item_index() {
        let mut bar = bar();
        measured(&mut bar, 600.0, 40.0);
        laid_out(&mut bar, 600.0, 40.0);
        // Press + release inside the "Open" button (item index 1).
        let b = bar.item_bounds[1];
        let x = b.min_x() + 4.0;
        let y = b.min_y() + 4.0;
        event(&mut bar, &press(x, y));
        event(&mut bar, &release(x, y));
        assert_eq!(bar.take_activated(), Some(1));
        assert_eq!(bar.take_activated(), None);
    }

    #[test]
    fn separator_is_a_real_child() {
        let mut bar = bar();
        measured(&mut bar, 600.0, 40.0);
        laid_out(&mut bar, 600.0, 40.0);
        // Child order: New, Open, Separator, Save.
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        bar.child(2).unwrap().accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::GenericContainer);
    }

    #[test]
    fn key_target_receives_keys() {
        let mut bar = bar();
        measured(&mut bar, 600.0, 40.0);
        laid_out(&mut bar, 600.0, 40.0);
        // Click item 0 to make it the key target, then press Enter —
        // the button activates on key release.
        let b = bar.item_bounds[0];
        event(&mut bar, &press(b.min_x() + 4.0, b.min_y() + 4.0));
        event(&mut bar, &release(b.min_x() + 4.0, b.min_y() + 4.0));
        let _ = bar.take_activated();
        let enter_down = WidgetEvent::KeyPressed {
            key: "Enter".to_string(),
            repeat: false,
        };
        let enter_up = WidgetEvent::KeyReleased {
            key: "Enter".to_string(),
        };
        event(&mut bar, &enter_down);
        event(&mut bar, &enter_up);
        assert_eq!(bar.take_activated(), Some(0));
    }

    #[test]
    fn accessibility_role_label_orientation() {
        let bar = bar().label("File actions");
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        bar.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Toolbar);
        assert_eq!(node.label(), Some("File actions"));
        assert_eq!(node.orientation(), Some(accesskit::Orientation::Horizontal));
        assert!(node.supports_action(accesskit::Action::Focus));
    }

    #[test]
    fn disabled_ignores_events() {
        let mut bar = bar().enabled(false);
        measured(&mut bar, 600.0, 40.0);
        laid_out(&mut bar, 600.0, 40.0);
        let b = bar.item_bounds[0];
        assert_eq!(
            event(&mut bar, &press(b.min_x() + 4.0, b.min_y() + 4.0)),
            EventResponse::Ignored
        );
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        bar.accessibility(&mut node);
        assert!(node.is_disabled());
    }

    #[test]
    fn focus_ring_tracks_focus_events() {
        let mut bar = bar();
        laid_out(&mut bar, 600.0, 40.0);
        event(&mut bar, &WidgetEvent::FocusGained);
        assert!(bar.focused);
        event(&mut bar, &WidgetEvent::FocusLost);
        assert!(!bar.focused);
    }

    #[test]
    fn debug_format() {
        let bar = bar();
        let debug = format!("{:?}", bar);
        assert!(debug.contains("Toolbar"));
    }
}
