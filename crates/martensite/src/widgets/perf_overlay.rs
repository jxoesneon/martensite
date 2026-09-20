//! `PerfOverlay` — a compact developer HUD: current FPS, frame-time
//! statistics, and a rolling frame-time graph strip. Chrome DevTools /
//! Unity stats-overlay idiom.
//!
//! Pure display: the host pushes frame samples via
//! [`PerfOverlay::push_frame`] each `tick`; the widget keeps a ring
//! buffer and renders a spark graph plus a text readout.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::perf_overlay::PerfOverlay;
//!
//! let mut p = PerfOverlay::new();
//! p.push_frame(16.7);
//! assert_eq!(p.frame_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};

use crate::text_paint::SharedTextPainter;

const CAP: usize = 120;
const PAD_PT: f32 = 8.0;
const TEXT_PT: f32 = 10.0;
const GRAPH_PT: f32 = 28.0;
const WIDTH_PT: f32 = 140.0;
/// Frame budget line — 16.7 ms ≈ 60 fps.
const BUDGET_MS: f32 = 16.7;

const FACE: [u8; 4] = [20, 22, 28, 220];
const OK: [u8; 4] = [90, 200, 120, 255];
const WARN: [u8; 4] = [235, 180, 70, 255];
const BAD: [u8; 4] = [230, 90, 80, 255];
const BUDGET_LINE: [u8; 4] = [120, 126, 140, 160];

/// The stats overlay — see the module docs.
///
/// ```
/// use martensite::widgets::perf_overlay::PerfOverlay;
///
/// assert_eq!(PerfOverlay::new().label, "Performance");
/// ```
pub struct PerfOverlay {
    /// Accessibility label.
    pub label: String,
    /// Whether the widget requests repaints while samples flow.
    pub live: bool,
    frames: Vec<f32>,
    max_ms: f32,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for PerfOverlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PerfOverlay")
            .field("frames", &self.frames.len())
            .finish()
    }
}

impl PerfOverlay {
    /// An empty overlay.
    ///
    /// ```
    /// use martensite::widgets::perf_overlay::PerfOverlay;
    ///
    /// assert_eq!(PerfOverlay::new().frame_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Performance".to_string(),
            live: true,
            frames: Vec::with_capacity(CAP),
            max_ms: BUDGET_MS * 2.0,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::perf_overlay::PerfOverlay;
    ///
    /// assert_eq!(PerfOverlay::new().label("HUD").label, "HUD");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::perf_overlay::PerfOverlay;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _p = PerfOverlay::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Push one frame-time sample in milliseconds.
    ///
    /// ```
    /// use martensite::widgets::perf_overlay::PerfOverlay;
    ///
    /// let mut p = PerfOverlay::new();
    /// p.push_frame(16.0);
    /// p.push_frame(33.0);
    /// assert_eq!(p.frame_count(), 2);
    /// ```
    pub fn push_frame(&mut self, ms: f32) {
        if self.frames.len() >= CAP {
            self.frames.remove(0);
        }
        let ms = ms.max(0.0);
        if ms > self.max_ms {
            self.max_ms = ms;
        }
        self.frames.push(ms);
    }

    /// Sample count in the ring.
    ///
    /// ```
    /// use martensite::widgets::perf_overlay::PerfOverlay;
    ///
    /// assert_eq!(PerfOverlay::new().frame_count(), 0);
    /// ```
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Most recent frame time in ms (0 when empty).
    ///
    /// ```
    /// use martensite::widgets::perf_overlay::PerfOverlay;
    ///
    /// let mut p = PerfOverlay::new();
    /// p.push_frame(20.0);
    /// assert_eq!(p.last_ms(), 20.0);
    /// ```
    pub fn last_ms(&self) -> f32 {
        self.frames.last().copied().unwrap_or(0.0)
    }

    /// Frames-per-second derived from the last sample.
    ///
    /// ```
    /// use martensite::widgets::perf_overlay::PerfOverlay;
    ///
    /// let mut p = PerfOverlay::new();
    /// p.push_frame(25.0);
    /// assert_eq!(p.fps().round() as u32, 40);
    /// ```
    pub fn fps(&self) -> f32 {
        let ms = self.last_ms();
        if ms > 0.0 {
            1000.0 / ms
        } else {
            0.0
        }
    }

    /// Average frame time over the ring in ms.
    ///
    /// ```
    /// use martensite::widgets::perf_overlay::PerfOverlay;
    ///
    /// let mut p = PerfOverlay::new();
    /// p.push_frame(10.0);
    /// p.push_frame(20.0);
    /// assert_eq!(p.avg_ms(), 15.0);
    /// ```
    pub fn avg_ms(&self) -> f32 {
        if self.frames.is_empty() {
            0.0
        } else {
            self.frames.iter().sum::<f32>() / self.frames.len() as f32
        }
    }

    /// Worst frame time in the ring in ms.
    ///
    /// ```
    /// use martensite::widgets::perf_overlay::PerfOverlay;
    ///
    /// let mut p = PerfOverlay::new();
    /// p.push_frame(10.0);
    /// p.push_frame(50.0);
    /// assert_eq!(p.worst_ms(), 50.0);
    /// ```
    pub fn worst_ms(&self) -> f32 {
        self.frames.iter().copied().fold(0.0, f32::max)
    }

    /// Clear the ring.
    ///
    /// ```
    /// use martensite::widgets::perf_overlay::PerfOverlay;
    ///
    /// let mut p = PerfOverlay::new();
    /// p.push_frame(16.0);
    /// p.clear();
    /// assert_eq!(p.frame_count(), 0);
    /// ```
    pub fn clear(&mut self) {
        self.frames.clear();
    }
}

impl Default for PerfOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for PerfOverlay {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let h = PAD_PT * 2.0 + TEXT_PT + 4.0 + GRAPH_PT;
        Vec2::new(
            (WIDTH_PT * s).min(constraints.max_size.x.max(0.0)),
            (h * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(100.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Status);
        node.set_label(self.label.clone());
        node.set_value(format!("{:.0} fps", self.fps()));
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn tick(&mut self, _dt: std::time::Duration) -> bool {
        self.live
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
        let pad = PAD_PT * s;
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            &martensite_core::shape::Shape::rounded(6.0 * s),
            FACE,
        );
        // Readout: fps + last ms.
        let fps = self.fps();
        let color = if self.last_ms() <= BUDGET_MS {
            OK
        } else if self.last_ms() <= BUDGET_MS * 2.0 {
            WARN
        } else {
            BAD
        };
        let fs = TEXT_PT * s;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(f64::from(b.min_x() + pad), f64::from(b.min_y() + pad + fs)),
            &format!("{fps:.0} fps · {:.1} ms", self.last_ms()),
            fs,
            color,
        );
        // Graph strip.
        let gy = b.min_y() + pad + fs + 4.0 * s;
        let gh = b.max_y() - pad - gy;
        if gh <= 0.0 {
            return;
        }
        let gw = b.width() - pad * 2.0;
        // Budget line at 16.7 ms.
        let scale_max = self.max_ms.max(BUDGET_MS * 1.5);
        let budget_y = gy + gh - (BUDGET_MS / scale_max).min(1.0) * gh;
        cx.list.push_stroke_path(
            line_path(
                Vec2::new(b.min_x() + pad, budget_y),
                Vec2::new(b.max_x() - pad, budget_y),
            ),
            1.0,
            BUDGET_LINE,
        );
        // Bars, oldest → newest left to right.
        if !self.frames.is_empty() {
            let bw = (gw / CAP as f32).max(1.0);
            for (i, &ms) in self.frames.iter().enumerate() {
                let frac = (ms / scale_max).min(1.0);
                let bh = frac * gh;
                let x = b.max_x() - pad - (self.frames.len() - i) as f32 * bw;
                let c = if ms <= BUDGET_MS {
                    OK
                } else if ms <= BUDGET_MS * 2.0 {
                    WARN
                } else {
                    BAD
                };
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(x),
                        f64::from(gy + gh - bh),
                        f64::from(x + bw - 1.0),
                        f64::from(gy + gh),
                    ),
                    c,
                );
            }
        }
    }
}

fn line_path(a: Vec2, b: Vec2) -> kurbo::BezPath {
    let mut p = kurbo::BezPath::new();
    p.move_to((f64::from(a.x), f64::from(a.y)));
    p.line_to((f64::from(b.x), f64::from(b.y)));
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn ring_evicts_oldest() {
        let mut p = PerfOverlay::new();
        for i in 0..130 {
            p.push_frame(i as f32);
        }
        assert_eq!(p.frame_count(), 120);
        assert_eq!(p.last_ms(), 129.0);
    }

    #[test]
    fn stats() {
        let mut p = PerfOverlay::new();
        p.push_frame(10.0);
        p.push_frame(20.0);
        assert_eq!(p.avg_ms(), 15.0);
        assert_eq!(p.worst_ms(), 20.0);
        assert_eq!(p.fps().round() as u32, 50);
    }

    #[test]
    fn paint_without_painter() {
        let mut p = PerfOverlay::new();
        for ms in [10.0, 16.0, 40.0, 12.0] {
            p.push_frame(ms);
        }
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        p.layout(&mut cx, Rect::new(0.0, 0.0, 140.0, 56.0));
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
