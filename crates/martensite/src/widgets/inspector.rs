//! `Inspector` — a sectioned property panel (Xcode / Figma
//! right-rail idiom): collapsible [`InspectorSection`]s of
//! label/value rows.
//!
//! Clicking a section header toggles it; clicking a row parks
//! `(section, row)` in [`Inspector::take_selected`]. Values are
//! host-driven via [`Inspector::set_value`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::inspector::Inspector;
//!
//! let i = Inspector::new()
//!     .section("Transform")
//!     .row("X", "12.5")
//!     .row("Y", "4.0");
//! assert_eq!(i.section_count(), 1);
//! assert_eq!(i.value_of(0, 1), Some("4.0"));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const PAD_PT: f32 = 8.0;
const HEAD_PT: f32 = 26.0;
const ROW_PT: f32 = 24.0;
const FONT_PT: f32 = 11.5;
const HEAD_FONT_PT: f32 = 11.0;

const FACE: [u8; 4] = [28, 30, 38, 255];
const HEAD_BG: [u8; 4] = [255, 255, 255, 10];
const ROW_HOVER: [u8; 4] = [255, 255, 255, 12];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const MUTED_FG: [u8; 4] = [150, 154, 164, 255];

/// One label/value row.
///
/// ```
/// use martensite::widgets::inspector::InspectorRow;
///
/// assert_eq!(InspectorRow::new("X", "0").value, "0");
/// ```
#[derive(Clone, Debug)]
pub struct InspectorRow {
    /// Left-hand property label.
    pub label: String,
    /// Right-hand value text.
    pub value: String,
}

impl InspectorRow {
    /// A row.
    ///
    /// ```
    /// use martensite::widgets::inspector::InspectorRow;
    ///
    /// assert_eq!(InspectorRow::new("W", "10").label, "W");
    /// ```
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

/// A collapsible section of rows.
///
/// ```
/// use martensite::widgets::inspector::{InspectorRow, InspectorSection};
///
/// let s = InspectorSection::new("Fill").row(InspectorRow::new("Color", "#fff"));
/// assert_eq!(s.rows.len(), 1);
/// assert!(s.open);
/// ```
#[derive(Clone, Debug)]
pub struct InspectorSection {
    /// Header title.
    pub title: String,
    /// Rows.
    pub rows: Vec<InspectorRow>,
    /// Expanded flag.
    pub open: bool,
}

impl InspectorSection {
    /// An open section.
    ///
    /// ```
    /// use martensite::widgets::inspector::InspectorSection;
    ///
    /// assert_eq!(InspectorSection::new("T").title, "T");
    /// ```
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            rows: Vec::new(),
            open: true,
        }
    }

    /// Appends a row.
    ///
    /// ```
    /// use martensite::widgets::inspector::{InspectorRow, InspectorSection};
    ///
    /// assert_eq!(InspectorSection::new("T").row(InspectorRow::new("a", "b")).rows.len(), 1);
    /// ```
    pub fn row(mut self, row: InspectorRow) -> Self {
        self.rows.push(row);
        self
    }
}

/// The panel — see the module docs.
///
/// ```
/// use martensite::widgets::inspector::Inspector;
///
/// assert_eq!(Inspector::new().section_count(), 0);
/// ```
pub struct Inspector {
    /// Accessibility label.
    pub label: String,
    /// Row height in points.
    pub row_height: f32,
    sections: Vec<InspectorSection>,
    selected: Option<(usize, usize)>,
    hovered: Option<usize>, // flat row index
    head_rects: Vec<Rect>,
    row_rects: Vec<(Rect, usize, usize)>, // rect + (section, row)
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for Inspector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inspector")
            .field("sections", &self.sections.len())
            .finish()
    }
}

impl Default for Inspector {
    fn default() -> Self {
        Self::new()
    }
}

impl Inspector {
    /// Empty panel.
    ///
    /// ```
    /// use martensite::widgets::inspector::Inspector;
    ///
    /// assert_eq!(Inspector::new().section_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Inspector".to_string(),
            row_height: ROW_PT,
            sections: Vec::new(),
            selected: None,
            hovered: None,
            head_rects: Vec::new(),
            row_rects: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Starts a new section; following `row` calls append to it.
    ///
    /// ```
    /// use martensite::widgets::inspector::Inspector;
    ///
    /// assert_eq!(Inspector::new().section("Fill").section_count(), 1);
    /// ```
    pub fn section(mut self, title: impl Into<String>) -> Self {
        self.sections.push(InspectorSection::new(title));
        self
    }

    /// Appends a row to the current section (creating a default
    /// section if none exists).
    ///
    /// ```
    /// use martensite::widgets::inspector::Inspector;
    ///
    /// let i = Inspector::new().section("S").row("X", "0").row("Y", "0");
    /// assert_eq!(i.value_of(0, 0), Some("0"));
    /// ```
    pub fn row(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        if self.sections.is_empty() {
            self.sections.push(InspectorSection::new("Properties"));
        }
        self.sections
            .last_mut()
            .expect("nonempty")
            .rows
            .push(InspectorRow::new(label, value));
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::inspector::Inspector;
    ///
    /// assert_eq!(Inspector::new().label("Props").label, "Props");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::inspector::Inspector;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _i = Inspector::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Section count.
    ///
    /// ```
    /// use martensite::widgets::inspector::Inspector;
    ///
    /// assert_eq!(Inspector::new().section_count(), 0);
    /// ```
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// Value at `(section, row)`.
    ///
    /// ```
    /// use martensite::widgets::inspector::Inspector;
    ///
    /// let i = Inspector::new().row("X", "1.5");
    /// assert_eq!(i.value_of(0, 0), Some("1.5"));
    /// ```
    pub fn value_of(&self, section: usize, row: usize) -> Option<&str> {
        self.sections
            .get(section)
            .and_then(|s| s.rows.get(row))
            .map(|r| r.value.as_str())
    }

    /// Sets a row's value host-side.
    ///
    /// ```
    /// use martensite::widgets::inspector::Inspector;
    ///
    /// let mut i = Inspector::new().row("X", "0");
    /// i.set_value(0, 0, "42");
    /// assert_eq!(i.value_of(0, 0), Some("42"));
    /// ```
    pub fn set_value(&mut self, section: usize, row: usize, value: impl Into<String>) {
        if let Some(r) = self
            .sections
            .get_mut(section)
            .and_then(|s| s.rows.get_mut(row))
        {
            r.value = value.into();
        }
    }

    /// Whether a section is expanded.
    ///
    /// ```
    /// use martensite::widgets::inspector::Inspector;
    ///
    /// assert!(Inspector::new().section("S").is_open(0));
    /// ```
    pub fn is_open(&self, section: usize) -> bool {
        self.sections.get(section).is_some_and(|s| s.open)
    }

    /// Toggles a section host-side.
    ///
    /// ```
    /// use martensite::widgets::inspector::Inspector;
    ///
    /// let mut i = Inspector::new().section("S");
    /// i.toggle_section(0);
    /// assert!(!i.is_open(0));
    /// ```
    pub fn toggle_section(&mut self, section: usize) {
        if let Some(s) = self.sections.get_mut(section) {
            s.open = !s.open;
        }
        self.rebuild_rows();
    }

    /// Drains the last clicked `(section, row)`.
    ///
    /// ```
    /// use martensite::widgets::inspector::Inspector;
    ///
    /// let mut i = Inspector::new();
    /// assert_eq!(i.take_selected(), None);
    /// ```
    pub fn take_selected(&mut self) -> Option<(usize, usize)> {
        self.selected.take()
    }

    /// Recomputes `head_rects`/`row_rects` from the stored bounds —
    /// the same math `layout` runs. Event-side mutations (section
    /// toggles) can't wait for the next layout pass: the app's layout
    /// is resize-gated, so stale rects would paint ghost rows under
    /// the collapsed section indefinitely.
    fn compute_rects(&mut self) {
        let s = self.scale;
        let bounds = self.bounds;
        let mut y = bounds.min_y() + PAD_PT * s;
        self.head_rects.clear();
        self.row_rects.clear();
        for (si, sec) in self.sections.iter().enumerate() {
            self.head_rects.push(Rect::new(
                bounds.min_x() + PAD_PT * s,
                y,
                (bounds.width() - PAD_PT * 2.0 * s).max(0.0),
                HEAD_PT * s,
            ));
            y += HEAD_PT * s;
            if sec.open {
                for ri in 0..sec.rows.len() {
                    self.row_rects.push((
                        Rect::new(
                            bounds.min_x() + PAD_PT * s,
                            y,
                            (bounds.width() - PAD_PT * 2.0 * s).max(0.0),
                            ROW_PT * s,
                        ),
                        si,
                        ri,
                    ));
                    y += ROW_PT * s;
                }
            }
        }
    }

    fn rebuild_rows(&mut self) {
        self.hovered = None;
        self.compute_rects();
    }
}

impl Widget for Inspector {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let mut rows = self.sections.len() as f32 * HEAD_PT;
        for sec in &self.sections {
            if sec.open {
                rows += sec.rows.len() as f32 * self.row_height;
            }
        }
        Vec2::new(
            (260.0 * s).min(constraints.max_size.x.max(0.0)),
            ((rows + PAD_PT * 2.0) * s).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(180.0, HEAD_PT + PAD_PT * 2.0))
            .with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.compute_rects();
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
        let rows: usize = self.sections.iter().map(|s| s.rows.len()).sum();
        node.set_value(format!("{} sections, {} rows", self.sections.len(), rows));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self
                    .row_rects
                    .iter()
                    .position(|(r, _, _)| r.contains(*position));
                if h != self.hovered {
                    self.hovered = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(si) = self.head_rects.iter().position(|r| r.contains(*position)) {
                    self.toggle_section(si);
                    return EventResponse::RequestRepaint;
                }
                if let Some((_, si, ri)) = self
                    .row_rects
                    .iter()
                    .find(|(r, _, _)| r.contains(*position))
                {
                    self.selected = Some((*si, *ri));
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let kbounds = kurbo::Rect::new(
            f64::from(self.bounds.min_x()),
            f64::from(self.bounds.min_y()),
            f64::from(self.bounds.max_x()),
            f64::from(self.bounds.max_y()),
        );
        cx.list
            .push_fill_rect(kbounds, cx.color(TokenKey::BackgroundColor, FACE));
        // Section content can exceed `bounds` (the widget is measured
        // shorter than its sections); clip to the widget so rows below
        // the edge don't emit text that sibling panels then cover or
        // the audit flags as escaping the container.
        cx.list.push_clip(kbounds);
        let shape = martensite_core::shape::Shape::rounded(4.0 * s);
        for (si, sec) in self.sections.iter().enumerate() {
            // `head_rects` is a layout-side cache — a widget painted
            // before its first layout (or rebuilt post-layout) has
            // sections but no rects; skip rather than panic.
            let Some(&hr) = self.head_rects.get(si) else {
                continue;
            };
            let khr = kurbo::Rect::new(
                f64::from(hr.min_x()),
                f64::from(hr.min_y()),
                f64::from(hr.max_x()),
                f64::from(hr.max_y()),
            );
            cx.list.push_fill_shape(khr, &shape, HEAD_BG);
            let head_clip = khr.intersect(kbounds);
            // Disclosure triangle.
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                head_clip,
                kurbo::Point::new(
                    f64::from(hr.min_x() + 6.0 * s),
                    f64::from(hr.min_y() + hr.height() * 0.72),
                ),
                if sec.open { "▾" } else { "▸" },
                HEAD_FONT_PT * s,
                MUTED_FG,
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                head_clip,
                kurbo::Point::new(
                    f64::from(hr.min_x() + 18.0 * s),
                    f64::from(hr.min_y() + hr.height() * 0.72),
                ),
                &sec.title,
                HEAD_FONT_PT * s,
                cx.color(TokenKey::TextColor, TEXT),
            );
        }
        for (r, si, ri) in &self.row_rects {
            // Stale `row_rects` can outlive the sections they were
            // built from (event-mutated or rebuilt widget) — treat a
            // dangling index as "nothing to paint", never a panic.
            let Some(row) = self.sections.get(*si).and_then(|s| s.rows.get(*ri)) else {
                continue;
            };
            let kr = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            if self
                .hovered
                .is_some_and(|h| self.row_rects.get(h) == Some(&(*r, *si, *ri)))
            {
                cx.list.push_fill_shape(kr, &shape, ROW_HOVER);
            }
            // The label column ends where the value column begins so
            // the two runs can never overlap.
            let row_clip = kr.intersect(kbounds);
            let value_x = r.min_x() + r.width() * 0.45;
            let label_clip = kurbo::Rect::new(
                row_clip.x0,
                row_clip.y0,
                row_clip.x1.min(f64::from(value_x - 4.0 * s)),
                row_clip.y1,
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                label_clip,
                kurbo::Point::new(
                    f64::from(r.min_x() + 18.0 * s),
                    f64::from(r.min_y() + r.height() * 0.72),
                ),
                &row.label,
                FONT_PT * s,
                MUTED_FG,
            );
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    row_clip.x0.max(f64::from(value_x)),
                    row_clip.y0,
                    row_clip.x1,
                    row_clip.y1,
                ),
                kurbo::Point::new(f64::from(value_x), f64::from(r.min_y() + r.height() * 0.72)),
                &row.value,
                FONT_PT * s,
                cx.color(TokenKey::TextColor, TEXT),
            );
        }
        cx.list.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> Inspector {
        Inspector::new()
            .section("Transform")
            .row("X", "12.5")
            .row("Y", "4.0")
            .section("Fill")
            .row("Color", "#a5c8ff")
    }

    fn laid_out(i: &mut Inspector) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        i.layout(&mut cx, Rect::new(0.0, 0.0, 260.0, 200.0));
    }

    fn click(i: &mut Inspector, r: Rect) {
        i.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
            bounds: i.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn header_toggles_section() {
        let mut i = fixture();
        laid_out(&mut i);
        assert_eq!(i.row_rects.len(), 3);
        let h = i.head_rects[0];
        click(&mut i, h);
        laid_out(&mut i);
        assert!(!i.is_open(0));
        assert_eq!(i.row_rects.len(), 1); // only Fill's row remains
    }

    #[test]
    fn row_click_parks_pair() {
        let mut i = fixture();
        laid_out(&mut i);
        let (r, si, ri) = i.row_rects[1];
        click(&mut i, r);
        assert_eq!(i.take_selected(), Some((si, ri)));
        assert_eq!(i.take_selected(), None);
    }

    #[test]
    fn set_value_updates() {
        let mut i = fixture();
        i.set_value(0, 1, "9.0");
        assert_eq!(i.value_of(0, 1), Some("9.0"));
    }

    #[test]
    fn paint_without_painter() {
        let mut i = fixture();
        laid_out(&mut i);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        i.paint(&mut PaintContext {
            list: &mut list,
            bounds: i.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
