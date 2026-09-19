//! `LogView` — a scrolling monospace log display (IDE console,
//! `journalctl -f`, Ant `Log`-style feeds).
//!
//! Append-only ring of [`LogLine`]s capped at `max_lines`. While
//! `follow` is on the view pins to the newest line; wheeling up
//! unfollows and reveals history, wheeling back to the bottom
//! re-follows. Lines paint severity-colored (debug muted, info
//! ink, warning, error) through the ambient text painter.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::log_view::{LogSeverity, LogView};
//!
//! let mut log = LogView::new();
//! log.push(LogSeverity::Info, "service started");
//! log.push(LogSeverity::Error, "disk full");
//! assert_eq!(log.len(), 2);
//! ```

use std::collections::VecDeque;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::{paint_label_clipped, SharedTextPainter};

const LINE_PT: f32 = 18.0;
const PAD_PT: f32 = 8.0;
const FONT_PT: f32 = 12.0;
const DEFAULT_MAX: usize = 1000;

const INK: [u8; 4] = [30, 30, 36, 255];
const MUTED: [u8; 4] = [110, 110, 118, 255];
const WARN: [u8; 4] = [200, 140, 20, 255];
const ERR: [u8; 4] = [200, 60, 60, 255];
const SURFACE: [u8; 4] = [250, 250, 252, 255];
const BORDER: [u8; 4] = [210, 212, 216, 255];

/// Severity bucket for a log line.
///
/// ```
/// use martensite::widgets::log_view::LogSeverity;
///
/// assert_ne!(LogSeverity::Info, LogSeverity::Error);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogSeverity {
    /// Trace noise — muted.
    Debug,
    /// Normal informational output.
    Info,
    /// Attention needed.
    Warning,
    /// Failure.
    Error,
}

/// One appended log line.
///
/// ```
/// use martensite::widgets::log_view::{LogLine, LogSeverity};
///
/// let l = LogLine::new(LogSeverity::Info, "ok");
/// assert_eq!(l.text, "ok");
/// ```
#[derive(Clone, Debug)]
pub struct LogLine {
    /// Severity bucket (drives the line color).
    pub severity: LogSeverity,
    /// Line text.
    pub text: String,
}

impl LogLine {
    /// Creates a line.
    ///
    /// ```
    /// use martensite::widgets::log_view::{LogLine, LogSeverity};
    ///
    /// let l = LogLine::new(LogSeverity::Warning, "high load");
    /// assert_eq!(l.severity, LogSeverity::Warning);
    /// ```
    pub fn new(severity: LogSeverity, text: impl Into<String>) -> Self {
        Self {
            severity,
            text: text.into(),
        }
    }
}

/// A scrolling log display — see the module docs.
///
/// ```
/// use martensite::widgets::log_view::LogView;
///
/// let log = LogView::new();
/// assert!(log.is_empty());
/// ```
pub struct LogView {
    /// When `false` the view is inert.
    pub enabled: bool,
    /// Ring capacity — oldest lines drop past this.
    pub max_lines: usize,
    lines: VecDeque<LogLine>,
    /// Lines above the bottom (0 = pinned to the newest line).
    scroll_back: f32,
    follow: bool,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl Default for LogView {
    fn default() -> Self {
        Self::new()
    }
}

impl LogView {
    /// Creates an empty, following log.
    ///
    /// ```
    /// use martensite::widgets::log_view::LogView;
    ///
    /// let log = LogView::new();
    /// assert!(log.is_following());
    /// ```
    pub fn new() -> Self {
        Self {
            enabled: true,
            max_lines: DEFAULT_MAX,
            lines: VecDeque::new(),
            scroll_back: 0.0,
            follow: true,
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Enables or disables the view.
    ///
    /// ```
    /// use martensite::widgets::log_view::LogView;
    ///
    /// let log = LogView::new().enabled(false);
    /// assert!(!log.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets the ring capacity.
    ///
    /// ```
    /// use martensite::widgets::log_view::LogView;
    ///
    /// let log = LogView::new().max_lines(100);
    /// assert_eq!(log.max_lines, 100);
    /// ```
    pub fn max_lines(mut self, max: usize) -> Self {
        self.max_lines = max.max(1);
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::log_view::LogView;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let log = LogView::new().with_text_painter(shared_painter());
    /// assert!(log.is_empty());
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Appends a line; drops the oldest when over `max_lines`.
    ///
    /// ```
    /// use martensite::widgets::log_view::{LogSeverity, LogView};
    ///
    /// let mut log = LogView::new();
    /// log.push(LogSeverity::Debug, "tick");
    /// assert_eq!(log.line(0).unwrap().text, "tick");
    /// ```
    pub fn push(&mut self, severity: LogSeverity, text: impl Into<String>) {
        self.lines.push_back(LogLine::new(severity, text));
        while self.lines.len() > self.max_lines {
            self.lines.pop_front();
        }
    }

    /// Empties the log and re-follows.
    ///
    /// ```
    /// use martensite::widgets::log_view::{LogSeverity, LogView};
    ///
    /// let mut log = LogView::new();
    /// log.push(LogSeverity::Info, "x");
    /// log.clear();
    /// assert!(log.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll_back = 0.0;
        self.follow = true;
    }

    /// Line count.
    ///
    /// ```
    /// use martensite::widgets::log_view::{LogSeverity, LogView};
    ///
    /// let mut log = LogView::new();
    /// log.push(LogSeverity::Info, "x");
    /// assert_eq!(log.len(), 1);
    /// ```
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// `true` when empty.
    ///
    /// ```
    /// use martensite::widgets::log_view::LogView;
    ///
    /// assert!(LogView::new().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Borrows line `i` (oldest-first).
    ///
    /// ```
    /// use martensite::widgets::log_view::{LogSeverity, LogView};
    ///
    /// let mut log = LogView::new();
    /// log.push(LogSeverity::Error, "boom");
    /// assert_eq!(log.line(0).unwrap().severity, LogSeverity::Error);
    /// ```
    pub fn line(&self, i: usize) -> Option<&LogLine> {
        self.lines.get(i)
    }

    /// `true` while pinned to the newest line.
    ///
    /// ```
    /// use martensite::widgets::log_view::LogView;
    ///
    /// assert!(LogView::new().is_following());
    /// ```
    pub fn is_following(&self) -> bool {
        self.follow
    }

    /// Lines scrolled above the bottom.
    ///
    /// ```
    /// use martensite::widgets::log_view::LogView;
    ///
    /// assert_eq!(LogView::new().scroll_offset(), 0.0);
    /// ```
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_back
    }

    /// Scrolls `lines` up from the bottom (0 = follow).
    ///
    /// ```
    /// use martensite::widgets::log_view::{LogSeverity, LogView};
    ///
    /// let mut log = LogView::new();
    /// log.push(LogSeverity::Info, "x");
    /// log.push(LogSeverity::Info, "y");
    /// log.push(LogSeverity::Info, "z");
    /// log.push(LogSeverity::Info, "w");
    /// log.set_scroll_offset(3.0);
    /// assert_eq!(log.scroll_offset(), 3.0);
    /// assert!(!log.is_following());
    /// ```
    pub fn set_scroll_offset(&mut self, lines: f32) {
        self.scroll_back = lines.clamp(0.0, self.lines.len() as f32);
        self.follow = self.scroll_back <= 0.0;
    }

    fn line_h(&self) -> f32 {
        LINE_PT * self.scale
    }

    fn severity_color(&self, cx: &PaintContext, s: LogSeverity) -> [u8; 4] {
        match s {
            LogSeverity::Debug => cx.color(TokenKey::TextMutedColor, MUTED),
            LogSeverity::Info => cx.color(TokenKey::TextColor, INK),
            LogSeverity::Warning => cx.color(TokenKey::WarningColor, WARN),
            LogSeverity::Error => cx.color(TokenKey::ErrorColor, ERR),
        }
    }
}

impl Widget for LogView {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(240.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(160.0).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(80.0, 36.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Log);
        node.set_label("Log");
        node.set_value(format!("{} lines", self.lines.len()));
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        if let WidgetEvent::Scroll { position, delta } = cx.event {
            if !self.bounds.contains(*position) {
                return EventResponse::Ignored;
            }
            let lines_delta = -delta.y / self.line_h();
            let next = (self.scroll_back + lines_delta)
                .clamp(0.0, self.lines.len().saturating_sub(1) as f32);
            if (next - self.scroll_back).abs() > f32::EPSILON || self.follow != (next <= 0.0) {
                self.scroll_back = next;
                self.follow = next <= 0.0;
                return EventResponse::RequestRepaint;
            }
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let shape = martensite_core::shape::Shape::rounded(cx.pt(6.0));
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
            &shape,
            cx.color(TokenKey::SurfaceColor, SURFACE),
        );
        cx.list.push_stroke_shape(
            f(self.bounds),
            &shape,
            cx.pt(0.5).max(1.0),
            cx.color(TokenKey::DividerColor, BORDER),
        );

        let line_h = self.line_h();
        let pad = cx.pt(PAD_PT);
        let inner = Rect::new(
            self.bounds.min_x() + pad,
            self.bounds.min_y() + pad * 0.5,
            (self.bounds.width() - 2.0 * pad).max(0.0),
            (self.bounds.height() - pad).max(0.0),
        );
        let visible = (inner.height() / line_h) as usize;
        if visible == 0 || self.lines.is_empty() {
            return;
        }
        // `end` = index one past the last shown line — the newest line
        // minus the whole-line scroll-back.
        let end = self
            .lines
            .len()
            .saturating_sub(self.scroll_back.floor() as usize);
        let start = end.saturating_sub(visible);
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let size = FONT_PT * cx.scale;
        for (row, i) in (start..end).enumerate() {
            let line = &self.lines[i];
            let y = inner.min_y() + row as f32 * line_h + (line_h - size) * 0.5;
            paint_label_clipped(
                painter,
                cx.list,
                f(inner),
                kurbo::Point::new(f64::from(inner.min_x()), f64::from(y)),
                &line.text,
                size,
                self.severity_color(cx, line.severity),
            );
        }
    }
}

impl std::fmt::Debug for LogView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogView")
            .field("lines", &self.lines.len())
            .field("follow", &self.follow)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(log: &mut LogView, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        log.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        log.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn scroll(log: &mut LogView, dy: f32) {
        log.event(&mut EventContext {
            event: &WidgetEvent::Scroll {
                position: Vec2::new(10.0, 10.0),
                delta: Vec2::new(0.0, dy),
            },
            bounds: Rect::new(0.0, 0.0, 400.0, 200.0),
            scale: 1.0,
        });
    }

    #[test]
    fn push_and_read() {
        let mut log = LogView::new();
        log.push(LogSeverity::Info, "a");
        log.push(LogSeverity::Error, "b");
        assert_eq!(log.len(), 2);
        assert_eq!(log.line(1).unwrap().severity, LogSeverity::Error);
    }

    #[test]
    fn ring_drops_oldest() {
        let mut log = LogView::new().max_lines(3);
        for i in 0..5 {
            log.push(LogSeverity::Info, format!("l{i}"));
        }
        assert_eq!(log.len(), 3);
        assert_eq!(log.line(0).unwrap().text, "l2");
        assert_eq!(log.line(2).unwrap().text, "l4");
    }

    #[test]
    fn clear_resets_follow() {
        let mut log = LogView::new();
        log.push(LogSeverity::Info, "x");
        log.set_scroll_offset(1.0);
        log.clear();
        assert!(log.is_following());
        assert_eq!(log.scroll_offset(), 0.0);
    }

    #[test]
    fn scroll_up_unfollows_down_refollows() {
        let mut log = LogView::new();
        for i in 0..50 {
            log.push(LogSeverity::Info, format!("l{i}"));
        }
        laid_out(&mut log, 400.0, 200.0);
        scroll(&mut log, -36.0); // wheel up two lines
        assert!(!log.is_following());
        assert!(log.scroll_offset() > 0.0);
        scroll(&mut log, 72.0); // wheel back to bottom
        assert!(log.is_following());
        assert_eq!(log.scroll_offset(), 0.0);
    }

    #[test]
    fn scroll_outside_bounds_ignored() {
        let mut log = LogView::new();
        for i in 0..10 {
            log.push(LogSeverity::Info, format!("l{i}"));
        }
        laid_out(&mut log, 400.0, 200.0);
        log.event(&mut EventContext {
            event: &WidgetEvent::Scroll {
                position: Vec2::new(999.0, 999.0),
                delta: Vec2::new(0.0, -50.0),
            },
            bounds: Rect::new(0.0, 0.0, 400.0, 200.0),
            scale: 1.0,
        });
        assert!(log.is_following());
    }

    #[test]
    fn scroll_clamps_at_top() {
        let mut log = LogView::new();
        for i in 0..5 {
            log.push(LogSeverity::Info, format!("l{i}"));
        }
        laid_out(&mut log, 400.0, 200.0);
        scroll(&mut log, -10_000.0);
        assert_eq!(log.scroll_offset(), 4.0); // len-1 max
        assert!(!log.is_following());
    }

    #[test]
    fn disabled_inert() {
        let mut log = LogView::new().enabled(false);
        for i in 0..10 {
            log.push(LogSeverity::Info, format!("l{i}"));
        }
        laid_out(&mut log, 400.0, 200.0);
        scroll(&mut log, -36.0);
        assert!(log.is_following());
    }
}
