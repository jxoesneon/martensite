//! `SplitFlap` — a split-flap departure-board display (Solari /
//! flip-board idiom).
//!
//! `text` sets the target; each cell is an independent flap that
//! steps through the character set on
//! [`Widget::tick`](martensite_core::widget::Widget::tick) until it
//! lands on its target — the cascading letter-flip animation.
//! Cells left of a landed cell settle first, giving the classic
//! left-to-right ripple. Display-only.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::split_flap::SplitFlap;
//!
//! let mut s = SplitFlap::new().cells(6).text("GATE 4");
//! assert_eq!(s.target_text(), "GATE 4");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

use crate::text_paint::{paint_label_clipped, SharedTextPainter};

const CELL_W_PT: f32 = 22.0;
const CELL_H_PT: f32 = 30.0;
const FONT_PT: f32 = 18.0;
const FLIP_SECS: f32 = 0.06;

const FACE: [u8; 4] = [28, 28, 32, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const GLYPH: [u8; 4] = [235, 235, 240, 255];
const HINGE: [u8; 4] = [50, 50, 56, 255];

/// The flap alphabet — blanks first so the board can idle empty.
const CHARSET: &[char] = &[
    ' ', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R',
    'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '-',
    '.', ':', '/',
];

/// A split-flap display — see the module docs.
///
/// ```
/// use martensite::widgets::split_flap::SplitFlap;
///
/// assert_eq!(SplitFlap::new().cell_count(), 8);
/// ```
pub struct SplitFlap {
    /// Accessibility label.
    pub label: String,
    target: String,
    /// Per-cell current charset index.
    flaps: Vec<usize>,
    /// Seconds until the next flip step.
    clock: f32,
    cells: usize,
    painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl Default for SplitFlap {
    fn default() -> Self {
        Self::new()
    }
}

impl SplitFlap {
    /// Creates an 8-cell blank board.
    ///
    /// ```
    /// use martensite::widgets::split_flap::SplitFlap;
    ///
    /// assert_eq!(SplitFlap::new().cell_count(), 8);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Departure board".to_string(),
            target: String::new(),
            flaps: vec![0; 8],
            clock: 0.0,
            cells: 8,
            painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Cell count (resizes the flap array, preserving landings).
    ///
    /// ```
    /// use martensite::widgets::split_flap::SplitFlap;
    ///
    /// assert_eq!(SplitFlap::new().cells(12).cell_count(), 12);
    /// ```
    pub fn cells(mut self, cells: usize) -> Self {
        self.cells = cells.max(1);
        self.flaps.resize(self.cells, 0);
        self
    }

    /// Target text — extra chars truncate, missing cells go blank.
    ///
    /// ```
    /// use martensite::widgets::split_flap::SplitFlap;
    ///
    /// let s = SplitFlap::new().cells(4).text("AB");
    /// assert_eq!(s.target_text(), "AB");
    /// ```
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.target = text.into();
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::split_flap::SplitFlap;
    ///
    /// assert_eq!(SplitFlap::new().label("Board").label, "Board");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter for glyph-accurate layout.
    ///
    /// ```
    /// use martensite::widgets::split_flap::SplitFlap;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let s = SplitFlap::new().with_text_painter(shared_painter());
    /// assert_eq!(s.cell_count(), 8);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.painter = Some(painter);
        self
    }

    /// Cell count.
    ///
    /// ```
    /// use martensite::widgets::split_flap::SplitFlap;
    ///
    /// assert_eq!(SplitFlap::new().cells(5).cell_count(), 5);
    /// ```
    pub fn cell_count(&self) -> usize {
        self.cells
    }

    /// The target text.
    ///
    /// ```
    /// use martensite::widgets::split_flap::SplitFlap;
    ///
    /// assert_eq!(SplitFlap::new().text("OK").target_text(), "OK");
    /// ```
    pub fn target_text(&self) -> &str {
        &self.target
    }

    /// Whether every flap has landed on its target.
    ///
    /// ```
    /// use martensite::widgets::split_flap::SplitFlap;
    ///
    /// assert!(SplitFlap::new().settled());
    /// ```
    pub fn settled(&self) -> bool {
        self.flaps
            .iter()
            .enumerate()
            .all(|(i, &f)| f == self.target_index(i))
    }

    /// Charset index a cell should land on.
    fn target_index(&self, i: usize) -> usize {
        let ch = self
            .target
            .chars()
            .nth(i)
            .unwrap_or(' ')
            .to_ascii_uppercase();
        CHARSET.iter().position(|&c| c == ch).unwrap_or(0)
    }

    /// Advances one cell's flap (left-to-right ripple).
    fn step(&mut self) {
        // Only flip while everything to the left has landed.
        for i in 0..self.cells {
            if self.flaps[i] != self.target_index(i) {
                self.flaps[i] = (self.flaps[i] + 1) % CHARSET.len();
                return;
            }
        }
    }
}

impl Widget for SplitFlap {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            (self.cells as f32 * cx.pt(CELL_W_PT)).min(constraints.max_size.x.max(0.0)),
            cx.pt(CELL_H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(CELL_W_PT, CELL_H_PT)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!("{} — {}", self.label, self.target));
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn tick(&mut self, dt: std::time::Duration) -> bool {
        if self.settled() {
            return false;
        }
        self.clock += dt.as_secs_f32();
        if self.clock >= FLIP_SECS {
            self.clock = 0.0;
            self.step();
            return true;
        }
        true // keep ticking until settled
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
        let painter = crate::text_paint::resolve_painter(&self.painter, cx.text_painter);
        let cell_w = self.bounds.width() / self.cells as f32;
        let cell_h = self.bounds.height();
        let size = FONT_PT * self.scale;
        let face = cx.color(TokenKey::SurfaceColor, FACE);
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        let glyph = cx.color(TokenKey::TextColor, GLYPH);
        let hinge = cx.color(TokenKey::BorderColor, HINGE);
        for i in 0..self.cells {
            let r = Rect::new(
                self.bounds.min_x() + i as f32 * cell_w,
                self.bounds.min_y(),
                cell_w - 1.0,
                cell_h,
            );
            cx.list.push_fill_shape(
                f(r),
                &martensite_core::shape::Shape::rounded(cx.pt(2.0)),
                face,
            );
            cx.list.push_stroke_shape(
                f(r),
                &martensite_core::shape::Shape::rounded(cx.pt(2.0)),
                cx.pt(0.5),
                edge,
            );
            // Hinge line across the middle.
            let mut h = kurbo::BezPath::new();
            h.move_to((f64::from(r.min_x()), f64::from(r.min_y() + cell_h / 2.0)));
            h.line_to((f64::from(r.max_x()), f64::from(r.min_y() + cell_h / 2.0)));
            cx.list.push_stroke_path(h, cx.pt(0.75), hinge);
            // Glyph, centered by estimate.
            let ch = CHARSET[self.flaps[i]];
            if ch != ' ' {
                let s = ch.to_string();
                let w = size * 0.62;
                paint_label_clipped(
                    painter,
                    cx.list,
                    f(r),
                    kurbo::Point::new(
                        f64::from(r.min_x() + (cell_w - w) / 2.0),
                        f64::from(r.min_y() + (cell_h - size) / 2.0),
                    ),
                    &s,
                    size,
                    glyph,
                );
            }
        }
    }
}

impl std::fmt::Debug for SplitFlap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitFlap")
            .field("target", &self.target)
            .field("settled", &self.settled())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn settles_on_blank() {
        assert!(SplitFlap::new().settled());
    }

    #[test]
    fn target_index_maps() {
        let s = SplitFlap::new().text("A7");
        assert_eq!(
            s.target_index(0),
            CHARSET.iter().position(|&c| c == 'A').unwrap()
        );
        assert_eq!(
            s.target_index(1),
            CHARSET.iter().position(|&c| c == '7').unwrap()
        );
    }

    #[test]
    fn tick_advances_left_first() {
        let mut s = SplitFlap::new().cells(2).text("AB");
        assert!(!s.settled());
        // Each tick ≥ FLIP_SECS advances cell 0 until it lands.
        for _ in 0..40 {
            s.tick(std::time::Duration::from_secs_f32(FLIP_SECS));
        }
        assert!(s.settled());
    }

    #[test]
    fn sub_frame_time_accumulates() {
        let mut s = SplitFlap::new().cells(1).text("Z");
        s.tick(std::time::Duration::from_secs_f32(FLIP_SECS / 2.0));
        assert_eq!(s.flaps[0], 0); // not yet
        s.tick(std::time::Duration::from_secs_f32(FLIP_SECS / 2.0 + 0.001));
        assert_eq!(s.flaps[0], 1); // stepped
    }

    #[test]
    fn unknown_char_blank() {
        let s = SplitFlap::new().text("é");
        assert_eq!(s.target_index(0), 0); // falls back to blank
    }

    #[test]
    fn lays_out() {
        let mut s = SplitFlap::new().cells(4);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let sz = s.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(300.0, 60.0),
            },
        );
        assert_eq!(sz, Vec2::new(88.0, 30.0));
    }
}
