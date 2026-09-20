//! `Ticket` — an event / boarding-pass card: a main face (title,
//! date, field grid like gate/seat/zone) separated from a stub by a
//! perforated edge with notches, plus a barcode strip.
//!
//! Display-only; the host composes fields via
//! [`Ticket::field`]. A tear-off affordance parks `true` in
//! [`Ticket::take_torn`] when the stub is clicked.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::ticket::Ticket;
//!
//! let t = Ticket::new("SFO → JFK").field("Seat", "12A");
//! assert_eq!(t.field_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 14.0;
const TITLE_PT: f32 = 15.0;
const FIELD_PT: f32 = 10.5;
const LABEL_PT: f32 = 8.5;
const STUB_PT: f32 = 56.0;
const COLS: usize = 3;

const FACE: [u8; 4] = [245, 246, 250, 255];
const EDGE: [u8; 4] = [90, 94, 104, 255];
const INK: [u8; 4] = [30, 32, 40, 255];
const MUTED_FG: [u8; 4] = [120, 124, 134, 255];

/// The ticket card — see the module docs.
///
/// ```
/// use martensite::widgets::ticket::Ticket;
///
/// assert_eq!(Ticket::new("Concert").title, "Concert");
/// ```
pub struct Ticket {
    /// Accessibility label.
    pub label: String,
    /// Headline (event name, route, …).
    pub title: String,
    /// Secondary caption under the title.
    pub caption: String,
    /// Barcode strip payload (encoded as hash marks).
    pub code: String,
    /// Whether the stub has been torn off.
    pub torn: bool,
    fields: Vec<(String, String)>,
    torn_flag: bool,
    stub_rect: Rect,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Ticket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ticket")
            .field("title", &self.title)
            .finish()
    }
}

impl Ticket {
    /// A ticket titled `title`.
    ///
    /// ```
    /// use martensite::widgets::ticket::Ticket;
    ///
    /// assert_eq!(Ticket::new("T").title, "T");
    /// ```
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            label: "Ticket".to_string(),
            title: title.into(),
            caption: String::new(),
            code: String::new(),
            torn: false,
            fields: Vec::new(),
            torn_flag: false,
            stub_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Secondary caption.
    ///
    /// ```
    /// use martensite::widgets::ticket::Ticket;
    ///
    /// assert_eq!(Ticket::new("T").caption("Sep 20").caption, "Sep 20");
    /// ```
    pub fn caption(mut self, caption: impl Into<String>) -> Self {
        self.caption = caption.into();
        self
    }

    /// `(label, value)` field in the grid.
    ///
    /// ```
    /// use martensite::widgets::ticket::Ticket;
    ///
    /// assert_eq!(Ticket::new("T").field("Gate", "B7").field_count(), 1);
    /// ```
    pub fn field(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.push((label.into(), value.into()));
        self
    }

    /// Barcode payload.
    ///
    /// ```
    /// use martensite::widgets::ticket::Ticket;
    ///
    /// assert_eq!(Ticket::new("T").code("ABC123").code, "ABC123");
    /// ```
    pub fn code(mut self, code: impl Into<String>) -> Self {
        self.code = code.into();
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::ticket::Ticket;
    ///
    /// assert_eq!(Ticket::new("T").label("Boarding pass").label, "Boarding pass");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::ticket::Ticket;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _t = Ticket::new("T").with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Field count.
    ///
    /// ```
    /// use martensite::widgets::ticket::Ticket;
    ///
    /// assert_eq!(Ticket::new("T").field_count(), 0);
    /// ```
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    /// A `(label, value)` field.
    ///
    /// ```
    /// use martensite::widgets::ticket::Ticket;
    ///
    /// let t = Ticket::new("T").field("Row", "12");
    /// assert_eq!(t.field_at(0), Some(("Row", "12")));
    /// ```
    pub fn field_at(&self, index: usize) -> Option<(&str, &str)> {
        self.fields
            .get(index)
            .map(|(l, v)| (l.as_str(), v.as_str()))
    }

    /// `true` once when the stub is torn (clicked).
    ///
    /// ```
    /// use martensite::widgets::ticket::Ticket;
    ///
    /// assert!(!Ticket::new("T").take_torn());
    /// ```
    pub fn take_torn(&mut self) -> bool {
        std::mem::take(&mut self.torn_flag)
    }
}

impl Widget for Ticket {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let rows = self.fields.len().div_ceil(COLS) as f32;
        let h = PAD_PT * 2.0 + TITLE_PT + 6.0 + rows * (LABEL_PT + FIELD_PT + 8.0) + STUB_PT;
        Vec2::new(
            (320.0 * s).min(constraints.max_size.x.max(0.0)),
            (h * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(200.0, 90.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        self.stub_rect = Rect::new(
            bounds.min_x(),
            bounds.max_y() - STUB_PT * s,
            bounds.width(),
            STUB_PT * s,
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!("{}: {}", self.label, self.title));
        node.set_value(format!("{} fields", self.fields.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerReleased {
            button: PointerButton::Primary,
            position,
        } = cx.event
        {
            if !self.torn && self.stub_rect.contains(*position) {
                self.torn = true;
                self.torn_flag = true;
                return EventResponse::RequestRepaint;
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
        let pad = PAD_PT * s;
        let stub_y = self.stub_rect.min_y();
        // Main face.
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(if self.torn { b.max_y() } else { stub_y }),
            ),
            &martensite_core::shape::Shape::rounded(8.0 * s),
            FACE,
        );
        // Title + caption.
        let tfs = TITLE_PT * s;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(f64::from(b.min_x() + pad), f64::from(b.min_y() + pad + tfs)),
            &self.title,
            tfs,
            INK,
        );
        if !self.caption.is_empty() {
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(b.min_x() + pad),
                    f64::from(b.min_y() + pad + tfs + LABEL_PT * s + 4.0 * s),
                ),
                &self.caption,
                LABEL_PT * s,
                MUTED_FG,
            );
        }
        // Field grid.
        let cols = COLS;
        let cw = (b.width() - pad * 2.0) / cols as f32;
        let mut fy = b.min_y() + pad + (TITLE_PT + 12.0) * s;
        for (i, (lab, val)) in self.fields.iter().enumerate() {
            let col = i % cols;
            if i > 0 && col == 0 {
                fy += (LABEL_PT + FIELD_PT + 8.0) * s;
            }
            let x = b.min_x() + pad + col as f32 * cw;
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(x), f64::from(fy + LABEL_PT * s)),
                &lab.to_uppercase(),
                LABEL_PT * s,
                MUTED_FG,
            );
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(x),
                    f64::from(fy + (LABEL_PT + 2.0) * s + FIELD_PT * s),
                ),
                val,
                FIELD_PT * s,
                INK,
            );
        }
        if !self.torn {
            // Perforated edge: notch circles + dashed line.
            let nr = 6.0 * s;
            for nx in [b.min_x(), b.max_x()] {
                cx.list.push_fill_shape(
                    kurbo::Rect::new(
                        f64::from(nx - nr),
                        f64::from(stub_y - nr),
                        f64::from(nx + nr),
                        f64::from(stub_y + nr),
                    ),
                    &martensite_core::shape::Shape::ELLIPSE,
                    cx.color(TokenKey::BackgroundColor, [20, 22, 28, 255]),
                );
            }
            let mut x = b.min_x() + nr + 4.0 * s;
            while x < b.max_x() - nr - 4.0 * s {
                cx.list.push_stroke_path(
                    line_path(Vec2::new(x, stub_y), Vec2::new(x + 6.0 * s, stub_y)),
                    1.0,
                    EDGE,
                );
                x += 12.0 * s;
            }
            // Stub: barcode strip.
            let mid = stub_y + self.stub_rect.height() / 2.0;
            let bh = self.stub_rect.height() * 0.5;
            let mut bx = b.min_x() + pad;
            let bytes = self.code.as_bytes();
            let mut i = 0usize;
            while bx < b.max_x() - pad {
                let w = if bytes.is_empty() {
                    (i % 3 + 1) as f32
                } else {
                    (bytes[i % bytes.len()] % 3 + 1) as f32
                } * s;
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(bx),
                        f64::from(mid - bh / 2.0),
                        f64::from(bx + w),
                        f64::from(mid + bh / 2.0),
                    ),
                    INK,
                );
                bx += w + 2.0 * s;
                i += 1;
            }
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(b.min_x() + pad), f64::from(stub_y + 12.0 * s)),
                "STUB",
                LABEL_PT * s,
                MUTED_FG,
            );
        }
    }
}

fn line_path(a: Vec2, b: Vec2) -> kurbo::BezPath {
    let mut p = kurbo::BezPath::new();
    p.move_to((f64::from(a.x), f64::from(a.y)));
    p.line_to((f64::from(b.x), f64::from(b.y)));
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(t: &mut Ticket) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        t.layout(&mut cx, Rect::new(0.0, 0.0, 320.0, 180.0));
    }

    #[test]
    fn stub_click_tears() {
        let mut t = Ticket::new("SFO → JFK").field("Seat", "12A");
        laid_out(&mut t);
        let r = t.stub_rect;
        t.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(r.min_x() + 8.0, r.min_y() + 8.0),
            },
            bounds: t.bounds,
            scale: 1.0,
        });
        assert!(t.torn);
        assert!(t.take_torn());
        assert!(!t.take_torn());
    }

    #[test]
    fn fields_pair() {
        let t = Ticket::new("T").field("Gate", "B7").field("Seat", "1A");
        assert_eq!(t.field_at(1), Some(("Seat", "1A")));
    }

    #[test]
    fn paint_without_painter() {
        let mut t = Ticket::new("Show").code("ABC").field("Row", "5");
        laid_out(&mut t);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        t.paint(&mut PaintContext {
            list: &mut list,
            bounds: t.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
