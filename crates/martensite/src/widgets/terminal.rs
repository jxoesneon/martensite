//! `Terminal` — a scrollback display with a live prompt line
//! (the read-only emulator surface; VT parsing and PTY wiring
//! are the host's job).
//!
//! [`Terminal::write`] appends output lines; the prompt row at
//! the bottom shows `prompt` + the in-progress `input` buffer
//! with a caret that blinks on
//! [`Terminal::tick`](martensite_core::widget::Widget::tick).
//! Character
//! keys append to `input`, `Backspace` deletes, and `Enter`
//! moves the buffer to the scrollback as a `prompt`-prefixed
//! echo, parking it in [`Terminal::take_submitted`]. The wheel
//! scrolls the backlog.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::terminal::Terminal;
//!
//! let mut t = Terminal::new().prompt("$");
//! t.write("hello world");
//! t.submit("ls -la");
//! assert_eq!(t.line_count(), 2);
//! assert_eq!(t.line(1), Some("$ ls -la"));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use std::time::Duration;

const W_PT: f32 = 420.0;
const H_PT: f32 = 240.0;
const ROW_PT: f32 = 16.0;
const PAD_PT: f32 = 8.0;
const CARET_BLINK: f32 = 0.5;
/// Scrollback cap.
const MAX_LINES: usize = 1000;

const FACE: [u8; 4] = [16, 18, 22, 255];
const TEXT: [u8; 4] = [200, 215, 200, 255];
const PROMPT: [u8; 4] = [120, 220, 140, 255];
const CARET: [u8; 4] = [220, 230, 220, 255];

/// A scrollback + prompt terminal surface — see the module
/// docs.
///
/// ```
/// use martensite::widgets::terminal::Terminal;
///
/// assert_eq!(Terminal::new().line_count(), 0);
/// ```
pub struct Terminal {
    /// Accessibility label.
    pub label: String,
    /// Prompt glyph(s) shown before input.
    pub prompt: String,
    /// Scrollback lines (echo included), oldest first.
    lines: Vec<String>,
    /// In-progress input buffer.
    input: String,
    scroll: f32,
    /// Caret blink phase.
    blink: f32,
    caret_on: bool,
    submitted: Option<String>,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminal")
            .field("lines", &self.lines.len())
            .field("input", &self.input)
            .finish()
    }
}

impl Default for Terminal {
    fn default() -> Self {
        Self::new()
    }
}

impl Terminal {
    /// Creates an empty terminal with a `>` prompt.
    ///
    /// ```
    /// use martensite::widgets::terminal::Terminal;
    ///
    /// assert_eq!(Terminal::new().prompt, ">");
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Terminal".to_string(),
            prompt: ">".to_string(),
            lines: Vec::new(),
            input: String::new(),
            scroll: 0.0,
            blink: 0.0,
            caret_on: true,
            submitted: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Prompt glyph(s).
    ///
    /// ```
    /// use martensite::widgets::terminal::Terminal;
    ///
    /// assert_eq!(Terminal::new().prompt("❯").prompt, "❯");
    /// ```
    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = prompt.into();
        self
    }

    /// Initial scrollback lines.
    ///
    /// ```
    /// use martensite::widgets::terminal::Terminal;
    ///
    /// assert_eq!(Terminal::new().lines(["a", "b"]).line_count(), 2);
    /// ```
    pub fn lines(mut self, ls: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.lines = ls.into_iter().map(Into::into).collect();
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::terminal::Terminal;
    ///
    /// assert_eq!(Terminal::new().label("shell").label, "shell");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::terminal::Terminal;
    ///
    /// let _ = Terminal::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Appends an output line to the scrollback (capped at
    /// `MAX_LINES`).
    ///
    /// ```
    /// use martensite::widgets::terminal::Terminal;
    ///
    /// let mut t = Terminal::new();
    /// t.write("out");
    /// assert_eq!(t.line(0), Some("out"));
    /// ```
    pub fn write(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
        if self.lines.len() > MAX_LINES {
            let drop = self.lines.len() - MAX_LINES;
            self.lines.drain(0..drop);
        }
        self.scroll = self.max_scroll();
    }

    /// Line `i` in the scrollback.
    ///
    /// ```
    /// use martensite::widgets::terminal::Terminal;
    ///
    /// let mut t = Terminal::new().lines(["x"]);
    /// assert_eq!(t.line(0), Some("x"));
    /// ```
    pub fn line(&self, i: usize) -> Option<&str> {
        self.lines.get(i).map(String::as_str)
    }

    /// Scrollback line count.
    ///
    /// ```
    /// use martensite::widgets::terminal::Terminal;
    ///
    /// assert_eq!(Terminal::new().lines(["a"]).line_count(), 1);
    /// ```
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// The in-progress input buffer.
    ///
    /// ```
    /// use martensite::widgets::terminal::Terminal;
    ///
    /// assert_eq!(Terminal::new().input(), "");
    /// ```
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Appends `text` to the input buffer (host-driven edit).
    ///
    /// ```
    /// use martensite::widgets::terminal::Terminal;
    ///
    /// let mut t = Terminal::new();
    /// t.type_str("ls");
    /// assert_eq!(t.input(), "ls");
    /// ```
    pub fn type_str(&mut self, text: &str) {
        self.input.push_str(text);
    }

    /// Submits the input buffer: echoes `prompt + input` to the
    /// scrollback, parks the text in `take_submitted`, and
    /// clears the buffer.
    ///
    /// ```
    /// use martensite::widgets::terminal::Terminal;
    ///
    /// let mut t = Terminal::new().prompt("$");
    /// t.type_str("pwd");
    /// t.submit_line();
    /// assert_eq!(t.take_submitted(), Some("pwd".to_string()));
    /// assert_eq!(t.input(), "");
    /// ```
    pub fn submit_line(&mut self) {
        let text = std::mem::take(&mut self.input);
        self.write(format!("{} {}", self.prompt, text));
        self.submitted = Some(text);
    }

    /// Submits `text` directly (echoes to the scrollback and
    /// parks it) without touching the input buffer.
    ///
    /// ```
    /// use martensite::widgets::terminal::Terminal;
    ///
    /// let mut t = Terminal::new().prompt("$");
    /// t.submit("ls");
    /// assert_eq!(t.line(0), Some("$ ls"));
    /// ```
    pub fn submit(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.write(format!("{} {}", self.prompt, text));
        self.submitted = Some(text);
    }

    /// Drains the last submitted command.
    ///
    /// ```
    /// use martensite::widgets::terminal::Terminal;
    ///
    /// assert_eq!(Terminal::new().take_submitted(), None);
    /// ```
    pub fn take_submitted(&mut self) -> Option<String> {
        self.submitted.take()
    }

    /// Current scroll offset.
    ///
    /// ```
    /// use martensite::widgets::terminal::Terminal;
    ///
    /// assert_eq!(Terminal::new().scroll(), 0.0);
    /// ```
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// Max scroll offset.
    fn max_scroll(&self) -> f32 {
        let row = ROW_PT * self.scale;
        let pad = PAD_PT * self.scale;
        (self.lines.len() as f32 * row + pad * 2.0 + row - self.bounds.height()).max(0.0)
    }
}

impl Widget for Terminal {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(160.0, 60.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Terminal);
        node.set_label(format!(
            "{} — {} lines, input {}",
            self.label,
            self.lines.len(),
            self.input
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::Scroll { position, delta } => {
                if self.bounds.contains(*position) {
                    self.scroll = (self.scroll - delta.y).clamp(0.0, self.max_scroll());
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::ImeCommitted { text } => {
                self.input.push_str(text);
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "Backspace" => {
                    self.input.pop();
                    EventResponse::RequestRepaint
                }
                "Enter" => {
                    self.submit_line();
                    EventResponse::RequestRepaint
                }
                // Single-character keys type directly (IME-free path).
                k if k.chars().count() == 1 => {
                    self.input.push_str(k);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, dt: Duration) -> bool {
        self.blink += dt.as_secs_f32();
        if self.blink >= CARET_BLINK {
            self.blink -= CARET_BLINK;
            self.caret_on = !self.caret_on;
            return true;
        }
        false
    }

    fn paint(&self, cx: &mut PaintContext) {
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::RECT,
            FACE,
        );
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let s = self.scale;
        let row = ROW_PT * s;
        let pad = PAD_PT * s;
        let size = 12.0 * s;
        let text = cx.color(TokenKey::TextColor, TEXT);
        let prompt = cx.color(TokenKey::SuccessColor, PROMPT);
        cx.list.push_clip(krect(self.bounds));
        // Scrollback.
        let mut y = self.bounds.min_y() + pad - self.scroll;
        for line in &self.lines {
            if y + row > self.bounds.min_y() && y < self.bounds.max_y() - row {
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(self.bounds.min_x() + pad),
                        f64::from(y + row * 0.12),
                    ),
                    line,
                    size,
                    text,
                );
            }
            y += row;
        }
        // Prompt line pinned to the bottom.
        let py = self.bounds.max_y() - pad - row * 0.8;
        let px = self.bounds.min_x() + pad;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(f64::from(px), f64::from(py)),
            &self.prompt,
            size,
            prompt,
        );
        let poff = (self.prompt.chars().count() + 1) as f32 * size * 0.62;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(f64::from(px + poff), f64::from(py)),
            &self.input,
            size,
            text,
        );
        if self.caret_on {
            let cxp = px + poff + self.input.chars().count() as f32 * size * 0.62;
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(cxp),
                    f64::from(py),
                    f64::from(cxp + size * 0.62),
                    f64::from(py + size * 1.15),
                ),
                &martensite_core::shape::Shape::RECT,
                cx.color(TokenKey::TextColor, CARET),
            );
        }
        cx.list.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(w: &mut Terminal, wd: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(wd, h),
            },
        );
        w.layout(&mut cx, Rect::new(0.0, 0.0, wd, h));
    }

    #[test]
    fn write_and_read() {
        let mut t = Terminal::new().prompt("$");
        t.write("out1");
        t.submit("ls");
        assert_eq!(t.line_count(), 2);
        assert_eq!(t.line(1), Some("$ ls"));
        assert_eq!(t.take_submitted(), Some("ls".to_string()));
        assert_eq!(t.take_submitted(), None);
    }

    #[test]
    fn typing_edits_buffer() {
        let mut t = Terminal::new();
        laid_out(&mut t, 420.0, 240.0);
        t.event(&mut EventContext {
            event: &WidgetEvent::ImeCommitted {
                text: "ls".to_string(),
            },
            bounds: t.bounds,
            scale: 1.0,
        });
        assert_eq!(t.input(), "ls");
        t.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: "Backspace".to_string(),
                repeat: false,
            },
            bounds: t.bounds,
            scale: 1.0,
        });
        assert_eq!(t.input(), "l");
    }

    #[test]
    fn enter_echoes_and_parks() {
        let mut t = Terminal::new().prompt("$");
        laid_out(&mut t, 420.0, 240.0);
        t.type_str("pwd");
        t.event(&mut EventContext {
            event: &WidgetEvent::KeyPressed {
                key: "Enter".to_string(),
                repeat: false,
            },
            bounds: t.bounds,
            scale: 1.0,
        });
        assert_eq!(t.input(), "");
        assert_eq!(t.line(0), Some("$ pwd"));
        assert_eq!(t.take_submitted(), Some("pwd".to_string()));
    }

    #[test]
    fn caret_blinks() {
        let mut t = Terminal::new();
        let on = t.caret_on;
        assert!(t.tick(Duration::from_millis(600)));
        assert_ne!(t.caret_on, on);
    }

    #[test]
    fn scrollback_caps() {
        let mut t = Terminal::new();
        for i in 0..1100 {
            t.write(format!("line {i}"));
        }
        assert_eq!(t.line_count(), MAX_LINES);
        assert_eq!(t.line(0), Some("line 100"));
    }
}
