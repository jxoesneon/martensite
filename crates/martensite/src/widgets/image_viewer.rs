//! `ImageViewer` — a pan/zoom viewport over an [`ImageData`]
//! (Preview-app / photo-viewer idiom).
//!
//! The image fits the viewport at `zoom == 1.0`; `Scroll` zooms
//! around the pointer, primary-drag pans, and a double-click resets
//! to fit. `+`/`-`/`0` do the same from the keyboard. The image is
//! clipped to the viewport and the pan is clamped so at least a
//! quarter of it stays reachable.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::image_viewer::ImageViewer;
//! use martensite_core::ImageData;
//!
//! let data = ImageData::from_rgba(4, 4, vec![0; 4 * 4 * 4]).unwrap();
//! let viewer = ImageViewer::new(data);
//! assert_eq!(viewer.zoom(), 1.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, ImageData, LayoutConstraints, LayoutContext, PaintContext,
    PointerButton, Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const ZOOM_MIN: f32 = 0.1;
const ZOOM_MAX: f32 = 20.0;
const ZOOM_STEP: f32 = 1.25;

const SURFACE: [u8; 4] = [30, 30, 34, 255];
const CHECKER_A: [u8; 4] = [200, 200, 200, 255];
const CHECKER_B: [u8; 4] = [160, 160, 160, 255];

/// A pan/zoom image viewport — see the module docs.
///
/// ```
/// use martensite::widgets::image_viewer::ImageViewer;
/// use martensite_core::ImageData;
///
/// let data = ImageData::from_rgba(2, 2, vec![0; 2 * 2 * 4]).unwrap();
/// let viewer = ImageViewer::new(data);
/// assert_eq!(viewer.zoom(), 1.0);
/// ```
pub struct ImageViewer {
    /// When `false` input is ignored.
    pub enabled: bool,
    /// Accessibility label.
    pub label: String,
    /// Paint a checkerboard under the image (transparency cue).
    pub checker: bool,
    image: ImageData,
    zoom: f32,
    pan: Vec2,
    dragging: bool,
    last: Vec2,
    bounds: Rect,
}

impl ImageViewer {
    /// Creates a viewer fitted to its viewport.
    ///
    /// ```
    /// use martensite::widgets::image_viewer::ImageViewer;
    /// use martensite_core::ImageData;
    ///
    /// let data = ImageData::from_rgba(2, 2, vec![0; 2 * 2 * 4]).unwrap();
    /// assert_eq!(ImageViewer::new(data).zoom(), 1.0);
    /// ```
    pub fn new(image: ImageData) -> Self {
        Self {
            enabled: true,
            label: "Image".to_string(),
            checker: true,
            image,
            zoom: 1.0,
            pan: Vec2::ZERO,
            dragging: false,
            last: Vec2::ZERO,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::image_viewer::ImageViewer;
    /// use martensite_core::ImageData;
    ///
    /// let data = ImageData::from_rgba(1, 1, vec![0; 4]).unwrap();
    /// let viewer = ImageViewer::new(data).label("Scan");
    /// assert_eq!(viewer.label, "Scan");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Toggles the transparency checkerboard.
    ///
    /// ```
    /// use martensite::widgets::image_viewer::ImageViewer;
    /// use martensite_core::ImageData;
    ///
    /// let data = ImageData::from_rgba(1, 1, vec![0; 4]).unwrap();
    /// let viewer = ImageViewer::new(data).checker(false);
    /// assert!(!viewer.checker);
    /// ```
    pub fn checker(mut self, on: bool) -> Self {
        self.checker = on;
        self
    }

    /// Enables or disables interaction.
    ///
    /// ```
    /// use martensite::widgets::image_viewer::ImageViewer;
    /// use martensite_core::ImageData;
    ///
    /// let data = ImageData::from_rgba(1, 1, vec![0; 4]).unwrap();
    /// assert!(!ImageViewer::new(data).enabled(false).enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Current zoom (1.0 = fit).
    ///
    /// ```
    /// use martensite::widgets::image_viewer::ImageViewer;
    /// use martensite_core::ImageData;
    ///
    /// let data = ImageData::from_rgba(1, 1, vec![0; 4]).unwrap();
    /// assert_eq!(ImageViewer::new(data).zoom(), 1.0);
    /// ```
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Sets zoom and pan programmatically.
    ///
    /// ```
    /// use martensite::widgets::image_viewer::ImageViewer;
    /// use martensite_core::ImageData;
    ///
    /// let data = ImageData::from_rgba(1, 1, vec![0; 4]).unwrap();
    /// let mut viewer = ImageViewer::new(data);
    /// viewer.set_view(2.0, glam::Vec2::new(10.0, 0.0));
    /// assert_eq!(viewer.zoom(), 2.0);
    /// ```
    pub fn set_view(&mut self, zoom: f32, pan: Vec2) {
        self.zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        self.pan = pan;
    }

    /// Resets to fit.
    ///
    /// ```
    /// use martensite::widgets::image_viewer::ImageViewer;
    /// use martensite_core::ImageData;
    ///
    /// let data = ImageData::from_rgba(1, 1, vec![0; 4]).unwrap();
    /// let mut viewer = ImageViewer::new(data);
    /// viewer.set_view(4.0, glam::Vec2::ZERO);
    /// viewer.reset();
    /// assert_eq!(viewer.zoom(), 1.0);
    /// ```
    pub fn reset(&mut self) {
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
    }

    /// Fit scale: viewport → image pixel ratio at `zoom == 1`.
    fn fit_scale(&self) -> f32 {
        let iw = self.image.width().max(1) as f32;
        let ih = self.image.height().max(1) as f32;
        (self.bounds.width() / iw)
            .min(self.bounds.height() / ih)
            .max(0.001)
    }

    /// Destination rect of the image under current view.
    fn dest(&self) -> Rect {
        let s = self.fit_scale() * self.zoom;
        let w = self.image.width() as f32 * s;
        let h = self.image.height() as f32 * s;
        Rect::new(
            self.bounds.min_x() + (self.bounds.width() - w) / 2.0 + self.pan.x,
            self.bounds.min_y() + (self.bounds.height() - h) / 2.0 + self.pan.y,
            w,
            h,
        )
    }

    /// Zooms keeping `anchor` stationary.
    fn zoom_at(&mut self, anchor: Vec2, factor: f32) {
        let before = self.dest();
        self.zoom = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        let after = self.dest();
        // Adjust pan so the content point under the cursor stays put:
        // shift the pan by how far the anchor-relative point moved
        // between the old and new destination rects.
        let rel = Vec2::new(
            (anchor.x - before.min_x()) / before.width().max(0.001),
            (anchor.y - before.min_y()) / before.height().max(0.001),
        );
        self.pan += Vec2::new(
            (before.min_x() + rel.x * before.width()) - (after.min_x() + rel.x * after.width()),
            (before.min_y() + rel.y * before.height()) - (after.min_y() + rel.y * after.height()),
        );
        self.clamp_pan();
    }

    /// Keeps at least a quarter of the image inside the viewport.
    fn clamp_pan(&mut self) {
        let dest = self.dest();
        let keep_x = (dest.width() * 0.25).min(self.bounds.width());
        let keep_y = (dest.height() * 0.25).min(self.bounds.height());
        if dest.min_x() > self.bounds.max_x() - keep_x {
            self.pan.x -= dest.min_x() - (self.bounds.max_x() - keep_x);
        }
        if dest.max_x() < self.bounds.min_x() + keep_x {
            self.pan.x += (self.bounds.min_x() + keep_x) - dest.max_x();
        }
        if dest.min_y() > self.bounds.max_y() - keep_y {
            self.pan.y -= dest.min_y() - (self.bounds.max_y() - keep_y);
        }
        if dest.max_y() < self.bounds.min_y() + keep_y {
            self.pan.y += (self.bounds.min_y() + keep_y) - dest.max_y();
        }
    }
}

impl Widget for ImageViewer {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(240.0);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(64.0, 64.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        node.set_label(self.label.clone());
        node.set_description(format!("zoom {:.0}%", self.zoom * 100.0));
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::Scroll { position, delta } => {
                if self.bounds.contains(*position) {
                    self.zoom_at(*position, ZOOM_STEP.powf(-delta.y.signum()));
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                count,
            } => {
                if self.bounds.contains(*position) {
                    if *count == 2 {
                        self.reset();
                        return EventResponse::RequestRepaint;
                    }
                    self.dragging = true;
                    self.last = *position;
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if self.dragging {
                    self.pan += *position - self.last;
                    self.last = *position;
                    self.clamp_pan();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.dragging {
                    self.dragging = false;
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "+" | "=" => {
                    let c = Vec2::new(
                        self.bounds.min_x() + self.bounds.width() / 2.0,
                        self.bounds.min_y() + self.bounds.height() / 2.0,
                    );
                    self.zoom_at(c, ZOOM_STEP);
                    EventResponse::RequestRepaint
                }
                "-" => {
                    let c = Vec2::new(
                        self.bounds.min_x() + self.bounds.width() / 2.0,
                        self.bounds.min_y() + self.bounds.height() / 2.0,
                    );
                    self.zoom_at(c, 1.0 / ZOOM_STEP);
                    EventResponse::RequestRepaint
                }
                "0" => {
                    self.reset();
                    EventResponse::RequestRepaint
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
        let vp = f(self.bounds);
        cx.list
            .push_fill_rect(vp, cx.color(TokenKey::SurfaceColor, SURFACE));
        cx.list.push_clip(vp);

        let dest = self.dest();
        if self.checker {
            // Checkerboard behind the image — 8pt cells clipped to
            // the dest rect so alpha reads as transparency.
            let cell = cx.pt(8.0).max(1.0);
            let a = cx.color(TokenKey::BorderColor, CHECKER_A);
            let b = cx.color(TokenKey::SurfaceColor, CHECKER_B);
            let mut y = dest.min_y();
            let mut row = 0u32;
            while y < dest.max_y() {
                let mut x = dest.min_x();
                let mut col = row;
                while x < dest.max_x() {
                    let r = kurbo::Rect::new(
                        f64::from(x),
                        f64::from(y),
                        f64::from((x + cell).min(dest.max_x())),
                        f64::from((y + cell).min(dest.max_y())),
                    );
                    cx.list
                        .push_fill_rect(r, if col.is_multiple_of(2) { a } else { b });
                    x += cell;
                    col += 1;
                }
                y += cell;
                row += 1;
            }
        }
        cx.list.push_image(f(dest), self.image.clone());
        cx.list.pop_clip();
    }
}

impl std::fmt::Debug for ImageViewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageViewer")
            .field("zoom", &self.zoom)
            .field("pan", &self.pan)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn data() -> ImageData {
        ImageData::from_rgba(100, 50, vec![128; 100 * 50 * 4]).unwrap()
    }

    fn laid_out(v: &mut ImageViewer, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        v.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        v.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev(v: &mut ImageViewer, event: WidgetEvent) {
        v.event(&mut EventContext {
            event: &event,
            bounds: Rect::new(0.0, 0.0, 400.0, 300.0),
            scale: 1.0,
        });
    }

    #[test]
    fn scroll_zooms_in_and_out() {
        let mut v = ImageViewer::new(data());
        laid_out(&mut v, 400.0, 300.0);
        ev(
            &mut v,
            WidgetEvent::Scroll {
                position: Vec2::new(200.0, 150.0),
                delta: Vec2::new(0.0, -1.0),
            },
        );
        assert!(v.zoom() > 1.0);
        ev(
            &mut v,
            WidgetEvent::Scroll {
                position: Vec2::new(200.0, 150.0),
                delta: Vec2::new(0.0, 1.0),
            },
        );
        assert!((v.zoom() - 1.0).abs() < 0.001);
    }

    #[test]
    fn zoom_clamped() {
        let mut v = ImageViewer::new(data());
        laid_out(&mut v, 400.0, 300.0);
        for _ in 0..40 {
            ev(
                &mut v,
                WidgetEvent::Scroll {
                    position: Vec2::new(200.0, 150.0),
                    delta: Vec2::new(0.0, -1.0),
                },
            );
        }
        assert!(v.zoom() <= ZOOM_MAX + 0.001);
    }

    #[test]
    fn drag_pans() {
        let mut v = ImageViewer::new(data());
        laid_out(&mut v, 400.0, 300.0);
        v.set_view(2.0, Vec2::ZERO);
        ev(
            &mut v,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(200.0, 150.0),
                count: 1,
            },
        );
        ev(
            &mut v,
            WidgetEvent::PointerMoved {
                position: Vec2::new(230.0, 170.0),
            },
        );
        assert!(v.pan.x > 0.0 && v.pan.y > 0.0);
    }

    #[test]
    fn pan_clamps_to_quarter_visible() {
        let mut v = ImageViewer::new(data());
        laid_out(&mut v, 400.0, 300.0);
        v.set_view(2.0, Vec2::ZERO);
        ev(
            &mut v,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(10.0, 10.0),
                count: 1,
            },
        );
        ev(
            &mut v,
            WidgetEvent::PointerMoved {
                position: Vec2::new(-4000.0, 10.0),
            },
        );
        let dest = v.dest();
        assert!(dest.max_x() > v.bounds.min_x());
    }

    #[test]
    fn double_click_resets() {
        let mut v = ImageViewer::new(data());
        laid_out(&mut v, 400.0, 300.0);
        v.set_view(3.0, Vec2::new(50.0, 0.0));
        ev(
            &mut v,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(200.0, 150.0),
                count: 2,
            },
        );
        assert_eq!(v.zoom(), 1.0);
        assert_eq!(v.pan, Vec2::ZERO);
    }

    #[test]
    fn keyboard_zoom() {
        let mut v = ImageViewer::new(data());
        laid_out(&mut v, 400.0, 300.0);
        for key in ["+", "+", "0"] {
            ev(
                &mut v,
                WidgetEvent::KeyPressed {
                    key: key.to_string(),
                    repeat: false,
                },
            );
        }
        assert_eq!(v.zoom(), 1.0);
    }

    #[test]
    fn scroll_outside_ignored() {
        let mut v = ImageViewer::new(data());
        laid_out(&mut v, 400.0, 300.0);
        ev(
            &mut v,
            WidgetEvent::Scroll {
                position: Vec2::new(500.0, 500.0),
                delta: Vec2::new(0.0, -1.0),
            },
        );
        assert_eq!(v.zoom(), 1.0);
    }

    #[test]
    fn disabled_inert() {
        let mut v = ImageViewer::new(data()).enabled(false);
        laid_out(&mut v, 400.0, 300.0);
        ev(
            &mut v,
            WidgetEvent::Scroll {
                position: Vec2::new(200.0, 150.0),
                delta: Vec2::new(0.0, -1.0),
            },
        );
        assert_eq!(v.zoom(), 1.0);
    }
}
