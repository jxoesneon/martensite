//! `Treemap` — an area-proportional rectangle chart (Ant `Treemap`,
//! d3 `treemap`, Qt `QTreemap` example).
//!
//! [`TreemapItem`]s slice the plot into rectangles whose areas match
//! their values, laid out with the squarified algorithm (rows fold
//! when a candidate worsens the worst aspect ratio). Items color
//! from the shared chart palette unless overridden; pointer hover
//! parks `(index, name)` in [`Treemap::take_hovered`] and item
//! labels paint when the rect can fit them.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::treemap::{Treemap, TreemapItem};
//!
//! let t = Treemap::new()
//!     .item(TreemapItem::new("src", 60.0))
//!     .item(TreemapItem::new("docs", 40.0));
//! assert_eq!(t.items.len(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const W_PT: f32 = 240.0;
const H_PT: f32 = 160.0;
const GAP_PT: f32 = 1.0;
const FONT_PT: f32 = 10.0;

const SURFACE: [u8; 4] = [250, 250, 252, 255];
const LABEL: [u8; 4] = [250, 250, 252, 255];
const PALETTE: [[u8; 4]; 6] = [
    [80, 140, 220, 255],
    [230, 120, 60, 255],
    [70, 170, 110, 255],
    [190, 90, 180, 255],
    [210, 170, 60, 255],
    [90, 90, 200, 255],
];

/// One weighted treemap cell.
///
/// ```
/// use martensite::widgets::treemap::TreemapItem;
///
/// let i = TreemapItem::new("src", 60.0);
/// assert_eq!(i.value, 60.0);
/// ```
pub struct TreemapItem {
    /// Cell label (paints when the rect fits it).
    pub name: String,
    /// Weight — cell area is `value / Σ values` of the plot.
    pub value: f32,
    /// Explicit color; `None` draws from the palette.
    pub color: Option<[u8; 4]>,
}

impl TreemapItem {
    /// Creates an item.
    ///
    /// ```
    /// use martensite::widgets::treemap::TreemapItem;
    ///
    /// let i = TreemapItem::new("a", 1.0);
    /// assert_eq!(i.name, "a");
    /// ```
    pub fn new(name: impl Into<String>, value: f32) -> Self {
        Self {
            name: name.into(),
            value: value.max(0.0),
            color: None,
        }
    }

    /// Overrides the palette color.
    ///
    /// ```
    /// use martensite::widgets::treemap::TreemapItem;
    ///
    /// let i = TreemapItem::new("a", 1.0).color([1, 2, 3, 255]);
    /// assert_eq!(i.color, Some([1, 2, 3, 255]));
    /// ```
    pub fn color(mut self, c: [u8; 4]) -> Self {
        self.color = Some(c);
        self
    }
}

/// A squarified treemap — see the module docs.
///
/// ```
/// use martensite::widgets::treemap::Treemap;
///
/// let t = Treemap::new();
/// assert!(t.items.is_empty());
/// ```
pub struct Treemap {
    /// Items in draw order.
    pub items: Vec<TreemapItem>,
    /// When `false` the chart is inert.
    pub enabled: bool,
    hovered: Option<usize>,
    hovered_out: Option<usize>,
    rects: Vec<Rect>,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
    scale: f32,
}

impl Default for Treemap {
    fn default() -> Self {
        Self::new()
    }
}

impl Treemap {
    /// Creates an empty treemap.
    ///
    /// ```
    /// use martensite::widgets::treemap::Treemap;
    ///
    /// assert!(Treemap::new().items.is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            enabled: true,
            hovered: None,
            hovered_out: None,
            rects: Vec::new(),
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            scale: 1.0,
        }
    }

    /// Appends an item.
    ///
    /// ```
    /// use martensite::widgets::treemap::{Treemap, TreemapItem};
    ///
    /// let t = Treemap::new().item(TreemapItem::new("a", 1.0));
    /// assert_eq!(t.items.len(), 1);
    /// ```
    pub fn item(mut self, item: TreemapItem) -> Self {
        self.items.push(item);
        self
    }

    /// Enables or disables the chart.
    ///
    /// ```
    /// use martensite::widgets::treemap::Treemap;
    ///
    /// let t = Treemap::new().enabled(false);
    /// assert!(!t.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::treemap::Treemap;
    ///
    /// let t = Treemap::new();
    /// let _ = t.enabled;
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Drains the hovered item index.
    ///
    /// ```
    /// use martensite::widgets::treemap::Treemap;
    ///
    /// let mut t = Treemap::new();
    /// assert_eq!(t.take_hovered(), None);
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.hovered_out.take()
    }

    /// Item `i`'s laid-out rect (after `layout`).
    ///
    /// ```
    /// use martensite::widgets::treemap::Treemap;
    ///
    /// assert!(Treemap::new().item_rect(0).is_none());
    /// ```
    pub fn item_rect(&self, i: usize) -> Option<Rect> {
        self.rects.get(i).copied()
    }

    /// Worst aspect ratio of a row laid along `side` in `len` width.
    fn worst(row: &[f32], side: f32, len: f32) -> f32 {
        let sum: f32 = row.iter().sum();
        if sum <= 0.0 || side <= 0.0 || len <= 0.0 {
            return f32::MAX;
        }
        let row_len = sum / side;
        row.iter()
            .map(|&v| {
                let w = v / row_len;
                (w.max(row_len) / w.min(row_len).max(f32::EPSILON))
                    .max(row_len / w.max(f32::EPSILON))
            })
            .fold(0.0, f32::max)
    }

    /// Squarified layout — fills `rect` in order, folding rows when a
    /// candidate worsens the worst aspect ratio.
    fn squarify(&mut self, mut rect: Rect) {
        let total: f32 = self.items.iter().map(|i| i.value).sum();
        if total <= 0.0 || rect.width() <= 0.0 || rect.height() <= 0.0 {
            self.rects.clear();
            return;
        }
        // Normalize values to the rect area.
        let scale = rect.width() * rect.height() / total;
        let values: Vec<f32> = self.items.iter().map(|i| i.value * scale).collect();
        let mut out: Vec<Rect> = Vec::with_capacity(self.items.len());
        let mut idx = 0usize;
        while idx < values.len() {
            // Grow a row while the worst aspect ratio improves.
            let side = rect.width().min(rect.height());
            let mut row = vec![values[idx]];
            idx += 1;
            while idx < values.len() {
                let mut cand = row.clone();
                cand.push(values[idx]);
                if Self::worst(&cand, side, rect.width().max(rect.height()))
                    <= Self::worst(&row, side, rect.width().max(rect.height()))
                {
                    row = cand;
                    idx += 1;
                } else {
                    break;
                }
            }
            // Lay the row along the short side.
            let sum: f32 = row.iter().sum();
            let horizontal = rect.width() >= rect.height(); // row across the top
            if horizontal {
                let row_h = sum / rect.width().max(f32::EPSILON);
                let mut x = rect.min_x();
                for &v in &row {
                    let w = v / row_h.max(f32::EPSILON);
                    out.push(Rect::new(x, rect.min_y(), w, row_h));
                    x += w;
                }
                rect = Rect::new(
                    rect.min_x(),
                    rect.min_y() + row_h,
                    rect.width(),
                    (rect.height() - row_h).max(0.0),
                );
            } else {
                let row_w = sum / rect.height().max(f32::EPSILON);
                let mut y = rect.min_y();
                for &v in &row {
                    let h = v / row_w.max(f32::EPSILON);
                    out.push(Rect::new(rect.min_x(), y, row_w, h));
                    y += h;
                }
                rect = Rect::new(
                    rect.min_x() + row_w,
                    rect.min_y(),
                    (rect.width() - row_w).max(0.0),
                    rect.height(),
                );
            }
        }
        // `out` is in row-fold order, which is still item order (rows
        // consume items left-to-right).
        self.rects = out;
    }

    fn palette(&self, i: usize) -> [u8; 4] {
        PALETTE[i % PALETTE.len()]
    }
}

impl Widget for Treemap {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(64.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.squarify(bounds);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label("Treemap");
        node.set_value(format!("{} items", self.items.len()));
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.rects.iter().position(|r| r.contains(*position));
                if h != self.hovered {
                    self.hovered = h;
                    self.hovered_out = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.is_some() {
                    self.hovered = None;
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
        cx.list.push_fill_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(0.0),
            cx.color(TokenKey::SurfaceColor, SURFACE),
        );
        let gap = cx.pt(GAP_PT);
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size = FONT_PT * cx.scale;
        for (i, item) in self.items.iter().enumerate() {
            let Some(r) = self.rects.get(i).copied() else {
                continue;
            };
            let inner = Rect::new(
                r.min_x() + gap / 2.0,
                r.min_y() + gap / 2.0,
                (r.width() - gap).max(0.0),
                (r.height() - gap).max(0.0),
            );
            if inner.width() <= 0.0 || inner.height() <= 0.0 {
                continue;
            }
            let mut color = item.color.unwrap_or_else(|| self.palette(i));
            if self.hovered == Some(i) {
                color = [
                    color[0].saturating_add(30),
                    color[1].saturating_add(30),
                    color[2].saturating_add(30),
                    255,
                ];
            }
            cx.list.push_fill_shape(
                f(inner),
                &martensite_core::shape::Shape::rounded(cx.pt(2.0)),
                color,
            );
            // Label when the cell fits it.
            let w = painter
                .and_then(|p| p.measure_text(&item.name, size))
                .unwrap_or(item.name.chars().count() as f32 * size * 0.55);
            if w <= inner.width() - gap * 2.0 && size <= inner.height() - gap * 2.0 {
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    f(inner),
                    kurbo::Point::new(
                        f64::from(inner.min_x() + (inner.width() - w) / 2.0),
                        f64::from(inner.min_y() + (inner.height() - size) / 2.0),
                    ),
                    &item.name,
                    size,
                    LABEL,
                );
            }
        }
    }
}

impl std::fmt::Debug for Treemap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Treemap")
            .field("items", &self.items.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(t: &mut Treemap, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        t.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        t.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn rects_cover_plot() {
        let mut t = Treemap::new()
            .item(TreemapItem::new("a", 50.0))
            .item(TreemapItem::new("b", 50.0));
        laid_out(&mut t, 200.0, 100.0);
        let total: f32 = t.rects.iter().map(|r| r.width() * r.height()).sum();
        assert!((total - 20_000.0).abs() < 1.0);
    }

    #[test]
    fn areas_proportional() {
        let mut t = Treemap::new()
            .item(TreemapItem::new("big", 75.0))
            .item(TreemapItem::new("small", 25.0));
        laid_out(&mut t, 200.0, 100.0);
        let a0 = t.rects[0].width() * t.rects[0].height();
        let a1 = t.rects[1].width() * t.rects[1].height();
        assert!((a0 / a1 - 3.0).abs() < 0.05);
    }

    #[test]
    fn rects_dont_overlap() {
        let mut t = Treemap::new()
            .item(TreemapItem::new("a", 40.0))
            .item(TreemapItem::new("b", 30.0))
            .item(TreemapItem::new("c", 20.0))
            .item(TreemapItem::new("d", 10.0));
        laid_out(&mut t, 200.0, 100.0);
        for i in 0..t.rects.len() {
            for j in i + 1..t.rects.len() {
                let (a, b) = (t.rects[i], t.rects[j]);
                let overlap = (a.max_x().min(b.max_x()) - a.min_x().max(b.min_x()))
                    * (a.max_y().min(b.max_y()) - a.min_y().max(b.min_y()));
                assert!(overlap <= 0.01, "rects {i} and {j} overlap");
            }
        }
    }

    #[test]
    fn hover_parks_index() {
        let mut t = Treemap::new().item(TreemapItem::new("a", 100.0));
        laid_out(&mut t, 200.0, 100.0);
        t.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(50.0, 50.0),
            },
            bounds: Rect::new(0.0, 0.0, 200.0, 100.0),
            scale: 1.0,
        });
        assert_eq!(t.take_hovered(), Some(0));
    }

    #[test]
    fn empty_and_zero_safe() {
        let mut t = Treemap::new();
        laid_out(&mut t, 200.0, 100.0);
        assert!(t.rects.is_empty());
        let mut zeros = Treemap::new().item(TreemapItem::new("z", 0.0));
        laid_out(&mut zeros, 200.0, 100.0);
        assert!(zeros.rects.is_empty());
    }
}
