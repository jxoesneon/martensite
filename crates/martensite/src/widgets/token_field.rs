//! `TokenField` widget: a text input that commits entries into
//! removable token chips (AppKit `NSTokenField`, Windows Community
//! Toolkit `TokenizingTextBox`, Ant `Select tags` mode).
//!
//! Typing a delimiter (`,` `;` or `Enter`) commits the pending text
//! as a token; `Backspace` on an empty input deletes the last token;
//! clicking a chip's `×` removes it. The widget composes a
//! [`TextInput`] child for the in-progress entry — the child's
//! `take_edited` drives token commit decisions. Poll
//! [`TokenField::take_edited`] and read [`TokenField::tokens`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::token_field::TokenField;
//!
//! let t = TokenField::new().tokens(["rust", "gui"]);
//! assert_eq!(t.token_list(), &["rust", "gui"]);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

use crate::widgets::text_input::TextInput;

/// Chip height, logical points.
const CHIP_PT: f32 = 20.0;
/// Chip horizontal padding, logical points.
const CHIP_PAD_PT: f32 = 8.0;
/// Chip corner radius, logical points.
const CHIP_RADIUS_PT: f32 = 10.0;
/// Gap between chips, logical points.
const CHIP_GAP_PT: f32 = 4.0;
/// Field height, logical points.
const HEIGHT_PT: f32 = 32.0;
/// Field padding, logical points.
const FIELD_PAD_PT: f32 = 4.0;
/// Chip label font size, logical points.
const CHIP_FONT_PT: f32 = 11.0;
/// The `×` affordance width, logical points.
const REMOVE_W_PT: f32 = 14.0;
/// Minimum input width, logical points.
const MIN_INPUT_PT: f32 = 60.0;

/// Chip face.
const CHIP_FACE: [u8; 4] = [222, 225, 231, 255];
/// Chip ink.
const CHIP_INK: [u8; 4] = [30, 31, 36, 255];
/// Remove affordance ink.
const REMOVE_INK: [u8; 4] = [110, 114, 123, 255];
/// Hovered chip face.
const CHIP_HOVER: [u8; 4] = [205, 210, 220, 255];

/// A chip-ized token entry field.
///
/// # Examples
///
/// ```
/// use martensite::widgets::token_field::TokenField;
///
/// let t = TokenField::new().placeholder("Add tags…");
/// ```
pub struct TokenField {
    /// Committed tokens.
    tokens: Vec<String>,
    /// The pending-entry input.
    input: TextInput,
    /// Delimiter characters that commit a token.
    delimiters: Vec<char>,
    /// Hovered chip index.
    highlighted: Option<usize>,
    /// Edited flag.
    edited: bool,
    /// Pending removal notification (index + text).
    removed: Option<String>,
    /// Pending addition notification.
    added: Option<String>,
    /// Per-chip rects — estimated in `layout`, refined in `paint`
    /// (which has the real text shaper). Mutex because
    /// `Widget::paint` is `&self`.
    chip_rects: parking_lot::Mutex<Vec<Rect>>,
    /// Cached bounds.
    bounds: Rect,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl TokenField {
    /// Creates an empty field.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::token_field::TokenField;
    ///
    /// let t = TokenField::new();
    /// assert!(t.token_list().is_empty());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            tokens: Vec::new(),
            input: TextInput::new("Add token"),
            delimiters: vec![',', ';'],
            highlighted: None,
            edited: false,
            removed: None,
            added: None,
            chip_rects: parking_lot::Mutex::new(Vec::new()),
            bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets initial tokens.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::token_field::TokenField;
    ///
    /// let t = TokenField::new().tokens(["a", "b"]);
    /// assert_eq!(t.token_list().len(), 2);
    /// ```
    #[must_use]
    pub fn tokens(mut self, tokens: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.tokens = tokens.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the input placeholder.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::token_field::TokenField;
    ///
    /// let t = TokenField::new().placeholder("Type + Enter");
    /// ```
    #[must_use]
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.input = self.input.placeholder(text);
        self
    }

    /// Sets the commit delimiters (default `,` and `;`; `Enter`
    /// always commits).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::token_field::TokenField;
    ///
    /// let t = TokenField::new().delimiters(vec![' ']);
    /// ```
    #[must_use]
    pub fn delimiters(mut self, delimiters: Vec<char>) -> Self {
        self.delimiters = delimiters;
        self
    }

    /// Enables or disables the field.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::token_field::TokenField;
    ///
    /// let t = TokenField::new().enabled(false);
    /// ```
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.input = self.input.enabled(enabled);
        self
    }

    /// The committed tokens.
    #[inline]
    #[must_use]
    pub fn token_list(&self) -> &[String] {
        &self.tokens
    }

    /// Appends a token programmatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::token_field::TokenField;
    ///
    /// let mut t = TokenField::new();
    /// t.add_token("x");
    /// assert_eq!(t.token_list(), &["x"]);
    /// ```
    pub fn add_token(&mut self, token: impl Into<String>) {
        let token = token.into().trim().to_string();
        if !token.is_empty() {
            self.added = Some(token.clone());
            self.tokens.push(token);
            self.edited = true;
        }
    }

    /// Removes a token by index.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::token_field::TokenField;
    ///
    /// let mut t = TokenField::new().tokens(["a", "b"]);
    /// t.remove_token(0);
    /// assert_eq!(t.token_list(), &["b"]);
    /// ```
    pub fn remove_token(&mut self, index: usize) {
        if index < self.tokens.len() {
            self.removed = Some(self.tokens.remove(index));
            self.edited = true;
        }
    }

    /// Drains the edited flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::token_field::TokenField;
    ///
    /// let mut t = TokenField::new();
    /// assert!(!t.take_edited());
    /// ```
    pub fn take_edited(&mut self) -> bool {
        std::mem::take(&mut self.edited)
    }

    /// Drains the last removed token.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::token_field::TokenField;
    ///
    /// let mut t = TokenField::new();
    /// assert_eq!(t.take_removed(), None);
    /// ```
    pub fn take_removed(&mut self) -> Option<String> {
        self.removed.take()
    }

    /// Drains the last added token.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::token_field::TokenField;
    ///
    /// let mut t = TokenField::new();
    /// assert_eq!(t.take_added(), None);
    /// ```
    pub fn take_added(&mut self) -> Option<String> {
        self.added.take()
    }

    /// Accesses the inner input (e.g. to drain its own signals).
    #[inline]
    #[must_use]
    pub fn input(&self) -> &TextInput {
        &self.input
    }

    /// Accesses the inner input mutably.
    #[inline]
    #[must_use]
    pub fn input_mut(&mut self) -> &mut TextInput {
        &mut self.input
    }

    /// Installs a shared shaped-text painter on the field and input.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.input = self.input.with_text_painter(painter.clone());
        self.text_painter = Some(painter);
        self
    }

    /// Commits the pending input text as a token (no-op when empty).
    fn commit_pending(&mut self) -> bool {
        let text = self.input.value.trim().to_string();
        if text.is_empty() {
            return false;
        }
        self.input.set_value("");
        self.add_token(text);
        true
    }

    /// Computes chip rects with whatever painter is available —
    /// `layout` estimates (own painter or char counts), `paint`
    /// refines with the resolved ambient shaper.
    fn chip_layout(
        &self,
        painter: Option<&(dyn martensite_core::paint::TextShaper + Send + Sync)>,
        scale: f32,
    ) -> Vec<Rect> {
        let pad = FIELD_PAD_PT * scale;
        let chip_h = CHIP_PT * scale;
        let font = CHIP_FONT_PT * scale;
        let chip_pad = CHIP_PAD_PT * scale;
        let remove_w = REMOVE_W_PT * scale;
        let gap = CHIP_GAP_PT * scale;
        let mut x = self.bounds.origin.x + pad;
        let y = self.bounds.origin.y + (self.bounds.size.y - chip_h) / 2.0;
        self.tokens
            .iter()
            .map(|token| {
                let label_w = painter
                    .and_then(|p| p.measure_text(token, font))
                    .unwrap_or(font * token.chars().count() as f32 * 0.55);
                let w = label_w + chip_pad * 2.0 + remove_w;
                let r = Rect::new(x, y, w, chip_h);
                x += w + gap;
                r
            })
            .collect()
    }

    /// The `×` zone of chip `i` — the trailing square.
    fn remove_zone(&self, i: usize, scale: f32) -> Option<Rect> {
        let rects = self.chip_rects.lock();
        let r = *rects.get(i)?;
        let w = scale * REMOVE_W_PT;
        Some(Rect::new(r.max_x() - w, r.origin.y, w, r.size.y))
    }
}

impl Default for TokenField {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for TokenField {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(200.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        *self.chip_rects.lock() = self.chip_layout(
            self.text_painter
                .as_ref()
                .map(|p| p as &(dyn martensite_core::paint::TextShaper + Send + Sync)),
            cx.scale,
        );
        // The input claims the remainder after the chips.
        let input_x = self
            .chip_rects
            .lock()
            .last()
            .map(|r| r.max_x() + cx.pt(CHIP_GAP_PT))
            .unwrap_or(bounds.origin.x + cx.pt(FIELD_PAD_PT));
        let input_w = (bounds.max_x() - cx.pt(FIELD_PAD_PT) - input_x).max(cx.pt(MIN_INPUT_PT));
        cx.layout_child(
            &mut self.input,
            Rect::new(input_x, bounds.origin.y, input_w, bounds.size.y),
        );
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!("Tokens, {} entered", self.tokens.len()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let hit = self
                    .chip_rects
                    .lock()
                    .iter()
                    .position(|r| r.contains(*position));
                if hit != self.highlighted {
                    self.highlighted = hit;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                self.highlighted = None;
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                // Chip `×` zones consume the press before the input.
                let n = self.chip_rects.lock().len();
                for i in 0..n {
                    if self
                        .remove_zone(i, cx.scale)
                        .is_some_and(|z| z.contains(*position))
                    {
                        self.remove_token(i);
                        return EventResponse::RequestRepaint;
                    }
                }
                EventResponse::Ignored // falls through to input child
            }
            WidgetEvent::KeyPressed { key, .. } => {
                if key == "Backspace" && self.input.value.is_empty() && !self.tokens.is_empty() {
                    self.remove_token(self.tokens.len() - 1);
                    return EventResponse::RequestRepaint;
                }
                if key == "Enter" && self.commit_pending() {
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored // falls through to input child
            }
            WidgetEvent::ImeCommitted { text } => {
                // A trailing delimiter commits the pending text as a
                // token; the input never sees the delimiter.
                let ends = text
                    .chars()
                    .last()
                    .is_some_and(|c| self.delimiters.contains(&c));
                if ends {
                    let mut stripped = text.clone();
                    stripped.pop();
                    let combined = format!("{}{}", self.input.value, stripped);
                    self.input.set_value(combined);
                    self.commit_pending();
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        // Refine chip rects with the real shaper — paint owns the
        // plan hit-testing and `child_bounds` read.
        {
            let rects = self.chip_layout(painter, cx.scale);
            *self.chip_rects.lock() = rects;
        }
        let font = cx.pt(CHIP_FONT_PT);
        let pad = cx.pt(CHIP_PAD_PT);
        let remove_w = cx.pt(REMOVE_W_PT);
        let rects = self.chip_rects.lock();
        for (i, token) in self.tokens.iter().enumerate() {
            let r = rects[i];
            let kr = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            let shape = martensite_core::shape::Shape::rounded(cx.pt(CHIP_RADIUS_PT));
            let face = if self.highlighted == Some(i) {
                CHIP_HOVER
            } else {
                cx.color(TokenKey::DividerColor, CHIP_FACE)
            };
            cx.list.push_fill_shape(kr, &shape, face);
            // Label.
            let lx = r.origin.x + pad;
            let ly = r.origin.y + (r.size.y - font) / 2.0;
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(r.min_x()),
                    f64::from(r.min_y()),
                    f64::from(r.max_x() - remove_w),
                    f64::from(r.max_y()),
                ),
                kurbo::Point::new(f64::from(lx), f64::from(ly)),
                token,
                font,
                cx.color(TokenKey::TextColor, CHIP_INK),
            );
            // `×` affordance.
            let rx = r.max_x() - remove_w + cx.pt(2.0);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(r.max_x() - remove_w),
                    f64::from(r.min_y()),
                    f64::from(r.max_x()),
                    f64::from(r.max_y()),
                ),
                kurbo::Point::new(f64::from(rx), f64::from(ly)),
                "×",
                font,
                cx.color(TokenKey::TextMutedColor, REMOVE_INK),
            );
        }
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
        if index != 0 {
            return None;
        }
        let rects = self.chip_rects.lock();
        let x = rects
            .last()
            .map(|r| r.max_x() + CHIP_GAP_PT)
            .unwrap_or(self.bounds.origin.x + FIELD_PAD_PT);
        Some(Rect::new(
            x,
            self.bounds.origin.y,
            (self.bounds.max_x() - x).max(0.0),
            self.bounds.size.y,
        ))
    }
}

impl std::fmt::Debug for TokenField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenField")
            .field("tokens", &self.tokens)
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

    fn key(name: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: name.into(),
            repeat: false,
        }
    }

    #[test]
    fn add_remove_tokens() {
        let mut t = TokenField::new();
        t.add_token("rust");
        t.add_token("gui");
        assert_eq!(t.token_list(), &["rust", "gui"]);
        assert_eq!(t.take_added(), Some("gui".into()));
        t.remove_token(0);
        assert_eq!(t.token_list(), &["gui"]);
        assert_eq!(t.take_removed(), Some("rust".into()));
        assert!(t.take_edited());
    }

    #[test]
    fn add_token_ignores_blank() {
        let mut t = TokenField::new();
        t.add_token("   ");
        assert!(t.token_list().is_empty());
    }

    #[test]
    fn backspace_empty_pops_last() {
        let mut t = TokenField::new().tokens(["a", "b"]);
        let mut hot = HotNode::default();
        t.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 300.0, 32.0));
        assert_eq!(
            t.event(&mut ev(&key("Backspace"))),
            EventResponse::RequestRepaint
        );
        assert_eq!(t.token_list(), &["a"]);
    }

    #[test]
    fn backspace_with_text_ignored() {
        let mut t = TokenField::new().tokens(["a"]);
        t.input.set_value("xy");
        // The widget returns Ignored so the input child can handle it.
        assert_eq!(t.event(&mut ev(&key("Backspace"))), EventResponse::Ignored);
        assert_eq!(t.token_list(), &["a"]);
    }

    #[test]
    fn enter_commits_pending() {
        let mut t = TokenField::new();
        t.input.set_value("tag1");
        assert_eq!(
            t.event(&mut ev(&key("Enter"))),
            EventResponse::RequestRepaint
        );
        assert_eq!(t.token_list(), &["tag1"]);
        assert_eq!(t.input.value, "");
    }

    #[test]
    fn delimiter_commit() {
        let mut t = TokenField::new();
        let ev_commit = WidgetEvent::ImeCommitted {
            text: "alpha,".into(),
        };
        assert_eq!(t.event(&mut ev(&ev_commit)), EventResponse::RequestRepaint);
        assert_eq!(t.token_list(), &["alpha"]);
    }

    #[test]
    fn chips_layout_and_remove_zone() {
        let mut t = TokenField::new().tokens(["one", "two"]);
        let mut hot = HotNode::default();
        t.layout(&mut make_cx(&mut hot), Rect::new(0.0, 0.0, 300.0, 32.0));
        assert_eq!(t.chip_rects.lock().len(), 2);
        let zone = t.remove_zone(0, 1.0).unwrap();
        assert!(zone.max_x() <= t.chip_rects.lock()[0].max_x());
        // Press in the remove zone pops the chip.
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(zone.origin.x + 2.0, zone.origin.y + 2.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(t.event(&mut ev(&press)), EventResponse::RequestRepaint);
        assert_eq!(t.token_list(), &["two"]);
    }

    #[test]
    fn child_protocol_exposes_input() {
        let t = TokenField::new();
        assert_eq!(Widget::child_count(&t), 1);
        assert!(Widget::child(&t, 0).is_some());
    }
}
