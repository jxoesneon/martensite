# martensite-bevy

Host-mode **Bevy 3D** viewport adapter for Martensite.

`martensite-bevy` embeds a headless [Bevy](https://bevyengine.org) `App`
(PBR + picking) as a
[`martensite-engine-bridge`](https://crates.io/crates/martensite-engine-bridge)
`Engine` producer: each `Engine::render` call pumps one Bevy update so the
engine's `Camera3d` draws into a `wgpu::Texture` the host composites
**without any copy** — Bevy's renderer shares the host's exact
`wgpu::Instance`/`Adapter`/`Device`/`Queue` through
`RenderCreation::Manual`, and published frames report
`FrameSync::None` (same-queue serial submission ordering).

```text
host paint thread                    martensite-bevy render thread
─────────────────                    ────────────────────────────
engine.render(viewport)  ──cmd──►    ring.acquire() → slot Writing
                                     register slot texture as a
                                     ManualTextureView
                                     app.update() → Camera3d renders
                                     device.poll(Wait)
                                     ring.mark_ready_full(frame)
                      ◄──reply──     BevyFrame { texture, size }
host composites the slot texture
ring.release()           ──►         engine.release(token)
```

## Status

**Pre-release.** The crate is pinned to a Bevy `main` revision —
`5036d978a294a3fbb1c42bf005d6a255e2978a74` ("Upgrade to wgpu 30",
v0.20.0-dev) — because same-device injection requires Bevy's `wgpu` to be
the same `wgpu` version the host uses, and no crates.io Bevy release builds
on wgpu 30 yet.

Consequences:

- `publish = false` — the git dependency cannot go to crates.io. The pin is
  expected to be replaced by a versioned dependency once **Bevy 0.20**
  ships with wgpu 30.
- The crate is **excluded from the workspace** (empty `[workspace]` table in
  its manifest, like `martensite-godot`) so the pin does not constrain the
  shared lockfile; build it via `--manifest-path`.
- Bevy's API is not stable at this revision; expect small upstream churn.

## Usage

```rust,no_run
use martensite_bevy::BevyEngine;
use martensite_engine_bridge::BridgeHandle;
use martensite_wgpu::GpuContext;

let gpu = GpuContext::new().expect("GPU");
let handle = BridgeHandle::new();
let surface = handle.lock().register();
let mut engine = BevyEngine::new(
    &gpu,
    handle,
    surface,
    (1280, 720),
    wgpu::TextureFormat::Bgra8UnormSrgb, // match the host surface format
).expect("bevy app boots");

// Author the scene on the render thread (the App is !Send and lives there).
engine.with_app(|app| {
    use martensite_bevy::bevy::prelude::*;
    app.world_mut().spawn((
        PointLight::default(),
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));
});
```

The engine app starts with one `Camera3d` at `(0, 0, 5)` looking at the
origin, targeting `RenderTarget::TextureView(CAMERA_VIEW_HANDLE)`.

## Frame lifecycle

- The engine owns **one `wgpu::Texture` per ring slot** (the bridge ring has
  two), with `RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC` usage.
- Each `render()`: pick a free slot → register its texture as a
  `ManualTextureView` under the fixed `CAMERA_VIEW_HANDLE` →
  `app.update()` → `device.poll(Wait)` → `mark_ready_full`. If both slots
  are in flight, `render()` returns `None` and the host keeps the last
  frame.
- `release(token)` frees the published-frame bookkeeping; the slot texture
  itself is reused in place (it is `Arc`-shared, so a released
  `Box<dyn Frame>` is never the last owner).
- `to_pixmap(token)` performs a `COPY_SRC` → `MAP_READ` readback of the
  pending frame's texture (B/R-swizzled for `Bgra*` formats). It is the
  explicit CPU-fallback path and never runs on the streaming path.
- `bevy::app::App` is `!Send`/`!Sync`, so it lives on a dedicated render
  thread; `render`/`with_app`/`on_event` communicate over channels. A
  panicking `App::update` is caught and yields `None` for that frame —
  the thread stays alive.

## Input

`EngineEvent`s queued via `on_event` are drained in `PreUpdate` (before
`InputSystems` and `PickingSystems::ProcessInput`) into:

- `PointerInput` messages for `bevy_picking` (locations target
  `NormalizedRenderTarget::TextureView(CAMERA_VIEW_HANDLE)`), plus
  `MeshPickingPlugin` for mesh ray-casts;
- `MouseButtonInput` / `MouseWheel` / `KeyboardInput` messages feeding
  `ButtonInput<MouseButton>`, `AccumulatedMouseScroll`, and
  `ButtonInput<KeyCode>`/`ButtonInput<Key>`;
- `WindowFocused` (+ `KeyboardFocusLost` on focus loss).

Positions are surface-local physical pixels; scroll deltas are physical
pixels (`MouseScrollUnit::Pixel`). Keyboard events carry the raw platform
scancode as `KeyCode::Unidentified(NativeKeyCode::{Windows|MacOS|Xkb|Android})`
— layout-independent `KeyCode`s cannot be recovered from a scancode alone.
`TextInput` arrives as a synthetic press/release `KeyboardInput` pair with
`text`/`logical_key` populated. Pointer buttons beyond
primary/secondary/middle (`Back`/`Forward`/`Other`) reach
`ButtonInput<MouseButton>` but not `bevy_picking` (its `PointerButton` has
only three variants).

## Resize

`render()` compares `Viewport.size` against the slot allocation and
recreates both textures when it changes; the manual view is re-registered
every frame regardless. In-flight frames keep their own (old-size)
`Arc<Texture>` alive — the ring records each frame's own size.

## Testing

```sh
cargo check --manifest-path crates/martensite-bevy/Cargo.toml
cargo test  --manifest-path crates/martensite-bevy/Cargo.toml \
            --features test-noop
```

`test-noop` enables wgpu's `noop` backend (forwarded to
`martensite-engine-bridge`); the tests boot a real headless Bevy app on a
validating no-op adapter/device and exercise construction, frame
production, input draining, resize, and readback without a GPU.
