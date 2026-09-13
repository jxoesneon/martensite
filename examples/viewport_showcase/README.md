# viewport_showcase

The v0.15.0 engine-adapter verification app: one Martensite window with
**two real external engines** composited side by side, ordinary Martensite
UI around both, and window input forwarded into whichever engine panel
it lands on.

```text
┌─ Martensite — Engine Showcase (v0.15.0) ───────────── winit chrome ─┐
│ header strip (ordinary PaintList content)                           │
│ ┌─ Bevy viewport ────────┐  ┌─ Godot viewport ───────────────────┐  │
│ │ PaintCommand::External │  │ placeholder fill + status text OR  │  │
│ │ → bridge ring front    │  │ PaintCommand::External once frames │  │
│ │   frame (wgpu texture) │  │ arrive over TCP                    │  │
│ └────────────────────────┘  └────────────────────────────────────┘  │
│ footer strip (painted OVER both External markers — z-order proof)    │
└──────────────────────────────────────────────────────────────────────┘
```

## What it demonstrates

- **`martensite-bevy` (left panel)** — `BevyEngine` runs a headless Bevy
  `App` on its own render thread. `RenderCreation::Manual` injects the
  host's `wgpu` device/queue, so every published frame is a same-device
  texture composited **zero-copy** by `RenderOrchestrator`. The scene
  (spawned via `BevyEngine::with_app`) is a metallic PBR cube tumbled by
  an `Update` system, lit by a `PointLight`, viewed by the adapter's
  fixed `Camera3d` (at `(0,0,5)`, targeting `CAMERA_VIEW_HANDLE`).
- **`martensite-godot` (right panel)** — `GodotEngine::new_tcp` binds a
  loopback listener on `127.0.0.1:7890` and drains `FrameMsg`s into the
  bridge ring on a background thread. Until a Godot editor connects, the
  panel shows an honest placeholder (a `PaintList` fill + status text
  under the `External` marker — nothing is composited because the ring
  has no front frame). When frames do arrive they are Godot's
  `texture_get_data_async` readback pixels uploaded via
  `queue.write_texture`: **two GPU boundary crossings per frame**, the
  documented Tier-1 cost — this panel is intentionally *not* zero-copy.
- **Ordinary UI + z-order** — header, captions, borders, and a footer
  are plain `PaintList` commands placed under, between, and over the two
  `PaintCommand::External` markers; the segmented dispatch reproduces
  that exact paint order.
- **Input forwarding** — `CursorMoved`/`MouseInput`/`MouseWheel`
  hit-test the laid-out panel rects and forward
  `EngineEvent::{PointerMove, PointerButton, Scroll}` with
  surface-local coordinates (panel origin subtracted).
  `KeyboardInput` produces `EngineEvent::Key`/`TextInput` routed to the
  panel that last received a pointer press; `WindowEvent::Focused`
  maps to `EngineEvent::Focus`.

## Run

This example is `exclude`d from the workspace (the Bevy git pin and the
`godot` crate would otherwise constrain the shared lockfile), so build it
by manifest path:

```sh
cargo run --manifest-path examples/viewport_showcase/Cargo.toml
```

Requires a GPU adapter. The `martensite-wgpu/vello` feature is enabled so
paint segments dispatch through Vello; on device loss the
`RecoveryMachine` falls back to the TinySkia/`CpuFrame` path.

## The Godot half

The example works with no Godot running — the right panel keeps the
placeholder. To stream real frames:

1. Build the GDExtension cdylib:

   ```sh
   cargo build --manifest-path crates/martensite-godot/Cargo.toml
   ```

   (`martensite-godot` is also workspace-excluded; its `[lib]` produces
   both `cdylib` — the extension — and `rlib` — the host side linked
   here.)

2. In a Godot 4.2+ project, register the cdylib via a `.gdextension`
   file, add a `MartensiteViewport` node under the `SubViewport` you want
   to stream, and point its transport at `127.0.0.1:7890`
   (`TcpTransport::connect`). Launch the editor — the listener re-accepts
   a fresh connection if the editor restarts.

## Honest limitations (not bugs)

- `PaintCommand::DrawText` currently renders as per-character block
  rectangles in the Vello backend — full glyph shaping needs the text
  pipeline (`DrawGlyphRun` + `FontResource`). The captions and the Godot
  placeholder use it anyway; that approximation is what the backend
  documents.
- `EngineEvent::Key` receives the `KeyCode` discriminant for identified
  keys rather than a true platform scancode (winit 0.31 no longer
  exposes one on `KeyEvent`). It is still a stable per-key value, so
  press/release pairing works.
- The Godot path is Tier-1 readback+upload by design; the crate's
  `godot-gpu-copy` feature (Tier 2, one GPU→GPU copy) is experimental
  and not exercised here.

Part of the Martensite GUI framework. Internal/non-publishable example.
