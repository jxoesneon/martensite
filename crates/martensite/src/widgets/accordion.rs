//! `Accordion` widget: a vertical stack of collapsible sections
//! (Ant Design `Collapse` / MUI `Accordion`).
//!
//! Each section is a [`Disclosure`] internal child — chevron header
//! plus a body shown while open. By default the accordion is
//! *exclusive*: opening one section closes the others (the classic
//! accordion). Set [`Accordion::allow_multiple`] to let several
//! sections stay open at once.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::accordion::Accordion;
//! use martensite::widgets::text::Text;
//! use martensite_core::widget::Widget;
//!
//! let a = Accordion::new()
//!     .section("General", Text::new("general settings"))
//!     .section("Advanced", Text::new("advanced settings"));
//! assert_eq!(a.child_count(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Widget,
};
use martensite_core::{Rect, TokenKey};

use crate::widgets::disclosure::Disclosure;

const HAIRLINE: [u8; 4] = [210, 213, 218, 255];

/// A vertical stack of collapsible [`Disclosure`] sections with
/// managed expansion.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Accordion;
/// use martensite_core::widget::DummyWidget;
///
/// let mut a = Accordion::new()
///     .section("One", DummyWidget)
///     .section("Two", DummyWidget);
/// a.open_section(1);
/// assert_eq!(a.expanded(), vec![1]);
/// ```
pub struct Accordion {
    /// The sections, top to bottom — one [`Disclosure`] each.
    pub sections: Vec<Disclosure>,
    /// When `false` (the default), opening a section closes the
    /// others — managed single expansion.
    pub allow_multiple: bool,
    /// Vertical gap between sections in logical points.
    pub gap: f32,
    /// Cached per-section sizes from the last measure pass.
    section_sizes: Vec<Vec2>,
    /// Cached per-section rects from the last layout pass.
    section_rects: Vec<Rect>,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter, forwarded to every section.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Accordion {
    /// An empty, exclusive accordion.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Accordion;
    ///
    /// let a = Accordion::new();
    /// assert!(a.sections.is_empty());
    /// assert!(!a.allow_multiple);
    /// ```
    pub fn new() -> Self {
        Self {
            sections: Vec::new(),
            allow_multiple: false,
            gap: 0.0,
            section_sizes: Vec::new(),
            section_rects: Vec::new(),
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Appends a section with the given header title and body widget.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Accordion, Text};
    ///
    /// let a = Accordion::new().section("Details", Text::new("body"));
    /// assert_eq!(a.sections.len(), 1);
    /// ```
    #[must_use]
    pub fn section(mut self, title: impl Into<String>, child: impl Widget + 'static) -> Self {
        let mut d = Disclosure::new(title).child(child);
        if let Some(p) = &self.text_painter {
            d = d.with_text_painter(p.clone());
        }
        self.sections.push(d);
        self
    }

    /// Appends a pre-built [`Disclosure`] section.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Accordion, Disclosure, Text};
    ///
    /// let a = Accordion::new().add_section(Disclosure::new("S").child(Text::new("b")));
    /// assert_eq!(a.sections.len(), 1);
    /// ```
    #[must_use]
    pub fn add_section(mut self, section: Disclosure) -> Self {
        self.sections.push(section);
        self
    }

    /// Sets whether several sections may be open at once.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Accordion;
    ///
    /// let a = Accordion::new().allow_multiple(true);
    /// assert!(a.allow_multiple);
    /// ```
    #[inline]
    #[must_use]
    pub fn allow_multiple(mut self, allow: bool) -> Self {
        self.allow_multiple = allow;
        self
    }

    /// Sets the gap between sections in logical points.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Accordion;
    ///
    /// let a = Accordion::new().gap(8.0);
    /// assert_eq!(a.gap, 8.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] — applied to every
    /// current section and to sections added later.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.sections = self
            .sections
            .into_iter()
            .map(|d| d.with_text_painter(painter.clone()))
            .collect();
        self.text_painter = Some(painter);
        self
    }

    /// Whether the section at `index` is open.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Accordion, Text};
    ///
    /// let a = Accordion::new().section("S", Text::new("b"));
    /// assert!(!a.is_open(0));
    /// ```
    #[inline]
    pub fn is_open(&self, index: usize) -> bool {
        self.sections.get(index).is_some_and(|d| d.open)
    }

    /// Indices of the currently open sections.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Accordion, Text};
    ///
    /// let a = Accordion::new().section("S", Text::new("b"));
    /// assert!(a.expanded().is_empty());
    /// ```
    pub fn expanded(&self) -> Vec<usize> {
        self.sections
            .iter()
            .enumerate()
            .filter_map(|(i, d)| d.open.then_some(i))
            .collect()
    }

    /// Sets a section's open state, honouring exclusivity: opening a
    /// section while `allow_multiple` is off closes the others.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Accordion, Text};
    ///
    /// let mut a = Accordion::new()
    ///     .section("A", Text::new("a"))
    ///     .section("B", Text::new("b"));
    /// a.set_open(0, true);
    /// a.set_open(1, true);
    /// assert_eq!(a.expanded(), vec![1]); // 0 was closed by 1 opening
    /// ```
    pub fn set_open(&mut self, index: usize, open: bool) {
        let Some(section) = self.sections.get_mut(index) else {
            return;
        };
        section.set_open(open);
        if open {
            self.enforce_exclusivity(index);
        }
    }

    /// Opens a section (closing the others unless `allow_multiple`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Accordion, Text};
    ///
    /// let mut a = Accordion::new().section("S", Text::new("b"));
    /// a.open_section(0);
    /// assert!(a.is_open(0));
    /// ```
    #[inline]
    pub fn open_section(&mut self, index: usize) {
        self.set_open(index, true);
    }

    /// Toggles a section, honouring exclusivity.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Accordion, Text};
    ///
    /// let mut a = Accordion::new().section("S", Text::new("b"));
    /// a.toggle_section(0);
    /// assert!(a.is_open(0));
    /// a.toggle_section(0);
    /// assert!(!a.is_open(0));
    /// ```
    pub fn toggle_section(&mut self, index: usize) {
        let open = self.is_open(index);
        self.set_open(index, !open);
    }

    /// Closes every section except `keep` (no-op when
    /// `allow_multiple`).
    fn enforce_exclusivity(&mut self, keep: usize) {
        if self.allow_multiple {
            return;
        }
        for (i, section) in self.sections.iter_mut().enumerate() {
            if i != keep {
                section.set_open(false);
            }
        }
    }
}

impl Default for Accordion {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Accordion {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        self.section_sizes.clear();
        self.section_sizes.reserve(self.sections.len());
        let n = self.sections.len();
        let total_gap = cx.pt(self.gap) * n.saturating_sub(1) as f32;
        let mut total_h = 0.0f32;
        let mut max_w = 0.0f32;
        for section in &mut self.sections {
            // Each section gets the vertical space left after the
            // previously-measured siblings and the inter-section gaps.
            let remaining = if constraints.max_size.y.is_finite() {
                (constraints.max_size.y - total_h - total_gap).max(0.0)
            } else {
                f32::MAX
            };
            let size = section.measure(
                cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: Vec2::new(constraints.max_size.x, remaining),
                },
            );
            self.section_sizes.push(size);
            total_h += size.y;
            max_w = max_w.max(size.x);
        }
        total_h += total_gap;
        Vec2::new(
            max_w.min(constraints.max_size.x.max(0.0)),
            total_h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.section_rects.clear();

        // Sections may have been added since the last measure — fall
        // back to a direct probe so layout never stacks at zero.
        if self.section_sizes.len() != self.sections.len() {
            self.section_sizes.clear();
            for section in &mut self.sections {
                let size = section.measure(
                    cx,
                    LayoutConstraints {
                        min_size: Vec2::ZERO,
                        max_size: bounds.size,
                    },
                );
                self.section_sizes.push(size);
            }
        }

        let gap = cx.pt(self.gap);
        let mut y = bounds.origin.y;
        for (i, section) in self.sections.iter_mut().enumerate() {
            let size = self.section_sizes.get(i).copied().unwrap_or(Vec2::ZERO);
            let h = size.y.min((bounds.max_y() - y).max(0.0));
            let r = Rect::new(bounds.origin.x, y, bounds.size.x, h);
            self.section_rects.push(r);
            cx.layout_child(section, r);
            y += h + gap;
        }

        // Defensive: programmatically opened extras collapse to the
        // first open section under exclusivity.
        if !self.allow_multiple {
            if let Some(first_open) = self.sections.iter().position(|d| d.open) {
                self.enforce_exclusivity(first_open);
            }
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        // Sections surface as `DisclosureTriangle` children.
        node.set_role(accesskit::Role::Group);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Forward bounds-gated, topmost-first — then enforce managed
        // expansion when a section's toggle changed the set.
        let n = self.sections.len();
        for i in (0..n).rev() {
            let Some(section_bounds) = self.section_rects.get(i).copied() else {
                continue;
            };
            if let Some(pos) = cx.event.position() {
                if !section_bounds.contains(pos) {
                    continue;
                }
            }
            let was_open = self.sections[i].open;
            let mut child_cx = EventContext {
                event: cx.event,
                bounds: section_bounds,
                scale: cx.scale,
            };
            match self.sections[i].event(&mut child_cx) {
                EventResponse::Ignored => continue,
                response => {
                    if self.sections[i].open && !was_open {
                        self.enforce_exclusivity(i);
                    }
                    return response;
                }
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        // Hairline divider between stacked sections — the Ant Collapse
        // look. Skipped after the last one.
        if self.section_rects.len() < 2 {
            return;
        }
        let divider = cx.color(TokenKey::DividerColor, HAIRLINE);
        for r in self.section_rects.iter().take(self.section_rects.len() - 1) {
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(r.origin.x),
                    f64::from(r.max_y()),
                    f64::from(r.max_x()),
                    f64::from(r.max_y()) + cx.ptf(1.0),
                ),
                divider,
            );
        }
    }

    fn child_count(&self) -> usize {
        self.sections.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.sections.get(index).map(|d| d as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.sections.get_mut(index).map(|d| d as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.section_rects.get(index).copied()
    }
}

impl std::fmt::Debug for Accordion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Accordion")
            .field("sections", &self.sections.len())
            .field("allow_multiple", &self.allow_multiple)
            .field("gap", &self.gap)
            .field("expanded", &self.expanded())
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
    fn accordion_new_is_empty_exclusive() {
        let a = Accordion::new();
        assert!(a.sections.is_empty());
        assert!(!a.allow_multiple);
        assert_eq!(a.child_count(), 0);
    }

    #[test]
    fn accordion_sections_stack() {
        let a = Accordion::new()
            .section("One", DummyWidget)
            .section("Two", DummyWidget)
            .section("Three", DummyWidget);
        assert_eq!(a.child_count(), 3);
        assert!(Widget::child(&a, 2).is_some());
        assert!(Widget::child(&a, 3).is_none());
    }

    #[test]
    fn accordion_exclusivity_programmatic() {
        let mut a = Accordion::new()
            .section("A", DummyWidget)
            .section("B", DummyWidget);
        a.open_section(0);
        a.open_section(1);
        // Exclusive: opening B closed A.
        assert_eq!(a.expanded(), vec![1]);
        a.toggle_section(1);
        assert!(a.expanded().is_empty());
    }

    #[test]
    fn accordion_allow_multiple_keeps_open() {
        let mut a = Accordion::new()
            .allow_multiple(true)
            .section("A", DummyWidget)
            .section("B", DummyWidget);
        a.open_section(0);
        a.open_section(1);
        assert_eq!(a.expanded(), vec![0, 1]);
    }

    #[test]
    fn accordion_measure_sums_sections() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut a = Accordion::new()
            .section("A", DummyWidget)
            .section("B", DummyWidget);
        let size = a.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(300.0, 300.0),
            },
        );
        // Two closed Disclosure headers at 24pt each.
        assert_eq!(size, Vec2::new(300.0, 48.0));
    }

    #[test]
    fn accordion_layout_stacks_rects() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut cx = make_cx(&mut hot);
        let mut a = Accordion::new()
            .gap(4.0)
            .section("A", DummyWidget)
            .section("B", DummyWidget);
        a.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(300.0, 300.0),
            },
        );
        a.layout(&mut cx, Rect::new(0.0, 0.0, 300.0, 200.0));
        let r0 = a.child_bounds(0).unwrap();
        let r1 = a.child_bounds(1).unwrap();
        assert_eq!(r0.origin.y, 0.0);
        assert_eq!(r1.origin.y, r0.max_y() + 4.0);
    }

    #[test]
    fn accordion_exclusive_via_header_click() {
        let mut hot = HotNode::new(taffy::NodeId::new(1));
        let mut a = Accordion::new()
            .section("A", DummyWidget)
            .section("B", DummyWidget);
        let mut lcx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        a.measure(
            &mut lcx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(300.0, 300.0),
            },
        );
        let bounds = Rect::new(0.0, 0.0, 300.0, 200.0);
        a.layout(&mut lcx, bounds);

        // Click section A's header.
        let r0 = a.child_bounds(0).unwrap();
        let mut ecx = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: Vec2::new(r0.origin.x + 8.0, r0.origin.y + 8.0),
                button: PointerButton::Primary,
            },
            bounds,
            scale: 1.0,
        };
        assert_eq!(a.event(&mut ecx), EventResponse::RequestRepaint);
        assert_eq!(a.expanded(), vec![0]);

        // Opening A grew section A's body — re-layout, then click B.
        a.measure(
            &mut lcx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(300.0, 300.0),
            },
        );
        a.layout(&mut lcx, bounds);
        let r1 = a.child_bounds(1).unwrap();
        let mut ecx = EventContext {
            event: &WidgetEvent::PointerReleased {
                position: Vec2::new(r1.origin.x + 8.0, r1.origin.y + 8.0),
                button: PointerButton::Primary,
            },
            bounds,
            scale: 1.0,
        };
        assert_eq!(a.event(&mut ecx), EventResponse::RequestRepaint);
        // B opened, A auto-closed.
        assert_eq!(a.expanded(), vec![1]);
    }

    #[test]
    fn accordion_a11y_group() {
        let a = Accordion::new();
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        a.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Group);
    }

    #[test]
    fn accordion_debug_format() {
        let a = Accordion::new().section("S", DummyWidget);
        let debug = format!("{:?}", a);
        assert!(debug.contains("Accordion"));
    }
}
