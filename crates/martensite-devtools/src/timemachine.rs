//! Hybrid command-ledger + snapshot time-travel debugger.
//!
//! Compiled only with the `devtools-timemachine` feature. Martensite is a
//! retained-arena + push-pull reactive-graph framework, so a pure
//! message-log replay model does not fit: the debugger journals
//! *user-meaningful mutations* as [`ChangeOp`]s against a [`World`]
//! (widget arena + reactive runtime) inside a [`HistoryLedger`], and
//! takes periodic [`Checkpoint`]s covering
//!
//! - [`WidgetArena`] hot/cold state (structure, bounds, metadata, and
//!   widget-internal [`TimemachineState`](martensite_core::TimemachineState)),
//! - [`ReactiveRuntime`] source-signal values (via
//!   [`SignalSnapshot`](martensite_reactive::SignalSnapshot)).
//!
//! [`TimeMachine::replay_to`] restores the nearest ancestor checkpoint
//! and re-applies the journal forward — never reverting — while a
//! [`JournalGuard`](martensite_reactive::JournalGuard) suppresses
//! re-journaling of replayed source writes. [`Memo`](martensite_reactive::Memo)s
//! are derived state and recompute lazily during the pull phase; only
//! source writes are journaled.
//!
//! # Determinism contract
//!
//! Deterministic replay requires deterministic command inputs: drive
//! time through `martensite-test`'s `VirtualClock`, use seeded RNG and
//! deterministic task ordering, and mock I/O while replaying. The
//! debugger guarantees its own half: replay applies the same ops in the
//! same order with journaling suppressed, and
//! [`WidgetArena::state_fingerprint`] + signal values provide the
//! replayed-state == recorded-state equality check.
//!
//! # Examples
//!
//! ```
//! use martensite_devtools::timemachine::{TimeMachine, World};
//! use martensite_reactive::{ReactiveRuntime, Signal};
//!
//! let runtime = ReactiveRuntime::new();
//! let count = Signal::new_with_runtime(0i32, runtime.clone());
//! let mut tm = TimeMachine::new(World::new(Default::default(), runtime));
//!
//! tm.set_signal(&count, 41);
//! let tip = tm.current_node();
//! tm.set_signal(&count, 99);
//!
//! tm.replay_to(tip).unwrap();
//! assert_eq!(count.get_untracked(), 41);
//! ```
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use martensite_core::{ArenaRestoreError, ArenaState, Widget, WidgetArena, WidgetId};
use martensite_history::{ChangeOp, HistoryLedger, LedgerError, NodeId};
use martensite_reactive::{JournalGuard, ReactiveRuntime, Signal, SignalSnapshot, SourceJournal};
use parking_lot::MutexGuard;

/// The mutable world a [`ChangeOp`] operates on: the widget arena plus
/// the reactive runtime.
///
/// `TimeMachine` owns one `World` as its ledger state — journal entries
/// are `Box<dyn ChangeOp<World>>`. Signals bind to the runtime's `Arc`,
/// so signal ops hold their own handle and need no access to `World` at
/// all; arena ops mutate `world.arena`.
///
/// # Examples
///
/// ```
/// use martensite_devtools::timemachine::World;
/// use martensite_reactive::ReactiveRuntime;
///
/// let world = World::new(Default::default(), ReactiveRuntime::new());
/// assert_eq!(world.arena().len(), 0);
/// ```
pub struct World {
    arena: WidgetArena,
    runtime: Arc<ReactiveRuntime>,
}

impl World {
    /// Creates a `World` from an arena and a shared reactive runtime.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::World;
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let world = World::new(
    ///     martensite_core::WidgetArena::new(),
    ///     ReactiveRuntime::new(),
    /// );
    /// ```
    pub fn new(arena: WidgetArena, runtime: Arc<ReactiveRuntime>) -> Self {
        Self { arena, runtime }
    }

    /// Returns the widget arena.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::World;
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let world = World::new(Default::default(), ReactiveRuntime::new());
    /// assert_eq!(world.arena().len(), 0);
    /// ```
    #[inline]
    pub fn arena(&self) -> &WidgetArena {
        &self.arena
    }

    /// Returns the widget arena mutably. Mutations made this way are
    /// *not* journaled — use [`TimeMachine::commit`] for recorded
    /// mutations.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::World;
    /// use martensite_core::{DummyWidget, HotNode};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let mut world = World::new(Default::default(), ReactiveRuntime::new());
    /// world
    ///     .arena_mut()
    ///     .insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// assert_eq!(world.arena().len(), 1);
    /// ```
    #[inline]
    pub fn arena_mut(&mut self) -> &mut WidgetArena {
        &mut self.arena
    }

    /// Returns the shared reactive runtime.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::World;
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let world = World::new(Default::default(), ReactiveRuntime::new());
    /// assert_eq!(world.runtime().journal().len(), 0);
    /// ```
    #[inline]
    pub fn runtime(&self) -> &Arc<ReactiveRuntime> {
        &self.runtime
    }
}

impl fmt::Debug for World {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("World")
            .field("arena_len", &self.arena.len())
            .finish_non_exhaustive()
    }
}

/// A point-in-time capture of a [`World`] at one history node.
///
/// Covers the arena's hot/cold state and every registered source-signal
/// value. Produced by [`TimeMachine::checkpoint`] (manually or via the
/// configured interval) and consumed by [`TimeMachine::replay_to`].
///
/// # Examples
///
/// ```
/// use martensite_devtools::timemachine::{TimeMachine, World};
/// use martensite_reactive::ReactiveRuntime;
///
/// let runtime = ReactiveRuntime::new();
/// let mut tm = TimeMachine::new(World::new(Default::default(), runtime));
/// // `TimeMachine::new` already captured the root checkpoint.
/// let cp = tm.checkpoint_at(tm.current_node()).unwrap();
/// assert_eq!(cp.frame(), 0);
/// ```
pub struct Checkpoint {
    /// History node this checkpoint was captured at.
    node: NodeId,
    /// Monotonic commit counter at capture time.
    frame: u64,
    /// Arena hot/cold state.
    arena: ArenaState,
    /// Source-signal values.
    signals: SignalSnapshot,
    /// `WidgetArena::state_fingerprint` at capture time.
    arena_fingerprint: u64,
}

impl Checkpoint {
    /// The history node this checkpoint was captured at.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let mut tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ));
    /// let node = tm.checkpoint();
    /// assert_eq!(tm.checkpoint_at(node).unwrap().node(), node);
    /// ```
    #[inline]
    pub fn node(&self) -> NodeId {
        self.node
    }

    /// The commit counter value at capture time (0 for checkpoints taken
    /// before any commit).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let mut tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ));
    /// assert_eq!(tm.checkpoint_at(tm.current_node()).unwrap().frame(), 0);
    /// ```
    #[inline]
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// The arena fingerprint captured alongside the snapshot — compare
    /// against [`WidgetArena::state_fingerprint`] after replay for the
    /// replayed-state == recorded-state check.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let mut tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ));
    /// let cp = tm.checkpoint_at(tm.current_node()).unwrap();
    /// assert_eq!(cp.arena_fingerprint(), tm.arena_fingerprint());
    /// ```
    #[inline]
    pub fn arena_fingerprint(&self) -> u64 {
        self.arena_fingerprint
    }

    /// The captured source-signal values.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let s = Signal::new_with_runtime(3i32, runtime.clone());
    /// let _ = s.get_untracked(); // registers the source for snapshots
    /// let mut tm = TimeMachine::new(World::new(Default::default(), runtime));
    /// let node = tm.checkpoint();
    /// let cp = tm.checkpoint_at(node).unwrap();
    /// assert_eq!(cp.signals().get::<i32>(s.id()), Some(&3));
    /// ```
    #[inline]
    pub fn signals(&self) -> &SignalSnapshot {
        &self.signals
    }
}

impl fmt::Debug for Checkpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Checkpoint")
            .field("node", &self.node)
            .field("frame", &self.frame)
            .field("arena_fingerprint", &self.arena_fingerprint)
            .field("signals", &self.signals.len())
            .finish()
    }
}

/// Errors returned by [`TimeMachine::replay_to`].
///
/// # Examples
///
/// ```
/// use martensite_devtools::timemachine::{ReplayError, TimeMachine, World};
/// use martensite_reactive::ReactiveRuntime;
///
/// let mut tm = TimeMachine::new(World::new(
///     Default::default(),
///     ReactiveRuntime::new(),
/// ));
/// // A stale/foreign node id cannot be replayed to.
/// let stale = martensite_history::NodeId::default();
/// assert!(tm.replay_to(stale).is_err());
/// ```
#[derive(Debug)]
pub enum ReplayError {
    /// No checkpoint exists on the path from the target to the root.
    /// Cannot happen for a `TimeMachine` created by
    /// [`TimeMachine::new`] (which checkpoints the root), but can after
    /// [`TimeMachine::clear_checkpoints`].
    NoCheckpoint,
    /// The history ledger rejected the replay (invalid node, or the
    /// checkpoint is not an ancestor of the target).
    Ledger(LedgerError),
    /// Restoring the arena snapshot failed — e.g. snapshot widgets no
    /// longer alive and no factory could reconstruct them.
    Arena(ArenaRestoreError),
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCheckpoint => write!(f, "no checkpoint on the path to the target"),
            Self::Ledger(e) => write!(f, "history ledger error: {e}"),
            Self::Arena(e) => write!(f, "arena restore error: {e}"),
        }
    }
}

impl std::error::Error for ReplayError {}

/// A journaled `Signal::set` against a [`World`].
///
/// `apply` writes `new` and `revert` writes `previous` — both through
/// the normal `Signal::set` path, so subscribers are marked dirty and
/// [`Memo`](martensite_reactive::Memo)s recompute lazily on pull. During
/// [`TimeMachine::replay_to`] and [`TimeMachine::jump_to`] the writes are
/// applied under a [`JournalGuard`] so they are not re-journaled.
///
/// Construct via [`TimeMachine::set_signal`] (which captures `previous`
/// automatically) or directly.
///
/// # Examples
///
/// ```
/// use martensite_devtools::timemachine::{SignalWrite, World};
/// use martensite_history::ChangeOp;
/// use martensite_reactive::{ReactiveRuntime, Signal};
///
/// let runtime = ReactiveRuntime::new();
/// let flag = Signal::new_with_runtime(false, runtime.clone());
///
/// let op = SignalWrite::new(&flag, true);
/// assert_eq!(op.previous(), &false);
/// assert_eq!(op.new_value(), &true);
/// ```
pub struct SignalWrite<T: Send + Sync + 'static> {
    signal: Signal<T>,
    previous: T,
    new: T,
}

impl<T: Clone + Send + Sync + 'static> SignalWrite<T> {
    /// Captures a `set(signal, value)` operation, reading `previous`
    /// from the signal's current value.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::SignalWrite;
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let n = Signal::new_with_runtime(1i32, runtime.clone());
    /// let op = SignalWrite::new(&n, 2);
    /// assert_eq!(op.previous(), &1);
    /// ```
    pub fn new(signal: &Signal<T>, value: T) -> Self {
        Self {
            signal: signal.clone(),
            previous: signal.get_untracked(),
            new: value,
        }
    }

    /// The value the signal held when the op was constructed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::SignalWrite;
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let n = Signal::new_with_runtime(5i32, runtime.clone());
    /// let op = SignalWrite::new(&n, 9);
    /// assert_eq!(op.previous(), &5);
    /// ```
    #[inline]
    pub fn previous(&self) -> &T {
        &self.previous
    }

    /// The value written by `apply`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::SignalWrite;
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let n = Signal::new_with_runtime(5i32, runtime.clone());
    /// let op = SignalWrite::new(&n, 9);
    /// assert_eq!(op.new_value(), &9);
    /// ```
    #[inline]
    pub fn new_value(&self) -> &T {
        &self.new
    }

    /// The signal this write targets.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::SignalWrite;
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let n = Signal::new_with_runtime(5i32, runtime.clone());
    /// let op = SignalWrite::new(&n, 9);
    /// assert_eq!(op.signal().id(), n.id());
    /// ```
    #[inline]
    pub fn signal(&self) -> &Signal<T> {
        &self.signal
    }
}

impl<T: Clone + Send + Sync + 'static> ChangeOp<World> for SignalWrite<T> {
    fn apply(&self, _world: &mut World) {
        self.signal.set(self.new.clone());
    }

    fn revert(&self, _world: &mut World) {
        self.signal.set(self.previous.clone());
    }
}

/// Reconstructs a widget object for `id` during replay when the
/// original widget is no longer alive in the arena.
///
/// See [`TimeMachine::with_widget_factory`].
pub type WidgetFactory = dyn FnMut(WidgetId) -> Option<Box<dyn Widget>> + Send;

/// Hybrid command-ledger + snapshot time-travel debugger.
///
/// Owns a [`World`] (widget arena + reactive runtime) and a
/// [`HistoryLedger`] of `Box<dyn ChangeOp<World>>` commands. Committed
/// commands are journaled into the branching history tree; checkpoints
/// snapshot arena + source-signal state so [`replay_to`](Self::replay_to)
/// can restore the nearest ancestor and re-apply forward.
///
/// # Checkpoint policy
///
/// A checkpoint is always captured at the root on construction and is
/// pinned — it is never evicted, so [`replay_to`](Self::replay_to)
/// always has a checkpoint on the ancestor chain. Pass a
/// `checkpoint_every` interval to [`with_checkpoint_interval`] to capture
/// every N commits automatically, and/or call [`checkpoint`](Self::checkpoint)
/// manually at user-meaningful boundaries. At most `max_checkpoints`
/// checkpoints are retained; the oldest (by commit order, root excepted)
/// are evicted.
///
/// # Examples
///
/// ```
/// use martensite_devtools::timemachine::{TimeMachine, World};
/// use martensite_reactive::ReactiveRuntime;
///
/// let mut tm = TimeMachine::new(World::new(
///     Default::default(),
///     ReactiveRuntime::new(),
/// ));
/// assert_eq!(tm.current_node(), tm.root_node());
/// assert_eq!(tm.checkpoint_count(), 1, "root checkpoint");
/// ```
pub struct TimeMachine {
    /// Command journal: the branching history of `ChangeOp<World>`s.
    ledger: HistoryLedger<World>,
    /// Snapshots keyed by the node they were captured at.
    checkpoints: HashMap<NodeId, Checkpoint>,
    /// Auto-checkpoint interval in commits; 0 disables auto-checkpoints.
    checkpoint_every: u64,
    /// Maximum retained checkpoints (oldest evicted first — the root
    /// checkpoint is pinned and never evicted).
    max_checkpoints: usize,
    /// Monotonic count of committed commands.
    commit_counter: u64,
    /// Optional factory for reconstructing widget objects that were
    /// removed after a checkpoint was captured.
    widget_factory: Option<Box<WidgetFactory>>,
}

impl TimeMachine {
    /// Default number of retained checkpoints.
    pub const DEFAULT_MAX_CHECKPOINTS: usize = 64;

    /// Default maximum number of history nodes before pruning.
    pub const DEFAULT_MAX_NODES: usize = 1024;

    /// Creates a `TimeMachine` over `world`, capturing a checkpoint at
    /// the root.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ));
    /// assert_eq!(tm.checkpoint_count(), 1);
    /// ```
    pub fn new(world: World) -> Self {
        let mut tm = Self {
            ledger: HistoryLedger::new(world, Self::DEFAULT_MAX_NODES),
            checkpoints: HashMap::new(),
            checkpoint_every: 0,
            max_checkpoints: Self::DEFAULT_MAX_CHECKPOINTS,
            commit_counter: 0,
            widget_factory: None,
        };
        tm.checkpoint();
        tm
    }

    /// Sets the auto-checkpoint interval: every `every` commits captures
    /// a checkpoint. `0` disables auto-checkpoints (manual
    /// [`checkpoint`](Self::checkpoint) only). Returns `self` for
    /// builder-style setup.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ))
    /// .with_checkpoint_interval(10);
    /// ```
    pub fn with_checkpoint_interval(mut self, every: u64) -> Self {
        self.checkpoint_every = every;
        self
    }

    /// Sets the maximum number of retained checkpoints.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ))
    /// .with_max_checkpoints(8);
    /// ```
    pub fn with_max_checkpoints(mut self, max: usize) -> Self {
        self.max_checkpoints = max.max(1);
        self
    }

    /// Creates a `TimeMachine` with a custom history depth limit
    /// (`max_nodes` nodes before the tree prunes).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let tm = TimeMachine::with_max_nodes(
    ///     World::new(Default::default(), ReactiveRuntime::new()),
    ///     256,
    /// );
    /// ```
    pub fn with_max_nodes(world: World, max_nodes: usize) -> Self {
        let mut tm = Self {
            ledger: HistoryLedger::new(world, max_nodes.max(2)),
            checkpoints: HashMap::new(),
            checkpoint_every: 0,
            max_checkpoints: Self::DEFAULT_MAX_CHECKPOINTS,
            commit_counter: 0,
            widget_factory: None,
        };
        tm.checkpoint();
        tm
    }

    /// Registers a factory used by [`replay_to`](Self::replay_to) to
    /// reconstruct widget objects whose `WidgetId`s appear in the
    /// checkpoint but are no longer alive in the arena.
    ///
    /// Without a factory, replaying into a checkpoint that references
    /// removed widgets fails with
    /// [`ReplayError::Arena`](ReplayError::Arena)
    /// ([`ArenaRestoreError::MissingWidgets`]) before any state is
    /// mutated. The factory may be invoked during replay *validation*
    /// (before any world mutation) to confirm every missing widget can
    /// be reconstructed; the widgets it returns are kept and receive
    /// their captured [`TimemachineState`] on restore.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_core::DummyWidget;
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let tm = TimeMachine::new(World::new(Default::default(), ReactiveRuntime::new()))
    ///     .with_widget_factory(|_id| Some(Box::new(DummyWidget)));
    /// ```
    pub fn with_widget_factory(
        mut self,
        factory: impl FnMut(WidgetId) -> Option<Box<dyn Widget>> + Send + 'static,
    ) -> Self {
        self.widget_factory = Some(Box::new(factory));
        self
    }

    /// Returns the world (arena + reactive runtime).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ));
    /// assert_eq!(tm.world().arena().len(), 0);
    /// ```
    #[inline]
    pub fn world(&self) -> &World {
        self.ledger.state()
    }

    /// Returns the world mutably. Direct mutations are *not* journaled —
    /// use [`commit`](Self::commit) for recorded mutations.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let mut tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ));
    /// tm.world_mut().arena_mut();
    /// ```
    #[inline]
    pub fn world_mut(&mut self) -> &mut World {
        self.ledger.state_mut()
    }

    /// Returns the underlying command journal.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ));
    /// assert_eq!(tm.ledger().node_count(), 1);
    /// ```
    #[inline]
    pub fn ledger(&self) -> &HistoryLedger<World> {
        &self.ledger
    }

    /// Returns the current history node.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ));
    /// assert_eq!(tm.current_node(), tm.root_node());
    /// ```
    #[inline]
    pub fn current_node(&self) -> NodeId {
        self.ledger.current_node()
    }

    /// Returns the root history node.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ));
    /// let _ = tm.root_node();
    /// ```
    #[inline]
    pub fn root_node(&self) -> NodeId {
        self.ledger.root_node()
    }

    /// Returns the number of commands committed so far — the monotonic
    /// "frame" counter recorded on each [`Checkpoint`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let n = Signal::new_with_runtime(0i32, runtime.clone());
    /// let mut tm = TimeMachine::new(World::new(Default::default(), runtime));
    /// assert_eq!(tm.commit_count(), 0);
    /// tm.set_signal(&n, 1);
    /// assert_eq!(tm.commit_count(), 1);
    /// ```
    #[inline]
    pub fn commit_count(&self) -> u64 {
        self.commit_counter
    }

    /// Commits a user-meaningful command: applies it to the world and
    /// appends it to the journal on a new history node.
    ///
    /// Source-signal writes performed by the op *are* recorded in the
    /// reactive [`SourceJournal`] (they are real user writes). Returns
    /// the new current node, and auto-checkpoints if the configured
    /// interval was reached.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{SignalWrite, TimeMachine, World};
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let n = Signal::new_with_runtime(0i32, runtime.clone());
    /// let mut tm = TimeMachine::new(World::new(Default::default(), runtime));
    ///
    /// let node = tm.commit(Box::new(SignalWrite::new(&n, 7)));
    /// assert_eq!(n.get_untracked(), 7);
    /// assert_eq!(tm.current_node(), node);
    /// ```
    pub fn commit(&mut self, op: Box<dyn ChangeOp<World>>) -> NodeId {
        self.ledger.commit(op);
        let node = self.ledger.current_node();
        self.commit_counter += 1;
        if self.checkpoint_every > 0 && self.commit_counter.is_multiple_of(self.checkpoint_every) {
            self.checkpoint();
        }
        node
    }

    /// Commits a `Signal::set` as a journaled [`SignalWrite`] command.
    ///
    /// The write is recorded in the history journal (revertible,
    /// replayable) *and* the reactive [`SourceJournal`] audit log.
    /// Returns the new node.
    ///
    /// Note the asymmetry: only commands committed through
    /// `commit`/`set_signal` are visible to [`replay_to`](Self::replay_to).
    /// A direct `signal.set(...)` outside a command is captured by the
    /// `SourceJournal` audit log but is *not* replayable — replay
    /// restores source values from checkpoints and re-applies committed
    /// commands only.
    ///
    /// `signal` must be bound to the world's runtime; passing a signal
    /// from another runtime is a logic error (debug-asserted).
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let n = Signal::new_with_runtime(0i32, runtime.clone());
    /// let mut tm = TimeMachine::new(World::new(Default::default(), runtime));
    ///
    /// tm.set_signal(&n, 42);
    /// assert_eq!(n.get_untracked(), 42);
    /// assert_eq!(tm.source_journal().len(), 1);
    /// ```
    pub fn set_signal<T: Clone + Send + Sync + 'static>(
        &mut self,
        signal: &Signal<T>,
        value: T,
    ) -> NodeId {
        debug_assert!(
            Arc::ptr_eq(signal.runtime(), self.world().runtime()),
            "set_signal: signal is bound to a different ReactiveRuntime than the world's"
        );
        self.commit(Box::new(SignalWrite::new(signal, value)))
    }

    /// Captures a [`Checkpoint`] at the current history node. Returns
    /// the checkpointed node.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let mut tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ));
    /// let node = tm.checkpoint();
    /// assert_eq!(node, tm.current_node());
    /// assert_eq!(tm.checkpoint_count(), 1, "root checkpoint replaced");
    /// ```
    pub fn checkpoint(&mut self) -> NodeId {
        let node = self.ledger.current_node();
        let world = self.ledger.state();
        let checkpoint = Checkpoint {
            node,
            frame: self.commit_counter,
            arena_fingerprint: world.arena.state_fingerprint(),
            arena: world.arena.snapshot_state(),
            signals: world.runtime.snapshot_signals(),
        };
        self.checkpoints.insert(node, checkpoint);
        self.evict_checkpoints();
        node
    }

    /// Returns the checkpoint captured at `node`, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let mut tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ));
    /// assert!(tm.checkpoint_at(tm.current_node()).is_some());
    /// ```
    #[inline]
    pub fn checkpoint_at(&self, node: NodeId) -> Option<&Checkpoint> {
        self.checkpoints.get(&node)
    }

    /// Returns the number of retained checkpoints.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let mut tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ));
    /// assert_eq!(tm.checkpoint_count(), 1);
    /// ```
    #[inline]
    pub fn checkpoint_count(&self) -> usize {
        self.checkpoints.len()
    }

    /// Drops every checkpoint. After this call,
    /// [`replay_to`](Self::replay_to) returns
    /// [`ReplayError::NoCheckpoint`] until a new checkpoint is taken.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let mut tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ));
    /// tm.clear_checkpoints();
    /// assert_eq!(tm.checkpoint_count(), 0);
    /// ```
    pub fn clear_checkpoints(&mut self) {
        self.checkpoints.clear();
    }

    /// Replays from the nearest ancestor checkpoint to `target`:
    /// restores the checkpoint's arena + signal snapshot, then
    /// re-applies every journaled command on the path forward.
    ///
    /// "Nearest" is measured by history-tree depth — the deepest
    /// checkpointed ancestor of `target`. Source writes made by
    /// replayed commands are applied under a [`JournalGuard`] so they
    /// are not re-journaled; derived [`Memo`]s recompute lazily on the
    /// next pull. Returns the node of the checkpoint that was restored.
    ///
    /// `target` must be a descendant (or self) of the nearest
    /// checkpoint — guaranteed whenever a checkpoint exists on the
    /// ancestor chain, which is always true unless
    /// [`clear_checkpoints`](Self::clear_checkpoints) ran. For
    /// backward or cross-branch scrubbing use [`jump_to`](Self::jump_to).
    ///
    /// # Failure atomicity
    ///
    /// If the checkpoint references widgets that are no longer alive,
    /// the registered [`with_widget_factory`](Self::with_widget_factory)
    /// is invoked during validation — before the world is touched — and
    /// a `MissingWidgets` failure leaves the world *and* the history
    /// cursor unchanged. A `RestoreRejected` failure (a widget refusing
    /// its captured state) can leave partially-restored widget
    /// internals; the signal snapshot is still installed, journal ops
    /// are still re-applied, and the cursor lands on `target`, so the
    /// world reflects the replayed commands except for the rejected
    /// widget state.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let n = Signal::new_with_runtime(0i32, runtime.clone());
    /// let mut tm = TimeMachine::new(World::new(Default::default(), runtime));
    ///
    /// let a = tm.set_signal(&n, 1);
    /// tm.checkpoint();
    /// let b = tm.set_signal(&n, 2);
    ///
    /// tm.replay_to(b).unwrap();
    /// assert_eq!(n.get_untracked(), 2);
    /// ```
    pub fn replay_to(&mut self, target: NodeId) -> Result<NodeId, ReplayError> {
        // Nearest ancestor-or-self of `target` holding a checkpoint —
        // the deepest one on the ancestor chain.
        let cp_node = self
            .checkpoints
            .keys()
            .filter(|&&node| self.ledger.is_ancestor(node, target))
            .max_by_key(|&&node| self.ledger.depth_of(node).unwrap_or(0))
            .copied()
            .ok_or(ReplayError::NoCheckpoint)?;

        // Pre-flight: every widget the checkpoint needs must be alive or
        // reconstructable — *before* any state is mutated or the cursor
        // moves.
        let missing = self
            .world()
            .arena()
            .missing_snapshot_widgets(&self.checkpoints[&cp_node].arena);
        let mut fabricated: HashMap<WidgetId, Box<dyn Widget>> = HashMap::new();
        if !missing.is_empty() {
            let Some(factory) = self.widget_factory.as_mut() else {
                return Err(ReplayError::Arena(ArenaRestoreError::MissingWidgets(
                    missing,
                )));
            };
            for &id in &missing {
                match factory(id) {
                    Some(widget) => {
                        fabricated.insert(id, widget);
                    }
                    None => {
                        return Err(ReplayError::Arena(ArenaRestoreError::MissingWidgets(
                            missing,
                        )));
                    }
                }
            }
        }

        let runtime = Arc::clone(self.world().runtime());
        // Replayed commands must not re-journal their source writes.
        let _guard = runtime.suppress_journal();

        let checkpoint = &self.checkpoints[&cp_node];
        let mut arena_err = None;
        self.ledger
            .replay(cp_node, target, |world| {
                // Signals restore is infallible (dead/typed-mismatched
                // sources are skipped) — install it first so replayed
                // ops see the snapshot baseline even if the arena
                // restore partially fails.
                world.runtime.restore_signals(&checkpoint.signals);
                let mut factory = |id: WidgetId| fabricated.remove(&id);
                if let Err(e) = world
                    .arena
                    .restore_state_with(&checkpoint.arena, &mut factory)
                {
                    arena_err = Some(e);
                }
            })
            .map_err(ReplayError::Ledger)?;

        if let Some(e) = arena_err {
            return Err(ReplayError::Arena(e));
        }
        Ok(cp_node)
    }

    /// Navigates to any node in the history tree using LCA revert/apply
    /// — no snapshots involved. Unlike [`replay_to`](Self::replay_to)
    /// this can move backward and across branches. Source writes made by
    /// reverted/re-applied commands run under a [`JournalGuard`] and are
    /// not journaled.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let n = Signal::new_with_runtime(0i32, runtime.clone());
    /// let mut tm = TimeMachine::new(World::new(Default::default(), runtime));
    ///
    /// let a = tm.set_signal(&n, 1);
    /// let b = tm.set_signal(&n, 2);
    /// tm.jump_to(a).unwrap();
    /// assert_eq!(n.get_untracked(), 1);
    /// ```
    pub fn jump_to(&mut self, target: NodeId) -> Result<(), LedgerError> {
        let runtime = Arc::clone(self.world().runtime());
        let _guard = runtime.suppress_journal();
        self.ledger.jump_to(target)
    }

    /// Undoes the last command on the current branch (suppressed —
    /// navigation writes are not journaled).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let n = Signal::new_with_runtime(0i32, runtime.clone());
    /// let mut tm = TimeMachine::new(World::new(Default::default(), runtime));
    ///
    /// tm.set_signal(&n, 1);
    /// tm.undo().unwrap();
    /// assert_eq!(n.get_untracked(), 0);
    /// ```
    pub fn undo(&mut self) -> Result<(), LedgerError> {
        let runtime = Arc::clone(self.world().runtime());
        let _guard = runtime.suppress_journal();
        self.ledger.undo()
    }

    /// Redoes the next command on the current branch (suppressed).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let n = Signal::new_with_runtime(0i32, runtime.clone());
    /// let mut tm = TimeMachine::new(World::new(Default::default(), runtime));
    ///
    /// tm.set_signal(&n, 1);
    /// tm.undo().unwrap();
    /// tm.redo().unwrap();
    /// assert_eq!(n.get_untracked(), 1);
    /// ```
    pub fn redo(&mut self) -> Result<(), LedgerError> {
        let runtime = Arc::clone(self.world().runtime());
        let _guard = runtime.suppress_journal();
        self.ledger.redo()
    }

    /// Suppresses source-write journaling until the returned guard
    /// drops — the `debug::disable`-style guard for performing writes
    /// that must not enter the audit log (e.g. replay scaffolding or
    /// mocked I/O driven state).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let n = Signal::new_with_runtime(0i32, runtime.clone());
    /// let tm = TimeMachine::new(World::new(Default::default(), runtime));
    ///
    /// {
    ///     let _guard = tm.suppress_journal();
    ///     n.set(5); // applied but not journaled
    /// }
    /// assert_eq!(tm.source_journal().len(), 0);
    /// ```
    pub fn suppress_journal(&self) -> JournalGuard<'_> {
        self.world().runtime().suppress_journal()
    }

    /// Returns a lock guard on the reactive source-write journal —
    /// the audit log of every `Signal::set`/`set_if_changed` against
    /// the world's runtime.
    ///
    /// **Deadlock warning**: `Signal::set`/`set_if_changed` take the
    /// journal lock while writing. Do not write any signal bound to
    /// this runtime while holding the returned guard — the write will
    /// block on the journal lock you already hold.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::{ReactiveRuntime, Signal};
    ///
    /// let runtime = ReactiveRuntime::new();
    /// let n = Signal::new_with_runtime(0i32, runtime.clone());
    /// let mut tm = TimeMachine::new(World::new(Default::default(), runtime));
    ///
    /// tm.set_signal(&n, 4);
    /// assert_eq!(tm.source_journal().len(), 1);
    /// ```
    pub fn source_journal(&self) -> MutexGuard<'_, SourceJournal> {
        self.world().runtime().journal()
    }

    /// Deterministic fingerprint of the arena's current state — covers
    /// arena structure, hot/cold metadata, and widget-internal
    /// `TimemachineState`, but **not** source-signal values. For the
    /// replayed-state == recorded-state check, compare signal values
    /// separately (live reads or [`Checkpoint::signals`]).
    /// # Examples
    ///
    /// ```
    /// use martensite_devtools::timemachine::{TimeMachine, World};
    /// use martensite_reactive::ReactiveRuntime;
    ///
    /// let tm = TimeMachine::new(World::new(
    ///     Default::default(),
    ///     ReactiveRuntime::new(),
    /// ));
    /// assert_eq!(tm.arena_fingerprint(), tm.arena_fingerprint());
    /// ```
    #[inline]
    pub fn arena_fingerprint(&self) -> u64 {
        self.world().arena().state_fingerprint()
    }

    /// Evicts the oldest checkpoints beyond `max_checkpoints`. The root
    /// checkpoint is pinned and never evicted, so `replay_to` always
    /// has a checkpoint on the ancestor chain.
    fn evict_checkpoints(&mut self) {
        let root = self.ledger.root_node();
        while self.checkpoints.len() > self.max_checkpoints {
            let oldest = self
                .checkpoints
                .iter()
                .filter(|(n, _)| **n != root)
                .min_by_key(|(_, cp)| cp.frame)
                .map(|(n, _)| *n);
            match oldest {
                Some(node) => {
                    self.checkpoints.remove(&node);
                }
                None => break,
            }
        }
    }
}

impl fmt::Debug for TimeMachine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimeMachine")
            .field("nodes", &self.ledger.node_count())
            .field("checkpoints", &self.checkpoints.len())
            .field("commits", &self.commit_counter)
            .finish()
    }
}
