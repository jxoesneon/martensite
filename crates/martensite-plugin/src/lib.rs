//! WebAssembly plugin runtime for Martensite.
//!
//! This crate embeds a Wasmtime sandbox and exposes a zero-allocation shared
//! memory ring buffer so that third-party widgets can push paint commands at
//! 60 Hz / 120 Hz without host-trampoline overhead. Plugins run with a fuel
//! budget and epoch-based interruption, and every host call is validated against
//! an explicit capability set.
//!
//! The crate is written entirely in safe Rust.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Zero-allocation shared-memory ring buffer for paint commands.
pub mod ring_buffer;
/// Wasmtime sandbox, WASIp1 context, and plugin instance lifecycle.
pub mod runtime;
/// Capability-based security model and builder API.
pub mod security;

pub use ring_buffer::{
    PluginPaintCmd, PluginRingBuffer, RingBufferError, DEFAULT_CAPACITY, SHARED_HEADER_SIZE,
};
pub use runtime::{
    PluginError, PluginInstance, PluginRuntime, PluginState, DEFAULT_FUEL_BUDGET,
    RING_BUFFER_REGION_SIZE,
};
pub use security::{Capability, CapabilitySet, PluginBuilder};
