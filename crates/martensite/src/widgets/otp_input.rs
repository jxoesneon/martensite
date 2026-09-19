//! `OtpInput` widget: a segmented one-time-code / PIN field (Ant
//! `Input.OTP`, `OTPField` conventions, `WCT`-style PIN boxes).
//!
//! Renders `length` adjacent cells; typed digits fill left-to-right
//! and the caret auto-advances, Backspace retreats, pasted text
//! distributes across cells (non-digits are filtered unless
//! `alphabetic` is set). Poll [`OtpInput::take_completed`] — it fires
//! once each time all cells fill.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::otp_input::OtpInput;
//!
//! let o = OtpInput::new().length(6);
//! assert_eq!(o.get_value(), "");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

/// Cell size, logical points.
const CELL_PT: f32 = 32.0;
/// Gap between cells, logical points.
const GAP_PT: f32 = 8.0;
/// Digit font size, logical points.
const FONT_PT: f32 = 16.0;
/// Cell corner radius, logical points.
const RADIUS_PT: f32 = 6.0;
/// Border width, logical points.
const BORDER_PT: f32 = 1.0;
/// Caret width, logical points.
const CARET_PT: f32 = 1.5;

/// Cell face.
const FACE: [u8; 4] = [245, 246, 248, 255];
/// Cell border.
const BORDER: [u8; 4] = [200, 203, 210, 255];
/// Active-cell border.
const ACTIVE_EDGE: [u8; 4] = [70, 110, 200, 255];
/// Digit ink.
const INK: [u8; 4] = [30, 31, 36, 255];
/// Caret.
const CARET: [u8; 4] = [70, 110, 200, 255];

/// A segmented one-time-code input.
///
/// # Examples
///
/// ```
/// use martensite::widgets::otp_input::OtpInput;
///
/// let o = OtpInput::new().length(4).masked(true);
/// ```
pub struct OtpInput {
    /// Number of cells.
    length: usize,
    /// Entered characters (≤ `length`).
    value: String,
    /// Caret position (char index, 0..=value.len()).
    caret: usize,
    /// Accept letters as well as digits.
    alphabetic: bool,
    /// Mask entered characters as `●`.
    masked: bool,
    /// Enabled flag.
    enabled: bool,
    /// Pending completion notification.
    completed: Option<String>,
    /// Pending edit flag.
    edited: bool,
    /// Completion already signaled for this fill (fires once per
    /// fill, re-arms after any edit shortens the value).
    signaled: bool,
    /// Per-cell hit rects from the last layout.
    cell_rects: Vec<Rect>,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl OtpInput {
    /// Creates a 6-cell input.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::otp_input::OtpInput;
    ///
    /// let o = OtpInput::new();
    /// assert_eq!(o.get_value(), "");
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            length: 6,
            value: String::new(),
            caret: 0,
            alphabetic: false,
            masked: false,
            enabled: true,
            completed: None,
            edited: false,
            signaled: false,
            cell_rects: Vec::new(),
            text_painter: None,
        }
    }

    /// Sets the cell count.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::otp_input::OtpInput;
    ///
    /// let o = OtpInput::new().length(8);
    /// ```
    #[must_use]
    pub fn length(mut self, length: usize) -> Self {
        self.length = length.max(1);
        self.truncate();
        self
    }

    /// Sets the initial value (filtered + truncated).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::otp_input::OtpInput;
    ///
    /// let o = OtpInput::new().value("12");
    /// assert_eq!(o.get_value(), "12");
    /// ```
    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = self.filter(&value.into());
        self.caret = self.value.chars().count();
        self
    }

    /// Accepts letters in addition to digits.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::otp_input::OtpInput;
    ///
    /// let o = OtpInput::new().alphabetic(true);
    /// ```
    #[must_use]
    pub fn alphabetic(mut self, alphabetic: bool) -> Self {
        self.alphabetic = alphabetic;
        self
    }

    /// Masks cells with `●` instead of showing digits (PIN mode).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::otp_input::OtpInput;
    ///
    /// let o = OtpInput::new().masked(true);
    /// ```
    #[must_use]
    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    /// Enables or disables the input.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::otp_input::OtpInput;
    ///
    /// let o = OtpInput::new().enabled(false);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The entered characters.
    #[inline]
    #[must_use]
    pub fn get_value(&self) -> &str {
        &self.value
    }

    /// Sets the value programmatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::otp_input::OtpInput;
    ///
    /// let mut o = OtpInput::new();
    /// o.set_value("42");
    /// assert_eq!(o.get_value(), "42");
    /// ```
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = self.filter(&value.into());
        self.caret = self.value.chars().count();
        self.signaled = false;
    }

    /// Drains the completion notification — the full code once all
    /// cells fill.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::otp_input::OtpInput;
    ///
    /// let mut o = OtpInput::new();
    /// assert_eq!(o.take_completed(), None);
    /// ```
    pub fn take_completed(&mut self) -> Option<String> {
        self.completed.take()
    }

    /// Drains the edited flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::otp_input::OtpInput;
    ///
    /// let mut o = OtpInput::new();
    /// assert!(!o.take_edited());
    /// ```
    pub fn take_edited(&mut self) -> bool {
        std::mem::take(&mut self.edited)
    }

    /// Installs a shared shaped-text painter.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Filters a string to acceptable characters, truncated.
    fn filter(&self, text: &str) -> String {
        text.chars()
            .filter(|c| c.is_ascii_digit() || (self.alphabetic && c.is_ascii_alphabetic()))
            .take(self.length)
            .collect()
    }

    /// Truncates the value after a `length` shrink.
    fn truncate(&mut self) {
        let filtered: String = self.value.chars().take(self.length).collect();
        self.value = filtered;
        self.caret = self.caret.min(self.value.chars().count());
    }

    /// Inserts text at the caret, filtering + truncating; returns
    /// whether anything changed.
    fn insert(&mut self, text: &str) -> bool {
        let clean = self.filter(text);
        if clean.is_empty() {
            return false;
        }
        let chars: Vec<char> = self.value.chars().collect();
        let mut new: String = chars[..self.caret].iter().collect();
        new.push_str(&clean);
        new.extend(chars[self.caret..].iter());
        let new: String = new.chars().take(self.length).collect();
        if new == self.value {
            return false;
        }
        let advanced = clean.chars().count();
        self.value = new;
        self.caret = (self.caret + advanced).min(self.value.chars().count());
        self.after_edit();
        true
    }

    /// Deletes the char before the caret; at value end it removes the
    /// last cell (standard OTP backspace semantics).
    fn backspace(&mut self) -> bool {
        if self.caret == 0 {
            return false;
        }
        let mut chars: Vec<char> = self.value.chars().collect();
        if self.caret <= chars.len() {
            chars.remove(self.caret - 1);
            self.caret -= 1;
            self.value = chars.into_iter().collect();
            self.after_edit();
            return true;
        }
        false
    }

    /// Post-edit bookkeeping: edited flag + completion signal.
    fn after_edit(&mut self) {
        self.edited = true;
        if self.value.chars().count() < self.length {
            self.signaled = false;
        }
        if self.value.chars().count() == self.length && !self.signaled {
            self.signaled = true;
            self.completed = Some(self.value.clone());
        }
    }
}

impl Default for OtpInput {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for OtpInput {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let w = self.length as f32 * cx.pt(CELL_PT)
            + self.length.saturating_sub(1) as f32 * cx.pt(GAP_PT);
        Vec2::new(w.min(constraints.max_size.x.max(0.0)), cx.pt(CELL_PT))
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cell_rects.clear();
        let cell = cx.pt(CELL_PT);
        let gap = cx.pt(GAP_PT);
        for i in 0..self.length {
            self.cell_rects.push(Rect::new(
                bounds.origin.x + i as f32 * (cell + gap),
                bounds.origin.y,
                cell,
                cell,
            ));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::TextInput);
        node.set_label("One-time code");
        node.set_value(self.value.clone());
        if !self.enabled {
            node.set_disabled();
        }
        if self.enabled {
            node.add_action(accesskit::Action::SetValue);
            node.add_action(accesskit::Action::Focus);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::ImeCommitted { text } => {
                if self.insert(text) {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Handled
                }
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self.cell_rects.iter().position(|r| r.contains(*position)) {
                    self.caret = i.min(self.value.chars().count());
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "Backspace" => {
                    if self.backspace() {
                        EventResponse::RequestRepaint
                    } else {
                        EventResponse::Handled
                    }
                }
                "Delete" => {
                    let mut chars: Vec<char> = self.value.chars().collect();
                    if self.caret < chars.len() {
                        chars.remove(self.caret);
                        self.value = chars.into_iter().collect();
                        self.after_edit();
                        EventResponse::RequestRepaint
                    } else {
                        EventResponse::Handled
                    }
                }
                "ArrowLeft" => {
                    self.caret = self.caret.saturating_sub(1);
                    EventResponse::RequestRepaint
                }
                "ArrowRight" => {
                    self.caret = (self.caret + 1).min(self.value.chars().count());
                    EventResponse::RequestRepaint
                }
                "Home" => {
                    self.caret = 0;
                    EventResponse::RequestRepaint
                }
                "End" => {
                    self.caret = self.value.chars().count();
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let face = cx.color(TokenKey::SurfaceColor, FACE);
        let border = cx.color(TokenKey::BorderColor, BORDER);
        let active = cx.color(TokenKey::AccentColor, ACTIVE_EDGE);
        let ink = cx.color(TokenKey::TextColor, INK);
        let chars: Vec<char> = self.value.chars().collect();
        let size = cx.pt(FONT_PT);
        let shape = martensite_core::shape::Shape::rounded(cx.pt(RADIUS_PT));

        for (i, r) in self.cell_rects.iter().enumerate() {
            let kr = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            cx.list.push_fill_shape(kr, &shape, face);
            cx.list.push_stroke_shape(
                kr,
                &shape,
                cx.pt(BORDER_PT),
                if i == self.caret { active } else { border },
            );
            if let Some(c) = chars.get(i) {
                let glyph = if self.masked {
                    "●".to_string()
                } else {
                    c.to_string()
                };
                let w = painter
                    .and_then(|p| p.measure_text(&glyph, size))
                    .unwrap_or(size * 0.6);
                let x = r.origin.x + (r.size.x - w) / 2.0;
                let y = r.origin.y + (r.size.y - size) / 2.0;
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    kr,
                    kurbo::Point::new(f64::from(x), f64::from(y)),
                    &glyph,
                    size,
                    ink,
                );
            } else if i == self.caret {
                // Caret in the empty active cell.
                let cxr = r.origin.x + r.size.x / 2.0 - cx.pt(CARET_PT) / 2.0;
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(cxr),
                        f64::from(r.origin.y + cx.pt(8.0)),
                        f64::from(cxr + cx.pt(CARET_PT)),
                        f64::from(r.max_y() - cx.pt(8.0)),
                    ),
                    cx.color(TokenKey::AccentColor, CARET),
                );
            }
        }
    }
}

impl std::fmt::Debug for OtpInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OtpInput")
            .field("length", &self.length)
            .field("filled", &self.value.chars().count())
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
            bounds: Rect::new(0.0, 0.0, 300.0, 32.0),
            scale: 1.0,
        }
    }

    fn commit(text: &str) -> WidgetEvent {
        WidgetEvent::ImeCommitted { text: text.into() }
    }

    fn key(name: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: name.into(),
            repeat: false,
        }
    }

    #[test]
    fn digits_fill_and_complete() {
        let mut o = OtpInput::new().length(4);
        o.event(&mut ev(&commit("12")));
        assert_eq!(o.get_value(), "12");
        assert_eq!(o.take_completed(), None);
        o.event(&mut ev(&commit("34")));
        assert_eq!(o.take_completed(), Some("1234".into()));
    }

    #[test]
    fn paste_distributes() {
        let mut o = OtpInput::new().length(6);
        o.event(&mut ev(&commit("1-2-3-4-5-6"))); // separators filtered
        assert_eq!(o.get_value(), "123456");
    }

    #[test]
    fn letters_rejected_unless_alphabetic() {
        let mut o = OtpInput::new().length(4);
        o.event(&mut ev(&commit("ab12")));
        assert_eq!(o.get_value(), "12");
        let mut a = OtpInput::new().length(4).alphabetic(true);
        a.event(&mut ev(&commit("ab12")));
        assert_eq!(a.get_value(), "ab12");
    }

    #[test]
    fn backspace_retreats() {
        let mut o = OtpInput::new().length(4).value("123");
        o.event(&mut ev(&key("Backspace")));
        assert_eq!(o.get_value(), "12");
        assert!(o.take_edited());
    }

    #[test]
    fn completion_rearms_after_edit() {
        let mut o = OtpInput::new().length(2).value("12");
        o.event(&mut ev(&key("Backspace")));
        assert_eq!(o.take_completed(), None);
        o.event(&mut ev(&commit("9")));
        assert_eq!(o.take_completed(), Some("19".into()));
    }

    #[test]
    fn click_positions_caret() {
        let mut o = OtpInput::new().length(4).value("12");
        let mut hot = HotNode::default();
        o.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 300.0, 32.0));
        // Click cell 2 → caret parks at value end (2) since cells
        // beyond the value clamp to len.
        let r = o.cell_rects[0];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(r.origin.x + 4.0, r.origin.y + 4.0),
            button: PointerButton::Primary,
            count: 1,
        };
        o.event(&mut ev(&press));
        assert_eq!(o.caret, 0);
    }

    #[test]
    fn arrows_move_caret() {
        let mut o = OtpInput::new().length(4).value("123");
        o.event(&mut ev(&key("ArrowLeft")));
        assert_eq!(o.caret, 2);
        o.event(&mut ev(&key("Home")));
        assert_eq!(o.caret, 0);
        o.event(&mut ev(&key("End")));
        assert_eq!(o.caret, 3);
    }

    #[test]
    fn disabled_inert() {
        let mut o = OtpInput::new().enabled(false);
        assert_eq!(o.event(&mut ev(&commit("1"))), EventResponse::Ignored);
        assert_eq!(o.get_value(), "");
    }
}
