//! `Tooltip` widget: an ARIA APG tooltip.
//!
//! Implements the [APG tooltip pattern](https://www.w3.org/WAI/ARIA/apg/patterns/tooltip/):
//!
//! - A `Role::Tooltip` popup rendered in the
//!   [`OverlayLayer`](martensite_core::overlay::OverlayLayer), emitted
//!   into the accessibility tree as an overlay virtual node.
//! - The owning element is wired with `aria-describedby` pointing at
//!   the tooltip node while the popup is open.
//! - **Hover**: the popup appears after a configurable delay
//!   ([`Tooltip::tick`], default 700 ms) once the pointer enters the
//!   trigger, and hides on hover exit — the framework dispatches
//!   [`WidgetEvent::PointerEnter`](martensite_core::WidgetEvent::PointerEnter)/[`WidgetEvent::PointerLeave`](martensite_core::WidgetEvent::PointerLeave) at the
//!   hover boundary.
//! - **Focus**: keyboard focus shows the tooltip immediately;
//!   `FocusLost` hides it.
//! - **Dismissal**: `Escape` hides the tooltip (both via the overlay's
//!   top-level Escape handling and the widget's own key handler) —
//!   honouring WCAG 1.4.13's "dismissable without moving focus".
//! - **Hoverable** (WCAG 1.4.13): after the pointer leaves the trigger
//!   a [`TOOLTIP_HOVER_GRACE_MS`] grace window keeps the popup open —
//!   long enough to cross the placement gap onto the bubble. The
//!   bubble reports pointer activity back through a shared flag
//!   drained in [`Tooltip::sync_overlay`], so the tooltip stays open
//!   while the pointer is on the popup itself.
//!
//! Timing is explicit: the widget does not consult a wall clock, so
//! the app (or the `martensite-test` virtual clock) drives the delay
//! by calling [`Tooltip::tick`] each frame.
//!
//! # Examples
//!
//! ```
//! use std::time::Duration;
//!
//! use martensite::widgets::{Text, Tooltip};
//!
//! let mut tip = Tooltip::new(Text::new("Save"), "Save the document");
//! // Hover for the delay duration…
//! tip.begin_hover();
//! tip.tick(Duration::from_millis(700));
//! assert!(tip.is_shown());
//! ```

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use kurbo::Shape;
use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
use martensite_core::widget::{
    A11yEmittedNode, EventContext, EventResponse, LayoutConstraints, LayoutContext, OverlayA11yRef,
    PaintContext, SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

/// Default hover delay before a tooltip appears (milliseconds).
///
/// # Examples
///
/// ```
/// use martensite::widgets::DEFAULT_TOOLTIP_DELAY_MS;
///
/// assert_eq!(DEFAULT_TOOLTIP_DELAY_MS, 700);
/// ```
pub const DEFAULT_TOOLTIP_DELAY_MS: u64 = 700;
/// Grace window after the pointer leaves the trigger during which the
/// tooltip stays open — the WCAG 1.4.13 "hoverable" budget that lets
/// the pointer cross the anchor gap onto the popup (milliseconds).
///
/// # Examples
///
/// ```
/// use martensite::widgets::TOOLTIP_HOVER_GRACE_MS;
///
/// assert_eq!(TOOLTIP_HOVER_GRACE_MS, 300);
/// ```
pub const TOOLTIP_HOVER_GRACE_MS: u64 = 300;
/// Minimum allowed hover delay.
const MIN_DELAY_MS: u64 = 500;
/// Maximum allowed hover delay.
const MAX_DELAY_MS: u64 = 1000;

/// Bubble padding in logical pixels.
const PAD: f32 = 6.0;
/// Bubble background.
const BUBBLE_BG: [u8; 4] = [40, 40, 46, 245];
/// Bubble text colour.
const BUBBLE_INK: [u8; 4] = [245, 245, 248, 255];
/// Bubble border.
const BUBBLE_BORDER: [u8; 4] = [80, 80, 88, 255];
/// Bubble corner radius.
const RADIUS: f64 = 4.0;

/// The popup surface for a [`Tooltip`] — a `Role::Tooltip` bubble.
///
/// `Tooltip` opens one of these in the
/// [`OverlayLayer`]; it is
/// public so custom popup code can reuse it.
///
/// # Examples
///
/// ```
/// use martensite::widgets::TooltipBubble;
///
/// let bubble = TooltipBubble::new("tooltip text");
/// ```
pub struct TooltipBubble {
    /// The tooltip text.
    pub text: String,
    /// Set on any pointer event inside the bubble — drained by the
    /// owning `Tooltip` in `sync_overlay` to keep the popup open while
    /// hovered (WCAG 1.4.13). A fresh flag each frame means "hovered
    /// since the last sync".
    hover_flag: Arc<AtomicBool>,
    /// Shared shaped-text painter from the owning `Tooltip`.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl TooltipBubble {
    /// Creates a bubble with the given text.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::TooltipBubble;
    ///
    /// let b = TooltipBubble::new("hello");
    /// assert_eq!(b.text, "hello");
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            hover_flag: Arc::new(AtomicBool::new(false)),
            text_painter: None,
        }
    }
}

impl Widget for TooltipBubble {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let w =
            (self.text.chars().count() as f32 * cx.pt(7.0) + cx.pt(PAD * 2.0)).min(cx.pt(400.0));
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            cx.pt(24.0).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Tooltip);
        node.set_label(self.text.as_str());
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::RoundedRect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
            cx.ptf(RADIUS),
        );
        // Themed surface, keeping the bubble's near-opaque alpha.
        let bg = cx.color(TokenKey::SurfaceColor, BUBBLE_BG);
        cx.list
            .push_path(rect.to_path(0.1), [bg[0], bg[1], bg[2], BUBBLE_BG[3]]);
        cx.list.push_stroke_path(
            rect.to_path(0.1),
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, BUBBLE_BORDER),
        );
        crate::text_paint::paint_label(
            crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
            cx.list,
            kurbo::Point::new(
                f64::from(b.min_x() + cx.pt(PAD)),
                f64::from(b.min_y() + (b.height() - cx.pt(12.0)) / 2.0),
            ),
            &self.text,
            cx.pt(12.0),
            cx.color(TokenKey::TextColor, BUBBLE_INK),
        );
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Any pointer activity inside the bubble reports back to the
        // owning Tooltip through the shared hover flag (drained in
        // `sync_overlay`) so the popup survives the trigger→popup
        // pointer gap. Events are swallowed — a tooltip is
        // presentational, not interactive.
        match cx.event {
            WidgetEvent::PointerMoved { .. }
            | WidgetEvent::PointerPressed { .. }
            | WidgetEvent::PointerReleased { .. }
            | WidgetEvent::PointerEnter
            | WidgetEvent::PointerLeave => {
                self.hover_flag.store(true, Ordering::Relaxed);
            }
            _ => {}
        }
        EventResponse::Ignored
    }
}

/// A tooltip wrapping a trigger widget.
///
/// The trigger is `child(0)` and receives input normally; the tooltip
/// itself lives in the overlay and is reconciled by
/// [`Tooltip::sync_overlay`], which the app calls once per frame
/// before `layout_pass`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Button, Tooltip};
///
/// let tip = Tooltip::new(Button::new("Delete"), "Permanently delete");
/// assert!(!tip.is_shown());
/// ```
pub struct Tooltip {
    /// The wrapped trigger widget (internal child 0).
    trigger: Box<dyn Widget>,
    /// The tooltip text.
    pub text: String,
    /// Hover delay in milliseconds (clamped to 500..=1000).
    pub delay_ms: u64,
    /// Trigger bounds from the last layout pass.
    trigger_bounds: Option<Rect>,
    /// Whether the pointer is currently hovering the trigger.
    hovered: bool,
    /// Accumulated hover time toward the delay.
    hover_elapsed_ms: u64,
    /// Whether the popup is logically shown.
    shown: bool,
    /// The open overlay entry id, if the popup is open.
    popup_id: Option<u64>,
    /// Last seen pointer position (anchors pointer-anchored bubbles).
    last_pointer: Option<Vec2>,
    /// Whether the pointer has left the trigger and the hide grace
    /// window is counting down (WCAG 1.4.13 "hoverable").
    leaving: bool,
    /// Time elapsed since the pointer left the trigger.
    leave_elapsed_ms: u64,
    /// Shared flag the open bubble sets on pointer activity; drained
    /// in [`sync_overlay`](Self::sync_overlay) to cancel the grace
    /// countdown while the popup itself is hovered.
    popup_hover: Arc<AtomicBool>,
    /// Shared shaped-text painter — handed to the bubble at
    /// `sync_overlay`. See [`crate::text_paint`].
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Tooltip {
    /// Wraps `trigger` with a tooltip showing `text`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Text, Tooltip};
    ///
    /// let tip = Tooltip::new(Text::new("trigger"), "tip");
    /// assert_eq!(tip.text, "tip");
    /// ```
    pub fn new(trigger: impl Widget + 'static, text: impl Into<String>) -> Self {
        Self {
            trigger: Box::new(trigger),
            text: text.into(),
            delay_ms: DEFAULT_TOOLTIP_DELAY_MS,
            trigger_bounds: None,
            hovered: false,
            hover_elapsed_ms: 0,
            shown: false,
            popup_id: None,
            last_pointer: None,
            leaving: false,
            leave_elapsed_ms: 0,
            popup_hover: Arc::new(AtomicBool::new(false)),
            text_painter: None,
        }
    }

    /// Shares a [`crate::text_paint::TextPainter`] so the bubble emits
    /// real glyph runs instead of `DrawText` placeholder boxes.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Sets the hover delay; clamped to the 500..=1000 ms window the
    /// platform conventions use.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Text, Tooltip};
    ///
    /// let tip = Tooltip::new(Text::new("t"), "tip").delay_ms(0);
    /// assert_eq!(tip.delay_ms, 500);
    /// ```
    #[inline]
    #[must_use]
    pub fn delay_ms(mut self, ms: u64) -> Self {
        self.delay_ms = ms.clamp(MIN_DELAY_MS, MAX_DELAY_MS);
        self
    }

    /// Whether the popup is logically shown (open or about to open on
    /// the next [`sync_overlay`](Self::sync_overlay)).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Text, Tooltip};
    ///
    /// let mut tip = Tooltip::new(Text::new("t"), "tip");
    /// assert!(!tip.is_shown());
    /// tip.show();
    /// assert!(tip.is_shown());
    /// ```
    #[inline]
    pub fn is_shown(&self) -> bool {
        self.shown
    }

    /// The overlay entry id of the open popup, if any.
    #[inline]
    pub fn popup_id(&self) -> Option<u64> {
        self.popup_id
    }

    /// Shows the tooltip immediately (keyboard-focus path).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Text, Tooltip};
    ///
    /// let mut tip = Tooltip::new(Text::new("t"), "tip");
    /// tip.show();
    /// assert!(tip.is_shown());
    /// ```
    pub fn show(&mut self) {
        self.shown = true;
        // An explicit show (focus / ShowTooltip action) is not subject
        // to the pointer-leave grace countdown.
        self.leaving = false;
        self.leave_elapsed_ms = 0;
    }

    /// Hides the tooltip and resets hover tracking.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Text, Tooltip};
    ///
    /// let mut tip = Tooltip::new(Text::new("t"), "tip");
    /// tip.show();
    /// tip.hide();
    /// assert!(!tip.is_shown());
    /// ```
    pub fn hide(&mut self) {
        self.shown = false;
        self.hovered = false;
        self.hover_elapsed_ms = 0;
        self.leaving = false;
        self.leave_elapsed_ms = 0;
    }

    /// Marks the trigger as hovered, starting the delay countdown.
    /// Called by [`Widget::event`] on `PointerEnter`; exposed for tests
    /// and manual driving.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use martensite::widgets::{Text, Tooltip};
    ///
    /// let mut tip = Tooltip::new(Text::new("t"), "tip");
    /// tip.begin_hover();
    /// tip.tick(Duration::from_millis(700));
    /// assert!(tip.is_shown());
    /// ```
    pub fn begin_hover(&mut self) {
        self.hovered = true;
        self.hover_elapsed_ms = 0;
        self.leaving = false;
        self.leave_elapsed_ms = 0;
    }

    /// Advances the hover-delay clock by `dt`.
    ///
    /// Call once per frame (or once per test step). When the
    /// accumulated hover time reaches [`delay_ms`](Self::delay_ms) the
    /// tooltip is marked shown and opens on the next
    /// [`sync_overlay`](Self::sync_overlay).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use martensite::widgets::{Text, Tooltip};
    ///
    /// let mut tip = Tooltip::new(Text::new("t"), "tip").delay_ms(500);
    /// tip.begin_hover();
    /// tip.tick(Duration::from_millis(400));
    /// assert!(!tip.is_shown()); // delay not yet reached
    /// tip.tick(Duration::from_millis(100));
    /// assert!(tip.is_shown());
    /// ```
    pub fn tick(&mut self, dt: std::time::Duration) {
        if self.hovered && !self.shown {
            self.hover_elapsed_ms = self.hover_elapsed_ms.saturating_add(dt.as_millis() as u64);
            if self.hover_elapsed_ms >= self.delay_ms {
                self.shown = true;
            }
        }
        // WCAG 1.4.13 "hoverable": the pointer has left the trigger —
        // the popup survives for the grace window so the user can move
        // onto the bubble. `sync_overlay` cancels the countdown when
        // the bubble reports pointer activity.
        if self.leaving && self.shown {
            self.leave_elapsed_ms = self.leave_elapsed_ms.saturating_add(dt.as_millis() as u64);
            if self.leave_elapsed_ms >= TOOLTIP_HOVER_GRACE_MS {
                self.hide();
            }
        }
    }

    /// Reconciles the overlay with the tooltip's shown state.
    ///
    /// Call once per frame before `OverlayLayer::layout_pass`:
    ///
    /// - opens the popup when `shown` and no entry exists;
    /// - closes the entry when hidden;
    /// - notices when the layer dismissed the popup (outside press,
    ///   Escape) and resets `shown`/`popup_id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Text, Tooltip};
    /// use martensite_core::overlay::OverlayLayer;
    /// use martensite_core::{HotNode, LayoutContext, Rect, Widget};
    ///
    /// let mut tip = Tooltip::new(Text::new("t"), "tip");
    /// let mut hot = HotNode::default();
    /// let mut cx = LayoutContext {
    ///     hot: &mut hot,
    ///     scale: 1.0,
    /// };
    /// tip.layout(&mut cx, Rect::new(0.0, 0.0, 100.0, 40.0));
    ///
    /// let mut overlay = OverlayLayer::new();
    /// overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
    /// tip.show();
    /// tip.sync_overlay(&mut overlay);
    /// overlay.layout_pass();
    /// assert_eq!(overlay.len(), 1);
    /// ```
    pub fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // The layer dismissed our popup (outside press / Escape).
        if let Some(id) = self.popup_id {
            if !overlay.is_open(id) {
                self.popup_id = None;
                self.shown = false;
                self.hovered = false;
                self.hover_elapsed_ms = 0;
                self.leaving = false;
                self.leave_elapsed_ms = 0;
            }
        }
        // The pointer reached the bubble since the last sync — cancel
        // the grace countdown so the popup stays open while hovered.
        if self.popup_hover.swap(false, Ordering::Relaxed) {
            self.leaving = false;
            self.leave_elapsed_ms = 0;
        }
        if self.shown && self.popup_id.is_none() {
            let anchor = self
                .last_pointer
                .map(OverlayAnchor::Pointer)
                .or_else(|| self.trigger_bounds.map(OverlayAnchor::Bounds))
                .unwrap_or(OverlayAnchor::Pointer(Vec2::ZERO));
            let mut bubble = TooltipBubble::new(self.text.clone());
            bubble.hover_flag = Arc::clone(&self.popup_hover);
            bubble.text_painter = self.text_painter.clone();
            self.popup_id = Some(overlay.open(Box::new(bubble), anchor));
        } else if !self.shown {
            if let Some(id) = self.popup_id.take() {
                overlay.close(id);
            }
        }
    }
}

impl Widget for Tooltip {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        self.trigger.measure(cx, constraints)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.trigger_bounds = Some(bounds);
        // The wrapper carries keyboard focus for the APG "focus shows
        // the tooltip" contract — the trigger is an internal child
        // with no arena node of its own.
        cx.hot.flags |= NodeFlags::FOCUSABLE;
        cx.layout_child(self.trigger.as_mut(), bounds);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        // The trigger carries the semantics; the wrapper node is a
        // plain container that advertises the show/hide tooltip
        // actions so assistive tech can control the popup directly.
        node.set_role(accesskit::Role::GenericContainer);
        node.add_action(accesskit::Action::ShowTooltip);
        node.add_action(accesskit::Action::HideTooltip);
    }

    fn a11y_fixup(
        &self,
        emitted: &mut Vec<A11yEmittedNode>,
        overlay_nodes: &[OverlayA11yRef],
        _this_node: &mut AccessKitNode,
    ) {
        // Wire aria-describedby: while the popup is open the trigger
        // node (emitted path [0]) describes-by the tooltip's root node.
        let Some(popup) = self.popup_id else {
            return;
        };
        let Some(tip_id) = overlay_nodes
            .iter()
            .find(|r| r.entry == popup && r.path.is_empty())
            .map(|r| r.id)
        else {
            return;
        };
        if let Some(trigger) = emitted.iter_mut().find(|e| e.path.as_slice() == [0]) {
            trigger.node.set_described_by(vec![tip_id]);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerEnter => {
                self.begin_hover();
            }
            WidgetEvent::PointerLeave => {
                self.hovered = false;
                self.hover_elapsed_ms = 0;
                // WCAG 1.4.13 "hoverable": don't hide immediately — arm
                // the grace countdown in `tick` so the pointer can
                // reach the popup first.
                if self.shown {
                    self.leaving = true;
                    self.leave_elapsed_ms = 0;
                }
            }
            WidgetEvent::PointerMoved { position } => {
                self.last_pointer = Some(*position);
            }
            WidgetEvent::FocusGained => {
                // Keyboard focus shows the tooltip without the hover
                // delay (APG).
                self.shown = true;
            }
            WidgetEvent::FocusLost => {
                self.hide();
            }
            WidgetEvent::KeyPressed { key, .. } if key == "Escape" => {
                if self.shown {
                    self.hide();
                    return EventResponse::Handled;
                }
            }
            WidgetEvent::SemanticAction(SemanticAction::ShowTooltip) => {
                self.show();
                return EventResponse::Handled;
            }
            WidgetEvent::SemanticAction(SemanticAction::HideTooltip) => {
                self.hide();
                return EventResponse::Handled;
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => {
                // Focus shows the tooltip (FocusGained does the work).
                return EventResponse::CaptureFocus;
            }
            _ => {}
        }
        // Forward everything else (and the events above) to the trigger.
        if let Some(bounds) = self.trigger_bounds {
            let mut child_cx = EventContext {
                event: cx.event,
                bounds,
            };
            self.trigger.event(&mut child_cx)
        } else {
            EventResponse::Ignored
        }
    }

    fn sync_overlay(&mut self, overlay: &mut OverlayLayer) {
        // Delegate to the inherent method so `Tooltip::sync_overlay`
        // and the `Widget` trait seam stay in lock-step.
        Tooltip::sync_overlay(self, overlay);
    }

    fn tick(&mut self, dt: std::time::Duration) -> bool {
        // Delegate to the inherent method — same delegation pattern as
        // `sync_overlay` — then report whether time-dependent work is
        // in flight: the hover-delay countdown, a `shown` transition,
        // or the hoverable grace countdown all need the next frame's
        // repaint / a11y re-emission and overlay sync.
        let was_shown = self.shown;
        Tooltip::tick(self, dt);
        self.shown != was_shown || self.hovered || self.leaving
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, _index: usize) -> Option<&dyn Widget> {
        Some(&*self.trigger)
    }

    fn child_mut(&mut self, _index: usize) -> Option<&mut dyn Widget> {
        Some(&mut *self.trigger)
    }

    fn child_bounds(&self, _index: usize) -> Option<Rect> {
        self.trigger_bounds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Text;
    use martensite_core::overlay::OverlayLayer;
    use martensite_core::HotNode;
    use std::time::Duration;

    fn laid_out(tip: &mut Tooltip, bounds: Rect) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        tip.layout(&mut cx, bounds);
    }

    fn event(tip: &mut Tooltip, ev: &WidgetEvent) -> EventResponse {
        let mut cx = EventContext {
            event: ev,
            bounds: tip.trigger_bounds.unwrap_or_default(),
        };
        tip.event(&mut cx)
    }

    #[test]
    fn hover_delay_then_show() {
        let mut tip = Tooltip::new(Text::new("t"), "tip").delay_ms(500);
        laid_out(&mut tip, Rect::new(0.0, 0.0, 100.0, 40.0));
        event(&mut tip, &WidgetEvent::PointerEnter);
        tip.tick(Duration::from_millis(300));
        assert!(!tip.is_shown());
        tip.tick(Duration::from_millis(250));
        assert!(tip.is_shown());
    }

    #[test]
    fn hover_exit_arms_grace_then_hides() {
        let mut tip = Tooltip::new(Text::new("t"), "tip").delay_ms(500);
        laid_out(&mut tip, Rect::new(0.0, 0.0, 100.0, 40.0));
        event(&mut tip, &WidgetEvent::PointerEnter);
        tip.tick(Duration::from_millis(600));
        assert!(tip.is_shown());
        // WCAG 1.4.13: the exit does not hide immediately — the grace
        // window lets the pointer cross onto the popup.
        event(&mut tip, &WidgetEvent::PointerLeave);
        assert!(tip.is_shown());
        tip.tick(Duration::from_millis(TOOLTIP_HOVER_GRACE_MS));
        assert!(!tip.is_shown());
        // Re-entering restarts the countdown.
        event(&mut tip, &WidgetEvent::PointerEnter);
        tip.tick(Duration::from_millis(100));
        assert!(!tip.is_shown());
    }

    #[test]
    fn hover_exit_reentry_cancels_grace() {
        let mut tip = Tooltip::new(Text::new("t"), "tip").delay_ms(500);
        laid_out(&mut tip, Rect::new(0.0, 0.0, 100.0, 40.0));
        event(&mut tip, &WidgetEvent::PointerEnter);
        tip.tick(Duration::from_millis(600));
        assert!(tip.is_shown());
        event(&mut tip, &WidgetEvent::PointerLeave);
        // Re-entering the trigger during the grace window cancels it.
        tip.tick(Duration::from_millis(TOOLTIP_HOVER_GRACE_MS - 50));
        event(&mut tip, &WidgetEvent::PointerEnter);
        tip.tick(Duration::from_millis(TOOLTIP_HOVER_GRACE_MS));
        assert!(tip.is_shown());
    }

    #[test]
    fn focus_shows_immediately() {
        let mut tip = Tooltip::new(Text::new("t"), "tip");
        laid_out(&mut tip, Rect::new(0.0, 0.0, 100.0, 40.0));
        event(&mut tip, &WidgetEvent::FocusGained);
        assert!(tip.is_shown());
        event(&mut tip, &WidgetEvent::FocusLost);
        assert!(!tip.is_shown());
    }

    #[test]
    fn escape_hides_shown_tooltip() {
        let mut tip = Tooltip::new(Text::new("t"), "tip");
        laid_out(&mut tip, Rect::new(0.0, 0.0, 100.0, 40.0));
        tip.show();
        let esc = WidgetEvent::KeyPressed {
            key: "Escape".to_string(),
            repeat: false,
        };
        // `Text` ignores keys; the response is the trigger's, but the
        // tooltip state is already hidden.
        event(&mut tip, &esc);
        assert!(!tip.is_shown());
    }

    #[test]
    fn sync_overlay_opens_and_closes() {
        let mut tip = Tooltip::new(Text::new("t"), "tip");
        laid_out(&mut tip, Rect::new(10.0, 10.0, 100.0, 40.0));
        let mut overlay = OverlayLayer::new();
        overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));

        tip.show();
        tip.sync_overlay(&mut overlay);
        overlay.layout_pass();
        assert_eq!(overlay.len(), 1);
        let id = tip.popup_id().unwrap();
        assert!(overlay.is_open(id));

        tip.hide();
        tip.sync_overlay(&mut overlay);
        assert!(overlay.is_empty());
        assert_eq!(tip.popup_id(), None);
    }

    #[test]
    fn overlay_dismissal_resets_shown() {
        let mut tip = Tooltip::new(Text::new("t"), "tip");
        laid_out(&mut tip, Rect::new(10.0, 10.0, 100.0, 40.0));
        let mut overlay = OverlayLayer::new();
        overlay.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
        tip.show();
        tip.sync_overlay(&mut overlay);
        let id = tip.popup_id().unwrap();
        overlay.close(id); // simulate layer-level dismissal
        tip.sync_overlay(&mut overlay);
        assert!(!tip.is_shown());
        assert_eq!(tip.popup_id(), None);
    }

    #[test]
    fn bubble_emits_tooltip_role() {
        let bubble = TooltipBubble::new("help text");
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        bubble.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Tooltip);
        assert_eq!(node.label(), Some("help text"));
    }

    #[test]
    fn semantic_show_hide() {
        let mut tip = Tooltip::new(Text::new("t"), "tip");
        laid_out(&mut tip, Rect::new(0.0, 0.0, 50.0, 20.0));
        event(
            &mut tip,
            &WidgetEvent::SemanticAction(SemanticAction::ShowTooltip),
        );
        assert!(tip.is_shown());
        event(
            &mut tip,
            &WidgetEvent::SemanticAction(SemanticAction::HideTooltip),
        );
        assert!(!tip.is_shown());
    }
}
