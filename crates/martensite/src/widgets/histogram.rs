//! `Histogram` — a binned frequency chart (contiguous
//! equal-width bars over a numeric range).
//!
//! Unlike [`crate::widgets::bar_chart::BarChart`], which draws
//! *categorical* labeled columns, `Histogram` takes raw samples
//! (or pre-binned counts) and renders dense contiguous bars —
//! the distribution / density-chart idiom. Hovering a bin parks
//! its index in [`Histogram::take_hovered`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::histogram::Histogram;
//!
//! let h = Histogram::new().bins(4).samples([0.1, 0.2, 0.9, 1.0, 0.5]);
//! assert_eq!(h.bin_count(), 4);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const WIDTH_PT: f32 = 240.0;
const HEIGHT_PT: f32 = 120.0;
const PAD_PT: f32 = 6.0;

const FACE: [u8; 4] = [46, 46, 52, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const BAR: [u8; 4] = [90, 140, 210, 255];
const HOT: [u8; 4] = [120, 175, 240, 255];

/// A binned frequency chart — see the module docs.
///
/// ```
/// use martensite::widgets::histogram::Histogram;
///
/// assert_eq!(Histogram::new().bin_count(), 0);
/// ```
#[derive(Debug)]
pub struct Histogram {
    /// Accessibility label.
    pub label: String,
    samples: Vec<f32>,
    bins: usize,
    counts: Vec<usize>,
    hovered: Option<usize>,
    pending: Option<usize>,
    bounds: Rect,
    scale: f32,
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}

impl Histogram {
    /// Creates an empty histogram (8 default bins once samples land).
    ///
    /// ```
    /// use martensite::widgets::histogram::Histogram;
    ///
    /// assert_eq!(Histogram::new().bin_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Histogram".to_string(),
            samples: Vec::new(),
            bins: 8,
            counts: Vec::new(),
            hovered: None,
            pending: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Bin count to use when binning samples.
    ///
    /// ```
    /// use martensite::widgets::histogram::Histogram;
    ///
    /// let h = Histogram::new().bins(4).samples([0.0, 0.5, 1.0]);
    /// assert_eq!(h.bin_count(), 4);
    /// ```
    pub fn bins(mut self, bins: usize) -> Self {
        self.bins = bins.max(1);
        self.rebin();
        self
    }

    /// Raw samples — binned over their `min..=max` (0..1 fallback).
    ///
    /// ```
    /// use martensite::widgets::histogram::Histogram;
    ///
    /// let h = Histogram::new().bins(2).samples([0.0, 0.1, 0.9, 1.0]);
    /// assert_eq!(h.count_list(), &[2, 2]);
    /// ```
    pub fn samples(mut self, samples: impl Into<Vec<f32>>) -> Self {
        self.samples = samples.into();
        self.rebin();
        self
    }

    /// Pre-binned counts (skips sample binning).
    ///
    /// ```
    /// use martensite::widgets::histogram::Histogram;
    ///
    /// let h = Histogram::new().counts(vec![3, 7, 2]);
    /// assert_eq!(h.bin_count(), 3);
    /// ```
    pub fn counts(mut self, counts: Vec<usize>) -> Self {
        self.counts = counts;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::histogram::Histogram;
    ///
    /// assert_eq!(Histogram::new().label("Dwell").label, "Dwell");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Number of bins.
    ///
    /// ```
    /// use martensite::widgets::histogram::Histogram;
    ///
    /// assert_eq!(Histogram::new().counts(vec![1, 2]).bin_count(), 2);
    /// ```
    pub fn bin_count(&self) -> usize {
        self.counts.len()
    }

    /// Per-bin counts.
    ///
    /// ```
    /// use martensite::widgets::histogram::Histogram;
    ///
    /// assert_eq!(Histogram::new().counts(vec![4, 6]).count_list(), &[4, 6]);
    /// ```
    pub fn count_list(&self) -> &[usize] {
        &self.counts
    }

    /// Largest bin count.
    ///
    /// ```
    /// use martensite::widgets::histogram::Histogram;
    ///
    /// assert_eq!(Histogram::new().counts(vec![4, 6]).max_count(), 6);
    /// ```
    pub fn max_count(&self) -> usize {
        self.counts.iter().copied().max().unwrap_or(0)
    }

    /// Drains the last hovered bin index.
    ///
    /// ```
    /// use martensite::widgets::histogram::Histogram;
    ///
    /// let mut h = Histogram::new();
    /// assert!(h.take_hovered().is_none());
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// Distribute samples into `self.bins` buckets over their span.
    fn rebin(&mut self) {
        self.counts.clear();
        if self.samples.is_empty() {
            return;
        }
        let lo = self.samples.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = self
            .samples
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        let span = (hi - lo).max(0.0001);
        self.counts = vec![0; self.bins];
        for &s in &self.samples {
            let f = ((s - lo) / span).clamp(0.0, 1.0);
            let i = ((f * self.bins as f32) as usize).min(self.bins - 1);
            self.counts[i] += 1;
        }
    }

    /// Bin index under a point.
    fn bin_at(&self, p: Vec2) -> Option<usize> {
        if self.counts.is_empty() || !self.bounds.contains(p) {
            return None;
        }
        let w = self.bounds.width() / self.counts.len() as f32;
        let i = ((p.x - self.bounds.min_x()) / w.max(1.0)) as usize;
        Some(i.min(self.counts.len() - 1))
    }
}

impl Widget for Histogram {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 30.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!(
            "{} — {} bins, peak {}",
            self.label,
            self.counts.len(),
            self.max_count()
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.bin_at(*position);
                if hit != self.hovered {
                    self.hovered = hit;
                    self.pending = hit;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let max = self.max_count().max(1) as f32;
        let pad = PAD_PT * self.scale;
        let n = self.counts.len();
        if n > 0 {
            let inner_w = self.bounds.width() - 2.0 * pad;
            let inner_h = self.bounds.height() - 2.0 * pad;
            let bw = inner_w / n as f32;
            for (i, &c) in self.counts.iter().enumerate() {
                let bh = (c as f32 / max) * inner_h;
                let r = Rect::new(
                    self.bounds.min_x() + pad + i as f32 * bw,
                    self.bounds.max_y() - pad - bh,
                    bw.max(1.0),
                    bh.max(0.0),
                );
                cx.list.push_fill_rect(
                    f(r),
                    if self.hovered == Some(i) {
                        cx.color(TokenKey::AccentColor, HOT)
                    } else {
                        cx.color(TokenKey::AccentColor, BAR)
                    },
                );
            }
        }
        cx.list.push_stroke_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.pt(0.75),
            cx.color(TokenKey::BorderColor, EDGE),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(h: &mut Histogram, w: f32, ht: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        h.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, ht),
            },
        );
        h.layout(&mut cx, Rect::new(0.0, 0.0, w, ht));
    }

    #[test]
    fn bins_samples() {
        let h = Histogram::new().bins(2).samples([0.0, 0.1, 0.9, 1.0]);
        assert_eq!(h.count_list(), &[2, 2]);
    }

    #[test]
    fn single_sample_piles_up() {
        let h = Histogram::new().bins(4).samples([5.0, 5.0, 5.0]);
        assert_eq!(h.count_list().iter().sum::<usize>(), 3);
        assert_eq!(h.max_count(), 3);
    }

    #[test]
    fn pre_binned() {
        let h = Histogram::new().counts(vec![1, 9, 3]);
        assert_eq!(h.bin_count(), 3);
        assert_eq!(h.max_count(), 9);
    }

    #[test]
    fn hover_parks_index() {
        let mut h = Histogram::new().counts(vec![1, 1, 1, 1]);
        laid_out(&mut h, 240.0, 120.0);
        h.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(200.0, 60.0), // right quarter → bin 3
            },
            bounds: Rect::new(0.0, 0.0, 240.0, 120.0),
            scale: 1.0,
        });
        assert_eq!(h.take_hovered(), Some(3));
    }

    #[test]
    fn leave_clears_hover() {
        let mut h = Histogram::new().counts(vec![1, 1]);
        laid_out(&mut h, 240.0, 120.0);
        h.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(10.0, 60.0),
            },
            bounds: Rect::new(0.0, 0.0, 240.0, 120.0),
            scale: 1.0,
        });
        assert_eq!(h.take_hovered(), Some(0));
        h.event(&mut EventContext {
            event: &WidgetEvent::PointerLeave,
            bounds: Rect::new(0.0, 0.0, 240.0, 120.0),
            scale: 1.0,
        });
        assert_eq!(h.hovered, None);
    }
}
