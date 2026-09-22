//! Descriptions — a label:value grid for detail pages.
//!
//! Mirrors Ant Design `Descriptions` and Qt's form/detail view: a
//! titled grid of `label: content` cells, optionally bordered, with
//! per-item column spans. Display-only — editing belongs to the
//! `SettingsRow`/`SettingsGroup` family.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::Descriptions;
//!
//! let d = Descriptions::new()
//!     .title("Device")
//!     .item("Model", "MX-2000")
//!     .item("Firmware", "1.4.2");
//! assert_eq!(d.item_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

/// Cell padding in points.
const PAD_PT: f32 = 10.0;
/// Row height in points.
const ROW_PT: f32 = 32.0;
/// Title block height in points.
const TITLE_PT: f32 = 24.0;
/// Label share of a cell's width.
const LABEL_SHARE: f32 = 0.38;

/// One `label: content` cell.
///
/// # Examples
///
/// ```
/// use martensite::widgets::DescriptionItem;
///
/// let it = DescriptionItem::new("Model", "MX-2000").span(2);
/// assert_eq!(it.span, 2);
/// ```
pub struct DescriptionItem {
    /// The field's label.
    pub label: String,
    /// The field's content text.
    pub content: String,
    /// Columns spanned (clamped to the grid width at layout).
    pub span: usize,
    /// Cell bounds from the last layout pass — also the child's AT
    /// bounds.
    bounds: Rect,
}

impl DescriptionItem {
    /// Creates an item.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DescriptionItem;
    ///
    /// assert_eq!(DescriptionItem::new("a", "b").label, "a");
    /// ```
    pub fn new(label: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            content: content.into(),
            span: 1,
            bounds: Rect::default(),
        }
    }

    /// Sets the column span.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::DescriptionItem;
    ///
    /// assert_eq!(DescriptionItem::new("a", "b").span(3).span, 3);
    /// ```
    #[must_use]
    pub fn span(mut self, span: usize) -> Self {
        self.span = span.max(1);
        self
    }
}

impl Widget for DescriptionItem {
    fn measure(&mut self, cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(cx.pt(80.0), cx.pt(ROW_PT))
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListItem);
        node.set_label(format!("{}: {}", self.label, self.content));
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }
}

/// A detail-page description grid.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Descriptions;
///
/// let d = Descriptions::new().bordered(true).column_count(3);
/// assert!(d.bordered);
/// ```
pub struct Descriptions {
    /// Optional section title.
    pub title: Option<String>,
    /// Whether the widget is enabled.
    pub enabled: bool,
    /// Whether cells draw hairline borders.
    pub bordered: bool,
    /// The cells, row-major.
    items: Vec<DescriptionItem>,
    /// Grid columns (default 2).
    columns: usize,
    /// Cached bounds from the last layout pass.
    bounds: Rect,
    /// Shared shaped-text painter — see [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Descriptions {
    /// Creates an empty grid.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Descriptions;
    ///
    /// assert_eq!(Descriptions::new().item_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            title: None,
            enabled: true,
            bordered: false,
            items: Vec::new(),
            columns: 2,
            bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the section title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Descriptions;
    ///
    /// let d = Descriptions::new().title("Device");
    /// assert_eq!(d.title.as_deref(), Some("Device"));
    /// ```
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Appends a `label: content` cell.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Descriptions;
    ///
    /// let d = Descriptions::new().item("a", "b");
    /// assert_eq!(d.item_count(), 1);
    /// ```
    #[must_use]
    pub fn item(mut self, label: impl Into<String>, content: impl Into<String>) -> Self {
        self.items.push(DescriptionItem::new(label, content));
        self
    }

    /// Appends a pre-built item (for `span`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{DescriptionItem, Descriptions};
    ///
    /// let d = Descriptions::new().with_item(DescriptionItem::new("a", "b").span(2));
    /// assert_eq!(d.item_count(), 1);
    /// ```
    #[must_use]
    pub fn with_item(mut self, item: DescriptionItem) -> Self {
        self.items.push(item);
        self
    }

    /// Sets the grid column count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Descriptions;
    ///
    /// let d = Descriptions::new().column_count(4);
    /// assert_eq!(d.columns(), 4);
    /// ```
    #[must_use]
    pub fn column_count(mut self, columns: usize) -> Self {
        self.columns = columns.clamp(1, 6);
        self
    }

    /// Sets whether cells draw hairline borders.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Descriptions;
    ///
    /// let d = Descriptions::new().bordered(true);
    /// assert!(d.bordered);
    /// ```
    #[must_use]
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    /// Sets whether the widget is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Descriptions;
    ///
    /// let d = Descriptions::new().enabled(false);
    /// assert!(!d.enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so cells emit real
    /// glyph runs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Number of cells.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Descriptions;
    ///
    /// assert_eq!(Descriptions::new().item("a", "b").item_count(), 1);
    /// ```
    #[inline]
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// The grid column count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Descriptions;
    ///
    /// assert_eq!(Descriptions::new().columns(), 2);
    /// ```
    #[inline]
    pub fn columns(&self) -> usize {
        self.columns
    }

    /// Row-major placement as `(row, column, span)` per item. A row
    /// that fills exactly advances the cursor so the next item can't
    /// overlap a full-width cell.
    fn flow_cells(&self) -> Vec<(usize, usize, usize)> {
        let mut cells = Vec::with_capacity(self.items.len());
        let mut row = 0usize;
        let mut col = 0usize;
        for it in &self.items {
            let span = it.span.min(self.columns);
            if col + span > self.columns {
                row += 1;
                col = 0;
            }
            cells.push((row, col, span));
            col += span;
            if col >= self.columns {
                row += 1;
                col = 0;
            }
        }
        cells
    }

    /// Laid-out row count.
    fn flow(&self) -> usize {
        self.flow_cells().last().map_or(0, |(r, _, _)| r + 1)
    }
}

impl Default for Descriptions {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Descriptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Descriptions")
            .field("items", &self.items.len())
            .field("columns", &self.columns)
            .field("bordered", &self.bordered)
            .finish()
    }
}

impl Widget for Descriptions {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let title_h = if self.title.is_some() {
            cx.pt(TITLE_PT)
        } else {
            0.0
        };
        let h = title_h + self.flow() as f32 * cx.pt(ROW_PT);
        let max_w = constraints.max_size.x.max(0.0);
        Vec2::new(
            cx.pt(200.0).min(max_w).max(cx.pt(80.0).min(max_w)),
            h.max(cx.pt(ROW_PT)).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, ROW_PT)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        let title_h = if self.title.is_some() {
            cx.pt(TITLE_PT)
        } else {
            0.0
        };
        let row_h = cx.pt(ROW_PT);
        let cell_w = bounds.width() / self.columns as f32;
        let cells = self.flow_cells();
        for (it, (row, col, span)) in self.items.iter_mut().zip(cells) {
            let r = Rect::new(
                bounds.min_x() + col as f32 * cell_w,
                bounds.min_y() + title_h + row as f32 * row_h,
                cell_w * span as f32,
                row_h,
            );
            cx.layout_child(it, r);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        if let Some(ref t) = self.title {
            node.set_label(t.as_str());
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        // Rows can exceed a shallow allocation — clip to the widget so
        // cell text cuts at the edge instead of spilling.
        cx.list.push_clip(kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        ));
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let ink = cx.color(TokenKey::TextColor, [30, 30, 36, 255]);
        let muted = cx.color(TokenKey::TextMutedColor, [110, 110, 118, 255]);
        let border = cx.color(TokenKey::DividerColor, [210, 212, 218, 255]);
        let label_bg = cx.color(TokenKey::SurfaceColor, [244, 246, 250, 255]);
        let font_px = cx.pt(13.0);

        if let Some(ref t) = self.title {
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(b.min_x()),
                    f64::from(b.min_y()),
                    f64::from(b.max_x()),
                    f64::from(b.min_y() + cx.pt(TITLE_PT)),
                ),
                kurbo::Point::new(f64::from(b.min_x()), f64::from(b.min_y() + cx.pt(2.0))),
                t,
                cx.pt(15.0),
                ink,
            );
        }

        for it in &self.items {
            let r = it.bounds;
            if r.width() <= 0.0 {
                continue;
            }
            let label_w = (r.width() * LABEL_SHARE).min(cx.pt(140.0));
            let pad = cx.pt(PAD_PT);
            let cell_k = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            if self.bordered {
                // Bordered: label zone gets a muted fill, whole cell
                // gets a hairline stroke.
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(r.min_x()),
                        f64::from(r.min_y()),
                        f64::from(r.min_x() + label_w),
                        f64::from(r.max_y()),
                    ),
                    label_bg,
                );
                cx.list.push_stroke_rect(cell_k, cx.pt(1.0), border);
            }
            let label_clip = kurbo::Rect::new(
                f64::from(r.min_x() + pad),
                f64::from(r.min_y()),
                f64::from(r.min_x() + label_w - pad / 2.0),
                f64::from(r.max_y()),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                label_clip,
                kurbo::Point::new(
                    f64::from(r.min_x() + pad),
                    f64::from(r.min_y() + (r.height() - font_px) / 2.0),
                ),
                &it.label,
                font_px,
                muted,
            );
            let content_clip = kurbo::Rect::new(
                f64::from(r.min_x() + label_w + pad),
                f64::from(r.min_y()),
                f64::from(r.max_x() - pad / 2.0),
                f64::from(r.max_y()),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                content_clip,
                kurbo::Point::new(
                    f64::from(r.min_x() + label_w + pad),
                    f64::from(r.min_y() + (r.height() - font_px) / 2.0),
                ),
                &it.content,
                font_px,
                ink,
            );
        }
        cx.list.pop_clip();
    }

    fn child_count(&self) -> usize {
        self.items.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.items.get(index).map(|i| i as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.items.get_mut(index).map(|i| i as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.items.get(index).map(|i| i.bounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(d: &mut Descriptions, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        d.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    #[test]
    fn builder() {
        let d = Descriptions::new()
            .title("Device")
            .item("Model", "MX")
            .with_item(DescriptionItem::new("a", "b").span(2))
            .bordered(true)
            .column_count(3);
        assert_eq!(d.item_count(), 2);
        assert!(d.bordered);
        assert_eq!(d.columns(), 3);
    }

    #[test]
    fn flow_wraps_on_span_overflow() {
        let d = Descriptions::new()
            .column_count(2)
            .item("a", "1")
            .with_item(DescriptionItem::new("b", "2").span(2))
            .item("c", "3");
        // a | (span-2 wraps to next row) b b | c
        assert_eq!(d.flow(), 3);
    }

    #[test]
    fn layout_assigns_cell_bounds() {
        let mut d = Descriptions::new()
            .column_count(2)
            .item("a", "1")
            .item("b", "2");
        laid_out(&mut d, 400.0, 100.0);
        let a = d.child_bounds(0).unwrap();
        let b = d.child_bounds(1).unwrap();
        assert_eq!(a.min_x(), 0.0);
        assert_eq!(b.min_x(), 200.0);
        assert_eq!(a.width(), 200.0);
    }

    #[test]
    fn span_cells_widen() {
        let mut d = Descriptions::new()
            .column_count(2)
            .with_item(DescriptionItem::new("a", "1").span(2));
        laid_out(&mut d, 400.0, 100.0);
        assert_eq!(d.child_bounds(0).unwrap().width(), 400.0);
    }

    #[test]
    fn title_shifts_grid_down() {
        let mut d = Descriptions::new().title("T").item("a", "1");
        laid_out(&mut d, 400.0, 100.0);
        assert!(d.child_bounds(0).unwrap().min_y() > 0.0);
    }

    #[test]
    fn accessibility_list_with_items() {
        let d = Descriptions::new().title("Spec").item("k", "v");
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        d.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::List);
        let mut child = AccessKitNode::new(accesskit::Role::Unknown);
        d.child(0).unwrap().accessibility(&mut child);
        assert_eq!(child.role(), accesskit::Role::ListItem);
        assert!(child.label().unwrap_or_default().contains("k: v"));
    }

    #[test]
    fn disabled_is_inert() {
        let mut d = Descriptions::new().enabled(false);
        let ev = martensite_core::WidgetEvent::PointerPressed {
            position: Vec2::new(5.0, 5.0),
            button: martensite_core::PointerButton::Primary,
            count: 1,
        };
        let mut cx = EventContext {
            event: &ev,
            bounds: Rect::new(0.0, 0.0, 100.0, 60.0),
            scale: 1.0,
        };
        assert_eq!(d.event(&mut cx), EventResponse::Ignored);
    }
}
