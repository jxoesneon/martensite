//! `Switch` widget: an iOS-style pill toggle.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::switch::Switch;
//!
//! let sw = Switch::new("Enabled").on(true);
//! assert!(sw.on);
//! ```

use accesskit::{Node as AccessKitNode, Toggled};
use glam::Vec2;
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Widget, WidgetEvent,
};
use martensite_core::{NodeFlags, Rect, TokenKey};

const EDGE: [u8; 4] = [110, 115, 125, 255];
const ACCENT: [u8; 4] = [40, 110, 220, 255];
const KNOB: [u8; 4] = [255, 255, 255, 255];
const INK: [u8; 4] = [20, 20, 25, 255];
/// Track size in logical points (roughly the iOS/macOS proportion).
const TRACK_W: f32 = 34.0;
const TRACK_H: f32 = 20.0;
const LABEL_GAP: f32 = 8.0;

/// A pill-shaped toggle switch with an accessible label.
///
/// # Examples
///
/// ```
/// use martensite::widgets::Switch;
///
/// let mut sw = Switch::new("Live updates");
/// sw.toggle();
/// assert!(sw.on);
/// ```
#[derive(Clone)]
pub struct Switch {
    /// Accessible label shown beside the track.
    pub label: String,
    /// Accessible name used when `label` is empty — set when the
    /// switch's meaning is carried by a sibling (a settings row's
    /// title), so the painted track stays bare while assistive tech
    /// still gets a name. Painted nowhere; surfaced to AccessKit and
    /// to design-lint via the `@labeled` scope marker.
    pub a11y_label: String,
    /// Whether the switch is on.
    pub on: bool,
    /// Whether the switch is enabled.
    pub enabled: bool,
    /// Keyboard focus — paints the WCAG 2.4.13 accent ring.
    focused: bool,
    /// Cached bounds from the last layout pass.
    cached_bounds: Rect,
    /// Shared shaped-text painter (see [`crate::text_paint`]).
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl Switch {
    /// Creates a switch in the off state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Switch;
    ///
    /// let sw = Switch::new("Notifications");
    /// assert!(!sw.on);
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            a11y_label: String::new(),
            on: false,
            enabled: true,
            focused: false,
            cached_bounds: Rect::default(),
            text_painter: None,
        }
    }

    /// Sets the accessible name reported when `label` is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Switch;
    ///
    /// let sw = Switch::new("").a11y_label("Line running");
    /// assert_eq!(sw.a11y_label, "Line running");
    /// ```
    #[must_use]
    pub fn a11y_label(mut self, label: impl Into<String>) -> Self {
        self.a11y_label = label.into();
        self
    }

    /// Sets the on state.
    #[inline]
    #[must_use]
    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    /// Sets whether the switch is enabled.
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Flips the switch.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::Switch;
    ///
    /// let mut sw = Switch::new("Dark mode");
    /// sw.toggle();
    /// assert!(sw.on);
    /// ```
    #[inline]
    pub fn toggle(&mut self) {
        self.on = !self.on;
    }

    /// Shares a [`crate::text_paint::TextPainter`] for real glyph runs.
    #[must_use]
    pub fn with_text_painter(mut self, painter: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }
}

impl Widget for Switch {
    #[cfg(feature = "devtools-timemachine")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let w = cx.pt(TRACK_W + LABEL_GAP + 8.0 * self.label.chars().count() as f32);
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            // The whole bounds are the pointer target — WCAG 2.5.8
            // floors it at 24pt even though the track itself is 20pt.
            cx.pt(24.0).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.cached_bounds = bounds;
        if self.enabled {
            cx.hot.flags |= NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(NodeFlags::FOCUSABLE);
        }
    }

    /// `@labeled` declares the accessible name to design-lint's
    /// `icon-only-control` rule — the paint list can't see the
    /// AccessKit label, so the scope marker carries it.
    fn debug_name(&self) -> &'static str {
        if self.label.is_empty() && self.a11y_label.is_empty() {
            "Switch"
        } else {
            "Switch@labeled"
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Switch);
        let name = if self.label.is_empty() {
            self.a11y_label.as_str()
        } else {
            self.label.as_str()
        };
        node.set_label(name);
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Focus);
        node.set_toggled(if self.on {
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
        match cx.event {
            WidgetEvent::FocusGained => {
                self.focused = true;
                return EventResponse::RequestRepaint;
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                return EventResponse::RequestRepaint;
            }
            _ => {}
        }
        // APG switch: Space/Enter toggle; other keys must not flip
        // the track while it holds focus.
        let toggle = matches!(
            cx.event,
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            }
        ) || matches!(
            cx.event,
            WidgetEvent::KeyPressed { key, repeat, .. }
                if matches!(key.as_str(), " " | "Space" | "Enter") && !*repeat
        );
        if toggle {
            self.on = !self.on;
            EventResponse::RequestRepaint
        } else {
            EventResponse::Ignored
        }
    }

    fn focused(&self) -> bool {
        self.focused
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let tw = cx.pt(TRACK_W).min(b.size.x);
        let th = cx.pt(TRACK_H).min(b.size.y);
        let ty = b.origin.y + (b.size.y - th).max(0.0) / 2.0;
        let track = kurbo::Rect::new(
            f64::from(b.origin.x),
            f64::from(ty),
            f64::from(b.origin.x + tw),
            f64::from(ty + th),
        );
        let pill = Shape::PILL;
        let track_fill = if self.on {
            cx.color(TokenKey::AccentColor, ACCENT)
        } else {
            cx.color(TokenKey::SurfaceColor, [70, 74, 82, 255])
        };
        cx.list.push_fill_shape(track, &pill, track_fill);
        // Keyline: the track edge is drawn *inside* the fill — a stroke
        // straddling the boundary can't read on both a dark surface and
        // a chromatic face, so it must only contrast the fill.
        let ew = cx.pt(1.0);
        cx.list.push_stroke_shape(
            track.inset(-f64::from(ew)),
            &pill,
            ew,
            crate::text_paint::better_ink(
                track_fill,
                cx.color(TokenKey::BorderColor, EDGE),
                [20, 20, 24, 255],
            ),
        );

        // Knob: circle inset by 2px, parked right when on.
        let inset = cx.pt(3.0);
        let d = th - inset * 2.0;
        let kx = if self.on {
            b.origin.x + tw - inset - d
        } else {
            b.origin.x + inset
        };
        cx.list.push_fill_shape(
            kurbo::Rect::new(
                f64::from(kx),
                f64::from(ty + inset),
                f64::from(kx + d),
                f64::from(ty + inset + d),
            ),
            &Shape::ELLIPSE,
            KNOB,
        );

        if self.focused && self.enabled {
            crate::widgets::paint_focus_ring(cx, b, 3.0, 2.0);
        }

        // Clip the label to the widget bounds — a long label can't
        // spill past the right edge.
        let text_x = b.origin.x + tw + cx.pt(LABEL_GAP);
        crate::text_paint::paint_label_vcenter(
            crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
            cx.list,
            kurbo::Rect::new(
                f64::from(text_x),
                f64::from(b.origin.y),
                f64::from(b.max_x()),
                f64::from(b.origin.y + (b.size.y)),
            ),
            f64::from(text_x),
            &self.label,
            cx.pt(14.0),
            cx.color(TokenKey::TextColor, INK),
        );
    }

    fn paint_overlay(&self, cx: &mut PaintContext) {
        if !self.enabled {
            crate::widgets::paint_disabled_veil(cx, cx.bounds, TRACK_H);
        }
    }
}

impl std::fmt::Debug for Switch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Switch")
            .field("label", &self.label)
            .field("on", &self.on)
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    // Counts `StrokePath` commands — the WCAG 2.4.13 focus ring lands
    // as a stroke, so a focused control emits strictly more strokes
    // than its unfocused twin.
    fn stroke_count_paint(w: &impl Widget, bounds: Rect) -> usize {
        use martensite_core::{PaintCommand, PaintList, Theme};
        let mut list = PaintList::new();
        let theme = Theme::new("test");
        let mut cx = PaintContext {
            list: &mut list,
            bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        };
        w.paint(&mut cx);
        cx.list
            .commands
            .iter()
            .filter(|c| matches!(c, PaintCommand::StrokePath(..)))
            .count()
    }

    fn drive_event(w: &mut impl Widget, ev: WidgetEvent) -> EventResponse {
        w.event(&mut EventContext {
            event: &ev,
            bounds: Rect::new(0.0, 0.0, 80.0, 32.0),
            scale: 1.0,
        })
    }
    #[test]
    fn switch_toggle() {
        let mut sw = Switch::new("Live");
        sw.toggle();
        assert!(sw.on);
    }

    #[test]
    fn switch_a11y() {
        let sw = Switch::new("Live").on(true);
        let mut node = accesskit::Node::new(accesskit::Role::Unknown);
        sw.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Switch);
        assert_eq!(node.toggled(), Some(Toggled::True));
    }

    #[test]
    fn switch_disabled_not_focusable() {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let mut sw = Switch::new("Live").enabled(false);
        sw.layout(&mut cx, Rect::new(0.0, 0.0, 60.0, 24.0));
        assert!(!hot.flags.contains(NodeFlags::FOCUSABLE));
    }

    #[test]
    fn switch_paints_focus_ring_only_while_focused() {
        // WCAG 2.4.7/2.4.13: keyboard focus must be visible. The ring
        // is emitted as a stroke, so focused paint adds strokes over
        // the unfocused baseline.
        let bounds = Rect::new(0.0, 0.0, 80.0, 32.0);
        let mut w = Switch::new("T");
        let unfocused = stroke_count_paint(&w, bounds);
        assert!(!w.focused());
        assert_eq!(
            drive_event(&mut w, WidgetEvent::FocusGained),
            EventResponse::RequestRepaint
        );
        assert!(w.focused());
        let focused = stroke_count_paint(&w, bounds);
        assert!(
            focused > unfocused,
            "no focus ring: {unfocused} strokes unfocused vs {focused} focused"
        );
        drive_event(&mut w, WidgetEvent::FocusLost);
        assert!(!w.focused());
        assert_eq!(
            stroke_count_paint(&w, bounds),
            unfocused,
            "focus ring lingered after FocusLost"
        );
    }
    #[test]
    fn switch_space_and_enter_toggle_but_arrows_do_not() {
        // APG switch: Space or Enter toggles; arrows are navigation —
        // and a held key (repeat) must not re-toggle.
        let mut sw = Switch::new("T");
        let arrow = WidgetEvent::KeyPressed {
            key: "ArrowDown".to_string(),
            repeat: false,
        };
        drive_event(&mut sw, arrow);
        assert!(!sw.on, "arrow toggled the switch");
        let repeat = WidgetEvent::KeyPressed {
            key: "Space".to_string(),
            repeat: true,
        };
        drive_event(&mut sw, repeat);
        assert!(!sw.on, "key repeat toggled the switch");
        for key in ["Space", "Enter"] {
            let mut sw = Switch::new("T");
            drive_event(
                &mut sw,
                WidgetEvent::KeyPressed {
                    key: key.to_string(),
                    repeat: false,
                },
            );
            assert!(sw.on, "{key} did not toggle the switch");
        }
    }
}
