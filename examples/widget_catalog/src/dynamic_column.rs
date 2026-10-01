//! A column container whose children can be wholesale replaced —
//! `Flex` is builder-only, so the props panel, reference pane, and
//! snippet block use this to swap content when the selected page or
//! its props change.

use glam::Vec2;
use martensite::core::{LayoutConstraints, LayoutContext, PaintContext, Rect, Widget};

/// Vertical list of internal children laid out top-to-bottom at each
/// child's measured height, full width, separated by `gap`.
pub struct DynamicColumn {
    children: Vec<Box<dyn Widget>>,
    rects: Vec<Rect>,
    gap: f32,
    /// Uniform row height override — `Some(h)` ignores measured heights
    /// (log lines, rail items).
    row_height: Option<f32>,
}

impl Default for DynamicColumn {
    fn default() -> Self {
        Self::new()
    }
}

impl DynamicColumn {
    /// Empty column, 4pt gaps.
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            rects: Vec::new(),
            gap: 4.0,
            row_height: None,
        }
    }

    /// Builder: gap between rows.
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// Builder: uniform row height (ignores child measures).
    pub fn row_height(mut self, h: f32) -> Self {
        self.row_height = Some(h);
        self
    }

    /// Replaces all children. Layout runs on the next pass.
    pub fn set_children(&mut self, children: Vec<Box<dyn Widget>>) {
        self.children = children;
        self.rects.clear();
    }

    /// Appends a child.
    pub fn push(&mut self, child: Box<dyn Widget>) {
        self.children.push(child);
    }

    /// Number of children.
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}

impl Widget for DynamicColumn {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut h = 0.0f32;
        let mut w = 0.0f32;
        for (i, child) in self.children.iter_mut().enumerate() {
            let s = if let Some(rh) = self.row_height {
                Vec2::new(constraints.max_size.x, rh)
            } else {
                child.measure(cx, constraints)
            };
            h += s.y + f32::from(i > 0) * self.gap;
            w = w.max(s.x);
        }
        Vec2::new(w.min(constraints.max_size.x), h.min(constraints.max_size.y))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.rects.clear();
        let mut y = bounds.min_y();
        for child in self.children.iter_mut() {
            let h = self.row_height.unwrap_or_else(|| {
                // Children already measured under the column's width
                // constraint keep that height; unmeasured children
                // (freshly pushed) measure against the real width now.
                let mut hot_probe = martensite::core::HotNode::default();
                let mut pcx = LayoutContext {
                    hot: &mut hot_probe,
                    scale: cx.scale,
                };
                child
                    .measure(
                        &mut pcx,
                        LayoutConstraints {
                            min_size: Vec2::ZERO,
                            max_size: Vec2::new(bounds.width(), f32::MAX),
                        },
                    )
                    .y
            });
            let rect = Rect::new(bounds.min_x(), y, bounds.width(), h);
            self.rects.push(rect);
            cx.layout_child(child.as_mut(), rect);
            y += h + self.gap;
        }
    }

    fn paint(&self, _cx: &mut PaintContext) {}

    fn child_count(&self) -> usize {
        self.children.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.children.get(index).map(|c| &**c)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.children.get_mut(index).map(|c| &mut **c)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.rects.get(index).copied()
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
