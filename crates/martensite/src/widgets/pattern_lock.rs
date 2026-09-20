//! `PatternLock` — the Android-style 3×3 unlock pattern: nine
//! dots that connect as the pointer drags across them (each dot
//! usable once), releasing parks the sequence of indices in
//! [`PatternLock::take_pattern`] for the host to verify.
//!
//! Row-major indices: `0 1 2 / 3 4 5 / 6 7 8`.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::pattern_lock::PatternLock;
//!
//! let p = PatternLock::new();
//! assert_eq!(p.dot_count(), 9);
//! assert!(p.pattern().is_empty());
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const PAD_PT: f32 = 20.0;
const DOT_PT: f32 = 14.0;
const DOT_ACTIVE_PT: f32 = 20.0;
const HIT_PT: f32 = 44.0;

const FACE: [u8; 4] = [30, 32, 40, 255];
const DOT: [u8; 4] = [120, 124, 134, 255];
const LINE: [u8; 4] = [90, 140, 220, 255];

/// The lock grid — see the module docs.
///
/// ```
/// use martensite::widgets::pattern_lock::PatternLock;
///
/// assert_eq!(PatternLock::new().dot_count(), 9);
/// ```
pub struct PatternLock {
    /// Accessibility label.
    pub label: String,
    dots: Vec<Vec2>,
    pattern: Vec<usize>,
    dragging: bool,
    cursor: Option<Vec2>,
    taken: Option<Vec<usize>>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for PatternLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PatternLock")
            .field("pattern_len", &self.pattern.len())
            .finish()
    }
}

impl Default for PatternLock {
    fn default() -> Self {
        Self::new()
    }
}

impl PatternLock {
    /// Empty lock.
    ///
    /// ```
    /// use martensite::widgets::pattern_lock::PatternLock;
    ///
    /// assert_eq!(PatternLock::new().dot_count(), 9);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Pattern lock".to_string(),
            dots: Vec::new(),
            pattern: Vec::new(),
            dragging: false,
            cursor: None,
            taken: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::pattern_lock::PatternLock;
    ///
    /// assert_eq!(PatternLock::new().label("Unlock").label, "Unlock");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Dot count (always 9).
    ///
    /// ```
    /// use martensite::widgets::pattern_lock::PatternLock;
    ///
    /// assert_eq!(PatternLock::new().dot_count(), 9);
    /// ```
    pub fn dot_count(&self) -> usize {
        9
    }

    /// The in-progress or completed dot sequence.
    ///
    /// ```
    /// use martensite::widgets::pattern_lock::PatternLock;
    ///
    /// assert!(PatternLock::new().pattern().is_empty());
    /// ```
    pub fn pattern(&self) -> &[usize] {
        &self.pattern
    }

    /// Clears the current pattern.
    ///
    /// ```
    /// use martensite::widgets::pattern_lock::PatternLock;
    ///
    /// let mut p = PatternLock::new();
    /// p.clear();
    /// assert!(p.pattern().is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.pattern.clear();
        self.dragging = false;
        self.cursor = None;
    }

    /// Drains the released pattern (index sequence).
    ///
    /// ```
    /// use martensite::widgets::pattern_lock::PatternLock;
    ///
    /// let mut p = PatternLock::new();
    /// assert_eq!(p.take_pattern(), None);
    /// ```
    pub fn take_pattern(&mut self) -> Option<Vec<usize>> {
        self.taken.take()
    }

    fn dot_at(&self, p: Vec2) -> Option<usize> {
        let hit = HIT_PT * self.scale / 2.0;
        self.dots
            .iter()
            .position(|d| (d.x - p.x).abs() <= hit && (d.y - p.y).abs() <= hit)
    }

    fn visit(&mut self, i: usize) {
        if !self.pattern.contains(&i) {
            self.pattern.push(i);
        }
    }
}

fn line_path(a: Vec2, b: Vec2) -> kurbo::BezPath {
    let mut p = kurbo::BezPath::new();
    p.move_to((f64::from(a.x), f64::from(a.y)));
    p.line_to((f64::from(b.x), f64::from(b.y)));
    p
}

impl Widget for PatternLock {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let side = (HIT_PT * 3.0 + PAD_PT * 2.0) * s;
        Vec2::new(
            side.min(constraints.max_size.x.max(0.0)),
            side.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 120.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        // Evenly spaced 3×3 inside the bounds.
        self.dots.clear();
        let pad = PAD_PT * cx.scale;
        let step_x = (bounds.width() - pad * 2.0) / 2.0;
        let step_y = (bounds.height() - pad * 2.0) / 2.0;
        for row in 0..3 {
            for col in 0..3 {
                self.dots.push(Vec2::new(
                    bounds.min_x() + pad + col as f32 * step_x,
                    bounds.min_y() + pad + row as f32 * step_y,
                ));
            }
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
        node.set_value(format!("{} dots connected", self.pattern.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                self.pattern.clear();
                self.dragging = true;
                if let Some(i) = self.dot_at(*position) {
                    self.visit(i);
                }
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerMoved { position } => {
                if !self.dragging {
                    return EventResponse::Ignored;
                }
                self.cursor = Some(*position);
                if let Some(i) = self.dot_at(*position) {
                    self.visit(i);
                }
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if !self.dragging {
                    return EventResponse::Ignored;
                }
                self.dragging = false;
                self.cursor = None;
                if self.pattern.len() >= 2 {
                    self.taken = Some(self.pattern.clone());
                }
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(self.bounds.min_x()),
                f64::from(self.bounds.min_y()),
                f64::from(self.bounds.max_x()),
                f64::from(self.bounds.max_y()),
            ),
            cx.color(TokenKey::BackgroundColor, FACE),
        );
        let accent = cx.color(TokenKey::AccentColor, LINE);
        // Committed segments.
        for pair in self.pattern.windows(2) {
            cx.list.push_stroke_path(
                line_path(self.dots[pair[0]], self.dots[pair[1]]),
                3.0 * s,
                accent,
            );
        }
        // Live segment to the cursor.
        if self.dragging {
            if let (Some(&last), Some(cur)) = (self.pattern.last(), self.cursor) {
                cx.list
                    .push_stroke_path(line_path(self.dots[last], cur), 2.0 * s, accent);
            }
        }
        // Dots.
        for (i, d) in self.dots.iter().enumerate() {
            let active = self.pattern.contains(&i);
            let r = if active { DOT_ACTIVE_PT } else { DOT_PT } * s;
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(d.x - r / 2.0),
                    f64::from(d.y - r / 2.0),
                    f64::from(d.x + r / 2.0),
                    f64::from(d.y + r / 2.0),
                ),
                &martensite_core::shape::Shape::ELLIPSE,
                if active { accent } else { DOT },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(p: &mut PatternLock) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        p.layout(&mut cx, Rect::new(0.0, 0.0, 160.0, 160.0));
    }

    fn ev(p: &mut PatternLock, e: WidgetEvent) {
        p.event(&mut EventContext {
            event: &e,
            bounds: p.bounds,
            scale: 1.0,
        });
    }

    fn drag_l_shape(p: &mut PatternLock) {
        laid_out(p);
        let d0 = p.dots[0];
        ev(
            p,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: d0,
                count: 1,
            },
        );
        let d3 = p.dots[3];
        ev(p, WidgetEvent::PointerMoved { position: d3 });
        let d6 = p.dots[6];
        ev(p, WidgetEvent::PointerMoved { position: d6 });
        let d7 = p.dots[7];
        ev(p, WidgetEvent::PointerMoved { position: d7 });
        ev(
            p,
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: d7,
            },
        );
    }

    #[test]
    fn drag_connects_dots() {
        let mut p = PatternLock::new();
        drag_l_shape(&mut p);
        assert_eq!(p.take_pattern(), Some(vec![0, 3, 6, 7]));
    }

    #[test]
    fn dots_not_revisited() {
        let mut p = PatternLock::new();
        laid_out(&mut p);
        let d0 = p.dots[0];
        ev(
            &mut p,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: d0,
                count: 1,
            },
        );
        let d4 = p.dots[4];
        ev(&mut p, WidgetEvent::PointerMoved { position: d4 });
        ev(&mut p, WidgetEvent::PointerMoved { position: d0 });
        assert_eq!(p.pattern(), &[0, 4]);
    }

    #[test]
    fn single_dot_isnt_a_pattern() {
        let mut p = PatternLock::new();
        laid_out(&mut p);
        let d0 = p.dots[0];
        ev(
            &mut p,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: d0,
                count: 1,
            },
        );
        ev(
            &mut p,
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: d0,
            },
        );
        assert_eq!(p.take_pattern(), None);
    }

    #[test]
    fn paint_without_painter() {
        let mut p = PatternLock::new();
        drag_l_shape(&mut p);
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
