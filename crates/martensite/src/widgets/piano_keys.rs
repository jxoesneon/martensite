//! `PianoKeys` — a musical keyboard strip (DAW piano-roll /
//! MIDI-input idiom).
//!
//! `octaves` white keys (`C D E F G A B` per octave) with overlay
//! black keys at `C# D# F# G# A#`. Clicking or dragging across
//! keys parks the struck semitone index (`0` = lowest C) in
//! [`PianoKeys::take_struck`]; held keys highlight. Octave `4`
//! middle-C is index `48`.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::piano_keys::PianoKeys;
//!
//! let p = PianoKeys::new().octaves(2);
//! assert_eq!(p.note_count(), 24);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const HEIGHT_PT: f32 = 64.0;
const BLACK_W: f32 = 0.62; // fraction of a white key's width
const BLACK_H: f32 = 0.6; // fraction of total height

const WHITE: [u8; 4] = [235, 235, 238, 255];
const WHITE_HOT: [u8; 4] = [150, 190, 240, 255];
const BLACK: [u8; 4] = [30, 30, 34, 255];
const BLACK_HOT: [u8; 4] = [80, 120, 190, 255];
const EDGE: [u8; 4] = [70, 70, 76, 255];

/// A musical keyboard strip — see the module docs.
///
/// ```
/// use martensite::widgets::piano_keys::PianoKeys;
///
/// assert_eq!(PianoKeys::new().note_count(), 12);
/// ```
#[derive(Debug)]
pub struct PianoKeys {
    /// Accessibility label.
    pub label: String,
    octaves: usize,
    held: Option<usize>,
    pending: Option<usize>,
    bounds: Rect,
    scale: f32,
}

impl Default for PianoKeys {
    fn default() -> Self {
        Self::new()
    }
}

impl PianoKeys {
    /// Creates a single-octave keyboard.
    ///
    /// ```
    /// use martensite::widgets::piano_keys::PianoKeys;
    ///
    /// assert_eq!(PianoKeys::new().octave_count(), 1);
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Piano".to_string(),
            octaves: 1,
            held: None,
            pending: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        }
    }

    /// Number of octaves (1–10 clamped).
    ///
    /// ```
    /// use martensite::widgets::piano_keys::PianoKeys;
    ///
    /// assert_eq!(PianoKeys::new().octaves(88).octave_count(), 10);
    /// ```
    pub fn octaves(mut self, octaves: usize) -> Self {
        self.octaves = octaves.clamp(1, 10);
        self
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::piano_keys::PianoKeys;
    ///
    /// assert_eq!(PianoKeys::new().label("Keys").label, "Keys");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Octave count.
    ///
    /// ```
    /// use martensite::widgets::piano_keys::PianoKeys;
    ///
    /// assert_eq!(PianoKeys::new().octave_count(), 1);
    /// ```
    pub fn octave_count(&self) -> usize {
        self.octaves
    }

    /// Total semitone count (12 per octave).
    ///
    /// ```
    /// use martensite::widgets::piano_keys::PianoKeys;
    ///
    /// assert_eq!(PianoKeys::new().octaves(2).note_count(), 24);
    /// ```
    pub fn note_count(&self) -> usize {
        self.octaves * 12
    }

    /// The currently held note, if any.
    ///
    /// ```
    /// use martensite::widgets::piano_keys::PianoKeys;
    ///
    /// assert_eq!(PianoKeys::new().held(), None);
    /// ```
    pub fn held(&self) -> Option<usize> {
        self.held
    }

    /// Drains the last struck semitone index.
    ///
    /// ```
    /// use martensite::widgets::piano_keys::PianoKeys;
    ///
    /// let mut p = PianoKeys::new();
    /// assert!(p.take_struck().is_none());
    /// ```
    pub fn take_struck(&mut self) -> Option<usize> {
        self.pending.take()
    }

    /// White-key slot count.
    fn white_count(&self) -> usize {
        self.octaves * 7
    }

    /// White-key width in px.
    fn white_w(&self) -> f32 {
        self.bounds.width() / self.white_count().max(1) as f32
    }

    /// Semitone at a point — black keys hit-test first.
    fn note_at(&self, p: Vec2) -> Option<usize> {
        if !self.bounds.contains(p) {
            return None;
        }
        let ww = self.white_w();
        let bw = ww * BLACK_W;
        let bh = self.bounds.height() * BLACK_H;
        // Black keys sit after white indices 0,1,3,4,5 (C#,D#,F#,G#,A#).
        const BLACK_AFTER: [usize; 5] = [0, 1, 3, 4, 5];
        if p.y - self.bounds.min_y() <= bh {
            let widx = ((p.x - self.bounds.min_x()) / ww) as usize;
            // A boundary sits between whites w-1 and w; the point may
            // fall in either neighboring white slot.
            for w in [widx.wrapping_sub(1), widx] {
                if w >= self.white_count() {
                    continue;
                }
                if let Some(slot) = BLACK_AFTER.iter().position(|&a| a == w % 7) {
                    let center = self.bounds.min_x() + (w + 1) as f32 * ww;
                    if (p.x - center).abs() <= bw / 2.0 {
                        let semi = [1, 3, 6, 8, 10][slot];
                        return Some((w / 7) * 12 + semi);
                    }
                }
            }
        }
        // White key.
        let widx =
            (((p.x - self.bounds.min_x()) / ww) as usize).min(self.white_count().saturating_sub(1));
        const WHITE_SEMI: [usize; 7] = [0, 2, 4, 5, 7, 9, 11];
        Some((widx / 7) * 12 + WHITE_SEMI[widx % 7])
    }
}

impl Widget for PianoKeys {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(self.white_count() as f32 * 24.0)
                .min(constraints.max_size.x.max(0.0)),
            cx.pt(HEIGHT_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 24.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Group);
        node.set_label(format!("{} — {} notes", self.label, self.note_count()));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                if let Some(n) = self.note_at(*position) {
                    self.held = Some(n);
                    self.pending = Some(n);
                    return EventResponse::CapturePointer;
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerMoved { position } => {
                if self.held.is_none() {
                    return EventResponse::Ignored;
                }
                let n = self.note_at(*position);
                if n != self.held && n.is_some() {
                    self.held = n;
                    self.pending = n;
                    return EventResponse::RequestRepaint;
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerReleased {
                button: PointerButton::Primary,
                ..
            } => {
                if self.held.take().is_some() {
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
        let ww = self.white_w();
        const WHITE_SEMI: [usize; 7] = [0, 2, 4, 5, 7, 9, 11];
        // White keys.
        for w in 0..self.white_count() {
            let note = (w / 7) * 12 + WHITE_SEMI[w % 7];
            let r = Rect::new(
                self.bounds.min_x() + w as f32 * ww,
                self.bounds.min_y(),
                ww,
                self.bounds.height(),
            );
            cx.list.push_fill_shape(
                f(r),
                &martensite_core::shape::Shape::rounded(cx.pt(2.0)),
                if self.held == Some(note) {
                    cx.color(TokenKey::AccentColor, WHITE_HOT)
                } else {
                    cx.color(TokenKey::SurfaceColor, WHITE)
                },
            );
            cx.list.push_stroke_shape(
                f(r),
                &martensite_core::shape::Shape::rounded(cx.pt(2.0)),
                cx.pt(0.5),
                cx.color(TokenKey::BorderColor, EDGE),
            );
        }
        // Black keys.
        let bw = ww * BLACK_W;
        let bh = self.bounds.height() * BLACK_H;
        const BLACK_AFTER: [usize; 5] = [0, 1, 3, 4, 5];
        const BLACK_SEMI: [usize; 5] = [1, 3, 6, 8, 10];
        for w in 0..self.white_count() {
            if let Some(slot) = BLACK_AFTER.iter().position(|&a| a == w % 7) {
                // Last white key of the last octave gets no black key.
                if w + 1 >= self.white_count() {
                    break;
                }
                let note = (w / 7) * 12 + BLACK_SEMI[slot];
                let r = Rect::new(
                    self.bounds.min_x() + (w + 1) as f32 * ww - bw / 2.0,
                    self.bounds.min_y(),
                    bw,
                    bh,
                );
                cx.list.push_fill_shape(
                    f(r),
                    &martensite_core::shape::Shape::rounded(cx.pt(1.5)),
                    if self.held == Some(note) {
                        cx.color(TokenKey::AccentColor, BLACK_HOT)
                    } else {
                        cx.color(TokenKey::TextColor, BLACK)
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(p: &mut PianoKeys, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        p.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        p.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev(p: &mut PianoKeys, e: WidgetEvent) {
        p.event(&mut EventContext {
            event: &e,
            bounds: Rect::new(0.0, 0.0, 168.0, 64.0), // 7 whites × 24px
            scale: 1.0,
        });
    }

    #[test]
    fn semitone_count() {
        assert_eq!(PianoKeys::new().octaves(2).note_count(), 24);
    }

    #[test]
    fn white_key_strikes() {
        let mut p = PianoKeys::new();
        laid_out(&mut p, 168.0, 64.0);
        ev(
            &mut p,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(36.0, 50.0), // second white key → D = 2
                count: 1,
            },
        );
        assert_eq!(p.take_struck(), Some(2));
    }

    #[test]
    fn black_key_strikes() {
        let mut p = PianoKeys::new();
        laid_out(&mut p, 168.0, 64.0);
        ev(
            &mut p,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(24.0, 10.0), // boundary of C/D high → C# = 1
                count: 1,
            },
        );
        assert_eq!(p.take_struck(), Some(1));
    }

    #[test]
    fn drag_glissando() {
        let mut p = PianoKeys::new();
        laid_out(&mut p, 168.0, 64.0);
        ev(
            &mut p,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(12.0, 50.0),
                count: 1,
            },
        );
        assert_eq!(p.take_struck(), Some(0));
        ev(
            &mut p,
            WidgetEvent::PointerMoved {
                position: Vec2::new(84.0, 50.0), // 4th white → F = 5
            },
        );
        assert_eq!(p.take_struck(), Some(5));
    }

    #[test]
    fn release_clears_held() {
        let mut p = PianoKeys::new();
        laid_out(&mut p, 168.0, 64.0);
        ev(
            &mut p,
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(12.0, 50.0),
                count: 1,
            },
        );
        ev(
            &mut p,
            WidgetEvent::PointerReleased {
                position: Vec2::new(12.0, 50.0),
                button: PointerButton::Primary,
            },
        );
        assert_eq!(p.held(), None);
    }
}
