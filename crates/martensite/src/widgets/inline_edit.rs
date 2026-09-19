//! `InlineEdit` — click-to-edit text (Ant `Typography.Paragraph
//! editable`, GWT `InlineLabel`).
//!
//! Displays a value as plain text; a press swaps in a real
//! [`TextInput`] child for in-place editing. `Enter` commits, `Escape`
//! reverts, and focus loss or a click outside commits — the platform
//! editable-label conventions. Completed commits park
//! `(previous, committed)` in [`InlineEdit::take_committed`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::inline_edit::InlineEdit;
//!
//! let mut edit = InlineEdit::new("Document title").placeholder("Untitled");
//! assert!(!edit.is_editing());
//! edit.begin_edit();
//! assert!(edit.is_editing());
//! edit.cancel_edit();
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, NodeFlags, PaintContext,
    PointerButton, Rect, RenderMinimum, SemanticAction, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;
use crate::widgets::text_input::TextInput;

const FONT_PT: f32 = 14.0;
/// Matches [`TextInput`]'s line height so the edit swap does not
/// reflow surrounding layout.
const LINE_PT: f32 = 24.0;
const HOVER_WASH: [u8; 4] = [128, 128, 128, 18];

/// Click-to-edit text — see the module docs.
///
/// ```
/// use martensite::widgets::inline_edit::InlineEdit;
///
/// let edit = InlineEdit::new("Hello");
/// assert_eq!(edit.value, "Hello");
/// ```
pub struct InlineEdit {
    /// The current display value. Direct mutation is safe — it only
    /// affects the display phase; an in-progress edit owns its own
    /// copy inside the input child.
    pub value: String,
    /// When `false` the field ignores input and cannot enter editing.
    pub enabled: bool,
    editing: bool,
    /// The value captured at [`Self::begin_edit`] — `Escape` semantics
    /// need it if the app ever wants a revert seam; today cancel just
    /// abandons the input's copy (the display value is untouched), so
    /// this is retained for future use and debugging clarity.
    edit_origin: String,
    input: TextInput,
    committed: Option<(String, String)>,
    placeholder: String,
    bounds: Rect,
    hovered: bool,
    text_painter: Option<SharedTextPainter>,
    scale: f32,
}

impl InlineEdit {
    /// Creates an inline edit displaying `value`.
    ///
    /// ```
    /// use martensite::widgets::inline_edit::InlineEdit;
    ///
    /// let edit = InlineEdit::new("Click me");
    /// assert_eq!(edit.value, "Click me");
    /// ```
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            enabled: true,
            editing: false,
            edit_origin: String::new(),
            input: TextInput::new("inline edit"),
            committed: None,
            placeholder: String::new(),
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            hovered: false,
            text_painter: None,
            scale: 1.0,
        }
    }

    /// Sets the muted text shown when the value is empty.
    ///
    /// ```
    /// use martensite::widgets::inline_edit::InlineEdit;
    ///
    /// let edit = InlineEdit::new("").placeholder("Click to add a title");
    /// assert_eq!(edit.value, "");
    /// ```
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self.input = self.input.placeholder(self.placeholder.clone());
        self
    }

    /// Enables or disables the field.
    ///
    /// ```
    /// use martensite::widgets::inline_edit::InlineEdit;
    ///
    /// let edit = InlineEdit::new("x").enabled(false);
    /// assert!(!edit.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self.input = self.input.enabled(enabled);
        self
    }

    /// Installs a shared shaped-text painter on the display and input.
    ///
    /// ```
    /// use martensite::widgets::inline_edit::InlineEdit;
    ///
    /// let edit = InlineEdit::new("x");
    /// let _ = edit.value;
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.input = self.input.with_text_painter(painter.clone());
        self.text_painter = Some(painter);
        self
    }

    /// Whether the field is currently in edit mode.
    ///
    /// ```
    /// use martensite::widgets::inline_edit::InlineEdit;
    ///
    /// let mut edit = InlineEdit::new("x");
    /// edit.begin_edit();
    /// assert!(edit.is_editing());
    /// ```
    pub fn is_editing(&self) -> bool {
        self.editing
    }

    /// The embedded input — live while editing. Useful for tests and
    /// for seeding the edit value programmatically.
    ///
    /// ```
    /// use martensite::widgets::inline_edit::InlineEdit;
    ///
    /// let mut edit = InlineEdit::new("seed");
    /// edit.begin_edit();
    /// edit.input_mut().set_value("edited");
    /// edit.commit_edit();
    /// assert_eq!(edit.value, "edited");
    /// ```
    pub fn input_mut(&mut self) -> &mut TextInput {
        &mut self.input
    }

    /// Enters edit mode: the input child adopts the current value.
    ///
    /// ```
    /// use martensite::widgets::inline_edit::InlineEdit;
    ///
    /// let mut edit = InlineEdit::new("before");
    /// edit.begin_edit();
    /// assert!(edit.is_editing());
    /// ```
    pub fn begin_edit(&mut self) {
        if self.editing || !self.enabled {
            return;
        }
        self.edit_origin = self.value.clone();
        self.input.set_value(self.value.clone());
        self.editing = true;
    }

    /// Commits the in-progress edit: the display value takes the
    /// input's text and `(previous, committed)` parks for
    /// [`take_committed`](Self::take_committed). No-op when not editing.
    ///
    /// ```
    /// use martensite::widgets::inline_edit::InlineEdit;
    ///
    /// let mut edit = InlineEdit::new("old");
    /// edit.begin_edit();
    /// edit.input_mut().set_value("new");
    /// edit.commit_edit();
    /// assert_eq!(edit.take_committed(), Some(("old".into(), "new".into())));
    /// ```
    pub fn commit_edit(&mut self) {
        if !self.editing {
            return;
        }
        let new = self.input.value.clone();
        self.editing = false;
        if new != self.value {
            self.committed = Some((std::mem::replace(&mut self.value, new), self.value.clone()));
        }
    }

    /// Abandons the in-progress edit — the display value is untouched.
    ///
    /// ```
    /// use martensite::widgets::inline_edit::InlineEdit;
    ///
    /// let mut edit = InlineEdit::new("keep");
    /// edit.begin_edit();
    /// edit.input_mut().set_value("discard");
    /// edit.cancel_edit();
    /// assert_eq!(edit.value, "keep");
    /// ```
    pub fn cancel_edit(&mut self) {
        self.editing = false;
    }

    /// Drains the parked `(previous, committed)` pair.
    ///
    /// ```
    /// use martensite::widgets::inline_edit::InlineEdit;
    ///
    /// let mut edit = InlineEdit::new("a");
    /// edit.begin_edit();
    /// edit.input_mut().set_value("b");
    /// edit.commit_edit();
    /// assert_eq!(edit.take_committed(), Some(("a".into(), "b".into())));
    /// assert_eq!(edit.take_committed(), None);
    /// ```
    pub fn take_committed(&mut self) -> Option<(String, String)> {
        self.committed.take()
    }

    /// Forwards an event into the input child while editing.
    fn forward_to_input(&mut self, cx: &mut EventContext) -> EventResponse {
        let mut child_cx = EventContext {
            event: cx.event,
            bounds: self.bounds,
            scale: cx.scale,
        };
        self.input.event(&mut child_cx)
    }
}

impl Widget for InlineEdit {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let _ = self.input.measure(cx, constraints);
        Vec2::new(
            cx.pt(40.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(LINE_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(40.0, LINE_PT)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
        if self.editing {
            cx.layout_child(&mut self.input, bounds);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::TextInput);
        node.set_label("inline edit");
        node.set_value(self.value.as_str());
        node.add_action(accesskit::Action::Focus);
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::SetValue);
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        if self.editing {
            match cx.event {
                // Commit/revert keys are the wrapper's concern, not
                // the input's — intercept before forwarding.
                WidgetEvent::KeyPressed { key, .. } if key == "Enter" => {
                    self.commit_edit();
                    return EventResponse::RequestRepaint;
                }
                WidgetEvent::KeyPressed { key, .. } if key == "Escape" => {
                    self.cancel_edit();
                    return EventResponse::RequestRepaint;
                }
                WidgetEvent::FocusLost => {
                    self.commit_edit();
                    return EventResponse::RequestRepaint;
                }
                // A press outside the (captured) field commits — the
                // Ant editable click-away convention.
                WidgetEvent::PointerPressed {
                    position,
                    button: PointerButton::Primary,
                    ..
                } if !self.bounds.contains(*position) => {
                    self.commit_edit();
                    return EventResponse::ReleasePointer;
                }
                _ => return self.forward_to_input(cx),
            }
        }
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let h = self.bounds.contains(*position);
                if h != self.hovered {
                    self.hovered = h;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerLeave => {
                self.hovered = false;
                EventResponse::RequestRepaint
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } if self.bounds.contains(*position) => {
                self.begin_edit();
                // Forward the same press so the input places its
                // caret under the pointer and captures tracking.
                let response = self.forward_to_input(cx);
                if response == EventResponse::Ignored {
                    EventResponse::CapturePointer
                } else {
                    response
                }
            }
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                self.begin_edit();
                EventResponse::CaptureFocus
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => EventResponse::CaptureFocus,
            WidgetEvent::SemanticAction(SemanticAction::SetValue(text)) => {
                let text = text.clone();
                if text != self.value {
                    self.committed =
                        Some((std::mem::replace(&mut self.value, text), self.value.clone()));
                }
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, .. } if key == "Enter" || key == "Space" => {
                self.begin_edit();
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        if self.editing {
            // The input child paints itself via traversal.
            return;
        }
        if self.hovered {
            cx.list.push_fill_rect(
                kurbo::Rect::new(
                    f64::from(self.bounds.min_x()),
                    f64::from(self.bounds.min_y()),
                    f64::from(self.bounds.max_x()),
                    f64::from(self.bounds.max_y()),
                ),
                cx.color(TokenKey::SecondaryColor, HOVER_WASH),
            );
        }
        let (text, ink) = if self.value.is_empty() {
            (
                self.placeholder.as_str(),
                cx.color(TokenKey::TextMutedColor, [120, 120, 128, 255]),
            )
        } else {
            (
                self.value.as_str(),
                cx.color(TokenKey::TextColor, [30, 30, 34, 255]),
            )
        };
        if text.is_empty() {
            return;
        }
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let font = cx.pt(FONT_PT);
        let ly = self.bounds.origin.y + (self.bounds.size.y - font) / 2.0;
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            kurbo::Rect::new(
                f64::from(self.bounds.min_x()),
                f64::from(self.bounds.min_y()),
                f64::from(self.bounds.max_x()),
                f64::from(self.bounds.max_y()),
            ),
            kurbo::Point::new(f64::from(self.bounds.min_x()), f64::from(ly)),
            text,
            font,
            ink,
        );
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
        if index == 0 && self.editing {
            Some(self.bounds)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for InlineEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InlineEdit")
            .field("value", &self.value)
            .field("editing", &self.editing)
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

    fn laid_out(edit: &mut InlineEdit, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        edit.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        edit.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 200.0, 24.0),
            scale: 1.0,
        }
    }

    fn press(x: f32, y: f32) -> WidgetEvent {
        WidgetEvent::PointerPressed {
            position: Vec2::new(x, y),
            button: PointerButton::Primary,
            count: 1,
        }
    }

    #[test]
    fn click_enters_editing_with_value() {
        let mut e = InlineEdit::new("hello");
        laid_out(&mut e, 200.0, 24.0);
        e.event(&mut ev(&press(10.0, 12.0)));
        assert!(e.is_editing());
        assert_eq!(e.input.value, "hello");
    }

    #[test]
    fn enter_commits_and_parks_pair() {
        let mut e = InlineEdit::new("old");
        laid_out(&mut e, 200.0, 24.0);
        e.event(&mut ev(&press(10.0, 12.0)));
        e.input.set_value("new");
        e.event(&mut ev(&WidgetEvent::KeyPressed {
            key: "Enter".into(),
            repeat: false,
        }));
        assert!(!e.is_editing());
        assert_eq!(e.value, "new");
        assert_eq!(e.take_committed(), Some(("old".into(), "new".into())));
        assert_eq!(e.take_committed(), None);
    }

    #[test]
    fn escape_reverts() {
        let mut e = InlineEdit::new("keep");
        laid_out(&mut e, 200.0, 24.0);
        e.event(&mut ev(&press(10.0, 12.0)));
        e.input.set_value("discard");
        e.event(&mut ev(&WidgetEvent::KeyPressed {
            key: "Escape".into(),
            repeat: false,
        }));
        assert!(!e.is_editing());
        assert_eq!(e.value, "keep");
        assert_eq!(e.take_committed(), None);
    }

    #[test]
    fn focus_lost_commits() {
        let mut e = InlineEdit::new("a");
        laid_out(&mut e, 200.0, 24.0);
        e.event(&mut ev(&press(10.0, 12.0)));
        e.input.set_value("b");
        e.event(&mut ev(&WidgetEvent::FocusLost));
        assert_eq!(e.value, "b");
    }

    #[test]
    fn outside_press_commits_and_releases() {
        let mut e = InlineEdit::new("a");
        laid_out(&mut e, 200.0, 24.0);
        e.event(&mut ev(&press(10.0, 12.0)));
        e.input.set_value("b");
        let resp = e.event(&mut ev(&press(10.0, 400.0)));
        assert_eq!(resp, EventResponse::ReleasePointer);
        assert_eq!(e.value, "b");
    }

    #[test]
    fn unchanged_edit_parks_nothing() {
        let mut e = InlineEdit::new("same");
        laid_out(&mut e, 200.0, 24.0);
        e.event(&mut ev(&press(10.0, 12.0)));
        e.event(&mut ev(&WidgetEvent::KeyPressed {
            key: "Enter".into(),
            repeat: false,
        }));
        assert_eq!(e.take_committed(), None);
    }

    #[test]
    fn semantic_set_value_commits() {
        let mut e = InlineEdit::new("a");
        laid_out(&mut e, 200.0, 24.0);
        e.event(&mut ev(&WidgetEvent::SemanticAction(
            SemanticAction::SetValue("b".into()),
        )));
        assert_eq!(e.value, "b");
        assert_eq!(e.take_committed(), Some(("a".into(), "b".into())));
    }

    #[test]
    fn semantic_click_enters_editing() {
        let mut e = InlineEdit::new("x");
        laid_out(&mut e, 200.0, 24.0);
        e.event(&mut ev(&WidgetEvent::SemanticAction(SemanticAction::Click)));
        assert!(e.is_editing());
    }

    #[test]
    fn disabled_ignores_press() {
        let mut e = InlineEdit::new("x").enabled(false);
        laid_out(&mut e, 200.0, 24.0);
        assert_eq!(e.event(&mut ev(&press(10.0, 12.0))), EventResponse::Ignored);
        assert!(!e.is_editing());
    }

    #[test]
    fn hover_wash_tracks_pointer() {
        let mut e = InlineEdit::new("x");
        laid_out(&mut e, 200.0, 24.0);
        e.event(&mut ev(&WidgetEvent::PointerMoved {
            position: Vec2::new(10.0, 12.0),
        }));
        assert!(e.hovered);
        e.event(&mut ev(&WidgetEvent::PointerLeave));
        assert!(!e.hovered);
    }
}
