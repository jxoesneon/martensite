//! `GroupBox` widget: a titled frame around related content
//! (QGroupBox / GTK `Frame` / WinForms `GroupBox`).
//!
//! The classic frame look: a rounded border whose top edge is broken
//! by the title text. In checkable mode the title becomes a
//! [`CheckBox`] internal child — unchecking it dims the content and
//! blocks events to it, matching `QGroupBox::setCheckable` semantics.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::group_box::GroupBox;
//! use martensite::widgets::text::Text;
//!
//! let gb = GroupBox::new("Network").child(Text::new("settings"));
//! assert_eq!(gb.title, "Network");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Widget,
};
use martensite_core::{Rect, TokenKey};
use martensite_layout::geometry::EdgeInsets;

use crate::widgets::checkbox::CheckBox;

const INK: [u8; 4] = [20, 20, 25, 255];
const EDGE: [u8; 4] = [140, 145, 155, 255];
const PAGE: [u8; 4] = [245, 245, 247, 255];
/// Title band height in logical points — the frame's top edge runs
/// through its vertical middle so the label straddles the border.
const TITLE_H: f32 = 22.0;
/// Interior inset between the frame border and the content (logical
/// points), before user padding.
const FRAME_INSET: f32 = 12.0;
/// Horizontal breathing room painted behind the title so the frame's
/// top edge reads as interrupted (logical points).
const TITLE_GAP_PAD: f32 = 4.0;
/// Title text size in logical points.
const TITLE_SIZE: f32 = 14.0;

/// A titled, framed container grouping related controls.
///
/// With [`GroupBox::checkable`] the title becomes a real [`CheckBox`]
/// child: while unchecked the content is painted dimmed and receives
/// no events (Qt checkable-group-box behaviour).
///
/// # Examples
///
/// ```
/// use martensite::widgets::GroupBox;
/// use martensite_core::widget::DummyWidget;
///
/// let gb = GroupBox::new("Options")
///     .checkable(true)
///     .checked(true)
///     .child(DummyWidget);
/// assert!(gb.is_checked());
/// ```
pub struct GroupBox {
    /// The title drawn in the top-edge gap.
    pub title: String,
    /// Whether the title carries a [`CheckBox`] that enables the
    /// content.
    pub checkable: bool,
    /// The title checkbox — created by `checkable(true)`.
    pub checkbox: Option<CheckBox>,
    /// Padding inside the frame, before the content.
    pub padding: EdgeInsets,
    /// The grouped content widget.
    pub child: Option<Box<dyn Widget>>,
    /// Cached checkbox size from the last measure pass.
    checkbox_size: Vec2,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Cached frame (border) rect — starts half a title-band down so
    /// the title straddles the top edge.
    frame_rect: Rect,
    /// Cached title band rect (text or checkbox zone).
    title_rect: Rect,
    /// Cached checkbox rect (device px).
    checkbox_rect: Rect,
    /// Cached content rect (device px).
    content_rect: Rect,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl GroupBox {
    /// A group box with the given title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::GroupBox;
    ///
    /// let gb = GroupBox::new("Audio");
    /// assert_eq!(gb.title, "Audio");
    /// assert!(!gb.checkable);
    /// ```
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            checkable: false,
            checkbox: None,
            padding: EdgeInsets::default(),
            child: None,
            checkbox_size: Vec2::ZERO,
            cached_bounds: Rect::default(),
            frame_rect: Rect::default(),
            title_rect: Rect::default(),
            checkbox_rect: Rect::default(),
            content_rect: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the content widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Container, GroupBox};
    ///
    /// let gb = GroupBox::new("X").child(Container::new());
    /// assert!(gb.child.is_some());
    /// ```
    #[inline]
    #[must_use]
    pub fn child(mut self, child: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    /// Sets whether the title is a checkbox gating the content.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::GroupBox;
    ///
    /// let gb = GroupBox::new("Enable sync").checkable(true);
    /// assert!(gb.checkbox.is_some());
    /// ```
    #[inline]
    #[must_use]
    pub fn checkable(mut self, checkable: bool) -> Self {
        self.checkable = checkable;
        if checkable && self.checkbox.is_none() {
            self.checkbox = Some(CheckBox::new(self.title.clone()));
        }
        self
    }

    /// Sets the checked state of a checkable group box (no-op for a
    /// plain one).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::GroupBox;
    ///
    /// let gb = GroupBox::new("X").checkable(true).checked(true);
    /// assert!(gb.is_checked());
    /// ```
    #[inline]
    #[must_use]
    pub fn checked(mut self, checked: bool) -> Self {
        if let Some(cb) = &mut self.checkbox {
            cb.set_checked(checked);
        }
        self
    }

    /// Whether the content is enabled — always `true` for a
    /// non-checkable box; otherwise mirrors the title checkbox.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::GroupBox;
    ///
    /// assert!(GroupBox::new("X").is_checked());
    /// assert!(!GroupBox::new("X").checkable(true).is_checked());
    /// ```
    #[inline]
    pub fn is_checked(&self) -> bool {
        self.checkbox.as_ref().is_none_or(|cb| cb.checked)
    }

    /// Programmatically sets the checked state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::GroupBox;
    ///
    /// let mut gb = GroupBox::new("X").checkable(true);
    /// gb.set_checked(true);
    /// assert!(gb.is_checked());
    /// ```
    #[inline]
    pub fn set_checked(&mut self, checked: bool) {
        if let Some(cb) = &mut self.checkbox {
            cb.set_checked(checked);
        }
    }

    /// Sets the padding inside the frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::GroupBox;
    /// use martensite_layout::geometry::EdgeInsets;
    ///
    /// let gb = GroupBox::new("X").padding(EdgeInsets::uniform(8.0));
    /// assert_eq!(gb.padding.left, 8.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    /// Sets uniform padding inside the frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::GroupBox;
    ///
    /// let gb = GroupBox::new("X").padding_uniform(4.0);
    /// assert_eq!(gb.padding.horizontal(), 8.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn padding_uniform(mut self, value: f32) -> Self {
        self.padding = EdgeInsets::uniform(value);
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] for real glyph runs.
    /// Also handed to the title checkbox.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.checkbox = self
            .checkbox
            .take()
            .map(|cb| cb.with_text_painter(painter.clone()));
        self.text_painter = Some(painter);
        self
    }
}

impl Widget for GroupBox {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let title_h = cx.pt(TITLE_H);
        let inset = cx.pt(FRAME_INSET);
        let pad_h = self.padding.horizontal();
        let pad_v = self.padding.vertical();

        self.checkbox_size = if let Some(cb) = &mut self.checkbox {
            cb.measure(cx, constraints)
        } else {
            Vec2::ZERO
        };

        let content_size = if let Some(child) = &mut self.child {
            child.measure(
                cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: Vec2::new(
                        (constraints.max_size.x - inset * 2.0 - pad_h).max(0.0),
                        (constraints.max_size.y - title_h - inset * 2.0 - pad_v).max(0.0),
                    ),
                },
            )
        } else {
            Vec2::ZERO
        };

        let w = if constraints.max_size.x.is_finite() {
            constraints.max_size.x.max(0.0)
        } else {
            (content_size.x + inset * 2.0 + pad_h)
                .max(self.checkbox_size.x + inset * 2.0 + cx.pt(TITLE_GAP_PAD) * 2.0)
        };
        let h = title_h + inset * 2.0 + pad_v + content_size.y;
        Vec2::new(w, h.min(constraints.max_size.y.max(0.0)))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        let title_h = cx.pt(TITLE_H).min(bounds.size.y);
        let inset = cx.pt(FRAME_INSET);

        // The frame's top edge runs through the middle of the title
        // band — the title straddles the border, classic frame style.
        self.frame_rect = Rect::new(
            bounds.origin.x,
            bounds.origin.y + title_h / 2.0,
            bounds.size.x,
            (bounds.size.y - title_h / 2.0).max(0.0),
        );
        self.title_rect = Rect::new(
            bounds.origin.x + inset,
            bounds.origin.y,
            (bounds.size.x - inset * 2.0).max(0.0),
            title_h,
        );

        if let Some(cb) = &mut self.checkbox {
            cb.label.clone_from(&self.title);
            let w = self.checkbox_size.x.min(self.title_rect.size.x);
            let h = self.checkbox_size.y.min(title_h);
            self.checkbox_rect = Rect::new(self.title_rect.origin.x, bounds.origin.y, w, h);
            cx.layout_child(cb, self.checkbox_rect);
        } else {
            self.checkbox_rect = Rect::default();
        }

        let pad = &self.padding;
        self.content_rect = Rect::new(
            self.frame_rect.origin.x + inset + pad.left,
            self.frame_rect.origin.y + inset + pad.top,
            (self.frame_rect.size.x - inset * 2.0 - pad.horizontal()).max(0.0),
            (self.frame_rect.size.y - inset * 2.0 - pad.vertical()).max(0.0),
        );
        if let Some(child) = &mut self.child {
            cx.layout_child(child.as_mut(), self.content_rect);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.title.as_str());
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Checkable + unchecked → only the title checkbox is live;
        // the content child is inert (Qt behaviour).
        if self.checkable && !self.is_checked() {
            if let Some(cb) = &mut self.checkbox {
                if let Some(pos) = cx.event.position() {
                    if !self.checkbox_rect.contains(pos) {
                        return EventResponse::Ignored;
                    }
                }
                let mut child_cx = EventContext {
                    event: cx.event,
                    bounds: self.checkbox_rect,
                    scale: cx.scale,
                };
                return cb.event(&mut child_cx);
            }
            return EventResponse::Ignored;
        }

        // Default forwarding: children topmost-first, bounds-gated.
        let n = self.child_count();
        for i in (0..n).rev() {
            let Some(child_bounds) = self.child_bounds(i) else {
                continue;
            };
            if let Some(pos) = cx.event.position() {
                if !child_bounds.contains(pos) {
                    continue;
                }
            }
            let Some(child) = self.child_mut(i) else {
                continue;
            };
            let mut child_cx = EventContext {
                event: cx.event,
                bounds: child_bounds,
                scale: cx.scale,
            };
            match child.event(&mut child_cx) {
                EventResponse::Ignored => continue,
                response => return response,
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let f = &self.frame_rect;
        let frame = kurbo::Rect::new(
            f64::from(f.origin.x),
            f64::from(f.origin.y),
            f64::from(f.max_x()),
            f64::from(f.max_y()),
        );
        let shape = Shape::rounded(cx.dim(TokenKey::BorderRadius, 6.0));
        cx.list.push_stroke_shape(
            frame,
            &shape,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, EDGE),
        );

        // Title-in-a-gap: paint a page-colored patch over the top
        // border behind the title (or checkbox), then the title text.
        // Cheaper than breaking the border path and reads identically.
        let t = &self.title_rect;
        let gap_pad = cx.pt(TITLE_GAP_PAD);
        let patch_right = if self.checkbox.is_some() {
            self.checkbox_rect.max_x()
        } else {
            // Patch just past the label's estimated extent — the
            // painter clips the text itself.
            (t.origin.x + cx.pt(TITLE_GAP_PAD + 8.0 * self.title.chars().count() as f32))
                .min(t.max_x())
        };
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                f64::from(t.origin.x - gap_pad),
                f64::from(t.origin.y),
                f64::from(patch_right + gap_pad),
                f64::from(t.max_y()),
            ),
            cx.color(TokenKey::BackgroundColor, PAGE),
        );

        if self.checkbox.is_none() {
            crate::text_paint::paint_label_clipped(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Rect::new(
                    f64::from(t.origin.x),
                    f64::from(t.origin.y),
                    f64::from(t.max_x()),
                    f64::from(t.max_y()),
                ),
                kurbo::Point::new(
                    f64::from(t.origin.x),
                    f64::from(t.origin.y + (t.size.y - cx.pt(TITLE_SIZE)) / 2.0),
                ),
                &self.title,
                cx.pt(TITLE_SIZE),
                cx.color(TokenKey::TextColor, INK),
            );
        }

        // Checkable + unchecked → veil the content to read disabled.
        if self.checkable && !self.is_checked() {
            let c = &self.content_rect;
            let mut veil = cx.color(TokenKey::BackgroundColor, PAGE);
            veil[3] = 140;
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(c.origin.x),
                    f64::from(c.origin.y),
                    f64::from(c.max_x()),
                    f64::from(c.max_y()),
                ),
                veil,
            );
        }
    }

    fn child_count(&self) -> usize {
        usize::from(self.checkbox.is_some()) + usize::from(self.child.is_some())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        let lead = usize::from(self.checkbox.is_some());
        if index < lead {
            return self.checkbox.as_ref().map(|cb| cb as &dyn Widget);
        }
        if index == lead {
            self.child.as_deref()
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        let lead = usize::from(self.checkbox.is_some());
        if index < lead {
            return self.checkbox.as_mut().map(|cb| cb as &mut dyn Widget);
        }
        if index == lead {
            self.child.as_deref_mut()
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        let lead = usize::from(self.checkbox.is_some());
        if index < lead {
            return Some(self.checkbox_rect);
        }
        if index == lead && self.child.is_some() {
            Some(self.content_rect)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for GroupBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GroupBox")
            .field("title", &self.title)
            .field("checkable", &self.checkable)
            .field("checked", &self.is_checked())
            .field("has_child", &self.child.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::widget::DummyWidget;
    use martensite_core::{HotNode, PointerButton, WidgetEvent};

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    #[test]
    fn groupbox_new() {
        let gb = GroupBox::new("Audio");
        assert_eq!(gb.title, "Audio");
        assert!(!gb.checkable);
        assert!(gb.checkbox.is_none());
        assert!(gb.is_checked());
    }

    #[test]
    fn groupbox_checkable_gate() {
        let mut gb = GroupBox::new("Sync").checkable(true);
        assert!(gb.checkbox.is_some());
        assert!(!gb.is_checked());
        gb.set_checked(true);
        assert!(gb.is_checked());
    }

    #[test]
    fn groupbox_child_protocol() {
        let gb = GroupBox::new("X").checkable(true).child(DummyWidget);
        // checkbox first, then content.
        assert_eq!(gb.child_count(), 2);
        assert!(Widget::child(&gb, 0).is_some());
        assert!(Widget::child(&gb, 1).is_some());
        assert!(Widget::child(&gb, 2).is_none());
    }

    #[test]
    fn groupbox_measure_and_layout() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut gb = GroupBox::new("Frame").child(DummyWidget);
        gb.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(300.0, 300.0),
            },
        );
        gb.layout(&mut cx, Rect::new(0.0, 0.0, 300.0, 200.0));
        // Frame top edge runs through the middle of the title band.
        assert_eq!(gb.frame_rect.origin.y, 11.0);
        assert!(gb.content_rect.origin.y > gb.frame_rect.origin.y);
        assert!(gb.content_rect.max_y() <= gb.frame_rect.max_y());
    }

    #[test]
    fn groupbox_unchecked_blocks_content_events() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut gb = GroupBox::new("X").checkable(true).child(DummyWidget);
        let mut lcx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        gb.layout(&mut lcx, Rect::new(0.0, 0.0, 300.0, 200.0));
        // Press in the content area — inert while unchecked.
        let c = gb.content_rect;
        let mut ecx = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: Vec2::new(c.origin.x + 2.0, c.origin.y + 2.0),
                button: PointerButton::Primary,
            },
            bounds: Rect::new(0.0, 0.0, 300.0, 200.0),
            scale: 1.0,
        };
        assert_eq!(gb.event(&mut ecx), EventResponse::Ignored);
        assert!(!gb.is_checked());
    }

    #[test]
    fn groupbox_checkbox_toggles_via_event() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut gb = GroupBox::new("X").checkable(true).child(DummyWidget);
        let mut lcx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        gb.measure(
            &mut lcx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(300.0, 300.0),
            },
        );
        gb.layout(&mut lcx, Rect::new(0.0, 0.0, 300.0, 200.0));
        let r = gb.checkbox_rect;
        let mut ecx = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: Vec2::new(r.origin.x + 2.0, r.origin.y + 2.0),
                button: PointerButton::Primary,
            },
            bounds: Rect::new(0.0, 0.0, 300.0, 200.0),
            scale: 1.0,
        };
        assert_eq!(gb.event(&mut ecx), EventResponse::RequestRepaint);
        assert!(gb.is_checked());
    }

    #[test]
    fn groupbox_a11y() {
        let gb = GroupBox::new("Panel");
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        gb.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Group);
        assert_eq!(node.label(), Some("Panel"));
    }

    #[test]
    fn groupbox_debug_format() {
        let gb = GroupBox::new("D").checkable(true);
        let debug = format!("{:?}", gb);
        assert!(debug.contains("GroupBox"));
    }
}
