//! `AspectFrame` — aspect-ratio-locked container.
//!
//! The GTK `AspectFrame` pattern: the child gets the largest rect
//! inside the allocation that preserves [`AspectFrame::ratio`]
//! (width ÷ height), centered on both axes. The classic fit for
//! video surfaces, image previews, and game viewports that must
//! not stretch.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::aspect_frame::AspectFrame;
//! use martensite::widgets::Text;
//! use martensite::core::Widget;
//!
//! let f = AspectFrame::new(16.0 / 9.0).child(Text::new("16:9"));
//! assert_eq!(f.child_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};

/// An aspect-ratio-locked container — see the module docs.
///
/// # Examples
///
/// ```
/// use martensite::widgets::aspect_frame::AspectFrame;
/// use martensite::core::Widget;
///
/// assert_eq!(AspectFrame::new(1.0).child_count(), 0);
/// ```
pub struct AspectFrame {
    child: Option<Box<dyn Widget>>,
    /// Width ÷ height (e.g. `16.0 / 9.0`).
    pub ratio: f32,
    /// Horizontal alignment of the fitted rect: 0 = left, 0.5 =
    /// center (default), 1 = right.
    pub xalign: f32,
    /// Vertical alignment of the fitted rect: 0 = top, 0.5 =
    /// center (default), 1 = bottom.
    pub yalign: f32,
    /// Accessibility label.
    pub label: Option<String>,
    /// Child bounds from the last layout (widget-local).
    child_rect: Option<Rect>,
}

impl AspectFrame {
    /// A frame preserving `ratio` (w/h, clamped >0).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::aspect_frame::AspectFrame;
    ///
    /// assert_eq!(AspectFrame::new(2.0).ratio, 2.0);
    /// ```
    #[must_use]
    pub fn new(ratio: f32) -> Self {
        Self {
            child: None,
            ratio: ratio.max(1e-6),
            xalign: 0.5,
            yalign: 0.5,
            label: None,
            child_rect: None,
        }
    }

    /// Sets the single child.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::aspect_frame::AspectFrame;
    /// use martensite::widgets::Text;
    /// use martensite::core::Widget;
    ///
    /// assert_eq!(AspectFrame::new(1.0).child(Text::new("x")).child_count(), 1);
    /// ```
    #[must_use]
    pub fn child(mut self, w: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(w));
        self
    }

    /// Sets the horizontal fit alignment (0=left, 0.5=center,
    /// 1=right).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::aspect_frame::AspectFrame;
    ///
    /// assert_eq!(AspectFrame::new(1.0).xalign(0.0).xalign, 0.0);
    /// ```
    #[must_use]
    pub fn xalign(mut self, v: f32) -> Self {
        self.xalign = v.clamp(0.0, 1.0);
        self
    }

    /// Sets the vertical fit alignment (0=top, 0.5=center, 1=bottom).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::aspect_frame::AspectFrame;
    ///
    /// assert_eq!(AspectFrame::new(1.0).yalign(1.0).yalign, 1.0);
    /// ```
    #[must_use]
    pub fn yalign(mut self, v: f32) -> Self {
        self.yalign = v.clamp(0.0, 1.0);
        self
    }

    /// Sets the accessibility label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::aspect_frame::AspectFrame;
    ///
    /// let f = AspectFrame::new(1.0).label("Viewport");
    /// ```
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// The fitted rect for an allocation — the largest `ratio`
    /// rectangle that fits, aligned by `xalign`/`yalign`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::aspect_frame::AspectFrame;
    ///
    /// let f = AspectFrame::new(2.0);
    /// let r = f.fit(martensite::prelude::Rect::new(0.0, 0.0, 200.0, 200.0));
    /// assert_eq!(r.width(), 200.0);
    /// assert_eq!(r.height(), 100.0);
    /// ```
    pub fn fit(&self, bounds: Rect) -> Rect {
        let w = bounds.width();
        let h = bounds.height();
        let (fw, fh) = if w / self.ratio <= h {
            (w, w / self.ratio)
        } else {
            (h * self.ratio, h)
        };
        Rect::new(
            bounds.min_x() + (w - fw) * self.xalign,
            bounds.min_y() + (h - fh) * self.yalign,
            fw,
            fh,
        )
    }
}

impl std::fmt::Debug for AspectFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AspectFrame")
            .field("ratio", &self.ratio)
            .field("has_child", &self.child.is_some())
            .finish()
    }
}

impl Widget for AspectFrame {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        self.child
            .as_mut()
            .map(|c| c.measure(cx, constraints))
            .unwrap_or(Vec2::ZERO)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let r = self.fit(bounds);
        self.child_rect = self.child.as_mut().map(|child| {
            cx.layout_child(child.as_mut(), r);
            r
        });
    }

    fn paint(&self, _cx: &mut PaintContext) {}

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
    }

    fn child_count(&self) -> usize {
        self.child.is_some() as usize
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(self.child.as_deref()).flatten()
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(self.child.as_deref_mut()).flatten()
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.child_rect).flatten()
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(16.0, 16.0)).with_policy(UnderflowPolicy::Lint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    struct Cell;

    impl Widget for Cell {
        fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
            Vec2::new(10.0, 10.0)
        }
        fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
    }

    fn lay(f: &mut AspectFrame, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        f.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn wide_allocation_letterboxes() {
        // 400×100 with ratio 2 → fit is 200×100 centered (x=100).
        let mut f = AspectFrame::new(2.0).child(Cell);
        lay(&mut f, 400.0, 100.0);
        let r = f.child_bounds(0).unwrap();
        assert_eq!(r.width(), 200.0);
        assert_eq!(r.height(), 100.0);
        assert_eq!(r.min_x(), 100.0);
    }

    #[test]
    fn tall_allocation_pillarboxes() {
        // 100×400 with ratio 2 → fit is 100×50 centered (y=175).
        let mut f = AspectFrame::new(2.0).child(Cell);
        lay(&mut f, 100.0, 400.0);
        let r = f.child_bounds(0).unwrap();
        assert_eq!(r.width(), 100.0);
        assert_eq!(r.height(), 50.0);
        assert_eq!(r.min_y(), 175.0);
    }

    #[test]
    fn alignment_shifts_the_fit() {
        let mut f = AspectFrame::new(2.0).xalign(0.0).yalign(1.0).child(Cell);
        lay(&mut f, 300.0, 200.0);
        let r = f.child_bounds(0).unwrap();
        // Fit 300×150; xalign 0 → x=0, yalign 1 → y=50.
        assert_eq!(r.min_x(), 0.0);
        assert_eq!(r.min_y(), 50.0);
    }

    #[test]
    fn exact_ratio_fills() {
        let mut f = AspectFrame::new(2.0).child(Cell);
        lay(&mut f, 200.0, 100.0);
        let r = f.child_bounds(0).unwrap();
        assert_eq!(r, Rect::new(0.0, 0.0, 200.0, 100.0));
    }
}
