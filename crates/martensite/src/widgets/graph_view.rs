//! `GraphView` — a general node-link diagram: labeled nodes
//! joined by straight edges, with draggable nodes (the
//! free-form sibling of [`crate::widgets::org_chart::OrgChart`],
//! [`crate::widgets::mind_map::MindMap`], and
//! [`crate::widgets::sankey::Sankey`]).
//!
//! Hosts supply node labels and `(from, to)` edge pairs; the
//! widget places nodes on a ring by default (deterministic —
//! force layouts need a sim loop, which `tick` provides via
//! [`GraphView::relax`]). Dragging a node repins it and parks
//! the index in [`GraphView::take_moved`]; hovering parks it in
//! [`GraphView::take_hovered`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::graph_view::GraphView;
//!
//! let g = GraphView::new()
//!     .node("a").node("b").node("c")
//!     .edge(0, 1).edge(1, 2);
//! assert_eq!(g.node_count(), 3);
//! assert_eq!(g.edge_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use std::f32::consts::TAU;
use std::time::Duration;

const SIZE_PT: f32 = 260.0;
const NODE_PT: f32 = 14.0;
/// Spring-relax iterations per tick.
const RELAX_ITERS: usize = 2;

const FACE: [u8; 4] = [30, 30, 34, 255];
const EDGE: [u8; 4] = [90, 90, 98, 255];
const NODE: [u8; 4] = [120, 200, 255, 255];
const NODE_HI: [u8; 4] = [160, 220, 255, 255];
const TEXT: [u8; 4] = [210, 210, 216, 255];

/// A node-link diagram — see the module docs.
///
/// ```
/// use martensite::widgets::graph_view::GraphView;
///
/// assert_eq!(GraphView::new().node_count(), 0);
/// ```
pub struct GraphView {
    /// Accessibility label.
    pub label: String,
    /// Node labels (index-stable).
    pub nodes: Vec<String>,
    /// `(from, to)` node-index edges.
    pub edges: Vec<(usize, usize)>,
    /// Node centers in widget coords; `layout` seeds a ring.
    positions: Vec<Vec2>,
    /// Nodes pinned by a drag (relax skips them).
    pinned: Vec<bool>,
    drag: Option<usize>,
    hover: Option<usize>,
    hovered: Option<usize>,
    moved: Option<usize>,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for GraphView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphView")
            .field("nodes", &self.nodes.len())
            .field("edges", &self.edges.len())
            .finish()
    }
}

impl Default for GraphView {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphView {
    /// Creates an empty graph.
    ///
    /// ```
    /// use martensite::widgets::graph_view::GraphView;
    ///
    /// assert!(GraphView::new().is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Graph".to_string(),
            nodes: Vec::new(),
            edges: Vec::new(),
            positions: Vec::new(),
            pinned: Vec::new(),
            drag: None,
            hover: None,
            hovered: None,
            moved: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Appends a labeled node; returns the index implicitly.
    ///
    /// ```
    /// use martensite::widgets::graph_view::GraphView;
    ///
    /// assert_eq!(GraphView::new().node("n").node_count(), 1);
    /// ```
    pub fn node(mut self, label: impl Into<String>) -> Self {
        self.nodes.push(label.into());
        self.positions.push(Vec2::ZERO);
        self.pinned.push(false);
        self
    }

    /// Connects nodes `a` and `b` (out-of-range indices are
    /// ignored).
    ///
    /// ```
    /// use martensite::widgets::graph_view::GraphView;
    ///
    /// let g = GraphView::new().node("a").node("b").edge(0, 1).edge(0, 9);
    /// assert_eq!(g.edge_count(), 1);
    /// ```
    pub fn edge(mut self, a: usize, b: usize) -> Self {
        if a < self.nodes.len() && b < self.nodes.len() && a != b {
            self.edges.push((a, b));
        }
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::graph_view::GraphView;
    ///
    /// assert_eq!(GraphView::new().label("cfg").label, "cfg");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::graph_view::GraphView;
    ///
    /// let _ = GraphView::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Node count.
    ///
    /// ```
    /// use martensite::widgets::graph_view::GraphView;
    ///
    /// assert_eq!(GraphView::new().node("x").node_count(), 1);
    /// ```
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Edge count.
    ///
    /// ```
    /// use martensite::widgets::graph_view::GraphView;
    ///
    /// assert_eq!(GraphView::new().edge_count(), 0);
    /// ```
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Whether the graph has no nodes.
    ///
    /// ```
    /// use martensite::widgets::graph_view::GraphView;
    ///
    /// assert!(GraphView::new().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Node `i`'s center in widget coords (zero before layout).
    ///
    /// ```
    /// use martensite::widgets::graph_view::GraphView;
    ///
    /// assert_eq!(GraphView::new().node("x").position_of(0), glam::Vec2::ZERO);
    /// ```
    pub fn position_of(&self, i: usize) -> Vec2 {
        self.positions.get(i).copied().unwrap_or(Vec2::ZERO)
    }

    /// Drains the last dragged/hovered index respectively.
    ///
    /// ```
    /// use martensite::widgets::graph_view::GraphView;
    ///
    /// assert_eq!(GraphView::new().take_moved(), None);
    /// ```
    pub fn take_moved(&mut self) -> Option<usize> {
        self.moved.take()
    }

    /// Drains the last hovered node index.
    ///
    /// ```
    /// use martensite::widgets::graph_view::GraphView;
    ///
    /// assert_eq!(GraphView::new().take_hovered(), None);
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.hovered.take()
    }

    /// One spring-relax pass (call per frame for animated
    /// settling); returns `true` while nodes are still moving.
    /// Pinned nodes don't move.
    ///
    /// ```
    /// use martensite::widgets::graph_view::GraphView;
    ///
    /// assert!(!GraphView::new().relax()); // empty graph settles
    /// ```
    pub fn relax(&mut self) -> bool {
        let n = self.nodes.len();
        if n < 2 {
            return false;
        }
        let node_r = NODE_PT * self.scale;
        let mut moved = false;
        for _ in 0..RELAX_ITERS {
            let mut delta = vec![Vec2::ZERO; n];
            // Repulsion between all pairs.
            for i in 0..n {
                for j in (i + 1)..n {
                    let d = self.positions[i] - self.positions[j];
                    let dist = d.length().max(1.0);
                    let want = node_r * 6.0;
                    if dist < want {
                        let push = d / dist * (want - dist) * 0.08;
                        delta[i] += push;
                        delta[j] -= push;
                    }
                }
            }
            // Springs along edges.
            for &(a, b) in &self.edges {
                let d = self.positions[b] - self.positions[a];
                let dist = d.length().max(1.0);
                let want = node_r * 5.0;
                let pull = d / dist * (dist - want) * 0.05;
                delta[a] += pull;
                delta[b] -= pull;
            }
            for (i, d) in delta.iter().enumerate() {
                if !self.pinned[i] && d.length() > 0.05 {
                    self.positions[i] += *d;
                    moved = true;
                }
            }
        }
        for i in 0..n {
            self.clamp_inside(i);
        }
        moved
    }

    /// Keeps node `i` — and the caption painted `r*1.2` below it —
    /// inside the widget bounds. Springs and drags can push nodes out;
    /// without this their labels paint past the container edge.
    fn clamp_inside(&mut self, i: usize) {
        let r = self.node_r();
        let size = 12.0 * self.scale;
        let half_w = self.nodes[i].chars().count() as f32 * size * 0.32;
        let inset_x = r.max(half_w);
        let top = r;
        let bottom = r * 1.2 + size * 1.3;
        let b = self.bounds;
        // Pre-layout or too-small bounds: nothing meaningful to clamp to.
        if b.width() < inset_x * 2.0 || b.height() < top + bottom {
            return;
        }
        let p = self.positions[i];
        self.positions[i] = Vec2::new(
            p.x.clamp(b.min_x() + inset_x, b.max_x() - inset_x),
            p.y.clamp(b.min_y() + top, b.max_y() - bottom),
        );
    }

    /// Node radius in pixels.
    fn node_r(&self) -> f32 {
        NODE_PT * self.scale
    }

    /// Node index at `p`, if any.
    fn hit(&self, p: Vec2) -> Option<usize> {
        let r = self.node_r() * 1.4;
        self.positions.iter().position(|c| c.distance(p) <= r)
    }
}

impl Widget for GraphView {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(SIZE_PT);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 80.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        // Seed a ring for any unpositioned nodes.
        let n = self.nodes.len();
        if n == 0 {
            return;
        }
        let c = Vec2::new(
            (bounds.min_x() + bounds.max_x()) / 2.0,
            (bounds.min_y() + bounds.max_y()) / 2.0,
        );
        let r = (bounds.width().min(bounds.height()) / 2.0 - self.node_r() * 2.0).max(1.0);
        for i in 0..n {
            if self.positions[i] == Vec2::ZERO {
                let a = i as f32 / n as f32 * TAU - TAU / 4.0;
                self.positions[i] = c + Vec2::new(a.cos(), a.sin()) * r;
            }
            // Resizes leave stale coordinates — a node seeded for a
            // taller allotment keeps it until the next relax tick.
            self.clamp_inside(i);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!(
            "{} — {} nodes, {} edges",
            self.label,
            self.nodes.len(),
            self.edges.len()
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self.hit(*position) {
                    self.drag = Some(i);
                    self.pinned[i] = true;
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some(i) = self.drag {
                    self.positions[i] = *position;
                    self.clamp_inside(i);
                    self.moved = Some(i);
                    return EventResponse::RequestRepaint;
                }
                let h = if self.bounds.contains(*position) {
                    self.hit(*position)
                } else {
                    None
                };
                if h != self.hover {
                    self.hover = h;
                    self.hovered = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if let Some(i) = self.drag.take() {
                    self.pinned[i] = true; // stays pinned where dropped
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hover.take().is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, _dt: Duration) -> bool {
        // Relax settles the layout; drag positions are pinned.
        self.relax()
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
        let wire = cx.color(TokenKey::BorderColor, EDGE);
        // Edges.
        for &(a, b) in &self.edges {
            let (pa, pb) = (self.position_of(a), self.position_of(b));
            let mut path = kurbo::BezPath::new();
            path.move_to((f64::from(pa.x), f64::from(pa.y)));
            path.line_to((f64::from(pb.x), f64::from(pb.y)));
            cx.list.push_stroke_path(path, self.scale.max(0.75), wire);
        }
        // Nodes + labels.
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let r = self.node_r();
        let size = 12.0 * self.scale;
        // Node captions can't reflow — when two nodes sit close their
        // labels would overprint. First-wins culling keeps the
        // diagram readable instead of stacking runs.
        let mut label_boxes: Vec<kurbo::Rect> = Vec::with_capacity(self.positions.len());
        let node_box = |p: &Vec2| {
            kurbo::Rect::new(
                f64::from(p.x - r),
                f64::from(p.y - r),
                f64::from(p.x + r),
                f64::from(p.y + r),
            )
        };
        // Circles first — a caption must never sit *under* a later
        // node's fill.
        for (i, p) in self.positions.iter().enumerate() {
            let fill = if self.hover == Some(i) || self.drag == Some(i) {
                NODE_HI
            } else {
                cx.color(TokenKey::AccentColor, NODE)
            };
            cx.list.push_fill_shape(
                node_box(p),
                &martensite_core::shape::Shape::circle(*p, r),
                fill,
            );
        }
        for (i, p) in self.positions.iter().enumerate() {
            let origin = kurbo::Point::new(
                f64::from(p.x - self.nodes[i].chars().count() as f32 * size * 0.28),
                f64::from(p.y + r * 1.2),
            );
            // Captions dragged outside the enclosing clip are dead
            // emissions — cull them.
            let ink = crate::text_paint::label_ink_bounds(painter, origin, &self.nodes[i], size);
            let on_clip = ink.is_none_or(|b| crate::text_paint::visible_ink(cx.list, b));
            // ...or *on* a node's circle — captions print on the
            // widget face, and a crowded layout that puts ink on a
            // chromatic node reads as mud. Skip those labels.
            let collides = ink.is_some_and(|b| {
                let hits_box =
                    |o: &kurbo::Rect| b.x0 < o.x1 && b.x1 > o.x0 && b.y0 < o.y1 && b.y1 > o.y0;
                label_boxes.iter().any(hits_box)
                    || self
                        .positions
                        .iter()
                        .enumerate()
                        .any(|(j, q)| j != i && hits_box(&node_box(q)))
            });
            if on_clip && !collides {
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    origin,
                    &self.nodes[i],
                    size,
                    cx.color(TokenKey::TextColor, TEXT),
                );
                if let Some(b) = ink {
                    label_boxes.push(b);
                }
            }
        }
        cx.list.push_stroke_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.pt(0.75),
            wire,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn graph() -> GraphView {
        GraphView::new()
            .node("a")
            .node("b")
            .node("c")
            .edge(0, 1)
            .edge(1, 2)
    }

    fn laid_out(w: &mut GraphView, wd: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(wd, h),
            },
        );
        w.layout(&mut cx, Rect::new(0.0, 0.0, wd, h));
    }

    #[test]
    fn builds_and_validates_edges() {
        let g = graph().edge(0, 0).edge(0, 9);
        assert_eq!(g.node_count(), 3);
        assert_eq!(g.edge_count(), 2); // self-loop + OOB dropped
    }

    #[test]
    fn layout_seeds_ring() {
        let mut g = graph();
        laid_out(&mut g, 260.0, 260.0);
        let c = Vec2::new(130.0, 130.0);
        for i in 0..3 {
            let p = g.position_of(i);
            assert!((p.distance(c) - 102.0).abs() < 1.0, "node {i} off ring");
        }
    }

    #[test]
    fn drag_repins_node() {
        let mut g = graph();
        laid_out(&mut g, 260.0, 260.0);
        let p0 = g.position_of(0);
        g.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: p0,
                count: 1,
            },
            bounds: g.bounds,
            scale: 1.0,
        });
        g.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(60.0, 60.0),
            },
            bounds: g.bounds,
            scale: 1.0,
        });
        g.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(60.0, 60.0),
            },
            bounds: g.bounds,
            scale: 1.0,
        });
        assert_eq!(g.position_of(0), Vec2::new(60.0, 60.0));
        assert_eq!(g.take_moved(), Some(0));
        // Pinned node doesn't relax.
        g.relax();
        assert_eq!(g.position_of(0), Vec2::new(60.0, 60.0));
    }

    #[test]
    fn relax_separates_neighbors() {
        let mut g = GraphView::new().node("x").node("y").edge(0, 1);
        laid_out(&mut g, 260.0, 260.0);
        // Overlap both nodes manually.
        g.positions[1] = g.positions[0] + Vec2::new(4.0, 0.0);
        let d0 = g.position_of(0).distance(g.position_of(1));
        for _ in 0..30 {
            g.relax();
        }
        assert!(g.position_of(0).distance(g.position_of(1)) > d0);
    }
}
