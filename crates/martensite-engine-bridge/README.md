# martensite-engine-bridge

Producer/consumer protocol for embedding external GPU renderers —
a Bevy scene, a hardware video decoder, an offscreen web compositor —
as zero-copy surfaces inside the [Martensite](https://github.com/jxoesneon/martensite)
widget tree.

Martensite remains the application host: it owns the winit event loop,
the `wgpu::Device`, the window, and the compositing pass. External
renderers implement the `Engine` trait and publish `Frame`s into a
two-slot mailbox ring (`SurfaceRing`) shared through
`BridgeRegistry`/`BridgeHandle`; Martensite's `WgpuHost` samples the
published `wgpu::Texture` directly in its composite pass — no
`write_texture`, no CPU readback, no atlas copies.

## Frame lifecycle

```text
producer thread                    host (paint) thread
───────────────                    ─────────────────────
ring.acquire()      →  slot Writing
render into texture
ring.mark_ready()   →  slot Ready, ready_events.push(surface)
                       widget drains ready events → request_redraw
                       ring.take_front() → slot Compositing
                       WgpuHost::composite_front() samples the texture
                       ring.release() → slot Free  (after queue.submit)
ring.drain_released() → Engine::release → producer recycles the slot
```

Same-queue submission ordering provides the synchronization for
`FrameSync::None` frames; cross-device `FrameSync` variants declare the
transport metadata consumed by later milestones.

This crate contains zero `unsafe` code (`#![forbid(unsafe_code)]`).

## License

MIT OR Apache-2.0
