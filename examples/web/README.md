# Martensite web smoke test (`wasm32-unknown-unknown`)

Compile-checked skeleton exercising every web platform backend added in
milestone v0.17.0 §4.3:

- `martensite-wgpu::web` — WebGPU probe → WebGL2 downlevel → TinySkia
  CPU-raster decision boundary (Vello is compute-only; there is no
  WebGL2 Vello path).
- `martensite-window::web` — canvas binding, `ControlFlow::Poll`/rAF
  event loop, manual backing-store DPI scaling, hidden-`<input>` IME.
- `martensite-text::web` — `fetch`-loaded fonts (no system fonts on
  wasm).
- `martensite-clipboard::web` — async `navigator.clipboard` on click.
- `martensite-dnd::web` — HTML5 `DataTransfer` drop target.
- `martensite-access::web` — minimal hidden-DOM/ARIA bridge (live-region
  announcer + focusable-role DOM mirror), **not** a full AccessKit
  adapter.

## Build

### Option A: trunk

```sh
cargo install trunk            # once
rustup target add wasm32-unknown-unknown
cd examples/web
trunk serve                    # http://127.0.0.1:8080
```

### Option B: wasm-bindgen-cli

```sh
rustup target add wasm32-unknown-unknown
cargo build -p martensite-web-example --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir examples/web/pkg \
  ../../target/wasm32-unknown-unknown/release/martensite_web_example.wasm
# then serve examples/web statically and add an ES-module init script
# (trunk generates this automatically):
python3 -m http.server --directory examples/web 8080
```

### Compile check only

```sh
cargo check -p martensite-web-example --target wasm32-unknown-unknown
```

This is the compile boundary the wasm layer is verified against. CI
covers it via the `target-checks` wasm32 job (added on `main` at
milestone integration); until this branch lands there, run the check
above locally before touching the web backends.

### Headless-browser gate (spec §5)

`tests/browser_gate.rs` is the §5 Web gate artifact: a `#[ignore]`-gated
test that `trunk serve`s the example and drives headless Chromium via
playwright, asserting the wasm entry point starts, a GPU backend is
selected, the a11y mirror exists, and the `aria-live` announcement
lands. It runs only with `MARTENSITE_WEB_BROWSER=1` in the environment
(and `trunk`, `node`/`npm` installed) — the same opt-in convention as
`MARTENSITE_MEDIA_4K120`:

```sh
MARTENSITE_WEB_BROWSER=1 cargo test -p martensite-web-example \
  --test browser_gate -- --ignored --nocapture
```

CI does not run this; the manual checklist below is the no-tooling
equivalent.

## Smoke-test checklist (browser devtools console)

1. `web backend selected: WebGpu` on a WebGPU browser (Chrome/Edge,
   Firefox ≥ 141, Safari 26). On a WebGL2-only browser expect `WebGl2`
   followed by `Vello unavailable — TinySkia CPU raster path`; with no
   GPU at all, `no GPU adapter ... — TinySkia CPU raster only`.
2. An animated indigo→blue gradient clears the canvas every frame on
   the GPU paths — proof the surface presents.
3. Resize the window: the canvas stays sharp (backing store tracks
   `devicePixelRatio`).
4. Click the canvas: `clipboard write ok` (requires HTTPS/localhost —
   the user gesture is the click itself).
5. Drag a text file onto the canvas: `drop accepted` + `dropped file …`
   log lines.
6. Type with an IME (e.g. Japanese input): `ime: CompositionUpdate(…)`
   log lines.
7. Inspect the DOM: a visually-hidden
   `div[data-martensite-a11y-mirror]` contains `role="document"` and a
   `role="button"` mirror node; a sibling `aria-live` region holds the
   load announcement.
8. `font fetch (expected without assets/fonts/font.ttf): …` unless a
   font is placed at `assets/fonts/font.ttf` next to `index.html`.

## Known scope limits

- The a11y mirror is the committed minimal bridge — not a full DOM
  mirror (v1.0+ hardening).
- Drag *source* (outgoing HTML5 drags) is not implemented; in-app drags
  use `martensite-dnd`'s `DndSession` directly.
- COOP/COEP headers are not required (no SharedArrayBuffer); see the
  comment in `index.html`.
