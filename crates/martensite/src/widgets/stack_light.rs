//! `StackLight` — an industrial andon signal tower: a vertical
//! stack of independently lit colored lamps (the Banner/Patlite
//! tower-light idiom on every machine panel).
//!
//! Unlike [`crate::widgets::status_dot::StatusDot`], which shows one
//! status, a stack light shows a *combination* — e.g. amber
//! flashing over green. Clicking a lamp toggles it and parks the
//! index in [`StackLight::take_changed`]; flashing lamps blink on
//! [`Widget::tick`](martensite_core::widget::Widget::tick).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::stack_light::{Lamp, StackLight};
//!
//! let mut t = StackLight::new()
//!     .lamp(Lamp::new("Fault", [235, 87, 87, 255]))
//!     .lamp(Lamp::new("Run", [92, 200, 120, 255]).lit(true));
//! assert_eq!(t.lamp_count(), 2);
//! assert!(t.lit(1));
//! t.set(0, true);
//! assert!(t.lit(0));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use parking_lot::Mutex;

const W_PT: f32 = 56.0;
const LAMP_PT: f32 = 36.0;
const GAP_PT: f32 = 4.0;
const PAD_PT: f32 = 6.0;

const HOUSING: [u8; 4] = [30, 30, 34, 255];
const UNLIT: [u8; 4] = [48, 48, 54, 255];

/// One lamp in a [`StackLight`] tower.
///
/// ```
/// use martensite::widgets::stack_light::Lamp;
///
/// let l = Lamp::new("Run", [92, 200, 120, 255]);
/// assert_eq!(l.label, "Run");
/// ```
#[derive(Debug, Clone)]
pub struct Lamp {
    /// Accessibility label for the lamp.
    pub label: String,
    /// Lit color (RGBA).
    pub color: [u8; 4],
    /// Whether the lamp is on.
    pub lit: bool,
    /// Whether the lamp blinks when lit.
    pub flashing: bool,
}

impl Lamp {
    /// An unlit lamp.
    ///
    /// ```
    /// use martensite::widgets::stack_light::Lamp;
    ///
    /// assert!(!Lamp::new("x", [255, 0, 0, 255]).lit);
    /// ```
    pub fn new(label: impl Into<String>, color: [u8; 4]) -> Self {
        Self {
            label: label.into(),
            color,
            lit: false,
            flashing: false,
        }
    }

    /// Starts lit.
    ///
    /// ```
    /// use martensite::widgets::stack_light::Lamp;
    ///
    /// assert!(Lamp::new("x", [0, 255, 0, 255]).lit(true).lit);
    /// ```
    pub fn lit(mut self, lit: bool) -> Self {
        self.lit = lit;
        self
    }

    /// Blinks when lit (~1.4 Hz on `tick`).
    ///
    /// ```
    /// use martensite::widgets::stack_light::Lamp;
    ///
    /// assert!(Lamp::new("x", [255, 0, 0, 255]).flashing(true).flashing);
    /// ```
    pub fn flashing(mut self, flashing: bool) -> Self {
        self.flashing = flashing;
        self
    }
}

/// An andon signal tower — see the module docs.
///
/// ```
/// use martensite::widgets::stack_light::StackLight;
///
/// assert_eq!(StackLight::new().lamp_count(), 0);
/// ```
pub struct StackLight {
    /// Accessibility label.
    pub label: String,
    /// Lamps, top to bottom.
    lamps: Vec<Lamp>,
    changed: Option<usize>,
    /// Whether the lamp data is pending (ADR-0040). A pending tower
    /// renders the explicit *data unknown* treatment — muted hollow
    /// lenses with `?` marks — never generic shimmer and never a
    /// stale lit lamp.
    loading: bool,
    /// Blink phase for flashing lamps.
    blink: f32,
    bounds: Rect,
    scale: f32,
    /// Per-lamp rects painted last frame.
    hits: Mutex<Vec<Rect>>,
}

impl std::fmt::Debug for StackLight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StackLight")
            .field("lamps", &self.lamps.len())
            .finish()
    }
}

impl Default for StackLight {
    fn default() -> Self {
        Self::new()
    }
}

impl StackLight {
    /// Creates an empty tower.
    ///
    /// ```
    /// use martensite::widgets::stack_light::StackLight;
    ///
    /// assert_eq!(StackLight::new().lamp_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Signal tower".to_string(),
            lamps: Vec::new(),
            changed: None,
            loading: false,
            blink: 0.0,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            hits: Mutex::new(Vec::new()),
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::stack_light::StackLight;
    ///
    /// assert_eq!(StackLight::new().label("Cell 3").label, "Cell 3");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Appends a lamp (top to bottom).
    ///
    /// ```
    /// use martensite::widgets::stack_light::{Lamp, StackLight};
    ///
    /// let t = StackLight::new().lamp(Lamp::new("a", [255, 0, 0, 255]));
    /// assert_eq!(t.lamp_count(), 1);
    /// ```
    pub fn lamp(mut self, lamp: Lamp) -> Self {
        self.lamps.push(lamp);
        self
    }

    /// Lamp count.
    ///
    /// ```
    /// use martensite::widgets::stack_light::StackLight;
    ///
    /// assert_eq!(StackLight::new().lamp_count(), 0);
    /// ```
    pub fn lamp_count(&self) -> usize {
        self.lamps.len()
    }

    /// Lamp at `i`.
    ///
    /// ```
    /// use martensite::widgets::stack_light::{Lamp, StackLight};
    ///
    /// let t = StackLight::new().lamp(Lamp::new("Warn", [245, 176, 66, 255]));
    /// assert_eq!(t.lamp_at(0).unwrap().label, "Warn");
    /// ```
    pub fn lamp_at(&self, i: usize) -> Option<&Lamp> {
        self.lamps.get(i)
    }

    /// Whether lamp `i` is lit.
    ///
    /// ```
    /// use martensite::widgets::stack_light::{Lamp, StackLight};
    ///
    /// let t = StackLight::new().lamp(Lamp::new("x", [0, 0, 255, 255]));
    /// assert!(!t.lit(0));
    /// ```
    pub fn lit(&self, i: usize) -> bool {
        self.lamps.get(i).is_some_and(|l| l.lit)
    }

    /// Sets lamp `i` lit/unlit.
    ///
    /// ```
    /// use martensite::widgets::stack_light::{Lamp, StackLight};
    ///
    /// let mut t = StackLight::new().lamp(Lamp::new("x", [0, 0, 255, 255]));
    /// t.set(0, true);
    /// assert!(t.lit(0));
    /// ```
    pub fn set(&mut self, i: usize, lit: bool) {
        if let Some(l) = self.lamps.get_mut(i) {
            l.lit = lit;
        }
    }

    /// Toggles lamp `i`.
    ///
    /// ```
    /// use martensite::widgets::stack_light::{Lamp, StackLight};
    ///
    /// let mut t = StackLight::new().lamp(Lamp::new("x", [0, 0, 255, 255]));
    /// t.toggle(0);
    /// assert!(t.lit(0));
    /// ```
    pub fn toggle(&mut self, i: usize) {
        if let Some(l) = self.lamps.get_mut(i) {
            l.lit = !l.lit;
        }
    }

    /// Drains the index of the last lamp toggled by a click.
    ///
    /// ```
    /// use martensite::widgets::stack_light::StackLight;
    ///
    /// assert_eq!(StackLight::new().take_changed(), None);
    /// ```
    pub fn take_changed(&mut self) -> Option<usize> {
        self.changed.take()
    }

    /// Sets whether the lamp data is pending (builder version).
    ///
    /// A pending tower paints the explicit *data unknown* treatment —
    /// every lens hollow with a `?` mark — and reports `status
    /// unknown` to assistive tech; it never shows stale lamps and
    /// never shimmers.
    ///
    /// ```
    /// use martensite::widgets::stack_light::StackLight;
    ///
    /// let t = StackLight::new().loading(true);
    /// assert!(t.is_loading());
    /// ```
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// Sets whether the lamp data is pending (mutable version) — the
    /// `Bound::push` seam for a stale telemetry feed.
    ///
    /// ```
    /// use martensite::widgets::stack_light::StackLight;
    ///
    /// let mut t = StackLight::new();
    /// t.set_loading(true);
    /// assert!(t.is_loading());
    /// t.set_loading(false);
    /// assert!(!t.is_loading());
    /// ```
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    /// Whether the lamp data is pending.
    ///
    /// ```
    /// use martensite::widgets::stack_light::StackLight;
    ///
    /// assert!(!StackLight::new().is_loading());
    /// ```
    pub fn is_loading(&self) -> bool {
        self.loading
    }
}

impl Widget for StackLight {
    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    /// Andon tower — the red segment is the alarm channel by
    /// construction, declared via the `@alarm` marker for the lint
    /// lineage.
    fn debug_name(&self) -> &'static str {
        "StackLight@alarm"
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = cx.pt(self.lamps.len() as f32 * (LAMP_PT + GAP_PT) + PAD_PT * 2.0);
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(28.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        if self.loading {
            // A pending tower reports "unknown" — never a stale lamp
            // combination — matching the `?` marks on screen.
            node.set_label(format!("{} — status unknown", self.label));
            return;
        }
        let lit: Vec<&str> = self
            .lamps
            .iter()
            .filter(|l| l.lit)
            .map(|l| l.label.as_str())
            .collect();
        let desc = if lit.is_empty() {
            "all lamps off".to_string()
        } else {
            format!("lit: {}", lit.join(", "))
        };
        node.set_label(format!("{} — {desc}", self.label));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Pending lenses are placeholders — swallow presses so a stale
        // `hits` rect can't toggle a lamp whose state is unknown.
        if self.loading {
            return EventResponse::Handled;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                let hits = self.hits.lock();
                if let Some(i) = hits.iter().position(|r| r.contains(*position)) {
                    drop(hits);
                    self.toggle(i);
                    self.changed = Some(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, dt: std::time::Duration) -> bool {
        if !self.lamps.iter().any(|l| l.lit && l.flashing) {
            return false;
        }
        self.blink = (self.blink + dt.as_secs_f32() * 1.4) % 1.0;
        true
    }

    fn is_loading(&self) -> bool {
        self.loading
    }

    fn paint_loading(&self, cx: &mut PaintContext, _phase: Option<f32>) {
        // "Data unknown", not shimmer: the tower keeps its housing and
        // lamp slots, but every lens is a muted hollow ring with a `?`
        // mark — a pending andon must never read as a live color or a
        // content placeholder. `_phase` is ignored deliberately;
        // safety widgets never sweep.
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
            &martensite_core::shape::Shape::rounded(8.0 * self.scale),
            cx.color(TokenKey::SurfaceColor, HOUSING),
        );
        let s = self.scale;
        let pad = PAD_PT * s;
        let lamp_h = LAMP_PT * s;
        let gap = GAP_PT * s;
        let muted = cx.color(TokenKey::TextMutedColor, [120, 126, 140, 255]);
        let mut hits = self.hits.lock();
        hits.clear();
        for (i, l) in self.lamps.iter().enumerate() {
            let y = self.bounds.min_y() + pad + i as f32 * (lamp_h + gap);
            let r = Rect::new(
                self.bounds.min_x() + pad,
                y,
                self.bounds.width() - pad * 2.0,
                lamp_h,
            );
            hits.push(r);
            // Lens base — the same slot outline as `paint`.
            cx.list.push_fill_shape(
                krect(r),
                &martensite_core::shape::Shape::rounded(6.0 * s),
                cx.color(TokenKey::BackgroundColor, UNLIT),
            );
            // The etched label still rides the row in muted ink — the
            // slot's *name* is known even when its state isn't.
            if !l.label.is_empty() {
                let size_px = cx.pt(12.0);
                let kr = krect(r);
                crate::text_paint::paint_label_vcenter(
                    cx.text_painter,
                    cx.list,
                    kurbo::Rect::new(kr.x0, kr.y0, kr.x1, kr.y0 + kr.height()),
                    kr.x0 + 6.0 * f64::from(s),
                    &l.label,
                    size_px,
                    muted,
                );
            }
            // The `?` mark on the lens face — the redundant non-color
            // unknown channel (same glyph family as `StatusDot`).
            let gr = lamp_h * 0.22;
            let center = Vec2::new(r.max_x() - pad - gr, y + lamp_h / 2.0);
            crate::widgets::status_dot::paint_unknown_glyph(cx.list, center, gr, muted);
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
            &martensite_core::shape::Shape::rounded(8.0 * self.scale),
            cx.color(TokenKey::SurfaceColor, HOUSING),
        );
        let s = self.scale;
        let pad = PAD_PT * s;
        let lamp_h = LAMP_PT * s;
        let gap = GAP_PT * s;
        let blink_on = self.blink < 0.55;
        let mut hits = self.hits.lock();
        hits.clear();
        for (i, l) in self.lamps.iter().enumerate() {
            let y = self.bounds.min_y() + pad + i as f32 * (lamp_h + gap);
            let r = Rect::new(
                self.bounds.min_x() + pad,
                y,
                self.bounds.width() - pad * 2.0,
                lamp_h,
            );
            hits.push(r);
            // Lens base.
            cx.list.push_fill_shape(
                krect(r),
                &martensite_core::shape::Shape::rounded(6.0 * s),
                cx.color(TokenKey::BackgroundColor, UNLIT),
            );
            let visible = l.lit && (!l.flashing || blink_on);
            if visible {
                let lens = Rect::new(
                    r.min_x() + 3.0 * s,
                    y + 3.0 * s,
                    r.width() - 6.0 * s,
                    lamp_h - 6.0 * s,
                );
                cx.list.push_fill_shape(
                    krect(lens),
                    &martensite_core::shape::Shape::rounded(4.0 * s),
                    l.color,
                );
            }
            // Redundant encoding (WCAG 1.4.1 / ISA-101): the lamp's
            // etched label rides the row so state never depends on hue
            // alone — dark ink on a lit lens, muted ink on the unlit
            // base.
            if !l.label.is_empty() {
                let ink = if visible && luminance(l.color) > 0.45 {
                    [18, 20, 26, 255]
                } else if visible {
                    [242, 245, 250, 255]
                } else {
                    cx.color(TokenKey::TextMutedColor, [120, 126, 140, 255])
                };
                let size_px = cx.pt(12.0);
                let kr = krect(r);
                crate::text_paint::paint_label_vcenter(
                    cx.text_painter,
                    cx.list,
                    kurbo::Rect::new(kr.x0, kr.y0, kr.x1, kr.y0 + kr.height()),
                    kr.x0 + 6.0 * f64::from(s),
                    &l.label,
                    size_px,
                    ink,
                );
            }
        }
    }
}

/// Rec. 601 luma of a lamp's lit color, 0.0–1.0 — picks the ink that
/// survives the lens fill.
fn luminance(c: [u8; 4]) -> f64 {
    (0.299 * f64::from(c[0]) + 0.587 * f64::from(c[1]) + 0.114 * f64::from(c[2])) / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;
    use martensite_core::PaintList;

    fn laid_out(w: &mut StackLight, wd: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(wd, h),
            },
        );
        w.layout(&mut cx, Rect::new(0.0, 0.0, wd, h));
    }

    fn painted(w: &StackLight) {
        let theme = martensite_theme::Theme::new("test");
        let mut list = PaintList::new();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: w.bounds,
            scale: 1.0,
            theme: &theme,
            text_painter: None,
        };
        w.paint(&mut cx);
    }

    #[test]
    fn lamps_toggle() {
        let mut t = StackLight::new()
            .lamp(Lamp::new("r", [255, 0, 0, 255]))
            .lamp(Lamp::new("g", [0, 255, 0, 255]).lit(true));
        assert!(!t.lit(0));
        assert!(t.lit(1));
        t.toggle(0);
        t.set(1, false);
        assert!(t.lit(0));
        assert!(!t.lit(1));
    }

    #[test]
    fn click_toggles_and_parks() {
        let mut t = StackLight::new()
            .lamp(Lamp::new("r", [255, 0, 0, 255]))
            .lamp(Lamp::new("g", [0, 255, 0, 255]));
        laid_out(&mut t, 56.0, 100.0);
        painted(&t);
        let second = t.hits.lock()[1];
        t.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(
                    (second.min_x() + second.max_x()) / 2.0,
                    (second.min_y() + second.max_y()) / 2.0,
                ),
                count: 1,
            },
            bounds: t.bounds,
            scale: 1.0,
        });
        assert!(t.lit(1));
        assert_eq!(t.take_changed(), Some(1));
        assert_eq!(t.take_changed(), None);
    }

    #[test]
    fn flashing_ticks_only_when_lit() {
        let mut t = StackLight::new().lamp(Lamp::new("r", [255, 0, 0, 255]).flashing(true));
        assert!(!<StackLight as Widget>::tick(
            &mut t,
            std::time::Duration::from_millis(50)
        ));
        t.set(0, true);
        assert!(<StackLight as Widget>::tick(
            &mut t,
            std::time::Duration::from_millis(50)
        ));
        assert!(t.blink > 0.0);
    }

    fn painted_loading(w: &StackLight, phase: Option<f32>) -> PaintList {
        let theme = martensite_theme::Theme::new("test");
        let mut list = PaintList::new();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: w.bounds,
            scale: 1.0,
            theme: &theme,
            text_painter: None,
        };
        w.paint_loading(&mut cx, phase);
        list
    }

    #[test]
    fn loading_flag_round_trip() {
        let mut t = StackLight::new().lamp(Lamp::new("r", [255, 0, 0, 255]));
        assert!(!t.is_loading());
        assert!(!<StackLight as Widget>::is_loading(&t));
        t.set_loading(true);
        assert!(t.is_loading());
        assert!(<StackLight as Widget>::is_loading(&t));
        t.set_loading(false);
        assert!(!t.is_loading());
    }

    #[test]
    fn loading_a11y_reports_unknown_not_stale_lamps() {
        let t = StackLight::new()
            .label("Cell 3")
            .lamp(Lamp::new("Run", [92, 200, 120, 255]).lit(true))
            .loading(true);
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        t.accessibility(&mut node);
        // "status unknown" — never the stale "lit: Run" reading.
        assert_eq!(node.label(), Some("Cell 3 — status unknown"));
    }

    #[test]
    fn loading_paint_is_unknown_not_shimmer() {
        use martensite_core::PaintCommand;
        let mut t = StackLight::new()
            .lamp(Lamp::new("Fault", [235, 87, 87, 255]).lit(true))
            .lamp(
                Lamp::new("Run", [92, 200, 120, 255])
                    .lit(true)
                    .flashing(true),
            )
            .loading(true);
        laid_out(&mut t, 56.0, 100.0);
        // Even with an animated phase a safety tower must never emit
        // a shimmer band.
        let list = painted_loading(&t, Some(0.5));
        assert!(!list.commands.iter().any(|c| matches!(
            c,
            PaintCommand::FillLinearGradient(..) | PaintCommand::FillLinearGradientPath(..)
        )));
        // No lamp may paint its lit color — a pending tower never
        // reads as a live fault/run combination.
        assert!(!list.commands.iter().any(|c| matches!(
            c,
            PaintCommand::FillRect(_, [235, 87, 87, 255])
                | PaintCommand::FillPath(_, [235, 87, 87, 255])
                | PaintCommand::FillRect(_, [92, 200, 120, 255])
                | PaintCommand::FillPath(_, [92, 200, 120, 255])
        )));
        // The etched lamp labels still paint — slot names are known.
        let labels = list
            .commands
            .iter()
            .filter(|c| matches!(c, PaintCommand::DrawText(..)))
            .count();
        assert_eq!(labels, 2);
        // And each lens carries the `?` stroke — two unknown marks.
        let marks = list
            .commands
            .iter()
            .filter(|c| matches!(c, PaintCommand::StrokePath(..)))
            .count();
        assert_eq!(marks, 2);
    }

    #[test]
    fn loading_swallows_lamp_presses() {
        let mut t = StackLight::new()
            .lamp(Lamp::new("r", [255, 0, 0, 255]))
            .loading(true);
        laid_out(&mut t, 56.0, 100.0);
        painted_loading(&t, None);
        let first = t.hits.lock()[0];
        let press = WidgetEvent::PointerPressed {
            button: PointerButton::Primary,
            position: Vec2::new(
                (first.min_x() + first.max_x()) / 2.0,
                (first.min_y() + first.max_y()) / 2.0,
            ),
            count: 1,
        };
        assert_eq!(
            t.event(&mut EventContext {
                event: &press,
                bounds: t.bounds,
                scale: 1.0,
            }),
            EventResponse::Handled
        );
        assert!(!t.lit(0));
        assert_eq!(t.take_changed(), None);
    }
}
