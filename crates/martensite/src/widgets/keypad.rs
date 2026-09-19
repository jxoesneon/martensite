//! `Keypad` — the telephony 3×4 digit pad (POS / dialer idiom).
//!
//! Keys `1`–`9`, `*`, `0`, `#` paint as rounded cells in a 3×4
//! grid; pressing a cell (or the matching keyboard key) parks the
//! character in [`Keypad::take_pressed`]. `Backspace` parks `'\x08'`
//! for the standard delete idiom.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::keypad::Keypad;
//!
//! let pad = Keypad::new();
//! assert_eq!(pad.key_count(), 12);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

use crate::text_paint::SharedTextPainter;

const CELL_PT: f32 = 56.0;
const GAP_PT: f32 = 4.0;
const KEYS: [char; 12] = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '*', '0', '#'];

const FACE: [u8; 4] = [58, 58, 64, 255];
const DOWN: [u8; 4] = [80, 80, 88, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];
const FG: [u8; 4] = [230, 230, 235, 255];
const MUTED: [u8; 4] = [140, 140, 148, 255];

/// A telephony digit pad — see the module docs.
///
/// ```
/// use martensite::widgets::keypad::Keypad;
///
/// assert_eq!(Keypad::new().key_count(), 12);
/// ```
pub struct Keypad {
    /// When `false` presses are ignored.
    pub enabled: bool,
    /// Accessibility label.
    pub label: String,
    pressed: Option<usize>,
    pending: Option<char>,
    hovered: Option<usize>,
    bounds: Rect,
    text_painter: Option<SharedTextPainter>,
}

impl Default for Keypad {
    fn default() -> Self {
        Self::new()
    }
}

impl Keypad {
    /// Creates a 12-key pad.
    ///
    /// ```
    /// use martensite::widgets::keypad::Keypad;
    ///
    /// assert_eq!(Keypad::new().key_count(), 12);
    /// ```
    pub fn new() -> Self {
        Self {
            enabled: true,
            label: "Keypad".to_string(),
            pressed: None,
            pending: None,
            hovered: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::keypad::Keypad;
    ///
    /// let k = Keypad::new().label("Dial");
    /// assert_eq!(k.label, "Dial");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Enables or disables the pad.
    ///
    /// ```
    /// use martensite::widgets::keypad::Keypad;
    ///
    /// assert!(!Keypad::new().enabled(false).enabled);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Explicit painter override — see [`crate::text_paint`].
    ///
    /// ```
    /// use martensite::widgets::keypad::Keypad;
    /// use martensite::text_paint::shared_painter;
    ///
    /// let k = Keypad::new().with_text_painter(shared_painter());
    /// assert_eq!(k.key_count(), 12);
    /// ```
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Key count (always 12).
    ///
    /// ```
    /// use martensite::widgets::keypad::Keypad;
    ///
    /// assert_eq!(Keypad::new().key_count(), 12);
    /// ```
    pub fn key_count(&self) -> usize {
        KEYS.len()
    }

    /// Drains the pressed key character since the last drain.
    ///
    /// ```
    /// use martensite::widgets::keypad::Keypad;
    ///
    /// let mut k = Keypad::new();
    /// assert!(k.take_pressed().is_none());
    /// ```
    pub fn take_pressed(&mut self) -> Option<char> {
        self.pending.take()
    }

    /// Cell rect for key `i` (row-major, 3 columns).
    fn cell(&self, i: usize, scale: f32) -> Rect {
        let gap = GAP_PT * scale;
        let cell_w = (self.bounds.width() - 2.0 * gap) / 3.0;
        let cell_h = (self.bounds.height() - 3.0 * gap) / 4.0;
        let (col, row) = (i % 3, i / 3);
        Rect::new(
            self.bounds.min_x() + col as f32 * (cell_w + gap),
            self.bounds.min_y() + row as f32 * (cell_h + gap),
            cell_w,
            cell_h,
        )
    }

    /// Key index at a device-space point.
    fn key_at(&self, p: Vec2, scale: f32) -> Option<usize> {
        (0..KEYS.len()).find(|&i| self.cell(i, scale).contains(p))
    }
}

impl Widget for Keypad {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(CELL_PT * 3.0).min(constraints.max_size.x.max(0.0)),
            cx.pt(CELL_PT * 4.0).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 80.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, _cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(self.label.clone());
        if !self.enabled {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(i) = self.key_at(*position, cx.scale) {
                    self.pressed = Some(i);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                let hit = self.key_at(*position, cx.scale);
                if hit != self.hovered {
                    self.hovered = hit;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position,
            } => {
                if let Some(i) = self.pressed.take() {
                    if self.key_at(*position, cx.scale) == Some(i) {
                        self.pending = Some(KEYS[i]);
                    }
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => {
                let c = key.chars().next().unwrap_or('\0');
                if KEYS.contains(&c) || c == '\u{8}' {
                    self.pending = Some(if c == '\u{8}' { '\x08' } else { c });
                    return EventResponse::RequestRepaint;
                }
                if key == "Backspace" {
                    self.pending = Some('\x08');
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let f = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let face = cx.color(TokenKey::SurfaceColor, FACE);
        let down = cx.color(TokenKey::BorderColor, DOWN);
        let edge = cx.color(TokenKey::BorderColor, EDGE);
        let fg = cx.color(TokenKey::TextColor, FG);
        let muted = cx.color(TokenKey::TextMutedColor, MUTED);
        let size = 15.0 * cx.scale;
        for (i, &key) in KEYS.iter().enumerate() {
            let cell = self.cell(i, cx.scale);
            let shape = martensite_core::shape::Shape::rounded(cx.pt(4.0));
            let fill = if self.pressed == Some(i) {
                down
            } else if self.hovered == Some(i) {
                [
                    face[0].saturating_add(14),
                    face[1].saturating_add(14),
                    face[2].saturating_add(14),
                    255,
                ]
            } else {
                face
            };
            cx.list.push_fill_shape(f(cell), &shape, fill);
            cx.list.push_stroke_shape(f(cell), &shape, cx.pt(0.5), edge);
            let s = key.to_string();
            let w = painter
                .and_then(|p| p.measure_text(&s, size))
                .unwrap_or(size * 0.6);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                f(cell),
                kurbo::Point::new(
                    f64::from(cell.min_x() + (cell.width() - w) / 2.0),
                    f64::from(cell.min_y() + (cell.height() - size * 1.2) / 2.0),
                ),
                &s,
                size,
                if self.enabled { fg } else { muted },
            );
        }
    }
}

impl std::fmt::Debug for Keypad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keypad")
            .field("pressed", &self.pressed)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(k: &mut Keypad, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        k.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        k.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn click(k: &mut Keypad, p: Vec2) {
        k.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: p,
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 180.0, 240.0),
            scale: 1.0,
        });
        k.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: p,
            },
            bounds: Rect::new(0.0, 0.0, 180.0, 240.0),
            scale: 1.0,
        });
    }

    #[test]
    fn click_emits_key() {
        let mut k = Keypad::new();
        laid_out(&mut k, 180.0, 240.0);
        // Top-left cell is '1'.
        click(&mut k, Vec2::new(10.0, 10.0));
        assert_eq!(k.take_pressed(), Some('1'));
        assert!(k.take_pressed().is_none());
    }

    #[test]
    fn bottom_row_keys() {
        let mut k = Keypad::new();
        laid_out(&mut k, 180.0, 240.0);
        // Bottom-left is '*'.
        click(&mut k, Vec2::new(10.0, 230.0));
        assert_eq!(k.take_pressed(), Some('*'));
    }

    #[test]
    fn release_off_cell_cancels() {
        let mut k = Keypad::new();
        laid_out(&mut k, 180.0, 240.0);
        k.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(10.0, 10.0),
                count: 1,
            },
            bounds: Rect::new(0.0, 0.0, 180.0, 240.0),
            scale: 1.0,
        });
        k.event(&mut EventContext {
            event: &WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                position: Vec2::new(400.0, 400.0),
            },
            bounds: Rect::new(0.0, 0.0, 180.0, 240.0),
            scale: 1.0,
        });
        assert!(k.take_pressed().is_none());
    }

    #[test]
    fn keyboard_digits() {
        let mut k = Keypad::new();
        laid_out(&mut k, 180.0, 240.0);
        for (key, want) in [("5", '5'), ("Backspace", '\x08')] {
            k.event(&mut EventContext {
                event: &WidgetEvent::KeyPressed {
                    key: key.to_string(),
                    repeat: false,
                },
                bounds: Rect::new(0.0, 0.0, 180.0, 240.0),
                scale: 1.0,
            });
            assert_eq!(k.take_pressed(), Some(want));
        }
    }

    #[test]
    fn disabled_inert() {
        let mut k = Keypad::new().enabled(false);
        laid_out(&mut k, 180.0, 240.0);
        click(&mut k, Vec2::new(10.0, 10.0));
        assert!(k.take_pressed().is_none());
    }
}
