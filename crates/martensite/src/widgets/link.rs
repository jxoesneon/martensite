//! `Link` — inline hyperlink label.
//!
//! The Ant `Typography.Link` / GTK `LinkButton` / WPF `Hyperlink`
//! pattern: a single underlined text run that looks and acts like a
//! web link — accent ink, pointer cursor affordance via hover
//! emphasis, activation on click and `Enter`/`Space`. Unlike
//! [`Anchor`](crate::widgets::anchor::Anchor) (a scroll-spy rail)
//! or a `Markdown` inline link, `Link` is a standalone control.
//!
//! The widget does **not** open URLs itself — the shell owns
//! navigation. Activation parks the target in
//! [`Link::take_activated`] for the app to handle (open in browser,
//! route internally, copy).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::link::Link;
//!
//! let mut l = Link::new("Documentation").target("https://docs.rs");
//! assert_eq!(l.href(), Some("https://docs.rs"));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

/// Link ink.
const INK: TokenKey = TokenKey::AccentColor;
/// Disabled ink.
const DISABLED_INK: [u8; 4] = [150, 152, 158, 255];
/// Visited ink — purple-ish, the classic visited-link cue.
const VISITED_INK: [u8; 4] = [128, 90, 180, 255];
/// Fallback link ink.
const FALLBACK_INK: [u8; 4] = [50, 115, 230, 255];
/// Label font size (logical points).
const FONT_PT: f32 = 13.0;

/// An inline hyperlink — see the module docs. Leaf widget.
///
/// # Examples
///
/// ```
/// use martensite::widgets::link::Link;
/// use martensite::core::Widget;
///
/// assert_eq!(Link::new("x").child_count(), 0);
/// ```
pub struct Link {
    text: String,
    target: Option<String>,
    enabled: bool,
    /// Press-armed flag (press inside → release inside activates).
    armed: bool,
    /// Hover flag.
    hover: bool,
    /// Keyboard-focus flag.
    focused: bool,
    /// Whether the link shows visited styling.
    visited: bool,
    /// Parked activation for `take_activated`.
    pending: Option<String>,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Link {
    /// A link showing `text`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::link::Link;
    ///
    /// let l = Link::new("Read more");
    /// assert_eq!(l.text(), "Read more");
    /// ```
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            target: None,
            enabled: true,
            armed: false,
            hover: false,
            focused: false,
            visited: false,
            pending: None,
            text_painter: None,
        }
    }

    /// Sets the activation target (URL, route, id — opaque to the
    /// widget).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::link::Link;
    ///
    /// let l = Link::new("Docs").target("https://example.com");
    /// assert_eq!(l.href(), Some("https://example.com"));
    /// ```
    #[must_use]
    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    /// Enable or disable the link (default `true`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::link::Link;
    ///
    /// let l = Link::new("x").enabled(false);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets visited styling (default `false`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::link::Link;
    ///
    /// let l = Link::new("x").visited(true);
    /// assert!(l.is_visited());
    /// ```
    #[must_use]
    pub fn visited(mut self, visited: bool) -> Self {
        self.visited = visited;
        self
    }

    /// Marks the link visited after activation — call from the
    /// activation handler.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::link::Link;
    ///
    /// let mut l = Link::new("x");
    /// l.mark_visited();
    /// assert!(l.is_visited());
    /// ```
    pub fn mark_visited(&mut self) {
        self.visited = true;
    }

    /// Overrides the shaped-text painter (tests and tooling).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::link::Link;
    ///
    /// let l = Link::new("x");
    /// ```
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The link text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::link::Link;
    ///
    /// assert_eq!(Link::new("go").text(), "go");
    /// ```
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The activation target, if set.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::link::Link;
    ///
    /// assert_eq!(Link::new("x").href(), None);
    /// ```
    pub fn href(&self) -> Option<&str> {
        self.target.as_deref()
    }

    /// Whether visited styling is shown.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::link::Link;
    ///
    /// assert!(!Link::new("x").is_visited());
    /// ```
    pub fn is_visited(&self) -> bool {
        self.visited
    }

    /// Drains the parked activation target — one-shot.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::link::Link;
    ///
    /// let mut l = Link::new("x");
    /// assert_eq!(l.take_activated(), None);
    /// ```
    pub fn take_activated(&mut self) -> Option<String> {
        self.pending.take()
    }

    /// Shared activate path — parks the target (or the text when no
    /// explicit target was set).
    fn activate(&mut self) {
        self.pending = Some(self.target.clone().unwrap_or_else(|| self.text.clone()));
    }
}

impl Widget for Link {
    fn measure(&mut self, cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        let size = cx.pt(FONT_PT);
        let painter = crate::text_paint::resolve_painter(&self.text_painter, None);
        let w = painter
            .and_then(|p| p.measure_text(&self.text, size))
            .unwrap_or(size * self.text.len() as f32 * 0.55);
        Vec2::new(w.max(cx.pt(8.0)), size * 1.4)
    }

    fn layout(&mut self, cx: &mut LayoutContext, _bounds: Rect) {
        cx.hot.flags |= martensite_core::NodeFlags::FOCUSABLE;
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = cx.bounds;
        let size = cx.pt(FONT_PT);
        let ink = if !self.enabled {
            DISABLED_INK
        } else if self.visited {
            VISITED_INK
        } else {
            cx.color(INK, FALLBACK_INK)
        };
        let w = painter
            .and_then(|p| p.measure_text(&self.text, size))
            .unwrap_or(size * self.text.len() as f32 * 0.55)
            .min(b.width());
        let x = b.min_x();
        let y = b.min_y() + (b.height() - size) / 2.0;
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            kurbo::Point::new(f64::from(x), f64::from(y)),
            &self.text,
            size,
            ink,
        );
        // Underline — always on for links (accessibility: don't rely
        // on color alone); hover/armed thickens it.
        let thickness = if self.armed || self.focused {
            cx.pt(2.0)
        } else {
            cx.pt(1.0)
        };
        let underline_y = y + size + cx.pt(1.0);
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(x),
                f64::from(underline_y),
                f64::from(x + w),
                f64::from(underline_y + thickness),
            ),
            ink,
        );
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                position, button, ..
            } if *button == martensite_core::PointerButton::Primary
                && cx.bounds.contains(*position) =>
            {
                self.armed = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerReleased { position, button }
                if *button == martensite_core::PointerButton::Primary =>
            {
                let was_armed = self.armed;
                self.armed = false;
                if was_armed && cx.bounds.contains(*position) {
                    self.activate();
                    return EventResponse::RequestRepaint;
                }
                if was_armed {
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                let inside = cx.bounds.contains(*position);
                if inside != self.hover {
                    self.hover = inside;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerLeave => {
                if self.hover || self.armed {
                    self.hover = false;
                    self.armed = false;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                self.armed = false;
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, .. }
                if matches!(key.as_str(), "Enter" | " ") && self.focused =>
            {
                self.activate();
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(martensite_core::SemanticAction::Click) => {
                self.activate();
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Link);
        node.set_label(self.text.clone());
        if let Some(t) = &self.target {
            node.set_value(t.clone());
        }
        node.add_action(accesskit::Action::Click);
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(12.0, FONT_PT)).with_policy(UnderflowPolicy::Lint)
    }
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link")
            .field("text", &self.text)
            .field("target", &self.target)
            .field("enabled", &self.enabled)
            .field("visited", &self.visited)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::PointerButton;

    fn bounds() -> Rect {
        Rect::new(0.0, 0.0, 120.0, 20.0)
    }

    fn ev(l: &mut Link, e: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: e,
            bounds: bounds(),
            scale: 1.0,
        };
        l.event(&mut cx)
    }

    fn press(l: &mut Link) {
        ev(
            l,
            &WidgetEvent::PointerPressed {
                position: Vec2::new(10.0, 10.0),
                button: PointerButton::Primary,
                count: 1,
            },
        );
    }

    #[test]
    fn click_activates_and_parks_target() {
        let mut l = Link::new("Docs").target("docs://x");
        press(&mut l);
        let r = ev(
            &mut l,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(10.0, 10.0),
                button: PointerButton::Primary,
            },
        );
        assert_eq!(r, EventResponse::RequestRepaint);
        assert_eq!(l.take_activated(), Some("docs://x".to_string()));
        assert_eq!(l.take_activated(), None);
    }

    #[test]
    fn release_outside_does_not_activate() {
        let mut l = Link::new("Docs");
        press(&mut l);
        ev(
            &mut l,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(500.0, 10.0),
                button: PointerButton::Primary,
            },
        );
        assert_eq!(l.take_activated(), None);
    }

    #[test]
    fn enter_activates_when_focused() {
        let mut l = Link::new("Docs").target("t");
        ev(&mut l, &WidgetEvent::FocusGained);
        ev(
            &mut l,
            &WidgetEvent::KeyPressed {
                key: "Enter".to_string(),
                repeat: false,
            },
        );
        assert_eq!(l.take_activated(), Some("t".to_string()));
    }

    #[test]
    fn no_target_falls_back_to_text() {
        let mut l = Link::new("SelfRef");
        ev(&mut l, &WidgetEvent::FocusGained);
        ev(
            &mut l,
            &WidgetEvent::KeyPressed {
                key: " ".to_string(),
                repeat: false,
            },
        );
        assert_eq!(l.take_activated(), Some("SelfRef".to_string()));
    }

    #[test]
    fn disabled_is_inert() {
        let mut l = Link::new("x").target("t").enabled(false);
        press(&mut l);
        let r = ev(
            &mut l,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(10.0, 10.0),
                button: PointerButton::Primary,
            },
        );
        assert_eq!(r, EventResponse::Ignored);
        assert_eq!(l.take_activated(), None);
    }

    #[test]
    fn semantic_click_activates() {
        let mut l = Link::new("x").target("at");
        ev(
            &mut l,
            &WidgetEvent::SemanticAction(martensite_core::SemanticAction::Click),
        );
        assert_eq!(l.take_activated(), Some("at".to_string()));
    }
}
