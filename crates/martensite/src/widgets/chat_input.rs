//! `ChatInput` — a message composer: draft field + send button,
//! with optional attach and emoji affordances (Slack/iMessage
//! composer idiom).
//!
//! Enter commits the draft to [`ChatInput::take_sent`] and clears
//! it — `Shift+Enter`, the newline chord of multi-line composers,
//! is deliberately inert here because the draft is a single line.
//! `Escape` clears without sending; the send button (or the
//! attach/emoji buttons when enabled) park intents in
//! [`ChatInput::take_attach`]/[`ChatInput::take_emoji`]. Completes
//! the chat family with [`MessageList`](crate::widgets::MessageList)
//! and [`TypingIndicator`](crate::widgets::TypingIndicator).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::chat_input::ChatInput;
//!
//! let mut c = ChatInput::new();
//! c.insert("hi");
//! assert_eq!(c.draft(), "hi");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, SemanticAction, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_sanitize::{Phase, Sanitize, SanitizeContext, SanitizerConfig};
use martensite_theme::TokenKey;
use std::sync::Arc;

use crate::text_paint::SharedTextPainter;
use crate::widgets::text_input::{parse_key_chord, TextInput};

const FONT_PT: f32 = 13.0;
const PAD_V_PT: f32 = 9.0;
const PAD_H_PT: f32 = 12.0;
const SEND_W_PT: f32 = 64.0;
const AUX_W_PT: f32 = 30.0;
const BTN_GAP_PT: f32 = 6.0;
const RADIUS_PT: f32 = 8.0;

const SEND: [u8; 4] = [88, 130, 247, 255];
const SEND_DIM: [u8; 4] = [70, 72, 80, 255];
const MUTED: [u8; 4] = [139, 148, 158, 255];

/// A message composer — see the module docs.
///
/// ```
/// use martensite::widgets::chat_input::ChatInput;
///
/// assert_eq!(ChatInput::new().draft(), "");
/// ```
pub struct ChatInput {
    /// Accessibility label.
    pub label: String,
    /// Placeholder shown for an empty draft.
    pub placeholder: String,
    /// Show an attach (📎) button.
    pub attachable: bool,
    /// Show an emoji button.
    pub emoji_button: bool,
    /// Disabled state.
    pub enabled: bool,
    /// The draft field — a real [`TextInput`] internal child, so the
    /// composer gets the full editing surface (caret, selection,
    /// clipboard, word ops, undo/redo, IME) for free.
    input: TextInput,
    sent: Option<String>,
    /// The sanitization pipeline — propagated to the draft field for
    /// insert-time filtering and applied to the sent message with
    /// [`Phase::Commit`].
    sanitizer: SanitizerConfig,
    attach: bool,
    emoji: bool,
    held_send: bool,
    /// Shift state tracked from `KeyPressed`/`KeyReleased` —
    /// `WidgetEvent::KeyPressed` carries no modifier state (F17), so
    /// the wrapper tracks it to keep `Shift+Enter` from sending
    /// (the newline chord of multi-line composers; inert in this
    /// single-line field).
    shift_held: bool,
    send_rect: Rect,
    attach_rect: Rect,
    emoji_rect: Rect,
    field_rect: Rect,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for ChatInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatInput")
            .field("draft", &self.draft())
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Default for ChatInput {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatInput {
    /// Empty composer.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// assert_eq!(ChatInput::new().draft(), "");
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Message".to_string(),
            placeholder: "Message…".to_string(),
            attachable: false,
            emoji_button: false,
            enabled: true,
            input: TextInput::new("Message"),
            sent: None,
            sanitizer: SanitizerConfig::default(),
            attach: false,
            emoji: false,
            held_send: false,
            shift_held: false,
            send_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            attach_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            emoji_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            field_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// assert_eq!(ChatInput::new().label("Reply").label, "Reply");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Placeholder builder.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// assert_eq!(ChatInput::new().placeholder("Say hi").placeholder, "Say hi");
    /// ```
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Attach-button builder.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// assert!(ChatInput::new().attachable(true).attachable);
    /// ```
    pub fn attachable(mut self, attachable: bool) -> Self {
        self.attachable = attachable;
        self
    }

    /// Emoji-button builder.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// assert!(ChatInput::new().emoji_button(true).emoji_button);
    /// ```
    pub fn emoji_button(mut self, show: bool) -> Self {
        self.emoji_button = show;
        self
    }

    /// Disabled builder.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// assert!(!ChatInput::new().enabled(false).enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self.input.enabled = enabled;
        self
    }

    /// Toggles input sanitization — `true` (the default) runs the
    /// aggressive [`martensite_sanitize`] profile on every ingestion
    /// and on the sent draft; `false` keeps only the structural
    /// control-character floor; [`raw`](Self::raw) disables the
    /// engine entirely.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// let c = ChatInput::new().sanitize(false);
    /// assert!(!c.sanitizer_config().is_raw());
    /// ```
    #[inline]
    #[must_use]
    pub fn sanitize(mut self, on: bool) -> Self {
        self.set_sanitizer(if on {
            SanitizerConfig::Aggressive
        } else {
            SanitizerConfig::Baseline
        });
        self
    }

    /// Fully verbatim input — nothing is removed, normalized, or
    /// rewritten.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// assert!(ChatInput::new().raw().sanitizer_config().is_raw());
    /// ```
    #[inline]
    #[must_use]
    pub fn raw(mut self) -> Self {
        self.set_sanitizer(SanitizerConfig::Raw);
        self
    }

    /// Replaces the sanitization pipeline with a caller-supplied
    /// [`Sanitize`] rule.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    /// use martensite_sanitize::Profile;
    /// use std::sync::Arc;
    ///
    /// let c = ChatInput::new().with_sanitizer(Arc::new(Profile::baseline()));
    /// assert!(c.sanitizer_config().is_custom());
    /// ```
    #[inline]
    #[must_use]
    pub fn with_sanitizer(mut self, rule: Arc<dyn Sanitize>) -> Self {
        self.set_sanitizer(SanitizerConfig::Custom(rule));
        self
    }

    /// Replaces the sanitization configuration, propagating it to the
    /// draft field.
    #[inline]
    pub fn set_sanitizer(&mut self, config: SanitizerConfig) {
        self.input.set_sanitizer(config.clone());
        self.sanitizer = config;
    }

    /// The configured sanitization pipeline.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// assert!(ChatInput::new().raw().sanitizer_config().is_raw());
    /// ```
    #[inline]
    pub fn sanitizer_config(&self) -> &SanitizerConfig {
        &self.sanitizer
    }

    /// Shared text painter for real glyph metrics.
    ///
    /// ```no_run
    /// use martensite::widgets::chat_input::ChatInput;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _c = ChatInput::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The current draft.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// assert_eq!(ChatInput::new().draft(), "");
    /// ```
    pub fn draft(&self) -> &str {
        &self.input.value
    }

    /// Appends text to the draft (paste insertion seam).
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// let mut c = ChatInput::new();
    /// c.insert("hello");
    /// assert_eq!(c.draft(), "hello");
    /// ```
    pub fn insert(&mut self, text: &str) {
        let combined = format!("{}{}", self.input.value, text);
        self.input.set_value(combined);
    }

    /// Replaces the draft (host-controlled editing, e.g. reply
    /// prefill).
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// let mut c = ChatInput::new();
    /// c.set_draft("re: hi");
    /// assert_eq!(c.draft(), "re: hi");
    /// ```
    pub fn set_draft(&mut self, draft: impl Into<String>) {
        self.input.set_value(draft);
    }

    /// Clears the draft.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// let mut c = ChatInput::new();
    /// c.insert("x");
    /// c.clear();
    /// assert_eq!(c.draft(), "");
    /// ```
    pub fn clear(&mut self) {
        self.input.set_value("");
    }

    /// Whether the draft has sendable content.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// assert!(!ChatInput::new().can_send());
    /// ```
    pub fn can_send(&self) -> bool {
        self.enabled && !self.input.value.trim().is_empty()
    }

    /// Drains the committed message (Enter / send click).
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// let mut c = ChatInput::new();
    /// assert_eq!(c.take_sent(), None);
    /// ```
    pub fn take_sent(&mut self) -> Option<String> {
        self.sent.take()
    }

    /// Drains an attach-button press.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// let mut c = ChatInput::new();
    /// assert!(!c.take_attach());
    /// ```
    pub fn take_attach(&mut self) -> bool {
        std::mem::take(&mut self.attach)
    }

    /// Drains an emoji-button press.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// let mut c = ChatInput::new();
    /// assert!(!c.take_emoji());
    /// ```
    pub fn take_emoji(&mut self) -> bool {
        std::mem::take(&mut self.emoji)
    }

    /// Commits the draft if non-empty. Surrounding whitespace is
    /// trimmed before parking — `can_send` already gates on the
    /// trimmed check, so the sent message matches what made the
    /// button active.
    fn submit(&mut self) {
        if self.can_send() {
            let draft = self
                .sanitizer
                .sanitize(
                    &self.input.value,
                    &SanitizeContext::single_line(Phase::Commit),
                )
                .trim()
                .to_string();
            self.input.set_value("");
            self.sent = Some(draft);
        }
    }

    /// Delivers an event to the draft field under its own bounds.
    /// Positional events outside the field are dropped; keys, IME and
    /// focus transitions pass through so the field's own focus gate
    /// applies.
    fn forward_input(&mut self, cx: &mut EventContext) -> EventResponse {
        if let Some(pos) = cx.event.position() {
            let drag = matches!(
                cx.event,
                WidgetEvent::PointerMoved { .. } | WidgetEvent::PointerReleased { .. }
            );
            if !drag && !self.field_rect.contains(pos) {
                return EventResponse::Ignored;
            }
        }
        let mut child_cx = EventContext {
            event: cx.event,
            bounds: self.field_rect,
            scale: cx.scale,
        };
        self.input.event(&mut child_cx)
    }
}

impl Widget for ChatInput {
    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = (FONT_PT + PAD_V_PT * 2.0 + 2.0) * cx.scale;
        Vec2::new(constraints.max_size.x.max(160.0 * cx.scale), h)
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 30.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let btn_h = bounds.height() - PAD_V_PT * s;
        let mut x = bounds.max_x() - PAD_H_PT * s;
        // Send button on the right.
        x -= SEND_W_PT * s;
        self.send_rect = Rect::new(x, bounds.min_y() + PAD_V_PT * s / 2.0, SEND_W_PT * s, btn_h);
        // Aux buttons left of send.
        if self.emoji_button {
            x -= AUX_W_PT * s + BTN_GAP_PT * s;
            self.emoji_rect =
                Rect::new(x, bounds.min_y() + PAD_V_PT * s / 2.0, AUX_W_PT * s, btn_h);
        }
        if self.attachable {
            x -= AUX_W_PT * s + BTN_GAP_PT * s;
            self.attach_rect =
                Rect::new(x, bounds.min_y() + PAD_V_PT * s / 2.0, AUX_W_PT * s, btn_h);
        }
        // The draft field takes the remainder.
        self.field_rect = Rect::new(
            bounds.min_x(),
            bounds.min_y(),
            (x - BTN_GAP_PT * s - bounds.min_x()).max(0.0),
            bounds.height(),
        );
        self.input.placeholder.clone_from(&self.placeholder);
        self.input.enabled = self.enabled;
        self.input.label.clone_from(&self.label);
        cx.layout_child(&mut self.input, self.field_rect);
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&self.input as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut self.input as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.field_rect)
    }

    fn focused(&self) -> bool {
        self.input.focused()
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::TextInput);
        node.set_label(self.label.clone());
        node.set_value(self.input.value.clone());
        if !self.enabled {
            node.set_disabled();
        }
        node.add_action(accesskit::Action::SetValue);
        node.add_action(accesskit::Action::Focus);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } => {
                if key == "Shift" {
                    self.shift_held = true;
                }
                // Composer chrome — only while the draft holds focus.
                if self.input.focused() {
                    let (_, shift_chord, base) = parse_key_chord(key);
                    if base == "Enter" {
                        // Enter sends; Shift+Enter is the newline
                        // chord of multi-line composers — this field
                        // is single-line, so it stays deliberately
                        // inert rather than becoming an accidental
                        // send.
                        if !shift_chord && !self.shift_held {
                            self.submit();
                        }
                        return EventResponse::RequestRepaint;
                    }
                    if key == "Escape" {
                        // Escape clears the draft without sending; an
                        // empty draft lets the field collapse any
                        // selection and return `Ignored`.
                        if !self.input.value.is_empty() {
                            self.input.set_value("");
                            return EventResponse::RequestRepaint;
                        }
                    }
                }
                self.forward_input(cx)
            }
            WidgetEvent::KeyReleased { key } => {
                if key == "Shift" {
                    self.shift_held = false;
                }
                self.forward_input(cx)
            }
            WidgetEvent::FocusLost => {
                // A swallowed release must not leave Shift latched —
                // the input clears its own modifier state the same way.
                self.shift_held = false;
                self.forward_input(cx)
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if !self.bounds.contains(*position) {
                    return EventResponse::Ignored;
                }
                if self.attachable && self.attach_rect.contains(*position) {
                    self.attach = true;
                    return EventResponse::Handled;
                }
                if self.emoji_button && self.emoji_rect.contains(*position) {
                    self.emoji = true;
                    return EventResponse::Handled;
                }
                if self.send_rect.contains(*position) {
                    self.held_send = true;
                    return EventResponse::CapturePointer;
                }
                self.forward_input(cx)
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if self.held_send {
                    self.held_send = false;
                    if self.send_rect.contains(*position) {
                        self.submit();
                    }
                    return EventResponse::ReleasePointer;
                }
                self.forward_input(cx)
            }
            WidgetEvent::SemanticAction(action) => match action {
                SemanticAction::SetValue(text) => {
                    self.input.set_value(text.clone());
                    EventResponse::RequestRepaint
                }
                SemanticAction::Focus => EventResponse::CaptureFocus,
                _ => self.forward_input(cx),
            },
            _ => self.forward_input(cx),
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
        let shape = martensite_core::shape::Shape::rounded(RADIUS_PT * s);
        // The draft field paints its own chrome (face, placeholder,
        // text, caret, selection) through the internal-child walk.
        let size = FONT_PT * s;
        // Aux buttons: 📎 / ☺ glyphs as labels.
        for (rect, glyph) in [(self.attach_rect, "📎"), (self.emoji_rect, "☺")] {
            if rect.width() > 0.0 {
                let g = kurbo::Point::new(
                    f64::from(rect.min_x() + rect.width() / 2.0 - size * 0.4),
                    f64::from(rect.min_y() + rect.height() / 2.0),
                );
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    g,
                    glyph,
                    size,
                    cx.color(TokenKey::TextMutedColor, MUTED),
                );
            }
        }
        // Send button.
        let send = if self.can_send() {
            cx.color(TokenKey::AccentColor, SEND)
        } else {
            cx.color(TokenKey::SecondaryColor, SEND_DIM)
        };
        cx.list.push_fill_shape(krect(self.send_rect), &shape, send);
        let st = kurbo::Point::new(
            f64::from(self.send_rect.min_x() + self.send_rect.width() / 2.0 - size * 0.9),
            f64::from(self.send_rect.min_y() + self.send_rect.height() / 2.0),
        );
        crate::text_paint::paint_label(
            painter,
            cx.list,
            st,
            "Send",
            size,
            // Inverse ink — the send face is a chromatic fill
            // (accent/secondary), not a neutral surface.
            cx.color(TokenKey::TextInverseColor, [22, 22, 22, 255]),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut ChatInput) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 400.0, 36.0));
    }

    fn ev(c: &mut ChatInput, e: &WidgetEvent) -> EventResponse {
        c.event(&mut EventContext {
            event: e,
            bounds: c.bounds,
            scale: 1.0,
        })
    }

    fn type_text(c: &mut ChatInput, s: &str) {
        ev(c, &WidgetEvent::FocusGained);
        ev(
            c,
            &WidgetEvent::ImeCommitted {
                text: s.to_string(),
            },
        );
    }

    #[test]
    fn typing_and_enter_sends() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        type_text(&mut c, "hello");
        assert_eq!(c.draft(), "hello");
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: "Enter".to_string(),
                repeat: false,
            },
        );
        assert_eq!(c.take_sent(), Some("hello".to_string()));
        assert_eq!(c.draft(), "");
    }

    #[test]
    fn empty_draft_does_not_send() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: "Enter".to_string(),
                repeat: false,
            },
        );
        assert_eq!(c.take_sent(), None);
    }

    #[test]
    fn escape_clears() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        type_text(&mut c, "draft");
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: "Escape".to_string(),
                repeat: false,
            },
        );
        assert_eq!(c.draft(), "");
    }

    #[test]
    fn backspace_edits() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        type_text(&mut c, "abc");
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: "Backspace".to_string(),
                repeat: false,
            },
        );
        assert_eq!(c.draft(), "ab");
    }

    #[test]
    fn send_button_submits() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        type_text(&mut c, "via button");
        let r = c.send_rect;
        let mid = Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0);
        ev(
            &mut c,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: mid,
                count: 1,
            },
        );
        ev(
            &mut c,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: mid,
            },
        );
        assert_eq!(c.take_sent(), Some("via button".to_string()));
    }

    #[test]
    fn attach_button_parks() {
        let mut c = ChatInput::new().attachable(true);
        laid_out(&mut c);
        let r = c.attach_rect;
        assert!(r.width() > 0.0);
        ev(
            &mut c,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
                count: 1,
            },
        );
        assert!(c.take_attach());
    }

    #[test]
    fn disabled_ignores_input() {
        let mut c = ChatInput::new().enabled(false);
        laid_out(&mut c);
        type_text(&mut c, "nope");
        assert_eq!(c.draft(), "");
    }

    #[test]
    fn paint_without_painter() {
        let mut c = ChatInput::new().attachable(true).emoji_button(true);
        laid_out(&mut c);
        c.insert("hi");
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        c.paint(&mut PaintContext {
            list: &mut list,
            bounds: c.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }

    fn key(name: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: name.into(),
            repeat: false,
        }
    }

    #[test]
    fn shift_enter_chord_does_not_send() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        type_text(&mut c, "draft");
        // The single-line composer has no newline — Shift+Enter must
        // not turn into an accidental send.
        ev(&mut c, &key("Shift+Enter"));
        assert_eq!(c.take_sent(), None);
        assert_eq!(c.draft(), "draft");
    }

    #[test]
    fn tracked_shift_modifier_blocks_send() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        type_text(&mut c, "draft");
        // Window layers may deliver Shift as its own key event rather
        // than a `+`-joined chord name — the suppression must hold
        // under both encodings.
        ev(&mut c, &key("Shift"));
        ev(&mut c, &key("Enter"));
        assert_eq!(c.take_sent(), None);
        assert_eq!(c.draft(), "draft");
        ev(
            &mut c,
            &WidgetEvent::KeyReleased {
                key: "Shift".into(),
            },
        );
        ev(&mut c, &key("Enter"));
        assert_eq!(c.take_sent(), Some("draft".to_string()));
    }

    #[test]
    fn whitespace_only_draft_does_not_send() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        type_text(&mut c, "   ");
        assert!(!c.can_send());
        ev(&mut c, &key("Enter"));
        assert_eq!(c.take_sent(), None);
        assert_eq!(c.draft(), "   ");
    }

    #[test]
    fn send_trims_surrounding_whitespace() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        type_text(&mut c, "  hi  ");
        ev(&mut c, &key("Enter"));
        assert_eq!(c.take_sent(), Some("hi".to_string()));
        assert_eq!(c.draft(), "");
    }

    #[test]
    fn multiline_commit_collapses_to_one_line() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        // The draft is single-line: committed/pasted newlines are
        // stripped by the embedded field.
        type_text(&mut c, "line one\nline two");
        assert_eq!(c.draft(), "line oneline two");
    }

    #[test]
    fn unfocused_enter_does_not_send() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        c.set_draft("draft");
        ev(&mut c, &key("Enter"));
        assert_eq!(c.take_sent(), None);
        assert_eq!(c.draft(), "draft");
    }

    #[test]
    fn unfocused_ime_does_not_edit() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        ev(
            &mut c,
            &WidgetEvent::ImeCommitted {
                text: "nope".into(),
            },
        );
        assert_eq!(c.draft(), "");
    }

    #[test]
    fn escape_on_empty_draft_passes_through() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        ev(&mut c, &WidgetEvent::FocusGained);
        // Empty draft: Escape reaches the field, which has nothing
        // to collapse — the event stays Ignored for ancestors.
        assert_eq!(ev(&mut c, &key("Escape")), EventResponse::Ignored);
    }

    #[test]
    fn focus_lost_drops_input_focus() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        type_text(&mut c, "draft");
        assert!(c.input.focused());
        ev(&mut c, &WidgetEvent::FocusLost);
        assert!(!c.input.focused());
        // And a stale latched modifier cannot suppress the next send.
        ev(&mut c, &WidgetEvent::FocusGained);
        ev(&mut c, &key("Enter"));
        assert_eq!(c.take_sent(), Some("draft".to_string()));
    }

    #[test]
    fn send_release_off_button_cancels() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        type_text(&mut c, "draft");
        let r = c.send_rect;
        let mid = Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0);
        ev(
            &mut c,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: mid,
                count: 1,
            },
        );
        // Drag-off cancels: releasing outside the send target sends nothing.
        ev(
            &mut c,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(5.0, 5.0),
            },
        );
        assert_eq!(c.take_sent(), None);
        assert_eq!(c.draft(), "draft");
    }

    #[test]
    fn emoji_button_parks() {
        let mut c = ChatInput::new().emoji_button(true);
        laid_out(&mut c);
        let r = c.emoji_rect;
        assert!(r.width() > 0.0);
        ev(
            &mut c,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
                count: 1,
            },
        );
        assert!(c.take_emoji());
    }

    #[test]
    fn send_button_keeps_input_focused() {
        let mut c = ChatInput::new();
        laid_out(&mut c);
        type_text(&mut c, "draft");
        let r = c.send_rect;
        let mid = Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0);
        ev(
            &mut c,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: mid,
                count: 1,
            },
        );
        ev(
            &mut c,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: mid,
            },
        );
        assert_eq!(c.take_sent(), Some("draft".to_string()));
        // The composer keeps editing context after a send.
        assert!(c.input.focused());
    }
}
