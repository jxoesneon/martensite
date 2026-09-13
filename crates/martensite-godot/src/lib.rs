//! `martensite-godot` — the Godot 4 GDExtension adapter that ships
//! `SubViewport` frames to a Martensite host, for the v0.15.0
//! engine-showcase milestone.
//!
//! Built on `godot` (gdext) 0.5.x; the crate compiles as a `cdylib`
//! (the GDExtension loaded by Godot) **and** as an `rlib` (linked by the
//! Martensite host app for the in-process [`host`] side).
//!
//! # The two tiers — and the honesty contract
//!
//! - **Tier 1 (default, shippable):** [`viewport::MartensiteViewport`]
//!   maps the `SubViewport`'s `ViewportTexture` to a `RenderingDevice`
//!   RID (`RenderingServer::texture_get_rd_texture`) and issues
//!   `texture_get_data_async` readbacks, throttled to a bounded number
//!   of in-flight requests. Callbacks ship pixels through
//!   [`transport`]; the host-side [`host::GodotEngine`] uploads them via
//!   `queue.write_texture` into the slot texture published to the
//!   `martensite-engine-bridge` ring. This costs **one GPU→CPU readback
//!   plus one CPU→GPU upload per frame** (plus the socket/channel copy
//!   and one bookkeeping clone) — measured via `stats()`, documented in
//!   the README.
//! - **Tier 2 (feature `godot-gpu-copy`, experimental):**
//!   [`experimental_shared::MartensiteSharedBlit`], a `CompositorEffect`
//!   that `texture_copy`s the scene color buffer into a host-allocated
//!   texture imported via `texture_create_from_extension`. **One GPU→GPU
//!   copy — still not zero-copy**, and synchronization is best-effort
//!   because Godot exposes no public fence/semaphore API to extensions.
//!
//! True zero-copy is **not available** without upstream Godot engine
//! patches (export-flagged internal render targets, external-texture
//! render targets, a public fence/semaphore API, and the merged
//! `libgodot` native-window hooks). The README lists them; this crate's
//! docs and code never claim otherwise.
//!
//! # Module map
//!
//! - [`viewport`] — `MartensiteViewport` (`GodotClass`, `Node`):
//!   `SubViewport` wiring, the `process`/`frame_post_draw` pumps, and
//!   `stats()` instrumentation.
//! - [`readback`] — the `texture_get_data_async` wrapper that turns
//!   `PackedByteArray` callbacks into framed pixels.
//! - [`transport`] — `FrameMsg` codec plus TCP / Unix-socket /
//!   in-process-channel transports. Pure Rust: no Godot runtime needed.
//! - [`host`] — `GodotEngine`, the `martensite-engine-bridge::Engine`
//!   the host app registers. Pure Rust + wgpu: no Godot runtime needed.
//! - [`experimental_shared`] — `cfg(feature = "godot-gpu-copy")` only.
//!
//! # Safety
//!
//! This crate is the workspace's eighth `#![allow(unsafe_code)]`
//! boundary: the `#[gdextension]` entry point is an FFI surface and
//! requires `unsafe impl ExtensionLibrary`. Every `unsafe` block carries
//! a `// SAFETY:` comment; host/transport/readback logic itself is safe
//! Rust.

// SAFETY-level: this crate is the GDExtension FFI boundary —
// `#[gdextension]` requires `unsafe impl ExtensionLibrary`, and the
// Tier-2 shared-texture path passes opaque native handles. Confined to
// this adapter per the v0.15.0 unsafe-code policy.
#![allow(unsafe_code)]

pub mod host;
pub mod readback;
pub mod transport;
pub mod viewport;

#[cfg(feature = "godot-gpu-copy")]
pub mod experimental_shared;

use godot::init::{ExtensionLibrary, gdextension};

/// The GDExtension entry point Godot loads.
struct MartensiteGodot;

// SAFETY: `ExtensionLibrary` is `unsafe` because the implementor asserts
// the extension respects GDExtension init/deinit ordering. The defaults
// (Scene-level init, tool-classes-only editor behavior) are correct for
// this adapter — no classes need earlier init levels.
#[gdextension]
unsafe impl ExtensionLibrary for MartensiteGodot {}
