//! `Magnifier` — a loupe that renders a zoomed region of a source
//! [`ImageData`] snapshot (the design-tool / color-pick loupe idiom).
//!
//! The host feeds a snapshot through [`Magnifier::source`]; the
//! pointer's position inside the widget picks the sample point in
//! source coordinates, and the lens circle shows that neighborhood at
//! [`Magnifier::zoom`]. `+`/`-` change the zoom, arrows nudge the
//! sample point, and clicking parks the source-space point in
//! [`Magnifier::take_picked`] (for eyedropper-style flows).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::magnifier::Magnifier;
//!
//! let m = Magnifier::new().zoom(8.0);
//! assert_eq!(m.zoom_value(), 8.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, ImageData, LayoutConstraints, LayoutContext, PaintContext,
    PointerButton, Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const LENS_PT: f32 = 140.0;

const FACE: [u8; 4] = [24, 24, 28, 255];
const RING: [u8; 4] = [70, 72, 78, 255];
const CROSS: [u8; 4] = [96, 165, 250, 200];
const TEXT: [u8; 4] = [200, 200, 206, 255];

/// A zoom loupe over a source snapshot — see the module docs.
///
/// ```
/// use martensite::widgets::magnifier::Magnifier;
///
/// assert_eq!(Magnifier::new().zoom_value(), 4.0);
/// ```
pub struct Magnifier {
    /// Accessibility label.
    pub label: String,
    source: Option<ImageData>,
    zoom: f32,
    /// Sample point in *source* coordinates.
    focus: Vec2,
    picked: Option<Vec2>,
    /// Shows the zoom factor in the corner.
    show_zoom: bool,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for Magnifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Magnifier")
            .field("zoom", &self.zoom)
            .field("focus", &self.focus)
            .finish()
    }
}

impl Default for Magnifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Magnifier {
    /// Empty loupe at 4× zoom.
    ///
    /// ```
    /// use martensite::widgets::magnifier::Magnifier;
    ///
    /// assert!(Magnifier::new().source_image().is_none());
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Magnifier".to_string(),
            source: None,
            zoom: 4.0,
            focus: Vec2::ZERO,
            picked: None,
            show_zoom: true,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::magnifier::Magnifier;
    ///
    /// assert_eq!(Magnifier::new().label("Loupe").label, "Loupe");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::magnifier::Magnifier;
    ///
    /// let _ = Magnifier::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Source snapshot to magnify.
    ///
    /// ```
    /// use martensite::widgets::magnifier::Magnifier;
    /// use martensite_core::ImageData;
    ///
    /// let img = ImageData::from_rgba(2, 2, vec![0; 16]).unwrap();
    /// assert!(Magnifier::new().source(img).source_image().is_some());
    /// ```
    pub fn source(mut self, img: ImageData) -> Self {
        self.source = Some(img);
        self
    }

    /// The current source snapshot.
    ///
    /// ```
    /// use martensite::widgets::magnifier::Magnifier;
    ///
    /// assert!(Magnifier::new().source_image().is_none());
    /// ```
    pub fn source_image(&self) -> Option<&ImageData> {
        self.source.as_ref()
    }

    /// Replaces the source snapshot.
    ///
    /// ```
    /// use martensite::widgets::magnifier::Magnifier;
    /// use martensite_core::ImageData;
    ///
    /// let img = ImageData::from_rgba(1, 1, vec![0; 4]).unwrap();
    /// let mut m = Magnifier::new();
    /// m.set_source(img);
    /// assert!(m.source_image().is_some());
    /// ```
    pub fn set_source(&mut self, img: ImageData) {
        self.source = Some(img);
    }

    /// Magnification factor (`1`–`16`, clamped).
    ///
    /// ```
    /// use martensite::widgets::magnifier::Magnifier;
    ///
    /// assert_eq!(Magnifier::new().zoom(99.0).zoom_value(), 16.0);
    /// ```
    pub fn zoom(mut self, z: f32) -> Self {
        self.zoom = z.clamp(1.0, 16.0);
        self
    }

    /// Current zoom factor.
    ///
    /// ```
    /// use martensite::widgets::magnifier::Magnifier;
    ///
    /// assert_eq!(Magnifier::new().zoom_value(), 4.0);
    /// ```
    pub fn zoom_value(&self) -> f32 {
        self.zoom
    }

    /// Hides the zoom-factor caption when `off`.
    ///
    /// ```
    /// use martensite::widgets::magnifier::Magnifier;
    ///
    /// assert!(!Magnifier::new().zoom_caption(false).has_zoom_caption());
    /// ```
    pub fn zoom_caption(mut self, on: bool) -> Self {
        self.show_zoom = on;
        self
    }

    /// Whether the zoom caption is drawn.
    ///
    /// ```
    /// use martensite::widgets::magnifier::Magnifier;
    ///
    /// assert!(Magnifier::new().has_zoom_caption());
    /// ```
    pub fn has_zoom_caption(&self) -> bool {
        self.show_zoom
    }

    /// Sample point in source coordinates.
    ///
    /// ```
    /// use martensite::widgets::magnifier::Magnifier;
    ///
    /// assert_eq!(Magnifier::new().focus_point(), glam::Vec2::ZERO);
    /// ```
    pub fn focus_point(&self) -> Vec2 {
        self.focus
    }

    /// Moves the sample point (clamped to the source bounds).
    ///
    /// ```
    /// use martensite::widgets::magnifier::Magnifier;
    /// use glam::Vec2;
    ///
    /// let mut m = Magnifier::new();
    /// m.set_focus(Vec2::new(10.0, 20.0));
    /// assert_eq!(m.focus_point(), Vec2::new(10.0, 20.0));
    /// ```
    pub fn set_focus(&mut self, p: Vec2) {
        self.focus = self.clamp_focus(p);
    }

    /// Drains the source-space point of the last click.
    ///
    /// ```
    /// use martensite::widgets::magnifier::Magnifier;
    ///
    /// assert_eq!(Magnifier::new().take_picked(), None);
    /// ```
    pub fn take_picked(&mut self) -> Option<Vec2> {
        self.picked.take()
    }

    /// Clamps `p` to the source image bounds.
    fn clamp_focus(&self, p: Vec2) -> Vec2 {
        match &self.source {
            Some(img) => Vec2::new(
                p.x.clamp(0.0, img.width() as f32),
                p.y.clamp(0.0, img.height() as f32),
            ),
            None => Vec2::new(p.x.max(0.0), p.y.max(0.0)),
        }
    }

    /// Lens circle `(center, radius)` in device space.
    fn lens(&self) -> (Vec2, f32) {
        let r = self.bounds.width().min(self.bounds.height()) / 2.0;
        (
            self.bounds.origin + self.bounds.size / 2.0,
            (r - 2.0 * self.scale).max(0.0),
        )
    }
}

impl Widget for Magnifier {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let side = cx.pt(LENS_PT);
        Vec2::new(
            side.min(constraints.max_size.x.max(0.0)),
            side.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(48.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.focus = self.clamp_focus(self.focus);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(format!(
            "{} — {:.0}× at ({:.0}, {:.0})",
            self.label, self.zoom, self.focus.x, self.focus.y
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                if self.bounds.contains(*position) {
                    self.focus = self.clamp_focus(*position - self.bounds.origin);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if self.bounds.contains(*position) {
                    self.focus = self.clamp_focus(*position - self.bounds.origin);
                    self.picked = Some(self.focus);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "+" | "=" => {
                    self.zoom = (self.zoom * 1.25).min(16.0);
                    EventResponse::RequestRepaint
                }
                "-" | "_" => {
                    self.zoom = (self.zoom / 1.25).max(1.0);
                    EventResponse::RequestRepaint
                }
                "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" => {
                    let step = match key.as_str() {
                        "ArrowLeft" => Vec2::new(-1.0, 0.0),
                        "ArrowRight" => Vec2::new(1.0, 0.0),
                        "ArrowUp" => Vec2::new(0.0, -1.0),
                        _ => Vec2::new(0.0, 1.0),
                    };
                    self.focus = self.clamp_focus(self.focus + step);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let (c, r) = self.lens();
        let lens = kurbo::Rect::new(
            f64::from(c.x - r),
            f64::from(c.y - r),
            f64::from(c.x + r),
            f64::from(c.y + r),
        );
        // Dark lens backplate, then the zoomed source clipped to a circle.
        cx.list.push_fill_shape(
            lens,
            &martensite_core::shape::Shape::ELLIPSE,
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        if let Some(img) = &self.source {
            let z = self.zoom;
            let dw = img.width() as f32 * z;
            let dh = img.height() as f32 * z;
            let dest = kurbo::Rect::new(
                f64::from(c.x - self.focus.x * z),
                f64::from(c.y - self.focus.y * z),
                f64::from(c.x - self.focus.x * z + dw),
                f64::from(c.y - self.focus.y * z + dh),
            );
            cx.list
                .push_clip_shape(lens, &martensite_core::shape::Shape::ELLIPSE);
            cx.list.push_image(dest, img.clone());
            cx.list.pop_clip();
        }
        // Crosshair on the sample pixel.
        let cross = cx.color(TokenKey::AccentColor, CROSS);
        let arm = r * 0.25;
        let mut h = kurbo::BezPath::new();
        h.move_to((f64::from(c.x - arm), f64::from(c.y)));
        h.line_to((f64::from(c.x + arm), f64::from(c.y)));
        cx.list.push_stroke_path(h, self.scale, cross);
        let mut v = kurbo::BezPath::new();
        v.move_to((f64::from(c.x), f64::from(c.y - arm)));
        v.line_to((f64::from(c.x), f64::from(c.y + arm)));
        cx.list.push_stroke_path(v, self.scale, cross);
        // Lens ring.
        cx.list.push_stroke_shape(
            lens,
            &martensite_core::shape::Shape::ELLIPSE,
            2.0 * self.scale,
            cx.color(TokenKey::BorderColor, RING),
        );
        // Zoom caption.
        if self.show_zoom {
            let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
            let size = 10.0 * self.scale;
            let t = format!("{:.0}×", self.zoom);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                krect(self.bounds),
                kurbo::Point::new(
                    f64::from(self.bounds.min_x() + 4.0 * self.scale),
                    f64::from(self.bounds.max_y() - size - 4.0 * self.scale),
                ),
                &t,
                size,
                cx.color(TokenKey::TextMutedColor, TEXT),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PaintList};

    fn laid_out(m: &mut Magnifier, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        m.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        m.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev(m: &mut Magnifier, e: &WidgetEvent) {
        m.event(&mut EventContext {
            event: e,
            bounds: m.bounds,
            scale: 1.0,
        });
    }

    fn source() -> ImageData {
        ImageData::from_rgba(4, 4, vec![0; 64]).unwrap()
    }

    #[test]
    fn zoom_clamps_and_keys() {
        let mut m = Magnifier::new();
        laid_out(&mut m, 140.0, 140.0);
        ev(
            &mut m,
            &WidgetEvent::KeyPressed {
                key: "+".to_string(),
                repeat: false,
            },
        );
        assert!((m.zoom_value() - 5.0).abs() < 0.01);
        for _ in 0..20 {
            ev(
                &mut m,
                &WidgetEvent::KeyPressed {
                    key: "+".to_string(),
                    repeat: false,
                },
            );
        }
        assert_eq!(m.zoom_value(), 16.0);
        ev(
            &mut m,
            &WidgetEvent::KeyPressed {
                key: "-".to_string(),
                repeat: false,
            },
        );
        assert!(m.zoom_value() < 16.0);
    }

    #[test]
    fn pointer_tracks_source_coords() {
        let mut m = Magnifier::new().source(source());
        laid_out(&mut m, 140.0, 140.0);
        ev(
            &mut m,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(30.0, 40.0),
            },
        );
        // Clamped to the 4×4 source bounds.
        assert_eq!(m.focus_point(), Vec2::new(4.0, 4.0));
    }

    #[test]
    fn click_parks_pick() {
        let mut m = Magnifier::new().source(source());
        laid_out(&mut m, 140.0, 140.0);
        ev(
            &mut m,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(2.0, 3.0),
                count: 1,
            },
        );
        assert_eq!(m.take_picked(), Some(Vec2::new(2.0, 3.0)));
        assert_eq!(m.take_picked(), None);
    }

    #[test]
    fn arrows_nudge_focus() {
        let mut m = Magnifier::new().source(source());
        laid_out(&mut m, 140.0, 140.0);
        m.set_focus(Vec2::new(2.0, 2.0));
        ev(
            &mut m,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        assert_eq!(m.focus_point(), Vec2::new(3.0, 2.0));
        ev(
            &mut m,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        assert_eq!(m.focus_point(), Vec2::new(4.0, 2.0)); // clamped
    }

    #[test]
    fn paints() {
        let mut m = Magnifier::new().source(source());
        laid_out(&mut m, 140.0, 140.0);
        let theme = martensite_theme::Theme::new("test");
        let mut list = PaintList::new();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: m.bounds,
            scale: 1.0,
            theme: &theme,
            text_painter: None,
        };
        m.paint(&mut cx);
    }
}
