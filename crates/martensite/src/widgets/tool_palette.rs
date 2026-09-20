//! `ToolPalette` — a compact grid of single-select tool buttons
//! (Photoshop tools palette / paint-app toolbox idiom).
//!
//! Distinct from [`Toolbar`](crate::widgets::Toolbar): a palette
//! is a *modal tool choice* — one tool is always active (accent
//! face) and clicks park the index in
//! [`ToolPalette::take_selected`]. Arrows navigate the grid,
//! `Home`/`End` jump. Each [`ToolItem`] is a glyph + label; the
//! label feeds accessibility and can be shown under the glyph.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::tool_palette::{ToolItem, ToolPalette};
//!
//! let p = ToolPalette::new()
//!     .tool(ToolItem::new("✏", "Pencil"))
//!     .tool(ToolItem::new("🧽", "Eraser"));
//! assert_eq!(p.tool_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const CELL_PT: f32 = 36.0;
const GAP_PT: f32 = 4.0;
const PAD_PT: f32 = 4.0;
const GLYPH_PT: f32 = 16.0;
const LABEL_PT: f32 = 9.0;
const RADIUS_PT: f32 = 6.0;

const FACE: [u8; 4] = [36, 38, 44, 255];
const HOT: [u8; 4] = [50, 52, 60, 255];
const SEL: [u8; 4] = [44, 62, 92, 255];
const ACCENT: [u8; 4] = [88, 130, 247, 255];
const TEXT: [u8; 4] = [220, 222, 228, 255];
const MUTED: [u8; 4] = [139, 148, 158, 255];

/// One palette tool — glyph + accessible label.
///
/// ```
/// use martensite::widgets::tool_palette::ToolItem;
///
/// assert_eq!(ToolItem::new("✏", "Pencil").label, "Pencil");
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct ToolItem {
    /// Display glyph (emoji or icon char).
    pub glyph: String,
    /// Tool name — accessibility + optional caption.
    pub label: String,
}

impl ToolItem {
    /// New tool.
    ///
    /// ```
    /// use martensite::widgets::tool_palette::ToolItem;
    ///
    /// assert_eq!(ToolItem::new("x", "Cut").glyph, "x");
    /// ```
    pub fn new(glyph: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            glyph: glyph.into(),
            label: label.into(),
        }
    }
}

/// A modal tool grid — see the module docs.
///
/// ```
/// use martensite::widgets::tool_palette::ToolPalette;
///
/// assert_eq!(ToolPalette::new().tool_count(), 0);
/// ```
pub struct ToolPalette {
    /// Accessibility label.
    pub label: String,
    /// Grid columns.
    pub columns: usize,
    /// Show label captions under glyphs.
    pub show_labels: bool,
    tools: Vec<ToolItem>,
    selected: usize,
    selected_out: Option<usize>,
    hovered: Option<usize>,
    rects: Vec<Rect>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for ToolPalette {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolPalette")
            .field("tools", &self.tools)
            .field("selected", &self.selected)
            .finish()
    }
}

impl Default for ToolPalette {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolPalette {
    /// Empty palette, 2 columns.
    ///
    /// ```
    /// use martensite::widgets::tool_palette::ToolPalette;
    ///
    /// assert_eq!(ToolPalette::new().columns, 2);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Tools".to_string(),
            columns: 2,
            show_labels: false,
            tools: Vec::new(),
            selected: 0,
            selected_out: None,
            hovered: None,
            rects: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a tool.
    ///
    /// ```
    /// use martensite::widgets::tool_palette::{ToolItem, ToolPalette};
    ///
    /// assert_eq!(ToolPalette::new().tool(ToolItem::new("x", "Cut")).tool_count(), 1);
    /// ```
    pub fn tool(mut self, tool: ToolItem) -> Self {
        self.tools.push(tool);
        self
    }

    /// Column-count builder.
    ///
    /// ```
    /// use martensite::widgets::tool_palette::ToolPalette;
    ///
    /// assert_eq!(ToolPalette::new().columns(4).columns, 4);
    /// ```
    pub fn columns(mut self, columns: usize) -> Self {
        self.columns = columns.max(1);
        self
    }

    /// Initial selection.
    ///
    /// ```
    /// use martensite::widgets::tool_palette::{ToolItem, ToolPalette};
    ///
    /// assert_eq!(ToolPalette::new().tool(ToolItem::new("x", "C")).selected(0).current(), Some(0));
    /// ```
    pub fn selected(mut self, index: usize) -> Self {
        self.selected = index;
        self
    }

    /// Label captions builder.
    ///
    /// ```
    /// use martensite::widgets::tool_palette::ToolPalette;
    ///
    /// assert!(ToolPalette::new().show_labels(true).show_labels);
    /// ```
    pub fn show_labels(mut self, show: bool) -> Self {
        self.show_labels = show;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::tool_palette::ToolPalette;
    ///
    /// assert_eq!(ToolPalette::new().label("Brushes").label, "Brushes");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter for glyphs/labels.
    ///
    /// ```no_run
    /// use martensite::widgets::tool_palette::ToolPalette;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _p = ToolPalette::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Tool count.
    ///
    /// ```
    /// use martensite::widgets::tool_palette::ToolPalette;
    ///
    /// assert_eq!(ToolPalette::new().tool_count(), 0);
    /// ```
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Active tool index.
    ///
    /// ```
    /// use martensite::widgets::tool_palette::ToolPalette;
    ///
    /// assert_eq!(ToolPalette::new().current(), None);
    /// ```
    pub fn current(&self) -> Option<usize> {
        (!self.tools.is_empty()).then(|| self.selected.min(self.tools.len() - 1))
    }

    /// Sets the active tool (host-driven, no seam).
    ///
    /// ```
    /// use martensite::widgets::tool_palette::{ToolItem, ToolPalette};
    ///
    /// let mut p = ToolPalette::new().tool(ToolItem::new("a", "A")).tool(ToolItem::new("b", "B"));
    /// p.set_selected(1);
    /// assert_eq!(p.current(), Some(1));
    /// ```
    pub fn set_selected(&mut self, index: usize) {
        if !self.tools.is_empty() {
            self.selected = index.min(self.tools.len() - 1);
        }
    }

    /// Drains the last selected index.
    ///
    /// ```
    /// use martensite::widgets::tool_palette::ToolPalette;
    ///
    /// let mut p = ToolPalette::new();
    /// assert_eq!(p.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.selected_out.take()
    }

    /// Cell hit-test.
    fn cell_at(&self, p: Vec2) -> Option<usize> {
        self.rects.iter().position(|r| r.contains(p))
    }

    /// Moves the selection in grid terms.
    fn step(&mut self, d: isize) {
        if self.tools.is_empty() {
            return;
        }
        let next = (self.selected as isize)
            .saturating_add(d)
            .clamp(0, self.tools.len() as isize - 1) as usize;
        if next != self.selected {
            self.selected = next;
            self.selected_out = Some(next);
        }
    }
}

impl Widget for ToolPalette {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let cell_h = CELL_PT * s + if self.show_labels { LABEL_PT * s } else { 0.0 };
        let rows = self.tools.len().div_ceil(self.columns) as f32;
        let w = self.columns as f32 * (CELL_PT + GAP_PT) * s + PAD_PT * 2.0 * s;
        let h = (rows * (cell_h + GAP_PT * s) + PAD_PT * 2.0 * s).max(CELL_PT * s);
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(40.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.rects.clear();
        let s = cx.scale;
        let cell_h = CELL_PT * s + if self.show_labels { LABEL_PT * s } else { 0.0 };
        let pitch_x = CELL_PT * s + GAP_PT * s;
        let pitch_y = cell_h + GAP_PT * s;
        let x0 = bounds.min_x() + PAD_PT * s;
        let y0 = bounds.min_y() + PAD_PT * s;
        for i in 0..self.tools.len() {
            let col = i % self.columns;
            let row = i / self.columns;
            self.rects.push(Rect::new(
                x0 + col as f32 * pitch_x,
                y0 + row as f32 * pitch_y,
                CELL_PT * s,
                cell_h,
            ));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::RadioGroup);
        node.set_label(self.label.clone());
        if let Some(i) = self.current() {
            node.set_value(self.tools[i].label.clone());
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let now = self.cell_at(*position);
                if now != self.hovered {
                    self.hovered = now;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(i) = self.cell_at(*position) {
                    if Some(i) != self.current() {
                        self.selected = i;
                        self.selected_out = Some(i);
                    }
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowRight" => {
                    self.step(1);
                    EventResponse::RequestRepaint
                }
                "ArrowLeft" => {
                    self.step(-1);
                    EventResponse::RequestRepaint
                }
                "ArrowDown" => {
                    self.step(self.columns as isize);
                    EventResponse::RequestRepaint
                }
                "ArrowUp" => {
                    self.step(-(self.columns as isize));
                    EventResponse::RequestRepaint
                }
                "Home" => {
                    self.step(-(self.selected as isize));
                    EventResponse::RequestRepaint
                }
                "End" => {
                    self.step(isize::MAX);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let shape = martensite_core::shape::Shape::rounded(RADIUS_PT * s);
        let sel = self.current();
        for (i, rect) in self.rects.iter().enumerate() {
            let is_sel = sel == Some(i);
            let face = if is_sel {
                cx.color(TokenKey::SecondaryColor, SEL)
            } else if self.hovered == Some(i) {
                cx.color(TokenKey::SurfaceColor, HOT)
            } else {
                cx.color(TokenKey::SurfaceColor, FACE)
            };
            cx.list.push_fill_shape(krect(*rect), &shape, face);
            if is_sel {
                let accent = cx.color(TokenKey::AccentColor, ACCENT);
                cx.list
                    .push_stroke_shape(krect(*rect), &shape, 1.2 * s, accent);
            }
            let t = &self.tools[i];
            // Glyph centered (or upper when labels show).
            let gsize = GLYPH_PT * s;
            let gw = painter
                .and_then(|p| p.measure_text(&t.glyph, gsize))
                .unwrap_or(t.glyph.chars().count() as f32 * gsize * 0.6);
            let gy = if self.show_labels {
                rect.min_y() + CELL_PT * s * 0.5
            } else {
                rect.min_y() + rect.height() / 2.0
            };
            let go = kurbo::Point::new(
                f64::from(rect.min_x() + (rect.width() - gw) / 2.0),
                f64::from(gy),
            );
            let color = if is_sel {
                cx.color(TokenKey::AccentColor, ACCENT)
            } else {
                cx.color(TokenKey::TextColor, TEXT)
            };
            crate::text_paint::paint_label(painter, cx.list, go, &t.glyph, gsize, color);
            // Caption.
            if self.show_labels {
                let lsize = LABEL_PT * s;
                let lw = painter
                    .and_then(|p| p.measure_text(&t.label, lsize))
                    .unwrap_or(t.label.len() as f32 * lsize * 0.6);
                let lo = kurbo::Point::new(
                    f64::from(rect.min_x() + (rect.width() - lw) / 2.0),
                    f64::from(rect.max_y() - lsize * 0.4),
                );
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    krect(*rect),
                    lo,
                    &t.label,
                    lsize,
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

    fn fixture() -> ToolPalette {
        ToolPalette::new()
            .tool(ToolItem::new("a", "A"))
            .tool(ToolItem::new("b", "B"))
            .tool(ToolItem::new("c", "C"))
            .tool(ToolItem::new("d", "D"))
    }

    fn laid_out(p: &mut ToolPalette) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        p.layout(&mut cx, Rect::new(0.0, 0.0, 100.0, 100.0));
    }

    fn ev(p: &mut ToolPalette, e: &WidgetEvent) -> EventResponse {
        p.event(&mut EventContext {
            event: e,
            bounds: p.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn click_selects_and_parks() {
        let mut p = fixture();
        laid_out(&mut p);
        let r = p.rects[2];
        ev(
            &mut p,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
        );
        assert_eq!(p.current(), Some(2));
        assert_eq!(p.take_selected(), Some(2));
        // Re-click same cell doesn't re-park.
        ev(
            &mut p,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
        );
        assert_eq!(p.take_selected(), None);
    }

    #[test]
    fn grid_arrows_move() {
        let mut p = fixture(); // 2 cols: a b / c d
        laid_out(&mut p);
        ev(
            &mut p,
            &WidgetEvent::KeyPressed {
                key: "ArrowDown".to_string(),
                repeat: false,
            },
        );
        assert_eq!(p.current(), Some(2));
        ev(
            &mut p,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        assert_eq!(p.current(), Some(3));
        ev(
            &mut p,
            &WidgetEvent::KeyPressed {
                key: "ArrowUp".to_string(),
                repeat: false,
            },
        );
        assert_eq!(p.current(), Some(1));
    }

    #[test]
    fn home_end_jump() {
        let mut p = fixture().selected(1);
        laid_out(&mut p);
        ev(
            &mut p,
            &WidgetEvent::KeyPressed {
                key: "End".to_string(),
                repeat: false,
            },
        );
        assert_eq!(p.current(), Some(3));
        ev(
            &mut p,
            &WidgetEvent::KeyPressed {
                key: "Home".to_string(),
                repeat: false,
            },
        );
        assert_eq!(p.current(), Some(0));
    }

    #[test]
    fn paint_without_painter() {
        let mut p = fixture().show_labels(true);
        laid_out(&mut p);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        p.paint(&mut PaintContext {
            list: &mut list,
            bounds: p.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
