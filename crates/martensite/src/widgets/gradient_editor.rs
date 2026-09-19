//! `GradientEditor` — a gradient rail with draggable color stops
//! (design-tool / CSS gradient-editor idiom).
//!
//! Stops are `(position 0..=1, RGBA)` pairs painting as a
//! horizontal color bar with a handle per stop. Dragging a handle
//! moves its stop; clicking empty rail space adds one (color
//! sampled from the gradient at that point); a double-click on a
//! handle removes it (two-stop minimum enforced). Any edit parks
//! [`GradientEditor::take_changed`] and the selected index in
//! [`GradientEditor::take_selected`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::gradient_editor::GradientEditor;
//!
//! let g = GradientEditor::new();
//! assert_eq!(g.stop_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const WIDTH_PT: f32 = 240.0;
const HEIGHT_PT: f32 = 34.0;
const BAR_PT: f32 = 18.0;
const HANDLE_PT: f32 = 12.0;

const EDGE: [u8; 4] = [70, 70, 76, 255];
const FG: [u8; 4] = [230, 230, 235, 255];
const ACCENT: [u8; 4] = [80, 140, 220, 255];

/// One gradient stop — position and color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    /// Position along the bar `0..=1`.
    pub position: f32,
    /// Stop color.
    pub color: [u8; 4],
}

impl GradientStop {
    /// Creates a stop.
    ///
    /// ```
    /// use martensite::widgets::gradient_editor::GradientStop;
    ///
    /// let s = GradientStop::new(0.5, [255, 0, 0, 255]);
    /// assert_eq!(s.position, 0.5);
    /// ```
    pub fn new(position: f32, color: [u8; 4]) -> Self {
        Self {
            position: position.clamp(0.0, 1.0),
            color,
        }
    }
}

/// A gradient rail with draggable stops — see the module docs.
///
/// ```
/// use martensite::widgets::gradient_editor::GradientEditor;
///
/// assert_eq!(GradientEditor::new().stop_count(), 2);
/// ```
pub struct GradientEditor {
    /// When `false` edits are ignored.
    pub enabled: bool,
    /// Accessibility label.
    pub label: String,
    stops: Vec<GradientStop>,
    selected: Option<usize>,
    dragging: Option<usize>,
    changed: bool,
    selected_flag: Option<usize>,
    bounds: Rect,
    scale: f32,
}

impl Default for GradientEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl GradientEditor {
    /// Creates a black→white two-stop gradient.
    ///
    /// ```
    /// use martensite::widgets::gradient_editor::GradientEditor;
    ///
    /// assert_eq!(GradientEditor::new().stop_count(), 2);
    /// ```
    pub fn new() -> Self {
        Self {
            enabled: true,
            label: "Gradient".to_string(),
            stops: vec![
                GradientStop::new(0.0, [0, 0, 0, 255]),
                GradientStop::new(1.0, [255, 255, 255, 255]),
            ],
            selected: None,
            dragging: None,
            changed: false,
            selected_flag: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Initial stops (sorted by position, ≥2 kept).
    ///
    /// ```
    /// use martensite::widgets::gradient_editor::{GradientEditor, GradientStop};
    ///
    /// let g = GradientEditor::new().stops(vec![
    ///     GradientStop::new(1.0, [0, 0, 255, 255]),
    ///     GradientStop::new(0.0, [255, 0, 0, 255]),
    /// ]);
    /// assert_eq!(g.stop_list()[0].position, 0.0);
    /// ```
    pub fn stops(mut self, stops: Vec<GradientStop>) -> Self {
        if stops.len() >= 2 {
            self.stops = stops;
            self.sort_stops();
        }
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::gradient_editor::GradientEditor;
    ///
    /// let g = GradientEditor::new().label("Fade");
    /// assert_eq!(g.label, "Fade");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Enables or disables edits.
    ///
    /// ```
    /// use martensite::widgets::gradient_editor::GradientEditor;
    ///
    /// assert!(!GradientEditor::new().enabled(false).enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Stop list.
    ///
    /// ```
    /// use martensite::widgets::gradient_editor::GradientEditor;
    ///
    /// assert_eq!(GradientEditor::new().stop_list().len(), 2);
    /// ```
    pub fn stop_list(&self) -> &[GradientStop] {
        &self.stops
    }

    /// Stop count.
    ///
    /// ```
    /// use martensite::widgets::gradient_editor::GradientEditor;
    ///
    /// assert_eq!(GradientEditor::new().stop_count(), 2);
    /// ```
    pub fn stop_count(&self) -> usize {
        self.stops.len()
    }

    /// Adds a stop (sorted in).
    ///
    /// ```
    /// use martensite::widgets::gradient_editor::GradientEditor;
    ///
    /// let mut g = GradientEditor::new();
    /// g.add_stop(martensite::widgets::gradient_editor::GradientStop::new(0.5, [1, 2, 3, 255]));
    /// assert_eq!(g.stop_count(), 3);
    /// ```
    pub fn add_stop(&mut self, stop: GradientStop) {
        self.stops.push(stop);
        self.sort_stops();
        self.changed = true;
    }

    /// Recolors a stop.
    ///
    /// ```
    /// use martensite::widgets::gradient_editor::GradientEditor;
    ///
    /// let mut g = GradientEditor::new();
    /// g.set_stop_color(0, [255, 0, 0, 255]);
    /// assert_eq!(g.stop_list()[0].color, [255, 0, 0, 255]);
    /// ```
    pub fn set_stop_color(&mut self, index: usize, color: [u8; 4]) {
        if let Some(s) = self.stops.get_mut(index) {
            s.color = color;
            self.changed = true;
        }
    }

    /// Drains the any-edit flag.
    ///
    /// ```
    /// use martensite::widgets::gradient_editor::GradientEditor;
    ///
    /// let mut g = GradientEditor::new();
    /// assert!(!g.take_changed());
    /// ```
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Drains the last selected stop index.
    ///
    /// ```
    /// use martensite::widgets::gradient_editor::GradientEditor;
    ///
    /// let mut g = GradientEditor::new();
    /// assert!(g.take_selected().is_none());
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.selected_flag.take()
    }

    /// The selected stop index (handle highlight + a11y).
    ///
    /// ```
    /// use martensite::widgets::gradient_editor::GradientEditor;
    ///
    /// assert_eq!(GradientEditor::new().selected(), None);
    /// ```
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Linear-interpolated color at `t` `0..=1`.
    ///
    /// ```
    /// use martensite::widgets::gradient_editor::GradientEditor;
    ///
    /// let g = GradientEditor::new();
    /// assert_eq!(g.color_at(0.5), [128, 128, 128, 255]);
    /// ```
    pub fn color_at(&self, t: f32) -> [u8; 4] {
        let t = t.clamp(0.0, 1.0);
        let (mut lo, mut hi) = (self.stops[0], self.stops[0]);
        for &s in &self.stops {
            if s.position <= t {
                lo = s;
            }
            if s.position >= t {
                hi = s;
                break;
            }
        }
        let span = (hi.position - lo.position).max(0.0001);
        let f = (t - lo.position) / span;
        let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * f).round() as u8;
        [
            lerp(lo.color[0], hi.color[0]),
            lerp(lo.color[1], hi.color[1]),
            lerp(lo.color[2], hi.color[2]),
            lerp(lo.color[3], hi.color[3]),
        ]
    }

    fn sort_stops(&mut self) {
        self.stops.sort_by(|a, b| a.position.total_cmp(&b.position));
    }

    /// Gradient bar rect.
    fn bar(&self) -> Rect {
        Rect::new(
            self.bounds.min_x(),
            self.bounds.min_y(),
            self.bounds.width(),
            BAR_PT * self.scale,
        )
    }

    /// Handle center for stop `i`.
    fn handle_center(&self, i: usize) -> Vec2 {
        Vec2::new(
            self.bounds.min_x() + self.stops[i].position * self.bounds.width(),
            self.bounds.min_y() + BAR_PT * self.scale + HANDLE_PT * self.scale / 2.0,
        )
    }

    /// Stop index whose handle contains the point.
    fn handle_at(&self, p: Vec2) -> Option<usize> {
        let r = HANDLE_PT * self.scale / 2.0 + 2.0;
        (0..self.stops.len()).find(|&i| {
            let c = self.handle_center(i);
            (p - c).length() <= r
        })
    }
}

impl Widget for GradientEditor {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 24.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!("{} — {} stops", self.label, self.stops.len()));
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
                count,
            } => {
                if let Some(i) = self.handle_at(*position) {
                    if *count == 2 && self.stops.len() > 2 {
                        self.stops.remove(i);
                        self.selected = None;
                        self.changed = true;
                        return EventResponse::RequestRepaint;
                    }
                    self.selected = Some(i);
                    self.selected_flag = Some(i);
                    self.dragging = Some(i);
                    return EventResponse::CapturePointer;
                }
                if self.bar().contains(*position) {
                    // Click on empty rail — insert a sampled stop.
                    let t = ((position.x - self.bounds.min_x()) / self.bounds.width().max(1.0))
                        .clamp(0.0, 1.0);
                    let color = self.color_at(t);
                    self.stops.push(GradientStop::new(t, color));
                    self.sort_stops();
                    let idx = self
                        .stops
                        .iter()
                        .position(|s| (s.position - t).abs() < 0.001)
                        .unwrap_or(0);
                    self.selected = Some(idx);
                    self.selected_flag = Some(idx);
                    self.changed = true;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some(i) = self.dragging {
                    let t = ((position.x - self.bounds.min_x()) / self.bounds.width().max(1.0))
                        .clamp(0.0, 1.0);
                    if (self.stops[i].position - t).abs() > 0.0001 {
                        self.stops[i].position = t;
                        self.sort_stops();
                        // Track the stop through the re-sort.
                        self.dragging = self
                            .stops
                            .iter()
                            .position(|s| (s.position - t).abs() < 0.0001);
                        self.selected = self.dragging;
                        self.changed = true;
                        return EventResponse::RequestRepaint;
                    }
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.dragging.take().is_some() {
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
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
        // Gradient bar — vertical color slices between stops.
        let bar = self.bar();
        let cols = (bar.width() / (self.scale * 2.0).max(1.0)).ceil().max(1.0) as usize;
        let col_w = bar.width() / cols as f32;
        for i in 0..cols {
            let t = i as f32 / (cols.saturating_sub(1)).max(1) as f32;
            cx.list.push_fill_rect(
                f(Rect::new(
                    bar.min_x() + i as f32 * col_w,
                    bar.min_y(),
                    col_w + 0.5,
                    bar.height(),
                )),
                self.color_at(t),
            );
        }
        cx.list.push_stroke_shape(
            f(bar),
            &martensite_core::shape::Shape::rounded(cx.pt(3.0)),
            cx.pt(0.75),
            cx.color(TokenKey::BorderColor, EDGE),
        );

        // Handles — squares rotated 45° below the bar.
        for (i, s) in self.stops.iter().enumerate() {
            let c = self.handle_center(i);
            let r = HANDLE_PT * self.scale / 2.0;
            let mut path = kurbo::BezPath::new();
            path.move_to((f64::from(c.x), f64::from(c.y - r)));
            path.line_to((f64::from(c.x + r), f64::from(c.y)));
            path.line_to((f64::from(c.x), f64::from(c.y + r)));
            path.line_to((f64::from(c.x - r), f64::from(c.y)));
            path.close_path();
            cx.list.push_path(path.clone(), s.color);
            cx.list.push_stroke_path(
                path,
                cx.pt(if self.selected == Some(i) { 2.0 } else { 0.75 }),
                if self.selected == Some(i) {
                    cx.color(TokenKey::AccentColor, ACCENT)
                } else {
                    cx.color(TokenKey::TextColor, FG)
                },
            );
        }
    }
}

impl std::fmt::Debug for GradientEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GradientEditor")
            .field("stops", &self.stops.len())
            .field("selected", &self.selected)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(g: &mut GradientEditor, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        g.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        g.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev(g: &mut GradientEditor, e: WidgetEvent) {
        g.event(&mut EventContext {
            event: &e,
            bounds: Rect::new(0.0, 0.0, 240.0, 34.0),
            scale: 1.0,
        });
    }

    #[test]
    fn stops_sort() {
        let g = GradientEditor::new().stops(vec![
            GradientStop::new(0.8, [0, 0, 0, 255]),
            GradientStop::new(0.2, [0, 0, 0, 255]),
        ]);
        assert_eq!(g.stop_list()[0].position, 0.2);
    }

    #[test]
    fn color_at_interpolates() {
        let g = GradientEditor::new();
        assert_eq!(g.color_at(0.0), [0, 0, 0, 255]);
        assert_eq!(g.color_at(1.0), [255, 255, 255, 255]);
        let mid = g.color_at(0.5);
        assert!((mid[0] as i32 - 128).abs() <= 1);
    }

    #[test]
    fn click_rail_adds_stop() {
        let mut g = GradientEditor::new();
        laid_out(&mut g, 240.0, 34.0);
        ev(
            &mut g,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(60.0, 6.0), // bar, 25% in
                count: 1,
            },
        );
        assert_eq!(g.stop_count(), 3);
        assert!(g.take_changed());
        assert_eq!(g.take_selected(), Some(1));
    }

    #[test]
    fn double_click_removes_stop() {
        let mut g = GradientEditor::new();
        laid_out(&mut g, 240.0, 34.0);
        g.add_stop(GradientStop::new(0.5, [1, 2, 3, 255]));
        let c = g.handle_center(1);
        ev(
            &mut g,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: c,
                count: 2,
            },
        );
        assert_eq!(g.stop_count(), 2);
    }

    #[test]
    fn drag_moves_stop() {
        let mut g = GradientEditor::new();
        laid_out(&mut g, 240.0, 34.0);
        let c = g.handle_center(0); // left stop at x=0
        ev(
            &mut g,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: c,
                count: 1,
            },
        );
        ev(
            &mut g,
            WidgetEvent::PointerMoved {
                position: Vec2::new(120.0, c.y),
            },
        );
        assert!((g.stop_list().iter().map(|s| s.position).fold(0.0, f32::max) - 1.0).abs() < 0.001);
        // Some stop now sits near the middle.
        assert!(g
            .stop_list()
            .iter()
            .any(|s| (s.position - 0.5).abs() < 0.05));
    }

    #[test]
    fn min_two_stops_enforced() {
        let mut g = GradientEditor::new();
        laid_out(&mut g, 240.0, 34.0);
        let c = g.handle_center(0);
        ev(
            &mut g,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: c,
                count: 2,
            },
        );
        assert_eq!(g.stop_count(), 2); // can't drop below 2
    }

    #[test]
    fn disabled_inert() {
        let mut g = GradientEditor::new().enabled(false);
        laid_out(&mut g, 240.0, 34.0);
        ev(
            &mut g,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(60.0, 6.0),
                count: 1,
            },
        );
        assert_eq!(g.stop_count(), 2);
        assert!(!g.take_changed());
    }
}
