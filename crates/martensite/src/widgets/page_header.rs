//! `PageHeader` — page-top bar with back, title, and actions.
//!
//! The Ant `PageHeader` pattern: an optional back chevron (parks
//! [`PageHeader::take_back`] — the shell owns navigation), a title
//! with optional subtitle, and a trailing action slot for buttons.
//! Unlike [`HeaderBar`](crate::widgets::header_bar::HeaderBar)
//! (centered window title, symmetric slots), a page header is
//! left-anchored content chrome inside a page, not the window's
//! title area.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::page_header::PageHeader;
//! use martensite::widgets::Button;
//!
//! let h = PageHeader::new("Invoice #1042")
//!     .back(true)
//!     .subtitle("Draft")
//!     .action(Button::new("Send"));
//! assert_eq!(h.title(), "Invoice #1042");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

/// Title ink.
const TITLE_INK: [u8; 4] = [32, 33, 36, 255];
/// Subtitle ink.
const SUB_INK: [u8; 4] = [110, 114, 123, 255];
/// Chevron ink.
const CHEVRON_INK: TokenKey = TokenKey::TextColor;
/// Divider ink.
const DIVIDER: TokenKey = TokenKey::DividerColor;
/// Horizontal padding (logical points).
const PAD_PT: f32 = 16.0;
/// Title font size (logical points).
const TITLE_PT: f32 = 17.0;
/// Subtitle font size (logical points).
const SUB_PT: f32 = 12.0;
/// Back-chevron hit zone width (logical points).
const BACK_PT: f32 = 28.0;
/// Action-slot gap (logical points).
const ACTION_GAP_PT: f32 = 8.0;
/// Default height (logical points).
const HEIGHT_PT: f32 = 56.0;

/// A page-top bar — see the module docs. Children = action slot.
///
/// # Examples
///
/// ```
/// use martensite::widgets::page_header::PageHeader;
/// use martensite::widgets::Button;
/// use martensite::core::Widget;
///
/// let h = PageHeader::new("T").action(Button::new("Save"));
/// assert_eq!(h.child_count(), 1);
/// ```
pub struct PageHeader {
    title: String,
    subtitle: Option<String>,
    /// Whether the back chevron shows.
    show_back: bool,
    /// Back chevron armed/hover.
    back_armed: bool,
    back_hover: bool,
    /// Parked back activation.
    back_pending: bool,
    actions: Vec<Box<dyn Widget>>,
    /// Whether the bottom divider paints (default `true`).
    pub show_divider: bool,
    /// Cached action bounds (widget-local).
    action_rects: Vec<Rect>,
    /// Back-chevron rect (widget-local).
    back_rect: Rect,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl PageHeader {
    /// A header with `title`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::page_header::PageHeader;
    ///
    /// assert_eq!(PageHeader::new("Orders").title(), "Orders");
    /// ```
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            show_back: false,
            back_armed: false,
            back_hover: false,
            back_pending: false,
            actions: Vec::new(),
            show_divider: true,
            action_rects: Vec::new(),
            back_rect: Rect::default(),
            text_painter: None,
        }
    }

    /// Shows or hides the back chevron (default `false`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::page_header::PageHeader;
    ///
    /// let h = PageHeader::new("T").back(true);
    /// ```
    #[must_use]
    pub fn back(mut self, show: bool) -> Self {
        self.show_back = show;
        self
    }

    /// Sets the subtitle line.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::page_header::PageHeader;
    ///
    /// let h = PageHeader::new("T").subtitle("3 items");
    /// ```
    #[must_use]
    pub fn subtitle(mut self, text: impl Into<String>) -> Self {
        self.subtitle = Some(text.into());
        self
    }

    /// Appends a trailing action child.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::page_header::PageHeader;
    /// use martensite::widgets::Button;
    ///
    /// assert_eq!(PageHeader::new("T").action(Button::new("x")).action_count(), 1);
    /// ```
    #[must_use]
    pub fn action(mut self, child: impl Widget + 'static) -> Self {
        self.actions.push(Box::new(child));
        self
    }

    /// Overrides the shaped-text painter (tests and tooling).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::page_header::PageHeader;
    ///
    /// let h = PageHeader::new("T");
    /// ```
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The title text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::page_header::PageHeader;
    ///
    /// assert_eq!(PageHeader::new("T").title(), "T");
    /// ```
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The subtitle, if set.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::page_header::PageHeader;
    ///
    /// assert_eq!(PageHeader::new("T").subtitle_text(), None);
    /// ```
    pub fn subtitle_text(&self) -> Option<&str> {
        self.subtitle.as_deref()
    }

    /// Action-slot child count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::page_header::PageHeader;
    ///
    /// assert_eq!(PageHeader::new("T").action_count(), 0);
    /// ```
    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// Whether the back chevron shows.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::page_header::PageHeader;
    ///
    /// assert!(!PageHeader::new("T").has_back());
    /// ```
    pub fn has_back(&self) -> bool {
        self.show_back
    }

    /// Drains the parked back activation — one-shot.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::page_header::PageHeader;
    ///
    /// let mut h = PageHeader::new("T");
    /// assert!(!h.take_back());
    /// ```
    pub fn take_back(&mut self) -> bool {
        std::mem::take(&mut self.back_pending)
    }
}

impl Widget for PageHeader {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut max_child_h = 0.0f32;
        for a in &mut self.actions {
            max_child_h = max_child_h.max(a.measure(cx, constraints).y);
        }
        let text_h = if self.subtitle.is_some() {
            TITLE_PT + SUB_PT + 3.0
        } else {
            TITLE_PT
        };
        Vec2::new(
            constraints
                .max_size
                .x
                .min(constraints.max_size.x)
                .max(160.0),
            (max_child_h + PAD_PT).max(text_h + PAD_PT).max(HEIGHT_PT),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let pad = cx.pt(PAD_PT);
        let gap = cx.pt(ACTION_GAP_PT);
        let mid_y = bounds.min_y() + bounds.height() / 2.0;

        self.back_rect = if self.show_back {
            Rect::new(
                bounds.min_x(),
                bounds.min_y(),
                cx.pt(BACK_PT),
                bounds.height(),
            )
        } else {
            Rect::default()
        };

        // Actions — right to left at natural size.
        let mut x = bounds.max_x() - pad;
        self.action_rects = self
            .actions
            .iter_mut()
            .rev()
            .map(|a| {
                let s = a.measure(
                    cx,
                    LayoutConstraints {
                        min_size: Vec2::ZERO,
                        max_size: bounds.size,
                    },
                );
                let r = Rect::new(x - s.x, mid_y - s.y / 2.0, s.x, s.y);
                cx.layout_child(a.as_mut(), r);
                x -= s.x + gap;
                r
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = cx.bounds;
        let ink = cx.color(CHEVRON_INK, TITLE_INK);
        let pad = cx.pt(PAD_PT);

        // Back chevron.
        let mut x = b.min_x() + pad;
        if self.show_back {
            if self.back_armed || self.back_hover {
                let r = kurbo::Rect::new(
                    f64::from(b.min_x() + self.back_rect.min_x()),
                    f64::from(b.min_y() + self.back_rect.min_y()),
                    f64::from(b.min_x() + self.back_rect.max_x()),
                    f64::from(b.min_y() + self.back_rect.max_y()),
                );
                cx.list.push_fill_shape(
                    r,
                    &martensite_core::shape::Shape::rounded(cx.pt(6.0)),
                    [30, 31, 36, 14],
                );
            }
            let cy = f64::from(b.min_y() + b.height() / 2.0);
            let tip = f64::from(x);
            let chev = kurbo::BezPath::from_vec(vec![
                kurbo::PathEl::MoveTo(kurbo::Point::new(tip + cx.ptf(7.0), cy - cx.ptf(6.0))),
                kurbo::PathEl::LineTo(kurbo::Point::new(tip, cy)),
                kurbo::PathEl::LineTo(kurbo::Point::new(tip + cx.ptf(7.0), cy + cx.ptf(6.0))),
            ]);
            cx.list.push_stroke_path(chev, cx.pt(1.8), ink);
            x += cx.pt(BACK_PT);
        }

        // Title + subtitle, clipped left of the action slot.
        let title_size = cx.pt(TITLE_PT);
        let text_right = self
            .action_rects
            .first()
            .map(|r| b.min_x() + r.min_x() - cx.pt(ACTION_GAP_PT))
            .unwrap_or(b.max_x() - pad);
        let clip = kurbo::Rect::new(
            f64::from(x),
            f64::from(b.min_y()),
            f64::from(text_right),
            f64::from(b.max_y()),
        );
        if let Some(sub) = &self.subtitle {
            let sub_size = cx.pt(SUB_PT);
            let total = title_size + cx.pt(3.0) + sub_size;
            let ty = b.min_y() + (b.height() - total) / 2.0;
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(f64::from(x), f64::from(ty)),
                &self.title,
                title_size,
                cx.color(TokenKey::TextColor, TITLE_INK),
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(f64::from(x), f64::from(ty + title_size + cx.pt(3.0))),
                sub,
                sub_size,
                cx.color(TokenKey::TextMutedColor, SUB_INK),
            );
        } else {
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(
                    f64::from(x),
                    f64::from(b.min_y() + (b.height() - title_size) / 2.0),
                ),
                &self.title,
                title_size,
                cx.color(TokenKey::TextColor, TITLE_INK),
            );
        }

        if self.show_divider {
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(b.min_x()),
                    f64::from(b.max_y() - cx.pt(1.0)),
                    f64::from(b.max_x()),
                    f64::from(b.max_y()),
                ),
                cx.color(DIVIDER, [208, 211, 217, 255]),
            );
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Back chevron first.
        if self.show_back {
            let back_device = Rect::new(
                cx.bounds.min_x() + self.back_rect.min_x(),
                cx.bounds.min_y() + self.back_rect.min_y(),
                self.back_rect.width(),
                self.back_rect.height(),
            );
            match cx.event {
                WidgetEvent::PointerPressed {
                    position, button, ..
                } if *button == martensite_core::PointerButton::Primary
                    && back_device.contains(*position) =>
                {
                    self.back_armed = true;
                    return EventResponse::RequestRepaint;
                }
                WidgetEvent::PointerReleased { position, button }
                    if *button == martensite_core::PointerButton::Primary && self.back_armed =>
                {
                    self.back_armed = false;
                    if back_device.contains(*position) {
                        self.back_pending = true;
                    }
                    return EventResponse::RequestRepaint;
                }
                WidgetEvent::PointerMoved { position } => {
                    let inside = back_device.contains(*position);
                    if inside != self.back_hover {
                        self.back_hover = inside;
                        // Fall through to actions too — they may hover.
                    }
                }
                _ => {}
            }
        }
        // Forward to the hit action child (bounds-gated, topmost).
        if let WidgetEvent::PointerPressed { position, .. }
        | WidgetEvent::PointerReleased { position, .. }
        | WidgetEvent::PointerMoved { position }
        | WidgetEvent::Scroll { position, .. } = cx.event
        {
            let pos = *position;
            for i in (0..self.actions.len()).rev() {
                let Some(b) = self.action_rects.get(i).copied() else {
                    continue;
                };
                let device = Rect::new(
                    cx.bounds.min_x() + b.min_x(),
                    cx.bounds.min_y() + b.min_y(),
                    b.width(),
                    b.height(),
                );
                if !device.contains(pos) {
                    continue;
                }
                let mut child_cx = EventContext {
                    event: cx.event,
                    bounds: device,
                    scale: cx.scale,
                };
                if let Some(child) = self.actions.get_mut(i) {
                    return child.event(&mut child_cx);
                }
            }
        }
        EventResponse::Ignored
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Toolbar);
        node.set_label(self.title.clone());
    }

    fn child_count(&self) -> usize {
        self.actions.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.actions.get(index).map(|c| c.as_ref())
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.actions.get_mut(index).map(|c| c.as_mut())
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.action_rects.get(index).copied()
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }
}

impl std::fmt::Debug for PageHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageHeader")
            .field("title", &self.title)
            .field("back", &self.show_back)
            .field("actions", &self.actions.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::button::Button;
    use martensite_core::{HotNode, PointerButton};

    fn lay(h: &mut PageHeader, w: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        h.layout(&mut cx, Rect::new(0.0, 0.0, w, 56.0));
    }

    fn ev(h: &mut PageHeader, e: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: e,
            bounds: Rect::new(0.0, 0.0, 400.0, 56.0),
            scale: 1.0,
        };
        h.event(&mut cx)
    }

    #[test]
    fn back_chevron_parks_activation() {
        let mut h = PageHeader::new("T").back(true);
        lay(&mut h, 400.0);
        ev(
            &mut h,
            &WidgetEvent::PointerPressed {
                position: Vec2::new(10.0, 28.0),
                button: PointerButton::Primary,
                count: 1,
            },
        );
        ev(
            &mut h,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(10.0, 28.0),
                button: PointerButton::Primary,
            },
        );
        assert!(h.take_back());
        assert!(!h.take_back());
    }

    #[test]
    fn no_back_means_left_press_is_ignored() {
        let mut h = PageHeader::new("T");
        lay(&mut h, 400.0);
        let r = ev(
            &mut h,
            &WidgetEvent::PointerPressed {
                position: Vec2::new(4.0, 28.0),
                button: PointerButton::Primary,
                count: 1,
            },
        );
        assert_eq!(r, EventResponse::Ignored);
    }

    #[test]
    fn actions_right_align_and_forward() {
        let mut h = PageHeader::new("T").action(Button::new("Save"));
        lay(&mut h, 400.0);
        let r = h.child_bounds(0).unwrap();
        assert!(r.max_x() <= 400.0 - PAD_PT + 0.5);
        assert!(r.max_x() > 280.0);
        // A press inside the action forwards to the Button.
        let resp = ev(
            &mut h,
            &WidgetEvent::PointerPressed {
                position: Vec2::new(r.min_x() + 4.0, r.min_y() + 4.0),
                button: PointerButton::Primary,
                count: 1,
            },
        );
        assert_ne!(resp, EventResponse::Ignored);
    }

    #[test]
    fn actions_keep_append_order_left_to_right() {
        let mut h = PageHeader::new("T")
            .action(Button::new("A"))
            .action(Button::new("B"));
        lay(&mut h, 400.0);
        let a = h.child_bounds(0).unwrap();
        let b = h.child_bounds(1).unwrap();
        assert!(a.min_x() < b.min_x());
    }
}
