//! `Quadrant` — a 2×2 priority matrix (Eisenhower / Gartner
//! magic-quadrant idiom): perpendicular axes with named ends,
//! four region labels, and plotted [`QuadrantItem`] points.
//!
//! Points use normalized coordinates in `-1.0..=1.0` on both
//! axes. Clicking a point parks its index in
//! [`Quadrant::take_selected`]; hovering highlights it.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::quadrant::{Quadrant, QuadrantItem};
//!
//! let q = Quadrant::new("Urgent", "Important")
//!     .item(QuadrantItem::new("Fix crash", 0.8, 0.9));
//! assert_eq!(q.item_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 28.0;
const DOT_PT: f32 = 10.0;
const FONT_PT: f32 = 10.5;

const FACE: [u8; 4] = [30, 32, 40, 255];
const AXIS: [u8; 4] = [140, 146, 158, 255];
const REGION: [u8; 4] = [255, 255, 255, 10];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];

/// One plotted point.
///
/// ```
/// use martensite::widgets::quadrant::QuadrantItem;
///
/// let i = QuadrantItem::new("Task", 0.5, -0.5);
/// assert_eq!(i.label, "Task");
/// ```
#[derive(Clone, Debug)]
pub struct QuadrantItem {
    /// Point label.
    pub label: String,
    /// Horizontal position, -1 (left) .. 1 (right).
    pub x: f32,
    /// Vertical position, -1 (bottom) .. 1 (top).
    pub y: f32,
    /// Dot color.
    pub color: [u8; 4],
}

impl QuadrantItem {
    /// A point at normalized `(x, y)`.
    ///
    /// ```
    /// use martensite::widgets::quadrant::QuadrantItem;
    ///
    /// assert_eq!(QuadrantItem::new("A", 0.0, 0.0).x, 0.0);
    /// ```
    pub fn new(label: impl Into<String>, x: f32, y: f32) -> Self {
        Self {
            label: label.into(),
            x: x.clamp(-1.0, 1.0),
            y: y.clamp(-1.0, 1.0),
            color: [90, 140, 220, 255],
        }
    }

    /// Dot color.
    ///
    /// ```
    /// use martensite::widgets::quadrant::QuadrantItem;
    ///
    /// assert_eq!(QuadrantItem::new("A", 0.0, 0.0).color([1; 4]).color, [1; 4]);
    /// ```
    pub fn color(mut self, color: [u8; 4]) -> Self {
        self.color = color;
        self
    }
}

/// The matrix — see the module docs.
///
/// ```
/// use martensite::widgets::quadrant::Quadrant;
///
/// assert_eq!(Quadrant::new("X", "Y").item_count(), 0);
/// ```
pub struct Quadrant {
    /// Accessibility label.
    pub label: String,
    /// Axis end names: `(x_axis, y_axis)`.
    pub axes: (String, String),
    /// Region captions `[top-left, top-right, bottom-left, bottom-right]`.
    pub region_labels: [String; 4],
    items: Vec<QuadrantItem>,
    selected: Option<usize>,
    hovered: Option<usize>,
    dots: Vec<(Vec2, f32)>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Quadrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Quadrant")
            .field("items", &self.items.len())
            .finish()
    }
}

impl Quadrant {
    /// A matrix with named axes.
    ///
    /// ```
    /// use martensite::widgets::quadrant::Quadrant;
    ///
    /// assert_eq!(Quadrant::new("X", "Y").item_count(), 0);
    /// ```
    pub fn new(x_axis: impl Into<String>, y_axis: impl Into<String>) -> Self {
        Self {
            label: "Matrix".to_string(),
            axes: (x_axis.into(), y_axis.into()),
            region_labels: [String::new(), String::new(), String::new(), String::new()],
            items: Vec::new(),
            selected: None,
            hovered: None,
            dots: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a point.
    ///
    /// ```
    /// use martensite::widgets::quadrant::{Quadrant, QuadrantItem};
    ///
    /// assert_eq!(Quadrant::new("X", "Y").item(QuadrantItem::new("A", 0.0, 0.0)).item_count(), 1);
    /// ```
    pub fn item(mut self, item: QuadrantItem) -> Self {
        self.items.push(item);
        self
    }

    /// Region captions.
    ///
    /// ```
    /// use martensite::widgets::quadrant::Quadrant;
    ///
    /// let q = Quadrant::new("X", "Y").regions(["a", "b", "c", "d"]);
    /// assert_eq!(q.region_labels[1], "b");
    /// ```
    pub fn regions(mut self, labels: [&str; 4]) -> Self {
        self.region_labels = labels.map(str::to_string);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::quadrant::Quadrant;
    ///
    /// assert_eq!(Quadrant::new("X", "Y").label("Priorities").label, "Priorities");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::quadrant::Quadrant;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _q = Quadrant::new("X", "Y").with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Item count.
    ///
    /// ```
    /// use martensite::widgets::quadrant::Quadrant;
    ///
    /// assert_eq!(Quadrant::new("X", "Y").item_count(), 0);
    /// ```
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Which region a normalized point falls in:
    /// `0` top-left, `1` top-right, `2` bottom-left, `3` bottom-right.
    ///
    /// ```
    /// use martensite::widgets::quadrant::Quadrant;
    ///
    /// assert_eq!(Quadrant::region_of(-0.5, 0.5), 0);
    /// assert_eq!(Quadrant::region_of(0.5, -0.5), 3);
    /// ```
    pub fn region_of(x: f32, y: f32) -> usize {
        match (x >= 0.0, y >= 0.0) {
            (false, true) => 0,
            (true, true) => 1,
            (false, false) => 2,
            (true, false) => 3,
        }
    }

    /// Drains the last clicked item index.
    ///
    /// ```
    /// use martensite::widgets::quadrant::Quadrant;
    ///
    /// let mut q = Quadrant::new("X", "Y");
    /// assert_eq!(q.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.selected.take()
    }

    fn hit(&self, p: Vec2) -> Option<usize> {
        self.dots
            .iter()
            .position(|(c, r)| (c.x - p.x).abs() <= *r && (c.y - p.y).abs() <= *r)
    }
}

fn line_path(a: Vec2, b: Vec2) -> kurbo::BezPath {
    let mut p = kurbo::BezPath::new();
    p.move_to((f64::from(a.x), f64::from(a.y)));
    p.line_to((f64::from(b.x), f64::from(b.y)));
    p
}

impl Widget for Quadrant {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let side = 260.0 * s;
        Vec2::new(
            side.min(constraints.max_size.x.max(0.0)),
            side.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(140.0, 140.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let pad = PAD_PT * s;
        let plot = Rect::new(
            bounds.min_x() + pad,
            bounds.min_y() + pad,
            (bounds.width() - pad * 2.0).max(0.0),
            (bounds.height() - pad * 2.0).max(0.0),
        );
        self.dots.clear();
        for it in &self.items {
            self.dots.push((
                Vec2::new(
                    plot.min_x() + plot.width() * (it.x + 1.0) / 2.0,
                    plot.min_y() + plot.height() * (1.0 - it.y) / 2.0,
                ),
                DOT_PT * s,
            ));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
        node.set_value(format!("{} items", self.items.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.hit(*position);
                if h != self.hovered {
                    self.hovered = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(i) = self.hit(*position) {
                    self.selected = Some(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
        let pad = PAD_PT * s;
        let plot = Rect::new(
            b.min_x() + pad,
            b.min_y() + pad,
            (b.width() - pad * 2.0).max(0.0),
            (b.height() - pad * 2.0).max(0.0),
        );
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            cx.color(TokenKey::BackgroundColor, FACE),
        );
        // Regions.
        let mid = Vec2::new(
            plot.min_x() + plot.width() / 2.0,
            plot.min_y() + plot.height() / 2.0,
        );
        for (rx, ry) in [(0.0, 0.0), (0.5, 0.0), (0.0, 0.5), (0.5, 0.5)] {
            let r = kurbo::Rect::new(
                f64::from(plot.min_x() + plot.width() * rx),
                f64::from(plot.min_y() + plot.height() * ry),
                f64::from(plot.min_x() + plot.width() * (rx + 0.5)),
                f64::from(plot.min_y() + plot.height() * (ry + 0.5)),
            );
            cx.list.push_fill_rect(r, REGION);
        }
        // Axes.
        cx.list.push_stroke_path(
            line_path(
                Vec2::new(plot.min_x(), mid.y),
                Vec2::new(plot.max_x(), mid.y),
            ),
            1.0,
            AXIS,
        );
        cx.list.push_stroke_path(
            line_path(
                Vec2::new(mid.x, plot.min_y()),
                Vec2::new(mid.x, plot.max_y()),
            ),
            1.0,
            AXIS,
        );
        // Region captions.
        let fs = FONT_PT * s;
        let region_origin = [
            Vec2::new(plot.min_x() + 4.0 * s, plot.min_y() + 12.0 * s),
            Vec2::new(
                plot.min_x() + plot.width() * 0.5 + 4.0 * s,
                plot.min_y() + 12.0 * s,
            ),
            Vec2::new(plot.min_x() + 4.0 * s, mid.y + 12.0 * s),
            Vec2::new(
                plot.min_x() + plot.width() * 0.5 + 4.0 * s,
                mid.y + 12.0 * s,
            ),
        ];
        for (label, o) in self.region_labels.iter().zip(region_origin.iter()) {
            if !label.is_empty() {
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(f64::from(o.x), f64::from(o.y)),
                    label,
                    fs,
                    MUTED_FG,
                );
            }
        }
        // Axis names.
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(f64::from(mid.x + 4.0 * s), f64::from(b.max_y() - 6.0 * s)),
            &self.axes.0,
            fs,
            MUTED_FG,
        );
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(b.min_x() + 2.0 * s),
                f64::from(plot.min_y() - 4.0 * s),
            ),
            &self.axes.1,
            fs,
            MUTED_FG,
        );
        // Dots + labels.
        for (i, it) in self.items.iter().enumerate() {
            let (c, r) = self.dots[i];
            let color = if self.hovered == Some(i) || self.selected == Some(i) {
                cx.color(TokenKey::AccentColor, [120, 170, 250, 255])
            } else {
                it.color
            };
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(c.x - r / 2.0),
                    f64::from(c.y - r / 2.0),
                    f64::from(c.x + r / 2.0),
                    f64::from(c.y + r / 2.0),
                ),
                &martensite_core::shape::Shape::ELLIPSE,
                color,
            );
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(c.x + r * 0.7), f64::from(c.y + 3.0 * s)),
                &it.label,
                fs,
                cx.color(TokenKey::TextColor, TEXT),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> Quadrant {
        Quadrant::new("Urgent", "Important")
            .regions(["Plan", "Do", "Drop", "Delegate"])
            .item(QuadrantItem::new("Fix crash", 0.8, 0.9))
            .item(QuadrantItem::new("Scroll feed", -0.6, -0.8))
    }

    fn laid_out(q: &mut Quadrant) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        q.layout(&mut cx, Rect::new(0.0, 0.0, 280.0, 280.0));
    }

    #[test]
    fn regions_classify() {
        assert_eq!(Quadrant::region_of(-0.5, 0.5), 0);
        assert_eq!(Quadrant::region_of(0.5, 0.5), 1);
        assert_eq!(Quadrant::region_of(-0.5, -0.5), 2);
        assert_eq!(Quadrant::region_of(0.5, -0.5), 3);
    }

    #[test]
    fn click_selects_dot() {
        let mut q = fixture();
        laid_out(&mut q);
        let (c, _) = q.dots[0];
        q.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: c,
            },
            bounds: q.bounds,
            scale: 1.0,
        });
        assert_eq!(q.take_selected(), Some(0));
    }

    #[test]
    fn coords_clamp() {
        let i = QuadrantItem::new("X", 5.0, -9.0);
        assert_eq!((i.x, i.y), (1.0, -1.0));
    }

    #[test]
    fn paint_without_painter() {
        let mut q = fixture();
        laid_out(&mut q);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        q.paint(&mut PaintContext {
            list: &mut list,
            bounds: q.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
