//! `ColorPicker` widget: a color well with an HSV picker popup
//! (WinUI `ColorPicker` / `NSColorWell` / `QColorDialog`-lite).
//!
//! The face is a color well — a swatch over a checkerboard (so alpha
//! reads honestly) with a border; pressing it (or `Enter`/`Space`, or
//! AT `Click`/`Expand`) reconciles a `ColorSurface` into the
//! [`OverlayLayer`](martensite_core::overlay::OverlayLayer) at
//! `OverlayAnchor::Bounds` on the next
//! [`ColorPicker::sync_overlay`].
//!
//! - **Popup**: a `Role::Dialog` with a saturation×value square for
//!   the current hue — painted as a coarse 24×16 grid of small fill
//!   rects (an honest approximation: no per-pixel image, documented
//!   rather than faked) — a hue strip (a 7-stop rainbow gradient), an
//!   alpha strip when [`ColorPicker::with_alpha`] is set, a hex
//!   readout, and an OK press zone.
//! - **Editing is live**: dragging the square or a strip updates the
//!   working color and queues each change for
//!   [`ColorPicker::take_edited`], so a host can live-preview.
//! - **Commit**: only the OK zone (or `Enter`) commits —
//!   [`ColorPicker::take_selected`] reports it, the face swatch
//!   updates, and the popup closes. `Escape`/outside-press
//!   light-dismisses and discards the working edits — the two-channel
//!   design means live edits are observable without being committed.
//! - **Keyboard**: `Tab` cycles the active zone (square → hue →
//!   alpha), arrows nudge s/v on the square or the channel on a
//!   strip, `Enter` confirms, `Escape` cancels.
//!
//! The face emits `Role::Button` with a `"Color picker"` label, the
//! `#RRGGBB` value text, and `aria-haspopup="dialog"`.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{Color, ColorPicker};
//!
//! let mut cp = ColorPicker::new()
//!     .color(Color::rgb(60, 110, 220))
//!     .with_alpha(true);
//! cp.open();
//! assert!(cp.is_open());
//! ```

use std::sync::{Arc, Mutex};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{
    GradientStop, GradientStops, NodeFlags, Rect, RenderMinimum, TokenKey, UnderflowPolicy,
};

/// Well face size (logical points).
const WELL_W: f32 = 44.0;
const WELL_H: f32 = 24.0;
/// Checkerboard cell edge inside the swatch (logical points).
const CHECK: f32 = 4.0;
/// Well border.
const WELL_BORDER: [u8; 4] = [140, 145, 155, 255];
/// Checkerboard greys.
const CHECK_A: [u8; 4] = [210, 210, 214, 255];
const CHECK_B: [u8; 4] = [245, 245, 248, 255];
/// Popup background / border / ink.
const POPUP_BG: [u8; 4] = [252, 252, 254, 255];
const POPUP_BORDER: [u8; 4] = [140, 145, 155, 255];
const INK: [u8; 4] = [30, 30, 36, 255];
const ACCENT: [u8; 4] = [60, 110, 220, 255];
const ACCENT_INK: [u8; 4] = [255, 255, 255, 255];
/// Popup chrome (logical points).
const PAD: f32 = 10.0;
/// SV square height (logical points); width fills the popup.
const SV_H: f32 = 150.0;
/// Strip heights and gaps (logical points).
const STRIP_H: f32 = 16.0;
const GAP: f32 = 8.0;
/// Bottom row: hex readout + OK zone height (logical points).
const ROW_H: f32 = 24.0;
/// OK zone width (logical points).
const OK_W: f32 = 56.0;
/// SV grid resolution — coarse on purpose (see module docs).
const SV_COLS: usize = 24;
const SV_ROWS: usize = 16;
/// Popup width (logical points).
const POPUP_W: f32 = 240.0;

/// An RGBA color, 8 bits per channel.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Color;
///
/// let c = Color::rgba(255, 0, 0, 128);
/// assert_eq!(c.to_hex(), "#FF0000");
/// assert_eq!(c.to_hex_alpha(), "#FF000080");
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel (`255` = opaque).
    pub a: u8,
}

impl Color {
    /// An opaque color.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Color;
    ///
    /// assert_eq!(Color::rgb(10, 20, 30).a, 255);
    /// ```
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// A color with explicit alpha.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Color;
    ///
    /// assert_eq!(Color::rgba(10, 20, 30, 40).a, 40);
    /// ```
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// The `[r, g, b, a]` channel array.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Color;
    ///
    /// assert_eq!(Color::rgb(1, 2, 3).to_rgba8(), [1, 2, 3, 255]);
    /// ```
    pub fn to_rgba8(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// `#RRGGBB` — or `#RRGGBBAA` when `alpha` is requested (the
    /// picker uses it when `with_alpha` is set).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Color;
    ///
    /// assert_eq!(Color::rgb(255, 128, 0).to_hex(), "#FF8000");
    /// assert_eq!(Color::rgb(255, 128, 0).to_hex_alpha(), "#FF8000FF");
    /// ```
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// `#RRGGBBAA` — the alpha-inclusive form of [`to_hex`](Self::to_hex).
    pub fn to_hex_alpha(self) -> String {
        format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
    }
}

/// Converts HSV to 8-bit RGB: `h` is degrees and wraps (`370` ≡ `10`),
/// `s`/`v` are `0.0..=1.0` (clamped). Classic six-sector hue math.
///
/// # Examples
///
/// ```
/// use martensite::widgets::hsv_to_rgb;
///
/// assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), (255, 0, 0));
/// assert_eq!(hsv_to_rgb(120.0, 1.0, 1.0), (0, 255, 0));
/// assert_eq!(hsv_to_rgb(240.0, 1.0, 1.0), (0, 0, 255));
/// assert_eq!(hsv_to_rgb(0.0, 0.0, 0.5), (128, 128, 128));
/// ```
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0);
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

/// Converts 8-bit RGB to HSV `(h 0..360, s 0..=1, v 0..=1)`. Grey
/// colors report `h = 0` (hue is undefined at `s = 0`).
///
/// # Examples
///
/// ```
/// use martensite::widgets::rgb_to_hsv;
///
/// assert_eq!(rgb_to_hsv(255, 0, 0), (0.0, 1.0, 1.0));
/// assert_eq!(rgb_to_hsv(0, 255, 0), (120.0, 1.0, 1.0));
/// let (h, s, v) = rgb_to_hsv(128, 128, 128);
/// assert_eq!((s, v), (0.0, 128.0 / 255.0));
/// ```
pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = f32::from(r) / 255.0;
    let g = f32::from(g) / 255.0;
    let b = f32::from(b) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let h = if delta < f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    (
        h.rem_euclid(360.0),
        if max < f32::EPSILON { 0.0 } else { delta / max },
        max,
    )
}

/// Which interactive zone of the popup surface is active/dragging.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Zone {
    /// The saturation×value square.
    Sv,
    /// The hue strip.
    Hue,
    /// The alpha strip (`with_alpha` only).
    Alpha,
}

/// Channel between a [`ColorPicker`] and its live [`ColorSurface`]:
/// working-color edits and the confirm/close results flow surface →
/// face, which drains them in [`ColorPicker::sync_overlay`].
#[derive(Debug, Default)]
struct ColorChannel {
    /// Latest working-color edit (each change overwrites — the host
    /// samples per frame, so latest-wins loses nothing).
    edited: Option<Color>,
    /// The confirmed color (OK zone / `Enter`).
    confirmed: Option<Color>,
    /// The surface asked to close without confirming (embedded
    /// `Escape` — the layer handles it in arena use).
    close_requested: bool,
}

/// The popup surface for a [`ColorPicker`] — a `Role::Dialog` with an
/// SV square, a hue strip, an optional alpha strip, a hex readout, and
/// an OK zone. Edits live in `h`/`s`/`v`/`a` working state; results
/// flow back through [`ColorChannel`].
struct ColorSurface {
    /// Working hue, degrees `0..360`.
    h: f32,
    /// Working saturation `0..=1`.
    s: f32,
    /// Working value `0..=1`.
    v: f32,
    /// Working alpha `0..=1`.
    a: f32,
    /// Whether the alpha strip exists.
    with_alpha: bool,
    /// The zone arrow keys nudge (last-interacted or `Tab` target).
    active: Zone,
    /// The zone currently captured by a pointer drag.
    dragging: Option<Zone>,
    /// Surface bounds from the last layout pass.
    bounds: Rect,
    /// SV square rect.
    sv_rect: Rect,
    /// Hue strip rect.
    hue_rect: Rect,
    /// Alpha strip rect (empty without `with_alpha`).
    alpha_rect: Rect,
    /// OK zone rect.
    ok_rect: Rect,
    /// Result channel back to the owning `ColorPicker`.
    channel: Arc<Mutex<ColorChannel>>,
    /// The silhouette painted last frame — `clip_shape`/`hit_shape`.
    painted_shape: Mutex<Shape>,
    /// Shared shaped-text painter from the owning `ColorPicker`.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl ColorSurface {
    /// The working color as a [`Color`].
    fn working(&self) -> Color {
        let (r, g, b) = hsv_to_rgb(self.h, self.s, self.v);
        Color::rgba(r, g, b, (self.a.clamp(0.0, 1.0) * 255.0).round() as u8)
    }

    /// Records the current working color as a live edit.
    fn note_edit(&self) {
        self.channel.lock().expect("color channel poisoned").edited = Some(self.working());
    }

    /// The zones arrow keys can target (`Tab` cycles them).
    fn zones(&self) -> [Zone; 3] {
        [Zone::Sv, Zone::Hue, Zone::Alpha]
    }

    /// Updates the working channels from a pointer position inside
    /// `zone`'s rect, clamped to the rect.
    fn apply_pointer(&mut self, zone: Zone, pos: Vec2) {
        match zone {
            Zone::Sv => {
                let r = self.sv_rect;
                self.s = ((pos.x - r.min_x()) / r.width().max(1.0)).clamp(0.0, 1.0);
                self.v = (1.0 - (pos.y - r.min_y()) / r.height().max(1.0)).clamp(0.0, 1.0);
            }
            Zone::Hue => {
                let r = self.hue_rect;
                self.h = ((pos.x - r.min_x()) / r.width().max(1.0)).clamp(0.0, 1.0) * 360.0;
            }
            Zone::Alpha => {
                let r = self.alpha_rect;
                self.a = ((pos.x - r.min_x()) / r.width().max(1.0)).clamp(0.0, 1.0);
            }
        }
        self.active = zone;
        self.note_edit();
    }

    /// Keyboard nudge on the active zone.
    fn nudge(&mut self, key: &str) -> bool {
        let signed = match key {
            "ArrowLeft" | "ArrowDown" => -1.0f32,
            "ArrowRight" | "ArrowUp" => 1.0,
            _ => return false,
        };
        match self.active {
            Zone::Sv => match key {
                "ArrowLeft" | "ArrowRight" => {
                    self.s = (self.s + signed * 0.04).clamp(0.0, 1.0);
                }
                _ => {
                    self.v = (self.v + signed * 0.04).clamp(0.0, 1.0);
                }
            },
            Zone::Hue => {
                self.h = (self.h + signed * 6.0).rem_euclid(360.0);
            }
            Zone::Alpha => {
                self.a = (self.a + signed * 0.05).clamp(0.0, 1.0);
            }
        }
        self.note_edit();
        true
    }
}

impl Widget for ColorSurface {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut h = cx.pt(PAD) * 2.0
            + cx.pt(SV_H)
            + cx.pt(GAP)
            + cx.pt(STRIP_H)
            + cx.pt(GAP)
            + cx.pt(ROW_H);
        if self.with_alpha {
            h += cx.pt(GAP) + cx.pt(STRIP_H);
        }
        Vec2::new(
            cx.pt(POPUP_W).min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        cx.hot.flags |= NodeFlags::FOCUSABLE;
        let pad = cx.pt(PAD);
        let w = (bounds.width() - pad * 2.0).max(0.0);
        let mut y = bounds.min_y() + pad;
        self.sv_rect = Rect::new(bounds.min_x() + pad, y, w, cx.pt(SV_H));
        y += cx.pt(SV_H) + cx.pt(GAP);
        self.hue_rect = Rect::new(bounds.min_x() + pad, y, w, cx.pt(STRIP_H));
        y += cx.pt(STRIP_H) + cx.pt(GAP);
        self.alpha_rect = if self.with_alpha {
            let r = Rect::new(bounds.min_x() + pad, y, w, cx.pt(STRIP_H));
            y += cx.pt(STRIP_H) + cx.pt(GAP);
            r
        } else {
            Rect::default()
        };
        let ok_w = cx.pt(OK_W);
        self.ok_rect = Rect::new(bounds.max_x() - pad - ok_w, y, ok_w, cx.pt(ROW_H));
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Dialog);
        node.set_label("Color picker");
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                position,
                button: PointerButton::Primary,
                ..
            } => {
                if self.ok_rect.contains(*position) {
                    self.channel
                        .lock()
                        .expect("color channel poisoned")
                        .confirmed = Some(self.working());
                    return EventResponse::RequestRepaint;
                }
                for (zone, rect) in [
                    (Zone::Sv, self.sv_rect),
                    (Zone::Hue, self.hue_rect),
                    (Zone::Alpha, self.alpha_rect),
                ] {
                    if (zone != Zone::Alpha || self.with_alpha) && rect.contains(*position) {
                        self.apply_pointer(zone, *position);
                        self.dragging = Some(zone);
                        return EventResponse::CapturePointer;
                    }
                }
                // Inside the bubble but off the zones — consume so the
                // press can't count as an outside-press dismissal.
                EventResponse::Handled
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some(zone) = self.dragging {
                    self.apply_pointer(zone, *position);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.dragging.take().is_some() {
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Handled
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "Enter" | " " | "Space" => {
                    self.channel
                        .lock()
                        .expect("color channel poisoned")
                        .confirmed = Some(self.working());
                    EventResponse::Handled
                }
                // The layer eats `Escape` before content in arena use;
                // this path serves ownerless-embedded use.
                "Escape" => {
                    self.channel
                        .lock()
                        .expect("color channel poisoned")
                        .close_requested = true;
                    EventResponse::Handled
                }
                "Tab" => {
                    let zones = self.zones();
                    let count = if self.with_alpha { 3 } else { 2 };
                    let idx = zones[..count]
                        .iter()
                        .position(|z| *z == self.active)
                        .unwrap_or(0);
                    self.active = zones[(idx + 1) % count];
                    EventResponse::RequestRepaint
                }
                k => {
                    if self.nudge(k) {
                        EventResponse::RequestRepaint
                    } else {
                        EventResponse::Ignored
                    }
                }
            },
            WidgetEvent::Scroll { .. } => EventResponse::Handled,
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let face = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let shape = Shape::rounded(cx.dim(TokenKey::BorderRadius, 6.0));
        *self.painted_shape.lock().expect("popup shape poisoned") = shape.clone();
        cx.list
            .push_fill_shape(face, &shape, cx.color(TokenKey::SurfaceColor, POPUP_BG));
        cx.list.push_stroke_shape(
            face,
            &shape,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, POPUP_BORDER),
        );

        // SV square: a coarse 24×16 grid of fill rects — s runs left→
        // right, v top→bottom — each cell the exact HSV color it
        // represents. Honest resolution, no per-pixel image.
        let cell_w = self.sv_rect.width() / SV_COLS as f32;
        let cell_h = self.sv_rect.height() / SV_ROWS as f32;
        for row in 0..SV_ROWS {
            let v = 1.0 - row as f32 / (SV_ROWS - 1) as f32;
            for col in 0..SV_COLS {
                let s = col as f32 / (SV_COLS - 1) as f32;
                let (r, g, bl) = hsv_to_rgb(self.h, s, v);
                // +1px bleed hides raster seams between cells.
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(self.sv_rect.min_x() + col as f32 * cell_w),
                        f64::from(self.sv_rect.min_y() + row as f32 * cell_h),
                        f64::from(self.sv_rect.min_x() + (col + 1) as f32 * cell_w + 1.0),
                        f64::from(self.sv_rect.min_y() + (row + 1) as f32 * cell_h + 1.0),
                    ),
                    [r, g, bl, 255],
                );
            }
        }
        // Clip the bleed to the square's real edge.
        cx.list.push_stroke_rect(
            kurbo::Rect::new(
                f64::from(self.sv_rect.min_x()),
                f64::from(self.sv_rect.min_y()),
                f64::from(self.sv_rect.max_x()),
                f64::from(self.sv_rect.max_y()),
            ),
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, POPUP_BORDER),
        );
        // SV marker: a ring at (s, 1−v).
        let mx = self.sv_rect.min_x() + self.s * self.sv_rect.width();
        let my = self.sv_rect.min_y() + (1.0 - self.v) * self.sv_rect.height();
        let marker = Shape::Circle {
            center: Vec2::new(mx, my),
            radius: cx.pt(5.5),
        };
        cx.list
            .push_stroke_shape(kurbo::Rect::ZERO, &marker, cx.pt(2.5), [0, 0, 0, 160]);
        let marker = Shape::Circle {
            center: Vec2::new(mx, my),
            radius: cx.pt(4.5),
        };
        cx.list
            .push_stroke_shape(kurbo::Rect::ZERO, &marker, cx.pt(1.6), [255, 255, 255, 240]);

        // Hue strip: the classic 7-stop rainbow.
        let hue_stops = GradientStops::from_slice(&[
            GradientStop::new(0.0, [255, 0, 0, 255]),
            GradientStop::new(1.0 / 6.0, [255, 255, 0, 255]),
            GradientStop::new(2.0 / 6.0, [0, 255, 0, 255]),
            GradientStop::new(3.0 / 6.0, [0, 255, 255, 255]),
            GradientStop::new(4.0 / 6.0, [0, 0, 255, 255]),
            GradientStop::new(5.0 / 6.0, [255, 0, 255, 255]),
            GradientStop::new(1.0, [255, 0, 0, 255]),
        ]);
        let hue_krect = kurbo::Rect::new(
            f64::from(self.hue_rect.min_x()),
            f64::from(self.hue_rect.min_y()),
            f64::from(self.hue_rect.max_x()),
            f64::from(self.hue_rect.max_y()),
        );
        cx.list.push_linear_gradient(
            hue_krect,
            hue_stops,
            [hue_krect.x0, hue_krect.y0],
            [hue_krect.x1, hue_krect.y0],
        );
        cx.list.push_stroke_rect(
            hue_krect,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, POPUP_BORDER),
        );
        // Hue marker.
        let hx = self.hue_rect.min_x() + self.h / 360.0 * self.hue_rect.width();
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(hx - cx.pt(1.5)),
                f64::from(self.hue_rect.min_y() - cx.pt(1.0)),
                f64::from(hx + cx.pt(1.5)),
                f64::from(self.hue_rect.max_y() + cx.pt(1.0)),
            ),
            [255, 255, 255, 255],
        );
        cx.list.push_stroke_rect(
            kurbo::Rect::new(
                f64::from(hx - cx.pt(1.5)),
                f64::from(self.hue_rect.min_y() - cx.pt(1.0)),
                f64::from(hx + cx.pt(1.5)),
                f64::from(self.hue_rect.max_y() + cx.pt(1.0)),
            ),
            cx.pt(0.8),
            [0, 0, 0, 200],
        );

        // Alpha strip (optional): checkerboard + transparent→opaque
        // gradient of the working RGB.
        if self.with_alpha {
            let (r, g, bl) = hsv_to_rgb(self.h, self.s, self.v);
            let ar = self.alpha_rect;
            let check = cx.pt(CHECK);
            let mut cy = ar.min_y();
            while cy < ar.max_y() {
                let mut cxx = ar.min_x();
                let row_odd = ((cy - ar.min_y()) / check) as i32 % 2 == 1;
                while cxx < ar.max_x() {
                    let col_odd = ((cxx - ar.min_x()) / check) as i32 % 2 == 1;
                    cx.list.push_fill_rect(
                        kurbo::Rect::new(
                            f64::from(cxx),
                            f64::from(cy),
                            f64::from((cxx + check).min(ar.max_x())),
                            f64::from((cy + check).min(ar.max_y())),
                        ),
                        if row_odd ^ col_odd { CHECK_A } else { CHECK_B },
                    );
                    cxx += check;
                }
                cy += check;
            }
            let alpha_stops = GradientStops::from_slice(&[
                GradientStop::new(0.0, [r, g, bl, 0]),
                GradientStop::new(1.0, [r, g, bl, 255]),
            ]);
            let a_krect = kurbo::Rect::new(
                f64::from(ar.min_x()),
                f64::from(ar.min_y()),
                f64::from(ar.max_x()),
                f64::from(ar.max_y()),
            );
            cx.list.push_linear_gradient(
                a_krect,
                alpha_stops,
                [a_krect.x0, a_krect.y0],
                [a_krect.x1, a_krect.y0],
            );
            cx.list.push_stroke_rect(
                a_krect,
                cx.pt(1.0),
                cx.color(TokenKey::BorderColor, POPUP_BORDER),
            );
            let ax = ar.min_x() + self.a * ar.width();
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(ax - cx.pt(1.5)),
                    f64::from(ar.min_y() - cx.pt(1.0)),
                    f64::from(ax + cx.pt(1.5)),
                    f64::from(ar.max_y() + cx.pt(1.0)),
                ),
                [255, 255, 255, 255],
            );
            cx.list.push_stroke_rect(
                kurbo::Rect::new(
                    f64::from(ax - cx.pt(1.5)),
                    f64::from(ar.min_y() - cx.pt(1.0)),
                    f64::from(ax + cx.pt(1.5)),
                    f64::from(ar.max_y() + cx.pt(1.0)),
                ),
                cx.pt(0.8),
                [0, 0, 0, 200],
            );
        }

        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let font_px = cx.pt(12.0);
        let working = self.working();
        let hex = if self.with_alpha {
            working.to_hex_alpha()
        } else {
            working.to_hex()
        };
        // Hex readout, bottom-left of the OK row.
        let row_y = self.ok_rect.min_y();
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(self.sv_rect.min_x()),
                f64::from(row_y + (self.ok_rect.height() - font_px) / 2.0),
            ),
            &hex,
            font_px,
            cx.color(TokenKey::TextColor, INK),
        );

        // OK zone: an accent chip — the only path that commits.
        let ok = kurbo::Rect::new(
            f64::from(self.ok_rect.min_x()),
            f64::from(self.ok_rect.min_y()),
            f64::from(self.ok_rect.max_x()),
            f64::from(self.ok_rect.max_y()),
        );
        let ok_shape = Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 3.0));
        cx.list
            .push_fill_shape(ok, &ok_shape, cx.color(TokenKey::AccentColor, ACCENT));
        let ok_label_w = 2.0 * font_px * 0.6;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(
                f64::from(self.ok_rect.min_x() + (self.ok_rect.width() - ok_label_w) / 2.0),
                f64::from(row_y + (self.ok_rect.height() - font_px) / 2.0),
            ),
            "OK",
            font_px,
            cx.color(TokenKey::TextInverseColor, ACCENT_INK),
        );
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn clip_shape(&self) -> Option<Shape> {
        Some(
            self.painted_shape
                .lock()
                .expect("popup shape poisoned")
                .clone(),
        )
    }

    fn hit_shape(&self) -> Option<Shape> {
        Some(
            self.painted_shape
                .lock()
                .expect("popup shape poisoned")
                .clone(),
        )
    }
}

/// A color well with an HSV picker popup (WinUI `ColorPicker` /
/// `NSColorWell` / `QColorDialog`-lite).
///
/// See the module docs for the edit/commit split:
/// [`take_edited`](Self::take_edited) reports every live change inside
/// the popup; [`take_selected`](Self::take_selected) reports only an
/// OK/`Enter` confirm. `Escape` or an outside press light-dismisses
/// and discards working edits.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Color, ColorPicker};
///
/// let mut cp = ColorPicker::new().color(Color::rgb(200, 40, 40));
/// assert_eq!(cp.get_color(), Color::rgb(200, 40, 40));
/// cp.open();
/// cp.close();
/// assert!(!cp.is_open());
/// ```
pub struct ColorPicker {
    /// Accessible label — defaults to `"Color picker"`.
    pub label: Option<String>,
    /// Whether the well accepts input.
    pub enabled: bool,
    /// Whether the popup offers an alpha strip.
    pub with_alpha: bool,
    /// The committed color.
    color: Color,
    /// Whether the popup is logically open.
    open: bool,
    /// Overlay entry id of the open popup.
    popup_id: Option<u64>,
    /// Result channel shared with the live surface.
    channel: Arc<Mutex<ColorChannel>>,
    /// One-shot confirmed color awaiting `take_selected`.
    selected_pending: Option<Color>,
    /// Latest live edit awaiting `take_edited`.
    edited_pending: Option<Color>,
    /// Well bounds from the last layout pass.
    cached_bounds: Rect,
    /// The bounds the live popup was last anchored to.
    last_anchor: Option<Rect>,
    /// Shared shaped-text painter — propagated into the surface for
    /// the hex readout / OK label.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl ColorPicker {
    /// A color well starting at opaque black.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Color, ColorPicker};
    ///
    /// let cp = ColorPicker::new();
    /// assert_eq!(cp.get_color(), Color::rgb(0, 0, 0));
    /// assert!(!cp.is_open());
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            with_alpha: false,
            color: Color::rgb(0, 0, 0),
            open: false,
            popup_id: None,
            channel: Arc::new(Mutex::new(ColorChannel::default())),
            selected_pending: None,
            edited_pending: None,
            cached_bounds: Rect::default(),
            last_anchor: None,
            text_painter: None,
        }
    }

    /// Sets the initial color.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Color, ColorPicker};
    ///
    /// let cp = ColorPicker::new().color(Color::rgb(0, 128, 255));
    /// assert_eq!(cp.get_color().g, 128);
    /// ```
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Sets whether the popup offers an alpha strip.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ColorPicker;
    ///
    /// let cp = ColorPicker::new().with_alpha(true);
    /// assert!(cp.with_alpha);
    /// ```
    #[must_use]
    pub fn with_alpha(mut self, with_alpha: bool) -> Self {
        self.with_alpha = with_alpha;
        self
    }

    /// Sets the accessible label (default `"Color picker"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ColorPicker;
    ///
    /// let cp = ColorPicker::new().label("Accent color");
    /// assert_eq!(cp.label.as_deref(), Some("Accent color"));
    /// ```
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets whether the well is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ColorPicker;
    ///
    /// let cp = ColorPicker::new().enabled(false);
    /// assert!(!cp.enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so the popup's text
    /// emits real glyph runs instead of `DrawText` placeholder boxes.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The committed color.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Color, ColorPicker};
    ///
    /// assert_eq!(ColorPicker::new().get_color(), Color::rgb(0, 0, 0));
    /// ```
    #[inline]
    pub fn get_color(&self) -> Color {
        self.color
    }

    /// Sets the color programmatically — does not feed
    /// [`take_edited`](Self::take_edited) or
    /// [`take_selected`](Self::take_selected) (those are user seams).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Color, ColorPicker};
    ///
    /// let mut cp = ColorPicker::new();
    /// cp.set_color(Color::rgb(1, 2, 3));
    /// assert_eq!(cp.get_color(), Color::rgb(1, 2, 3));
    /// assert_eq!(cp.take_selected(), None);
    /// ```
    pub fn set_color(&mut self, color: Color) {
        self.color = color;
    }

    /// Drains the latest live edit made inside the popup — every
    /// working-color change lands here, whether or not it is later
    /// confirmed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ColorPicker;
    ///
    /// let mut cp = ColorPicker::new();
    /// assert_eq!(cp.take_edited(), None);
    /// ```
    pub fn take_edited(&mut self) -> Option<Color> {
        self.edited_pending.take()
    }

    /// Drains the confirmed color (OK zone / `Enter` in the popup) —
    /// `None` when nothing was confirmed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ColorPicker;
    ///
    /// let mut cp = ColorPicker::new();
    /// assert_eq!(cp.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<Color> {
        self.selected_pending.take()
    }

    /// Whether the popup is logically open.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ColorPicker;
    ///
    /// let mut cp = ColorPicker::new();
    /// cp.open();
    /// assert!(cp.is_open());
    /// ```
    #[inline]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The overlay entry id of the open popup, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ColorPicker;
    ///
    /// assert_eq!(ColorPicker::new().popup_id(), None);
    /// ```
    #[inline]
    pub fn popup_id(&self) -> Option<u64> {
        self.popup_id
    }

    /// Opens the picker popup on the next
    /// [`sync_overlay`](Self::sync_overlay).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ColorPicker;
    ///
    /// let mut cp = ColorPicker::new();
    /// cp.open();
    /// assert!(cp.is_open());
    /// ```
    pub fn open(&mut self) {
        self.open = true;
        let mut channel = self.channel.lock().expect("color channel poisoned");
        channel.edited = None;
        channel.confirmed = None;
        channel.close_requested = false;
    }

    /// Closes the popup, discarding unconfirmed working edits.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ColorPicker;
    ///
    /// let mut cp = ColorPicker::new();
    /// cp.open();
    /// cp.close();
    /// assert!(!cp.is_open());
    /// ```
    pub fn close(&mut self) {
        self.open = false;
    }

    /// The display hex text — `#RRGGBB` or `#RRGGBBAA` when
    /// `with_alpha`.
    fn hex_text(&self) -> String {
        if self.with_alpha {
            self.color.to_hex_alpha()
        } else {
            self.color.to_hex()
        }
    }

    /// Applies whatever the surface wrote into the channel since the
    /// last drain — shared by `sync_overlay` and `a11y_prepare`.
    fn drain_channel(&mut self) {
        let (edited, confirmed, close_req) = {
            let mut channel = self.channel.lock().expect("color channel poisoned");
            (
                channel.edited.take(),
                channel.confirmed.take(),
                std::mem::take(&mut channel.close_requested),
            )
        };
        if let Some(c) = edited {
            self.edited_pending = Some(c);
        }
        if let Some(c) = confirmed {
            self.color = c;
            self.selected_pending = Some(c);
            self.open = false;
        }
        if close_req {
            self.open = false;
        }
    }

    /// Reconciles the overlay with the picker's open state.
    ///
    /// Call once per frame before `OverlayLayer::layout_pass`:
    ///
    /// - applies live edits / confirm / close requests from the
    ///   popup;
    /// - opens/closes the entry to match [`is_open`](Self::is_open);
    /// - notices overlay-level dismissal (outside press, `Escape`) —
    ///   working edits are discarded, live `take_edited` reports
    ///   already delivered stand;
    /// - re-anchors a live popup whose well moved.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::ColorPicker;
    /// use martensite_core::overlay::OverlayLayer;
    /// use martensite_core::{HotNode, LayoutContext, Rect, Widget};
    ///
    /// let mut cp = ColorPicker::new();
    /// let mut hot = HotNode::default();
    /// let mut cx = LayoutContext {
    ///     hot: &mut hot,
    ///     scale: 1.0,
    /// };
    /// cp.layout(&mut cx, Rect::new(10.0, 10.0, 44.0, 24.0));
    ///
    /// let mut overlay = OverlayLayer::new();
    /// overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// cp.open();
    /// cp.sync_overlay(&mut overlay);
    /// overlay.layout_pass();
    /// assert_eq!(overlay.len(), 1);
    /// ```
    pub fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        self.drain_channel();
        // The layer dismissed our popup (outside press / Escape).
        if let Some(id) = self.popup_id {
            if !overlay.is_open(id) {
                self.popup_id = None;
                self.open = false;
                self.last_anchor = None;
            }
        }
        if self.open && self.popup_id.is_none() {
            let (h, s, v) = rgb_to_hsv(self.color.r, self.color.g, self.color.b);
            let surface = ColorSurface {
                h,
                s,
                v,
                a: f32::from(self.color.a) / 255.0,
                with_alpha: self.with_alpha,
                active: Zone::Sv,
                dragging: None,
                bounds: Rect::default(),
                sv_rect: Rect::default(),
                hue_rect: Rect::default(),
                alpha_rect: Rect::default(),
                ok_rect: Rect::default(),
                channel: Arc::clone(&self.channel),
                painted_shape: Mutex::new(Shape::RECT),
                text_painter: self.text_painter.clone(),
            };
            self.popup_id =
                Some(overlay.open(Box::new(surface), OverlayAnchor::Bounds(self.cached_bounds)));
            self.last_anchor = Some(self.cached_bounds);
        } else if !self.open {
            if let Some(id) = self.popup_id.take() {
                overlay.close(id);
            }
            self.last_anchor = None;
        } else if let Some(id) = self.popup_id {
            // The well moved while open — re-anchor so the popup
            // tracks it (mirrors `Dropdown`).
            if self.last_anchor != Some(self.cached_bounds) {
                overlay.set_anchor(id, OverlayAnchor::Bounds(self.cached_bounds));
                self.last_anchor = Some(self.cached_bounds);
            }
        }
    }
}

impl Default for ColorPicker {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ColorPicker {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WELL_W).min(constraints.max_size.x.max(0.0)),
            cx.pt(WELL_H).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(WELL_W, WELL_H)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Button);
        node.set_label(self.label.as_deref().unwrap_or("Color picker"));
        node.set_value(self.hex_text());
        node.set_color_value(accesskit::Color {
            red: self.color.r,
            green: self.color.g,
            blue: self.color.b,
            alpha: self.color.a,
        });
        node.set_has_popup(accesskit::HasPopup::Dialog);
        node.set_expanded(self.open);
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Expand);
        node.add_action(accesskit::Action::Collapse);
        if self.enabled {
            node.add_action(accesskit::Action::Focus);
        } else {
            node.set_disabled();
        }
    }

    fn a11y_prepare(&mut self) {
        // AT-driven activations recorded through the surface land in
        // the channel — drain so the emitted tree reflects them even
        // when they bypassed `sync_overlay`.
        self.drain_channel();
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                ..
            } => {
                if self.open {
                    self.close();
                } else {
                    self.open();
                }
                EventResponse::CaptureFocus
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                // The OverlayLayer consumes `Escape` first in arena
                // use — kept for ownerless-embedded use.
                "Escape" => {
                    if self.open {
                        self.close();
                        return EventResponse::RequestRepaint;
                    }
                    EventResponse::Ignored
                }
                "Enter" | " " | "Space" | "ArrowDown" => {
                    if self.open {
                        self.close();
                    } else {
                        self.open();
                    }
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::Expand => {
                    self.open();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Collapse => {
                    self.close();
                    EventResponse::RequestRepaint
                }
                SemanticAction::Click => {
                    if self.open {
                        self.close();
                    } else {
                        self.open();
                    }
                    EventResponse::RequestRepaint
                }
                SemanticAction::Focus => EventResponse::CaptureFocus,
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // Delegate to the inherent method so
        // `ColorPicker::sync_overlay` and the `Widget` trait seam stay
        // in lock-step.
        ColorPicker::sync_overlay(self, overlay);
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        // Checkerboard underlay so alpha reads honestly, then the
        // swatch, then the border.
        let check = cx.pt(CHECK);
        let mut cy = b.min_y();
        while cy < b.max_y() {
            let mut cxx = b.min_x();
            let row_odd = ((cy - b.min_y()) / check) as i32 % 2 == 1;
            while cxx < b.max_x() {
                let col_odd = ((cxx - b.min_x()) / check) as i32 % 2 == 1;
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(cxx),
                        f64::from(cy),
                        f64::from((cxx + check).min(b.max_x())),
                        f64::from((cy + check).min(b.max_y())),
                    ),
                    if row_odd ^ col_odd { CHECK_A } else { CHECK_B },
                );
                cxx += check;
            }
            cy += check;
        }
        let well = Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 3.0));
        cx.list.push_fill_shape(rect, &well, self.color.to_rgba8());
        cx.list.push_stroke_shape(
            rect,
            &well,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, WELL_BORDER),
        );
    }
}

impl std::fmt::Debug for ColorPicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColorPicker")
            .field("color", &self.color)
            .field("with_alpha", &self.with_alpha)
            .field("enabled", &self.enabled)
            .field("open", &self.open)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(cp: &mut ColorPicker) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        cp.layout(&mut cx, Rect::new(10.0, 10.0, 44.0, 24.0));
    }

    fn overlay() -> OverlayLayer {
        let mut o = OverlayLayer::new();
        o.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
        o
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn event(cp: &mut ColorPicker, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: cp.cached_bounds,
            scale: 1.0,
        };
        cp.event(&mut cx)
    }

    fn surface_event(surface: &mut dyn Widget, bounds: Rect, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds,
            scale: 1.0,
        };
        surface.event(&mut cx)
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.02
    }

    #[test]
    fn hsv_roundtrip_primaries() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), (255, 0, 0));
        assert_eq!(hsv_to_rgb(60.0, 1.0, 1.0), (255, 255, 0));
        assert_eq!(hsv_to_rgb(120.0, 1.0, 1.0), (0, 255, 0));
        assert_eq!(hsv_to_rgb(180.0, 1.0, 1.0), (0, 255, 255));
        assert_eq!(hsv_to_rgb(240.0, 1.0, 1.0), (0, 0, 255));
        assert_eq!(hsv_to_rgb(300.0, 1.0, 1.0), (255, 0, 255));
        assert_eq!(rgb_to_hsv(255, 0, 0), (0.0, 1.0, 1.0));
        assert_eq!(rgb_to_hsv(0, 0, 255), (240.0, 1.0, 1.0));
    }

    #[test]
    fn hsv_roundtrip_arbitrary() {
        for &(r, g, b) in &[(12u8, 200u8, 90u8), (200, 30, 130), (77, 77, 200)] {
            let (h, s, v) = rgb_to_hsv(r, g, b);
            let (r2, g2, b2) = hsv_to_rgb(h, s, v);
            assert!((r as i32 - r2 as i32).abs() <= 2);
            assert!((g as i32 - g2 as i32).abs() <= 2);
            assert!((b as i32 - b2 as i32).abs() <= 2);
        }
    }

    #[test]
    fn hsv_edge_cases() {
        // Hue wraps; grey has s = 0.
        assert_eq!(hsv_to_rgb(360.0, 1.0, 1.0), (255, 0, 0));
        assert_eq!(hsv_to_rgb(-60.0, 1.0, 1.0), (255, 0, 255));
        let (_h, s, v) = rgb_to_hsv(128, 128, 128);
        assert_eq!(s, 0.0);
        assert!(approx(v, 128.0 / 255.0));
    }

    #[test]
    fn hex_format() {
        assert_eq!(Color::rgb(255, 128, 0).to_hex(), "#FF8000");
        assert_eq!(Color::rgba(1, 2, 3, 4).to_hex_alpha(), "#01020304");
    }

    #[test]
    fn face_opens_on_press_and_keys() {
        let mut cp = ColorPicker::new();
        laid_out(&mut cp);
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(20.0, 20.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(event(&mut cp, &press), EventResponse::CaptureFocus);
        assert!(cp.is_open());
        event(&mut cp, &key("Escape"));
        assert!(!cp.is_open());
        event(&mut cp, &key("ArrowDown"));
        assert!(cp.is_open());
    }

    #[test]
    fn overlay_opens_popup_below() {
        let mut cp = ColorPicker::new();
        laid_out(&mut cp);
        let mut o = overlay();
        cp.open();
        cp.sync_overlay(&mut o);
        o.layout_pass();
        assert_eq!(o.len(), 1);
        let b = o.entry_bounds(cp.popup_id.unwrap()).unwrap();
        assert!(b.min_y() >= 34.0);
    }

    #[test]
    fn sv_drag_edits_live() {
        let mut cp = ColorPicker::new().color(Color::rgb(255, 0, 0));
        laid_out(&mut cp);
        let mut o = overlay();
        cp.open();
        cp.sync_overlay(&mut o);
        o.layout_pass();
        let id = cp.popup_id.unwrap();
        let bounds = o.entry_bounds(id).unwrap();
        // SV square: pad inset, top-left of the popup; press at its
        // bottom-right corner → s=1, v=0 → black.
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(bounds.min_x() + 10.0 + 219.0, bounds.min_y() + 10.0 + 149.0),
            button: PointerButton::Primary,
            count: 1,
        };
        let surface = o.widget_at_mut(id, &[]).expect("surface widget");
        assert_eq!(
            surface_event(surface, bounds, &press),
            EventResponse::CapturePointer
        );
        cp.sync_overlay(&mut o);
        let edited = cp.take_edited().expect("live edit");
        // Bottom-right of the square: s ≈ 1, v ≈ 0 → near black.
        let (_h, s, v) = rgb_to_hsv(edited.r, edited.g, edited.b);
        assert!(s > 0.9 && v < 0.05);
        // Still unconfirmed — the swatch keeps the old color.
        assert_eq!(cp.get_color(), Color::rgb(255, 0, 0));
        assert!(cp.is_open());
    }

    #[test]
    fn ok_zone_confirms_and_closes() {
        let mut cp = ColorPicker::new().color(Color::rgb(255, 0, 0));
        laid_out(&mut cp);
        let mut o = overlay();
        cp.open();
        cp.sync_overlay(&mut o);
        o.layout_pass();
        let id = cp.popup_id.unwrap();
        let bounds = o.entry_bounds(id).unwrap();
        let surface = o.widget_at_mut(id, &[]).expect("surface widget");
        // Drag the SV square first — top-left corner is exactly
        // s=0, v=1 → white — then confirm with Enter.
        let drag = WidgetEvent::PointerPressed {
            position: Vec2::new(bounds.min_x() + 10.0, bounds.min_y() + 10.0),
            button: PointerButton::Primary,
            count: 1,
        };
        surface_event(surface, bounds, &drag);
        let release = WidgetEvent::PointerReleased {
            position: Vec2::ZERO,
            button: PointerButton::Primary,
        };
        assert_eq!(
            surface_event(surface, bounds, &release),
            EventResponse::ReleasePointer
        );
        surface_event(surface, bounds, &key("Enter"));
        cp.sync_overlay(&mut o);
        let picked = cp.take_selected().expect("confirmed");
        assert_eq!(picked, Color::rgb(255, 255, 255));
        assert_eq!(cp.get_color(), picked);
        assert!(!cp.is_open());
    }

    #[test]
    fn outside_press_discards_working_edits() {
        let mut cp = ColorPicker::new().color(Color::rgb(10, 20, 30));
        laid_out(&mut cp);
        let mut o = overlay();
        cp.open();
        cp.sync_overlay(&mut o);
        o.layout_pass();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
            count: 1,
        };
        o.dispatch_event(&press);
        cp.sync_overlay(&mut o);
        assert!(!cp.is_open());
        assert_eq!(cp.get_color(), Color::rgb(10, 20, 30));
        assert_eq!(cp.take_selected(), None);
    }

    #[test]
    fn surface_keyboard_nudges() {
        let mut cp = ColorPicker::new().color(Color::rgb(255, 0, 0));
        laid_out(&mut cp);
        let mut o = overlay();
        cp.open();
        cp.sync_overlay(&mut o);
        o.layout_pass();
        let id = cp.popup_id.unwrap();
        let bounds = o.entry_bounds(id).unwrap();
        let surface = o.widget_at_mut(id, &[]).expect("surface widget");
        // Active zone starts on the square: Left lowers saturation.
        surface_event(surface, bounds, &key("ArrowLeft"));
        cp.sync_overlay(&mut o);
        let edited = cp.take_edited().expect("nudge edit");
        let (_h, s, _v) = rgb_to_hsv(edited.r, edited.g, edited.b);
        assert!(s < 1.0);
        // Tab to hue, arrows change hue.
        let surface = o.widget_at_mut(id, &[]).expect("surface widget");
        surface_event(surface, bounds, &key("Tab"));
        surface_event(surface, bounds, &key("ArrowRight"));
        cp.sync_overlay(&mut o);
        let edited = cp.take_edited().expect("hue edit");
        let (h, _s, _v) = rgb_to_hsv(edited.r, edited.g, edited.b);
        assert!(h > 0.0);
    }

    #[test]
    fn accessibility_contract() {
        let mut cp = ColorPicker::new()
            .color(Color::rgba(60, 110, 220, 128))
            .with_alpha(true);
        cp.open();
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        cp.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Button);
        assert_eq!(node.label(), Some("Color picker"));
        assert_eq!(node.value(), Some("#3C6EDC80"));
        assert_eq!(node.has_popup(), Some(accesskit::HasPopup::Dialog));
        assert_eq!(node.is_expanded(), Some(true));
        let cv = node.color_value().expect("color value");
        assert_eq!((cv.red, cv.green, cv.blue, cv.alpha), (60, 110, 220, 128));
    }
}
