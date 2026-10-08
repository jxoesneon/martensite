//! Web gate — headless-browser check for the `wasm32-unknown-unknown`
//! widget-catalog showcase.
//!
//! `#[ignore]`-gated per the repo convention for hardware/environment-
//! dependent gates (cf. `MARTENSITE_WEB_BROWSER` in
//! `examples/web/tests/browser_gate.rs`). The gate runs only when
//! `MARTENSITE_WEB_BROWSER=1` is set in the environment — it is a local
//! /self-hosted-runner check. CI compiles the wasm boundary separately
//! via the `target-checks` wasm32 job; it does not drive a browser.
//!
//! # Prerequisites (self-hosted runner / local run)
//!
//! * `cargo` with the `wasm32-unknown-unknown` target installed.
//! * `wasm-bindgen` CLI matching the lockfile's wasm-bindgen
//!   (`cargo install wasm-bindgen-cli --version 0.2.128 --locked`).
//! * `python3` — static file server for `examples/widget_catalog/web`.
//! * `node` + `npm` — used to install a pinned `playwright` into a temp
//!   dir and drive Chromium (a system Chromium/Chrome on PATH is used
//!   directly when present).
//!
//! # What it verifies
//!
//! The page loads, the wasm entry point logs `martensite widget
//! catalog starting`, a GPU backend decision is logged (`web backend
//! selected: …` or the CPU-raster fallback), the web font fetch lands,
//! the hidden a11y mirror container (`[data-martensite-a11y-mirror]`)
//! is in the DOM, and — after programmatic enablement through the
//! `martensite_catalog_enable_a11y` wasm export — the mirror holds a
//! real projected role tree (proving the catalog's `AccessKitAdapter`
//! fed `WebA11yBridge`, not a placeholder). A synthetic pointer click
//! on the canvas is also dispatched: the app must still be logging no
//! `pageerror` afterwards.
//!
//! The page runs with a test-injected stylesheet that stretches the
//! canvas's layout box to 2200×1300, so the device-pixel content box
//! reported to winit exceeds the WebGL2 `max_texture_dimension_2d`
//! floor of 2048 — the regression where `Surface::configure` failed
//! validation and the subsequent `get_current_texture` panicked.
//! (Headless SwiftShader does not emulate `devicePixelContentBoxSize`
//! through `deviceScaleFactor`, so the oversize comes from CSS, not
//! DPR.) The gate asserts the console shows no `maximum supported
//! texture size` validation error and no panic markers, and that the
//! pixel assertion still passes (a clamped backing store upscaled by
//! the browser paints identically at the compositor).
//!
//! # Manual equivalent
//!
//! Build + serve per README "Web (wasm32)", open the page in a
//! browser, click around — the gate automates exactly that.

use std::io::Write as _;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The web gate: wasm-bindgen-built catalog + headless-browser check.
///
/// Skipped (not failed) unless `MARTENSITE_WEB_BROWSER` is set — the
/// same skip-if-unset convention as the `examples/web` gate.
#[test]
#[ignore = "requires static wasm build + node/npm + python3 — set MARTENSITE_WEB_BROWSER=1"]
fn web_headless_browser_gate() {
    if std::env::var_os("MARTENSITE_WEB_BROWSER").is_none() {
        eprintln!("MARTENSITE_WEB_BROWSER not set; gate skipped");
        return;
    }

    let example_dir = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
    let workspace_dir = example_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("examples/widget_catalog has a workspace grandparent")
        .to_path_buf();
    for (tool, args) in [
        ("node", ["--version"].as_slice()),
        ("npm", ["--version"].as_slice()),
        ("python3", ["--version"].as_slice()),
        ("wasm-bindgen", ["--version"].as_slice()),
    ] {
        let ok = Command::new(tool)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(
            ok,
            "web browser gate: `{tool}` not found on PATH — install it \
             (cargo install wasm-bindgen-cli; node + npm + python3) \
             or unset MARTENSITE_WEB_BROWSER"
        );
    }

    // Build the wasm cdylib (--lib keeps the bin's artifact out of the
    // filename collision), regenerate the JS glue, and statically
    // serve examples/widget_catalog/web — the same flow the README's
    // Option B documents.
    let build = Command::new("cargo")
        .args([
            "build",
            "-p",
            "widget_catalog",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
            "--lib",
        ])
        .current_dir(&workspace_dir)
        .status()
        .expect("spawn cargo build");
    assert!(
        build.success(),
        "web browser gate: wasm release build failed"
    );

    let pkg_dir = example_dir.join("web").join("pkg");
    std::fs::create_dir_all(&pkg_dir).expect("create web/pkg");
    let glue = Command::new("wasm-bindgen")
        .args([
            "--target",
            "web",
            "--out-dir",
            pkg_dir.to_str().expect("utf8 path"),
            workspace_dir
                .join("target/wasm32-unknown-unknown/release/widget_catalog.wasm")
                .to_str()
                .expect("utf8 path"),
        ])
        .status()
        .expect("spawn wasm-bindgen");
    assert!(glue.success(), "web browser gate: wasm-bindgen failed");

    // Pick a free port, then release it for the static server.
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port();
    let url = format!("http://127.0.0.1:{port}/index.html");

    let mut server = Command::new("python3")
        .args([
            "-m",
            "http.server",
            &port.to_string(),
            "--bind",
            "127.0.0.1",
            "--directory",
            example_dir.join("web").to_str().expect("utf8 path"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn python3 http.server");
    let _server_guard = ChildGuard(&mut server);

    wait_for_port(port, Duration::from_secs(30))
        .unwrap_or_else(|| panic!("python3 http.server did not open port {port}"));

    let temp = std::env::temp_dir().join(format!("martensite-web-gate-{}", std::process::id()));
    std::fs::create_dir_all(&temp).expect("create gate temp dir");
    install_playwright(&temp);
    let script = write_check_script(&temp);

    let output = Command::new("node")
        .arg(&script)
        .arg(&url)
        .current_dir(&temp)
        .output()
        .expect("run playwright check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = std::fs::remove_dir_all(&temp);
    assert!(
        output.status.success(),
        "web browser gate FAILED\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    eprintln!("{stdout}");
}

/// Kills the spawned server when the gate ends (pass or panic).
struct ChildGuard<'a>(&'a mut Child);

impl Drop for ChildGuard<'_> {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Polls until the static server accepts connections or the deadline
/// passes.
fn wait_for_port(port: u16, timeout: Duration) -> Option<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Some(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    None
}

/// Installs a pinned playwright package into `dir` so the check script
/// resolves `playwright` from `dir/node_modules`. Pinned per the repo's
/// no-floating-versions convention.
fn install_playwright(dir: &Path) {
    let status = Command::new("npm")
        .args(["install", "--no-save", "--prefix"])
        .arg(dir)
        .arg("playwright@1.55.0")
        .status()
        .expect("spawn npm install");
    assert!(
        status.success(),
        "web browser gate: `npm install playwright@1.55.0` failed"
    );
}

/// Writes the playwright check script into `dir` (next to its
/// `node_modules`, so the bare `playwright` import resolves) and returns
/// its path.
fn write_check_script(dir: &Path) -> PathBuf {
    const SCRIPT: &str = r#"import { chromium } from 'playwright';
import zlib from 'node:zlib';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// Minimal PNG decoder for playwright's 8-bit RGB/RGBA screenshots —
// enough to count distinct pixel byte values for the canvas-paint
// assertion below.
function pngPixels(buf) {
  const sig = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
  if (!buf.subarray(0, 8).equals(sig)) throw new Error('not a png');
  let off = 8;
  let width = 0, height = 0, bitDepth = 0, colorType = 0, interlace = 0;
  const idat = [];
  while (off < buf.length) {
    const len = buf.readUInt32BE(off);
    const type = buf.toString('ascii', off + 4, off + 8);
    const data = buf.subarray(off + 8, off + 8 + len);
    if (type === 'IHDR') {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      bitDepth = data[8];
      colorType = data[9];
      interlace = data[12];
    } else if (type === 'IDAT') {
      idat.push(data);
    } else if (type === 'IEND') {
      break;
    }
    off += 12 + len;
  }
  const channels = colorType === 6 ? 4 : colorType === 2 ? 3 : 0;
  if (bitDepth !== 8 || channels === 0 || interlace !== 0) {
    throw new Error(`unsupported png: depth=${bitDepth} type=${colorType} interlace=${interlace}`);
  }
  const raw = zlib.inflateSync(Buffer.concat(idat));
  const stride = width * channels;
  const out = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const row = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const prev = y ? out.subarray((y - 1) * stride, y * stride) : null;
    const cur = out.subarray(y * stride, (y + 1) * stride);
    for (let x = 0; x < stride; x++) {
      const a = x >= channels ? cur[x - channels] : 0;
      const b = prev ? prev[x] : 0;
      const c = x >= channels && prev ? prev[x - channels] : 0;
      let v = row[x];
      if (filter === 1) v += a;
      else if (filter === 2) v += b;
      else if (filter === 3) v += (a + b) >> 1;
      else if (filter === 4) {
        const p = a + b - c;
        const pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      cur[x] = v & 0xff;
    }
  }
  return out;
}

const url = process.argv[2];
const logs = [];
// Prefer a system Chromium/Chrome when present — `npm install
// playwright` then does not need to download its own browser build.
const candidates = [
  process.env.CHROMIUM_PATH,
  '/usr/bin/chromium',
  '/usr/bin/chromium-browser',
  '/usr/bin/google-chrome',
].filter((p) => p && fs.existsSync(p));
const launchArgs = {
  headless: true,
  // Headless Chromium does not raster canvases with default flags on
  // this runner; SwiftShader restores software rasterization so the
  // canvas-pixel assertion is meaningful. `--enable-unsafe-webgpu` is
  // deliberately absent: with it the probe selects a WebGPU adapter,
  // but the headless GPU process then destroys the device seconds
  // later ("A valid external Instance reference no longer exists"),
  // leaving a permanently blank canvas — observed at the raw JS level
  // (`device.lost` resolves "destroyed" even with retained handles).
  // Without the flag the adapter probe falls through to the WebGL2/
  // TinySkia path, which is the only backend that can present here.
  args: [
    '--no-sandbox',
    // `--use-angle=swiftshader` is the current spelling of the old
    // `--use-gl=swiftshader` (retired upstream — on Chromium ≥ ~120 the
    // GL backend picker rejects it, the GPU process exits, and the
    // browser never signals readiness on the debugging pipe).
    '--use-angle=swiftshader',
    '--enable-unsafe-swiftshader',
  ],
  ...(candidates.length ? { executablePath: candidates[0] } : {}),
};
const browser = await chromium.launch(launchArgs);
var mirror;
var roleCount = 0;
var pageErrors = [];
var distinctBytes = -1;
try {
  // Force the canvas's layout box past the WebGL2 texture-extent
  // floor (2048) — the deployed-site regression this gate covers. The
  // stylesheet pins `#martensite-canvas` to 1280×860 with
  // `max-width/max-height: 100v*`, and headless SwiftShader does not
  // emulate `devicePixelContentBoxSize` through `deviceScaleFactor`,
  // so the oversize has to come from CSS itself. `addInitScript` runs
  // at document-start, before the wasm entry point and winit's
  // ResizeObserver first tick. With the clamp in place the surface
  // configures at or below the device limit and the browser upscales
  // the backing store.
  const page = await browser.newPage({ viewport: { width: 2400, height: 1500 } });
  await page.addInitScript(() => {
    const inject = () => {
      const style = document.createElement('style');
      style.textContent =
        '#martensite-canvas { width: 2200px !important; height: 1300px !important; ' +
        'max-width: none !important; max-height: none !important; }';
      (document.head || document.documentElement).appendChild(style);
    };
    // `addInitScript` evaluates at document-start, when
    // `documentElement` may not exist yet; deferring to
    // DOMContentLoaded still lands the rule long before the wasm
    // module has fetched/instantiated and winit's ResizeObserver
    // reads the box.
    if (document.documentElement) inject();
    else document.addEventListener('DOMContentLoaded', inject, { once: true });
  });
  page.on('console', (m) => logs.push(m.text()));
  page.on('pageerror', (e) => {
    logs.push(`pageerror: ${e}`);
    pageErrors.push(String(e));
  });
  await page.goto(url, { waitUntil: 'load' });
  // The mirror container is appended by `init_web_a11y` the moment the
  // async init legs (GPU probe + font fetch) have both landed, so its
  // presence is the DOM-visible readiness signal — a fixed sleep is
  // flaky under load because the probe's adapter request can take
  // seconds under SwiftShader.
  // `attached`, not the default `visible`: the mirror container is a
  // 0x0 positioned box (the mirror projects bounds onto its children,
  // which are themselves `opacity:0`) so it never satisfies
  // Playwright's visibility check.
  mirror = await page.waitForSelector('[data-martensite-a11y-mirror]', {
    state: 'attached',
    timeout: 30000,
  }).catch(() => null);
  // A few rAF frames so the first present has a chance to land.
  await page.waitForTimeout(2000);

  // Programmatic a11y enablement: the production path is the hidden
  // "Enable accessibility" button (it insists on a trusted click);
  // the wasm export exists for exactly this gate.
  await page.evaluate(async () => {
    const mod = await import('./pkg/widget_catalog.js');
    mod.martensite_catalog_enable_a11y();
  });
  // The enable request is consumed on the next frame; the pending
  // tree materializes then — the projected role nodes appear inside
  // the already-appended container.
  await page.waitForSelector('[data-martensite-a11y-mirror] [role]', {
    state: 'attached',
    timeout: 10000,
  }).catch(() => null);
  if (mirror) {
    roleCount = await mirror.$$eval('[role]', (els) => els.length);
  }

  // Interaction smoke: click the rail area of the canvas — a real
  // pointer event must not produce a pageerror.
  const canvas = await page.$('#martensite-canvas');
  if (canvas) {
    const box = await canvas.boundingBox();
    if (box) {
      await page.mouse.click(box.x + 140, box.y + 200);
      await page.waitForTimeout(500);
      // Pixel assertion: the canvas must have actually painted.
      // WebGPU/WebGL canvases read back blank through `toDataURL`
      // (no preserveDrawingBuffer), so the compositor screenshot is
      // the reliable readback — a real widget scene (theme surface,
      // rail, cards, text) decodes to many distinct byte values
      // while a blank/unpainted canvas is a single flat color.
      const shot = await page.screenshot({
        type: 'png',
        // SwiftShader rasterizes a >2048-px canvas on the CPU — the
        // compositor needs a generous capture budget here.
        timeout: 120000,
        clip: {
          x: Math.floor(box.x),
          y: Math.floor(box.y),
          width: Math.floor(Math.min(box.width, 800)),
          height: Math.floor(Math.min(box.height, 500)),
        },
      });
      const bytes = new Set(pngPixels(shot));
      distinctBytes = bytes.size;
    }
  }
} finally {
  await browser.close();
}

const joined = logs.join('\n');
const failures = [];
if (!joined.includes('martensite widget catalog starting')) {
  failures.push('missing wasm start log');
}
if (!/web backend selected:|no GPU adapter/.test(joined)) {
  failures.push('no GPU backend decision logged');
}
if (!joined.includes('font loaded: assets/fonts/font.ttf')) {
  failures.push('web font fetch did not land');
}
if (!mirror) {
  failures.push('a11y mirror element missing');
}
if (roleCount < 10) {
  failures.push(`a11y mirror holds ${roleCount} projected roles — expected the real catalog tree`);
}
if (distinctBytes < 8) {
  failures.push(`canvas screenshot decoded to ${distinctBytes} distinct byte values — expected painted widgets, got a blank/uniform canvas`);
}
// The oversize-viewport regression: a `Surface::configure` extent past
// the device's `max_texture_dimension_2d` logged this validation error
// and the follow-on `get_current_texture` panicked (cascading into
// winit-web's `RefCell already borrowed`). All three markers must be
// absent — `pageerror` coverage alone misses the wgpu error because it
// arrives via `console.error`, not an exception.
const forbidden = [
  'maximum supported texture size',
  'not configured for presentation',
  'RefCell already borrowed',
];
for (const marker of forbidden) {
  if (joined.includes(marker)) {
    failures.push(`console contains ${marker}`);
  }
}
if (pageErrors.length) {
  failures.push(`page errors: ${pageErrors.join(' | ')}`);
}
if (failures.length) {
  console.error(`BROWSER GATE FAIL:\n - ${failures.join('\n - ')}\nconsole:\n${joined}`);
  process.exit(1);
}
console.log('BROWSER GATE PASS');
"#;
    let path = dir.join("browser_check.mjs");
    let mut file = std::fs::File::create(&path).expect("create check script");
    file.write_all(SCRIPT.as_bytes())
        .expect("write check script");
    path
}
