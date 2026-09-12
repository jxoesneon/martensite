//! Producer/consumer protocol for embedding external GPU renderers inside
//! the Martensite widget tree.
//!
//! `martensite-engine-bridge` implements the Milestone `v0.14.0` external
//! surface foundation (see `docs/milestones/v0.14.0-external-surfaces.md`
//! and `docs/adr/ADR-0033-host-mode-external-surface-embedding.md`).
//! Martensite remains the application host: it owns the window, the winit
//! event loop, the `wgpu::Device`, and the compositing pass. External
//! renderers — a Bevy scene, a hardware video decoder, an offscreen
//! compositor — implement the [`Engine`] trait and produce [`Frame`]s;
//! Martensite samples their `wgpu::Texture` directly in its composite
//! pass with zero GPU copies.
//!
//! # Architecture
//!
//! - [`engine`]: the [`Engine`] producer trait and the [`EngineContext`]
//!   / [`Viewport`] types passed to it.
//! - [`frame`]: the [`Frame`] trait, [`FrameToken`], [`FrameSync`], and
//!   [`SourceAlpha`]. [`NativeFrame`] declares the cross-device handle
//!   descriptors exercised in later milestones.
//! - [`bridge`]: [`BridgeRegistry`] / [`BridgeHandle`] — the shared
//!   mailbox that tracks each surface's two-slot frame ring and queues
//!   ready events for damage signaling.
//! - [`testing`]: [`MockEngine`](testing::MockEngine), a synthetic
//!   producer used by the conformance tests.
//!
//! # Frame lifecycle
//!
//! ```text
//! producer thread                    host (paint) thread
//! ───────────────                    ─────────────────────
//! ring.acquire()      →  slot Writing
//! render into texture
//! ring.mark_ready()   →  slot Ready, ready_events.push(surface)
//!                        widget drains ready events → request_redraw
//!                        ring.take_front() → slot Compositing
//!                        WgpuHost::composite() samples the texture
//!                        ring.release() → slot Free
//! ring.drain_released() → producer recycles the slot
//! ```
//!
//! This crate contains zero `unsafe` code (`#![forbid(unsafe_code)]`).
//! The same-device sharing path requires none: `wgpu::Texture` objects
//! created on the host device are used directly by the producer.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod bridge;
pub mod engine;
pub mod error;
pub mod frame;
pub mod testing;

/// Maximum frame dimension accepted by the bridge (width or height).
///
/// Matches wgpu's default `max_texture_dimension_2d` limit (16384);
/// producers publishing larger frames get [`BridgeError::InvalidPayload`].
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::MAX_FRAME_DIM;
///
/// assert_eq!(MAX_FRAME_DIM, 16384);
/// ```
pub const MAX_FRAME_DIM: u32 = 16384;

pub use bridge::{
    BridgeHandle, BridgeRegistry, FrontFrame, SurfaceId, SurfaceRing, TakenFrontFrame,
};
pub use engine::{Engine, EngineContext, Viewport};
pub use error::BridgeError;
pub use frame::{
    CpuFrame, Frame, FrameSync, FrameToken, NativeFrame, SharedTexture, SourceAlpha, TextureFrame,
};
