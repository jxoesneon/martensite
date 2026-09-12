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
pub use martensite_core::SurfaceId;

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

/// The payload extracted by
/// [`SurfaceRing::take_front_frame`]/[`BridgeRegistry::take_front_frame`].
///
/// The slot is already in `Compositing` state; the caller owns the
/// published [`Frame`] (if any) and must eventually call
/// [`SurfaceRing::release`]/[`BridgeRegistry::release`] on `slot` after
/// the GPU has consumed it.
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::SurfaceRing;
///
/// let mut ring = SurfaceRing::new();
/// assert!(ring.take_front_frame().is_none());
/// ```
pub struct TakenFrontFrame {
    /// The ring slot transitioned to `Compositing`.
    pub slot: u8,
    /// The published frame's unique token.
    pub token: FrameToken,
    /// The frame payload — `Some` for GPU-published frames
    /// (`mark_ready_frame`/`mark_ready_full`), `None` for CPU-only
    /// publications (`mark_ready_cpu`).
    pub frame: Option<Box<dyn Frame>>,
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
        // Free a stale ready slot: the mailbox keeps only the newest
        // frame. The dropped frame's token is pushed to `released` so a
        // producer tracking per-token resources can recycle it — without
        // this, mailbox overwrites would leak producer-side resources.
        if let Some(old_front) = self.front {
            let old = usize::from(old_front);
            if old != idx && self.slots[old].state == SlotState::Ready {
                self.push_released(self.slots[old].token);
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

    /// Takes the front `Ready` slot for compositing, moving the
    /// published frame payload out of the ring.
    ///
    /// Returns `(slot, token, frame)` where `frame` is `Some` for the
    /// GPU path (`mark_ready_full`/`set_frame`) and `None` for
    /// CPU-published frames. The slot transitions to `Compositing`;
    /// call [`release`](Self::release) when the host has submitted the
    /// composite. Because the payload is owned by the caller, the
    /// registry lock need not be held while the GPU reads the texture.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::SurfaceRing;
    ///
    /// let mut ring = SurfaceRing::new();
    /// assert!(ring.take_front_frame().is_none());
    /// ```
    pub fn take_front_frame(&mut self) -> Option<TakenFrontFrame> {
        let idx = usize::from(self.front?);
        if self.slots[idx].state != SlotState::Ready {
            return None;
        }
        let frame = self.slots[idx].frame.take();
        self.slots[idx].state = SlotState::Compositing;
        Some(TakenFrontFrame {
            slot: idx as u8,
            token: self.slots[idx].token,
            frame,
        })
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
        self.push_released(token);
        Ok(())
    }

    /// Maximum number of released tokens queued for producer recycling.
    /// If a producer never calls [`drain_released`](Self::drain_released)
    /// the oldest tokens are dropped — a lost token only means the
    /// producer doesn't reclaim that texture, not a host stall.
    const RELEASED_WATERMARK: usize = 128;

    fn push_released(&mut self, token: FrameToken) {
        if self.released.len() >= Self::RELEASED_WATERMARK {
            self.released.remove(0);
        }
        self.released.push(token);
    }

    /// Force-frees a slot stuck in `Writing` or `Ready`.
    ///
    /// The host calls this when a producer acquired a slot and never
    /// published (abandoned `Writing`), or when a `Ready` frame must be
    /// evicted without compositing. The slot's token is queued to
    /// `released` so the producer can recycle its resources.
    ///
    /// Must NOT be called on a `Compositing` slot — the host may still
    /// be sampling its texture. Returns `false` for `Compositing`,
    /// `Free`, or invalid slot indices.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::SurfaceRing;
    ///
    /// let mut ring = SurfaceRing::new();
    /// let (slot, _) = ring.acquire().unwrap();
    /// // Producer abandoned the slot without mark_ready.
    /// assert!(ring.force_release(slot));
    /// assert!(ring.acquire().is_ok());
    /// ```
    pub fn force_release(&mut self, slot: u8) -> bool {
        let idx = usize::from(slot);
        if idx >= 2 {
            return false;
        }
        match self.slots[idx].state {
            SlotState::Writing | SlotState::Ready => {
                let token = self.slots[idx].token;
                self.slots[idx].state = SlotState::Free;
                self.slots[idx].frame = None;
                self.slots[idx].cpu_frame = None;
                if self.front == Some(slot) {
                    self.front = None;
                }
                self.push_released(token);
                true
            }
            _ => false,
        }
    }

    /// Frees every slot stuck in `Writing` or `Ready` and returns the
    /// number freed. Called by the host when a producer is considered
    /// stalled (e.g., widget removed, engine stopped responding).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::SurfaceRing;
    ///
    /// let mut ring = SurfaceRing::new();
    /// ring.acquire().unwrap();
    /// assert_eq!(ring.reclaim_stalled(), 1);
    /// ```
    pub fn reclaim_stalled(&mut self) -> usize {
        let mut n = 0;
        for slot in 0..2u8 {
            if self.force_release(slot) {
                n += 1;
            }
        }
        n
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
    #[must_use]
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
    #[must_use]
    pub fn front_cpu_frame(&self) -> Option<&CpuFrame> {
        let idx = usize::from(self.front?);
        self.slots[idx].cpu_frame.as_ref()
    }

    /// Stores the [`Frame`] for a `Writing` slot without marking it
    /// ready — call before [`SurfaceRing::mark_ready`].
    ///
    /// # Errors
    ///
    /// [`BridgeError::InvalidSlot`] for slot indices ≥ 2,
    /// [`BridgeError::InvalidTransition`] if the slot is not `Writing`,
    /// or [`BridgeError::InvalidPayload`] if the frame's dimensions
    /// exceed [`crate::MAX_FRAME_DIM`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::{Frame, FrameSync, FrameToken, SourceAlpha, SurfaceRing};
    ///
    /// struct Solid;
    /// impl Frame for Solid {
    ///     fn token(&self) -> FrameToken { FrameToken(0) }
    ///     fn same_device_texture(&self) -> Option<&wgpu::Texture> { None }
    ///     fn native_handle(&self) -> Option<martensite_engine_bridge::NativeFrame> { None }
    ///     fn sync(&self) -> FrameSync { FrameSync::None }
    ///     fn size(&self) -> (u32, u32) { (4, 4) }
    ///     fn alpha_mode(&self) -> SourceAlpha { SourceAlpha::Premultiplied }
    /// }
    /// let mut ring = SurfaceRing::new();
    /// let (slot, _) = ring.acquire().unwrap();
    /// ring.set_frame(slot, Box::new(Solid)).unwrap();
    /// ```
    pub fn set_frame(&mut self, slot: u8, frame: Box<dyn Frame>) -> Result<(), BridgeError> {
        let idx = usize::from(slot);
        if idx >= 2 {
            return Err(BridgeError::InvalidSlot(slot));
        }
        let (w, h) = frame.size();
        if w > crate::MAX_FRAME_DIM || h > crate::MAX_FRAME_DIM {
            return Err(BridgeError::InvalidPayload);
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::{CpuFrame, SurfaceRing};
    ///
    /// let mut ring = SurfaceRing::new();
    /// let (slot, _) = ring.acquire().unwrap();
    /// ring.set_cpu_frame(slot, CpuFrame::new(1, 1, vec![0, 0, 0, 255])).unwrap();
    /// ```
    pub fn set_cpu_frame(&mut self, slot: u8, frame: CpuFrame) -> Result<(), BridgeError> {
        let expected = frame.width as usize * frame.height as usize * 4;
        if frame.width == 0
            || frame.height == 0
            || frame.width > crate::MAX_FRAME_DIM
            || frame.height > crate::MAX_FRAME_DIM
            || frame.pixels.len() != expected
        {
            return Err(BridgeError::InvalidPayload);
        }
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::{SurfaceRing, Viewport};
    ///
    /// let mut ring = SurfaceRing::new();
    /// ring.set_viewport(Viewport::new(800, 600, 2.0));
    /// assert_eq!(ring.viewport().unwrap().size, (800, 600));
    /// ```
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
    #[must_use]
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
    #[must_use]
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
    #[must_use]
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
    /// Invoked once per newly-ready surface (the `notify_frame_ready`
    /// hook) — the app installs a closure that marks the owning widget
    /// dirty and calls `window.request_redraw()`.
    ready_waker: Option<Box<dyn Fn(SurfaceId) + Send + Sync>>,
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
            ready_waker: None,
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeRegistry;
    ///
    /// let mut registry = BridgeRegistry::new();
    /// let id = registry.register();
    /// let (slot, _token) = registry.acquire(id).unwrap();
    /// assert!(slot < 2);
    /// ```
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
    /// ```
    pub fn mark_ready(&mut self, id: SurfaceId, slot: u8) -> Result<(), BridgeError> {
        self.rings
            .get_mut(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .mark_ready(slot)?;
        self.push_ready_event(id);
        Ok(())
    }

    /// Takes the front slot of `id` for compositing, moving the
    /// published frame payload out of the ring.
    ///
    /// Unlike [`take_front`](Self::take_front), which borrows the frame
    /// through [`front_frame`](Self::front_frame), this variant hands
    /// ownership of the `Box<dyn Frame>` to the caller. The registry
    /// lock can then be released while the host records the composite —
    /// the producer's `acquire`/`mark_ready` calls are not blocked for
    /// the duration of the GPU pass. The slot remains `Compositing`
    /// until [`release`](Self::release).
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
    /// let mut registry = BridgeRegistry::new();
    /// let id = registry.register();
    /// assert!(registry.take_front_frame(id).unwrap().is_none());
    /// ```
    pub fn take_front_frame(
        &mut self,
        id: SurfaceId,
    ) -> Result<Option<TakenFrontFrame>, BridgeError> {
        Ok(self
            .rings
            .get_mut(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .take_front_frame())
    }

    /// Takes the front slot of `id` for compositing.
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
    /// let mut registry = BridgeRegistry::new();
    /// let id = registry.register();
    /// assert!(registry.take_front(id).unwrap().is_none());
    /// ```
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
    /// let (front, _) = registry.take_front(id).unwrap().unwrap();
    /// registry.release(id, front).unwrap();
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeRegistry;
    ///
    /// let mut registry = BridgeRegistry::new();
    /// let id = registry.register();
    /// assert!(registry.drain_released(id).unwrap().is_empty());
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeRegistry;
    ///
    /// let mut registry = BridgeRegistry::new();
    /// let id = registry.register();
    /// assert!(registry.front_frame(id).unwrap().is_none());
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeRegistry;
    ///
    /// let mut registry = BridgeRegistry::new();
    /// let id = registry.register();
    /// assert!(registry.front_cpu_frame(id).unwrap().is_none());
    /// ```
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
        // Validate both payloads before storing either — a failing
        // CpuFrame must not leave a half-applied frame in the slot.
        if let Some(c) = &cpu {
            let expected = c.width as usize * c.height as usize * 4;
            if c.width == 0
                || c.height == 0
                || c.width > crate::MAX_FRAME_DIM
                || c.height > crate::MAX_FRAME_DIM
                || c.pixels.len() != expected
            {
                return Err(BridgeError::InvalidPayload);
            }
        }
        if let Some(f) = &frame {
            let (w, h) = f.size();
            if w > crate::MAX_FRAME_DIM || h > crate::MAX_FRAME_DIM {
                return Err(BridgeError::InvalidPayload);
            }
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
        self.push_ready_event(id);
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
        self.push_ready_event(id);
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::{BridgeRegistry, Viewport};
    ///
    /// let mut reg = BridgeRegistry::new();
    /// let id = reg.register();
    /// reg.set_viewport(id, Viewport::new(800, 600, 1.0)).unwrap();
    /// assert!(reg.viewport(id).unwrap().is_some());
    /// ```
    pub fn viewport(&self, id: SurfaceId) -> Result<Option<crate::Viewport>, BridgeError> {
        Ok(self
            .rings
            .get(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .viewport())
    }

    // Maximum queued ready events. Beyond this watermark the oldest
    // event is dropped — a missed event only means one skipped redraw
    // for that surface, not a stalled ring.
    const READY_EVENTS_WATERMARK: usize = 1024;

    /// Installs the ready waker — invoked once per newly-ready surface
    /// (the spec's `notify_frame_ready` → "mark widget dirty +
    /// `request_redraw`" hook).
    ///
    /// The closure is called while the registry lock is held; it must
    /// not re-enter the registry (e.g. post to the event loop, don't
    /// call `lock()` inline).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use std::sync::atomic::{AtomicUsize, Ordering};
    /// use martensite_engine_bridge::BridgeRegistry;
    ///
    /// let wakes = Arc::new(AtomicUsize::new(0));
    /// let w = Arc::clone(&wakes);
    /// let mut registry = BridgeRegistry::new();
    /// registry.set_ready_waker(Some(Box::new(move |_id| {
    ///     w.fetch_add(1, Ordering::Relaxed);
    /// })));
    /// let id = registry.register();
    /// let (slot, _) = registry.acquire(id).unwrap();
    /// registry.mark_ready(id, slot).unwrap();
    /// assert_eq!(wakes.load(Ordering::Relaxed), 1);
    /// ```
    pub fn set_ready_waker(&mut self, waker: Option<Box<dyn Fn(SurfaceId) + Send + Sync>>) {
        self.ready_waker = waker;
    }

    fn push_ready_event(&mut self, id: SurfaceId) {
        if self.ready_events.back() != Some(&id) {
            if self.ready_events.len() >= Self::READY_EVENTS_WATERMARK {
                self.ready_events.pop_front();
            }
            self.ready_events.push_back(id);
            if let Some(waker) = &self.ready_waker {
                waker(id);
            }
        }
    }

    /// Force-frees a slot stuck in `Writing` or `Ready` on `id`'s ring.
    /// See [`SurfaceRing::force_release`].
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
    /// let mut registry = BridgeRegistry::new();
    /// let id = registry.register();
    /// let (slot, _) = registry.acquire(id).unwrap();
    /// assert!(registry.force_release(id, slot).unwrap());
    /// ```
    pub fn force_release(&mut self, id: SurfaceId, slot: u8) -> Result<bool, BridgeError> {
        Ok(self
            .rings
            .get_mut(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .force_release(slot))
    }

    /// Frees every `Writing`/`Ready` slot on `id`'s ring — the
    /// host-side recovery path for a stalled producer. Returns the
    /// number of slots reclaimed. See [`SurfaceRing::reclaim_stalled`].
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
    /// let mut registry = BridgeRegistry::new();
    /// let id = registry.register();
    /// registry.acquire(id).unwrap();
    /// assert_eq!(registry.reclaim_stalled(id).unwrap(), 1);
    /// ```
    pub fn reclaim_stalled(&mut self, id: SurfaceId) -> Result<usize, BridgeError> {
        Ok(self
            .rings
            .get_mut(&id)
            .ok_or(BridgeError::UnknownSurface(id))?
            .reclaim_stalled())
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
        self.push_ready_event(id);
        Ok(())
    }

    /// The front slot index of `id`, if a frame is ready or compositing.
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
    /// let mut registry = BridgeRegistry::new();
    /// let id = registry.register();
    /// assert_eq!(registry.front(id).unwrap(), None);
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeRegistry;
    ///
    /// let mut registry = BridgeRegistry::new();
    /// let id = registry.register();
    /// assert_eq!(registry.front_size(id).unwrap(), None);
    /// ```
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
    #[must_use]
    pub fn same_registry(&self, other: &BridgeHandle) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// A process-unique identifier for the underlying registry.
    ///
    /// Cloned handles share the same id; two handles built from
    /// different registries never collide. Useful for deduplicating
    /// handles into a `HashMap`/`HashSet` (e.g. draining each registry's
    /// ready queue exactly once).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let a = BridgeHandle::new();
    /// assert_eq!(a.registry_id(), a.clone().registry_id());
    /// assert_ne!(a.registry_id(), BridgeHandle::new().registry_id());
    /// ```
    #[must_use]
    pub fn registry_id(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }

    /// Installs the registry's ready waker — see
    /// [`BridgeRegistry::set_ready_waker`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_engine_bridge::BridgeHandle;
    ///
    /// let handle = BridgeHandle::new();
    /// handle.set_ready_waker(Some(Box::new(|_surface| {
    ///     // mark widget dirty + window.request_redraw()
    /// })));
    /// ```
    pub fn set_ready_waker(&self, waker: Option<Box<dyn Fn(SurfaceId) + Send + Sync>>) {
        self.lock().set_ready_waker(waker);
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
