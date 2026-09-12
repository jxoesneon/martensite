//! Error type for bridge operations.

use crate::bridge::SurfaceId;
use std::fmt;

/// Errors returned by [`BridgeRegistry`](crate::BridgeRegistry) and
/// [`SurfaceRing`](crate::SurfaceRing) operations.
///
/// # Examples
///
/// ```
/// use martensite_engine_bridge::{BridgeError, SurfaceId};
///
/// let err = BridgeError::UnknownSurface(SurfaceId(9));
/// assert!(err.to_string().contains("unknown surface"));
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BridgeError {
    /// The [`SurfaceId`] is not registered in the registry.
    UnknownSurface(SurfaceId),
    /// No free ring slot exists — the producer must wait for the host to
    /// release a composited frame (or drop this frame).
    RingExhausted,
    /// The slot index is not part of the ring (valid indices are `0` and
    /// `1`).
    InvalidSlot(u8),
    /// The requested transition is not legal from the slot's current
    /// state (e.g. marking a `Free` slot ready).
    InvalidTransition,
    /// The published frame payload is malformed — a `CpuFrame` whose
    /// pixel length does not match `width * height * 4`, or dimensions
    /// exceeding [`crate::MAX_FRAME_DIM`].
    InvalidPayload,
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSurface(id) => write!(f, "unknown surface {id:?}"),
            Self::RingExhausted => {
                write!(f, "ring exhausted: no free slot for a new frame")
            }
            Self::InvalidSlot(slot) => write!(f, "invalid ring slot {slot}"),
            Self::InvalidTransition => {
                write!(f, "illegal ring-slot state transition")
            }
            Self::InvalidPayload => {
                write!(
                    f,
                    "frame payload is malformed (bad dimensions or pixel length)"
                )
            }
        }
    }
}

impl std::error::Error for BridgeError {}
