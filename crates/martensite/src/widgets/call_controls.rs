//! `CallControls` — the video-call control cluster: toggle pills
//! for mic, camera, speaker, and screen share plus a red hang-up
//! button (Zoom/Meet idiom).
//!
//! Toggle clicks park the control in
//! [`CallControls::take_toggled`]; the hang-up parks
//! [`CallControls::take_hangup`]. Keyboard shortcuts mirror the
//! common apps: `m` mic, `v` camera, `s` speaker, `d` share,
//! `h`/`Escape` hang up.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::call_controls::{CallControl, CallControls};
//!
//! let c = CallControls::new();
//! assert!(c.is_on(CallControl::Mic));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const BTN_PT: f32 = 44.0;
const GAP_PT: f32 = 12.0;
const PAD_PT: f32 = 8.0;
const FONT_PT: f32 = 11.0;

const FACE: [u8; 4] = [50, 52, 62, 255];
const OFF: [u8; 4] = [70, 74, 88, 255];
const OFF_INK: [u8; 4] = [180, 184, 195, 255];
const HANGUP: [u8; 4] = [200, 60, 60, 255];
const TEXT: [u8; 4] = [235, 237, 240, 255];

/// A call control identity.
///
/// ```
/// use martensite::widgets::call_controls::CallControl;
///
/// assert_eq!(CallControl::Mic.label(), "Mic");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallControl {
    /// Microphone toggle.
    Mic,
    /// Camera toggle.
    Camera,
    /// Speaker (deafen) toggle.
    Speaker,
    /// Screen-share toggle.
    Share,
    /// End-call button.
    Hangup,
}

impl CallControl {
    /// Short label.
    ///
    /// ```
    /// use martensite::widgets::call_controls::CallControl;
    ///
    /// assert_eq!(CallControl::Hangup.label(), "End");
    /// ```
    pub fn label(&self) -> &'static str {
        match self {
            Self::Mic => "Mic",
            Self::Camera => "Cam",
            Self::Speaker => "Spk",
            Self::Share => "Shr",
            Self::Hangup => "End",
        }
    }

    /// Shortcut key.
    fn key(&self) -> &'static str {
        match self {
            Self::Mic => "m",
            Self::Camera => "v",
            Self::Speaker => "s",
            Self::Share => "d",
            Self::Hangup => "h",
        }
    }
}

/// The cluster — see the module docs.
///
/// ```
/// use martensite::widgets::call_controls::CallControls;
///
/// assert_eq!(CallControls::new().control_count(), 5);
/// ```
pub struct CallControls {
    /// Accessibility label.
    pub label: String,
    /// Which controls appear (hangup always last when present).
    pub controls: Vec<CallControl>,
    on: [bool; 4],
    toggled: Option<CallControl>,
    hangup: bool,
    rects: Vec<Rect>,
    text_painter: Option<SharedTextPainter>,
    bounds: Rect,
    scale: f32,
}

impl std::fmt::Debug for CallControls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallControls")
            .field("controls", &self.controls.len())
            .finish()
    }
}

impl Default for CallControls {
    fn default() -> Self {
        Self::new()
    }
}

impl CallControls {
    /// Full cluster: Mic, Camera, Speaker, Share, Hangup.
    ///
    /// ```
    /// use martensite::widgets::call_controls::CallControls;
    ///
    /// assert_eq!(CallControls::new().control_count(), 5);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Call controls".to_string(),
            controls: vec![
                CallControl::Mic,
                CallControl::Camera,
                CallControl::Speaker,
                CallControl::Share,
                CallControl::Hangup,
            ],
            on: [true; 4],
            toggled: None,
            hangup: false,
            rects: Vec::new(),
            text_painter: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::call_controls::CallControls;
    ///
    /// assert_eq!(CallControls::new().label("Call").label, "Call");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Shared text painter.
    ///
    /// ```no_run
    /// use martensite::widgets::call_controls::CallControls;
    /// # fn example(p: martensite::text_paint::SharedTextPainter) {
    /// let _c = CallControls::new().with_text_painter(p);
    /// # }
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Visible control count.
    ///
    /// ```
    /// use martensite::widgets::call_controls::CallControls;
    ///
    /// assert_eq!(CallControls::new().control_count(), 5);
    /// ```
    pub fn control_count(&self) -> usize {
        self.controls.len()
    }

    /// Whether a toggle control is on (hangup reports `false`).
    ///
    /// ```
    /// use martensite::widgets::call_controls::{CallControl, CallControls};
    ///
    /// assert!(CallControls::new().is_on(CallControl::Camera));
    /// ```
    pub fn is_on(&self, control: CallControl) -> bool {
        match control {
            CallControl::Mic => self.on[0],
            CallControl::Camera => self.on[1],
            CallControl::Speaker => self.on[2],
            CallControl::Share => self.on[3],
            CallControl::Hangup => false,
        }
    }

    /// Sets a toggle state host-side (no seam fired).
    ///
    /// ```
    /// use martensite::widgets::call_controls::{CallControl, CallControls};
    ///
    /// let mut c = CallControls::new();
    /// c.set_on(CallControl::Mic, false);
    /// assert!(!c.is_on(CallControl::Mic));
    /// ```
    pub fn set_on(&mut self, control: CallControl, on: bool) {
        match control {
            CallControl::Mic => self.on[0] = on,
            CallControl::Camera => self.on[1] = on,
            CallControl::Speaker => self.on[2] = on,
            CallControl::Share => self.on[3] = on,
            CallControl::Hangup => {}
        }
    }

    /// Drains the last toggled control.
    ///
    /// ```
    /// use martensite::widgets::call_controls::CallControls;
    ///
    /// let mut c = CallControls::new();
    /// assert_eq!(c.take_toggled(), None);
    /// ```
    pub fn take_toggled(&mut self) -> Option<CallControl> {
        self.toggled.take()
    }

    /// Drains the hang-up request.
    ///
    /// ```
    /// use martensite::widgets::call_controls::CallControls;
    ///
    /// let mut c = CallControls::new();
    /// assert!(!c.take_hangup());
    /// ```
    pub fn take_hangup(&mut self) -> bool {
        std::mem::take(&mut self.hangup)
    }

    fn toggle(&mut self, i: usize) {
        let c = self.controls[i];
        if c == CallControl::Hangup {
            self.hangup = true;
        } else {
            self.set_on(c, !self.is_on(c));
            self.toggled = Some(c);
        }
    }
}

impl Widget for CallControls {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let s = cx.scale;
        let n = self.controls.len().max(1) as f32;
        let w = (n * BTN_PT + (n - 1.0).max(0.0) * GAP_PT + PAD_PT * 2.0) * s;
        let h = (BTN_PT + PAD_PT * 2.0) * s;
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 50.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        let s = cx.scale;
        let btn = BTN_PT * s;
        let gap = GAP_PT * s;
        let n = self.controls.len();
        let strip = n as f32 * (btn + gap) - if n > 0 { gap } else { 0.0 };
        let mut x = bounds.min_x() + (bounds.width() - strip).max(0.0) / 2.0;
        let y = bounds.min_y() + (bounds.height() - btn).max(0.0) / 2.0;
        self.rects.clear();
        for _ in &self.controls {
            self.rects.push(Rect::new(x, y, btn, btn));
            x += btn + gap;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Toolbar);
        node.set_label(self.label.clone());
        let off: Vec<&str> = self
            .controls
            .iter()
            .filter(|c| **c != CallControl::Hangup && !self.is_on(**c))
            .map(|c| c.label())
            .collect();
        if !off.is_empty() {
            node.set_value(format!("off: {}", off.join(", ")));
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::KeyPressed { key, .. } => {
                let hit = self.controls.iter().position(|c| {
                    c.key() == key.as_str() || (c == &CallControl::Hangup && key == "Escape")
                });
                if let Some(i) = hit {
                    self.toggle(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(i) = self.rects.iter().position(|r| r.contains(*position)) {
                    self.toggle(i);
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let shape = martensite_core::shape::Shape::rounded(BTN_PT * 0.5 * s);
        for (i, c) in self.controls.iter().enumerate() {
            let r = self.rects[i];
            let kr = kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            );
            let (face, ink) = if *c == CallControl::Hangup {
                (cx.color(TokenKey::ErrorColor, HANGUP), TEXT)
            } else if self.is_on(*c) {
                (
                    cx.color(TokenKey::SecondaryColor, FACE),
                    cx.color(TokenKey::TextColor, TEXT),
                )
            } else {
                (OFF, OFF_INK)
            };
            cx.list.push_fill_shape(kr, &shape, face);
            let label = c.label();
            let fs = FONT_PT * s;
            let w = painter
                .and_then(|p| p.measure_text(label, fs))
                .unwrap_or(label.len() as f32 * fs * 0.6);
            crate::text_paint::paint_label(
                painter,
                cx.list,
                kurbo::Point::new(
                    f64::from(r.min_x() + (r.width() - w) / 2.0),
                    f64::from(r.min_y() + r.height() / 2.0),
                ),
                label,
                fs,
                ink,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut CallControls) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 400.0, 60.0));
    }

    fn ev(c: &mut CallControls, e: &WidgetEvent) -> EventResponse {
        c.event(&mut EventContext {
            event: e,
            bounds: c.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn click_toggles() {
        let mut c = CallControls::new();
        laid_out(&mut c);
        let r = c.rects[0];
        ev(
            &mut c,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
        );
        assert!(!c.is_on(CallControl::Mic));
        assert_eq!(c.take_toggled(), Some(CallControl::Mic));
    }

    #[test]
    fn hangup_parks() {
        let mut c = CallControls::new();
        laid_out(&mut c);
        let r = *c.rects.last().unwrap();
        ev(
            &mut c,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
            },
        );
        assert!(c.take_hangup());
        assert_eq!(c.take_toggled(), None);
    }

    #[test]
    fn keys_toggle_and_escape_hangs_up() {
        let mut c = CallControls::new();
        laid_out(&mut c);
        for key in ["m", "v"] {
            ev(
                &mut c,
                &WidgetEvent::KeyPressed {
                    key: key.to_string(),
                    repeat: false,
                },
            );
        }
        assert!(!c.is_on(CallControl::Mic));
        assert!(!c.is_on(CallControl::Camera));
        assert!(c.is_on(CallControl::Speaker));
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: "Escape".to_string(),
                repeat: false,
            },
        );
        assert!(c.take_hangup());
    }

    #[test]
    fn paint_without_painter() {
        let mut c = CallControls::new();
        laid_out(&mut c);
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
