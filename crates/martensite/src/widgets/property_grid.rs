//! `PropertyGrid` — a two-column inspector widget.
//!
//! Mirrors Qt's `QtPropertyBrowser`, Xcode's inspector panes, and an
//! editable Ant `Descriptions`: a vertical list of `name | value`
//! rows where the left column shows the property name in muted ink
//! and the right column hosts a lightweight inline editor. Rows can
//! be grouped under collapsible section headers (Xcode inspector
//! sections) via [`PropertyGrid::section`].
//!
//! - `Role::List` on the widget; visible rows are emitted as internal
//!   `Role::ListItem` children carrying the property name as the label
//!   and the value as the description — `Bool` rows also report the
//!   `Toggled` state. Section headers are internal
//!   `Role::DisclosureTriangle` children with `expanded` state.
//! - Editors: [`PropertyEditor::Text`] is display-only (inline text
//!   editing is a future seam), [`PropertyEditor::Bool`] toggles a
//!   checkbox on click, and [`PropertyEditor::Choice`] cycles its
//!   option list on click. Every Bool toggle and Choice cycle parks
//!   `(flat_row_index, new_value)` for [`PropertyGrid::take_changed`].
//! - Scrolling is owned (wheel, keyboard, and the shared `VScrollBar`
//!   strip) the way `ListView` manages its internals.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{PropertyGrid, PropertyRow};
//!
//! let grid = PropertyGrid::new()
//!     .section(
//!         "Transform",
//!         [
//!             PropertyRow::text("Position", "0, 0"),
//!             PropertyRow::bool("Visible", true),
//!         ],
//!     )
//!     .row(PropertyRow::choice("Scale", ["50%", "100%", "200%"]));
//! assert_eq!(grid.row_count(), 3);
//! assert_eq!(grid.section_count(), 1);
//! ```

use accesskit::{Node as AccessKitNode, Toggled};
use glam::Vec2;
use kurbo::BezPath;
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, RenderMinimum, TokenKey, UnderflowPolicy};

use crate::widgets::list_view::{BarRequest, VScrollBar};

/// Row height in logical points.
const ROW_PT: f32 = 26.0;
/// Section header height in logical points.
const HEADER_PT: f32 = 24.0;
/// Cap on the height `measure` requests before scrolling.
const MAX_CONTENT_PT: f32 = 320.0;
/// Scrollbar thickness in logical points.
const BAR: f32 = 10.0;
/// Minimum scrollbar thumb length.
const MIN_THUMB: f32 = 24.0;
/// Horizontal cell padding in logical points.
const PAD_PT: f32 = 10.0;
/// Name/value font size in logical points.
const FONT_PT: f32 = 12.0;
/// Section header font size in logical points.
const HEADER_FONT_PT: f32 = 11.0;
/// Disclosure chevron side in logical points.
const CHEVRON_PT: f32 = 8.0;
/// Bool checkbox side in logical points.
const BOX_PT: f32 = 14.0;
/// Choice caret width in logical points.
const CARET_W_PT: f32 = 9.0;
/// Choice caret height in logical points.
const CARET_H_PT: f32 = 5.0;
/// Default name-column width fraction.
const DEFAULT_NAME_FRAC: f32 = 0.4;

/// Grid face.
const SURFACE_BG: [u8; 4] = [250, 250, 252, 255];
/// Section header band.
const HEADER_BG: [u8; 4] = [243, 245, 248, 255];
/// Outer border.
const BORDER: [u8; 4] = [150, 155, 165, 255];
/// Row/column hairlines.
const HAIRLINE: [u8; 4] = [222, 225, 231, 255];
/// Value ink.
const INK: [u8; 4] = [30, 30, 36, 255];
/// Name-column and header ink.
const INK_MUTED: [u8; 4] = [110, 114, 123, 255];
/// Disabled ink.
const INK_DISABLED: [u8; 4] = [150, 150, 158, 255];
/// Checkbox frame.
const EDGE: [u8; 4] = [110, 115, 125, 255];
/// Check / focus accent.
const ACCENT: [u8; 4] = [60, 110, 220, 255];
/// Focus ring alpha (accent at 50%).
const FOCUS_RING: [u8; 4] = [60, 110, 220, 128];
/// Row/header hover tint.
const HOVER: [u8; 4] = [30, 31, 36, 8];

/// How a [`PropertyRow`] edits its value in the right-hand column.
///
/// # Examples
///
/// ```
/// use martensite::widgets::PropertyEditor;
///
/// let ed = PropertyEditor::Choice(vec!["Small".into(), "Large".into()]);
/// assert_ne!(ed, PropertyEditor::Text);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PropertyEditor {
    /// Read-only text display — inline text editing is a future seam
    /// (a `TextInput` child would ride the editor slot).
    #[default]
    Text,
    /// Checkbox toggle: `value` mirrors `"true"`/`"false"`.
    Bool,
    /// Click cycles the option list (wrapping); `value` mirrors the
    /// current option text.
    Choice(Vec<String>),
}

/// One `name | value` inspector row.
///
/// Rows are data-first: the grid owns them, emits them as internal
/// `Role::ListItem` children, and applies their parked presses
/// centrally (the `ListItemRow` convention) so [`take_changed`] stays
/// the single out-seam.
///
/// [`take_changed`]: PropertyGrid::take_changed
///
/// # Examples
///
/// ```
/// use martensite::widgets::PropertyRow;
///
/// let row = PropertyRow::bool("Enabled", true);
/// assert_eq!(row.value, "true");
/// ```
pub struct PropertyRow {
    /// The property name shown in the left column.
    pub name: String,
    /// The value text — the Bool `"true"`/`"false"` mirror, the
    /// Choice selection, or the literal Text display.
    pub value: String,
    /// The right-column editor kind.
    pub editor: PropertyEditor,
    /// Whether the row accepts input.
    pub enabled: bool,
    /// Bool editor state — `true` iff the box is checked.
    checked: bool,
    /// Choice editor: index of the current option.
    selected: usize,
    /// Name-column width fraction mirrored from the grid.
    name_fraction: f32,
    /// Grid `enabled` mirrored in `layout` (rows can't outlive their
    /// owner's disabled state).
    parent_enabled: bool,
    /// Hovered state mirrored from the grid.
    hovered: bool,
    /// Press parked for the grid to apply (`poll_pending`).
    press_pending: bool,
    /// Cached bounds from the last layout pass.
    bounds: Rect,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl PropertyRow {
    /// Creates a display-only text row with an empty value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyRow;
    ///
    /// let r = PropertyRow::new("Name");
    /// assert_eq!(r.name, "Name");
    /// ```
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: String::new(),
            editor: PropertyEditor::Text,
            enabled: true,
            checked: false,
            selected: 0,
            name_fraction: DEFAULT_NAME_FRAC,
            parent_enabled: true,
            hovered: false,
            press_pending: false,
            bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Creates a display-only text row showing `value`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyRow;
    ///
    /// let r = PropertyRow::text("Position", "12, 8");
    /// assert_eq!(r.value, "12, 8");
    /// ```
    #[must_use]
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(name).value(value)
    }

    /// Creates a checkbox row.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyRow;
    ///
    /// let r = PropertyRow::bool("Visible", true);
    /// assert!(r.is_checked());
    /// assert_eq!(r.value, "true");
    /// ```
    #[must_use]
    pub fn bool(name: impl Into<String>, checked: bool) -> Self {
        let mut row = Self::new(name);
        row.editor = PropertyEditor::Bool;
        row.checked = checked;
        row.value = if checked { "true" } else { "false" }.to_string();
        row
    }

    /// Creates a cycling choice row; the first option is selected.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyRow;
    ///
    /// let r = PropertyRow::choice("Size", ["S", "M", "L"]);
    /// assert_eq!(r.value, "S");
    /// ```
    #[must_use]
    pub fn choice(
        name: impl Into<String>,
        options: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let options: Vec<String> = options.into_iter().map(Into::into).collect();
        let mut row = Self::new(name);
        row.value = options.first().cloned().unwrap_or_default();
        row.editor = PropertyEditor::Choice(options);
        row
    }

    /// Sets the row's value text (builder form of
    /// [`PropertyGrid::set_value`]'s semantics — Bool rows parse
    /// truthy strings, Choice rows snap `selected` to a matching
    /// option).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyRow;
    ///
    /// let r = PropertyRow::new("Model").value("MX-2000");
    /// assert_eq!(r.value, "MX-2000");
    /// ```
    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.assign_value(value.into());
        self
    }

    /// Selects a Choice option by index (clamped).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyRow;
    ///
    /// let r = PropertyRow::choice("Size", ["S", "M"]).selected(1);
    /// assert_eq!(r.value, "M");
    /// ```
    #[must_use]
    pub fn selected(mut self, index: usize) -> Self {
        if let PropertyEditor::Choice(options) = &self.editor {
            if !options.is_empty() {
                self.selected = index.min(options.len() - 1);
                self.value = options[self.selected].clone();
            }
        }
        self
    }

    /// Sets whether the row accepts input.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyRow;
    ///
    /// let r = PropertyRow::new("Row").enabled(false);
    /// assert!(!r.enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The editor kind.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PropertyEditor, PropertyRow};
    ///
    /// let r = PropertyRow::bool("B", false);
    /// assert_eq!(r.editor(), &PropertyEditor::Bool);
    /// ```
    #[inline]
    #[must_use]
    pub fn editor(&self) -> &PropertyEditor {
        &self.editor
    }

    /// Whether a Bool row is checked (always `false` for other
    /// editors).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyRow;
    ///
    /// assert!(PropertyRow::bool("B", true).is_checked());
    /// assert!(!PropertyRow::text("T", "x").is_checked());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_checked(&self) -> bool {
        self.checked
    }

    /// The selected Choice option index (`None` for other editors).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyRow;
    ///
    /// assert_eq!(PropertyRow::choice("C", ["a", "b"]).choice_index(), Some(0));
    /// ```
    #[inline]
    #[must_use]
    pub fn choice_index(&self) -> Option<usize> {
        matches!(self.editor, PropertyEditor::Choice(_)).then_some(self.selected)
    }

    /// Applies a new value text with per-editor semantics (shared by
    /// the `value` builder and `PropertyGrid::set_value`).
    fn assign_value(&mut self, value: String) {
        match &self.editor {
            PropertyEditor::Bool => {
                self.checked = matches!(
                    value.as_str(),
                    "true" | "1" | "yes" | "on" | "checked" | "True"
                );
                self.value = if self.checked { "true" } else { "false" }.to_string();
            }
            PropertyEditor::Choice(options) => {
                if let Some(i) = options.iter().position(|o| *o == value) {
                    self.selected = i;
                }
                self.value = value;
            }
            PropertyEditor::Text => {
                self.value = value;
            }
        }
    }
}

impl Widget for PropertyRow {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(0.0),
            cx.pt(ROW_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        // Internal children share the parent's `hot` node — an
        // enabled row makes the grid a focus stop.
        if self.enabled && self.parent_enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListItem);
        node.set_label(self.name.as_str());
        node.set_description(self.value.as_str());
        if let PropertyEditor::Bool = self.editor {
            node.set_toggled(if self.checked {
                Toggled::True
            } else {
                Toggled::False
            });
        }
        if !matches!(self.editor, PropertyEditor::Text) {
            node.add_action(accesskit::Action::Click);
        }
        if !(self.enabled && self.parent_enabled) {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !(self.enabled && self.parent_enabled) {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                ..
            }
            | WidgetEvent::SemanticAction(SemanticAction::Click) => {
                // Park the activation; the grid applies it via
                // `poll_pending` so `take_changed` stays centralized.
                self.press_pending = true;
                EventResponse::CaptureFocus
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        if b.width() <= 0.0 || b.height() <= 0.0 {
            return;
        }
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let divider = cx.color(TokenKey::DividerColor, HAIRLINE);
        let enabled = self.enabled && self.parent_enabled;
        let muted = cx.color(TokenKey::TextMutedColor, INK_MUTED);
        let ink = if enabled {
            cx.color(TokenKey::TextColor, INK)
        } else {
            cx.color(TokenKey::TextMutedColor, INK_DISABLED)
        };

        if self.hovered {
            cx.list.push_fill_rect(rect, HOVER);
        }

        let pad = cx.pt(PAD_PT);
        // The name column is at least a sliver and never the whole row.
        let name_w =
            (b.width() * self.name_fraction).clamp(cx.pt(40.0).min(b.width()), b.width().max(0.0));
        let font_px = cx.pt(FONT_PT);
        let ty = f64::from(b.min_y() + (b.height() - font_px) / 2.0);

        // Name column (muted), clipped.
        let name_clip = kurbo::Rect::new(
            f64::from(b.min_x() + pad),
            f64::from(b.min_y()),
            f64::from(b.min_x() + name_w - pad / 2.0),
            f64::from(b.max_y()),
        );
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            name_clip,
            kurbo::Point::new(f64::from(b.min_x() + pad), ty),
            &self.name,
            font_px,
            muted,
        );

        // Column separator hairline.
        let sep_x = f64::from(b.min_x() + name_w);
        cx.list.push_fill_rect(
            kurbo::Rect::new(
                sep_x,
                f64::from(b.min_y()),
                sep_x + cx.ptf(1.0),
                f64::from(b.max_y()),
            ),
            divider,
        );

        // Value column.
        let vx = b.min_x() + name_w + pad;
        let vright = b.max_x() - pad / 2.0;
        match &self.editor {
            PropertyEditor::Text => {
                let clip = kurbo::Rect::new(
                    f64::from(vx),
                    f64::from(b.min_y()),
                    f64::from(vright),
                    f64::from(b.max_y()),
                );
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    clip,
                    kurbo::Point::new(f64::from(vx), ty),
                    &self.value,
                    font_px,
                    ink,
                );
            }
            PropertyEditor::Choice(_) => {
                // Down caret docked to the value column's right edge.
                let cw = f64::from(cx.pt(CARET_W_PT));
                let ch = f64::from(cx.pt(CARET_H_PT));
                let cx0 = f64::from(vright) - cw;
                let my = f64::from(b.min_y() + b.height() / 2.0);
                let mut caret = BezPath::new();
                caret.move_to((cx0, my - ch / 2.0));
                caret.line_to((cx0 + cw / 2.0, my + ch / 2.0));
                caret.line_to((cx0 + cw, my - ch / 2.0));
                cx.list.push_stroke_path(caret, cx.pt(1.5), muted);
                let clip = kurbo::Rect::new(
                    f64::from(vx),
                    f64::from(b.min_y()),
                    cx0 - f64::from(cx.pt(4.0)),
                    f64::from(b.max_y()),
                );
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    clip,
                    kurbo::Point::new(f64::from(vx), ty),
                    &self.value,
                    font_px,
                    ink,
                );
            }
            PropertyEditor::Bool => {
                // Checkbox at the left of the value column — the
                // CheckBox geometry at BOX_PT.
                let box_px = cx.pt(BOX_PT);
                let by = b.min_y() + (b.height() - box_px) / 2.0;
                let bx = kurbo::Rect::new(
                    f64::from(vx),
                    f64::from(by),
                    f64::from(vx + box_px),
                    f64::from(by + box_px),
                );
                cx.list.push_stroke_shape(
                    bx,
                    &Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 3.0)),
                    cx.pt(1.0),
                    cx.color(TokenKey::BorderColor, EDGE),
                );
                if self.checked {
                    let x0 = f64::from(vx) + cx.ptf(3.0);
                    let y0 = f64::from(by) + cx.ptf(7.5);
                    let mut tick = BezPath::new();
                    tick.move_to((x0, y0));
                    tick.line_to((x0 + cx.ptf(3.5), y0 + cx.ptf(3.5)));
                    tick.line_to((x0 + cx.ptf(9.0), y0 - cx.ptf(5.0)));
                    cx.list.push_stroke_path(
                        tick,
                        cx.pt(2.0),
                        cx.color(TokenKey::AccentColor, ACCENT),
                    );
                }
            }
        }

        // Bottom hairline separating this row from the next band.
        cx.list.push_fill_rect(
            kurbo::Rect::new(rect.x0, rect.y1 - cx.ptf(1.0), rect.x1, rect.y1),
            divider,
        );
    }
}

impl std::fmt::Debug for PropertyRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropertyRow")
            .field("name", &self.name)
            .field("value", &self.value)
            .field("editor", &self.editor)
            .field("enabled", &self.enabled)
            .finish()
    }
}

/// A collapsible section header — the private internal child that
/// presents a [`PropertySection`]'s chevron + title.
struct SectionHeader {
    /// Header title (mirrored from the section).
    title: String,
    /// Whether the section is collapsed (mirrored).
    collapsed: bool,
    /// Grid `enabled` mirrored in `layout`.
    enabled: bool,
    /// Hovered state mirrored from the grid.
    hovered: bool,
    /// Desired collapsed state parked for the grid to apply.
    toggle_pending: Option<bool>,
    /// Cached bounds from the last layout pass.
    bounds: Rect,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl SectionHeader {
    fn new() -> Self {
        Self {
            title: String::new(),
            collapsed: false,
            enabled: true,
            hovered: false,
            toggle_pending: None,
            bounds: Rect::default(),
            text_painter: None,
        }
    }
}

impl Widget for SectionHeader {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(0.0),
            cx.pt(HEADER_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::DisclosureTriangle);
        node.set_label(self.title.as_str());
        node.set_expanded(!self.collapsed);
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Focus);
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                ..
            } => EventResponse::CaptureFocus,
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            }
            | WidgetEvent::SemanticAction(SemanticAction::Click) => {
                self.toggle_pending = Some(!self.collapsed);
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(SemanticAction::Expand) => {
                self.toggle_pending = Some(false);
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(SemanticAction::Collapse) => {
                self.toggle_pending = Some(true);
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        if b.width() <= 0.0 || b.height() <= 0.0 {
            return;
        }
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        cx.list
            .push_fill_rect(rect, cx.color(TokenKey::SurfaceColor, HEADER_BG));
        if self.hovered {
            cx.list.push_fill_rect(rect, HOVER);
        }
        let muted = cx.color(TokenKey::TextMutedColor, INK_MUTED);

        // Disclosure chevron — right-pointing when collapsed, down
        // when expanded (the `Disclosure` geometry).
        let cs = cx.pt(CHEVRON_PT);
        let cy = b.min_y() + (b.height() - cs) / 2.0;
        let cxl = b.min_x() + cx.pt(8.0);
        let x0 = f64::from(cxl);
        let y0 = f64::from(cy);
        let s = f64::from(cs);
        let mut caret = BezPath::new();
        if self.collapsed {
            caret.move_to((x0 + s * 0.25, y0));
            caret.line_to((x0 + s * 0.75, y0 + s / 2.0));
            caret.line_to((x0 + s * 0.25, y0 + s));
        } else {
            caret.move_to((x0, y0 + s * 0.25));
            caret.line_to((x0 + s / 2.0, y0 + s * 0.75));
            caret.line_to((x0 + s, y0 + s * 0.25));
        }
        cx.list.push_stroke_path(caret, cx.pt(1.5), muted);

        // Title, clipped to the header band.
        let font_px = cx.pt(HEADER_FONT_PT);
        let title_x = cxl + cs + cx.pt(6.0);
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(title_x),
                f64::from(b.min_y()),
                f64::from(b.max_x() - cx.pt(4.0)),
                f64::from(b.max_y()),
            ),
            kurbo::Point::new(
                f64::from(title_x),
                f64::from(b.min_y() + (b.height() - font_px) / 2.0),
            ),
            &self.title,
            font_px,
            muted,
        );

        // Bottom hairline.
        let divider = cx.color(TokenKey::DividerColor, HAIRLINE);
        cx.list.push_fill_rect(
            kurbo::Rect::new(rect.x0, rect.y1 - cx.ptf(1.0), rect.x1, rect.y1),
            divider,
        );
    }
}

/// A titled, collapsible group of [`PropertyRow`]s inside a
/// [`PropertyGrid`] — an Xcode inspector section.
///
/// Constructed by [`PropertyGrid::section`]; inspect through
/// [`PropertyGrid::is_collapsed`]/[`PropertyGrid::set_collapsed`].
///
/// # Examples
///
/// ```
/// use martensite::widgets::{PropertyGrid, PropertyRow};
///
/// let g = PropertyGrid::new().section("General", [PropertyRow::text("a", "1")]);
/// assert!(!g.is_collapsed(0));
/// ```
pub struct PropertySection {
    /// The header title.
    pub title: String,
    /// Whether the section's rows are hidden.
    pub collapsed: bool,
    /// Flat indices (into `PropertyGrid::rows`) of the member rows.
    rows: Vec<usize>,
    /// The header widget (internal child).
    header: SectionHeader,
}

impl PropertySection {
    /// Creates an empty section (normally populated by
    /// [`PropertyGrid::section`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertySection;
    ///
    /// let s = PropertySection::new("Transform");
    /// assert_eq!(s.title, "Transform");
    /// ```
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            collapsed: false,
            rows: Vec::new(),
            header: SectionHeader::new(),
        }
    }

    /// Sets the collapsed state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertySection;
    ///
    /// let s = PropertySection::new("S").collapsed(true);
    /// assert!(s.collapsed);
    /// ```
    #[must_use]
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    /// The number of member rows.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertySection;
    ///
    /// assert_eq!(PropertySection::new("S").row_count(), 0);
    /// ```
    #[inline]
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

impl std::fmt::Debug for PropertySection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropertySection")
            .field("title", &self.title)
            .field("collapsed", &self.collapsed)
            .field("rows", &self.rows.len())
            .finish()
    }
}

/// Insertion-order entry in a [`PropertyGrid`].
#[derive(Copy, Clone, Debug)]
enum GridItem {
    /// Index into `sections`.
    Section(usize),
    /// Index into `rows`.
    Row(usize),
}

/// One entry in the emitted child list — a section header or a row.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ChildRef {
    /// Index into `sections` (the header child).
    Section(usize),
    /// Index into `rows`.
    Row(usize),
}

/// Identifies a row inside a [`PropertyGrid`] — by flat row index
/// (`usize`, position in insertion order across all sections) or by
/// name (`&str`/`String`, first match wins).
///
/// # Examples
///
/// ```
/// use martensite::widgets::{PropertyGrid, PropertyRow};
///
/// let g = PropertyGrid::new().row(PropertyRow::text("Name", "Cube"));
/// assert_eq!(g.value(0), Some("Cube"));
/// assert_eq!(g.value("Name"), Some("Cube"));
/// ```
pub trait PropertyRowKey {
    /// Resolves the key to a flat row index, if it exists.
    fn row_index(&self, grid: &PropertyGrid) -> Option<usize>;
}

impl PropertyRowKey for usize {
    fn row_index(&self, grid: &PropertyGrid) -> Option<usize> {
        (*self < grid.row_count()).then_some(*self)
    }
}

impl PropertyRowKey for &usize {
    fn row_index(&self, grid: &PropertyGrid) -> Option<usize> {
        (**self < grid.row_count()).then_some(**self)
    }
}

impl PropertyRowKey for &str {
    fn row_index(&self, grid: &PropertyGrid) -> Option<usize> {
        grid.rows.iter().position(|r| r.name == *self)
    }
}

impl PropertyRowKey for String {
    fn row_index(&self, grid: &PropertyGrid) -> Option<usize> {
        grid.rows.iter().position(|r| r.name == *self)
    }
}

impl PropertyRowKey for &String {
    fn row_index(&self, grid: &PropertyGrid) -> Option<usize> {
        grid.rows.iter().position(|r| &r.name == *self)
    }
}

/// A two-column property inspector.
///
/// Rows are stored flat (insertion order); sections reference their
/// members by flat index, so [`value`](Self::value),
/// [`set_value`](Self::set_value), and [`take_changed`](Self::take_changed)
/// all share one index space.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{PropertyGrid, PropertyRow};
///
/// let grid = PropertyGrid::new()
///     .label("Inspector")
///     .row(PropertyRow::bool("Enabled", false));
/// assert_eq!(grid.value("Enabled"), Some("false"));
/// ```
pub struct PropertyGrid {
    /// Whether the grid accepts input.
    pub enabled: bool,
    /// Optional accessible label.
    pub label: Option<String>,
    /// Name-column width fraction (0..1, default `0.4`) — set via
    /// [`columns`](Self::columns); clamped into a sane range at
    /// layout/mirror time.
    pub name_fraction: f32,
    /// Insertion order of sections and ungrouped rows.
    order: Vec<GridItem>,
    /// The sections, in creation order.
    sections: Vec<PropertySection>,
    /// All rows, flat, in insertion order.
    rows: Vec<PropertyRow>,
    /// `(flat_row_index, new_value)` parked by the last edit.
    changed: Option<(usize, String)>,
    /// Visible emitted children (headers + uncollapsed rows).
    child_map: Vec<ChildRef>,
    /// Per-child bounds from the last layout pass.
    child_rects: Vec<Rect>,
    /// Hovered child.
    hovered: Option<ChildRef>,
    /// Whether the widget holds keyboard focus.
    has_focus: bool,
    /// Vertical scroll offset in device pixels.
    scroll_y: f32,
    /// Row viewport (widget bounds minus the shown bar).
    viewport: Rect,
    /// The vertical scrollbar (last internal child).
    vbar: VScrollBar,
    /// Vertical bar rect when shown.
    vbar_rect: Option<Rect>,
    /// Thumb-drag state: grab offset inside the thumb.
    thumb_drag: Option<f32>,
    /// Display scale from `layout`.
    scale: f32,
    /// Cached widget bounds.
    cached_bounds: Rect,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl PropertyGrid {
    /// Creates an empty inspector.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyGrid;
    ///
    /// let g = PropertyGrid::new();
    /// assert_eq!(g.row_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            enabled: true,
            label: None,
            name_fraction: DEFAULT_NAME_FRAC,
            order: Vec::new(),
            sections: Vec::new(),
            rows: Vec::new(),
            changed: None,
            child_map: Vec::new(),
            child_rects: Vec::new(),
            hovered: None,
            has_focus: false,
            scroll_y: 0.0,
            viewport: Rect::default(),
            vbar: VScrollBar::new(),
            vbar_rect: None,
            thumb_drag: None,
            scale: 1.0,
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Appends an ungrouped row (builder form of
    /// [`add_row`](Self::add_row)).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PropertyGrid, PropertyRow};
    ///
    /// let g = PropertyGrid::new().row(PropertyRow::text("Name", "Cube"));
    /// assert_eq!(g.row_count(), 1);
    /// ```
    #[must_use]
    pub fn row(mut self, row: PropertyRow) -> Self {
        self.add_row(row);
        self
    }

    /// Appends an ungrouped row.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PropertyGrid, PropertyRow};
    ///
    /// let mut g = PropertyGrid::new();
    /// g.add_row(PropertyRow::text("a", "1"));
    /// assert_eq!(g.row_count(), 1);
    /// ```
    pub fn add_row(&mut self, row: PropertyRow) {
        self.rows.push(row);
        self.order.push(GridItem::Row(self.rows.len() - 1));
        self.rebuild_map();
    }

    /// Appends a collapsible section with `rows` (builder form of
    /// [`add_section`](Self::add_section)).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PropertyGrid, PropertyRow};
    ///
    /// let g = PropertyGrid::new().section("Transform", [PropertyRow::text("X", "0")]);
    /// assert_eq!(g.section_count(), 1);
    /// ```
    #[must_use]
    pub fn section(
        mut self,
        title: impl Into<String>,
        rows: impl IntoIterator<Item = PropertyRow>,
    ) -> Self {
        self.add_section(title, rows);
        self
    }

    /// Appends a collapsible section with `rows`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PropertyGrid, PropertyRow};
    ///
    /// let mut g = PropertyGrid::new();
    /// g.add_section("Transform", [PropertyRow::bool("Visible", true)]);
    /// assert_eq!(g.row_count(), 1);
    /// ```
    pub fn add_section(
        &mut self,
        title: impl Into<String>,
        rows: impl IntoIterator<Item = PropertyRow>,
    ) {
        let si = self.sections.len();
        let mut section = PropertySection::new(title);
        for row in rows {
            section.rows.push(self.rows.len());
            self.rows.push(row);
        }
        self.sections.push(section);
        self.order.push(GridItem::Section(si));
        self.rebuild_map();
    }

    /// Sets the accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyGrid;
    ///
    /// let g = PropertyGrid::new().label("Inspector");
    /// assert_eq!(g.label.as_deref(), Some("Inspector"));
    /// ```
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets whether the grid accepts input.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyGrid;
    ///
    /// let g = PropertyGrid::new().enabled(false);
    /// assert!(!g.enabled);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets the name-column width fraction (default `0.4`, clamped to
    /// `0.1..=0.9`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyGrid;
    ///
    /// let g = PropertyGrid::new().columns(0.5);
    /// assert_eq!(g.name_fraction, 0.5);
    /// ```
    #[must_use]
    pub fn columns(mut self, name_fraction: f32) -> Self {
        if name_fraction.is_finite() {
            self.name_fraction = name_fraction.clamp(0.1, 0.9);
        }
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so labels emit real
    /// glyph runs instead of `DrawText` placeholder boxes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyGrid;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let g = PropertyGrid::new().with_text_painter(shared_painter());
    /// ```
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The total number of rows (all sections plus ungrouped).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PropertyGrid, PropertyRow};
    ///
    /// let g = PropertyGrid::new()
    ///     .section("S", [PropertyRow::new("a")])
    ///     .row(PropertyRow::new("b"));
    /// assert_eq!(g.row_count(), 2);
    /// ```
    #[inline]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// The number of sections.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyGrid;
    ///
    /// assert_eq!(PropertyGrid::new().section_count(), 0);
    /// ```
    #[inline]
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// The title of section `index`, if in range.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyGrid;
    ///
    /// let g = PropertyGrid::new().section("T", []);
    /// assert_eq!(g.section_title(0), Some("T"));
    /// ```
    #[inline]
    pub fn section_title(&self, index: usize) -> Option<&str> {
        self.sections.get(index).map(|s| s.title.as_str())
    }

    /// Accesses a row by flat index.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PropertyGrid, PropertyRow};
    ///
    /// let g = PropertyGrid::new().row(PropertyRow::text("a", "1"));
    /// assert_eq!(g.row_at(0).map(|r| r.name.as_str()), Some("a"));
    /// ```
    #[inline]
    pub fn row_at(&self, index: usize) -> Option<&PropertyRow> {
        self.rows.get(index)
    }

    /// Accesses a row mutably by flat index.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PropertyGrid, PropertyRow};
    ///
    /// let mut g = PropertyGrid::new().row(PropertyRow::text("a", "1"));
    /// g.row_at_mut(0).unwrap().value = "2".into();
    /// assert_eq!(g.value(0), Some("2"));
    /// ```
    #[inline]
    pub fn row_at_mut(&mut self, index: usize) -> Option<&mut PropertyRow> {
        self.rows.get_mut(index)
    }

    /// The value text of the row addressed by `key` (flat index or
    /// name — see [`PropertyRowKey`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PropertyGrid, PropertyRow};
    ///
    /// let g = PropertyGrid::new().row(PropertyRow::text("a", "1"));
    /// assert_eq!(g.value(0), Some("1"));
    /// assert_eq!(g.value("missing"), None);
    /// ```
    #[inline]
    pub fn value(&self, key: impl PropertyRowKey) -> Option<&str> {
        self.rows
            .get(key.row_index(self)?)
            .map(|r| r.value.as_str())
    }

    /// Sets a row's value (Bool rows parse truthy strings; Choice
    /// rows snap to the matching option). Returns `false` when `key`
    /// doesn't resolve.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PropertyGrid, PropertyRow};
    ///
    /// let mut g = PropertyGrid::new().row(PropertyRow::bool("B", false));
    /// assert!(g.set_value("B", "true"));
    /// assert!(g.row_at(0).unwrap().is_checked());
    /// ```
    pub fn set_value(&mut self, key: impl PropertyRowKey, value: impl Into<String>) -> bool {
        let Some(i) = key.row_index(self) else {
            return false;
        };
        self.rows[i].assign_value(value.into());
        true
    }

    /// Drains the `(flat_row_index, new_value)` parked by the last
    /// Bool toggle or Choice cycle.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyGrid;
    ///
    /// let mut g = PropertyGrid::new();
    /// assert_eq!(g.take_changed(), None);
    /// ```
    pub fn take_changed(&mut self) -> Option<(usize, String)> {
        self.changed.take()
    }

    /// Whether section `index` is collapsed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PropertyGrid, PropertyRow};
    ///
    /// let g = PropertyGrid::new().section("S", [PropertyRow::new("a")]);
    /// assert!(!g.is_collapsed(0));
    /// ```
    #[inline]
    pub fn is_collapsed(&self, index: usize) -> bool {
        self.sections.get(index).is_some_and(|s| s.collapsed)
    }

    /// Sets a section's collapsed state; hidden rows drop out of
    /// layout, paint, hit-testing, and the accessibility tree (the
    /// `PanelSet` convention).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PropertyGrid, PropertyRow};
    ///
    /// let mut g = PropertyGrid::new().section("S", [PropertyRow::new("a")]);
    /// g.set_collapsed(0, true);
    /// assert!(g.is_collapsed(0));
    /// ```
    pub fn set_collapsed(&mut self, index: usize, collapsed: bool) {
        let Some(section) = self.sections.get_mut(index) else {
            return;
        };
        if section.collapsed != collapsed {
            section.collapsed = collapsed;
            self.rebuild_map();
            self.child_rects = self.stack_rects();
            self.sync_hover();
        }
    }

    /// Toggles a section's collapsed state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{PropertyGrid, PropertyRow};
    ///
    /// let mut g = PropertyGrid::new().section("S", [PropertyRow::new("a")]);
    /// g.toggle_collapsed(0);
    /// assert!(g.is_collapsed(0));
    /// ```
    pub fn toggle_collapsed(&mut self, index: usize) {
        let collapsed = self.is_collapsed(index);
        self.set_collapsed(index, !collapsed);
    }

    /// The current clamped vertical scroll offset in device pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyGrid;
    ///
    /// assert_eq!(PropertyGrid::new().scroll_offset(), 0.0);
    /// ```
    #[inline]
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_y
    }

    /// Maximum scroll offset: `max(0, content - viewport)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyGrid;
    ///
    /// assert_eq!(PropertyGrid::new().max_scroll_offset(), 0.0);
    /// ```
    #[inline]
    pub fn max_scroll_offset(&self) -> f32 {
        self.max_scroll()
    }

    /// Sets the scroll offset in device pixels, clamped.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyGrid;
    ///
    /// let mut g = PropertyGrid::new();
    /// g.set_scroll_offset(-5.0);
    /// assert_eq!(g.scroll_offset(), 0.0); // clamped
    /// ```
    pub fn set_scroll_offset(&mut self, offset: f32) {
        self.set_scroll(offset);
    }

    /// Scrolls by `delta` device pixels, clamped. Returns the
    /// actually-applied delta — `0.0` means nothing was consumed
    /// (chaining boundary for an ancestor scroll region).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyGrid;
    ///
    /// let mut g = PropertyGrid::new();
    /// assert_eq!(g.scroll_by(10.0), 0.0); // nothing to scroll
    /// ```
    pub fn scroll_by(&mut self, delta: f32) -> f32 {
        let old = self.scroll_y;
        self.set_scroll(old + delta);
        self.scroll_y - old
    }

    /// Applies pending presses/toggles recorded by row and header
    /// children and scroll requests parked by the scrollbar (AT
    /// actions delivered through `WidgetArena::internal_widget_mut`).
    ///
    /// Called automatically from `event`, `layout`, and `a11y_prepare`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::PropertyGrid;
    ///
    /// let mut g = PropertyGrid::new();
    /// g.poll_pending();
    /// ```
    pub fn poll_pending(&mut self) {
        let mut structural = false;
        for section in &mut self.sections {
            if let Some(target) = section.header.toggle_pending.take() {
                if section.collapsed != target {
                    section.collapsed = target;
                    structural = true;
                }
                // Keep the header's mirror current even between
                // layout passes — the next release computes its
                // parked target from it.
                section.header.collapsed = section.collapsed;
            }
        }
        for i in 0..self.rows.len() {
            if self.rows[i].press_pending {
                self.rows[i].press_pending = false;
                self.activate_row(i);
            }
        }
        if let Some(req) = self.vbar.pending.take() {
            match req {
                BarRequest::By(d) => {
                    self.scroll_by(d);
                }
                BarRequest::To(o) => {
                    self.set_scroll(o);
                }
            }
        }
        if structural {
            self.rebuild_map();
            self.child_rects = self.stack_rects();
            self.sync_hover();
        }
    }

    /// Row height in device pixels at the cached display scale.
    fn row_px(&self) -> f32 {
        ROW_PT * self.scale
    }

    /// Section header height in device pixels.
    fn header_px(&self) -> f32 {
        HEADER_PT * self.scale
    }

    /// Full stacked content height in device pixels.
    fn content_height(&self) -> f32 {
        let row_px = self.row_px();
        let header_px = self.header_px();
        let mut h = 0.0;
        for item in &self.order {
            match *item {
                GridItem::Section(si) => {
                    h += header_px;
                    let s = &self.sections[si];
                    if !s.collapsed {
                        h += s.rows.len() as f32 * row_px;
                    }
                }
                GridItem::Row(_) => h += row_px,
            }
        }
        h
    }

    /// Maximum scroll offset.
    fn max_scroll(&self) -> f32 {
        (self.content_height() - self.viewport.height()).max(0.0)
    }

    /// Rebuilds the emitted-child list from `order` + collapsed state.
    fn rebuild_map(&mut self) {
        self.child_map.clear();
        for item in &self.order {
            match *item {
                GridItem::Section(si) => {
                    self.child_map.push(ChildRef::Section(si));
                    if !self.sections[si].collapsed {
                        for &ri in &self.sections[si].rows {
                            self.child_map.push(ChildRef::Row(ri));
                        }
                    }
                }
                GridItem::Row(ri) => self.child_map.push(ChildRef::Row(ri)),
            }
        }
        if let Some(h) = self.hovered {
            if !self.child_map.contains(&h) {
                self.hovered = None;
            }
        }
    }

    /// Computes the stacked bounds of every emitted child in the
    /// current scroll state.
    fn stack_rects(&self) -> Vec<Rect> {
        let row_px = self.row_px();
        let header_px = self.header_px();
        let mut rects = Vec::with_capacity(self.child_map.len());
        let mut y = self.viewport.min_y() - self.scroll_y;
        for cr in &self.child_map {
            let h = match cr {
                ChildRef::Section(_) => header_px,
                ChildRef::Row(_) => row_px,
            };
            rects.push(Rect::new(
                self.viewport.min_x(),
                y,
                self.viewport.width(),
                h,
            ));
            y += h;
        }
        rects
    }

    /// Mirrors grid state onto rows, headers, and the scrollbar.
    fn sync_children(&mut self) {
        for row in &mut self.rows {
            row.parent_enabled = self.enabled;
            row.name_fraction = self.name_fraction;
            row.text_painter = self.text_painter.clone();
        }
        for section in &mut self.sections {
            section.header.title = section.title.clone();
            section.header.collapsed = section.collapsed;
            section.header.enabled = self.enabled;
            section.header.text_painter = self.text_painter.clone();
        }
        self.sync_hover();
    }

    /// Mirrors the hovered child onto row/header flags.
    fn sync_hover(&mut self) {
        for i in 0..self.child_map.len() {
            let hot = self.hovered == Some(self.child_map[i]);
            match self.child_map[i] {
                ChildRef::Section(si) => self.sections[si].header.hovered = hot,
                ChildRef::Row(ri) => self.rows[ri].hovered = hot,
            }
        }
    }

    /// Mirrors scroll state onto the scrollbar child.
    fn sync_bars(&mut self) {
        self.vbar.offset = self.scroll_y;
        self.vbar.max_offset = self.max_scroll();
        self.vbar.thumb = self.vbar_thumb();
        self.vbar.active = self.thumb_drag.is_some();
    }

    /// Sets the scroll offset, clamped; re-stacks child bounds.
    fn set_scroll(&mut self, offset: f32) {
        let clamped = if offset.is_finite() { offset } else { 0.0 };
        self.scroll_y = clamped.clamp(0.0, self.max_scroll());
        self.child_rects = self.stack_rects();
        self.sync_bars();
    }

    /// Emitted-child index under `position` (viewport-space hit test).
    fn child_at(&self, position: Vec2) -> Option<usize> {
        if !self.viewport.contains(position) {
            return None;
        }
        self.child_rects.iter().position(|r| r.contains(position))
    }

    /// Scrolls the minimum amount that makes child `target` visible.
    fn ensure_child_visible(&mut self, target: ChildRef) {
        let Some(i) = self.child_map.iter().position(|c| *c == target) else {
            return;
        };
        let Some(r) = self.child_rects.get(i).copied() else {
            return;
        };
        if r.min_y() < self.viewport.min_y() {
            self.scroll_by(r.min_y() - self.viewport.min_y());
        } else if r.max_y() > self.viewport.max_y() {
            self.scroll_by(r.max_y() - self.viewport.max_y());
        }
    }

    /// Applies a row activation: Bool toggles, Choice cycles (wrapping),
    /// Text is inert. Edits park `(flat_index, new_value)` for
    /// [`take_changed`](Self::take_changed).
    fn activate_row(&mut self, index: usize) {
        let Some(row) = self.rows.get_mut(index) else {
            return;
        };
        let new_value = match &row.editor {
            PropertyEditor::Bool => {
                row.checked = !row.checked;
                Some(if row.checked { "true" } else { "false" }.to_string())
            }
            PropertyEditor::Choice(options) => {
                if options.is_empty() {
                    None
                } else {
                    row.selected = (row.selected + 1) % options.len();
                    Some(options[row.selected].clone())
                }
            }
            PropertyEditor::Text => None,
        };
        if let Some(v) = new_value {
            row.value = v.clone();
            self.changed = Some((index, v));
        }
    }

    /// The vertical scrollbar thumb rect, if the bar is shown.
    fn vbar_thumb(&self) -> Option<Rect> {
        let track = self.vbar_rect?;
        let content = self.content_height();
        if content <= 0.0 {
            return None;
        }
        let track_len = track.height();
        let frac = (self.viewport.height() / content).clamp(0.0, 1.0);
        let thumb_len = (track_len * frac)
            .max(MIN_THUMB * self.scale)
            .min(track_len);
        let max = self.max_scroll();
        let t = if max > 0.0 { self.scroll_y / max } else { 0.0 };
        let top = track.min_y() + t * (track_len - thumb_len);
        Some(Rect::new(track.min_x(), top, track.width(), thumb_len))
    }

    /// Maps a pointer position inside the bar track to a thumb grab or
    /// a page scroll. Returns `true` if the press was consumed.
    fn press_bar(&mut self, position: Vec2) -> bool {
        let Some(thumb) = self.vbar_thumb() else {
            return false;
        };
        if thumb.contains(position) {
            self.thumb_drag = Some(position.y - thumb.min_y());
        } else {
            let sign = if position.y < thumb.min_y() {
                -1.0
            } else {
                1.0
            };
            self.scroll_by(sign * self.viewport.height() * 0.9);
        }
        true
    }

    /// Continues an active thumb drag.
    fn drag_thumb(&mut self, position: Vec2) {
        let Some(grab) = self.thumb_drag else {
            return;
        };
        let Some(track) = self.vbar_rect else {
            return;
        };
        let thumb_len = self.vbar_thumb().map(|t| t.height()).unwrap_or(0.0);
        let usable = (track.height() - thumb_len).max(f32::EPSILON);
        let frac = ((position.y - track.min_y() - grab) / usable).clamp(0.0, 1.0);
        self.set_scroll(frac * self.max_scroll());
    }

    /// Forwards a positional event to the emitted children,
    /// bounds-gated, topmost-first (the `Wizard` dispatch pattern).
    fn forward_to_children(&mut self, cx: &mut EventContext) -> EventResponse {
        let Some(pos) = cx.event.position() else {
            return EventResponse::Ignored;
        };
        for i in (0..self.child_map.len()).rev() {
            let Some(b) = self.child_rects.get(i).copied() else {
                continue;
            };
            if !b.contains(pos) {
                continue;
            }
            let mut child_cx = EventContext {
                event: cx.event,
                bounds: b,
                scale: cx.scale,
            };
            let response = match self.child_map[i] {
                ChildRef::Section(si) => self.sections[si].header.event(&mut child_cx),
                ChildRef::Row(ri) => self.rows[ri].event(&mut child_cx),
            };
            if response != EventResponse::Ignored {
                return response;
            }
        }
        EventResponse::Ignored
    }
}

impl Default for PropertyGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PropertyGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropertyGrid")
            .field("rows", &self.rows.len())
            .field("sections", &self.sections.len())
            .field("scroll_y", &self.scroll_y)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Widget for PropertyGrid {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let row_px = cx.pt(ROW_PT);
        let header_px = cx.pt(HEADER_PT);
        let mut content_h = 0.0;
        for item in &self.order {
            match *item {
                GridItem::Section(si) => {
                    content_h += header_px;
                    if !self.sections[si].collapsed {
                        content_h += self.sections[si].rows.len() as f32 * row_px;
                    }
                }
                GridItem::Row(_) => content_h += row_px,
            }
        }
        let max_w = constraints.max_size.x.max(0.0);
        let max_h = constraints.max_size.y.max(0.0);
        Vec2::new(
            cx.pt(200.0).min(max_w).max(cx.pt(120.0).min(max_w)),
            content_h
                .min(cx.pt(MAX_CONTENT_PT))
                .max(cx.pt(48.0).min(max_h))
                .min(max_h),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 120.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.scale = cx.scale;
        self.vbar.scale = cx.scale;
        // The grid is a single keyboard-focus stop (scroll keys).
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
        self.poll_pending();
        self.rebuild_map();
        self.sync_children();
        // Smart scrollbar: shown only when content overflows.
        let bar = cx.pt(BAR);
        let show_v = self.content_height() > bounds.height();
        let mut viewport = bounds;
        self.vbar_rect = None;
        if show_v {
            viewport.size.x = (viewport.width() - bar).max(0.0);
            self.vbar_rect = Some(Rect::new(
                bounds.max_x() - bar,
                bounds.min_y(),
                bar,
                bounds.height(),
            ));
        }
        self.viewport = viewport;
        self.vbar.shown = show_v;
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());
        self.child_rects = self.stack_rects();
        for i in 0..self.child_map.len() {
            let r = self.child_rects[i];
            match self.child_map[i] {
                ChildRef::Section(si) => cx.layout_child(&mut self.sections[si].header, r),
                ChildRef::Row(ri) => cx.layout_child(&mut self.rows[ri], r),
            }
        }
        if let Some(rect) = self.vbar_rect {
            cx.layout_child(&mut self.vbar, rect);
        }
        self.sync_bars();
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::List);
        node.set_orientation(accesskit::Orientation::Vertical);
        let visible_rows = self
            .child_map
            .iter()
            .filter(|c| matches!(c, ChildRef::Row(_)))
            .count();
        node.set_size_of_set(visible_rows);
        if let Some(ref label) = self.label {
            node.set_label(label.as_str());
        }
        let max = self.max_scroll();
        node.set_scroll_y(f64::from(self.scroll_y));
        node.set_scroll_y_min(0.0);
        node.set_scroll_y_max(f64::from(max));
        node.add_action(accesskit::Action::ScrollUp);
        node.add_action(accesskit::Action::ScrollDown);
        node.add_action(accesskit::Action::SetScrollOffset);
        node.add_action(accesskit::Action::Focus);
        node.add_child_action(accesskit::Action::ScrollIntoView);
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn a11y_prepare(&mut self) {
        self.poll_pending();
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        self.poll_pending();
        let response = match cx.event {
            WidgetEvent::PointerPressed {
                position,
                button: PointerButton::Primary,
                ..
            } => {
                if self.vbar_rect.is_some_and(|r| r.contains(*position)) {
                    self.press_bar(*position);
                    EventResponse::CapturePointer
                } else {
                    self.forward_to_children(cx)
                }
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.thumb_drag.is_some() {
                    self.thumb_drag = None;
                    self.sync_bars();
                    EventResponse::ReleasePointer
                } else {
                    self.forward_to_children(cx)
                }
            }
            WidgetEvent::PointerMoved { position } => {
                if self.thumb_drag.is_some() {
                    self.drag_thumb(*position);
                    return EventResponse::RequestRepaint;
                }
                let hov = self.child_at(*position).map(|i| self.child_map[i]);
                if hov != self.hovered {
                    self.hovered = hov;
                    self.sync_hover();
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::PointerLeave => {
                if self.hovered.take().is_some() {
                    self.sync_hover();
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::Scroll { delta, .. } => {
                // Nested chaining: an unconsumed delta returns `Ignored`
                // so an ancestor scroll region can take it.
                let applied = self.scroll_by(delta.y);
                if applied != 0.0 {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowUp" => {
                    self.scroll_by(-self.row_px());
                    EventResponse::RequestRepaint
                }
                "ArrowDown" => {
                    self.scroll_by(self.row_px());
                    EventResponse::RequestRepaint
                }
                "PageUp" => {
                    self.scroll_by(-self.viewport.height() * 0.9);
                    EventResponse::RequestRepaint
                }
                "PageDown" => {
                    self.scroll_by(self.viewport.height() * 0.9);
                    EventResponse::RequestRepaint
                }
                "Home" => {
                    self.set_scroll(0.0);
                    EventResponse::RequestRepaint
                }
                "End" => {
                    self.set_scroll(self.max_scroll());
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::FocusGained => {
                self.has_focus = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.has_focus = false;
                self.thumb_drag = None;
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::Focus => EventResponse::CaptureFocus,
                SemanticAction::ScrollUp => {
                    self.scroll_by(-self.row_px());
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollDown => {
                    self.scroll_by(self.row_px());
                    EventResponse::RequestRepaint
                }
                SemanticAction::SetScrollOffset(offset) => {
                    self.set_scroll(offset.y);
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollToPoint(point) => {
                    self.set_scroll(point.y - self.viewport.min_y());
                    EventResponse::RequestRepaint
                }
                SemanticAction::ScrollIntoView => {
                    if let Some(target) = self.hovered {
                        self.ensure_child_visible(target);
                    }
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        };
        self.poll_pending();
        response
    }

    fn paint(&self, cx: &mut PaintContext) {
        // Chrome: the inspector face. Rows, headers, and the scrollbar
        // paint through the child walk, clipped to the widget bounds
        // by `clips_children`.
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        cx.list
            .push_fill_rect(rect, cx.color(TokenKey::SurfaceColor, SURFACE_BG));
        cx.list
            .push_stroke_rect(rect, cx.pt(1.0), cx.color(TokenKey::BorderColor, BORDER));
        if self.has_focus {
            let accent = cx.color(TokenKey::AccentColor, ACCENT);
            cx.list.push_stroke_rect(
                rect,
                cx.pt(2.0),
                [accent[0], accent[1], accent[2], FOCUS_RING[3]],
            );
        }
    }

    fn child_count(&self) -> usize {
        self.child_map.len() + 1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        match self.child_map.get(index) {
            Some(ChildRef::Section(si)) => Some(&self.sections[*si].header as &dyn Widget),
            Some(ChildRef::Row(ri)) => Some(&self.rows[*ri] as &dyn Widget),
            None => (index == self.child_map.len()).then_some(&self.vbar as &dyn Widget),
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        match self.child_map.get(index).copied() {
            Some(ChildRef::Section(si)) => Some(&mut self.sections[si].header as &mut dyn Widget),
            Some(ChildRef::Row(ri)) => Some(&mut self.rows[ri] as &mut dyn Widget),
            None => {
                if index == self.child_map.len() {
                    Some(&mut self.vbar as &mut dyn Widget)
                } else {
                    None
                }
            }
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index < self.child_map.len() {
            self.child_rects.get(index).copied()
        } else if index == self.child_map.len() {
            self.vbar_rect
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(grid: &mut PropertyGrid, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        grid.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn event(grid: &mut PropertyGrid, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: grid.cached_bounds,
            scale: 1.0,
        };
        grid.event(&mut cx)
    }

    fn press(grid: &mut PropertyGrid, pos: Vec2) -> EventResponse {
        event(
            grid,
            &WidgetEvent::PointerPressed {
                position: pos,
                button: PointerButton::Primary,
                count: 1,
            },
        )
    }

    fn release(grid: &mut PropertyGrid, pos: Vec2) -> EventResponse {
        event(
            grid,
            &WidgetEvent::PointerReleased {
                position: pos,
                button: PointerButton::Primary,
            },
        )
    }

    #[test]
    fn grid_builder_defaults() {
        let g = PropertyGrid::new();
        assert_eq!(g.row_count(), 0);
        assert_eq!(g.section_count(), 0);
        assert!(g.enabled);
        assert_eq!(g.name_fraction, 0.4);
        assert!(g.label.is_none());
    }

    #[test]
    fn grid_sections_and_rows_count() {
        let g = PropertyGrid::new()
            .section(
                "Transform",
                [
                    PropertyRow::text("Position", "0, 0"),
                    PropertyRow::bool("Visible", true),
                ],
            )
            .row(PropertyRow::choice("Mode", ["a", "b"]));
        assert_eq!(g.section_count(), 1);
        assert_eq!(g.row_count(), 3);
        assert_eq!(g.section_title(0), Some("Transform"));
        assert_eq!(g.value(0), Some("0, 0"));
        assert_eq!(g.value("Visible"), Some("true"));
        assert_eq!(g.value("Mode"), Some("a"));
        assert_eq!(g.value(9), None);
    }

    #[test]
    fn bool_row_press_toggles_and_parks_changed() {
        let mut g = PropertyGrid::new().row(PropertyRow::bool("Visible", false));
        laid_out(&mut g, 200.0, 100.0);
        let r = g.child_bounds(0).unwrap();
        let pos = Vec2::new(r.min_x() + 150.0, r.min_y() + 13.0);
        assert_eq!(press(&mut g, pos), EventResponse::CaptureFocus);
        assert_eq!(g.take_changed(), Some((0, "true".to_string())));
        assert_eq!(g.value(0), Some("true"));
        assert!(g.row_at(0).unwrap().is_checked());
        // Second press toggles back.
        assert_eq!(press(&mut g, pos), EventResponse::CaptureFocus);
        assert_eq!(g.take_changed(), Some((0, "false".to_string())));
        assert_eq!(g.take_changed(), None);
    }

    #[test]
    fn choice_row_cycles_and_wraps() {
        let mut g = PropertyGrid::new().row(PropertyRow::choice("Size", ["S", "M", "L"]));
        laid_out(&mut g, 200.0, 100.0);
        let r = g.child_bounds(0).unwrap();
        let pos = Vec2::new(r.min_x() + 150.0, r.min_y() + 13.0);
        press(&mut g, pos);
        assert_eq!(g.take_changed(), Some((0, "M".to_string())));
        press(&mut g, pos);
        assert_eq!(g.take_changed(), Some((0, "L".to_string())));
        press(&mut g, pos); // wraps to the first option
        assert_eq!(g.take_changed(), Some((0, "S".to_string())));
        assert_eq!(g.row_at(0).unwrap().choice_index(), Some(0));
    }

    #[test]
    fn section_header_release_collapses() {
        let mut g = PropertyGrid::new()
            .section(
                "Transform",
                [
                    PropertyRow::bool("Visible", false),
                    PropertyRow::text("P", "1"),
                ],
            )
            .row(PropertyRow::text("Tail", "t"));
        laid_out(&mut g, 200.0, 200.0);
        // header + 2 section rows + 1 tail row + vbar.
        assert_eq!(g.child_count(), 5);
        let header = g.child_bounds(0).unwrap();
        let pos = Vec2::new(header.min_x() + 40.0, header.min_y() + 12.0);
        release(&mut g, pos);
        assert!(g.is_collapsed(0));
        // Collapsed rows drop out of the emitted child list entirely.
        assert_eq!(g.child_count(), 3);
        release(&mut g, pos);
        assert!(!g.is_collapsed(0));
        assert_eq!(g.child_count(), 5);
    }

    #[test]
    fn collapsed_rows_are_not_hit_tested() {
        let mut g = PropertyGrid::new()
            .section("S", [PropertyRow::bool("Visible", false)])
            .row(PropertyRow::text("Tail", "t"));
        laid_out(&mut g, 200.0, 200.0);
        // Collapse via the API (header hit-test covered elsewhere).
        g.set_collapsed(0, true);
        laid_out(&mut g, 200.0, 200.0);
        // Where the bool row used to sit is now the tail text row —
        // pressing there cannot reach the hidden editor.
        press(&mut g, Vec2::new(100.0, 36.0));
        assert_eq!(g.take_changed(), None);
    }

    #[test]
    fn disabled_grid_is_inert() {
        let mut g = PropertyGrid::new()
            .enabled(false)
            .row(PropertyRow::bool("B", false));
        laid_out(&mut g, 200.0, 100.0);
        assert_eq!(
            press(&mut g, Vec2::new(100.0, 13.0)),
            EventResponse::Ignored
        );
        assert_eq!(
            release(&mut g, Vec2::new(100.0, 13.0)),
            EventResponse::Ignored
        );
        let ev = WidgetEvent::Scroll {
            position: Vec2::new(10.0, 10.0),
            delta: Vec2::new(0.0, 30.0),
        };
        assert_eq!(event(&mut g, &ev), EventResponse::Ignored);
        assert_eq!(g.take_changed(), None);
    }

    #[test]
    fn wheel_scrolls_and_clamps() {
        let mut g = PropertyGrid::new();
        for i in 0..10 {
            g.add_row(PropertyRow::text(format!("p{i}"), "v"));
        }
        laid_out(&mut g, 200.0, 100.0);
        // 10 rows × 26 = 260 content; max offset 160.
        assert_eq!(g.max_scroll_offset(), 160.0);
        let ev = WidgetEvent::Scroll {
            position: Vec2::new(10.0, 10.0),
            delta: Vec2::new(0.0, 1000.0),
        };
        assert_eq!(event(&mut g, &ev), EventResponse::RequestRepaint);
        assert_eq!(g.scroll_offset(), 160.0);
        // At the bottom the delta is unconsumed → chains upward.
        assert_eq!(event(&mut g, &ev), EventResponse::Ignored);
    }

    #[test]
    fn set_value_by_name_and_index() {
        let mut g = PropertyGrid::new()
            .row(PropertyRow::text("Name", "a"))
            .row(PropertyRow::bool("On", false))
            .row(PropertyRow::choice("Size", ["S", "M"]));
        assert!(g.set_value("Name", "b"));
        assert_eq!(g.value("Name"), Some("b"));
        assert!(g.set_value(1, "true"));
        assert!(g.row_at(1).unwrap().is_checked());
        assert!(g.set_value("Size", "M"));
        assert_eq!(g.row_at(2).unwrap().choice_index(), Some(1));
        assert!(!g.set_value("missing", "x"));
    }

    #[test]
    fn a11y_grid_and_children() {
        let mut g = PropertyGrid::new()
            .label("Inspector")
            .section("S", [PropertyRow::bool("B", true)])
            .row(PropertyRow::text("T", "v"));
        laid_out(&mut g, 200.0, 200.0);

        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        g.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::List);
        assert_eq!(node.label(), Some("Inspector"));

        // Child 0: the section header — DisclosureTriangle, expanded.
        let header = Widget::child(&g, 0).unwrap();
        let mut hn = accesskit::Node::new(accesskit::Role::Unknown);
        header.accessibility(&mut hn);
        assert_eq!(hn.role(), accesskit::Role::DisclosureTriangle);
        assert_eq!(hn.is_expanded(), Some(true));
        assert_eq!(hn.label(), Some("S"));

        // Child 1: the bool row — ListItem carrying name + toggled.
        let row = Widget::child(&g, 1).unwrap();
        let mut rn = accesskit::Node::new(accesskit::Role::Unknown);
        row.accessibility(&mut rn);
        assert_eq!(rn.role(), accesskit::Role::ListItem);
        assert_eq!(rn.label(), Some("B"));
        assert_eq!(rn.toggled(), Some(Toggled::True));

        // Collapsing removes the row from the emitted tree.
        g.set_collapsed(0, true);
        laid_out(&mut g, 200.0, 200.0);
        // header + tail row + vbar.
        assert_eq!(g.child_count(), 3);
    }

    #[test]
    fn min_render_and_measure() {
        let g = PropertyGrid::new();
        assert_eq!(g.min_render().size, Vec2::new(160.0, 120.0));
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let mut g = PropertyGrid::new().section(
            "S",
            [PropertyRow::text("a", "1"), PropertyRow::text("b", "2")],
        );
        let size = g.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(400.0, 400.0),
            },
        );
        // header 24 + 2 rows × 26 = 76; width fills the constraint.
        assert_eq!(size, Vec2::new(200.0, 76.0));
    }
}
