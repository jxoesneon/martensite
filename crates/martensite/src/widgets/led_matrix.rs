//! `LedMatrix` — a dot-matrix LED display (departure-board /
//! marquee-sign idiom).
//!
//! A `cols × rows` grid of cells; `set`/`clear`/`toggle` flip
//! individual dots and `fill_all`/`clear_all` reset the board.
//! Dots paint as circles — lit dots in the accent color, unlit
//! in a faint ghost — giving the classic LED-panel look.
//! Display-only.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::led_matrix::LedMatrix;
//!
//! let mut m = LedMatrix::new(8, 8);
//! m.set(3, 4, true);
//! assert!(m.get(3, 4));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

const CELL_PT: f32 = 10.0;

const FACE: [u8; 4] = [24, 24, 28, 255];
const OFF: [u8; 4] = [52, 52, 58, 255];
const ON: [u8; 4] = [255, 170, 60, 255];

/// A dot-matrix LED display — see the module docs.
///
/// ```
/// use martensite::widgets::led_matrix::LedMatrix;
///
/// let m = LedMatrix::new(16, 8);
/// assert_eq!(m.cols(), 16);
/// ```
#[derive(Debug)]
pub struct LedMatrix {
    /// Accessibility label.
    pub label: String,
    cols: usize,
    rows: usize,
    cells: Vec<bool>,
    /// Dot color override (accent default).
    pub on_color: Option<[u8; 4]>,
    bounds: Rect,
    scale: f32,
}

impl LedMatrix {
    /// Creates a `cols × rows` blank matrix.
    ///
    /// ```
    /// use martensite::widgets::led_matrix::LedMatrix;
    ///
    /// let m = LedMatrix::new(5, 7);
    /// assert_eq!(m.rows(), 7);
    /// ```
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            label: "LED matrix".to_string(),
            cols: cols.max(1),
            rows: rows.max(1),
            cells: vec![false; cols.max(1) * rows.max(1)],
            on_color: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::led_matrix::LedMatrix;
    ///
    /// assert_eq!(LedMatrix::new(2, 2).label("Sign").label, "Sign");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Lit-dot color override.
    ///
    /// ```
    /// use martensite::widgets::led_matrix::LedMatrix;
    ///
    /// assert!(LedMatrix::new(2, 2).on_color([0, 255, 0, 255]).on_color.is_some());
    /// ```
    pub fn on_color(mut self, color: [u8; 4]) -> Self {
        self.on_color = Some(color);
        self
    }

    /// Column count.
    ///
    /// ```
    /// use martensite::widgets::led_matrix::LedMatrix;
    ///
    /// assert_eq!(LedMatrix::new(9, 4).cols(), 9);
    /// ```
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// Row count.
    ///
    /// ```
    /// use martensite::widgets::led_matrix::LedMatrix;
    ///
    /// assert_eq!(LedMatrix::new(9, 4).rows(), 4);
    /// ```
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Sets one cell (out-of-range ignored).
    ///
    /// ```
    /// use martensite::widgets::led_matrix::LedMatrix;
    ///
    /// let mut m = LedMatrix::new(4, 4);
    /// m.set(1, 2, true);
    /// m.set(99, 0, true); // ignored
    /// assert!(m.get(1, 2));
    /// ```
    pub fn set(&mut self, col: usize, row: usize, on: bool) {
        if col < self.cols && row < self.rows {
            self.cells[row * self.cols + col] = on;
        }
    }

    /// Toggles one cell.
    ///
    /// ```
    /// use martensite::widgets::led_matrix::LedMatrix;
    ///
    /// let mut m = LedMatrix::new(4, 4);
    /// m.toggle(0, 0);
    /// assert!(m.get(0, 0));
    /// ```
    pub fn toggle(&mut self, col: usize, row: usize) {
        if col < self.cols && row < self.rows {
            let i = row * self.cols + col;
            self.cells[i] = !self.cells[i];
        }
    }

    /// Cell state.
    ///
    /// ```
    /// use martensite::widgets::led_matrix::LedMatrix;
    ///
    /// let m = LedMatrix::new(4, 4);
    /// assert!(!m.get(0, 0));
    /// ```
    pub fn get(&self, col: usize, row: usize) -> bool {
        col < self.cols && row < self.rows && self.cells[row * self.cols + col]
    }

    /// Lights every cell.
    ///
    /// ```
    /// use martensite::widgets::led_matrix::LedMatrix;
    ///
    /// let mut m = LedMatrix::new(3, 3);
    /// m.fill_all();
    /// assert_eq!(m.lit_count(), 9);
    /// ```
    pub fn fill_all(&mut self) {
        self.cells.fill(true);
    }

    /// Blanks every cell.
    ///
    /// ```
    /// use martensite::widgets::led_matrix::LedMatrix;
    ///
    /// let mut m = LedMatrix::new(3, 3);
    /// m.fill_all();
    /// m.clear_all();
    /// assert_eq!(m.lit_count(), 0);
    /// ```
    pub fn clear_all(&mut self) {
        self.cells.fill(false);
    }

    /// Lit cell count.
    ///
    /// ```
    /// use martensite::widgets::led_matrix::LedMatrix;
    ///
    /// let mut m = LedMatrix::new(4, 4);
    /// m.set(0, 0, true);
    /// assert_eq!(m.lit_count(), 1);
    /// ```
    pub fn lit_count(&self) -> usize {
        self.cells.iter().filter(|&&c| c).count()
    }
}

impl Widget for LedMatrix {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            (self.cols as f32 * cx.pt(CELL_PT)).min(constraints.max_size.x.max(0.0)),
            (self.rows as f32 * cx.pt(CELL_PT)).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(20.0, 20.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!(
            "{} — {}×{}, {} lit",
            self.label,
            self.cols,
            self.rows,
            self.lit_count()
        ));
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(self.bounds.min_x()),
                f64::from(self.bounds.min_y()),
                f64::from(self.bounds.max_x()),
                f64::from(self.bounds.max_y()),
            ),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let cw = self.bounds.width() / self.cols as f32;
        let ch = self.bounds.height() / self.rows as f32;
        let r = (cw.min(ch) * 0.38).max(0.5);
        let on = self
            .on_color
            .unwrap_or_else(|| cx.color(TokenKey::AccentColor, ON));
        let off = cx.color(TokenKey::BorderColor, OFF);
        for row in 0..self.rows {
            for col in 0..self.cols {
                let c = Vec2::new(
                    self.bounds.min_x() + (col as f32 + 0.5) * cw,
                    self.bounds.min_y() + (row as f32 + 0.5) * ch,
                );
                cx.list.push_fill_shape(
                    kurbo::Rect::new(0.0, 0.0, 1.0, 1.0),
                    &martensite_core::shape::Shape::circle(c, r),
                    if self.cells[row * self.cols + col] {
                        on
                    } else {
                        off
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get() {
        let mut m = LedMatrix::new(8, 8);
        m.set(3, 4, true);
        assert!(m.get(3, 4));
        assert!(!m.get(0, 0));
    }

    #[test]
    fn out_of_range_safe() {
        let mut m = LedMatrix::new(4, 4);
        m.set(9, 9, true);
        assert_eq!(m.lit_count(), 0);
        assert!(!m.get(9, 9));
    }

    #[test]
    fn fill_clear() {
        let mut m = LedMatrix::new(5, 5);
        m.fill_all();
        assert_eq!(m.lit_count(), 25);
        m.clear_all();
        assert_eq!(m.lit_count(), 0);
    }

    #[test]
    fn toggle_flips() {
        let mut m = LedMatrix::new(2, 2);
        m.toggle(1, 1);
        assert!(m.get(1, 1));
        m.toggle(1, 1);
        assert!(!m.get(1, 1));
    }
}
