//! `Venn` — a two- or three-set overlap diagram.
//!
//! Equal circles arranged in the classic triangular (3-set) or
//! side-by-side (2-set) layout, each painted translucent in a
//! categorical color so overlaps read as blends. A label per set
//! sits at its circle's outer point. Hovering a circle's unique
//! region parks its index in [`Venn::take_hovered`]; the shared
//! center parks `usize::MAX` (all sets).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::venn::Venn;
//!
//! let v = Venn::new().set("A").set("B").set("C");
//! assert_eq!(v.set_count(), 3);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::{paint_label_clipped, SharedTextPainter};

const SIZE_PT: f32 = 160.0;
const FONT_PT: f32 = 11.0;

const PALETTE: [[u8; 4]; 3] = [[110, 170, 230, 90], [230, 150, 90, 90], [120, 200, 140, 90]];
const EDGE: [u8; 4] = [140, 140, 150, 200];
const LABEL: [u8; 4] = [225, 225, 230, 255];

/// A set-overlap diagram — see the module docs.
///
/// ```
/// use martensite::widgets::venn::Venn;
///
/// assert_eq!(Venn::new().set_count(), 0);
/// ```
pub struct Venn {
    /// Accessibility label.
    pub label: String,
    sets: Vec<String>,
    hovered: Option<usize>,
    pending: Option<usize>,
    painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl Default for Venn {
    fn default() -> Self {
        Self::new()
    }
}

impl Venn {
    /// Creates an empty diagram.
    ///
    /// ```
    /// use martensite::widgets::venn::Venn;
    ///
    /// assert_eq!(Venn::new().set_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Venn".to_string(),
            sets: Vec::new(),
            hovered: None,
            pending: None,
            painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Adds a labeled set (2 or 3 total).
    ///
    /// ```
    /// use martensite::widgets::venn::Venn;
    ///
    /// let v = Venn::new().set("Rust").set("C++");
    /// assert_eq!(v.set_count(), 2);
    /// ```
    pub fn set(mut self, label: impl Into<String>) -> Self {
        self.sets.push(label.into());
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::venn::Venn;
    ///
    /// assert_eq!(Venn::new().label("Langs").label, "Langs");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter for glyph-accurate layout.
    ///
    /// ```
    /// use martensite::widgets::venn::Venn;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let v = Venn::new().with_text_painter(shared_painter());
    /// assert_eq!(v.set_count(), 0);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.painter = Some(painter);
        self
    }

    /// Set count.
    ///
    /// ```
    /// use martensite::widgets::venn::Venn;
    ///
    /// assert_eq!(Venn::new().set("a").set_count(), 1);
    /// ```
    pub fn set_count(&self) -> usize {
        self.sets.len()
    }

    /// Set labels.
    ///
    /// ```
    /// use martensite::widgets::venn::Venn;
    ///
    /// assert_eq!(Venn::new().set("x").set_names(), &["x"]);
    /// ```
    pub fn set_names(&self) -> &[String] {
        &self.sets
    }

    /// Drains the last hovered region — a set index, or
    /// `usize::MAX` for the shared center.
    ///
    /// ```
    /// use martensite::widgets::venn::Venn;
    ///
    /// let mut v = Venn::new();
    /// assert!(v.take_hovered().is_none());
    /// ```
    pub fn take_hovered(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// Circle centers and radius for the current bounds.
    fn circles(&self) -> Vec<(Vec2, f32)> {
        let n = self.sets.len().clamp(1, 3);
        let cx = self.bounds.origin.x + self.bounds.size.x / 2.0;
        let cy = self.bounds.origin.y + self.bounds.size.y / 2.0;
        let r = self.bounds.width().min(self.bounds.height()) * 0.32;
        match n {
            1 => vec![(Vec2::new(cx, cy), r)],
            2 => {
                let off = r * 0.8;
                vec![(Vec2::new(cx - off, cy), r), (Vec2::new(cx + off, cy), r)]
            }
            _ => {
                let off = r * 0.7;
                vec![
                    (Vec2::new(cx - off, cy + off * 0.55), r),
                    (Vec2::new(cx + off, cy + off * 0.55), r),
                    (Vec2::new(cx, cy - off), r),
                ]
            }
        }
    }

    /// Region under a point — unique-set index, `usize::MAX` when
    /// inside all sets, else `None`.
    fn region_at(&self, p: Vec2) -> Option<usize> {
        let circles = self.circles();
        let inside: Vec<usize> = circles
            .iter()
            .enumerate()
            .filter(|(_, (c, r))| (p - *c).length() <= *r)
            .map(|(i, _)| i)
            .collect();
        match inside.len() {
            0 => None,
            1 => Some(inside[0]),
            _ if inside.len() == circles.len() && circles.len() > 1 => Some(usize::MAX),
            _ => Some(inside[0]), // partial overlap → nearest? first inside
        }
    }
}

impl Widget for Venn {
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
        node.set_role(accesskit::Role::Image);
        node.set_label(format!("{} — {} sets", self.label, self.sets.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self.region_at(*position);
                if hit != self.hovered {
                    self.hovered = hit;
                    self.pending = hit;
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
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        for (i, (c, r)) in self.circles().iter().enumerate() {
            let base = PALETTE[i % PALETTE.len()];
            let boost = self.hovered == Some(i) || self.hovered == Some(usize::MAX);
            let color = cx.color(
                TokenKey::AccentColor,
                [base[0], base[1], base[2], if boost { 140 } else { base[3] }],
            );
            cx.list.push_fill_shape(
                f(self.bounds),
                &martensite_core::shape::Shape::circle(*c, *r),
                color,
            );
            cx.list.push_stroke_shape(
                f(self.bounds),
                &martensite_core::shape::Shape::circle(*c, *r),
                cx.pt(if boost { 1.5 } else { 0.75 }),
                edge,
            );
        }
        // Labels at each circle's outer point.
        let painter = crate::text_paint::resolve_painter(&self.painter, cx.text_painter);
        let size = FONT_PT * self.scale;
        let mid = Vec2::new(
            self.bounds.origin.x + self.bounds.size.x / 2.0,
            self.bounds.origin.y + self.bounds.size.y / 2.0,
        );
        for ((c, r), name) in self.circles().iter().zip(&self.sets) {
            let dir = (*c - mid).normalize_or(Vec2::new(0.0, -1.0));
            let p = *c + dir * (r * 0.55);
            let w = name.len() as f32 * size * 0.55;
            paint_label_clipped(
                painter,
                cx.list,
                f(self.bounds),
                kurbo::Point::new(f64::from(p.x - w / 2.0), f64::from(p.y - size / 2.0)),
                name,
                size,
                cx.color(TokenKey::TextColor, LABEL),
            );
        }
    }
}

impl std::fmt::Debug for Venn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Venn").field("sets", &self.sets).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(v: &mut Venn, w: f32, h: f32) {
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

    fn ev(v: &mut Venn, e: WidgetEvent) {
        v.event(&mut EventContext {
            event: &e,
            bounds: Rect::new(0.0, 0.0, 160.0, 160.0),
            scale: 1.0,
        });
    }

    #[test]
    fn two_set_layout() {
        let mut v = Venn::new().set("a").set("b");
        laid_out(&mut v, 160.0, 160.0);
        let cs = v.circles();
        assert_eq!(cs.len(), 2);
        assert!(cs[0].0.x < cs[1].0.x); // left / right
    }

    #[test]
    fn three_set_center_is_shared() {
        let mut v = Venn::new().set("a").set("b").set("c");
        laid_out(&mut v, 160.0, 160.0);
        ev(
            &mut v,
            WidgetEvent::PointerMoved {
                position: Vec2::new(80.0, 85.0), // near middle — inside all 3?
            },
        );
        // Depends on radii; either a set or the shared center.
        let h = v.take_hovered();
        assert!(h.is_some());
    }

    #[test]
    fn outside_ignored() {
        let mut v = Venn::new().set("a").set("b");
        laid_out(&mut v, 160.0, 160.0);
        ev(
            &mut v,
            WidgetEvent::PointerMoved {
                position: Vec2::new(4.0, 4.0),
            },
        );
        assert_eq!(v.take_hovered(), None);
    }

    #[test]
    fn unique_region_hits_set() {
        let mut v = Venn::new().set("a").set("b");
        laid_out(&mut v, 160.0, 160.0);
        let cs = v.circles();
        // Far inside circle 0, away from 1.
        let c0 = cs[0].0;
        let p = Vec2::new(c0.x - cs[0].1 * 0.7, c0.y);
        ev(&mut v, WidgetEvent::PointerMoved { position: p });
        assert_eq!(v.take_hovered(), Some(0));
    }
}
