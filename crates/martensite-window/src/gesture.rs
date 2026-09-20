//! Multi-touch gesture recognition over raw [`PointerEvent`]s.
//!
//! [`GestureRecognizer`] consumes per-finger pointer events (each
//! [`PointerKind::Touch`] carries a distinct [`PointerId`]) and emits
//! recognized [`Gesture`]s into a queue drained by
//! [`GestureRecognizer::drain`]. Recognized gestures:
//!
//! * [`Gesture::LongPress`] — a press held past
//!   [`GestureConfig::long_press`] without exceeding the slop radius.
//!   Fires from [`GestureRecognizer::tick`], so the host must pump it
//!   each frame while touches are down.
//! * [`Gesture::Swipe`] — a release whose recent travel velocity exceeds
//!   [`GestureConfig::swipe_velocity`].
//! * [`Gesture::Pinch`] — two simultaneous fingers: incremental scale,
//!   rotation, and pan deltas about the pair centroid.
//!
//! Single-finger taps and drags are deliberately out of scope — tap
//! streaks are already handled by the click `count` in
//! `WidgetEvent::PointerPressed`, and press-drag is the widget layer's
//! domain. The recognizer is passive: it never consumes events, so the
//! host can feed it in parallel with normal dispatch.
//!
//! # Examples
//!
//! ```
//! use martensite_window::event::{PointerEvent, PointerId, PointerKind,
//!     PointerState};
//! use martensite_window::gesture::{Gesture, GestureRecognizer};
//! use std::time::Instant;
//! use glam::Vec2;
//!
//! let mut g = GestureRecognizer::new();
//! let t0 = Instant::now();
//! g.pointer_event(&PointerEvent {
//!     pointer_id: PointerId::new(1),
//!     kind: PointerKind::Touch,
//!     position: Vec2::new(10.0, 10.0),
//!     state: PointerState::Pressed,
//!     button: None,
//!     modifiers: Default::default(),
//! }, t0);
//! g.tick(t0 + std::time::Duration::from_millis(600));
//! assert!(g.drain().any(|g| matches!(g, Gesture::LongPress { .. })));
//! ```

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use glam::Vec2;

use crate::event::{PointerEvent, PointerId, PointerKind, PointerState};

/// A recognized gesture.
///
/// # Examples
///
/// ```
/// use martensite_window::gesture::Gesture;
/// use martensite_window::event::PointerId;
/// use glam::Vec2;
///
/// let g = Gesture::Swipe {
///     velocity: Vec2::new(500.0, 0.0),
///     origin: Vec2::ZERO,
///     pointer: PointerId::PRIMARY,
/// };
/// assert!(matches!(g, Gesture::Swipe { .. }));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    /// A press held past the long-press threshold without moving.
    LongPress {
        /// Where the finger is held.
        position: Vec2,
        /// Which finger/pointer.
        pointer: PointerId,
    },
    /// A fast flick released above the velocity threshold.
    Swipe {
        /// Velocity at release, logical px/s.
        velocity: Vec2,
        /// Where the swipe started.
        origin: Vec2,
        /// Which finger/pointer.
        pointer: PointerId,
    },
    /// Two-finger transform update (incremental deltas).
    ///
    /// Emitted on each move while exactly two touch fingers are down.
    /// `scale_delta` and `rotation_delta` are relative to the previous
    /// frame (`1.0`/`0.0` = unchanged); `pan` is centroid motion.
    Pinch {
        /// Current pair centroid.
        centroid: Vec2,
        /// Distance ratio since the last event (`> 1` = spread).
        scale_delta: f32,
        /// Angle change since the last event, radians.
        rotation_delta: f32,
        /// Centroid translation since the last event.
        pan: Vec2,
    },
}

/// Tunables for [`GestureRecognizer`].
///
/// # Examples
///
/// ```
/// use martensite_window::gesture::GestureConfig;
/// use std::time::Duration;
///
/// let c = GestureConfig::default();
/// assert_eq!(c.long_press, Duration::from_millis(500));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct GestureConfig {
    /// Hold duration that fires [`Gesture::LongPress`]. Default 500 ms.
    pub long_press: Duration,
    /// Distance a held finger may drift before long-press eligibility is
    /// lost, logical px. Default 8.
    pub slop: f32,
    /// Minimum release velocity for [`Gesture::Swipe`], logical px/s.
    /// Default 400.
    pub swipe_velocity: f32,
    /// Window over which release velocity is measured. Default 100 ms.
    pub swipe_window: Duration,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            long_press: Duration::from_millis(500),
            slop: 8.0,
            swipe_velocity: 400.0,
            swipe_window: Duration::from_millis(100),
        }
    }
}

/// Per-finger tracking state.
#[derive(Clone, Debug)]
struct Finger {
    /// Down position.
    origin: Vec2,
    /// Down timestamp.
    down_at: Instant,
    /// Latest position.
    pos: Vec2,
    /// Position `swipe_window` ago (velocity baseline).
    window_pos: Vec2,
    /// Timestamp of `window_pos`.
    window_at: Instant,
    /// Input-source kind (only `Touch` fingers join pairs).
    kind: PointerKind,
    /// Moved beyond slop — kills long-press eligibility.
    moved: bool,
    /// Long-press already emitted for this hold.
    long_press_fired: bool,
}

/// Snapshot of a two-finger pair for delta computation.
#[derive(Clone, Copy)]
struct Pair {
    centroid: Vec2,
    distance: f32,
    angle: f32,
}

fn pair_state(a: Vec2, b: Vec2) -> Pair {
    Pair {
        centroid: (a + b) * 0.5,
        distance: (b - a).length().max(f32::EPSILON),
        angle: (b.y - a.y).atan2(b.x - a.x),
    }
}

/// Recognizes [`Gesture`]s from a stream of [`PointerEvent`]s.
///
/// Passive — never consumes or mutates events; feed it alongside normal
/// widget dispatch. Time is supplied explicitly (`now` parameters) so the
/// recognizer is fully deterministic in tests and can run off any clock.
///
/// # Examples
///
/// ```
/// use martensite_window::gesture::GestureRecognizer;
///
/// let g = GestureRecognizer::new();
/// assert_eq!(g.active_touches(), 0);
/// ```
#[derive(Default)]
pub struct GestureRecognizer {
    config: GestureConfig,
    fingers: BTreeMap<PointerId, Finger>,
    pair: Option<Pair>,
    out: Vec<Gesture>,
}

impl GestureRecognizer {
    /// A recognizer with default [`GestureConfig`].
    ///
    /// ```
    /// use martensite_window::gesture::GestureRecognizer;
    ///
    /// assert_eq!(GestureRecognizer::new().active_touches(), 0);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// A recognizer with custom tuning.
    ///
    /// ```
    /// use martensite_window::gesture::{GestureConfig, GestureRecognizer};
    ///
    /// let g = GestureRecognizer::with_config(GestureConfig {
    ///     slop: 20.0,
    ///     ..Default::default()
    /// });
    /// assert_eq!(g.active_touches(), 0);
    /// ```
    pub fn with_config(config: GestureConfig) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }

    /// How many fingers are currently down.
    ///
    /// ```
    /// use martensite_window::gesture::GestureRecognizer;
    ///
    /// assert_eq!(GestureRecognizer::new().active_touches(), 0);
    /// ```
    pub fn active_touches(&self) -> usize {
        self.fingers.len()
    }

    /// Feed one raw pointer event. Only [`PointerKind::Touch`] events
    /// participate in multi-finger gestures; other kinds are tracked for
    /// long-press/swipe but cannot form a [`Gesture::Pinch`].
    pub fn pointer_event(&mut self, ev: &PointerEvent, now: Instant) {
        match ev.state {
            PointerState::Pressed => {
                self.fingers.insert(
                    ev.pointer_id,
                    Finger {
                        origin: ev.position,
                        down_at: now,
                        pos: ev.position,
                        window_pos: ev.position,
                        window_at: now,
                        kind: ev.kind,
                        moved: false,
                        long_press_fired: false,
                    },
                );
                self.reset_pair_baseline();
            }
            PointerState::Moved => {
                if let Some(f) = self.fingers.get_mut(&ev.pointer_id) {
                    if !f.moved && (ev.position - f.origin).length() > self.config.slop {
                        f.moved = true;
                    }
                    if now.duration_since(f.window_at) >= self.config.swipe_window {
                        f.window_pos = f.pos;
                        f.window_at = now;
                    }
                    f.pos = ev.position;
                }
                self.emit_pinch();
            }
            PointerState::Released => {
                if let Some(f) = self.fingers.remove(&ev.pointer_id) {
                    // Velocity is measured from the trailing window
                    // baseline to the release position/time — `f.at` can
                    // coincide with `window_at` when the last move just
                    // slid the window, which would zero the divisor.
                    let dt = now.duration_since(f.window_at).as_secs_f32();
                    if f.moved && dt > 0.0 {
                        let v = (ev.position - f.window_pos) / dt;
                        if v.length() >= self.config.swipe_velocity {
                            self.out.push(Gesture::Swipe {
                                velocity: v,
                                origin: f.origin,
                                pointer: ev.pointer_id,
                            });
                        }
                    }
                }
                self.reset_pair_baseline();
            }
        }
    }

    /// Pump the recognizer for time-driven gestures. Call every frame
    /// while [`active_touches`](Self::active_touches) is non-zero;
    /// [`Gesture::LongPress`] fires here, not on an event.
    pub fn tick(&mut self, now: Instant) {
        let threshold = self.config.long_press;
        let mut out = Vec::new();
        for (id, f) in self.fingers.iter_mut() {
            if !f.moved && !f.long_press_fired && now.duration_since(f.down_at) >= threshold {
                f.long_press_fired = true;
                out.push(Gesture::LongPress {
                    position: f.pos,
                    pointer: *id,
                });
            }
        }
        self.out.extend(out);
    }

    /// Drain all pending gestures.
    ///
    /// ```
    /// use martensite_window::gesture::GestureRecognizer;
    ///
    /// let mut g = GestureRecognizer::new();
    /// assert_eq!(g.drain().count(), 0);
    /// ```
    pub fn drain(&mut self) -> impl Iterator<Item = Gesture> + '_ {
        self.out.drain(..)
    }

    /// Positions of the active touch fingers (BTreeMap order — stable
    /// for a given finger set).
    fn touch_positions(&self) -> Vec<Vec2> {
        self.fingers
            .values()
            .filter(|f| f.kind == PointerKind::Touch)
            .map(|f| f.pos)
            .collect()
    }

    /// Re-anchor the pair baseline when the finger set changes
    /// (press/release) so no delta is emitted across a member change.
    fn reset_pair_baseline(&mut self) {
        let t = self.touch_positions();
        self.pair = if t.len() >= 2 {
            Some(pair_state(t[0], t[1]))
        } else {
            None
        };
    }

    /// Emit a [`Gesture::Pinch`] delta if a two-finger pair is active.
    fn emit_pinch(&mut self) {
        let t = self.touch_positions();
        if t.len() < 2 {
            self.pair = None;
            return;
        }
        let now_pair = pair_state(t[0], t[1]);
        if let Some(prev) = self.pair {
            let scale = now_pair.distance / prev.distance;
            let rot = now_pair.angle - prev.angle;
            let pan = now_pair.centroid - prev.centroid;
            if scale != 1.0 || rot != 0.0 || pan != Vec2::ZERO {
                self.out.push(Gesture::Pinch {
                    centroid: now_pair.centroid,
                    scale_delta: scale,
                    rotation_delta: rot,
                    pan,
                });
            }
        }
        self.pair = Some(now_pair);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{MouseButton, PointerKind};

    fn touch(id: u64, pos: Vec2, state: PointerState) -> PointerEvent {
        PointerEvent {
            pointer_id: PointerId::new(id),
            kind: PointerKind::Touch,
            position: pos,
            state,
            button: None,
            modifiers: Default::default(),
        }
    }

    fn mouse(pos: Vec2, state: PointerState) -> PointerEvent {
        PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: pos,
            state,
            button: Some(MouseButton::Left),
            modifiers: Default::default(),
        }
    }

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn long_press_fires_once_on_tick() {
        let mut g = GestureRecognizer::new();
        let t0 = Instant::now();
        g.pointer_event(&touch(1, Vec2::new(5.0, 5.0), PointerState::Pressed), t0);
        g.tick(t0 + 600 * MS);
        let out: Vec<_> = g.drain().collect();
        assert!(matches!(out.as_slice(), [Gesture::LongPress { .. }]));
        // Held longer — no repeat.
        g.tick(t0 + 1200 * MS);
        assert_eq!(g.drain().count(), 0);
    }

    #[test]
    fn move_past_slop_kills_long_press() {
        let mut g = GestureRecognizer::new();
        let t0 = Instant::now();
        g.pointer_event(&touch(1, Vec2::ZERO, PointerState::Pressed), t0);
        g.pointer_event(
            &touch(1, Vec2::new(50.0, 0.0), PointerState::Moved),
            t0 + 100 * MS,
        );
        g.tick(t0 + 600 * MS);
        assert_eq!(g.drain().count(), 0);
    }

    #[test]
    fn fast_release_is_swipe() {
        let mut g = GestureRecognizer::new();
        let t0 = Instant::now();
        g.pointer_event(&touch(1, Vec2::ZERO, PointerState::Pressed), t0);
        g.pointer_event(
            &touch(1, Vec2::new(60.0, 0.0), PointerState::Moved),
            t0 + 50 * MS,
        );
        g.pointer_event(
            &touch(1, Vec2::new(120.0, 0.0), PointerState::Moved),
            t0 + 100 * MS,
        );
        g.pointer_event(
            &touch(1, Vec2::new(120.0, 0.0), PointerState::Released),
            t0 + 110 * MS,
        );
        let out: Vec<_> = g.drain().collect();
        assert!(matches!(
            out.as_slice(),
            [Gesture::Swipe { velocity, .. }] if velocity.x > 400.0
        ));
    }

    #[test]
    fn slow_release_is_not_swipe() {
        let mut g = GestureRecognizer::new();
        let t0 = Instant::now();
        g.pointer_event(&touch(1, Vec2::ZERO, PointerState::Pressed), t0);
        g.pointer_event(
            &touch(1, Vec2::new(20.0, 0.0), PointerState::Moved),
            t0 + 400 * MS,
        );
        g.pointer_event(
            &touch(1, Vec2::new(20.0, 0.0), PointerState::Released),
            t0 + 500 * MS,
        );
        assert_eq!(g.drain().count(), 0);
    }

    #[test]
    fn pinch_spread_emits_scale_delta() {
        let mut g = GestureRecognizer::new();
        let t0 = Instant::now();
        g.pointer_event(&touch(1, Vec2::new(0.0, 0.0), PointerState::Pressed), t0);
        g.pointer_event(&touch(2, Vec2::new(100.0, 0.0), PointerState::Pressed), t0);
        // Spread finger 2 outward — distance 100 → 150.
        g.pointer_event(
            &touch(2, Vec2::new(150.0, 0.0), PointerState::Moved),
            t0 + 16 * MS,
        );
        let out: Vec<_> = g.drain().collect();
        match out.as_slice() {
            [Gesture::Pinch {
                scale_delta, pan, ..
            }] => {
                assert!((*scale_delta - 1.5).abs() < 0.01);
                assert!((pan.x - 25.0).abs() < 0.01);
            }
            other => panic!("expected one pinch, got {other:?}"),
        }
    }

    #[test]
    fn mouse_does_not_pinch() {
        let mut g = GestureRecognizer::new();
        let t0 = Instant::now();
        g.pointer_event(&mouse(Vec2::ZERO, PointerState::Pressed), t0);
        g.pointer_event(&touch(2, Vec2::new(100.0, 0.0), PointerState::Pressed), t0);
        g.pointer_event(
            &touch(2, Vec2::new(200.0, 0.0), PointerState::Moved),
            t0 + 16 * MS,
        );
        // Only one touch finger — no pair.
        assert_eq!(g.drain().count(), 0);
    }

    #[test]
    fn third_finger_reanchors_baseline() {
        let mut g = GestureRecognizer::new();
        let t0 = Instant::now();
        g.pointer_event(&touch(1, Vec2::new(0.0, 0.0), PointerState::Pressed), t0);
        g.pointer_event(&touch(2, Vec2::new(100.0, 0.0), PointerState::Pressed), t0);
        g.pointer_event(&touch(3, Vec2::new(50.0, 50.0), PointerState::Pressed), t0);
        // Pair baseline re-anchored on press; moving finger 1 emits a
        // small delta rather than a jump.
        g.pointer_event(
            &touch(1, Vec2::new(5.0, 0.0), PointerState::Moved),
            t0 + 16 * MS,
        );
        for g2 in g.drain() {
            if let Gesture::Pinch { scale_delta, .. } = g2 {
                assert!((scale_delta - 1.0).abs() < 0.2);
            }
        }
    }
}
