//! `ScrollView` widget: a scrollable viewport with smart scrollbars.
//!
//! - Scrollbars appear only when the content overflows the viewport
//!   ("smart" scrollbars).
//! - Wheel, keyboard, content-drag, and scrollbar-thumb scrolling.
//! - Nested scroll chaining: unconsumed wheel deltas at the scroll
//!   bounds return `Ignored` so an ancestor scroll region can take them.
//! - Rubber-band overscroll on pointer drags via `martensite-motion`'s
//!   `RubberBandScroller2D`, with spring-back on release.
//! - Scroll anchoring: [`ScrollView::adjust_for_prepended`] applies the
//!   `S1 = S0 + (L1 − L0)` compensation when content grows above the
//!   viewport.
//! - Accessibility: `Role::ScrollView` with `scroll_x/y` + min/max and
//!   `ScrollUp/Down/Left/Right`/`SetScrollOffset` actions, plus
//!   `Role::ScrollBar` children carrying the same range semantics.
//!
//! # Documented limitations
//!
//! - **`ScrollIntoView` semantics**: `accesskit::Action::ScrollIntoView`
//!   carries no descendant identity, so the view scrolls the first
//!   clipped direct descendant of its content into view (document
//!   order) — a best-effort interpretation, not per-node targeting.
//! - **Virtual scrolling**: the view always lays out and emits its
//!   full content; virtualization for very large lists (windowed
//!   `posinset`/`setsize` emission) is deferred — today it is the
//!   app's responsibility to bound the content it composes.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{ScrollView, Text};
//!
//! let view = ScrollView::new(Text::new("tall content"));
//! assert_eq!(view.scroll_offset(), glam::Vec2::ZERO);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use kurbo::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};
use martensite_motion::RubberBandScroller2D;

/// Scrollbar thickness in logical pixels.
const BAR: f32 = 10.0;
/// Minimum scrollbar thumb length.
const MIN_THUMB: f32 = 24.0;
/// Line scroll amount for arrow keys and `ScrollUp/Down` actions.
const LINE: f32 = 48.0;
/// Track colour.
const TRACK_COLOR: [u8; 4] = [235, 237, 240, 255];
/// Thumb colour.
const THUMB_COLOR: [u8; 4] = [160, 166, 176, 255];
/// Thumb colour while dragged.
const THUMB_ACTIVE: [u8; 4] = [120, 126, 138, 255];

/// A scrollbar strip inside a [`ScrollView`].
///
/// Emitted as an internal child with `Role::ScrollBar`; its fields are
/// mirrors of the view's scroll state, refreshed by the owner on every
/// change. AT actions it receives are parked in `pending` and applied
/// by the owner via [`ScrollView::poll_pending`].
pub struct ScrollBarWidget {
    /// `true` for the vertical bar, `false` for horizontal.
    vertical: bool,
    /// Whether the bar is shown (content overflows on this axis).
    shown: bool,
    /// Current offset on this axis.
    offset: f32,
    /// Maximum scrollable offset on this axis.
    max_offset: f32,
    /// Parked AT action for the owner to apply.
    pending: Option<BarRequest>,
    /// Thumb rect within the bar, mirrored by the owner for painting.
    thumb: Option<Rect>,
    /// Whether the thumb is being dragged (mirrored from the owner).
    active: bool,
    /// Display scale from `layout` — scroll steps are logical pt.
    scale: f32,
}

/// A scroll request parked by a [`ScrollBarWidget`] for the owner.
#[derive(Copy, Clone, Debug)]
enum BarRequest {
    /// Scroll by this delta (logical pixels).
    By(Vec2),
    /// Scroll to this absolute offset.
    To(Vec2),
}

impl ScrollBarWidget {
    fn new(vertical: bool) -> Self {
        Self {
            vertical,
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

impl Widget for ScrollBarWidget {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let size = if self.vertical {
            Vec2::new(cx.pt(BAR), 0.0)
        } else {
            Vec2::new(0.0, cx.pt(BAR))
        };
        size.min(constraints.max_size.max(Vec2::ZERO))
    }

    fn layout(&mut self, cx: &mut LayoutContext, _bounds: Rect) {
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ScrollBar);
        node.set_orientation(if self.vertical {
            accesskit::Orientation::Vertical
        } else {
            accesskit::Orientation::Horizontal
        });
        node.set_numeric_value(f64::from(self.offset));
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(f64::from(self.max_offset));
        node.set_numeric_value_step(f64::from(LINE * self.scale));
        if self.vertical {
            node.add_action(accesskit::Action::ScrollUp);
            node.add_action(accesskit::Action::ScrollDown);
        } else {
            node.add_action(accesskit::Action::ScrollLeft);
            node.add_action(accesskit::Action::ScrollRight);
        }
        node.add_action(accesskit::Action::SetScrollOffset);
        if !self.shown {
            node.set_hidden();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        let WidgetEvent::SemanticAction(action) = cx.event else {
            return EventResponse::Ignored;
        };
        let axis = |v: f32| {
            if self.vertical {
                Vec2::new(0.0, v)
            } else {
                Vec2::new(v, 0.0)
            }
        };
        let line = LINE * self.scale;
        let request = match action {
            SemanticAction::ScrollUp | SemanticAction::ScrollLeft => BarRequest::By(axis(-line)),
            SemanticAction::ScrollDown | SemanticAction::ScrollRight => BarRequest::By(axis(line)),
            SemanticAction::SetScrollOffset(offset) => BarRequest::To(*offset),
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
            cx.list.push_path(
                kurbo::RoundedRect::from_rect(t, cx.ptf(f64::from(BAR) / 2.0)).to_path(0.1),
                if self.active {
                    cx.color(TokenKey::TextMutedColor, THUMB_ACTIVE)
                } else {
                    cx.color(TokenKey::BorderColor, THUMB_COLOR)
                },
            );
        }
    }
}

/// A scrollable viewport with smart scrollbars, implementing the ARIA
/// APG scroll-region contract.
///
/// The widget owns one content child laid out at its natural size and
/// clipped to the viewport (`clips_children`); scrollbars are internal
/// children shown only on overflowing axes.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{ScrollView, Text};
///
/// let mut view = ScrollView::new(Text::new("content"));
/// view.scroll_by(glam::Vec2::new(0.0, 30.0));
/// ```
pub struct ScrollView {
    /// Whether the view accepts input.
    pub enabled: bool,
    /// The scrolled content (child 0).
    content: Box<dyn Widget>,
    /// Vertical scrollbar (child 1).
    vbar: ScrollBarWidget,
    /// Horizontal scrollbar (child 2).
    hbar: ScrollBarWidget,
    /// Authoritative clamped scroll offset.
    offset: Vec2,
    /// Rubber-band driver for pointer-drag overscroll.
    scroller: RubberBandScroller2D,
    /// Content size from the last layout.
    content_size: Vec2,
    /// Cached widget bounds.
    cached_bounds: Rect,
    /// Content viewport (widget bounds minus shown bars).
    viewport: Rect,
    /// Vertical bar rect when shown.
    vbar_rect: Option<Rect>,
    /// Horizontal bar rect when shown.
    hbar_rect: Option<Rect>,
    /// Content-child bounds from the last layout.
    content_rect: Option<Rect>,
    /// Pointer-drag state: last pointer position while content-dragging.
    drag_last: Option<Vec2>,
    /// Thumb-drag state: `(vertical?, grab_offset_in_thumb)`.
    thumb_drag: Option<(bool, f32)>,
    /// Display scale from `layout` — bar width, min thumb, scroll step
    /// are logical pt.
    scale: f32,
}

impl ScrollView {
    /// Creates a scroll view around `content`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ScrollView, Text};
    ///
    /// let view = ScrollView::new(Text::new("scroll me"));
    /// assert_eq!(view.scroll_offset(), glam::Vec2::ZERO);
    /// ```
    pub fn new(content: impl Widget + 'static) -> Self {
        Self {
            enabled: true,
            content: Box::new(content),
            vbar: ScrollBarWidget::new(true),
            hbar: ScrollBarWidget::new(false),
            offset: Vec2::ZERO,
            scroller: RubberBandScroller2D::new((0.0, 0.0), (0.0, 0.0)),
            content_size: Vec2::ZERO,
            cached_bounds: Rect::default(),
            viewport: Rect::default(),
            vbar_rect: None,
            hbar_rect: None,
            content_rect: None,
            drag_last: None,
            thumb_drag: None,
            scale: 1.0,
        }
    }

    /// Sets whether the view is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ScrollView, Text};
    ///
    /// let v = ScrollView::new(Text::new("x")).enabled(false);
    /// assert!(!v.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The current clamped scroll offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ScrollView, Text};
    ///
    /// let v = ScrollView::new(Text::new("x"));
    /// assert_eq!(v.scroll_offset(), glam::Vec2::ZERO);
    /// ```
    #[inline]
    pub fn scroll_offset(&self) -> Vec2 {
        self.offset
    }

    /// The laid-out content size.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ScrollView, Text};
    ///
    /// let v = ScrollView::new(Text::new("x"));
    /// assert_eq!(v.content_size(), glam::Vec2::ZERO);
    /// ```
    #[inline]
    pub fn content_size(&self) -> Vec2 {
        self.content_size
    }

    /// The content viewport rect (bounds minus shown scrollbars).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ScrollView, Text};
    ///
    /// let v = ScrollView::new(Text::new("x"));
    /// assert_eq!(v.viewport().width(), 0.0);
    /// ```
    #[inline]
    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    /// Maximum scroll offset: `max(0, content - viewport)` per axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ScrollView, Text};
    ///
    /// let v = ScrollView::new(Text::new("x"));
    /// assert_eq!(v.max_offset(), glam::Vec2::ZERO);
    /// ```
    pub fn max_offset(&self) -> Vec2 {
        Vec2::new(
            (self.content_size.x - self.viewport.width()).max(0.0),
            (self.content_size.y - self.viewport.height()).max(0.0),
        )
    }

    /// Sets the scroll offset, clamped to `0..=max_offset` per axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ScrollView, Text};
    ///
    /// let mut v = ScrollView::new(Text::new("x"));
    /// v.set_scroll_offset(glam::Vec2::new(10.0, -5.0));
    /// assert_eq!(v.scroll_offset().y, 0.0); // clamped
    /// ```
    pub fn set_scroll_offset(&mut self, offset: Vec2) {
        let max = self.max_offset();
        self.offset = offset.clamp(Vec2::ZERO, max);
        self.sync_scroller();
        self.sync_bars();
        self.relayout_content();
    }

    /// Scrolls by `delta`, clamped. Returns the actually-applied delta —
    /// `Vec2::ZERO` means nothing was consumed (chaining boundary).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ScrollView, Text};
    ///
    /// let mut v = ScrollView::new(Text::new("x"));
    /// let applied = v.scroll_by(glam::Vec2::new(0.0, 10.0));
    /// assert_eq!(applied, glam::Vec2::ZERO); // nothing to scroll
    /// ```
    pub fn scroll_by(&mut self, delta: Vec2) -> Vec2 {
        let old = self.offset;
        self.set_scroll_offset(self.offset + delta);
        self.offset - old
    }

    /// Scrolls the minimum amount that makes `rect` (content-space)
    /// fully visible, honouring `ScrollIntoView` semantics.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ScrollView, Text};
    /// use martensite_core::Rect;
    ///
    /// let mut v = ScrollView::new(Text::new("x"));
    /// v.scroll_rect_into_view(Rect::new(0.0, 0.0, 10.0, 10.0));
    /// ```
    pub fn scroll_rect_into_view(&mut self, rect: Rect) {
        let vp = self.viewport;
        let mut target = self.offset;
        // Vertical axis: the rect's content-space y maps to
        // screen-space via `content_top = viewport.min_y - offset.y`.
        if rect.min_y() < self.offset.y {
            target.y = rect.min_y();
        } else if rect.max_y() > self.offset.y + vp.height() {
            target.y = rect.max_y() - vp.height();
        }
        if rect.min_x() < self.offset.x {
            target.x = rect.min_x();
        } else if rect.max_x() > self.offset.x + vp.width() {
            target.x = rect.max_x() - vp.width();
        }
        self.set_scroll_offset(target);
    }

    /// `ScrollIntoView` best-effort: scrolls the first direct
    /// descendant of the content (document order) that is clipped by
    /// the viewport fully into view. `accesskit::Action::ScrollIntoView`
    /// carries no descendant identity — see the module-level documented
    /// limitations — so when every child is already visible (or the
    /// content has no internal children), the content's own origin is
    /// scrolled into view instead.
    fn scroll_first_clipped_descendant(&mut self) {
        let vp = self.viewport;
        for i in 0..self.content.child_count() {
            let Some(b) = self.content.child_bounds(i) else {
                continue;
            };
            // Window-space child bounds → content-space rect.
            let rect = Rect::new(
                b.min_x() - vp.min_x() + self.offset.x,
                b.min_y() - vp.min_y() + self.offset.y,
                b.width(),
                b.height(),
            );
            let clipped = rect.min_y() < self.offset.y - 0.5
                || rect.max_y() > self.offset.y + vp.height() + 0.5
                || rect.min_x() < self.offset.x - 0.5
                || rect.max_x() > self.offset.x + vp.width() + 0.5;
            if clipped {
                self.scroll_rect_into_view(rect);
                return;
            }
        }
        // Everything already visible — scroll the content origin in.
        if let Some(rect) = self.content_rect {
            self.scroll_rect_into_view(Rect::new(
                rect.min_x() + self.offset.x,
                rect.min_y() + self.offset.y,
                rect.width(),
                rect.height(),
            ));
        }
    }

    /// Scroll anchoring: compensates the offset when `delta` logical
    /// pixels of content were inserted *above* the current viewport —
    /// the `S1 = S0 + (L1 − L0)` rule, keeping the same content under
    /// the user's eyes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ScrollView, Text};
    ///
    /// let mut v = ScrollView::new(Text::new("x"));
    /// // 40px of content prepended above the viewport → shift down by 40.
    /// v.adjust_for_prepended(glam::Vec2::new(0.0, 40.0));
    /// ```
    pub fn adjust_for_prepended(&mut self, delta: Vec2) {
        // L1 - L0 = delta (the anchored content moved `delta` pixels
        // further down); S1 = S0 + delta preserves the visible region.
        self.set_scroll_offset(self.offset + delta);
    }

    /// Whether a rubber-band spring-back is still animating — call
    /// [`update`](Self::update) each frame until this is `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ScrollView, Text};
    ///
    /// let v = ScrollView::new(Text::new("x"));
    /// assert!(v.is_settled());
    /// ```
    #[inline]
    pub fn is_settled(&self) -> bool {
        self.scroller.is_settled()
    }

    /// Advances the rubber-band spring by `dt` seconds; returns `true`
    /// while animation continues (a repaint is needed).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ScrollView, Text};
    ///
    /// let mut v = ScrollView::new(Text::new("x"));
    /// assert!(!v.update(0.016));
    /// ```
    pub fn update(&mut self, dt: f32) -> bool {
        self.scroller.update(dt);
        if self.scroller.is_settled() {
            // Land on the clamped boundary the spring targeted.
            let (x, y) = self.scroller.content_offset();
            let clamped = Vec2::new(x, y).clamp(Vec2::ZERO, self.max_offset());
            if clamped != self.offset {
                self.offset = clamped;
                self.relayout_content();
            }
            self.sync_bars();
            false
        } else {
            self.relayout_content();
            self.sync_bars();
            true
        }
    }

    /// Applies scroll requests parked by scrollbar children (AT actions
    /// delivered through `WidgetArena::internal_widget_mut`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{ScrollView, Text};
    ///
    /// let mut v = ScrollView::new(Text::new("x"));
    /// v.poll_pending();
    /// ```
    pub fn poll_pending(&mut self) {
        let mut pending = Vec::new();
        for bar in [&mut self.vbar, &mut self.hbar] {
            if let Some(req) = bar.pending.take() {
                pending.push(req);
            }
        }
        for req in pending {
            match req {
                BarRequest::By(d) => {
                    self.scroll_by(d);
                }
                BarRequest::To(o) => {
                    self.set_scroll_offset(o);
                }
            }
        }
    }

    /// The effective render offset: the rubber-banded visible offset
    /// while a drag/spring is active, else the clamped offset.
    fn effective_offset(&self) -> Vec2 {
        if self.drag_last.is_some() || !self.scroller.is_settled() {
            let (x, y) = self.scroller.visible_offset();
            Vec2::new(x, y)
        } else {
            self.offset
        }
    }

    /// Re-lays out the content child at the current effective offset so
    /// its `child_bounds` tracks the scroll position.
    fn relayout_content(&mut self) {
        // `content_rect` starts out `None`; this is the one place it is
        // assigned, so it must run unconditionally — otherwise the
        // content child is never laid out, painted, or hit-tested.
        let mut hot = martensite_core::HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            // The layout pass caches the real factor in `self.scale` —
            // hardcoding 1.0 here lays content out at a different scale
            // than `paint` uses and rows/text collide at HiDPI.
            scale: self.scale,
        };
        let off = self.effective_offset();
        let rect = Rect::new(
            self.viewport.min_x() - off.x,
            self.viewport.min_y() - off.y,
            self.content_size.x,
            self.content_size.y,
        );
        self.content_rect = Some(rect);
        self.content.layout(&mut cx, rect);
    }

    /// Synchronises the rubber-band scroller's sizes and offset with
    /// authoritative state.
    fn sync_scroller(&mut self) {
        self.scroller
            .set_content_size((self.content_size.x, self.content_size.y));
        self.scroller
            .set_viewport_size((self.viewport.width(), self.viewport.height()));
        let (cx, cy) = self.scroller.content_offset();
        let dx = self.offset.x - cx;
        let dy = self.offset.y - cy;
        if dx != 0.0 || dy != 0.0 {
            self.scroller.drag((dx, dy));
        }
    }

    /// Mirrors scroll state onto the scrollbar children for their
    /// emitted `ScrollBar` nodes and thumb painting.
    fn sync_bars(&mut self) {
        let max = self.max_offset();
        let v_thumb = self.vbar_thumb();
        let h_thumb = self.hbar_thumb();
        self.vbar.offset = self.offset.y;
        self.vbar.max_offset = max.y;
        self.vbar.thumb = v_thumb;
        self.vbar.active = matches!(self.thumb_drag, Some((true, _)));
        self.hbar.offset = self.offset.x;
        self.hbar.max_offset = max.x;
        self.hbar.thumb = h_thumb;
        self.hbar.active = matches!(self.thumb_drag, Some((false, _)));
    }

    /// The vertical scrollbar thumb rect, if the bar is shown.
    fn vbar_thumb(&self) -> Option<Rect> {
        let track = self.vbar_rect?;
        if self.content_size.y <= 0.0 {
            return None;
        }
        let track_len = track.height();
        let frac = (self.viewport.height() / self.content_size.y).clamp(0.0, 1.0);
        let thumb_len = (track_len * frac)
            .max(MIN_THUMB * self.scale)
            .min(track_len);
        let max_off = self.max_offset().y;
        let t = if max_off > 0.0 {
            self.offset.y / max_off
        } else {
            0.0
        };
        let top = track.min_y() + t * (track_len - thumb_len);
        Some(Rect::new(track.min_x(), top, track.width(), thumb_len))
    }

    /// The horizontal scrollbar thumb rect, if the bar is shown.
    fn hbar_thumb(&self) -> Option<Rect> {
        let track = self.hbar_rect?;
        if self.content_size.x <= 0.0 {
            return None;
        }
        let track_len = track.width();
        let frac = (self.viewport.width() / self.content_size.x).clamp(0.0, 1.0);
        let thumb_len = (track_len * frac)
            .max(MIN_THUMB * self.scale)
            .min(track_len);
        let max_off = self.max_offset().x;
        let t = if max_off > 0.0 {
            self.offset.x / max_off
        } else {
            0.0
        };
        let left = track.min_x() + t * (track_len - thumb_len);
        Some(Rect::new(left, track.min_y(), thumb_len, track.height()))
    }

    /// Maps a pointer position inside a bar track to the grab delta for
    /// a new thumb drag, or pages the scroll if the press landed outside
    /// the thumb. Returns `true` if a thumb drag started.
    fn press_bar(&mut self, vertical: bool, position: Vec2) -> bool {
        let (track, thumb) = if vertical {
            (self.vbar_rect, self.vbar_thumb())
        } else {
            (self.hbar_rect, self.hbar_thumb())
        };
        let (Some(_track), Some(thumb)) = (track, thumb) else {
            return false;
        };
        if thumb.contains(position) {
            let grab = if vertical {
                position.y - thumb.min_y()
            } else {
                position.x - thumb.min_x()
            };
            self.thumb_drag = Some((vertical, grab));
        } else {
            // Track press: page toward the click.
            let vp_len = if vertical {
                self.viewport.height()
            } else {
                self.viewport.width()
            };
            let sign = if vertical {
                if position.y < thumb.min_y() {
                    -1.0
                } else {
                    1.0
                }
            } else if position.x < thumb.min_x() {
                -1.0
            } else {
                1.0
            };
            let delta = if vertical {
                Vec2::new(0.0, sign * vp_len * 0.9)
            } else {
                Vec2::new(sign * vp_len * 0.9, 0.0)
            };
            self.scroll_by(delta);
        }
        true
    }

    /// Continues an active thumb drag.
    fn drag_thumb(&mut self, position: Vec2) {
        let Some((vertical, grab)) = self.thumb_drag else {
            return;
        };
        let (track, thumb_len) = if vertical {
            let Some(track) = self.vbar_rect else {
                return;
            };
            (track, self.vbar_thumb().map(|t| t.height()).unwrap_or(0.0))
        } else {
            let Some(track) = self.hbar_rect else {
                return;
            };
            (track, self.hbar_thumb().map(|t| t.width()).unwrap_or(0.0))
        };
        let (track_len, max_off) = if vertical {
            (track.height(), self.max_offset().y)
        } else {
            (track.width(), self.max_offset().x)
        };
        let usable = (track_len - thumb_len).max(f32::EPSILON);
        let pos_in_track = if vertical {
            position.y - track.min_y() - grab
        } else {
            position.x - track.min_x() - grab
        };
        let frac = (pos_in_track / usable).clamp(0.0, 1.0);
        let mut target = self.offset;
        if vertical {
            target.y = frac * max_off;
        } else {
            target.x = frac * max_off;
        }
        self.set_scroll_offset(target);
    }
}

impl Widget for ScrollView {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Measure the content against the given constraints to derive a
        // reasonable desired viewport (bounded so the view does not ask
        // for unbounded space).
        let desired = self.content.measure(cx, constraints);
        // `clamp` panics when min > max — cap the preferred minimum at
        // the constraint max so zero-constraint probes stay safe.
        let max_w = constraints.max_size.x.max(0.0);
        let max_h = constraints.max_size.y.max(0.0);
        Vec2::new(
            desired.x.clamp(cx.pt(40.0).min(max_w), max_w),
            desired.y.clamp(cx.pt(40.0).min(max_h), max_h),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.scale = cx.scale;
        // Hidden bars skip `layout` below but still answer
        // `accessibility` steps and parked deltas from `self.scale`.
        self.vbar.scale = cx.scale;
        self.hbar.scale = cx.scale;
        // Declare keyboard focusability on the arena node — scroll
        // regions are keyboard-scrollable (arrows/PageUp/PageDown).
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
        // Bottom-stick anchoring: capture whether the view is pinned to
        // the bottom *before* the content is re-measured, so a growth in
        // content height keeps the latest content visible.
        let old_max = self.max_offset();
        let was_at_bottom = old_max.y > 0.0 && self.offset.y >= old_max.y - 0.5;
        // Measure the content unbounded to learn its natural size.
        let desired = self.content.measure(
            cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(f32::MAX, f32::MAX),
            },
        );

        // Smart scrollbars: shown only when the axis overflows.
        let bar = cx.pt(BAR);
        let show_v = desired.y > bounds.height();
        let show_h = desired.x > bounds.width() - if show_v { bar } else { 0.0 };
        // Re-check vertical with horizontal bar accounted for.
        let show_v = desired.y > bounds.height() - if show_h { bar } else { 0.0 };

        let mut viewport = bounds;
        self.vbar_rect = None;
        self.hbar_rect = None;
        if show_v {
            viewport.size.x = (viewport.width() - bar).max(0.0);
            self.vbar_rect = Some(Rect::new(
                bounds.max_x() - bar,
                bounds.min_y(),
                bar,
                bounds.height() - if show_h { bar } else { 0.0 },
            ));
        }
        if show_h {
            viewport.size.y = (viewport.height() - bar).max(0.0);
            self.hbar_rect = Some(Rect::new(
                bounds.min_x(),
                bounds.max_y() - bar,
                bounds.width() - if show_v { bar } else { 0.0 },
                bar,
            ));
        }
        self.viewport = viewport;
        self.vbar.shown = show_v;
        self.hbar.shown = show_h;
        self.content_size = Vec2::new(
            desired.x.max(viewport.width()),
            desired.y.max(viewport.height()),
        );

        // Bottom-stick anchoring: if the view was pinned to the bottom
        // before this relayout, keep it pinned after the content resize.
        if was_at_bottom {
            self.offset.y = self.max_offset().y;
        }

        // Lay out the scrollbar children so their bounds are current.
        if let Some(rect) = self.vbar_rect {
            cx.layout_child(&mut self.vbar, rect);
        }
        if let Some(rect) = self.hbar_rect {
            cx.layout_child(&mut self.hbar, rect);
        }

        self.sync_scroller();
        self.set_scroll_offset(self.offset);
        self.relayout_content();
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ScrollView);
        let max = self.max_offset();
        node.set_scroll_x(f64::from(self.offset.x));
        node.set_scroll_x_min(0.0);
        node.set_scroll_x_max(f64::from(max.x));
        node.set_scroll_y(f64::from(self.offset.y));
        node.set_scroll_y_min(0.0);
        node.set_scroll_y_max(f64::from(max.y));
        node.add_action(accesskit::Action::ScrollUp);
        node.add_action(accesskit::Action::ScrollDown);
        node.add_action(accesskit::Action::ScrollLeft);
        node.add_action(accesskit::Action::ScrollRight);
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
            } => {
                if self.vbar_rect.is_some_and(|r| r.contains(*position)) {
                    self.press_bar(true, *position);
                    return EventResponse::CapturePointer;
                }
                if self.hbar_rect.is_some_and(|r| r.contains(*position)) {
                    self.press_bar(false, *position);
                    return EventResponse::CapturePointer;
                }
                // Inside the viewport: let the content take the press
                // first; if it ignores it, start a potential drag scroll.
                if let Some(content_rect) = self.content_rect.filter(|r| r.contains(*position)) {
                    let mut child_cx = EventContext {
                        event: cx.event,
                        bounds: content_rect,
                    };
                    let response = self.content.event(&mut child_cx);
                    if response != EventResponse::Ignored {
                        return response;
                    }
                }
                self.drag_last = Some(*position);
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerMoved { position } => {
                if self.thumb_drag.is_some() {
                    self.drag_thumb(*position);
                    return EventResponse::RequestRepaint;
                }
                if let Some(last) = self.drag_last {
                    let delta = *position - last;
                    self.drag_last = Some(*position);
                    // Dragging content up scrolls down: content offset
                    // increases as the pointer moves up/left.
                    self.scroller.drag((-delta.x, -delta.y));
                    self.relayout_content();
                    return EventResponse::RequestRepaint;
                }
                // No drag in flight: hover reaches the scrollable
                // content (e.g. listbox option highlighting).
                if let Some(content_rect) = self.content_rect.filter(|r| r.contains(*position)) {
                    let mut child_cx = EventContext {
                        event: cx.event,
                        bounds: content_rect,
                    };
                    return self.content.event(&mut child_cx);
                }
                EventResponse::Ignored
            }
            // Hover boundaries propagate to the scrollable content so
            // children can clear their hover state.
            WidgetEvent::PointerEnter | WidgetEvent::PointerLeave => {
                if let Some(content_rect) = self.content_rect {
                    let mut child_cx = EventContext {
                        event: cx.event,
                        bounds: content_rect,
                    };
                    return self.content.event(&mut child_cx);
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                position,
                button: PointerButton::Primary,
            } => {
                if self.thumb_drag.is_some() {
                    self.thumb_drag = None;
                    return EventResponse::ReleasePointer;
                }
                if self.drag_last.is_some() {
                    self.drag_last = None;
                    self.scroller.release((0.0, 0.0));
                    return EventResponse::ReleasePointer;
                }
                // Symmetric with the press path: content that took the
                // press also sees the release.
                if let Some(content_rect) = self.content_rect.filter(|r| r.contains(*position)) {
                    let mut child_cx = EventContext {
                        event: cx.event,
                        bounds: content_rect,
                    };
                    return self.content.event(&mut child_cx);
                }
                EventResponse::Ignored
            }
            WidgetEvent::Scroll { delta, .. } => {
                // Nested chaining: unconsumed delta returns `Ignored` so
                // an ancestor scroll region can take it.
                let applied = self.scroll_by(*delta);
                if applied.length_squared() > 0.0 {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::KeyPressed { key, .. } => {
                let vp = self.viewport;
                let line = LINE * self.scale;
                let delta = match key.as_str() {
                    "ArrowDown" => Vec2::new(0.0, line),
                    "ArrowUp" => Vec2::new(0.0, -line),
                    "ArrowRight" => Vec2::new(line, 0.0),
                    "ArrowLeft" => Vec2::new(-line, 0.0),
                    "PageDown" => Vec2::new(0.0, vp.height() * 0.9),
                    "PageUp" => Vec2::new(0.0, -vp.height() * 0.9),
                    "Home" => {
                        return {
                            self.set_scroll_offset(Vec2::new(self.offset.x, 0.0));
                            EventResponse::RequestRepaint
                        }
                    }
                    "End" => {
                        return {
                            let max = self.max_offset();
                            self.set_scroll_offset(Vec2::new(self.offset.x, max.y));
                            EventResponse::RequestRepaint
                        }
                    }
                    _ => return EventResponse::Ignored,
                };
                self.scroll_by(delta);
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::ScrollUp => {
                    self.scroll_by(Vec2::new(0.0, -LINE * self.scale));
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollDown => {
                    self.scroll_by(Vec2::new(0.0, LINE * self.scale));
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollLeft => {
                    self.scroll_by(Vec2::new(-LINE * self.scale, 0.0));
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollRight => {
                    self.scroll_by(Vec2::new(LINE * self.scale, 0.0));
                    EventResponse::RequestRepaint
                }
                SemanticAction::SetScrollOffset(offset) => {
                    self.set_scroll_offset(*offset);
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollToPoint(point) => {
                    self.scroll_rect_into_view(Rect::new(point.x, point.y, 1.0, 1.0));
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollIntoView => {
                    self.scroll_first_clipped_descendant();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Focus => EventResponse::CaptureFocus,
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        // Chrome: just the scrollbar thumbs (tracks paint via the bar
        // children). Content paints through the child walk, clipped to
        // the widget bounds by `clips_children`.
        let _ = cx;
    }

    fn tick(&mut self, dt: std::time::Duration) -> bool {
        // The decaying scroll/rubber-band animation — `update` returns
        // `true` while a repaint is needed.
        self.update(dt.as_secs_f32())
    }

    fn child_count(&self) -> usize {
        3
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        match index {
            0 => Some(&*self.content),
            1 => Some(&self.vbar),
            2 => Some(&self.hbar),
            _ => None,
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        match index {
            0 => Some(&mut *self.content),
            1 => Some(&mut self.vbar),
            2 => Some(&mut self.hbar),
            _ => None,
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        match index {
            0 => self.content_rect,
            1 => self.vbar_rect,
            2 => self.hbar_rect,
            _ => None,
        }
    }
}

impl std::fmt::Debug for ScrollView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollView")
            .field("offset", &self.offset)
            .field("content_size", &self.content_size)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    /// Fixed-size content stub.
    struct Fixed(Vec2);
    impl Widget for Fixed {
        fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
            self.0
        }
        fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
    }

    fn make_view(content: Vec2, viewport: Vec2) -> ScrollView {
        let mut v = ScrollView::new(Fixed(content));
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        v.layout(&mut cx, Rect::new(0.0, 0.0, viewport.x, viewport.y));
        v
    }

    fn scroll_event(v: &mut ScrollView, delta: Vec2) -> EventResponse {
        let ev = WidgetEvent::Scroll {
            position: Vec2::new(10.0, 10.0),
            delta,
        };
        let mut cx = EventContext {
            event: &ev,
            bounds: v.cached_bounds,
        };
        v.event(&mut cx)
    }

    #[test]
    fn wheel_scrolls_and_clamps() {
        let mut v = make_view(Vec2::new(80.0, 400.0), Vec2::new(100.0, 100.0));
        assert_eq!(v.max_offset().y, 300.0);
        scroll_event(&mut v, Vec2::new(0.0, 50.0));
        assert_eq!(v.scroll_offset().y, 50.0);
        scroll_event(&mut v, Vec2::new(0.0, 1000.0));
        assert_eq!(v.scroll_offset().y, 300.0);
    }

    #[test]
    fn wheel_at_bounds_chains_to_parent() {
        let mut v = make_view(Vec2::new(80.0, 400.0), Vec2::new(100.0, 100.0));
        // At the top, scrolling up is unconsumed → Ignored.
        assert_eq!(
            scroll_event(&mut v, Vec2::new(0.0, -20.0)),
            EventResponse::Ignored
        );
        // Consumed delta is handled.
        assert_eq!(
            scroll_event(&mut v, Vec2::new(0.0, 20.0)),
            EventResponse::RequestRepaint
        );
        // At the bottom, further down-scroll chains again.
        v.set_scroll_offset(Vec2::new(0.0, 300.0));
        assert_eq!(
            scroll_event(&mut v, Vec2::new(0.0, 20.0)),
            EventResponse::Ignored
        );
    }

    #[test]
    fn smart_bars_only_on_overflow() {
        let v = make_view(Vec2::new(400.0, 400.0), Vec2::new(100.0, 100.0));
        assert!(v.vbar_rect.is_some() && v.hbar_rect.is_some());
        let v2 = make_view(Vec2::new(400.0, 50.0), Vec2::new(100.0, 100.0));
        assert!(v2.vbar_rect.is_none() && v2.hbar_rect.is_some());
    }

    #[test]
    fn keyboard_scrolling() {
        let mut v = make_view(Vec2::new(80.0, 400.0), Vec2::new(100.0, 100.0));
        let key = |k: &str| WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        };
        let mut cx = EventContext {
            event: &key("End"),
            bounds: v.cached_bounds,
        };
        v.event(&mut cx);
        assert_eq!(v.scroll_offset().y, 300.0);
        let mut cx = EventContext {
            event: &key("Home"),
            bounds: v.cached_bounds,
        };
        v.event(&mut cx);
        assert_eq!(v.scroll_offset().y, 0.0);
        let mut cx = EventContext {
            event: &key("PageDown"),
            bounds: v.cached_bounds,
        };
        v.event(&mut cx);
        assert_eq!(v.scroll_offset().y, 90.0);
        let mut cx = EventContext {
            event: &key("ArrowDown"),
            bounds: v.cached_bounds,
        };
        v.event(&mut cx);
        assert_eq!(v.scroll_offset().y, 138.0);
    }

    #[test]
    fn thumb_drag_maps_to_offset() {
        let mut v = make_view(Vec2::new(80.0, 400.0), Vec2::new(100.0, 100.0));
        let track = v.vbar_rect.unwrap();
        let thumb = v.vbar_thumb().unwrap();
        // Press inside the thumb grabs it.
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(track.min_x() + 5.0, thumb.min_y() + 5.0),
            button: PointerButton::Primary,
        };
        let mut cx = EventContext {
            event: &press,
            bounds: v.cached_bounds,
        };
        assert_eq!(v.event(&mut cx), EventResponse::CapturePointer);
        assert!(v.thumb_drag.is_some());
        // Drag to the very bottom of the track → max offset.
        let moved = WidgetEvent::PointerMoved {
            position: Vec2::new(track.min_x() + 5.0, track.max_y() - 1.0),
        };
        let mut cx = EventContext {
            event: &moved,
            bounds: v.cached_bounds,
        };
        v.event(&mut cx);
        assert_eq!(v.scroll_offset().y, 300.0);
    }

    #[test]
    fn anchoring_adjusts_offset_on_prepend() {
        let mut v = make_view(Vec2::new(80.0, 400.0), Vec2::new(100.0, 100.0));
        v.set_scroll_offset(Vec2::new(0.0, 100.0));
        // 60px prepended above the viewport: L1 - L0 = 60 → S1 = 160.
        v.adjust_for_prepended(Vec2::new(0.0, 60.0));
        assert_eq!(v.scroll_offset().y, 160.0);
    }

    #[test]
    fn semantic_scroll_actions() {
        let mut v = make_view(Vec2::new(80.0, 400.0), Vec2::new(100.0, 100.0));
        let ev =
            WidgetEvent::SemanticAction(SemanticAction::SetScrollOffset(Vec2::new(0.0, 120.0)));
        let mut cx = EventContext {
            event: &ev,
            bounds: v.cached_bounds,
        };
        v.event(&mut cx);
        assert_eq!(v.scroll_offset().y, 120.0);
        let ev = WidgetEvent::SemanticAction(SemanticAction::ScrollDown);
        let mut cx = EventContext {
            event: &ev,
            bounds: v.cached_bounds,
        };
        v.event(&mut cx);
        assert_eq!(v.scroll_offset().y, 168.0);
    }

    #[test]
    fn rubber_band_overscroll_springs_back() {
        let mut v = make_view(Vec2::new(80.0, 400.0), Vec2::new(100.0, 100.0));
        // Content drag past the top edge overscrolls.
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(10.0, 10.0),
            button: PointerButton::Primary,
        };
        let mut cx = EventContext {
            event: &press,
            bounds: v.cached_bounds,
        };
        assert_eq!(v.event(&mut cx), EventResponse::CapturePointer);
        let moved = WidgetEvent::PointerMoved {
            position: Vec2::new(10.0, 200.0),
        };
        let mut cx = EventContext {
            event: &moved,
            bounds: v.cached_bounds,
        };
        v.event(&mut cx);
        // Visible offset overscrolls below zero.
        assert!(v.effective_offset().y < 0.0);
        let release = WidgetEvent::PointerReleased {
            position: Vec2::new(10.0, 200.0),
            button: PointerButton::Primary,
        };
        let mut cx = EventContext {
            event: &release,
            bounds: v.cached_bounds,
        };
        assert_eq!(v.event(&mut cx), EventResponse::ReleasePointer);
        assert!(!v.is_settled());
        // Advance the spring until settled — lands clamped at the top.
        for _ in 0..600 {
            v.update(1.0 / 60.0);
        }
        assert!(v.is_settled());
        assert_eq!(v.scroll_offset().y, 0.0);
    }

    #[test]
    fn scrollview_accessibility_contract() {
        let mut v = make_view(Vec2::new(80.0, 400.0), Vec2::new(100.0, 100.0));
        v.set_scroll_offset(Vec2::new(0.0, 100.0));
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        v.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::ScrollView);
        assert_eq!(node.scroll_y(), Some(100.0));
        assert_eq!(node.scroll_y_max(), Some(300.0));
        assert_eq!(node.scroll_y_min(), Some(0.0));
        assert!(node.supports_action(accesskit::Action::SetScrollOffset));
        assert!(node.supports_action(accesskit::Action::ScrollDown));

        // The scrollbar child emits Role::ScrollBar with the same range.
        let mut bar_node = AccessKitNode::new(accesskit::Role::Unknown);
        v.child(1).unwrap().accessibility(&mut bar_node);
        assert_eq!(bar_node.role(), accesskit::Role::ScrollBar);
        assert_eq!(bar_node.numeric_value(), Some(100.0));
        assert_eq!(bar_node.max_numeric_value(), Some(300.0));
    }
}
