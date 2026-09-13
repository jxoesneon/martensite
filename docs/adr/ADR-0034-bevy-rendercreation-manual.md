# [ADR-0034] Bevy Host-Mode Device Injection via `RenderCreation::Manual`

* **Status:** Accepted
* **Date:** 2026-09-12
* **Deciders:** Martensite Architecture Working Group
* **Technical Domain:** `martensite-bevy`, `martensite-wgpu`,
  `martensite-engine-bridge`
* **Amends:** None; implements ADR-0033 (host-mode embedding) for the
  Bevy adapter.

## Context and Problem Statement

ADR-0033 chose host-mode embedding: Martensite owns the window, wgpu
device, and compositing; the external engine renders into a
Martensite-owned texture. For Bevy this requires sharing a single
`wgpu::Device`/`Queue` between Martensite and the Bevy `RenderApp` —
`wgpu::Texture` objects are device-bound and wgpu exposes no
device-exportable handles, so a second Bevy-owned device cannot
produce textures the host can composite.

Two hard constraints shape the decision:

* **wgpu version coupling.** The workspace is on wgpu 30. Released Bevy
  0.19.x depends on `wgpu ^29` — the two major versions cannot coexist
  as a single type, so `RenderDevice`/`RenderQueue` could not wrap the
  host's handles. wgpu 30 landed on Bevy `main` for the 0.20 milestone
  (bevyengine/bevy#24841). Until 0.20 ships, `martensite-bevy` must pin
  `bevy = { git = "https://github.com/bevyengine/bevy", rev = "5036d97" }`
  — the wgpu-30 merge commit on Bevy `main`.
* **Frame pacing.** Bevy's `PipelinedRenderingPlugin` runs the render
  sub-app on a dedicated thread one frame behind; sampling its output
  the same frame would require a GPU fence the plugin does not expose.

## Decision Drivers

* Zero GPU copies: Bevy's tonemapped output lands directly in a
  host-owned `wgpu::Texture` the v0.14.0 `WgpuHost` composite pipeline
  samples.
* The adapter must not destabilize core crates — version pinning and
  publishability concerns stay inside `martensite-bevy`.
* Deterministic per-frame output: the host drives Bevy synchronously so
  a published frame is complete before it is composited.

## Considered Options

* **Option 1**: Wait for Bevy 0.20 (released wgpu-30 support).
* **Option 2**: Vendor a minimal Bevy 0.19 fork with a wgpu-30 backport.
* **Option 3**: **Git-pin Bevy `main` at the wgpu-30 merge commit**
  (`rev = "5036d97"`) inside the adapter only.

## Decision Outcome

Chosen option: **Option 3**.

`martensite-bevy` builds a headless `bevy::app::App` — `DefaultPlugins`
minus `WinitPlugin`, `WindowPlugin { primary_window: None }` — and
injects the host's GPU objects:

```rust
RenderPlugin {
    render_creation: RenderCreation::manual(RenderResources(
        RenderDevice::from(device),
        RenderQueue(Arc::new(WgpuWrapper::new(queue))),
        RenderAdapterInfo(WgpuWrapper::new(adapter.get_info())),
        RenderAdapter(Arc::new(WgpuWrapper::new(adapter))),
        RenderInstance(Arc::new(WgpuWrapper::new(instance))),
    )),
    synchronous_pipeline_compilation: true,
    ..default()
}
```

Each `ExternalEngine` viewport owns a Martensite-created
`wgpu::Texture`; its `TextureView` is registered in
`ManualTextureViews` and the `Camera3d` targets
`RenderTarget::TextureView(ManualTextureViewHandle)`. Per host frame:
`sub_apps.update()` then `RenderDevice::poll(PollType::Wait)`, so the
published texture is complete before `notify_frame_ready`.
`PipelinedRenderingPlugin` is omitted — its one-frame-behind render
thread would need a GPU fence before the host may sample the output.

### Positive Consequences

* True zero-copy: the composite pipeline samples the texture Bevy
  wrote — no `write_texture`, no readback.
* Bevy's full render feature set (PBR, tonemapping, bloom) resolves
  into the host-owned target unchanged.
* A Bevy version bump is a `martensite-bevy` concern; core crates never
  see the `bevy` dependency.

### Negative Consequences

* `martensite-bevy` is `publish = false`: crates.io forbids git
  dependencies, so the adapter cannot be published while it carries the
  git pin. It is also excluded from the default workspace build and
  checked by the dedicated `adapters` CI job.
* The pin tracks a pre-release Bevy `main` commit — API drift risk on
  every bump until 0.20 stabilizes.
* No pipelined rendering: `sub_apps.update()` runs on the host's frame
  critical path, so Bevy frame cost is additive with UI render cost.
