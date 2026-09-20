//! `Equalizer` — a bank of vertical gain sliders (the mixer /
//! graphic-EQ channel-strip idiom; control-side companion to
//! the [`crate::widgets::spectrum::Spectrum`] display).
//!
//! Each band is a `0.0..=1.0` fader (0.5 = unity). Dragging a
//! slider moves it and parks [`Equalizer::take_changed`];
//! [`Equalizer::changed_band`] reports which band moved last.
//! Arrow keys nudge the focused band, `0` resets it to unity,
//! and `Tab`-style `Left`/`Right` moves focus between bands.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::equalizer::Equalizer;
//!
//! let e = Equalizer::new().bands([0.5, 0.7, 0.3]);
//! assert_eq!(e.band_count(), 3);
//! assert_eq!(e.gain(1), 0.7);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const BAND_W_PT: f32 = 18.0;
const GAP_PT: f32 = 4.0;
const HEIGHT_PT: f32 = 110.0;
const PAD_PT: f32 = 6.0;
const THUMB_PT: f32 = 8.0;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const TRACK: [u8; 4] = [58, 58, 66, 255];
const THUMB: [u8; 4] = [110, 170, 230, 255];
const THUMB_HI: [u8; 4] = [140, 195, 245, 255];
const UNITY: [u8; 4] = [150, 150, 160, 255];

/// A bank of vertical gain faders — see the module docs.
///
/// ```
/// use martensite::widgets::equalizer::Equalizer;
///
/// assert_eq!(Equalizer::new().band_count(), 0);
/// ```
#[derive(Debug)]
pub struct Equalizer {
    /// Accessibility label.
    pub label: String,
    gains: Vec<f32>,
    focused: usize,
    dragging: Option<usize>,
    changed: bool,
    last_band: Option<usize>,
    bounds: Rect,
    scale: f32,
}

impl Default for Equalizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Equalizer {
    /// Creates an empty bank.
    ///
    /// ```
    /// use martensite::widgets::equalizer::Equalizer;
    ///
    /// assert_eq!(Equalizer::new().band_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Equalizer".to_string(),
            gains: Vec::new(),
            focused: 0,
            dragging: None,
            changed: false,
            last_band: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets band gains (`0.0..=1.0`, clamped; 0.5 = unity).
    ///
    /// ```
    /// use martensite::widgets::equalizer::Equalizer;
    ///
    /// assert_eq!(Equalizer::new().bands([0.5, 2.0]).gain(1), 1.0);
    /// ```
    pub fn bands(mut self, gains: impl IntoIterator<Item = f32>) -> Self {
        self.gains = gains.into_iter().map(|g| g.clamp(0.0, 1.0)).collect();
        self
    }

    /// Sets band count at unity gain.
    ///
    /// ```
    /// use martensite::widgets::equalizer::Equalizer;
    ///
    /// assert_eq!(Equalizer::new().faders(8).gain(3), 0.5);
    /// ```
    pub fn faders(mut self, n: usize) -> Self {
        self.gains = vec![0.5; n];
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::equalizer::Equalizer;
    ///
    /// assert_eq!(Equalizer::new().label("Mix").label, "Mix");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Band count.
    ///
    /// ```
    /// use martensite::widgets::equalizer::Equalizer;
    ///
    /// assert_eq!(Equalizer::new().bands([0.5; 5]).band_count(), 5);
    /// ```
    pub fn band_count(&self) -> usize {
        self.gains.len()
    }

    /// A band's gain.
    ///
    /// ```
    /// use martensite::widgets::equalizer::Equalizer;
    ///
    /// assert_eq!(Equalizer::new().bands([0.8]).gain(0), 0.8);
    /// ```
    pub fn gain(&self, band: usize) -> f32 {
        self.gains.get(band).copied().unwrap_or(0.0)
    }

    /// Sets a band's gain directly.
    ///
    /// ```
    /// use martensite::widgets::equalizer::Equalizer;
    ///
    /// let mut e = Equalizer::new().faders(2);
    /// e.set_gain(0, 0.9);
    /// assert_eq!(e.gain(0), 0.9);
    /// ```
    pub fn set_gain(&mut self, band: usize, value: f32) {
        if let Some(g) = self.gains.get_mut(band) {
            *g = value.clamp(0.0, 1.0);
            self.last_band = Some(band);
            self.changed = true;
        }
    }

    /// Resets every band to unity (0.5).
    ///
    /// ```
    /// use martensite::widgets::equalizer::Equalizer;
    ///
    /// let mut e = Equalizer::new().bands([0.9, 0.1]);
    /// e.reset();
    /// assert_eq!(e.gain(0), 0.5);
    /// ```
    pub fn reset(&mut self) {
        self.gains.fill(0.5);
        self.changed = true;
    }

    /// All band gains.
    ///
    /// ```
    /// use martensite::widgets::equalizer::Equalizer;
    ///
    /// assert_eq!(Equalizer::new().bands([0.1, 0.2]).gains(), &[0.1, 0.2]);
    /// ```
    pub fn gains(&self) -> &[f32] {
        &self.gains
    }

    /// Band that moved most recently.
    ///
    /// ```
    /// use martensite::widgets::equalizer::Equalizer;
    ///
    /// assert_eq!(Equalizer::new().changed_band(), None);
    /// ```
    pub fn changed_band(&self) -> Option<usize> {
        self.last_band
    }

    /// Drains whether any band moved since the last call.
    ///
    /// ```
    /// use martensite::widgets::equalizer::Equalizer;
    ///
    /// assert!(!Equalizer::new().take_changed());
    /// ```
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Band under an x-coordinate.
    fn band_at(&self, p: Vec2) -> Option<usize> {
        if !self.bounds.contains(p) {
            return None;
        }
        let pad = PAD_PT * self.scale;
        let gap = GAP_PT * self.scale;
        let pitch = (self.bounds.width() - 2.0 * pad + gap) / self.gains.len().max(1) as f32;
        let i = ((p.x - self.bounds.min_x() - pad) / pitch) as usize;
        (i < self.gains.len()).then_some(i)
    }

    /// Slider track rect for a band.
    fn track_rect(&self, band: usize) -> Rect {
        let pad = PAD_PT * self.scale;
        let gap = GAP_PT * self.scale;
        let pitch = (self.bounds.width() - 2.0 * pad + gap) / self.gains.len().max(1) as f32;
        let w = (pitch - gap).min(BAND_W_PT * self.scale);
        let x = self.bounds.min_x() + pad + band as f32 * pitch + (pitch - w) / 2.0;
        Rect::new(
            x,
            self.bounds.min_y() + pad,
            w,
            self.bounds.height() - 2.0 * pad,
        )
    }

    /// Set a band's gain from a pointer y.
    fn set_from_y(&mut self, band: usize, y: f32) {
        let r = self.track_rect(band);
        let g = 1.0 - (y - r.min_y()) / r.height().max(1.0);
        self.set_gain(band, g);
    }
}

impl Widget for Equalizer {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let w = (self.gains.len() as f32 * (BAND_W_PT + GAP_PT) + 2.0 * PAD_PT).max(60.0);
        Vec2::new(
            cx.pt(w).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.focused = self.focused.min(self.gains.len().saturating_sub(1));
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!("{} — {} bands", self.label, self.gains.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(b) = self.band_at(*position) {
                    self.dragging = Some(b);
                    self.focused = b;
                    self.set_from_y(b, position.y);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if let Some(b) = self.dragging {
                    self.set_from_y(b, position.y);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.dragging.take().is_some() {
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => {
                if self.gains.is_empty() {
                    return EventResponse::Ignored;
                }
                match key.as_str() {
                    "ArrowLeft" => {
                        self.focused = self.focused.saturating_sub(1);
                        EventResponse::RequestRepaint
                    }
                    "ArrowRight" => {
                        self.focused = (self.focused + 1).min(self.gains.len() - 1);
                        EventResponse::RequestRepaint
                    }
                    "ArrowUp" => {
                        let f = self.focused;
                        self.set_gain(f, self.gain(f) + 0.05);
                        EventResponse::RequestRepaint
                    }
                    "ArrowDown" => {
                        let f = self.focused;
                        self.set_gain(f, self.gain(f) - 0.05);
                        EventResponse::RequestRepaint
                    }
                    "0" => {
                        let f = self.focused;
                        self.set_gain(f, 0.5);
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
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        let thumb_c = cx.color(TokenKey::AccentColor, THUMB);
        let unity = cx.color(TokenKey::TextMutedColor, UNITY);
        for (i, &g) in self.gains.iter().enumerate() {
            let r = self.track_rect(i);
            // Track groove.
            let tw = 3.0 * self.scale;
            let groove = Rect::new(
                r.min_x() + r.width() / 2.0 - tw / 2.0,
                r.min_y(),
                tw,
                r.height(),
            );
            cx.list.push_fill_shape(
                krect(groove),
                &martensite_core::shape::Shape::rounded(tw / 2.0),
                TRACK,
            );
            // Unity marker at 0.5.
            let uy = r.min_y() + r.height() * 0.5;
            let mut u = kurbo::BezPath::new();
            u.move_to((f64::from(r.min_x()), f64::from(uy)));
            u.line_to((f64::from(r.max_x()), f64::from(uy)));
            cx.list.push_stroke_path(u, cx.pt(0.5), unity);
            // Thumb.
            let th = THUMB_PT * self.scale;
            let ty = r.min_y() + (1.0 - g) * r.height();
            let thumb = Rect::new(r.min_x(), ty - th / 2.0, r.width(), th);
            let fill = if self.dragging == Some(i) || (self.focused == i && self.dragging.is_none())
            {
                THUMB_HI
            } else {
                thumb_c
            };
            cx.list.push_fill_shape(
                krect(thumb),
                &martensite_core::shape::Shape::rounded(cx.pt(2.0)),
                fill,
            );
            cx.list.push_stroke_shape(
                krect(thumb),
                &martensite_core::shape::Shape::rounded(cx.pt(2.0)),
                cx.pt(0.75),
                edge,
            );
        }
        cx.list.push_stroke_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.pt(0.75),
            edge,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(e: &mut Equalizer, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        e.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        e.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev(e: &mut Equalizer, ev: &WidgetEvent) {
        e.event(&mut EventContext {
            event: ev,
            bounds: e.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn bands_and_gains() {
        let e = Equalizer::new().bands([0.5, 0.7, 0.3]);
        assert_eq!(e.band_count(), 3);
        assert_eq!(e.gain(1), 0.7);
        assert_eq!(e.gains(), &[0.5, 0.7, 0.3]);
    }

    #[test]
    fn clamps_input() {
        let e = Equalizer::new().bands([2.0, -1.0]);
        assert_eq!(e.gains(), &[1.0, 0.0]);
    }

    #[test]
    fn drag_sets_gain() {
        let mut e = Equalizer::new().bands([0.5, 0.5]);
        laid_out(&mut e, 120.0, 120.0);
        // Band 0 track: x ~6..24; drag to top → gain 1.
        ev(
            &mut e,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(15.0, 10.0),
                count: 1,
            },
        );
        assert!(e.gain(0) > 0.9);
        assert_eq!(e.changed_band(), Some(0));
        assert!(e.take_changed());
        ev(
            &mut e,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(15.0, 10.0),
            },
        );
    }

    #[test]
    fn keyboard_moves_focus_and_gain() {
        let mut e = Equalizer::new().bands([0.5, 0.5, 0.5]);
        laid_out(&mut e, 120.0, 120.0);
        ev(
            &mut e,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        ev(
            &mut e,
            &WidgetEvent::KeyPressed {
                key: "ArrowUp".to_string(),
                repeat: false,
            },
        );
        assert!((e.gain(1) - 0.55).abs() < 1e-5);
        assert_eq!(e.changed_band(), Some(1));
        ev(
            &mut e,
            &WidgetEvent::KeyPressed {
                key: "0".to_string(),
                repeat: false,
            },
        );
        assert_eq!(e.gain(1), 0.5);
    }

    #[test]
    fn reset_restores_unity() {
        let mut e = Equalizer::new().bands([0.9, 0.1]);
        e.reset();
        assert_eq!(e.gains(), &[0.5, 0.5]);
        assert!(e.take_changed());
    }
}
