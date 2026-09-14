//! Arena state snapshots and fingerprints for time-travel debugging.
//!
//! Compiled only with the `devtools-timemachine` feature. An
//! [`ArenaState`] captures everything in a [`WidgetArena`] that can be
//! checkpointed without cloning widget objects: the slot indirection
//! table, dense [`HotNode`] records, hierarchy links, cold-node metadata,
//! and — for widgets that opt in — each widget's internal state via
//! [`Widget::timemachine_snapshot`].
//!
//! Widget *objects* are never copied: [`WidgetArena::restore_state`]
//! reuses the live [`Box<dyn Widget>`] for every `WidgetId` that is still
//! alive and reconstructs missing ones through a caller-supplied factory
//! ([`WidgetArena::restore_state_with`]). Journaled insert/remove
//! commands rebuild structure; snapshots restore state.
//!
//! [`WidgetArena::state_fingerprint`] produces a deterministic `u64`
//! fingerprint of the whole arena — the equality primitive used to
//! assert that replayed state matches recorded state.
//!
//! # Examples
//!
//! ```
//! use martensite_core::{DummyWidget, HotNode, WidgetArena};
//!
//! let mut arena = WidgetArena::new();
//! let root = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
//!
//! let snapshot = arena.snapshot_state();
//! let fp = arena.state_fingerprint();
//!
//! arena.remove(root);
//! assert!(arena.restore_state(&snapshot).is_err(), "widget is gone");
//! ```
#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::fmt;

use crate::arena::Slot;
use crate::id::WidgetId;
use crate::node::{ColdNode, HotNode, InlineTextCache};
use crate::widget::{DummyWidget, Widget};
use crate::WidgetArena;

/// Opaque widget state captured by [`Widget::timemachine_snapshot`].
///
/// Implement this on the (usually small, `Copy`-able) state struct a
/// widget extracts from itself. The `fingerprint` is folded into
/// [`WidgetArena::state_fingerprint`] so replay-vs-record equality
/// checks cover widget internals, and `as_any` lets
/// [`Widget::timemachine_restore`] downcast back to the concrete type.
///
/// # Examples
///
/// ```
/// use martensite_core::TimemachineState;
///
/// #[derive(Debug)]
/// struct ScrollState {
///     offset: f32,
/// }
///
/// impl TimemachineState for ScrollState {
///     fn fingerprint(&self) -> u64 {
///         self.offset.to_bits() as u64
///     }
///     fn as_any(&self) -> &dyn std::any::Any {
///         self
///     }
/// }
/// ```
pub trait TimemachineState: fmt::Debug + Send + Sync + 'static {
    /// Deterministic fingerprint of this state. Two snapshots of equal
    /// state must produce equal fingerprints.
    fn fingerprint(&self) -> u64;
    /// Views this state as `&dyn Any` so `Widget::timemachine_restore`
    /// can downcast to its concrete state type.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Cold-node payload captured in an [`ArenaState`].
///
/// Carries every [`ColdNode`] field except the widget object itself;
/// the widget's restorable internals live in `widget_state`.
pub(crate) struct ColdEntry {
    /// See [`ColdNode::debug_name`].
    pub debug_name: Option<&'static str>,
    /// See [`ColdNode::tooltip`].
    pub tooltip: Option<String>,
    /// See [`ColdNode::a11y_role`].
    pub a11y_role: accesskit::Role,
    /// See [`ColdNode::a11y_name`].
    pub a11y_name: Option<String>,
    /// See [`ColdNode::text_cache`].
    pub text_cache: InlineTextCache,
    /// Widget-internal state from `Widget::timemachine_snapshot`.
    pub widget_state: Option<Box<dyn TimemachineState>>,
}

/// A point-in-time capture of a [`WidgetArena`]'s complete restorable state.
///
/// Produced by [`WidgetArena::snapshot_state`] and consumed by
/// [`WidgetArena::restore_state`]/[`WidgetArena::restore_state_with`].
/// Opaque: the only meaningful operations are restore and
/// [`WidgetArena::state_fingerprint`] comparison.
///
/// # Examples
///
/// ```
/// use martensite_core::{DummyWidget, HotNode, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
///
/// let snapshot = arena.snapshot_state();
/// // The same arena restores cleanly while its widgets are alive.
/// assert!(arena.restore_state(&snapshot).is_ok());
/// ```
pub struct ArenaState {
    /// Sparse slot indirection table (generation + dense index).
    pub(crate) slots: Vec<Slot>,
    /// Dense hot node records (bounds, flags, hierarchy links).
    pub(crate) hot_nodes: Vec<HotNode>,
    /// Reverse mapping from dense index to sparse slot index.
    pub(crate) dense_to_slot: Vec<u32>,
    /// FIFO queue of recycled slot indices.
    pub(crate) free_slots: VecDeque<u32>,
    /// Cold-node metadata + widget state, dense-index aligned.
    pub(crate) cold: Vec<ColdEntry>,
}

impl fmt::Debug for ArenaState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArenaState")
            .field("len", &self.hot_nodes.len())
            .field("slot_count", &self.slots.len())
            .finish_non_exhaustive()
    }
}

/// Errors returned by [`WidgetArena::restore_state`] and
/// [`WidgetArena::restore_state_with`].
///
/// # Examples
///
/// ```
/// use martensite_core::{ArenaRestoreError, DummyWidget, HotNode, WidgetArena};
///
/// let mut arena = WidgetArena::new();
/// let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
/// let snapshot = arena.snapshot_state();
/// arena.remove(id);
///
/// assert!(matches!(
///     arena.restore_state(&snapshot).unwrap_err(),
///     ArenaRestoreError::MissingWidgets(_)
/// ));
/// ```
#[derive(Debug)]
pub enum ArenaRestoreError {
    /// The snapshot references widgets that are not alive in the arena
    /// and the factory could not reconstruct them.
    MissingWidgets(Vec<WidgetId>),
    /// A live widget rejected its captured state
    /// (`Widget::timemachine_restore` returned `false`).
    RestoreRejected(WidgetId),
    /// The snapshot is internally inconsistent (corrupt or produced by
    /// a different arena implementation).
    Corrupt(&'static str),
}

impl fmt::Display for ArenaRestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingWidgets(ids) => write!(
                f,
                "snapshot requires {} widget(s) not alive in the arena",
                ids.len()
            ),
            Self::RestoreRejected(id) => {
                write!(f, "widget {:?} rejected its snapshot state", id)
            }
            Self::Corrupt(msg) => write!(f, "corrupt arena snapshot: {msg}"),
        }
    }
}

impl std::error::Error for ArenaRestoreError {}

impl WidgetArena {
    /// Captures the arena's complete restorable state into an [`ArenaState`].
    ///
    /// Snapshots the slot table, hot nodes, hierarchy links, free-slot
    /// queue, and cold-node metadata, then asks every widget for its
    /// [`TimemachineState`] via `Widget::timemachine_snapshot`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let snapshot = arena.snapshot_state();
    /// assert!(arena.restore_state(&snapshot).is_ok());
    /// ```
    pub fn snapshot_state(&self) -> ArenaState {
        ArenaState {
            slots: self.slots.clone(),
            hot_nodes: self.hot_nodes.clone(),
            dense_to_slot: self.dense_to_slot.clone(),
            free_slots: self.free_slots.clone(),
            cold: self
                .cold_nodes
                .iter()
                .map(|cold| ColdEntry {
                    debug_name: cold.debug_name,
                    tooltip: cold.tooltip.clone(),
                    a11y_role: cold.a11y_role,
                    a11y_name: cold.a11y_name.clone(),
                    text_cache: cold.text_cache,
                    widget_state: cold.widget.timemachine_snapshot(),
                })
                .collect(),
        }
    }

    /// Restores a previously captured [`ArenaState`].
    ///
    /// Equivalent to [`restore_state_with`](Self::restore_state_with)
    /// with a factory that reconstructs nothing — every `WidgetId` in
    /// the snapshot must still be alive.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let snapshot = arena.snapshot_state();
    /// arena.remove(id);
    /// // `id`'s widget object no longer exists — restore fails.
    /// assert!(arena.restore_state(&snapshot).is_err());
    /// ```
    pub fn restore_state(&mut self, state: &ArenaState) -> Result<(), ArenaRestoreError> {
        self.restore_state_with(state, &mut |_| None)
    }

    /// Restores a previously captured [`ArenaState`], reconstructing
    /// widgets that are no longer alive via `factory`.
    ///
    /// The arena's live widget objects are reused for every `WidgetId`
    /// still alive and receive their captured state through
    /// `Widget::timemachine_restore`; `factory` is consulted once per
    /// snapshot widget that is no longer alive. After validation, the
    /// sparse/dense arrays and hierarchy links are replaced wholesale
    /// with the snapshot's — so structure (insert/remove/parenting)
    /// recorded in the snapshot is reproduced exactly.
    ///
    /// Note: a `RestoreRejected` error may leave some widget internals
    /// already restored; structural arrays are only replaced after all
    /// widget state applies cleanly.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// let id = arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let snapshot = arena.snapshot_state();
    /// arena.remove(id);
    ///
    /// // A factory can stand in for removed widget objects.
    /// arena
    ///     .restore_state_with(&snapshot, &mut |_| Some(Box::new(DummyWidget)))
    ///     .unwrap();
    /// assert!(arena.is_alive(id));
    /// ```
    pub fn restore_state_with(
        &mut self,
        state: &ArenaState,
        factory: &mut dyn FnMut(WidgetId) -> Option<Box<dyn Widget>>,
    ) -> Result<(), ArenaRestoreError> {
        if state.cold.len() != state.hot_nodes.len()
            || state.dense_to_slot.len() != state.hot_nodes.len()
        {
            return Err(ArenaRestoreError::Corrupt("hot/cold/dense length mismatch"));
        }
        if state
            .dense_to_slot
            .iter()
            .any(|&s| s as usize >= state.slots.len())
        {
            return Err(ArenaRestoreError::Corrupt("dense_to_slot out of bounds"));
        }

        // WidgetIds in snapshot dense order.
        let ids: Vec<WidgetId> = state
            .dense_to_slot
            .iter()
            .map(|&slot_idx| {
                let generation = state.slots[slot_idx as usize].generation;
                WidgetId::from_parts(slot_idx, generation)
            })
            .collect();

        // Fabricate replacements for widgets that are gone — before any
        // mutation, so a failure leaves the arena untouched.
        let mut missing: Vec<WidgetId> = ids
            .iter()
            .copied()
            .filter(|&id| !self.is_alive(id))
            .collect();
        let mut fabricated: HashMap<WidgetId, Box<dyn Widget>> = HashMap::new();
        if !missing.is_empty() {
            for &id in &missing {
                match factory(id) {
                    Some(widget) => {
                        fabricated.insert(id, widget);
                    }
                    None => {
                        missing.sort_by_key(|id| id.to_u64());
                        return Err(ArenaRestoreError::MissingWidgets(missing));
                    }
                }
            }
        }

        // Apply widget-internal state on the live objects first — a
        // rejection leaves the arena structurally untouched.
        for (snapshot_dense, &id) in ids.iter().enumerate() {
            let dense = self
                .slots
                .get(id.slot_idx() as usize)
                .filter(|s| s.generation == id.generation())
                .map(|s| s.dense_idx as usize);
            if let Some(dense) = dense {
                if let Some(state_obj) = &state.cold[snapshot_dense].widget_state {
                    if !self.cold_nodes[dense]
                        .widget
                        .timemachine_restore(&**state_obj)
                    {
                        return Err(ArenaRestoreError::RestoreRejected(id));
                    }
                }
            }
        }

        // Move widget objects out of current cold storage, keyed by id.
        let mut widgets: HashMap<WidgetId, Box<dyn Widget>> =
            HashMap::with_capacity(self.cold_nodes.len());
        for dense in 0..self.cold_nodes.len() {
            let slot_idx = self.dense_to_slot[dense];
            let generation = self.slots[slot_idx as usize].generation;
            let id = WidgetId::from_parts(slot_idx, generation);
            let placeholder: Box<dyn Widget> = Box::new(DummyWidget);
            widgets.insert(
                id,
                std::mem::replace(&mut self.cold_nodes[dense].widget, placeholder),
            );
        }

        // Rebuild cold storage in snapshot order, reusing live widgets
        // and fabricated replacements.
        let mut cold_nodes = Vec::with_capacity(state.cold.len());
        for (i, entry) in state.cold.iter().enumerate() {
            let id = ids[i];
            let widget = widgets
                .remove(&id)
                .or_else(|| fabricated.remove(&id))
                .expect("missing widget was fabricated or alive");
            cold_nodes.push(ColdNode {
                debug_name: entry.debug_name,
                tooltip: entry.tooltip.clone(),
                a11y_role: entry.a11y_role,
                a11y_name: entry.a11y_name.clone(),
                text_cache: entry.text_cache,
                widget,
            });
        }

        self.slots = state.slots.clone();
        self.hot_nodes = state.hot_nodes.clone();
        self.dense_to_slot = state.dense_to_slot.clone();
        self.free_slots = state.free_slots.clone();
        self.cold_nodes = cold_nodes;
        Ok(())
    }

    /// Deterministic `u64` fingerprint of the arena's full state.
    ///
    /// Covers slot/generation bookkeeping, every `HotNode` field
    /// (bounds, layout id, flags, hierarchy links), cold-node metadata,
    /// the inline text cache, and each widget's
    /// [`TimemachineState::fingerprint`]. Equal states fingerprint equal;
    /// the converse is overwhelmingly likely (FNV-1a, not cryptographic).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_core::{DummyWidget, HotNode, WidgetArena};
    ///
    /// let mut arena = WidgetArena::new();
    /// arena.insert_with_widget(HotNode::default(), Box::new(DummyWidget));
    /// let fp = arena.state_fingerprint();
    /// assert_eq!(fp, arena.state_fingerprint(), "unchanged arena");
    /// ```
    pub fn state_fingerprint(&self) -> u64 {
        let mut fp = Fnv1a::new();
        fp.u64(self.hot_nodes.len() as u64);

        for slot in &self.slots {
            fp.u32(slot.generation);
            fp.u32(slot.dense_idx);
        }

        for hot in &self.hot_nodes {
            fp.u32(hot.bounds.origin.x.to_bits());
            fp.u32(hot.bounds.origin.y.to_bits());
            fp.u32(hot.bounds.size.x.to_bits());
            fp.u32(hot.bounds.size.y.to_bits());
            fp.u64(u64::from(hot.layout_id));
            fp.u32(hot.flags.bits());
            fp.u16(hot.depth_rank);
            fp.u16(hot.z_index as u16);
            for link in [
                hot.parent,
                hot.first_child,
                hot.next_sibling,
                hot.prev_sibling,
            ] {
                fp.u64(link.map(WidgetId::to_u64).unwrap_or(0));
            }
        }

        for &slot in &self.dense_to_slot {
            fp.u32(slot);
        }
        for &slot in &self.free_slots {
            fp.u32(slot);
        }

        for cold in &self.cold_nodes {
            fp.opt_str(cold.debug_name);
            fp.opt_str(cold.tooltip.as_deref());
            // `accesskit::Role` has no stable repr; its Debug name is
            // stable across runs for fingerprinting purposes.
            fp.str(&format!("{:?}", cold.a11y_role));
            fp.opt_str(cold.a11y_name.as_deref());
            for &(w, mw, mh) in cold.text_cache.entries() {
                fp.u32(w.to_bits());
                fp.u32(mw.to_bits());
                fp.u32(mh.to_bits());
            }
            fp.u64(cold.text_cache.cursor as u64);
            fp.u64(
                cold.widget
                    .timemachine_snapshot()
                    .map(|s| s.fingerprint())
                    .unwrap_or(0),
            );
        }

        fp.finish()
    }
}

/// Minimal FNV-1a hasher for arena fingerprints — deterministic across
/// runs and platforms, no `std::hash` state dependence.
struct Fnv1a(u64);

impl Fnv1a {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    #[inline]
    fn byte(&mut self, b: u8) {
        self.0 ^= u64::from(b);
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }

    #[inline]
    fn u16(&mut self, v: u16) {
        for b in v.to_le_bytes() {
            self.byte(b);
        }
    }

    #[inline]
    fn u32(&mut self, v: u32) {
        for b in v.to_le_bytes() {
            self.byte(b);
        }
    }

    #[inline]
    fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.byte(b);
        }
    }

    fn str(&mut self, s: &str) {
        self.u64(s.len() as u64);
        for &b in s.as_bytes() {
            self.byte(b);
        }
    }

    fn opt_str(&mut self, s: Option<&str>) {
        match s {
            Some(s) => {
                self.byte(1);
                self.str(s);
            }
            None => self.byte(0),
        }
    }

    fn finish(self) -> u64 {
        self.0
    }
}
