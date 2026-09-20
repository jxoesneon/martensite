//! `ChatInput` — a message composer: draft field + send button,
//! with optional attach and emoji affordances (Slack/iMessage
//! composer idiom).
//!
//! Enter commits the draft to [`ChatInput::take_sent`] and clears
//! it; `Escape` clears without sending; the send button (or the
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
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const FONT_PT: f32 = 13.0;
const PAD_V_PT: f32 = 9.0;
const PAD_H_PT: f32 = 12.0;
const SEND_W_PT: f32 = 64.0;
const AUX_W_PT: f32 = 30.0;
const BTN_GAP_PT: f32 = 6.0;
const RADIUS_PT: f32 = 8.0;

const FACE: [u8; 4] = [36, 38, 44, 255];
const SEND: [u8; 4] = [88, 130, 247, 255];
const SEND_DIM: [u8; 4] = [70, 72, 80, 255];
const TEXT: [u8; 4] = [220, 222, 228, 255];
const MUTED: [u8; 4] = [139, 148, 158, 255];
const EDGE: [u8; 4] = [70, 72, 80, 255];

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
    draft: String,
    sent: Option<String>,
    attach: bool,
    emoji: bool,
    held_send: bool,
    send_rect: Rect,
    attach_rect: Rect,
    emoji_rect: Rect,
    focused: bool,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for ChatInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatInput")
            .field("draft", &self.draft)
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
            draft: String::new(),
            sent: None,
            attach: false,
            emoji: false,
            held_send: false,
            send_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            attach_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            emoji_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            focused: false,
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
        self
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
        &self.draft
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
        self.draft.push_str(text);
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
        self.draft = draft.into();
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
        self.draft.clear();
    }

    /// Whether the draft has sendable content.
    ///
    /// ```
    /// use martensite::widgets::chat_input::ChatInput;
    ///
    /// assert!(!ChatInput::new().can_send());
    /// ```
    pub fn can_send(&self) -> bool {
        self.enabled && !self.draft.trim().is_empty()
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

    /// Commits the draft if non-empty.
    fn submit(&mut self) {
        if self.can_send() {
            self.sent = Some(std::mem::take(&mut self.draft));
        }
    }
}

impl Widget for ChatInput {
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
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::TextInput);
        node.set_label(self.label.clone());
        node.set_value(self.draft.clone());
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
            WidgetEvent::ImeCommitted { text } => {
                self.draft.push_str(text);
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "Backspace" => {
                    if self.draft.pop().is_some() {
                        return EventResponse::RequestRepaint;
                    }
                    EventResponse::Ignored
                }
                "Enter" => {
                    self.submit();
                    EventResponse::RequestRepaint
                }
                "Escape" => {
                    if !self.draft.is_empty() {
                        self.draft.clear();
                        return EventResponse::RequestRepaint;
                    }
                    EventResponse::Ignored
                }
                k if k.chars().count() == 1 => {
                    self.draft.push_str(k);
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if !self.bounds.contains(*position) {
                    if self.focused {
                        self.focused = false;
                        return EventResponse::RequestRepaint;
                    }
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
                self.focused = true;
                EventResponse::CaptureFocus
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
        let shape = martensite_core::shape::Shape::rounded(RADIUS_PT * s);
        // Input face.
        let field = Rect::new(
            self.bounds.min_x(),
            self.bounds.min_y(),
            self.send_rect.min_x() - self.bounds.min_x() - BTN_GAP_PT * s,
            self.bounds.height(),
        );
        let face = cx.color(TokenKey::SurfaceColor, FACE);
        let edge = cx.color(TokenKey::DividerColor, EDGE);
        cx.list.push_fill_shape(krect(field), &shape, face);
        cx.list
            .push_stroke_shape(krect(field), &shape, 1.0 * s, edge);
        // Draft or placeholder.
        let shown: &str = if self.draft.is_empty() {
            &self.placeholder
        } else {
            &self.draft
        };
        let color = if self.draft.is_empty() {
            cx.color(TokenKey::TextMutedColor, MUTED)
        } else {
            cx.color(TokenKey::TextColor, TEXT)
        };
        let size = FONT_PT * s;
        let y = self.bounds.min_y() + self.bounds.height() / 2.0;
        let origin = kurbo::Point::new(f64::from(self.bounds.min_x() + PAD_H_PT * s), f64::from(y));
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            krect(field),
            origin,
            shown,
            size,
            color,
        );
        // Caret when focused.
        if self.focused && !self.draft.is_empty() {
            let x = self.bounds.min_x()
                + PAD_H_PT * s
                + painter
                    .and_then(|p| p.measure_text(&self.draft, size))
                    .unwrap_or(0.0)
                + 2.0 * s;
            let caret = kurbo::Rect::new(
                f64::from(x),
                f64::from(y - size * 0.7),
                f64::from(x + s),
                f64::from(y + size * 0.7),
            );
            cx.list.push_fill_rect(caret, color);
        }
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
            cx.color(TokenKey::TextColor, TEXT),
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
}
