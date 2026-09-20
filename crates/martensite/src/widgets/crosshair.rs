//! `Crosshair` — design-tool hairlines: vertical + horizontal
//! guide lines tracking the pointer through the widget, with an
//! optional `x, y` coordinate readout (GIMP/Figma idiom).
//!
//! Pointer movement updates the tracked position and parks it in
//! [`Crosshair::take_moved`] (normalized `0..=1`); the host can
//! also drive it with [`Crosshair::set_position`]. Pairs with
//! [`Magnifier`](crate::widgets::Magnifier) and
//! [`Ruler`](crate::widgets::Ruler) in design-tool surfaces.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::crosshair::Crosshair;
//!
//! let mut c = Crosshair::new();
//! c.set_position(glam::Vec2::new(0.5, 0.5));
//! assert_eq!(c.position(), Some(glam::Vec2::new(0.5, 0.5)));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const HAIR: [u8; 4] = [230, 90, 90, 200];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const CHIP: [u8; 4] = [30, 32, 40, 220];
const FONT_PT: f32 = 11.0;
const CHIP_PAD_PT: f32 = 6.0;

/// The crosshair — see the module docs.
///
/// ```
/// use martensite::widgets::crosshair::Crosshair;
///
/// assert_eq!(Crosshair::new().position(), None);
/// ```
pub struct Crosshair {
    /// Accessibility label.
    pub label: String,
    /// Show the coordinate readout chip.
    pub show_readout: bool,
    /// Hairline color override.
    pub color: Option<[u8; 4]>,
    pos: Option<Vec2>,
    moved: Option<Vec2>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Crosshair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Crosshair").field("pos", &self.pos).finish()
    }
}

impl Default for Crosshair {
    fn default() -> Self {
        Self::new()
    }
}

impl Crosshair {
    /// Idle crosshair (no position until the pointer enters).
    ///
    /// ```
    /// use martensite::widgets::crosshair::Crosshair;
    ///
    /// assert_eq!(Crosshair::new().position(), None);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Crosshair".to_string(),
            show_readout: true,
            color: None,
            pos: None,
            moved: None,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::crosshair::Crosshair;
    ///
    /// assert_eq!(Crosshair::new().label("Align").label, "Align");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Hairline color.
    ///
    /// ```
    /// use martensite::widgets::crosshair::Crosshair;
    ///
    /// assert_eq!(Crosshair::new().color([1; 4]).color, Some([1; 4]));
    /// ```
    pub fn color(mut self, color: [u8; 4]) -> Self {
        self.color = Some(color);
        self
    }

    /// Shared text painter for the readout.
    ///
    /// ```no_run
    /// use martensite::widgets::crosshair::Crosshair;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _c = Crosshair::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Tracked position, normalized `0..=1` over the bounds.
    ///
    /// ```
    /// use martensite::widgets::crosshair::Crosshair;
    ///
    /// assert_eq!(Crosshair::new().position(), None);
    /// ```
    pub fn position(&self) -> Option<Vec2> {
        self.pos
    }

    /// Sets the position host-side (normalized, clamped).
    ///
    /// ```
    /// use martensite::widgets::crosshair::Crosshair;
    ///
    /// let mut c = Crosshair::new();
    /// c.set_position(glam::Vec2::new(1.5, -1.0));
    /// assert_eq!(c.position(), Some(glam::Vec2::new(1.0, 0.0)));
    /// ```
    pub fn set_position(&mut self, pos: Vec2) {
        self.pos = Some(Vec2::new(pos.x.clamp(0.0, 1.0), pos.y.clamp(0.0, 1.0)));
    }

    /// Clears the tracked position (pointer left).
    ///
    /// ```
    /// use martensite::widgets::crosshair::Crosshair;
    ///
    /// let mut c = Crosshair::new();
    /// c.set_position(glam::Vec2::new(0.5, 0.5));
    /// c.clear();
    /// assert_eq!(c.position(), None);
    /// ```
    pub fn clear(&mut self) {
        self.pos = None;
    }

    /// Drains the last pointer-driven position.
    ///
    /// ```
    /// use martensite::widgets::crosshair::Crosshair;
    ///
    /// let mut c = Crosshair::new();
    /// assert_eq!(c.take_moved(), None);
    /// ```
    pub fn take_moved(&mut self) -> Option<Vec2> {
        self.moved.take()
    }
}

impl Widget for Crosshair {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        constraints.max_size
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
        if let Some(p) = self.pos {
            node.set_value(format!("{:.0}%, {:.0}%", p.x * 100.0, p.y * 100.0));
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                if !self.bounds.contains(*position) {
                    if self.pos.is_some() {
                        self.pos = None;
                        return EventResponse::RequestRepaint;
                    }
                    return EventResponse::Ignored;
                }
                let n = Vec2::new(
                    ((position.x - self.bounds.min_x()) / self.bounds.width().max(1.0))
                        .clamp(0.0, 1.0),
                    ((position.y - self.bounds.min_y()) / self.bounds.height().max(1.0))
                        .clamp(0.0, 1.0),
                );
                if self.pos != Some(n) {
                    self.pos = Some(n);
                    self.moved = Some(n);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.pos.take().is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let Some(p) = self.pos else { return };
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let hair = cx.color(TokenKey::ErrorColor, self.color.unwrap_or(HAIR));
        let x = self.bounds.min_x() + p.x * self.bounds.width();
        let y = self.bounds.min_y() + p.y * self.bounds.height();
        let w = 1.0f32.max(s);
        // Hairlines.
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(x - w / 2.0),
                f64::from(self.bounds.min_y()),
                f64::from(x + w / 2.0),
                f64::from(self.bounds.max_y()),
            ),
            hair,
        );
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(self.bounds.min_x()),
                f64::from(y - w / 2.0),
                f64::from(self.bounds.max_x()),
                f64::from(y + w / 2.0),
            ),
            hair,
        );
        // Readout chip next to the intersection.
        if self.show_readout {
            let text = format!("{:.0}, {:.0}", p.x * 100.0, p.y * 100.0);
            let fs = FONT_PT * s;
            let tw = painter
                .and_then(|pt| pt.measure_text(&text, fs))
                .unwrap_or(text.len() as f32 * fs * 0.6);
            let pad = CHIP_PAD_PT * s;
            let mut cx0 = x + pad * 2.0;
            let mut cy0 = y + pad * 2.0;
            // Keep the chip inside the bounds.
            cx0 = cx0.min(self.bounds.max_x() - tw - pad * 2.0);
            cy0 = cy0.min(self.bounds.max_y() - fs - pad * 2.0);
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(cx0),
                    f64::from(cy0),
                    f64::from(cx0 + tw + pad * 2.0),
                    f64::from(cy0 + fs + pad * 2.0),
                ),
                cx.color(TokenKey::SurfaceColor, CHIP),
            );
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(f64::from(cx0 + pad), f64::from(cy0 + pad + fs * 0.5)),
                &text,
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

    fn laid_out(c: &mut Crosshair) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 200.0, 100.0));
    }

    #[test]
    fn pointer_tracks_normalized() {
        let mut c = Crosshair::new();
        laid_out(&mut c);
        let resp = c.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(100.0, 50.0),
            },
            bounds: c.bounds,
            scale: 1.0,
        });
        assert_eq!(resp, EventResponse::RequestRepaint);
        assert_eq!(c.position(), Some(Vec2::new(0.5, 0.5)));
        assert_eq!(c.take_moved(), Some(Vec2::new(0.5, 0.5)));
    }

    #[test]
    fn pointer_outside_clears() {
        let mut c = Crosshair::new();
        laid_out(&mut c);
        c.set_position(Vec2::new(0.5, 0.5));
        c.event(&mut EventContext {
            event: &WidgetEvent::PointerLeave,
            bounds: c.bounds,
            scale: 1.0,
        });
        assert_eq!(c.position(), None);
    }

    #[test]
    fn paint_without_painter() {
        let mut c = Crosshair::new();
        laid_out(&mut c);
        c.set_position(Vec2::new(0.5, 0.5));
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        c.paint(&mut PaintContext {
            list: &mut list,
            bounds: c.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
        // Nothing paints without a position.
        c.clear();
        let mut list2 = martensite_core::PaintList::default();
        c.paint(&mut PaintContext {
            list: &mut list2,
            bounds: c.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(list2.is_empty());
    }
}
