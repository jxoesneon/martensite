//! `Anchor` — scroll-spy navigation rail.
//!
//! The Ant `Anchor` / docs-site "on this page" rail: a vertical list of
//! section links where exactly one is *active* — the section the user
//! has scrolled to. Clicking a link parks its index in
//! [`Anchor::take_clicked`] (the caller performs the actual scroll);
//! the app reports scroll position back via [`Anchor::set_active`].
//!
//! Nested sections render indented under their parent. `affix`-style
//! geometry is the caller's job — `Anchor` is just the link list.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::anchor::{Anchor, AnchorItem};
//!
//! let mut a = Anchor::new().items([
//!     AnchorItem::new("Overview", "overview"),
//!     AnchorItem::new("API", "api"),
//! ]);
//! a.set_active(1);
//! assert_eq!(a.active(), Some(1));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

/// Active-link ink and rail indicator.
const ACCENT: TokenKey = TokenKey::AccentColor;
/// Inactive link ink.
const TEXT: TokenKey = TokenKey::TextColor;
/// Rail line.
const RAIL: TokenKey = TokenKey::DividerColor;
/// Hovered link ink.
const HOVER: TokenKey = TokenKey::PrimaryColor;

/// Row height per link (logical points).
const ROW_H: f32 = 28.0;
/// Indent per nesting level (logical points).
const INDENT: f32 = 16.0;
/// Rail stroke width (logical points).
const RAIL_W: f32 = 2.0;
/// Indicator width over the rail (logical points).
const IND_W: f32 = 3.0;

/// One entry in an [`Anchor`] rail.
///
/// `title` renders as the link text; `target` is the opaque id the
/// caller maps to a scroll position (a DOM-id analogue). `depth`
/// indents nested sections (`0` = top level).
///
/// # Examples
///
/// ```
/// use martensite::widgets::anchor::AnchorItem;
///
/// let item = AnchorItem::new("Install", "install").depth(1);
/// assert_eq!(item.target, "install");
/// ```
#[derive(Clone)]
pub struct AnchorItem {
    /// Link text.
    pub title: String,
    /// Opaque scroll target id.
    pub target: String,
    /// Nesting depth (`0` = top level).
    pub depth: u32,
}

impl AnchorItem {
    /// A top-level item.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::anchor::AnchorItem;
    ///
    /// let item = AnchorItem::new("Usage", "usage");
    /// assert_eq!(item.depth, 0);
    /// ```
    pub fn new(title: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            target: target.into(),
            depth: 0,
        }
    }

    /// Set the nesting depth (indent level).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::anchor::AnchorItem;
    ///
    /// assert_eq!(AnchorItem::new("a", "a").depth(2).depth, 2);
    /// ```
    pub fn depth(mut self, depth: u32) -> Self {
        self.depth = depth;
        self
    }
}

/// A scroll-spy link rail — see the module docs.
///
/// `Anchor` is a leaf widget; it paints its own links and the rail
/// indicator and reports no children.
///
/// # Examples
///
/// ```
/// use martensite::widgets::anchor::Anchor;
/// use martensite::core::Widget;
///
/// let mut a = Anchor::new();
/// assert_eq!(a.child_count(), 0);
/// ```
pub struct Anchor {
    label: String,
    enabled: bool,
    items: Vec<AnchorItem>,
    /// The active (spy) index.
    active: Option<usize>,
    /// Hovered index.
    hover: Option<usize>,
    /// Parked click for `take_clicked` — `(index, target)`.
    pending: Option<(usize, String)>,
    /// Row rects from the last layout, widget-local.
    rows: Vec<Rect>,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Anchor {
    /// An empty rail.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::anchor::Anchor;
    ///
    /// let a = Anchor::new();
    /// assert_eq!(a.active(), None);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Anchor".into(),
            enabled: true,
            items: Vec::new(),
            active: None,
            hover: None,
            pending: None,
            rows: Vec::new(),
            text_painter: None,
        }
    }

    /// Install the link items.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::anchor::{Anchor, AnchorItem};
    ///
    /// let a = Anchor::new().items([AnchorItem::new("Top", "top")]);
    /// ```
    pub fn items(mut self, items: impl IntoIterator<Item = AnchorItem>) -> Self {
        self.items = items.into_iter().collect();
        self
    }

    /// Set the accessibility label (default `"Anchor"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::anchor::Anchor;
    ///
    /// let a = Anchor::new().label("On this page");
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
    /// use martensite::widgets::anchor::Anchor;
    ///
    /// let a = Anchor::new().enabled(false);
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

    /// The active (scrolled-to) index.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::anchor::Anchor;
    ///
    /// assert_eq!(Anchor::new().active(), None);
    /// ```
    pub fn active(&self) -> Option<usize> {
        self.active
    }

    /// Mark `index` active — the app calls this from its scroll
    /// handler (clamped to the item range; `None` clears).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::anchor::{Anchor, AnchorItem};
    ///
    /// let mut a = Anchor::new().items([AnchorItem::new("a", "a")]);
    /// a.set_active(0);
    /// assert_eq!(a.active(), Some(0));
    /// ```
    pub fn set_active(&mut self, index: usize) {
        self.active = (index < self.items.len()).then_some(index);
    }

    /// Clear the active marker.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::anchor::{Anchor, AnchorItem};
    ///
    /// let mut a = Anchor::new().items([AnchorItem::new("a", "a")]);
    /// a.set_active(0);
    /// a.clear_active();
    /// assert_eq!(a.active(), None);
    /// ```
    pub fn clear_active(&mut self) {
        self.active = None;
    }

    /// Drain the parked click — `(item index, target)` — one-shot.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::anchor::Anchor;
    ///
    /// let mut a = Anchor::new();
    /// assert_eq!(a.take_clicked(), None);
    /// ```
    pub fn take_clicked(&mut self) -> Option<(usize, String)> {
        self.pending.take()
    }

    /// The item at `index`, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::anchor::{Anchor, AnchorItem};
    ///
    /// let a = Anchor::new().items([AnchorItem::new("T", "t")]);
    /// assert_eq!(a.item(0).unwrap().title, "T");
    /// ```
    pub fn item(&self, index: usize) -> Option<&AnchorItem> {
        self.items.get(index)
    }
}

impl Default for Anchor {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Anchor {
    fn measure(&mut self, _cx: &mut LayoutContext, _constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(180.0, self.items.len() as f32 * ROW_H + 8.0)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let row_h = cx.pt(ROW_H);
        self.rows = (0..self.items.len())
            .map(|i| Rect::new(0.0, i as f32 * row_h, bounds.width(), row_h))
            .collect();
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let accent = cx.color(ACCENT, [50, 115, 230, 255]);
        let ink = cx.color(TEXT, [60, 60, 68, 255]);
        let rail = cx.color(RAIL, [215, 217, 222, 255]);
        let hover_ink = cx.color(HOVER, [40, 95, 200, 255]);
        let rail_w = cx.pt(RAIL_W);
        let indent = cx.pt(INDENT);
        let size = 13.0 * cx.scale;

        // The rail — a vertical hairline the indicator rides on.
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.min_y()),
                f64::from(b.min_x() + rail_w),
                f64::from(b.min_y() + self.items.len() as f32 * cx.pt(ROW_H)),
            ),
            rail,
        );

        for (i, item) in self.items.iter().enumerate() {
            let Some(row) = self.rows.get(i).copied() else {
                continue;
            };
            let x0 = b.min_x() + row.min_x() + rail_w + cx.pt(6.0) + item.depth as f32 * indent;
            let clip = kurbo::Rect::new(
                f64::from(x0),
                f64::from(b.min_y() + row.min_y()),
                f64::from(b.min_x() + row.max_x()),
                f64::from(b.min_y() + row.max_y()),
            );
            let is_active = self.active == Some(i);
            let is_hover = self.enabled && self.hover == Some(i);
            let color = if is_active || is_hover {
                if is_active {
                    accent
                } else {
                    hover_ink
                }
            } else if self.enabled {
                ink
            } else {
                cx.color(TokenKey::TextMutedColor, [150, 150, 158, 255])
            };
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(clip.x0, clip.y0 + clip.height() * 0.72),
                &item.title,
                size,
                color,
            );
            if is_active {
                // The indicator pill overlapping the rail.
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(b.min_x()),
                        f64::from(b.min_y() + row.min_y() + row.height() * 0.5 - cx.pt(8.0)),
                        f64::from(b.min_x() + cx.pt(IND_W)),
                        f64::from(b.min_y() + row.min_y() + row.height() * 0.5 + cx.pt(8.0)),
                    ),
                    accent,
                );
            }
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let local = *position - cx.bounds.origin;
                let hit = self.rows.iter().position(|r| r.contains(local));
                if hit != self.hover {
                    self.hover = hit;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerReleased { position, button }
                if *button == martensite_core::PointerButton::Primary =>
            {
                let local = *position - cx.bounds.origin;
                if let Some(i) = self.rows.iter().position(|r| r.contains(local)) {
                    self.active = Some(i);
                    self.pending = Some((i, self.items[i].target.clone()));
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hover.take().is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Navigation);
        node.set_label(self.label.as_str());
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, ROW_H)).with_policy(UnderflowPolicy::Lint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PointerButton};

    fn rail(items: usize) -> Anchor {
        Anchor::new().items((0..items).map(|i| AnchorItem::new(format!("S{i}"), format!("s{i}"))))
    }

    fn lay(w: &mut Anchor) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, 180.0, 300.0));
    }

    fn ev(w: &mut Anchor, e: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: e,
            bounds: Rect::new(0.0, 0.0, 180.0, 300.0),
            scale: 1.0,
        };
        w.event(&mut cx)
    }

    #[test]
    fn rows_stack_by_item() {
        let mut a = rail(3);
        lay(&mut a);
        assert_eq!(a.rows.len(), 3);
        assert!((a.rows[1].min_y() - ROW_H).abs() < 0.01);
    }

    #[test]
    fn click_selects_and_parks_target() {
        let mut a = rail(3);
        lay(&mut a);
        ev(
            &mut a,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(40.0, ROW_H * 1.5),
                button: PointerButton::Primary,
            },
        );
        assert_eq!(a.active(), Some(1));
        assert_eq!(a.take_clicked(), Some((1, "s1".to_string())));
        assert_eq!(a.take_clicked(), None);
    }

    #[test]
    fn set_active_clamps_to_range() {
        let mut a = rail(2);
        a.set_active(5);
        assert_eq!(a.active(), None);
        a.set_active(1);
        assert_eq!(a.active(), Some(1));
    }

    #[test]
    fn hover_tracks_pointer() {
        let mut a = rail(2);
        lay(&mut a);
        ev(
            &mut a,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(30.0, ROW_H * 0.5),
            },
        );
        assert_eq!(a.hover, Some(0));
        ev(&mut a, &WidgetEvent::PointerLeave);
        assert_eq!(a.hover, None);
    }

    #[test]
    fn disabled_ignores_clicks() {
        let mut a = rail(2).enabled(false);
        lay(&mut a);
        let r = ev(
            &mut a,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(30.0, ROW_H * 0.5),
                button: PointerButton::Primary,
            },
        );
        assert_eq!(r, EventResponse::Ignored);
        assert_eq!(a.take_clicked(), None);
    }

    #[test]
    fn depth_indents_items() {
        let a = Anchor::new().items([AnchorItem::new("x", "x").depth(2)]);
        assert_eq!(a.item(0).unwrap().depth, 2);
    }
}
