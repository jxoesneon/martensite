//! `Sankey` — a flow diagram (layered node columns joined by
//! value-weighted ribbons — the d3-sankey / energy-flow idiom).
//!
//! Nodes are declared in column order via
//! [`Sankey::node`]; weighted [`Sankey::link`]s connect them.
//! Layout assigns each node to a column (explicit `column`
//! hints, else longest-path layering), sizes it by total
//! throughput, and draws links as ribbon bands whose width is
//! proportional to value. Hovering a ribbon parks its index in
//! [`Sankey::take_hovered`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::sankey::Sankey;
//!
//! let s = Sankey::new()
//!     .node("in")
//!     .node("out")
//!     .link("in", "out", 4.0);
//! assert_eq!(s.node_count(), 2);
//! assert_eq!(s.throughput(0), 4.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const WIDTH_PT: f32 = 320.0;
const HEIGHT_PT: f32 = 160.0;
const PAD_PT: f32 = 8.0;
const NODE_PT: f32 = 12.0;
const V_GAP_PT: f32 = 8.0;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const PALETTE: [[u8; 4]; 6] = [
    [110, 170, 230, 255],
    [230, 150, 90, 255],
    [120, 200, 140, 255],
    [220, 110, 110, 255],
    [190, 140, 230, 255],
    [230, 200, 110, 255],
];

fn dim(c: [u8; 4], a: u8) -> [u8; 4] {
    [c[0], c[1], c[2], a]
}

/// A weighted flow diagram — see the module docs.
///
/// ```
/// use martensite::widgets::sankey::Sankey;
///
/// assert_eq!(Sankey::new().node_count(), 0);
/// ```
#[derive(Debug)]
pub struct Sankey {
    /// Accessibility label.
    pub label: String,
    nodes: Vec<Node>,
    links: Vec<Link>,
    hovered: Option<usize>,
    pending: Option<usize>,
    bounds: Rect,
    scale: f32,
}

#[derive(Debug)]
struct Node {
    name: String,
    /// Column hint; `None` → longest-path layering.
    column: Option<usize>,
    /// Layout: resolved column, y top, height.
    col: usize,
    y: f32,
    h: f32,
}

#[derive(Debug)]
struct Link {
    from: usize,
    to: usize,
    value: f32,
    /// Cached ribbon path after layout/paint for hit testing.
    /// Approximated by the link's endpoint midpoints.
    mid: Vec<(Vec2, Vec2)>,
}

impl Default for Sankey {
    fn default() -> Self {
        Self::new()
    }
}

impl Sankey {
    /// Creates an empty diagram.
    ///
    /// ```
    /// use martensite::widgets::sankey::Sankey;
    ///
    /// assert_eq!(Sankey::new().node_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Sankey".to_string(),
            nodes: Vec::new(),
            links: Vec::new(),
            hovered: None,
            pending: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Adds a node to the next unnamed column.
    ///
    /// ```
    /// use martensite::widgets::sankey::Sankey;
    ///
    /// assert_eq!(Sankey::new().node("a").node_count(), 1);
    /// ```
    pub fn node(self, name: impl Into<String>) -> Self {
        self.node_at(name, None)
    }

    /// Adds a node pinned to a specific column.
    ///
    /// ```
    /// use martensite::widgets::sankey::Sankey;
    ///
    /// let s = Sankey::new().node_at("a", Some(2));
    /// assert_eq!(s.column_of(0), 2);
    /// ```
    pub fn node_at(mut self, name: impl Into<String>, column: Option<usize>) -> Self {
        self.nodes.push(Node {
            name: name.into(),
            column,
            col: 0,
            y: 0.0,
            h: 0.0,
        });
        self
    }

    /// Adds a weighted link between two named nodes.
    ///
    /// Unknown endpoints are ignored.
    ///
    /// ```
    /// use martensite::widgets::sankey::Sankey;
    ///
    /// let s = Sankey::new().node("a").node("b").link("a", "b", 3.0);
    /// assert_eq!(s.link_count(), 1);
    /// ```
    pub fn link(mut self, from: &str, to: &str, value: f32) -> Self {
        let (Some(f), Some(t)) = (
            self.nodes.iter().position(|n| n.name == from),
            self.nodes.iter().position(|n| n.name == to),
        ) else {
            return self;
        };
        if f == t || value <= 0.0 {
            return self;
        }
        self.links.push(Link {
            from: f,
            to: t,
            value,
            mid: Vec::new(),
        });
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::sankey::Sankey;
    ///
    /// assert_eq!(Sankey::new().label("Energy").label, "Energy");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Node count.
    ///
    /// ```
    /// use martensite::widgets::sankey::Sankey;
    ///
    /// assert_eq!(Sankey::new().node("x").node_count(), 1);
    /// ```
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Link count.
    ///
    /// ```
    /// use martensite::widgets::sankey::Sankey;
    ///
    /// assert_eq!(Sankey::new().link_count(), 0);
    /// ```
    pub fn link_count(&self) -> usize {
        self.links.len()
    }

    /// A node's name.
    ///
    /// ```
    /// use martensite::widgets::sankey::Sankey;
    ///
    /// assert_eq!(Sankey::new().node("x").node_name(0), "x");
    /// ```
    pub fn node_name(&self, index: usize) -> &str {
        self.nodes.get(index).map(|n| n.name.as_str()).unwrap_or("")
    }

    /// A node's resolved column (before layout: the hint or 0).
    ///
    /// ```
    /// use martensite::widgets::sankey::Sankey;
    ///
    /// assert_eq!(Sankey::new().node("x").column_of(0), 0);
    /// ```
    pub fn column_of(&self, index: usize) -> usize {
        self.nodes
            .get(index)
            .map(|n| n.column.unwrap_or(n.col))
            .unwrap_or(0)
    }

    /// Total flow through a node (max of in/out sums).
    ///
    /// ```
    /// use martensite::widgets::sankey::Sankey;
    ///
    /// let s = Sankey::new()
    ///     .node("a").node("b")
    ///     .link("a", "b", 2.0).link("a", "b", 3.0);
    /// assert_eq!(s.throughput(0), 5.0);
    /// ```
    pub fn throughput(&self, index: usize) -> f32 {
        let out: f32 = self
            .links
            .iter()
            .filter(|l| l.from == index)
            .map(|l| l.value)
            .sum();
        let inn: f32 = self
            .links
            .iter()
            .filter(|l| l.to == index)
            .map(|l| l.value)
            .sum();
        out.max(inn)
    }

    /// Drains the last hovered link index.
    ///
    /// ```
    /// use martensite::widgets::sankey::Sankey;
    ///
    /// assert_eq!(Sankey::new().take_hovered(), None);
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// Resolve columns (longest-path layering unless hinted) and
    /// vertical positions; called from `layout`.
    fn compute_layout(&mut self) {
        // Longest-path layering: col(node) = hint.unwrap_or(max(col(pred))+1).
        // Iterate links in declaration order a few passes — fine for
        // small DAGs; cycles leave unpinned nodes at column 0.
        for _ in 0..self.nodes.len() {
            for l in &self.links {
                if self.nodes[l.to].column.is_none() {
                    let c = self.nodes[l.from].col + 1;
                    if c > self.nodes[l.to].col {
                        self.nodes[l.to].col = c;
                    }
                }
            }
        }
        for n in &mut self.nodes {
            if let Some(c) = n.column {
                n.col = c;
            }
        }
        let max_col = self.nodes.iter().map(|n| n.col).max().unwrap_or(0);
        let pad = PAD_PT * self.scale;
        let v_gap = V_GAP_PT * self.scale;
        let inner_h = (self.bounds.height() - 2.0 * pad).max(0.0);
        for col in 0..=max_col {
            let idx: Vec<usize> = (0..self.nodes.len())
                .filter(|&i| self.nodes[i].col == col)
                .collect();
            let total: f32 = idx.iter().map(|&i| self.throughput(i)).sum();
            let gaps = v_gap * idx.len().saturating_sub(1) as f32;
            let avail = (inner_h - gaps).max(0.0);
            let mut y = self.bounds.min_y() + pad;
            for &i in &idx {
                let t = self.throughput(i);
                self.nodes[i].h = if total > 0.0 { t / total * avail } else { 0.0 };
                self.nodes[i].y = y;
                y += self.nodes[i].h + v_gap;
            }
        }
        // Cache link endpoints for hit testing: out/in offsets
        // accumulate per node so ribbons stack within each node.
        let mut out_off = vec![0.0f32; self.nodes.len()];
        let mut in_off = vec![0.0f32; self.nodes.len()];
        let col_w = self.column_width();
        let through: Vec<f32> = (0..self.nodes.len()).map(|i| self.throughput(i)).collect();
        for l in &mut self.links {
            let (fy, fh) = (self.nodes[l.from].y, self.nodes[l.from].h);
            let (ty, th) = (self.nodes[l.to].y, self.nodes[l.to].h);
            let ft = through[l.from].max(f32::EPSILON);
            let tt = through[l.to].max(f32::EPSILON);
            let scale_f = fh / ft;
            let scale_t = th / tt;
            let band = l.value;
            let x0 = self.bounds.min_x()
                + PAD_PT * self.scale
                + self.nodes[l.from].col as f32 * col_w
                + NODE_PT * self.scale;
            let x1 =
                self.bounds.min_x() + PAD_PT * self.scale + self.nodes[l.to].col as f32 * col_w;
            let y0 = fy + out_off[l.from] * scale_f;
            let y1 = ty + in_off[l.to] * scale_t;
            let h0 = band * scale_f;
            let h1 = band * scale_t;
            l.mid = vec![
                (Vec2::new(x0, y0), Vec2::new(x0, y0 + h0)),
                (Vec2::new(x1, y1), Vec2::new(x1, y1 + h1)),
            ];
            out_off[l.from] += band;
            in_off[l.to] += band;
        }
    }

    /// Horizontal pitch between column origins.
    fn column_width(&self) -> f32 {
        let max_col = self.nodes.iter().map(|n| n.col).max().unwrap_or(0);
        let pad = PAD_PT * self.scale;
        let node_w = NODE_PT * self.scale;
        let usable = (self.bounds.width() - 2.0 * pad - node_w).max(1.0);
        if max_col == 0 {
            0.0
        } else {
            usable / max_col as f32
        }
    }

    /// X origin of a node's column strip.
    fn node_x(&self, col: usize) -> f32 {
        self.bounds.min_x() + PAD_PT * self.scale + col as f32 * self.column_width()
    }

    /// Ribbon path for a laid-out link.
    fn link_path(&self, l: &Link) -> kurbo::BezPath {
        let mut path = kurbo::BezPath::new();
        if l.mid.len() < 2 {
            return path;
        }
        let (a0, a1) = l.mid[0];
        let (b0, b1) = l.mid[1];
        let p = |v: Vec2| (f64::from(v.x), f64::from(v.y));
        let mid_x = f64::from((a0.x + b0.x) / 2.0);
        path.move_to(p(a0));
        path.curve_to((mid_x, f64::from(a0.y)), (mid_x, f64::from(b0.y)), p(b0));
        path.line_to(p(b1));
        path.curve_to((mid_x, f64::from(b1.y)), (mid_x, f64::from(a1.y)), p(a1));
        path.close_path();
        path
    }

    /// Link index under a point (approximated by a thick segment
    /// between the ribbon's endpoint centers).
    fn link_at(&self, p: Vec2) -> Option<usize> {
        for (i, l) in self.links.iter().enumerate().rev() {
            if l.mid.len() < 2 {
                continue;
            }
            let (a0, a1) = l.mid[0];
            let (b0, b1) = l.mid[1];
            let ca = Vec2::new(a0.x, (a0.y + a1.y) / 2.0);
            let cb = Vec2::new(b0.x, (b0.y + b1.y) / 2.0);
            let band = ((a1.y - a0.y) + (b1.y - b0.y)) / 4.0 + 2.0 * self.scale;
            if dist_to_segment(p, ca, cb) <= band {
                return Some(i);
            }
        }
        None
    }
}

/// Distance from `p` to segment `a–b`.
fn dist_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(f32::EPSILON)).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

impl Widget for Sankey {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.compute_layout();
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!(
            "{} — {} nodes, {} links",
            self.label,
            self.nodes.len(),
            self.links.len()
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = if self.bounds.contains(*position) {
                    self.link_at(*position)
                } else {
                    None
                };
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
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        // Links first so nodes overlay the ribbon ends.
        for (i, l) in self.links.iter().enumerate() {
            let base = PALETTE[l.from % PALETTE.len()];
            let alpha = if self.hovered == Some(i) { 200 } else { 110 };
            cx.list.push_path(self.link_path(l), dim(base, alpha));
        }
        let node_w = NODE_PT * self.scale;
        for (i, n) in self.nodes.iter().enumerate() {
            if n.h <= 0.0 {
                continue;
            }
            let r = Rect::new(self.node_x(n.col), n.y, node_w, n.h);
            cx.list.push_fill_shape(
                krect(r),
                &martensite_core::shape::Shape::RECT,
                PALETTE[i % PALETTE.len()],
            );
            cx.list.push_stroke_shape(
                krect(r),
                &martensite_core::shape::Shape::RECT,
                cx.pt(0.75),
                edge,
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

    fn laid_out(s: &mut Sankey, w: f32, h: f32) {
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
    fn builds_and_layers() {
        let mut s = Sankey::new()
            .node("src")
            .node("mid")
            .node("sink")
            .link("src", "mid", 3.0)
            .link("mid", "sink", 3.0);
        laid_out(&mut s, 320.0, 160.0);
        assert_eq!(s.column_of(0), 0);
        assert_eq!(s.column_of(1), 1);
        assert_eq!(s.column_of(2), 2);
    }

    #[test]
    fn unknown_endpoints_ignored() {
        let s = Sankey::new()
            .node("a")
            .link("a", "ghost", 1.0)
            .link("a", "a", 1.0);
        assert_eq!(s.link_count(), 0);
    }

    #[test]
    fn throughput_max_of_in_out() {
        let s = Sankey::new()
            .node("a")
            .node("b")
            .node("c")
            .link("a", "b", 2.0)
            .link("a", "b", 3.0)
            .link("c", "b", 1.0);
        assert_eq!(s.throughput(0), 5.0);
        assert_eq!(s.throughput(1), 6.0);
    }

    #[test]
    fn hover_ribbon_parks_index() {
        let mut s = Sankey::new().node("a").node("b").link("a", "b", 4.0);
        laid_out(&mut s, 320.0, 160.0);
        let l = &s.links[0];
        let mid_x = (l.mid[0].0.x + l.mid[1].0.x) / 2.0;
        let mid_y = (l.mid[0].0.y + l.mid[1].0.y) / 2.0;
        s.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(mid_x, mid_y),
            },
            bounds: s.bounds,
            scale: 1.0,
        });
        assert_eq!(s.take_hovered(), Some(0));
    }

    #[test]
    fn smoke() {
        let mut s = Sankey::new()
            .node("gen")
            .node("grid")
            .node("use")
            .link("gen", "grid", 8.0)
            .link("grid", "use", 6.0)
            .label("Power");
        laid_out(&mut s, 400.0, 200.0);
        assert_eq!(s.node_count(), 3);
        assert_eq!(s.link_count(), 2);
    }
}
