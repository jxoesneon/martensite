//! `CheckBox` widget: a toggleable checkbox with an accessible label.
//!
//! The `CheckBox` widget exposes `Role::CheckBox`, an accessible label,
//! the `Action::Click` and `Action::Focus` accessibility actions, and
//! the `Toggled` state. It integrates with the focus system via
//! `NodeFlags::FOCUSABLE`.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::checkbox::CheckBox;
//!
//! let cb = CheckBox::new("Accept Terms").checked(true);
//! assert!(cb.checked);
//! ```

use accesskit::{Node as AccessKitNode, Toggled};
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::TokenKey;
use martensite_core::{NodeFlags, Rect};

/// Checkbox frame border colour.
const EDGE: [u8; 4] = [110, 115, 125, 255];
/// Checkmark / fill colour.
const ACCENT: [u8; 4] = [40, 110, 220, 255];
/// Label ink colour.
const INK: [u8; 4] = [20, 20, 25, 255];
/// Side length of the checkbox square.
const BOX_SIZE: f32 = 16.0;
/// Gap between the box and the label.
const LABEL_GAP: f32 = 8.0;

/// A checkbox widget with a label and toggle state.
///
/// # Examples
///
/// ```
/// use martensite::widgets::CheckBox;
///
/// let cb = CheckBox::new("Accept terms")
///     .checked(true);
/// assert_eq!(cb.label, "Accept terms");
/// assert!(cb.checked);
/// ```
#[derive(Clone)]
pub struct CheckBox {
    /// The accessible label for the checkbox.
    pub label: String,
    /// Whether the checkbox is currently checked.
    pub checked: bool,
    /// Whether the checkbox is enabled.
    pub enabled: bool,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter — when set, `paint` emits real
    /// `GlyphRun`s; without it text falls back to `DrawText`
    /// placeholder boxes. See [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl CheckBox {
    /// Creates a new checkbox with the given label, unchecked by default.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CheckBox;
    ///
    /// let cb = CheckBox::new("Subscribe");
    /// assert_eq!(cb.label, "Subscribe");
    /// assert!(!cb.checked);
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            checked: false,
            enabled: true,
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the checked state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CheckBox;
    ///
    /// let cb = CheckBox::new("Remember me").checked(true);
    /// assert!(cb.checked);
    /// ```
    #[inline]
    #[must_use]
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Sets whether the checkbox is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CheckBox;
    ///
    /// let cb = CheckBox::new("Remember me").enabled(false);
    /// assert!(!cb.enabled);
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Toggles the checked state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CheckBox;
    ///
    /// let mut cb = CheckBox::new("Toggle me");
    /// assert!(!cb.checked);
    /// cb.toggle();
    /// assert!(cb.checked);
    /// ```
    #[inline]
    pub fn toggle(&mut self) {
        self.checked = !self.checked;
    }

    /// Returns the cached bounds from the last layout pass.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CheckBox;
    ///
    /// let cb = CheckBox::new("Check");
    /// let bounds = cb.cached_bounds();
    /// assert_eq!(bounds.size.x, 0.0);
    /// ```
    #[inline]
    pub fn cached_bounds(&self) -> Rect {
        self.cached_bounds
    }

    /// Shares a [`crate::text_paint::TextPainter`] so `paint` emits real
    /// glyph runs instead of `DrawText` placeholder boxes.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }
}

impl Widget for CheckBox {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        // Box + gap + label, matching `RadioOption` — a box-only answer
        // lets a tight parent clip the label.
        let w = cx.pt(20.0 + LABEL_GAP + 8.0 * self.label.chars().count() as f32);
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            cx.pt(20.0).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        // Declare keyboard focusability on the arena node — the
        // `FocusManager` rejects focus requests for nodes without it.
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::CheckBox);
        node.set_label(self.label.as_str());
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Focus);
        node.set_toggled(if self.checked {
            Toggled::True
        } else {
            Toggled::False
        });
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        let toggle = matches!(
            cx.event,
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } | WidgetEvent::KeyPressed { .. }
        );
        if toggle {
            self.checked = !self.checked;
            EventResponse::RequestRepaint
        } else {
            EventResponse::Ignored
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let box_px = cx.pt(BOX_SIZE);
        let y = b.origin.y + (b.size.y - box_px) / 2.0;
        let bx = kurbo::Rect::new(
            f64::from(b.origin.x),
            f64::from(y),
            f64::from(b.origin.x + box_px),
            f64::from(y + box_px),
        );
        cx.list.push_stroke_shape(
            bx,
            &Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 3.0)),
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, EDGE),
        );

        if self.checked {
            // Check mark: two strokes forming a tick inside the box —
            // offsets are logical pt, scaled like the box they sit in.
            let x0 = f64::from(b.origin.x) + cx.ptf(3.5);
            let y0 = f64::from(y) + cx.ptf(8.5);
            let mut tick = kurbo::BezPath::new();
            tick.move_to((x0, y0));
            tick.line_to((x0 + cx.ptf(3.5), y0 + cx.ptf(3.5)));
            tick.line_to((x0 + cx.ptf(9.0), y0 - cx.ptf(5.0)));
            cx.list
                .push_stroke_path(tick, cx.pt(2.0), cx.color(TokenKey::AccentColor, ACCENT));
        }

        crate::text_paint::paint_label(
            crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
            cx.list,
            kurbo::Point::new(
                f64::from(b.origin.x + box_px + cx.pt(LABEL_GAP)),
                f64::from(b.origin.y + (b.size.y - cx.pt(14.0)) / 2.0),
            ),
            &self.label,
            cx.pt(14.0),
            cx.color(TokenKey::TextColor, INK),
        );
    }
}

impl std::fmt::Debug for CheckBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckBox")
            .field("label", &self.label)
            .field("checked", &self.checked)
            .field("enabled", &self.enabled)
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

    #[test]
    fn checkbox_new() {
        let cb = CheckBox::new("Accept");
        assert_eq!(cb.label, "Accept");
        assert!(!cb.checked);
        assert!(cb.enabled);
    }

    #[test]
    fn checkbox_builder_methods() {
        let cb = CheckBox::new("Agree").checked(true).enabled(false);
        assert!(cb.checked);
        assert!(!cb.enabled);
    }

    #[test]
    fn checkbox_toggle() {
        let mut cb = CheckBox::new("Test");
        assert!(!cb.checked);
        cb.toggle();
        assert!(cb.checked);
        cb.toggle();
        assert!(!cb.checked);
    }

    #[test]
    fn checkbox_measure_returns_size() {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let mut cb = CheckBox::new("Test");
        let size = cb.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(200.0, 100.0),
            },
        );
        assert!(size.x >= 0.0 && size.y >= 0.0);
    }

    #[test]
    fn checkbox_layout_sets_bounds() {
        let mut hot = HotNode::default();
        let mut cx = make_cx(&mut hot);
        let mut cb = CheckBox::new("Test");
        let bounds = Rect::new(0.0, 0.0, 20.0, 20.0);
        cb.layout(&mut cx, bounds);
        assert_eq!(cb.cached_bounds(), bounds);
    }

    #[test]
    fn checkbox_accessibility_sets_role_label_toggled() {
        let cb = CheckBox::new("Accept").checked(true);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        cb.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::CheckBox);
        assert_eq!(node.label(), Some("Accept"));
        assert_eq!(node.toggled(), Some(Toggled::True));
        assert!(node.supports_action(accesskit::Action::Click));
        assert!(node.supports_action(accesskit::Action::Focus));
    }

    #[test]
    fn checkbox_accessibility_unchecked_state() {
        let cb = CheckBox::new("Decline");
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        cb.accessibility(&mut node);
        assert_eq!(node.toggled(), Some(Toggled::False));
    }

    #[test]
    fn checkbox_accessibility_disabled() {
        let cb = CheckBox::new("Locked").enabled(false);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        cb.accessibility(&mut node);
        assert!(node.is_disabled());
    }

    #[test]
    fn checkbox_clone() {
        let cb = CheckBox::new("Test").checked(true);
        let cloned = cb.clone();
        assert_eq!(cb.label, cloned.label);
        assert_eq!(cb.checked, cloned.checked);
    }

    #[test]
    fn checkbox_debug_format() {
        let cb = CheckBox::new("Test");
        let debug = format!("{:?}", cb);
        assert!(debug.contains("CheckBox"));
        assert!(debug.contains("Test"));
    }
}
