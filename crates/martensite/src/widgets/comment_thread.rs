//! `CommentThread` — a nested comment list (forum/blog idiom):
//! avatar dot, author + timestamp header, body text, and
//! per-depth indent with a reply connector rail.
//!
//! Distinct from [`MessageList`](crate::widgets::MessageList),
//! which renders flat chat bubbles; a comment thread renders a
//! depth-annotated discussion tree. Clicking a comment's reply
//! affordance parks its id in [`CommentThread::take_reply`];
//! `Up`/`Down` move the focus ring and `Enter` replies.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::comment_thread::{Comment, CommentThread};
//!
//! let t = CommentThread::new()
//!     .comment(Comment::new(1, "ana", "2h", "Top level"))
//!     .comment(Comment::new(2, "ben", "1h", "Nested").depth(1));
//! assert_eq!(t.comment_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const AVATAR_PT: f32 = 24.0;
const ROW_PAD_PT: f32 = 10.0;
const HEADER_PT: f32 = 14.0;
const BODY_PT: f32 = 13.0;
const LINE_PT: f32 = 17.0;
const INDENT_PT: f32 = 28.0;
const REPLY_PT: f32 = 11.0;

const RAIL: [u8; 4] = [90, 95, 110, 120];
const FOCUS: [u8; 4] = [110, 140, 230, 120];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const MUTED: [u8; 4] = [150, 155, 170, 255];
const LINK: [u8; 4] = [140, 170, 250, 255];
const AVATAR: [u8; 4] = [110, 130, 200, 255];

/// One comment: id, author, relative timestamp, body, nest depth.
///
/// ```
/// use martensite::widgets::comment_thread::Comment;
///
/// let c = Comment::new(7, "ana", "2h", "Hello").depth(2);
/// assert_eq!(c.depth, 2);
/// ```
#[derive(Clone, Debug)]
pub struct Comment {
    /// Stable id (host correlation key).
    pub id: u64,
    /// Author name.
    pub author: String,
    /// Relative timestamp text.
    pub time: String,
    /// Body text.
    pub body: String,
    /// Nest depth (0 = top level).
    pub depth: usize,
    /// Avatar dot color override.
    pub avatar_color: Option<[u8; 4]>,
}

impl Comment {
    /// Top-level comment.
    ///
    /// ```
    /// use martensite::widgets::comment_thread::Comment;
    ///
    /// assert_eq!(Comment::new(1, "a", "now", "hi").depth, 0);
    /// ```
    pub fn new(
        id: u64,
        author: impl Into<String>,
        time: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            id,
            author: author.into(),
            time: time.into(),
            body: body.into(),
            depth: 0,
            avatar_color: None,
        }
    }

    /// Nest depth.
    ///
    /// ```
    /// use martensite::widgets::comment_thread::Comment;
    ///
    /// assert_eq!(Comment::new(1, "a", "t", "b").depth(3).depth, 3);
    /// ```
    pub fn depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    /// Avatar dot color.
    ///
    /// ```
    /// use martensite::widgets::comment_thread::Comment;
    ///
    /// assert_eq!(Comment::new(1, "a", "t", "b").avatar_color([1; 4]).avatar_color, Some([1; 4]));
    /// ```
    pub fn avatar_color(mut self, color: [u8; 4]) -> Self {
        self.avatar_color = Some(color);
        self
    }
}

/// The thread — see the module docs.
///
/// ```
/// use martensite::widgets::comment_thread::CommentThread;
///
/// assert_eq!(CommentThread::new().comment_count(), 0);
/// ```
pub struct CommentThread {
    /// Accessibility label.
    pub label: String,
    /// Show the reply affordance under each body.
    pub show_reply: bool,
    comments: Vec<Comment>,
    focused: Option<usize>,
    reply: Option<u64>,
    activated: Option<u64>,
    rows: Vec<Rect>,
    reply_rects: Vec<Rect>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for CommentThread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommentThread")
            .field("comments", &self.comments.len())
            .finish()
    }
}

impl Default for CommentThread {
    fn default() -> Self {
        Self::new()
    }
}

impl CommentThread {
    /// Empty thread.
    ///
    /// ```
    /// use martensite::widgets::comment_thread::CommentThread;
    ///
    /// assert_eq!(CommentThread::new().comment_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Comments".to_string(),
            show_reply: true,
            comments: Vec::new(),
            focused: None,
            reply: None,
            activated: None,
            rows: Vec::new(),
            reply_rects: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a comment.
    ///
    /// ```
    /// use martensite::widgets::comment_thread::{Comment, CommentThread};
    ///
    /// assert_eq!(CommentThread::new().comment(Comment::new(1, "a", "t", "b")).comment_count(), 1);
    /// ```
    pub fn comment(mut self, comment: Comment) -> Self {
        self.comments.push(comment);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::comment_thread::CommentThread;
    ///
    /// assert_eq!(CommentThread::new().label("Discuss").label, "Discuss");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::comment_thread::CommentThread;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _t = CommentThread::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Comment count.
    ///
    /// ```
    /// use martensite::widgets::comment_thread::CommentThread;
    ///
    /// assert_eq!(CommentThread::new().comment_count(), 0);
    /// ```
    pub fn comment_count(&self) -> usize {
        self.comments.len()
    }

    /// Focused comment index.
    ///
    /// ```
    /// use martensite::widgets::comment_thread::CommentThread;
    ///
    /// assert_eq!(CommentThread::new().focused(), None);
    /// ```
    pub fn focused(&self) -> Option<usize> {
        self.focused
    }

    /// Drains a reply request (comment id).
    ///
    /// ```
    /// use martensite::widgets::comment_thread::CommentThread;
    ///
    /// let mut t = CommentThread::new();
    /// assert_eq!(t.take_reply(), None);
    /// ```
    pub fn take_reply(&mut self) -> Option<u64> {
        self.reply.take()
    }

    /// Drains an activation (body click / Enter on focus).
    ///
    /// ```
    /// use martensite::widgets::comment_thread::CommentThread;
    ///
    /// let mut t = CommentThread::new();
    /// assert_eq!(t.take_activated(), None);
    /// ```
    pub fn take_activated(&mut self) -> Option<u64> {
        self.activated.take()
    }
}

fn row_h(scale: f32) -> f32 {
    (AVATAR_PT + ROW_PAD_PT + LINE_PT + REPLY_PT) * scale
}

impl Widget for CommentThread {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = self.comments.len() as f32 * row_h(cx.scale);
        Vec2::new(
            (280.0 * cx.scale).min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(180.0, 80.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        self.rows.clear();
        self.reply_rects.clear();
        let mut y = bounds.min_y();
        for c in &self.comments {
            let x = bounds.min_x() + c.depth.min(6) as f32 * INDENT_PT * s;
            let w = (bounds.max_x() - x).max(0.0);
            let h = row_h(s);
            self.rows.push(Rect::new(x, y, w, h));
            // Reply affordance bottom-left of the row.
            let rw = 40.0 * s;
            self.reply_rects.push(Rect::new(
                x + AVATAR_PT * s + ROW_PAD_PT * s,
                y + h - REPLY_PT * s,
                rw,
                REPLY_PT * s,
            ));
            y += h;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        node.set_label(self.label.clone());
        node.set_value(format!("{} comments", self.comments.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowDown" | "ArrowUp" => {
                    if self.comments.is_empty() {
                        return EventResponse::Ignored;
                    }
                    let cur = self.focused.unwrap_or(usize::MAX);
                    self.focused = Some(match key.as_str() {
                        "ArrowDown" => {
                            if cur == usize::MAX {
                                0
                            } else {
                                (cur + 1).min(self.comments.len() - 1)
                            }
                        }
                        _ => {
                            if cur == usize::MAX {
                                0
                            } else {
                                cur.saturating_sub(1)
                            }
                        }
                    });
                    EventResponse::RequestRepaint
                }
                "Enter" => {
                    if let Some(i) = self.focused {
                        self.reply = Some(self.comments[i].id);
                        return EventResponse::Handled;
                    }
                    EventResponse::Ignored
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                for (i, rr) in self.reply_rects.iter().enumerate() {
                    if rr.contains(*position) {
                        self.reply = Some(self.comments[i].id);
                        return EventResponse::Handled;
                    }
                }
                for (i, r) in self.rows.iter().enumerate() {
                    if r.contains(*position) {
                        self.focused = Some(i);
                        self.activated = Some(self.comments[i].id);
                        return EventResponse::RequestRepaint;
                    }
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        // Rows stack from `bounds.min_y` and can extend past the
        // widget's bottom edge; clip to the widget so partially
        // visible rows paint only their visible slice and fully
        // offscreen rows emit nothing.
        let visible = krect(self.bounds);
        cx.list.push_clip(visible);
        for (i, c) in self.comments.iter().enumerate() {
            let row = self.rows[i];
            if self.focused == Some(i) {
                cx.list.push_fill_rect(krect(row), FOCUS);
            }
            // Connector rail for nested comments.
            if c.depth > 0 {
                let rail_x = row.min_x() - INDENT_PT * 0.5 * s;
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(rail_x),
                        f64::from(row.min_y()),
                        f64::from(rail_x + s.max(1.0)),
                        f64::from(row.max_y()),
                    ),
                    RAIL,
                );
            }
            // Avatar dot with initial.
            let av = AVATAR_PT * s;
            let ar = kurbo::Rect::new(
                f64::from(row.min_x()),
                f64::from(row.min_y()),
                f64::from(row.min_x() + av),
                f64::from(row.min_y() + av),
            );
            cx.list.push_fill_shape(
                ar,
                &martensite_core::shape::Shape::ELLIPSE,
                c.avatar_color.unwrap_or(AVATAR),
            );
            let row_clip = krect(row).intersect(visible);
            let initial: String = c.author.chars().take(1).collect();
            let isize_ = HEADER_PT * 0.8 * s;
            let iw = painter
                .and_then(|p| p.measure_text(&initial, isize_))
                .unwrap_or(isize_ * 0.5);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                ar.intersect(visible),
                kurbo::Point::new(
                    f64::from(row.min_x() + (av - iw) / 2.0),
                    f64::from(row.min_y() + av / 2.0),
                ),
                &initial,
                isize_,
                cx.color(TokenKey::TextInverseColor, TEXT),
            );
            // Header: author + time.
            let tx = row.min_x() + av + ROW_PAD_PT * s;
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                row_clip,
                kurbo::Point::new(f64::from(tx), f64::from(row.min_y() + HEADER_PT * s)),
                &format!("{} · {}", c.author, c.time),
                HEADER_PT * s,
                cx.color(TokenKey::TextMutedColor, MUTED),
            );
            // Body.
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                row_clip,
                kurbo::Point::new(
                    f64::from(tx),
                    f64::from(row.min_y() + av + LINE_PT * 0.8 * s),
                ),
                &c.body,
                BODY_PT * s,
                cx.color(TokenKey::TextColor, TEXT),
            );
            // Reply affordance.
            if self.show_reply {
                let rr = self.reply_rects[i];
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    krect(rr).intersect(visible),
                    kurbo::Point::new(f64::from(rr.min_x()), f64::from(rr.max_y())),
                    "Reply",
                    REPLY_PT * s,
                    cx.color(TokenKey::AccentColor, LINK),
                );
            }
        }
        cx.list.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> CommentThread {
        CommentThread::new()
            .comment(Comment::new(1, "ana", "2h", "Top"))
            .comment(Comment::new(2, "ben", "1h", "Nested").depth(1))
            .comment(Comment::new(3, "cat", "30m", "Deeper").depth(2))
    }

    fn laid_out(t: &mut CommentThread) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        t.layout(&mut cx, Rect::new(0.0, 0.0, 400.0, 300.0));
    }

    fn ev(t: &mut CommentThread, e: &WidgetEvent) -> EventResponse {
        t.event(&mut EventContext {
            event: e,
            bounds: t.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn nested_rows_indent() {
        let mut t = fixture();
        laid_out(&mut t);
        assert!(t.rows[1].min_x() > t.rows[0].min_x());
        assert!(t.rows[2].min_x() > t.rows[1].min_x());
    }

    #[test]
    fn reply_click_parks_id() {
        let mut t = fixture();
        laid_out(&mut t);
        let rr = t.reply_rects[1];
        ev(
            &mut t,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(
                    (rr.min_x() + rr.max_x()) / 2.0,
                    (rr.min_y() + rr.max_y()) / 2.0,
                ),
            },
        );
        assert_eq!(t.take_reply(), Some(2));
    }

    #[test]
    fn keys_focus_and_enter_replies() {
        let mut t = fixture();
        laid_out(&mut t);
        ev(
            &mut t,
            &WidgetEvent::KeyPressed {
                key: "ArrowDown".to_string(),
                repeat: false,
            },
        );
        assert_eq!(t.focused(), Some(0));
        ev(
            &mut t,
            &WidgetEvent::KeyPressed {
                key: "ArrowDown".to_string(),
                repeat: false,
            },
        );
        assert_eq!(t.focused(), Some(1));
        ev(
            &mut t,
            &WidgetEvent::KeyPressed {
                key: "Enter".to_string(),
                repeat: false,
            },
        );
        assert_eq!(t.take_reply(), Some(2));
    }

    #[test]
    fn body_click_focuses_and_activates() {
        let mut t = fixture();
        laid_out(&mut t);
        let r = t.rows[0];
        ev(
            &mut t,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(r.min_x() + 60.0, r.min_y() + 12.0),
            },
        );
        assert_eq!(t.focused(), Some(0));
        assert_eq!(t.take_activated(), Some(1));
    }

    #[test]
    fn paint_without_painter() {
        let mut t = fixture();
        laid_out(&mut t);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        t.paint(&mut PaintContext {
            list: &mut list,
            bounds: t.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
