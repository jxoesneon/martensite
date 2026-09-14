//! §5 Web gate — headless-browser check for the `wasm32-unknown-unknown`
//! example.
//!
//! `#[ignore]`-gated per the repo convention for hardware/environment-
//! dependent gates (cf. `MARTENSITE_MEDIA_4K120` in
//! `martensite-media-test`). The gate runs only when
//! `MARTENSITE_WEB_BROWSER=1` is set in the environment — automated CI
//! does not run it; CI exercises the compile boundary
//! (`cargo check --target wasm32-unknown-unknown`).
//!
//! # Prerequisites (self-hosted runner / local run)
//!
//! * `trunk` (`cargo install trunk`) — builds and serves the example.
//! * `node` + `npm` — used to install a pinned `playwright` into a temp
//!   dir and drive headless Chromium (`npm install playwright` also
//!   fetches the browser binary on first run).
//! * `rustup target add wasm32-unknown-unknown`.
//!
//! # What it verifies
//!
//! The page loads, the wasm entry point logs `martensite web example
//! starting`, a GPU backend decision is logged (`web backend selected: …`
//! or the `no GPU adapter …` CPU-raster fallback), the hidden a11y mirror
//! container (`[data-martensite-a11y-mirror]`) is in the DOM, and the
//! `aria-live` announcer holds the load announcement.
//!
//! # Manual equivalent
//!
//! `cd examples/web && trunk serve`, open the page, and follow the
//! README's smoke-test checklist — the gate automates exactly that.

use std::io::Write as _;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The §5 gate: `trunk`-built example + headless-browser check.
///
/// Skipped (not failed) unless `MARTENSITE_WEB_BROWSER` is set — the same
/// skip-if-unset convention as the media 4K120 gate.
#[test]
#[ignore = "requires trunk + node/npm + playwright Chromium — set MARTENSITE_WEB_BROWSER=1"]
fn web_headless_browser_gate() {
    if std::env::var_os("MARTENSITE_WEB_BROWSER").is_none() {
        eprintln!("MARTENSITE_WEB_BROWSER not set; gate skipped");
        return;
    }

    let example_dir = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
    for (tool, args) in [
        ("trunk", ["--version"].as_slice()),
        ("node", ["--version"].as_slice()),
        ("npm", ["--version"].as_slice()),
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
             (cargo install trunk; node + npm) or unset MARTENSITE_WEB_BROWSER"
        );
    }

    // Pick a free port, then release it for `trunk serve`.
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port();
    let url = format!("http://127.0.0.1:{port}");

    let mut trunk = Command::new("trunk")
        .args(["serve", "--port", &port.to_string()])
        .current_dir(&example_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn trunk serve");
    let _trunk_guard = ChildGuard(&mut trunk);

    // First `trunk serve` run builds the wasm crate — allow generously.
    wait_for_port(port, Duration::from_secs(240)).unwrap_or_else(|| {
        panic!("trunk serve did not open {url} — is the wasm target installed?")
    });

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

/// Polls until `trunk serve` accepts connections or the deadline passes.
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

const url = process.argv[2];
const logs = [];
const browser = await chromium.launch();
try {
  const page = await browser.newPage();
  page.on('console', (m) => logs.push(m.text()));
  page.on('pageerror', (e) => logs.push(`pageerror: ${e}`));
  await page.goto(url, { waitUntil: 'load' });
  // Let wasm init, the GPU probe resolve, and a few rAF frames pass.
  await page.waitForTimeout(5000);
  var mirror = await page.$('[data-martensite-a11y-mirror]');
  var liveTexts = await page.$$eval('[aria-live]', (els) =>
    els.map((e) => e.textContent || ''),
  );
} finally {
  await browser.close();
}

const joined = logs.join('\n');
const failures = [];
if (!joined.includes('martensite web example starting')) {
  failures.push('missing wasm start log');
}
if (!/web backend selected:|no GPU adapter/.test(joined)) {
  failures.push('no GPU backend decision logged');
}
if (!mirror) {
  failures.push('a11y mirror element missing');
}
if (!liveTexts.some((t) => t.includes('Martensite web example loaded'))) {
  failures.push('aria-live load announcement missing');
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
