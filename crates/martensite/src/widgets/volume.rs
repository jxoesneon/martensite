//! `Volume` — a speaker icon + gain slider + mute toggle (the
//! system-tray / media-player volume idiom).
//!
//! Gain is `0.0..=1.0` with an optional boost range via
//! [`Volume::max`]. Clicking the speaker toggles mute (the pre-mute
//! level is restored on unmute); dragging or scrolling the rail
//! adjusts gain and implicitly unmutes. Every change parks the new
//! gain in [`Volume::take_changed`] and mutes park `bool` in
//! [`Volume::take_muted`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::volume::Volume;
//!
//! let mut v = Volume::new().gain(0.5);
//! v.toggle_mute();
//! assert!(v.is_muted());
//! assert_eq!(v.display_gain(), 0.0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_motion::SpringConfig;
use martensite_theme::TokenKey;

use crate::widgets::morph_icon::{demo, MorphIcon};

const ICON_PT: f32 = 20.0;
const PAD_PT: f32 = 8.0;
const RAIL_PT: f32 = 4.0;
const HANDLE_PT: f32 = 12.0;
const H_PT: f32 = 24.0;

const TRACK: [u8; 4] = [80, 82, 90, 255];
const FILL: [u8; 4] = [96, 165, 250, 255];
const FG: [u8; 4] = [200, 202, 210, 255];
const DIM: [u8; 4] = [110, 112, 120, 255];
const HANDLE: [u8; 4] = [240, 240, 245, 255];

/// One-time probe for the ADR-0041 geometry engine: builds the
/// canonical `VOLUME_ON` icon once per process and caches the result.
/// A `MorphError` — or a caught panic, reachable while the engine port
/// was still in flight — reports "not ready" and `Volume` paints the
/// hand-drawn glyph instead of crashing. Never panics.
fn engine_probe() -> bool {
    static READY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *READY.get_or_init(|| {
        std::panic::catch_unwind(|| MorphIcon::icon(demo::VOLUME_ON).is_ok()).unwrap_or(false)
    })
}

/// A speaker + rail volume control — see the module docs.
///
/// ```
/// use martensite::widgets::volume::Volume;
///
/// assert_eq!(Volume::new().gain_value(), 0.75);
/// ```
pub struct Volume {
    /// Accessibility label.
    pub label: String,
    gain: f32,
    max: f32,
    muted: bool,
    /// Gain saved when muted (restored on unmute).
    saved: f32,
    dragging: bool,
    changed: Option<f32>,
    muted_out: Option<bool>,
    rail: Rect,
    bounds: Rect,
    scale: f32,
    /// Embedded morphing speaker icon — an internal child occupying
    /// `icon_rect`, morphing the ADR-0041 `VOLUME_ON`/`VOLUME_OFF`
    /// pair on mute transitions. `icon_ok` flips to `false` on any
    /// engine error so `paint` falls back to the hand-drawn glyph.
    icon: MorphIcon,
    icon_ok: bool,
    /// `layout` has run — gates `child_bounds` per the internal-child
    /// contract (`None` before first layout so tick/paint/a11y walks
    /// skip the child).
    icon_laid_out: bool,
}

impl std::fmt::Debug for Volume {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Volume")
            .field("gain", &self.gain)
            .field("muted", &self.muted)
            .finish()
    }
}

impl Default for Volume {
    fn default() -> Self {
        Self::new()
    }
}

impl Volume {
    /// 75% gain, unmuted.
    ///
    /// ```
    /// use martensite::widgets::volume::Volume;
    ///
    /// assert_eq!(Volume::new().gain_value(), 0.75);
    /// ```
    pub fn new() -> Self {
        let icon_ok = engine_probe();
        // Decorative: the Volume row owns the a11y name.
        let icon = icon_ok
            .then(|| MorphIcon::icon(demo::VOLUME_ON).ok())
            .flatten()
            .unwrap_or_default()
            .decorative(true);
        Self {
            label: "Volume".to_string(),
            gain: 0.75,
            max: 1.0,
            muted: false,
            saved: 0.75,
            dragging: false,
            changed: None,
            muted_out: None,
            rail: Rect::new(0.0, 0.0, 0.0, 0.0),
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            icon,
            icon_ok,
            icon_laid_out: false,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::volume::Volume;
    ///
    /// assert_eq!(Volume::new().label("Master").label, "Master");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Initial gain `0.0..=max`.
    ///
    /// ```
    /// use martensite::widgets::volume::Volume;
    ///
    /// assert_eq!(Volume::new().gain(2.0).gain_value(), 1.0);
    /// ```
    pub fn gain(mut self, gain: f32) -> Self {
        self.gain = gain.clamp(0.0, self.max);
        self
    }

    /// Maximum gain (`1.0` = 100%; `1.5` allows boost).
    ///
    /// ```
    /// use martensite::widgets::volume::Volume;
    ///
    /// assert_eq!(Volume::new().max(1.5).max_value(), 1.5);
    /// ```
    pub fn max(mut self, max: f32) -> Self {
        self.max = max.clamp(0.1, 4.0);
        self.gain = self.gain.min(self.max);
        self
    }

    /// The configured maximum.
    ///
    /// ```
    /// use martensite::widgets::volume::Volume;
    ///
    /// assert_eq!(Volume::new().max_value(), 1.0);
    /// ```
    pub fn max_value(&self) -> f32 {
        self.max
    }

    /// Current gain (`0.0..=max`).
    ///
    /// ```
    /// use martensite::widgets::volume::Volume;
    ///
    /// assert_eq!(Volume::new().gain(0.5).gain_value(), 0.5);
    /// ```
    pub fn gain_value(&self) -> f32 {
        self.gain
    }

    /// Sets the gain (clamped, unmutes).
    ///
    /// ```
    /// use martensite::widgets::volume::Volume;
    ///
    /// let mut v = Volume::new().muted(true);
    /// v.set_gain(0.4);
    /// assert_eq!(v.gain_value(), 0.4);
    /// assert!(!v.is_muted());
    /// ```
    pub fn set_gain(&mut self, gain: f32) {
        self.gain = gain.clamp(0.0, self.max);
        if self.muted {
            self.muted = false;
            self.muted_out = Some(false);
            self.sync_icon(true);
        }
        self.changed = Some(self.gain);
    }

    /// Effective output (`0.0` while muted).
    ///
    /// ```
    /// use martensite::widgets::volume::Volume;
    ///
    /// assert_eq!(Volume::new().muted(true).display_gain(), 0.0);
    /// ```
    pub fn display_gain(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            self.gain
        }
    }

    /// Whether the control is muted.
    ///
    /// ```
    /// use martensite::widgets::volume::Volume;
    ///
    /// assert!(!Volume::new().is_muted());
    /// ```
    pub fn is_muted(&self) -> bool {
        self.muted
    }

    /// Sets the mute state (gain is preserved for unmute).
    ///
    /// ```
    /// use martensite::widgets::volume::Volume;
    ///
    /// let mut v = Volume::new().gain(0.6);
    /// v.set_muted(true);
    /// assert!(v.is_muted());
    /// v.set_muted(false);
    /// assert_eq!(v.gain_value(), 0.6);
    /// ```
    pub fn set_muted(&mut self, muted: bool) {
        if muted == self.muted {
            return;
        }
        if muted {
            self.saved = self.gain;
        }
        self.muted = muted;
        self.muted_out = Some(muted);
        self.sync_icon(true);
    }

    /// Muted builder.
    ///
    /// ```
    /// use martensite::widgets::volume::Volume;
    ///
    /// assert!(Volume::new().muted(true).is_muted());
    /// ```
    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self.sync_icon(false);
        self
    }

    /// Toggles mute.
    ///
    /// ```
    /// use martensite::widgets::volume::Volume;
    ///
    /// let mut v = Volume::new();
    /// v.toggle_mute();
    /// assert!(v.is_muted());
    /// v.toggle_mute();
    /// assert!(!v.is_muted());
    /// ```
    pub fn toggle_mute(&mut self) {
        self.set_muted(!self.muted);
    }

    /// Drains the last committed gain.
    ///
    /// ```
    /// use martensite::widgets::volume::Volume;
    ///
    /// assert_eq!(Volume::new().take_changed(), None);
    /// ```
    pub fn take_changed(&mut self) -> Option<f32> {
        self.changed.take()
    }

    /// Drains the last mute transition.
    ///
    /// ```
    /// use martensite::widgets::volume::Volume;
    ///
    /// assert_eq!(Volume::new().take_muted(), None);
    /// ```
    pub fn take_muted(&mut self) -> Option<bool> {
        self.muted_out.take()
    }

    /// Drives the embedded [`MorphIcon`] to the state matching
    /// `self.muted` — `morph_to` under [`SpringConfig::SNAPPY`] when
    /// `animate`, a plain `set_icon` jump otherwise. Any engine error
    /// retires the icon (`icon_ok = false`) and `paint` falls back to
    /// the hand-drawn glyph; never panics.
    fn sync_icon(&mut self, animate: bool) {
        if !self.icon_ok {
            return;
        }
        let d = if self.muted {
            demo::VOLUME_OFF
        } else {
            demo::VOLUME_ON
        };
        // `Ok(Err)` is the contract error path; a caught panic —
        // reachable while the ADR-0041 engine port is in flight —
        // retires the icon the same way. Never propagates.
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if animate {
                self.icon.morph_to(d, SpringConfig::SNAPPY)
            } else {
                self.icon.set_icon(d)
            }
        }));
        if !matches!(r, Ok(Ok(()))) {
            self.icon_ok = false;
        }
    }

    /// Speaker icon rect — also the internal MorphIcon child's
    /// bounds. Under RTL it anchors on the leading (right) edge.
    fn icon_rect(&self) -> Rect {
        let s = self.scale;
        let x = if martensite_core::intl::ambient_direction().is_rtl() {
            self.bounds.max_x() - PAD_PT * s - ICON_PT * s
        } else {
            self.bounds.min_x() + PAD_PT * s
        };
        Rect::new(
            x,
            self.bounds.min_y() + (self.bounds.height() - ICON_PT * s) / 2.0,
            ICON_PT * s,
            ICON_PT * s,
        )
    }

    /// Gain from a rail x-coordinate.
    fn gain_at(&self, x: f32) -> f32 {
        let f = ((x - self.rail.min_x()) / self.rail.width().max(0.001)).clamp(0.0, 1.0);
        // Under RTL the rail's zero end anchors on the right.
        let f = if martensite_core::intl::ambient_direction().is_rtl() {
            1.0 - f
        } else {
            f
        };
        f * self.max
    }
}

impl Widget for Volume {
    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(140.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, H_PT * 0.6)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let icon = self.icon_rect();
        // Under RTL the rail sits left of the right-anchored icon.
        let (rail_x, rail_w) = if cx.is_rtl() {
            (
                bounds.min_x() + PAD_PT * s,
                (icon.min_x() - bounds.min_x() - PAD_PT * s * 2.0).max(0.0),
            )
        } else {
            (
                icon.max_x() + PAD_PT * s,
                (bounds.max_x() - icon.max_x() - PAD_PT * s * 2.0).max(0.0),
            )
        };
        self.rail = Rect::new(
            rail_x,
            bounds.min_y() + (bounds.height() - HANDLE_PT * s) / 2.0,
            rail_w,
            HANDLE_PT * s,
        );
        // Allocate the internal MorphIcon child its glyph rect so the
        // framework's tick/paint/a11y recursion reaches it.
        self.icon.layout(cx, icon);
        self.icon_laid_out = true;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Slider);
        node.set_label(format!(
            "{} — {:.0}%{}",
            self.label,
            self.display_gain() / self.max * 100.0,
            if self.muted { " muted" } else { "" }
        ));
        node.set_numeric_value(f64::from(self.display_gain() / self.max));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if self.icon_rect().contains(*position) {
                    self.toggle_mute();
                    return EventResponse::RequestRepaint;
                }
                // Generous rail hitbox.
                let hit = Rect::new(
                    self.rail.min_x(),
                    self.rail.min_y() - 6.0 * self.scale,
                    self.rail.width(),
                    self.rail.height() + 12.0 * self.scale,
                );
                if hit.contains(*position) {
                    self.dragging = true;
                    self.set_gain(self.gain_at(position.x));
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if self.dragging {
                    self.set_gain(self.gain_at(position.x));
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
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::Scroll { position, delta } => {
                if self.bounds.contains(*position) {
                    self.set_gain(self.gain + delta.y * 0.02 + delta.x * 0.02);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowLeft" | "ArrowDown" | "ArrowRight" | "ArrowUp" => {
                    // Under RTL the horizontal arrows mirror.
                    let inc = key == "ArrowRight" || key == "ArrowUp";
                    let d = if inc != cx.is_rtl() { 0.05 } else { -0.05 };
                    self.set_gain(self.gain + self.max * d);
                    EventResponse::RequestRepaint
                }
                "m" | "M" => {
                    self.toggle_mute();
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    // Internal-child protocol — the embedded MorphIcon is child 0,
    // occupying the glyph rect computed by `icon_rect`. Bounds stay
    // `None` until `layout` runs (and while the icon is retired to the
    // hand-drawn fallback) so tick/paint/a11y recursion skips it.
    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&self.icon as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut self.icon as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0 && self.icon_laid_out && self.icon_ok).then(|| self.icon_rect())
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
        let s = self.scale;

        // Speaker icon: the embedded MorphIcon child paints the
        // VOLUME_ON ↔ VOLUME_OFF morph (ADR-0041) while `icon_ok`
        // holds; on engine failure fall back to the hand-drawn glyph —
        // box + wedge + wave arcs (hidden when muted).
        if !self.icon_ok {
            let fg = cx.color(TokenKey::TextColor, if self.muted { DIM } else { FG });
            let icon = self.icon_rect();
            let ix = icon.min_x();
            let iy = icon.min_y();
            let iw = icon.width();
            let ih = icon.height();
            cx.list.push_fill_rect(
                krect(Rect::new(ix, iy + ih * 0.32, iw * 0.25, ih * 0.36)),
                fg,
            );
            let mut wedge = kurbo::BezPath::new();
            wedge.move_to(pt(Vec2::new(ix + iw * 0.25, iy + ih * 0.35)));
            wedge.line_to(pt(Vec2::new(ix + iw * 0.62, iy + ih * 0.1)));
            wedge.line_to(pt(Vec2::new(ix + iw * 0.62, iy + ih * 0.9)));
            wedge.line_to(pt(Vec2::new(ix + iw * 0.25, iy + ih * 0.65)));
            wedge.close_path();
            cx.list.push_path(wedge, fg);
            if self.muted {
                // ✕ over the speaker.
                let mut x = kurbo::BezPath::new();
                x.move_to(pt(Vec2::new(ix + iw * 0.62, iy + ih * 0.35)));
                x.line_to(pt(Vec2::new(ix + iw * 0.95, iy + ih * 0.65)));
                x.move_to(pt(Vec2::new(ix + iw * 0.95, iy + ih * 0.35)));
                x.line_to(pt(Vec2::new(ix + iw * 0.62, iy + ih * 0.65)));
                cx.list
                    .push_stroke_path(x, 1.6 * s, cx.color(TokenKey::ErrorColor, DIM));
            } else {
                // Wave arcs scale with gain.
                let waves = (self.display_gain() / self.max * 3.0).ceil() as usize;
                for i in 0..waves.min(3) {
                    let rr = iw * (0.18 + i as f32 * 0.14);
                    let cy = iy + ih / 2.0;
                    let ax = ix + iw * 0.62;
                    let mut arc = kurbo::BezPath::new();
                    arc.move_to(pt(Vec2::new(ax + rr * 0.3, cy - rr)));
                    arc.line_to(pt(Vec2::new(ax + rr, cy - rr * 0.5)));
                    arc.line_to(pt(Vec2::new(ax + rr, cy + rr * 0.5)));
                    arc.line_to(pt(Vec2::new(ax + rr * 0.3, cy + rr)));
                    cx.list.push_stroke_path(arc, 1.2 * s, fg);
                }
            }
        }

        // Rail: track + fill + handle.
        let rail_h = RAIL_PT * s;
        let ry = self.rail.min_y() + (self.rail.height() - rail_h) / 2.0;
        let track = Rect::new(self.rail.min_x(), ry, self.rail.width(), rail_h);
        let shape = &martensite_core::shape::Shape::rounded(rail_h / 2.0);
        cx.list
            .push_fill_shape(krect(track), shape, cx.color(TokenKey::DividerColor, TRACK));
        let frac = self.display_gain() / self.max;
        // Under RTL the fill and handle anchor on the right.
        if frac > 0.0 {
            let fill = if cx.is_rtl() {
                Rect::new(
                    track.max_x() - track.width() * frac,
                    ry,
                    track.width() * frac,
                    rail_h,
                )
            } else {
                Rect::new(track.min_x(), ry, track.width() * frac, rail_h)
            };
            cx.list
                .push_fill_shape(krect(fill), shape, cx.color(TokenKey::AccentColor, FILL));
        }
        let hx = if cx.is_rtl() {
            self.rail.max_x() - self.rail.width() * frac
        } else {
            self.rail.min_x() + self.rail.width() * frac
        };
        let hd = HANDLE_PT * s;
        cx.list.push_fill_shape(
            krect(Rect::new(
                hx - hd / 2.0,
                self.rail.min_y() + (self.rail.height() - hd) / 2.0,
                hd,
                hd,
            )),
            &martensite_core::shape::Shape::ELLIPSE,
            cx.color(TokenKey::TextInverseColor, HANDLE),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(v: &mut Volume, w: f32, h: f32) {
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

    fn ev(v: &mut Volume, e: &WidgetEvent) -> EventResponse {
        v.event(&mut EventContext {
            event: e,
            bounds: v.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn icon_toggles_mute() {
        let mut v = Volume::new().gain(0.6);
        laid_out(&mut v, 140.0, 24.0);
        let icon = v.icon_rect();
        let p = Vec2::new(
            (icon.min_x() + icon.max_x()) / 2.0,
            (icon.min_y() + icon.max_y()) / 2.0,
        );
        ev(
            &mut v,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: p,
                count: 1,
            },
        );
        assert!(v.is_muted());
        assert_eq!(v.take_muted(), Some(true));
        assert_eq!(v.display_gain(), 0.0);
        ev(
            &mut v,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: p,
                count: 1,
            },
        );
        assert!(!v.is_muted());
        assert_eq!(v.gain_value(), 0.6);
    }

    #[test]
    fn rail_drag_sets_gain() {
        let mut v = Volume::new();
        laid_out(&mut v, 140.0, 24.0);
        let mid = Vec2::new(
            (v.rail.min_x() + v.rail.max_x()) / 2.0,
            (v.rail.min_y() + v.rail.max_y()) / 2.0,
        );
        assert_eq!(
            ev(
                &mut v,
                &WidgetEvent::PointerPressed {
                    button: PointerButton::Primary,
                    position: mid,
                    count: 1,
                }
            ),
            EventResponse::CapturePointer
        );
        assert!((v.gain_value() - 0.5).abs() < 0.05);
        assert!(v.take_changed().is_some());
    }

    #[test]
    fn arrows_and_scroll() {
        let mut v = Volume::new().gain(0.5);
        laid_out(&mut v, 140.0, 24.0);
        ev(
            &mut v,
            &WidgetEvent::KeyPressed {
                key: "ArrowUp".to_string(),
                repeat: false,
            },
        );
        assert!((v.gain_value() - 0.55).abs() < 1e-5);
        ev(
            &mut v,
            &WidgetEvent::Scroll {
                position: Vec2::new(70.0, 12.0),
                delta: Vec2::new(0.0, 10.0),
            },
        );
        assert!(v.gain_value() > 0.55);
    }

    #[test]
    fn scroll_unmutes() {
        let mut v = Volume::new().gain(0.5).muted(true);
        laid_out(&mut v, 140.0, 24.0);
        ev(
            &mut v,
            &WidgetEvent::Scroll {
                position: Vec2::new(70.0, 12.0),
                delta: Vec2::new(0.0, 5.0),
            },
        );
        assert!(!v.is_muted());
        assert_eq!(v.take_muted(), Some(false));
    }

    #[test]
    fn mute_drives_internal_icon() {
        let mut v = Volume::new().gain(0.6);
        laid_out(&mut v, 140.0, 24.0);

        // Internal-child wiring: one MorphIcon child, present via the
        // protocol whether or not the engine leg has landed.
        assert_eq!(v.child_count(), 1);
        assert_eq!(v.child(0).unwrap().debug_name(), "MorphIcon");
        assert!(!v.icon.is_animating());

        // Never panics — engine landed or not.
        v.set_muted(true);
        assert!(v.is_muted());
        assert_eq!(v.take_muted(), Some(true));

        // The demo `d` constants are canonical — the probe must pass.
        assert!(v.icon_ok, "MorphIcon::icon(VOLUME_ON) failed");
        // The toggle is mid-morph, allocated the glyph rect, and
        // drivable through `child_mut` — exactly what the arena's tick
        // recursion does per frame.
        assert_eq!(v.child_bounds(0), Some(v.icon_rect()));
        assert!(v.icon.is_animating());
        assert!(v
            .child_mut(0)
            .unwrap()
            .tick(std::time::Duration::from_millis(16)));

        // Unmute re-morphs toward VOLUME_ON mid-flight.
        v.set_muted(false);
        assert!(v.icon.is_animating());

        // The spring eventually settles.
        for _ in 0..600 {
            v.child_mut(0)
                .unwrap()
                .tick(std::time::Duration::from_millis(16));
        }
        assert!(!v.icon.is_animating());
    }

    #[test]
    fn icon_child_hidden_until_layout() {
        let v = Volume::new();
        assert_eq!(v.child_count(), 1);
        assert!(v.child(0).is_some());
        assert_eq!(v.child_bounds(0), None);
    }

    #[test]
    fn reduced_motion_snaps_icon() {
        let mut v = Volume::new();
        laid_out(&mut v, 140.0, 24.0);
        // Default `set_reduced_motion` forwards to internal children.
        v.set_reduced_motion(true);
        v.set_muted(true);
        assert!(!v.icon.is_animating());
        assert_eq!(v.icon.progress(), 1.0);
    }

    #[test]
    fn builder_muted_sets_icon_without_flight() {
        let mut v = Volume::new().muted(true);
        laid_out(&mut v, 140.0, 24.0);
        assert!(!v.icon.is_animating());
        // A later unmute morphs toward VOLUME_ON.
        v.set_muted(false);
        assert!(v.icon.is_animating());
    }

    #[test]
    fn boost_range() {
        let mut v = Volume::new().max(1.5);
        v.set_gain(1.2);
        assert_eq!(v.gain_value(), 1.2);
        v.set_gain(9.0);
        assert_eq!(v.gain_value(), 1.5);
    }
}
