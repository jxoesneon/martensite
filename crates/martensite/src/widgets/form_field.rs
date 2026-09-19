//! `FormField` — labeled control row with a validation strip.
//!
//! The Ant `Form.Item` / UIKit form-cell pattern: a label, a control,
//! and a message strip that shows the validation error (accent-red)
//! or, when clean, an optional hint (muted). The strip only takes
//! vertical space when it has something to show, so a stack of
//! `FormField`s aligns cleanly inside a `Flex` column.
//!
//! `FormField` owns the control as its only internal child — pointer
//! and keyboard events forward to it through the standard child
//! protocol. Validation is deliberately dumb: the caller sets
//! [`FormField::set_error`] after whatever validation pass it runs;
//! the field just renders the outcome. `required` paints the
//! conventional `*` prefix on the label.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::TextInput;
//! use martensite::widgets::form_field::FormField;
//!
//! let mut f = FormField::new()
//!     .label("Hostname")
//!     .required(true)
//!     .hint("e.g. plc-east-07")
//!     .child(TextInput::new("x"));
//! f.set_error(Some("required".into()));
//! assert!(f.error().is_some());
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, Rect,
    RenderMinimum, UnderflowPolicy, Widget,
};
use martensite_theme::TokenKey;

/// Label ink.
const TEXT: TokenKey = TokenKey::TextColor;
/// Hint ink.
const MUTED: TokenKey = TokenKey::TextMutedColor;
/// Error ink.
const ERROR: TokenKey = TokenKey::ErrorColor;
/// Label strip height (logical points).
const LABEL_H: f32 = 20.0;
/// Message strip height (logical points).
const MESSAGE_H: f32 = 18.0;
/// Vertical gap between strips (logical points).
const GAP: f32 = 4.0;
/// Left-column label width in `LabelPosition::Left` (logical points).
const LEFT_LABEL_W: f32 = 120.0;

/// Where the label sits relative to the control.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelPosition {
    /// Label above the control (the default — best for narrow forms).
    #[default]
    Top,
    /// Label in a fixed-width column left of the control.
    Left,
}

/// A labeled control row with a validation strip — see the module
/// docs.
///
/// `FormField` reports exactly one internal child when a control is
/// installed (zero before).
///
/// # Examples
///
/// ```
/// use martensite::widgets::TextInput;
/// use martensite::widgets::form_field::FormField;
/// use martensite::core::Widget;
///
/// let mut f = FormField::new().child(TextInput::new("x"));
/// assert_eq!(f.child_count(), 1);
/// ```
pub struct FormField {
    label: String,
    required: bool,
    hint: String,
    error: Option<String>,
    position: LabelPosition,
    label_width: f32,
    enabled: bool,
    control: Option<Box<dyn Widget>>,
    /// Laid-out strips for `child_bounds` — the control rect, in the
    /// same space `layout` was called with.
    control_bounds: Option<Rect>,
    /// Cached label size (`pt`-scaled at paint time).
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl FormField {
    /// An unlabeled field with no control.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::form_field::FormField;
    ///
    /// let f = FormField::new();
    /// assert!(f.error().is_none());
    /// ```
    pub fn new() -> Self {
        Self {
            label: String::new(),
            required: false,
            hint: String::new(),
            error: None,
            position: LabelPosition::Top,
            label_width: LEFT_LABEL_W,
            enabled: true,
            control: None,
            control_bounds: None,
            text_painter: None,
        }
    }

    /// Set the label text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::form_field::FormField;
    ///
    /// let f = FormField::new().label("Hostname");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Mark the field required — paints a `*` prefix on the label.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::form_field::FormField;
    ///
    /// let f = FormField::new().required(true);
    /// ```
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// Set the muted hint shown under the control while no error is
    /// active.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::form_field::FormField;
    ///
    /// let f = FormField::new().hint("letters and dashes");
    /// ```
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    /// Install the control child.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::form_field::FormField;
    /// use martensite::widgets::CheckBox;
    ///
    /// let f = FormField::new().child(CheckBox::new("ok"));
    /// ```
    pub fn child(mut self, control: impl Widget) -> Self {
        self.control = Some(Box::new(control));
        self
    }

    /// Label placement — `Top` (default) or `Left`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::form_field::{FormField, LabelPosition};
    ///
    /// let f = FormField::new().label_position(LabelPosition::Left);
    /// ```
    pub fn label_position(mut self, position: LabelPosition) -> Self {
        self.position = position;
        self
    }

    /// Left-column label width in logical points (default 120) — only
    /// used under `LabelPosition::Left`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::form_field::FormField;
    ///
    /// let f = FormField::new().label_width(140.0);
    /// ```
    pub fn label_width(mut self, width: f32) -> Self {
        self.label_width = width;
        self
    }

    /// Enable or disable the field and its control (default `true`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::form_field::FormField;
    ///
    /// let f = FormField::new().enabled(false);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Share a text painter. `SharedTextPainter` is not `Default`, so
    /// this builder is exercised indirectly through `paint`.
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// The active validation error, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::form_field::FormField;
    ///
    /// assert!(FormField::new().error().is_none());
    /// ```
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Set or clear the validation error (post-validation hook).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::form_field::FormField;
    ///
    /// let mut f = FormField::new();
    /// f.set_error(Some("required".into()));
    /// f.set_error(None);
    /// assert!(f.error().is_none());
    /// ```
    pub fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }

    /// The message-strip text: the error when active, else the hint.
    fn message(&self) -> Option<&str> {
        self.error.as_deref().or(match self.hint.is_empty() {
            true => None,
            false => Some(self.hint.as_str()),
        })
    }

    /// The label text including the required marker.
    fn label_text(&self) -> String {
        if self.required {
            format!("* {}", self.label)
        } else {
            self.label.clone()
        }
    }
}

impl Default for FormField {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for FormField {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut h = GAP; // spacing above the control
        if !self.label.is_empty() && self.position == LabelPosition::Top {
            h += LABEL_H + GAP;
        }
        // The control contributes its own desired height.
        let mut control_h = 24.0;
        if let Some(c) = self.control.as_mut() {
            let want = c.measure(cx, constraints);
            control_h = want.y;
        }
        h += control_h;
        if self.message().is_some() {
            h += GAP + MESSAGE_H;
        }
        let w = if self.position == LabelPosition::Left {
            self.label_width + GAP + 160.0
        } else {
            200.0
        };
        Vec2::new(w, h)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        let gap = cx.pt(GAP);
        let label_h = cx.pt(LABEL_H);
        let msg_h = cx.pt(MESSAGE_H);
        let has_label = !self.label.is_empty();
        let message_h = if self.message().is_some() {
            msg_h + gap
        } else {
            0.0
        };

        let (label_rect, control_rect) = match self.position {
            LabelPosition::Top => {
                let label = if has_label {
                    Rect::new(bounds.min_x(), bounds.min_y(), bounds.width(), label_h)
                } else {
                    Rect::new(bounds.min_x(), bounds.min_y(), 0.0, 0.0)
                };
                let top = bounds.min_y() + if has_label { label_h + gap } else { 0.0 };
                let control_h = (bounds.max_y() - message_h - top).max(0.0);
                let control = Rect::new(bounds.min_x(), top, bounds.width(), control_h);
                (label, control)
            }
            LabelPosition::Left => {
                let lw = cx.pt(self.label_width);
                let label = Rect::new(bounds.min_x(), bounds.min_y(), lw, label_h);
                let cx0 = bounds.min_x() + lw + gap;
                let control_h = (bounds.max_y() - message_h - bounds.min_y()).max(0.0);
                let control = Rect::new(
                    cx0,
                    bounds.min_y(),
                    (bounds.max_x() - cx0).max(0.0),
                    control_h.min(label_h.max(control_h)),
                );
                (label, control)
            }
        };
        let _ = label_rect; // paint recomputes; only the control caches bounds
        self.control_bounds = Some(control_rect);
        if let Some(c) = self.control.as_mut() {
            cx.layout_child(c.as_mut(), control_rect);
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let ink = cx.color(TEXT, [30, 30, 35, 255]);
        let muted = cx.color(MUTED, [140, 140, 150, 255]);
        let error = cx.color(ERROR, [220, 60, 60, 255]);
        let label_size = 12.0 * cx.scale;

        // Label.
        if !self.label.is_empty() {
            let label_h = cx.pt(LABEL_H);
            let (clip, origin) = match self.position {
                LabelPosition::Top => {
                    let clip = kurbo::Rect::new(
                        f64::from(b.min_x()),
                        f64::from(b.min_y()),
                        f64::from(b.max_x()),
                        f64::from(b.min_y() + label_h),
                    );
                    (
                        clip,
                        kurbo::Point::new(clip.x0, clip.y0 + clip.height() * 0.72),
                    )
                }
                LabelPosition::Left => {
                    let lw = cx.pt(self.label_width);
                    let clip = kurbo::Rect::new(
                        f64::from(b.min_x()),
                        f64::from(b.min_y()),
                        f64::from(b.min_x() + lw),
                        f64::from(b.min_y() + label_h),
                    );
                    (
                        clip,
                        kurbo::Point::new(clip.x0, clip.y0 + clip.height() * 0.72),
                    )
                }
            };
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                origin,
                &self.label_text(),
                label_size,
                if self.enabled { ink } else { muted },
            );
        }

        // Message strip (error wins over hint, colored accordingly).
        if let Some(msg) = self.message() {
            let msg_h = cx.pt(MESSAGE_H);
            let clip = kurbo::Rect::new(
                f64::from(b.min_x()),
                f64::from(b.max_y() - msg_h),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            );
            let color = if self.error.is_some() { error } else { muted };
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(clip.x0, clip.y0 + clip.height() * 0.72),
                msg,
                11.0 * cx.scale,
                color,
            );
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        // The control is the only child — forward through the default
        // bounds-gated dispatch.
        if let (Some(cb), Some(c)) = (self.control_bounds, self.control.as_mut()) {
            if cx.event.position().is_none_or(|p| cb.contains(p)) {
                let mut child_cx = EventContext {
                    event: cx.event,
                    bounds: cb,
                    scale: cx.scale,
                };
                return c.event(&mut child_cx);
            }
        }
        EventResponse::Ignored
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        if !self.label.is_empty() {
            node.set_label(self.label.as_str());
        }
        if let Some(err) = self.error.as_deref() {
            node.set_description(err.to_string());
        }
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn child_count(&self) -> usize {
        usize::from(self.control.is_some())
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index == 0 {
            self.control.as_deref()
        } else {
            None
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index == 0 {
            self.control.as_deref_mut()
        } else {
            None
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index == 0 {
            self.control_bounds
        } else {
            None
        }
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(140.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::button::Button;
    use martensite_core::{HotNode, WidgetEvent};

    fn lay(w: &mut FormField) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.layout(&mut cx, Rect::new(0.0, 0.0, 320.0, 90.0));
    }

    fn ev(w: &mut FormField, e: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: e,
            bounds: Rect::new(0.0, 0.0, 320.0, 90.0),
            scale: 1.0,
        };
        w.event(&mut cx)
    }

    #[test]
    fn child_count_tracks_control() {
        let mut f = FormField::new();
        assert_eq!(f.child_count(), 0);
        let f2 = FormField::new().child(Button::new("x"));
        assert_eq!(Widget::child_count(&f2), 1);
        let _ = &mut f;
    }

    #[test]
    fn error_wins_over_hint() {
        let mut f = FormField::new().hint("the hint");
        assert_eq!(f.message(), Some("the hint"));
        f.set_error(Some("bad".into()));
        assert_eq!(f.message(), Some("bad"));
        f.set_error(None);
        assert_eq!(f.message(), Some("the hint"));
    }

    #[test]
    fn required_prefixes_asterisk() {
        let f = FormField::new().label("Name").required(true);
        assert_eq!(f.label_text(), "* Name");
    }

    #[test]
    fn top_layout_stacks_label_then_control() {
        let mut f = FormField::new().label("L").child(Button::new("x"));
        lay(&mut f);
        let cb = f.control_bounds.unwrap();
        assert!(cb.min_y() >= LABEL_H, "control sits under the label");
        assert!(cb.max_y() <= 90.0);
    }

    #[test]
    fn left_layout_puts_control_beside_label() {
        let mut f = FormField::new()
            .label("L")
            .label_position(LabelPosition::Left)
            .child(Button::new("x"));
        lay(&mut f);
        let cb = f.control_bounds.unwrap();
        assert!(cb.min_x() >= LEFT_LABEL_W, "control right of the column");
    }

    #[test]
    fn message_strip_shrinks_control_area() {
        let mut with = FormField::new().child(Button::new("x")).hint("h");
        let mut without = FormField::new().child(Button::new("x"));
        lay(&mut with);
        lay(&mut without);
        let hw = with.control_bounds.unwrap().height();
        let hn = without.control_bounds.unwrap().height();
        assert!(hw < hn, "hint strip takes space from the control");
    }

    #[test]
    fn click_inside_control_reaches_child() {
        let mut f = FormField::new().child(Button::new("x"));
        lay(&mut f);
        let cb = f.control_bounds.unwrap();
        let r = ev(
            &mut f,
            &WidgetEvent::PointerPressed {
                position: Vec2::new(cb.min_x() + 4.0, cb.min_y() + 4.0),
                button: martensite_core::PointerButton::Primary,
                count: 1,
            },
        );
        // Button handles the press (arms its activation seam).
        assert_ne!(r, EventResponse::Ignored);
    }

    #[test]
    fn click_outside_control_is_ignored() {
        let mut f = FormField::new().child(Button::new("x"));
        lay(&mut f);
        let r = ev(
            &mut f,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(2.0, 2.0), // label zone — no control
                button: martensite_core::PointerButton::Primary,
            },
        );
        assert_eq!(r, EventResponse::Ignored);
    }

    #[test]
    fn disabled_swallows_events() {
        let mut f = FormField::new().enabled(false).child(Button::new("x"));
        lay(&mut f);
        let cb = f.control_bounds.unwrap();
        let r = ev(
            &mut f,
            &WidgetEvent::PointerReleased {
                position: Vec2::new(cb.min_x() + 4.0, cb.min_y() + 4.0),
                button: martensite_core::PointerButton::Primary,
            },
        );
        assert_eq!(r, EventResponse::Ignored);
    }
}
