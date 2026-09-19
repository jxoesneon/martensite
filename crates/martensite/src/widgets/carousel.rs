//! `Carousel` — paged content rotator.
//!
//! The Ant `Carousel` / UIKit page-control pattern: one child page
//! visible at a time, dot indicators along the bottom edge, `‹`/`›`
//! arrow zones on the sides, and `PageUp`/`PageDown`/arrow keys for
//! keyboard paging. Swipes arrive as `Scroll` events and page
//! horizontally. Navigation parks the new index in
//! [`Carousel::take_navigated`].
//!
//! Hidden pages report `None` bounds (the PanelSet convention) so
//! they drop out of paint, hit-testing, and the a11y tree.
//!
//! # Examples
//!
//! ```
//! use martensite::prelude::*;
//! use martensite::widgets::carousel::Carousel;
//!
//! let mut c = Carousel::new()
//!     .page(Text::new("one"))
//!     .page(Text::new("two"))
//!     .wrap(true);
//! c.go_to(1);
//! assert_eq!(c.current(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

/// Active dot / arrow ink.
const ACCENT: TokenKey = TokenKey::AccentColor;
/// Inactive dot.
const DOT: TokenKey = TokenKey::TextMutedColor;
/// Arrow ink.
const ARROW: TokenKey = TokenKey::TextColor;
/// Dot strip height (logical points).
const DOTS_H: f32 = 20.0;
/// Dot diameter (logical points).
const DOT_D: f32 = 6.0;
/// Dot gap (logical points).
const DOT_GAP: f32 = 8.0;
/// Arrow hit-zone width (logical points).
const ARROW_W: f32 = 36.0;

/// A paged content rotator — see the module docs.
///
/// Children are pages; only the current page reports bounds, so
/// traversal paints/hit-tests exactly one page.
///
/// # Examples
///
/// ```
/// use martensite::prelude::*;
/// use martensite::widgets::carousel::Carousel;
/// use martensite::core::Widget;
///
/// let mut c = Carousel::new().page(Text::new("a")).page(Text::new("b"));
/// assert_eq!(c.child_count(), 2);
/// ```
pub struct Carousel {
    label: String,
    enabled: bool,
    pages: Vec<Box<dyn Widget>>,
    current: usize,
    /// Whether paging wraps past the ends.
    wrap: bool,
    /// Parked navigation for `take_navigated`.
    pending: Option<usize>,
    /// Hovered arrow zone (`-1` = ‹, `1` = ›).
    hover_arrow: i8,
    /// Content rect (bounds minus the dot strip) and per-page bounds
    /// from the last layout — in the same space `layout` saw.
    content: Option<Rect>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Carousel {
    /// An empty carousel.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::carousel::Carousel;
    ///
    /// let c = Carousel::new();
    /// assert_eq!(c.current(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Carousel".into(),
            enabled: true,
            pages: Vec::new(),
            current: 0,
            wrap: false,
            pending: None,
            hover_arrow: 0,
            content: None,
            text_painter: None,
        }
    }

    /// Append a page.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::carousel::Carousel;
    ///
    /// let c = Carousel::new().page(Text::new("slide"));
    /// ```
    pub fn page(mut self, page: impl Widget) -> Self {
        self.pages.push(Box::new(page));
        self
    }

    /// Wrap paging past the ends (`false` clamps; default `false`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::carousel::Carousel;
    ///
    /// let c = Carousel::new().wrap(true);
    /// ```
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Set the accessibility label (default `"Carousel"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::carousel::Carousel;
    ///
    /// let c = Carousel::new().label("Featured");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Enable or disable interaction (default `true`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::carousel::Carousel;
    ///
    /// let c = Carousel::new().enabled(false);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Share a text painter. `SharedTextPainter` is not `Default`, so
    /// this builder is exercised indirectly through `paint`.
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The current page index.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::carousel::Carousel;
    ///
    /// assert_eq!(Carousel::new().current(), 0);
    /// ```
    pub fn current(&self) -> usize {
        self.current
    }

    /// Jump to `index` (clamped to the page range).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::carousel::Carousel;
    ///
    /// let mut c = Carousel::new().page(Text::new("a")).page(Text::new("b"));
    /// c.go_to(1);
    /// assert_eq!(c.current(), 1);
    /// ```
    pub fn go_to(&mut self, index: usize) {
        if !self.pages.is_empty() {
            self.current = index.min(self.pages.len() - 1);
        }
    }

    /// Advance (or retreat) one page honouring `wrap`; parks the new
    /// index for `take_navigated`. Returns `true` when the page moved.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::carousel::Carousel;
    ///
    /// let mut c = Carousel::new().page(Text::new("a")).page(Text::new("b"));
    /// assert!(c.next_page());
    /// assert!(!c.next_page()); // clamped at the last page without wrap
    /// ```
    pub fn step(&mut self, delta: i32) -> bool {
        let n = self.pages.len();
        if n == 0 {
            return false;
        }
        let next = self.current as i32 + delta;
        let next = if self.wrap {
            next.rem_euclid(n as i32) as usize
        } else {
            if !(0..n as i32).contains(&next) {
                return false;
            }
            next as usize
        };
        if next == self.current {
            return false;
        }
        self.current = next;
        self.pending = Some(next);
        true
    }

    /// Advance one page — `step(1)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::carousel::Carousel;
    ///
    /// let mut c = Carousel::new().page(Text::new("a")).page(Text::new("b"));
    /// c.next_page();
    /// assert_eq!(c.current(), 1);
    /// ```
    pub fn next_page(&mut self) -> bool {
        self.step(1)
    }

    /// Retreat one page — `step(-1)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::carousel::Carousel;
    ///
    /// let mut c = Carousel::new().page(Text::new("a")).page(Text::new("b"));
    /// c.go_to(1);
    /// c.prev_page();
    /// assert_eq!(c.current(), 0);
    /// ```
    pub fn prev_page(&mut self) -> bool {
        self.step(-1)
    }

    /// Drain the parked navigation (one-shot, like every `take_*`
    /// seam).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::carousel::Carousel;
    ///
    /// let mut c = Carousel::new();
    /// assert_eq!(c.take_navigated(), None);
    /// ```
    pub fn take_navigated(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// Page count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::carousel::Carousel;
    ///
    /// let c = Carousel::new().page(Text::new("a"));
    /// assert_eq!(c.page_count(), 1);
    /// ```
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Which arrow zone `local` (widget-local point) hits.
    fn arrow_hit(&self, local: Vec2, width: f32, arrow_w: f32) -> i8 {
        if local.x < arrow_w {
            -1
        } else if local.x > width - arrow_w {
            1
        } else {
            0
        }
    }

    /// Which dot `local` hits — dots are centered under the content.
    fn dot_hit(&self, local: Vec2, width: f32, dots_h: f32, height: f32) -> Option<usize> {
        if local.y < height - dots_h {
            return None;
        }
        let n = self.pages.len();
        let d = DOT_D;
        let gap = DOT_GAP;
        let strip_w = n as f32 * d + n.saturating_sub(1) as f32 * gap;
        let x0 = (width - strip_w) * 0.5;
        for i in 0..n {
            let dx = x0 + i as f32 * (d + gap);
            if local.x >= dx && local.x <= dx + d {
                return Some(i);
            }
        }
        None
    }
}

impl Default for Carousel {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Carousel {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Widest/tallest page plus the dot strip.
        let mut size = Vec2::new(200.0, 140.0);
        for p in self.pages.iter_mut() {
            let want = p.measure(cx, constraints);
            size.x = size.x.max(want.x);
            size.y = size.y.max(want.y);
        }
        if self.pages.len() > 1 {
            size.y += DOTS_H;
        }
        size
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let dots_h = if self.pages.len() > 1 {
            cx.pt(DOTS_H)
        } else {
            0.0
        };
        let content = Rect::new(
            bounds.min_x(),
            bounds.min_y(),
            bounds.width(),
            (bounds.height() - dots_h).max(0.0),
        );
        self.content = Some(content);
        // Layout every page (state must stay warm) — hidden pages get
        // `None` bounds from `child_bounds` and drop out of traversal.
        for p in self.pages.iter_mut() {
            cx.layout_child(p.as_mut(), content);
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let n = self.pages.len();
        if n <= 1 {
            return;
        }
        let accent = cx.color(ACCENT, [50, 115, 230, 255]);
        let dot_off = cx.color(DOT, [160, 160, 170, 255]);
        let arrow_ink = cx.color(ARROW, [70, 70, 78, 255]);
        let dots_h = cx.pt(DOTS_H);
        let d = cx.pt(DOT_D);
        let gap = cx.pt(DOT_GAP);
        let arrow_w = cx.pt(ARROW_W);

        // Dot strip centered under the content.
        let strip_w = n as f32 * d + (n - 1) as f32 * gap;
        let x0 = b.min_x() + (b.width() - strip_w) * 0.5;
        let cy = b.max_y() - dots_h * 0.5;
        for i in 0..n {
            let dx = f64::from(x0 + i as f32 * (d + gap));
            let r = kurbo::Rect::new(
                dx,
                f64::from(cy - d * 0.5),
                dx + f64::from(d),
                f64::from(cy + d * 0.5),
            );
            cx.list.push_fill_shape(
                r,
                &martensite_core::shape::Shape::ELLIPSE,
                if i == self.current { accent } else { dot_off },
            );
        }

        // Arrow chevrons inside the side zones (brighter on hover).
        let strip = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y() - dots_h),
        );
        let can_prev = self.wrap || self.current > 0;
        let can_next = self.wrap || self.current + 1 < n;
        if can_prev {
            let ink = if self.hover_arrow == -1 {
                accent
            } else {
                arrow_ink
            };
            paint_chev(cx.list, strip, arrow_w * 0.5, true, ink);
        }
        if can_next {
            let ink = if self.hover_arrow == 1 {
                accent
            } else {
                arrow_ink
            };
            paint_chev(cx.list, strip, b.width() - arrow_w * 0.5, false, ink);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        let arrow_w = cx.scale * ARROW_W;
        let dots_h = cx.scale * DOTS_H;
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let local = *position - cx.bounds.origin;
                let arrow = self.arrow_hit(local, cx.bounds.width(), arrow_w);
                if arrow != self.hover_arrow {
                    self.hover_arrow = arrow;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerReleased { position, button }
                if *button == martensite_core::PointerButton::Primary =>
            {
                let local = *position - cx.bounds.origin;
                if let Some(dot) =
                    self.dot_hit(local, cx.bounds.width(), dots_h, cx.bounds.height())
                {
                    if dot != self.current {
                        self.current = dot;
                        self.pending = Some(dot);
                    }
                    return EventResponse::RequestRepaint;
                }
                match self.arrow_hit(local, cx.bounds.width(), arrow_w) {
                    -1 if self.step(-1) => EventResponse::RequestRepaint,
                    1 if self.step(1) => EventResponse::RequestRepaint,
                    0 => EventResponse::Ignored,
                    _ => EventResponse::Handled,
                }
            }
            WidgetEvent::Scroll { position, delta } => {
                let local = *position - cx.bounds.origin;
                if local.y > cx.bounds.height() - dots_h {
                    return EventResponse::Ignored;
                }
                if delta.x > 0.5 && self.step(-1) {
                    return EventResponse::RequestRepaint;
                }
                if delta.x < -0.5 && self.step(1) {
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerLeave => {
                if self.hover_arrow != 0 {
                    self.hover_arrow = 0;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowLeft" | "PageUp" => {
                    if self.step(-1) {
                        EventResponse::RequestRepaint
                    } else {
                        EventResponse::Ignored
                    }
                }
                "ArrowRight" | "PageDown" => {
                    if self.step(1) {
                        EventResponse::RequestRepaint
                    } else {
                        EventResponse::Ignored
                    }
                }
                "Home" => {
                    if self.current == 0 {
                        EventResponse::Ignored
                    } else {
                        self.current = 0;
                        self.pending = Some(0);
                        EventResponse::RequestRepaint
                    }
                }
                "End" => {
                    let last = self.pages.len().saturating_sub(1);
                    if self.current == last {
                        EventResponse::Ignored
                    } else {
                        self.current = last;
                        self.pending = Some(last);
                        EventResponse::RequestRepaint
                    }
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.as_str());
        node.set_value(format!("{} of {}", self.current + 1, self.pages.len()));
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn child_count(&self) -> usize {
        self.pages.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.pages.get(index).map(|p| p.as_ref() as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.pages
            .get_mut(index)
            .map(|p| p.as_mut() as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        // Hidden pages report `None` — the PanelSet convention drops
        // them from paint, hit-testing, and the a11y tree.
        if index == self.current {
            self.content
        } else {
            None
        }
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 120.0)).with_policy(UnderflowPolicy::Lint)
    }
}

/// Paint a chevron centered `cx_off` from the strip's left edge
/// (`left` → `‹`).
fn paint_chev(
    list: &mut martensite_core::PaintList,
    strip: kurbo::Rect,
    cx_off: f32,
    left: bool,
    color: [u8; 4],
) {
    let cx = strip.x0 + f64::from(cx_off);
    let cy = strip.y0 + strip.height() * 0.5;
    let s = 5.0f64;
    let dir = if left { -1.0 } else { 1.0 };
    let mut path = kurbo::BezPath::new();
    path.move_to(kurbo::Point::new(cx - dir * s, cy - s));
    path.line_to(kurbo::Point::new(cx + dir * s, cy));
    path.line_to(kurbo::Point::new(cx - dir * s, cy + s));
    list.push_stroke_path(path, 1.6, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::text::Text;
    use martensite_core::{HotNode, PointerButton};

    fn car(n: usize) -> Carousel {
        (0..n).fold(Carousel::new(), |c, i| c.page(Text::new(format!("p{i}"))))
    }

    fn lay(w: &mut Carousel) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, 300.0, 200.0));
    }

    fn ev(w: &mut Carousel, e: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: e,
            bounds: Rect::new(0.0, 0.0, 300.0, 200.0),
            scale: 1.0,
        };
        w.event(&mut cx)
    }

    #[test]
    fn only_current_page_reports_bounds() {
        let mut c = car(3);
        lay(&mut c);
        assert!(c.child_bounds(0).is_some());
        assert!(c.child_bounds(1).is_none());
        assert!(c.child_bounds(2).is_none());
        c.go_to(2);
        assert!(c.child_bounds(0).is_none());
        assert!(c.child_bounds(2).is_some());
    }

    #[test]
    fn step_clamps_without_wrap() {
        let mut c = car(2);
        assert!(c.next_page());
        assert_eq!(c.current(), 1);
        assert!(!c.next_page());
        assert_eq!(c.current(), 1);
        assert_eq!(c.take_navigated(), Some(1));
    }

    #[test]
    fn wrap_cycles_both_ends() {
        let mut c = car(3).wrap(true);
        assert!(c.prev_page());
        assert_eq!(c.current(), 2);
        assert!(c.next_page());
        assert_eq!(c.current(), 0);
    }

    #[test]
    fn arrow_zone_pages() {
        let mut c = car(3);
        lay(&mut c);
        ev(
            &mut c,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(10.0, 100.0), // ‹ zone — nothing to go back to
                button: PointerButton::Primary,
            },
        );
        assert_eq!(c.current(), 0);
        ev(
            &mut c,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(290.0, 100.0), // › zone
                button: PointerButton::Primary,
            },
        );
        assert_eq!(c.current(), 1);
        assert_eq!(c.take_navigated(), Some(1));
    }

    #[test]
    fn dot_click_jumps() {
        let mut c = car(3);
        lay(&mut c);
        // 3 dots: strip_w = 3*6 + 2*8 = 34, x0 = (300-34)/2 = 133.
        // Dot 2 center ≈ 133 + 2*14 + 3 = 161.
        ev(
            &mut c,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(161.0, 190.0),
                button: PointerButton::Primary,
            },
        );
        assert_eq!(c.current(), 2);
    }

    #[test]
    fn keys_page_and_jump_ends() {
        let mut c = car(4);
        lay(&mut c);
        for key in ["ArrowRight", "ArrowRight", "End"] {
            ev(
                &mut c,
                &WidgetEvent::KeyPressed {
                    key: key.into(),
                    repeat: false,
                },
            );
        }
        assert_eq!(c.current(), 3);
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: "Home".into(),
                repeat: false,
            },
        );
        assert_eq!(c.current(), 0);
    }

    #[test]
    fn scroll_pages_horizontally() {
        let mut c = car(2);
        lay(&mut c);
        ev(
            &mut c,
            &WidgetEvent::Scroll {
                position: Vec2::new(150.0, 100.0),
                delta: Vec2::new(-10.0, 0.0),
            },
        );
        assert_eq!(c.current(), 1);
    }

    #[test]
    fn disabled_is_inert() {
        let mut c = car(2).enabled(false);
        lay(&mut c);
        let r = ev(
            &mut c,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(290.0, 100.0),
                button: PointerButton::Primary,
            },
        );
        assert_eq!(r, EventResponse::Ignored);
        assert_eq!(c.current(), 0);
    }
}
