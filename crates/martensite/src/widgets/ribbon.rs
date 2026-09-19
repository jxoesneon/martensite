//! `Ribbon` — corner ribbon overlay on a card.
//!
//! The Ant `Badge.Ribbon` pattern: a colored band pinned to a corner
//! of the wrapped content carrying short text ("New", "Beta",
//! "Pro"). The ribbon is chrome — the child keeps its full bounds and
//! event flow; clicks inside the ribbon band still reach the child.
//!
//! # Examples
//!
//! ```
//! use martensite::prelude::*;
//! use martensite::widgets::ribbon::{Ribbon, RibbonCorner};
//!
//! let r = Ribbon::new("Beta")
//!     .corner(RibbonCorner::TopEnd)
//!     .child(Container::new());
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    LayoutConstraints, LayoutContext, PaintContext, Rect, RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

/// Default band fill.
const ACCENT: TokenKey = TokenKey::AccentColor;
/// Band text ink.
const INK: TokenKey = TokenKey::TextInverseColor;
/// Band height (logical points).
const BAND_H: f32 = 22.0;
/// Horizontal padding inside the band (logical points).
const PAD_X: f32 = 10.0;
/// Corner offset from the child's edge (logical points).
const OFFSET: f32 = 8.0;

/// Which corner the band hugs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RibbonCorner {
    /// Top-right (the Ant default).
    #[default]
    TopEnd,
    /// Top-left.
    TopStart,
}

/// A corner ribbon wrapper — see the module docs. Reports exactly one
/// child when installed.
///
/// # Examples
///
/// ```
/// use martensite::prelude::*;
/// use martensite::widgets::ribbon::Ribbon;
/// use martensite::core::Widget;
///
/// let mut r = Ribbon::new("New").child(Container::new());
/// assert_eq!(r.child_count(), 1);
/// ```
pub struct Ribbon {
    text: String,
    corner: RibbonCorner,
    color: Option<TokenKey>,
    enabled: bool,
    child: Option<Box<dyn Widget>>,
    child_bounds: Option<Rect>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Ribbon {
    /// A ribbon showing `text`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ribbon::Ribbon;
    ///
    /// let r = Ribbon::new("Beta");
    /// assert_eq!(r.text(), "Beta");
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            corner: RibbonCorner::TopEnd,
            color: None,
            enabled: true,
            child: None,
            child_bounds: None,
            text_painter: None,
        }
    }

    /// Which corner the band hugs (default `TopEnd`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ribbon::{Ribbon, RibbonCorner};
    ///
    /// let r = Ribbon::new("x").corner(RibbonCorner::TopStart);
    /// ```
    pub fn corner(mut self, corner: RibbonCorner) -> Self {
        self.corner = corner;
        self
    }

    /// Override the band color (default `AccentColor`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ribbon::Ribbon;
    /// use martensite_theme::TokenKey;
    ///
    /// let r = Ribbon::new("Sale").color(TokenKey::ErrorColor);
    /// ```
    pub fn color(mut self, color: TokenKey) -> Self {
        self.color = Some(color);
        self
    }

    /// Install the wrapped child.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::ribbon::Ribbon;
    ///
    /// let r = Ribbon::new("x").child(Container::new());
    /// ```
    pub fn child(mut self, child: impl Widget) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    /// Enable or disable (dims the band; default `true`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ribbon::Ribbon;
    ///
    /// let r = Ribbon::new("x").enabled(false);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Share a text painter. `SharedTextPainter` is not `Default`, so
    /// this builder is exercised indirectly through `paint`.
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The band text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ribbon::Ribbon;
    ///
    /// assert_eq!(Ribbon::new("Pro").text(), "Pro");
    /// ```
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl Widget for Ribbon {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        if let Some(c) = self.child.as_mut() {
            return c.measure(cx, constraints);
        }
        Vec2::new(80.0, 60.0)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.child_bounds = Some(bounds);
        if let Some(c) = self.child.as_mut() {
            cx.layout_child(c.as_mut(), bounds);
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let Some(cb) = self.child_bounds else { return };
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let band_color = cx.color(self.color.unwrap_or(ACCENT), [50, 115, 230, 255]);
        let ink = cx.color(INK, [255, 255, 255, 255]);
        let muted = cx.color(TokenKey::TextMutedColor, [150, 150, 158, 255]);
        let band_h = cx.pt(BAND_H);
        let pad = cx.pt(PAD_X);
        let off = cx.pt(OFFSET);
        let size = 11.0 * cx.scale;

        let text_w = painter
            .and_then(|p| p.measure_text(&self.text, size))
            .unwrap_or(size * self.text.chars().count() as f32 * 0.55);
        let band_w = text_w + pad * 2.0;
        let (x0, shape) = match self.corner {
            RibbonCorner::TopEnd => (
                f64::from(cb.max_x() - off - band_w),
                martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            ),
            RibbonCorner::TopStart => (
                f64::from(cb.min_x() + off),
                martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            ),
        };
        let band = kurbo::Rect::new(
            x0,
            f64::from(b.min_y() + off),
            x0 + f64::from(band_w),
            f64::from(b.min_y() + off + band_h),
        );
        cx.list
            .push_fill_shape(band, &shape, if self.enabled { band_color } else { muted });
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            band,
            kurbo::Point::new(band.x0 + f64::from(pad), band.y0 + band.height() * 0.72),
            &self.text,
            size,
            ink,
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
        node.set_label(self.text.as_str());
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn child_count(&self) -> usize {
        usize::from(self.child.is_some())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(())?;
        self.child.as_deref()
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(())?;
        self.child.as_deref_mut()
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(())?;
        self.child_bounds
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::button::Button;
    use martensite_core::HotNode;

    fn lay(w: &mut Ribbon) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, 200.0, 120.0));
    }

    #[test]
    fn child_gets_full_bounds() {
        let mut r = Ribbon::new("x").child(Button::new("y"));
        lay(&mut r);
        let cb = r.child_bounds.unwrap();
        assert_eq!(cb.width(), 200.0);
        assert_eq!(cb.height(), 120.0);
    }

    #[test]
    fn measure_delegates_to_child() {
        let mut r = Ribbon::new("x").child(Button::new("y"));
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let size = r.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(400.0, 400.0),
            },
        );
        assert!(size.x > 0.0 && size.y > 0.0);
    }

    #[test]
    fn events_flow_through_default_dispatch() {
        let mut r = Ribbon::new("x").child(Button::new("y"));
        lay(&mut r);
        // The default `event` forwards to the child bounds-gated —
        // the ribbon never intercepts.
        assert_eq!(r.child_count(), 1);
        assert!(r.child_bounds(0).is_some());
    }
}
