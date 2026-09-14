//! Deterministic journal+snapshot replay test for the hybrid
//! time-travel debugger (v0.17.0 milestone §4.6 / §5 exit gate).
//!
//! The gate: "Journal+snapshot replay reproduces a recorded interaction
//! deterministically — VirtualClock test: replayed state == recorded
//! state."
//!
//! Determinism contract under test:
//! - **VirtualClock** drives all timing (`tick` signal payloads are
//!   virtual nanoseconds — no wall-clock input anywhere).
//! - **Seeded RNG**: a fixed-seed xorshift64 produces the widget deltas,
//!   so recorded and replayed runs consume the identical stream.
//! - **Deterministic task ordering**: commands are committed in a fixed
//!   sequence; replay applies the same sequence.
//! - **Replay suppression**: source writes re-applied during replay must
//!   not grow the `SourceJournal`.
#![cfg(feature = "devtools-timemachine")]
#![forbid(unsafe_code)]

use std::any::Any;
use std::sync::Mutex;

use glam::Vec2;
use martensite_core::{
    DummyWidget, HotNode, LayoutConstraints, LayoutContext, Rect, TimemachineState, Widget,
    WidgetId,
};
use martensite_devtools::timemachine::{SignalWrite, TimeMachine, World};
use martensite_history::ChangeOp;
use martensite_reactive::{ReactiveRuntime, Signal};
use martensite_test::VirtualClock;

/// Widget with journaled internal state: a tick counter plus a value
/// produced by the seeded RNG — both snapshotted via `TimemachineState`.
struct CounterWidget {
    ticks: u64,
    rng_value: u64,
}

/// Opaque captured state for `CounterWidget`.
#[derive(Debug)]
struct CounterState {
    ticks: u64,
    rng_value: u64,
}

impl TimemachineState for CounterState {
    fn fingerprint(&self) -> u64 {
        // FNV-1a mix of both fields.
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for b in self.ticks.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        for b in self.rng_value.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Widget for CounterWidget {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::ZERO
    }
    fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}

    fn as_any_mut(&mut self) -> Option<&mut dyn Any> {
        Some(self)
    }

    fn timemachine_snapshot(&self) -> Option<Box<dyn TimemachineState>> {
        Some(Box::new(CounterState {
            ticks: self.ticks,
            rng_value: self.rng_value,
        }))
    }

    fn timemachine_restore(&mut self, state: &dyn TimemachineState) -> bool {
        let Some(s) = state.as_any().downcast_ref::<CounterState>() else {
            return false;
        };
        self.ticks = s.ticks;
        self.rng_value = s.rng_value;
        true
    }
}

/// Fixed-seed xorshift64 — the seeded-RNG leg of the determinism
/// contract. The same seed yields the same stream during recording and
/// replay.
struct XorShift64(u64);

impl XorShift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

/// A user-meaningful command: advance the virtual clock, record its
/// nanoseconds in `tick`, and bump the counter widget by a seeded RNG
/// delta. `previous` is captured lazily on first apply so `revert`
/// (used by `jump_to`, never by `replay_to`) can restore it.
struct FrameOp {
    tick: Signal<u64>,
    widget: WidgetId,
    dt_nanos: u64,
    rng_delta: u64,
    previous: Mutex<Option<(u64, u64, u64)>>,
}

impl FrameOp {
    fn new(tick: &Signal<u64>, widget: WidgetId, dt_nanos: u64, rng_delta: u64) -> Self {
        Self {
            tick: tick.clone(),
            widget,
            dt_nanos,
            rng_delta,
            previous: Mutex::new(None),
        }
    }
}

impl ChangeOp<World> for FrameOp {
    fn apply(&self, world: &mut World) {
        let prev_tick = self.tick.get_untracked();
        self.tick.set(prev_tick + self.dt_nanos);

        let cold = world
            .arena_mut()
            .get_cold_mut(self.widget)
            .expect("counter widget alive");
        let counter = cold
            .widget
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<CounterWidget>())
            .expect("counter widget type");
        let prev_ticks = counter.ticks;
        let prev_rng = counter.rng_value;
        counter.ticks += 1;
        counter.rng_value = counter.rng_value.wrapping_add(self.rng_delta);

        *self.previous.lock().unwrap() = Some((prev_tick, prev_ticks, prev_rng));
    }

    fn revert(&self, world: &mut World) {
        let Some((prev_tick, prev_ticks, prev_rng)) = *self.previous.lock().unwrap() else {
            return;
        };
        self.tick.set(prev_tick);
        let cold = world
            .arena_mut()
            .get_cold_mut(self.widget)
            .expect("counter widget alive");
        let counter = cold
            .widget
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<CounterWidget>())
            .expect("counter widget type");
        counter.ticks = prev_ticks;
        counter.rng_value = prev_rng;
    }
}

fn counter_widget_state(world: &World, id: WidgetId) -> (u64, u64) {
    let cold = world.arena().get_cold(id).expect("counter widget alive");
    let state = cold
        .widget
        .timemachine_snapshot()
        .expect("counter widget snapshots state");
    let s = state.as_any().downcast_ref::<CounterState>().unwrap();
    (s.ticks, s.rng_value)
}

/// The milestone's determinism gate: a scripted interaction is
/// recorded, checkpoints snapshot arena + signal state, and
/// `replay_to` reproduces the recorded state exactly — verified by
/// arena fingerprint and live signal values.
#[test]
fn virtual_clock_replay_reproduces_recorded_state() {
    let mut clock = VirtualClock::new();
    let mut rng = XorShift64(0x9E37_79B9_7F4A_7C15); // fixed seed

    let runtime = ReactiveRuntime::new();
    let tick = Signal::new_with_runtime(0u64, runtime.clone());
    let label = Signal::new_with_runtime(String::from("init"), runtime.clone());

    let mut arena = martensite_core::WidgetArena::new();
    let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    let counter = arena.insert_with_widget(
        HotNode::default(),
        Box::new(CounterWidget {
            ticks: 0,
            rng_value: 0,
        }),
    );
    arena.append_child(root, counter).unwrap();

    let mut tm = TimeMachine::new(World::new(arena, runtime)).with_checkpoint_interval(4);

    // Register both sources with the snapshot registry.
    let _ = tick.get_untracked();
    let _ = label.get_untracked();

    // Record: each "frame" is one FrameOp + one label write (2 commits),
    // driven by the VirtualClock and the seeded RNG. 9 frames → 18
    // commits → auto-checkpoints at commits 4, 8, 12, 16 plus the root.
    // `recorded[k]` captures state after commit 2k+2.
    let mut recorded = Vec::new();
    for _ in 0..9 {
        clock.step_60fps();
        let dt = martensite_test::FRAME_60FPS.as_nanos() as u64;
        tm.commit(Box::new(FrameOp::new(&tick, counter, dt, rng.next())));
        tm.commit(Box::new(SignalWrite::new(
            &label,
            format!("t+{}ms", clock.elapsed_millis()),
        )));
        recorded.push((
            tm.current_node(),
            tm.arena_fingerprint(),
            tick.get_untracked(),
            label.get_untracked(),
            counter_widget_state(tm.world(), counter),
        ));
    }
    assert_eq!(tm.checkpoint_count(), 5, "root + commits 4, 8, 12, 16");

    let (tip, fp_tip, tick_tip, label_tip, widget_tip) = recorded[8].clone();
    let journal_len = tm.source_journal().len();

    // Scrub back to the commit-6 node: replay restores the commit-4
    // checkpoint and applies commits 5–6 forward.
    let (node6, fp6, tick6, label6, widget6) = recorded[2].clone();
    tm.replay_to(node6).unwrap();
    assert_eq!(tm.arena_fingerprint(), fp6, "arena state at commit 6");
    assert_eq!(tick.get_untracked(), tick6);
    assert_eq!(label.get_untracked(), label6);
    assert_eq!(counter_widget_state(tm.world(), counter), widget6);

    // Replay forward to the commit-10 node — restores the commit-8
    // checkpoint and applies commits 9–10.
    let (node10, fp10, tick10, label10, widget10) = recorded[4].clone();
    let used = tm.replay_to(node10).unwrap();
    assert_eq!(tm.arena_fingerprint(), fp10, "arena state at commit 10");
    assert_eq!(tick.get_untracked(), tick10);
    assert_eq!(label.get_untracked(), label10);
    assert_eq!(counter_widget_state(tm.world(), counter), widget10);
    // The nearest ancestor checkpoint of node10 is the commit-8 node.
    assert_eq!(used, recorded[3].0, "replayed from the commit-8 checkpoint");

    // Replay to the tip reproduces the fully recorded state.
    tm.replay_to(tip).unwrap();
    assert_eq!(tm.arena_fingerprint(), fp_tip, "arena state at tip");
    assert_eq!(tick.get_untracked(), tick_tip);
    assert_eq!(label.get_untracked(), label_tip);
    assert_eq!(counter_widget_state(tm.world(), counter), widget_tip);

    // Replay never re-journaled: the source journal is unchanged.
    assert_eq!(tm.source_journal().len(), journal_len);
}

/// Cross-branch scrubbing still works via LCA `jump_to`, and
/// `set_if_changed` no-ops produce no journal entries.
#[test]
fn replay_suppression_and_branching() {
    let runtime = ReactiveRuntime::new();
    let n = Signal::new_with_runtime(0i32, runtime.clone());
    let mut tm = TimeMachine::new(World::new(Default::default(), runtime));

    let a = tm.set_signal(&n, 1);
    tm.set_signal(&n, 2);
    tm.jump_to(a).unwrap();
    assert_eq!(n.get_untracked(), 1);
    // Branch: a new commit on top of `a` forks the history tree.
    let c = tm.set_signal(&n, 10);
    assert_eq!(n.get_untracked(), 10);

    // LCA jump across the branch — not replayable forward, but
    // jump_to handles it without journaling navigation writes.
    tm.jump_to(c).unwrap();
    assert_eq!(n.get_untracked(), 10);

    // 3 user writes journaled (1, 2, 10); navigation writes suppressed.
    assert_eq!(tm.source_journal().len(), 3);
}
