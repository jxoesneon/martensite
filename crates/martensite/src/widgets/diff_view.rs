//! `DiffView` — a unified-diff display (code-review idiom).
//!
//! Lines are tagged [`DiffKind`]: `Added` paints a green-tinted
//! row, `Removed` a red-tinted row, `Hunk` (the `@@` headers) a
//! muted header, and `Context` a plain row — each with the
//! leading `+`/`-`/`@`/space marker. Clicking a row parks its
//! index in [`DiffView::take_selected`]; mouse-wheel scrolls.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::diff_view::{DiffKind, DiffView};
//!
//! let d = DiffView::new().line(DiffKind::Added, "use foo;");
//! assert_eq!(d.line_count(), 1);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::{paint_label_clipped, SharedTextPainter};

const WIDTH_PT: f32 = 360.0;
const HEIGHT_PT: f32 = 200.0;
const FONT_PT: f32 = 12.0;
const LINE_H: f32 = 1.5;
const PAD_PT: f32 = 8.0;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const CODE: [u8; 4] = [215, 215, 222, 255];
const MUTED: [u8; 4] = [130, 130, 140, 255];
const ADD_TXT: [u8; 4] = [120, 210, 140, 255];
const DEL_TXT: [u8; 4] = [230, 130, 120, 255];
const ADD_BG: [u8; 4] = [60, 160, 90, 40];
const DEL_BG: [u8; 4] = [200, 80, 70, 40];

/// Diff line classification.
///
/// ```
/// use martensite::widgets::diff_view::DiffKind;
///
/// assert_eq!(DiffKind::Added.marker(), '+');
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    /// Unchanged context line.
    Context,
    /// `+` line (green).
    Added,
    /// `-` line (red).
    Removed,
    /// `@@` hunk header (muted).
    Hunk,
}

impl DiffKind {
    /// Leading marker character.
    ///
    /// ```
    /// use martensite::widgets::diff_view::DiffKind;
    ///
    /// assert_eq!(DiffKind::Removed.marker(), '-');
    /// ```
    pub fn marker(self) -> char {
        match self {
            DiffKind::Context => ' ',
            DiffKind::Added => '+',
            DiffKind::Removed => '-',
            DiffKind::Hunk => '@',
        }
    }
}

/// A unified-diff display — see the module docs.
///
/// ```
/// use martensite::widgets::diff_view::DiffView;
///
/// assert_eq!(DiffView::new().line_count(), 0);
/// ```
pub struct DiffView {
    /// Accessibility label.
    pub label: String,
    lines: Vec<(DiffKind, String)>,
    scroll_back: f32,
    pending: Option<usize>,
    selected: Option<usize>,
    painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl Default for DiffView {
    fn default() -> Self {
        Self::new()
    }
}

impl DiffView {
    /// Creates an empty view.
    ///
    /// ```
    /// use martensite::widgets::diff_view::DiffView;
    ///
    /// assert_eq!(DiffView::new().line_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Diff".to_string(),
            lines: Vec::new(),
            scroll_back: 0.0,
            pending: None,
            selected: None,
            painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Appends a line.
    ///
    /// ```
    /// use martensite::widgets::diff_view::{DiffKind, DiffView};
    ///
    /// let d = DiffView::new().line(DiffKind::Context, "same");
    /// assert_eq!(d.line_count(), 1);
    /// ```
    pub fn line(mut self, kind: DiffKind, text: impl Into<String>) -> Self {
        self.lines.push((kind, text.into()));
        self
    }

    /// Replaces all lines.
    ///
    /// ```
    /// use martensite::widgets::diff_view::{DiffKind, DiffView};
    ///
    /// let d = DiffView::new().lines(vec![(DiffKind::Hunk, "@@".to_string())]);
    /// assert_eq!(d.line_count(), 1);
    /// ```
    pub fn lines(mut self, lines: Vec<(DiffKind, String)>) -> Self {
        self.lines = lines;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::diff_view::DiffView;
    ///
    /// assert_eq!(DiffView::new().label("pr #4").label, "pr #4");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter for glyph-accurate layout.
    ///
    /// ```
    /// use martensite::widgets::diff_view::DiffView;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let d = DiffView::new().with_text_painter(shared_painter());
    /// assert_eq!(d.line_count(), 0);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.painter = Some(painter);
        self
    }

    /// Line count.
    ///
    /// ```
    /// use martensite::widgets::diff_view::{DiffKind, DiffView};
    ///
    /// let d = DiffView::new().line(DiffKind::Added, "+");
    /// assert_eq!(d.line_count(), 1);
    /// ```
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Count of `Added`/`Removed`/`Context`/`Hunk` lines.
    ///
    /// ```
    /// use martensite::widgets::diff_view::{DiffKind, DiffView};
    ///
    /// let d = DiffView::new()
    ///     .line(DiffKind::Added, "a")
    ///     .line(DiffKind::Removed, "r")
    ///     .line(DiffKind::Context, "c");
    /// assert_eq!(d.tally(), (1, 1, 1, 0));
    /// ```
    pub fn tally(&self) -> (usize, usize, usize, usize) {
        let mut t = (0, 0, 0, 0);
        for (k, _) in &self.lines {
            match k {
                DiffKind::Added => t.0 += 1,
                DiffKind::Removed => t.1 += 1,
                DiffKind::Context => t.2 += 1,
                DiffKind::Hunk => t.3 += 1,
            }
        }
        t
    }

    /// Currently selected line.
    ///
    /// ```
    /// use martensite::widgets::diff_view::DiffView;
    ///
    /// assert_eq!(DiffView::new().selected(), None);
    /// ```
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Drains the last clicked line index.
    ///
    /// ```
    /// use martensite::widgets::diff_view::DiffView;
    ///
    /// let mut d = DiffView::new();
    /// assert!(d.take_selected().is_none());
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// Lines scrolled back from the bottom.
    ///
    /// ```
    /// use martensite::widgets::diff_view::DiffView;
    ///
    /// assert_eq!(DiffView::new().scroll_offset(), 0.0);
    /// ```
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_back
    }

    fn line_h(&self) -> f32 {
        FONT_PT * LINE_H * self.scale
    }

    fn visible(&self) -> usize {
        (self.bounds.height() / self.line_h().max(1.0)) as usize
    }

    fn first_visible(&self) -> usize {
        self.lines
            .len()
            .saturating_sub(self.scroll_back.floor() as usize)
            .saturating_sub(self.visible())
    }

    fn line_at(&self, p: Vec2) -> Option<usize> {
        if !self.bounds.contains(p) {
            return None;
        }
        let row = ((p.y - self.bounds.min_y()) / self.line_h()) as usize;
        let i = self.first_visible() + row;
        (i < self.lines.len()).then_some(i)
    }

    fn text_color(&self, cx: &mut PaintContext, kind: DiffKind) -> [u8; 4] {
        match kind {
            DiffKind::Added => cx.color(TokenKey::SuccessColor, ADD_TXT),
            DiffKind::Removed => cx.color(TokenKey::ErrorColor, DEL_TXT),
            DiffKind::Hunk => cx.color(TokenKey::TextMutedColor, MUTED),
            DiffKind::Context => cx.color(TokenKey::TextColor, CODE),
        }
    }
}

impl Widget for DiffView {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Code);
        let (add, del, _, _) = self.tally();
        node.set_label(format!("{} — +{add} −{del}", self.label));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::Scroll { delta, .. } => {
                let max = self.lines.len().saturating_sub(self.visible()) as f32;
                let next = (self.scroll_back - delta.y / self.line_h()).clamp(0.0, max);
                if (next - self.scroll_back).abs() > f32::EPSILON {
                    self.scroll_back = next;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self.line_at(*position) {
                    self.selected = Some(i);
                    self.pending = Some(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.color(TokenKey::InsetColor, FACE),
        );
        let line_h = self.line_h();
        let start = self.first_visible();
        let end = (start + self.visible() + 1).min(self.lines.len());
        let painter = crate::text_paint::resolve_painter(&self.painter, cx.text_painter);
        let size = FONT_PT * cx.scale;
        let pad = PAD_PT * self.scale;

        for (row, i) in (start..end).enumerate() {
            let (kind, text) = &self.lines[i];
            let y = self.bounds.min_y() + row as f32 * line_h;
            // Row wash for added/removed.
            match kind {
                DiffKind::Added | DiffKind::Removed => {
                    let wash = if *kind == DiffKind::Added {
                        cx.color(TokenKey::SuccessColor, ADD_BG)
                    } else {
                        cx.color(TokenKey::ErrorColor, DEL_BG)
                    };
                    cx.list.push_fill_rect(
                        f(Rect::new(
                            self.bounds.min_x(),
                            y,
                            self.bounds.width(),
                            line_h,
                        )),
                        wash,
                    );
                }
                _ => {}
            }
            let ty = y + (line_h - size) * 0.5;
            let mut full = String::with_capacity(text.len() + 2);
            full.push(kind.marker());
            full.push(' ');
            full.push_str(text);
            let color = self.text_color(cx, *kind);
            paint_label_clipped(
                painter,
                cx.list,
                f(self.bounds),
                kurbo::Point::new(f64::from(self.bounds.min_x() + pad), f64::from(ty)),
                &full,
                size,
                color,
            );
        }
        cx.list.push_stroke_shape(
            f(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.pt(0.75),
            cx.color(TokenKey::BorderColor, EDGE),
        );
    }
}

impl std::fmt::Debug for DiffView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiffView")
            .field("lines", &self.lines.len())
            .field("selected", &self.selected)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(d: &mut DiffView, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        d.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        d.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev(d: &mut DiffView, e: WidgetEvent) {
        d.event(&mut EventContext {
            event: &e,
            bounds: Rect::new(0.0, 0.0, 360.0, 200.0),
            scale: 1.0,
        });
    }

    #[test]
    fn tally_counts() {
        let d = DiffView::new()
            .line(DiffKind::Hunk, "@@ -1,2 +1,2 @@")
            .line(DiffKind::Removed, "old")
            .line(DiffKind::Added, "new")
            .line(DiffKind::Context, "same");
        assert_eq!(d.tally(), (1, 1, 1, 1));
    }

    #[test]
    fn markers() {
        assert_eq!(DiffKind::Added.marker(), '+');
        assert_eq!(DiffKind::Removed.marker(), '-');
        assert_eq!(DiffKind::Hunk.marker(), '@');
        assert_eq!(DiffKind::Context.marker(), ' ');
    }

    #[test]
    fn click_selects() {
        let mut d = DiffView::new().line(DiffKind::Added, "x");
        laid_out(&mut d, 360.0, 200.0);
        ev(
            &mut d,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(200.0, 8.0),
                count: 1,
            },
        );
        assert_eq!(d.take_selected(), Some(0));
        assert_eq!(d.selected(), Some(0));
    }

    #[test]
    fn wheel_scrolls() {
        let mut d = DiffView::new().lines(
            (0..60)
                .map(|i| (DiffKind::Context, i.to_string()))
                .collect(),
        );
        laid_out(&mut d, 360.0, 60.0);
        ev(
            &mut d,
            WidgetEvent::Scroll {
                delta: Vec2::new(0.0, -36.0),
                position: Vec2::new(100.0, 30.0),
            },
        );
        assert!(d.scroll_offset() > 0.0);
    }

    #[test]
    fn scroll_clamps_at_zero() {
        let mut d = DiffView::new().line(DiffKind::Context, "x");
        laid_out(&mut d, 360.0, 60.0);
        ev(
            &mut d,
            WidgetEvent::Scroll {
                delta: Vec2::new(0.0, -400.0),
                position: Vec2::new(100.0, 30.0),
            },
        );
        assert_eq!(d.scroll_offset(), 0.0);
    }
}
