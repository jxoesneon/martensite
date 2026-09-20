//! `Coverflow` — the iTunes-style cover browser: the selected item
//! fronts the center full-size while neighbors recede to the sides
//! scaled down, faking the ring's perspective.
//!
//! Arrow keys and horizontal `Scroll` move the selection, clicking
//! a side cover selects it, and the index parks in
//! [`Coverflow::take_selected`]. Items are
//! [`Thumbnail`](crate::widgets::Thumbnail) swatches — label +
//! color — like [`Filmstrip`](crate::widgets::Filmstrip), but
//! spatial instead of linear.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::coverflow::Coverflow;
//! use martensite::widgets::Thumbnail;
//!
//! let c = Coverflow::new()
//!     .item(Thumbnail::new("A", [255, 0, 0, 255]))
//!     .item(Thumbnail::new("B", [0, 255, 0, 255]));
//! assert_eq!(c.item_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;
use crate::widgets::Thumbnail;

const COVER_PT: f32 = 140.0;
const STEP_PT: f32 = 70.0;
const TITLE_PT: f32 = 12.0;
const MAX_SIDE: usize = 3;
const SCALE_PER: f32 = 0.22;

const FACE: [u8; 4] = [24, 25, 29, 255];
const EDGE: [u8; 4] = [70, 72, 80, 255];
const TEXT: [u8; 4] = [220, 222, 228, 255];
const MUTED: [u8; 4] = [139, 148, 158, 255];

/// A receding cover browser — see the module docs.
///
/// ```
/// use martensite::widgets::coverflow::Coverflow;
///
/// assert_eq!(Coverflow::new().item_count(), 0);
/// ```
pub struct Coverflow {
    /// Accessibility label.
    pub label: String,
    /// Show the focused title below the covers.
    pub show_title: bool,
    items: Vec<Thumbnail>,
    selected: usize,
    selected_out: Option<usize>,
    rects: Vec<(usize, Rect)>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Coverflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Coverflow")
            .field("items", &self.items.len())
            .field("selected", &self.selected)
            .finish()
    }
}

impl Default for Coverflow {
    fn default() -> Self {
        Self::new()
    }
}

impl Coverflow {
    /// Empty coverflow.
    ///
    /// ```
    /// use martensite::widgets::coverflow::Coverflow;
    ///
    /// assert_eq!(Coverflow::new().item_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Cover flow".to_string(),
            show_title: true,
            items: Vec::new(),
            selected: 0,
            selected_out: None,
            rects: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a cover.
    ///
    /// ```
    /// use martensite::widgets::coverflow::Coverflow;
    /// use martensite::widgets::Thumbnail;
    ///
    /// assert_eq!(Coverflow::new().item(Thumbnail::new("A", [1; 4])).item_count(), 1);
    /// ```
    pub fn item(mut self, item: Thumbnail) -> Self {
        self.items.push(item);
        self
    }

    /// Initial selection.
    ///
    /// ```
    /// use martensite::widgets::coverflow::Coverflow;
    /// use martensite::widgets::Thumbnail;
    ///
    /// assert_eq!(
    ///     Coverflow::new().item(Thumbnail::new("A", [1; 4])).selected(0).current(),
    ///     Some(0)
    /// );
    /// ```
    pub fn selected(mut self, index: usize) -> Self {
        self.selected = index;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::coverflow::Coverflow;
    ///
    /// assert_eq!(Coverflow::new().label("Albums").label, "Albums");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter for labels.
    ///
    /// ```no_run
    /// use martensite::widgets::coverflow::Coverflow;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _c = Coverflow::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Number of covers.
    ///
    /// ```
    /// use martensite::widgets::coverflow::Coverflow;
    ///
    /// assert_eq!(Coverflow::new().item_count(), 0);
    /// ```
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Selected index, if any items exist.
    ///
    /// ```
    /// use martensite::widgets::coverflow::Coverflow;
    ///
    /// assert_eq!(Coverflow::new().current(), None);
    /// ```
    pub fn current(&self) -> Option<usize> {
        (!self.items.is_empty()).then(|| self.selected.min(self.items.len() - 1))
    }

    /// Sets the selection (host-driven, no seam).
    ///
    /// ```
    /// use martensite::widgets::coverflow::Coverflow;
    /// use martensite::widgets::Thumbnail;
    ///
    /// let mut c = Coverflow::new().item(Thumbnail::new("A", [1; 4])).item(Thumbnail::new("B", [1; 4]));
    /// c.set_selected(1);
    /// assert_eq!(c.current(), Some(1));
    /// ```
    pub fn set_selected(&mut self, index: usize) {
        if !self.items.is_empty() {
            self.selected = index.min(self.items.len() - 1);
        }
    }

    /// Drains the last selected index.
    ///
    /// ```
    /// use martensite::widgets::coverflow::Coverflow;
    ///
    /// let mut c = Coverflow::new();
    /// assert_eq!(c.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.selected_out.take()
    }

    /// Steps the selection by `d` (clamped), parking the index.
    fn step(&mut self, d: isize) {
        self.jump_to(
            (self.selected as isize)
                .saturating_add(d)
                .clamp(0, self.items.len().saturating_sub(1) as isize) as usize,
        );
    }

    /// Selects `index` and parks it when it changed.
    fn jump_to(&mut self, index: usize) {
        if self.items.is_empty() || index == self.selected {
            return;
        }
        self.selected = index;
        self.selected_out = Some(index);
    }

    /// Cover rect for item `i` (None when off the visible arc).
    fn cover_rect(&self, i: usize, sel: usize) -> Option<Rect> {
        let d = i as isize - sel as isize;
        if d.unsigned_abs() > MAX_SIDE {
            return None;
        }
        let s = self.scale;
        let shrink = (1.0 - d.unsigned_abs() as f32 * SCALE_PER).max(0.4);
        let size = COVER_PT * s * shrink;
        let cxm = self.bounds.min_x() + self.bounds.width() / 2.0;
        let cym = self.bounds.min_y() + self.bounds.height() / 2.0;
        // Sides pull inward and scale down — the perspective fake.
        let x = cxm + d as f32 * STEP_PT * s * shrink - size / 2.0;
        Some(Rect::new(x, cym - size / 2.0, size, size))
    }
}

impl Widget for Coverflow {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = (COVER_PT + TITLE_PT + 12.0) * cx.scale;
        Vec2::new(
            constraints.max_size.x.max(COVER_PT * 2.0 * cx.scale),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 120.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.rects.clear();
        if let Some(sel) = self.current() {
            for i in 0..self.items.len() {
                if let Some(r) = self.cover_rect(i, sel) {
                    self.rects.push((i, r));
                }
            }
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListBox);
        node.set_label(self.label.clone());
        if let Some(i) = self.current() {
            node.set_value(format!("{} of {}", i + 1, self.items.len()));
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowLeft" | "ArrowUp" => {
                    self.step(-1);
                    EventResponse::RequestRepaint
                }
                "ArrowRight" | "ArrowDown" => {
                    self.step(1);
                    EventResponse::RequestRepaint
                }
                "Home" => {
                    self.jump_to(0);
                    EventResponse::RequestRepaint
                }
                "End" => {
                    self.jump_to(self.items.len().saturating_sub(1));
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::Scroll { delta, .. } => {
                let d = if delta.y.abs() >= delta.x.abs() {
                    delta.y
                } else {
                    delta.x
                };
                if d > 0.0 {
                    self.step(1);
                } else if d < 0.0 {
                    self.step(-1);
                } else {
                    return EventResponse::Ignored;
                }
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                // Click a side cover to select it (center is already selected).
                for (i, r) in &self.rects {
                    if r.contains(*position) && Some(*i) != self.current() {
                        self.selected = *i;
                        self.selected_out = Some(*i);
                        return EventResponse::Handled;
                    }
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let Some(sel) = self.current() else {
            return;
        };
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(self.bounds.min_x()),
                f64::from(self.bounds.min_y()),
                f64::from(self.bounds.max_x()),
                f64::from(self.bounds.max_y()),
            ),
            cx.color(TokenKey::BackgroundColor, FACE),
        );
        let edge = cx.color(TokenKey::DividerColor, EDGE);
        // Paint far-to-near so the selected cover fronts the rest.
        for (i, r) in self.rects.iter().rev() {
            let item = &self.items[*i];
            let kr = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            cx.list.push_fill_rect(kr, item.color);
            let is_sel = *i == sel;
            cx.list
                .push_stroke_rect(kr, if is_sel { 1.5 } else { 0.8 } * s, edge);
            // Label strip inside each cover's bottom.
            let strip = kurbo::Rect::new(kr.x0, kr.y1 - f64::from(18.0 * s), kr.x1, kr.y1);
            cx.list.push_fill_rect(strip, [0, 0, 0, 120]);
            let o = kurbo::Point::new(kr.x0 + f64::from(6.0 * s), kr.y1 - f64::from(6.0 * s));
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kr,
                o,
                &item.label,
                TITLE_PT * s * (if is_sel { 1.0 } else { 0.8 }),
                cx.color(TokenKey::TextColor, TEXT),
            );
        }
        // Focused title under the arc.
        if self.show_title {
            if let Some(item) = self.items.get(sel) {
                let w = painter
                    .and_then(|p| p.measure_text(&item.label, TITLE_PT * s))
                    .unwrap_or(item.label.len() as f32 * TITLE_PT * 0.6 * s);
                let o = kurbo::Point::new(
                    f64::from(self.bounds.min_x() + self.bounds.width() / 2.0 - w / 2.0),
                    f64::from(self.bounds.max_y() - 6.0 * s),
                );
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    o,
                    &item.label,
                    TITLE_PT * s,
                    cx.color(TokenKey::TextMutedColor, MUTED),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> Coverflow {
        Coverflow::new()
            .item(Thumbnail::new("A", [255, 0, 0, 255]))
            .item(Thumbnail::new("B", [0, 255, 0, 255]))
            .item(Thumbnail::new("C", [0, 0, 255, 255]))
    }

    fn laid_out(c: &mut Coverflow) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 500.0, 180.0));
    }

    fn ev(c: &mut Coverflow, e: &WidgetEvent) -> EventResponse {
        c.event(&mut EventContext {
            event: e,
            bounds: c.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn arrows_step_and_park() {
        let mut c = fixture();
        laid_out(&mut c);
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        assert_eq!(c.current(), Some(1));
        assert_eq!(c.take_selected(), Some(1));
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: "ArrowLeft".to_string(),
                repeat: false,
            },
        );
        assert_eq!(c.current(), Some(0));
    }

    #[test]
    fn step_clamps_at_edges() {
        let mut c = fixture();
        laid_out(&mut c);
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: "ArrowLeft".to_string(),
                repeat: false,
            },
        );
        assert_eq!(c.current(), Some(0));
        assert_eq!(c.take_selected(), None);
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: "End".to_string(),
                repeat: false,
            },
        );
        assert_eq!(c.current(), Some(2));
    }

    #[test]
    fn scroll_moves() {
        let mut c = fixture();
        laid_out(&mut c);
        ev(
            &mut c,
            &WidgetEvent::Scroll {
                delta: Vec2::new(0.0, 1.0),
                position: Vec2::new(250.0, 90.0),
            },
        );
        assert_eq!(c.current(), Some(1));
    }

    #[test]
    fn click_side_cover_selects() {
        let mut c = fixture();
        laid_out(&mut c);
        // Find the rect for item 1 and click it.
        let r = c
            .rects
            .iter()
            .find(|(i, _)| *i == 1)
            .map(|(_, r)| *r)
            .unwrap();
        ev(
            &mut c,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
        );
        assert_eq!(c.current(), Some(1));
        assert_eq!(c.take_selected(), Some(1));
    }

    #[test]
    fn far_items_cull() {
        let mut c = Coverflow::new();
        for i in 0..10 {
            c = c.item(Thumbnail::new(format!("{i}"), [1; 4]));
        }
        laid_out(&mut c);
        // Selected 0: only items 0..=3 visible.
        assert_eq!(c.rects.len(), 4);
    }

    #[test]
    fn paint_without_painter() {
        let mut c = fixture();
        laid_out(&mut c);
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
    }
}
