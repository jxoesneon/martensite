# martensite-godot

Godot 4 **GDExtension** adapter that streams `SubViewport` frames into a
Martensite host, for the v0.15.0 engine-showcase milestone.

The crate builds two ways from one source tree:

- **`cdylib`** — the `libmartensite_godot` extension Godot loads
  (`#[gdextension]` entry point via `godot`/gdext 0.5.x, Godot 4.2+ API
  levels).
- **`rlib`** — linked by the Martensite host application for the
  receiving side (`host::GodotEngine`, a `martensite-engine-bridge`
  `Engine`). The host side is pure Rust + wgpu and never calls into the
  Godot runtime; `transport`/`host` compile and run without Godot
  present.

## The honesty contract — read this first

**Godot does not permit zero-copy frame sharing.** Research for this
milestone established, concretely:

- `RenderingDevice::texture_get_native_handle` /
  `get_driver_resource(DRIVER_RESOURCE_TEXTURE)` return API object
  pointers (`VkImage`, `ID3D12Resource*`, `id<MTLTexture>`) — **not**
  cross-process shareable handles.
- Godot's internal render targets are **not** allocated with
  `D3D12_HEAP_FLAG_SHARED` / `VkExportMemoryAllocateInfo` / IOSurface
  backing, and a GDExtension cannot retroactively add export flags.
- `fence_create`/semaphores/cross-API sync are **not** exposed to
  GDExtension; the only public completion primitive is the CPU-side
  `texture_get_data_async` callback.
- `libgodot` (merged to Godot master) puts Godot in-process but does
  **not** solve logical-device sharing.

Accordingly this crate ships two tiers, and **neither is zero-copy**.
The phrase "zero-copy" is intentionally never used for the Godot path
except in this paragraph explaining why it is unavailable.

### Tier 1 — readback path (default, shippable)

```text
SubViewport → ViewportTexture → RenderingServer::texture_get_rd_texture
→ RenderingDevice::texture_get_data_async → PackedByteArray
→ transport (TCP / Unix socket / in-process channel)
→ GodotEngine::render → queue.write_texture → bridge ring → composite
```

Works on every Godot driver (Forward+/Mobile on Vulkan, D3D12, Metal)
and every platform. Cost per frame, stated plainly:

| Step | Copy |
|------|------|
| `texture_get_data_async` | GPU → CPU readback (driver staging buffer) |
| transport `send` | CPU → CPU (socket write or channel clone) |
| `queue.write_texture` | CPU → GPU upload into the slot texture |
| `CpuFrame` bookkeeping | +1 CPU clone for `to_pixmap`/TinySkia fallback |

Throughput and latency are **measured and published**, not hidden:
`MartensiteViewport.stats()` reports `sent`, `dropped`, `errors`,
`in_flight`, and `avg_readback_us` (mean request→callback latency). The
readback is throttled to `max_in_flight` (default 2) outstanding
requests; Godot's callback delay equals the driver's
`frame_queue_size`, so a small bound pipelines well.

Readbacks are pumped either every `_process` (`auto_readback`, default)
or after each drawn frame via `connect_post_draw()`
(`RenderingServer.frame_post_draw`).

### Tier 2 — shared-texture blit (`godot-gpu-copy`, experimental)

```text
host allocates shareable texture (HEAP_FLAG_SHARED / exportable
VkDeviceMemory / IOSurface) → extension imports it via
texture_create_from_extension → CompositorEffect POST_TRANSPARENT
texture_copy(scene color → shared texture)
```

**One GPU→GPU copy per frame — still not zero-copy.** Platform-unsafe
(opaque native image handle), requires the `godot` crate's
`experimental-godot-api` feature (the `CompositorEffect` class is an
experimental API surface, enabled transitively by `godot-gpu-copy`), and
synchronization is best-effort with one-frame-delay tolerance until
Godot exposes fence APIs.

### What true zero-copy would take (upstream Godot changes)

Out of scope for this crate — documented as upstream-contribution
opportunities:

1. Internal render targets allocated with export flags
   (`D3D12_HEAP_FLAG_SHARED`, `VkExportMemoryAllocateInfo`, IOSurface).
2. External-texture render targets (render into an imported image, not
   just blit into it).
3. A public fence/semaphore API for GDExtension (cross-queue sync).
4. `libgodot` native-window embedding surface (merged hook, still needs
   logical-device sharing to matter).

## Usage

Godot side (scene tree):

```text
MartensiteViewport          # this extension's Node
SubViewport                 # renders the content to stream
```

```gdscript
$MartensiteViewport.transport_addr = "tcp://127.0.0.1:9177"
# or "unix:///tmp/martensite-godot.sock", or "channel://myvp" for
# in-process libgodot embedding.
$MartensiteViewport.set_subviewport("../SubViewport")
```

Host side (Martensite app):

```rust
use martensite_engine_bridge::BridgeHandle;
use martensite_godot::host::GodotEngine;

let handle = BridgeHandle::new();
let surface = handle.lock().register();
let engine = GodotEngine::new_tcp(handle, surface, "127.0.0.1:9177")?;
// register `engine` with the ExternalEngine widget like any other Engine.
```

Transport addresses: `tcp://host:port` (portable, works on Windows),
`unix:///path.sock` (Unix only), `channel://name` (in-process; the host
retrieves the receiver via
`martensite_godot::transport::take_channel_receiver("name")`).

### Y-flip

Godot renders viewports top-left-origin. `MartensiteViewport.flip_y`
(default `false`) swaps rows before transport so a bottom-left-origin
consumer can opt in; the host always treats row 0 as top.

## Features

| Feature | Effect |
|---------|--------|
| *(default)* | Tier-1 readback path only. |
| `godot-gpu-copy` | Tier-2 `CompositorEffect` blit; enables `godot/experimental-godot-api`. |
| `test-noop` | Enables wgpu's `noop` backend for GPU-free tests. |

## Unsafe code

This crate is the workspace's eighth `#![allow(unsafe_code)]` boundary —
the `#[gdextension]` entry point is FFI by nature. Every `unsafe` block
carries a `// SAFETY:` comment; all transport/host/readback logic is
safe Rust.
