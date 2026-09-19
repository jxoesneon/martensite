//! `PipsPager` — a row of page dots (WinUI `PipsPager`, iOS
//! `UIPageControl`).
//!
//! Displays `count` dots with the `current` one emphasized; a press
//! selects that page and parks it in [`PipsPager::take_selected`].
//! Long lists clamp to a sliding window around `current` — the
//! `UIPageControl` convention — so the strip never grows unboundedly.
//!
//! `Carousel` embeds the same dot logic internally; use `PipsPager`
//! when the paged content is your own (custom page switchers, forms,
//! onboarding flows).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::pips_pager::PipsPager;
//!
//! let pager = PipsPager::new(5);
//! assert_eq!(pager.current, 0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, SemanticAction, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const DOT_PT: f32 = 7.0;
const GAP_PT: f32 = 8.0;
/// Maximum dots shown before the window slides (`UIPageControl`-style).
const MAX_VISIBLE: usize = 9;
const ON: [u8; 4] = [0, 120, 215, 255];
const OFF: [u8; 4] = [0, 0, 0, 70];
const HOVER: [u8; 4] = [0, 0, 0, 130];

/// A row of page dots — see the module docs.
///
/// ```
/// use martensite::widgets::pips_pager::PipsPager;
///
/// let p = PipsPager::new(3);
/// assert_eq!(p.count, 3);
/// ```
pub struct PipsPager {
    /// Total page count.
    pub count: usize,
    /// The emphasized page index (clamped to `count - 1` on set).
    pub current: usize,
    /// Maximum visible dots before the window slides.
    pub max_visible: usize,
    /// When `false` presses are ignored.
    pub enabled: bool,
    selected: Option<usize>,
    hovered: Option<usize>,
    bounds: Rect,
    scale: f32,
}

impl PipsPager {
    /// Creates a pager for `count` pages.
    ///
    /// ```
    /// use martensite::widgets::pips_pager::PipsPager;
    ///
    /// let p = PipsPager::new(4);
    /// assert_eq!(p.count, 4);
    /// assert_eq!(p.current, 0);
    /// ```
    pub fn new(count: usize) -> Self {
        Self {
            count,
            current: 0,
            max_visible: MAX_VISIBLE,
            enabled: true,
            selected: None,
            hovered: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Sets the current page (clamped to `count - 1`).
    ///
    /// ```
    /// use martensite::widgets::pips_pager::PipsPager;
    ///
    /// let mut p = PipsPager::new(4);
    /// p.set_current(10);
    /// assert_eq!(p.current, 3);
    /// ```
    pub fn set_current(&mut self, index: usize) {
        self.current = index.min(self.count.saturating_sub(1));
    }

    /// Sets the maximum number of visible dots.
    ///
    /// ```
    /// use martensite::widgets::pips_pager::PipsPager;
    ///
    /// let p = PipsPager::new(20).max_visible(5);
    /// assert_eq!(p.max_visible, 5);
    /// ```
    pub fn max_visible(mut self, n: usize) -> Self {
        self.max_visible = n.max(1);
        self
    }

    /// Enables or disables the pager.
    ///
    /// ```
    /// use martensite::widgets::pips_pager::PipsPager;
    ///
    /// let p = PipsPager::new(3).enabled(false);
    /// assert!(!p.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Drains the last dot-selected page.
    ///
    /// ```
    /// use martensite::widgets::pips_pager::PipsPager;
    ///
    /// let mut p = PipsPager::new(3);
    /// assert_eq!(p.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.selected.take()
    }

    /// `[first, last]` dot indices actually drawn — a sliding window
    /// around `current` when `count > max_visible`.
    fn visible_range(&self) -> (usize, usize) {
        let n = self.max_visible.min(self.count);
        if self.count <= n {
            return (0, self.count);
        }
        // Keep `current` centered; clamp at the edges.
        let half = n / 2;
        let start = self.current.saturating_sub(half).min(self.count - n);
        (start, start + n)
    }

    /// Device-pixel center-x of a *visible-slot* index (0-based within
    /// the drawn run).
    fn dot_x(&self, slot: usize) -> f32 {
        let pitch = (DOT_PT + GAP_PT) * self.scale;
        let dots = self.visible_range().1 - self.visible_range().0;
        let strip_w = pitch * dots as f32 - GAP_PT * self.scale;
        self.bounds.origin.x
            + (self.bounds.size.x - strip_w) / 2.0
            + slot as f32 * pitch
            + (DOT_PT * self.scale) / 2.0
    }

    /// Hit-test: page index under `x`, or `None`.
    fn hit(&self, position: Vec2) -> Option<usize> {
        let (lo, hi) = self.visible_range();
        for slot in 0..(hi - lo) {
            let cx_dot = self.dot_x(slot);
            let r = (DOT_PT + GAP_PT) * self.scale / 2.0;
            if (position.x - cx_dot).abs() <= r {
                return Some(lo + slot);
            }
        }
        None
    }
}

impl Widget for PipsPager {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let dots = self.visible_range().1 - self.visible_range().0;
        let w = DOT_PT * dots.max(1) as f32 + GAP_PT * dots.saturating_sub(1) as f32;
        Vec2::new(
            cx.pt(w.max(DOT_PT)).min(constraints.max_size.x.max(0.0)),
            cx.pt(DOT_PT + 4.0).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(DOT_PT * 3.0, DOT_PT + 4.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.current = self.current.min(self.count.saturating_sub(1));
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Slider);
        node.set_label("Page");
        node.set_value(format!("{} of {}", self.current + 1, self.count));
        node.add_action(accesskit::Action::Increment);
        node.add_action(accesskit::Action::Decrement);
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.hit(*position);
                if h != self.hovered {
                    self.hovered = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.is_some() {
                    self.hovered = None;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => match self.hit(*position) {
                Some(i) if i != self.current => {
                    self.current = i;
                    self.selected = Some(i);
                    EventResponse::RequestRepaint
                }
                Some(_) => EventResponse::Handled,
                None => EventResponse::Ignored,
            },
            WidgetEvent::SemanticAction(SemanticAction::Increment) => {
                if self.current + 1 < self.count {
                    self.current += 1;
                    self.selected = Some(self.current);
                }
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(SemanticAction::Decrement) => {
                if self.current > 0 {
                    self.current -= 1;
                    self.selected = Some(self.current);
                }
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, .. } if key == "ArrowLeft" || key == "ArrowRight" => {
                let next = if key == "ArrowRight" {
                    (self.current + 1).min(self.count.saturating_sub(1))
                } else {
                    self.current.saturating_sub(1)
                };
                if next != self.current {
                    self.current = next;
                    self.selected = Some(next);
                }
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let (lo, hi) = self.visible_range();
        let r = DOT_PT * self.scale / 2.0;
        let cy = self.bounds.origin.y + self.bounds.size.y / 2.0;
        for slot in 0..(hi - lo) {
            let page = lo + slot;
            let x = self.dot_x(slot);
            let color = if page == self.current {
                cx.color(TokenKey::AccentColor, ON)
            } else if self.hovered == Some(page) {
                cx.color(TokenKey::TextMutedColor, HOVER)
            } else {
                cx.color(TokenKey::DividerColor, OFF)
            };
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(x - r),
                    f64::from(cy - r),
                    f64::from(x + r),
                    f64::from(cy + r),
                ),
                &martensite_core::shape::Shape::circle(Vec2::new(x, cy), r),
                color,
            );
        }
    }
}

impl std::fmt::Debug for PipsPager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipsPager")
            .field("count", &self.count)
            .field("current", &self.current)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(p: &mut PipsPager, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        p.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        p.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 200.0, 11.0),
            scale: 1.0,
        }
    }

    #[test]
    fn press_selects_dot() {
        let mut p = PipsPager::new(3);
        laid_out(&mut p, 200.0, 11.0);
        let x = p.dot_x(2); // third dot
        p.event(&mut ev(&WidgetEvent::PointerPressed {
            position: Vec2::new(x, 5.5),
            button: PointerButton::Primary,
            count: 1,
        }));
        assert_eq!(p.current, 2);
        assert_eq!(p.take_selected(), Some(2));
        assert_eq!(p.take_selected(), None);
    }

    #[test]
    fn arrows_step() {
        let mut p = PipsPager::new(3);
        laid_out(&mut p, 200.0, 11.0);
        p.event(&mut ev(&WidgetEvent::KeyPressed {
            key: "ArrowRight".into(),
            repeat: false,
        }));
        assert_eq!(p.current, 1);
        p.event(&mut ev(&WidgetEvent::KeyPressed {
            key: "ArrowRight".into(),
            repeat: false,
        }));
        p.event(&mut ev(&WidgetEvent::KeyPressed {
            key: "ArrowRight".into(),
            repeat: false,
        }));
        assert_eq!(p.current, 2); // clamps at the end
        assert_eq!(p.take_selected(), Some(2));
    }

    #[test]
    fn window_slides_around_current() {
        let mut p = PipsPager::new(20).max_visible(5);
        p.set_current(10);
        let (lo, hi) = p.visible_range();
        assert_eq!(hi - lo, 5);
        assert!(lo <= 10 && 10 < hi);
        p.set_current(19);
        let (lo, hi) = p.visible_range();
        assert_eq!(hi, 20); // clamped at the end
        assert_eq!(hi - lo, 5);
    }

    #[test]
    fn semantic_increment_selects() {
        let mut p = PipsPager::new(3);
        laid_out(&mut p, 200.0, 11.0);
        p.event(&mut ev(&WidgetEvent::SemanticAction(
            SemanticAction::Increment,
        )));
        assert_eq!(p.current, 1);
        assert_eq!(p.take_selected(), Some(1));
    }

    #[test]
    fn disabled_ignores_press() {
        let mut p = PipsPager::new(3).enabled(false);
        laid_out(&mut p, 200.0, 11.0);
        assert_eq!(
            p.event(&mut ev(&WidgetEvent::PointerPressed {
                position: Vec2::new(100.0, 5.5),
                button: PointerButton::Primary,
                count: 1,
            })),
            EventResponse::Ignored
        );
        assert_eq!(p.current, 0);
    }
}
