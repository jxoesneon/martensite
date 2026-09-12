//! Surface registration, the two-slot frame ring, and damage signaling.
//!
//! [`BridgeRegistry`] owns one [`SurfaceRing`] per [`SurfaceId`]. Each
//! ring is a mailbox: at most one slot is being written by the producer
//! and at most one slot is the ready "front" the host composites. When a
//! newer frame becomes ready, a stale ready slot is freed — the host
//! always displays the freshest frame, matching `PresentMode::Mailbox`
//! semantics.
//!
//! [`BridgeHandle`] is the `Arc`-shared, `Mutex`-guarded clone used to
//! move the registry between the producer thread and the host paint
//! thread. It uses `parking_lot::Mutex` (poison-free), matching the
//! workspace convention.

use crate::error::BridgeError;
use crate::frame::FrameToken;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Opaque identifier for one external surface (one embedded viewport).
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::SurfaceId;
///
/// let id = SurfaceId(3);
/// assert_eq!(id, SurfaceId(3));
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SurfaceId(pub u64);

/// A snapshot of a ring's front frame, returned atomically by
/// [`BridgeRegistry::front_with_token`].
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::{BridgeRegistry, FrontFrame};
///
/// let mut reg = BridgeRegistry::new();
/// let id = reg.register();
/// assert!(reg.front_with_token(id).unwrap().is_none());
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FrontFrame {
    /// The slot index holding the front frame.
    pub slot: u8,
    /// The frame's unique token — fresh even when the mailbox recycles
    /// the same slot.
    pub token: FrameToken,
    /// The frame's recorded size in physical pixels, if the producer
    /// provided it via `mark_ready_sized`.
    pub size: Option<(u32, u32)>,
}

/// Lifecycle state of one ring slot.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum SlotState {
    /// Available for the producer to acquire.
    Free,
    /// The producer is actively rendering into this slot.
    Writing,
    /// Fully rendered; waiting for the host to composite.
    Ready,
    /// The host is currently sampling this slot in a composite pass.
    Compositing,
}

/// A two-slot mailbox ring for one external surface.
///
/// The ring carries tokens, not textures: the producer and the
/// `WgpuHost` map slot indices to their own texture objects. Keeping the
/// ring GPU-agnostic makes the full state machine testable without a
/// device.
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::SurfaceRing;
///
/// let mut ring = SurfaceRing::new();
/// let (slot, token) = ring.acquire().unwrap();
/// ring.mark_ready(slot).unwrap();
/// let (front, front_token) = ring.take_front().unwrap();
/// assert_eq!(token, front_token);
/// ring.release(front).unwrap();
/// assert_eq!(ring.drain_released(), vec![token]);
/// ```
pub struct SurfaceRing {
    slots: [(SlotState, FrameToken); 2],
    next_token: u64,
    /// Index of the ready slot the host should composite next (newest).
    front: Option<u8>,
    /// Physical-pixel size of the front frame, set via
    /// [`SurfaceRing::set_front_size`] — consumed by the widget's
    /// intrinsic-size measure.
    front_size: Option<(u32, u32)>,
    /// Tokens the host has released, awaiting producer recycling.
    released: Vec<FrameToken>,
}

impl SurfaceRing {
    /// Creates an empty ring with both slots free.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::SurfaceRing;
    ///
    /// let ring = SurfaceRing::new();
    /// assert!(ring.front().is_none());
    /// ```
    pub fn new() -> Self {
        Self {
            slots: [
                (SlotState::Free, FrameToken(0)),
                (SlotState::Free, FrameToken(0)),
            ],
            next_token: 1,
            front: None,
            front_size: None,
            released: Vec::new(),
        }
    }

    /// Acquires a free slot for the producer and returns its index and
    /// the new frame token.
    ///
    /// # Errors
    ///
    /// [`BridgeError::RingExhausted`] when no slot is `Free` — the
    /// producer should drop the frame or wait for the host to release.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::SurfaceRing;
    ///
    /// let mut ring = SurfaceRing::new();
    /// let (slot, _token) = ring.acquire().unwrap();
    /// assert!(slot < 2);
    /// ```
    pub fn acquire(&mut self) -> Result<(u8, FrameToken), BridgeError> {
        for (i, (state, _)) in self.slots.iter_mut().enumerate() {
            if *state == SlotState::Free {
                let token = FrameToken(self.next_token);
                self.next_token += 1;
                *state = SlotState::Writing;
                self.slots[i].1 = token;
                return Ok((i as u8, token));
            }
        }
        Err(BridgeError::RingExhausted)
    }

    /// Marks a `Writing` slot ready and makes it the front buffer.
    ///
    /// If the previous front slot was still `Ready` (never composited),
    /// it is freed — the mailbox keeps only the freshest frame.
    ///
    /// # Errors
    ///
    /// [`BridgeError::InvalidSlot`] for slot indices ≥ 2, and
    /// [`BridgeError::InvalidTransition`] if the slot is not `Writing`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::SurfaceRing;
    ///
    /// let mut ring = SurfaceRing::new();
    /// let (slot, _) = ring.acquire().unwrap();
    /// ring.mark_ready(slot).unwrap();
    /// assert!(ring.front().is_some());
    /// ```
    pub fn mark_ready(&mut self, slot: u8) -> Result<(), BridgeError> {
        let idx = usize::from(slot);
        if idx >= 2 {
            return Err(BridgeError::InvalidSlot(slot));
        }
        if self.slots[idx].0 != SlotState::Writing {
            return Err(BridgeError::InvalidTransition);
        }
        // Free a stale ready slot: the mailbox keeps only the newest frame.
        if let Some(old_front) = self.front {
            let old = usize::from(old_front);
            if old != idx && self.slots[old].0 == SlotState::Ready {
                self.slots[old].0 = SlotState::Free;
            }
        }
        self.slots[idx].0 = SlotState::Ready;
        self.front = Some(slot);
        Ok(())
    }

    /// Takes the front slot for compositing, transitioning it to
    /// `Compositing`. Returns `None` when no frame is ready.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::SurfaceRing;
    ///
    /// let mut ring = SurfaceRing::new();
    /// assert!(ring.take_front().is_none());
    /// ```
    pub fn take_front(&mut self) -> Option<(u8, FrameToken)> {
        let idx = usize::from(self.front?);
        if self.slots[idx].0 != SlotState::Ready {
            return None;
        }
        self.slots[idx].0 = SlotState::Compositing;
        Some((idx as u8, self.slots[idx].1))
    }

    /// Releases a `Compositing` slot back to `Free` and queues its token
    /// for producer recycling.
    ///
    /// # Errors
    ///
    /// [`BridgeError::InvalidSlot`] for slot indices ≥ 2, and
    /// [`BridgeError::InvalidTransition`] if the slot is not
    /// `Compositing`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::SurfaceRing;
    ///
    /// let mut ring = SurfaceRing::new();
    /// let (slot, _) = ring.acquire().unwrap();
    /// assert!(ring.release(slot).is_err());
    /// ```
    pub fn release(&mut self, slot: u8) -> Result<(), BridgeError> {
        let idx = usize::from(slot);
        if idx >= 2 {
            return Err(BridgeError::InvalidSlot(slot));
        }
        if self.slots[idx].0 != SlotState::Compositing {
            return Err(BridgeError::InvalidTransition);
        }
        let token = self.slots[idx].1;
        self.slots[idx].0 = SlotState::Free;
        if self.front == Some(slot) {
            self.front = None;
        }
        self.released.push(token);
        Ok(())
    }

    /// Drains tokens the host has released since the last call. The
    /// producer calls this to recycle texture slots.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::SurfaceRing;
    ///
    /// let mut ring = SurfaceRing::new();
    /// assert!(ring.drain_released().is_empty());
    /// ```
    pub fn drain_released(&mut self) -> Vec<FrameToken> {
        std::mem::take(&mut self.released)
    }

    /// The slot index the host would composite next, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::SurfaceRing;
    ///
    /// let ring = SurfaceRing::new();
    /// assert!(ring.front().is_none());
    /// ```
    pub fn front(&self) -> Option<u8> {
        self.front
    }

    /// Records the physical-pixel size of the front frame.
    ///
    /// Called by the producer after [`SurfaceRing::mark_ready`] so the
    /// widget's `measure` can report the frame's intrinsic size and
    /// request layout when it changes.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::SurfaceRing;
    ///
    /// let mut ring = SurfaceRing::new();
    /// let (slot, _) = ring.acquire().unwrap();
    /// ring.mark_ready(slot).unwrap();
    /// ring.set_front_size((640, 480));
    /// assert_eq!(ring.front_size(), Some((640, 480)));
    /// ```
    pub fn set_front_size(&mut self, size: (u32, u32)) {
        self.front_size = Some(size);
    }

    /// The recorded size of the front frame, if set.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::SurfaceRing;
    ///
    /// let ring = SurfaceRing::new();
    /// assert_eq!(ring.front_size(), None);
    /// ```
    pub fn front_size(&self) -> Option<(u32, u32)> {
        self.front_size
    }
}

impl Default for SurfaceRing {
    fn default() -> Self {
        Self::new()
    }
}

/// The shared registry: surfaces, rings, and the ready-event queue used
/// for damage signaling.
///
/// `BridgeRegistry` is not itself shared — wrap it in [`BridgeHandle`]
/// to move clones across threads.
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::BridgeRegistry;
///
/// let mut registry = BridgeRegistry::new();
/// let surface = registry.register();
/// let (slot, _token) = registry.acquire(surface).unwrap();
/// registry.mark_ready(surface, slot).unwrap();
/// assert_eq!(registry.drain_ready(), vec![surface]);
/// ```
pub struct BridgeRegistry {
    rings: HashMap<SurfaceId, SurfaceRing>,
    ready_events: VecDeque<SurfaceId>,
    next_surface: u64,
}

impl BridgeRegistry {
    /// Creates an empty registry.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeRegistry;
    ///
    /// let registry = BridgeRegistry::new();
    /// assert_eq!(registry.surface_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            rings: HashMap::new(),
            ready_events: VecDeque::new(),
            next_surface: 1,
        }
    }

    /// Registers a new surface and returns its [`SurfaceId`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeRegistry;
    ///
    /// let mut registry = BridgeRegistry::new();
    /// assert_ne!(registry.register(), registry.register());
    /// ```
    pub fn register(&mut self) -> SurfaceId {
        let id = SurfaceId(self.next_surface);
        self.next_surface += 1;
        self.rings.insert(id, SurfaceRing::new());
        id
    }

    /// Removes a surface and its ring. In-flight tokens are discarded.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeRegistry;
    ///
    /// let mut registry = BridgeRegistry::new();
    /// let id = registry.register();
    /// registry.unregister(id);
    /// assert!(registry.acquire(id).is_err());
    /// ```
    pub fn unregister(&mut self, id: SurfaceId) {
        self.rings.remove(&id);
    }

    /// Acquires a writing slot on `id`'s ring. See
    /// [`SurfaceRing::acquire`].
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`] if `id` is not registered;
    /// [`BridgeError::RingExhausted`] if no slot is free.
    pub fn acquire(&mut self, id: SurfaceId) -> Result<(u8, FrameToken), BridgeError> {
        self.rings
            .get_mut(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .acquire()
    }

    /// Marks a writing slot ready and queues a ready event for damage
    /// signaling. See [`SurfaceRing::mark_ready`].
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`], [`BridgeError::InvalidSlot`], or
    /// [`BridgeError::InvalidTransition`].
    pub fn mark_ready(&mut self, id: SurfaceId, slot: u8) -> Result<(), BridgeError> {
        self.rings
            .get_mut(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .mark_ready(slot)?;
        if self.ready_events.back() != Some(&id) {
            self.ready_events.push_back(id);
        }
        Ok(())
    }

    /// Takes the front slot of `id` for compositing.
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`] if `id` is not registered.
    pub fn take_front(&mut self, id: SurfaceId) -> Result<Option<(u8, FrameToken)>, BridgeError> {
        Ok(self
            .rings
            .get_mut(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .take_front())
    }

    /// Releases a composited slot on `id`. See [`SurfaceRing::release`].
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`], [`BridgeError::InvalidSlot`], or
    /// [`BridgeError::InvalidTransition`].
    pub fn release(&mut self, id: SurfaceId, slot: u8) -> Result<(), BridgeError> {
        self.rings
            .get_mut(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .release(slot)
    }

    /// Drains released tokens for `id` (producer recycling).
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`] if `id` is not registered.
    pub fn drain_released(&mut self, id: SurfaceId) -> Result<Vec<FrameToken>, BridgeError> {
        Ok(self
            .rings
            .get_mut(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .drain_released())
    }

    /// Drains the ready-event queue. Each event means "this surface has a
    /// newer frame than the host last composited" — the widget layer
    /// turns it into a dirty flag + `request_redraw`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeRegistry;
    ///
    /// let mut registry = BridgeRegistry::new();
    /// let id = registry.register();
    /// let (slot, _) = registry.acquire(id).unwrap();
    /// registry.mark_ready(id, slot).unwrap();
    /// assert_eq!(registry.drain_ready(), vec![id]);
    /// assert!(registry.drain_ready().is_empty());
    /// ```
    pub fn drain_ready(&mut self) -> Vec<SurfaceId> {
        self.ready_events.drain(..).collect()
    }

    /// Marks a writing slot ready and records the frame's size in one
    /// call. See [`SurfaceRing::mark_ready`] and
    /// [`SurfaceRing::set_front_size`].
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`], [`BridgeError::InvalidSlot`], or
    /// [`BridgeError::InvalidTransition`].
    pub fn mark_ready_sized(
        &mut self,
        id: SurfaceId,
        slot: u8,
        size: (u32, u32),
    ) -> Result<(), BridgeError> {
        let ring = self
            .rings
            .get_mut(&id)
            .ok_or(BridgeError::UnknownSurface(id))?;
        ring.mark_ready(slot)?;
        ring.set_front_size(size);
        if self.ready_events.back() != Some(&id) {
            self.ready_events.push_back(id);
        }
        Ok(())
    }

    /// The front slot index of `id`, if a frame is ready or compositing.
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`] if `id` is not registered.
    pub fn front(&self, id: SurfaceId) -> Result<Option<u8>, BridgeError> {
        Ok(self
            .rings
            .get(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .front())
    }

    /// The recorded size of `id`'s front frame, if set.
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`] if `id` is not registered.
    pub fn front_size(&self, id: SurfaceId) -> Result<Option<(u32, u32)>, BridgeError> {
        Ok(self
            .rings
            .get(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .front_size())
    }

    /// Atomically returns the front slot, its [`FrameToken`], and its
    /// recorded size for `id`.
    ///
    /// Consumers should prefer this over calling [`Self::front`],
    /// [`Self::front_size`], and slot-token queries separately: those
    /// each take the registry lock, so an interleaved
    /// [`mark_ready_sized`](Self::mark_ready_sized) could mix fields
    /// from two different frames.
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`] if `id` is not registered.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeRegistry;
    ///
    /// let mut reg = BridgeRegistry::new();
    /// let id = reg.register();
    /// let (slot, _) = reg.acquire(id).unwrap();
    /// reg.mark_ready_sized(id, slot, (64, 64)).unwrap();
    /// let front = reg.front_with_token(id).unwrap().unwrap();
    /// assert_eq!(front.slot, slot);
    /// assert_eq!(front.size, Some((64, 64)));
    /// ```
    pub fn front_with_token(&self, id: SurfaceId) -> Result<Option<FrontFrame>, BridgeError> {
        let ring = self.rings.get(&id).ok_or(BridgeError::UnknownSurface(id))?;
        Ok(ring.front().map(|slot| FrontFrame {
            slot,
            token: ring.slots[usize::from(slot)].1,
            size: ring.front_size(),
        }))
    }

    /// Number of registered surfaces.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeRegistry;
    ///
    /// let mut registry = BridgeRegistry::new();
    /// registry.register();
    /// assert_eq!(registry.surface_count(), 1);
    /// ```
    pub fn surface_count(&self) -> usize {
        self.rings.len()
    }
}

impl Default for BridgeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// A thread-safe, cloneable handle to a [`BridgeRegistry`].
///
/// Producers typically hold one clone on their render thread while the
/// widget/host holds another on the paint thread. All ring operations go
/// through the shared `parking_lot::Mutex` — short critical sections,
/// no lock poisoning.
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::BridgeHandle;
///
/// let handle = BridgeHandle::new();
/// let surface = handle.lock().register();
/// let clone = handle.clone();
/// assert!(clone.lock().acquire(surface).is_ok());
/// ```
#[derive(Clone, Default)]
pub struct BridgeHandle {
    inner: Arc<Mutex<BridgeRegistry>>,
}

impl BridgeHandle {
    /// Creates a handle over a fresh registry.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// assert_eq!(handle.lock().surface_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(BridgeRegistry::new())),
        }
    }

    /// Wraps an existing registry.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::{BridgeHandle, BridgeRegistry};
    ///
    /// let handle = BridgeHandle::from_registry(BridgeRegistry::new());
    /// assert_eq!(handle.lock().surface_count(), 0);
    /// ```
    pub fn from_registry(registry: BridgeRegistry) -> Self {
        Self {
            inner: Arc::new(Mutex::new(registry)),
        }
    }

    /// Locks the registry for a batch of operations.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// let id = handle.lock().register();
    /// assert!(handle.lock().front(id).is_ok());
    /// ```
    pub fn lock(&self) -> parking_lot::MutexGuard<'_, BridgeRegistry> {
        self.inner.lock()
    }
}

impl std::fmt::Debug for BridgeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // try_lock: formatting a handle while the caller already holds
        // the guard must not deadlock (parking_lot is not reentrant).
        match self.inner.try_lock() {
            Some(reg) => f
                .debug_struct("BridgeHandle")
                .field("surfaces", &reg.surface_count())
                .finish(),
            None => f.debug_struct("BridgeHandle").finish_non_exhaustive(),
        }
    }
}
