//! `ActionSheet` widget: a bottom sheet presenting a short list of
//! actions — title, optional message, action rows (destructive rows
//! tinted), and a separated cancel row (iOS `UIActionSheet`, Ant
//! `ActionSheet`).
//!
//! Open with `OverlayAnchor::EdgeBottom` + `OverlayOptions::modal()
//! .light_dismiss()`; poll [`ActionSheet::take_result`] after dispatch.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::action_sheet::ActionSheet;
//!
//! let s = ActionSheet::new()
//!     .title("Delete photo?")
//!     .destructive("Delete")
//!     .action("Keep");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

/// Sheet face colour.
const SURFACE: [u8; 4] = [38, 41, 48, 255];
/// Row separator / border colour.
const EDGE: [u8; 4] = [110, 115, 125, 255];
/// Normal row ink.
const INK: [u8; 4] = [235, 236, 240, 255];
/// Title ink (muted).
const TITLE_INK: [u8; 4] = [150, 154, 163, 255];
/// Destructive row ink.
const DESTRUCTIVE: [u8; 4] = [235, 90, 80, 255];
/// Highlighted row tint.
const HIGHLIGHT: [u8; 4] = [255, 255, 255, 18];
/// Top corner radius, logical points.
const CORNER: f64 = 14.0;
/// Row height, logical points.
const ROW_PT: f32 = 44.0;
/// Title/message header height per line, logical points.
const HEADER_LINE_PT: f32 = 20.0;
/// Vertical padding in the header, logical points.
const HEADER_PAD_PT: f32 = 12.0;
/// Gap between the action group and the cancel row, logical points.
const CANCEL_GAP_PT: f32 = 8.0;
/// Row font size, logical points.
const ROW_FONT_PT: f32 = 15.0;
/// Maximum sheet width — centred when the entry is wider, logical pt.
const MAX_W_PT: f32 = 480.0;

/// What an [`ActionSheet`] resolved to.
///
/// # Examples
///
/// ```
/// use martensite::widgets::action_sheet::ActionSheetResult;
///
/// assert_ne!(ActionSheetResult::Cancel, ActionSheetResult::Dismissed);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionSheetResult {
    /// The action row at this index fired — index counts all
    /// `.action(...)` and `.destructive(...)` rows in insertion order.
    Action(usize),
    /// The cancel row fired, or Escape was pressed.
    Cancel,
    /// A press landed outside the card (scrim tap).
    Dismissed,
}

/// One tappable row in an [`ActionSheet`].
#[derive(Clone, Debug)]
struct ActionRow {
    /// Row label.
    label: String,
    /// Destructive rows render in red — used for delete/remove acts.
    destructive: bool,
}

/// A bottom action sheet — a vertical list of full-width action rows
/// with an optional header and a separated cancel row.
///
/// Unlike [`crate::widgets::BottomSheet`] (a general container with
/// detents), `ActionSheet` sizes itself to its rows and reports a
/// single [`ActionSheetResult`].
///
/// # Examples
///
/// ```
/// use martensite::widgets::action_sheet::ActionSheet;
///
/// let s = ActionSheet::new()
///     .title("Choose")
///     .action("One")
///     .action("Two")
///     .cancel("Never mind");
/// assert_eq!(s.row_count(), 2);
/// ```
pub struct ActionSheet {
    /// Optional header title (muted, smaller).
    title: Option<String>,
    /// Optional second header line.
    message: Option<String>,
    /// Action rows in insertion order.
    rows: Vec<ActionRow>,
    /// Cancel row label; `None` hides the row entirely.
    cancel_label: Option<String>,
    /// Row index under the pointer / keyboard highlight.
    highlight: Option<usize>,
    /// Pointer-held row index (press visual).
    pressed: Option<usize>,
    /// Pending result for `take_result`.
    result: Option<ActionSheetResult>,
    /// Per-row bounds from the last layout (widget space) — one entry
    /// per action row, then the cancel row last when present.
    row_rects: Vec<Rect>,
    /// The card rect within the entry.
    card_rect: Rect,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter. See [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl ActionSheet {
    /// Creates an empty action sheet with a "Cancel" row.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::action_sheet::ActionSheet;
    ///
    /// let s = ActionSheet::new();
    /// assert!(s.has_cancel());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            title: None,
            message: None,
            rows: Vec::new(),
            cancel_label: Some("Cancel".into()),
            highlight: None,
            pressed: None,
            result: None,
            row_rects: Vec::new(),
            card_rect: Rect::default(),
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the header title line.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::action_sheet::ActionSheet;
    ///
    /// let s = ActionSheet::new().title("Share to…");
    /// ```
    #[must_use]
    pub fn title(mut self, text: impl Into<String>) -> Self {
        self.title = Some(text.into());
        self
    }

    /// Sets the secondary header message line.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::action_sheet::ActionSheet;
    ///
    /// let s = ActionSheet::new().message("This can't be undone.");
    /// ```
    #[must_use]
    pub fn message(mut self, text: impl Into<String>) -> Self {
        self.message = Some(text.into());
        self
    }

    /// Adds a normal action row.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::action_sheet::ActionSheet;
    ///
    /// let s = ActionSheet::new().action("Save draft");
    /// assert_eq!(s.row_count(), 1);
    /// ```
    #[must_use]
    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.rows.push(ActionRow {
            label: label.into(),
            destructive: false,
        });
        self
    }

    /// Adds a destructive action row — rendered in the danger colour.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::action_sheet::ActionSheet;
    ///
    /// let s = ActionSheet::new().destructive("Delete");
    /// ```
    #[must_use]
    pub fn destructive(mut self, label: impl Into<String>) -> Self {
        self.rows.push(ActionRow {
            label: label.into(),
            destructive: true,
        });
        self
    }

    /// Sets the cancel row label; an empty string hides the row.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::action_sheet::ActionSheet;
    ///
    /// let s = ActionSheet::new().cancel("Dismiss");
    /// ```
    #[must_use]
    pub fn cancel(mut self, label: impl Into<String>) -> Self {
        let label = label.into();
        self.cancel_label = if label.is_empty() { None } else { Some(label) };
        self
    }

    /// Number of action rows (excluding cancel).
    #[inline]
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Whether the cancel row is shown.
    #[inline]
    #[must_use]
    pub fn has_cancel(&self) -> bool {
        self.cancel_label.is_some()
    }

    /// Drains the pending result — `Some` once per user resolution.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::action_sheet::ActionSheet;
    ///
    /// let mut s = ActionSheet::new();
    /// assert_eq!(s.take_result(), None);
    /// ```
    pub fn take_result(&mut self) -> Option<ActionSheetResult> {
        self.result.take()
    }

    /// Installs a shared shaped-text painter for the rows.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Content height in logical points — header + rows + gap +
    /// cancel row.
    fn content_height_pt(&self) -> f32 {
        let header_lines = usize::from(self.title.is_some()) + usize::from(self.message.is_some());
        let header = if header_lines > 0 {
            header_lines as f32 * HEADER_LINE_PT + HEADER_PAD_PT * 2.0
        } else {
            0.0
        };
        let rows = self.rows.len() as f32 * ROW_PT;
        let cancel = if self.cancel_label.is_some() {
            CANCEL_GAP_PT + ROW_PT
        } else {
            0.0
        };
        header + rows + cancel
    }

    /// Index of the row containing `position` — `rows.len()` means
    /// the cancel row, `None` means header/gap/outside.
    fn row_at(&self, position: Vec2) -> Option<usize> {
        self.row_rects.iter().position(|r| r.contains(position))
    }

    /// Resolves the row at `index` — cancel rows resolve to
    /// `ActionSheetResult::Cancel`.
    fn activate(&mut self, index: usize) {
        if index < self.rows.len() {
            self.result = Some(ActionSheetResult::Action(index));
        } else {
            self.result = Some(ActionSheetResult::Cancel);
        }
    }
}

impl Default for ActionSheet {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ActionSheet {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(0.0),
            cx.pt(self.content_height_pt())
                .min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        self.row_rects.clear();
        let w = cx.pt(MAX_W_PT).min(bounds.size.x);
        let h = cx.pt(self.content_height_pt()).min(bounds.size.y);
        let x = bounds.origin.x + (bounds.size.x - w) / 2.0;
        self.card_rect = Rect::new(x, bounds.max_y() - h, w, h);

        let mut y = self.card_rect.origin.y;
        let header_lines = usize::from(self.title.is_some()) + usize::from(self.message.is_some());
        if header_lines > 0 {
            y += cx.pt(header_lines as f32 * HEADER_LINE_PT + HEADER_PAD_PT * 2.0);
        }
        let row_h = cx.pt(ROW_PT);
        for _ in &self.rows {
            self.row_rects.push(Rect::new(x, y, w, row_h));
            y += row_h;
        }
        if self.cancel_label.is_some() {
            y += cx.pt(CANCEL_GAP_PT);
            self.row_rects.push(Rect::new(x, y, w, row_h));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Dialog);
        if let Some(title) = &self.title {
            node.set_label(title.as_str());
        } else {
            node.set_label("Action sheet");
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if self.card_rect.contains(*position) {
                    self.pressed = self.row_at(*position);
                    self.highlight = self.pressed;
                    EventResponse::CapturePointer
                } else {
                    self.result = Some(ActionSheetResult::Dismissed);
                    EventResponse::Handled
                }
            }
            WidgetEvent::PointerMoved { position } if self.pressed.is_some() => {
                let row = self.row_at(*position);
                if row != self.highlight {
                    self.highlight = row;
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Handled
                }
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } if self.pressed.is_some() => {
                let pressed = self.pressed.take().unwrap_or(0);
                self.highlight = None;
                if self.row_at(*position) == Some(pressed) {
                    self.activate(pressed);
                }
                EventResponse::ReleasePointer
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "Escape" => {
                    self.result = Some(ActionSheetResult::Cancel);
                    EventResponse::Handled
                }
                "ArrowDown" | "ArrowUp" => {
                    let total = self.row_rects.len();
                    if total == 0 {
                        return EventResponse::Ignored;
                    }
                    let cur = self.highlight.unwrap_or(match key.as_str() {
                        "ArrowDown" => usize::MAX,
                        _ => total,
                    });
                    self.highlight = Some(match key.as_str() {
                        "ArrowDown" => cur.wrapping_add(1) % total,
                        _ => cur.wrapping_sub(1) % total,
                    });
                    EventResponse::RequestRepaint
                }
                "Enter" | "Space" | " " => {
                    if let Some(row) = self.highlight {
                        self.activate(row);
                    }
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let card = self.card_rect;
        if card.size.x <= 0.0 || card.size.y <= 0.0 {
            return;
        }
        // Rounded-top card path — same shape as BottomSheet.
        let rect = kurbo::Rect::new(
            f64::from(card.min_x()),
            f64::from(card.min_y()),
            f64::from(card.max_x()),
            f64::from(card.max_y()),
        );
        let r = cx.ptf(CORNER).min(f64::from(card.size.y));
        let mut path = kurbo::BezPath::new();
        path.move_to((rect.x0, rect.y1));
        path.line_to((rect.x0, rect.y0 + r));
        path.quad_to((rect.x0, rect.y0), (rect.x0 + r, rect.y0));
        path.line_to((rect.x1 - r, rect.y0));
        path.quad_to((rect.x1, rect.y0), (rect.x1, rect.y0 + r));
        path.line_to((rect.x1, rect.y1));
        path.close_path();
        cx.list
            .push_path(path.clone(), cx.color(TokenKey::SurfaceColor, SURFACE));
        cx.list.push_stroke_path(
            path.clone(),
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, EDGE),
        );
        cx.list.push_clip_path(path);

        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let font = cx.pt(ROW_FONT_PT);
        let center = |list: &mut martensite_core::PaintList,
                      text: &str,
                      row: Rect,
                      ink: [u8; 4],
                      size: f32| {
            let w = painter
                .and_then(|p| p.measure_text(text, size))
                .unwrap_or(size * text.chars().count() as f32 * 0.5);
            let tx = row.origin.x + (row.size.x - w.min(row.size.x)) / 2.0;
            let ty = row.origin.y + (row.size.y - size) / 2.0;
            crate::text_paint::paint_label_clipped(
                painter,
                list,
                kurbo::Rect::new(
                    f64::from(row.min_x()),
                    f64::from(row.min_y()),
                    f64::from(row.max_x()),
                    f64::from(row.max_y()),
                ),
                kurbo::Point::new(f64::from(tx), f64::from(ty)),
                text,
                size,
                ink,
            );
        };

        // Header lines (muted, centred).
        let header_lines = usize::from(self.title.is_some()) + usize::from(self.message.is_some());
        let mut y = card.origin.y;
        if header_lines > 0 {
            y += cx.pt(HEADER_PAD_PT);
            let line_h = cx.pt(HEADER_LINE_PT);
            if let Some(title) = &self.title {
                center(
                    cx.list,
                    title,
                    Rect::new(card.origin.x, y, card.size.x, line_h),
                    cx.color(TokenKey::TextMutedColor, TITLE_INK),
                    cx.pt(13.0),
                );
                y += line_h;
            }
            if let Some(message) = &self.message {
                center(
                    cx.list,
                    message,
                    Rect::new(card.origin.x, y, card.size.x, line_h),
                    cx.color(TokenKey::TextMutedColor, TITLE_INK),
                    cx.pt(13.0),
                );
            }
        }

        // Action rows with separators and highlight.
        for (i, row) in self.row_rects.iter().enumerate() {
            if self.highlight == Some(i) || self.pressed == Some(i) {
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(row.min_x()),
                        f64::from(row.min_y()),
                        f64::from(row.max_x()),
                        f64::from(row.max_y()),
                    ),
                    HIGHLIGHT,
                );
            }
            let is_cancel = i >= self.rows.len();
            let ink = if is_cancel {
                cx.color(TokenKey::TextColor, INK)
            } else if self.rows[i].destructive {
                DESTRUCTIVE
            } else {
                cx.color(TokenKey::TextColor, INK)
            };
            let label = if is_cancel {
                self.cancel_label.as_deref().unwrap_or("Cancel")
            } else {
                self.rows[i].label.as_str()
            };
            center(cx.list, label, *row, ink, font);
            // Separator under each action row (not under cancel).
            if !is_cancel {
                let sy = row.max_y();
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(row.origin.x),
                        f64::from(sy),
                        f64::from(row.origin.x + row.size.x),
                        f64::from(sy + 1.0),
                    ),
                    cx.color(TokenKey::BorderColor, EDGE),
                );
            }
        }
        cx.list.pop_clip();
    }
}

impl std::fmt::Debug for ActionSheet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionSheet")
            .field("rows", &self.rows.len())
            .field("cancel", &self.cancel_label)
            .field("highlight", &self.highlight)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn make_cx(hot: &mut HotNode) -> LayoutContext<'_> {
        LayoutContext { hot, scale: 1.0 }
    }

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 480.0, 600.0),
            scale: 1.0,
        }
    }

    fn laid_out() -> ActionSheet {
        let mut s = ActionSheet::new()
            .title("Choose")
            .action("One")
            .destructive("Delete");
        let mut hot = HotNode::default();
        s.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 480.0, 600.0));
        s
    }

    #[test]
    fn builder_and_rows() {
        let s = ActionSheet::new()
            .title("T")
            .action("A")
            .destructive("D")
            .cancel("C");
        assert_eq!(s.row_count(), 2);
        assert!(s.has_cancel());
    }

    #[test]
    fn cancel_empty_hides_row() {
        let s = ActionSheet::new().cancel("");
        assert!(!s.has_cancel());
    }

    #[test]
    fn rows_get_laid_out() {
        let s = laid_out();
        // Two action rows + one cancel row = 3 rects.
        assert_eq!(s.row_rects.len(), 3);
        // Rows stack top-to-bottom inside the card.
        assert!(s.row_rects[0].origin.y < s.row_rects[1].origin.y);
        assert!(s.row_rects[1].origin.y < s.row_rects[2].origin.y);
    }

    #[test]
    fn click_action_resolves() {
        let mut s = laid_out();
        let row0 = s.row_rects[0];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(row0.origin.x + 10.0, row0.origin.y + 10.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(s.event(&mut ev(&press)), EventResponse::CapturePointer);
        let rel = WidgetEvent::PointerReleased {
            position: Vec2::new(row0.origin.x + 10.0, row0.origin.y + 10.0),
            button: PointerButton::Primary,
        };
        s.event(&mut ev(&rel));
        assert_eq!(s.take_result(), Some(ActionSheetResult::Action(0)));
        assert_eq!(s.take_result(), None);
    }

    #[test]
    fn click_cancel_resolves_cancel() {
        let mut s = laid_out();
        let cancel = *s.row_rects.last().unwrap();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(cancel.origin.x + 10.0, cancel.origin.y + 10.0),
            button: PointerButton::Primary,
            count: 1,
        };
        s.event(&mut ev(&press));
        let rel = WidgetEvent::PointerReleased {
            position: Vec2::new(cancel.origin.x + 10.0, cancel.origin.y + 10.0),
            button: PointerButton::Primary,
        };
        s.event(&mut ev(&rel));
        assert_eq!(s.take_result(), Some(ActionSheetResult::Cancel));
    }

    #[test]
    fn press_outside_dismisses() {
        let mut s = laid_out();
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(10.0, 10.0), // well above the card
            button: PointerButton::Primary,
            count: 1,
        };
        s.event(&mut ev(&press));
        assert_eq!(s.take_result(), Some(ActionSheetResult::Dismissed));
    }

    #[test]
    fn escape_resolves_cancel() {
        let mut s = laid_out();
        let key = WidgetEvent::KeyPressed {
            key: "Escape".into(),
            repeat: false,
        };
        s.event(&mut ev(&key));
        assert_eq!(s.take_result(), Some(ActionSheetResult::Cancel));
    }

    #[test]
    fn arrows_and_enter_activate() {
        let mut s = laid_out();
        let down = WidgetEvent::KeyPressed {
            key: "ArrowDown".into(),
            repeat: false,
        };
        s.event(&mut ev(&down));
        s.event(&mut ev(&down)); // highlight row 1
        let enter = WidgetEvent::KeyPressed {
            key: "Enter".into(),
            repeat: false,
        };
        s.event(&mut ev(&enter));
        assert_eq!(s.take_result(), Some(ActionSheetResult::Action(1)));
    }

    #[test]
    fn drag_off_row_cancels_activation() {
        let mut s = laid_out();
        let row0 = s.row_rects[0];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(row0.origin.x + 10.0, row0.origin.y + 10.0),
            button: PointerButton::Primary,
            count: 1,
        };
        s.event(&mut ev(&press));
        let rel = WidgetEvent::PointerReleased {
            position: Vec2::new(row0.origin.x + 10.0, row0.origin.y - 60.0),
            button: PointerButton::Primary,
        };
        s.event(&mut ev(&rel));
        assert_eq!(s.take_result(), None);
    }
}
