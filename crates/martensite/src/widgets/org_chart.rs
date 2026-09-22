//! `OrgChart` — a top-down hierarchy diagram (the classic
//! organization chart; the vertical sibling of
//! [`crate::widgets::mind_map::MindMap`]'s radial layout).
//!
//! Nodes are `(title, subtitle)` cards; children stack in
//! columns under their parent with elbow connectors. The chart
//! centers the root over its subtree, distributes siblings on
//! even spacing, and hovering a card parks its index in
//! [`OrgChart::take_hovered`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::org_chart::{OrgChart, OrgNode};
//!
//! let o = OrgChart::new(OrgNode::new("CEO", "Exec")
//!     .child(OrgNode::new("CTO", "Eng"))
//!     .child(OrgNode::new("CFO", "Fin")));
//! assert_eq!(o.node_count(), 3);
//! assert_eq!(o.root().title, "CEO");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const W_PT: f32 = 400.0;
const H_PT: f32 = 280.0;
const CARD_W_PT: f32 = 110.0;
const CARD_H_PT: f32 = 40.0;
const GAP_X_PT: f32 = 16.0;
const GAP_Y_PT: f32 = 36.0;

const FACE: [u8; 4] = [30, 30, 34, 255];
const CARD: [u8; 4] = [48, 48, 54, 255];
const CARD_HI: [u8; 4] = [58, 58, 66, 255];
const WIRE: [u8; 4] = [90, 90, 98, 255];
const TITLE: [u8; 4] = [218, 218, 224, 255];
const SUB: [u8; 4] = [145, 145, 155, 255];

/// One card in the hierarchy — see [`OrgChart`].
///
/// ```
/// use martensite::widgets::org_chart::OrgNode;
///
/// let n = OrgNode::new("Lead", "Platform");
/// assert!(n.children.is_empty());
/// ```
#[derive(Debug, Clone)]
pub struct OrgNode {
    /// Primary name/role line.
    pub title: String,
    /// Secondary line (department, email, headcount…).
    pub subtitle: String,
    /// Direct reports.
    pub children: Vec<OrgNode>,
}

impl OrgNode {
    /// A leaf card.
    ///
    /// ```
    /// use martensite::widgets::org_chart::OrgNode;
    ///
    /// assert_eq!(OrgNode::new("A", "B").title, "A");
    /// ```
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: subtitle.into(),
            children: Vec::new(),
        }
    }

    /// Appends a direct report.
    ///
    /// ```
    /// use martensite::widgets::org_chart::OrgNode;
    ///
    /// let n = OrgNode::new("CEO", "").child(OrgNode::new("CTO", ""));
    /// assert_eq!(n.children.len(), 1);
    /// ```
    pub fn child(mut self, child: OrgNode) -> Self {
        self.children.push(child);
        self
    }

    /// Total node count including self.
    fn count(&self) -> usize {
        1 + self.children.iter().map(OrgNode::count).sum::<usize>()
    }
}

/// A top-down hierarchy diagram — see the module docs.
///
/// ```
/// use martensite::widgets::org_chart::{OrgChart, OrgNode};
///
/// assert_eq!(OrgChart::new(OrgNode::new("R", "")).node_count(), 1);
/// ```
pub struct OrgChart {
    /// Accessibility label.
    pub label: String,
    /// The hierarchy root.
    pub root: OrgNode,
    /// Card rects in DFS order, assigned by `layout`.
    rects: Vec<Rect>,
    hover: Option<usize>,
    hovered: Option<usize>,
    bounds: Rect,
    scale: f32,
    /// Fit factor applied by `layout` when the natural tree width
    /// exceeds the allocation — cards, gaps, wires, and label sizes
    /// all scale by it so the chart stays coherent.
    fit: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for OrgChart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrgChart")
            .field("label", &self.label)
            .field("nodes", &self.node_count())
            .finish()
    }
}

impl OrgChart {
    /// Creates a chart over `root`.
    ///
    /// ```
    /// use martensite::widgets::org_chart::{OrgChart, OrgNode};
    ///
    /// let o = OrgChart::new(OrgNode::new("R", "root"));
    /// assert_eq!(o.root().subtitle, "root");
    /// ```
    pub fn new(root: OrgNode) -> Self {
        Self {
            label: "Organization".to_string(),
            root,
            rects: Vec::new(),
            hover: None,
            hovered: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            fit: 1.0,
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::org_chart::{OrgChart, OrgNode};
    ///
    /// let o = OrgChart::new(OrgNode::new("R", "")).label("Team");
    /// assert_eq!(o.label, "Team");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::org_chart::{OrgChart, OrgNode};
    ///
    /// let _ = OrgChart::new(OrgNode::new("R", "")); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// The hierarchy root.
    ///
    /// ```
    /// use martensite::widgets::org_chart::{OrgChart, OrgNode};
    ///
    /// assert_eq!(OrgChart::new(OrgNode::new("R", "")).root().title, "R");
    /// ```
    pub fn root(&self) -> &OrgNode {
        &self.root
    }

    /// Total card count.
    ///
    /// ```
    /// use martensite::widgets::org_chart::{OrgChart, OrgNode};
    ///
    /// let o = OrgChart::new(OrgNode::new("R", "").child(OrgNode::new("C", "")));
    /// assert_eq!(o.node_count(), 2);
    /// ```
    pub fn node_count(&self) -> usize {
        self.root.count()
    }

    /// Node `i` in DFS order (0 = root).
    ///
    /// ```
    /// use martensite::widgets::org_chart::{OrgChart, OrgNode};
    ///
    /// let o = OrgChart::new(OrgNode::new("R", "").child(OrgNode::new("C", "")));
    /// assert_eq!(o.node(1).unwrap().title, "C");
    /// ```
    pub fn node(&self, i: usize) -> Option<&OrgNode> {
        let mut out = Vec::new();
        Self::dfs(&self.root, &mut out);
        out.get(i).copied()
    }

    fn dfs<'a>(n: &'a OrgNode, out: &mut Vec<&'a OrgNode>) {
        out.push(n);
        for c in &n.children {
            Self::dfs(c, out);
        }
    }

    /// Card rect for node `i` (DFS order); assigned by `layout`.
    ///
    /// ```
    /// use martensite::widgets::org_chart::{OrgChart, OrgNode};
    ///
    /// assert!(OrgChart::new(OrgNode::new("R", "")).rect_of(0).is_none());
    /// ```
    pub fn rect_of(&self, i: usize) -> Option<Rect> {
        self.rects.get(i).copied()
    }

    /// Drains the last hovered card index.
    ///
    /// ```
    /// use martensite::widgets::org_chart::{OrgChart, OrgNode};
    ///
    /// assert_eq!(OrgChart::new(OrgNode::new("R", "")).take_hovered(), None);
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.hovered.take()
    }

    /// DFS index of the card containing `p`.
    fn hit(&self, p: Vec2) -> Option<usize> {
        self.rects.iter().position(|r| r.contains(p))
    }
}

/// Returns subtree width in "card units" (a leaf = 1).
fn units(n: &OrgNode) -> f32 {
    if n.children.is_empty() {
        1.0
    } else {
        n.children.iter().map(units).sum()
    }
}

/// Card/grid metrics for the layout pass.
struct Grid {
    unit_w: f32,
    card_w: f32,
    card_h: f32,
    gap_y: f32,
    top: f32,
}

/// Assigns card centers in DFS order, top-down: children span
/// `x..x+width` centered under the parent.
fn place(n: &OrgNode, x: f32, depth: usize, g: &Grid, rects: &mut Vec<Rect>) {
    let w = units(n) * g.unit_w;
    let cx = x + w / 2.0;
    let y = g.top + depth as f32 * (g.card_h + g.gap_y);
    rects.push(Rect::new(cx - g.card_w / 2.0, y, g.card_w, g.card_h));
    let mut cx0 = x;
    for c in &n.children {
        let cw = units(c) * g.unit_w;
        place(c, cx0, depth + 1, g, rects);
        cx0 += cw;
    }
}

impl Widget for OrgChart {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 100.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.rects.clear();
        let s = cx.scale;
        // Fit the natural tree width into the allocation: shrink cards
        // and gaps proportionally, but never below half size — below
        // that labels are illegible anyway and honest clipping is
        // better than unreadably small cards.
        let natural_w = units(&self.root) * (CARD_W_PT + GAP_X_PT) - GAP_X_PT;
        let avail = bounds.width() / s - 16.0;
        let fit = (avail / natural_w).clamp(0.5, 1.0);
        self.fit = fit;
        let card_w = CARD_W_PT * s * fit;
        let card_h = CARD_H_PT * s * fit;
        let gap_x = GAP_X_PT * s * fit;
        let gap_y = GAP_Y_PT * s * fit;
        let unit_w = card_w + gap_x;
        let total = units(&self.root) * unit_w - gap_x;
        let x0 = bounds.min_x() + (bounds.width() - total).max(0.0) / 2.0;
        let g = Grid {
            unit_w,
            card_w,
            card_h,
            gap_y,
            top: bounds.min_y() + 8.0 * s,
        };
        place(&self.root, x0, 0, &g, &mut self.rects);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Tree);
        node.set_label(format!(
            "{} — {} reports under {}",
            self.label,
            self.node_count().saturating_sub(1),
            self.root.title
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
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
            &martensite_core::shape::Shape::RECT,
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        // Cards/wires can extend past the widget when the tree is wider
        // than the fit floor allows — clip everything to the widget so
        // nothing emits (or leaks) outside it.
        cx.list.push_clip(krect(self.bounds));
        // Elbow connectors: parent bottom → mid → child top.
        let wire = cx.color(TokenKey::BorderColor, WIRE);
        let thick = (self.scale * self.fit).max(0.75);
        let mut idx = 0usize;
        self.paint_wires(&self.root, &mut idx, cx, wire, thick);
        // Cards.
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        // Card labels never shrink below the 12pt floor — a card too
        // small to host them honestly clips (or drops the subtitle)
        // rather than render microtext.
        let title_sz = (11.0 * self.scale * self.fit).max(12.0 * self.scale);
        let sub_sz = 12.0 * self.scale;
        let mut i = 0usize;
        self.paint_cards(&self.root, &mut i, cx, painter, title_sz, sub_sz);
        cx.list.pop_clip();
    }
}

impl OrgChart {
    /// Draws elbow connectors to each child, advancing `idx`
    /// through DFS rect order.
    fn paint_wires(
        &self,
        n: &OrgNode,
        idx: &mut usize,
        cx: &mut PaintContext,
        wire: [u8; 4],
        thick: f32,
    ) {
        let here = *idx;
        *idx += 1;
        let pr = match self.rects.get(here) {
            Some(r) => *r,
            None => return,
        };
        let px = (pr.min_x() + pr.max_x()) / 2.0;
        let py = pr.max_y();
        let mid = py + (GAP_Y_PT * self.scale * self.fit) / 2.0;
        for c in &n.children {
            let child_idx = *idx;
            if let Some(cr) = self.rects.get(child_idx) {
                let cx0 = (cr.min_x() + cr.max_x()) / 2.0;
                let mut path = kurbo::BezPath::new();
                path.move_to((f64::from(px), f64::from(py)));
                path.line_to((f64::from(px), f64::from(mid)));
                path.line_to((f64::from(cx0), f64::from(mid)));
                path.line_to((f64::from(cx0), f64::from(cr.min_y())));
                cx.list.push_stroke_path(path, thick, wire);
            }
            self.paint_wires(c, idx, cx, wire, thick);
        }
    }

    /// Draws cards in DFS order.
    fn paint_cards(
        &self,
        n: &OrgNode,
        idx: &mut usize,
        cx: &mut PaintContext,
        painter: Option<&(dyn martensite_core::paint::TextShaper + Send + Sync)>,
        title_sz: f32,
        sub_sz: f32,
    ) {
        let i = *idx;
        *idx += 1;
        if let Some(r) = self.rects.get(i).copied() {
            let kr = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            let fill = if self.hover == Some(i) {
                CARD_HI
            } else {
                cx.color(TokenKey::BackgroundColor, CARD)
            };
            cx.list.push_fill_shape(
                kr,
                &martensite_core::shape::Shape::rounded(6.0 * self.scale),
                fill,
            );
            cx.list.push_stroke_shape(
                kr,
                &martensite_core::shape::Shape::rounded(6.0 * self.scale),
                self.scale.max(0.75),
                cx.color(TokenKey::BorderColor, WIRE),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kr,
                kurbo::Point::new(
                    f64::from(r.min_x() + 8.0 * self.scale),
                    f64::from(r.min_y() + 6.0 * self.scale),
                ),
                &n.title,
                title_sz,
                cx.color(TokenKey::TextColor, TITLE),
            );
            if !n.subtitle.is_empty()
                && r.height() > 6.0 * self.scale + title_sz * 1.4 + sub_sz * 1.3
            {
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    kr,
                    kurbo::Point::new(
                        f64::from(r.min_x() + 8.0 * self.scale),
                        f64::from(r.min_y() + 6.0 * self.scale + title_sz * 1.4),
                    ),
                    &n.subtitle,
                    sub_sz,
                    cx.color(TokenKey::TextMutedColor, SUB),
                );
            }
        }
        for c in &n.children {
            self.paint_cards(c, idx, cx, painter, title_sz, sub_sz);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn tree() -> OrgNode {
        OrgNode::new("CEO", "Exec")
            .child(OrgNode::new("CTO", "Eng").child(OrgNode::new("Lead", "Platform")))
            .child(OrgNode::new("CFO", "Fin"))
    }

    fn laid_out(w: &mut OrgChart, wd: f32, h: f32) {
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
    fn counts_and_dfs() {
        let o = OrgChart::new(tree());
        assert_eq!(o.node_count(), 4);
        assert_eq!(o.node(1).unwrap().title, "CTO");
        assert_eq!(o.node(2).unwrap().title, "Lead");
        assert_eq!(o.node(3).unwrap().title, "CFO");
    }

    #[test]
    fn layout_assigns_rects() {
        let mut o = OrgChart::new(tree());
        laid_out(&mut o, 400.0, 280.0);
        let root = o.rect_of(0).unwrap();
        let lead = o.rect_of(2).unwrap();
        assert!(lead.min_y() > root.max_y()); // deeper = lower
                                              // Root centered over subtree span.
        let cto = o.rect_of(1).unwrap();
        let cfo = o.rect_of(3).unwrap();
        let span_mid = (cto.min_x() + cfo.max_x()) / 2.0;
        let root_mid = (root.min_x() + root.max_x()) / 2.0;
        assert!((span_mid - root_mid).abs() < 1.0);
    }

    #[test]
    fn hover_parks_index() {
        let mut o = OrgChart::new(tree());
        laid_out(&mut o, 400.0, 280.0);
        let r = o.rect_of(3).unwrap();
        o.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
            bounds: o.bounds,
            scale: 1.0,
        });
        assert_eq!(o.take_hovered(), Some(3));
    }
}
