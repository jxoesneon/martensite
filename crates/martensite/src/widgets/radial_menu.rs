//! `RadialMenu` — a pie-menu selector (items arranged as equal
//! sectors around a dead-zone center — the game-ui / marking-menu
//! idiom, `QMenu`-style radial pick).
//!
//! Sectors divide the ring evenly starting at 12 o'clock. Hovering
//! a sector highlights it; clicking parks its index in
//! [`RadialMenu::take_selected`]. `Up`/`Right` advance the
//! highlight, `Down`/`Left` retreat, `Enter`/`Space` confirm.
//! [`RadialMenu::labels`] pairs glyphs with accessibility names.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::radial_menu::RadialMenu;
//!
//! let r = RadialMenu::new().items(["Cut", "Copy", "Paste"]);
//! assert_eq!(r.item_count(), 3);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use std::f32::consts::TAU;

const SIZE_PT: f32 = 160.0;
/// Center dead zone as a fraction of the outer radius.
const DEAD: f32 = 0.28;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const SECTOR: [u8; 4] = [52, 52, 60, 255];
const HOVER: [u8; 4] = [80, 130, 200, 255];
const GLYPH: [u8; 4] = [220, 220, 228, 255];

/// A radial pie-menu selector — see the module docs.
///
/// ```
/// use martensite::widgets::radial_menu::RadialMenu;
///
/// assert_eq!(RadialMenu::new().item_count(), 0);
/// ```
#[derive(Debug)]
pub struct RadialMenu {
    /// Accessibility label.
    pub label: String,
    items: Vec<String>,
    hovered: Option<usize>,
    pressed: Option<usize>,
    pending: Option<usize>,
    bounds: Rect,
    scale: f32,
}

impl Default for RadialMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl RadialMenu {
    /// Creates an empty menu.
    ///
    /// ```
    /// use martensite::widgets::radial_menu::RadialMenu;
    ///
    /// assert_eq!(RadialMenu::new().item_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Radial menu".to_string(),
            items: Vec::new(),
            hovered: None,
            pressed: None,
            pending: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets the sector labels.
    ///
    /// ```
    /// use martensite::widgets::radial_menu::RadialMenu;
    ///
    /// let r = RadialMenu::new().items(["a", "b", "c", "d"]);
    /// assert_eq!(r.item_count(), 4);
    /// ```
    pub fn items(mut self, items: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.items = items.into_iter().map(Into::into).collect();
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::radial_menu::RadialMenu;
    ///
    /// assert_eq!(RadialMenu::new().label("Tools").label, "Tools");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Sector count.
    ///
    /// ```
    /// use martensite::widgets::radial_menu::RadialMenu;
    ///
    /// assert_eq!(RadialMenu::new().items(["x"]).item_count(), 1);
    /// ```
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// A sector's label.
    ///
    /// ```
    /// use martensite::widgets::radial_menu::RadialMenu;
    ///
    /// assert_eq!(RadialMenu::new().items(["x", "y"]).item(1), "y");
    /// ```
    pub fn item(&self, index: usize) -> &str {
        self.items.get(index).map(String::as_str).unwrap_or("")
    }

    /// Currently highlighted sector, if any.
    ///
    /// ```
    /// use martensite::widgets::radial_menu::RadialMenu;
    ///
    /// assert_eq!(RadialMenu::new().highlighted(), None);
    /// ```
    pub fn highlighted(&self) -> Option<usize> {
        self.hovered
    }

    /// Drains the last clicked/confirmed sector index.
    ///
    /// ```
    /// use martensite::widgets::radial_menu::RadialMenu;
    ///
    /// assert_eq!(RadialMenu::new().take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// Center of the ring in widget coordinates.
    fn center(&self) -> Vec2 {
        Vec2::new(
            (self.bounds.min_x() + self.bounds.max_x()) / 2.0,
            (self.bounds.min_y() + self.bounds.max_y()) / 2.0,
        )
    }

    /// Outer radius (ring is inscribed in the bounds).
    fn radius(&self) -> f32 {
        self.bounds.width().min(self.bounds.height()) / 2.0
    }

    /// Sector index under a point, honoring the dead zone.
    fn sector_at(&self, p: Vec2) -> Option<usize> {
        let n = self.items.len();
        if n == 0 {
            return None;
        }
        let d = p - self.center();
        let r = self.radius();
        let dist = d.length();
        if dist > r || dist < r * DEAD {
            return None;
        }
        // Angle from 12 o'clock, clockwise, in turns.
        // atan2: right = 0, down = 0.25, up = -0.25 → +0.25 shifts up to 0.
        let mut t = d.y.atan2(d.x) / TAU + 0.25;
        t -= t.floor();
        let i = (t * n as f32) as usize;
        Some(i.min(n - 1))
    }

    /// Annulus-sector path for sector `i`.
    fn sector_path(&self, i: usize) -> kurbo::BezPath {
        let n = self.items.len() as f32;
        let c = self.center();
        let r = self.radius();
        let ri = r * DEAD;
        let a0 = (i as f32 / n - 0.25) * TAU; // -90° → 12 o'clock
        let a1 = ((i + 1) as f32 / n - 0.25) * TAU;
        let mut path = kurbo::BezPath::new();
        let p = |a: f32, rad: f32| {
            (
                f64::from(c.x + a.cos() * rad),
                f64::from(c.y + a.sin() * rad),
            )
        };
        path.move_to(p(a0, ri));
        path.line_to(p(a0, r));
        let steps = 8;
        for k in 1..=steps {
            let a = a0 + (a1 - a0) * k as f32 / steps as f32;
            path.line_to(p(a, r));
        }
        path.line_to(p(a1, ri));
        for k in (0..steps).rev() {
            let a = a0 + (a1 - a0) * k as f32 / steps as f32;
            path.line_to(p(a, ri));
        }
        path.close_path();
        path
    }
}

impl Widget for RadialMenu {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.pt(SIZE_PT);
        Vec2::new(
            s.min(constraints.max_size.x.max(0.0)),
            s.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Menu);
        node.set_label(format!("{} — {} items", self.label, self.items.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.sector_at(*position);
                if hit != self.hovered {
                    self.hovered = hit;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                self.pressed = self.sector_at(*position);
                if self.pressed.is_some() {
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(i) = self.pressed.take() {
                    if self.sector_at(*position) == Some(i) {
                        self.pending = Some(i);
                        return EventResponse::Handled;
                    }
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => {
                let n = self.items.len();
                if n == 0 {
                    return EventResponse::Ignored;
                }
                match key.as_str() {
                    "ArrowRight" | "ArrowUp" => {
                        self.hovered = Some(self.hovered.map_or(0, |i| (i + 1) % n));
                        EventResponse::RequestRepaint
                    }
                    "ArrowLeft" | "ArrowDown" => {
                        self.hovered = Some(self.hovered.map_or(0, |i| (i + n - 1) % n));
                        EventResponse::RequestRepaint
                    }
                    "Enter" | " " => {
                        if let Some(i) = self.hovered {
                            self.pending = Some(i);
                            return EventResponse::Handled;
                        }
                        EventResponse::Ignored
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
            &martensite_core::shape::Shape::RECT,
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let n = self.items.len();
        if n == 0 {
            return;
        }
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        let glyph = cx.color(TokenKey::TextColor, GLYPH);
        for i in 0..n {
            let path = self.sector_path(i);
            let fill = if self.hovered == Some(i) {
                cx.color(TokenKey::AccentColor, HOVER)
            } else {
                SECTOR
            };
            cx.list.push_path(path.clone(), fill);
            cx.list.push_stroke_path(path, cx.pt(0.75), edge);
            // Glyph stub at the sector's mid-angle (labels are
            // accessibility-facing; a short tick marks the sector).
            let a = ((i as f32 + 0.5) / n as f32 - 0.25) * TAU;
            let c = self.center();
            let r = self.radius();
            let mid = r * (DEAD + (1.0 - DEAD) * 0.55);
            let p0 = Vec2::new(c.x + a.cos() * (mid - cx.pt(4.0)), c.y + a.sin() * (mid - cx.pt(4.0)));
            let p1 = Vec2::new(c.x + a.cos() * mid, c.y + a.sin() * mid);
            let mut tick = kurbo::BezPath::new();
            tick.move_to((f64::from(p0.x), f64::from(p0.y)));
            tick.line_to((f64::from(p1.x), f64::from(p1.y)));
            cx.list.push_stroke_path(tick, cx.pt(2.0), glyph);
        }
        // Dead-zone hub.
        let c = self.center();
        let ri = self.radius() * DEAD;
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(c.x - ri),
                f64::from(c.y - ri),
                f64::from(c.x + ri),
                f64::from(c.y + ri),
            ),
            &martensite_core::shape::Shape::circle(c, ri),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        cx.list.push_stroke_shape(
            kurbo::Rect::new(
                f64::from(c.x - ri),
                f64::from(c.y - ri),
                f64::from(c.x + ri),
                f64::from(c.y + ri),
            ),
            &martensite_core::shape::Shape::circle(c, ri),
            cx.pt(0.75),
            edge,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(r: &mut RadialMenu, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        r.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        r.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn click_at(r: &mut RadialMenu, p: Vec2) {
        r.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: p,
                count: 1,
            },
            bounds: r.bounds,
            scale: 1.0,
        });
        r.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: p,
            },
            bounds: r.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn items_and_labels() {
        let r = RadialMenu::new().items(["Cut", "Copy", "Paste"]);
        assert_eq!(r.item_count(), 3);
        assert_eq!(r.item(1), "Copy");
    }

    #[test]
    fn sector_hit_testing() {
        let mut r = RadialMenu::new().items(["n", "e", "s", "w"]);
        laid_out(&mut r, 200.0, 200.0);
        // Center (100,100); sector 0 starts at 12 o'clock.
        assert_eq!(r.sector_at(Vec2::new(100.0, 30.0)), Some(0)); // top
        assert_eq!(r.sector_at(Vec2::new(170.0, 100.0)), Some(1)); // right
        assert_eq!(r.sector_at(Vec2::new(100.0, 170.0)), Some(2)); // bottom
        assert_eq!(r.sector_at(Vec2::new(30.0, 100.0)), Some(3)); // left
        // Dead zone and outside.
        assert_eq!(r.sector_at(Vec2::new(100.0, 100.0)), None);
        assert_eq!(r.sector_at(Vec2::new(195.0, 195.0)), None);
    }

    #[test]
    fn click_selects() {
        let mut r = RadialMenu::new().items(["a", "b", "c", "d"]);
        laid_out(&mut r, 200.0, 200.0);
        click_at(&mut r, Vec2::new(170.0, 100.0));
        assert_eq!(r.take_selected(), Some(1));
        assert_eq!(r.take_selected(), None); // drained
    }

    #[test]
    fn keyboard_cycles_and_confirms() {
        let mut r = RadialMenu::new().items(["a", "b", "c"]);
        laid_out(&mut r, 200.0, 200.0);
        let key = |r: &mut RadialMenu, k: &str| {
            r.event(&mut EventContext {
                event: &WidgetEvent::KeyPressed {
                    key: k.to_string(),
                    repeat: false,
                },
                bounds: r.bounds,
                scale: 1.0,
            });
        };
        key(&mut r, "ArrowRight");
        assert_eq!(r.highlighted(), Some(0));
        key(&mut r, "ArrowRight");
        assert_eq!(r.highlighted(), Some(1));
        key(&mut r, "ArrowLeft");
        assert_eq!(r.highlighted(), Some(0));
        key(&mut r, "ArrowDown"); // wraps back to 2
        assert_eq!(r.highlighted(), Some(2));
        key(&mut r, "Enter");
        assert_eq!(r.take_selected(), Some(2));
    }

    #[test]
    fn dead_zone_ignored() {
        let mut r = RadialMenu::new().items(["a", "b"]);
        laid_out(&mut r, 200.0, 200.0);
        click_at(&mut r, Vec2::new(100.0, 100.0));
        assert_eq!(r.take_selected(), None);
    }
}
