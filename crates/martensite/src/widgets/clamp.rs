//! `Clamp` — maximum-width centering container.
//!
//! The libadwaita `AdwClamp` pattern: the child gets the full
//! height but its width is capped at [`Clamp::maximum`], centered
//! horizontally. The readability container — keeps long text and
//! forms at a comfortable measure on wide windows without manual
//! padding math.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::clamp::Clamp;
//! use martensite::widgets::Text;
//! use martensite::core::Widget;
//!
//! let c = Clamp::new().maximum(600.0).child(Text::new("content"));
//! assert_eq!(c.child_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};

/// Default width cap (logical points) — a comfortable text measure.
const DEFAULT_MAX_PT: f32 = 600.0;

/// A max-width centering container — see the module docs.
///
/// # Examples
///
/// ```
/// use martensite::widgets::clamp::Clamp;
/// use martensite::widgets::Text;
/// use martensite::core::Widget;
///
/// assert_eq!(Clamp::new().child_count(), 0);
/// ```
pub struct Clamp {
    child: Option<Box<dyn Widget>>,
    /// Width cap in logical points.
    pub maximum: f32,
    /// Whether the child can be narrower than `maximum` — when
    /// `false` the child always gets exactly `maximum` (or the
    /// allocation when tighter). Default `true`.
    pub tighten_threshold: bool,
    /// Accessibility label.
    pub label: Option<String>,
    /// Child bounds from the last layout (widget-local).
    child_rect: Option<Rect>,
}

impl Clamp {
    /// An empty clamp (600pt cap).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::clamp::Clamp;
    ///
    /// assert_eq!(Clamp::new().maximum, 600.0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            child: None,
            maximum: DEFAULT_MAX_PT,
            tighten_threshold: true,
            label: None,
            child_rect: None,
        }
    }

    /// Sets the width cap (clamped ≥0).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::clamp::Clamp;
    ///
    /// assert_eq!(Clamp::new().maximum(400.0).maximum, 400.0);
    /// ```
    #[must_use]
    pub fn maximum(mut self, pts: f32) -> Self {
        self.maximum = pts.max(0.0);
        self
    }

    /// Sets the single child.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::clamp::Clamp;
    /// use martensite::widgets::Text;
    /// use martensite::core::Widget;
    ///
    /// assert_eq!(Clamp::new().child(Text::new("x")).child_count(), 1);
    /// ```
    #[must_use]
    pub fn child(mut self, w: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(w));
        self
    }

    /// Sets the accessibility label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::clamp::Clamp;
    ///
    /// let c = Clamp::new().label("Content");
    /// ```
    #[must_use]
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// The effective clamp width for an allocated width (device px).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::clamp::Clamp;
    ///
    /// let c = Clamp::new().maximum(100.0);
    /// // scale=1: cap is 100 device px.
    /// assert_eq!(c.effective_width(800.0, 1.0), 100.0);
    /// assert_eq!(c.effective_width(60.0, 1.0), 60.0);
    /// ```
    pub fn effective_width(&self, allocated: f32, scale: f32) -> f32 {
        allocated.min(self.maximum * scale)
    }
}

impl Default for Clamp {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Clamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Clamp")
            .field("maximum", &self.maximum)
            .field("has_child", &self.child.is_some())
            .finish()
    }
}

impl Widget for Clamp {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let cap = cx.pt(self.maximum);
        let inner = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(constraints.max_size.x.min(cap), constraints.max_size.y),
        };
        self.child
            .as_mut()
            .map(|c| c.measure(cx, inner))
            .unwrap_or(Vec2::ZERO)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.child_rect = self.child.as_mut().map(|child| {
            let cap = cx.pt(self.maximum);
            let w = bounds.width().min(cap);
            // Natural-width children (buttons, fields) center inside
            // the cap rather than stretching to it.
            let natural = child.measure(
                cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: Vec2::new(w, bounds.height()),
                },
            );
            let cw = if self.tighten_threshold {
                natural.x.min(w)
            } else {
                w
            };
            let r = Rect::new(
                bounds.min_x() + (bounds.width() - cw) / 2.0,
                bounds.min_y(),
                cw,
                bounds.height(),
            );
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
        RenderMinimum::new(Vec2::new(24.0, 16.0)).with_policy(UnderflowPolicy::Lint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    /// Fixed-size stub child.
    struct Cell {
        w: f32,
        h: f32,
    }

    impl Widget for Cell {
        fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
            Vec2::new(self.w, self.h)
        }
        fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
    }

    fn lay(c: &mut Clamp, width: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, width, 300.0));
    }

    #[test]
    fn wide_allocation_centers_child_at_cap() {
        // Child naturally 400 wide; cap 600 → child gets 400 centered.
        let mut c = Clamp::new()
            .maximum(600.0)
            .child(Cell { w: 400.0, h: 20.0 });
        lay(&mut c, 1000.0);
        let r = c.child_bounds(0).unwrap();
        assert_eq!(r.width(), 400.0);
        assert_eq!(r.min_x(), 300.0);
    }

    #[test]
    fn narrow_allocation_shrinks_below_cap() {
        let mut c = Clamp::new()
            .maximum(600.0)
            .child(Cell { w: 800.0, h: 20.0 });
        lay(&mut c, 300.0);
        let r = c.child_bounds(0).unwrap();
        assert!(r.width() <= 300.0);
    }

    #[test]
    fn child_wider_than_cap_clamps_to_cap() {
        let mut c = Clamp::new()
            .maximum(200.0)
            .child(Cell { w: 500.0, h: 20.0 });
        lay(&mut c, 1000.0);
        let r = c.child_bounds(0).unwrap();
        assert_eq!(r.width(), 200.0);
        assert_eq!(r.min_x(), 400.0);
    }

    #[test]
    fn empty_clamp_reports_no_child() {
        let mut c = Clamp::new();
        lay(&mut c, 400.0);
        assert_eq!(c.child_count(), 0);
        assert!(c.child_bounds(0).is_none());
    }
}
