//! `Dock` — a macOS-style icon dock with proximity magnification and
//! running-indicator dots.
//!
//! Icons tile along the bottom of the widget's bounds. Moving the
//! pointer across the dock magnifies nearby icons toward
//! [`Dock::magnification`]; a dot under an icon marks it running.
//! Clicking parks the index in [`Dock::take_launched`]; `←`/`→` move
//! the hover and `Enter` launches.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::dock::{Dock, DockItem};
//!
//! let mut d = Dock::new()
//!     .item(DockItem::new("Mail", [80, 120, 220, 255]).running(true))
//!     .item(DockItem::new("Chat", [90, 190, 120, 255]));
//! assert_eq!(d.item_count(), 2);
//! assert!(d.item_at(0).unwrap().running);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, ImageData, LayoutConstraints, LayoutContext, PaintContext,
    PointerButton, Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const ICON_PT: f32 = 44.0;
const GAP_PT: f32 = 8.0;
const PAD_PT: f32 = 10.0;
const DOT_PT: f32 = 4.0;

const FACE: [u8; 4] = [30, 30, 34, 200];
const EDGE: [u8; 4] = [80, 80, 88, 160];
const DOT: [u8; 4] = [200, 200, 206, 255];
const TEXT: [u8; 4] = [220, 220, 226, 255];
const TIP: [u8; 4] = [45, 45, 50, 245];

/// One dock icon — see [`Dock`].
///
/// ```
/// use martensite::widgets::dock::DockItem;
///
/// let i = DockItem::new("Finder", [90, 150, 230, 255]).running(true);
/// assert_eq!(i.label, "Finder");
/// assert!(i.running);
/// ```
#[derive(Debug, Clone)]
pub struct DockItem {
    /// Tooltip / accessibility name.
    pub label: String,
    /// Fallback tile color when `image` is `None`.
    pub color: [u8; 4],
    /// Whether the app is running (indicator dot).
    pub running: bool,
    /// Optional decoded icon image.
    pub image: Option<ImageData>,
    /// Optional status lamp painted as a glyph-marked chip on the
    /// icon's top-right corner — pair the state with a mark, never
    /// with the tile color alone.
    pub status: Option<crate::widgets::status_dot::Status>,
}

impl DockItem {
    /// A color-backed icon.
    ///
    /// ```
    /// use martensite::widgets::dock::DockItem;
    ///
    /// assert!(!DockItem::new("x", [0, 0, 0, 255]).running);
    /// ```
    pub fn new(label: impl Into<String>, color: [u8; 4]) -> Self {
        Self {
            label: label.into(),
            color,
            running: false,
            image: None,
            status: None,
        }
    }

    /// Marks the app as running (dot under the icon).
    ///
    /// ```
    /// use martensite::widgets::dock::DockItem;
    ///
    /// assert!(DockItem::new("x", [0, 0, 0, 255]).running(true).running);
    /// ```
    pub fn running(mut self, on: bool) -> Self {
        self.running = on;
        self
    }

    /// A decoded icon image.
    ///
    /// ```
    /// use martensite::widgets::dock::DockItem;
    /// use martensite_core::ImageData;
    ///
    /// let img = ImageData::from_rgba(1, 1, vec![0, 0, 0, 255]).unwrap();
    /// assert!(DockItem::new("x", [0, 0, 0, 255]).image(img).image.is_some());
    /// ```
    pub fn image(mut self, image: ImageData) -> Self {
        self.image = Some(image);
        self
    }

    /// Attaches a status lamp chip to the icon — the redundant
    /// color+mark channel for callers that used to encode status in
    /// the tile color alone.
    ///
    /// ```
    /// use martensite::widgets::dock::DockItem;
    /// use martensite::widgets::status_dot::Status;
    ///
    /// let i = DockItem::new("x", [0, 0, 0, 255]).status(Status::Error);
    /// assert_eq!(i.status, Some(Status::Error));
    /// ```
    pub fn status(mut self, status: crate::widgets::status_dot::Status) -> Self {
        self.status = Some(status);
        self
    }
}

/// A magnification icon dock — see the module docs.
///
/// ```
/// use martensite::widgets::dock::Dock;
///
/// assert_eq!(Dock::new().item_count(), 0);
/// ```
pub struct Dock {
    /// Accessibility label.
    pub label: String,
    items: Vec<DockItem>,
    /// Pointer position while over the dock (drives magnification).
    hover_x: Option<f32>,
    hovered: Option<usize>,
    launched: Option<usize>,
    /// Max icon scale at the pointer (`1.0` = no magnification).
    magnify: f32,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for Dock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dock")
            .field("items", &self.items.len())
            .field("hovered", &self.hovered)
            .finish()
    }
}

impl Default for Dock {
    fn default() -> Self {
        Self::new()
    }
}

impl Dock {
    /// Creates an empty dock.
    ///
    /// ```
    /// use martensite::widgets::dock::Dock;
    ///
    /// assert_eq!(Dock::new().item_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Dock".to_string(),
            items: Vec::new(),
            hover_x: None,
            hovered: None,
            launched: None,
            magnify: 1.6,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::dock::Dock;
    ///
    /// assert_eq!(Dock::new().label("Apps").label, "Apps");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::dock::Dock;
    ///
    /// let _ = Dock::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Appends an icon.
    ///
    /// ```
    /// use martensite::widgets::dock::{Dock, DockItem};
    ///
    /// assert_eq!(Dock::new().item(DockItem::new("a", [0, 0, 0, 255])).item_count(), 1);
    /// ```
    pub fn item(mut self, item: DockItem) -> Self {
        self.items.push(item);
        self
    }

    /// Peak icon scale under the pointer (`1.0` disables magnification).
    ///
    /// ```
    /// use martensite::widgets::dock::Dock;
    ///
    /// assert_eq!(Dock::new().magnification(2.0).magnification_value(), 2.0);
    /// ```
    pub fn magnification(mut self, m: f32) -> Self {
        self.magnify = m.clamp(1.0, 3.0);
        self
    }

    /// The configured magnification peak.
    ///
    /// ```
    /// use martensite::widgets::dock::Dock;
    ///
    /// assert_eq!(Dock::new().magnification_value(), 1.6);
    /// ```
    pub fn magnification_value(&self) -> f32 {
        self.magnify
    }

    /// Item count.
    ///
    /// ```
    /// use martensite::widgets::dock::Dock;
    ///
    /// assert_eq!(Dock::new().item_count(), 0);
    /// ```
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Item `i`, if in range.
    ///
    /// ```
    /// use martensite::widgets::dock::{Dock, DockItem};
    ///
    /// let d = Dock::new().item(DockItem::new("a", [0, 0, 0, 255]));
    /// assert_eq!(d.item_at(0).unwrap().label, "a");
    /// ```
    pub fn item_at(&self, i: usize) -> Option<&DockItem> {
        self.items.get(i)
    }

    /// Hovered icon index.
    ///
    /// ```
    /// use martensite::widgets::dock::Dock;
    ///
    /// assert_eq!(Dock::new().hovered(), None);
    /// ```
    pub fn hovered(&self) -> Option<usize> {
        self.hovered
    }

    /// Drains the index of the last clicked icon.
    ///
    /// ```
    /// use martensite::widgets::dock::Dock;
    ///
    /// assert_eq!(Dock::new().take_launched(), None);
    /// ```
    pub fn take_launched(&mut self) -> Option<usize> {
        self.launched.take()
    }

    /// Icon size at `i` given the current pointer position.
    fn icon_size(&self, i: usize) -> f32 {
        let base = ICON_PT * self.scale;
        let Some(hx) = self.hover_x else {
            return base;
        };
        let cx = self.icon_center_x(i);
        let sigma = base * 1.2;
        let d = (cx - hx).abs();
        let bump = (-d * d / (2.0 * sigma * sigma)).exp();
        base * (1.0 + (self.magnify - 1.0) * bump)
    }

    /// Horizontal center of icon `i`'s base (un-magnified) slot.
    fn icon_center_x(&self, i: usize) -> f32 {
        let s = self.scale;
        let slot = ICON_PT * s + GAP_PT * s;
        let strip_w = self.items.len() as f32 * slot - GAP_PT * s;
        let x0 = self.bounds.min_x() + (self.bounds.width() - strip_w) / 2.0;
        x0 + i as f32 * slot + ICON_PT * s / 2.0
    }

    /// Icon index under `p`, if any.
    fn icon_at(&self, p: Vec2) -> Option<usize> {
        for i in 0..self.items.len() {
            let sz = self.icon_size(i);
            let cx = self.icon_center_x(i);
            let bottom = self.bounds.max_y() - (PAD_PT + DOT_PT * 2.0) * self.scale;
            let r = Rect::new(cx - sz / 2.0, bottom - sz, sz, sz);
            if r.contains(p) {
                return Some(i);
            }
        }
        None
    }
}

impl Widget for Dock {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let n = self.items.len().max(1) as f32;
        let w = n * ICON_PT + (n - 1.0).max(0.0) * GAP_PT + PAD_PT * 2.0;
        let h = ICON_PT * self.magnify + PAD_PT * 2.0 + DOT_PT * 2.0;
        Vec2::new(
            cx.pt(w.max(120.0)).min(constraints.max_size.x.max(0.0)),
            cx.pt(h).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 56.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Toolbar);
        node.set_label(format!("{} — {} items", self.label, self.items.len()));
        // Status chips are paint-only — fold per-item state into the
        // description so AT parity doesn't depend on pixels.
        let statuses = self
            .items
            .iter()
            .filter_map(|i| i.status.map(|s| format!("{} {}", i.label, s.label())))
            .collect::<Vec<_>>();
        if !statuses.is_empty() {
            node.set_description(statuses.join(", "));
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                if self.bounds.contains(*position) {
                    self.hover_x = Some(position.x);
                    let h = self.icon_at(*position);
                    if h != self.hovered {
                        self.hovered = h;
                        return EventResponse::RequestRepaint;
                    }
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                let had_x = self.hover_x.take().is_some();
                let had_h = self.hovered.take().is_some();
                if had_x || had_h {
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self.icon_at(*position) {
                    self.launched = Some(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => {
                let n = self.items.len();
                match key.as_str() {
                    "ArrowLeft" | "ArrowRight" if n > 0 => {
                        let cur = self.hovered.unwrap_or(0);
                        let next = if key == "ArrowRight" {
                            (cur + 1).min(n - 1)
                        } else {
                            cur.saturating_sub(1)
                        };
                        self.hovered = Some(next);
                        EventResponse::RequestRepaint
                    }
                    "Enter" | " " => {
                        if let Some(i) = self.hovered {
                            self.launched = Some(i);
                            return EventResponse::RequestRepaint;
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
        let s = self.scale;
        // Frosted tray behind the icons.
        let tray = Rect::new(
            self.bounds.min_x() + PAD_PT * s,
            self.bounds.min_y() + PAD_PT * s * 0.5,
            (self.bounds.width() - 2.0 * PAD_PT * s).max(0.0),
            (self.bounds.height() - PAD_PT * s).max(0.0),
        );
        cx.list.push_fill_shape(
            krect(tray),
            &martensite_core::shape::Shape::rounded(12.0 * s),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        cx.list.push_stroke_shape(
            krect(tray),
            &martensite_core::shape::Shape::rounded(12.0 * s),
            1.0 * s,
            cx.color(TokenKey::BorderColor, EDGE),
        );

        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let bottom = self.bounds.max_y() - (PAD_PT + DOT_PT * 2.0) * s;
        for (i, item) in self.items.iter().enumerate() {
            let sz = self.icon_size(i);
            let cxm = self.icon_center_x(i);
            let r = Rect::new(cxm - sz / 2.0, bottom - sz, sz, sz);
            let kr = krect(r);
            cx.list.push_fill_shape(
                kr,
                &martensite_core::shape::Shape::rounded(sz * 0.22),
                item.color,
            );
            if let Some(img) = &item.image {
                cx.list.push_image(kr, img.clone());
            }
            if self.hovered == Some(i) {
                cx.list.push_stroke_shape(
                    kr,
                    &martensite_core::shape::Shape::rounded(sz * 0.22),
                    1.5 * s,
                    cx.color(TokenKey::AccentColor, TEXT),
                );
            }
            if item.running {
                let d = DOT_PT * s;
                cx.list.push_fill_shape(
                    krect(Rect::new(cxm - d / 2.0, bottom + d * 0.8, d, d)),
                    &martensite_core::shape::Shape::ELLIPSE,
                    cx.color(TokenKey::TextColor, DOT),
                );
            }
            // Status chip straddling the icon's top-right corner —
            // glyph-marked so the state is never color-only.
            if let Some(status) = item.status {
                crate::widgets::status_dot::paint_status_chip(
                    cx,
                    Vec2::new(r.max_x(), r.min_y()),
                    6.0 * s,
                    status,
                );
            }
        }
        // Tooltip over the hovered icon.
        if let Some(i) = self.hovered {
            if let Some(item) = self.items.get(i) {
                let size = 10.0 * s;
                let w = painter
                    .and_then(|p| p.measure_text(&item.label, size))
                    .unwrap_or(item.label.len() as f32 * size * 0.55);
                let cxm = self.icon_center_x(i);
                let icon_top = bottom - self.icon_size(i);
                let pad = 5.0 * s;
                let tip = Rect::new(
                    cxm - w / 2.0 - pad,
                    icon_top - size - pad * 2.4,
                    w + pad * 2.0,
                    size + pad * 1.8,
                );
                cx.list.push_fill_shape(
                    krect(tip),
                    &martensite_core::shape::Shape::rounded(4.0 * s),
                    cx.color(TokenKey::SurfaceColor, TIP),
                );
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    krect(tip),
                    kurbo::Point::new(
                        f64::from(tip.min_x() + pad),
                        f64::from(tip.min_y() + pad * 0.8),
                    ),
                    &item.label,
                    size,
                    cx.color(TokenKey::TextColor, TEXT),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PaintList};

    fn laid_out(d: &mut Dock, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        d.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        d.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev(d: &mut Dock, e: &WidgetEvent) {
        d.event(&mut EventContext {
            event: e,
            bounds: d.bounds,
            scale: 1.0,
        });
    }

    fn dock3() -> Dock {
        Dock::new()
            .item(DockItem::new("a", [1, 2, 3, 255]))
            .item(DockItem::new("b", [4, 5, 6, 255]).running(true))
            .item(DockItem::new("c", [7, 8, 9, 255]))
    }

    #[test]
    fn magnification_peaks_at_pointer() {
        let mut d = dock3();
        laid_out(&mut d, 300.0, 90.0);
        let cx1 = d.icon_center_x(1);
        ev(
            &mut d,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(cx1, 60.0),
            },
        );
        let mid = d.icon_size(1);
        let edge = d.icon_size(0);
        assert!(mid > edge);
        assert!((mid - ICON_PT * d.magnify).abs() < 1.0);
    }

    #[test]
    fn click_launches() {
        let mut d = dock3();
        laid_out(&mut d, 300.0, 90.0);
        let cx1 = d.icon_center_x(1);
        ev(
            &mut d,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(cx1, 60.0),
            },
        );
        ev(
            &mut d,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(cx1, 60.0),
                count: 1,
            },
        );
        assert_eq!(d.take_launched(), Some(1));
        assert_eq!(d.take_launched(), None);
    }

    #[test]
    fn leave_clears_hover() {
        let mut d = dock3();
        laid_out(&mut d, 300.0, 90.0);
        ev(
            &mut d,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(150.0, 60.0),
            },
        );
        assert!(d.hover_x.is_some());
        ev(&mut d, &WidgetEvent::PointerLeave);
        assert!(d.hover_x.is_none());
        assert_eq!(d.hovered(), None);
    }

    #[test]
    fn arrows_and_enter() {
        let mut d = dock3();
        laid_out(&mut d, 300.0, 90.0);
        ev(
            &mut d,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        assert_eq!(d.hovered(), Some(1));
        ev(
            &mut d,
            &WidgetEvent::KeyPressed {
                key: "Enter".to_string(),
                repeat: false,
            },
        );
        assert_eq!(d.take_launched(), Some(1));
    }

    #[test]
    fn paints_without_painter() {
        let mut d = dock3();
        laid_out(&mut d, 300.0, 90.0);
        let theme = martensite_theme::Theme::new("test");
        let mut list = PaintList::new();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: d.bounds,
            scale: 1.0,
            theme: &theme,
            text_painter: None,
        };
        d.paint(&mut cx);
    }
}
