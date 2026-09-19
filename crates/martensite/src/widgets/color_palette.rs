//! `ColorPalette` — preset color-swatch grid.
//!
//! The Ant `ColorPicker` presets row / `NSColorList` pattern: a fixed
//! grid of swatches where click selects one (accent ring) and parks
//! it in [`ColorPalette::take_selected`]. Unlike `ColorPicker`'s
//! free-form spectrum, a palette is the curated preset list — theme
//! colors, brand swatches, "recently used".
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::color_palette::ColorPalette;
//!
//! let mut p = ColorPalette::new()
//!     .swatches([[255, 0, 0, 255], [0, 128, 0, 255], [0, 0, 255, 255]]);
//! assert_eq!(p.swatch_count(), 3);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

/// Selection ring.
const ACCENT: TokenKey = TokenKey::AccentColor;
/// Hover ring.
const HOVER: TokenKey = TokenKey::TextMutedColor;
/// Swatch edge.
const EDGE: TokenKey = TokenKey::DividerColor;
/// Swatch size (logical points).
const SWATCH: f32 = 24.0;
/// Swatch gap (logical points).
const GAP: f32 = 6.0;
/// Swatches per row.
const PER_ROW: usize = 8;

/// A preset swatch grid — see the module docs. Leaf widget.
///
/// # Examples
///
/// ```
/// use martensite::widgets::color_palette::ColorPalette;
/// use martensite::core::Widget;
///
/// let mut p = ColorPalette::new();
/// assert_eq!(p.child_count(), 0);
/// ```
pub struct ColorPalette {
    label: String,
    enabled: bool,
    /// Swatch colors as `[r, g, b, a]`.
    swatches: Vec<[u8; 4]>,
    /// Selected index.
    selected: Option<usize>,
    /// Parked selection for `take_selected`.
    pending: Option<usize>,
    /// Hover index.
    hover: Option<usize>,
    /// Cell rects from the last layout (widget-local).
    cells: Vec<Rect>,
    /// Columns per row actually used (layout may wrap narrower).
    per_row: usize,
}

impl ColorPalette {
    /// An empty palette (8 swatches per row).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::color_palette::ColorPalette;
    ///
    /// let p = ColorPalette::new();
    /// assert_eq!(p.swatch_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Color palette".into(),
            enabled: true,
            swatches: Vec::new(),
            selected: None,
            pending: None,
            hover: None,
            cells: Vec::new(),
            per_row: PER_ROW,
        }
    }

    /// Install the swatches (`[r, g, b, a]`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::color_palette::ColorPalette;
    ///
    /// let p = ColorPalette::new().swatches([[0, 0, 0, 255], [255, 255, 255, 255]]);
    /// assert_eq!(p.swatch_count(), 2);
    /// ```
    pub fn swatches(mut self, swatches: impl IntoIterator<Item = [u8; 4]>) -> Self {
        self.swatches = swatches.into_iter().collect();
        self
    }

    /// Set the accessibility label (default `"Color palette"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::color_palette::ColorPalette;
    ///
    /// let p = ColorPalette::new().label("Theme colors");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Enable or disable interaction (default `true`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::color_palette::ColorPalette;
    ///
    /// let p = ColorPalette::new().enabled(false);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Swatch count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::color_palette::ColorPalette;
    ///
    /// assert_eq!(ColorPalette::new().swatch_count(), 0);
    /// ```
    pub fn swatch_count(&self) -> usize {
        self.swatches.len()
    }

    /// The selected swatch index.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::color_palette::ColorPalette;
    ///
    /// assert_eq!(ColorPalette::new().selected_index(), None);
    /// ```
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// The selected swatch's color, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::color_palette::ColorPalette;
    ///
    /// let mut p = ColorPalette::new().swatches([[9, 9, 9, 255]]);
    /// p.select(0);
    /// assert_eq!(p.selected_color(), Some([9, 9, 9, 255]));
    /// ```
    pub fn selected_color(&self) -> Option<[u8; 4]> {
        self.selected.and_then(|i| self.swatches.get(i).copied())
    }

    /// Select `index` programmatically (out-of-range clears).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::color_palette::ColorPalette;
    ///
    /// let mut p = ColorPalette::new().swatches([[1, 2, 3, 255]]);
    /// p.select(0);
    /// p.select(9);
    /// assert_eq!(p.selected_index(), None);
    /// ```
    pub fn select(&mut self, index: usize) {
        self.selected = (index < self.swatches.len()).then_some(index);
    }

    /// Drain the parked user selection — `(index, [r,g,b,a])` —
    /// one-shot.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::color_palette::ColorPalette;
    ///
    /// let mut p = ColorPalette::new();
    /// assert_eq!(p.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<(usize, [u8; 4])> {
        self.pending
            .take()
            .and_then(|i| self.swatches.get(i).copied().map(|c| (i, c)))
    }

    /// The cell index at a widget-local point.
    fn hit(&self, local: Vec2) -> Option<usize> {
        self.cells.iter().position(|r| r.contains(local))
    }
}

impl Default for ColorPalette {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ColorPalette {
    fn measure(&mut self, _cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        let rows = self.swatches.len().div_ceil(PER_ROW).max(1) as f32;
        Vec2::new(
            PER_ROW as f32 * (SWATCH + GAP) - GAP,
            rows * (SWATCH + GAP) - GAP,
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let sw = cx.pt(SWATCH);
        let gap = cx.pt(GAP);
        // Wrap to however many swatches actually fit per row.
        self.per_row = ((bounds.width() + gap) / (sw + gap)).floor().max(1.0) as usize;
        self.cells = (0..self.swatches.len())
            .map(|i| {
                let col = (i % self.per_row) as f32;
                let row = (i / self.per_row) as f32;
                Rect::new(col * (sw + gap), row * (sw + gap), sw, sw)
            })
            .collect();
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let accent = cx.color(ACCENT, [50, 115, 230, 255]);
        let hover = cx.color(HOVER, [150, 150, 158, 255]);
        let edge = cx.color(EDGE, [205, 207, 212, 255]);
        let radius = cx.pt(4.0);
        for (i, (cell, color)) in self.cells.iter().zip(self.swatches.iter()).enumerate() {
            let r = kurbo::Rect::new(
                f64::from(b.min_x() + cell.min_x()),
                f64::from(b.min_y() + cell.min_y()),
                f64::from(b.min_x() + cell.max_x()),
                f64::from(b.min_y() + cell.max_y()),
            );
            let shape = martensite_core::shape::Shape::rounded(radius);
            cx.list.push_fill_shape(r, &shape, *color);
            cx.list.push_stroke_shape(r, &shape, cx.pt(0.75), edge);
            if self.selected == Some(i) {
                cx.list.push_stroke_shape(r, &shape, cx.pt(2.5), accent);
            } else if self.enabled && self.hover == Some(i) {
                cx.list.push_stroke_shape(r, &shape, cx.pt(1.5), hover);
            }
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let local = *position - cx.bounds.origin;
                let hit = self.hit(local);
                if hit != self.hover {
                    self.hover = hit;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerReleased { position, button }
                if *button == martensite_core::PointerButton::Primary =>
            {
                let local = *position - cx.bounds.origin;
                if let Some(i) = self.hit(local) {
                    self.selected = Some(i);
                    self.pending = Some(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hover.take().is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.as_str());
        if let Some(c) = self.selected_color() {
            node.set_value(format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]));
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(SWATCH, SWATCH)).with_policy(UnderflowPolicy::Lint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PointerButton};

    fn pal(n: usize) -> ColorPalette {
        ColorPalette::new().swatches((0..n).map(|i| [i as u8, 0, 0, 255]))
    }

    fn lay(w: &mut ColorPalette) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, 240.0, 120.0));
    }

    fn ev(w: &mut ColorPalette, e: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: e,
            bounds: Rect::new(0.0, 0.0, 240.0, 120.0),
            scale: 1.0,
        };
        w.event(&mut cx)
    }

    #[test]
    fn wraps_at_eight_per_row() {
        let mut p = pal(10);
        lay(&mut p);
        // 240px fits 8 per row (8*30-6=234).
        assert_eq!(p.per_row, 8);
        assert!(p.cells[8].min_y() > 0.0, "swatch 9 wraps to row 2");
        assert_eq!(p.cells[8].min_x(), 0.0);
    }

    #[test]
    fn click_selects_and_parks_rgba() {
        let mut p = pal(4);
        lay(&mut p);
        let c = p.cells[1];
        ev(
            &mut p,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(c.min_x() + 4.0, c.min_y() + 4.0),
                button: PointerButton::Primary,
            },
        );
        assert_eq!(p.selected_index(), Some(1));
        assert_eq!(p.take_selected(), Some((1, [1, 0, 0, 255])));
        assert_eq!(p.take_selected(), None);
    }

    #[test]
    fn programmatic_select_clamps() {
        let mut p = pal(2);
        p.select(5);
        assert_eq!(p.selected_index(), None);
        p.select(1);
        assert_eq!(p.selected_color(), Some([1, 0, 0, 255]));
    }

    #[test]
    fn narrow_layout_reflows() {
        let mut p = pal(6);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        // 70px fits 2 per row (2*30-6=54).
        p.layout(&mut cx, Rect::new(0.0, 0.0, 70.0, 120.0));
        assert_eq!(p.per_row, 2);
        assert_eq!(p.cells[5].min_y(), 2.0 * (SWATCH + GAP));
    }

    #[test]
    fn disabled_is_inert() {
        let mut p = pal(2).enabled(false);
        lay(&mut p);
        let c = p.cells[0];
        let r = ev(
            &mut p,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(c.min_x() + 4.0, c.min_y() + 4.0),
                button: PointerButton::Primary,
            },
        );
        assert_eq!(r, EventResponse::Ignored);
        assert_eq!(p.take_selected(), None);
    }
}
