//! `StatusBar` widget: a bottom-of-window status strip — QStatusBar /
//! WPF `StatusBar`.
//!
//! A status bar is a single row split into three zones:
//!
//! - **Zone widgets** ([`add_zone`](StatusBar::add_zone)) — left-side
//!   widgets packed before the message, in declaration order.
//! - **The message** — a left-aligned text zone. A *permanent* message
//!   set with [`message`](StatusBar::message) /
//!   [`set_message`](StatusBar::set_message) shows by default; a
//!   *temporary* message posted with
//!   [`temporary`](StatusBar::temporary) displaces it until
//!   [`clear_temporary`](StatusBar::clear_temporary) runs — QStatusBar's
//!   `showMessage` / `currentMessage` semantics. Temporary text paints
//!   in the muted ink so the transient state reads at a glance.
//! - **Permanent widgets** ([`add_permanent`](StatusBar::add_permanent))
//!   — right-docked widgets (a small `ProgressBar`, a `Switch`, a
//!   `Spinner`) laid out right-to-left: the last added sits closest to
//!   the right edge, matching Qt's `addPermanentWidget`.
//!
//! Embedded widgets are real children: events, focus, and
//! accessibility nodes flow through the internal-child protocol in
//! *zone-first, then permanent* order. [`Button`]s added through
//! [`add_zone`](StatusBar::add_zone) / [`add_permanent`](StatusBar::add_permanent)
//! report activations through [`take_activated`](StatusBar::take_activated),
//! which returns the child index of whichever button fired.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{StatusBar, StatusItem, Text};
//!
//! let mut bar = StatusBar::new()
//!     .message("Ready")
//!     .add_zone(StatusItem::widget(Text::new("Ln 1, Col 1")));
//! bar.temporary("Saved");
//! assert_eq!(bar.current_message(), "Saved");
//! bar.clear_temporary();
//! assert_eq!(bar.current_message(), "Ready");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, SemanticAction,
    Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

use crate::widgets::button::Button;

/// Horizontal padding between the strip edge and its end zones.
const PAD_X: f32 = 10.0;
/// Vertical padding inside the strip.
const PAD_Y: f32 = 3.0;
/// Gap between adjacent docked widgets, and between the zone run and
/// the message.
const GAP: f32 = 8.0;
/// Minimum strip height in logical points.
const MIN_H: f32 = 24.0;
/// Message text size in logical points.
const MESSAGE_PT: f32 = 12.0;
/// Strip background fallback.
const FACE: [u8; 4] = [241, 242, 245, 255];
/// Top hairline fallback.
const HAIRLINE: [u8; 4] = [140, 145, 155, 255];
/// Permanent-message ink fallback.
const INK: [u8; 4] = [20, 20, 25, 255];
/// Temporary-message ink fallback — muted so the transient state is
/// visually distinct without italics.
const INK_TEMPORARY: [u8; 4] = [105, 109, 118, 255];
/// Arena-focus ring fallback (translucent accent).
const FOCUS_RING: [u8; 4] = [60, 110, 220, 200];

/// A widget embedded in a [`StatusBar`] — either a [`Button`] whose
/// activation the bar polls for [`StatusBar::take_activated`], or any
/// other widget boxed behind the child protocol.
///
/// `add_zone` / `add_permanent` take `impl Into<StatusItem>`, so a
/// `Button` converts implicitly and any other widget arrives via
/// [`StatusItem::widget`] (or `Box<dyn Widget>`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Button, ProgressBar, StatusItem};
///
/// let button = StatusItem::from(Button::new("Stop"));
/// let gauge = StatusItem::widget(ProgressBar::new());
/// ```
pub enum StatusItem {
    /// A [`Button`] — the bar polls `take_activated` on it.
    Button(Button),
    /// Any other widget.
    Widget(Box<dyn Widget>),
}

impl StatusItem {
    /// Wraps any widget as a [`StatusItem::Widget`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Spinner, StatusItem};
    ///
    /// let item = StatusItem::widget(Spinner::new());
    /// ```
    pub fn widget(w: impl Widget + 'static) -> Self {
        Self::Widget(Box::new(w))
    }

    /// The widget this item carries.
    fn as_widget(&self) -> &dyn Widget {
        match self {
            Self::Button(b) => b,
            Self::Widget(w) => w.as_ref(),
        }
    }

    /// Mutable form of [`as_widget`](Self::as_widget).
    fn as_widget_mut(&mut self) -> &mut dyn Widget {
        match self {
            Self::Button(b) => b,
            Self::Widget(w) => w.as_mut(),
        }
    }
}

impl From<Button> for StatusItem {
    fn from(button: Button) -> Self {
        Self::Button(button)
    }
}

impl From<Box<dyn Widget>> for StatusItem {
    fn from(widget: Box<dyn Widget>) -> Self {
        Self::Widget(widget)
    }
}

impl std::fmt::Debug for StatusItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Button(b) => f.debug_tuple("Button").field(&b.label).finish(),
            Self::Widget(_) => f.debug_tuple("Widget").field(&"..").finish(),
        }
    }
}

/// A bottom-of-window status strip — QStatusBar / WPF `StatusBar`.
///
/// See the [module documentation](self) for the zone model and the
/// temporary-message contract.
///
/// # Examples
///
/// ```
/// use martensite::widgets::StatusBar;
///
/// let bar = StatusBar::new().message("Ready");
/// assert_eq!(bar.current_message(), "Ready");
/// ```
pub struct StatusBar {
    /// Optional accessible label — overrides the message text as the
    /// node's label when set.
    pub label: Option<String>,
    /// Whether the bar accepts input.
    pub enabled: bool,
    /// The permanent message zone text.
    message: String,
    /// The transient message displacing `message` until cleared —
    /// QStatusBar `showMessage` semantics.
    temporary: Option<String>,
    /// Left-side zone widgets, in declaration order (child-protocol
    /// indices `0..zones.len()`).
    zones: Vec<StatusItem>,
    /// Right-docked permanent widgets (child-protocol indices
    /// `zones.len()..`), laid out right-to-left so the last added sits
    /// closest to the right edge.
    permanents: Vec<StatusItem>,
    /// Cached measured sizes in child-protocol order (zones then
    /// permanents).
    child_sizes: Vec<Vec2>,
    /// Per-child rects in child-protocol order.
    child_rects: Vec<Rect>,
    /// The rect reserved for the message text.
    message_rect: Rect,
    /// The last button activation not yet drained by
    /// [`take_activated`](Self::take_activated) — the child index of
    /// the button that fired.
    activated: Option<usize>,
    /// The child that receives non-positional events — set by the
    /// last pointer press (internal focus).
    key_target: Option<usize>,
    /// Child currently holding a pointer press; while set, positional
    /// events forward to it regardless of hit position.
    press_target: Option<usize>,
    /// Whether arena focus currently rests on the strip.
    focused: bool,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter for the message zone. See
    /// [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl StatusBar {
    /// Creates an empty status bar; set the message with
    /// [`message`](Self::message) and dock widgets with
    /// [`add_zone`](Self::add_zone) / [`add_permanent`](Self::add_permanent).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::StatusBar;
    /// use martensite_core::widget::Widget;
    ///
    /// let bar = StatusBar::new();
    /// assert_eq!(bar.current_message(), "");
    /// assert_eq!(bar.child_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            message: String::new(),
            temporary: None,
            zones: Vec::new(),
            permanents: Vec::new(),
            child_sizes: Vec::new(),
            child_rects: Vec::new(),
            message_rect: Rect::default(),
            activated: None,
            key_target: None,
            press_target: None,
            focused: false,
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the permanent message text (builder form of
    /// [`set_message`](Self::set_message)).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::StatusBar;
    ///
    /// let bar = StatusBar::new().message("Ready");
    /// assert_eq!(bar.current_message(), "Ready");
    /// ```
    #[inline]
    #[must_use]
    pub fn message(mut self, text: impl Into<String>) -> Self {
        self.message = text.into();
        self
    }

    /// Sets the permanent message text at runtime — the text shown
    /// whenever no temporary message is active.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::StatusBar;
    ///
    /// let mut bar = StatusBar::new();
    /// bar.set_message("3 items");
    /// assert_eq!(bar.current_message(), "3 items");
    /// ```
    #[inline]
    pub fn set_message(&mut self, text: impl Into<String>) {
        self.message = text.into();
    }

    /// Posts a temporary message that displaces the permanent one
    /// until [`clear_temporary`](Self::clear_temporary) runs —
    /// QStatusBar `showMessage` semantics. Painted in the muted ink.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::StatusBar;
    ///
    /// let mut bar = StatusBar::new().message("Ready");
    /// bar.temporary("Saving…");
    /// assert_eq!(bar.current_message(), "Saving…");
    /// assert!(bar.has_temporary());
    /// ```
    #[inline]
    pub fn temporary(&mut self, text: impl Into<String>) {
        self.temporary = Some(text.into());
    }

    /// Clears the temporary message, restoring the permanent one —
    /// QStatusBar `clearMessage` semantics. No-op when no temporary
    /// message is active.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::StatusBar;
    ///
    /// let mut bar = StatusBar::new().message("Ready");
    /// bar.temporary("Busy");
    /// bar.clear_temporary();
    /// assert_eq!(bar.current_message(), "Ready");
    /// ```
    #[inline]
    pub fn clear_temporary(&mut self) {
        self.temporary = None;
    }

    /// Whether a temporary message is currently displacing the
    /// permanent one.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::StatusBar;
    ///
    /// let mut bar = StatusBar::new();
    /// assert!(!bar.has_temporary());
    /// bar.temporary("Working");
    /// assert!(bar.has_temporary());
    /// ```
    #[inline]
    pub fn has_temporary(&self) -> bool {
        self.temporary.is_some()
    }

    /// The text the message zone currently displays — the temporary
    /// message while one is active, else the permanent message.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::StatusBar;
    ///
    /// let mut bar = StatusBar::new().message("Idle");
    /// assert_eq!(bar.current_message(), "Idle");
    /// bar.temporary("Syncing");
    /// assert_eq!(bar.current_message(), "Syncing");
    /// ```
    #[inline]
    pub fn current_message(&self) -> &str {
        self.temporary.as_deref().unwrap_or(&self.message)
    }

    /// Docks a widget in the left zone run, before the message.
    /// Accepts a [`Button`] directly (its activation reports through
    /// [`take_activated`](Self::take_activated)) or any other widget
    /// via [`StatusItem::widget`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{StatusBar, StatusItem, Text};
    /// use martensite_core::widget::Widget;
    ///
    /// let bar = StatusBar::new().add_zone(StatusItem::widget(Text::new("UTF-8")));
    /// assert_eq!(bar.child_count(), 1);
    /// ```
    #[must_use]
    pub fn add_zone(mut self, item: impl Into<StatusItem>) -> Self {
        self.zones.push(item.into());
        self
    }

    /// Docks a widget at the right edge. Permanents lay out
    /// right-to-left — the last added sits closest to the edge,
    /// matching `QStatusBar::addPermanentWidget`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Button, ProgressBar, StatusBar, StatusItem};
    /// use martensite_core::widget::Widget;
    ///
    /// let bar = StatusBar::new()
    ///     .add_permanent(StatusItem::widget(ProgressBar::new()))
    ///     .add_permanent(Button::new("Stop"));
    /// assert_eq!(bar.child_count(), 2);
    /// ```
    #[must_use]
    pub fn add_permanent(mut self, item: impl Into<StatusItem>) -> Self {
        self.permanents.push(item.into());
        self
    }

    /// Sets the bar's accessible label — overrides the displayed
    /// message as the node's label when set.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::StatusBar;
    ///
    /// let bar = StatusBar::new().label("Workspace status");
    /// assert_eq!(bar.label.as_deref(), Some("Workspace status"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets whether the bar accepts input.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::StatusBar;
    ///
    /// let bar = StatusBar::new().enabled(false);
    /// assert!(!bar.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Drains the last button activation: the *child index* (zones
    /// first, then permanents, in declaration order) of the [`Button`]
    /// that fired since the previous call — the same `take_*` seam
    /// `Button::take_activated` exposes per item.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Button, StatusBar};
    ///
    /// let mut bar = StatusBar::new().add_permanent(Button::new("Stop"));
    /// assert_eq!(bar.take_activated(), None);
    /// ```
    #[inline]
    pub fn take_activated(&mut self) -> Option<usize> {
        self.activated.take()
    }

    /// Shares a [`crate::text_paint::TextPainter`] so the message zone
    /// emits real glyph runs instead of `DrawText` placeholder boxes.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Drains embedded-`Button` activation flags into
    /// [`take_activated`](Self::take_activated). Called automatically
    /// from `event`, `layout`, and `a11y_prepare` — public so hosts
    /// reaching children through `child_mut` directly can fold the
    /// marks in afterwards (the [`Segmented::poll_pending`](crate::widgets::Segmented)
    /// contract).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Button, StatusBar};
    ///
    /// let mut bar = StatusBar::new().add_permanent(Button::new("OK"));
    /// bar.poll_signals();
    /// assert_eq!(bar.take_activated(), None);
    /// ```
    pub fn poll_signals(&mut self) {
        for (i, item) in self
            .zones
            .iter_mut()
            .chain(self.permanents.iter_mut())
            .enumerate()
        {
            if let StatusItem::Button(button) = item {
                if button.take_activated() {
                    self.activated = Some(i);
                }
            }
        }
    }

    /// Refreshes `child_sizes` when the child list changed since the
    /// last measure — layout tolerates being called without a prior
    /// measure pass.
    fn ensure_sizes(&mut self, cx: &mut LayoutContext) {
        let n = self.zones.len() + self.permanents.len();
        if self.child_sizes.len() == n {
            return;
        }
        self.child_sizes.clear();
        for item in self.zones.iter_mut().chain(self.permanents.iter_mut()) {
            let size = item.as_widget_mut().measure(
                cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: Vec2::new(f32::INFINITY, f32::INFINITY),
                },
            );
            self.child_sizes.push(size);
        }
    }

    /// Forwards a non-positional event to the current key target only —
    /// the internal-focus contract.
    fn forward_key(&mut self, cx: &mut EventContext) -> EventResponse {
        let Some(i) = self.key_target else {
            return EventResponse::Ignored;
        };
        let Some(bounds) = self.child_rects.get(i).copied() else {
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

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for StatusBar {
    fn debug_name(&self) -> &'static str {
        "StatusBar"
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let tight = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: constraints.max_size,
        };
        self.child_sizes.clear();
        let mut max_h = 0.0f32;
        for item in self.zones.iter_mut().chain(self.permanents.iter_mut()) {
            let size = item.as_widget_mut().measure(cx, tight);
            max_h = max_h.max(size.y);
            self.child_sizes.push(size);
        }
        let h = (max_h + 2.0 * cx.pt(PAD_Y))
            .max(cx.pt(MIN_H))
            .min(constraints.max_size.y.max(0.0));
        // Fill the offered width when it is bounded — the strip is a
        // docked band; an unbounded width still reports a floor.
        let w = if constraints.max_size.x.is_finite() {
            constraints.max_size.x.max(0.0)
        } else {
            cx.pt(200.0)
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
        let n = self.zones.len() + self.permanents.len();
        self.child_rects.clear();
        self.child_rects.resize(n, Rect::default());
        self.message_rect = Rect::default();
        if bounds.size.x <= 0.0 {
            return;
        }

        let pad_x = cx.pt(PAD_X);
        let pad_y = cx.pt(PAD_Y);
        let gap = cx.pt(GAP);
        let inner = Rect::new(
            bounds.min_x() + pad_x,
            bounds.min_y() + pad_y,
            (bounds.size.x - 2.0 * pad_x).max(0.0),
            (bounds.size.y - 2.0 * pad_y).max(0.0),
        );

        // Vertically centred rect for a child of measured size `size`
        // whose left edge is `x`.
        let centered = |x: f32, size: Vec2| -> Rect {
            let w = size.x.max(0.0);
            let h = size.y.min(inner.size.y).max(0.0);
            Rect::new(x, inner.min_y() + (inner.size.y - h) / 2.0, w, h)
        };

        // Permanents claim the right edge first — right-to-left, so
        // the last added sits closest to the edge (Qt
        // `addPermanentWidget` order).
        let mut right = inner.max_x();
        for i in (0..self.permanents.len()).rev() {
            let size = self
                .child_sizes
                .get(self.zones.len() + i)
                .copied()
                .unwrap_or(Vec2::ZERO);
            let w = size.x.min((right - inner.min_x()).max(0.0));
            let rect = centered(right - w, Vec2::new(w, size.y));
            self.child_rects[self.zones.len() + i] = rect;
            cx.layout_child(self.permanents[i].as_widget_mut(), rect);
            right = rect.min_x() - gap;
        }

        // Zones pack left-to-right in the space the permanents left.
        let mut left = inner.min_x();
        for i in 0..self.zones.len() {
            let size = self.child_sizes.get(i).copied().unwrap_or(Vec2::ZERO);
            let w = size.x.min((right - left).max(0.0));
            let rect = centered(left, Vec2::new(w, size.y));
            self.child_rects[i] = rect;
            cx.layout_child(self.zones[i].as_widget_mut(), rect);
            left = rect.max_x() + gap;
        }

        // The message zone takes whatever is left between the two
        // runs; a gap on each side keeps text off the widgets.
        self.message_rect = Rect::new(
            left.max(inner.min_x()),
            inner.min_y(),
            (right + gap - left.max(inner.min_x())).max(0.0),
            inner.size.y,
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        // AccessKit 0.25 has no `StatusBar` role — `Role::Status` is
        // the aria `status` landmark, the same role the dashboard's
        // bespoke strip emits.
        node.set_role(accesskit::Role::Status);
        let label = self.label.as_deref().unwrap_or_else(|| {
            let msg = self.current_message();
            if msg.is_empty() {
                "Status bar"
            } else {
                msg
            }
        });
        node.set_label(label);
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
                // A held press keeps the event stream — drag moves and
                // the release forward to `press_target` even when the
                // pointer leaves every child rect.
                let n = self.child_rects.len();
                let hit = if matches!(cx.event, WidgetEvent::PointerPressed { .. }) {
                    (0..n).rev().find(|&i| self.child_rects[i].contains(pos))
                } else {
                    self.press_target
                        .or_else(|| (0..n).rev().find(|&i| self.child_rects[i].contains(pos)))
                };
                let Some(i) = hit else {
                    return EventResponse::Ignored;
                };
                if matches!(cx.event, WidgetEvent::PointerPressed { .. }) {
                    self.key_target = Some(i);
                    self.press_target = Some(i);
                }
                let bounds = self.child_rects[i];
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
        // Top hairline separating the strip from the content above —
        // the bar docks at the window's bottom edge.
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.min_y()) + 1.0,
            ),
            cx.color(TokenKey::DividerColor, HAIRLINE),
        );

        // The message zone — temporary text paints muted so the
        // transient state reads at a glance (italics are not
        // available through the placeholder text path).
        let m = &self.message_rect;
        if m.size.x > 0.0 && m.size.y > 0.0 {
            let text = self.current_message();
            if !text.is_empty() {
                let size_px = cx.pt(MESSAGE_PT);
                let ink = if self.temporary.is_some() {
                    cx.color(TokenKey::TextMutedColor, INK_TEMPORARY)
                } else {
                    cx.color(TokenKey::TextColor, INK)
                };
                crate::text_paint::paint_label_clipped(
                    crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                    cx.list,
                    kurbo::Rect::new(
                        f64::from(m.min_x()),
                        f64::from(m.min_y()),
                        f64::from(m.max_x()),
                        f64::from(m.max_y()),
                    ),
                    kurbo::Point::new(
                        f64::from(m.min_x()),
                        f64::from(m.min_y() + (m.size.y - size_px) / 2.0),
                    ),
                    text,
                    size_px,
                    ink,
                );
            }
        }

        // Arena-focus ring on the strip itself.
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
        self.zones.len() + self.permanents.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index < self.zones.len() {
            Some(self.zones[index].as_widget())
        } else {
            self.permanents
                .get(index - self.zones.len())
                .map(StatusItem::as_widget)
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index < self.zones.len() {
            Some(self.zones[index].as_widget_mut())
        } else {
            self.permanents
                .get_mut(index - self.zones.len())
                .map(StatusItem::as_widget_mut)
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.child_rects.get(index).copied()
    }
}

impl std::fmt::Debug for StatusBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusBar")
            .field("message", &self.message)
            .field("temporary", &self.temporary)
            .field("zones", &self.zones)
            .field("permanents", &self.permanents)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;
    use martensite_core::PointerButton;

    fn bar() -> StatusBar {
        StatusBar::new()
            .message("Ready")
            .add_zone(StatusItem::widget(crate::widgets::text::Text::new("Ln 1")))
            .add_permanent(Button::new("A"))
            .add_permanent(Button::new("B"))
    }

    fn laid_out(bar: &mut StatusBar, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        bar.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn measured(bar: &mut StatusBar, w: f32, h: f32) {
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
        );
    }

    fn event(bar: &mut StatusBar, ev: &WidgetEvent) -> EventResponse {
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
    fn builder_collects_zones_and_permanents() {
        let bar = bar();
        assert_eq!(bar.child_count(), 3);
        assert_eq!(bar.current_message(), "Ready");
        assert!(bar.child(0).is_some());
        assert!(bar.child(2).is_some());
        assert!(bar.child(3).is_none());
    }

    #[test]
    fn temporary_displaces_until_cleared() {
        let mut bar = bar();
        assert!(!bar.has_temporary());
        bar.temporary("Saving…");
        assert_eq!(bar.current_message(), "Saving…");
        bar.clear_temporary();
        assert_eq!(bar.current_message(), "Ready");
        // Clearing twice is a no-op.
        bar.clear_temporary();
        assert_eq!(bar.current_message(), "Ready");
    }

    #[test]
    fn permanents_dock_right_to_left() {
        let mut bar = bar();
        measured(&mut bar, 600.0, 28.0);
        laid_out(&mut bar, 600.0, 28.0);
        // Children: [zone Text, permanent A, permanent B]. The last
        // permanent added (B) sits closest to the right edge.
        let a = bar.child_bounds(1).unwrap();
        let b = bar.child_bounds(2).unwrap();
        assert!(b.min_x() > a.min_x());
        assert!((b.max_x() - (600.0 - 10.0)).abs() < 0.5);
    }

    #[test]
    fn zone_and_message_share_the_left() {
        let mut bar = bar();
        measured(&mut bar, 600.0, 28.0);
        laid_out(&mut bar, 600.0, 28.0);
        let zone = bar.child_bounds(0).unwrap();
        assert_eq!(zone.min_x(), 10.0);
        let m = bar.message_rect;
        assert!(m.min_x() >= zone.max_x());
        assert!(m.max_x() <= bar.child_bounds(1).unwrap().min_x() + 0.5);
    }

    #[test]
    fn take_activated_reports_child_index() {
        let mut bar = bar();
        measured(&mut bar, 600.0, 28.0);
        laid_out(&mut bar, 600.0, 28.0);
        // Press + release inside permanent "B" — child index 2.
        let b = bar.child_bounds(2).unwrap();
        let x = b.min_x() + 4.0;
        let y = b.min_y() + 4.0;
        event(&mut bar, &press(x, y));
        event(&mut bar, &release(x, y));
        assert_eq!(bar.take_activated(), Some(2));
        assert_eq!(bar.take_activated(), None);
    }

    #[test]
    fn accessibility_role_status_and_label() {
        let bar = bar();
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        bar.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Status);
        assert_eq!(node.label(), Some("Ready"));
        // An explicit label wins over the message text.
        let bar = StatusBar::new().message("Ready").label("Workspace status");
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        bar.accessibility(&mut node);
        assert_eq!(node.label(), Some("Workspace status"));
    }

    #[test]
    fn disabled_ignores_events() {
        let mut bar = bar().enabled(false);
        measured(&mut bar, 600.0, 28.0);
        laid_out(&mut bar, 600.0, 28.0);
        let b = bar.child_bounds(2).unwrap();
        assert_eq!(
            event(&mut bar, &press(b.min_x() + 4.0, b.min_y() + 4.0)),
            EventResponse::Ignored
        );
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        bar.accessibility(&mut node);
        assert!(node.is_disabled());
    }

    #[test]
    fn measure_fills_width_and_floors_height() {
        let mut bar = bar();
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let size = bar.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(600.0, 28.0),
            },
        );
        assert_eq!(size.x, 600.0);
        assert!(size.y >= 24.0);
    }

    #[test]
    fn debug_format() {
        let bar = bar();
        let debug = format!("{:?}", bar);
        assert!(debug.contains("StatusBar"));
        assert!(debug.contains("Ready"));
    }
}
