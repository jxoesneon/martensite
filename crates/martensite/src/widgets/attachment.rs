//! `Attachment` — a file-attachment chip (email composer / chat
//! attach idiom): type glyph + file name + size, an optional upload
//! progress bar, and a × remove affordance.
//!
//! Clicking × parks `true` in [`Attachment::take_removed`]; the
//! upload fraction is display-driven via
//! [`Attachment::uploading`]. The host owns the file model.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::attachment::Attachment;
//!
//! let a = Attachment::new("report.pdf", 204_800);
//! assert_eq!(a.name, "report.pdf");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 8.0;
const ICON_PT: f32 = 22.0;
const GAP_PT: f32 = 8.0;
const NAME_PT: f32 = 11.5;
const SIZE_PT: f32 = 9.5;
const BAR_PT: f32 = 3.0;
const CLOSE_PT: f32 = 14.0;

const FACE: [u8; 4] = [40, 43, 52, 255];
const EDGE: [u8; 4] = [78, 82, 92, 255];
const TEXT_FG: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];
const ICON_BG: [u8; 4] = [60, 64, 76, 255];
const BAR_BG: [u8; 4] = [60, 63, 72, 255];

/// The attachment chip — see the module docs.
///
/// ```
/// use martensite::widgets::attachment::Attachment;
///
/// assert_eq!(Attachment::new("a.txt", 12).name, "a.txt");
/// ```
pub struct Attachment {
    /// Accessibility label.
    pub label: String,
    /// File name.
    pub name: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Type glyph (emoji or short text like "PDF").
    pub glyph: String,
    /// Upload progress `0.0..=1.0`; `None` hides the bar.
    pub upload: Option<f32>,
    removed: bool,
    close_rect: Rect,
    bar_rect: Rect,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Attachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attachment")
            .field("name", &self.name)
            .finish()
    }
}

fn human_size(bytes: u64) -> String {
    const K: u64 = 1024;
    if bytes >= K * K {
        format!("{:.1} MB", bytes as f64 / (K * K) as f64)
    } else if bytes >= K {
        format!("{:.0} KB", bytes as f64 / K as f64)
    } else {
        format!("{bytes} B")
    }
}

impl Attachment {
    /// A chip for `name` of `size_bytes`.
    ///
    /// ```
    /// use martensite::widgets::attachment::Attachment;
    ///
    /// assert_eq!(Attachment::new("a.txt", 5).size_bytes, 5);
    /// ```
    pub fn new(name: impl Into<String>, size_bytes: u64) -> Self {
        Self {
            label: "Attachment".to_string(),
            name: name.into(),
            size_bytes,
            glyph: "📄".to_string(),
            upload: None,
            removed: false,
            close_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            bar_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Type glyph override.
    ///
    /// ```
    /// use martensite::widgets::attachment::Attachment;
    ///
    /// assert_eq!(Attachment::new("a.zip", 1).glyph("🗜").glyph, "🗜");
    /// ```
    pub fn glyph(mut self, glyph: impl Into<String>) -> Self {
        self.glyph = glyph.into();
        self
    }

    /// Shows the upload bar at `0.0..=1.0`.
    ///
    /// ```
    /// use martensite::widgets::attachment::Attachment;
    ///
    /// assert_eq!(Attachment::new("a", 1).uploading(0.4).upload, Some(0.4));
    /// ```
    pub fn uploading(mut self, fraction: f32) -> Self {
        self.upload = Some(fraction.clamp(0.0, 1.0));
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::attachment::Attachment;
    ///
    /// assert_eq!(Attachment::new("a", 1).label("File").label, "File");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::attachment::Attachment;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _a = Attachment::new("a", 1).with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// `true` once when × is clicked.
    ///
    /// ```
    /// use martensite::widgets::attachment::Attachment;
    ///
    /// assert!(!Attachment::new("a", 1).take_removed());
    /// ```
    pub fn take_removed(&mut self) -> bool {
        std::mem::take(&mut self.removed)
    }

    /// Human-readable size (`"200 KB"`).
    ///
    /// ```
    /// use martensite::widgets::attachment::Attachment;
    ///
    /// assert_eq!(Attachment::new("a", 204_800).size_label(), "200 KB");
    /// ```
    pub fn size_label(&self) -> String {
        human_size(self.size_bytes)
    }
}

impl Widget for Attachment {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let w = (PAD_PT + ICON_PT + GAP_PT + 120.0 + GAP_PT + CLOSE_PT + PAD_PT) * s;
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            ((PAD_PT * 2.0 + ICON_PT.max(NAME_PT + 2.0 + SIZE_PT)) * s)
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 32.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let cs = CLOSE_PT * s;
        self.close_rect = Rect::new(
            bounds.max_x() - PAD_PT * s - cs,
            bounds.min_y() + (bounds.height() - cs) / 2.0,
            cs,
            cs,
        );
        self.bar_rect = Rect::new(
            bounds.min_x() + PAD_PT * s,
            bounds.max_y() - 2.0 * s - BAR_PT * s,
            (bounds.width() - PAD_PT * 2.0 * s).max(0.0),
            BAR_PT * s,
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!("{}: {}", self.label, self.name));
        node.set_value(self.size_label());
        node.add_action(accesskit::Action::Click);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerReleased {
            button: PointerButton::Primary,
            position,
        } = cx.event
        {
            if self.close_rect.contains(*position) {
                self.removed = true;
                return EventResponse::RequestRepaint;
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = self.bounds;
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            &martensite_core::shape::Shape::rounded(5.0 * s),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        cx.list.push_stroke_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            1.0,
            EDGE,
        );
        // Icon square.
        let pad = PAD_PT * s;
        let ic = ICON_PT * s;
        let iy = b.min_y() + (b.height() - ic) / 2.0;
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(b.min_x() + pad),
                f64::from(iy),
                f64::from(b.min_x() + pad + ic),
                f64::from(iy + ic),
            ),
            &martensite_core::shape::Shape::rounded(4.0 * s),
            ICON_BG,
        );
        let ifs = SIZE_PT * s;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(b.min_x() + pad + ic * 0.18),
                f64::from(iy + ic * 0.72),
            ),
            &self.glyph,
            ifs,
            TEXT_FG,
        );
        // Name + size.
        let tx = b.min_x() + pad + ic + GAP_PT * s;
        let nfs = NAME_PT * s;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(f64::from(tx), f64::from(b.min_y() + pad + nfs)),
            &self.name,
            nfs,
            cx.color(TokenKey::TextColor, TEXT_FG),
        );
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(tx),
                f64::from(b.min_y() + pad + nfs + 2.0 * s + SIZE_PT * s),
            ),
            &self.size_label(),
            SIZE_PT * s,
            MUTED_FG,
        );
        // Upload bar.
        if let Some(p) = self.upload {
            let br = self.bar_rect;
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(br.min_x()),
                    f64::from(br.min_y()),
                    f64::from(br.max_x()),
                    f64::from(br.max_y()),
                ),
                BAR_BG,
            );
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(br.min_x()),
                    f64::from(br.min_y()),
                    f64::from(br.min_x() + br.width() * p),
                    f64::from(br.max_y()),
                ),
                cx.color(TokenKey::AccentColor, [90, 140, 220, 255]),
            );
        }
        // Close ×.
        let cr = self.close_rect;
        let q = cr.width() * 0.22;
        let mut p = kurbo::BezPath::new();
        p.move_to((f64::from(cr.min_x() + q), f64::from(cr.min_y() + q)));
        p.line_to((f64::from(cr.max_x() - q), f64::from(cr.max_y() - q)));
        p.move_to((f64::from(cr.max_x() - q), f64::from(cr.min_y() + q)));
        p.line_to((f64::from(cr.min_x() + q), f64::from(cr.max_y() - q)));
        cx.list.push_stroke_path(p, 1.4 * s, MUTED_FG);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(a: &mut Attachment) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        a.layout(&mut cx, Rect::new(0.0, 0.0, 220.0, 44.0));
    }

    #[test]
    fn close_click_flags() {
        let mut a = Attachment::new("f.pdf", 1024);
        laid_out(&mut a);
        let r = a.close_rect;
        a.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(r.min_x() + r.width() / 2.0, r.min_y() + r.height() / 2.0),
            },
            bounds: a.bounds,
            scale: 1.0,
        });
        assert!(a.take_removed());
        assert!(!a.take_removed());
    }

    #[test]
    fn size_formatting() {
        assert_eq!(Attachment::new("a", 500).size_label(), "500 B");
        assert_eq!(Attachment::new("a", 204_800).size_label(), "200 KB");
        assert_eq!(Attachment::new("a", 5_242_880).size_label(), "5.0 MB");
    }

    #[test]
    fn paint_without_painter() {
        let mut a = Attachment::new("f.pdf", 1024).uploading(0.4);
        laid_out(&mut a);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        a.paint(&mut PaintContext {
            list: &mut list,
            bounds: a.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
