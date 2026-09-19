//! `NavStack` — push/pop navigation container.
//!
//! The SwiftUI `NavigationStack` / `NavigationPage` / `Navigator`
//! pattern: a titled header with a `‹ Back` affordance and a stack of
//! pages where only the topmost is visible. [`NavStack::push`] adds a
//! page (optionally with a new title); `pop`, the back zone, `Escape`,
//! `Backspace`, or `Alt`+`ArrowLeft` retreat. Every navigation —
//! push *or* pop — parks `(depth, title)` in
//! [`NavStack::take_navigated`].
//!
//! Non-top pages report `None` bounds (the PanelSet convention) so
//! they drop out of paint, hit-testing, and the a11y tree.
//!
//! # Examples
//!
//! ```
//! use martensite::prelude::*;
//! use martensite::widgets::nav_stack::NavStack;
//!
//! let mut nav = NavStack::new(Text::new("root")).title("Home");
//! nav.push(Text::new("detail"), "Detail");
//! assert_eq!(nav.depth(), 2);
//! nav.pop();
//! assert_eq!(nav.depth(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

/// Title / back-link ink.
const TEXT: TokenKey = TokenKey::TextColor;
/// Back-link hover ink.
const ACCENT: TokenKey = TokenKey::AccentColor;
/// Header hairline.
const HAIRLINE: TokenKey = TokenKey::DividerColor;
/// Header strip height (logical points).
const HEADER_H: f32 = 40.0;
/// Back hit-zone width (logical points).
const BACK_W: f32 = 64.0;

/// One stack entry.
struct Page {
    /// The page widget.
    widget: Box<dyn Widget>,
    /// Title shown while this page is on top.
    title: String,
}

/// A push/pop navigation container — see the module docs.
///
/// `NavStack` owns every pushed page; only the top page reports
/// bounds to traversal.
///
/// # Examples
///
/// ```
/// use martensite::prelude::*;
/// use martensite::widgets::nav_stack::NavStack;
/// use martensite::core::Widget;
///
/// let mut nav = NavStack::new(Text::new("root"));
/// assert_eq!(nav.child_count(), 1);
/// ```
pub struct NavStack {
    label: String,
    enabled: bool,
    pages: Vec<Page>,
    /// Parked navigation `(depth_after, title)` for `take_navigated`.
    pending: Option<(usize, String)>,
    /// Back-zone hover.
    hover_back: bool,
    /// Header/content rects from the last layout.
    content: Option<Rect>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl NavStack {
    /// A stack rooted at `root` (title defaults to the stack label).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::nav_stack::NavStack;
    ///
    /// let nav = NavStack::new(Text::new("home"));
    /// assert_eq!(nav.depth(), 1);
    /// ```
    pub fn new(root: impl Widget) -> Self {
        Self {
            label: "Navigation".into(),
            enabled: true,
            pages: vec![Page {
                widget: Box::new(root),
                title: String::new(),
            }],
            pending: None,
            hover_back: false,
            content: None,
            text_painter: None,
        }
    }

    /// Set the stack's accessibility label (default `"Navigation"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::nav_stack::NavStack;
    ///
    /// let nav = NavStack::new(Text::new("x")).label("Sections");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Set the root page's title (shown in the header at depth 1).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::nav_stack::NavStack;
    ///
    /// let nav = NavStack::new(Text::new("x")).title("Home");
    /// ```
    pub fn title(mut self, title: impl Into<String>) -> Self {
        if let Some(root) = self.pages.first_mut() {
            root.title = title.into();
        }
        self
    }

    /// Enable or disable interaction (default `true`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::nav_stack::NavStack;
    ///
    /// let nav = NavStack::new(Text::new("x")).enabled(false);
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

    /// Push `page` under `title`; parks the navigation.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::nav_stack::NavStack;
    ///
    /// let mut nav = NavStack::new(Text::new("a"));
    /// nav.push(Text::new("b"), "B");
    /// assert_eq!(nav.depth(), 2);
    /// ```
    pub fn push(&mut self, page: impl Widget, title: impl Into<String>) {
        self.pages.push(Page {
            widget: Box::new(page),
            title: title.into(),
        });
        self.pending = Some((self.pages.len(), self.current_title().to_string()));
    }

    /// Pop the top page (`false` at the root); parks the navigation.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::nav_stack::NavStack;
    ///
    /// let mut nav = NavStack::new(Text::new("a"));
    /// assert!(!nav.pop());
    /// nav.push(Text::new("b"), "B");
    /// assert!(nav.pop());
    /// ```
    pub fn pop(&mut self) -> bool {
        if self.pages.len() <= 1 {
            return false;
        }
        self.pages.pop();
        self.pending = Some((self.pages.len(), self.current_title().to_string()));
        true
    }

    /// Pop back to the root.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::nav_stack::NavStack;
    ///
    /// let mut nav = NavStack::new(Text::new("a"));
    /// nav.push(Text::new("b"), "B");
    /// nav.push(Text::new("c"), "C");
    /// nav.pop_to_root();
    /// assert_eq!(nav.depth(), 1);
    /// ```
    pub fn pop_to_root(&mut self) {
        if self.pages.len() > 1 {
            self.pages.truncate(1);
            self.pending = Some((1, self.current_title().to_string()));
        }
    }

    /// Stack depth (root counts as 1).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::nav_stack::NavStack;
    ///
    /// assert_eq!(NavStack::new(Text::new("x")).depth(), 1);
    /// ```
    pub fn depth(&self) -> usize {
        self.pages.len()
    }

    /// The top page's title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::nav_stack::NavStack;
    ///
    /// let mut nav = NavStack::new(Text::new("x")).title("Root");
    /// assert_eq!(nav.current_title(), "Root");
    /// ```
    pub fn current_title(&self) -> &str {
        self.pages.last().map(|p| p.title.as_str()).unwrap_or("")
    }

    /// Drain the parked `(depth, title)` navigation event — one-shot.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::prelude::*;
    /// use martensite::widgets::nav_stack::NavStack;
    ///
    /// let mut nav = NavStack::new(Text::new("x"));
    /// assert_eq!(nav.take_navigated(), None);
    /// ```
    pub fn take_navigated(&mut self) -> Option<(usize, String)> {
        self.pending.take()
    }
}

impl Widget for NavStack {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut size = Vec2::new(240.0, 160.0);
        if let Some(top) = self.pages.last_mut() {
            let want = top.widget.measure(cx, constraints);
            size.x = size.x.max(want.x);
            size.y = size.y.max(want.y + HEADER_H);
        }
        size
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let header_h = cx.pt(HEADER_H);
        let content = Rect::new(
            bounds.min_x(),
            bounds.min_y() + header_h,
            bounds.width(),
            (bounds.height() - header_h).max(0.0),
        );
        self.content = Some(content);
        // Layout every page (state stays warm); hidden pages report
        // `None` bounds and drop out of traversal.
        for p in self.pages.iter_mut() {
            cx.layout_child(p.widget.as_mut(), content);
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let header_h = cx.pt(HEADER_H);
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let ink = cx.color(TEXT, [35, 35, 42, 255]);
        let accent = cx.color(ACCENT, [50, 115, 230, 255]);
        let hairline = cx.color(HAIRLINE, [215, 217, 222, 255]);

        // Header hairline.
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y() + header_h - cx.pt(0.5)),
                f64::from(b.max_x()),
                f64::from(b.min_y() + header_h + cx.pt(0.5)),
            ),
            hairline,
        );

        // ‹ Back zone — only at depth ≥ 2.
        let deep = self.pages.len() > 1;
        if deep {
            let back_ink = if self.hover_back { accent } else { ink };
            let strip = kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.min_x() + cx.pt(BACK_W)),
                f64::from(b.min_y() + header_h),
            );
            let s = 4.0f64;
            let cy = strip.y0 + strip.height() * 0.5;
            let cxp = strip.x0 + 10.0;
            let mut path = kurbo::BezPath::new();
            path.move_to(kurbo::Point::new(cxp + s, cy - s));
            path.line_to(kurbo::Point::new(cxp - s, cy));
            path.line_to(kurbo::Point::new(cxp + s, cy + s));
            cx.list.push_stroke_path(path, 1.6, back_ink);
            let size = 13.0 * cx.scale;
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                strip,
                kurbo::Point::new(strip.x0 + f64::from(cx.pt(20.0)), cy + 4.0),
                "Back",
                size,
                back_ink,
            );
        }

        // Title centered in the header.
        let title = self.current_title();
        if !title.is_empty() {
            let size = 14.0 * cx.scale;
            let w = painter
                .and_then(|p| p.measure_text(title, size))
                .unwrap_or(size * title.len() as f32 * 0.5);
            let strip = kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.min_y() + header_h),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                strip,
                kurbo::Point::new(
                    strip.x0 + (strip.width() - f64::from(w)) * 0.5,
                    strip.y0 + strip.height() * 0.72,
                ),
                title,
                size,
                ink,
            );
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        let header_h = cx.scale * HEADER_H;
        let back_w = cx.scale * BACK_W;
        let deep = self.pages.len() > 1;
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let local = *position - cx.bounds.origin;
                let over = deep && local.y < header_h && local.x < back_w;
                if over != self.hover_back {
                    self.hover_back = over;
                    return EventResponse::RequestRepaint;
                }
                self.forward_top(cx)
            }
            WidgetEvent::PointerReleased { position, button }
                if *button == martensite_core::PointerButton::Primary =>
            {
                let local = *position - cx.bounds.origin;
                if deep && local.y < header_h && local.x < back_w {
                    self.pop();
                    return EventResponse::RequestRepaint;
                }
                self.forward_top(cx)
            }
            WidgetEvent::PointerPressed { .. } | WidgetEvent::Scroll { .. } => self.forward_top(cx),
            WidgetEvent::KeyPressed { key, .. } if deep => match key.as_str() {
                "Escape" | "Backspace" => {
                    self.pop();
                    EventResponse::RequestRepaint
                }
                _ => self.forward_top(cx),
            },
            WidgetEvent::PointerLeave => {
                if self.hover_back {
                    self.hover_back = false;
                    return EventResponse::RequestRepaint;
                }
                self.forward_top(cx)
            }
            _ => self.forward_top(cx),
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Navigation);
        node.set_label(self.label.as_str());
        let title = self.current_title();
        if !title.is_empty() {
            node.set_value(title.to_string());
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn child_count(&self) -> usize {
        self.pages.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.pages
            .get(index)
            .map(|p| p.widget.as_ref() as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.pages
            .get_mut(index)
            .map(|p| p.widget.as_mut() as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        // Only the top page is visible/interactive.
        if index + 1 == self.pages.len() {
            self.content
        } else {
            None
        }
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(200.0, 140.0)).with_policy(UnderflowPolicy::Lint)
    }
}

impl NavStack {
    /// Forward the event to the top page through the child protocol.
    fn forward_top(&mut self, cx: &mut EventContext) -> EventResponse {
        let idx = self.pages.len().saturating_sub(1);
        let Some(b) = self.child_bounds(idx) else {
            return EventResponse::Ignored;
        };
        // Keyboard/IME events go to the focus owner regardless of
        // pointer position; pointer events stay bounds-gated.
        if let Some(pos) = cx.event.position() {
            if !b.contains(pos) {
                return EventResponse::Ignored;
            }
        }
        let mut child_cx = EventContext {
            event: cx.event,
            bounds: b,
            scale: cx.scale,
        };
        match self.child_mut(idx) {
            Some(c) => c.event(&mut child_cx),
            None => EventResponse::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::text::Text;
    use martensite_core::{HotNode, PointerButton};

    fn lay(w: &mut NavStack) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, 320.0, 240.0));
    }

    fn ev(w: &mut NavStack, e: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: e,
            bounds: Rect::new(0.0, 0.0, 320.0, 240.0),
            scale: 1.0,
        };
        w.event(&mut cx)
    }

    #[test]
    fn only_top_page_reports_bounds() {
        let mut nav = NavStack::new(Text::new("a"));
        nav.push(Text::new("b"), "B");
        lay(&mut nav);
        assert!(nav.child_bounds(0).is_none());
        assert!(nav.child_bounds(1).is_some());
    }

    #[test]
    fn push_parks_navigation() {
        let mut nav = NavStack::new(Text::new("a")).title("Root");
        nav.push(Text::new("b"), "B");
        assert_eq!(nav.take_navigated(), Some((2, "B".to_string())));
        assert_eq!(nav.take_navigated(), None);
    }

    #[test]
    fn pop_refuses_at_root() {
        let mut nav = NavStack::new(Text::new("a"));
        assert!(!nav.pop());
        assert_eq!(nav.take_navigated(), None);
    }

    #[test]
    fn back_zone_pops() {
        let mut nav = NavStack::new(Text::new("a")).title("Root");
        nav.push(Text::new("b"), "B");
        lay(&mut nav);
        ev(
            &mut nav,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(20.0, 20.0), // ‹ Back zone
                button: PointerButton::Primary,
            },
        );
        assert_eq!(nav.depth(), 1);
        assert_eq!(nav.take_navigated(), Some((1, "Root".to_string())));
    }

    #[test]
    fn escape_pops_when_deep() {
        let mut nav = NavStack::new(Text::new("a"));
        nav.push(Text::new("b"), "B");
        lay(&mut nav);
        ev(
            &mut nav,
            &WidgetEvent::KeyPressed {
                key: "Escape".into(),
                repeat: false,
            },
        );
        assert_eq!(nav.depth(), 1);
    }

    #[test]
    fn escape_at_root_forwards_to_page() {
        let mut nav = NavStack::new(Text::new("a"));
        lay(&mut nav);
        let r = ev(
            &mut nav,
            &WidgetEvent::KeyPressed {
                key: "Escape".into(),
                repeat: false,
            },
        );
        // Root page (Text) ignores it — nothing swallowed by the stack.
        assert_eq!(r, EventResponse::Ignored);
    }

    #[test]
    fn pop_to_root_truncates() {
        let mut nav = NavStack::new(Text::new("a")).title("Root");
        nav.push(Text::new("b"), "B");
        nav.push(Text::new("c"), "C");
        nav.pop_to_root();
        assert_eq!(nav.depth(), 1);
        assert_eq!(nav.current_title(), "Root");
    }

    #[test]
    fn disabled_is_inert() {
        let mut nav = NavStack::new(Text::new("a")).enabled(false);
        nav.push(Text::new("b"), "B");
        lay(&mut nav);
        let r = ev(
            &mut nav,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(20.0, 20.0),
                button: PointerButton::Primary,
            },
        );
        assert_eq!(r, EventResponse::Ignored);
        assert_eq!(nav.depth(), 2);
    }
}
