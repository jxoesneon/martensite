//! `KeyboardShortcuts` — grouped shortcut reference (GTK
//! `ShortcutsWindow`/`ShortcutsGroup`/`ShortcutsShortcut`, the
//! "Keyboard shortcuts" help panel).
//!
//! Renders [`ShortcutGroup`]s — a title plus `label : keys` rows —
//! flowed across `columns` columns. Display-only: mount it inside a
//! [`Dialog`](crate::widgets::dialog) or overlay for the full
//! shortcuts-window chrome.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::keyboard_shortcuts::{
//!     KeyboardShortcuts, ShortcutGroup, ShortcutRow,
//! };
//!
//! let shortcuts = KeyboardShortcuts::new(vec![
//!     ShortcutGroup::new("General")
//!         .row("Save", "⌘S")
//!         .row("Quit", "⌘Q"),
//!     ShortcutGroup::new("Editing").row("Undo", "⌘Z"),
//! ]);
//! assert_eq!(shortcuts.groups.len(), 2);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const TITLE_FONT_PT: f32 = 15.0;
const ROW_FONT_PT: f32 = 13.0;
const TITLE_H_PT: f32 = 26.0;
const ROW_H_PT: f32 = 22.0;
const PAD_PT: f32 = 12.0;
const COL_GAP_PT: f32 = 28.0;
const KEYS_GAP_PT: f32 = 16.0;
const INK: [u8; 4] = [30, 30, 34, 255];
const MUTED: [u8; 4] = [110, 110, 118, 255];

/// One shortcut row: a human label and its key chord.
///
/// ```
/// use martensite::widgets::keyboard_shortcuts::ShortcutRow;
///
/// let row = ShortcutRow::new("Save", "⌘S");
/// assert_eq!(row.label, "Save");
/// ```
pub struct ShortcutRow {
    /// What the shortcut does.
    pub label: String,
    /// The chord text (`"⌘S"`, `"Ctrl+Shift+P"`, …) — kept as text so
    /// apps render platform-native glyphs themselves.
    pub keys: String,
}

impl ShortcutRow {
    /// Creates a row.
    ///
    /// ```
    /// use martensite::widgets::keyboard_shortcuts::ShortcutRow;
    ///
    /// let r = ShortcutRow::new("Copy", "⌘C");
    /// assert_eq!(r.keys, "⌘C");
    /// ```
    pub fn new(label: impl Into<String>, keys: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            keys: keys.into(),
        }
    }
}

/// A titled group of [`ShortcutRow`]s (GTK `ShortcutsGroup`).
///
/// ```
/// use martensite::widgets::keyboard_shortcuts::ShortcutGroup;
///
/// let g = ShortcutGroup::new("File").row("New", "⌘N").row("Open", "⌘O");
/// assert_eq!(g.rows.len(), 2);
/// ```
pub struct ShortcutGroup {
    /// Group heading.
    pub title: String,
    /// Rows, top to bottom.
    pub rows: Vec<ShortcutRow>,
}

impl ShortcutGroup {
    /// Creates a group.
    ///
    /// ```
    /// use martensite::widgets::keyboard_shortcuts::ShortcutGroup;
    ///
    /// let g = ShortcutGroup::new("General");
    /// assert!(g.rows.is_empty());
    /// ```
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            rows: Vec::new(),
        }
    }

    /// Appends a row.
    ///
    /// ```
    /// use martensite::widgets::keyboard_shortcuts::ShortcutGroup;
    ///
    /// let g = ShortcutGroup::new("Edit").row("Undo", "⌘Z");
    /// assert_eq!(g.rows[0].label, "Undo");
    /// ```
    pub fn row(mut self, label: impl Into<String>, keys: impl Into<String>) -> Self {
        self.rows.push(ShortcutRow::new(label, keys));
        self
    }
}

/// A column of one group's rows, computed at layout for painting and
/// hit-free rendering.
struct GroupCell {
    group: usize,
    rect: Rect,
}

/// Grouped shortcut reference — see the module docs.
///
/// ```
/// use martensite::widgets::keyboard_shortcuts::{
///     KeyboardShortcuts, ShortcutGroup,
/// };
///
/// let s = KeyboardShortcuts::new(vec![ShortcutGroup::new("A")]);
/// assert_eq!(s.columns, 2);
/// ```
pub struct KeyboardShortcuts {
    /// Groups in display order.
    pub groups: Vec<ShortcutGroup>,
    /// Column count groups flow across (GTK sections use two).
    pub columns: usize,
    /// When `false` the panel is inert.
    pub enabled: bool,
    cells: Vec<GroupCell>,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
    scale: f32,
}

impl KeyboardShortcuts {
    /// Creates a two-column shortcuts panel.
    ///
    /// ```
    /// use martensite::widgets::keyboard_shortcuts::{
    ///     KeyboardShortcuts, ShortcutGroup,
    /// };
    ///
    /// let s = KeyboardShortcuts::new(vec![ShortcutGroup::new("General")]);
    /// assert_eq!(s.groups.len(), 1);
    /// ```
    pub fn new(groups: Vec<ShortcutGroup>) -> Self {
        Self {
            groups,
            columns: 2,
            enabled: true,
            cells: Vec::new(),
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            scale: 1.0,
        }
    }

    /// Sets the column count.
    ///
    /// ```
    /// use martensite::widgets::keyboard_shortcuts::{
    ///     KeyboardShortcuts, ShortcutGroup,
    /// };
    ///
    /// let s = KeyboardShortcuts::new(vec![ShortcutGroup::new("G")]).columns(3);
    /// assert_eq!(s.columns, 3);
    /// ```
    pub fn columns(mut self, n: usize) -> Self {
        self.columns = n.max(1);
        self
    }

    /// Enables or disables the panel.
    ///
    /// ```
    /// use martensite::widgets::keyboard_shortcuts::{
    ///     KeyboardShortcuts, ShortcutGroup,
    /// };
    ///
    /// let s = KeyboardShortcuts::new(vec![ShortcutGroup::new("G")]).enabled(false);
    /// assert!(!s.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Installs a shared shaped-text painter.
    ///
    /// ```
    /// use martensite::widgets::keyboard_shortcuts::{
    ///     KeyboardShortcuts, ShortcutGroup,
    /// };
    ///
    /// let s = KeyboardShortcuts::new(vec![ShortcutGroup::new("G")]);
    /// let _ = s.columns;
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Logical height of one group (pt, unscaled).
    fn group_pt(&self, g: &ShortcutGroup) -> f32 {
        TITLE_H_PT + g.rows.len() as f32 * ROW_H_PT + PAD_PT
    }

    /// Height of the tallest column after the round-robin flow (pt).
    fn tallest_column_pt(&self) -> f32 {
        let cols = self.columns.max(1);
        let mut heights = vec![0.0f32; cols];
        // Round-robin keeps declaration order left-to-right — the GTK
        // section convention.
        for (i, g) in self.groups.iter().enumerate() {
            heights[i % cols] += self.group_pt(g);
        }
        heights.into_iter().fold(0.0, f32::max)
    }
}

impl Widget for KeyboardShortcuts {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = (self.tallest_column_pt() + PAD_PT * 2.0).max(1.0);
        Vec2::new(
            cx.pt(200.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(h).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.cells.clear();
        let cols = self.columns.max(1);
        let pad = cx.pt(PAD_PT);
        let gap = cx.pt(COL_GAP_PT);
        let col_w = ((bounds.size.x - pad * 2.0 - gap * (cols - 1) as f32) / cols as f32).max(0.0);
        let mut tops = vec![bounds.origin.y + pad; cols];
        for (i, g) in self.groups.iter().enumerate() {
            let col = i % cols;
            let h = cx.pt(self.group_pt(g));
            let rect = Rect::new(
                bounds.origin.x + pad + (col_w + gap) * col as f32,
                tops[col],
                col_w,
                h,
            );
            tops[col] += h;
            self.cells.push(GroupCell { group: i, rect });
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label("Keyboard shortcuts");
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, _cx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        if !self.enabled {
            return;
        }
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let title_font = cx.pt(TITLE_FONT_PT);
        let row_font = cx.pt(ROW_FONT_PT);
        let title_h = cx.pt(TITLE_H_PT);
        let row_h = cx.pt(ROW_H_PT);
        let keys_gap = cx.pt(KEYS_GAP_PT);
        for cell in &self.cells {
            let g = &self.groups[cell.group];
            let r = cell.rect;
            let kr = |y0: f32, y1: f32| {
                kurbo::Rect::new(
                    f64::from(r.min_x()),
                    f64::from(y0),
                    f64::from(r.max_x()),
                    f64::from(y1),
                )
            };
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kr(r.min_y(), r.min_y() + title_h),
                kurbo::Point::new(
                    f64::from(r.min_x()),
                    f64::from(r.min_y() + (title_h - title_font) / 2.0),
                ),
                &g.title,
                title_font,
                cx.color(TokenKey::TextColor, INK),
            );
            for (i, row) in g.rows.iter().enumerate() {
                let y = r.min_y() + title_h + i as f32 * row_h;
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    kr(y, y + row_h),
                    kurbo::Point::new(
                        f64::from(r.min_x()),
                        f64::from(y + (row_h - row_font) / 2.0),
                    ),
                    &row.label,
                    row_font,
                    cx.color(TokenKey::TextColor, INK),
                );
                // Keys right-aligned, muted — the GTK row convention.
                let kw = painter
                    .and_then(|p| p.measure_text(&row.keys, row_font))
                    .unwrap_or(0.0);
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    kr(y, y + row_h),
                    kurbo::Point::new(
                        f64::from((r.max_x() - kw).max(r.min_x() + keys_gap)),
                        f64::from(y + (row_h - row_font) / 2.0),
                    ),
                    &row.keys,
                    row_font,
                    cx.color(TokenKey::TextMutedColor, MUTED),
                );
            }
        }
    }
}

impl std::fmt::Debug for KeyboardShortcuts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyboardShortcuts")
            .field("groups", &self.groups.len())
            .field("columns", &self.columns)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, WidgetEvent};

    fn laid_out(s: &mut KeyboardShortcuts, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        s.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        s.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn sample() -> KeyboardShortcuts {
        KeyboardShortcuts::new(vec![
            ShortcutGroup::new("General")
                .row("Save", "⌘S")
                .row("Quit", "⌘Q"),
            ShortcutGroup::new("Editing").row("Undo", "⌘Z"),
            ShortcutGroup::new("View").row("Zoom in", "⌘+"),
        ])
    }

    #[test]
    fn groups_flow_round_robin_into_columns() {
        let mut s = sample();
        laid_out(&mut s, 600.0, 400.0);
        assert_eq!(s.cells.len(), 3);
        // Groups 0 and 2 share column 0; group 1 is column 1.
        assert_eq!(s.cells[0].rect.min_x(), s.cells[2].rect.min_x());
        assert!(s.cells[1].rect.min_x() > s.cells[0].rect.min_x());
        // Group 2 stacks below group 0.
        assert!(s.cells[2].rect.min_y() >= s.cells[0].rect.max_y());
    }

    #[test]
    fn measure_covers_tallest_column() {
        let mut s = sample();
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let size = s.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(600.0, 600.0),
            },
        );
        // Column 0: (26+2*22+12) + (26+1*22+12) = 82 + 60 = 142 pt + pad.
        assert!(size.y >= 140.0);
    }

    #[test]
    fn single_column_stacks_all() {
        let mut s = sample().columns(1);
        laid_out(&mut s, 300.0, 600.0);
        assert!(s
            .cells
            .iter()
            .all(|c| c.rect.min_x() == s.cells[0].rect.min_x()));
        for i in 1..s.cells.len() {
            assert!(s.cells[i].rect.min_y() >= s.cells[i - 1].rect.max_y());
        }
    }

    #[test]
    fn display_only_ignores_input() {
        let mut s = sample();
        laid_out(&mut s, 600.0, 400.0);
        let mut cx = EventContext {
            event: &WidgetEvent::KeyPressed {
                key: "Enter".into(),
                repeat: false,
            },
            bounds: Rect::new(0.0, 0.0, 600.0, 400.0),
            scale: 1.0,
        };
        assert_eq!(s.event(&mut cx), EventResponse::Ignored);
    }

    #[test]
    fn empty_groups_measure_small() {
        let mut s = KeyboardShortcuts::new(vec![]);
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let size = s.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(600.0, 600.0),
            },
        );
        assert!(size.y <= 30.0);
    }
}
