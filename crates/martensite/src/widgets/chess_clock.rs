//! `ChessClock` — a dual-sided game clock (FIDE / chess-app idiom).
//!
//! Two countdown faces; tapping a face (or Space) presses that
//! side's plunger — the presser's clock stops and the opponent's
//! starts, parking a move count in [`ChessClock::take_pressed`].
//! `tick` decrements the running side with fractional precision;
//! reaching zero parks the side in [`ChessClock::take_flagged`]
//! ("flag fall"). `r` resets both clocks.
//!
//! Companion to [`ChessBoard`](crate::widgets::ChessBoard).
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::chess_clock::ChessClock;
//! use std::time::Duration;
//!
//! let c = ChessClock::new(Duration::from_secs(300));
//! assert_eq!(c.moves(), 0);
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;
use std::time::Duration;

use crate::text_paint::SharedTextPainter;

const W_PT: f32 = 240.0;
const H_PT: f32 = 72.0;
const FONT_PT: f32 = 24.0;

const FACE: [u8; 4] = [52, 54, 62, 255];
const FACE_ACTIVE: [u8; 4] = [40, 60, 48, 255];
const FACE_FLAGGED: [u8; 4] = [70, 40, 42, 255];
const EDGE: [u8; 4] = [90, 92, 100, 255];
const FG: [u8; 4] = [230, 232, 238, 255];
const DIM: [u8; 4] = [130, 132, 140, 255];
const GOOD: [u8; 4] = [74, 222, 128, 255];
const BAD: [u8; 4] = [248, 113, 113, 255];

/// Which side of the clock — White on the left, Black on the right.
///
/// ```
/// use martensite::widgets::chess_clock::ClockSide;
///
/// assert_eq!(ClockSide::White.other(), ClockSide::Black);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockSide {
    /// Left face.
    White,
    /// Right face.
    Black,
}

impl ClockSide {
    /// The opposing side.
    ///
    /// ```
    /// use martensite::widgets::chess_clock::ClockSide;
    ///
    /// assert_eq!(ClockSide::Black.other(), ClockSide::White);
    /// ```
    pub fn other(self) -> Self {
        match self {
            Self::White => Self::Black,
            Self::Black => Self::White,
        }
    }

    /// Face index.
    fn index(self) -> usize {
        match self {
            Self::White => 0,
            Self::Black => 1,
        }
    }

    /// Display name.
    fn name(self) -> &'static str {
        match self {
            Self::White => "White",
            Self::Black => "Black",
        }
    }
}

/// A dual game clock — see the module docs.
///
/// ```
/// use martensite::widgets::chess_clock::ChessClock;
/// use std::time::Duration;
///
/// assert_eq!(ChessClock::new(Duration::from_secs(60)).moves(), 0);
/// ```
pub struct ChessClock {
    /// Accessibility label.
    pub label: String,
    /// Per-side fractional seconds remaining `[white, black]`.
    time: [f32; 2],
    /// Initial per-side seconds (reset target).
    initial: Duration,
    /// The currently running side (`None` = paused).
    running: Option<ClockSide>,
    /// Plunger presses (completed moves).
    moves: u32,
    /// Side that flagged, parked for the host.
    flagged: Option<ClockSide>,
    /// Side last pressed, parked for the host.
    pressed: Option<ClockSide>,
    /// Increment added to the presser's clock per move (Fischer).
    pub increment: Duration,
    bounds: Rect,
    scale: f32,
    enabled: bool,
    text_painter: Option<SharedTextPainter>,
}

impl std::fmt::Debug for ChessClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChessClock")
            .field("running", &self.running)
            .field("moves", &self.moves)
            .finish()
    }
}

impl ChessClock {
    /// Both clocks at `per_side`, paused — the first tap starts the
    /// opponent's clock (the plunger convention).
    ///
    /// ```
    /// use martensite::widgets::chess_clock::{ChessClock, ClockSide};
    /// use std::time::Duration;
    ///
    /// let c = ChessClock::new(Duration::from_secs(300));
    /// assert_eq!(c.remaining(ClockSide::White).as_secs(), 300);
    /// ```
    pub fn new(per_side: Duration) -> Self {
        let secs = per_side.as_secs_f32();
        Self {
            label: "Game clock".to_string(),
            time: [secs, secs],
            initial: per_side,
            running: None,
            moves: 0,
            flagged: None,
            pressed: None,
            increment: Duration::ZERO,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            enabled: true,
            text_painter: None,
        }
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::chess_clock::{ChessClock, ClockSide};
    /// use std::time::Duration;
    ///
    /// assert_eq!(ChessClock::new(Duration::from_secs(1)).label("Blitz").label, "Blitz");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Fischer increment added to a presser's clock per move.
    ///
    /// ```
    /// use martensite::widgets::chess_clock::{ChessClock, ClockSide};
    /// use std::time::Duration;
    ///
    /// let c = ChessClock::new(Duration::from_secs(60)).increment(Duration::from_secs(2));
    /// assert_eq!(c.increment, Duration::from_secs(2));
    /// ```
    pub fn increment(mut self, increment: Duration) -> Self {
        self.increment = increment;
        self
    }

    /// Disabled builder.
    ///
    /// ```
    /// use martensite::widgets::chess_clock::{ChessClock, ClockSide};
    /// use std::time::Duration;
    ///
    /// assert!(!ChessClock::new(Duration::from_secs(1)).enabled(false).is_enabled());
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Optional painter override (tests / headless).
    pub fn with_text_painter(mut self, painter: SharedTextPainter) -> Self {
        self.text_painter = Some(painter);
        self
    }

    /// Whether the clock accepts input.
    ///
    /// ```
    /// use martensite::widgets::chess_clock::{ChessClock, ClockSide};
    /// use std::time::Duration;
    ///
    /// assert!(ChessClock::new(Duration::from_secs(1)).is_enabled());
    /// ```
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Remaining time on a face.
    ///
    /// ```
    /// use martensite::widgets::chess_clock::{ChessClock, ClockSide};
    /// use std::time::Duration;
    ///
    /// let c = ChessClock::new(Duration::from_secs(9));
    /// assert_eq!(c.remaining(ClockSide::Black).as_secs(), 9);
    /// ```
    pub fn remaining(&self, side: ClockSide) -> Duration {
        Duration::from_secs_f32(self.time[side.index()].max(0.0))
    }

    /// The running side, if any.
    ///
    /// ```
    /// use martensite::widgets::chess_clock::{ChessClock, ClockSide};
    /// use std::time::Duration;
    ///
    /// assert_eq!(ChessClock::new(Duration::from_secs(9)).running(), None);
    /// ```
    pub fn running(&self) -> Option<ClockSide> {
        self.running
    }

    /// Completed moves (plunger presses).
    ///
    /// ```
    /// use martensite::widgets::chess_clock::{ChessClock, ClockSide};
    /// use std::time::Duration;
    ///
    /// assert_eq!(ChessClock::new(Duration::from_secs(9)).moves(), 0);
    /// ```
    pub fn moves(&self) -> u32 {
        self.moves
    }

    /// Whether either side has flagged.
    ///
    /// ```
    /// use martensite::widgets::chess_clock::{ChessClock, ClockSide};
    /// use std::time::Duration;
    ///
    /// assert!(!ChessClock::new(Duration::from_secs(9)).is_flagged());
    /// ```
    pub fn is_flagged(&self) -> bool {
        self.time.iter().any(|t| *t <= 0.0)
    }

    /// Presses a side's plunger: that clock stops, the opponent's
    /// starts (plus any increment for the presser). No-op while
    /// flagged.
    ///
    /// ```
    /// use martensite::widgets::chess_clock::{ChessClock, ClockSide};
    /// use std::time::Duration;
    ///
    /// let mut c = ChessClock::new(Duration::from_secs(60));
    /// c.press(ClockSide::White);
    /// assert_eq!(c.running(), Some(ClockSide::Black));
    /// assert_eq!(c.moves(), 1);
    /// ```
    pub fn press(&mut self, side: ClockSide) {
        if !self.enabled || self.is_flagged() {
            return;
        }
        let i = side.index();
        self.time[i] += self.increment.as_secs_f32();
        self.running = Some(side.other());
        self.moves += 1;
        self.pressed = Some(side);
    }

    /// Pauses or resumes without switching sides.
    ///
    /// ```
    /// use martensite::widgets::chess_clock::{ChessClock, ClockSide};
    /// use std::time::Duration;
    ///
    /// let mut c = ChessClock::new(Duration::from_secs(60));
    /// c.press(ClockSide::White);
    /// c.pause();
    /// assert_eq!(c.running(), None);
    /// ```
    pub fn pause(&mut self) {
        self.running = None;
    }

    /// Resets both faces to the initial time and clears moves.
    ///
    /// ```
    /// use martensite::widgets::chess_clock::{ChessClock, ClockSide};
    /// use std::time::Duration;
    ///
    /// let mut c = ChessClock::new(Duration::from_secs(60));
    /// c.press(ClockSide::White);
    /// c.reset();
    /// assert_eq!((c.moves(), c.running()), (0, None));
    /// ```
    pub fn reset(&mut self) {
        let secs = self.initial.as_secs_f32();
        self.time = [secs, secs];
        self.running = None;
        self.moves = 0;
    }

    /// Drains the last pressed side.
    ///
    /// ```
    /// use martensite::widgets::chess_clock::{ChessClock, ClockSide};
    /// use std::time::Duration;
    ///
    /// assert_eq!(ChessClock::new(Duration::from_secs(9)).take_pressed(), None);
    /// ```
    pub fn take_pressed(&mut self) -> Option<ClockSide> {
        self.pressed.take()
    }

    /// Drains the flagged side (fires once).
    ///
    /// ```
    /// use martensite::widgets::chess_clock::{ChessClock, ClockSide};
    /// use std::time::Duration;
    ///
    /// assert_eq!(ChessClock::new(Duration::from_secs(9)).take_flagged(), None);
    /// ```
    pub fn take_flagged(&mut self) -> Option<ClockSide> {
        self.flagged.take()
    }

    /// `MM:SS` face text.
    fn face(&self, side: ClockSide) -> String {
        let secs = self.time[side.index()].ceil().max(0.0) as u64;
        format!("{}:{:02}", secs / 60, secs % 60)
    }

    /// Face rect (left or right half).
    fn face_rect(&self, side: ClockSide) -> Rect {
        let half = self.bounds.width() / 2.0;
        match side {
            ClockSide::White => Rect::new(
                self.bounds.min_x(),
                self.bounds.min_y(),
                half,
                self.bounds.height(),
            ),
            ClockSide::Black => Rect::new(
                self.bounds.min_x() + half,
                self.bounds.min_y(),
                half,
                self.bounds.height(),
            ),
        }
    }
}

impl Widget for ChessClock {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            cx.pt(W_PT).min(constraints.max_size.x.max(0.0)),
            cx.pt(H_PT).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 40.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Timer);
        node.set_label(format!(
            "{} — White {} Black {}",
            self.label,
            self.face(ClockSide::White),
            self.face(ClockSide::Black)
        ));
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
                for side in [ClockSide::White, ClockSide::Black] {
                    if self.face_rect(side).contains(*position) {
                        self.press(side);
                        return EventResponse::RequestRepaint;
                    }
                }
                EventResponse::Ignored
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                " " => {
                    // Space presses the running side's plunger (or
                    // White's when paused — the opening press).
                    self.press(self.running.unwrap_or(ClockSide::White));
                    EventResponse::RequestRepaint
                }
                "p" | "P" => {
                    self.pause();
                    EventResponse::RequestRepaint
                }
                "r" | "R" => {
                    self.reset();
                    EventResponse::RequestRepaint
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn tick(&mut self, dt: Duration) -> bool {
        let Some(side) = self.running else {
            return false;
        };
        if !self.enabled {
            return false;
        }
        let i = side.index();
        if self.time[i] <= 0.0 {
            return false;
        }
        self.time[i] = (self.time[i] - dt.as_secs_f32()).max(0.0);
        if self.time[i] <= 0.0 {
            self.flagged = Some(side);
            self.running = None;
        }
        true
    }

    fn paint(&self, cx: &mut PaintContext) {
        let s = cx.scale;
        let size = FONT_PT * s;
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        for side in [ClockSide::White, ClockSide::Black] {
            let rect = self.face_rect(side);
            let flagged = self.time[side.index()] <= 0.0;
            let active = self.running == Some(side);
            let fill = if flagged {
                cx.color(TokenKey::ErrorColor, FACE_FLAGGED)
            } else if active {
                cx.color(TokenKey::SuccessColor, FACE_ACTIVE)
            } else {
                cx.color(TokenKey::SurfaceColor, FACE)
            };
            let krect = kurbo::Rect::new(
                f64::from(rect.min_x()),
                f64::from(rect.min_y()),
                f64::from(rect.max_x()),
                f64::from(rect.max_y()),
            );
            cx.list.push_fill_rect(krect, fill);
            cx.list
                .push_stroke_rect(krect, s, cx.color(TokenKey::BorderColor, EDGE));

            // Face text.
            let face = self.face(side);
            let fg = if flagged {
                cx.color(TokenKey::ErrorColor, BAD)
            } else if active {
                cx.color(TokenKey::SuccessColor, GOOD)
            } else {
                cx.color(TokenKey::TextColor, FG)
            };
            let w = painter
                .and_then(|p| p.measure_text(&face, size))
                .unwrap_or(face.chars().count() as f32 * size * 0.55);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                krect,
                kurbo::Point::new(
                    f64::from(rect.min_x() + (rect.width() - w).max(0.0) / 2.0),
                    f64::from(rect.min_y() + (rect.height() - size * 1.2).max(0.0) / 2.0),
                ),
                &face,
                size,
                fg,
            );

            // Side tag under the time.
            let tag = side.name();
            let small = 9.0 * s;
            let tw = painter
                .and_then(|p| p.measure_text(tag, small))
                .unwrap_or(tag.chars().count() as f32 * small * 0.55);
            crate::text_paint::paint_label_clipped(
                painter,
                cx.list,
                krect,
                kurbo::Point::new(
                    f64::from(rect.min_x() + (rect.width() - tw).max(0.0) / 2.0),
                    f64::from(rect.max_y() - small * 1.6),
                ),
                tag,
                small,
                cx.color(TokenKey::TextMutedColor, DIM),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn laid_out(c: &mut ChessClock) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        c.layout(&mut cx, Rect::new(0.0, 0.0, 240.0, 72.0));
    }

    fn ev(c: &mut ChessClock, e: &WidgetEvent) -> EventResponse {
        c.event(&mut EventContext {
            event: e,
            bounds: c.bounds,
            scale: 1.0,
        })
    }

    #[test]
    fn press_alternates_sides() {
        let mut c = ChessClock::new(Duration::from_secs(60));
        laid_out(&mut c);
        c.press(ClockSide::White);
        assert_eq!(c.running(), Some(ClockSide::Black));
        assert_eq!(c.take_pressed(), Some(ClockSide::White));
        c.press(ClockSide::Black);
        assert_eq!(c.running(), Some(ClockSide::White));
        assert_eq!(c.moves(), 2);
    }

    #[test]
    fn tick_drains_running_side_only() {
        let mut c = ChessClock::new(Duration::from_secs(60));
        laid_out(&mut c);
        c.press(ClockSide::White); // Black runs.
        c.tick(Duration::from_secs(10));
        assert_eq!(c.remaining(ClockSide::Black).as_secs(), 50);
        assert_eq!(c.remaining(ClockSide::White).as_secs(), 60);
    }

    #[test]
    fn increment_adds_to_presser() {
        let mut c = ChessClock::new(Duration::from_secs(60)).increment(Duration::from_secs(2));
        laid_out(&mut c);
        c.press(ClockSide::White);
        assert_eq!(c.remaining(ClockSide::White).as_secs(), 62);
    }

    #[test]
    fn flag_fall_parks_side() {
        let mut c = ChessClock::new(Duration::from_secs(1));
        laid_out(&mut c);
        c.press(ClockSide::White); // Black runs.
        c.tick(Duration::from_secs(2));
        assert_eq!(c.take_flagged(), Some(ClockSide::Black));
        assert_eq!(c.take_flagged(), None);
        assert_eq!(c.running(), None);
        // Further presses are ignored.
        c.press(ClockSide::Black);
        assert_eq!(c.moves(), 1);
    }

    #[test]
    fn face_tap_presses() {
        let mut c = ChessClock::new(Duration::from_secs(60));
        laid_out(&mut c);
        ev(
            &mut c,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(180.0, 36.0), // right face = Black
                count: 1,
            },
        );
        assert_eq!(c.running(), Some(ClockSide::White));
    }

    #[test]
    fn reset_restores() {
        let mut c = ChessClock::new(Duration::from_secs(60));
        laid_out(&mut c);
        c.press(ClockSide::White);
        c.tick(Duration::from_secs(30));
        ev(
            &mut c,
            &WidgetEvent::KeyPressed {
                key: "r".to_string(),
                repeat: false,
            },
        );
        assert_eq!(c.remaining(ClockSide::Black).as_secs(), 60);
        assert_eq!(c.moves(), 0);
    }
}
