//! `Legend` — a chart series key: colored swatches with labels in
//! a wrapping flow (matplotlib / ECharts legend idiom).
//!
//! Entries carry a color and label; `dimmed` entries paint at
//! reduced opacity with a struck-through swatch — the standard
//! "series hidden" affordance. Clicking an entry toggles its dim
//! state and parks the index in [`Legend::take_toggled`] so the
//! host can show or hide the matching series. Left/Right arrows
//! move focus, Space toggles.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::legend::Legend;
//!
//! let l = Legend::new().entry("Alpha", [96, 165, 250, 255]);
//! assert_eq!(l.entry_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const SWATCH_PT: f32 = 12.0;
const GAP_PT: f32 = 6.0;
const ITEM_GAP_PT: f32 = 16.0;
const ROW_PT: f32 = 20.0;
const FONT_PT: f32 = 12.0;

const FG: [u8; 4] = [210, 212, 220, 255];
const DIM_FG: [u8; 4] = [120, 122, 130, 255];
const FOCUS: [u8; 4] = [96, 165, 250, 255];

/// One legend entry — swatch color + label + dim state.
///
/// ```
/// use martensite::widgets::legend::LegendEntry;
///
/// let e = LegendEntry::new("Series", [255, 0, 0, 255]);
/// assert!(!e.dimmed);
/// ```
#[derive(Clone, Debug)]
pub struct LegendEntry {
    /// Row label.
    pub label: String,
    /// Swatch color.
    pub color: [u8; 4],
    /// Hidden-series affordance (struck swatch, dimmed text).
    pub dimmed: bool,
}

impl LegendEntry {
    /// A visible entry.
    ///
    /// ```
    /// use martensite::widgets::legend::LegendEntry;
    ///
    /// assert_eq!(LegendEntry::new("A", [0, 0, 0, 255]).label, "A");
    /// ```
    pub fn new(label: impl Into<String>, color: [u8; 4]) -> Self {
        Self {
            label: label.into(),
            color,
            dimmed: false,
        }
    }
}

/// A chart legend — see the module docs.
///
/// ```
/// use martensite::widgets::legend::Legend;
///
/// assert_eq!(Legend::new().entry_count(), 0);
/// ```
pub struct Legend {
    /// Accessibility label.
    pub label: String,
    entries: Vec<LegendEntry>,
    /// Row layout computed in `layout` — `Vec<Rect>` per entry.
    rects: Vec<Rect>,
    focus: usize,
    toggled: Option<usize>,
    bounds: Rect,
    scale: f32,
    text_painter: Option<SharedTextPainter>,
}

impl std::fmt::Debug for Legend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Legend")
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl Default for Legend {
    fn default() -> Self {
        Self::new()
    }
}

impl Legend {
    /// An empty legend.
    ///
    /// ```
    /// use martensite::widgets::legend::Legend;
    ///
    /// assert_eq!(Legend::new().entry_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Legend".to_string(),
            entries: Vec::new(),
            rects: Vec::new(),
            focus: 0,
            toggled: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::legend::Legend;
    ///
    /// assert_eq!(Legend::new().label("CPU").label, "CPU");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Appends a visible entry.
    ///
    /// ```
    /// use martensite::widgets::legend::Legend;
    ///
    /// let l = Legend::new().entry("A", [1, 2, 3, 255]).entry("B", [4, 5, 6, 255]);
    /// assert_eq!(l.entry_count(), 2);
    /// ```
    pub fn entry(mut self, label: impl Into<String>, color: [u8; 4]) -> Self {
        self.entries.push(LegendEntry::new(label, color));
        self
    }

    /// Replaces the entry list.
    ///
    /// ```
    /// use martensite::widgets::legend::{Legend, LegendEntry};
    ///
    /// let l = Legend::new().entries(vec![LegendEntry::new("S", [0, 0, 0, 255])]);
    /// assert_eq!(l.entry_count(), 1);
    /// ```
    pub fn entries(mut self, entries: Vec<LegendEntry>) -> Self {
        self.entries = entries;
        self.focus = self.focus.min(self.entries.len().saturating_sub(1));
        self
    }

    /// Optional painter override (tests / headless).
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Number of entries.
    ///
    /// ```
    /// use martensite::widgets::legend::Legend;
    ///
    /// assert_eq!(Legend::new().entry_count(), 0);
    /// ```
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Entry accessor.
    ///
    /// ```
    /// use martensite::widgets::legend::Legend;
    ///
    /// let l = Legend::new().entry("Alpha", [9, 9, 9, 255]);
    /// assert_eq!(l.entry_at(0).unwrap().label, "Alpha");
    /// ```
    pub fn entry_at(&self, index: usize) -> Option<&LegendEntry> {
        self.entries.get(index)
    }

    /// Whether entry `index` is dimmed (hidden series).
    ///
    /// ```
    /// use martensite::widgets::legend::Legend;
    ///
    /// assert!(!Legend::new().entry("A", [0, 0, 0, 255]).is_dimmed(0));
    /// ```
    pub fn is_dimmed(&self, index: usize) -> bool {
        self.entries.get(index).is_some_and(|e| e.dimmed)
    }

    /// Sets an entry's dim state.
    ///
    /// ```
    /// use martensite::widgets::legend::Legend;
    ///
    /// let mut l = Legend::new().entry("A", [0, 0, 0, 255]);
    /// l.set_dimmed(0, true);
    /// assert!(l.is_dimmed(0));
    /// ```
    pub fn set_dimmed(&mut self, index: usize, dimmed: bool) {
        if let Some(e) = self.entries.get_mut(index) {
            e.dimmed = dimmed;
        }
    }

    /// The focused entry index (keyboard focus ring).
    ///
    /// ```
    /// use martensite::widgets::legend::Legend;
    ///
    /// assert_eq!(Legend::new().focused(), 0);
    /// ```
    pub fn focused(&self) -> usize {
        self.focus
    }

    /// Drains the last toggled entry index.
    ///
    /// ```
    /// use martensite::widgets::legend::Legend;
    ///
    /// assert_eq!(Legend::new().take_toggled(), None);
    /// ```
    pub fn take_toggled(&mut self) -> Option<usize> {
        self.toggled.take()
    }

    /// Entry hit-test.
    fn entry_at_point(&self, p: Vec2) -> Option<usize> {
        self.rects.iter().position(|r| r.contains(p))
    }

    /// Toggles an entry's dim state and parks the index.
    fn toggle(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries[index].dimmed = !self.entries[index].dimmed;
            self.toggled = Some(index);
        }
    }
}

impl Widget for Legend {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // One row at natural height; entries wrap in `layout` when
        // narrower.
        Vec2::new(
            constraints.max_size.x.min(cx.pt(400.0)).max(0.0),
            cx.pt(ROW_PT),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(40.0, ROW_PT)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, None);
        let size = FONT_PT * s;
        let swatch = SWATCH_PT * s;
        let gap = GAP_PT * s;
        let item_gap = ITEM_GAP_PT * s;
        let row_h = ROW_PT * s;
        self.rects.clear();
        let mut x = bounds.min_x();
        let mut y = bounds.min_y();
        for entry in &self.entries {
            let tw = painter
                .and_then(|p| p.measure_text(&entry.label, size))
                .unwrap_or(entry.label.chars().count() as f32 * size * 0.55);
            let w = swatch + gap + tw;
            if x + w > bounds.max_x() && x > bounds.min_x() {
                x = bounds.min_x();
                y += row_h;
            }
            self.rects.push(Rect::new(x, y, w, row_h));
            x += w + item_gap;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListBox);
        node.set_label(format!("{} — {} entries", self.label, self.entries.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self.entry_at_point(*position) {
                    self.focus = i;
                    self.toggle(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowLeft" | "ArrowUp" => {
                    self.focus = self.focus.saturating_sub(1);
                    EventResponse::RequestRepaint
                }
                "ArrowRight" | "ArrowDown" => {
                    self.focus = (self.focus + 1).min(self.entries.len().saturating_sub(1));
                    EventResponse::RequestRepaint
                }
                " " | "Enter" => {
                    self.toggle(self.focus);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let swatch = SWATCH_PT * s;
        let gap = GAP_PT * s;
        let size = FONT_PT * s;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let fg = cx.color(TokenKey::TextColor, FG);
        let dim = cx.color(TokenKey::TextMutedColor, DIM_FG);
        let focus_ring = cx.color(TokenKey::AccentColor, FOCUS);
        for (i, (entry, rect)) in self.entries.iter().zip(self.rects.iter()).enumerate() {
            if i == self.focus {
                let pad = 2.0 * s;
                cx.list.push_stroke_rect(
                    kurbo::Rect::new(
                        f64::from(rect.min_x() - pad),
                        f64::from(rect.min_y() + pad),
                        f64::from(rect.max_x() + pad),
                        f64::from(rect.max_y() - pad),
                    ),
                    s,
                    focus_ring,
                );
            }
            // Swatch.
            let sy = rect.min_y() + (rect.height() - swatch) / 2.0;
            let mut color = entry.color;
            if entry.dimmed {
                color[3] /= 3;
            }
            let sw = kurbo::Rect::new(
                f64::from(rect.min_x()),
                f64::from(sy),
                f64::from(rect.min_x() + swatch),
                f64::from(sy + swatch),
            );
            cx.list
                .push_fill_shape(sw, &martensite_core::shape::Shape::rounded(2.0 * s), color);
            if entry.dimmed {
                // Diagonal strike through the swatch.
                let mut strike = kurbo::BezPath::new();
                strike.move_to(kurbo::Point::new(
                    f64::from(rect.min_x()),
                    f64::from(sy + swatch),
                ));
                strike.line_to(kurbo::Point::new(
                    f64::from(rect.min_x() + swatch),
                    f64::from(sy),
                ));
                cx.list.push_stroke_path(strike, s, dim);
            }
            // Label.
            let label_color = if entry.dimmed { dim } else { fg };
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(rect.min_x()),
                    f64::from(rect.min_y()),
                    f64::from(rect.max_x()),
                    f64::from(rect.max_y()),
                ),
                kurbo::Point::new(
                    f64::from(rect.min_x() + swatch + gap),
                    f64::from(rect.min_y() + (rect.height() - size * 1.2) / 2.0),
                ),
                &entry.label,
                size,
                label_color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn legend() -> Legend {
        Legend::new()
            .entry("Alpha", [255, 0, 0, 255])
            .entry("Beta", [0, 255, 0, 255])
            .entry("Gamma", [0, 0, 255, 255])
    }

    fn laid_out(l: &mut Legend) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        l.layout(&mut cx, Rect::new(0.0, 0.0, 400.0, 60.0));
    }

    fn ev(l: &mut Legend, e: &WidgetEvent) -> EventResponse {
        l.event(&mut EventContext {
            event: e,
            bounds: l.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn entries_lay_out_in_rows() {
        let mut l = legend();
        laid_out(&mut l);
        assert_eq!(l.rects.len(), 3);
        assert!(l.rects[0].min_x() < l.rects[1].min_x());
    }

    #[test]
    fn narrow_width_wraps() {
        let mut l = legend();
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        l.layout(&mut cx, Rect::new(0.0, 0.0, 60.0, 200.0));
        assert!(l.rects.iter().any(|r| r.min_y() > 0.0));
    }

    #[test]
    fn click_toggles_dim_and_parks() {
        let mut l = legend();
        laid_out(&mut l);
        let r = l.rects[1];
        let p = Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0);
        ev(
            &mut l,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: p,
                count: 1,
            },
        );
        assert!(l.is_dimmed(1));
        assert_eq!(l.take_toggled(), Some(1));
        ev(
            &mut l,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: p,
                count: 1,
            },
        );
        assert!(!l.is_dimmed(1));
    }

    #[test]
    fn arrows_and_space() {
        let mut l = legend();
        laid_out(&mut l);
        ev(
            &mut l,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        ev(
            &mut l,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        assert_eq!(l.focused(), 2);
        ev(
            &mut l,
            &WidgetEvent::KeyPressed {
                key: " ".to_string(),
                repeat: false,
            },
        );
        assert!(l.is_dimmed(2));
        assert_eq!(l.take_toggled(), Some(2));
    }

    #[test]
    fn paint_without_painter() {
        let mut l = legend();
        laid_out(&mut l);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        l.paint(&mut PaintContext {
            list: &mut list,
            bounds: l.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
