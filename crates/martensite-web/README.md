# martensite-web

Consolidated `wasm32-unknown-unknown` entry point for Martensite.

Martensite's web backends ship one module per crate —
`martensite-window::web` (canvas binding, rAF event loop, DPI, IME),
`martensite-wgpu::web` (WebGPU / WebGL2 / CPU-raster probe),
`martensite-text::web` (fetch-loaded fonts), plus the
`martensite-clipboard`, `martensite-dnd`, and `martensite-access`
web bridges. This crate re-exports them behind one umbrella
(`martensite_web::{window, gpu, text, clipboard, dnd, access}`) and
adds three bootstrap helpers:

- `canvas_element(id)` — typed `<canvas>` lookup from the DOM.
- `prepare_canvas(&canvas, w, h)` — one DPI-correct backing-store sync
  (physical attributes = CSS × `devicePixelRatio`, CSS size pinned).
- `run(app)` — panic hook + web poll/rAF event-loop configuration +
  `spawn_app`.

The crate body is entirely `#![cfg(all(target_arch = "wasm32",
target_os = "unknown"))]`: on host targets it compiles to an empty
library, so the workspace build and doctests are unaffected.

```toml
[target.'cfg(all(target_arch = "wasm32", target_os = "unknown"))'.dependencies]
martensite-web = { workspace = true }
```

See `examples/web` for the full smoke-test application (GPU probe,
clipboard, drag-and-drop, IME, a11y mirror).
