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
    ArenaRestoreError, ColdNode, DummyWidget, HotNode, LayoutConstraints, LayoutContext, Rect,
    TimemachineState, Widget, WidgetId,
};
use martensite_devtools::timemachine::{ReplayError, SignalWrite, TimeMachine, World};
use martensite_history::{ChangeOp, LedgerError};
use martensite_reactive::{Memo, ReactiveRuntime, Signal};

#[derive(Debug, Default)]
struct VirtualClock {
    elapsed: std::time::Duration,
}

impl VirtualClock {
    fn new() -> Self {
        Self::default()
    }
    fn step_60fps(&mut self) {
        self.elapsed += FRAME_60FPS;
    }
    fn elapsed_millis(&self) -> u128 {
        self.elapsed.as_millis()
    }
}

const FRAME_60FPS: std::time::Duration = std::time::Duration::from_nanos(16_666_667);

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
        let dt = FRAME_60FPS.as_nanos() as u64;
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

/// Removes a widget from the arena; `revert` re-inserts the stashed
/// node (at a new slot — best-effort; only `apply` is exercised here).
struct RemoveOp {
    id: WidgetId,
    stash: Mutex<Option<(HotNode, ColdNode)>>,
}

impl RemoveOp {
    fn new(id: WidgetId) -> Self {
        Self {
            id,
            stash: Mutex::new(None),
        }
    }
}

impl ChangeOp<World> for RemoveOp {
    fn apply(&self, world: &mut World) {
        *self.stash.lock().unwrap() = world.arena_mut().remove(self.id);
    }
    fn revert(&self, world: &mut World) {
        if let Some((hot, cold)) = self.stash.lock().unwrap().take() {
            world.arena_mut().insert(hot, cold);
        }
    }
}

/// Memos recompute lazily after replay restores source values — the
/// spec's "memos recompute lazily during pull" leg: derived state is
/// never journaled or snapshotted.
#[test]
fn memo_recomputes_lazily_after_replay() {
    let runtime = ReactiveRuntime::new();
    let src = Signal::new_with_runtime(1i32, runtime.clone());
    let doubled = Memo::new_with_runtime(
        {
            let src = src.clone();
            move || src.get() * 2
        },
        runtime.clone(),
    );
    let mut tm = TimeMachine::new(World::new(Default::default(), runtime));

    let _ = src.get_untracked(); // register the source for snapshots
    assert_eq!(doubled.get(), 2);

    let a = tm.set_signal(&src, 5);
    let _b = tm.set_signal(&src, 7);
    assert_eq!(doubled.get(), 14);

    // Restoring the source snapshot marks the memo dirty; the next
    // pull recomputes — nothing was journaled for the memo itself.
    tm.replay_to(a).unwrap();
    assert_eq!(src.get_untracked(), 5);
    assert_eq!(doubled.get(), 10, "memo recomputed from restored source");
}

/// Removing a widget after a checkpoint makes replay fail atomically
/// (world + cursor unchanged); a registered widget factory
/// reconstructs the widget *and* its captured internal state.
#[test]
fn missing_widgets_error_is_atomic_and_factory_restores_state() {
    // --- No factory: atomic failure ---
    let runtime = ReactiveRuntime::new();
    let n = Signal::new_with_runtime(0i32, runtime.clone());
    let mut arena = martensite_core::WidgetArena::new();
    let counter = arena.insert_with_widget(
        HotNode::default(),
        Box::new(CounterWidget {
            ticks: 3,
            rng_value: 7,
        }),
    );
    let mut tm = TimeMachine::new(World::new(arena, runtime));
    // Root checkpoint captured the arena *with* the counter widget.

    tm.commit(Box::new(RemoveOp::new(counter)));
    let before_fp = tm.arena_fingerprint();
    let before_node = tm.current_node();
    let before_sig = n.get_untracked();

    let err = tm.replay_to(tm.root_node()).unwrap_err();
    assert!(matches!(
        err,
        ReplayError::Arena(ArenaRestoreError::MissingWidgets(_))
    ));
    assert_eq!(tm.arena_fingerprint(), before_fp, "world unchanged");
    assert_eq!(tm.current_node(), before_node, "cursor unchanged");
    assert_eq!(n.get_untracked(), before_sig, "signals unchanged");

    // --- With a factory: reconstructed widget gets captured state ---
    let runtime2 = ReactiveRuntime::new();
    let mut arena2 = martensite_core::WidgetArena::new();
    let counter2 = arena2.insert_with_widget(
        HotNode::default(),
        Box::new(CounterWidget {
            ticks: 3,
            rng_value: 7,
        }),
    );
    let mut tm2 = TimeMachine::new(World::new(arena2, runtime2)).with_widget_factory(|_id| {
        Some(Box::new(CounterWidget {
            ticks: 999,
            rng_value: 999,
        }))
    });
    tm2.commit(Box::new(RemoveOp::new(counter2)));

    tm2.replay_to(tm2.root_node()).unwrap();
    assert!(
        tm2.world().arena().is_alive(counter2),
        "fabricated widget installed at its original id"
    );
    assert_eq!(
        counter_widget_state(tm2.world(), counter2),
        (3, 7),
        "fabricated widget received its captured TimemachineState"
    );
}

/// Checkpoint selection uses tree depth, not capture order: a manual
/// checkpoint taken late at a *shallow* node must not outrank the
/// genuinely nearest (deepest) ancestor checkpoint.
#[test]
fn replay_uses_deepest_checkpoint() {
    let runtime = ReactiveRuntime::new();
    let n = Signal::new_with_runtime(0i32, runtime.clone());
    let mut tm =
        TimeMachine::new(World::new(Default::default(), runtime)).with_checkpoint_interval(2);

    let _node1 = tm.set_signal(&n, 1);
    let node2 = tm.set_signal(&n, 2); // auto-checkpoint (depth 2)
    let _node3 = tm.set_signal(&n, 3);
    let node4 = tm.set_signal(&n, 4); // auto-checkpoint (depth 4)
    let node5 = tm.set_signal(&n, 5); // no checkpoint (depth 5)

    // Late manual checkpoint at the shallow node2 — highest capture
    // frame but shallowest depth.
    tm.jump_to(node2).unwrap();
    tm.checkpoint();
    tm.jump_to(node5).unwrap();

    let used = tm.replay_to(node5).unwrap();
    assert_eq!(used, node4, "deepest ancestor checkpoint wins");
    assert_eq!(n.get_untracked(), 5);
}

/// The root checkpoint is pinned: checkpoint eviction never removes
/// it, so `replay_to` always has a checkpoint on the ancestor chain.
#[test]
fn root_checkpoint_is_pinned() {
    let runtime = ReactiveRuntime::new();
    let n = Signal::new_with_runtime(0i32, runtime.clone());
    let mut tm = TimeMachine::new(World::new(Default::default(), runtime))
        .with_checkpoint_interval(1)
        .with_max_checkpoints(2);

    for i in 1..=6 {
        tm.set_signal(&n, i);
    }
    assert_eq!(tm.checkpoint_count(), 2);
    assert!(
        tm.checkpoint_at(tm.root_node()).is_some(),
        "root checkpoint survives eviction"
    );
    // Replay still works — the pinned root is always on the chain.
    tm.replay_to(tm.current_node()).unwrap();
    assert_eq!(n.get_untracked(), 6);
}

/// H1 reproduction: bounded history pruning compresses interior
/// ancestors of the active branch and drops their ops. Replaying
/// through the compressed region must return `LedgerError::HistoryGap`
/// — before the fix, `replay_to` silently applied only the surviving
/// ops and reported success.
#[test]
fn replay_across_pruned_region_errors() {
    let runtime = ReactiveRuntime::new();
    let n = Signal::new_with_runtime(0i32, runtime.clone());
    let mut tm = TimeMachine::with_max_nodes(World::new(Default::default(), runtime), 4);

    let mut tip = tm.root_node();
    for i in 1..=8 {
        tip = tm.set_signal(&n, i);
    }

    let err = tm.replay_to(tip).unwrap_err();
    assert!(
        matches!(err, ReplayError::Ledger(LedgerError::HistoryGap)),
        "expected HistoryGap, got {err:?}"
    );
    // The world and history cursor are untouched by the failed replay.
    assert_eq!(n.get_untracked(), 8);
    assert_eq!(tm.current_node(), tip);
}

/// A widget that snapshots state but always rejects it on restore —
/// exercises `RestoreRejected` propagating out of `replay_to`.
struct RejectWidget;

impl Widget for RejectWidget {
    fn measure(&mut self, _cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        Vec2::ZERO
    }
    fn layout(&mut self, _cx: &mut LayoutContext, _b: Rect) {}
    fn timemachine_snapshot(&self) -> Option<Box<dyn TimemachineState>> {
        Some(Box::new(CounterState {
            ticks: 0,
            rng_value: 0,
        }))
    }
    fn timemachine_restore(&mut self, _state: &dyn TimemachineState) -> bool {
        false
    }
}

/// When a widget rejects its captured state, `replay_to` returns the
/// `Arena` error and the cursor stays at the checkpoint node (the
/// restore aborts before forward ops run).
#[test]
fn restore_rejected_inside_replay_returns_arena_error() {
    struct InsertOp;
    impl ChangeOp<World> for InsertOp {
        fn apply(&self, world: &mut World) {
            world
                .arena_mut()
                .insert_with_widget(HotNode::default(), Box::new(RejectWidget));
        }
        fn revert(&self, _world: &mut World) {}
    }

    let runtime = ReactiveRuntime::new();
    let mut tm = TimeMachine::new(World::new(Default::default(), runtime));
    let node = tm.commit(Box::new(InsertOp));
    // Checkpoint at the current node captures the RejectWidget's
    // (acceptable) snapshot.
    assert_eq!(tm.checkpoint(), node);

    let err = tm.replay_to(node).unwrap_err();
    assert!(
        matches!(
            err,
            ReplayError::Arena(ArenaRestoreError::RestoreRejected(_))
        ),
        "expected RestoreRejected, got {err:?}"
    );
    // The cursor stays at the checkpoint that failed to restore.
    assert_eq!(tm.current_node(), node);
}
