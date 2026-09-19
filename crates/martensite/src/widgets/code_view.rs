//! `CodeView` — a read-only monospace code display with a
//! line-number gutter (editor / review-tool idiom).
//!
//! Source lines paint in monospace under a right-aligned gutter
//! of line numbers; the `current` line gets an accent wash, and
//! clicking a line parks its `0`-based index in
//! [`CodeView::take_selected`]. Mouse-wheel scrolls; a `follow`
//! flag keeps `current` visible.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::code_view::CodeView;
//!
//! let c = CodeView::new().lines(["fn main() {", "}", ""]);
//! assert_eq!(c.line_count(), 3);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::{paint_label_clipped, SharedTextPainter};

const WIDTH_PT: f32 = 320.0;
const HEIGHT_PT: f32 = 200.0;
const FONT_PT: f32 = 12.0;
const LINE_H: f32 = 1.5;
const PAD_PT: f32 = 6.0;
const GUTTER_PAD_PT: f32 = 10.0;

const FACE: [u8; 4] = [36, 36, 42, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const CODE: [u8; 4] = [215, 215, 222, 255];
const GUTTER: [u8; 4] = [120, 120, 128, 255];
const HOT: [u8; 4] = [80, 140, 220, 60];

/// A monospace code display — see the module docs.
///
/// ```
/// use martensite::widgets::code_view::CodeView;
///
/// assert_eq!(CodeView::new().line_count(), 0);
/// ```
pub struct CodeView {
    /// Accessibility label.
    pub label: String,
    /// Highlighted line index (`0`-based), if any.
    pub current: Option<usize>,
    /// When `true`, scrolls so `current` stays visible.
    pub follow: bool,
    lines: Vec<String>,
    scroll_back: f32,
    pending: Option<usize>,
    painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl Default for CodeView {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeView {
    /// Creates an empty view.
    ///
    /// ```
    /// use martensite::widgets::code_view::CodeView;
    ///
    /// assert_eq!(CodeView::new().line_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Code".to_string(),
            current: None,
            follow: true,
            lines: Vec::new(),
            scroll_back: 0.0,
            pending: None,
            painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Replaces the source lines.
    ///
    /// ```
    /// use martensite::widgets::code_view::CodeView;
    ///
    /// let c = CodeView::new().lines(["a", "b"]);
    /// assert_eq!(c.line_count(), 2);
    /// ```
    pub fn lines(mut self, lines: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.lines = lines.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the highlighted line.
    ///
    /// ```
    /// use martensite::widgets::code_view::CodeView;
    ///
    /// let c = CodeView::new().lines(["x"]).current(Some(0));
    /// assert_eq!(c.current, Some(0));
    /// ```
    pub fn current(mut self, line: Option<usize>) -> Self {
        self.current = line.filter(|&i| i < self.lines.len());
        self
    }

    /// Keeps `current` in view while scrolling (default `true`).
    ///
    /// ```
    /// use martensite::widgets::code_view::CodeView;
    ///
    /// assert!(!CodeView::new().follow(false).follow);
    /// ```
    pub fn follow(mut self, follow: bool) -> Self {
        self.follow = follow;
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::code_view::CodeView;
    ///
    /// assert_eq!(CodeView::new().label("main.rs").label, "main.rs");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter for glyph-accurate layout.
    ///
    /// ```
    /// use martensite::widgets::code_view::CodeView;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let c = CodeView::new().with_text_painter(shared_painter());
    /// assert_eq!(c.line_count(), 0);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.painter = Some(painter);
        self
    }

    /// Line count.
    ///
    /// ```
    /// use martensite::widgets::code_view::CodeView;
    ///
    /// assert_eq!(CodeView::new().lines(["a", "b"]).line_count(), 2);
    /// ```
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// The source lines.
    ///
    /// ```
    /// use martensite::widgets::code_view::CodeView;
    ///
    /// assert_eq!(CodeView::new().lines(["x"]).line_list()[0], "x");
    /// ```
    pub fn line_list(&self) -> &[String] {
        &self.lines
    }

    /// Drains the last clicked line index.
    ///
    /// ```
    /// use martensite::widgets::code_view::CodeView;
    ///
    /// let mut c = CodeView::new();
    /// assert!(c.take_selected().is_none());
    /// ```
    pub fn take_selected(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// Scrolls to the bottom.
    ///
    /// ```
    /// use martensite::widgets::code_view::CodeView;
    ///
    /// let mut c = CodeView::new();
    /// c.scroll_to_bottom();
    /// assert_eq!(c.scroll_offset(), 0.0);
    /// ```
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_back = 0.0;
    }

    /// Lines scrolled back from the bottom.
    ///
    /// ```
    /// use martensite::widgets::code_view::CodeView;
    ///
    /// assert_eq!(CodeView::new().scroll_offset(), 0.0);
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

    /// Gutter width from the largest line number's digit count.
    fn gutter_w(&self) -> f32 {
        let digits = self.lines.len().to_string().len().max(2) as f32;
        digits * FONT_PT * 0.62 * self.scale + GUTTER_PAD_PT * self.scale
    }

    /// First visible line index.
    fn first_visible(&self) -> usize {
        self.lines
            .len()
            .saturating_sub(self.scroll_back.floor() as usize)
            .saturating_sub(self.visible())
    }

    /// Line index under a point.
    fn line_at(&self, p: Vec2) -> Option<usize> {
        if !self.bounds.contains(p) {
            return None;
        }
        let row = ((p.y - self.bounds.min_y()) / self.line_h()) as usize;
        let i = self.first_visible() + row;
        (i < self.lines.len()).then_some(i)
    }

    /// Keeps `current` visible when `follow` is on.
    fn ensure_visible(&mut self) {
        if !self.follow {
            return;
        }
        if let Some(c) = self.current {
            let vis = self.visible().max(1);
            let first = self.first_visible();
            if c < first {
                self.scroll_back = self.lines.len().saturating_sub(c + vis) as f32;
            } else if c >= first + vis {
                self.scroll_back = self.lines.len().saturating_sub(c + 1) as f32;
            }
        }
    }
}

impl Widget for CodeView {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(WIDTH_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(100.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.ensure_visible();
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Code);
        node.set_label(format!("{} — {} lines", self.label, self.lines.len()));
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
                    self.current = Some(i);
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
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let gutter_w = self.gutter_w();
        let line_h = self.line_h();
        let start = self.first_visible();
        let end = (start + self.visible() + 1).min(self.lines.len());
        let painter = crate::text_paint::resolve_painter(&self.painter, cx.text_painter);
        let size = FONT_PT * cx.scale;
        let code_x = self.bounds.min_x() + gutter_w;
        let pad = PAD_PT * self.scale;

        // Current-line wash.
        if let Some(c) = self.current {
            if c >= start && c < end {
                let y = self.bounds.min_y() + (c - start) as f32 * line_h;
                cx.list.push_fill_rect(
                    f(Rect::new(
                        self.bounds.min_x(),
                        y,
                        self.bounds.width(),
                        line_h,
                    )),
                    cx.color(TokenKey::AccentColor, HOT),
                );
            }
        }
        for (row, i) in (start..end).enumerate() {
            let y = self.bounds.min_y() + row as f32 * line_h + (line_h - size) * 0.5;
            // Gutter number (right-aligned edge estimate).
            let num = (i + 1).to_string();
            let num_w = num.len() as f32 * size * 0.62;
            paint_label_clipped(
                painter,
                cx.list,
                f(self.bounds),
                kurbo::Point::new(
                    f64::from(code_x - GUTTER_PAD_PT * self.scale - num_w),
                    f64::from(y),
                ),
                &num,
                size,
                cx.color(TokenKey::TextMutedColor, GUTTER),
            );
            // Code line.
            paint_label_clipped(
                painter,
                cx.list,
                f(self.bounds),
                kurbo::Point::new(f64::from(code_x + pad), f64::from(y)),
                &self.lines[i],
                size,
                cx.color(TokenKey::TextColor, CODE),
            );
        }
        // Gutter divider.
        let mut div = kurbo::BezPath::new();
        div.move_to((f64::from(code_x), f64::from(self.bounds.min_y())));
        div.line_to((f64::from(code_x), f64::from(self.bounds.max_y())));
        cx.list
            .push_stroke_path(div, cx.pt(0.75), cx.color(TokenKey::BorderColor, EDGE));
    }
}

impl std::fmt::Debug for CodeView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeView")
            .field("lines", &self.lines.len())
            .field("current", &self.current)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut CodeView, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        c.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev(c: &mut CodeView, e: WidgetEvent) {
        c.event(&mut EventContext {
            event: &e,
            bounds: Rect::new(0.0, 0.0, 320.0, 200.0),
            scale: 1.0,
        });
    }

    #[test]
    fn line_count_and_clamp() {
        let c = CodeView::new().lines(["a", "b"]).current(Some(9));
        assert_eq!(c.line_count(), 2);
        assert_eq!(c.current, None); // out of range filtered
    }

    #[test]
    fn click_selects_line() {
        let mut c = CodeView::new().lines(["a", "b", "c"]);
        laid_out(&mut c, 320.0, 200.0);
        ev(
            &mut c,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(200.0, 30.0), // second row-ish
                count: 1,
            },
        );
        assert_eq!(c.take_selected(), Some(1));
        assert_eq!(c.current, Some(1));
    }

    #[test]
    fn wheel_scrolls() {
        let mut c = CodeView::new().lines((0..50).map(|i| i.to_string()).collect::<Vec<_>>());
        laid_out(&mut c, 320.0, 60.0); // ~3 visible rows
        ev(
            &mut c,
            WidgetEvent::Scroll {
                delta: Vec2::new(0.0, -40.0),
                position: Vec2::new(100.0, 30.0),
            },
        );
        assert!(c.scroll_offset() > 0.0);
    }

    #[test]
    fn scroll_clamps() {
        let mut c = CodeView::new().lines(["only"]);
        laid_out(&mut c, 320.0, 60.0);
        ev(
            &mut c,
            WidgetEvent::Scroll {
                delta: Vec2::new(0.0, -500.0),
                position: Vec2::new(100.0, 30.0),
            },
        );
        assert_eq!(c.scroll_offset(), 0.0);
    }

    #[test]
    fn follow_keeps_current_visible() {
        let mut c = CodeView::new()
            .lines((0..50).map(|i| i.to_string()).collect::<Vec<_>>())
            .current(Some(45));
        laid_out(&mut c, 320.0, 60.0); // 3 visible
        let first = c.first_visible();
        assert!(first <= 45 && first + c.visible() > 45);
    }
}
