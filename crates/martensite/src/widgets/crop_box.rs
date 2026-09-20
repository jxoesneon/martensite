//! `CropBox` — a draggable, resizable crop region over a content
//! surface (the photo-editor crop-tool idiom).
//!
//! The region lives in normalized `0..=1` coordinates relative to the
//! widget's bounds (the host draws whatever content it likes behind
//! it). Dragging inside the region moves it; the four corner handles
//! resize it, honoring an optional [`CropBox::aspect_ratio`] lock and
//! a [`CropBox::min_size`] floor. Every committed change parks the
//! normalized rect in [`CropBox::take_changed`]. The scrim outside
//! the region dims and a rule-of-thirds grid overlays the crop.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::crop_box::CropBox;
//!
//! let c = CropBox::new().crop(0.1, 0.1, 0.8, 0.8);
//! assert_eq!(c.crop_rect(), (0.1, 0.1, 0.8, 0.8));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const HANDLE_PT: f32 = 10.0;

const SCRIM: [u8; 4] = [0, 0, 0, 110];
const FRAME: [u8; 4] = [235, 235, 240, 255];
const THIRD: [u8; 4] = [235, 235, 240, 90];
const HANDLE: [u8; 4] = [250, 250, 252, 255];

/// Which corner a resize drag is anchored on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Corner {
    Tl,
    Tr,
    Bl,
    Br,
}

/// Active drag state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drag {
    None,
    Move,
    Resize(Corner),
}

/// A normalized crop region — see the module docs.
///
/// ```
/// use martensite::widgets::crop_box::CropBox;
///
/// assert_eq!(CropBox::new().crop_rect(), (0.0, 0.0, 1.0, 1.0));
/// ```
pub struct CropBox {
    /// Accessibility label.
    pub label: String,
    /// Normalized crop `(x, y, w, h)`.
    crop: (f32, f32, f32, f32),
    /// Optional locked aspect ratio `w / h`.
    aspect: Option<f32>,
    /// Minimum normalized edge.
    min_frac: f32,
    drag: Drag,
    grab: Vec2,
    changed: Option<(f32, f32, f32, f32)>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for CropBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CropBox")
            .field("crop", &self.crop)
            .field("aspect", &self.aspect)
            .finish()
    }
}

impl Default for CropBox {
    fn default() -> Self {
        Self::new()
    }
}

impl CropBox {
    /// Full-surface crop.
    ///
    /// ```
    /// use martensite::widgets::crop_box::CropBox;
    ///
    /// assert_eq!(CropBox::new().crop_rect(), (0.0, 0.0, 1.0, 1.0));
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Crop".to_string(),
            crop: (0.0, 0.0, 1.0, 1.0),
            aspect: None,
            min_frac: 0.05,
            drag: Drag::None,
            grab: Vec2::ZERO,
            changed: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::crop_box::CropBox;
    ///
    /// assert_eq!(CropBox::new().label("Photo").label, "Photo");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Normalized crop region `(x, y, w, h)` — clamped into the unit
    /// square.
    ///
    /// ```
    /// use martensite::widgets::crop_box::CropBox;
    ///
    /// assert_eq!(CropBox::new().crop(0.2, 0.2, 2.0, 2.0).crop_rect(), (0.2, 0.2, 0.8, 0.8));
    /// ```
    pub fn crop(mut self, x: f32, y: f32, w: f32, h: f32) -> Self {
        self.crop = Self::clamp_rect((x, y, w, h), self.min_frac);
        self
    }

    /// Locks the region to `w / h` *displayed* aspect (`1.0` = square,
    /// `16.0/9.0` = widescreen) regardless of surface shape.
    ///
    /// ```
    /// use martensite::widgets::crop_box::CropBox;
    ///
    /// assert_eq!(CropBox::new().aspect_ratio(1.0).aspect_value(), Some(1.0));
    /// ```
    pub fn aspect_ratio(mut self, ratio: f32) -> Self {
        self.aspect = if ratio > 0.0 { Some(ratio) } else { None };
        self
    }

    /// The locked aspect ratio, if any.
    ///
    /// ```
    /// use martensite::widgets::crop_box::CropBox;
    ///
    /// assert_eq!(CropBox::new().aspect_value(), None);
    /// ```
    pub fn aspect_value(&self) -> Option<f32> {
        self.aspect
    }

    /// Minimum normalized edge (`0.01`–`0.5`).
    ///
    /// ```
    /// use martensite::widgets::crop_box::CropBox;
    ///
    /// assert_eq!(CropBox::new().min_size(0.1).min_size_value(), 0.1);
    /// ```
    pub fn min_size(mut self, frac: f32) -> Self {
        self.min_frac = frac.clamp(0.01, 0.5);
        self
    }

    /// The configured minimum edge.
    ///
    /// ```
    /// use martensite::widgets::crop_box::CropBox;
    ///
    /// assert_eq!(CropBox::new().min_size_value(), 0.05);
    /// ```
    pub fn min_size_value(&self) -> f32 {
        self.min_frac
    }

    /// Current normalized `(x, y, w, h)` region.
    ///
    /// ```
    /// use martensite::widgets::crop_box::CropBox;
    ///
    /// assert_eq!(CropBox::new().crop_rect(), (0.0, 0.0, 1.0, 1.0));
    /// ```
    pub fn crop_rect(&self) -> (f32, f32, f32, f32) {
        self.crop
    }

    /// Sets the normalized region.
    ///
    /// ```
    /// use martensite::widgets::crop_box::CropBox;
    ///
    /// let mut c = CropBox::new();
    /// c.set_crop(0.25, 0.25, 0.5, 0.5);
    /// assert_eq!(c.crop_rect(), (0.25, 0.25, 0.5, 0.5));
    /// ```
    pub fn set_crop(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.crop = Self::clamp_rect((x, y, w, h), self.min_frac);
    }

    /// Drains the last committed region.
    ///
    /// ```
    /// use martensite::widgets::crop_box::CropBox;
    ///
    /// assert_eq!(CropBox::new().take_changed(), None);
    /// ```
    pub fn take_changed(&mut self) -> Option<(f32, f32, f32, f32)> {
        self.changed.take()
    }

    /// Whether a drag is active.
    ///
    /// ```
    /// use martensite::widgets::crop_box::CropBox;
    ///
    /// assert!(!CropBox::new().is_dragging());
    /// ```
    pub fn is_dragging(&self) -> bool {
        self.drag != Drag::None
    }

    /// Clamps a normalized rect into the unit square — origin wins,
    /// size shrinks to fit (builder/resize semantics).
    fn clamp_rect(r: (f32, f32, f32, f32), min: f32) -> (f32, f32, f32, f32) {
        let x = r.0.clamp(0.0, 1.0 - min);
        let y = r.1.clamp(0.0, 1.0 - min);
        (x, y, r.2.clamp(min, 1.0 - x), r.3.clamp(min, 1.0 - y))
    }

    /// Clamps a move — size wins, origin shifts to keep it inside.
    fn clamp_move(&self, x: f32, y: f32) -> (f32, f32) {
        (
            x.clamp(0.0, (1.0 - self.crop.2).max(0.0)),
            y.clamp(0.0, (1.0 - self.crop.3).max(0.0)),
        )
    }

    /// Device-space crop rect.
    fn crop_px(&self) -> Rect {
        let (x, y, w, h) = self.crop;
        Rect::new(
            self.bounds.min_x() + x * self.bounds.width(),
            self.bounds.min_y() + y * self.bounds.height(),
            w * self.bounds.width(),
            h * self.bounds.height(),
        )
    }

    /// Normalized point for device `p`.
    fn to_norm(&self, p: Vec2) -> Vec2 {
        Vec2::new(
            (p.x - self.bounds.min_x()) / self.bounds.width().max(0.001),
            (p.y - self.bounds.min_y()) / self.bounds.height().max(0.001),
        )
    }

    /// Corner device centers: `[TL, TR, BL, BR]`.
    fn corners(&self) -> [Vec2; 4] {
        let r = self.crop_px();
        [
            Vec2::new(r.min_x(), r.min_y()),
            Vec2::new(r.max_x(), r.min_y()),
            Vec2::new(r.min_x(), r.max_y()),
            Vec2::new(r.max_x(), r.max_y()),
        ]
    }

    /// Corner under `p`, if any.
    fn corner_at(&self, p: Vec2) -> Option<Corner> {
        let tol = HANDLE_PT * self.scale;
        for (i, c) in self.corners().iter().enumerate() {
            if c.distance(p) <= tol {
                return Some(match i {
                    0 => Corner::Tl,
                    1 => Corner::Tr,
                    2 => Corner::Bl,
                    _ => Corner::Br,
                });
            }
        }
        None
    }

    /// Applies a resize drag to normalized `p`.
    fn resize_to(&mut self, corner: Corner, p: Vec2) {
        let (x, y, w, h) = self.crop;
        let (x1, y1) = (x + w, y + h);
        let (nx, mut ny, nw, mut nh) = match corner {
            Corner::Tl => (p.x, p.y, x1 - p.x, y1 - p.y),
            Corner::Tr => (x, p.y, p.x - x, y1 - p.y),
            Corner::Bl => (p.x, y, x1 - p.x, p.y - y),
            Corner::Br => (x, y, p.x - x, p.y - y),
        };
        if let Some(a) = self.aspect {
            // `a` is visual (pixel) aspect: (nw*W)/(nh*H) == a.
            nh = nw * self.bounds.width() / (a * self.bounds.height().max(0.001));
            match corner {
                Corner::Tl | Corner::Tr => ny = y1 - nh,
                _ => {}
            }
        }
        self.crop = Self::clamp_rect((nx, ny, nw, nh), self.min_frac);
    }

    /// Commits the current crop into the change seam.
    fn commit(&mut self) {
        self.changed = Some(self.crop);
    }
}

impl Widget for CropBox {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(320.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(240.0).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(48.0, 48.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        let (x, y, w, h) = self.crop;
        node.set_label(format!(
            "{} — {:.0}%, {:.0}% of {:.0}%×{:.0}%",
            self.label,
            x * 100.0,
            y * 100.0,
            w * 100.0,
            h * 100.0
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                if let Some(c) = self.corner_at(*position) {
                    self.drag = Drag::Resize(c);
                } else if self.crop_px().contains(*position) {
                    self.drag = Drag::Move;
                    self.grab = *position - self.crop_px().origin;
                } else {
                    return EventResponse::Ignored;
                }
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerMoved { position } => match self.drag {
                Drag::Move => {
                    let x = (position.x - self.grab.x - self.bounds.min_x())
                        / self.bounds.width().max(0.001);
                    let y = (position.y - self.grab.y - self.bounds.min_y())
                        / self.bounds.height().max(0.001);
                    let (nx, ny) = self.clamp_move(x, y);
                    self.crop = (nx, ny, self.crop.2, self.crop.3);
                    EventResponse::RequestRepaint
                }
                Drag::Resize(c) => {
                    self.resize_to(c, self.to_norm(*position));
                    EventResponse::RequestRepaint
                }
                Drag::None => EventResponse::Ignored,
            },
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.drag != Drag::None {
                    self.drag = Drag::None;
                    self.commit();
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => {
                let step = 0.02;
                let (dx, dy) = match key.as_str() {
                    "ArrowLeft" => (-step, 0.0),
                    "ArrowRight" => (step, 0.0),
                    "ArrowUp" => (0.0, -step),
                    "ArrowDown" => (0.0, step),
                    _ => return EventResponse::Ignored,
                };
                let (x, y, w, h) = self.crop;
                let (nx, ny) = self.clamp_move(x + dx, y + dy);
                self.crop = (nx, ny, w, h);
                self.commit();
                EventResponse::RequestRepaint
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
        let pt = |p: Vec2| kurbo::Point::new(f64::from(p.x), f64::from(p.y));
        let r = self.crop_px();

        // Scrim the four outside strips.
        let scrim = cx.color(TokenKey::ScrimColor, SCRIM);
        for strip in [
            Rect::new(
                self.bounds.min_x(),
                self.bounds.min_y(),
                self.bounds.width(),
                (r.min_y() - self.bounds.min_y()).max(0.0),
            ),
            Rect::new(
                self.bounds.min_x(),
                r.max_y(),
                self.bounds.width(),
                (self.bounds.max_y() - r.max_y()).max(0.0),
            ),
            Rect::new(
                self.bounds.min_x(),
                r.min_y(),
                (r.min_x() - self.bounds.min_x()).max(0.0),
                r.height(),
            ),
            Rect::new(
                r.max_x(),
                r.min_y(),
                (self.bounds.max_x() - r.max_x()).max(0.0),
                r.height(),
            ),
        ] {
            cx.list.push_fill_rect(krect(strip), scrim);
        }

        // Rule-of-thirds grid inside the crop.
        for i in 1..3 {
            let f = i as f32 / 3.0;
            let mut v = kurbo::BezPath::new();
            let x = r.min_x() + r.width() * f;
            v.move_to(pt(Vec2::new(x, r.min_y())));
            v.line_to(pt(Vec2::new(x, r.max_y())));
            cx.list.push_stroke_path(v, 0.5 * self.scale, THIRD);
            let mut h = kurbo::BezPath::new();
            let y = r.min_y() + r.height() * f;
            h.move_to(pt(Vec2::new(r.min_x(), y)));
            h.line_to(pt(Vec2::new(r.max_x(), y)));
            cx.list.push_stroke_path(h, 0.5 * self.scale, THIRD);
        }

        // Frame.
        cx.list.push_stroke_shape(
            krect(r),
            &martensite_core::shape::Shape::RECT,
            1.0 * self.scale,
            cx.color(TokenKey::BorderColor, FRAME),
        );

        // Corner handles.
        let hs = HANDLE_PT * self.scale * 0.7;
        for c in self.corners() {
            cx.list.push_fill_shape(
                krect(Rect::new(c.x - hs / 2.0, c.y - hs / 2.0, hs, hs)),
                &martensite_core::shape::Shape::RECT,
                cx.color(TokenKey::TextInverseColor, HANDLE),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut CropBox, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        c.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev(c: &mut CropBox, e: &WidgetEvent) -> EventResponse {
        c.event(&mut EventContext {
            event: e,
            bounds: c.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn drag_moves_region() {
        let mut c = CropBox::new().crop(0.25, 0.25, 0.5, 0.5);
        laid_out(&mut c, 400.0, 400.0);
        let r = c.crop_px();
        let inside = Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0);
        assert_eq!(
            ev(
                &mut c,
                &WidgetEvent::PointerPressed {
                    button: PointerButton::Primary,
                    position: inside,
                    count: 1,
                }
            ),
            EventResponse::CapturePointer
        );
        ev(
            &mut c,
            &WidgetEvent::PointerMoved {
                position: inside + Vec2::new(40.0, 40.0),
            },
        );
        ev(
            &mut c,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: inside + Vec2::new(40.0, 40.0),
            },
        );
        let (x, y, _, _) = c.crop_rect();
        assert!((x - 0.35).abs() < 0.01 && (y - 0.35).abs() < 0.01);
        assert_eq!(c.take_changed().unwrap().0, x);
    }

    #[test]
    fn corner_resizes() {
        let mut c = CropBox::new().crop(0.25, 0.25, 0.5, 0.5);
        laid_out(&mut c, 400.0, 400.0);
        let br = c.corners()[3];
        ev(
            &mut c,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: br,
                count: 1,
            },
        );
        ev(
            &mut c,
            &WidgetEvent::PointerMoved {
                position: br + Vec2::new(80.0, 80.0),
            },
        );
        ev(
            &mut c,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: br,
            },
        );
        let (_, _, w, h) = c.crop_rect();
        assert!((w - 0.7).abs() < 0.01 && (h - 0.7).abs() < 0.01);
    }

    #[test]
    fn aspect_lock_holds() {
        let mut c = CropBox::new().crop(0.2, 0.2, 0.4, 0.4).aspect_ratio(1.0);
        laid_out(&mut c, 400.0, 400.0);
        let br = c.corners()[3];
        ev(
            &mut c,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: br,
                count: 1,
            },
        );
        ev(
            &mut c,
            &WidgetEvent::PointerMoved {
                position: br + Vec2::new(60.0, 10.0),
            },
        );
        let (_, _, w, h) = c.crop_rect();
        assert!((w / h - 1.0).abs() < 0.01); // square surface + 1:1 lock
    }

    #[test]
    fn arrows_nudge_and_commit() {
        let mut c = CropBox::new().crop(0.5, 0.5, 0.25, 0.25);
        laid_out(&mut c, 400.0, 400.0);
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: "ArrowLeft".to_string(),
                repeat: false,
            },
        );
        assert!((c.crop_rect().0 - 0.48).abs() < 1e-5);
        assert!(c.take_changed().is_some());
    }

    #[test]
    fn crop_clamps_to_bounds() {
        // Origin wins: y stays 0.9 and the height shrinks to the edge.
        let c = CropBox::new().crop(-0.5, 0.9, 0.5, 0.5);
        let (x, y, w, h) = c.crop_rect();
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.9);
        assert!((w - 0.5).abs() < 1e-5 && (h - 0.1).abs() < 1e-5);
    }
}
