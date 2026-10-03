//! `OtpInput` widget: a segmented one-time-code / PIN field (Ant
//! `Input.OTP`, `OTPField` conventions, `WCT`-style PIN boxes).
//!
//! Renders `length` adjacent cells; typed digits fill left-to-right
//! and the caret auto-advances, Backspace retreats, pasted text
//! distributes across cells (separators are filtered unless
//! `alphabetic` is set). Retyping a filled cell overwrites it rather
//! than shifting its neighbours — the `input-otp` slot model. Poll
//! [`OtpInput::take_completed`] — it fires each time all cells hold a
//! fresh full code.
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
use martensite_sanitize::{Phase, Sanitize, SanitizeContext, SanitizerConfig};
use std::sync::Arc;

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
    /// Keyboard focus — set by a claimed press or `FocusGained`,
    /// cleared by `FocusLost`. Editing keys are gated on it.
    focused: bool,
    /// Pending completion notification.
    completed: Option<String>,
    /// Pending edit flag.
    edited: bool,
    /// The code the completion last signaled — `None` re-arms it.
    /// Re-arms when the value shortens, and re-fires when a full
    /// value changes (a retyped digit is a fresh code worth
    /// re-signalling).
    signaled_value: Option<String>,
    /// Per-cell hit rects from the last layout.
    cell_rects: Vec<Rect>,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
    /// The sanitization pipeline applied to every ingestion before
    /// the digit/letter filter — NFKC folds fullwidth digits into
    /// ASCII digits that then validate.
    sanitizer: SanitizerConfig,
    /// Accessible label override — unset falls back to the
    /// built-in `"One-time code"` chrome string so the host app
    /// can localize it.
    pub a11y_label: Option<String>,
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
            focused: false,
            completed: None,
            edited: false,
            signaled_value: None,
            cell_rects: Vec::new(),
            text_painter: None,
            sanitizer: SanitizerConfig::default(),
            a11y_label: None,
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

    /// Toggles input sanitization — `true` (the default) runs the
    /// aggressive [`martensite_sanitize`] profile before the
    /// digit/letter filter (fullwidth digits fold to ASCII and
    /// validate); `false` keeps only the structural
    /// control-character floor; [`raw`](Self::raw) disables the
    /// engine entirely.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::otp_input::OtpInput;
    ///
    /// let mut o = OtpInput::new().length(4);
    /// o.set_value("\u{ff11}\u{ff12}"); // fullwidth "12"
    /// assert_eq!(o.get_value(), "12");
    /// ```
    #[inline]
    #[must_use]
    pub fn sanitize(mut self, on: bool) -> Self {
        self.sanitizer = if on {
            SanitizerConfig::Aggressive
        } else {
            SanitizerConfig::Baseline
        };
        self
    }

    /// Fully verbatim input — nothing is removed, normalized, or
    /// rewritten before the digit/letter filter.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::otp_input::OtpInput;
    ///
    /// let mut o = OtpInput::new().length(4).raw();
    /// o.set_value("\u{ff11}\u{ff12}");
    /// assert_eq!(o.get_value(), ""); // fullwidth stays non-ASCII
    /// ```
    #[inline]
    #[must_use]
    pub fn raw(mut self) -> Self {
        self.sanitizer = SanitizerConfig::Raw;
        self
    }

    /// Replaces the sanitization pipeline with a caller-supplied
    /// [`Sanitize`] rule.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::otp_input::OtpInput;
    /// use martensite_sanitize::Profile;
    /// use std::sync::Arc;
    ///
    /// let o = OtpInput::new().with_sanitizer(Arc::new(Profile::baseline()));
    /// assert!(o.sanitizer_config().is_custom());
    /// ```
    #[inline]
    #[must_use]
    pub fn with_sanitizer(mut self, rule: Arc<dyn Sanitize>) -> Self {
        self.sanitizer = SanitizerConfig::Custom(rule);
        self
    }

    /// Replaces the sanitization configuration in place.
    #[inline]
    pub fn set_sanitizer(&mut self, config: SanitizerConfig) {
        self.sanitizer = config;
    }

    /// The configured sanitization pipeline.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::otp_input::OtpInput;
    ///
    /// assert!(OtpInput::new().raw().sanitizer_config().is_raw());
    /// ```
    #[inline]
    pub fn sanitizer_config(&self) -> &SanitizerConfig {
        &self.sanitizer
    }

    /// Accept letters as well as digits.
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
        self.signaled_value = None;
    }

    /// Drains the completion notification — the full code once all
    /// cells fill, and again when an edit changes a still-full code.
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

    /// Filters a string to acceptable characters, truncated — the
    /// configured [`SanitizerConfig`] runs first, so e.g. fullwidth
    /// digits fold to ASCII before the digit check.
    fn filter(&self, text: &str) -> String {
        let clean = self
            .sanitizer
            .sanitize(text, &SanitizeContext::single_line(Phase::Insert));
        clean
            .chars()
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
    /// whether anything changed. Characters landing on filled cells
    /// overwrite them — retyping a cell replaces its digit instead of
    /// shifting neighbours off the end (`input-otp` semantics). When
    /// the caret sits past the last cell of a full field, typing folds
    /// back onto the final cell.
    fn insert(&mut self, text: &str) -> bool {
        let clean: Vec<char> = self.filter(text).chars().collect();
        if clean.is_empty() {
            return false;
        }
        let mut chars: Vec<char> = self.value.chars().collect();
        let pos = self.caret.min(self.length.saturating_sub(1));
        let end = (pos + clean.len()).min(chars.len());
        chars.splice(pos..end, clean.iter().copied());
        chars.truncate(self.length);
        let new: String = chars.into_iter().collect();
        if new == self.value {
            return false;
        }
        self.value = new;
        self.caret = (pos + clean.len()).min(self.value.chars().count());
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

    /// Post-edit bookkeeping: edited flag + completion signal. The
    /// signal fires on every fresh full code — shortening the value
    /// re-arms it, and overwriting a digit in a still-full field
    /// re-fires with the new code.
    fn after_edit(&mut self) {
        self.edited = true;
        let full = self.value.chars().count() == self.length;
        if !full {
            self.signaled_value = None;
        }
        if full && self.signaled_value.as_deref() != Some(self.value.as_str()) {
            self.signaled_value = Some(self.value.clone());
            self.completed = Some(self.value.clone());
        }
    }
}

impl Default for OtpInput {
    fn default() -> Self {
        Self::new()
    }
}

impl OtpInput {
    /// Sets the accessible label announced by assistive tech
    /// (default `"One-time code"`). Host apps localize the chrome string
    /// through this override.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::otp_input::OtpInput;
    ///
    /// let w = OtpInput::new().length(6).a11y_label("Custom name");
    /// assert_eq!(w.a11y_label.as_deref(), Some("Custom name"));
    /// ```
    #[must_use]
    pub fn a11y_label(mut self, label: impl Into<String>) -> Self {
        self.a11y_label = Some(label.into());
        self
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
        // Under RTL the digit cells run right-to-left.
        let rtl = cx.is_rtl();
        for i in 0..self.length {
            let slot = if rtl { self.length - 1 - i } else { i };
            self.cell_rects.push(Rect::new(
                bounds.origin.x + slot as f32 * (cell + gap),
                bounds.origin.y,
                cell,
                cell,
            ));
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::TextInput);
        node.set_label(self.a11y_label.as_deref().unwrap_or("One-time code"));
        node.set_value(self.value.clone());
        if !self.enabled {
            node.set_disabled();
        }
        if self.enabled {
            node.add_action(accesskit::Action::SetValue);
            node.add_action(accesskit::Action::Focus);
        }
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                EventResponse::RequestRepaint
            }
            // Editing events belong to the focused widget only.
            WidgetEvent::ImeCommitted { .. } | WidgetEvent::KeyPressed { .. } if !self.focused => {
                EventResponse::Ignored
            }
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
                    self.focused = true;
                    self.caret = i.min(self.value.chars().count());
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => {
                let (word, _, base) = crate::widgets::text_input::parse_key_chord(key);
                match base {
                    "Paste" | "v" | "V" if word || base == "Paste" => {
                        // Clipboard paste — one shot fills the cells, the
                        // same path `ImeCommitted` paste lands on.
                        let cb = martensite_clipboard::default_platform_clipboard();
                        if let Some(bytes) =
                            cb.get_contents(martensite_clipboard::clipboard::MIME_TEXT_PLAIN)
                        {
                            let text = String::from_utf8_lossy(&bytes);
                            if self.insert(&text) {
                                return EventResponse::RequestRepaint;
                            }
                        }
                        EventResponse::Handled
                    }
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
                        // Under RTL the cell strip mirrors — left moves
                        // the caret forward through the value.
                        self.caret = if cx.is_rtl() {
                            (self.caret + 1).min(self.value.chars().count())
                        } else {
                            self.caret.saturating_sub(1)
                        };
                        EventResponse::RequestRepaint
                    }
                    "ArrowRight" => {
                        self.caret = if cx.is_rtl() {
                            self.caret.saturating_sub(1)
                        } else {
                            (self.caret + 1).min(self.value.chars().count())
                        };
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
                }
            }
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
        let shape = martensite_core::shape::Shape::squircle(cx.pt(RADIUS_PT));

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
        o.event(&mut ev(&WidgetEvent::FocusGained));
        o.event(&mut ev(&commit("12")));
        assert_eq!(o.get_value(), "12");
        assert_eq!(o.take_completed(), None);
        o.event(&mut ev(&commit("34")));
        assert_eq!(o.take_completed(), Some("1234".into()));
    }

    #[test]
    fn paste_distributes() {
        let mut o = OtpInput::new().length(6);
        o.event(&mut ev(&WidgetEvent::FocusGained));
        o.event(&mut ev(&commit("1-2-3-4-5-6"))); // separators filtered
        assert_eq!(o.get_value(), "123456");
    }

    #[test]
    fn letters_rejected_unless_alphabetic() {
        let mut o = OtpInput::new().length(4);
        o.event(&mut ev(&WidgetEvent::FocusGained));
        o.event(&mut ev(&commit("ab12")));
        assert_eq!(o.get_value(), "12");
        let mut a = OtpInput::new().length(4).alphabetic(true);
        a.event(&mut ev(&WidgetEvent::FocusGained));
        a.event(&mut ev(&commit("ab12")));
        assert_eq!(a.get_value(), "ab12");
    }

    #[test]
    fn backspace_retreats() {
        let mut o = OtpInput::new().length(4).value("123");
        o.event(&mut ev(&WidgetEvent::FocusGained));
        o.event(&mut ev(&key("Backspace")));
        assert_eq!(o.get_value(), "12");
        assert!(o.take_edited());
    }

    #[test]
    fn completion_rearms_after_edit() {
        let mut o = OtpInput::new().length(2).value("12");
        o.event(&mut ev(&WidgetEvent::FocusGained));
        o.event(&mut ev(&key("Backspace")));
        assert_eq!(o.take_completed(), None);
        o.event(&mut ev(&commit("9")));
        assert_eq!(o.take_completed(), Some("19".into()));
    }

    #[test]
    fn click_positions_caret() {
        let mut o = OtpInput::new().length(4).value("12");
        o.event(&mut ev(&WidgetEvent::FocusGained));
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
        o.event(&mut ev(&WidgetEvent::FocusGained));
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
        o.event(&mut ev(&WidgetEvent::FocusGained));
        assert_eq!(o.event(&mut ev(&commit("1"))), EventResponse::Ignored);
        assert_eq!(o.get_value(), "");
    }

    #[test]
    fn editing_requires_focus() {
        let mut o = OtpInput::new().length(4);
        // No FocusGained — keys and committed text are dead input.
        assert_eq!(o.event(&mut ev(&commit("12"))), EventResponse::Ignored);
        assert_eq!(o.event(&mut ev(&key("Backspace"))), EventResponse::Ignored);
        assert_eq!(o.get_value(), "");
        // FocusLost also drops the gate.
        o.event(&mut ev(&WidgetEvent::FocusGained));
        o.event(&mut ev(&WidgetEvent::FocusLost));
        assert_eq!(o.event(&mut ev(&commit("1"))), EventResponse::Ignored);
        assert_eq!(o.get_value(), "");
    }

    #[test]
    fn paste_over_length_truncates_and_completes() {
        let mut o = OtpInput::new().length(4);
        o.event(&mut ev(&WidgetEvent::FocusGained));
        o.event(&mut ev(&commit("1234567")));
        assert_eq!(o.get_value(), "1234");
        assert_eq!(o.take_completed(), Some("1234".into()));
    }

    #[test]
    fn paste_strips_spaces_and_separators() {
        let mut o = OtpInput::new().length(4);
        o.event(&mut ev(&WidgetEvent::FocusGained));
        // Codes arrive formatted: "12 34", "12-34", "1-2 3_4".
        o.event(&mut ev(&commit(" 1-2 3_4 ")));
        assert_eq!(o.get_value(), "1234");
    }

    #[test]
    fn retype_overwrites_cell_not_shifts() {
        let mut o = OtpInput::new().length(4).value("1234");
        o.event(&mut ev(&WidgetEvent::FocusGained));
        o.event(&mut ev(&key("Home")));
        o.event(&mut ev(&key("ArrowRight"))); // caret on cell 1
        o.event(&mut ev(&commit("9")));
        assert_eq!(o.get_value(), "1934"); // cell 1 replaced, "34" kept
        assert_eq!(o.caret, 2);
    }

    #[test]
    fn mid_field_paste_overwrites_cells() {
        let mut o = OtpInput::new().length(4).value("1234");
        o.event(&mut ev(&WidgetEvent::FocusGained));
        o.event(&mut ev(&key("Home")));
        o.event(&mut ev(&key("ArrowRight")));
        o.event(&mut ev(&commit("98")));
        assert_eq!(o.get_value(), "1984"); // cells 1-2 replaced
    }

    #[test]
    fn typing_at_end_of_full_field_folds_to_last_cell() {
        let mut o = OtpInput::new().length(4).value("1234");
        o.event(&mut ev(&WidgetEvent::FocusGained));
        // Caret sits past the last cell after a fill — a keystroke
        // replaces the final digit rather than dead-ending.
        assert_eq!(o.caret, 4);
        o.event(&mut ev(&commit("5")));
        assert_eq!(o.get_value(), "1235");
    }

    #[test]
    fn overwrite_full_code_refires_completion() {
        let mut o = OtpInput::new().length(4).value("1234");
        o.event(&mut ev(&WidgetEvent::FocusGained));
        // First fill already signaled; a corrected digit is a new code.
        o.event(&mut ev(&key("Home")));
        o.event(&mut ev(&key("ArrowRight")));
        o.event(&mut ev(&commit("9")));
        assert_eq!(o.take_completed(), Some("1934".into()));
    }

    #[test]
    fn typing_same_digit_does_not_refire() {
        let mut o = OtpInput::new().length(2).value("12");
        o.event(&mut ev(&WidgetEvent::FocusGained));
        assert_eq!(o.take_completed(), None);
        // Caret folds to the last cell; retyping its digit is a no-op.
        o.event(&mut ev(&commit("2")));
        assert_eq!(o.get_value(), "12");
        assert_eq!(o.take_completed(), None);
    }

    #[test]
    fn delete_removes_char_at_caret() {
        let mut o = OtpInput::new().length(4).value("123");
        o.event(&mut ev(&WidgetEvent::FocusGained));
        o.event(&mut ev(&key("Home")));
        o.event(&mut ev(&key("ArrowRight")));
        o.event(&mut ev(&key("Delete")));
        assert_eq!(o.get_value(), "13");
        assert!(o.take_edited());
    }

    #[test]
    fn backspace_at_start_is_a_noop() {
        let mut o = OtpInput::new().length(4);
        o.event(&mut ev(&WidgetEvent::FocusGained));
        assert_eq!(o.event(&mut ev(&key("Backspace"))), EventResponse::Handled);
        assert_eq!(o.get_value(), "");
        // Backspace at caret 0 mid-value stays put too.
        let mut o = OtpInput::new().length(4).value("12");
        o.event(&mut ev(&WidgetEvent::FocusGained));
        o.event(&mut ev(&key("Home")));
        assert_eq!(o.event(&mut ev(&key("Backspace"))), EventResponse::Handled);
        assert_eq!(o.get_value(), "12");
        assert_eq!(o.caret, 0);
    }

    #[test]
    fn set_value_filters_and_truncates() {
        let mut o = OtpInput::new().length(4);
        o.set_value("9a876");
        assert_eq!(o.get_value(), "9876");
        assert_eq!(o.caret, 4);
    }

    #[test]
    fn length_floor_is_one() {
        let o = OtpInput::new().length(0);
        assert_eq!(o.length, 1);
    }

    #[test]
    fn click_past_value_clamps_caret_to_end() {
        let mut o = OtpInput::new().length(4).value("12");
        o.event(&mut ev(&WidgetEvent::FocusGained));
        let mut hot = HotNode::default();
        o.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 300.0, 32.0));
        // Click cell 3 — beyond the "12" fill — parks the caret at the
        // value end, matching first-empty-slot convention.
        let r = o.cell_rects[3];
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(r.origin.x + 4.0, r.origin.y + 4.0),
            button: PointerButton::Primary,
            count: 1,
        };
        o.event(&mut ev(&press));
        assert_eq!(o.caret, 2);
    }

    #[test]
    fn ctrl_v_paste_key_is_consumed() {
        let mut o = OtpInput::new().length(4);
        o.event(&mut ev(&WidgetEvent::FocusGained));
        // The chord reaches the clipboard path — stub/empty clipboard
        // is still claimed, never bubbled out as an unhandled key.
        let r = o.event(&mut ev(&key("Ctrl+V")));
        assert_ne!(r, EventResponse::Ignored);
    }
}
