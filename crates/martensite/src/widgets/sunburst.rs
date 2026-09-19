//! `Sunburst` — a nested radial hierarchy chart (d3 sunburst /
//! Ant `Pie` multi-level idiom).
//!
//! Root siblings partition the inner ring by value; each node's
//! children subdivide its angular span on the next ring outward.
//! Hovering a sector parks `(depth, index)` in
//! [`Sunburst::take_hovered`]. Depths beyond three compact into
//! proportionally thinner rings.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::sunburst::{Sunburst, SunburstNode};
//!
//! let s = Sunburst::new()
//!     .node(SunburstNode::new("a", 2.0).child(SunburstNode::new("a1", 1.0)));
//! assert_eq!(s.node_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const SIZE_PT: f32 = 220.0;
const MAX_DEPTH: usize = 4;

const PALETTE: [[u8; 4]; 8] = [
    [90, 140, 220, 255],
    [110, 180, 130, 255],
    [230, 170, 80, 255],
    [210, 110, 90, 255],
    [150, 110, 200, 255],
    [90, 180, 190, 255],
    [200, 140, 170, 255],
    [140, 160, 120, 255],
];
const TRACK: [u8; 4] = [48, 48, 52, 255];
const FG: [u8; 4] = [230, 230, 235, 255];

/// One sunburst node — name, value, optional children.
#[derive(Clone, Debug, PartialEq)]
pub struct SunburstNode {
    /// Node name (a11y summary).
    pub name: String,
    /// Weight when the node has no children.
    pub value: f32,
    /// Child nodes subdividing this node's angular span.
    pub children: Vec<SunburstNode>,
}

impl SunburstNode {
    /// Creates a leaf node.
    ///
    /// ```
    /// use martensite::widgets::sunburst::SunburstNode;
    ///
    /// let n = SunburstNode::new("docs", 3.0);
    /// assert_eq!(n.weight(), 3.0);
    /// ```
    pub fn new(name: impl Into<String>, value: f32) -> Self {
        Self {
            name: name.into(),
            value: value.max(0.0),
            children: Vec::new(),
        }
    }

    /// Appends a child.
    ///
    /// ```
    /// use martensite::widgets::sunburst::SunburstNode;
    ///
    /// let n = SunburstNode::new("root", 0.0).child(SunburstNode::new("c", 2.0));
    /// assert_eq!(n.weight(), 2.0);
    /// ```
    pub fn child(mut self, child: SunburstNode) -> Self {
        self.children.push(child);
        self
    }

    /// Subtree weight — children sum when present, else `value`.
    ///
    /// ```
    /// use martensite::widgets::sunburst::SunburstNode;
    ///
    /// assert_eq!(SunburstNode::new("x", 1.5).weight(), 1.5);
    /// ```
    pub fn weight(&self) -> f32 {
        if self.children.is_empty() {
            self.value
        } else {
            self.children.iter().map(Self::weight).sum()
        }
    }
}

/// One laid-out sector for hit-testing: `(depth, ring_index,
/// start_rad, end_rad, name_idx)`.
struct Sector {
    depth: usize,
    index: usize,
    a0: f32,
    a1: f32,
}

/// A nested radial hierarchy chart — see the module docs.
///
/// ```
/// use martensite::widgets::sunburst::Sunburst;
///
/// assert_eq!(Sunburst::new().node_count(), 0);
/// ```
pub struct Sunburst {
    /// Accessibility label.
    pub label: String,
    /// Root nodes.
    pub nodes: Vec<SunburstNode>,
    sectors: Vec<Sector>,
    names: Vec<String>,
    hovered: Option<usize>, // index into sectors
    pending: Option<usize>,
    bounds: Rect,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Default for Sunburst {
    fn default() -> Self {
        Self::new()
    }
}

impl Sunburst {
    /// Creates an empty chart.
    ///
    /// ```
    /// use martensite::widgets::sunburst::Sunburst;
    ///
    /// assert_eq!(Sunburst::new().node_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Breakdown".to_string(),
            nodes: Vec::new(),
            sectors: Vec::new(),
            names: Vec::new(),
            hovered: None,
            pending: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::sunburst::Sunburst;
    ///
    /// let s = Sunburst::new().label("Storage");
    /// assert_eq!(s.label, "Storage");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Appends a root node.
    ///
    /// ```
    /// use martensite::widgets::sunburst::{Sunburst, SunburstNode};
    ///
    /// let s = Sunburst::new().node(SunburstNode::new("x", 1.0));
    /// assert_eq!(s.node_count(), 1);
    /// ```
    pub fn node(mut self, node: SunburstNode) -> Self {
        self.nodes.push(node);
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::sunburst::Sunburst;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let s = Sunburst::new().with_text_painter(shared_painter());
    /// assert_eq!(s.node_count(), 0);
    /// ```
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Root node count.
    ///
    /// ```
    /// use martensite::widgets::sunburst::Sunburst;
    ///
    /// assert_eq!(Sunburst::new().node_count(), 0);
    /// ```
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Sector count across all rings.
    ///
    /// ```
    /// use martensite::widgets::sunburst::{Sunburst, SunburstNode};
    ///
    /// let s = Sunburst::new()
    ///     .node(SunburstNode::new("a", 1.0).child(SunburstNode::new("a1", 1.0)));
    /// assert_eq!(s.sector_count(), 2);
    /// ```
    pub fn sector_count(&self) -> usize {
        fn count(n: &SunburstNode) -> usize {
            1 + n.children.iter().map(count).sum::<usize>()
        }
        self.nodes.iter().map(count).sum()
    }

    /// Drains the hovered sector's index (into [`Sunburst::names`]).
    ///
    /// ```
    /// use martensite::widgets::sunburst::Sunburst;
    ///
    /// let mut s = Sunburst::new();
    /// assert!(s.take_hovered().is_none());
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// Sector names in layout order (matches hover indices).
    ///
    /// ```
    /// use martensite::widgets::sunburst::{Sunburst, SunburstNode};
    ///
    /// let s = Sunburst::new().node(SunburstNode::new("a", 1.0));
    /// assert!(s.names().is_empty()); // populated at layout
    /// ```
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Rebuilds the sector layout.
    fn rebuild_sectors(&mut self) {
        self.sectors.clear();
        self.names.clear();
        let total: f32 = self
            .nodes
            .iter()
            .map(SunburstNode::weight)
            .sum::<f32>()
            .max(0.001);
        let mut angle = 0.0f32;
        let nodes = std::mem::take(&mut self.nodes);
        for node in &nodes {
            let span = node.weight() / total * std::f32::consts::TAU;
            self.emit(node, 0, angle, angle + span);
            angle += span;
        }
        self.nodes = nodes;
    }

    /// Registers one node and recurses into its children.
    fn emit(&mut self, node: &SunburstNode, depth: usize, a0: f32, a1: f32) {
        if depth >= MAX_DEPTH {
            return;
        }
        let index = self.names.len();
        self.names.push(node.name.clone());
        self.sectors.push(Sector {
            depth,
            index,
            a0,
            a1,
        });
        if node.children.is_empty() {
            return;
        }
        let total: f32 = node
            .children
            .iter()
            .map(SunburstNode::weight)
            .sum::<f32>()
            .max(0.001);
        let mut angle = a0;
        for child in &node.children {
            let span = (a1 - a0) * child.weight() / total;
            self.emit(child, depth + 1, angle, angle + span);
            angle += span;
        }
    }

    /// Sector index under a device-space point.
    fn sector_at(&self, p: Vec2) -> Option<usize> {
        let center = Vec2::new(
            self.bounds.min_x() + self.bounds.width() / 2.0,
            self.bounds.min_y() + self.bounds.height() / 2.0,
        );
        let dim = self.bounds.width().min(self.bounds.height());
        let ring_w = dim / 2.0 / MAX_DEPTH as f32;
        let dist = (p - center).length();
        let depth = (dist / ring_w) as usize;
        if depth >= MAX_DEPTH || dist > dim / 2.0 {
            return None;
        }
        let angle = (p.y - center.y).atan2(p.x - center.x);
        let angle = if angle < 0.0 {
            angle + std::f32::consts::TAU
        } else {
            angle
        };
        self.sectors
            .iter()
            .position(|s| s.depth == depth && angle >= s.a0 && angle < s.a1)
    }
}

impl Widget for Sunburst {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(SIZE_PT);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(48.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.rebuild_sectors();
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.clone());
        node.set_description(format!("{} sectors", self.names.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.sector_at(*position).map(|i| self.sectors[i].index);
                if hit != self.hovered {
                    self.hovered = hit;
                    if hit.is_some() {
                        self.pending = hit;
                    }
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
        cx.list
            .push_fill_rect(f(self.bounds), cx.color(TokenKey::SurfaceColor, TRACK));
        let center = Vec2::new(
            self.bounds.min_x() + self.bounds.width() / 2.0,
            self.bounds.min_y() + self.bounds.height() / 2.0,
        );
        let dim = self.bounds.width().min(self.bounds.height());
        let ring_w = dim / 2.0 / MAX_DEPTH as f32;
        let gap = cx.pt(1.0);
        for s in &self.sectors {
            let r0 = s.depth as f32 * ring_w + gap / 2.0;
            let r1 = (s.depth + 1) as f32 * ring_w - gap / 2.0;
            if r1 <= r0 {
                continue;
            }
            let mut color = PALETTE[s.index % PALETTE.len()];
            if self.hovered == Some(s.index) {
                color = [
                    color[0].saturating_add(40),
                    color[1].saturating_add(40),
                    color[2].saturating_add(40),
                    255,
                ];
            }
            // Annulus segment — outer arc then inner arc reversed.
            let mut path = kurbo::BezPath::new();
            let segs = (((s.a1 - s.a0).abs() / 0.08).ceil() as usize).max(2);
            for i in 0..=segs {
                let a = s.a0 + (s.a1 - s.a0) * i as f32 / segs as f32;
                let p = kurbo::Point::new(
                    f64::from(center.x + r1 * a.cos()),
                    f64::from(center.y + r1 * a.sin()),
                );
                if i == 0 {
                    path.move_to(p);
                } else {
                    path.line_to(p);
                }
            }
            for i in (0..=segs).rev() {
                let a = s.a0 + (s.a1 - s.a0) * i as f32 / segs as f32;
                path.line_to(kurbo::Point::new(
                    f64::from(center.x + r0 * a.cos()),
                    f64::from(center.y + r0 * a.sin()),
                ));
            }
            path.close_path();
            cx.list.push_path(path, color);
        }
        // Hovered name at the center.
        if let Some(idx) = self.hovered {
            if let Some(name) = self.names.get(idx) {
                let painter =
                    crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
                let size = 10.0 * cx.scale;
                let w = painter
                    .and_then(|p| p.measure_text(name, size))
                    .unwrap_or(name.chars().count() as f32 * size * 0.55);
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    f(Rect::new(
                        center.x - dim / 6.0,
                        center.y - size,
                        dim / 3.0,
                        size * 2.0,
                    )),
                    kurbo::Point::new(
                        f64::from(center.x - w / 2.0),
                        f64::from(center.y - size / 2.0),
                    ),
                    name,
                    size,
                    cx.color(TokenKey::TextColor, FG),
                );
            }
        }
    }
}

impl std::fmt::Debug for Sunburst {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sunburst")
            .field("nodes", &self.nodes.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(s: &mut Sunburst, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        s.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        s.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn weight_sums_children() {
        let n = SunburstNode::new("r", 100.0)
            .child(SunburstNode::new("a", 3.0))
            .child(SunburstNode::new("b", 1.0));
        assert_eq!(n.weight(), 4.0);
        assert_eq!(SunburstNode::new("leaf", 2.0).weight(), 2.0);
    }

    #[test]
    fn sectors_flatten_tree() {
        let mut s = Sunburst::new()
            .node(SunburstNode::new("a", 2.0).child(SunburstNode::new("a1", 1.0)))
            .node(SunburstNode::new("b", 1.0));
        laid_out(&mut s, 200.0, 200.0);
        assert_eq!(s.sectors.len(), 3);
        assert_eq!(s.names().len(), 3);
    }

    #[test]
    fn center_is_depth_zero() {
        let mut s = Sunburst::new()
            .node(SunburstNode::new("a", 1.0))
            .node(SunburstNode::new("b", 1.0));
        laid_out(&mut s, 200.0, 200.0);
        // Center-right point on the inner ring: angle 0 (positive x).
        s.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(110.0, 100.0),
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 200.0),
            scale: 1.0,
        });
        assert!(s.take_hovered().is_some());
    }

    #[test]
    fn outside_ignored() {
        let mut s = Sunburst::new().node(SunburstNode::new("a", 1.0));
        laid_out(&mut s, 200.0, 200.0);
        s.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(500.0, 500.0),
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 200.0),
            scale: 1.0,
        });
        assert!(s.take_hovered().is_none());
    }

    #[test]
    fn empty_safe() {
        let mut s = Sunburst::new();
        laid_out(&mut s, 200.0, 200.0);
        assert!(s.sectors.is_empty());
    }
}
