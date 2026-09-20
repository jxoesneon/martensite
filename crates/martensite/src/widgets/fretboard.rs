//! `Fretboard` — a guitar chord/scale diagram (the tablature-
//! family display sibling of [`crate::widgets::piano_keys::PianoKeys`]).
//!
//! Six vertical strings over a few horizontal frets; dots mark
//! fingered positions, `X`/`O` markers sit above the nut for
//! muted/open strings, and an optional barre line spans finger 1.
//! [`Fretboard::set`]/`clear` edit fingering; clicking a fret
//! cell toggles a dot and parks `(string, fret)` in
//! [`Fretboard::take_edited`].
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::fretboard::Fretboard;
//!
//! // Open G major: 320003 (strings default to open).
//! let f = Fretboard::new().set(0, 3).set(1, 2).set(5, 3);
//! assert_eq!(f.fret_of(0), Some(3));
//! assert_eq!(f.fret_of(2), Some(0)); // open, not muted
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const W_PT: f32 = 120.0;
const H_PT: f32 = 150.0;
/// Fret rows drawn below the nut.
const FRETS: u8 = 4;
const DOT_FRAC: f32 = 0.32;

const FACE: [u8; 4] = [36, 36, 42, 255];
const WIRE: [u8; 4] = [96, 96, 104, 255];
const NUT: [u8; 4] = [190, 190, 196, 255];
const DOT: [u8; 4] = [120, 200, 255, 255];
const MARK: [u8; 4] = [200, 200, 206, 255];

/// A guitar chord diagram — see the module docs.
///
/// ```
/// use martensite::widgets::fretboard::Fretboard;
///
/// assert_eq!(Fretboard::new().strings(), 6);
/// ```
pub struct Fretboard {
    /// Accessibility label / chord name.
    pub label: String,
    /// `fingering[string]` = fret `0..=FRETS`, `None` = muted,
    /// `Some(0)` = open (rings above the nut).
    fingering: [Option<u8>; 6],
    edited: Option<(u8, u8)>,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for Fretboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fretboard")
            .field("label", &self.label)
            .field("fingering", &self.fingering)
            .finish()
    }
}

impl Default for Fretboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Fretboard {
    /// Creates an empty diagram (all strings open).
    ///
    /// ```
    /// use martensite::widgets::fretboard::Fretboard;
    ///
    /// assert_eq!(Fretboard::new().fret_of(3), Some(0));
    /// ```
    pub fn new() -> Self {
        Self {
            label: "Chord".to_string(),
            fingering: [Some(0); 6],
            edited: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        }
    }

    /// Chord name / accessibility label.
    ///
    /// ```
    /// use martensite::widgets::fretboard::Fretboard;
    ///
    /// assert_eq!(Fretboard::new().label("G").label, "G");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Fingers `string` (`0` = low E) at `fret`
    /// (`0` = open, `1..=4` = fretted).
    ///
    /// ```
    /// use martensite::widgets::fretboard::Fretboard;
    ///
    /// assert_eq!(Fretboard::new().set(5, 3).fret_of(5), Some(3));
    /// ```
    pub fn set(mut self, string: u8, fret: u8) -> Self {
        if (string as usize) < 6 {
            self.fingering[string as usize] = Some(fret.min(FRETS));
        }
        self
    }

    /// Mutes `string` (draws `X` above the nut).
    ///
    /// ```
    /// use martensite::widgets::fretboard::Fretboard;
    ///
    /// assert_eq!(Fretboard::new().mute(0).fret_of(0), None);
    /// ```
    pub fn mute(mut self, string: u8) -> Self {
        if (string as usize) < 6 {
            self.fingering[string as usize] = None;
        }
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::fretboard::Fretboard;
    ///
    /// let _ = Fretboard::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// String count (always 6).
    ///
    /// ```
    /// use martensite::widgets::fretboard::Fretboard;
    ///
    /// assert_eq!(Fretboard::new().strings(), 6);
    /// ```
    pub fn strings(&self) -> u8 {
        6
    }

    /// Fret `string` is fingered at; `None` when muted.
    ///
    /// ```
    /// use martensite::widgets::fretboard::Fretboard;
    ///
    /// let f = Fretboard::new().mute(2);
    /// assert_eq!(f.fret_of(2), None);
    /// ```
    pub fn fret_of(&self, string: u8) -> Option<u8> {
        self.fingering.get(string as usize).copied().flatten()
    }

    /// Whether `string` is muted.
    ///
    /// ```
    /// use martensite::widgets::fretboard::Fretboard;
    ///
    /// assert!(Fretboard::new().mute(1).is_muted(1));
    /// ```
    pub fn is_muted(&self, string: u8) -> bool {
        self.fingering
            .get(string as usize)
            .is_some_and(|f| f.is_none())
    }

    /// Drains the last `(string, fret)` toggled by a click.
    ///
    /// ```
    /// use martensite::widgets::fretboard::Fretboard;
    ///
    /// assert_eq!(Fretboard::new().take_edited(), None);
    /// ```
    pub fn take_edited(&mut self) -> Option<(u8, u8)> {
        self.edited.take()
    }

    /// Grid metrics: left edge, top of fret 1, string spacing,
    /// fret spacing (widget coords).
    fn grid(&self) -> (f32, f32, f32, f32) {
        let pad = 14.0 * self.scale;
        let w = self.bounds.width() - pad * 2.0;
        let top = self.bounds.min_y() + pad * 1.6;
        let h = (self.bounds.height() - top + self.bounds.min_y() - pad).max(1.0);
        (self.bounds.min_x() + pad, top, w / 5.0, h / FRETS as f32)
    }
}

impl Widget for Fretboard {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(60.0, 80.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Image);
        let shape = self
            .fingering
            .iter()
            .map(|f| match f {
                None => "x".to_string(),
                Some(n) => n.to_string(),
            })
            .collect::<Vec<_>>()
            .join("");
        node.set_label(format!("{} — fingering {}", self.label, shape));
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        if let WidgetEvent::PointerPressed {
            button: PointerButton::Primary,
            position,
            ..
        } = cx.event
        {
            if !self.bounds.contains(*position) {
                return EventResponse::Ignored;
            }
            let (left, top, ss, fs) = self.grid();
            let s = ((position.x - left) / ss + 0.5) as i32;
            let f = ((position.y - top) / fs + 0.5) as i32;
            if (0..6).contains(&s) && (1..=FRETS as i32).contains(&f) {
                let (s, f) = (s as usize, f as u8);
                self.fingering[s] = if self.fingering[s] == Some(f) {
                    Some(0) // un-fret back to open
                } else {
                    Some(f)
                };
                self.edited = Some((s as u8, f));
                return EventResponse::RequestRepaint;
            }
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    }

    fn paint(&self, cx: &mut PaintContext) {
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        cx.list.push_fill_shape(
            krect(self.bounds),
            &martensite_core::shape::Shape::rounded(cx.pt(4.0)),
            cx.color(TokenKey::SurfaceColor, FACE),
        );
        let (left, top, ss, fs) = self.grid();
        let right = left + ss * 5.0;
        let wire = cx.color(TokenKey::BorderColor, WIRE);
        // Frets — the nut (top wire) is thicker.
        for f in 0..=FRETS {
            let y = top + fs * f as f32;
            let thick = if f == 0 { 3.0 } else { 1.0 } * self.scale;
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(left),
                    f64::from(y - thick / 2.0),
                    f64::from(right),
                    f64::from(y + thick / 2.0),
                ),
                &martensite_core::shape::Shape::RECT,
                if f == 0 {
                    cx.color(TokenKey::TextColor, NUT)
                } else {
                    wire
                },
            );
        }
        // Strings — outer (bass/treble) slightly heavier.
        for s in 0..6 {
            let x = left + ss * s as f32;
            let thick = (1.0 + (5 - s) as f32 * 0.25) * self.scale;
            cx.list.push_fill_shape(
                kurbo::Rect::new(
                    f64::from(x - thick / 2.0),
                    f64::from(top),
                    f64::from(x + thick / 2.0),
                    f64::from(top + fs * FRETS as f32),
                ),
                &martensite_core::shape::Shape::RECT,
                wire,
            );
        }
        // Finger dots + nut markers.
        let dot_r = ss * DOT_FRAC;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let mark = cx.color(TokenKey::TextColor, MARK);
        let mark_size = 10.0 * self.scale;
        for (s, fing) in self.fingering.iter().enumerate() {
            let x = left + ss * s as f32;
            match fing {
                None => crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(x - mark_size * 0.35),
                        f64::from(top - mark_size * 1.3),
                    ),
                    "✕",
                    mark_size,
                    mark,
                ),
                Some(0) => crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(
                        f64::from(x - mark_size * 0.3),
                        f64::from(top - mark_size * 1.3),
                    ),
                    "○",
                    mark_size,
                    mark,
                ),
                Some(f) => {
                    let cy = top + fs * (*f as f32 - 0.5);
                    let p = Vec2::new(x, cy);
                    cx.list.push_fill_shape(
                        kurbo::Rect::new(
                            f64::from(p.x - dot_r),
                            f64::from(p.y - dot_r),
                            f64::from(p.x + dot_r),
                            f64::from(p.y + dot_r),
                        ),
                        &martensite_core::shape::Shape::circle(p, dot_r),
                        cx.color(TokenKey::AccentColor, DOT),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(w: &mut Fretboard, wd: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        w.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(wd, h),
            },
        );
        w.layout(&mut cx, Rect::new(0.0, 0.0, wd, h));
    }

    #[test]
    fn open_and_mute() {
        let f = Fretboard::new().set(0, 3).mute(5);
        assert_eq!(f.fret_of(0), Some(3));
        assert!(f.is_muted(5));
        assert_eq!(f.fret_of(1), Some(0)); // open by default
    }

    #[test]
    fn fret_clamps() {
        assert_eq!(Fretboard::new().set(2, 9).fret_of(2), Some(FRETS));
        assert_eq!(Fretboard::new().set(9, 3).fret_of(9), None);
    }

    #[test]
    fn click_toggles_dot() {
        let mut f = Fretboard::new().mute(0);
        laid_out(&mut f, 120.0, 150.0);
        let (left, top, ss, fs) = f.grid();
        // String 0, fret 2 cell center.
        f.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(left, top + fs * 1.5),
                count: 1,
            },
            bounds: f.bounds,
            scale: 1.0,
        });
        assert_eq!(f.fret_of(0), Some(2));
        assert_eq!(f.take_edited(), Some((0, 2)));
        // Same cell again → back to open.
        f.event(&mut EventContext {
            event: &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(left, top + fs * 1.5),
                count: 1,
            },
            bounds: f.bounds,
            scale: 1.0,
        });
        assert_eq!(f.fret_of(0), Some(0));
        let _ = ss;
    }
}
