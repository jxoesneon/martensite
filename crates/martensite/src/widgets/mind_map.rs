//! `MindMap` — a balanced mind-map diagram (a center root with
//! subtrees fanned alternately to the left and right — the
//! FreeMind / XMind idiom).
//!
//! Declare the tree with [`MindMap::root`] and named-parent
//! [`MindMap::child`] calls. Layout splits the root's children
//! between the two sides by leaf-count balance, stacks each
//! subtree in a leaf-proportioned vertical slot, and draws
//! elbow links from parent to child. Hovering a node parks its
//! index in [`MindMap::take_hovered`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::mind_map::MindMap;
//!
//! let m = MindMap::new()
//!     .root("Plan")
//!     .child("Plan", "Research")
//!     .child("Plan", "Build");
//! assert_eq!(m.node_count(), 3);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const WIDTH_PT: f32 = 300.0;
const HEIGHT_PT: f32 = 200.0;
const NODE_W_PT: f32 = 72.0;
const NODE_H_PT: f32 = 22.0;
const COL_GAP_PT: f32 = 40.0;
const V_GAP_PT: f32 = 10.0;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const NODE: [u8; 4] = [52, 52, 60, 255];
const ROOT: [u8; 4] = [80, 130, 200, 255];
const HOVER: [u8; 4] = [70, 90, 140, 255];
const LINK: [u8; 4] = [110, 110, 120, 255];
const GLYPH: [u8; 4] = [220, 220, 228, 255];

#[derive(Debug)]
struct Node {
    label: String,
    parent: Option<usize>,
    children: Vec<usize>,
    /// -1 left, +1 right, 0 root.
    side: i32,
    depth: usize,
    /// Leaf slots this subtree occupies.
    leaves: usize,
    /// Center position after layout.
    pos: Vec2,
}

/// A balanced mind-map diagram — see the module docs.
///
/// ```
/// use martensite::widgets::mind_map::MindMap;
///
/// assert_eq!(MindMap::new().node_count(), 0);
/// ```
#[derive(Debug)]
pub struct MindMap {
    /// Accessibility label.
    pub label: String,
    nodes: Vec<Node>,
    hovered: Option<usize>,
    pending: Option<usize>,
    bounds: Rect,
    scale: f32,
}

impl Default for MindMap {
    fn default() -> Self {
        Self::new()
    }
}

impl MindMap {
    /// Creates an empty map.
    ///
    /// ```
    /// use martensite::widgets::mind_map::MindMap;
    ///
    /// assert_eq!(MindMap::new().node_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Mind map".to_string(),
            nodes: Vec::new(),
            hovered: None,
            pending: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets the root label (first call wins; replaces index 0).
    ///
    /// ```
    /// use martensite::widgets::mind_map::MindMap;
    ///
    /// assert_eq!(MindMap::new().root("R").node_name(0), "R");
    /// ```
    pub fn root(mut self, label: impl Into<String>) -> Self {
        let n = Node {
            label: label.into(),
            parent: None,
            children: Vec::new(),
            side: 0,
            depth: 0,
            leaves: 1,
            pos: Vec2::ZERO,
        };
        if self.nodes.is_empty() {
            self.nodes.push(n);
        } else {
            self.nodes[0] = n;
        }
        self
    }

    /// Adds a child under a named parent (unknown parents ignored).
    ///
    /// ```
    /// use martensite::widgets::mind_map::MindMap;
    ///
    /// let m = MindMap::new().root("r").child("r", "c");
    /// assert_eq!(m.node_count(), 2);
    /// ```
    pub fn child(mut self, parent: &str, label: impl Into<String>) -> Self {
        let Some(p) = self.nodes.iter().position(|n| n.label == parent) else {
            return self;
        };
        let depth = self.nodes[p].depth + 1;
        let i = self.nodes.len();
        self.nodes.push(Node {
            label: label.into(),
            parent: Some(p),
            children: Vec::new(),
            side: 0,
            depth,
            leaves: 1,
            pos: Vec2::ZERO,
        });
        self.nodes[p].children.push(i);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::mind_map::MindMap;
    ///
    /// assert_eq!(MindMap::new().label("Map").label, "Map");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Node count including the root.
    ///
    /// ```
    /// use martensite::widgets::mind_map::MindMap;
    ///
    /// assert_eq!(MindMap::new().root("r").node_count(), 1);
    /// ```
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// A node's label.
    ///
    /// ```
    /// use martensite::widgets::mind_map::MindMap;
    ///
    /// assert_eq!(MindMap::new().root("r").child("r", "c").node_name(1), "c");
    /// ```
    pub fn node_name(&self, index: usize) -> &str {
        self.nodes
            .get(index)
            .map(|n| n.label.as_str())
            .unwrap_or("")
    }

    /// A node's center position after layout (`Vec2::ZERO` before).
    ///
    /// ```
    /// use martensite::widgets::mind_map::MindMap;
    ///
    /// assert_eq!(MindMap::new().root("r").node_pos(0), glam::Vec2::ZERO);
    /// ```
    pub fn node_pos(&self, index: usize) -> Vec2 {
        self.nodes.get(index).map(|n| n.pos).unwrap_or(Vec2::ZERO)
    }

    /// Which side a node fans to (-1 left, +1 right, 0 root).
    ///
    /// ```
    /// use martensite::widgets::mind_map::MindMap;
    ///
    /// assert_eq!(MindMap::new().root("r").side_of(0), 0);
    /// ```
    pub fn side_of(&self, index: usize) -> i32 {
        self.nodes.get(index).map(|n| n.side).unwrap_or(0)
    }

    /// Drains the last hovered node index.
    ///
    /// ```
    /// use martensite::widgets::mind_map::MindMap;
    ///
    /// assert_eq!(MindMap::new().take_hovered(), None);
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// Node rect under a point.
    fn node_at(&self, p: Vec2) -> Option<usize> {
        let (w, h) = (NODE_W_PT * self.scale, NODE_H_PT * self.scale);
        (0..self.nodes.len()).find(|&i| {
            let c = self.nodes[i].pos;
            Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h).contains(p)
        })
    }

    /// Leaf-slot count of a subtree.
    fn count_leaves(&mut self, i: usize) -> usize {
        if self.nodes[i].children.is_empty() {
            self.nodes[i].leaves = 1;
        } else {
            let kids = self.nodes[i].children.clone();
            self.nodes[i].leaves = kids.iter().map(|&k| self.count_leaves(k)).sum();
        }
        self.nodes[i].leaves
    }

    /// Full layout pass.
    fn compute_layout(&mut self) {
        if self.nodes.is_empty() {
            return;
        }
        self.count_leaves(0);
        // Split the root's children between sides by leaf balance
        // (each child joins the currently lighter side; ties go right).
        let kids = self.nodes[0].children.clone();
        let mut side_load = [0usize; 2]; // [right, left]
        for &k in &kids {
            let s = if side_load[0] <= side_load[1] { 1 } else { -1 };
            side_load[usize::from(s < 0)] += self.nodes[k].leaves;
            self.set_side(k, s);
        }
        let (nw, nh) = (NODE_W_PT * self.scale, NODE_H_PT * self.scale);
        let leaf_h = nh + V_GAP_PT * self.scale;
        let cx = (self.bounds.min_x() + self.bounds.max_x()) / 2.0;
        let cy = (self.bounds.min_y() + self.bounds.max_y()) / 2.0;
        self.nodes[0].pos = Vec2::new(cx, cy);
        // Each side stacks its level-1 children around the root.
        for s in [1i32, -1] {
            let side_kids: Vec<usize> = kids
                .iter()
                .copied()
                .filter(|&k| self.nodes[k].side == s)
                .collect();
            let total: usize = side_kids.iter().map(|&k| self.nodes[k].leaves).sum();
            let mut y = cy - total as f32 * leaf_h / 2.0;
            for k in side_kids {
                let slot_h = self.nodes[k].leaves as f32 * leaf_h;
                let slot_mid = y + slot_h / 2.0;
                self.place(
                    k,
                    cx + s as f32 * (nw + COL_GAP_PT * self.scale),
                    slot_mid,
                    leaf_h,
                    s,
                );
                y += slot_h;
            }
        }
    }

    /// Propagate a side down a subtree.
    fn set_side(&mut self, i: usize, s: i32) {
        self.nodes[i].side = s;
        let kids = self.nodes[i].children.clone();
        for k in kids {
            self.set_side(k, s);
        }
    }

    /// Place a node at `x`/center-y, stacking children in its slot.
    fn place(&mut self, i: usize, x: f32, mid_y: f32, leaf_h: f32, s: i32) {
        self.nodes[i].pos = Vec2::new(x, mid_y);
        let kids = self.nodes[i].children.clone();
        if kids.is_empty() {
            return;
        }
        let col_pitch = (NODE_W_PT + COL_GAP_PT) * self.scale;
        let total: usize = kids.iter().map(|&k| self.nodes[k].leaves).sum();
        let mut y = mid_y - total as f32 * leaf_h / 2.0;
        for k in kids {
            let slot_h = self.nodes[k].leaves as f32 * leaf_h;
            self.place(k, x + s as f32 * col_pitch, y + slot_h / 2.0, leaf_h, s);
            y += slot_h;
        }
    }
}

impl Widget for MindMap {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 80.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.compute_layout();
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!("{} — {} nodes", self.label, self.nodes.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.node_at(*position);
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
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        if self.nodes.is_empty() {
            return;
        }
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        let link = cx.color(TokenKey::TextMutedColor, LINK);
        let (nw, nh) = (NODE_W_PT * self.scale, NODE_H_PT * self.scale);
        // Elbow links first so nodes overlay the joins.
        for n in &self.nodes {
            let Some(p) = n.parent else { continue };
            let (a, b) = (self.nodes[p].pos, n.pos);
            let s = n.side as f32;
            let mut path = kurbo::BezPath::new();
            path.move_to((f64::from(a.x + s * nw / 2.0), f64::from(a.y)));
            let mid_x = a.x + s * nw / 2.0 + (b.x - (a.x + s * nw / 2.0)) * 0.5;
            path.curve_to(
                (f64::from(mid_x), f64::from(a.y)),
                (f64::from(mid_x), f64::from(b.y)),
                (f64::from(b.x - s * nw / 2.0), f64::from(b.y)),
            );
            cx.list.push_stroke_path(path, cx.pt(1.0), link);
        }
        // Nodes.
        for (i, n) in self.nodes.iter().enumerate() {
            let r = Rect::new(n.pos.x - nw / 2.0, n.pos.y - nh / 2.0, nw, nh);
            let fill = if i == 0 {
                cx.color(TokenKey::AccentColor, ROOT)
            } else if self.hovered == Some(i) {
                HOVER
            } else {
                NODE
            };
            cx.list.push_fill_shape(
                krect(r),
                &martensite_core::shape::Shape::rounded(nh / 2.0),
                fill,
            );
            cx.list.push_stroke_shape(
                krect(r),
                &martensite_core::shape::Shape::rounded(nh / 2.0),
                cx.pt(0.75),
                edge,
            );
            // Label stub — a short bar centered in the node.
            let lw = (n.label.len() as f32 * 3.0 * self.scale).min(nw - 12.0 * self.scale);
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(n.pos.x - lw / 2.0),
                    f64::from(n.pos.y - 1.0 * self.scale),
                    f64::from(n.pos.x + lw / 2.0),
                    f64::from(n.pos.y + 1.0 * self.scale),
                ),
                &martensite_core::shape::Shape::rounded(self.scale),
                cx.color(TokenKey::TextColor, GLYPH),
            );
        }
        cx.list.push_stroke_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.pt(0.75),
            edge,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(m: &mut MindMap, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        m.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        m.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn builds_tree() {
        let m = MindMap::new()
            .root("r")
            .child("r", "a")
            .child("a", "a1")
            .child("ghost", "ignored");
        assert_eq!(m.node_count(), 3);
        assert_eq!(m.node_name(2), "a1");
    }

    #[test]
    fn sides_balance_by_leaves() {
        // Three children of 1 leaf each: first two go right/left
        // by the <= tiebreak, third balances onto the right? No —
        // loads after two: right=1, left=1 → third also right.
        let mut m = MindMap::new()
            .root("r")
            .child("r", "a")
            .child("r", "b")
            .child("r", "c");
        laid_out(&mut m, 300.0, 200.0);
        assert_eq!(m.side_of(1), 1);
        assert_eq!(m.side_of(2), -1);
        assert_eq!(m.side_of(3), 1); // tie goes right
    }

    #[test]
    fn heavy_child_forces_balance() {
        // "big" has 3 leaves; it lands right first (load 3), so
        // "a" and "b" both go left.
        let mut m = MindMap::new()
            .root("r")
            .child("r", "big")
            .child("big", "b1")
            .child("big", "b2")
            .child("big", "b3")
            .child("r", "a")
            .child("r", "b");
        laid_out(&mut m, 400.0, 300.0);
        assert_eq!(m.side_of(1), 1); // big → right
        assert_eq!(m.side_of(5), -1); // a → left
        assert_eq!(m.side_of(6), -1); // b → left
    }

    #[test]
    fn positions_fan_out() {
        let mut m = MindMap::new().root("r").child("r", "a").child("r", "b");
        laid_out(&mut m, 400.0, 300.0);
        let (root, a, b) = (m.node_pos(0), m.node_pos(1), m.node_pos(2));
        assert!(a.x > root.x); // right side
        assert!(b.x < root.x); // left side
    }

    #[test]
    fn hover_parks_node() {
        let mut m = MindMap::new().root("r").child("r", "a");
        laid_out(&mut m, 300.0, 200.0);
        let p = m.node_pos(1);
        m.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved { position: p },
            bounds: m.bounds,
            scale: 1.0,
        });
        assert_eq!(m.take_hovered(), Some(1));
    }
}
