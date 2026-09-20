//! `Poll` — a voting widget: question + option rows that show
//! percentage bars and counts once voted (Slack/Telegram poll
//! idiom).
//!
//! Clicking an option casts the user's vote — switching moves it —
//! and the index parks in [`Poll::take_voted`] for the host to
//! sync. [`Poll::close`] freezes voting and reveals the final
//! bars. Before voting (or in anonymous mode) rows show as plain
//! selectable options.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::poll::{Poll, PollOption};
//!
//! let p = Poll::new("Lunch?")
//!     .option(PollOption::new("Pizza", 3))
//!     .option(PollOption::new("Sushi", 1));
//! assert_eq!(p.option_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const FONT_PT: f32 = 13.0;
const Q_FONT_PT: f32 = 14.0;
const ROW_H_PT: f32 = 32.0;
const ROW_GAP_PT: f32 = 6.0;
const Q_GAP_PT: f32 = 10.0;
const PAD_PT: f32 = 12.0;
const RADIUS_PT: f32 = 7.0;

const FACE: [u8; 4] = [36, 38, 44, 255];
const BAR: [u8; 4] = [44, 62, 92, 255];
const ACCENT: [u8; 4] = [88, 130, 247, 255];
const EDGE: [u8; 4] = [70, 72, 80, 255];
const TEXT: [u8; 4] = [220, 222, 228, 255];
const MUTED: [u8; 4] = [139, 148, 158, 255];

/// One poll choice.
///
/// ```
/// use martensite::widgets::poll::PollOption;
///
/// let o = PollOption::new("Pizza", 3);
/// assert_eq!(o.votes, 3);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct PollOption {
    /// Option label.
    pub label: String,
    /// Vote count.
    pub votes: u32,
}

impl PollOption {
    /// New option.
    ///
    /// ```
    /// use martensite::widgets::poll::PollOption;
    ///
    /// assert_eq!(PollOption::new("A", 0).votes, 0);
    /// ```
    pub fn new(label: impl Into<String>, votes: u32) -> Self {
        Self {
            label: label.into(),
            votes,
        }
    }
}

/// A voting widget — see the module docs.
///
/// ```
/// use martensite::widgets::poll::Poll;
///
/// assert_eq!(Poll::new("q").option_count(), 0);
/// ```
pub struct Poll {
    /// The poll question.
    pub question: String,
    /// Closed polls show results and reject votes.
    pub closed: bool,
    /// Anonymous polls hide counts until closed.
    pub anonymous: bool,
    options: Vec<PollOption>,
    my_vote: Option<usize>,
    voted: Option<usize>,
    held: Option<usize>,
    rects: Vec<Rect>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Poll {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Poll")
            .field("question", &self.question)
            .field("options", &self.options)
            .field("my_vote", &self.my_vote)
            .finish()
    }
}

impl Poll {
    /// Poll with a question and no options.
    ///
    /// ```
    /// use martensite::widgets::poll::Poll;
    ///
    /// assert_eq!(Poll::new("Lunch?").question, "Lunch?");
    /// ```
    pub fn new(question: impl Into<String>) -> Self {
        Self {
            question: question.into(),
            closed: false,
            anonymous: false,
            options: Vec::new(),
            my_vote: None,
            voted: None,
            held: None,
            rects: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends an option.
    ///
    /// ```
    /// use martensite::widgets::poll::{Poll, PollOption};
    ///
    /// assert_eq!(Poll::new("q").option(PollOption::new("a", 0)).option_count(), 1);
    /// ```
    pub fn option(mut self, option: PollOption) -> Self {
        self.options.push(option);
        self
    }

    /// Closed builder.
    ///
    /// ```
    /// use martensite::widgets::poll::Poll;
    ///
    /// assert!(Poll::new("q").closed(true).closed);
    /// ```
    pub fn closed(mut self, closed: bool) -> Self {
        self.closed = closed;
        self
    }

    /// Anonymous builder.
    ///
    /// ```
    /// use martensite::widgets::poll::Poll;
    ///
    /// assert!(Poll::new("q").anonymous(true).anonymous);
    /// ```
    pub fn anonymous(mut self, anonymous: bool) -> Self {
        self.anonymous = anonymous;
        self
    }

    /// Shared text painter for real glyph metrics.
    ///
    /// ```no_run
    /// use martensite::widgets::poll::Poll;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _p = Poll::new("q").with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Number of options.
    ///
    /// ```
    /// use martensite::widgets::poll::Poll;
    ///
    /// assert_eq!(Poll::new("q").option_count(), 0);
    /// ```
    pub fn option_count(&self) -> usize {
        self.options.len()
    }

    /// Total votes across options.
    ///
    /// ```
    /// use martensite::widgets::poll::{Poll, PollOption};
    ///
    /// assert_eq!(
    ///     Poll::new("q").option(PollOption::new("a", 2)).option(PollOption::new("b", 3)).total_votes(),
    ///     5
    /// );
    /// ```
    pub fn total_votes(&self) -> u32 {
        self.options.iter().map(|o| o.votes).sum()
    }

    /// The user's voted option index.
    ///
    /// ```
    /// use martensite::widgets::poll::Poll;
    ///
    /// assert_eq!(Poll::new("q").my_vote(), None);
    /// ```
    pub fn my_vote(&self) -> Option<usize> {
        self.my_vote
    }

    /// An option by index.
    ///
    /// ```
    /// use martensite::widgets::poll::{Poll, PollOption};
    ///
    /// assert_eq!(Poll::new("q").option(PollOption::new("a", 4)).option_at(0).unwrap().votes, 4);
    /// ```
    pub fn option_at(&self, index: usize) -> Option<&PollOption> {
        self.options.get(index)
    }

    /// Whether result bars are visible (voted or closed).
    ///
    /// ```
    /// use martensite::widgets::poll::Poll;
    ///
    /// assert!(!Poll::new("q").shows_results());
    /// ```
    pub fn shows_results(&self) -> bool {
        self.closed || (self.my_vote.is_some() && !self.anonymous)
    }

    /// Drains the last cast vote index.
    ///
    /// ```
    /// use martensite::widgets::poll::Poll;
    ///
    /// let mut p = Poll::new("q");
    /// assert_eq!(p.take_voted(), None);
    /// ```
    pub fn take_voted(&mut self) -> Option<usize> {
        self.voted.take()
    }

    /// Closes the poll — freezes voting, reveals results.
    ///
    /// ```
    /// use martensite::widgets::poll::Poll;
    ///
    /// let mut p = Poll::new("q");
    /// p.close();
    /// assert!(p.closed);
    /// ```
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// Casts or moves the user's vote.
    fn vote(&mut self, index: usize) {
        if self.closed || index >= self.options.len() {
            return;
        }
        if self.my_vote == Some(index) {
            return;
        }
        if let Some(prev) = self.my_vote {
            if let Some(o) = self.options.get_mut(prev) {
                o.votes = o.votes.saturating_sub(1);
            }
        }
        if let Some(o) = self.options.get_mut(index) {
            o.votes = o.votes.saturating_add(1);
        }
        self.my_vote = Some(index);
        self.voted = Some(index);
    }

    /// Option row hit-test.
    fn row_at(&self, p: Vec2) -> Option<usize> {
        self.rects.iter().position(|r| r.contains(p))
    }
}

impl Widget for Poll {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let h = (Q_FONT_PT + Q_GAP_PT + PAD_PT * 2.0) * s
            + self.options.len() as f32 * (ROW_H_PT + ROW_GAP_PT) * s;
        Vec2::new(
            constraints.max_size.x.max(200.0 * s).min(320.0 * s),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.rects.clear();
        let s = cx.scale;
        let mut y = bounds.min_y() + PAD_PT * s + (Q_FONT_PT + Q_GAP_PT) * s;
        let w = bounds.width() - PAD_PT * 2.0 * s;
        for _ in &self.options {
            self.rects
                .push(Rect::new(bounds.min_x() + PAD_PT * s, y, w, ROW_H_PT * s));
            y += (ROW_H_PT + ROW_GAP_PT) * s;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.question.clone());
        let total = self.total_votes();
        node.set_value(format!("{total} votes"));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if self.closed {
                    return EventResponse::Ignored;
                }
                if let Some(i) = self.row_at(*position) {
                    self.held = Some(i);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(i) = self.held.take() {
                    if self.row_at(*position) == Some(i) {
                        self.vote(i);
                    }
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } if !self.closed => {
                // Digit keys 1-9 vote directly.
                let idx = key
                    .chars()
                    .next()
                    .and_then(|c| c.to_digit(10))
                    .map(|d| d as usize - 1);
                match idx {
                    Some(i) if i < self.options.len() => {
                        self.vote(i);
                        EventResponse::RequestRepaint
                    }
                    _ => EventResponse::Ignored,
                }
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
        let shape = martensite_core::shape::Shape::rounded(RADIUS_PT * s);
        let text = cx.color(TokenKey::TextColor, TEXT);
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        // Question.
        let q = kurbo::Point::new(
            f64::from(self.bounds.min_x() + PAD_PT * s),
            f64::from(self.bounds.min_y() + PAD_PT * s + Q_FONT_PT * s * 0.7),
        );
        crate::text_paint::paint_label(painter, cx.list, q, &self.question, Q_FONT_PT * s, text);
        let results = self.shows_results();
        let total = self.total_votes().max(1) as f32;
        let size = FONT_PT * s;
        for (i, o) in self.options.iter().enumerate() {
            let rect = self.rects[i];
            let face = cx.color(TokenKey::SurfaceColor, FACE);
            let edge = if self.my_vote == Some(i) {
                cx.color(TokenKey::AccentColor, ACCENT)
            } else {
                cx.color(TokenKey::DividerColor, EDGE)
            };
            cx.list.push_fill_shape(krect(rect), &shape, face);
            // Result bar.
            if results {
                let frac = o.votes as f32 / total;
                if frac > 0.0 {
                    let bar = Rect::new(
                        rect.min_x(),
                        rect.min_y(),
                        rect.width() * frac,
                        rect.height(),
                    );
                    let fill = cx.color(TokenKey::SecondaryColor, BAR);
                    cx.list.push_fill_shape(krect(bar), &shape, fill);
                }
            }
            cx.list
                .push_stroke_shape(krect(rect), &shape, 1.0 * s, edge);
            // Label (+ check for my vote).
            let label = if self.my_vote == Some(i) {
                format!("✓ {}", o.label)
            } else {
                o.label.clone()
            };
            let origin = kurbo::Point::new(
                f64::from(rect.min_x() + PAD_PT * s * 0.8),
                f64::from(rect.min_y() + rect.height() / 2.0),
            );
            crate::text_paint::paint_label(painter, cx.list, origin, &label, size, text);
            // Percentage / count on the right.
            if results {
                let pct = format!("{:.0}%", o.votes as f32 / total * 100.0);
                let w = painter
                    .and_then(|p| p.measure_text(&pct, size))
                    .unwrap_or(pct.len() as f32 * size * 0.6);
                let po = kurbo::Point::new(
                    f64::from(rect.max_x() - w - PAD_PT * s * 0.8),
                    f64::from(rect.min_y() + rect.height() / 2.0),
                );
                crate::text_paint::paint_label(painter, cx.list, po, &pct, size, muted);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> Poll {
        Poll::new("Lunch?")
            .option(PollOption::new("Pizza", 3))
            .option(PollOption::new("Sushi", 1))
    }

    fn laid_out(p: &mut Poll) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        p.layout(&mut cx, Rect::new(0.0, 0.0, 280.0, 140.0));
    }

    fn ev(p: &mut Poll, e: &WidgetEvent) -> EventResponse {
        p.event(&mut EventContext {
            event: e,
            bounds: p.bounds,
            scale: 1.0,
        })
    }

    fn tap_row(p: &mut Poll, i: usize) {
        let r = p.rects[i];
        let mid = Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0);
        ev(
            p,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: mid,
                count: 1,
            },
        );
        ev(
            p,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: mid,
            },
        );
    }

    #[test]
    fn click_casts_vote() {
        let mut p = fixture();
        laid_out(&mut p);
        tap_row(&mut p, 1);
        assert_eq!(p.my_vote(), Some(1));
        assert_eq!(p.option_at(1).unwrap().votes, 2);
        assert_eq!(p.take_voted(), Some(1));
        assert!(p.shows_results());
    }

    #[test]
    fn vote_moves_between_options() {
        let mut p = fixture();
        laid_out(&mut p);
        tap_row(&mut p, 0);
        tap_row(&mut p, 1);
        assert_eq!(p.option_at(0).unwrap().votes, 3); // back to 3
        assert_eq!(p.option_at(1).unwrap().votes, 2);
        assert_eq!(p.my_vote(), Some(1));
    }

    #[test]
    fn closed_rejects_votes() {
        let mut p = fixture().closed(true);
        laid_out(&mut p);
        tap_row(&mut p, 0);
        assert_eq!(p.my_vote(), None);
        assert_eq!(p.option_at(0).unwrap().votes, 3);
    }

    #[test]
    fn digit_key_votes() {
        let mut p = fixture();
        laid_out(&mut p);
        ev(
            &mut p,
            &WidgetEvent::KeyPressed {
                key: "2".to_string(),
                repeat: false,
            },
        );
        assert_eq!(p.my_vote(), Some(1));
    }

    #[test]
    fn anonymous_hides_counts() {
        let mut p = fixture().anonymous(true);
        laid_out(&mut p);
        tap_row(&mut p, 0);
        assert!(p.my_vote().is_some());
        assert!(!p.shows_results());
        p.close();
        assert!(p.shows_results());
    }

    #[test]
    fn paint_without_painter() {
        let mut p = fixture();
        laid_out(&mut p);
        tap_row(&mut p, 0);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        p.paint(&mut PaintContext {
            list: &mut list,
            bounds: p.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
