//! `InkCanvas` — a freehand stroke-capture surface (signature pad /
//! sketch idiom).
//!
//! Primary-drag inside the canvas appends device-space points to the
//! active stroke; each released stroke is parked in
//! [`InkCanvas::take_stroke`] for app-level persistence and kept in
//! the canvas's stroke list for painting. `Escape` cancels the
//! active stroke and `Backspace`/`Delete` pops the last committed
//! stroke (undo). Strokes paint as polylines in the accent color.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::ink_canvas::InkCanvas;
//!
//! let canvas = InkCanvas::new();
//! assert_eq!(canvas.stroke_count(), 0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const HEIGHT_PT: f32 = 180.0;
const PEN_PT: f32 = 2.0;

const SURFACE: [u8; 4] = [250, 250, 248, 255];
const GRID: [u8; 4] = [210, 210, 205, 255];
const INK: [u8; 4] = [40, 60, 140, 255];

/// A committed pen stroke — device-space points in canvas coords.
pub type Stroke = Vec<Vec2>;

/// A freehand ink surface — see the module docs.
///
/// ```
/// use martensite::widgets::ink_canvas::InkCanvas;
///
/// let canvas = InkCanvas::new();
/// assert_eq!(canvas.stroke_count(), 0);
/// ```
pub struct InkCanvas {
    /// When `false` input is ignored.
    pub enabled: bool,
    /// Accessibility label.
    pub label: String,
    /// Pen width in pt.
    pub pen: f32,
    strokes: Vec<Stroke>,
    active: Option<Stroke>,
    pending: Option<Stroke>,
    bounds: Rect,
    canvas: Rect,
}

impl Default for InkCanvas {
    fn default() -> Self {
        Self::new()
    }
}

impl InkCanvas {
    /// Creates an empty canvas.
    ///
    /// ```
    /// use martensite::widgets::ink_canvas::InkCanvas;
    ///
    /// assert_eq!(InkCanvas::new().stroke_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            enabled: true,
            label: "Signature".to_string(),
            pen: PEN_PT,
            strokes: Vec::new(),
            active: None,
            pending: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            canvas: Rect::new(0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::ink_canvas::InkCanvas;
    ///
    /// let canvas = InkCanvas::new().label("Sketch");
    /// assert_eq!(canvas.label, "Sketch");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Pen width in pt.
    ///
    /// ```
    /// use martensite::widgets::ink_canvas::InkCanvas;
    ///
    /// let canvas = InkCanvas::new().pen(3.0);
    /// assert_eq!(canvas.pen, 3.0);
    /// ```
    pub fn pen(mut self, pt: f32) -> Self {
        self.pen = pt.max(0.5);
        self
    }

    /// Enables or disables input.
    ///
    /// ```
    /// use martensite::widgets::ink_canvas::InkCanvas;
    ///
    /// let canvas = InkCanvas::new().enabled(false);
    /// assert!(!canvas.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Committed stroke count.
    ///
    /// ```
    /// use martensite::widgets::ink_canvas::InkCanvas;
    ///
    /// assert_eq!(InkCanvas::new().stroke_count(), 0);
    /// ```
    pub fn stroke_count(&self) -> usize {
        self.strokes.len()
    }

    /// Whether the canvas is empty.
    ///
    /// ```
    /// use martensite::widgets::ink_canvas::InkCanvas;
    ///
    /// assert!(InkCanvas::new().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty()
    }

    /// Borrows the committed strokes.
    ///
    /// ```
    /// use martensite::widgets::ink_canvas::InkCanvas;
    ///
    /// assert!(InkCanvas::new().strokes().is_empty());
    /// ```
    pub fn strokes(&self) -> &[Stroke] {
        &self.strokes
    }

    /// Clears all strokes.
    ///
    /// ```
    /// use martensite::widgets::ink_canvas::InkCanvas;
    ///
    /// let mut canvas = InkCanvas::new();
    /// canvas.clear();
    /// assert!(canvas.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.strokes.clear();
        self.active = None;
    }

    /// Pops the last committed stroke (undo).
    ///
    /// ```
    /// use martensite::widgets::ink_canvas::InkCanvas;
    ///
    /// let mut canvas = InkCanvas::new();
    /// assert!(canvas.undo().is_none());
    /// ```
    pub fn undo(&mut self) -> Option<Stroke> {
        self.strokes.pop()
    }

    /// Drains the stroke committed since the last drain.
    ///
    /// ```
    /// use martensite::widgets::ink_canvas::InkCanvas;
    ///
    /// let mut canvas = InkCanvas::new();
    /// assert!(canvas.take_stroke().is_none());
    /// ```
    pub fn take_stroke(&mut self) -> Option<Stroke> {
        self.pending.take()
    }
}

impl Widget for InkCanvas {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(cx.pt(160.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(64.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        let m = cx.pt(1.0);
        self.canvas = Rect::new(
            bounds.min_x() + m,
            bounds.min_y() + m,
            (bounds.width() - 2.0 * m).max(0.0),
            (bounds.height() - 2.0 * m).max(0.0),
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Canvas);
        node.set_label(self.label.clone());
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if self.canvas.contains(*position) {
                    self.active = Some(vec![*position]);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some(active) = self.active.as_mut() {
                    let p = *position;
                    // Canvas-clamped so drags that leave the region
                    // hug the edge instead of overshooting.
                    let clamped = Vec2::new(
                        p.x.clamp(self.canvas.min_x(), self.canvas.max_x()),
                        p.y.clamp(self.canvas.min_y(), self.canvas.max_y()),
                    );
                    if active.last() != Some(&clamped) {
                        active.push(clamped);
                    }
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if let Some(stroke) = self.active.take() {
                    if stroke.len() >= 2 {
                        self.strokes.push(stroke.clone());
                        self.pending = Some(stroke);
                    }
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "Escape" => {
                    if self.active.take().is_some() {
                        EventResponse::RequestRepaint
                    } else {
                        EventResponse::Ignored
                    }
                }
                "Backspace" | "Delete" => {
                    if self.undo().is_some() {
                        EventResponse::RequestRepaint
                    } else {
                        EventResponse::Ignored
                    }
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
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
        let surface = cx.color(TokenKey::SurfaceColor, SURFACE);
        let grid = cx.color(TokenKey::DividerColor, GRID);
        let ink = cx.color(TokenKey::AccentColor, INK);
        let shape = martensite_core::shape::Shape::rounded(cx.pt(4.0));
        cx.list.push_fill_shape(f(self.canvas), &shape, surface);
        // Baseline hint — the signature line idiom.
        let base = self.canvas.min_y() + self.canvas.height() * 0.75;
        let mut line = kurbo::BezPath::new();
        line.move_to((
            f64::from(self.canvas.min_x() + cx.pt(12.0)),
            f64::from(base),
        ));
        line.line_to((
            f64::from(self.canvas.max_x() - cx.pt(12.0)),
            f64::from(base),
        ));
        cx.list.push_stroke_path(line, cx.pt(0.75), grid);
        cx.list
            .push_stroke_shape(f(self.canvas), &shape, cx.pt(0.75), grid);

        let w = cx.pt(self.pen);
        for stroke in self
            .strokes
            .iter()
            .chain(self.active.iter())
            .filter(|s| s.len() >= 2)
        {
            let mut path = kurbo::BezPath::new();
            path.move_to((f64::from(stroke[0].x), f64::from(stroke[0].y)));
            for p in &stroke[1..] {
                path.line_to((f64::from(p.x), f64::from(p.y)));
            }
            cx.list.push_stroke_path(path, w, ink);
        }
    }
}

impl std::fmt::Debug for InkCanvas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InkCanvas")
            .field("strokes", &self.strokes.len())
            .field("active", &self.active.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(canvas: &mut InkCanvas, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        canvas.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        canvas.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev(canvas: &mut InkCanvas, event: WidgetEvent) {
        canvas.event(&mut EventContext {
            event: &event,
            bounds: Rect::new(0.0, 0.0, 400.0, 200.0),
            scale: 1.0,
        });
    }

    fn draw_stroke(canvas: &mut InkCanvas, pts: &[(f32, f32)]) {
        let (x, y) = pts[0];
        ev(
            canvas,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(x, y),
                count: 1,
            },
        );
        for &(x, y) in &pts[1..] {
            ev(
                canvas,
                WidgetEvent::PointerMoved {
                    position: Vec2::new(x, y),
                },
            );
        }
        ev(
            canvas,
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::ZERO,
            },
        );
    }

    #[test]
    fn stroke_commits_on_release() {
        let mut canvas = InkCanvas::new();
        laid_out(&mut canvas, 400.0, 200.0);
        draw_stroke(&mut canvas, &[(10.0, 10.0), (50.0, 30.0), (90.0, 10.0)]);
        assert_eq!(canvas.stroke_count(), 1);
        let stroke = canvas.take_stroke().unwrap();
        assert_eq!(stroke.len(), 3);
        assert!(canvas.take_stroke().is_none());
    }

    #[test]
    fn tap_without_drag_does_not_commit() {
        let mut canvas = InkCanvas::new();
        laid_out(&mut canvas, 400.0, 200.0);
        draw_stroke(&mut canvas, &[(20.0, 20.0)]);
        assert_eq!(canvas.stroke_count(), 0);
    }

    #[test]
    fn drag_points_clamp_to_canvas() {
        let mut canvas = InkCanvas::new();
        laid_out(&mut canvas, 400.0, 200.0);
        draw_stroke(&mut canvas, &[(10.0, 10.0), (900.0, 10.0)]);
        let last = canvas.strokes()[0].last().unwrap();
        assert!(last.x <= canvas.canvas.max_x());
    }

    #[test]
    fn backspace_undos_last_stroke() {
        let mut canvas = InkCanvas::new();
        laid_out(&mut canvas, 400.0, 200.0);
        draw_stroke(&mut canvas, &[(10.0, 10.0), (40.0, 40.0)]);
        draw_stroke(&mut canvas, &[(50.0, 50.0), (80.0, 80.0)]);
        assert_eq!(canvas.stroke_count(), 2);
        ev(
            &mut canvas,
            WidgetEvent::KeyPressed {
                key: "Backspace".to_string(),
                repeat: false,
            },
        );
        assert_eq!(canvas.stroke_count(), 1);
    }

    #[test]
    fn escape_cancels_active_stroke() {
        let mut canvas = InkCanvas::new();
        laid_out(&mut canvas, 400.0, 200.0);
        ev(
            &mut canvas,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(10.0, 10.0),
                count: 1,
            },
        );
        ev(
            &mut canvas,
            WidgetEvent::KeyPressed {
                key: "Escape".to_string(),
                repeat: false,
            },
        );
        ev(
            &mut canvas,
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::ZERO,
            },
        );
        assert_eq!(canvas.stroke_count(), 0);
    }

    #[test]
    fn clear_empties() {
        let mut canvas = InkCanvas::new();
        laid_out(&mut canvas, 400.0, 200.0);
        draw_stroke(&mut canvas, &[(10.0, 10.0), (40.0, 40.0)]);
        canvas.clear();
        assert!(canvas.is_empty());
    }

    #[test]
    fn disabled_inert() {
        let mut canvas = InkCanvas::new().enabled(false);
        laid_out(&mut canvas, 400.0, 200.0);
        draw_stroke(&mut canvas, &[(10.0, 10.0), (40.0, 40.0)]);
        assert!(canvas.is_empty());
    }
}
