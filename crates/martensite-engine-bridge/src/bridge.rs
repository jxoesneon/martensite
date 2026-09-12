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
use crate::frame::{CpuFrame, Frame, FrameToken};
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

/// One ring slot: state machine, assigned token, and the published
/// frame payload (when the producer uses
/// [`BridgeRegistry::mark_ready_frame`]).
struct Slot {
    state: SlotState,
    token: FrameToken,
    /// The published frame, retained while `Ready`/`Compositing` so the
    /// host can sample its texture under the registry lock. Producers
    /// that want to recycle the underlying texture should wrap it in
    /// [`crate::SharedTexture`] — dropping the frame on `release` then
    /// leaves the producer's `Arc` alive.
    frame: Option<Box<dyn Frame>>,
    /// The CPU raster published alongside the frame (or standalone via
    /// [`BridgeRegistry::mark_ready_cpu`]) for the TinySkia fallback
    /// path.
    cpu_frame: Option<CpuFrame>,
}

/// A two-slot mailbox ring for one external surface.
///
/// The ring carries tokens and, optionally, the published [`Frame`]
/// payload per slot: producers that want the host to consume the ring
/// end-to-end publish the frame with
/// [`BridgeRegistry::mark_ready_frame`]; producers managing their own
/// texture table can keep using [`mark_ready`](SurfaceRing::mark_ready)
/// and register textures with the host directly.
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
    slots: [Slot; 2],
    next_token: u64,
    /// Index of the ready slot the host should composite next (newest).
    front: Option<u8>,
    /// Physical-pixel size of the front frame, set via
    /// [`SurfaceRing::set_front_size`] — consumed by the widget's
    /// intrinsic-size measure.
    front_size: Option<(u32, u32)>,
    /// Tokens the host has released, awaiting producer recycling.
    released: Vec<FrameToken>,
    /// The viewport the widget last laid out — producers read it to
    /// size their renders to the widget's physical bounds and DPI.
    viewport: Option<crate::Viewport>,
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
                Slot {
                    state: SlotState::Free,
                    token: FrameToken(0),
                    frame: None,
                    cpu_frame: None,
                },
                Slot {
                    state: SlotState::Free,
                    token: FrameToken(0),
                    frame: None,
                    cpu_frame: None,
                },
            ],
            next_token: 1,
            front: None,
            front_size: None,
            released: Vec::new(),
            viewport: None,
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
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.state == SlotState::Free {
                let token = FrameToken(self.next_token);
                self.next_token += 1;
                slot.state = SlotState::Writing;
                slot.token = token;
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
        if self.slots[idx].state != SlotState::Writing {
            return Err(BridgeError::InvalidTransition);
        }
        // Free a stale ready slot: the mailbox keeps only the newest frame.
        if let Some(old_front) = self.front {
            let old = usize::from(old_front);
            if old != idx && self.slots[old].state == SlotState::Ready {
                self.slots[old].state = SlotState::Free;
                self.slots[old].frame = None;
                self.slots[old].cpu_frame = None;
            }
        }
        self.slots[idx].state = SlotState::Ready;
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
        if self.slots[idx].state != SlotState::Ready {
            return None;
        }
        self.slots[idx].state = SlotState::Compositing;
        Some((idx as u8, self.slots[idx].token))
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
        if self.slots[idx].state != SlotState::Compositing {
            return Err(BridgeError::InvalidTransition);
        }
        let token = self.slots[idx].token;
        self.slots[idx].state = SlotState::Free;
        self.slots[idx].frame = None;
        self.slots[idx].cpu_frame = None;
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

    /// The [`Frame`] published into the front slot, if the producer
    /// used [`BridgeRegistry::mark_ready_frame`]. Borrows the ring —
    /// usable only while the registry lock is held.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::SurfaceRing;
    ///
    /// let ring = SurfaceRing::new();
    /// assert!(ring.front_frame().is_none());
    /// ```
    pub fn front_frame(&self) -> Option<&dyn Frame> {
        let idx = usize::from(self.front?);
        self.slots[idx].frame.as_deref()
    }

    /// The [`CpuFrame`] published for the front slot, if any — the
    /// TinySkia fallback payload.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::SurfaceRing;
    ///
    /// let ring = SurfaceRing::new();
    /// assert!(ring.front_cpu_frame().is_none());
    /// ```
    pub fn front_cpu_frame(&self) -> Option<&CpuFrame> {
        let idx = usize::from(self.front?);
        self.slots[idx].cpu_frame.as_ref()
    }

    /// Stores the [`Frame`] for a `Writing` slot without marking it
    /// ready — call before [`SurfaceRing::mark_ready`].
    ///
    /// # Errors
    ///
    /// [`BridgeError::InvalidSlot`] for slot indices ≥ 2, and
    /// [`BridgeError::InvalidTransition`] if the slot is not `Writing`.
    pub fn set_frame(&mut self, slot: u8, frame: Box<dyn Frame>) -> Result<(), BridgeError> {
        let idx = usize::from(slot);
        if idx >= 2 {
            return Err(BridgeError::InvalidSlot(slot));
        }
        if self.slots[idx].state != SlotState::Writing {
            return Err(BridgeError::InvalidTransition);
        }
        self.slots[idx].frame = Some(frame);
        Ok(())
    }

    /// Stores the [`CpuFrame`] for a `Writing` slot — call before
    /// [`SurfaceRing::mark_ready`].
    ///
    /// # Errors
    ///
    /// [`BridgeError::InvalidSlot`] for slot indices ≥ 2, and
    /// [`BridgeError::InvalidTransition`] if the slot is not `Writing`.
    pub fn set_cpu_frame(&mut self, slot: u8, frame: CpuFrame) -> Result<(), BridgeError> {
        let idx = usize::from(slot);
        if idx >= 2 {
            return Err(BridgeError::InvalidSlot(slot));
        }
        if self.slots[idx].state != SlotState::Writing {
            return Err(BridgeError::InvalidTransition);
        }
        self.slots[idx].cpu_frame = Some(frame);
        Ok(())
    }

    /// Records the viewport the widget last laid out into.
    ///
    /// The widget pushes this on every `layout`; producers read
    /// [`SurfaceRing::viewport`] to render at the widget's physical
    /// bounds and DPI scale.
    pub fn set_viewport(&mut self, viewport: crate::Viewport) {
        self.viewport = Some(viewport);
    }

    /// The viewport last pushed by the widget, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::SurfaceRing;
    ///
    /// let ring = SurfaceRing::new();
    /// assert!(ring.viewport().is_none());
    /// ```
    pub fn viewport(&self) -> Option<crate::Viewport> {
        self.viewport
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

    /// The [`Frame`] published into `id`'s front slot, if any. Borrows
    /// the registry — call under the `BridgeHandle` lock.
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`] if `id` is not registered.
    pub fn front_frame(&self, id: SurfaceId) -> Result<Option<&dyn Frame>, BridgeError> {
        Ok(self
            .rings
            .get(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .front_frame())
    }

    /// The [`CpuFrame`] published for `id`'s front slot — the TinySkia
    /// fallback payload set by [`mark_ready_cpu`](Self::mark_ready_cpu).
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`] if `id` is not registered.
    pub fn front_cpu_frame(&self, id: SurfaceId) -> Result<Option<&CpuFrame>, BridgeError> {
        Ok(self
            .rings
            .get(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .front_cpu_frame())
    }

    /// Publishes a produced [`Frame`] into `id`'s slot `slot`, marks it
    /// ready, records its size, and queues a ready event — one call for
    /// the full producer→host handoff.
    ///
    /// The frame is retained while the slot is `Ready`/`Compositing` and
    /// dropped on `release`; producers recycling the texture should wrap
    /// it in [`crate::SharedTexture`].
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`], [`BridgeError::InvalidSlot`], or
    /// [`BridgeError::InvalidTransition`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::{
    ///     BridgeRegistry, Frame, FrameSync, FrameToken, SourceAlpha,
    /// };
    ///
    /// struct Solid;
    /// impl Frame for Solid {
    ///     fn token(&self) -> FrameToken { FrameToken(0) }
    ///     fn same_device_texture(&self) -> Option<&wgpu::Texture> { None }
    ///     fn native_handle(&self) -> Option<martensite_engine_bridge::NativeFrame> { None }
    ///     fn sync(&self) -> FrameSync { FrameSync::None }
    ///     fn size(&self) -> (u32, u32) { (64, 64) }
    ///     fn alpha_mode(&self) -> SourceAlpha { SourceAlpha::Premultiplied }
    /// }
    ///
    /// let mut reg = BridgeRegistry::new();
    /// let id = reg.register();
    /// let (slot, _) = reg.acquire(id).unwrap();
    /// reg.mark_ready_frame(id, slot, Box::new(Solid)).unwrap();
    /// assert!(reg.front_frame(id).unwrap().is_some());
    /// ```
    pub fn mark_ready_frame(
        &mut self,
        id: SurfaceId,
        slot: u8,
        frame: Box<dyn Frame>,
    ) -> Result<(), BridgeError> {
        self.mark_ready_full(id, slot, Some(frame), None)
    }

    /// Publishes a produced [`Frame`] plus an optional [`CpuFrame`]
    /// fallback into `id`'s slot `slot`, marks it ready, records the
    /// frame's size, and queues a ready event.
    ///
    /// The frame is retained while the slot is `Ready`/`Compositing` and
    /// dropped on `release`; producers recycling the texture should wrap
    /// it in [`crate::SharedTexture`].
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`], [`BridgeError::InvalidSlot`], or
    /// [`BridgeError::InvalidTransition`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::{
    ///     BridgeRegistry, CpuFrame, Frame, FrameSync, FrameToken, SourceAlpha,
    /// };
    ///
    /// struct Solid;
    /// impl Frame for Solid {
    ///     fn token(&self) -> FrameToken { FrameToken(0) }
    ///     fn same_device_texture(&self) -> Option<&wgpu::Texture> { None }
    ///     fn native_handle(&self) -> Option<martensite_engine_bridge::NativeFrame> { None }
    ///     fn sync(&self) -> FrameSync { FrameSync::None }
    ///     fn size(&self) -> (u32, u32) { (64, 64) }
    ///     fn alpha_mode(&self) -> SourceAlpha { SourceAlpha::Premultiplied }
    /// }
    ///
    /// let mut reg = BridgeRegistry::new();
    /// let id = reg.register();
    /// let (slot, _) = reg.acquire(id).unwrap();
    /// let frame: Box<dyn Frame> = Box::new(Solid);
    /// reg.mark_ready_full(id, slot, Some(frame), Some(CpuFrame::new(1, 1, vec![0, 0, 0, 255])))
    ///     .unwrap();
    /// assert!(reg.front_cpu_frame(id).unwrap().is_some());
    /// ```
    pub fn mark_ready_full(
        &mut self,
        id: SurfaceId,
        slot: u8,
        frame: Option<Box<dyn Frame>>,
        cpu: Option<CpuFrame>,
    ) -> Result<(), BridgeError> {
        if usize::from(slot) >= 2 {
            return Err(BridgeError::InvalidSlot(slot));
        }
        let ring = self
            .rings
            .get_mut(&id)
            .ok_or(BridgeError::UnknownSurface(id))?;
        if let Some(f) = frame {
            ring.set_frame(slot, f)?;
        }
        if let Some(c) = cpu {
            ring.set_cpu_frame(slot, c)?;
        }
        let size = ring.slots[usize::from(slot)]
            .frame
            .as_ref()
            .map(|f| f.size())
            .or_else(|| {
                ring.slots[usize::from(slot)]
                    .cpu_frame
                    .as_ref()
                    .map(|c| (c.width, c.height))
            });
        ring.mark_ready(slot)?;
        if let Some(s) = size {
            ring.set_front_size(s);
        }
        if self.ready_events.back() != Some(&id) {
            self.ready_events.push_back(id);
        }
        Ok(())
    }

    /// Publishes a [`CpuFrame`] for `id`'s slot `slot` and marks it
    /// ready — the TinySkia fallback payload. The frame's dimensions
    /// become the front size so `poll_frame` reports `Resized`.
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`], [`BridgeError::InvalidSlot`], or
    /// [`BridgeError::InvalidTransition`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::{BridgeRegistry, CpuFrame};
    ///
    /// let mut reg = BridgeRegistry::new();
    /// let id = reg.register();
    /// let (slot, _) = reg.acquire(id).unwrap();
    /// reg.mark_ready_cpu(id, slot, CpuFrame::new(2, 2, vec![255u8; 16]))
    ///     .unwrap();
    /// assert_eq!(reg.front_size(id).unwrap(), Some((2, 2)));
    /// ```
    pub fn mark_ready_cpu(
        &mut self,
        id: SurfaceId,
        slot: u8,
        cpu_frame: CpuFrame,
    ) -> Result<(), BridgeError> {
        let ring = self
            .rings
            .get_mut(&id)
            .ok_or(BridgeError::UnknownSurface(id))?;
        let size = (cpu_frame.width, cpu_frame.height);
        ring.set_cpu_frame(slot, cpu_frame)?;
        ring.mark_ready(slot)?;
        ring.set_front_size(size);
        if self.ready_events.back() != Some(&id) {
            self.ready_events.push_back(id);
        }
        Ok(())
    }

    /// Records the viewport `id`'s widget last laid out into — producers
    /// read it via [`viewport`](Self::viewport) to size renders.
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`] if `id` is not registered.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::{BridgeRegistry, Viewport};
    ///
    /// let mut reg = BridgeRegistry::new();
    /// let id = reg.register();
    /// reg.set_viewport(id, Viewport::new(800, 600, 2.0)).unwrap();
    /// assert_eq!(reg.viewport(id).unwrap().unwrap().size, (800, 600));
    /// ```
    pub fn set_viewport(
        &mut self,
        id: SurfaceId,
        viewport: crate::Viewport,
    ) -> Result<(), BridgeError> {
        self.rings
            .get_mut(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .set_viewport(viewport);
        Ok(())
    }

    /// The viewport last pushed for `id`, if any.
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`] if `id` is not registered.
    pub fn viewport(&self, id: SurfaceId) -> Result<Option<crate::Viewport>, BridgeError> {
        Ok(self
            .rings
            .get(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .viewport())
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

    /// Drains only the ready events for surfaces matching `keep`;
    /// non-matching events stay queued for other consumers.
    ///
    /// `SurfaceId`s are unique per registry, so events for surfaces a
    /// caller doesn't own must be preserved — `ExternalEngines` uses
    /// this to avoid consuming a widget-only surface's redraw signal.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeRegistry;
    ///
    /// let mut reg = BridgeRegistry::new();
    /// let a = reg.register();
    /// let b = reg.register();
    /// for s in [a, b] {
    ///     let (slot, _) = reg.acquire(s).unwrap();
    ///     reg.mark_ready(s, slot).unwrap();
    /// }
    /// assert_eq!(reg.drain_ready_matching(|s| s == a), vec![a]);
    /// // `b`'s event survives for its own consumer.
    /// assert_eq!(reg.drain_ready_matching(|s| s == b), vec![b]);
    /// ```
    pub fn drain_ready_matching(&mut self, keep: impl Fn(SurfaceId) -> bool) -> Vec<SurfaceId> {
        let mut matched = Vec::new();
        self.ready_events.retain(|id| {
            if keep(*id) {
                if !matched.contains(id) {
                    matched.push(*id);
                }
                false
            } else {
                true
            }
        });
        matched
    }

    /// Marks a writing slot ready and records the frame's size in one
    /// call. See [`SurfaceRing::mark_ready`] and
    /// [`SurfaceRing::set_front_size`].
    ///
    /// # Errors
    ///
    /// [`BridgeError::UnknownSurface`], [`BridgeError::InvalidSlot`], or
    /// [`BridgeError::InvalidTransition`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeRegistry;
    ///
    /// let mut reg = BridgeRegistry::new();
    /// let id = reg.register();
    /// let (slot, _) = reg.acquire(id).unwrap();
    /// reg.mark_ready_sized(id, slot, (320, 240)).unwrap();
    /// assert_eq!(reg.front_size(id).unwrap(), Some((320, 240)));
    /// ```
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
            token: ring.slots[usize::from(slot)].token,
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

    /// Whether `self` and `other` share the same underlying registry.
    ///
    /// Several `ExternalEngines` bindings may attach to the same
    /// registry through different handles — callers deduplicating
    /// registry-level queues (like `drain_ready`) use this to drain
    /// each registry once.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let a = BridgeHandle::new();
    /// let b = a.clone();
    /// let c = BridgeHandle::new();
    /// assert!(a.same_registry(&b));
    /// assert!(!a.same_registry(&c));
    /// ```
    pub fn same_registry(&self, other: &BridgeHandle) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
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
