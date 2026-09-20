//! `Minimap` — a document-overview strip (editor minimap idiom).
//!
//! Each source line renders as a squashed content bar (width ∝
//! length) down a narrow column, and a translucent viewport rect
//! marks `scroll`/`viewport` fractions. Clicking or dragging
//! parks the picked scroll fraction `0..=1` in
//! [`Minimap::take_scrolled`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::minimap::Minimap;
//!
//! let m = Minimap::new().lines([10, 40, 8]);
//! assert_eq!(m.line_count(), 3);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const WIDTH_PT: f32 = 60.0;
const LINE_PT: f32 = 3.0;
const PAD_PT: f32 = 2.0;

const FACE: [u8; 4] = [30, 30, 36, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const INK: [u8; 4] = [110, 110, 120, 255];
const VIEW: [u8; 4] = [120, 170, 230, 40];
const VIEW_EDGE: [u8; 4] = [120, 170, 230, 160];

/// A document-overview strip — see the module docs.
///
/// ```
/// use martensite::widgets::minimap::Minimap;
///
/// assert_eq!(Minimap::new().line_count(), 0);
/// ```
#[derive(Debug)]
pub struct Minimap {
    /// Accessibility label.
    pub label: String,
    /// Scroll fraction `0..=1` shown by the viewport rect.
    pub scroll: f32,
    /// Viewport height fraction `0..=1`.
    pub viewport: f32,
    line_lens: Vec<u32>,
    dragging: bool,
    pending: Option<f32>,
    bounds: Rect,
    scale: f32,
}

impl Default for Minimap {
    fn default() -> Self {
        Self::new()
    }
}

impl Minimap {
    /// Creates an empty minimap.
    ///
    /// ```
    /// use martensite::widgets::minimap::Minimap;
    ///
    /// assert_eq!(Minimap::new().line_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Minimap".to_string(),
            scroll: 0.0,
            viewport: 0.25,
            line_lens: Vec::new(),
            dragging: false,
            pending: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Line lengths (one per document line; char counts).
    ///
    /// ```
    /// use martensite::widgets::minimap::Minimap;
    ///
    /// let m = Minimap::new().lines(vec![12u32, 5, 30]);
    /// assert_eq!(m.line_count(), 3);
    /// ```
    pub fn lines(mut self, lines: impl IntoIterator<Item = u32>) -> Self {
        self.line_lens = lines.into_iter().collect();
        self
    }

    /// Scroll fraction for the viewport rect.
    ///
    /// ```
    /// use martensite::widgets::minimap::Minimap;
    ///
    /// assert_eq!(Minimap::new().scroll(0.5).scroll_fraction(), 0.5);
    /// ```
    pub fn scroll(mut self, scroll: f32) -> Self {
        self.scroll = scroll.clamp(0.0, 1.0);
        self
    }

    /// Viewport height fraction.
    ///
    /// ```
    /// use martensite::widgets::minimap::Minimap;
    ///
    /// assert_eq!(Minimap::new().viewport(0.5).viewport_fraction(), 0.5);
    /// ```
    pub fn viewport(mut self, viewport: f32) -> Self {
        self.viewport = viewport.clamp(0.05, 1.0);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::minimap::Minimap;
    ///
    /// assert_eq!(Minimap::new().label("main.rs").label, "main.rs");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Line count.
    ///
    /// ```
    /// use martensite::widgets::minimap::Minimap;
    ///
    /// assert_eq!(Minimap::new().lines([1u32, 2]).line_count(), 2);
    /// ```
    pub fn line_count(&self) -> usize {
        self.line_lens.len()
    }

    /// Scroll fraction.
    ///
    /// ```
    /// use martensite::widgets::minimap::Minimap;
    ///
    /// assert_eq!(Minimap::new().scroll_fraction(), 0.0);
    /// ```
    pub fn scroll_fraction(&self) -> f32 {
        self.scroll
    }

    /// Viewport fraction.
    ///
    /// ```
    /// use martensite::widgets::minimap::Minimap;
    ///
    /// assert_eq!(Minimap::new().viewport_fraction(), 0.25);
    /// ```
    pub fn viewport_fraction(&self) -> f32 {
        self.viewport
    }

    /// Drains the last picked scroll fraction.
    ///
    /// ```
    /// use martensite::widgets::minimap::Minimap;
    ///
    /// let mut m = Minimap::new();
    /// assert!(m.take_scrolled().is_none());
    /// ```
    pub fn take_scrolled(&mut self) -> Option<f32> {
        self.pending.take()
    }

    /// Sets the scroll fraction (programmatic).
    ///
    /// ```
    /// use martensite::widgets::minimap::Minimap;
    ///
    /// let mut m = Minimap::new();
    /// m.set_scroll(0.4);
    /// assert_eq!(m.scroll_fraction(), 0.4);
    /// ```
    pub fn set_scroll(&mut self, scroll: f32) {
        self.scroll = scroll.clamp(0.0, 1.0);
    }

    /// Pixels per document line.
    fn row_h(&self) -> f32 {
        (LINE_PT * self.scale).max(1.0)
    }

    /// Fraction of the document a y position maps to.
    fn fraction_at(&self, y: f32) -> f32 {
        let content = self.line_lens.len() as f32 * self.row_h();
        ((y - self.bounds.min_y()) / content.max(1.0)).clamp(0.0, 1.0)
    }

    /// The viewport rect in widget space.
    fn view_rect(&self) -> Rect {
        let content = self.line_lens.len() as f32 * self.row_h();
        let vh = (self.viewport * content).min(content).max(6.0 * self.scale);
        let y = self.bounds.min_y() + self.scroll * (content - vh).max(0.0);
        Rect::new(
            self.bounds.min_x(),
            y,
            self.bounds.width(),
            vh.min(self.bounds.height()),
        )
    }
}

impl Widget for Minimap {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            constraints.max_size.y.max(0.0).min(400.0 * cx.scale),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(20.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Slider);
        node.set_label(format!(
            "{} — scroll {:.0}%",
            self.label,
            self.scroll * 100.0
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if self.bounds.contains(*position) {
                    self.dragging = true;
                    let f = self.fraction_at(position.y);
                    self.pending = Some(f);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if !self.dragging {
                    return EventResponse::Ignored;
                }
                self.pending = Some(self.fraction_at(position.y));
                EventResponse::Handled
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.dragging {
                    self.dragging = false;
                    return EventResponse::Handled;
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
            .push_fill_rect(f(self.bounds), cx.color(TokenKey::SurfaceColor, FACE));

        let pad = PAD_PT * self.scale;
        let row_h = self.row_h();
        let max_len = self.line_lens.iter().copied().max().unwrap_or(1) as f32;
        let ink = cx.color(TokenKey::TextMutedColor, INK);
        // Squashed line glyphs.
        for (i, &len) in self.line_lens.iter().enumerate() {
            let y = self.bounds.min_y() + i as f32 * row_h;
            if y > self.bounds.max_y() {
                break;
            }
            let w = pad + (len as f32 / max_len) * (self.bounds.width() - 2.0 * pad);
            cx.list.push_fill_rect(
                f(Rect::new(
                    self.bounds.min_x() + pad,
                    y,
                    w,
                    (row_h * 0.6).max(1.0),
                )),
                ink,
            );
        }
        // Viewport rect.
        let vr = self.view_rect();
        cx.list
            .push_fill_rect(f(vr), cx.color(TokenKey::AccentColor, VIEW));
        cx.list.push_stroke_shape(
            f(vr),
            &martensite_core::shape::Shape::RECT,
            cx.pt(0.75),
            cx.color(TokenKey::AccentColor, VIEW_EDGE),
        );
        cx.list.push_stroke_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::RECT,
            cx.pt(0.75),
            cx.color(TokenKey::BorderColor, EDGE),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(m: &mut Minimap, w: f32, h: f32) {
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

    fn ev(m: &mut Minimap, e: WidgetEvent) {
        m.event(&mut EventContext {
            event: &e,
            bounds: Rect::new(0.0, 0.0, 60.0, 300.0),
            scale: 1.0,
        });
    }

    #[test]
    fn clamps_fractions() {
        let m = Minimap::new().scroll(1.5).viewport(0.01);
        assert_eq!(m.scroll_fraction(), 1.0);
        assert_eq!(m.viewport_fraction(), 0.05);
    }

    #[test]
    fn click_picks_fraction() {
        let mut m = Minimap::new().lines(vec![10u32; 100]); // 300px tall content
        laid_out(&mut m, 60.0, 300.0);
        ev(
            &mut m,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(30.0, 150.0),
                count: 1,
            },
        );
        let f = m.take_scrolled().unwrap();
        assert!((f - 0.5).abs() < 0.02);
    }

    #[test]
    fn drag_repeats_picks() {
        let mut m = Minimap::new().lines(vec![10u32; 100]);
        laid_out(&mut m, 60.0, 300.0);
        ev(
            &mut m,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(30.0, 30.0),
                count: 1,
            },
        );
        m.take_scrolled();
        ev(
            &mut m,
            WidgetEvent::PointerMoved {
                position: Vec2::new(30.0, 270.0),
            },
        );
        let f = m.take_scrolled().unwrap();
        assert!(f > 0.85);
    }

    #[test]
    fn outside_ignored() {
        let mut m = Minimap::new().lines(vec![10u32; 10]);
        laid_out(&mut m, 60.0, 300.0);
        ev(
            &mut m,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(300.0, 30.0),
                count: 1,
            },
        );
        assert!(m.take_scrolled().is_none());
    }
}
