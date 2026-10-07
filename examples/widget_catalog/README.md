# Martensite Widget Catalog

Interactive per-widget showcase and developer reference: a searchable
rail, a live stage with theme/locale/direction/zoom controls, bespoke
per-page props, an event log, and dev-channel control.

The same `CatalogView` drives two targets:

- **Desktop** — a native winit window rendered by the
  `RenderOrchestrator` (Vello, TinySkia fallback), the platform
  AccessKit adapter, and the ADR-0038 dev-channel socket.
- **Web (`wasm32-unknown-unknown`)** — a browser canvas driven through
  `martensite-web`: canvas binding, the WebGPU → WebGL2 → CPU-raster
  backend probe, fetched fonts, the `WebA11yBridge` DOM/ARIA mirror,
  and the optional ADR-0042 web dev channel (`web-dev` feature).

## Desktop

```sh
cargo run -p widget_catalog
```

Developer tooling:

```sh
# Dev channel socket for `cargo martensite mcp` / the dev coordinator.
MARTENSITE_DEV_CHANNEL=1 cargo run -p widget_catalog

# Full widget tree + dev channel, no window or GPU (probe/CI path).
MARTENSITE_DEV_CHANNEL=1 ./target/debug/widget_catalog --live-headless

# Design-lint audit gate (CI).
./target/debug/widget_catalog --audit-gate
```

`MARTENSITE_HEADLESS_SCALE` / `MARTENSITE_HEADLESS_SIZE` (`WxH`) control
the headless viewport; `MARTENSITE_CPU=1` forces the TinySkia raster
path in windowed mode.

## Web (wasm32)

Prereqs: the `wasm32-unknown-unknown` target and `wasm-bindgen-cli`
matching the lockfile (`cargo install wasm-bindgen-cli --version
0.2.128 --locked`).

```sh
# Build the cdylib. `--lib` is deliberate: the package also ships a
# same-named native bin and cargo emits an output-filename-collision
# warning (and lets the bin overwrite the artifact) when both are built
# for wasm32 in one invocation.
cargo build -p widget_catalog --target wasm32-unknown-unknown --release --lib

# Generate the ES-module glue into web/pkg/ (gitignored).
mkdir -p examples/widget_catalog/web/pkg
wasm-bindgen --target web \
  --out-dir examples/widget_catalog/web/pkg \
  target/wasm32-unknown-unknown/release/widget_catalog.wasm

# Serve the static site — the whole web/ directory.
python3 -m http.server --directory examples/widget_catalog/web 8080
# → http://127.0.0.1:8080/
```

Trunk is intentionally not required: the static wasm-bindgen flow
exercises the exact same boundary.

What happens on load: `#[wasm_bindgen(start)]` binds
`#martensite-canvas`, sizes its backing store by `devicePixelRatio`,
probes `navigator.gpu` (WebGPU → Vello; WebGL2/no adapter → TinySkia
CPU raster — the console logs the decision), fetches
`web/assets/fonts/font.ttf` (the web has no system fonts), and drives
the real catalog — rail, pages, stage, overlays — with pointer,
scroll, keyboard, and IME input.

### Accessibility

The catalog's real `AccessKitAdapter` tree feeds a `WebA11yBridge`
mirror. The bridge is off by default: a visually hidden *Enable
accessibility* button (first tab stop) turns it on for AT users, and
`martensite_catalog_enable_a11y()` — a wasm-bindgen export — enables it
programmatically for tooling.

### Web dev channel (`web-dev`, ADR-0042)

An opt-in leg that dials `cargo martensite dev-web`'s local relay over
`ws://127.0.0.1:8788` and serves the dev-session protocol
(tree/lint/signals/events/a11y/logs) to connected tooling — the same
`DevSession` the native socket exposes. Off by default; a production
build contains none of the transport.

```sh
cargo build -p widget_catalog --target wasm32-unknown-unknown --release --lib \
  --features web-dev
# regenerate web/pkg, serve, then:
cargo martensite dev-web            # prints a per-run token
# open http://127.0.0.1:8080/?dev_token=<token>
```

Without `?dev_token=` the page never dials. F12 toggles the diagnostic
HUD/inspector overlay in `web-dev` builds.

### Compile check only

```sh
cargo check -p widget_catalog --target wasm32-unknown-unknown
cargo check -p widget_catalog --target wasm32-unknown-unknown --features web-dev
```

### Headless-browser gate

`tests/browser_gate.rs` — `#[ignore]`-gated — builds the wasm lib,
regenerates the glue, statically serves `web/` with `python3
http.server`, and drives headless Chromium via a pinned playwright.
It asserts the wasm start log, the GPU-backend decision, the font
fetch, the a11y mirror container *and* its projected role tree after
programmatic enablement, and clean interaction (no `pageerror` on a
synthetic click).

```sh
MARTENSITE_WEB_BROWSER=1 cargo test -p widget_catalog --test browser_gate -- --ignored
```

Prereqs: `node` + `npm`, `python3`, `wasm-bindgen` CLI, and the
`wasm32-unknown-unknown` target.
