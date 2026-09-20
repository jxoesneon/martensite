//! `MessageList` — a scrolling chat transcript: alternating
//! sent/received bubbles with sender names and timestamps
//! (iMessage / Slack channel idiom), pairing with
//! [`crate::widgets::typing_indicator::TypingIndicator`] and
//! [`crate::widgets::mention::Mention`].
//!
//! [`MessageList::push`] appends a [`Message`]; the wheel
//! scrolls the backlog and `follow` snaps to the newest bubble
//! when a message lands while pinned to the bottom. Row height
//! is estimated by text length — this is a list, not a
//! conversation engine.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::message_list::{Message, MessageList};
//!
//! let mut l = MessageList::new();
//! l.push(Message::received("Ann", "on my way").time("09:41"));
//! l.push(Message::sent("got it").time("09:42"));
//! assert_eq!(l.len(), 2);
//! assert_eq!(l.message(1).unwrap().sender, "You");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const W_PT: f32 = 300.0;
const H_PT: f32 = 260.0;
const ROW_PT: f32 = 20.0;
const PAD_PT: f32 = 10.0;
const BUBBLE_PAD_PT: f32 = 8.0;
const MAX_BUBBLE_FRAC: f32 = 0.72;

const FACE: [u8; 4] = [30, 30, 34, 255];
const IN_BUBBLE: [u8; 4] = [48, 48, 54, 255];
const OUT_BUBBLE: [u8; 4] = [50, 110, 200, 255];
const TEXT: [u8; 4] = [215, 215, 220, 255];
const MUTED: [u8; 4] = [140, 140, 150, 255];

/// One chat message — see [`MessageList`].
///
/// ```
/// use martensite::widgets::message_list::Message;
///
/// let m = Message::sent("hi");
/// assert!(m.outgoing);
/// ```
#[derive(Debug, Clone)]
pub struct Message {
    /// Sender display name ("You" for sent messages).
    pub sender: String,
    /// Body text.
    pub body: String,
    /// Small timestamp/label under the bubble.
    pub time: String,
    /// `true` = sent (right-aligned accent bubble).
    pub outgoing: bool,
}

impl Message {
    /// A received (left-aligned) message.
    ///
    /// ```
    /// use martensite::widgets::message_list::Message;
    ///
    /// let m = Message::received("Ann", "hey");
    /// assert!(!m.outgoing);
    /// assert_eq!(m.sender, "Ann");
    /// ```
    pub fn received(sender: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            sender: sender.into(),
            body: body.into(),
            time: String::new(),
            outgoing: false,
        }
    }

    /// A sent (right-aligned) message from the local user.
    ///
    /// ```
    /// use martensite::widgets::message_list::Message;
    ///
    /// let m = Message::sent("ok");
    /// assert!(m.outgoing);
    /// ```
    pub fn sent(body: impl Into<String>) -> Self {
        Self {
            sender: "You".to_string(),
            body: body.into(),
            time: String::new(),
            outgoing: true,
        }
    }

    /// Timestamp/label under the bubble.
    ///
    /// ```
    /// use martensite::widgets::message_list::Message;
    ///
    /// assert_eq!(Message::sent("x").time("10:02").time, "10:02");
    /// ```
    pub fn time(mut self, time: impl Into<String>) -> Self {
        self.time = time.into();
        self
    }
}

/// A scrolling chat transcript — see the module docs.
///
/// ```
/// use martensite::widgets::message_list::MessageList;
///
/// assert_eq!(MessageList::new().len(), 0);
/// ```
pub struct MessageList {
    /// Accessibility label.
    pub label: String,
    /// Backlog, oldest first.
    pub messages: Vec<Message>,
    /// Follow the newest message when already at the bottom.
    pub follow: bool,
    scroll: f32,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for MessageList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageList")
            .field("messages", &self.messages.len())
            .field("follow", &self.follow)
            .finish()
    }
}

impl Default for MessageList {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageList {
    /// Creates an empty transcript following the tail.
    ///
    /// ```
    /// use martensite::widgets::message_list::MessageList;
    ///
    /// assert!(MessageList::new().follow);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Messages".to_string(),
            messages: Vec::new(),
            follow: true,
            scroll: 0.0,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Initial backlog.
    ///
    /// ```
    /// use martensite::widgets::message_list::{Message, MessageList};
    ///
    /// let l = MessageList::new().messages([Message::sent("hi")]);
    /// assert_eq!(l.len(), 1);
    /// ```
    pub fn messages(mut self, msgs: impl IntoIterator<Item = Message>) -> Self {
        self.messages = msgs.into_iter().collect();
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::message_list::MessageList;
    ///
    /// assert_eq!(MessageList::new().label("#dev").label, "#dev");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::message_list::MessageList;
    ///
    /// let _ = MessageList::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Message count.
    ///
    /// ```
    /// use martensite::widgets::message_list::{Message, MessageList};
    ///
    /// let mut l = MessageList::new();
    /// l.push(Message::sent("a"));
    /// assert_eq!(l.len(), 1);
    /// ```
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Whether the transcript is empty.
    ///
    /// ```
    /// use martensite::widgets::message_list::MessageList;
    ///
    /// assert!(MessageList::new().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Message at `index` (oldest-first).
    ///
    /// ```
    /// use martensite::widgets::message_list::{Message, MessageList};
    ///
    /// let l = MessageList::new().messages([Message::sent("x")]);
    /// assert_eq!(l.message(0).unwrap().body, "x");
    /// ```
    pub fn message(&self, index: usize) -> Option<&Message> {
        self.messages.get(index)
    }

    /// Appends a message; when `follow` is on the view snaps
    /// to the bottom.
    ///
    /// ```
    /// use martensite::widgets::message_list::{Message, MessageList};
    ///
    /// let mut l = MessageList::new();
    /// l.push(Message::received("A", "hey"));
    /// assert_eq!(l.len(), 1);
    /// ```
    pub fn push(&mut self, msg: Message) {
        self.messages.push(msg);
        if self.follow {
            self.scroll = self.max_scroll();
        }
    }

    /// Current scroll offset in pixels.
    ///
    /// ```
    /// use martensite::widgets::message_list::MessageList;
    ///
    /// assert_eq!(MessageList::new().scroll(), 0.0);
    /// ```
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// Whether the view is pinned at the newest message
    /// (the `follow` flag).
    ///
    /// ```
    /// use martensite::widgets::message_list::MessageList;
    ///
    /// assert!(MessageList::new().at_bottom());
    /// ```
    pub fn at_bottom(&self) -> bool {
        self.follow
    }

    /// Estimated row height for a message (bubble + meta line).
    fn row_h(&self, m: &Message) -> f32 {
        let line = ROW_PT * self.scale;
        let chars_per_line = 28usize;
        let lines = (m.body.chars().count() / chars_per_line + 1).max(1) as f32;
        line * (lines + 1.2)
    }

    /// Total content height.
    fn content_h(&self) -> f32 {
        self.messages.iter().map(|m| self.row_h(m)).sum::<f32>() + PAD_PT * self.scale * 2.0
    }

    /// Max scroll offset.
    fn max_scroll(&self) -> f32 {
        (self.content_h() - self.bounds.height()).max(0.0)
    }
}

impl Widget for MessageList {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        if self.follow {
            self.scroll = self.max_scroll();
        } else {
            self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        node.set_label(format!("{} — {} messages", self.label, self.messages.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::Scroll { position, delta } = cx.event {
            if self.bounds.contains(*position) {
                let max = self.max_scroll();
                self.scroll = (self.scroll - delta.y).clamp(0.0, max);
                // Leaving the bottom unpins follow; returning re-pins.
                self.follow = self.scroll >= max - 1.0;
                return EventResponse::RequestRepaint;
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::RECT,
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let s = self.scale;
        let pad = PAD_PT * s;
        let text_sz = 12.0 * s;
        let meta_sz = 9.5 * s;
        let bubble_pad = BUBBLE_PAD_PT * s;
        let max_bubble = self.bounds.width() * MAX_BUBBLE_FRAC;
        let text = cx.color(TokenKey::TextColor, TEXT);
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);

        cx.list.push_clip(krect(self.bounds));
        let mut y = self.bounds.min_y() + pad - self.scroll;
        for m in &self.messages {
            let row = self.row_h(m);
            if y + row < self.bounds.min_y() {
                y += row;
                continue;
            }
            if y > self.bounds.max_y() {
                break;
            }
            // Bubble width by body length.
            let bw = (m.body.chars().count() as f32 * text_sz * 0.52 + bubble_pad * 2.0)
                .min(max_bubble)
                .max(bubble_pad * 4.0);
            let bh = row - meta_sz * 1.6;
            let bx = if m.outgoing {
                self.bounds.max_x() - pad - bw
            } else {
                self.bounds.min_x() + pad
            };
            let bubble = kurbo::Rect::new(
                f64::from(bx),
                f64::from(y),
                f64::from(bx + bw),
                f64::from(y + bh),
            );
            let fill = if m.outgoing {
                cx.color(TokenKey::AccentColor, OUT_BUBBLE)
            } else {
                IN_BUBBLE
            };
            cx.list.push_fill_shape(
                bubble,
                &martensite_core::shape::Shape::rounded(bubble_pad),
                fill,
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                bubble,
                kurbo::Point::new(f64::from(bx + bubble_pad), f64::from(y + bubble_pad * 0.7)),
                &m.body,
                text_sz,
                text,
            );
            // Meta line under the bubble: sender + time.
            let meta = if m.time.is_empty() {
                m.sender.clone()
            } else {
                format!("{} · {}", m.sender, m.time)
            };
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(bx), f64::from(y + bh + meta_sz * 0.3)),
                &meta,
                meta_sz,
                muted,
            );
            y += row;
        }
        cx.list.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(w: &mut MessageList, wd: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(wd, h),
            },
        );
        w.layout(&mut cx, Rect::new(0.0, 0.0, wd, h));
    }

    #[test]
    fn push_and_read() {
        let mut l = MessageList::new();
        l.push(Message::received("Ann", "hey"));
        l.push(Message::sent("hi").time("10:00"));
        assert_eq!(l.len(), 2);
        assert!(l.message(1).unwrap().outgoing);
        assert_eq!(l.message(0).unwrap().sender, "Ann");
    }

    #[test]
    fn follow_snaps_to_tail() {
        let mut l = MessageList::new();
        laid_out(&mut l, 300.0, 80.0); // short viewport
        for _ in 0..20 {
            l.push(Message::sent("a long-ish message body to fill rows"));
        }
        assert!(l.at_bottom());
        assert!(l.scroll() > 0.0);
    }

    #[test]
    fn scroll_unpins_follow() {
        let mut l = MessageList::new();
        laid_out(&mut l, 300.0, 80.0);
        for _ in 0..20 {
            l.push(Message::sent("body"));
        }
        l.event(&mut EventContext {
            event: &WidgetEvent::Scroll {
                position: Vec2::new(150.0, 40.0),
                delta: Vec2::new(0.0, 60.0), // up → read backlog
            },
            bounds: l.bounds,
            scale: 1.0,
        });
        assert!(!l.follow);
        // New message no longer snaps while reading history.
        let s = l.scroll();
        l.push(Message::sent("new"));
        assert_eq!(l.scroll(), s.min(l.max_scroll()));
    }

    #[test]
    fn scroll_clamps() {
        let mut l = MessageList::new().messages([Message::sent("x")]);
        laid_out(&mut l, 300.0, 260.0);
        l.event(&mut EventContext {
            event: &WidgetEvent::Scroll {
                position: Vec2::new(150.0, 40.0),
                delta: Vec2::new(0.0, -9999.0), // way down
            },
            bounds: l.bounds,
            scale: 1.0,
        });
        assert_eq!(l.scroll(), 0.0);
    }
}
