//! `PageFlip` — the ebook-reader two-page spread: left/right page
//! faces with a center gutter, edge clicks or arrow keys turn the
//! spread, and a `3–4 / 8` page counter.
//!
//! Pages are plain text bodies supplied by the host; the widget
//! renders them line-wrapped within each face. Turning parks the
//! new left-page index in [`PageFlip::take_turned`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::page_flip::PageFlip;
//!
//! let p = PageFlip::new().page("one").page("two").page("three");
//! assert_eq!(p.page_count(), 3);
//! ```

use crate::text_paint::SharedTextPainter;
use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};

const PAD_PT: f32 = 16.0;
const GUTTER_PT: f32 = 2.0;
const BODY_PT: f32 = 12.5;
const LINE_PT: f32 = 17.0;
const NUM_PT: f32 = 11.0;

const PAGE: [u8; 4] = [244, 240, 228, 255];
const INK: [u8; 4] = [40, 38, 34, 255];
const GUTTER: [u8; 4] = [120, 112, 96, 255];
const MUTED: [u8; 4] = [120, 112, 96, 255];

/// The reader — see the module docs.
///
/// ```
/// use martensite::widgets::page_flip::PageFlip;
///
/// assert_eq!(PageFlip::new().page_count(), 0);
/// ```
pub struct PageFlip {
    /// Accessibility label.
    pub label: String,
    /// Show the `n–m / N` counter.
    pub show_counter: bool,
    pages: Vec<String>,
    left: usize,
    turned: Option<usize>,
    left_rect: Rect,
    right_rect: Rect,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for PageFlip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageFlip")
            .field("pages", &self.pages.len())
            .field("left", &self.left)
            .finish()
    }
}

impl Default for PageFlip {
    fn default() -> Self {
        Self::new()
    }
}

impl PageFlip {
    /// Empty reader.
    ///
    /// ```
    /// use martensite::widgets::page_flip::PageFlip;
    ///
    /// assert_eq!(PageFlip::new().page_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Book".to_string(),
            show_counter: true,
            pages: Vec::new(),
            left: 0,
            turned: None,
            left_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            right_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a page of body text.
    ///
    /// ```
    /// use martensite::widgets::page_flip::PageFlip;
    ///
    /// assert_eq!(PageFlip::new().page("a").page_count(), 1);
    /// ```
    pub fn page(mut self, text: impl Into<String>) -> Self {
        self.pages.push(text.into());
        self
    }

    /// Pages from an iterator.
    ///
    /// ```
    /// use martensite::widgets::page_flip::PageFlip;
    ///
    /// assert_eq!(PageFlip::new().pages(["a", "b"]).page_count(), 2);
    /// ```
    pub fn pages(mut self, pages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.pages.extend(pages.into_iter().map(Into::into));
        self
    }

    /// Starting left-page index (even).
    ///
    /// ```
    /// use martensite::widgets::page_flip::PageFlip;
    ///
    /// assert_eq!(PageFlip::new().pages(["a", "b", "c"]).start(2).left_page(), Some(2));
    /// ```
    pub fn start(mut self, left: usize) -> Self {
        self.left = left;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::page_flip::PageFlip;
    ///
    /// assert_eq!(PageFlip::new().label("Novel").label, "Novel");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter for page bodies.
    ///
    /// ```no_run
    /// use martensite::widgets::page_flip::PageFlip;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _b = PageFlip::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Page count.
    ///
    /// ```
    /// use martensite::widgets::page_flip::PageFlip;
    ///
    /// assert_eq!(PageFlip::new().page_count(), 0);
    /// ```
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Current left-page index (always even).
    ///
    /// ```
    /// use martensite::widgets::page_flip::PageFlip;
    ///
    /// assert_eq!(PageFlip::new().left_page(), None);
    /// ```
    pub fn left_page(&self) -> Option<usize> {
        (!self.pages.is_empty()).then(|| (self.left / 2) * 2)
    }

    /// Jumps to a left-page index (host-driven).
    ///
    /// ```
    /// use martensite::widgets::page_flip::PageFlip;
    ///
    /// let mut p = PageFlip::new().pages(["a", "b", "c"]);
    /// p.set_left(2);
    /// assert_eq!(p.left_page(), Some(2));
    /// ```
    pub fn set_left(&mut self, left: usize) {
        if !self.pages.is_empty() {
            self.left = (left / 2 * 2).min(self.pages.len() - 1);
        }
    }

    /// Turns by `d` spreads (±1 typical), clamped; parks the new
    /// left index when it changed.
    ///
    /// ```
    /// use martensite::widgets::page_flip::PageFlip;
    ///
    /// let mut p = PageFlip::new().pages(["a", "b", "c", "d", "e"]);
    /// p.turn(1);
    /// assert_eq!(p.left_page(), Some(2));
    /// assert_eq!(p.take_turned(), Some(2));
    /// ```
    pub fn turn(&mut self, d: isize) {
        if self.pages.is_empty() {
            return;
        }
        let cur = (self.left / 2 * 2) as isize;
        let next = (cur + d * 2).clamp(0, (self.pages.len() - 1) as isize) as usize / 2 * 2;
        if next != cur as usize {
            self.left = next;
            self.turned = Some(next);
        }
    }

    /// Drains the turned-to left-page index.
    ///
    /// ```
    /// use martensite::widgets::page_flip::PageFlip;
    ///
    /// let mut p = PageFlip::new();
    /// assert_eq!(p.take_turned(), None);
    /// ```
    pub fn take_turned(&mut self) -> Option<usize> {
        self.turned.take()
    }
}

impl Widget for PageFlip {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        constraints.max_size
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(260.0, 180.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let half = (bounds.width() - GUTTER_PT * s) / 2.0;
        self.left_rect = Rect::new(bounds.min_x(), bounds.min_y(), half, bounds.height());
        self.right_rect = Rect::new(
            bounds.min_x() + half + GUTTER_PT * s,
            bounds.min_y(),
            half,
            bounds.height(),
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Document);
        node.set_label(self.label.clone());
        if let Some(l) = self.left_page() {
            node.set_value(format!("pages {}–{}", l + 1, (l + 2).min(self.pages.len())));
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowRight" | "PageDown" | " " => {
                    self.turn(1);
                    EventResponse::RequestRepaint
                }
                "ArrowLeft" | "PageUp" => {
                    self.turn(-1);
                    EventResponse::RequestRepaint
                }
                "Home" => {
                    if !self.pages.is_empty() && self.left != 0 {
                        self.left = 0;
                        self.turned = Some(0);
                    }
                    EventResponse::RequestRepaint
                }
                "End" => {
                    if !self.pages.is_empty() {
                        let last = (self.pages.len() - 1) / 2 * 2;
                        if self.left != last {
                            self.left = last;
                            self.turned = Some(last);
                        }
                    }
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if self.left_rect.contains(*position) {
                    self.turn(-1);
                    EventResponse::RequestRepaint
                } else if self.right_rect.contains(*position) {
                    self.turn(1);
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
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
        let l = self.left_page().unwrap_or(0);
        for (rect, page_idx) in [(self.left_rect, l), (self.right_rect, l + 1)] {
            cx.list.push_fill_rect(krect(rect), PAGE);
            if let Some(text) = self.pages.get(page_idx) {
                // Lay the body out word-wrapped to the inner lane —
                // measured through the painter so line breaks land on
                // word boundaries, not mid-glyph char chunks.
                let inner_x = rect.min_x() + PAD_PT * s;
                let inner_w = (rect.width() - PAD_PT * 2.0 * s).max(0.0);
                let body_sz = BODY_PT * s;
                let char_w = BODY_PT * 0.55 * s;
                let width_of = |t: &str| {
                    painter
                        .and_then(|p| p.measure_text(t, body_sz))
                        .unwrap_or_else(|| t.chars().count() as f32 * char_w)
                };
                let max_lines = ((rect.height() - PAD_PT * 2.0 * s) / (LINE_PT * s)) as usize;
                // Greedy word-wrap; a word wider than the lane is
                // broken mid-word at the last fitting char boundary.
                let mut rows: Vec<String> = Vec::new();
                let mut cur = String::new();
                for word in text.split_whitespace() {
                    let mut w = word;
                    loop {
                        let cand = if cur.is_empty() {
                            w.to_string()
                        } else {
                            format!("{cur} {w}")
                        };
                        if width_of(&cand) <= inner_w {
                            cur = cand;
                            break;
                        }
                        if !cur.is_empty() {
                            rows.push(std::mem::take(&mut cur));
                            continue;
                        }
                        // Lone word overruns the lane — break at the
                        // last fitting char boundary (≥1 char).
                        let mut fit = 0usize;
                        for (b, _) in w.char_indices().skip(1) {
                            if width_of(&w[..b]) > inner_w {
                                break;
                            }
                            fit = b;
                        }
                        if fit == 0 {
                            fit = w.chars().next().map_or(0, char::len_utf8);
                        }
                        rows.push(w[..fit].to_string());
                        w = &w[fit..];
                    }
                }
                if !cur.is_empty() {
                    rows.push(cur);
                }
                for (li, line) in rows.iter().take(max_lines).enumerate() {
                    crate::text_paint::paint_label(
                        painter,
                        cx.list,
                        kurbo::Point::new(
                            f64::from(inner_x),
                            f64::from(rect.min_y() + PAD_PT * s + li as f32 * LINE_PT * s),
                        ),
                        line,
                        body_sz,
                        INK,
                    );
                }
            }
        }
        // Gutter.
        let gx = self.left_rect.max_x();
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(gx),
                f64::from(self.bounds.min_y()),
                f64::from(gx + GUTTER_PT * s),
                f64::from(self.bounds.max_y()),
            ),
            GUTTER,
        );
        // Counter.
        if self.show_counter && !self.pages.is_empty() {
            let counter = format!(
                "{}–{} / {}",
                l + 1,
                (l + 2).min(self.pages.len()),
                self.pages.len()
            );
            let w = painter
                .and_then(|p| p.measure_text(&counter, NUM_PT * s))
                .unwrap_or(counter.len() as f32 * NUM_PT * 0.6 * s);
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(self.bounds.min_x() + (self.bounds.width() - w) / 2.0),
                    f64::from(self.bounds.max_y() - PAD_PT * 0.5 * s),
                ),
                &counter,
                NUM_PT * s,
                MUTED,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> PageFlip {
        PageFlip::new().pages(["a", "b", "c", "d", "e"])
    }

    fn laid_out(p: &mut PageFlip) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        p.layout(&mut cx, Rect::new(0.0, 0.0, 600.0, 400.0));
    }

    fn ev(p: &mut PageFlip, e: &WidgetEvent) -> EventResponse {
        p.event(&mut EventContext {
            event: e,
            bounds: p.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn turn_steps_spreads_and_clamps() {
        let mut p = fixture();
        p.turn(1);
        assert_eq!(p.left_page(), Some(2));
        assert_eq!(p.take_turned(), Some(2));
        p.turn(5); // clamps to last spread (page 4)
        assert_eq!(p.left_page(), Some(4));
        p.turn(-1);
        assert_eq!(p.left_page(), Some(2));
    }

    #[test]
    fn right_click_turns_forward() {
        let mut p = fixture();
        laid_out(&mut p);
        let r = p.right_rect;
        ev(
            &mut p,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
        );
        assert_eq!(p.left_page(), Some(2));
    }

    #[test]
    fn left_click_turns_back() {
        let mut p = fixture();
        laid_out(&mut p);
        p.set_left(2);
        let r = p.left_rect;
        ev(
            &mut p,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
        );
        assert_eq!(p.left_page(), Some(0));
    }

    #[test]
    fn keys_turn_and_jump() {
        let mut p = fixture();
        laid_out(&mut p);
        ev(
            &mut p,
            &WidgetEvent::KeyPressed {
                key: "End".to_string(),
                repeat: false,
            },
        );
        assert_eq!(p.left_page(), Some(4));
        ev(
            &mut p,
            &WidgetEvent::KeyPressed {
                key: "Home".to_string(),
                repeat: false,
            },
        );
        assert_eq!(p.left_page(), Some(0));
        ev(
            &mut p,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        assert_eq!(p.left_page(), Some(2));
    }

    #[test]
    fn paint_without_painter() {
        let mut p = fixture();
        laid_out(&mut p);
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

    #[test]
    fn body_wraps_at_word_boundaries() {
        // The body used to be laid out as fixed-width char chunks —
        // mid-word splits in the middle of a line. Wrap at word
        // boundaries instead, breaking only a lone oversized word.
        let body = "alpha beta gamma delta epsilon zeta eta theta";
        let mut p = PageFlip::new().page(body).page("x");
        laid_out(&mut p);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        p.paint(&mut PaintContext {
            list: &mut list,
            bounds: p.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        let lines: Vec<String> = list
            .commands
            .iter()
            .filter_map(|c| match c {
                martensite_core::PaintCommand::DrawText(_, t, _, _) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert!(lines.len() > 1, "expected wrapped lines, got {lines:?}");
        // Consume `body` a whole line at a time — every emitted line
        // must continue the source text word-for-word (other pages'
        // text and the widget label are skipped once the body is
        // exhausted).
        let mut consumed = String::new();
        let mut kept = Vec::new();
        for l in &lines {
            let cand = if consumed.is_empty() {
                l.clone()
            } else {
                format!("{consumed} {l}")
            };
            if body.starts_with(&cand) {
                consumed = cand;
                kept.push(l.clone());
            }
        }
        assert_eq!(
            consumed, body,
            "wrapped lines must tile the source: {kept:?}"
        );
        // No line may be wider than the word-wrap would allow: each
        // line is a whole-word run, so it never ends mid-word.
        for (i, l) in kept.iter().enumerate() {
            let last = l.chars().last().unwrap();
            assert!(
                last.is_alphanumeric(),
                "line {i} ends mid-word or mid-space: {l:?}"
            );
        }
    }
}
