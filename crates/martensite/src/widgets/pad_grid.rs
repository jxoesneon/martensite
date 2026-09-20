//! `PadGrid` — a velocity-pad grid (MPC / Launchpad / drum-machine
//! idiom): a `cols`×`rows` matrix of colored pads that brighten
//! while pressed and park their index in
//! [`PadGrid::take_triggered`] on press.
//!
//! Row-major indexing, `0` at top-left. Pads pulse on `tick` while
//! a `flash` is armed (host-driven "note played" feedback).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::pad_grid::PadGrid;
//!
//! let g = PadGrid::new(4, 4).pad_color(0, [220, 80, 80, 255]);
//! assert_eq!(g.pad_count(), 16);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const PAD_PT: f32 = 40.0;
const GAP_PT: f32 = 6.0;

const FACE: [u8; 4] = [30, 32, 40, 255];
const PAD_OFF: [u8; 4] = [70, 74, 84, 255];

struct Pad {
    color: [u8; 4],
    label: String,
    /// 0..1 flash decay for host-armed pulses.
    flash: f32,
}

/// The pad matrix — see the module docs.
///
/// ```
/// use martensite::widgets::pad_grid::PadGrid;
///
/// assert_eq!(PadGrid::new(2, 2).pad_count(), 4);
/// ```
pub struct PadGrid {
    /// Accessibility label.
    pub label: String,
    /// Column count.
    pub cols: usize,
    /// Row count.
    pub rows: usize,
    pads: Vec<Pad>,
    pressed: Option<usize>,
    triggered: Option<usize>,
    cells: Vec<Rect>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for PadGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PadGrid")
            .field("cols", &self.cols)
            .field("rows", &self.rows)
            .finish()
    }
}

impl PadGrid {
    /// A `cols`×`rows` grid of unlabeled pads.
    ///
    /// ```
    /// use martensite::widgets::pad_grid::PadGrid;
    ///
    /// assert_eq!(PadGrid::new(4, 4).pad_count(), 16);
    /// ```
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Self {
            label: "Pad grid".to_string(),
            cols,
            rows,
            pads: (0..cols * rows)
                .map(|_| Pad {
                    color: PAD_OFF,
                    label: String::new(),
                    flash: 0.0,
                })
                .collect(),
            pressed: None,
            triggered: None,
            cells: Vec::new(),
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets a pad's base color.
    ///
    /// ```
    /// use martensite::widgets::pad_grid::PadGrid;
    ///
    /// let g = PadGrid::new(4, 4).pad_color(3, [255, 0, 0, 255]);
    /// assert_eq!(g.color_at(3), [255, 0, 0, 255]);
    /// ```
    pub fn pad_color(mut self, index: usize, color: [u8; 4]) -> Self {
        if let Some(p) = self.pads.get_mut(index) {
            p.color = color;
        }
        self
    }

    /// Sets a pad's label (shown centered when the painter exists).
    ///
    /// ```
    /// use martensite::widgets::pad_grid::PadGrid;
    ///
    /// assert_eq!(PadGrid::new(2, 2).pad_label(0, "Kick").label_at(0), "Kick");
    /// ```
    pub fn pad_label(mut self, index: usize, label: impl Into<String>) -> Self {
        if let Some(p) = self.pads.get_mut(index) {
            p.label = label.into();
        }
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::pad_grid::PadGrid;
    ///
    /// assert_eq!(PadGrid::new(2, 2).label("Drums").label, "Drums");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Pad count (`cols`×`rows`).
    ///
    /// ```
    /// use martensite::widgets::pad_grid::PadGrid;
    ///
    /// assert_eq!(PadGrid::new(4, 4).pad_count(), 16);
    /// ```
    pub fn pad_count(&self) -> usize {
        self.pads.len()
    }

    /// A pad's base color.
    ///
    /// ```
    /// use martensite::widgets::pad_grid::PadGrid;
    ///
    /// assert_eq!(PadGrid::new(2, 2).color_at(0)[3], 255);
    /// ```
    pub fn color_at(&self, index: usize) -> [u8; 4] {
        self.pads.get(index).map(|p| p.color).unwrap_or(PAD_OFF)
    }

    /// A pad's label.
    ///
    /// ```
    /// use martensite::widgets::pad_grid::PadGrid;
    ///
    /// assert_eq!(PadGrid::new(2, 2).label_at(0), "");
    /// ```
    pub fn label_at(&self, index: usize) -> &str {
        self.pads.get(index).map(|p| p.label.as_str()).unwrap_or("")
    }

    /// Drains the last triggered pad index.
    ///
    /// ```
    /// use martensite::widgets::pad_grid::PadGrid;
    ///
    /// assert_eq!(PadGrid::new(2, 2).take_triggered(), None);
    /// ```
    pub fn take_triggered(&mut self) -> Option<usize> {
        self.triggered.take()
    }

    /// Arms a host-driven flash on a pad (e.g. sequenced playback).
    ///
    /// ```
    /// use martensite::widgets::pad_grid::PadGrid;
    ///
    /// let mut g = PadGrid::new(2, 2);
    /// g.flash(1);
    /// ```
    pub fn flash(&mut self, index: usize) {
        if let Some(p) = self.pads.get_mut(index) {
            p.flash = 1.0;
        }
    }

    fn hit(&self, p: Vec2) -> Option<usize> {
        self.cells.iter().position(|r| r.contains(p))
    }
}

impl Widget for PadGrid {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let w = (PAD_PT * self.cols as f32 + GAP_PT * (self.cols - 1) as f32) * s;
        let h = (PAD_PT * self.rows as f32 + GAP_PT * (self.rows - 1) as f32) * s;
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(48.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        self.cells.clear();
        let cell = PAD_PT * s;
        let gap = GAP_PT * s;
        let gw = cell * self.cols as f32 + gap * (self.cols - 1) as f32;
        let gh = cell * self.rows as f32 + gap * (self.rows - 1) as f32;
        let ox = bounds.min_x() + (bounds.width() - gw) / 2.0;
        let oy = bounds.min_y() + (bounds.height() - gh) / 2.0;
        for r in 0..self.rows {
            for c in 0..self.cols {
                self.cells.push(Rect::new(
                    ox + c as f32 * (cell + gap),
                    oy + r as f32 * (cell + gap),
                    cell,
                    cell,
                ));
            }
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Grid);
        node.set_label(self.label.clone());
        node.set_value(format!("{}×{}", self.cols, self.rows));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self.hit(*position) {
                    self.pressed = Some(i);
                    self.triggered = Some(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if self.pressed.is_some() {
                    let h = self.hit(*position);
                    if h != self.pressed {
                        self.pressed = h;
                        return EventResponse::RequestRepaint;
                    }
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.pressed.take().is_some() {
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, dt: std::time::Duration) -> bool {
        let mut live = false;
        for p in &mut self.pads {
            if p.flash > 0.0 {
                p.flash = (p.flash - dt.as_secs_f32() * 2.5).max(0.0);
                live = true;
            }
        }
        live
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let b = self.bounds;
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            cx.color(TokenKey::BackgroundColor, FACE),
        );
        for (i, cell) in self.cells.iter().enumerate() {
            let p = &self.pads[i];
            let lit = self.pressed == Some(i) || p.flash > 0.0;
            let color = if lit {
                p.color
            } else {
                let f = 0.55;
                [
                    (p.color[0] as f32 * f) as u8,
                    (p.color[1] as f32 * f) as u8,
                    (p.color[2] as f32 * f) as u8,
                    p.color[3],
                ]
            };
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(cell.min_x()),
                    f64::from(cell.min_y()),
                    f64::from(cell.max_x()),
                    f64::from(cell.max_y()),
                ),
                &martensite_core::shape::Shape::rounded(5.0 * s),
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(g: &mut PadGrid) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        g.layout(&mut cx, Rect::new(0.0, 0.0, 200.0, 200.0));
    }

    #[test]
    fn press_triggers_index() {
        let mut g = PadGrid::new(4, 4);
        laid_out(&mut g);
        let c = g.cells[5];
        g.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(c.min_x() + 4.0, c.min_y() + 4.0),
                count: 1,
            },
            bounds: g.bounds,
            scale: 1.0,
        });
        assert_eq!(g.take_triggered(), Some(5));
        assert_eq!(g.take_triggered(), None);
    }

    #[test]
    fn drag_slides_across_pads() {
        let mut g = PadGrid::new(4, 4);
        laid_out(&mut g);
        let c0 = g.cells[0];
        let c1 = g.cells[1];
        g.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(c0.min_x() + 2.0, c0.min_y() + 2.0),
                count: 1,
            },
            bounds: g.bounds,
            scale: 1.0,
        });
        g.event(&mut EventContext {
            event: &WidgetEvent::PointerMoved {
                position: Vec2::new(c1.min_x() + 2.0, c1.min_y() + 2.0),
            },
            bounds: g.bounds,
            scale: 1.0,
        });
        assert_eq!(g.pressed, Some(1));
    }

    #[test]
    fn flash_decays_on_tick() {
        let mut g = PadGrid::new(2, 2);
        g.flash(0);
        assert!(g.tick(std::time::Duration::from_millis(100)));
    }

    #[test]
    fn paint_without_painter() {
        let mut g = PadGrid::new(4, 4).pad_color(0, [200, 60, 60, 255]);
        laid_out(&mut g);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        g.paint(&mut PaintContext {
            list: &mut list,
            bounds: g.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
