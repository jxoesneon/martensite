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

/// The tri-state value of a [`CheckBox`].
///
/// `Indeterminate` is the "partially checked" state used by tree and
/// list selection headers (Qt `PartiallyChecked`, WinUI `null`,
/// HTML `input.indeterminate`) — the box paints a centered square
/// instead of a check mark and the accessibility node reports
/// `Toggled::Mixed`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::CheckState;
///
/// assert_eq!(CheckState::default(), CheckState::Unchecked);
/// assert_eq!(CheckState::Unchecked.cycle(), CheckState::Checked);
/// assert_eq!(CheckState::Checked.cycle(), CheckState::Indeterminate);
/// assert_eq!(CheckState::Indeterminate.cycle(), CheckState::Unchecked);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum CheckState {
    /// Not checked — the box is empty.
    #[default]
    Unchecked,
    /// Checked — the box shows a check mark.
    Checked,
    /// Partially checked — the box shows a centered square and the
    /// accessibility node reports `Toggled::Mixed`.
    Indeterminate,
}

impl CheckState {
    /// `true` when this is [`CheckState::Checked`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CheckState;
    ///
    /// assert!(CheckState::Checked.is_checked());
    /// assert!(!CheckState::Indeterminate.is_checked());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_checked(self) -> bool {
        matches!(self, Self::Checked)
    }

    /// The next state in the tri-state cycle:
    /// `Unchecked → Checked → Indeterminate → Unchecked`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CheckState;
    ///
    /// assert_eq!(CheckState::Indeterminate.cycle(), CheckState::Unchecked);
    /// ```
    #[inline]
    #[must_use]
    pub fn cycle(self) -> Self {
        match self {
            Self::Unchecked => Self::Checked,
            Self::Checked => Self::Indeterminate,
            Self::Indeterminate => Self::Unchecked,
        }
    }
}

impl From<bool> for CheckState {
    #[inline]
    fn from(checked: bool) -> Self {
        if checked {
            Self::Checked
        } else {
            Self::Unchecked
        }
    }
}

impl From<CheckState> for Toggled {
    #[inline]
    fn from(state: CheckState) -> Self {
        match state {
            CheckState::Unchecked => Self::False,
            CheckState::Checked => Self::True,
            CheckState::Indeterminate => Self::Mixed,
        }
    }
}

/// A checkbox widget with a label and toggle state.
///
/// Two-state by default; opt into tri-state cycling with
/// [`tristate`](Self::tristate) and drive the third state through
/// [`state`](Self::state)/[`set_state`](Self::set_state). The legacy
/// boolean API ([`checked`](Self::checked), [`toggle`](Self::toggle),
/// the `checked` field) keeps working — `checked` mirrors
/// `state == Checked`.
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
    /// Whether the checkbox is currently checked — the boolean mirror
    /// of [`state`](Self::state) (`true` iff `state == Checked`).
    /// All methods keep the two in sync; a direct write to this field
    /// is reconciled back onto `state` on the next event or
    /// accessibility pass.
    pub checked: bool,
    /// Whether the checkbox is enabled.
    pub enabled: bool,
    /// Whether user activation cycles through
    /// [`CheckState::Indeterminate`] (Qt `setTristate`, WinUI
    /// `IsThreeState`). `Indeterminate` may be set programmatically via
    /// [`set_state`](Self::set_state) regardless — the flag only
    /// controls what a click/Space does.
    pub tristate: bool,
    /// The authoritative check state.
    state: CheckState,
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
            tristate: false,
            state: CheckState::Unchecked,
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the checked state — `true` maps to
    /// [`CheckState::Checked`], `false` to [`CheckState::Unchecked`].
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
        self.set_checked(checked);
        self
    }

    /// Sets whether user activation cycles through the indeterminate
    /// state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CheckBox;
    ///
    /// let cb = CheckBox::new("Select all").tristate(true);
    /// assert!(cb.tristate);
    /// ```
    #[inline]
    #[must_use]
    pub fn tristate(mut self, tristate: bool) -> Self {
        self.tristate = tristate;
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

    /// The current tri-state value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{CheckBox, CheckState};
    ///
    /// let cb = CheckBox::new("Check");
    /// assert_eq!(cb.state(), CheckState::Unchecked);
    /// ```
    #[inline]
    pub fn state(&self) -> CheckState {
        self.state
    }

    /// Sets the tri-state value, keeping the `checked` mirror in sync.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{CheckBox, CheckState};
    ///
    /// let mut cb = CheckBox::new("Check");
    /// cb.set_state(CheckState::Indeterminate);
    /// assert_eq!(cb.state(), CheckState::Indeterminate);
    /// assert!(!cb.checked);
    /// ```
    #[inline]
    pub fn set_state(&mut self, state: CheckState) {
        self.state = state;
        self.checked = state.is_checked();
    }

    /// Sets the checked state through the boolean API.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::CheckBox;
    ///
    /// let mut cb = CheckBox::new("Check");
    /// cb.set_checked(true);
    /// assert!(cb.checked);
    /// ```
    #[inline]
    pub fn set_checked(&mut self, checked: bool) {
        self.set_state(CheckState::from(checked));
    }

    /// Toggles the checked state — the boolean flip:
    /// `Checked → Unchecked`, anything else → `Checked` (a click on an
    /// indeterminate box resolves it to checked, matching WinUI/Qt
    /// non-tristate activation).
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
        self.set_state(if self.state.is_checked() {
            CheckState::Unchecked
        } else {
            CheckState::Checked
        });
    }

    /// Advances through the tri-state cycle:
    /// `Unchecked → Checked → Indeterminate → Unchecked`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{CheckBox, CheckState};
    ///
    /// let mut cb = CheckBox::new("Select all");
    /// cb.cycle();
    /// assert_eq!(cb.state(), CheckState::Checked);
    /// cb.cycle();
    /// assert_eq!(cb.state(), CheckState::Indeterminate);
    /// cb.cycle();
    /// assert_eq!(cb.state(), CheckState::Unchecked);
    /// ```
    #[inline]
    pub fn cycle(&mut self) {
        self.set_state(self.state.cycle());
    }

    /// Reconciles a direct write to the legacy `checked` field back
    /// onto `state` — field writes are the only way the mirror can
    /// drift, since every method routes through `set_state`.
    fn reconcile(&mut self) {
        if self.checked != self.state.is_checked() {
            self.state = CheckState::from(self.checked);
        }
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
        // `Indeterminate` surfaces as `Toggled::Mixed` — the same
        // mapping ATs expose as `aria-checked="mixed"`.
        node.set_toggled(Toggled::from(self.state));
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn a11y_prepare(&mut self) {
        self.reconcile();
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        self.reconcile();
        let activate = matches!(
            cx.event,
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } | WidgetEvent::KeyPressed { .. }
        );
        if activate {
            if self.tristate {
                self.cycle();
            } else {
                self.toggle();
            }
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

        match self.state {
            CheckState::Checked => {
                // Check mark: two strokes forming a tick inside the
                // box — offsets are logical pt, scaled like the box
                // they sit in.
                let x0 = f64::from(b.origin.x) + cx.ptf(3.5);
                let y0 = f64::from(y) + cx.ptf(8.5);
                let mut tick = kurbo::BezPath::new();
                tick.move_to((x0, y0));
                tick.line_to((x0 + cx.ptf(3.5), y0 + cx.ptf(3.5)));
                tick.line_to((x0 + cx.ptf(9.0), y0 - cx.ptf(5.0)));
                cx.list
                    .push_stroke_path(tick, cx.pt(2.0), cx.color(TokenKey::AccentColor, ACCENT));
            }
            CheckState::Indeterminate => {
                // Partial mark: a centered filled square — the
                // Qt/WinUI "mixed" glyph rather than a check.
                let side = cx.pt(8.0);
                let cxm = f64::from(b.origin.x + box_px / 2.0);
                let cym = f64::from(y + box_px / 2.0);
                let half = f64::from(side / 2.0);
                cx.list.push_fill_shape(
                    kurbo::Rect::new(cxm - half, cym - half, cxm + half, cym + half),
                    &Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 1.5)),
                    cx.color(TokenKey::AccentColor, ACCENT),
                );
            }
            CheckState::Unchecked => {}
        }

        // Clip the label to the widget bounds — a long label can't
        // spill past the right edge.
        let text_x = b.origin.x + box_px + cx.pt(LABEL_GAP);
        crate::text_paint::paint_label_clipped(
            crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
            cx.list,
            kurbo::Rect::new(
                f64::from(text_x),
                f64::from(b.origin.y),
                f64::from(b.max_x()),
                f64::from(b.max_y()),
            ),
            kurbo::Point::new(
                f64::from(text_x),
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
            .field("state", &self.state)
            .field("tristate", &self.tristate)
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
    fn checkbox_state_mirror_stays_in_sync() {
        let mut cb = CheckBox::new("Test");
        cb.set_state(CheckState::Indeterminate);
        assert_eq!(cb.state(), CheckState::Indeterminate);
        assert!(!cb.checked);
        cb.set_state(CheckState::Checked);
        assert!(cb.checked);
        cb.set_checked(false);
        assert_eq!(cb.state(), CheckState::Unchecked);
    }

    #[test]
    fn checkbox_cycle_order() {
        let mut cb = CheckBox::new("Test");
        assert_eq!(cb.state(), CheckState::Unchecked);
        cb.cycle();
        assert_eq!(cb.state(), CheckState::Checked);
        cb.cycle();
        assert_eq!(cb.state(), CheckState::Indeterminate);
        cb.cycle();
        assert_eq!(cb.state(), CheckState::Unchecked);
    }

    #[test]
    fn checkbox_event_toggles_or_cycles() {
        let ev = WidgetEvent::KeyPressed {
            key: "Space".to_string(),
            repeat: false,
        };
        let mut cx = EventContext {
            event: &ev,
            bounds: Rect::default(),
            scale: 1.0,
        };
        // Two-state: Space flips Checked/Unchecked, never visits
        // Indeterminate.
        let mut cb = CheckBox::new("Test");
        assert_eq!(cb.event(&mut cx), EventResponse::RequestRepaint);
        assert_eq!(cb.state(), CheckState::Checked);
        cb.event(&mut cx);
        assert_eq!(cb.state(), CheckState::Unchecked);
        // Tri-state: Space cycles through all three states.
        let mut cb = CheckBox::new("Test").tristate(true);
        cb.event(&mut cx);
        assert_eq!(cb.state(), CheckState::Checked);
        cb.event(&mut cx);
        assert_eq!(cb.state(), CheckState::Indeterminate);
        cb.event(&mut cx);
        assert_eq!(cb.state(), CheckState::Unchecked);
    }

    #[test]
    fn checkbox_accessibility_mixed_state() {
        let mut cb = CheckBox::new("Select all");
        cb.set_state(CheckState::Indeterminate);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        cb.accessibility(&mut node);
        assert_eq!(node.toggled(), Some(Toggled::Mixed));
    }

    #[test]
    fn checkbox_direct_field_write_reconciles() {
        // Legacy `cb.checked = true` writes are adopted onto `state`
        // on the next event pass.
        let mut cb = CheckBox::new("Test");
        cb.set_state(CheckState::Indeterminate);
        cb.checked = true;
        let ev = WidgetEvent::FocusGained;
        let mut cx = EventContext {
            event: &ev,
            bounds: Rect::default(),
            scale: 1.0,
        };
        cb.event(&mut cx);
        assert_eq!(cb.state(), CheckState::Checked);
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
