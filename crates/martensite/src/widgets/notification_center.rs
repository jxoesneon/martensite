//! `NotificationCenter` — a stack of persistent notification
//! cards with per-card dismiss and clear-all (the macOS
//! Notification Center / Win11 Action Center idiom — persistent
//! siblings of the transient [`crate::widgets::toast::Toast`]).
//!
//! [`NotificationCenter::push`] prepends a card; clicking a
//! card's `✕` removes it and parks the index in
//! [`NotificationCenter::take_dismissed`]. `Clear all` empties
//! the stack. The wheel scrolls when the stack overflows.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::notification_center::{
//!     Notification, NotificationCenter,
//! };
//!
//! let mut c = NotificationCenter::new();
//! c.push(Notification::new("Build done", "all targets green"));
//! c.push(Notification::new("Sync stalled", "retrying in 30 s"));
//! assert_eq!(c.count(), 2);
//! assert_eq!(c.card(0).unwrap().title, "Sync stalled"); // newest first
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use parking_lot::Mutex;

const W_PT: f32 = 300.0;
const H_PT: f32 = 320.0;
const CARD_PT: f32 = 56.0;
const GAP_PT: f32 = 6.0;
const PAD_PT: f32 = 8.0;
const CLEAR_PT: f32 = 24.0;

const FACE: [u8; 4] = [28, 28, 32, 255];
const CARD: [u8; 4] = [44, 44, 50, 255];
const TITLE: [u8; 4] = [218, 218, 224, 255];
const BODY: [u8; 4] = [160, 160, 168, 255];
const DISMISS: [u8; 4] = [150, 150, 158, 255];
const CLEAR: [u8; 4] = [120, 200, 255, 255];

/// One notification card — see [`NotificationCenter`].
///
/// ```
/// use martensite::widgets::notification_center::Notification;
///
/// assert_eq!(Notification::new("t", "b").title, "t");
/// ```
#[derive(Debug, Clone)]
pub struct Notification {
    /// Bold title line.
    pub title: String,
    /// Body text.
    pub body: String,
    /// Optional small time/app label.
    pub meta: String,
}

impl Notification {
    /// A card with title + body.
    ///
    /// ```
    /// use martensite::widgets::notification_center::Notification;
    ///
    /// assert_eq!(Notification::new("t", "b").meta, "");
    /// ```
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            meta: String::new(),
        }
    }

    /// Small meta label (app name, time).
    ///
    /// ```
    /// use martensite::widgets::notification_center::Notification;
    ///
    /// assert_eq!(Notification::new("t", "b").meta("2m").meta, "2m");
    /// ```
    pub fn meta(mut self, meta: impl Into<String>) -> Self {
        self.meta = meta.into();
        self
    }
}

/// A persistent notification stack — see the module docs.
///
/// ```
/// use martensite::widgets::notification_center::NotificationCenter;
///
/// assert_eq!(NotificationCenter::new().count(), 0);
/// ```
pub struct NotificationCenter {
    /// Accessibility label.
    pub label: String,
    /// Cards, newest first.
    cards: Vec<Notification>,
    dismissed: Option<usize>,
    cleared: bool,
    scroll: f32,
    bounds: Rect,
    scale: f32,
    /// Hit rects painted last frame: `(card_i, ✕ rect)` plus the
    /// clear-all row — behind a mutex since `paint` is `&self`.
    hits: Mutex<Hits>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

#[derive(Debug, Default)]
struct Hits {
    dismiss: Vec<(usize, Rect)>,
    clear: Option<Rect>,
}

impl std::fmt::Debug for NotificationCenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotificationCenter")
            .field("cards", &self.cards.len())
            .finish()
    }
}

impl Default for NotificationCenter {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationCenter {
    /// Creates an empty center.
    ///
    /// ```
    /// use martensite::widgets::notification_center::NotificationCenter;
    ///
    /// assert!(NotificationCenter::new().is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Notifications".to_string(),
            cards: Vec::new(),
            dismissed: None,
            cleared: false,
            scroll: 0.0,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            hits: Mutex::new(Hits::default()),
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::notification_center::NotificationCenter;
    ///
    /// assert_eq!(NotificationCenter::new().label("Alerts").label, "Alerts");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::notification_center::NotificationCenter;
    ///
    /// let _ = NotificationCenter::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Prepends a card (newest on top).
    ///
    /// ```
    /// use martensite::widgets::notification_center::{
    ///     Notification, NotificationCenter,
    /// };
    ///
    /// let mut c = NotificationCenter::new();
    /// c.push(Notification::new("a", "b"));
    /// assert_eq!(c.count(), 1);
    /// ```
    pub fn push(&mut self, n: Notification) {
        self.cards.insert(0, n);
    }

    /// Card at `i` (0 = newest).
    ///
    /// ```
    /// use martensite::widgets::notification_center::{
    ///     Notification, NotificationCenter,
    /// };
    ///
    /// let mut c = NotificationCenter::new();
    /// c.push(Notification::new("t", "b"));
    /// assert_eq!(c.card(0).unwrap().title, "t");
    /// ```
    pub fn card(&self, i: usize) -> Option<&Notification> {
        self.cards.get(i)
    }

    /// Card count.
    ///
    /// ```
    /// use martensite::widgets::notification_center::NotificationCenter;
    ///
    /// assert_eq!(NotificationCenter::new().count(), 0);
    /// ```
    pub fn count(&self) -> usize {
        self.cards.len()
    }

    /// Whether the stack is empty.
    ///
    /// ```
    /// use martensite::widgets::notification_center::NotificationCenter;
    ///
    /// assert!(NotificationCenter::new().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// Removes card `i`.
    ///
    /// ```
    /// use martensite::widgets::notification_center::{
    ///     Notification, NotificationCenter,
    /// };
    ///
    /// let mut c = NotificationCenter::new();
    /// c.push(Notification::new("a", "b"));
    /// c.dismiss(0);
    /// assert!(c.is_empty());
    /// ```
    pub fn dismiss(&mut self, i: usize) {
        if i < self.cards.len() {
            self.cards.remove(i);
        }
    }

    /// Empties the stack.
    ///
    /// ```
    /// use martensite::widgets::notification_center::{
    ///     Notification, NotificationCenter,
    /// };
    ///
    /// let mut c = NotificationCenter::new();
    /// c.push(Notification::new("a", "b"));
    /// c.clear_all();
    /// assert_eq!(c.count(), 0);
    /// ```
    pub fn clear_all(&mut self) {
        self.cards.clear();
    }

    /// Drains the index of the last card dismissed via its `✕`.
    ///
    /// ```
    /// use martensite::widgets::notification_center::NotificationCenter;
    ///
    /// assert_eq!(NotificationCenter::new().take_dismissed(), None);
    /// ```
    pub fn take_dismissed(&mut self) -> Option<usize> {
        self.dismissed.take()
    }

    /// Drains whether `Clear all` fired.
    ///
    /// ```
    /// use martensite::widgets::notification_center::NotificationCenter;
    ///
    /// assert!(!NotificationCenter::new().take_cleared());
    /// ```
    pub fn take_cleared(&mut self) -> bool {
        std::mem::take(&mut self.cleared)
    }

    /// Content height.
    fn content_h(&self) -> f32 {
        let s = self.scale;
        self.cards.len() as f32 * (CARD_PT + GAP_PT) * s + PAD_PT * s * 2.0
    }

    /// Max scroll offset.
    fn max_scroll(&self) -> f32 {
        let clear = if self.cards.is_empty() {
            0.0
        } else {
            CLEAR_PT * self.scale
        };
        (self.content_h() + clear - self.bounds.height()).max(0.0)
    }
}

impl Widget for NotificationCenter {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 80.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        node.set_label(format!("{} — {}", self.label, self.cards.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::Scroll { position, delta } => {
                if self.bounds.contains(*position) {
                    self.scroll = (self.scroll - delta.y).clamp(0.0, self.max_scroll());
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                let hits = self.hits.lock();
                if let Some(r) = hits.clear {
                    if r.contains(*position) {
                        drop(hits);
                        self.clear_all();
                        self.cleared = true;
                        return EventResponse::RequestRepaint;
                    }
                }
                if let Some((i, _)) = hits.dismiss.iter().find(|(_, r)| r.contains(*position)) {
                    let i = *i;
                    drop(hits);
                    self.dismiss(i);
                    self.dismissed = Some(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
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
        let card_h = CARD_PT * s;
        let gap = GAP_PT * s;
        let title_sz = 12.0 * s;
        let body_sz = 10.5 * s;
        let mut hits = self.hits.lock();
        hits.dismiss.clear();
        hits.clear = None;
        cx.list.push_clip(krect(self.bounds));
        for (i, n) in self.cards.iter().enumerate() {
            let y = self.bounds.min_y() + pad + i as f32 * (card_h + gap) - self.scroll;
            if y + card_h < self.bounds.min_y() || y > self.bounds.max_y() {
                continue;
            }
            let r = Rect::new(
                self.bounds.min_x() + pad,
                y,
                self.bounds.width() - pad * 2.0,
                card_h,
            );
            let kr = krect(r);
            cx.list.push_fill_shape(
                kr,
                &martensite_core::shape::Shape::rounded(6.0 * s),
                cx.color(TokenKey::BackgroundColor, CARD),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kr,
                kurbo::Point::new(f64::from(r.min_x() + 8.0 * s), f64::from(y + 7.0 * s)),
                &n.title,
                title_sz,
                cx.color(TokenKey::TextColor, TITLE),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kr,
                kurbo::Point::new(
                    f64::from(r.min_x() + 8.0 * s),
                    f64::from(y + 7.0 * s + title_sz * 1.5),
                ),
                &n.body,
                body_sz,
                cx.color(TokenKey::TextMutedColor, BODY),
            );
            // Meta label at the card's top-right, ✕ beside it.
            let close = Rect::new(r.max_x() - 20.0 * s, y + 6.0 * s, 16.0 * s, 16.0 * s);
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(close.min_x() + 4.0 * s),
                    f64::from(close.min_y() + 2.0 * s),
                ),
                "✕",
                body_sz,
                cx.color(TokenKey::TextMutedColor, DISMISS),
            );
            hits.dismiss.push((i, close));
            if !n.meta.is_empty() {
                let mw = n.meta.chars().count() as f32 * body_sz * 0.55;
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(close.min_x() - mw - 6.0 * s),
                        f64::from(y + 8.0 * s),
                    ),
                    &n.meta,
                    body_sz * 0.85,
                    cx.color(TokenKey::TextMutedColor, BODY),
                );
            }
        }
        // Clear-all row pinned at the stack bottom.
        if !self.cards.is_empty() {
            let cy = self.bounds.min_y() + pad + self.content_h() - pad - self.scroll;
            if cy > self.bounds.min_y() && cy < self.bounds.max_y() {
                let label = "Clear all";
                let lw = label.chars().count() as f32 * body_sz * 0.55;
                let x = self.bounds.max_x() - pad - lw;
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(f64::from(x), f64::from(cy)),
                    label,
                    body_sz,
                    cx.color(TokenKey::AccentColor, CLEAR),
                );
                hits.clear = Some(Rect::new(
                    x - 4.0 * s,
                    cy - 2.0 * s,
                    lw + 8.0 * s,
                    body_sz * 1.6,
                ));
            }
        }
        cx.list.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;
    use martensite_core::PaintList;

    fn laid_out(w: &mut NotificationCenter, wd: f32, h: f32) {
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

    fn painted(w: &NotificationCenter) {
        let theme = martensite_theme::Theme::new("test");
        let mut list = PaintList::new();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: w.bounds,
            scale: 1.0,
            theme: &theme,
            text_painter: None,
        };
        w.paint(&mut cx);
    }

    #[test]
    fn push_prepends() {
        let mut c = NotificationCenter::new();
        c.push(Notification::new("one", "a"));
        c.push(Notification::new("two", "b"));
        assert_eq!(c.card(0).unwrap().title, "two");
        assert_eq!(c.card(1).unwrap().title, "one");
    }

    #[test]
    fn dismiss_click() {
        let mut c = NotificationCenter::new();
        c.push(Notification::new("a", "b"));
        c.push(Notification::new("c", "d"));
        laid_out(&mut c, 300.0, 320.0);
        painted(&c);
        // Card 0's ✕ sits at the card's top-right.
        let close = c.hits.lock().dismiss[0].1;
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(
                    (close.min_x() + close.max_x()) / 2.0,
                    (close.min_y() + close.max_y()) / 2.0,
                ),
                count: 1,
            },
            bounds: c.bounds,
            scale: 1.0,
        });
        assert_eq!(c.count(), 1);
        assert_eq!(c.take_dismissed(), Some(0));
    }

    #[test]
    fn clear_all_row() {
        let mut c = NotificationCenter::new();
        c.push(Notification::new("a", "b"));
        laid_out(&mut c, 300.0, 320.0);
        painted(&c);
        let clear = c.hits.lock().clear.unwrap();
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(
                    (clear.min_x() + clear.max_x()) / 2.0,
                    (clear.min_y() + clear.max_y()) / 2.0,
                ),
                count: 1,
            },
            bounds: c.bounds,
            scale: 1.0,
        });
        assert!(c.is_empty());
        assert!(c.take_cleared());
    }
}
