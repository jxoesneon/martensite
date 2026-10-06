//! `StatusDot` — a severity status lamp (industrial HMI lamp, Ant
//! `Badge.Status`, GTK status icon).
//!
//! A filled severity-colored circle with an optional text label —
//! the at-a-glance "pump online" / "link degraded" indicator rows of
//! an operations dashboard. Display-only; the app flips the status
//! through [`StatusDot::set_status`]. A `pulse` flag paints a halo
//! ring for active-but-attention states.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::status_dot::{Status, StatusDot};
//!
//! let d = StatusDot::new("Pump A").status(Status::Ok);
//! assert_eq!(d.status_value(), Status::Ok);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

const DOT_PT: f32 = 10.0;
const GAP_PT: f32 = 6.0;
const FONT_PT: f32 = 12.0;
const HEIGHT_PT: f32 = 18.0;

const FG: [u8; 4] = [30, 30, 36, 255];
const MUTED: [u8; 4] = [110, 110, 118, 255];
const OK: [u8; 4] = [46, 160, 90, 255];
const WARN: [u8; 4] = [220, 160, 40, 255];
const ERR: [u8; 4] = [210, 60, 60, 255];
const OFF: [u8; 4] = [160, 160, 166, 255];
const INFO: [u8; 4] = [60, 120, 220, 255];

/// Status severity for the lamp.
///
/// ```
/// use martensite::widgets::status_dot::Status;
///
/// assert_ne!(Status::Ok, Status::Error);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Neutral/no signal — gray.
    Off,
    /// Informational — blue.
    Info,
    /// Nominal — green.
    Ok,
    /// Degraded — amber.
    Warning,
    /// Fault — red.
    Error,
}

impl Status {
    /// Human-readable status word — used in accessible names so the
    /// chip's meaning reaches assistive tech, not just the paint list.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Info => "info",
            Self::Ok => "ok",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    /// The theme token this status resolves through.
    pub(crate) fn token(self) -> TokenKey {
        match self {
            Self::Off => TokenKey::TextMutedColor,
            Self::Info => TokenKey::AccentColor,
            Self::Ok => TokenKey::SuccessColor,
            Self::Warning => TokenKey::WarningColor,
            Self::Error => TokenKey::ErrorColor,
        }
    }

    /// sRGBA fallback when the theme carries no token.
    pub(crate) fn fallback(self) -> [u8; 4] {
        match self {
            Self::Off => OFF,
            Self::Info => INFO,
            Self::Ok => OK,
            Self::Warning => WARN,
            Self::Error => ERR,
        }
    }
}

/// Paints the per-status mark centered at `center` — the redundant
/// non-color channel for status lamps (WCAG 1.4.1 / ISA-101: status
/// is never conveyed by hue alone). `r` is the mark's radius in
/// device px; `ink` is the mark color (inverse ink on the lamp fill).
///
/// Marks: `Ok` check, `Warning` exclamation, `Error` cross, `Info`
/// i-stem, `Off` dash.
pub(crate) fn paint_status_glyph(
    list: &mut martensite_core::PaintList,
    center: Vec2,
    r: f32,
    status: Status,
    ink: [u8; 4],
) {
    let (x, y) = (f64::from(center.x), f64::from(center.y));
    let r = f64::from(r);
    let mut path = kurbo::BezPath::new();
    match status {
        Status::Ok => {
            // Check mark.
            path.move_to((x - r * 0.52, y + r * 0.05));
            path.line_to((x - r * 0.12, y + r * 0.42));
            path.line_to((x + r * 0.55, y - r * 0.38));
        }
        Status::Warning => {
            // Exclamation stem; the dot is filled below.
            path.move_to((x, y - r * 0.5));
            path.line_to((x, y + r * 0.12));
        }
        Status::Error => {
            // Cross.
            path.move_to((x - r * 0.38, y - r * 0.38));
            path.line_to((x + r * 0.38, y + r * 0.38));
            path.move_to((x + r * 0.38, y - r * 0.38));
            path.line_to((x - r * 0.38, y + r * 0.38));
        }
        Status::Info => {
            // i-stem; the dot is filled above.
            path.move_to((x, y - r * 0.02));
            path.line_to((x, y + r * 0.5));
        }
        Status::Off => {
            // Horizontal dash — the "muted/no signal" mark.
            path.move_to((x - r * 0.42, y));
            path.line_to((x + r * 0.42, y));
        }
    }
    list.push_stroke_path(path, (r * 0.34).max(1.0) as f32, ink);
    let dot_dy = match status {
        Status::Warning => Some(y + r * 0.44),
        Status::Info => Some(y - r * 0.4),
        _ => None,
    };
    if let Some(dy) = dot_dy {
        let dr = r * 0.18;
        list.push_fill_shape(
            kurbo::Rect::new(x - dr, dy - dr, x + dr, dy + dr),
            &martensite_core::shape::Shape::circle(Vec2::new(x as f32, dy as f32), dr as f32),
            ink,
        );
    }
}

/// Paints the "data unknown" mark centered at `center` — a hollow
/// question-mark glyph in muted ink. This is the safety-widget
/// loading treatment (ADR-0040): a pending status lamp must read
/// *unknown*, never as a live value and never as generic shimmer —
/// `paint_loading` callers pass this mark instead of the shared
/// skeleton painter.
pub(crate) fn paint_unknown_glyph(
    list: &mut martensite_core::PaintList,
    center: Vec2,
    r: f32,
    ink: [u8; 4],
) {
    let (x, y) = (f64::from(center.x), f64::from(center.y));
    let r = f64::from(r);
    let mut path = kurbo::BezPath::new();
    // The hook: a small arc over the top, curling into the stem.
    path.move_to((x - r * 0.42, y - r * 0.3));
    path.quad_to((x - r * 0.35, y - r * 0.72), (x, y - r * 0.72));
    path.quad_to((x + r * 0.42, y - r * 0.72), (x + r * 0.38, y - r * 0.28));
    path.quad_to((x + r * 0.34, y - r * 0.02), (x, y + r * 0.12));
    path.line_to((x, y + r * 0.3));
    list.push_stroke_path(path, (r * 0.3).max(1.0) as f32, ink);
    // The dot under the stem.
    let dr = r * 0.2;
    let dy = y + r * 0.58;
    list.push_fill_shape(
        kurbo::Rect::new(x - dr, dy - dr, x + dr, dy + dr),
        &martensite_core::shape::Shape::circle(Vec2::new(x as f32, dy as f32), dr as f32),
        ink,
    );
}

/// Paints a small status lamp chip — the status-colored disc plus the
/// per-status glyph — centered at `center` with device-px radius `r`.
/// Shared by the icon widgets (`AppGrid`, `Dock`, `Filmstrip`) whose
/// tiles carry a status, so a bare icon swatch is never the only
/// status channel.
pub(crate) fn paint_status_chip(cx: &mut PaintContext<'_>, center: Vec2, r: f32, status: Status) {
    let color = cx.color(status.token(), status.fallback());
    let kr = kurbo::Rect::new(
        f64::from(center.x - r),
        f64::from(center.y - r),
        f64::from(center.x + r),
        f64::from(center.y + r),
    );
    let shape = martensite_core::shape::Shape::circle(center, r);
    cx.list.push_fill_shape(kr, &shape, color);
    // Surface-colored hairline separates the chip from the icon it
    // straddles — the same edge the `Badge` overlay paints.
    cx.list.push_stroke_shape(
        kr,
        &shape,
        cx.pt(1.0),
        cx.color(TokenKey::SurfaceColor, [245, 245, 246, 255]),
    );
    let ink = cx.color(TokenKey::TextInverseColor, [255, 255, 255, 255]);
    paint_status_glyph(cx.list, center, r * 0.62, status, ink);
}

/// A status lamp — see the module docs.
///
/// ```
/// use martensite::widgets::status_dot::StatusDot;
///
/// let d = StatusDot::new("Link");
/// assert_eq!(d.text(), "Link");
/// ```
pub struct StatusDot {
    /// Optional label text.
    pub text: String,
    /// When `false` the lamp renders muted.
    pub enabled: bool,
    /// Halo ring for active-attention states.
    pub pulse: bool,
    /// Whether the per-status glyph mark (check/`!`/`×`/`i`/`–`)
    /// paints inside the dot — the redundant non-color channel
    /// pairing required so status is never conveyed by hue alone.
    /// Defaults to `true`.
    pub glyph: bool,
    status: Status,
    /// Whether the status data is pending (ADR-0040). A pending lamp
    /// renders the explicit *data unknown* treatment — a hollow ring
    /// with a `?` mark — never generic shimmer.
    loading: bool,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl StatusDot {
    /// Creates a lamp labeled `text` in the `Off` state.
    ///
    /// ```
    /// use martensite::widgets::status_dot::{Status, StatusDot};
    ///
    /// let d = StatusDot::new("Pump");
    /// assert_eq!(d.status_value(), Status::Off);
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            enabled: true,
            pulse: false,
            glyph: true,
            status: Status::Off,
            loading: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Sets the status.
    ///
    /// ```
    /// use martensite::widgets::status_dot::{Status, StatusDot};
    ///
    /// let d = StatusDot::new("L").status(Status::Ok);
    /// assert_eq!(d.status_value(), Status::Ok);
    /// ```
    pub fn status(mut self, status: Status) -> Self {
        self.status = status;
        self
    }

    /// Paints a halo ring (active-attention states).
    ///
    /// ```
    /// use martensite::widgets::status_dot::StatusDot;
    ///
    /// let d = StatusDot::new("L").pulse(true);
    /// assert!(d.pulse);
    /// ```
    pub fn pulse(mut self, pulse: bool) -> Self {
        self.pulse = pulse;
        self
    }

    /// Enables or suppresses the per-status glyph mark inside the
    /// dot. The mark is the lamp's non-color status channel (a check
    /// for `Ok`, `!` for `Warning`, `×` for `Error`, `i` for `Info`,
    /// `–` for `Off`) — leave it on unless an adjacent label already
    /// spells the status out.
    ///
    /// ```
    /// use martensite::widgets::status_dot::StatusDot;
    ///
    /// let d = StatusDot::new("L").glyph(false);
    /// assert!(!d.glyph);
    /// ```
    pub fn glyph(mut self, glyph: bool) -> Self {
        self.glyph = glyph;
        self
    }

    /// Enables or disables (mutes) the lamp.
    ///
    /// ```
    /// use martensite::widgets::status_dot::StatusDot;
    ///
    /// let d = StatusDot::new("L").enabled(false);
    /// assert!(!d.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets whether the status data is pending (builder version).
    ///
    /// A pending lamp paints the explicit *data unknown* treatment —
    /// a hollow ring with a `?` mark — and reports `unknown` to
    /// assistive tech; it never shimmers like a content placeholder.
    ///
    /// ```
    /// use martensite::widgets::status_dot::StatusDot;
    ///
    /// let d = StatusDot::new("L").loading(true);
    /// assert!(d.is_loading());
    /// ```
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// Sets whether the status data is pending (mutable version) —
    /// the `Bound::push` seam for a stale telemetry feed.
    ///
    /// ```
    /// use martensite::widgets::status_dot::StatusDot;
    ///
    /// let mut d = StatusDot::new("L");
    /// d.set_loading(true);
    /// assert!(d.is_loading());
    /// d.set_loading(false);
    /// assert!(!d.is_loading());
    /// ```
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    /// Whether the status data is pending.
    ///
    /// ```
    /// use martensite::widgets::status_dot::StatusDot;
    ///
    /// assert!(!StatusDot::new("L").is_loading());
    /// ```
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::status_dot::StatusDot;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let d = StatusDot::new("L").with_text_painter(shared_painter());
    /// assert_eq!(d.text(), "L");
    /// ```
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Label text.
    ///
    /// ```
    /// use martensite::widgets::status_dot::StatusDot;
    ///
    /// assert_eq!(StatusDot::new("X").text(), "X");
    /// ```
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Current status.
    ///
    /// ```
    /// use martensite::widgets::status_dot::{Status, StatusDot};
    ///
    /// assert_eq!(StatusDot::new("X").status_value(), Status::Off);
    /// ```
    pub fn status_value(&self) -> Status {
        self.status
    }

    /// Updates the status (post-construction flips).
    ///
    /// ```
    /// use martensite::widgets::status_dot::{Status, StatusDot};
    ///
    /// let mut d = StatusDot::new("X");
    /// d.set_status(Status::Error);
    /// assert_eq!(d.status_value(), Status::Error);
    /// ```
    pub fn set_status(&mut self, status: Status) {
        self.status = status;
    }

    /// Base color for the status.
    fn base_color(&self) -> [u8; 4] {
        self.status.fallback()
    }

    /// Paints the label text right of the dot — shared by `paint` and
    /// `paint_loading` so the lamp's name stays on screen while its
    /// value is unknown.
    fn paint_label(&self, cx: &mut PaintContext) {
        if self.text.is_empty() {
            return;
        }
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size = FONT_PT * cx.scale;
        let d = cx.pt(DOT_PT);
        let slot = if self.pulse { d * 1.8 } else { d };
        let cy = self.bounds.min_y() + self.bounds.height() / 2.0;
        let x = self.bounds.min_x() + slot + cx.pt(GAP_PT);
        let clip = Rect::new(
            x,
            self.bounds.min_y(),
            (self.bounds.max_x() - x).max(0.0),
            self.bounds.height(),
        );
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(clip.min_x()),
                f64::from(clip.min_y()),
                f64::from(clip.max_x()),
                f64::from(clip.max_y()),
            ),
            kurbo::Point::new(
                f64::from(x),
                crate::text_paint::centered_label_top(painter, cy, &self.text, size),
            ),
            &self.text,
            size,
            cx.color(TokenKey::TextColor, FG),
        );
    }
}

impl Widget for StatusDot {
    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    /// The status lamp is the alarm channel by construction — `Error`
    /// paints the reserved alarm hue deliberately. The `@alarm` name
    /// marker records that semantics for the design-lint lineage walk.
    fn debug_name(&self) -> &'static str {
        "StatusDot@alarm"
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let text_w = if self.text.is_empty() {
            0.0
        } else {
            crate::text_paint::estimate_text_width_px(&(self.text), FONT_PT, 0.55) * cx.scale
                + cx.pt(GAP_PT)
        };
        let slot = if self.pulse { DOT_PT * 1.8 } else { DOT_PT };
        Vec2::new(
            (cx.pt(slot) + text_w).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(10.0, 10.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Status);
        node.set_label(if self.text.is_empty() {
            "Status".to_string()
        } else {
            self.text.clone()
        });
        // A pending lamp reports "unknown" — never a stale status —
        // so the a11y channel matches the `?` mark on screen.
        node.set_value(if self.loading {
            "unknown"
        } else {
            self.status.label()
        });
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn is_loading(&self) -> bool {
        self.loading
    }

    fn paint_loading(&self, cx: &mut PaintContext, _phase: Option<f32>) {
        // "Data unknown", not shimmer: a pending status lamp keeps its
        // labelled frame but the disc becomes a hollow ring with a `?`
        // mark — distinct from every valid status (all of which fill
        // the disc) and from a content placeholder. `_phase` is
        // ignored deliberately; safety widgets never sweep.
        if self.bounds.width() < 1.0 || self.bounds.height() < 1.0 {
            return;
        }
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        let d = cx.pt(DOT_PT).min(self.bounds.height());
        let cy = self.bounds.min_y() + self.bounds.height() / 2.0;
        let cxdot = self.bounds.min_x() + d / 2.0;
        let center = Vec2::new(cxdot, cy);
        let r = d / 2.0;
        let dot_rect = kurbo::Rect::new(
            f64::from(cxdot - r),
            f64::from(cy - r),
            f64::from(cxdot + r),
            f64::from(cy + r),
        );
        cx.list.push_stroke_shape(
            dot_rect,
            &martensite_core::shape::Shape::circle(center, r),
            cx.pt(1.5),
            muted,
        );
        if self.glyph {
            paint_unknown_glyph(cx.list, center, r * 0.66, muted);
        }
        self.paint_label(cx);
    }

    fn paint(&self, cx: &mut PaintContext) {
        let color = cx.color(self.status.token(), self.base_color());
        let color = if self.enabled {
            color
        } else {
            cx.color(TokenKey::TextMutedColor, MUTED)
        };
        if self.bounds.width() < 1.0 || self.bounds.height() < 1.0 {
            // Collapsed allocation — nothing can render inside; the
            // halo or dot would paint past the zero-height edge.
            return;
        }
        // A squeezed allocation (a dense status strip is allowed to
        // give us less than DOT_PT) shrinks the disc to fit instead of
        // painting past the bounds — an out-of-bounds fill trips the
        // paint audit's overflow-clip lint.
        let d = cx.pt(DOT_PT).min(self.bounds.height());
        // The pulse halo (1.8× the dot radius) is part of the widget's
        // own footprint — the dot is inset so the halo stays inside
        // `bounds` instead of overflowing into the parent clip.
        let slot = (if self.pulse { d * 1.8 } else { d }).min(self.bounds.height());
        let cy = self.bounds.min_y() + self.bounds.height() / 2.0;
        let cxdot = self.bounds.min_x() + slot / 2.0;
        let center = Vec2::new(cxdot, cy);
        let r = d / 2.0;
        let dot_rect = kurbo::Rect::new(
            f64::from(cxdot - r),
            f64::from(cy - r),
            f64::from(cxdot + r),
            f64::from(cy + r),
        );
        if self.pulse {
            // `slot` may have been clamped by a short allocation —
            // the halo follows the fitted slot, never exceeds it.
            let halo_r = (slot / 2.0).max(r);
            let halo = [color[0], color[1], color[2], 70];
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(cxdot - halo_r),
                    f64::from(cy - halo_r),
                    f64::from(cxdot + halo_r),
                    f64::from(cy + halo_r),
                ),
                &martensite_core::shape::Shape::circle(center, halo_r),
                halo,
            );
        }
        cx.list.push_fill_shape(
            dot_rect,
            &martensite_core::shape::Shape::circle(center, r),
            color,
        );
        if self.glyph {
            // The redundant non-color channel: a per-status mark in
            // inverse ink inside the disc.
            let ink = cx.color(TokenKey::TextInverseColor, [255, 255, 255, 255]);
            paint_status_glyph(cx.list, center, r * 0.62, self.status, ink);
        }

        self.paint_label(cx);
    }
}

impl std::fmt::Debug for StatusDot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusDot")
            .field("text", &self.text)
            .field("status", &self.status)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    #[test]
    fn status_roundtrip() {
        let mut d = StatusDot::new("A").status(Status::Warning);
        assert_eq!(d.status_value(), Status::Warning);
        d.set_status(Status::Ok);
        assert_eq!(d.status_value(), Status::Ok);
    }

    #[test]
    fn a11y_value_is_status() {
        let d = StatusDot::new("Pump").status(Status::Error);
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        d.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Status);
        assert_eq!(node.value(), Some("error"));
        assert_eq!(node.label(), Some("Pump"));
    }

    #[test]
    fn empty_label_measures_dot_only() {
        let mut with = StatusDot::new("Long Label Text");
        let mut bare = StatusDot::new("");
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let cons = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(500.0, 50.0),
        };
        let w1 = with.measure(&mut cx, cons).x;
        let w2 = bare.measure(&mut cx, cons).x;
        assert!(w1 > w2);
        assert!(w2 <= 20.0);
    }

    #[test]
    fn colors_map() {
        assert_eq!(StatusDot::new("x").status(Status::Ok).base_color(), OK);
        assert_eq!(StatusDot::new("x").status(Status::Off).base_color(), OFF);
    }

    #[test]
    fn glyph_paints_status_mark() {
        use martensite_core::PaintList;
        let theme = martensite_theme::Theme::new("test");
        let paint = |d: &StatusDot| {
            let mut list = PaintList::new();
            let mut cx = PaintContext {
                list: &mut list,
                bounds: Rect::new(0.0, 0.0, 18.0, 18.0),
                theme: &theme,
                scale: 1.0,
                text_painter: None,
            };
            d.paint(&mut cx);
            cx.list.commands.len()
        };
        let mut on = StatusDot::new("").status(Status::Ok);
        let mut off = StatusDot::new("").status(Status::Ok).glyph(false);
        let mut hot = HotNode::default();
        let bounds = Rect::new(0.0, 0.0, 18.0, 18.0);
        for d in [&mut on, &mut off] {
            d.layout(
                &mut LayoutContext {
                    hot: &mut hot,
                    scale: 1.0,
                },
                bounds,
            );
        }
        // The glyph mark adds the check stroke on top of the disc fill.
        assert!(paint(&on) > paint(&off));
    }

    fn painted_loading(d: &StatusDot, phase: Option<f32>) -> martensite_core::PaintList {
        let theme = martensite_theme::Theme::new("test");
        let mut list = martensite_core::PaintList::new();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: Rect::new(0.0, 0.0, 80.0, 18.0),
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        };
        d.paint_loading(&mut cx, phase);
        list
    }

    fn laid_out_dot(d: &mut StatusDot) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        d.layout(&mut cx, Rect::new(0.0, 0.0, 80.0, 18.0));
    }

    #[test]
    fn loading_flag_round_trip() {
        let mut d = StatusDot::new("Pump").status(Status::Ok);
        assert!(!d.is_loading());
        assert!(!<StatusDot as Widget>::is_loading(&d));
        d.set_loading(true);
        assert!(d.is_loading());
        assert!(<StatusDot as Widget>::is_loading(&d));
        d.set_loading(false);
        assert!(!d.is_loading());
    }

    #[test]
    fn loading_a11y_reports_unknown_not_stale_status() {
        let d = StatusDot::new("Pump").status(Status::Error).loading(true);
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        d.accessibility(&mut node);
        // "unknown" — never the stale "error" the data had.
        assert_eq!(node.value(), Some("unknown"));
    }

    #[test]
    fn loading_paint_is_unknown_not_shimmer() {
        use martensite_core::PaintCommand;
        let mut d = StatusDot::new("Pump").status(Status::Error).loading(true);
        laid_out_dot(&mut d);
        // Even with an animated phase a safety lamp must never emit a
        // shimmer band.
        let list = painted_loading(&d, Some(0.5));
        assert!(!list.commands.iter().any(|c| matches!(
            c,
            PaintCommand::FillLinearGradient(..) | PaintCommand::FillLinearGradientPath(..)
        )));
        // The disc must not borrow the stale status hue — no filled
        // shape in `Error` red anywhere.
        assert!(!list.commands.iter().any(|c| matches!(
            c,
            PaintCommand::FillRect(_, ERR) | PaintCommand::FillPath(_, ERR)
        )));
        // The label still paints — the lamp's name is known even when
        // its value isn't.
        assert!(list
            .commands
            .iter()
            .any(|c| matches!(c, PaintCommand::DrawText(_, text, _, _) if text == "Pump")));
    }

    #[test]
    fn loading_paint_without_glyph_is_ring_only() {
        use martensite_core::PaintCommand;
        // `glyph(false)` drops the `?` mark too — the hollow ring is
        // the minimum unknown treatment.
        let mut d = StatusDot::new("")
            .status(Status::Ok)
            .glyph(false)
            .loading(true);
        laid_out_dot(&mut d);
        let list = painted_loading(&d, None);
        assert!(list
            .commands
            .iter()
            .any(|c| matches!(c, PaintCommand::StrokePath(..))));
        assert!(!list
            .commands
            .iter()
            .any(|c| matches!(c, PaintCommand::FillLinearGradient(..))));
    }
}
