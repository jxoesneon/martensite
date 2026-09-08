//! Deterministic randomized and long-running fuzz campaigns.
//!
//! The harness exercises arena allocation, reactive propagation, and event
//! state machines without requiring a native window or a coverage-guided
//! fuzzer executable.

use std::fmt;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use martensite_clipboard::{ClipboardItem, ClipboardService, InMemoryClipboard};
use martensite_core::{ColdNode, HotNode, NodeFlags, Rect, WidgetArena, WidgetId};
use martensite_dnd::{DndSessionManager, DropEffect, SessionId};
use martensite_focus::{FocusManager, TabNavigation};
use martensite_reactive::{Memo, ReactiveRuntime, Signal};
use martensite_window::event::{
    EventDispatchOutcome, EventRouter, ModifierKeys, MouseButton, PointerEvent, PointerId,
    PointerState,
};

/// The subsystem selected for a failed invariant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzTarget {
    /// Generational arena allocation and dense compaction.
    ArenaCompaction,
    /// Reactive signal and memo graph mutation.
    ReactiveDag,
    /// Input routing, focus, clipboard, and drag-and-drop state.
    EventRouting,
}

/// Configuration for one fuzz campaign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuzzConfig {
    /// Seed controlling every generated operation.
    pub seed: u64,
    /// Maximum number of iterations; `None` means duration-only.
    pub iterations: Option<u64>,
    /// Maximum wall-clock duration; `None` means iteration-only.
    pub duration: Option<Duration>,
}

impl FuzzConfig {
    /// Returns a bounded configuration suitable for CI.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::FuzzConfig;
    /// assert_eq!(FuzzConfig::quick(7).iterations, Some(100));
    /// ```
    #[must_use]
    pub const fn quick(seed: u64) -> Self {
        Self {
            seed,
            iterations: Some(100),
            duration: None,
        }
    }

    /// Returns a bounded configuration suitable for parallel CI runs.
    ///
    /// `quick_parallel` keeps the same per-thread budget as [`FuzzConfig::quick`]
    /// but is intended for use with [`FuzzEngine::run_parallel`] so that each
    /// fuzz target executes on its own thread.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::FuzzConfig;
    /// assert_eq!(FuzzConfig::quick_parallel(7).iterations, Some(100));
    /// ```
    #[must_use]
    pub const fn quick_parallel(seed: u64) -> Self {
        Self {
            seed,
            iterations: Some(100),
            duration: None,
        }
    }

    /// Returns a duration-only configuration for a 48-hour soak campaign.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::FuzzConfig;
    /// use std::time::Duration;
    /// assert_eq!(FuzzConfig::soak(1).duration, Some(Duration::from_secs(48 * 60 * 60)));
    /// ```
    #[must_use]
    pub const fn soak(seed: u64) -> Self {
        Self {
            seed,
            iterations: None,
            duration: Some(Duration::from_secs(48 * 60 * 60)),
        }
    }
}

/// Summary returned after a successful campaign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuzzReport {
    /// Seed used by the campaign.
    pub seed: u64,
    /// Number of complete cross-subsystem iterations.
    pub iterations: u64,
    /// Number of individual randomized operations.
    pub operations: u64,
}

/// A reproducible invariant failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzError {
    /// Subsystem in which the failure occurred.
    pub target: FuzzTarget,
    /// Campaign seed needed to reproduce the failure.
    pub seed: u64,
    /// Zero-based campaign iteration.
    pub iteration: u64,
    /// Description of the violated invariant.
    pub invariant: String,
}

impl fmt::Display for FuzzError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "{:?} invariant failed at iteration {} with seed {}: {}",
            self.target, self.iteration, self.seed, self.invariant
        )
    }
}

impl std::error::Error for FuzzError {}

/// Stateful deterministic fuzz campaign runner.
pub struct FuzzEngine {
    config: FuzzConfig,
}

impl FuzzEngine {
    /// Creates an engine from an explicit configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_test::{FuzzConfig, FuzzEngine};
    /// let report = FuzzEngine::new(FuzzConfig { seed: 9, iterations: Some(10), duration: None })
    ///     .run()
    ///     .expect("fuzz smoke campaign");
    /// assert_eq!(report.iterations, 10);
    /// ```
    #[must_use]
    pub const fn new(config: FuzzConfig) -> Self {
        Self { config }
    }

    /// Runs all three fuzz targets until either configured limit is reached.
    pub fn run(&self) -> Result<FuzzReport, FuzzError> {
        let started = Instant::now();
        let mut rng = Generator::new(self.config.seed);
        let mut arena = ArenaState::default();
        let mut reactive = ReactiveState::new();
        let mut events = EventState::default();
        let mut iteration = 0;

        while self.should_continue(iteration, started.elapsed()) {
            arena.step(&mut rng).map_err(|invariant| {
                self.error(FuzzTarget::ArenaCompaction, iteration, invariant)
            })?;
            reactive
                .step(&mut rng)
                .map_err(|invariant| self.error(FuzzTarget::ReactiveDag, iteration, invariant))?;
            events
                .step(&mut rng)
                .map_err(|invariant| self.error(FuzzTarget::EventRouting, iteration, invariant))?;
            iteration += 1;
        }

        Ok(FuzzReport {
            seed: self.config.seed,
            iterations: iteration,
            operations: iteration * 3,
        })
    }

    /// Runs a single fuzz target until the configured limit is reached.
    fn run_target(&self, target: FuzzTarget) -> Result<FuzzReport, FuzzError> {
        let started = Instant::now();
        let mut rng = Generator::new(self.config.seed);
        let mut iteration = 0;

        match target {
            FuzzTarget::ArenaCompaction => {
                let mut arena = ArenaState::default();
                while self.should_continue(iteration, started.elapsed()) {
                    arena
                        .step(&mut rng)
                        .map_err(|invariant| self.error(target, iteration, invariant))?;
                    iteration += 1;
                }
            }
            FuzzTarget::ReactiveDag => {
                let mut reactive = ReactiveState::new();
                while self.should_continue(iteration, started.elapsed()) {
                    reactive
                        .step(&mut rng)
                        .map_err(|invariant| self.error(target, iteration, invariant))?;
                    iteration += 1;
                }
            }
            FuzzTarget::EventRouting => {
                let mut events = EventState::default();
                while self.should_continue(iteration, started.elapsed()) {
                    events
                        .step(&mut rng)
                        .map_err(|invariant| self.error(target, iteration, invariant))?;
                    iteration += 1;
                }
            }
        }

        Ok(FuzzReport {
            seed: self.config.seed,
            iterations: iteration,
            operations: iteration,
        })
    }

    /// Runs each target on a dedicated `std::thread` with a distinct seed offset.
    ///
    /// The returned vector contains one [`FuzzReport`] per target, in the same
    /// order as `targets`. A panic in any worker thread is reported as a
    /// [`FuzzError`].
    pub fn run_parallel(&self, targets: &[FuzzTarget]) -> Result<Vec<FuzzReport>, FuzzError> {
        let mut handles = Vec::with_capacity(targets.len());
        for (index, &target) in targets.iter().enumerate() {
            let config = FuzzConfig {
                seed: self.config.seed.wrapping_add(index as u64),
                ..self.config
            };
            let worker_seed = config.seed;
            let handle = thread::spawn(move || FuzzEngine::new(config).run_target(target));
            handles.push((target, worker_seed, handle));
        }

        let mut reports = Vec::with_capacity(handles.len());
        for (target, worker_seed, handle) in handles {
            let result = handle.join().map_err(|_| FuzzError {
                target,
                seed: worker_seed,
                iteration: 0,
                invariant: "worker thread panicked".into(),
            })?;
            reports.push(result?);
        }
        Ok(reports)
    }

    fn should_continue(&self, iteration: u64, elapsed: Duration) -> bool {
        let under_iterations = self.config.iterations.is_none_or(|limit| iteration < limit);
        let under_duration = self.config.duration.is_none_or(|limit| elapsed < limit);
        under_iterations && under_duration
    }

    fn error(&self, target: FuzzTarget, iteration: u64, invariant: String) -> FuzzError {
        FuzzError {
            target,
            seed: self.config.seed,
            iteration,
            invariant,
        }
    }
}

/// Runs a fuzz campaign with the supplied configuration.
///
/// # Examples
///
/// ```
/// use martensite_test::{run_fuzz_campaign, FuzzConfig};
/// let report = run_fuzz_campaign(FuzzConfig { seed: 42, iterations: Some(10), duration: None })?;
/// assert_eq!(report.operations, 30);
/// # Ok::<(), martensite_test::FuzzError>(())
/// ```
pub fn run_fuzz_campaign(config: FuzzConfig) -> Result<FuzzReport, FuzzError> {
    FuzzEngine::new(config).run()
}

/// Runs a deterministic fuzz campaign and panics with reproduction details on failure.
///
/// The two-argument form accepts a seed and iteration count. The one-argument
/// form accepts a complete [`FuzzConfig`].
#[macro_export]
macro_rules! fuzz {
    ($seed:expr, $iterations:expr) => {{
        $crate::run_fuzz_campaign($crate::FuzzConfig {
            seed: $seed,
            iterations: Some($iterations),
            duration: None,
        })
        .expect("fuzz campaign invariant failure")
    }};
    ($config:expr) => {{
        $crate::run_fuzz_campaign($config).expect("fuzz campaign invariant failure")
    }};
}

struct Generator(u64);

impl Generator {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn index(&mut self, len: usize) -> usize {
        (self.next() as usize) % len
    }
}

#[derive(Default)]
struct ArenaState {
    arena: WidgetArena,
    live: Vec<WidgetId>,
    stale: Vec<WidgetId>,
}

impl ArenaState {
    fn step(&mut self, rng: &mut Generator) -> Result<(), String> {
        if self.live.is_empty() || rng.next().is_multiple_of(3) {
            let hot = HotNode {
                bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
                flags: NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED | NodeFlags::FOCUSABLE,
                ..HotNode::default()
            };
            self.live.push(self.arena.insert(hot, ColdNode::default()));
        } else {
            let index = rng.index(self.live.len());
            let id = self.live.swap_remove(index);
            if self.arena.remove(id).is_none() {
                return Err("live ID could not be removed".into());
            }
            if self.arena.remove(id).is_some() {
                return Err("stale ID was removed twice".into());
            }
            self.stale.push(id);
        }
        if self.arena.len() != self.live.len() {
            return Err("dense length differs from live model".into());
        }
        if self.live.iter().any(|id| !self.arena.is_alive(*id)) {
            return Err("live ID became stale during compaction".into());
        }
        if self
            .stale
            .iter()
            .any(|id| self.arena.is_alive(*id) || self.arena.get_hot(*id).is_some())
        {
            return Err("stale ID survived generation change".into());
        }
        if self.arena.free_slots_len() + self.arena.len() != self.arena.slot_count() {
            return Err("free and occupied slots do not partition the arena".into());
        }
        Ok(())
    }
}

struct ReactiveEntry {
    signal: Signal<i64>,
    memo: Memo<i64>,
}

struct ReactiveState {
    runtime: Arc<ReactiveRuntime>,
    entries: Vec<ReactiveEntry>,
}

impl ReactiveState {
    fn new() -> Self {
        Self {
            runtime: ReactiveRuntime::new(),
            entries: Vec::new(),
        }
    }

    fn step(&mut self, rng: &mut Generator) -> Result<(), String> {
        if self.entries.is_empty() || rng.next().is_multiple_of(3) {
            let signal = Signal::new_with_runtime(rng.next() as i64, Arc::clone(&self.runtime));
            let source = signal.clone();
            let memo = Memo::new_with_runtime(
                move || source.get().wrapping_mul(2),
                Arc::clone(&self.runtime),
            );
            self.entries.push(ReactiveEntry { signal, memo });
        } else if rng.next().is_multiple_of(4) {
            let entry = self.entries.swap_remove(rng.index(self.entries.len()));
            self.runtime.unregister_node(entry.memo.id());
            self.runtime.unregister_node(entry.signal.id());
        } else {
            let entry = &self.entries[rng.index(self.entries.len())];
            entry.signal.set(rng.next() as i64);
            self.runtime.flush();
        }
        self.runtime
            .detect_cycles()
            .map_err(|error| error.to_string())?;
        for entry in &self.entries {
            if entry.memo.get_untracked() != entry.signal.get_untracked().wrapping_mul(2) {
                return Err("memo did not propagate its source value".into());
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct EventState {
    arena: ArenaState,
    router: EventRouter,
    focus: FocusManager,
    clipboard: InMemoryClipboard,
    dnd: DndSessionManager,
    sessions: Vec<SessionId>,
}

impl EventState {
    fn step(&mut self, rng: &mut Generator) -> Result<(), String> {
        self.arena.step(rng)?;
        if self.arena.live.is_empty() {
            return Ok(());
        }
        let id = self.arena.live[rng.index(self.arena.live.len())];
        let pointer = PointerId::new((rng.next() % 4) as u32);
        let state = match rng.next() % 3 {
            0 => PointerState::Pressed,
            1 => PointerState::Released,
            _ => PointerState::Moved,
        };
        let event = PointerEvent {
            pointer_id: pointer,
            position: glam::Vec2::new((rng.next() % 200) as f32, (rng.next() % 200) as f32),
            state,
            button: (state != PointerState::Moved).then_some(MouseButton::Left),
            modifiers: ModifierKeys::from_bits_truncate(rng.next() as u8),
        };
        match rng.next() % 6 {
            0 => {
                self.router.capture_pointer(event.pointer_id, id);
            }
            1 => {
                self.router.release_pointer(event.pointer_id);
            }
            2 => {
                self.focus.set_focus(&mut self.arena.arena, id);
                if self.router.route_keyboard_event(self.focus.current_focus())
                    != EventDispatchOutcome::Handled(id)
                {
                    return Err("keyboard event disagreed with focus state".into());
                }
            }
            3 => {
                let _ = self.focus.tab(
                    &self.arena.arena,
                    if rng.next().is_multiple_of(2) {
                        TabNavigation::Forward
                    } else {
                        TabNavigation::Reverse
                    },
                );
            }
            4 => {
                let text = rng.next().to_string();
                self.clipboard
                    .set_contents(&ClipboardItem::new().offer_text(&text));
                if self.clipboard.get_contents("text/plain;charset=utf-8")
                    != Some(text.into_bytes())
                {
                    return Err("clipboard read did not match write".into());
                }
            }
            _ => {
                let session =
                    self.dnd
                        .start_session(Arc::new(event), vec!["text/plain".into()], None);
                self.sessions.push(session);
                if rng.next().is_multiple_of(2) {
                    let _ = self.dnd.complete_session(session, DropEffect::Copy);
                } else {
                    let _ = self.dnd.cancel_session(session);
                }
                let _ = self.dnd.purge_completed();
            }
        }
        if let Some(focused) = self.focus.current_focus() {
            if !self.arena.arena.is_alive(focused) {
                self.focus.clear_focus();
            }
        }
        for raw_pointer in 0..4 {
            let pointer = PointerId::new(raw_pointer);
            if let Some(captured) = self.router.captured_widget(pointer) {
                if !self.arena.arena.is_alive(captured) {
                    self.router.release_pointer(pointer);
                }
            }
        }
        if self
            .sessions
            .iter()
            .filter(|id| self.dnd.session(**id).is_some())
            .count()
            != self.dnd.active_sessions()
        {
            return Err("drag-and-drop session count diverged from model".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_campaign_is_deterministic() {
        let config = FuzzConfig {
            seed: 17,
            iterations: Some(25),
            duration: None,
        };
        assert_eq!(run_fuzz_campaign(config), run_fuzz_campaign(config));
    }

    #[test]
    fn zero_iterations_is_valid() {
        let report = run_fuzz_campaign(FuzzConfig {
            seed: 1,
            iterations: Some(0),
            duration: None,
        })
        .unwrap();
        assert_eq!(report.operations, 0);
    }

    #[test]
    fn fuzz_macro_runs_smoke_campaign() {
        let report = crate::fuzz!(99, 10);
        assert_eq!(report.iterations, 10);
    }

    #[test]
    fn run_parallel_executes_all_targets() {
        let targets = &[
            FuzzTarget::ArenaCompaction,
            FuzzTarget::ReactiveDag,
            FuzzTarget::EventRouting,
        ];
        let reports = FuzzEngine::new(FuzzConfig::quick_parallel(123))
            .run_parallel(targets)
            .expect("parallel fuzz run should succeed");
        assert_eq!(reports.len(), targets.len());
        for report in reports {
            assert_eq!(report.iterations, 100);
        }
    }
}
