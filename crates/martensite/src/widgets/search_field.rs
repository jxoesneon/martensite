//! `SearchField` widget: a dedicated search input — the NSSearchField /
//! Carbon `Search` equivalent.
//!
//! The widget embeds a [`TextInput`] internal child configured with a
//! magnifier [`prefix`](TextInput::prefix) and the
//! [`clearable`](TextInput::clearable) ✕ target, so caret, selection,
//! IME, and clipboard editing all come for free through the
//! `child_count`/`child_bounds` forwarding protocol. On top it adds:
//!
//! - `Role::SearchInput` accessibility — the inner field keeps its own
//!   `Role::TextInput` node as a virtual child;
//! - an `Enter` submit seam: [`SearchField::take_submitted`] yields
//!   the field text from the moment `Enter` was pressed;
//! - an optional caption [`label`](SearchField::label) painted above
//!   the field when the widget's bounds leave room for a caption
//!   strip (the same face/strip split `TextInput`'s
//!   `validation_message` uses).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::SearchField;
//!
//! let field = SearchField::new().placeholder("Search files…");
//! assert_eq!(field.value(), "");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, SemanticAction,
    Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, RenderMinimum, TokenKey, UnderflowPolicy};

use crate::widgets::text_input::TextInput;

/// One-line field height in logical pt — the `measure` floor and the
/// threshold above which a caption strip can fit.
const FIELD_PT: f32 = 24.0;
/// Caption strip height in logical pt.
const CAPTION_PT: f32 = 16.0;
/// Caption font size in logical pt.
const CAPTION_FONT_PT: f32 = 12.0;
/// Caption ink.
const CAPTION_INK: [u8; 4] = [110, 112, 120, 255];
/// The magnifier glyph painted muted ahead of the value — the search
/// field's platform-standard leading adornment.
const SEARCH_GLYPH: &str = "🔍";
/// Fallback accessible name when no [`SearchField::label`] is set.
const DEFAULT_LABEL: &str = "Search";

/// A dedicated search field: a magnifier-prefixed, clearable text
/// input with an `Enter` submit seam.
///
/// # Examples
///
/// ```
/// use martensite::widgets::SearchField;
///
/// let mut field = SearchField::new().with_value("rust");
/// assert_eq!(field.value(), "rust");
/// assert_eq!(field.take_submitted(), None); // no Enter yet
/// ```
pub struct SearchField {
    /// Optional caption painted above the field — also the widget's
    /// accessible label (defaults to `"Search"` when unset).
    pub label: Option<String>,
    /// Whether the field accepts input.
    pub enabled: bool,
    /// Placeholder text shown while the value is empty.
    pub placeholder: String,
    /// The embedded text field (internal child) — carries the
    /// magnifier prefix and the ✕ clear target.
    field: TextInput,
    /// Field bounds assigned in `layout` (below the caption strip).
    field_rect: Rect,
    /// `Enter` submit out-seam — see
    /// [`take_submitted`](Self::take_submitted).
    submitted: Option<String>,
    /// Shared shaped-text painter — caption text emits real
    /// `GlyphRun`s when set; propagated to the embedded field.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
}

impl SearchField {
    /// Creates a search field with an empty value, a `"Search…"`
    /// placeholder, the magnifier prefix, and the ✕ clear target.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SearchField;
    ///
    /// let field = SearchField::new();
    /// assert_eq!(field.value(), "");
    /// assert_eq!(field.placeholder, "Search…");
    /// ```
    pub fn new() -> Self {
        Self {
            label: None,
            enabled: true,
            placeholder: "Search…".to_string(),
            field: TextInput::new(DEFAULT_LABEL)
                .prefix(SEARCH_GLYPH)
                .clearable(true),
            field_rect: Rect::default(),
            submitted: None,
            text_painter: None,
            cached_bounds: Rect::default(),
        }
    }

    /// Sets the caption label painted above the field (when the
    /// widget's bounds leave a caption strip) and used as the
    /// accessible name.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SearchField;
    ///
    /// let field = SearchField::new().label("Find in page");
    /// assert_eq!(field.label.as_deref(), Some("Find in page"));
    /// ```
    #[inline]
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the placeholder text shown while the value is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SearchField;
    ///
    /// let field = SearchField::new().placeholder("Search docs…");
    /// assert_eq!(field.placeholder, "Search docs…");
    /// ```
    #[inline]
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets the current text value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SearchField;
    ///
    /// let field = SearchField::new().with_value("query");
    /// assert_eq!(field.value(), "query");
    /// ```
    #[inline]
    #[must_use]
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.set_value(value);
        self
    }

    /// Sets the current text value (mutable version for programmatic
    /// updates — does not set the [`take_edited`](Self::take_edited)
    /// flag, matching `TextInput::set_value`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SearchField;
    ///
    /// let mut field = SearchField::new();
    /// field.set_value("martensite");
    /// assert_eq!(field.value(), "martensite");
    /// ```
    #[inline]
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.field.set_value(value);
    }

    /// The current text value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SearchField;
    ///
    /// assert_eq!(SearchField::new().with_value("abc").value(), "abc");
    /// ```
    #[inline]
    pub fn value(&self) -> &str {
        &self.field.value
    }

    /// Sets whether the field is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SearchField;
    ///
    /// let field = SearchField::new().enabled(false);
    /// assert!(!field.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Shares a [`crate::text_paint::TextPainter`] so the caption and
    /// the embedded field emit real glyph runs instead of `DrawText`
    /// placeholder boxes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::text_paint::shared_painter;
    /// use martensite::widgets::SearchField;
    ///
    /// let field = SearchField::new().with_text_painter(shared_painter());
    /// ```
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.field = self.field.clone().with_text_painter(painter.clone());
        self.text_painter = Some(painter);
        self
    }

    /// The field text at the last `Enter` press, or `None` when
    /// `Enter` has not been pressed since the last call — the
    /// widget's submit out-seam (mirrors `Banner::take_dismissed`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SearchField;
    ///
    /// let mut field = SearchField::new().with_value("rust gui");
    /// assert_eq!(field.take_submitted(), None);
    /// ```
    #[inline]
    pub fn take_submitted(&mut self) -> Option<String> {
        self.submitted.take()
    }

    /// `true` when a user-driven edit mutated the value since the
    /// last call — forwards the embedded field's
    /// [`take_edited`](TextInput::take_edited) flag (typing, deletion,
    /// paste, cut, the ✕ clear, undo/redo; programmatic
    /// [`set_value`](Self::set_value) writes do not set it).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SearchField;
    ///
    /// let mut field = SearchField::new();
    /// assert!(!field.take_edited());
    /// ```
    #[inline]
    pub fn take_edited(&mut self) -> bool {
        self.field.take_edited()
    }

    /// Returns the cached bounds from the last layout pass.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::SearchField;
    /// use martensite_core::Rect;
    ///
    /// let field = SearchField::new();
    /// assert_eq!(field.cached_bounds(), Rect::default());
    /// ```
    #[inline]
    pub fn cached_bounds(&self) -> Rect {
        self.cached_bounds
    }

    /// The caption strip's height inside `b` in device px — `0`
    /// unless a `label` is set and the bounds leave a full
    /// `CAPTION_PT` above a one-line face. Exactly-one-line bounds
    /// show no caption (the same documented fit rule `TextInput`'s
    /// `validation_message` uses).
    fn caption_h(&self, b: Rect, scale: f32) -> f32 {
        let fits = b.size.y >= (FIELD_PT + CAPTION_PT) * scale;
        if self.label.is_some() && fits {
            CAPTION_PT * scale
        } else {
            0.0
        }
    }

    /// Mirrors widget state onto the embedded field.
    fn sync_field(&mut self) {
        self.field.enabled = self.enabled;
        self.field.label = self
            .label
            .clone()
            .unwrap_or_else(|| DEFAULT_LABEL.to_string());
        self.field.placeholder.clone_from(&self.placeholder);
    }

    /// Forwards an event to the embedded field the way the default
    /// `Widget::event` child walk would: positional events only inside
    /// the field's bounds, except moves and releases which always
    /// reach it so a captured drag keeps tracking outside.
    fn forward(&mut self, cx: &mut EventContext) -> EventResponse {
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
        self.field.event(&mut child_cx)
    }
}

impl Default for SearchField {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for SearchField {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // A search field has the embedded input's 120x24 logical pt
        // floor; a `label` asks for the caption strip's height on top
        // so the layout leaves room to show it.
        let mut size = self.field.measure(cx, constraints);
        if self.label.is_some() {
            size.y = (size.y + cx.pt(CAPTION_PT)).min(constraints.max_size.y.max(0.0));
        }
        size
    }

    fn min_render(&self) -> RenderMinimum {
        // The embedded field's floor — a shorter face cannot render
        // the text lane legibly.
        RenderMinimum::new(Vec2::new(120.0, FIELD_PT)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        // Declare keyboard focusability on the arena node so
        // press-to-focus applies; key/IME input then forwards into
        // the field child.
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
        self.sync_field();
        let caption_h = self.caption_h(bounds, cx.scale);
        self.field_rect = Rect::new(
            bounds.min_x(),
            bounds.min_y() + caption_h,
            bounds.width(),
            (bounds.height() - caption_h).max(0.0),
        );
        cx.layout_child(&mut self.field, self.field_rect);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::SearchInput);
        node.set_label(self.label.as_deref().unwrap_or(DEFAULT_LABEL));
        node.set_value(self.field.value.as_str());
        if self.enabled {
            node.add_action(accesskit::Action::Focus);
            node.add_action(accesskit::Action::SetValue);
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            // `Enter` is the submit seam — the embedded field ignores
            // the key itself, so intercepting here cannot steal an
            // editing gesture.
            WidgetEvent::KeyPressed { key, .. } if key.as_str() == "Enter" => {
                self.submitted = Some(self.field.value.clone());
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(SemanticAction::SetValue(text)) => {
                self.field.set_value(text.clone());
                EventResponse::RequestRepaint
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => EventResponse::CaptureFocus,
            _ => self.forward(cx),
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        // The embedded field paints its own face/border/text/caret via
        // the internal-child walk; the only chrome here is the caption
        // strip, clipped so a long label can't spill past the widget.
        let b = cx.bounds;
        let caption_h = self.caption_h(b, cx.scale);
        if caption_h <= 0.0 {
            return;
        }
        if let Some(ref label) = self.label {
            let font_px = cx.pt(CAPTION_FONT_PT);
            crate::text_paint::paint_label_clipped(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Rect::new(
                    f64::from(b.min_x()),
                    f64::from(b.min_y()),
                    f64::from(b.max_x()),
                    f64::from(b.min_y() + caption_h),
                ),
                kurbo::Point::new(
                    f64::from(b.min_x() + cx.pt(2.0)),
                    f64::from(b.min_y() + (caption_h - font_px) / 2.0),
                ),
                label,
                font_px,
                cx.color(TokenKey::TextMutedColor, CAPTION_INK),
            );
        }
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        (index == 0).then_some(&self.field as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        (index == 0).then_some(&mut self.field as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        (index == 0).then_some(self.field_rect)
    }
}

impl std::fmt::Debug for SearchField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchField")
            .field("label", &self.label)
            .field("value", &self.field.value)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(field: &mut SearchField, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        field.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn key(k: &str) -> WidgetEvent {
        WidgetEvent::KeyPressed {
            key: k.to_string(),
            repeat: false,
        }
    }

    fn event(field: &mut SearchField, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: field.cached_bounds,
            scale: 1.0,
        };
        field.event(&mut cx)
    }

    #[test]
    fn new_configures_search_chrome() {
        let field = SearchField::new();
        assert!(field.field.clearable);
        assert_eq!(field.field.prefix.as_deref(), Some(SEARCH_GLYPH));
        assert_eq!(field.placeholder, "Search…");
        assert_eq!(field.value(), "");
    }

    #[test]
    fn enter_submits_current_text() {
        let mut field = SearchField::new().with_value("widgets");
        laid_out(&mut field, 200.0, FIELD_PT);
        assert_eq!(
            event(&mut field, &key("Enter")),
            EventResponse::RequestRepaint
        );
        assert_eq!(field.take_submitted(), Some("widgets".to_string()));
        // One-shot seam — drained by the first take.
        assert_eq!(field.take_submitted(), None);
    }

    #[test]
    fn typing_sets_edited_flag() {
        let mut field = SearchField::new();
        laid_out(&mut field, 200.0, FIELD_PT);
        let ime = WidgetEvent::ImeCommitted {
            text: "rus".to_string(),
        };
        event(&mut field, &ime);
        assert_eq!(field.value(), "rus");
        assert!(field.take_edited());
        assert!(!field.take_edited());
    }

    #[test]
    fn clear_button_edit_propagates() {
        let mut field = SearchField::new().with_value("abc");
        laid_out(&mut field, 200.0, FIELD_PT);
        // Press the ✕ clear target at the field's right edge.
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(200.0 - 9.0, 12.0),
            button: martensite_core::PointerButton::Primary,
            count: 1,
        };
        event(&mut field, &press);
        assert_eq!(field.value(), "");
        assert!(field.take_edited());
    }

    #[test]
    fn accessibility_emits_search_role() {
        let field = SearchField::new().label("Find").with_value("q");
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        field.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::SearchInput);
        assert_eq!(node.label(), Some("Find"));
        assert_eq!(node.value(), Some("q"));
        assert!(node.supports_action(accesskit::Action::Focus));
        assert!(node.supports_action(accesskit::Action::SetValue));
    }

    #[test]
    fn inner_field_stays_child() {
        let mut field = SearchField::new();
        laid_out(&mut field, 200.0, FIELD_PT);
        assert_eq!(field.child_count(), 1);
        assert!(field.child(0).is_some());
        assert!(field.child_bounds(0).is_some());
    }

    #[test]
    fn caption_strip_splits_bounds() {
        let mut field = SearchField::new().label("Find in page");
        // Tall enough for caption + one-line face.
        laid_out(&mut field, 200.0, FIELD_PT + CAPTION_PT);
        assert_eq!(field.field_rect.min_y(), CAPTION_PT);
        assert_eq!(field.field_rect.height(), FIELD_PT);
        // Exactly one-line bounds: no caption strip.
        laid_out(&mut field, 200.0, FIELD_PT);
        assert_eq!(field.field_rect.min_y(), 0.0);
    }

    #[test]
    fn disabled_ignores_events() {
        let mut field = SearchField::new().enabled(false).with_value("x");
        laid_out(&mut field, 200.0, FIELD_PT);
        assert_eq!(event(&mut field, &key("Enter")), EventResponse::Ignored);
        assert_eq!(field.take_submitted(), None);
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        field.accessibility(&mut node);
        assert!(node.is_disabled());
    }

    #[test]
    fn semantic_set_value_writes_text() {
        let mut field = SearchField::new();
        laid_out(&mut field, 200.0, FIELD_PT);
        event(
            &mut field,
            &WidgetEvent::SemanticAction(SemanticAction::SetValue("typed".to_string())),
        );
        assert_eq!(field.value(), "typed");
    }
}
