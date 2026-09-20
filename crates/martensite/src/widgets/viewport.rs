//! `Viewport` — a pannable, zoomable canvas hosting one child
//! (Figma / map / CAD canvas idiom).
//!
//! Unlike [`ScrollView`](crate::widgets::ScrollView), which scrolls a
//! document at 1×, `Viewport` scales its content: the child is laid
//! out at `scale * zoom`, so text, strokes, and spacing magnify
//! uniformly. A dotted canvas grid paints under the content.
//!
//! - Middle-button drag (or a primary drag the content ignores) pans.
//! - Scroll wheel zooms around the cursor; `+`/`=`/`-` zoom around
//!   the center, `0` fits the content, `Home` resets to 1×.
//! - [`Viewport::to_content`] / [`Viewport::to_screen`] convert
//!   between screen and content coordinates; every pan/zoom change
//!   parks `(pan, zoom)` in [`Viewport::take_changed`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::viewport::Viewport;
//! use martensite::widgets::Text;
//!
//! let v = Viewport::new().child(Text::new("canvas"));
//! assert_eq!(v.zoom_value(), 1.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, HotNode, LayoutConstraints, LayoutContext, NodeFlags,
    PaintContext, PointerButton, Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 8.0;
const GRID_PT: f32 = 24.0;

const GRID: [u8; 4] = [90, 92, 100, 90];
const SURFACE: [u8; 4] = [28, 29, 33, 255];

/// A pannable/zoomable content canvas — see the module docs.
///
/// ```
/// use martensite::widgets::viewport::Viewport;
///
/// assert_eq!(Viewport::new().pan_offset(), glam::Vec2::ZERO);
/// ```
pub struct Viewport {
    /// Accessibility label.
    pub label: String,
    content: Box<dyn Widget>,
    /// Natural content size in px at `scale` (unzoomed).
    content_size: Vec2,
    /// Screen-px offset of the content origin from `bounds` origin.
    pan: Vec2,
    zoom: f32,
    /// On-screen rect of the zoomed content.
    content_rect: Option<Rect>,
    bounds: Rect,
    scale: f32,
    panning: Option<Vec2>,
    changed: Option<(Vec2, f32)>,
    enabled: bool,
}

impl std::fmt::Debug for Viewport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Viewport")
            .field("pan", &self.pan)
            .field("zoom", &self.zoom)
            .field("content_size", &self.content_size)
            .finish()
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Self::new()
    }
}

impl Viewport {
    /// Empty canvas at zoom 1.
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// assert_eq!(Viewport::new().zoom_value(), 1.0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Canvas".to_string(),
            content: Box::new(martensite_core::DummyWidget),
            content_size: Vec2::ZERO,
            pan: Vec2::ZERO,
            zoom: 1.0,
            content_rect: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            panning: None,
            changed: None,
            enabled: true,
        }
    }

    /// Sets the hosted content widget.
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    /// use martensite::widgets::Text;
    ///
    /// let v = Viewport::new().child(Text::new("c"));
    /// assert_eq!(v.zoom_value(), 1.0);
    /// ```
    pub fn child(mut self, content: impl Widget + 'static) -> Self {
        self.content = Box::new(content);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// assert_eq!(Viewport::new().label("Board").label, "Board");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Initial zoom (`0.1..=8.0`).
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// assert_eq!(Viewport::new().zoom(2.0).zoom_value(), 2.0);
    /// assert_eq!(Viewport::new().zoom(99.0).zoom_value(), 8.0);
    /// ```
    pub fn zoom(mut self, zoom: f32) -> Self {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self
    }

    /// Initial pan offset (screen px of the content origin).
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// assert_eq!(
    ///     Viewport::new().pan(glam::Vec2::new(4.0, 2.0)).pan_offset(),
    ///     glam::Vec2::new(4.0, 2.0)
    /// );
    /// ```
    pub fn pan(mut self, pan: Vec2) -> Self {
        self.pan = pan;
        self
    }

    /// Disabled builder — input is ignored while disabled.
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// assert!(!Viewport::new().enabled(false).is_enabled());
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Whether the canvas accepts input.
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// assert!(Viewport::new().is_enabled());
    /// ```
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Current zoom factor.
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// assert_eq!(Viewport::new().zoom(0.5).zoom_value(), 0.5);
    /// ```
    pub fn zoom_value(&self) -> f32 {
        self.zoom
    }

    /// Current pan offset (screen px of the content origin).
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// assert_eq!(Viewport::new().pan_offset(), glam::Vec2::ZERO);
    /// ```
    pub fn pan_offset(&self) -> Vec2 {
        self.pan
    }

    /// Natural (unzoomed) content size; `ZERO` before layout.
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// assert_eq!(Viewport::new().content_size(), glam::Vec2::ZERO);
    /// ```
    pub fn content_size(&self) -> Vec2 {
        self.content_size
    }

    /// Sets the zoom, keeping the given screen point stationary.
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// let mut v = Viewport::new();
    /// v.zoom_at(glam::Vec2::ZERO, 2.0);
    /// assert_eq!(v.zoom_value(), 2.0);
    /// ```
    pub fn zoom_at(&mut self, screen_pt: Vec2, zoom: f32) {
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        if (zoom - self.zoom).abs() < f32::EPSILON {
            return;
        }
        // Keep the content point under `screen_pt` fixed:
        // c = (p - o) / z  →  o' = p - c·z' = p - (p - o)·z'/z.
        let bounds_min = Vec2::new(self.bounds.min_x(), self.bounds.min_y());
        let origin = bounds_min + self.pan;
        let origin = screen_pt - (screen_pt - origin) * (zoom / self.zoom);
        self.pan = origin - bounds_min;
        self.zoom = zoom;
        self.relayout_content();
        self.changed = Some((self.pan, self.zoom));
    }

    /// Sets the pan offset directly.
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// let mut v = Viewport::new();
    /// v.set_pan(glam::Vec2::new(10.0, 5.0));
    /// assert_eq!(v.pan_offset(), glam::Vec2::new(10.0, 5.0));
    /// ```
    pub fn set_pan(&mut self, pan: Vec2) {
        self.pan = pan;
        self.relayout_content();
        self.changed = Some((self.pan, self.zoom));
    }

    /// Adds to the pan offset.
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// let mut v = Viewport::new();
    /// v.pan_by(glam::Vec2::new(3.0, -2.0));
    /// assert_eq!(v.pan_offset(), glam::Vec2::new(3.0, -2.0));
    /// ```
    pub fn pan_by(&mut self, delta: Vec2) {
        self.set_pan(self.pan + delta);
    }

    /// Resets to zoom 1 with the content origin at the top-left.
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// let mut v = Viewport::new().zoom(3.0).pan(glam::Vec2::splat(9.0));
    /// v.reset_view();
    /// assert_eq!((v.zoom_value(), v.pan_offset()), (1.0, glam::Vec2::ZERO));
    /// ```
    pub fn reset_view(&mut self) {
        self.pan = Vec2::ZERO;
        self.zoom = 1.0;
        self.relayout_content();
        self.changed = Some((self.pan, self.zoom));
    }

    /// Zooms so the whole content fits inside the bounds, centered.
    /// No-op before layout (content size unknown).
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// Viewport::new().fit_view(); // safe before layout
    /// ```
    pub fn fit_view(&mut self) {
        if self.content_size.x <= 0.0 || self.content_size.y <= 0.0 {
            return;
        }
        let z = (self.bounds.width() / self.content_size.x)
            .min(self.bounds.height() / self.content_size.y)
            .clamp(MIN_ZOOM, MAX_ZOOM);
        self.zoom = z;
        self.pan = Vec2::new(
            (self.bounds.width() - self.content_size.x * z) / 2.0,
            (self.bounds.height() - self.content_size.y * z) / 2.0,
        );
        self.relayout_content();
        self.changed = Some((self.pan, self.zoom));
    }

    /// Converts a screen-space point into content coordinates.
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// let v = Viewport::new().zoom(2.0);
    /// assert_eq!(v.to_content(glam::Vec2::new(10.0, 6.0)), glam::Vec2::new(5.0, 3.0));
    /// ```
    pub fn to_content(&self, screen_pt: Vec2) -> Vec2 {
        (screen_pt - Vec2::new(self.bounds.min_x(), self.bounds.min_y()) - self.pan) / self.zoom
    }

    /// Converts a content-space point into screen coordinates.
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// let v = Viewport::new().zoom(2.0);
    /// assert_eq!(v.to_screen(glam::Vec2::new(5.0, 3.0)), glam::Vec2::new(10.0, 6.0));
    /// ```
    pub fn to_screen(&self, content_pt: Vec2) -> Vec2 {
        Vec2::new(self.bounds.min_x(), self.bounds.min_y()) + self.pan + content_pt * self.zoom
    }

    /// Drains the last view change `(pan, zoom)`.
    ///
    /// ```
    /// use martensite::widgets::viewport::Viewport;
    ///
    /// assert_eq!(Viewport::new().take_changed(), None);
    /// ```
    pub fn take_changed(&mut self) -> Option<(Vec2, f32)> {
        self.changed.take()
    }

    /// Re-lays out the content at the current pan/zoom so
    /// `child_bounds` and hit-testing track the view.
    fn relayout_content(&mut self) {
        if self.content_size == Vec2::ZERO {
            return;
        }
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: self.scale * self.zoom,
        };
        let rect = Rect::new(
            self.bounds.min_x() + self.pan.x,
            self.bounds.min_y() + self.pan.y,
            self.content_size.x * self.zoom,
            self.content_size.y * self.zoom,
        );
        self.content_rect = Some(rect);
        self.content.layout(&mut cx, rect);
    }
}

impl Widget for Viewport {
    fn measure(&mut self, _cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Fill whatever the parent offers (canvas idiom).
        Vec2::new(
            constraints.max_size.x.max(80.0),
            constraints.max_size.y.max(80.0),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(40.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
        // Measure the content unbounded at base scale to learn its
        // natural size — the zoom pass scales the layout context.
        let mut hot = HotNode::default();
        let mut base = LayoutContext {
            hot: &mut hot,
            scale: cx.scale,
        };
        self.content_size = self.content.measure(
            &mut base,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(f32::MAX, f32::MAX),
            },
        );
        self.relayout_content();
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ScrollView);
        node.set_label(format!("{} — {:.0}%", self.label, self.zoom * 100.0));
        node.add_action(accesskit::Action::Focus);
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
                position, button, ..
            } => {
                // Content gets first refusal on a primary press.
                if *button == PointerButton::Primary {
                    if let Some(rect) = self.content_rect.filter(|r| r.contains(*position)) {
                        let mut child_cx = EventContext {
                            event: cx.event,
                            bounds: rect,
                            scale: cx.scale * self.zoom,
                        };
                        let response = self.content.event(&mut child_cx);
                        if response != EventResponse::Ignored {
                            return response;
                        }
                    }
                }
                if matches!(button, PointerButton::Primary | PointerButton::Middle) {
                    self.panning = Some(*position);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some(last) = self.panning {
                    // Forward the move to the content first (a child
                    // may itself be mid-drag).
                    if let Some(rect) = self.content_rect {
                        let mut child_cx = EventContext {
                            event: cx.event,
                            bounds: rect,
                            scale: cx.scale * self.zoom,
                        };
                        let _ = self.content.event(&mut child_cx);
                    }
                    self.panning = Some(*position);
                    self.set_pan(self.pan + (*position - last));
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased { button, .. } => {
                if self.panning.is_some()
                    && matches!(button, PointerButton::Primary | PointerButton::Middle)
                {
                    self.panning = None;
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::Scroll { position, delta } => {
                if self.bounds.contains(*position) {
                    let factor = 1.0015_f32.powf(delta.y);
                    self.zoom_at(*position, self.zoom * factor);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => {
                let center = Vec2::new(
                    (self.bounds.min_x() + self.bounds.max_x()) / 2.0,
                    (self.bounds.min_y() + self.bounds.max_y()) / 2.0,
                );
                match key.as_str() {
                    "+" | "=" => {
                        self.zoom_at(center, self.zoom * 1.2);
                        EventResponse::RequestRepaint
                    }
                    "-" | "_" => {
                        self.zoom_at(center, self.zoom / 1.2);
                        EventResponse::RequestRepaint
                    }
                    "0" => {
                        self.fit_view();
                        EventResponse::RequestRepaint
                    }
                    "Home" => {
                        self.reset_view();
                        EventResponse::RequestRepaint
                    }
                    "ArrowLeft" => {
                        self.pan_by(Vec2::new(20.0 * self.scale, 0.0));
                        EventResponse::RequestRepaint
                    }
                    "ArrowRight" => {
                        self.pan_by(Vec2::new(-20.0 * self.scale, 0.0));
                        EventResponse::RequestRepaint
                    }
                    "ArrowUp" => {
                        self.pan_by(Vec2::new(0.0, 20.0 * self.scale));
                        EventResponse::RequestRepaint
                    }
                    "ArrowDown" => {
                        self.pan_by(Vec2::new(0.0, -20.0 * self.scale));
                        EventResponse::RequestRepaint
                    }
                    _ => EventResponse::Ignored,
                }
            }
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
        cx.list.push_fill_rect(
            krect(cx.bounds),
            cx.color(TokenKey::BackgroundColor, SURFACE),
        );
        // Dot grid — spacing tracks zoom, dots track pan.
        let s = self.scale;
        let step = GRID_PT * s * self.zoom;
        if step > 6.0 {
            let dot = 1.5 * s;
            let grid = cx.color(TokenKey::DividerColor, GRID);
            let ox = cx.bounds.min_x() + self.pan.x.rem_euclid(step);
            let oy = cx.bounds.min_y() + self.pan.y.rem_euclid(step);
            let mut y = oy;
            while y < cx.bounds.max_y() {
                let mut x = ox;
                while x < cx.bounds.max_x() {
                    cx.list.push_fill_rect(
                        kurbo::Rect::new(
                            f64::from(x - dot / 2.0),
                            f64::from(y - dot / 2.0),
                            f64::from(x + dot / 2.0),
                            f64::from(y + dot / 2.0),
                        ),
                        grid,
                    );
                    x += step;
                }
                y += step;
            }
        }
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&*self.content)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut *self.content)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.content_rect).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed-size probe content.
    struct Stub(Vec2);

    impl Widget for Stub {
        fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
            self.0
        }

        fn min_render(&self) -> RenderMinimum {
            RenderMinimum::new(self.0)
        }

        fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
    }

    fn viewport() -> Viewport {
        Viewport::new().child(Stub(Vec2::new(200.0, 100.0)))
    }

    fn laid_out(v: &mut Viewport, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        v.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev(v: &mut Viewport, e: &WidgetEvent) -> EventResponse {
        v.event(&mut EventContext {
            event: e,
            bounds: v.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn zoom_keeps_cursor_point_fixed() {
        let mut v = viewport();
        laid_out(&mut v, 400.0, 300.0);
        let cursor = Vec2::new(150.0, 90.0);
        let before = v.to_content(cursor);
        v.zoom_at(cursor, 2.0);
        let after = v.to_content(cursor);
        assert!((before - after).length() < 0.01);
        assert_eq!(v.zoom_value(), 2.0);
        assert!(v.take_changed().is_some());
    }

    #[test]
    fn scroll_zooms() {
        let mut v = viewport();
        laid_out(&mut v, 400.0, 300.0);
        ev(
            &mut v,
            &WidgetEvent::Scroll {
                position: Vec2::new(200.0, 150.0),
                delta: Vec2::new(0.0, 500.0),
            },
        );
        assert!(v.zoom_value() > 1.5);
    }

    #[test]
    fn middle_drag_pans() {
        let mut v = viewport();
        laid_out(&mut v, 400.0, 300.0);
        assert_eq!(
            ev(
                &mut v,
                &WidgetEvent::PointerPressed {
                    button: PointerButton::Middle,
                    position: Vec2::new(200.0, 150.0),
                    count: 1,
                }
            ),
            EventResponse::CapturePointer
        );
        ev(
            &mut v,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(230.0, 170.0),
            },
        );
        assert_eq!(v.pan_offset(), Vec2::new(30.0, 20.0));
        ev(
            &mut v,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Middle,
                position: Vec2::new(230.0, 170.0),
            },
        );
    }

    #[test]
    fn primary_drag_on_ignored_content_pans() {
        let mut v = viewport();
        laid_out(&mut v, 400.0, 300.0);
        // Stub ignores events — the primary drag becomes a pan.
        ev(
            &mut v,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(50.0, 50.0),
                count: 1,
            },
        );
        ev(
            &mut v,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(60.0, 55.0),
            },
        );
        assert_eq!(v.pan_offset(), Vec2::new(10.0, 5.0));
    }

    #[test]
    fn child_bounds_track_zoom() {
        let mut v = viewport().zoom(2.0);
        laid_out(&mut v, 400.0, 300.0);
        let r = v.child_bounds(0).unwrap();
        assert_eq!(r.width(), 400.0); // 200 natural × 2 zoom
        assert_eq!(r.height(), 200.0);
    }

    #[test]
    fn keys_fit_reset_and_zoom() {
        let mut v = viewport();
        laid_out(&mut v, 400.0, 300.0);
        ev(
            &mut v,
            &WidgetEvent::KeyPressed {
                key: "0".to_string(),
                repeat: false,
            },
        );
        // Content 200×100 in 400×300 → zoom 2.0 (width-limited), centered.
        assert_eq!(v.zoom_value(), 2.0);
        assert_eq!(v.pan_offset(), Vec2::new(0.0, 50.0));
        ev(
            &mut v,
            &WidgetEvent::KeyPressed {
                key: "Home".to_string(),
                repeat: false,
            },
        );
        assert_eq!((v.zoom_value(), v.pan_offset()), (1.0, Vec2::ZERO));
        ev(
            &mut v,
            &WidgetEvent::KeyPressed {
                key: "+".to_string(),
                repeat: false,
            },
        );
        assert!((v.zoom_value() - 1.2).abs() < 1e-5);
    }

    #[test]
    fn paint_without_painter() {
        let mut v = viewport();
        laid_out(&mut v, 400.0, 300.0);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        v.paint(&mut PaintContext {
            list: &mut list,
            bounds: v.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
