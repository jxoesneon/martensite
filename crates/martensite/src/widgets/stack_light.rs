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
}

impl Widget for StackLight {
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
        }
    }
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
}
