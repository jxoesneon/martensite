//! `ExpanderRow` — a settings row that expands to reveal nested child
//! rows (libadwaita `AdwExpanderRow`).
//!
//! The header is an activatable [`SettingsRow`]; activating it (click,
//! Enter/Space, or a semantic `Click`/`Expand`/`Collapse`) toggles a
//! vertically stacked list of child widgets indented beneath it.
//! `ArrowRight` expands and `ArrowLeft` collapses — the ARIA
//! treegrid-row convention. Toggles park in
//! [`ExpanderRow::take_toggled`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::expander_row::ExpanderRow;
//! use martensite::widgets::SettingsRow;
//!
//! let row = ExpanderRow::new("Network")
//!     .subtitle("3 adapters")
//!     .child(SettingsRow::new("Ethernet"))
//!     .child(SettingsRow::new("Wi-Fi"));
//! assert_eq!(row.child_len(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use super::settings_row::SettingsRow;

const INDENT_PT: f32 = 16.0;
const CARET_PT: f32 = 8.0;
const MUTED: [u8; 4] = [110, 110, 118, 255];

/// An expanding settings row — see the module docs.
///
/// ```
/// use martensite::widgets::expander_row::ExpanderRow;
///
/// let row = ExpanderRow::new("Details");
/// assert!(!row.is_expanded());
/// ```
pub struct ExpanderRow {
    /// When `false` the row is inert.
    pub enabled: bool,
    expanded: bool,
    toggled: bool,
    header: SettingsRow,
    children: Vec<Box<dyn Widget>>,
    bounds: Rect,
    header_bounds: Rect,
    child_rects: Vec<Rect>,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl ExpanderRow {
    /// Creates a collapsed row titled `title`.
    ///
    /// ```
    /// use martensite::widgets::expander_row::ExpanderRow;
    ///
    /// let row = ExpanderRow::new("Advanced");
    /// assert_eq!(row.title(), "Advanced");
    /// ```
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            enabled: true,
            expanded: false,
            toggled: false,
            header: SettingsRow::new(title).activatable(true),
            children: Vec::new(),
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            header_bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            child_rects: Vec::new(),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Sets the header subtitle.
    ///
    /// ```
    /// use martensite::widgets::expander_row::ExpanderRow;
    ///
    /// let row = ExpanderRow::new("Net").subtitle("wired");
    /// assert_eq!(row.title(), "Net");
    /// ```
    pub fn subtitle(mut self, text: impl Into<String>) -> Self {
        self.header = std::mem::replace(&mut self.header, SettingsRow::new("")).subtitle(text);
        self
    }

    /// Sets the header icon glyph.
    ///
    /// ```
    /// use martensite::widgets::expander_row::ExpanderRow;
    ///
    /// let row = ExpanderRow::new("Net").icon("🌐");
    /// assert_eq!(row.title(), "Net");
    /// ```
    pub fn icon(mut self, glyph: impl Into<String>) -> Self {
        self.header = std::mem::replace(&mut self.header, SettingsRow::new("")).icon(glyph);
        self
    }

    /// Appends a nested child widget (revealed while expanded).
    ///
    /// ```
    /// use martensite::widgets::expander_row::ExpanderRow;
    /// use martensite::widgets::SettingsRow;
    ///
    /// let row = ExpanderRow::new("G").child(SettingsRow::new("C"));
    /// assert_eq!(row.child_len(), 1);
    /// ```
    pub fn child(mut self, child: impl Widget + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    /// Sets the expanded state.
    ///
    /// ```
    /// use martensite::widgets::expander_row::ExpanderRow;
    ///
    /// let row = ExpanderRow::new("G").expanded(true);
    /// assert!(row.is_expanded());
    /// ```
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// Enables or disables the row.
    ///
    /// ```
    /// use martensite::widgets::expander_row::ExpanderRow;
    ///
    /// let row = ExpanderRow::new("G").enabled(false);
    /// assert!(!row.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::expander_row::ExpanderRow;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let row = ExpanderRow::new("G").with_text_painter(shared_painter());
    /// assert_eq!(row.title(), "G");
    /// ```
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Header title.
    ///
    /// ```
    /// use martensite::widgets::expander_row::ExpanderRow;
    ///
    /// assert_eq!(ExpanderRow::new("T").title(), "T");
    /// ```
    pub fn title(&self) -> &str {
        self.header.title()
    }

    /// `true` while expanded.
    ///
    /// ```
    /// use martensite::widgets::expander_row::ExpanderRow;
    ///
    /// assert!(!ExpanderRow::new("T").is_expanded());
    /// ```
    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// Nested child count.
    ///
    /// ```
    /// use martensite::widgets::expander_row::ExpanderRow;
    /// use martensite::widgets::SettingsRow;
    ///
    /// let row = ExpanderRow::new("T").child(SettingsRow::new("C"));
    /// assert_eq!(row.child_len(), 1);
    /// ```
    pub fn child_len(&self) -> usize {
        self.children.len()
    }

    /// Programmatic expand/collapse.
    ///
    /// ```
    /// use martensite::widgets::expander_row::ExpanderRow;
    ///
    /// let mut row = ExpanderRow::new("T");
    /// row.set_expanded(true);
    /// assert!(row.is_expanded());
    /// ```
    pub fn set_expanded(&mut self, expanded: bool) {
        if self.expanded != expanded {
            self.expanded = expanded;
            self.toggled = true;
        }
    }

    /// Drains a pending expansion toggle.
    ///
    /// ```
    /// use martensite::widgets::expander_row::ExpanderRow;
    ///
    /// let mut row = ExpanderRow::new("T");
    /// row.set_expanded(true);
    /// assert!(row.take_toggled());
    /// assert!(!row.take_toggled());
    /// ```
    pub fn take_toggled(&mut self) -> bool {
        std::mem::take(&mut self.toggled)
    }

    /// Borrows a nested child.
    ///
    /// ```
    /// use martensite::widgets::expander_row::ExpanderRow;
    /// use martensite::widgets::SettingsRow;
    ///
    /// let row = ExpanderRow::new("T").child(SettingsRow::new("C"));
    /// assert!(row.child_at(0).is_some());
    /// ```
    pub fn child_at(&self, index: usize) -> Option<&dyn Widget> {
        self.children.get(index).map(|c| &**c)
    }

    /// Borrows a nested child mutably.
    ///
    /// ```
    /// use martensite::widgets::expander_row::ExpanderRow;
    /// use martensite::widgets::SettingsRow;
    ///
    /// let mut row = ExpanderRow::new("T").child(SettingsRow::new("C"));
    /// assert!(row.child_at_mut(0).is_some());
    /// ```
    pub fn child_at_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.children.get_mut(index).map(|c| &mut **c)
    }
}

impl Widget for ExpanderRow {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let header = self.header.measure(cx, constraints);
        let mut w = header.x;
        let mut h = header.y;
        if self.expanded {
            let inner = LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(
                    (constraints.max_size.x - cx.pt(INDENT_PT)).max(0.0),
                    constraints.max_size.y,
                ),
            };
            for child in &mut self.children {
                let s = child.measure(cx, inner);
                w = w.max(s.x + cx.pt(INDENT_PT));
                h += s.y;
            }
        }
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        // Header occupies the top strip at its measured height.
        let header_h = self
            .header
            .measure(
                cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: bounds.size,
                },
            )
            .y
            .min(bounds.size.y.max(0.0));
        self.header_bounds = Rect::new(bounds.origin.x, bounds.origin.y, bounds.size.x, header_h);
        self.header.layout(cx, self.header_bounds);
        self.child_rects.clear();
        if self.expanded {
            let indent = cx.pt(INDENT_PT);
            let mut y = bounds.origin.y + header_h;
            for child in &mut self.children {
                let avail_w = (bounds.size.x - indent).max(0.0);
                let avail_h = (bounds.max_y() - y).max(0.0);
                let s = child.measure(
                    cx,
                    LayoutConstraints {
                        min_size: Vec2::ZERO,
                        max_size: Vec2::new(avail_w, avail_h),
                    },
                );
                let h = s.y.min(avail_h);
                let r = Rect::new(bounds.origin.x + indent, y, avail_w, h);
                child.layout(cx, r);
                self.child_rects.push(r);
                y += h;
            }
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.header.title().to_string());
        node.set_expanded(self.expanded);
        if self.enabled {
            node.add_action(accesskit::Action::Click);
            if self.expanded {
                node.add_action(accesskit::Action::Collapse);
            } else {
                node.add_action(accesskit::Action::Expand);
            }
        } else {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        // Row-level treegrid keys.
        if let WidgetEvent::KeyPressed { key, .. } = cx.event {
            match key.as_str() {
                "ArrowRight" if !self.expanded => {
                    self.expanded = true;
                    self.toggled = true;
                    return EventResponse::RequestRepaint;
                }
                "ArrowLeft" if self.expanded => {
                    self.expanded = false;
                    self.toggled = true;
                    return EventResponse::RequestRepaint;
                }
                _ => {}
            }
        }
        if let WidgetEvent::SemanticAction(action) = cx.event {
            use martensite_core::widget::SemanticAction as A;
            match action {
                A::Expand if !self.expanded => {
                    self.expanded = true;
                    self.toggled = true;
                    return EventResponse::RequestRepaint;
                }
                A::Collapse if self.expanded => {
                    self.expanded = false;
                    self.toggled = true;
                    return EventResponse::RequestRepaint;
                }
                _ => {}
            }
        }
        // Forward to internal children topmost-first, bounds-gated.
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
                response => {
                    if i == 0 && self.header.take_activated() {
                        self.expanded = !self.expanded;
                        self.toggled = true;
                    }
                    return response;
                }
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        // Disclosure caret at the header's trailing edge — right when
        // collapsed, down when expanded.
        let cs = cx.pt(CARET_PT);
        let x0 = f64::from(self.header_bounds.max_x() - cx.pt(14.0) - cs);
        let y0 = f64::from(self.header_bounds.min_y() + (self.header_bounds.height() - cs) / 2.0);
        let s = f64::from(cs);
        let mut caret = kurbo::BezPath::new();
        if self.expanded {
            caret.move_to((x0, y0 + s * 0.25));
            caret.line_to((x0 + s / 2.0, y0 + s * 0.75));
            caret.line_to((x0 + s, y0 + s * 0.25));
        } else {
            caret.move_to((x0 + s * 0.25, y0));
            caret.line_to((x0 + s * 0.75, y0 + s / 2.0));
            caret.line_to((x0 + s * 0.25, y0 + s));
        }
        cx.list
            .push_stroke_path(caret, cx.pt(1.5), cx.color(TokenKey::TextMutedColor, MUTED));
    }

    fn child_count(&self) -> usize {
        1 + self.children.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index == 0 {
            Some(&self.header)
        } else {
            self.children.get(index - 1).map(|c| &**c)
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index == 0 {
            Some(&mut self.header)
        } else {
            self.children.get_mut(index - 1).map(|c| &mut **c)
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index == 0 {
            Some(self.header_bounds)
        } else if self.expanded {
            self.child_rects.get(index - 1).copied()
        } else {
            None
        }
    }
}

impl std::fmt::Debug for ExpanderRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExpanderRow")
            .field("title", &self.header.title())
            .field("expanded", &self.expanded)
            .field("children", &self.children.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PointerButton};

    fn laid_out(row: &mut ExpanderRow, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        row.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        row.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn press(row: &mut ExpanderRow, p: Vec2) -> EventResponse {
        row.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: p,
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 400.0, 200.0),
            scale: 1.0,
        })
    }

    fn key(row: &mut ExpanderRow, k: &str) -> EventResponse {
        row.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: k.to_string(),
                repeat: false,
            },
            bounds: Rect::new(0.0, 0.0, 400.0, 200.0),
            scale: 1.0,
        })
    }

    #[test]
    fn collapsed_children_have_no_bounds() {
        let mut row = ExpanderRow::new("G").child(SettingsRow::new("C"));
        laid_out(&mut row, 400.0, 200.0);
        assert!(row.child_bounds(1).is_none());
        row.set_expanded(true);
        laid_out(&mut row, 400.0, 200.0);
        assert!(row.child_bounds(1).is_some());
    }

    #[test]
    fn header_press_toggles() {
        let mut row = ExpanderRow::new("G").child(SettingsRow::new("C"));
        laid_out(&mut row, 400.0, 200.0);
        let hb = row.header_bounds;
        press(&mut row, Vec2::new(hb.min_x() + 10.0, hb.min_y() + 10.0));
        assert!(row.is_expanded());
        assert!(row.take_toggled());
        assert!(!row.take_toggled());
    }

    #[test]
    fn arrow_keys_expand_collapse() {
        let mut row = ExpanderRow::new("G");
        laid_out(&mut row, 400.0, 200.0);
        key(&mut row, "ArrowRight");
        assert!(row.is_expanded());
        key(&mut row, "ArrowLeft");
        assert!(!row.is_expanded());
    }

    #[test]
    fn semantic_expand_collapse() {
        use martensite_core::widget::SemanticAction;
        let mut row = ExpanderRow::new("G");
        laid_out(&mut row, 400.0, 200.0);
        row.event(&mut EventContext {
            event: &WidgetEvent::SemanticAction(SemanticAction::Expand),
            bounds: Rect::new(0.0, 0.0, 400.0, 200.0),
            scale: 1.0,
        });
        assert!(row.is_expanded());
        row.event(&mut EventContext {
            event: &WidgetEvent::SemanticAction(SemanticAction::Collapse),
            bounds: Rect::new(0.0, 0.0, 400.0, 200.0),
            scale: 1.0,
        });
        assert!(!row.is_expanded());
    }

    #[test]
    fn expanded_measure_adds_children() {
        let mut collapsed = ExpanderRow::new("G").child(SettingsRow::new("C"));
        let mut expanded = ExpanderRow::new("G")
            .child(SettingsRow::new("C"))
            .expanded(true);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let cons = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(400.0, 500.0),
        };
        let hc = collapsed.measure(&mut cx, cons).y;
        let he = expanded.measure(&mut cx, cons).y;
        assert!(he > hc);
    }

    #[test]
    fn children_stack_below_header_indented() {
        let mut row = ExpanderRow::new("G")
            .child(SettingsRow::new("A"))
            .child(SettingsRow::new("B"))
            .expanded(true);
        laid_out(&mut row, 400.0, 300.0);
        let a = row.child_bounds(1).unwrap();
        let b = row.child_bounds(2).unwrap();
        assert!(a.min_y() >= row.header_bounds.max_y());
        assert!(b.min_y() >= a.max_y());
        assert!(a.min_x() > row.header_bounds.min_x());
    }

    #[test]
    fn disabled_inert() {
        let mut row = ExpanderRow::new("G").enabled(false);
        laid_out(&mut row, 400.0, 200.0);
        key(&mut row, "ArrowRight");
        assert!(!row.is_expanded());
    }
}
