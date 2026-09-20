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
        self.zoom = Some(zoom.clamp(0.01, 100.0));
        self
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
        self.zoom = Some(zoom.clamp(0.01, 100.0));
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

    /// Visible action list in paint order.
    fn actions(&self) -> Vec<ZoomAction> {
        let mut v = vec![ZoomAction::ZoomIn, ZoomAction::ZoomOut];
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
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let btn = cx.pt(BTN_PT);
        let h = btn * self.button_count() as f32
            + if self.zoom.is_some() {
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
                if let Some((action, _)) = self.rects.iter().find(|(_, r)| r.contains(*position)) {
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
                "+" | "=" => {
                    self.action = Some(ZoomAction::ZoomIn);
                    EventResponse::RequestRepaint
                }
                "-" | "_" => {
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
                fg,
            );
        }
        // Percent readout under the stack.
        if let Some(zoom) = self.zoom {
            let readout = format!("{:.0}%", zoom * 100.0);
            let ry = self
                .rects
                .last()
                .map(|(_, r)| r.max_y())
                .unwrap_or(cx.bounds.min_y());
            let dim = cx.color(TokenKey::TextMutedColor, DIM);
            let small = 10.0 * s;
            let rw = painter
                .and_then(|p| p.measure_text(&readout, small))
                .unwrap_or(readout.chars().count() as f32 * small * 0.55);
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
                    f64::from(cx.bounds.min_x() + (cx.bounds.width() - rw).max(0.0) / 2.0),
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
