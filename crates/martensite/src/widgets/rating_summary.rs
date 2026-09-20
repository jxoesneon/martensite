//! `RatingSummary` — an aggregate review block (App Store /
//! Play Store idiom): a large average score, a total-review
//! caption, and a five-row distribution of filled bars from
//! 5★ down to 1★.
//!
//! The host feeds counts via [`RatingSummary::counts`]; the
//! widget derives the average and per-star fractions. Display-only.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::rating_summary::RatingSummary;
//!
//! let r = RatingSummary::new().counts([10, 4, 2, 1, 0]);
//! assert_eq!(r.total(), 17);
//! assert!((r.average() - 4.35).abs() < 0.01);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 12.0;
const SCORE_PT: f32 = 34.0;
const FONT_PT: f32 = 11.0;
const BAR_H_PT: f32 = 8.0;
const BAR_GAP_PT: f32 = 7.0;
const ROW_LABEL_PT: f32 = 18.0;

const FACE: [u8; 4] = [30, 32, 40, 255];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];
const STAR: [u8; 4] = [240, 190, 80, 255];
const TRACK: [u8; 4] = [255, 255, 255, 24];

/// The summary block — see the module docs.
///
/// ```
/// use martensite::widgets::rating_summary::RatingSummary;
///
/// assert_eq!(RatingSummary::new().total(), 0);
/// ```
pub struct RatingSummary {
    /// Accessibility label.
    pub label: String,
    /// Star color.
    pub star_color: [u8; 4],
    /// Counts per star, index 0 = 5★ … index 4 = 1★.
    pub counts: [u32; 5],
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for RatingSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RatingSummary")
            .field("total", &self.total())
            .field("average", &self.average())
            .finish()
    }
}

impl Default for RatingSummary {
    fn default() -> Self {
        Self::new()
    }
}

impl RatingSummary {
    /// An empty summary.
    ///
    /// ```
    /// use martensite::widgets::rating_summary::RatingSummary;
    ///
    /// assert_eq!(RatingSummary::new().total(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Ratings".to_string(),
            star_color: STAR,
            counts: [0; 5],
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Per-star counts, index 0 = 5★ down to index 4 = 1★.
    ///
    /// ```
    /// use martensite::widgets::rating_summary::RatingSummary;
    ///
    /// assert_eq!(RatingSummary::new().counts([1, 0, 0, 0, 2]).total(), 3);
    /// ```
    pub fn counts(mut self, counts: [u32; 5]) -> Self {
        self.counts = counts;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::rating_summary::RatingSummary;
    ///
    /// assert_eq!(RatingSummary::new().label("Reviews").label, "Reviews");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::rating_summary::RatingSummary;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _r = RatingSummary::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Total review count.
    ///
    /// ```
    /// use martensite::widgets::rating_summary::RatingSummary;
    ///
    /// assert_eq!(RatingSummary::new().counts([1, 2, 3, 4, 5]).total(), 15);
    /// ```
    pub fn total(&self) -> u32 {
        self.counts.iter().sum()
    }

    /// Weighted average (0 when empty).
    ///
    /// ```
    /// use martensite::widgets::rating_summary::RatingSummary;
    ///
    /// assert_eq!(RatingSummary::new().average(), 0.0);
    /// assert_eq!(RatingSummary::new().counts([0, 0, 0, 0, 3]).average(), 1.0);
    /// ```
    pub fn average(&self) -> f32 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        let sum: u32 = self
            .counts
            .iter()
            .enumerate()
            .map(|(i, c)| (5 - i) as u32 * c)
            .sum();
        sum as f32 / total as f32
    }

    /// Fraction of reviews at each star (index 0 = 5★).
    ///
    /// ```
    /// use martensite::widgets::rating_summary::RatingSummary;
    ///
    /// let r = RatingSummary::new().counts([3, 1, 0, 0, 0]);
    /// assert_eq!(r.fraction(0), 0.75);
    /// ```
    pub fn fraction(&self, index: usize) -> f32 {
        let total = self.total();
        if total == 0 || index >= 5 {
            return 0.0;
        }
        self.counts[index] as f32 / total as f32
    }
}

impl Widget for RatingSummary {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        Vec2::new(
            (280.0 * s).min(constraints.max_size.x.max(0.0)),
            ((SCORE_PT + 5.0 * (BAR_H_PT + BAR_GAP_PT) + PAD_PT * 3.0) * s)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(200.0, 110.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.clone());
        node.set_value(format!(
            "{:.1} out of 5, {} reviews",
            self.average(),
            self.total()
        ));
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            cx.color(TokenKey::BackgroundColor, FACE),
        );
        // Big score.
        let fs = SCORE_PT * s;
        let score = format!("{:.1}", self.average());
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(b.min_x() + PAD_PT * s),
                f64::from(b.min_y() + PAD_PT * s + fs * 0.8),
            ),
            &score,
            fs,
            cx.color(TokenKey::TextColor, TEXT),
        );
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(b.min_x() + PAD_PT * s),
                f64::from(b.min_y() + PAD_PT * s + fs * 0.8 + 16.0 * s),
            ),
            &format!("{} reviews", self.total()),
            FONT_PT * s,
            MUTED_FG,
        );
        // Distribution bars, 5★ at top.
        let shape = martensite_core::shape::Shape::rounded(BAR_H_PT * s / 2.0);
        let bars_x = b.min_x() + (PAD_PT + 60.0) * s;
        let bar_w = (b.max_x() - PAD_PT * s - bars_x - ROW_LABEL_PT * s).max(0.0);
        let mut y = b.min_y() + PAD_PT * s + 6.0 * s;
        for i in 0..5 {
            // Star label.
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(b.min_x() + b.width() * 0.32),
                    f64::from(y + BAR_H_PT * s * 0.95),
                ),
                &format!("{}★", 5 - i),
                FONT_PT * s,
                MUTED_FG,
            );
            // Track + fill.
            let track = kurbo::Rect::new(
                f64::from(bars_x + ROW_LABEL_PT * s),
                f64::from(y),
                f64::from(bars_x + ROW_LABEL_PT * s + bar_w),
                f64::from(y + BAR_H_PT * s),
            );
            cx.list.push_fill_shape(track, &shape, TRACK);
            let frac = self.fraction(i);
            if frac > 0.0 {
                let fill = kurbo::Rect::new(
                    track.x0,
                    track.y0,
                    track.x0 + f64::from(bar_w) * f64::from(frac),
                    track.y1,
                );
                cx.list.push_fill_shape(
                    fill,
                    &shape,
                    cx.color(TokenKey::WarningColor, self.star_color),
                );
            }
            y += (BAR_H_PT + BAR_GAP_PT) * s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn average_weights_stars() {
        let r = RatingSummary::new().counts([10, 0, 0, 0, 10]);
        assert!((r.average() - 3.0).abs() < 1e-6);
    }

    #[test]
    fn fractions_sum() {
        let r = RatingSummary::new().counts([5, 3, 1, 1, 0]);
        let sum: f32 = (0..5).map(|i| r.fraction(i)).sum();
        assert!((sum - 1.0).abs() < 1e-6);
    }

    #[test]
    fn empty_is_zero() {
        let r = RatingSummary::new();
        assert_eq!(r.average(), 0.0);
        assert_eq!(r.fraction(2), 0.0);
    }

    #[test]
    fn paint_without_painter() {
        let mut r = RatingSummary::new().counts([8, 2, 0, 0, 1]);
        let mut hot = HotNode::default();
        let mut lcx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        r.layout(&mut lcx, Rect::new(0.0, 0.0, 280.0, 140.0));
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        r.paint(&mut PaintContext {
            list: &mut list,
            bounds: r.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
