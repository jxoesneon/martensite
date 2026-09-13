//! Headless [`App`](bevy::app::App) construction: plugin-group setup and
//! manual `wgpu` resource injection.
//!
//! `build_app` assembles a Bevy `App` that:
//!
//! * shares the host's `wgpu` instance/adapter/device/queue through
//!   [`RenderCreation::Manual`](bevy::render::settings::RenderCreation::Manual)
//!   — the zero-copy foundation;
//! * opens **no window** — [`WindowPlugin`](bevy::window::WindowPlugin) is
//!   configured with `primary_window: None` and `bevy_winit` is not compiled
//!   in, so nothing touches the OS event loop;
//! * runs single-threaded against the host pump — `PipelinedRenderingPlugin`
//!   is absent (the `multi_threaded` feature is off) and the app is driven by
//!   `App::update` once per
//!   [`Engine::render`](martensite_engine_bridge::Engine::render) call;
//! * spawns a `Camera3d` targeting
//!   [`CAMERA_VIEW_HANDLE`](crate::CAMERA_VIEW_HANDLE).
//!
//! The `App` is created inside the engine's render thread — `App` is `!Send`
//! and cannot be moved after construction.

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use bevy::DefaultPlugins;
use bevy::app::{App, PluginGroup};
use bevy::camera::{Camera3d, RenderTarget};
use bevy::math::Vec3;
use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy::render::RenderPlugin;
use bevy::render::renderer::{
    RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue, WgpuWrapper,
};
use bevy::render::settings::RenderCreation;
use bevy::render::texture::ManualTextureView;
use bevy::transform::components::Transform;
use bevy::window::{ExitCondition, WindowPlugin};
use martensite_engine_bridge::EngineEvent;
use martensite_wgpu::GpuContext;

use crate::input::MartensiteInputPlugin;
use crate::viewport::CAMERA_VIEW_HANDLE;

/// The `wgpu` handles Bevy needs for [`RenderCreation::Manual`].
///
/// wgpu 30 implements `Clone` on `Instance`/`Adapter`/`Device`/`Queue` (the
/// clones share the same underlying objects), so this is a cheap snapshot of
/// what [`GpuContext`] holds behind `Arc`s. Cloning is required because
/// `RenderCreation::manual` takes ownership — `RenderDevice`,
/// `RenderAdapter`, `RenderInstance`, and `RenderQueue` wrap their `wgpu`
/// objects in [`WgpuWrapper`], which has no `From<Arc<T>>`.
pub(crate) struct RenderHandles {
    /// The instance that created `adapter`/`device` (shared with the host).
    pub(crate) instance: wgpu::Instance,
    /// The adapter `device` was requested from.
    pub(crate) adapter: wgpu::Adapter,
    /// The logical device shared with the host's composite pass.
    pub(crate) device: wgpu::Device,
    /// The command queue shared with the host — same-queue ordering provides
    /// frame synchronization ([`FrameSync::None`](martensite_engine_bridge::FrameSync::None)).
    pub(crate) queue: wgpu::Queue,
    /// Cached adapter metadata for [`RenderAdapterInfo`].
    pub(crate) adapter_info: wgpu::AdapterInfo,
}

impl RenderHandles {
    /// Clones the five handles out of a [`GpuContext`].
    pub(crate) fn from_gpu(gpu: &GpuContext) -> Self {
        Self {
            instance: (*gpu.instance).clone(),
            adapter: (*gpu.adapter).clone(),
            device: (*gpu.device).clone(),
            queue: (*gpu.queue).clone(),
            adapter_info: gpu.adapter_info.clone(),
        }
    }

    /// Packages the handles as Bevy's manual render-creation payload.
    fn into_render_creation(self) -> RenderCreation {
        RenderCreation::manual(
            RenderDevice::new(WgpuWrapper::new(self.device)),
            RenderQueue(Arc::new(WgpuWrapper::new(self.queue))),
            RenderAdapterInfo(WgpuWrapper::new(self.adapter_info)),
            RenderAdapter(Arc::new(WgpuWrapper::new(self.adapter))),
            RenderInstance(Arc::new(WgpuWrapper::new(self.instance))),
        )
    }
}

/// Builds the headless Bevy [`App`] for one engine instance.
///
/// `render` carries the host's `wgpu` objects (see [`RenderHandles`]);
/// `events` is the receiver half of the engine's input channel drained by
/// [`MartensiteInputPlugin`]; `initial_view` is the `ManualTextureView`
/// registered under [`CAMERA_VIEW_HANDLE`] so `camera_system` resolves the
/// camera's target on the very first update.
///
/// The app is `finish`ed and `cleanup`ed before returning — `RenderPlugin`
/// moves the injected resources into the main and render worlds in
/// `finish()`, so callers must not (and need not) do it again; the app is
/// ready for `App::update` immediately.
pub(crate) fn build_app(
    render: RenderHandles,
    events: Receiver<EngineEvent>,
    initial_view: ManualTextureView,
) -> App {
    let mut app = App::new();

    let group = DefaultPlugins
        .build()
        // No window, no exit-on-close: the Martensite host owns the window
        // and the app lifetime. `WindowPlugin` stays in the group because it
        // registers the window/input `Messages` and `PrimaryWindow` type the
        // picking/input systems reference.
        .set(WindowPlugin {
            primary_window: None,
            primary_cursor_options: None,
            exit_condition: ExitCondition::DontExit,
            close_when_requested: false,
        })
        // Inject the host's GPU instead of letting Bevy create its own
        // instance/adapter/device — this is what makes the produced frames
        // same-device textures for the host's composite pass.
        .set(RenderPlugin {
            render_creation: render.into_render_creation(),
            synchronous_pipeline_compilation: true,
            ..Default::default()
        });
    // An embedded library must not hijack the process's Ctrl+C handling;
    // the plugin only exists on unix (non-horizon) and Windows.
    #[cfg(any(all(unix, not(target_os = "horizon")), windows))]
    let group = group.disable::<bevy::app::TerminalCtrlCHandlerPlugin>();
    app.add_plugins(group);

    // Mesh ray-cast picking backend (PointerInput messages come from
    // MartensiteInputPlugin; `PointerInputPlugin`'s winit window input is
    // inert here because there are no windows).
    app.add_plugins(MeshPickingPlugin);

    app.add_plugins(MartensiteInputPlugin::new(events, CAMERA_VIEW_HANDLE));

    // The engine camera renders into whichever ring-slot texture is
    // registered under CAMERA_VIEW_HANDLE this frame.
    app.world_mut()
        .resource_mut::<bevy::render::texture::ManualTextureViews>()
        .insert(CAMERA_VIEW_HANDLE, initial_view);
    // `RenderTarget` is its own component at this Bevy revision (a required
    // component of `Camera`); inserting it directly overrides the
    // `Window(Primary)` default.
    app.world_mut().spawn((
        Camera3d::default(),
        RenderTarget::TextureView(CAMERA_VIEW_HANDLE),
        Transform::from_xyz(0.0, 0.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // `RenderPlugin::finish` moves the injected `RenderResources` into the
    // main and render worlds; `cleanup` finalizes plugin state. `App::update`
    // only refuses to run while a plugin is mid-build, so the app is ready
    // after these two calls.
    app.finish();
    app.cleanup();

    app
}
