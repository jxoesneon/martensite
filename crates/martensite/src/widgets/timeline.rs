//! Timeline — a vertical event feed with a dot-and-connector rail.
//!
//! Mirrors Ant Design `Timeline`, KDE `KTimeline`, and the WCT
//! `Timeline` control: each item paints a coloured dot on a left rail
//! joined by a connector line, with a label and optional subtitle to
//! the right. A trailing *pending* node (Ant convention) marks the
//! stream's live edge; reversing places newest first.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{Timeline, TimelineDot, TimelineItem};
//!
//! let tl = Timeline::new()
//!     .item(TimelineItem::new("Order placed").subtitle("09:41"))
//!     .item(TimelineItem::new("Payment failed").dot(TimelineDot::Error))
//!     .pending("Awaiting retry…");
//! assert_eq!(tl.item_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

/// Dot diameter in points.
const DOT_PT: f32 = 10.0;
/// Rail x-offset from the widget's left edge, in points.
const RAIL_PT: f32 = 6.0;
/// Connector stroke width in points.
const STEM_PT: f32 = 2.0;
/// Gap between the rail and the text block, in points.
const TEXT_GAP_PT: f32 = 14.0;
/// Vertical item pitch for a label-only row, in points.
const ITEM_PT: f32 = 30.0;
/// Added pitch when an item carries a subtitle, in points.
const SUBTITLE_PT: f32 = 16.0;
/// Pending-tail row height, in points.
const PENDING_PT: f32 = 26.0;

/// Semantic colour of a timeline dot.
///
/// # Examples
///
/// ```
/// use martensite::widgets::TimelineDot;
///
/// assert_eq!(TimelineDot::default(), TimelineDot::Accent);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum TimelineDot {
    /// Theme accent — a neutral step.
    #[default]
    Accent,
    /// Success green — a completed/good step.
    Success,
    /// Warning amber — a step needing attention.
    Warning,
    /// Error red — a failed step.
    Error,
    /// Muted grey — a skipped or future step.
    Muted,
}

/// One event on the feed.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{TimelineDot, TimelineItem};
///
/// let it = TimelineItem::new("Deployed").subtitle("v1.4.2").dot(TimelineDot::Success);
/// assert_eq!(it.dot, TimelineDot::Success);
/// ```
#[derive(Clone, Debug)]
pub struct TimelineItem {
    /// The event's primary text.
    pub label: String,
    /// Optional secondary text (timestamp, detail).
    pub subtitle: Option<String>,
    /// The dot's semantic colour.
    pub dot: TimelineDot,
}

impl TimelineItem {
    /// Creates an item with the given label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TimelineItem;
    ///
    /// assert_eq!(TimelineItem::new("a").label, "a");
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            subtitle: None,
            dot: TimelineDot::Accent,
        }
    }

    /// Sets the secondary text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TimelineItem;
    ///
    /// let it = TimelineItem::new("a").subtitle("09:00");
    /// assert_eq!(it.subtitle.as_deref(), Some("09:00"));
    /// ```
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Sets the dot colour.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{TimelineDot, TimelineItem};
    ///
    /// let it = TimelineItem::new("a").dot(TimelineDot::Warning);
    /// assert_eq!(it.dot, TimelineDot::Warning);
    /// ```
    #[must_use]
    pub fn dot(mut self, dot: TimelineDot) -> Self {
        self.dot = dot;
        self
    }
}

/// A vertical event feed. Display-only — timelines narrate state;
/// interactive steps belong to [`crate::widgets::Steps`].
///
/// # Examples
///
/// ```
/// use martensite::widgets::Timeline;
///
/// let tl = Timeline::new().enabled(false);
/// assert!(!tl.enabled);
/// ```
pub struct Timeline {
    /// Optional accessible label.
    pub label: Option<String>,
    /// Whether the widget accepts input (display-only; greys the dots
    /// when false).
    pub enabled: bool,
    /// When true, the last item renders as a hollow "pending" ring —
    /// Ant's live-edge convention — and [`pending`](Self::pending)'s
    /// text follows it.
    items: Vec<TimelineItem>,
    /// Optional pending-tail text; `None` hides the tail.
    pending_text: Option<String>,
    /// Newest-first ordering.
    reversed: bool,
    /// Cached bounds from the last layout pass.
    bounds: Rect,
    /// Shared shaped-text painter — see [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Timeline {
    /// Creates an empty timeline.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Timeline;
    ///
    /// assert_eq!(Timeline::new().item_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            items: Vec::new(),
            pending_text: None,
            reversed: false,
            bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Appends an item.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Timeline, TimelineItem};
    ///
    /// let tl = Timeline::new().item(TimelineItem::new("a"));
    /// assert_eq!(tl.item_count(), 1);
    /// ```
    #[must_use]
    pub fn item(mut self, item: TimelineItem) -> Self {
        self.items.push(item);
        self
    }

    /// Sets all items at once.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Timeline, TimelineItem};
    ///
    /// let tl = Timeline::new().items([TimelineItem::new("a"), TimelineItem::new("b")]);
    /// assert_eq!(tl.item_count(), 2);
    /// ```
    #[must_use]
    pub fn items(mut self, items: impl IntoIterator<Item = TimelineItem>) -> Self {
        self.items = items.into_iter().collect();
        self
    }

    /// Sets the pending-tail text. `Some` renders a hollow tail node
    /// (the stream's live edge); `None` hides it.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Timeline;
    ///
    /// let tl = Timeline::new().pending("live…");
    /// assert!(tl.has_pending());
    /// ```
    #[must_use]
    pub fn pending(mut self, text: impl Into<String>) -> Self {
        self.pending_text = Some(text.into());
        self
    }

    /// Sets newest-first ordering.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Timeline;
    ///
    /// let tl = Timeline::new().reversed(true);
    /// assert!(tl.is_reversed());
    /// ```
    #[must_use]
    pub fn reversed(mut self, reversed: bool) -> Self {
        self.reversed = reversed;
        self
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Timeline;
    ///
    /// let tl = Timeline::new().label("Order history");
    /// assert_eq!(tl.label.as_deref(), Some("Order history"));
    /// ```
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets whether the widget is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Timeline;
    ///
    /// let tl = Timeline::new().enabled(false);
    /// assert!(!tl.enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so labels emit real
    /// glyph runs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Number of items (excluding the pending tail).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Timeline, TimelineItem};
    ///
    /// assert_eq!(Timeline::new().item(TimelineItem::new("a")).item_count(), 1);
    /// ```
    #[inline]
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Whether the pending tail is shown.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Timeline;
    ///
    /// assert!(!Timeline::new().has_pending());
    /// ```
    #[inline]
    pub fn has_pending(&self) -> bool {
        self.pending_text.is_some()
    }

    /// Whether the feed renders newest-first.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Timeline;
    ///
    /// assert!(!Timeline::new().is_reversed());
    /// ```
    #[inline]
    pub fn is_reversed(&self) -> bool {
        self.reversed
    }

    /// Dot colour for a variant.
    fn dot_color(&self, cx: &PaintContext, dot: TimelineDot) -> [u8; 4] {
        if !self.enabled {
            return cx.color(TokenKey::TextMutedColor, [140, 140, 148, 255]);
        }
        match dot {
            TimelineDot::Accent => cx.color(TokenKey::AccentColor, [60, 110, 220, 255]),
            TimelineDot::Success => cx.color(TokenKey::SuccessColor, [60, 160, 90, 255]),
            TimelineDot::Warning => cx.color(TokenKey::WarningColor, [210, 150, 40, 255]),
            TimelineDot::Error => cx.color(TokenKey::ErrorColor, [200, 60, 60, 255]),
            TimelineDot::Muted => cx.color(TokenKey::TextMutedColor, [140, 140, 148, 255]),
        }
    }
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Timeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Timeline")
            .field("items", &self.items.len())
            .field("pending", &self.pending_text.is_some())
            .field("reversed", &self.reversed)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Widget for Timeline {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut h = 0.0f32;
        for it in &self.items {
            h += cx.pt(ITEM_PT);
            if it.subtitle.is_some() {
                h += cx.pt(SUBTITLE_PT);
            }
        }
        if self.pending_text.is_some() {
            h += cx.pt(PENDING_PT);
        }
        let max_w = constraints.max_size.x.max(0.0);
        let max_h = constraints.max_size.y.max(0.0);
        Vec2::new(
            max_w.min(cx.pt(160.0).max(0.0)).max(cx.pt(80.0).min(max_w)),
            h.max(cx.pt(ITEM_PT)).min(max_h),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, ITEM_PT)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        // Display-only: narration consumes nothing.
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        // Rows can run past the allocated height — clip to bounds so
        // text never paints outside the widget.
        cx.list.push_clip(kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        ));
        let rail_x = b.min_x() + cx.pt(RAIL_PT);
        let text_x = rail_x + cx.pt(TEXT_GAP_PT);
        let text_right = b.max_x() - cx.pt(2.0);
        let ink = cx.color(TokenKey::TextColor, [30, 30, 36, 255]);
        let muted = cx.color(TokenKey::TextMutedColor, [110, 110, 118, 255]);
        let stem = cx.color(TokenKey::DividerColor, [210, 212, 218, 255]);
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let font_px = cx.pt(13.0);
        let sub_px = cx.pt(11.0);

        let order: Vec<&TimelineItem> = if self.reversed {
            self.items.iter().rev().collect()
        } else {
            self.items.iter().collect()
        };

        let mut y = b.min_y();
        let mut prev_dot_y: Option<f32> = None;
        for it in &order {
            let item_h = cx.pt(ITEM_PT)
                + if it.subtitle.is_some() {
                    cx.pt(SUBTITLE_PT)
                } else {
                    0.0
                };
            let dot_y = y + cx.pt(4.0);
            // Connector from the previous dot into this one.
            if let Some(py) = prev_dot_y {
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(rail_x + cx.pt(DOT_PT - STEM_PT) / 2.0),
                        f64::from(py),
                        f64::from(rail_x + cx.pt(DOT_PT + STEM_PT) / 2.0),
                        f64::from(dot_y),
                    ),
                    stem,
                );
            }
            let dot_rect = kurbo::Rect::new(
                f64::from(rail_x),
                f64::from(dot_y),
                f64::from(rail_x + cx.pt(DOT_PT)),
                f64::from(dot_y + cx.pt(DOT_PT)),
            );
            cx.list
                .push_fill_shape(dot_rect, &Shape::ELLIPSE, self.dot_color(cx, it.dot));
            // Label + subtitle, clipped to the text column.
            let row_clip = kurbo::Rect::new(
                f64::from(text_x),
                f64::from(y),
                f64::from(text_right),
                f64::from(y + item_h),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                row_clip,
                kurbo::Point::new(f64::from(text_x), f64::from(y + cx.pt(1.0))),
                &it.label,
                font_px,
                ink,
            );
            if let Some(ref sub) = it.subtitle {
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    row_clip,
                    kurbo::Point::new(
                        f64::from(text_x),
                        f64::from(y + cx.pt(1.0) + font_px + cx.pt(2.0)),
                    ),
                    sub,
                    sub_px,
                    muted,
                );
            }
            prev_dot_y = Some(dot_y + cx.pt(DOT_PT));
            y += item_h;
        }

        // Pending tail — a hollow ring marking the live edge.
        if let Some(ref text) = self.pending_text {
            let dot_y = y + cx.pt(4.0);
            if let Some(py) = prev_dot_y {
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(rail_x + cx.pt(DOT_PT - STEM_PT) / 2.0),
                        f64::from(py),
                        f64::from(rail_x + cx.pt(DOT_PT + STEM_PT) / 2.0),
                        f64::from(dot_y),
                    ),
                    stem,
                );
            }
            let ring = kurbo::Rect::new(
                f64::from(rail_x),
                f64::from(dot_y),
                f64::from(rail_x + cx.pt(DOT_PT)),
                f64::from(dot_y + cx.pt(DOT_PT)),
            );
            let accent = self.dot_color(cx, TimelineDot::Accent);
            cx.list
                .push_stroke_shape(ring, &Shape::ELLIPSE, cx.pt(STEM_PT), accent);
            let tail_clip = kurbo::Rect::new(
                f64::from(text_x),
                f64::from(y),
                f64::from(text_right),
                f64::from(y + cx.pt(PENDING_PT)),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                tail_clip,
                kurbo::Point::new(f64::from(text_x), f64::from(y + cx.pt(1.0))),
                text,
                font_px,
                muted,
            );
        }
        cx.list.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, WidgetEvent};

    fn laid_out(tl: &mut Timeline, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        tl.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn builder() {
        let tl = Timeline::new()
            .item(
                TimelineItem::new("a")
                    .subtitle("s")
                    .dot(TimelineDot::Success),
            )
            .item(TimelineItem::new("b"))
            .pending("live")
            .reversed(true)
            .label("feed");
        assert_eq!(tl.item_count(), 2);
        assert!(tl.has_pending());
        assert!(tl.is_reversed());
        assert_eq!(tl.label.as_deref(), Some("feed"));
    }

    #[test]
    fn items_builder() {
        let tl = Timeline::new().items([TimelineItem::new("x"), TimelineItem::new("y")]);
        assert_eq!(tl.item_count(), 2);
    }

    #[test]
    fn measure_counts_subtitles_and_pending() {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let c = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(400.0, 400.0),
        };
        let plain = Timeline::new()
            .item(TimelineItem::new("a"))
            .item(TimelineItem::new("b"))
            .measure(&mut cx, c);
        let rich = Timeline::new()
            .item(TimelineItem::new("a").subtitle("s"))
            .item(TimelineItem::new("b"))
            .pending("p")
            .measure(&mut cx, c);
        assert!(rich.y > plain.y);
    }

    #[test]
    fn ignores_input() {
        let mut tl = Timeline::new();
        laid_out(&mut tl, 200.0, 100.0);
        let ev = WidgetEvent::PointerPressed {
            position: Vec2::new(30.0, 10.0),
            button: martensite_core::PointerButton::Primary,
            count: 1,
        };
        let mut cx = EventContext {
            event: &ev,
            bounds: tl.bounds,
            scale: 1.0,
        };
        assert_eq!(tl.event(&mut cx), EventResponse::Ignored);
    }

    #[test]
    fn accessibility_list_role() {
        let tl = Timeline::new().label("history").enabled(false);
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        tl.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::List);
        assert!(node.is_disabled());
    }

    #[test]
    fn paint_emits_dots_and_labels() {
        let mut tl = Timeline::new()
            .item(TimelineItem::new("a").dot(TimelineDot::Error))
            .item(TimelineItem::new("b"))
            .pending("p");
        laid_out(&mut tl, 200.0, 120.0);
        let theme = martensite_theme::Theme::new("test");
        let mut list = martensite_core::paint::PaintList::default();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: tl.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        };
        tl.paint(&mut cx);
        assert!(!list.is_empty());
    }
}
