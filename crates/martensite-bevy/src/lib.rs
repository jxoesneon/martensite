//! Host-mode Bevy 3D viewport adapter for Martensite.
//!
//! `martensite-bevy` embeds a headless Bevy [`App`](bevy::app::App) as a
//! [`martensite_engine_bridge::Engine`] producer: each
//! [`Engine::render`](martensite_engine_bridge::Engine::render) call pumps one
//! Bevy update so the engine's `Camera3d` draws into a `wgpu::Texture` the host
//! composites **zero-copy** — Bevy's renderer and the host share the same
//! `wgpu::Device` via
//! [`RenderCreation::Manual`](bevy::render::settings::RenderCreation).
//!
//! # Architecture
//!
//! ```text
//! host paint thread                    martensite-bevy render thread
//! ─────────────────                    ────────────────────────────
//! engine.render(viewport)  ──cmd──►    ring.acquire() → slot Writing
//!                                      register slot texture as a
//!                                      ManualTextureView
//!                                      app.update() → Camera3d renders
//!                                      device.poll(Wait)
//!                                      ring.mark_ready_full(frame)
//!                       ◄──reply──     BevyFrame { texture, size }
//! host composites the slot texture
//! ring.release()           ──►         engine.release(token)
//! ```
//!
//! A `bevy::app::App` is `!Send`/`!Sync` (its `runner` is a
//! `Box<dyn FnOnce(App) -> AppExit>` without `Send` bounds), so the `App`
//! lives on a dedicated render thread owned by [`BevyEngine`]. `render`,
//! `on_event`, and [`BevyEngine::with_app`] communicate with it over
//! `std::sync::mpsc` channels; frames and textures cross the boundary because
//! `wgpu` 30 resources are `Send + Sync` and `Clone` (internally shared).
//!
//! # Input
//!
//! [`Engine::on_event`](martensite_engine_bridge::Engine::on_event) queues
//! [`EngineEvent`](martensite_engine_bridge::EngineEvent)s that a `PreUpdate`
//! system drains into Bevy's input pipeline: `PointerInput` messages for
//! `bevy_picking` (targeted at the camera's
//! [`ManualTextureViewHandle`](bevy::camera::ManualTextureViewHandle)),
//! `MouseButtonInput`/`MouseWheel`/`KeyboardInput` messages, and
//! `KeyboardFocusLost` on focus loss.
//!
//! This crate contains zero `unsafe` code (`#![forbid(unsafe_code)]`).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Headless [`App`](bevy::app::App) construction: plugin-group setup and
/// manual `wgpu` resource injection.
pub mod app;
/// [`BevyEngine`] — the
/// [`Engine`](martensite_engine_bridge::Engine) implementation.
pub mod engine;
/// [`EngineEvent`](martensite_engine_bridge::EngineEvent) → Bevy input
/// mapping (`MartensiteInputPlugin`).
pub mod input;
/// Ring-slot texture pool and
/// [`ManualTextureViews`](bevy::render::texture::ManualTextureViews)
/// registration.
pub mod viewport;

/// Re-export of the exact Bevy revision this adapter is built against, so
/// hosts can write scene code (`martensite_bevy::bevy::prelude::*`) without
/// risking a version skew against the injected `wgpu` resources.
pub use bevy;

pub use engine::{BevyAdapterError, BevyEngine, BevyFrame};
pub use input::MartensiteInputPlugin;
pub use viewport::CAMERA_VIEW_HANDLE;
