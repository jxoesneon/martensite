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
    /// Scope name for the lint/a11y tree (`"Name@marker"` allowed).
    name: &'static str,
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
            name: "DynamicColumn",
            row_height: None,
        }
    }

    /// Builder: scope name for the lint/a11y tree — may carry
    /// `@lint:`/`@prose`-style markers.
    pub fn named(mut self, name: &'static str) -> Self {
        self.name = name;
        self
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
                Vec2::new(constraints.max_size.x, cx.pt(rh))
            } else {
                child.measure(cx, constraints)
            };
            h += s.y + f32::from(i > 0) * cx.pt(self.gap);
            w = w.max(s.x);
        }
        Vec2::new(w.min(constraints.max_size.x), h.min(constraints.max_size.y))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.rects.clear();
        let mut y = bounds.min_y();
        let gap = cx.pt(self.gap);
        for child in self.children.iter_mut() {
            let h = self.row_height.map(|rh| cx.pt(rh)).unwrap_or_else(|| {
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
            y += h + gap;
        }
    }

    fn paint(&self, _cx: &mut PaintContext) {}

    fn debug_name(&self) -> &'static str {
        self.name
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use martensite::core::HotNode;
    use martensite::widgets::Text;

    #[test]
    fn stacks_children_at_measured_heights() {
        let mut col = DynamicColumn::new().gap(3.0);
        col.set_children(vec![
            Box::new(Text::new("Button".to_string()).font_size(18.0)),
            Box::new(Text::new("Role: button".to_string())),
            Box::new(Text::new("Snippet:".to_string()).font_size(13.0)),
        ]);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let c = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(300.0, 200.0),
        };
        let m = col.measure(&mut cx, c);
        assert!(m.x > 0.0 && m.y > 0.0);
        let mut hot2 = HotNode::default();
        let mut cx2 = LayoutContext {
            hot: &mut hot2,
            scale: 1.0,
        };
        col.layout(&mut cx2, Rect::new(0.0, 0.0, 300.0, 200.0));
        let mut prev_bottom = 0.0;
        for i in 0..col.len() {
            let r = col.child_bounds(i).expect("row rect");
            assert!(r.min_y() >= prev_bottom);
            assert_eq!(r.width(), 300.0);
            prev_bottom = r.max_y();
        }
    }

    #[test]
    fn row_height_override_stacks_uniformly() {
        let mut col = DynamicColumn::new().gap(2.0).row_height(10.0);
        col.set_children(vec![
            Box::new(Text::new("a".to_string())),
            Box::new(Text::new("b".to_string())),
        ]);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        col.layout(&mut cx, Rect::new(0.0, 0.0, 100.0, 100.0));
        let r0 = col.child_bounds(0).unwrap();
        let r1 = col.child_bounds(1).unwrap();
        assert_eq!(r0.height(), 10.0);
        assert_eq!(r1.min_y(), 12.0);
    }
}
