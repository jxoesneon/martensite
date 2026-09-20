//! `MergeView` — a three-pane merge display (Meld / GitLens
//! conflict-resolver idiom): aligned `Ours | Result | Theirs`
//! rows, conflict rows tinted, and per-row `‹`/`›` accept buttons
//! that write the chosen side into the result column and park the
//! decision in [`MergeView::take_choice`].
//!
//! [`MergeRow::aligned`] builds an unchanged row;
//! [`MergeRow::conflict`] marks a disputed row awaiting a choice.
//! `Up`/`Down` focus rows and `Left`/`Right` accept a side.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::merge_view::{MergeRow, MergeView};
//!
//! let m = MergeView::new()
//!     .row(MergeRow::aligned("same", "same", "same"))
//!     .row(MergeRow::conflict("ours", "", "theirs"));
//! assert_eq!(m.conflict_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const ROW_PT: f32 = 20.0;
const HEADER_PT: f32 = 20.0;
const FONT_PT: f32 = 12.0;
const BTN_PT: f32 = 18.0;
const PAD_PT: f32 = 8.0;

const TINT: [u8; 4] = [140, 90, 60, 60];
const RESOLVED: [u8; 4] = [60, 140, 90, 50];
const FOCUS: [u8; 4] = [110, 140, 230, 80];
const LINE: [u8; 4] = [80, 84, 96, 255];
const TEXT: [u8; 4] = [235, 237, 240, 255];
const HEAD: [u8; 4] = [170, 175, 190, 255];

/// Which side a conflict resolved toward.
///
/// ```
/// use martensite::widgets::merge_view::MergeSide;
///
/// assert_eq!(MergeSide::Ours, MergeSide::Ours);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeSide {
    /// The left ("ours") column won.
    Ours,
    /// The right ("theirs") column won.
    Theirs,
}

/// One aligned row across the three columns.
///
/// ```
/// use martensite::widgets::merge_view::MergeRow;
///
/// let r = MergeRow::conflict("a", "", "b");
/// assert!(r.is_conflict());
/// ```
#[derive(Clone, Debug)]
pub struct MergeRow {
    /// "Ours" cell text (empty = blank).
    pub ours: String,
    /// Result cell text.
    pub result: String,
    /// "Theirs" cell text.
    pub theirs: String,
    conflict: bool,
    resolution: Option<MergeSide>,
}

impl MergeRow {
    /// An unchanged row shown identically in all columns.
    ///
    /// ```
    /// use martensite::widgets::merge_view::MergeRow;
    ///
    /// assert!(!MergeRow::aligned("x", "x", "x").is_conflict());
    /// ```
    pub fn aligned(
        ours: impl Into<String>,
        result: impl Into<String>,
        theirs: impl Into<String>,
    ) -> Self {
        Self {
            ours: ours.into(),
            result: result.into(),
            theirs: theirs.into(),
            conflict: false,
            resolution: None,
        }
    }

    /// A disputed row — `result` is pending until a side is chosen.
    ///
    /// ```
    /// use martensite::widgets::merge_view::MergeRow;
    ///
    /// assert!(MergeRow::conflict("l", "?", "r").is_conflict());
    /// ```
    pub fn conflict(
        ours: impl Into<String>,
        result: impl Into<String>,
        theirs: impl Into<String>,
    ) -> Self {
        Self {
            ours: ours.into(),
            result: result.into(),
            theirs: theirs.into(),
            conflict: true,
            resolution: None,
        }
    }

    /// Whether this row awaits a merge decision.
    ///
    /// ```
    /// use martensite::widgets::merge_view::MergeRow;
    ///
    /// assert!(MergeRow::conflict("l", "", "r").is_conflict());
    /// assert!(!MergeRow::aligned("a", "a", "a").is_conflict());
    /// ```
    pub fn is_conflict(&self) -> bool {
        self.conflict && self.resolution.is_none()
    }

    /// The chosen side, if resolved.
    ///
    /// ```
    /// use martensite::widgets::merge_view::MergeRow;
    ///
    /// assert_eq!(MergeRow::aligned("a", "a", "a").resolution(), None);
    /// ```
    pub fn resolution(&self) -> Option<MergeSide> {
        self.resolution
    }
}

/// The merge view — see the module docs.
///
/// ```
/// use martensite::widgets::merge_view::MergeView;
///
/// assert_eq!(MergeView::new().row_count(), 0);
/// ```
pub struct MergeView {
    /// Accessibility label.
    pub label: String,
    /// Column headers.
    pub headers: (String, String, String),
    rows: Vec<MergeRow>,
    focused: Option<usize>,
    choice: Option<(usize, MergeSide)>,
    row_rects: Vec<Rect>,
    col_x: [f32; 4],
    accept_rects: Vec<(Rect, Rect)>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for MergeView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MergeView")
            .field("rows", &self.rows.len())
            .finish()
    }
}

impl Default for MergeView {
    fn default() -> Self {
        Self::new()
    }
}

impl MergeView {
    /// Empty view with default `Ours / Result / Theirs` headers.
    ///
    /// ```
    /// use martensite::widgets::merge_view::MergeView;
    ///
    /// assert_eq!(MergeView::new().row_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Merge".to_string(),
            headers: (
                "Ours".to_string(),
                "Result".to_string(),
                "Theirs".to_string(),
            ),
            rows: Vec::new(),
            focused: None,
            choice: None,
            row_rects: Vec::new(),
            col_x: [0.0; 4],
            accept_rects: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a row.
    ///
    /// ```
    /// use martensite::widgets::merge_view::{MergeRow, MergeView};
    ///
    /// assert_eq!(MergeView::new().row(MergeRow::aligned("a", "a", "a")).row_count(), 1);
    /// ```
    pub fn row(mut self, row: MergeRow) -> Self {
        self.rows.push(row);
        self
    }

    /// Column headers.
    ///
    /// ```
    /// use martensite::widgets::merge_view::MergeView;
    ///
    /// assert_eq!(MergeView::new().headers(("L", "M", "R")).headers.1, "M");
    /// ```
    pub fn headers(
        mut self,
        headers: (impl Into<String>, impl Into<String>, impl Into<String>),
    ) -> Self {
        self.headers = (headers.0.into(), headers.1.into(), headers.2.into());
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::merge_view::MergeView;
    ///
    /// assert_eq!(MergeView::new().label("Merge").label, "Merge");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::merge_view::MergeView;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _m = MergeView::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Row count.
    ///
    /// ```
    /// use martensite::widgets::merge_view::MergeView;
    ///
    /// assert_eq!(MergeView::new().row_count(), 0);
    /// ```
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Unresolved conflict count.
    ///
    /// ```
    /// use martensite::widgets::merge_view::{MergeRow, MergeView};
    ///
    /// let m = MergeView::new().row(MergeRow::conflict("a", "", "b"));
    /// assert_eq!(m.conflict_count(), 1);
    /// ```
    pub fn conflict_count(&self) -> usize {
        self.rows.iter().filter(|r| r.is_conflict()).count()
    }

    /// Focused row index.
    ///
    /// ```
    /// use martensite::widgets::merge_view::MergeView;
    ///
    /// assert_eq!(MergeView::new().focused(), None);
    /// ```
    pub fn focused(&self) -> Option<usize> {
        self.focused
    }

    /// Result text of a row (post-resolution where chosen).
    ///
    /// ```
    /// use martensite::widgets::merge_view::{MergeRow, MergeView};
    ///
    /// let m = MergeView::new().row(MergeRow::aligned("a", "mid", "b"));
    /// assert_eq!(m.result(0), Some("mid"));
    /// ```
    pub fn result(&self, row: usize) -> Option<&str> {
        self.rows.get(row).map(|r| r.result.as_str())
    }

    /// Accepts a side for a conflict row host-side.
    ///
    /// ```
    /// use martensite::widgets::merge_view::{MergeRow, MergeSide, MergeView};
    ///
    /// let mut m = MergeView::new().row(MergeRow::conflict("ours", "", "theirs"));
    /// m.accept(0, MergeSide::Ours);
    /// assert_eq!(m.result(0), Some("ours"));
    /// assert_eq!(m.conflict_count(), 0);
    /// ```
    pub fn accept(&mut self, row: usize, side: MergeSide) {
        if let Some(r) = self.rows.get_mut(row) {
            if r.is_conflict() {
                r.resolution = Some(side);
                r.result = match side {
                    MergeSide::Ours => r.ours.clone(),
                    MergeSide::Theirs => r.theirs.clone(),
                };
                self.choice = Some((row, side));
            }
        }
    }

    /// Drains the last `(row, side)` decision.
    ///
    /// ```
    /// use martensite::widgets::merge_view::MergeView;
    ///
    /// let mut m = MergeView::new();
    /// assert_eq!(m.take_choice(), None);
    /// ```
    pub fn take_choice(&mut self) -> Option<(usize, MergeSide)> {
        self.choice.take()
    }
}

impl Widget for MergeView {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let h = (HEADER_PT + self.rows.len().max(1) as f32 * ROW_PT + PAD_PT) * s;
        Vec2::new(
            (480.0 * s).min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(300.0, 120.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        // Buttons column width reserved on the left edge.
        let btn_w = BTN_PT * 2.0 * s + PAD_PT * s;
        let col_w = (bounds.width() - btn_w) / 3.0;
        self.col_x = [
            bounds.min_x() + btn_w,
            bounds.min_x() + btn_w + col_w,
            bounds.min_x() + btn_w + col_w * 2.0,
            bounds.max_x(),
        ];
        self.row_rects.clear();
        self.accept_rects.clear();
        let mut y = bounds.min_y() + HEADER_PT * s;
        for _ in &self.rows {
            self.row_rects
                .push(Rect::new(bounds.min_x(), y, bounds.width(), ROW_PT * s));
            let b = BTN_PT * s;
            self.accept_rects.push((
                Rect::new(bounds.min_x() + 2.0 * s, y + (ROW_PT * s - b) / 2.0, b, b),
                Rect::new(
                    bounds.min_x() + 2.0 * s + b + 2.0 * s,
                    y + (ROW_PT * s - b) / 2.0,
                    b,
                    b,
                ),
            ));
            y += ROW_PT * s;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Table);
        node.set_label(self.label.clone());
        node.set_value(format!("{} conflicts", self.conflict_count()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowDown" | "ArrowUp" => {
                    if self.rows.is_empty() {
                        return EventResponse::Ignored;
                    }
                    let cur = self.focused.unwrap_or(usize::MAX);
                    self.focused = Some(if key.as_str() == "ArrowDown" {
                        if cur == usize::MAX {
                            0
                        } else {
                            (cur + 1).min(self.rows.len() - 1)
                        }
                    } else if cur == usize::MAX {
                        0
                    } else {
                        cur.saturating_sub(1)
                    });
                    EventResponse::RequestRepaint
                }
                "ArrowLeft" | "ArrowRight" => {
                    if let Some(i) = self.focused {
                        let side = if key.as_str() == "ArrowLeft" {
                            MergeSide::Ours
                        } else {
                            MergeSide::Theirs
                        };
                        if self.rows[i].is_conflict() {
                            self.accept(i, side);
                            return EventResponse::RequestRepaint;
                        }
                    }
                    EventResponse::Ignored
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                for (i, (l, r)) in self.accept_rects.iter().enumerate() {
                    if self.rows[i].is_conflict() {
                        if l.contains(*position) {
                            self.accept(i, MergeSide::Ours);
                            return EventResponse::RequestRepaint;
                        }
                        if r.contains(*position) {
                            self.accept(i, MergeSide::Theirs);
                            return EventResponse::RequestRepaint;
                        }
                    }
                }
                if let Some(i) = self.row_rects.iter().position(|r| r.contains(*position)) {
                    self.focused = Some(i);
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
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        // Column headers.
        for (ci, h) in [&self.headers.0, &self.headers.1, &self.headers.2]
            .iter()
            .enumerate()
        {
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(self.col_x[ci] + PAD_PT * s),
                    f64::from(self.bounds.min_y() + HEADER_PT * 0.7 * s),
                ),
                h,
                FONT_PT * s,
                cx.color(TokenKey::TextMutedColor, HEAD),
            );
        }
        // Column separators.
        for x in [self.col_x[0], self.col_x[1], self.col_x[2]] {
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(x),
                    f64::from(self.bounds.min_y()),
                    f64::from(x + s.max(1.0)),
                    f64::from(self.bounds.max_y()),
                ),
                cx.color(TokenKey::DividerColor, LINE),
            );
        }
        for (i, row) in self.rows.iter().enumerate() {
            let r = self.row_rects[i];
            if row.is_conflict() {
                cx.list
                    .push_fill_rect(krect(r), cx.color(TokenKey::WarningColor, TINT));
            } else if row.resolution.is_some() {
                cx.list
                    .push_fill_rect(krect(r), cx.color(TokenKey::SuccessColor, RESOLVED));
            }
            if self.focused == Some(i) {
                cx.list.push_fill_rect(krect(r), FOCUS);
            }
            let mid_y = r.min_y() + ROW_PT * 0.72 * s;
            for (ci, text) in [&row.ours, &row.result, &row.theirs].iter().enumerate() {
                if !text.is_empty() {
                    crate::text_paint::paint_label_clipped(
                        painter,
                        cx.list,
                        krect(Rect::new(
                            self.col_x[ci],
                            r.min_y(),
                            self.col_x[ci + 1] - self.col_x[ci],
                            r.height(),
                        )),
                        kurbo::Point::new(f64::from(self.col_x[ci] + PAD_PT * s), f64::from(mid_y)),
                        text,
                        FONT_PT * s,
                        cx.color(TokenKey::TextColor, TEXT),
                    );
                }
            }
            // Accept buttons on conflict rows.
            if row.is_conflict() {
                let (l, rr) = self.accept_rects[i];
                for (rect, glyph) in [(l, "‹"), (rr, "›")] {
                    cx.list.push_stroke_rect(
                        krect(rect),
                        s.max(1.0),
                        cx.color(TokenKey::BorderColor, LINE),
                    );
                    crate::text_paint::paint_label(
                        painter,
                        cx.list,
                        kurbo::Point::new(
                            f64::from(rect.min_x() + rect.width() * 0.3),
                            f64::from(rect.min_y() + rect.height() * 0.72),
                        ),
                        glyph,
                        FONT_PT * s,
                        cx.color(TokenKey::TextColor, TEXT),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn fixture() -> MergeView {
        MergeView::new()
            .row(MergeRow::aligned("same", "same", "same"))
            .row(MergeRow::conflict("ours", "", "theirs"))
            .row(MergeRow::aligned("tail", "tail", "tail"))
    }

    fn laid_out(m: &mut MergeView) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        m.layout(&mut cx, Rect::new(0.0, 0.0, 600.0, 200.0));
    }

    fn ev(m: &mut MergeView, e: &WidgetEvent) -> EventResponse {
        m.event(&mut EventContext {
            event: e,
            bounds: m.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn accept_writes_result() {
        let mut m = fixture();
        m.accept(1, MergeSide::Theirs);
        assert_eq!(m.result(1), Some("theirs"));
        assert_eq!(m.conflict_count(), 0);
        assert_eq!(m.take_choice(), Some((1, MergeSide::Theirs)));
        // Non-conflict rows can't be accepted.
        m.accept(0, MergeSide::Ours);
        assert_eq!(m.take_choice(), None);
    }

    #[test]
    fn accept_buttons_resolve() {
        let mut m = fixture();
        laid_out(&mut m);
        let (l, _) = m.accept_rects[1];
        ev(
            &mut m,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((l.min_x() + l.max_x()) / 2.0, (l.min_y() + l.max_y()) / 2.0),
            },
        );
        assert_eq!(m.take_choice(), Some((1, MergeSide::Ours)));
    }

    #[test]
    fn keys_focus_and_resolve() {
        let mut m = fixture();
        laid_out(&mut m);
        ev(
            &mut m,
            &WidgetEvent::KeyPressed {
                key: "ArrowDown".to_string(),
                repeat: false,
            },
        );
        ev(
            &mut m,
            &WidgetEvent::KeyPressed {
                key: "ArrowDown".to_string(),
                repeat: false,
            },
        );
        assert_eq!(m.focused(), Some(1));
        ev(
            &mut m,
            &WidgetEvent::KeyPressed {
                key: "ArrowRight".to_string(),
                repeat: false,
            },
        );
        assert_eq!(m.take_choice(), Some((1, MergeSide::Theirs)));
    }

    #[test]
    fn row_click_focuses() {
        let mut m = fixture();
        laid_out(&mut m);
        let r = m.row_rects[2];
        ev(
            &mut m,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(r.max_x() - 10.0, (r.min_y() + r.max_y()) / 2.0),
            },
        );
        assert_eq!(m.focused(), Some(2));
    }

    #[test]
    fn paint_without_painter() {
        let mut m = fixture();
        laid_out(&mut m);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        m.paint(&mut PaintContext {
            list: &mut list,
            bounds: m.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
