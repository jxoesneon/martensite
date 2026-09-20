//! `ToolbarOverflow` — a collapsing toolbar strip (Qt `QToolBar`
//! extension / VS Code `···` idiom): labeled pill items laid out
//! left-to-right; whatever doesn't fit the allotted width folds
//! behind a trailing `⋯` chevron.
//!
//! Clicking a visible item parks its index in
//! [`ToolbarOverflow::take_activated`]; clicking the chevron parks
//! the overflowed indices in
//! [`ToolbarOverflow::take_overflow`] for the host to open as a
//! menu. Companion to [`Toolbar`](crate::widgets::Toolbar).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::toolbar_overflow::ToolbarOverflow;
//!
//! let t = ToolbarOverflow::new().item("Save").item("Share");
//! assert_eq!(t.item_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 4.0;
const GAP_PT: f32 = 6.0;
const PILL_H_PT: f32 = 26.0;
const PILL_PAD_PT: f32 = 10.0;
const FONT_PT: f32 = 11.0;
const CHEVRON_PT: f32 = 30.0;

const FACE: [u8; 4] = [30, 32, 40, 255];
const PILL: [u8; 4] = [52, 55, 66, 255];
const PILL_HOVER: [u8; 4] = [66, 70, 84, 255];
const TEXT: [u8; 4] = [235, 237, 240, 255];

/// The collapsing strip — see the module docs.
///
/// ```
/// use martensite::widgets::toolbar_overflow::ToolbarOverflow;
///
/// assert_eq!(ToolbarOverflow::new().item_count(), 0);
/// ```
pub struct ToolbarOverflow {
    /// Accessibility label.
    pub label: String,
    items: Vec<String>,
    activated: Option<usize>,
    overflow_taken: Option<Vec<usize>>,
    hovered: Option<usize>, // usize::MAX = chevron
    pill_rects: Vec<Rect>,
    chevron_rect: Rect,
    visible: usize,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for ToolbarOverflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolbarOverflow")
            .field("items", &self.items.len())
            .field("visible", &self.visible)
            .finish()
    }
}

impl Default for ToolbarOverflow {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolbarOverflow {
    /// Empty strip.
    ///
    /// ```
    /// use martensite::widgets::toolbar_overflow::ToolbarOverflow;
    ///
    /// assert_eq!(ToolbarOverflow::new().item_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Toolbar overflow".to_string(),
            items: Vec::new(),
            activated: None,
            overflow_taken: None,
            hovered: None,
            pill_rects: Vec::new(),
            chevron_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            visible: 0,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends an item.
    ///
    /// ```
    /// use martensite::widgets::toolbar_overflow::ToolbarOverflow;
    ///
    /// assert_eq!(ToolbarOverflow::new().item("A").item_count(), 1);
    /// ```
    pub fn item(mut self, label: impl Into<String>) -> Self {
        self.items.push(label.into());
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::toolbar_overflow::ToolbarOverflow;
    ///
    /// assert_eq!(ToolbarOverflow::new().label("Tools").label, "Tools");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::toolbar_overflow::ToolbarOverflow;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _t = ToolbarOverflow::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Item count.
    ///
    /// ```
    /// use martensite::widgets::toolbar_overflow::ToolbarOverflow;
    ///
    /// assert_eq!(ToolbarOverflow::new().item_count(), 0);
    /// ```
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// How many items fit in the current bounds.
    ///
    /// ```
    /// use martensite::widgets::toolbar_overflow::ToolbarOverflow;
    ///
    /// assert_eq!(ToolbarOverflow::new().visible_count(), 0);
    /// ```
    pub fn visible_count(&self) -> usize {
        self.visible
    }

    /// Indices folded behind the chevron.
    ///
    /// ```
    /// use martensite::widgets::toolbar_overflow::ToolbarOverflow;
    ///
    /// assert!(ToolbarOverflow::new().overflowed().is_empty());
    /// ```
    pub fn overflowed(&self) -> Vec<usize> {
        (self.visible..self.items.len()).collect()
    }

    /// Drains the last clicked item index.
    ///
    /// ```
    /// use martensite::widgets::toolbar_overflow::ToolbarOverflow;
    ///
    /// let mut t = ToolbarOverflow::new();
    /// assert_eq!(t.take_activated(), None);
    /// ```
    pub fn take_activated(&mut self) -> Option<usize> {
        self.activated.take()
    }

    /// Drains the overflowed indices after a chevron click.
    ///
    /// ```
    /// use martensite::widgets::toolbar_overflow::ToolbarOverflow;
    ///
    /// let mut t = ToolbarOverflow::new();
    /// assert_eq!(t.take_overflow(), None);
    /// ```
    pub fn take_overflow(&mut self) -> Option<Vec<usize>> {
        self.overflow_taken.take()
    }

    fn pill_width(&self, i: usize, s: f32) -> f32 {
        let fs = FONT_PT * s;
        self.items[i].len() as f32 * fs * 0.55 + PILL_PAD_PT * 2.0 * s
    }
}

impl Widget for ToolbarOverflow {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        Vec2::new(
            (240.0 * s).min(constraints.max_size.x.max(0.0)),
            ((PILL_H_PT + PAD_PT * 2.0) * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(
            CHEVRON_PT + PAD_PT * 2.0,
            PILL_H_PT + PAD_PT * 2.0,
        ))
        .with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let mut x = bounds.min_x() + PAD_PT * s;
        let y = bounds.min_y() + PAD_PT * s;
        let h = PILL_H_PT * s;
        let chevron_w = CHEVRON_PT * s;
        let limit = bounds.max_x() - PAD_PT * s;
        self.pill_rects.clear();
        self.visible = 0;
        for i in 0..self.items.len() {
            let w = self.pill_width(i, s);
            // Reserve room for the chevron unless this is the last item.
            let reserve = if i + 1 < self.items.len() {
                chevron_w
            } else {
                0.0
            };
            if x + w + reserve > limit {
                break;
            }
            self.pill_rects.push(Rect::new(x, y, w, h));
            self.visible = i + 1;
            x += w + GAP_PT * s;
        }
        self.chevron_rect = if self.visible < self.items.len() {
            Rect::new(x, y, chevron_w, h)
        } else {
            Rect::new(0.0, 0.0, 0.0, 0.0)
        };
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Toolbar);
        node.set_label(self.label.clone());
        node.set_value(format!(
            "{} of {} items visible",
            self.visible,
            self.items.len()
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = if self.chevron_rect.contains(*position) {
                    Some(usize::MAX)
                } else {
                    self.pill_rects.iter().position(|r| r.contains(*position))
                };
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
                if self.visible < self.items.len() && self.chevron_rect.contains(*position) {
                    self.overflow_taken = Some(self.overflowed());
                    return EventResponse::RequestRepaint;
                }
                if let Some(i) = self.pill_rects.iter().position(|r| r.contains(*position)) {
                    self.activated = Some(i);
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
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(self.bounds.min_x()),
                f64::from(self.bounds.min_y()),
                f64::from(self.bounds.max_x()),
                f64::from(self.bounds.max_y()),
            ),
            cx.color(TokenKey::BackgroundColor, FACE),
        );
        let shape = martensite_core::shape::Shape::rounded(5.0 * s);
        let fs = FONT_PT * s;
        for (i, r) in self.pill_rects.iter().enumerate() {
            let kr = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            let bg = if self.hovered == Some(i) {
                PILL_HOVER
            } else {
                PILL
            };
            cx.list.push_fill_shape(kr, &shape, bg);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kr,
                kurbo::Point::new(
                    f64::from(r.min_x() + PILL_PAD_PT * s),
                    f64::from(r.min_y() + r.height() * 0.7),
                ),
                &self.items[i],
                fs,
                cx.color(TokenKey::TextColor, TEXT),
            );
        }
        // Chevron.
        if self.visible < self.items.len() {
            let r = self.chevron_rect;
            let kr = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            let bg = if self.hovered == Some(usize::MAX) {
                PILL_HOVER
            } else {
                PILL
            };
            cx.list.push_fill_shape(kr, &shape, bg);
            let label = format!("+{}", self.items.len() - self.visible);
            let tw = painter
                .and_then(|p| p.measure_text(&label, fs))
                .unwrap_or(label.len() as f32 * fs * 0.55);
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(r.min_x() + (r.width() - tw) / 2.0),
                    f64::from(r.min_y() + r.height() * 0.7),
                ),
                &label,
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

    fn fixture() -> ToolbarOverflow {
        ToolbarOverflow::new()
            .item("Save")
            .item("Share")
            .item("Export")
            .item("Delete")
            .item("Print")
    }

    fn laid_out(t: &mut ToolbarOverflow, w: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        t.layout(&mut cx, Rect::new(0.0, 0.0, w, 34.0));
    }

    fn click(t: &mut ToolbarOverflow, r: Rect) {
        t.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
            bounds: t.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn all_fit_when_wide() {
        let mut t = fixture();
        laid_out(&mut t, 800.0);
        assert_eq!(t.visible_count(), 5);
        assert!(t.overflowed().is_empty());
    }

    #[test]
    fn narrow_folds_items() {
        let mut t = fixture();
        laid_out(&mut t, 160.0);
        assert!(t.visible_count() < 5);
        assert_eq!(t.visible_count() + t.overflowed().len(), 5);
    }

    #[test]
    fn chevron_parks_overflow() {
        let mut t = fixture();
        laid_out(&mut t, 160.0);
        assert!(t.visible_count() < 5);
        let r = t.chevron_rect;
        click(&mut t, r);
        let taken = t.take_overflow().unwrap();
        assert_eq!(taken, t.overflowed());
    }

    #[test]
    fn pill_parks_activated() {
        let mut t = fixture();
        laid_out(&mut t, 800.0);
        let r = t.pill_rects[1];
        click(&mut t, r);
        assert_eq!(t.take_activated(), Some(1));
    }

    #[test]
    fn paint_without_painter() {
        let mut t = fixture();
        laid_out(&mut t, 160.0);
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
