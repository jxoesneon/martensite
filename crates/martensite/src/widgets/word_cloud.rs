//! `WordCloud` — weight-scaled packed word display (Ant `WordCloud`
//! / tag-cloud idiom).
//!
//! Entries are `(text, weight)`; the heaviest words render largest
//! and are packed row-wise across the surface in a categorical
//! palette. Clicking a word parks its original index in
//! [`WordCloud::take_clicked`]; hovering parks
//! [`WordCloud::take_hovered`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::word_cloud::WordCloud;
//!
//! let w = WordCloud::new().word("rust", 10.0).word("gui", 6.0);
//! assert_eq!(w.word_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::{paint_label_clipped, SharedTextPainter};
use martensite_core::paint::TextShaper;

const WIDTH_PT: f32 = 300.0;
const HEIGHT_PT: f32 = 160.0;
const MIN_PT: f32 = 10.0;
const MAX_PT: f32 = 34.0;
const GAP_PT: f32 = 6.0;

const PALETTE: [[u8; 4]; 6] = [
    [110, 170, 230, 255],
    [230, 150, 90, 255],
    [120, 200, 140, 255],
    [220, 110, 110, 255],
    [190, 140, 230, 255],
    [230, 210, 120, 255],
];

/// One placed word's geometry (index into `words`, rect, font size).
struct Placed {
    idx: usize,
    rect: Rect,
    size: f32,
}

/// A weight-scaled word display — see the module docs.
///
/// ```
/// use martensite::widgets::word_cloud::WordCloud;
///
/// assert_eq!(WordCloud::new().word_count(), 0);
/// ```
pub struct WordCloud {
    /// Accessibility label.
    pub label: String,
    words: Vec<(String, f32)>,
    pending_click: Option<usize>,
    pending_hover: Option<usize>,
    painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl Default for WordCloud {
    fn default() -> Self {
        Self::new()
    }
}

impl WordCloud {
    /// Creates an empty cloud.
    ///
    /// ```
    /// use martensite::widgets::word_cloud::WordCloud;
    ///
    /// assert_eq!(WordCloud::new().word_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Word cloud".to_string(),
            words: Vec::new(),
            pending_click: None,
            pending_hover: None,
            painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Adds a word with a weight (≥0).
    ///
    /// ```
    /// use martensite::widgets::word_cloud::WordCloud;
    ///
    /// let w = WordCloud::new().word("a", 5.0).word("b", 3.0);
    /// assert_eq!(w.word_count(), 2);
    /// ```
    pub fn word(mut self, text: impl Into<String>, weight: f32) -> Self {
        self.words.push((text.into(), weight.max(0.0)));
        self
    }

    /// Replaces the word list.
    ///
    /// ```
    /// use martensite::widgets::word_cloud::WordCloud;
    ///
    /// let w = WordCloud::new().words(vec![("x".to_string(), 1.0)]);
    /// assert_eq!(w.word_count(), 1);
    /// ```
    pub fn words(mut self, words: Vec<(String, f32)>) -> Self {
        self.words = words;
        self
    }

    /// Shared text painter for glyph-accurate layout.
    ///
    /// ```
    /// use martensite::widgets::word_cloud::WordCloud;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let w = WordCloud::new().with_text_painter(shared_painter());
    /// assert_eq!(w.word_count(), 0);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.painter = Some(painter);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::word_cloud::WordCloud;
    ///
    /// assert_eq!(WordCloud::new().label("Topics").label, "Topics");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Entry count.
    ///
    /// ```
    /// use martensite::widgets::word_cloud::WordCloud;
    ///
    /// assert_eq!(WordCloud::new().word("x", 1.0).word_count(), 1);
    /// ```
    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    /// The heaviest weight.
    ///
    /// ```
    /// use martensite::widgets::word_cloud::WordCloud;
    ///
    /// assert_eq!(WordCloud::new().word("x", 7.5).max_weight(), 7.5);
    /// ```
    pub fn max_weight(&self) -> f32 {
        self.words.iter().map(|(_, w)| *w).fold(0.0_f32, f32::max)
    }

    /// Drains the last clicked original index.
    ///
    /// ```
    /// use martensite::widgets::word_cloud::WordCloud;
    ///
    /// let mut w = WordCloud::new();
    /// assert!(w.take_clicked().is_none());
    /// ```
    pub fn take_clicked(&mut self) -> Option<usize> {
        self.pending_click.take()
    }

    /// Drains the last hovered original index.
    ///
    /// ```
    /// use martensite::widgets::word_cloud::WordCloud;
    ///
    /// let mut w = WordCloud::new();
    /// assert!(w.take_hovered().is_none());
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.pending_hover.take()
    }

    /// Word size for a weight.
    fn size_of(&self, weight: f32) -> f32 {
        let max = self.max_weight().max(0.0001);
        let t = (weight / max).sqrt(); // sqrt dampens extreme skew
        MIN_PT + t * (MAX_PT - MIN_PT)
    }

    /// Estimated word width without a painter (≈0.55em per char).
    fn est_width(&self, text: &str, size_pt: f32) -> f32 {
        text.chars().count() as f32 * size_pt * 0.55 * self.scale
    }

    /// Measured word width via the painter when available.
    fn word_width(&self, text: &str, size_pt: f32) -> f32 {
        if let Some(p) = &self.painter {
            if let Some(w) = p.measure_text(text, size_pt * self.scale) {
                return w;
            }
        }
        self.est_width(text, size_pt)
    }

    /// Row-pack the words (heaviest first) into `bounds`.
    fn place(&self) -> Vec<Placed> {
        let mut order: Vec<usize> = (0..self.words.len()).collect();
        order.sort_by(|&a, &b| self.words[b].1.total_cmp(&self.words[a].1));
        let mut placed = Vec::new();
        let gap = GAP_PT * self.scale;
        let (mut x, mut y, mut row_h) = (self.bounds.min_x(), self.bounds.min_y(), 0.0_f32);
        for &i in &order {
            let (text, weight) = &self.words[i];
            let size = self.size_of(*weight);
            let w = self.word_width(text, size);
            let h = size * 1.2 * self.scale;
            if x + w > self.bounds.max_x() && x > self.bounds.min_x() {
                x = self.bounds.min_x();
                y += row_h + gap;
                row_h = 0.0;
            }
            if y + h > self.bounds.max_y() {
                break; // out of vertical room
            }
            placed.push(Placed {
                idx: i,
                rect: Rect::new(x, y, w, h),
                size,
            });
            x += w + gap;
            row_h = row_h.max(h);
        }
        placed
    }

    /// Placed word under a point.
    fn word_at(&self, p: Vec2) -> Option<usize> {
        self.place()
            .iter()
            .find(|pl| pl.rect.contains(p))
            .map(|pl| pl.idx)
    }
}

impl Widget for WordCloud {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!("{} — {} words", self.label, self.words.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self.word_at(*position) {
                    self.pending_click = Some(i);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                let hit = self.word_at(*position);
                if hit != self.pending_hover.or(hit) && hit.is_some() {
                    self.pending_hover = hit;
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.painter, cx.text_painter);
        for pl in self.place() {
            let (text, _) = &self.words[pl.idx];
            let color = cx.color(TokenKey::TextColor, PALETTE[pl.idx % PALETTE.len()]);
            paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(pl.rect.min_x()),
                    f64::from(pl.rect.min_y()),
                    f64::from(pl.rect.max_x()),
                    f64::from(pl.rect.max_y()),
                ),
                kurbo::Point::new(f64::from(pl.rect.min_x()), f64::from(pl.rect.min_y())),
                text,
                pl.size * self.scale,
                color,
            );
        }
    }
}

impl std::fmt::Debug for WordCloud {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WordCloud")
            .field("words", &self.words.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(w: &mut WordCloud, bw: f32, bh: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(bw, bh),
            },
        );
        w.layout(&mut cx, Rect::new(0.0, 0.0, bw, bh));
    }

    #[test]
    fn sizes_scale_with_weight() {
        let w = WordCloud::new().word("big", 10.0).word("small", 1.0);
        assert!(w.size_of(10.0) > w.size_of(1.0));
        assert_eq!(w.size_of(10.0), MAX_PT);
    }

    #[test]
    fn heaviest_first() {
        let mut w = WordCloud::new().word("tiny", 1.0).word("huge", 10.0);
        laid_out(&mut w, 300.0, 160.0);
        let placed = w.place();
        assert_eq!(placed[0].idx, 1); // "huge" placed first
    }

    #[test]
    fn click_parks_index() {
        let mut w = WordCloud::new().word("only", 5.0);
        laid_out(&mut w, 300.0, 160.0);
        let pl = &w.place()[0];
        let mid = Vec2::new(
            (pl.rect.min_x() + pl.rect.max_x()) / 2.0,
            (pl.rect.min_y() + pl.rect.max_y()) / 2.0,
        );
        w.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: mid,
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 300.0, 160.0),
            scale: 1.0,
        });
        assert_eq!(w.take_clicked(), Some(0));
    }

    #[test]
    fn empty_cloud_no_panic() {
        let mut w = WordCloud::new();
        laid_out(&mut w, 300.0, 160.0);
        assert!(w.place().is_empty());
    }

    #[test]
    fn overflow_wraps() {
        let mut w = WordCloud::new();
        for i in 0..40 {
            w.words.push((format!("word{i}"), 5.0));
        }
        laid_out(&mut w, 300.0, 160.0);
        let placed = w.place();
        assert!(!placed.is_empty());
        // Everything placed stays inside bounds.
        for p in &placed {
            assert!(p.rect.max_y() <= 160.0 + 0.01);
        }
    }
}
