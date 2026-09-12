# engine_embed

The v0.14.0 external-surface milestone, end to end: a `MockEngine`
producer renders deterministic frames into same-device `wgpu::Texture`s,
publishes them through the `BridgeRegistry` two-slot mailbox ring, and
the `ExternalEngine` widget's `PaintCommand::External` marker is
composited zero-copy by `RenderOrchestrator` between the surrounding
Vello paint segments — the full producer → ring → widget → paint →
composite loop inside a real winit window.

```text
drive_frame ──► MockEngine::render ──► acquire → render → mark_ready_full
                                                   │ ready waker
                                                   ▼
                           widget.poll_frame + window.request_redraw
                                                   │
record_paint ─► PaintCommand::External ─► orchestrator.render
                                                   ▼
             render_to_surface: take_front → composite_front →
             queue.submit → release → pre_present_notify → present
                                                   │
             engines.drain_released ◄──────────────┘
```

## Hooks exercised

- `ExternalEngines::drive_frame` — drives each bound producer for one
  frame; returns the surfaces that published.
- `set_ready_waker` → `window.request_redraw()` — a published frame
  schedules the repaint; producers stay event-driven, never polled in
  `about_to_wait`.
- `FramePoll` mapping — `poll_frame()` results drive `request_layout`
  on a size change and `request_redraw` on a same-size new frame.
- `set_pre_present_notify` — forwards to `Window::pre_present_notify()`
  before `queue.present()` (winit pacing contract).
- `set_cpu_frame_resolver` → `ExternalEngines::cpu_frame_for` — feeds
  `Engine::to_pixmap` to the TinySkia fallback path.
- `drain_released` — returns composited `FrameToken`s to the producers
  after `queue.submit` so ring slots recycle.

## Run

```sh
cargo run -p engine_embed
```

Requires a GPU adapter. The `martensite-wgpu/vello` feature is enabled
so paint segments dispatch through Vello; on device loss the
`RecoveryMachine` falls back to the TinySkia/`CpuFrame` path.

Part of the Martensite GUI framework. This is an internal/non-publishable workspace member.
