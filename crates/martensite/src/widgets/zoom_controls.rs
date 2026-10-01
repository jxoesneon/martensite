//! `ZoomControls` — a map/canvas zoom button cluster (Leaflet /
//! Google Maps corner control idiom).
//!
//! A vertical stack of `+`, `−`, and optional `fit`/`1:1` buttons.
//! Each press parks a [`ZoomAction`] in
//! [`ZoomControls::take_action`] — the widget is stateless about the
//! view (the host owns zoom, typically feeding a
//! [`Viewport`](crate::widgets::Viewport)). An optional readout
//! shows the current percent via [`ZoomControls::set_zoom`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::zoom_controls::ZoomControls;
//!
//! let z = ZoomControls::new();
//! assert_eq!(z.button_count(), 4);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const BTN_PT: f32 = 28.0;
/// Gap between buttons in horizontal mode — vertical stacks stay
/// joined.
const GAP_PT: f32 = 6.0;
const READOUT_PT: f32 = 20.0;
const FONT_PT: f32 = 15.0;

const FACE: [u8; 4] = [58, 60, 68, 255];
const FACE_DOWN: [u8; 4] = [82, 84, 94, 255];
const EDGE: [u8; 4] = [90, 92, 100, 255];
const FG: [u8; 4] = [230, 232, 238, 255];
const DIM: [u8; 4] = [140, 142, 150, 255];

/// A zoom action parked for the host.
///
/// ```
/// use martensite::widgets::zoom_controls::ZoomAction;
///
/// assert_eq!(ZoomAction::ZoomIn, ZoomAction::ZoomIn);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomAction {
    /// `+` — zoom in one step.
    ZoomIn,
    /// `−` — zoom out one step.
    ZoomOut,
    /// Fit the content to the view.
    Fit,
    /// Reset to 100%.
    Reset,
}

/// A zoom button cluster — see the module docs.
///
/// ```
/// use martensite::widgets::zoom_controls::ZoomControls;
///
/// assert_eq!(ZoomControls::new().take_action(), None);
/// ```
pub struct ZoomControls {
    /// Accessibility label.
    pub label: String,
    /// Whether the `fit` button is shown.
    pub show_fit: bool,
    /// Whether the `1:1` reset button is shown.
    pub show_reset: bool,
    /// Current zoom for the percent readout (`None` hides it).
    zoom: Option<f32>,
    /// Whether the `N%` readout paints — the zoom value still drives
    /// end-of-range disabling when the readout is hidden.
    show_readout: bool,
    /// Horizontal row layout (default: vertical stack).
    horizontal: bool,
    /// Inclusive zoom bounds — `+`/`−` disable at the ends.
    min_zoom: f32,
    max_zoom: f32,
    rects: Vec<(ZoomAction, Rect)>,
    held: Option<ZoomAction>,
    action: Option<ZoomAction>,
    bounds: Rect,
    scale: f32,
    text_painter: Option<SharedTextPainter>,
}

impl std::fmt::Debug for ZoomControls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZoomControls")
            .field("zoom", &self.zoom)
            .finish()
    }
}

impl Default for ZoomControls {
    fn default() -> Self {
        Self::new()
    }
}

impl ZoomControls {
    /// `+`, `−`, `fit`, `1:1` stack with no readout.
    ///
    /// ```
    /// use martensite::widgets::zoom_controls::ZoomControls;
    ///
    /// assert_eq!(ZoomControls::new().button_count(), 4);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Zoom".to_string(),
            show_fit: true,
            show_reset: true,
            zoom: None,
            show_readout: true,
            horizontal: false,
            min_zoom: 0.01,
            max_zoom: 100.0,
            rects: Vec::new(),
            held: None,
            action: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::zoom_controls::ZoomControls;
    ///
    /// assert_eq!(ZoomControls::new().label("Map").label, "Map");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Hides/shows the `fit` button.
    ///
    /// ```
    /// use martensite::widgets::zoom_controls::ZoomControls;
    ///
    /// assert_eq!(ZoomControls::new().fit(false).button_count(), 3);
    /// ```
    pub fn fit(mut self, show: bool) -> Self {
        self.show_fit = show;
        self
    }

    /// Hides/shows the `1:1` reset button.
    ///
    /// ```
    /// use martensite::widgets::zoom_controls::ZoomControls;
    ///
    /// assert_eq!(ZoomControls::new().reset(false).fit(false).button_count(), 2);
    /// ```
    pub fn reset(mut self, show: bool) -> Self {
        self.show_reset = show;
        self
    }

    /// Initial readout zoom.
    ///
    /// ```
    /// use martensite::widgets::zoom_controls::ZoomControls;
    ///
    /// assert_eq!(ZoomControls::new().zoom(2.0).zoom_value(), Some(2.0));
    /// ```
    pub fn zoom(mut self, zoom: f32) -> Self {
        self.zoom = Some(zoom.clamp(self.min_zoom, self.max_zoom));
        self
    }

    /// Hides the `N%` readout while keeping the zoom value driving
    /// end-of-range disabling — for quantized ladders where a
    /// percentage is meaningless.
    ///
    /// ```
    /// use martensite::widgets::zoom_controls::ZoomControls;
    ///
    /// let z = ZoomControls::new().zoom(2.0).readout(false);
    /// assert_eq!(z.zoom_value(), Some(2.0));
    /// ```
    #[must_use]
    pub fn readout(mut self, show: bool) -> Self {
        self.show_readout = show;
        self
    }

    /// Lays the buttons out in a horizontal row instead of the
    /// default vertical stack — header/toolbar placement.
    ///
    /// ```
    /// use martensite::widgets::zoom_controls::ZoomControls;
    ///
    /// assert!(ZoomControls::new().horizontal(true).is_horizontal());
    /// ```
    #[must_use]
    pub fn horizontal(mut self, on: bool) -> Self {
        self.horizontal = on;
        self
    }

    /// Whether the cluster lays out horizontally.
    ///
    /// ```
    /// use martensite::widgets::zoom_controls::ZoomControls;
    ///
    /// assert!(!ZoomControls::new().is_horizontal());
    /// ```
    #[must_use]
    pub fn is_horizontal(&self) -> bool {
        self.horizontal
    }

    /// Inclusive zoom bounds; `+`/`−` disable at the ends so a
    /// quantized zoom ladder can't step past its ladder.
    ///
    /// ```
    /// use martensite::widgets::zoom_controls::{ZoomAction, ZoomControls};
    ///
    /// let z = ZoomControls::new().zoom_range(1.0, 3.0).zoom(3.0);
    /// assert!(!z.action_enabled(ZoomAction::ZoomIn));
    /// assert!(z.action_enabled(ZoomAction::ZoomOut));
    /// ```
    #[must_use]
    pub fn zoom_range(mut self, min: f32, max: f32) -> Self {
        self.min_zoom = min.max(0.01);
        self.max_zoom = max.max(self.min_zoom);
        self
    }

    /// Whether `action` is currently live — `+`/`−` go inactive at
    /// the `zoom_range` ends (unknown zoom leaves both enabled).
    ///
    /// ```
    /// use martensite::widgets::zoom_controls::{ZoomAction, ZoomControls};
    ///
    /// let z = ZoomControls::new().zoom_range(1.0, 3.0).zoom(1.0);
    /// assert!(!z.action_enabled(ZoomAction::ZoomOut));
    /// ```
    #[must_use]
    pub fn action_enabled(&self, action: ZoomAction) -> bool {
        match (action, self.zoom) {
            (ZoomAction::ZoomIn, Some(z)) => z < self.max_zoom - f32::EPSILON,
            (ZoomAction::ZoomOut, Some(z)) => z > self.min_zoom + f32::EPSILON,
            _ => true,
        }
    }

    /// Optional painter override (tests / headless).
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Number of visible buttons.
    ///
    /// ```
    /// use martensite::widgets::zoom_controls::ZoomControls;
    ///
    /// assert_eq!(ZoomControls::new().button_count(), 4);
    /// ```
    pub fn button_count(&self) -> usize {
        2 + usize::from(self.show_fit) + usize::from(self.show_reset)
    }

    /// The readout zoom, if shown.
    ///
    /// ```
    /// use martensite::widgets::zoom_controls::ZoomControls;
    ///
    /// assert_eq!(ZoomControls::new().zoom_value(), None);
    /// ```
    pub fn zoom_value(&self) -> Option<f32> {
        self.zoom
    }

    /// Feeds the current zoom for the readout.
    ///
    /// ```
    /// use martensite::widgets::zoom_controls::ZoomControls;
    ///
    /// let mut z = ZoomControls::new();
    /// z.set_zoom(1.5);
    /// assert_eq!(z.zoom_value(), Some(1.5));
    /// ```
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = Some(zoom.clamp(self.min_zoom, self.max_zoom));
    }

    /// Hides the readout.
    ///
    /// ```
    /// use martensite::widgets::zoom_controls::ZoomControls;
    ///
    /// let mut z = ZoomControls::new().zoom(2.0);
    /// z.clear_zoom();
    /// assert_eq!(z.zoom_value(), None);
    /// ```
    pub fn clear_zoom(&mut self) {
        self.zoom = None;
    }

    /// Drains the last requested action.
    ///
    /// ```
    /// use martensite::widgets::zoom_controls::ZoomControls;
    ///
    /// assert_eq!(ZoomControls::new().take_action(), None);
    /// ```
    pub fn take_action(&mut self) -> Option<ZoomAction> {
        self.action.take()
    }

    /// Visible action list in paint order — `−` leads in horizontal
    /// rows (the reading-order idiom); the vertical stack keeps `+`
    /// on top.
    fn actions(&self) -> Vec<ZoomAction> {
        let mut v = if self.horizontal {
            vec![ZoomAction::ZoomOut, ZoomAction::ZoomIn]
        } else {
            vec![ZoomAction::ZoomIn, ZoomAction::ZoomOut]
        };
        if self.show_fit {
            v.push(ZoomAction::Fit);
        }
        if self.show_reset {
            v.push(ZoomAction::Reset);
        }
        v
    }

    /// Button glyph.
    fn glyph(action: ZoomAction) -> &'static str {
        match action {
            ZoomAction::ZoomIn => "+",
            ZoomAction::ZoomOut => "−",
            ZoomAction::Fit => "⤢",
            ZoomAction::Reset => "1:1",
        }
    }
}

impl Widget for ZoomControls {
    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let btn = cx.pt(BTN_PT);
        if self.horizontal {
            let gap = cx.pt(GAP_PT) * (self.button_count() as f32 - 1.0).max(0.0);
            return Vec2::new(
                (btn * self.button_count() as f32 + gap).min(constraints.max_size.x.max(0.0)),
                btn.min(constraints.max_size.y.max(0.0)),
            );
        }
        let h = btn * self.button_count() as f32
            + if self.zoom.is_some() && self.show_readout {
                cx.pt(READOUT_PT)
            } else {
                0.0
            };
        Vec2::new(
            btn.min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(20.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.rects.clear();
        let btn = BTN_PT * cx.scale;
        if self.horizontal {
            let gap = GAP_PT * cx.scale;
            let mut x = bounds.min_x();
            for action in self.actions() {
                self.rects
                    .push((action, Rect::new(x, bounds.min_y(), btn, btn)));
                x += btn + gap;
            }
            return;
        }
        let mut y = bounds.min_y();
        for action in self.actions() {
            self.rects
                .push((action, Rect::new(bounds.min_x(), y, btn, btn)));
            y += btn;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!(
            "{}{}",
            self.label,
            self.zoom
                .filter(|_| self.show_readout)
                .map(|z| format!(" — {:.0}%", z * 100.0))
                .unwrap_or_default()
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some((action, _)) = self
                    .rects
                    .iter()
                    .find(|(a, r)| r.contains(*position) && self.action_enabled(*a))
                {
                    self.held = Some(*action);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(action) = self.held.take() {
                    if self
                        .rects
                        .iter()
                        .any(|(a, r)| *a == action && r.contains(*position))
                    {
                        self.action = Some(action);
                    }
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "+" | "=" if self.action_enabled(ZoomAction::ZoomIn) => {
                    self.action = Some(ZoomAction::ZoomIn);
                    EventResponse::RequestRepaint
                }
                "-" | "_" if self.action_enabled(ZoomAction::ZoomOut) => {
                    self.action = Some(ZoomAction::ZoomOut);
                    EventResponse::RequestRepaint
                }
                "0" => {
                    self.action = Some(ZoomAction::Fit);
                    EventResponse::RequestRepaint
                }
                "1" => {
                    self.action = Some(ZoomAction::Reset);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let size = FONT_PT * s;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let fg = cx.color(TokenKey::TextColor, FG);
        let dim = cx.color(TokenKey::TextMutedColor, DIM);
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        for (action, rect) in &self.rects {
            let fill = if self.held == Some(*action) {
                cx.color(TokenKey::PrimaryColor, FACE_DOWN)
            } else {
                cx.color(TokenKey::SurfaceColor, FACE)
            };
            let krect = kurbo::Rect::new(
                f64::from(rect.min_x()),
                f64::from(rect.min_y()),
                f64::from(rect.max_x()),
                f64::from(rect.max_y()),
            );
            let shape = &martensite_core::shape::Shape::rounded(4.0 * s);
            cx.list.push_fill_shape(krect, shape, fill);
            cx.list.push_stroke_shape(krect, shape, s, edge);
            let glyph = Self::glyph(*action);
            let gw = painter
                .and_then(|p| p.measure_text(glyph, size))
                .unwrap_or(glyph.chars().count() as f32 * size * 0.55);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                krect,
                kurbo::Point::new(
                    f64::from(rect.min_x() + (rect.width() - gw).max(0.0) / 2.0),
                    f64::from(rect.min_y() + (rect.height() - size * 1.2).max(0.0) / 2.0),
                ),
                glyph,
                size,
                if self.action_enabled(*action) {
                    fg
                } else {
                    dim
                },
            );
        }
        // Percent readout under the stack — or right of the row in
        // horizontal mode.
        if let Some(zoom) = self.zoom.filter(|_| self.show_readout) {
            let readout = format!("{:.0}%", zoom * 100.0);
            let ry = if self.horizontal {
                cx.bounds.min_y() + (cx.bounds.height() - READOUT_PT * s).max(0.0) / 2.0
            } else {
                self.rects
                    .last()
                    .map(|(_, r)| r.max_y())
                    .unwrap_or(cx.bounds.min_y())
            };
            let dim = cx.color(TokenKey::TextMutedColor, DIM);
            let small = 10.0 * s;
            let rw = painter
                .and_then(|p| p.measure_text(&readout, small))
                .unwrap_or(readout.chars().count() as f32 * small * 0.55);
            let rx = if self.horizontal {
                self.rects
                    .last()
                    .map(|(_, r)| r.max_x() + 6.0 * s)
                    .unwrap_or(cx.bounds.min_x())
            } else {
                cx.bounds.min_x()
            };
            let rw_x = if self.horizontal {
                rx
            } else {
                cx.bounds.min_x() + (cx.bounds.width() - rw).max(0.0) / 2.0
            };
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(cx.bounds.min_x()),
                    f64::from(ry),
                    f64::from(cx.bounds.max_x()),
                    f64::from(ry + READOUT_PT * s),
                ),
                kurbo::Point::new(
                    f64::from(rw_x),
                    f64::from(ry + (READOUT_PT * s - small * 1.2).max(0.0) / 2.0),
                ),
                &readout,
                small,
                dim,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(z: &mut ZoomControls) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        z.layout(&mut cx, Rect::new(0.0, 0.0, 28.0, 132.0));
    }

    fn ev(z: &mut ZoomControls, e: &WidgetEvent) -> EventResponse {
        z.event(&mut EventContext {
            event: e,
            bounds: z.bounds,
            scale: 1.0,
        })
    }

    fn tap(z: &mut ZoomControls, action: ZoomAction) {
        let rect = z
            .rects
            .iter()
            .find(|(a, _)| *a == action)
            .map(|(_, r)| *r)
            .unwrap();
        let p = Vec2::new(
            (rect.min_x() + rect.max_x()) / 2.0,
            (rect.min_y() + rect.max_y()) / 2.0,
        );
        ev(
            z,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: p,
                count: 1,
            },
        );
        ev(
            z,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: p,
            },
        );
    }

    #[test]
    fn buttons_park_actions() {
        let mut z = ZoomControls::new();
        laid_out(&mut z);
        tap(&mut z, ZoomAction::ZoomIn);
        assert_eq!(z.take_action(), Some(ZoomAction::ZoomIn));
        tap(&mut z, ZoomAction::Fit);
        assert_eq!(z.take_action(), Some(ZoomAction::Fit));
        tap(&mut z, ZoomAction::Reset);
        assert_eq!(z.take_action(), Some(ZoomAction::Reset));
    }

    #[test]
    fn keys_park_actions() {
        let mut z = ZoomControls::new();
        laid_out(&mut z);
        for (key, want) in [
            ("+", ZoomAction::ZoomIn),
            ("-", ZoomAction::ZoomOut),
            ("0", ZoomAction::Fit),
            ("1", ZoomAction::Reset),
        ] {
            ev(
                &mut z,
                &WidgetEvent::KeyPressed {
                    key: key.to_string(),
                    repeat: false,
                },
            );
            assert_eq!(z.take_action(), Some(want));
        }
    }

    #[test]
    fn horizontal_rows_lead_with_zoom_out() {
        let mut z = ZoomControls::new().horizontal(true).fit(false).reset(false);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        z.layout(&mut cx, Rect::new(0.0, 0.0, 62.0, 28.0));
        assert_eq!(z.rects[0].0, ZoomAction::ZoomOut);
        assert_eq!(z.rects[1].0, ZoomAction::ZoomIn);
    }

    #[test]
    fn horizontal_buttons_have_gap() {
        let mut z = ZoomControls::new().horizontal(true).fit(false).reset(false);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        z.layout(&mut cx, Rect::new(0.0, 0.0, 62.0, 28.0));
        let [(_, a), (_, b)] = z.rects.as_slice() else {
            panic!("expected two buttons")
        };
        let gap = b.min_x() - a.max_x();
        assert!(gap > 0.0, "buttons overlap: gap {gap}");
        assert!((gap - 6.0).abs() < 1e-3, "gap {gap} != 6pt");
        // Vertical stacks stay joined.
        let mut v = ZoomControls::new().fit(false).reset(false);
        v.layout(&mut cx, Rect::new(0.0, 0.0, 28.0, 56.0));
        assert!((v.rects[1].1.min_y() - v.rects[0].1.max_y()).abs() < 1e-3);
    }

    #[test]
    fn hidden_buttons_skip() {
        let mut z = ZoomControls::new().fit(false).reset(false);
        laid_out(&mut z);
        assert_eq!(z.rects.len(), 2);
    }

    #[test]
    fn paint_without_painter() {
        let mut z = ZoomControls::new().zoom(1.5);
        laid_out(&mut z);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        z.paint(&mut PaintContext {
            list: &mut list,
            bounds: z.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
