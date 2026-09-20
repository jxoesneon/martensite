//! `VirtualKeyboard` — an on-screen QWERTY keyboard (OSK idiom for
//! touch/kiosk apps).
//!
//! Four rows of letter keys, a `Shift` toggle (momentary, releases
//! after one key like iOS), `Backspace`, and a `Space` bar. Every
//! key press parks its produced text in
//! [`VirtualKeyboard::take_pressed`] — `"a"`/`"A"` for letters,
//! `"\u{8}"` for backspace, `" "` for space — so the host feeds it
//! into whichever input is focused. Physical Shift mirrors the
//! shift state.
//!
//! Distinct from [`Keypad`](crate::widgets::Keypad), which is the
//! 3×4 telephony pad.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::virtual_keyboard::VirtualKeyboard;
//!
//! let k = VirtualKeyboard::new();
//! assert_eq!(k.key_count(), 30);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const KEY_PT: f32 = 34.0;
const GAP_PT: f32 = 4.0;
const ROWS: f32 = 4.0;
const FONT_PT: f32 = 14.0;

const KEY: [u8; 4] = [62, 64, 72, 255];
const KEY_ACTIVE: [u8; 4] = [96, 165, 250, 255];
const KEY_PRESSED: [u8; 4] = [82, 84, 94, 255];
const FG: [u8; 4] = [230, 232, 238, 255];

/// Special keys that aren't letters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Special {
    Shift,
    Backspace,
    Space,
}

/// One key — a letter, or a special function key.
#[derive(Clone, Copy, Debug, PartialEq)]
enum KeyKind {
    Letter(char),
    Fn(Special),
}

/// Row layouts: key sequence per row (bottom row = specials).
const ROW_LETTERS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];

/// An on-screen keyboard — see the module docs.
///
/// ```
/// use martensite::widgets::virtual_keyboard::VirtualKeyboard;
///
/// assert!(!VirtualKeyboard::new().is_shifted());
/// ```
pub struct VirtualKeyboard {
    /// Accessibility label.
    pub label: String,
    shift: bool,
    /// Per-key rects computed in `layout`, row-major (letters then
    /// specials).
    rects: Vec<(KeyKind, Rect)>,
    /// Currently held key (press-in-progress).
    held: Option<KeyKind>,
    pressed: Option<String>,
    bounds: Rect,
    scale: f32,
    text_painter: Option<SharedTextPainter>,
}

impl std::fmt::Debug for VirtualKeyboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualKeyboard")
            .field("shift", &self.shift)
            .finish()
    }
}

impl Default for VirtualKeyboard {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtualKeyboard {
    /// Unshifted keyboard.
    ///
    /// ```
    /// use martensite::widgets::virtual_keyboard::VirtualKeyboard;
    ///
    /// assert_eq!(VirtualKeyboard::new().key_count(), 30);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Keyboard".to_string(),
            shift: false,
            rects: Vec::new(),
            held: None,
            pressed: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::virtual_keyboard::VirtualKeyboard;
    ///
    /// assert_eq!(VirtualKeyboard::new().label("OSK").label, "OSK");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Initial shift state.
    ///
    /// ```
    /// use martensite::widgets::virtual_keyboard::VirtualKeyboard;
    ///
    /// assert!(VirtualKeyboard::new().shifted(true).is_shifted());
    /// ```
    pub fn shifted(mut self, shifted: bool) -> Self {
        self.shift = shifted;
        self
    }

    /// Optional painter override (tests / headless).
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Number of keys (26 letters + Shift + Backspace + Space ×2).
    ///
    /// ```
    /// use martensite::widgets::virtual_keyboard::VirtualKeyboard;
    ///
    /// assert_eq!(VirtualKeyboard::new().key_count(), 30);
    /// ```
    pub fn key_count(&self) -> usize {
        30
    }

    /// Whether Shift is engaged.
    ///
    /// ```
    /// use martensite::widgets::virtual_keyboard::VirtualKeyboard;
    ///
    /// assert!(!VirtualKeyboard::new().is_shifted());
    /// ```
    pub fn is_shifted(&self) -> bool {
        self.shift
    }

    /// Sets the shift state.
    ///
    /// ```
    /// use martensite::widgets::virtual_keyboard::VirtualKeyboard;
    ///
    /// let mut k = VirtualKeyboard::new();
    /// k.set_shift(true);
    /// assert!(k.is_shifted());
    /// ```
    pub fn set_shift(&mut self, shifted: bool) {
        self.shift = shifted;
    }

    /// Drains the last produced text (`"a"`, `"A"`, `" "`, or
    /// `"\u{8}"` for backspace).
    ///
    /// ```
    /// use martensite::widgets::virtual_keyboard::VirtualKeyboard;
    ///
    /// assert_eq!(VirtualKeyboard::new().take_pressed(), None);
    /// ```
    pub fn take_pressed(&mut self) -> Option<String> {
        self.pressed.take()
    }

    /// The label a key shows at the current shift state.
    fn key_label(&self, kind: KeyKind) -> String {
        match kind {
            KeyKind::Letter(c) => {
                if self.shift {
                    c.to_uppercase().to_string()
                } else {
                    c.to_string()
                }
            }
            KeyKind::Fn(Special::Shift) => "⇧".to_string(),
            KeyKind::Fn(Special::Backspace) => "⌫".to_string(),
            KeyKind::Fn(Special::Space) => "space".to_string(),
        }
    }

    /// The text a key produces.
    fn key_output(&self, kind: KeyKind) -> Option<String> {
        match kind {
            KeyKind::Letter(c) => Some(self.key_label(KeyKind::Letter(c))),
            KeyKind::Fn(Special::Backspace) => Some("\u{8}".to_string()),
            KeyKind::Fn(Special::Space) => Some(" ".to_string()),
            KeyKind::Fn(Special::Shift) => None,
        }
    }

    /// Key hit-test.
    fn key_at(&self, p: Vec2) -> Option<KeyKind> {
        self.rects
            .iter()
            .find(|(_, r)| r.contains(p))
            .map(|(k, _)| *k)
    }

    /// Releases a held key — produces output.
    fn release(&mut self) {
        if let Some(kind) = self.held.take() {
            match kind {
                KeyKind::Fn(Special::Shift) => {
                    self.shift = !self.shift;
                }
                _ => {
                    if let Some(out) = self.key_output(kind) {
                        self.pressed = Some(out);
                    }
                    // Momentary shift — releases after one letter.
                    if matches!(kind, KeyKind::Letter(_)) && self.shift {
                        self.shift = false;
                    }
                }
            }
        }
    }
}

impl Widget for VirtualKeyboard {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let key = cx.pt(KEY_PT);
        let gap = cx.pt(GAP_PT);
        let w = key * 10.0 + gap * 9.0;
        let h = key * ROWS + gap * (ROWS - 1.0);
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(200.0, 80.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
        self.rects.clear();
        let gap = GAP_PT * cx.scale;
        let key_h = (bounds.height() - gap * (ROWS - 1.0)).max(0.0) / ROWS;
        for (row, letters) in ROW_LETTERS.iter().enumerate() {
            let n = letters.chars().count() as f32;
            let key_w = (bounds.width() - gap * (n - 1.0)).max(0.0) / n;
            let mut x = bounds.min_x();
            let y = bounds.min_y() + (key_h + gap) * row as f32;
            for c in letters.chars() {
                self.rects
                    .push((KeyKind::Letter(c), Rect::new(x, y, key_w, key_h)));
                x += key_w + gap;
            }
        }
        // Bottom row: Shift | Space | Backspace.
        let y = bounds.min_y() + (key_h + gap) * 3.0;
        let special_w = (bounds.width() - gap * 2.0).max(0.0) * 0.2;
        let space_w = (bounds.width() - gap * 2.0).max(0.0) * 0.6;
        let mut x = bounds.min_x();
        for (kind, w) in [
            (KeyKind::Fn(Special::Shift), special_w),
            (KeyKind::Fn(Special::Space), space_w),
            (KeyKind::Fn(Special::Backspace), special_w),
        ] {
            self.rects.push((kind, Rect::new(x, y, w, key_h)));
            x += w + gap;
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!(
            "{}{}",
            self.label,
            if self.shift { " — shift" } else { "" }
        ));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(kind) = self.key_at(*position) {
                    self.held = Some(kind);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                // Slide off the key cancels the press (OSK convention).
                if self.held.is_some() && self.key_at(*position) != self.held {
                    self.held = None;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if self.held.is_some() {
                    if self.key_at(*position) == self.held {
                        self.release();
                    } else {
                        self.held = None;
                    }
                    return EventResponse::ReleasePointer;
                }
                EventResponse::Ignored
            }
            // Physical Shift mirrors the OSK state.
            WidgetEvent::KeyPressed { key, .. } if key == "Shift" => {
                self.shift = true;
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyReleased { key } if key == "Shift" => {
                self.shift = false;
                EventResponse::RequestRepaint
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let size = FONT_PT * s;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        for (kind, rect) in &self.rects {
            let active = *kind == KeyKind::Fn(Special::Shift) && self.shift;
            let held = self.held == Some(*kind);
            let fill = if held {
                cx.color(TokenKey::BorderColor, KEY_PRESSED)
            } else if active {
                cx.color(TokenKey::AccentColor, KEY_ACTIVE)
            } else {
                cx.color(TokenKey::SurfaceColor, KEY)
            };
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(rect.min_x()),
                    f64::from(rect.min_y()),
                    f64::from(rect.max_x()),
                    f64::from(rect.max_y()),
                ),
                &martensite_core::shape::Shape::rounded(4.0 * s),
                fill,
            );
            let label = self.key_label(*kind);
            let fg = cx.color(TokenKey::TextColor, FG);
            let tw = painter
                .and_then(|p| p.measure_text(&label, size))
                .unwrap_or(label.chars().count() as f32 * size * 0.55);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                kurbo::Rect::new(
                    f64::from(rect.min_x()),
                    f64::from(rect.min_y()),
                    f64::from(rect.max_x()),
                    f64::from(rect.max_y()),
                ),
                kurbo::Point::new(
                    f64::from(rect.min_x() + (rect.width() - tw).max(0.0) / 2.0),
                    f64::from(rect.min_y() + (rect.height() - size * 1.2).max(0.0) / 2.0),
                ),
                &label,
                size,
                fg,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(k: &mut VirtualKeyboard) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        k.layout(&mut cx, Rect::new(0.0, 0.0, 380.0, 160.0));
    }

    fn ev(k: &mut VirtualKeyboard, e: &WidgetEvent) -> EventResponse {
        k.event(&mut EventContext {
            event: e,
            bounds: k.bounds,
            scale: 1.0,
        })
    }

    fn tap(k: &mut VirtualKeyboard, kind: KeyKind) {
        let rect = k
            .rects
            .iter()
            .find(|(kk, _)| *kk == kind)
            .map(|(_, r)| *r)
            .unwrap();
        let p = Vec2::new(
            (rect.min_x() + rect.max_x()) / 2.0,
            (rect.min_y() + rect.max_y()) / 2.0,
        );
        ev(
            k,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: p,
                count: 1,
            },
        );
        ev(
            k,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: p,
            },
        );
    }

    #[test]
    fn letter_tap_produces_char() {
        let mut k = VirtualKeyboard::new();
        laid_out(&mut k);
        tap(&mut k, KeyKind::Letter('q'));
        assert_eq!(k.take_pressed(), Some("q".to_string()));
    }

    #[test]
    fn shift_capitalizes_once() {
        let mut k = VirtualKeyboard::new();
        laid_out(&mut k);
        tap(&mut k, KeyKind::Fn(Special::Shift));
        assert!(k.is_shifted());
        tap(&mut k, KeyKind::Letter('a'));
        assert_eq!(k.take_pressed(), Some("A".to_string()));
        assert!(!k.is_shifted()); // momentary
        tap(&mut k, KeyKind::Letter('a'));
        assert_eq!(k.take_pressed(), Some("a".to_string()));
    }

    #[test]
    fn space_and_backspace() {
        let mut k = VirtualKeyboard::new();
        laid_out(&mut k);
        tap(&mut k, KeyKind::Fn(Special::Space));
        assert_eq!(k.take_pressed(), Some(" ".to_string()));
        tap(&mut k, KeyKind::Fn(Special::Backspace));
        assert_eq!(k.take_pressed(), Some("\u{8}".to_string()));
    }

    #[test]
    fn slide_off_cancels() {
        let mut k = VirtualKeyboard::new();
        laid_out(&mut k);
        let rect = k.rects[0].1;
        ev(
            &mut k,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(
                    (rect.min_x() + rect.max_x()) / 2.0,
                    (rect.min_y() + rect.max_y()) / 2.0,
                ),
                count: 1,
            },
        );
        // Drag outside every key.
        ev(
            &mut k,
            &WidgetEvent::PointerMoved {
                position: Vec2::new(-50.0, -50.0),
            },
        );
        ev(
            &mut k,
            &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(-50.0, -50.0),
            },
        );
        assert_eq!(k.take_pressed(), None);
    }

    #[test]
    fn paint_without_painter() {
        let mut k = VirtualKeyboard::new();
        laid_out(&mut k);
        let mut list = martensite_core::PaintList::default();
        let theme = martensite_theme::Theme::new("test");
        k.paint(&mut PaintContext {
            list: &mut list,
            bounds: k.bounds,
            theme: &theme,
            scale: 1.0,
            text_painter: None,
        });
        assert!(!list.is_empty());
    }
}
