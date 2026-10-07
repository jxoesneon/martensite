//! Web (`wasm32-unknown-unknown`) bootstrap for the widget catalog.
//!
//! This module is the browser half of the dual-target app: the
//! `#[wasm_bindgen(start)]` shim binds `#martensite-canvas`, prepares
//! its DPI-scaled backing store, and hands the shared
//! [`App`](crate::app::App) to `martensite-web`'s event-loop driver.
//! Two async legs then run on the main thread:
//!
//! * **GPU probe** — `gpu::create_instance` +
//!   `gpu::gpu_context_for_web` perform the real `navigator.gpu`
//!   probe and log the selected [`WebBackend`](gpu::WebBackend):
//!   `WebGpu` → Vello compute, `WebGl2`/no adapter → TinySkia CPU
//!   raster (`prefer_cpu`), enforced by the boundary in
//!   `martensite-wgpu::web`.
//! * **Font fetch** — the web has no system fonts, so
//!   `assets/fonts/font.ttf` is downloaded and installed as the
//!   thread-local fixture-font source (`FontManager::new` consults it
//!   on lazy creation — the web build is single-threaded).
//!
//! The shared [`App`] consumes both legs from
//! `App::try_finish_web_init` once they land — first rendered frame
//! waits for the probe, so the canvas never presents against an
//! unconfigured surface.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use martensite_web::wasm_bindgen_futures::spawn_local;
use martensite_web::{gpu, wgpu};
use martensite_wgpu::GpuContext;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::app::{App, LOGICAL_HEIGHT, LOGICAL_WIDTH};

/// Console-side log line — the browser gate asserts on these.
pub(crate) fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

/// Async bootstrap state produced by [`spawn_init_tasks`] and consumed
/// exactly once by `App::try_finish_web_init`.
#[derive(Default)]
pub(crate) struct WebInit {
    /// The `#martensite-canvas` element the winit window binds to —
    /// stored by `App::new_web` so `create_surfaces_web` can attach
    /// it via `WebWindowAttributes`.
    pub(crate) canvas: Option<HtmlCanvasElement>,
    /// GPU probe result — `(context, instance, surface, prefer_cpu)`.
    /// The raw `Instance` rides alongside the context: `GpuContext`
    /// holds a clone internally, but wgpu's wasm backend resolves
    /// surfaces/adapters through external handle slots — keeping the
    /// originating handle alive for the app's lifetime is the cheap
    /// way to make the surface's instance reference permanent.
    gpu: Option<(GpuContext, wgpu::Instance, wgpu::Surface<'static>, bool)>,
    /// Set when the probe finishes. On failure `gpu` stays `None` and
    /// the app still comes up (arena/a11y/dev session) without
    /// presentation.
    pub(crate) gpu_ready: bool,
    /// Set when the font fetch finishes, success or failure.
    pub(crate) font_ready: bool,
    /// Fixture-font override guard holding the fetched font — moved
    /// into the app so the override outlives the bootstrap.
    pub(crate) font_guard: Option<martensite::text::font::TestFontsGuard>,
}

impl WebInit {
    /// Moves the completed GPU probe out, leaving `None`s behind.
    /// Called once from `try_finish_web_init`.
    pub(crate) fn take_gpu(
        &mut self,
    ) -> (
        Option<GpuContext>,
        Option<wgpu::Instance>,
        Option<wgpu::Surface<'static>>,
        bool,
    ) {
        match self.gpu.take() {
            Some((context, instance, surface, prefer_cpu)) => {
                (Some(context), Some(instance), Some(surface), prefer_cpu)
            }
            None => (None, None, None, false),
        }
    }
}

/// Spawns the two async bootstrap legs — GPU adapter probe and font
/// fetch. Both write back into `init`; the app's frame loop picks the
/// results up when `gpu_ready && font_ready`.
pub(crate) fn spawn_init_tasks(init: Rc<RefCell<WebInit>>, canvas: HtmlCanvasElement) {
    {
        let init = Rc::clone(&init);
        spawn_local(async move {
            let instance = gpu::create_instance().await;
            let surface = match instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas)) {
                Ok(surface) => surface,
                Err(err) => {
                    log(&format!("surface creation failed ({err}); no presentation"));
                    init.borrow_mut().gpu_ready = true;
                    return;
                }
            };
            match gpu::gpu_context_for_web(&instance, Some(&surface)).await {
                Ok((context, backend)) => {
                    log(&format!("web backend selected: {backend:?}"));
                    let prefer_cpu = backend.requires_cpu_raster();
                    if prefer_cpu {
                        // WebGL2 has no compute — Vello cannot run
                        // there; the orchestrator's TinySkia path
                        // rasterizes on CPU and uploads via
                        // `queue.write_texture`.
                        log("Vello unavailable — TinySkia CPU raster path");
                    }
                    context
                        .device
                        .on_uncaptured_error(std::sync::Arc::new(|err| {
                            log(&format!("wgpu uncaptured error: {err}"));
                        }));
                    init.borrow_mut().gpu = Some((context, instance, surface, prefer_cpu));
                }
                Err(err) => {
                    // No adapter at all — arena/a11y still come up; the
                    // canvas simply has no present target.
                    log(&format!(
                        "no GPU adapter ({err}) — TinySkia CPU raster only"
                    ));
                }
            }
            init.borrow_mut().gpu_ready = true;
        });
    }
    {
        let init = Rc::clone(&init);
        spawn_local(async move {
            match martensite_web::text::fetch_font("assets/fonts/font.ttf").await {
                Ok(bytes) => {
                    // The web has no system font set — `FontManager::new`
                    // would produce an empty database. The fixture
                    // override is consulted by every `FontManager::new`
                    // on this thread (the wasm main thread — the only
                    // one), so the lazily-created manager inside the
                    // shared text painter picks up the fetched face and
                    // aliases every generic family to it.
                    let guard = martensite::text::font::set_test_fonts(vec![
                        martensite::text::FontSource::binary(bytes),
                    ]);
                    init.borrow_mut().font_guard = Some(guard);
                    log("font loaded: assets/fonts/font.ttf");
                }
                Err(err) => log(&format!(
                    "font fetch failed — catalog renders without shaped text: {err}"
                )),
            }
            init.borrow_mut().font_ready = true;
        });
    }
}

thread_local! {
    /// Set by the JS export below; consumed by `App::pump_a11y` on the
    /// event thread — the bridge's `set_enabled` runs DOM focus calls
    /// that must not fire from inside an arbitrary JS callback stack.
    static A11Y_ENABLE_REQUESTED: Cell<bool> = const { Cell::new(false) };
}

/// Whether a programmatic a11y-enable request is pending (and clears
/// the flag). Called by the app's a11y pump each frame.
pub(crate) fn take_a11y_enable_request() -> bool {
    A11Y_ENABLE_REQUESTED.with(|flag| flag.replace(false))
}

/// JS export `martensite_catalog_enable_a11y()` — programmatically
/// enables the DOM/ARIA mirror. The production path is the bridge's
/// own hidden "Enable accessibility" button; this hook exists so the
/// headless browser gate (and the JS console) can turn the mirror on
/// without synthesizing a trusted click.
#[wasm_bindgen]
pub fn martensite_catalog_enable_a11y() {
    A11Y_ENABLE_REQUESTED.with(|flag| flag.set(true));
}

/// wasm-bindgen entry point: binds the page canvas, kicks off the
/// async init legs, and registers the shared catalog `App` on the web
/// event loop (`martensite-web` installs the panic hook, sets
/// `ControlFlow::Poll` + the scheduler poll strategy, and spawns the
/// handler — `run_app` returns immediately on the web and the
/// browser's rAF/scheduler drives the loop from there).
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    log("martensite widget catalog starting");
    let canvas = martensite_web::canvas_element("martensite-canvas")?;
    martensite_web::prepare_canvas(&canvas, LOGICAL_WIDTH, LOGICAL_HEIGHT);
    let app = App::new_web(canvas.clone());
    spawn_init_tasks(Rc::clone(&app.web_init), canvas);
    martensite_web::run(app)
}
