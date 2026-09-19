//! `HeaderBar` — window-top bar with leading/trailing slots.
//!
//! The GTK `HeaderBar` / WinUI `AppTitleBar` pattern: a fixed-height
//! strip with a centered title (optional subtitle), a leading slot
//! (back button, window controls) and a trailing slot (toolbar
//! actions), plus a bottom separator. Slot children keep their
//! natural size, vertically centered.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::header_bar::HeaderBar;
//! use martensite::widgets::Button;
//!
//! let bar = HeaderBar::new("Document")
//!     .subtitle("unsaved changes")
//!     .leading(Button::new("Back"))
//!     .trailing(Button::new("Save"));
//! assert_eq!(bar.title(), "Document");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

/// Title ink.
const TITLE_INK: [u8; 4] = [32, 33, 36, 255];
/// Subtitle ink.
const SUB_INK: [u8; 4] = [110, 114, 123, 255];
/// Bottom separator ink.
const SEP_INK: [u8; 4] = [208, 211, 217, 255];
/// Horizontal padding (logical points).
const PAD_PT: f32 = 12.0;
/// Title font size (logical points).
const TITLE_PT: f32 = 15.0;
/// Subtitle font size (logical points).
const SUB_PT: f32 = 11.0;
/// Inter-child gap inside a slot (logical points).
const SLOT_GAP_PT: f32 = 6.0;
/// Default bar height (logical points).
const HEIGHT_PT: f32 = 46.0;

/// A window-top bar — see the module docs.
///
/// Children are `[leading…, trailing…]`; `child_count` is the sum.
///
/// # Examples
///
/// ```
/// use martensite::widgets::header_bar::HeaderBar;
/// use martensite::widgets::Button;
/// use martensite::core::Widget;
///
/// let bar = HeaderBar::new("T").leading(Button::new("<")).trailing(Button::new("+"));
/// assert_eq!(bar.child_count(), 2);
/// ```
pub struct HeaderBar {
    title: String,
    subtitle: Option<String>,
    leading: Vec<Box<dyn Widget>>,
    trailing: Vec<Box<dyn Widget>>,
    /// Whether the bottom separator paints (default `true`).
    pub show_separator: bool,
    /// Cached child bounds from the last layout (widget-local).
    child_rects: Vec<Rect>,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl HeaderBar {
    /// A bar with `title` centered.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::header_bar::HeaderBar;
    ///
    /// let bar = HeaderBar::new("Settings");
    /// assert_eq!(bar.title(), "Settings");
    /// ```
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            leading: Vec::new(),
            trailing: Vec::new(),
            show_separator: true,
            child_rects: Vec::new(),
            text_painter: None,
        }
    }

    /// Sets a subtitle line under the title.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::header_bar::HeaderBar;
    ///
    /// let bar = HeaderBar::new("Doc").subtitle("editing");
    /// ```
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Appends a leading-slot child (left side).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::header_bar::HeaderBar;
    /// use martensite::widgets::Button;
    ///
    /// let bar = HeaderBar::new("T").leading(Button::new("Back"));
    /// assert_eq!(bar.leading_count(), 1);
    /// ```
    #[must_use]
    pub fn leading(mut self, child: impl Widget + 'static) -> Self {
        self.leading.push(Box::new(child));
        self
    }

    /// Appends a trailing-slot child (right side).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::header_bar::HeaderBar;
    /// use martensite::widgets::Button;
    ///
    /// let bar = HeaderBar::new("T").trailing(Button::new("Menu"));
    /// assert_eq!(bar.trailing_count(), 1);
    /// ```
    #[must_use]
    pub fn trailing(mut self, child: impl Widget + 'static) -> Self {
        self.trailing.push(Box::new(child));
        self
    }

    /// Overrides the shaped-text painter (tests and tooling).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::header_bar::HeaderBar;
    ///
    /// let bar = HeaderBar::new("T");
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
    /// use martensite::widgets::header_bar::HeaderBar;
    ///
    /// assert_eq!(HeaderBar::new("A").title(), "A");
    /// ```
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The subtitle, if set.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::header_bar::HeaderBar;
    ///
    /// assert_eq!(HeaderBar::new("A").subtitle_text(), None);
    /// ```
    pub fn subtitle_text(&self) -> Option<&str> {
        self.subtitle.as_deref()
    }

    /// Leading-slot child count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::header_bar::HeaderBar;
    ///
    /// assert_eq!(HeaderBar::new("A").leading_count(), 0);
    /// ```
    pub fn leading_count(&self) -> usize {
        self.leading.len()
    }

    /// Trailing-slot child count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::header_bar::HeaderBar;
    ///
    /// assert_eq!(HeaderBar::new("A").trailing_count(), 0);
    /// ```
    pub fn trailing_count(&self) -> usize {
        self.trailing.len()
    }
}

impl Widget for HeaderBar {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut max_child_h = 0.0f32;
        for child in self.leading.iter_mut().chain(self.trailing.iter_mut()) {
            let s = child.measure(cx, constraints);
            max_child_h = max_child_h.max(s.y);
        }
        let text_h = if self.subtitle.is_some() {
            TITLE_PT + SUB_PT + 2.0
        } else {
            TITLE_PT
        };
        Vec2::new(
            constraints
                .max_size
                .x
                .max(120.0)
                .min(constraints.max_size.x),
            (max_child_h + 2.0 * PAD_PT)
                .max(text_h + 2.0 * PAD_PT)
                .max(HEIGHT_PT),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let pad = cx.pt(PAD_PT);
        let gap = cx.pt(SLOT_GAP_PT);
        let mid_y = bounds.min_y() + bounds.height() / 2.0;
        let mut rects = Vec::with_capacity(self.leading.len() + self.trailing.len());

        // Leading slot — left to right at natural size.
        let child_constraints = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: bounds.size,
        };
        let mut x = bounds.min_x() + pad;
        for child in &mut self.leading {
            let s = child.measure(cx, child_constraints);
            let r = Rect::new(x, mid_y - s.y / 2.0, s.x, s.y);
            cx.layout_child(child.as_mut(), r);
            rects.push(r);
            x += s.x + gap;
        }

        // Trailing slot — right to left at natural size.
        let mut x = bounds.max_x() - pad;
        let mut trail_rects = Vec::with_capacity(self.trailing.len());
        for child in self.trailing.iter_mut().rev() {
            let s = child.measure(cx, child_constraints);
            let r = Rect::new(x - s.x, mid_y - s.y / 2.0, s.x, s.y);
            cx.layout_child(child.as_mut(), r);
            trail_rects.push(r);
            x -= s.x + gap;
        }
        trail_rects.reverse();
        rects.extend(trail_rects);
        self.child_rects = rects;
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let b = cx.bounds;
        let title_size = cx.pt(TITLE_PT);
        let mid_x = f64::from(b.min_x() + b.width() / 2.0);
        let clip = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        if let Some(sub) = &self.subtitle {
            let sub_size = cx.pt(SUB_PT);
            let total = title_size + cx.pt(2.0) + sub_size;
            let ty = b.min_y() + (b.height() - total) / 2.0;
            let w = painter
                .and_then(|p| p.measure_text(&self.title, title_size))
                .unwrap_or(title_size * self.title.len() as f32 * 0.5);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(mid_x - f64::from(w) / 2.0, f64::from(ty)),
                &self.title,
                title_size,
                cx.color(TokenKey::TextColor, TITLE_INK),
            );
            let sw = painter
                .and_then(|p| p.measure_text(sub, sub_size))
                .unwrap_or(sub_size * sub.len() as f32 * 0.5);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(
                    mid_x - f64::from(sw) / 2.0,
                    f64::from(ty + title_size + cx.pt(2.0)),
                ),
                sub,
                sub_size,
                cx.color(TokenKey::TextMutedColor, SUB_INK),
            );
        } else {
            let w = painter
                .and_then(|p| p.measure_text(&self.title, title_size))
                .unwrap_or(title_size * self.title.len() as f32 * 0.5);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(
                    mid_x - f64::from(w) / 2.0,
                    f64::from(b.min_y() + (b.height() - title_size) / 2.0),
                ),
                &self.title,
                title_size,
                cx.color(TokenKey::TextColor, TITLE_INK),
            );
        }
        if self.show_separator {
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(b.min_x()),
                    f64::from(b.max_y() - cx.pt(1.0)),
                    f64::from(b.max_x()),
                    f64::from(b.max_y()),
                ),
                cx.color(TokenKey::DividerColor, SEP_INK),
            );
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Slot children are plain targets — default bounds-gated
        // forwarding covers them.
        let _ = cx;
        EventResponse::Ignored
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Toolbar);
        node.set_label(self.title.clone());
    }

    fn child_count(&self) -> usize {
        self.leading.len() + self.trailing.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index < self.leading.len() {
            Some(self.leading[index].as_ref())
        } else {
            self.trailing
                .get(index - self.leading.len())
                .map(|c| c.as_ref())
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index < self.leading.len() {
            Some(self.leading[index].as_mut())
        } else {
            self.trailing
                .get_mut(index - self.leading.len())
                .map(|c| c.as_mut())
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.child_rects.get(index).copied()
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 32.0)).with_policy(UnderflowPolicy::Lint)
    }
}

impl std::fmt::Debug for HeaderBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeaderBar")
            .field("title", &self.title)
            .field("leading", &self.leading.len())
            .field("trailing", &self.trailing.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::button::Button;
    use martensite_core::HotNode;

    fn lay(w: &mut HeaderBar, bounds: Rect) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, bounds);
    }

    #[test]
    fn leading_lays_out_from_left() {
        let mut bar = HeaderBar::new("T")
            .leading(Button::new("A"))
            .leading(Button::new("B"));
        lay(&mut bar, Rect::new(0.0, 0.0, 400.0, 46.0));
        let a = bar.child_bounds(0).unwrap();
        let b = bar.child_bounds(1).unwrap();
        assert!(a.min_x() >= PAD_PT - 0.5);
        assert!(b.min_x() > a.max_x());
    }

    #[test]
    fn trailing_lays_out_from_right() {
        let mut bar = HeaderBar::new("T").trailing(Button::new("Z"));
        lay(&mut bar, Rect::new(0.0, 0.0, 400.0, 46.0));
        let z = bar.child_bounds(0).unwrap();
        assert!(z.max_x() <= 400.0 - PAD_PT + 0.5);
        assert!(z.max_x() > 300.0);
    }

    #[test]
    fn slots_are_vertical_centered() {
        let mut bar = HeaderBar::new("T").leading(Button::new("A"));
        lay(&mut bar, Rect::new(0.0, 0.0, 400.0, 60.0));
        let r = bar.child_bounds(0).unwrap();
        let center = (r.min_y() + r.max_y()) / 2.0;
        assert!((center - 30.0).abs() < 1.0);
    }

    #[test]
    fn index_order_is_leading_then_trailing() {
        let mut bar = HeaderBar::new("T")
            .leading(Button::new("L"))
            .trailing(Button::new("R1"))
            .trailing(Button::new("R2"));
        lay(&mut bar, Rect::new(0.0, 0.0, 400.0, 46.0));
        assert_eq!(bar.child_count(), 3);
        let r1 = bar.child_bounds(1).unwrap();
        let r2 = bar.child_bounds(2).unwrap();
        // Trailing children stay in append order left-to-right.
        assert!(r1.min_x() < r2.min_x());
    }

    #[test]
    fn measure_reports_min_height() {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let mut bar = HeaderBar::new("T");
        let s = bar.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(400.0, 400.0),
            },
        );
        assert!(s.y >= HEIGHT_PT);
    }
}
