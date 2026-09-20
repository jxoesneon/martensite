//! `CommandLink` — the Win32 command-link button: a full-width action
//! with a bold label, a smaller explanatory note, and a trailing `›`
//! affordance. Used in dialogs and wizards where a plain button is
//! too terse and a link is too weak — "the descriptive action row".
//!
//! Activation parks in [`CommandLink::take_activated`]; keyboard
//! `Enter`/`Space` and
//! [`SemanticAction::Click`](martensite_core::SemanticAction::Click)
//! activate too.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::command_link::CommandLink;
//!
//! let link = CommandLink::new("Create a new project")
//!     .note("Start from a template or an empty folder");
//! assert_eq!(link.label, "Create a new project");
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, SemanticAction, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const LABEL_FONT_PT: f32 = 14.0;
const NOTE_FONT_PT: f32 = 11.5;
const PAD_X_PT: f32 = 14.0;
const PAD_Y_PT: f32 = 9.0;
const GAP_Y_PT: f32 = 2.0;
const CHEV_W_PT: f32 = 20.0;
const RADIUS_PT: f32 = 6.0;
const FACE: [u8; 4] = [247, 247, 249, 255];
const FACE_HOVER: [u8; 4] = [238, 240, 245, 255];
const FACE_DOWN: [u8; 4] = [228, 231, 238, 255];
const EDGE: [u8; 4] = [0, 0, 0, 28];
const INK: [u8; 4] = [30, 30, 34, 255];
const MUTED: [u8; 4] = [100, 100, 110, 255];
const CHEV_INK: [u8; 4] = [120, 120, 130, 255];

/// A descriptive action row — see the module docs.
///
/// ```
/// use martensite::widgets::command_link::CommandLink;
///
/// let c = CommandLink::new("Action");
/// assert_eq!(c.label, "Action");
/// ```
pub struct CommandLink {
    /// The bold primary text.
    pub label: String,
    /// The smaller note under the label (may be empty).
    pub note: String,
    /// When `false` the row is dimmed and ignores input.
    pub enabled: bool,
    activated: bool,
    pressed: bool,
    hovered: bool,
    focused: bool,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
    scale: f32,
}

impl CommandLink {
    /// Creates a command link with `label`.
    ///
    /// ```
    /// use martensite::widgets::command_link::CommandLink;
    ///
    /// let c = CommandLink::new("Next");
    /// assert_eq!(c.label, "Next");
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            note: String::new(),
            enabled: true,
            activated: false,
            pressed: false,
            hovered: false,
            focused: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
            scale: 1.0,
        }
    }

    /// Sets the explanatory note line.
    ///
    /// ```
    /// use martensite::widgets::command_link::CommandLink;
    ///
    /// let c = CommandLink::new("Next").note("Installs updates first");
    /// assert_eq!(c.note, "Installs updates first");
    /// ```
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }

    /// Enables or disables the link.
    ///
    /// ```
    /// use martensite::widgets::command_link::CommandLink;
    ///
    /// let c = CommandLink::new("Next").enabled(false);
    /// assert!(!c.enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Installs a shared shaped-text painter.
    ///
    /// ```
    /// use martensite::widgets::command_link::CommandLink;
    ///
    /// let c = CommandLink::new("Next");
    /// let _ = c.label;
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Drains the activation flag (press-release, `Enter`/`Space`, or
    /// AT `Click`).
    ///
    /// ```
    /// use martensite::widgets::command_link::CommandLink;
    ///
    /// let mut c = CommandLink::new("Next");
    /// assert!(!c.take_activated());
    /// ```
    pub fn take_activated(&mut self) -> bool {
        std::mem::take(&mut self.activated)
    }

    fn activate(&mut self) -> EventResponse {
        self.activated = true;
        EventResponse::RequestRepaint
    }
}

impl Widget for CommandLink {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let h = PAD_Y_PT * 2.0
            + LABEL_FONT_PT
            + if self.note.is_empty() {
                0.0
            } else {
                GAP_Y_PT + NOTE_FONT_PT
            };
        Vec2::new(
            cx.pt(200.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(h).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 30.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        if self.enabled {
            cx.hot.flags |= martensite_core::NodeFlags::FOCUSABLE;
        } else {
            cx.hot.flags.remove(martensite_core::NodeFlags::FOCUSABLE);
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Button);
        node.set_label(self.label.as_str());
        if !self.note.is_empty() {
            node.set_description(self.note.as_str());
        }
        node.add_action(accesskit::Action::Click);
        node.add_action(accesskit::Action::Focus);
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
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
                if self.hovered || self.pressed {
                    self.hovered = false;
                    self.pressed = false;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } if self.bounds.contains(*position) => {
                self.pressed = true;
                EventResponse::CapturePointer
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                let was = std::mem::take(&mut self.pressed);
                if was && self.bounds.contains(*position) {
                    self.activated = true;
                }
                EventResponse::ReleasePointer
            }
            WidgetEvent::KeyPressed { key, .. } if key == "Enter" || key == "Space" => {
                self.activate()
            }
            WidgetEvent::SemanticAction(SemanticAction::Click) => self.activate(),
            WidgetEvent::SemanticAction(SemanticAction::Focus) => EventResponse::CaptureFocus,
            WidgetEvent::FocusGained => {
                self.focused = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::FocusLost => {
                self.focused = false;
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let r = kurbo::Rect::new(
            f64::from(self.bounds.min_x()),
            f64::from(self.bounds.min_y()),
            f64::from(self.bounds.max_x()),
            f64::from(self.bounds.max_y()),
        );
        let shape = martensite_core::shape::Shape::rounded(cx.pt(RADIUS_PT));
        let face = if !self.enabled {
            cx.color(TokenKey::SurfaceColor, FACE)
        } else if self.pressed {
            cx.color(TokenKey::SecondaryColor, FACE_DOWN)
        } else if self.hovered || self.focused {
            cx.color(TokenKey::SecondaryColor, FACE_HOVER)
        } else {
            cx.color(TokenKey::SurfaceColor, FACE)
        };
        cx.list.push_fill_shape(r, &shape, face);
        cx.list.push_stroke_shape(
            r,
            &shape,
            cx.pt(1.0),
            cx.color(TokenKey::DividerColor, EDGE),
        );

        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let label_font = cx.pt(LABEL_FONT_PT);
        let note_font = cx.pt(NOTE_FONT_PT);
        let pad_x = cx.pt(PAD_X_PT);
        let text_w = self.bounds.size.x - pad_x * 2.0 - cx.pt(CHEV_W_PT);
        let clip = kurbo::Rect::new(
            f64::from(self.bounds.min_x() + pad_x),
            f64::from(self.bounds.min_y()),
            f64::from(self.bounds.min_x() + pad_x + text_w),
            f64::from(self.bounds.max_y()),
        );
        let alpha = if self.enabled { 255u8 } else { 130 };
        let mut ink = cx.color(TokenKey::TextColor, INK);
        ink[3] = alpha;
        let mut muted = cx.color(TokenKey::TextMutedColor, MUTED);
        muted[3] = alpha;

        // Label + note stack, vertically centered as a unit.
        let label_h = label_font;
        let note_h = if self.note.is_empty() {
            0.0
        } else {
            cx.pt(GAP_Y_PT) + note_font
        };
        let stack_h = label_h + note_h;
        let y0 = self.bounds.origin.y + (self.bounds.size.y - stack_h) / 2.0;
        crate::text_paint::paint_label_clipped(
            painter,
            cx.list,
            clip,
            kurbo::Point::new(f64::from(self.bounds.min_x() + pad_x), f64::from(y0)),
            &self.label,
            label_font,
            ink,
        );
        if !self.note.is_empty() {
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                clip,
                kurbo::Point::new(
                    f64::from(self.bounds.min_x() + pad_x),
                    f64::from(y0 + label_h + cx.pt(GAP_Y_PT)),
                ),
                &self.note,
                note_font,
                muted,
            );
        }

        // Trailing `›` affordance.
        let chev_x = self.bounds.max_x() - pad_x - cx.pt(8.0);
        let chev_y = self.bounds.origin.y + (self.bounds.size.y - label_font) / 2.0;
        crate::text_paint::paint_label(
            painter,
            cx.list,
            kurbo::Point::new(f64::from(chev_x), f64::from(chev_y)),
            "›",
            label_font,
            cx.color(TokenKey::TextMutedColor, CHEV_INK),
        );
    }
}

impl std::fmt::Debug for CommandLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandLink")
            .field("label", &self.label)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut CommandLink, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        c.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev<'a>(event: &'a WidgetEvent) -> EventContext<'a> {
        EventContext {
            event,
            bounds: Rect::new(0.0, 0.0, 300.0, 50.0),
            scale: 1.0,
        }
    }

    #[test]
    fn press_release_activates() {
        let mut c = CommandLink::new("Go");
        laid_out(&mut c, 300.0, 50.0);
        c.event(&mut ev(&WidgetEvent::PointerPressed {
            position: Vec2::new(50.0, 25.0),
            button: PointerButton::Primary,
            count: 1,
        }));
        c.event(&mut ev(&WidgetEvent::PointerReleased {
            position: Vec2::new(50.0, 25.0),
            button: PointerButton::Primary,
        }));
        assert!(c.take_activated());
        assert!(!c.take_activated());
    }

    #[test]
    fn release_outside_does_not_activate() {
        let mut c = CommandLink::new("Go");
        laid_out(&mut c, 300.0, 50.0);
        c.event(&mut ev(&WidgetEvent::PointerPressed {
            position: Vec2::new(50.0, 25.0),
            button: PointerButton::Primary,
            count: 1,
        }));
        c.event(&mut ev(&WidgetEvent::PointerReleased {
            position: Vec2::new(50.0, 500.0),
            button: PointerButton::Primary,
        }));
        assert!(!c.take_activated());
    }

    #[test]
    fn enter_and_click_activate() {
        let mut c = CommandLink::new("Go");
        laid_out(&mut c, 300.0, 50.0);
        c.event(&mut ev(&WidgetEvent::KeyPressed {
            key: "Enter".into(),
            repeat: false,
        }));
        assert!(c.take_activated());
        c.event(&mut ev(&WidgetEvent::SemanticAction(SemanticAction::Click)));
        assert!(c.take_activated());
    }

    #[test]
    fn disabled_inert() {
        let mut c = CommandLink::new("Go").enabled(false);
        laid_out(&mut c, 300.0, 50.0);
        assert_eq!(
            c.event(&mut ev(&WidgetEvent::KeyPressed {
                key: "Enter".into(),
                repeat: false,
            })),
            EventResponse::Ignored
        );
    }

    #[test]
    fn note_grows_measure() {
        let mut no_note = CommandLink::new("Go");
        let mut with_note = CommandLink::new("Go").note("details");
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        let cons = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(400.0, 400.0),
        };
        let a = no_note.measure(&mut cx, cons);
        let b = with_note.measure(&mut cx, cons);
        assert!(b.y > a.y);
    }
}
