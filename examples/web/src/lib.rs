//! Martensite web (`wasm32-unknown-unknown`) smoke-test example.
//!
//! This crate is the compile-checked skeleton for the v0.17.0 web
//! platform layer (milestone §4.3). It wires every web backend added in
//! this milestone into one runnable page:
//!
//! * **GPU** — [`gpu::create_instance`] +
//!   [`gpu::gpu_context_for_web`] perform the real
//!   `navigator.gpu` probe and report which [`gpu::WebBackend`] was
//!   selected (`WebGpu` → Vello-capable, `WebGl2`/`CpuRaster` → TinySkia
//!   CPU raster, per the enforced boundary in `martensite-wgpu::web`).
//!   A clear-color frame proves the surface/device actually work.
//! * **Window/canvas** — [`win_web::WebWindowAttributes`] binds the
//!   `<canvas id="martensite-canvas">` from `index.html`;
//!   [`win_web::sync_canvas_backing_store`] keeps the backing store
//!   DPI-scaled; [`win_web::configure_web_event_loop`] +
//!   [`win_web::spawn_app`] drive the `ControlFlow::Poll`/rAF model.
//! * **IME** — a [`win_web::HiddenImeInput`] overlay logs composition
//!   events.
//! * **Clipboard** — clicking the canvas writes `text/plain` through
//!   [`martensite_clipboard::web::WebClipboard`]'s async API.
//! * **Drag-and-drop** — [`WebDropListener`] accepts file/text drops on
//!   the canvas and logs the captured payload.
//! * **Fonts** — [`text_web::fetch_and_load_font`] tries
//!   `assets/fonts/font.ttf` (see the README for how to supply one).
//! * **Accessibility** — [`WebA11yBridge`] mirrors a tiny AccessKit tree
//!   (root web area + one focusable button) into the hidden DOM/ARIA
//!   mirror and announces load through the `aria-live` region.
//!
//! See `README.md` for `trunk`/`wasm-bindgen-cli` build and smoke-test
//! instructions.
//!
//! The crate compiles to an empty library on non-wasm targets so the
//! host workspace build is unaffected.

#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]
// The example is a smoke-test harness, not a published API surface:
// lint noise that would matter in library code (e.g. `must_use` on
// internal helpers) is not useful here.
#![allow(clippy::missing_panics_doc)]

use std::cell::RefCell;
use std::rc::Rc;

use accesskit::{Action, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use martensite_access::web::WebA11yBridge;
use martensite_clipboard::web::WebClipboard;
use martensite_dnd::web::{read_file_text, WebDropListener};
use martensite_text::web as text_web;
use martensite_text::FontManager;
use martensite_wgpu::device::GpuContext;
use martensite_wgpu::web as gpu;
use martensite_wgpu::wgpu;
use martensite_window::web as win_web;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlCanvasElement;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

/// Logical (CSS-pixel) size the canvas is laid out at. The backing store
/// is `LOGICAL_* devicePixelRatio` physical pixels.
const LOGICAL_WIDTH: u32 = 800;
const LOGICAL_HEIGHT: u32 = 600;

const ROOT: NodeId = NodeId(1);
const BUTTON: NodeId = NodeId(2);

fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

/// GPU state held after the async adapter probe completes.
struct GpuState {
    context: GpuContext,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    backend: gpu::WebBackend,
}

/// The winit application driving the canvas.
#[derive(Default)]
struct App {
    window: Option<Box<dyn Window>>,
    canvas: Option<HtmlCanvasElement>,
    /// Filled asynchronously once the WebGPU/WebGL2 probe resolves.
    gpu: Rc<RefCell<Option<GpuState>>>,
    /// Accessibility bridge — kept alive so the mirror stays in the DOM.
    a11y: Option<WebA11yBridge>,
    /// Platform pieces kept alive for their listeners.
    _dnd: Option<WebDropListener>,
    _ime: Option<win_web::HiddenImeInput>,
    _clipboard: Option<Rc<WebClipboard>>,
    /// DOM event closures that must outlive `can_create_surfaces`.
    _closures: Vec<Closure<dyn FnMut(web_sys::Event)>>,
    /// Font database fed by `fetch_and_load_font`.
    _fonts: Rc<RefCell<FontManager>>,
    frame: u32,
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        let window = web_sys::window().expect("no `window` object");
        let document = window.document().expect("no `document` object");
        let canvas: HtmlCanvasElement = document
            .get_element_by_id("martensite-canvas")
            .and_then(|element| element.dyn_into().ok())
            .expect("index.html must contain <canvas id=\"martensite-canvas\">");

        // Manual backing-store DPI scaling: `width`/`height` attributes
        // are physical pixels; the CSS size stays logical.
        let scale = window.device_pixel_ratio();
        win_web::sync_canvas_backing_store(&canvas, LOGICAL_WIDTH, LOGICAL_HEIGHT, scale);

        let attrs = win_web::WebWindowAttributes::new()
            .with_canvas(Some(canvas.clone()))
            .with_append(true)
            .with_focusable(true)
            .apply(WindowAttributes::default().with_title("Martensite — web"));
        let winit_window = event_loop
            .create_window(attrs)
            .expect("failed to create winit window");
        self.canvas = Some(canvas.clone());
        self.window = Some(winit_window);

        self.init_accessibility();
        self.init_clipboard(&canvas);
        self.init_dnd(&canvas);
        self.init_ime(&document);
        self.init_fonts();
        self.init_gpu(canvas);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::SurfaceResized(size) => {
                let scale = web_sys::window()
                    .map(|w| w.device_pixel_ratio())
                    .unwrap_or(1.0);
                if let Some(canvas) = &self.canvas {
                    win_web::sync_canvas_backing_store(canvas, size.width, size.height, scale);
                }
                if let Some(state) = &mut *self.gpu.borrow_mut() {
                    state.config.width = (f64::from(size.width) * scale).round().max(1.0) as u32;
                    state.config.height = (f64::from(size.height) * scale).round().max(1.0) as u32;
                    state
                        .surface
                        .configure(&state.context.device, &state.config);
                }
            }
            WindowEvent::RedrawRequested => {
                self.render();
                if let Some(window) = &self.window {
                    // Continuous rAF-driven redraw for the smoke test.
                    window.request_redraw();
                }
            }
            WindowEvent::CloseRequested | WindowEvent::Destroyed => event_loop.exit(),
            _ => {}
        }
    }
}

impl App {
    /// Mirrors a minimal AccessKit tree and announces through the live
    /// region. This is the deliberately-minimal v0.17.0 bridge — a
    /// live-region announcer plus a focusable-role DOM mirror.
    fn init_accessibility(&mut self) {
        let mut bridge = match WebA11yBridge::new() {
            Ok(bridge) => bridge,
            Err(e) => {
                log(&format!("a11y bridge unavailable: {e}"));
                return;
            }
        };
        let mut root = Node::new(Role::RootWebArea);
        root.set_children([BUTTON]);
        let mut button = Node::new(Role::Button);
        button.set_label("Martensite canvas");
        button.add_action(Action::Focus);
        button.add_action(Action::Click);
        let update = TreeUpdate {
            nodes: vec![(ROOT, root), (BUTTON, button)],
            tree: Some(TreeInfo::new(ROOT)),
            tree_id: TreeId::ROOT,
            focus: ROOT,
        };
        if let Err(e) = bridge.update(&update) {
            log(&format!("a11y tree update failed: {e}"));
        }
        bridge.set_action_handler(|request| {
            log(&format!("a11y action: {:?}", request.action));
        });
        bridge.announce("Martensite web example loaded");
        self.a11y = Some(bridge);
    }

    /// Click-to-copy through `navigator.clipboard` (secure-context +
    /// user-gesture gated — the click handler *is* the gesture).
    fn init_clipboard(&mut self, canvas: &HtmlCanvasElement) {
        let clipboard = Rc::new(WebClipboard::new());
        log(&format!(
            "navigator.clipboard available: {}",
            clipboard.is_available()
        ));
        let cb = Rc::clone(&clipboard);
        let closure = Closure::wrap(Box::new(move |_: web_sys::Event| {
            let cb = Rc::clone(&cb);
            spawn_local(async move {
                match cb.write_text("Martensite web smoke test").await {
                    Ok(()) => log("clipboard write ok"),
                    Err(e) => log(&format!("clipboard write: {e}")),
                }
            });
        }) as Box<dyn FnMut(web_sys::Event)>);
        canvas
            .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
            .expect("click listener");
        self._closures.push(closure);
        self._clipboard = Some(clipboard);
    }

    /// HTML5 DataTransfer drop target on the canvas.
    fn init_dnd(&mut self, canvas: &HtmlCanvasElement) {
        let listener = match WebDropListener::attach(canvas) {
            Ok(listener) => listener,
            Err(e) => {
                log(&format!("dnd listener unavailable: {e}"));
                return;
            }
        };
        let handle = listener.clone();
        listener.set_on_outcome(move |outcome| {
            if outcome.accepted {
                log(&format!("drop accepted: {:?}", outcome.effect));
            }
            if let Some(id) = handle.last_transfer_id() {
                if let Some(capture) = handle.take_drop(id) {
                    for (mime, text) in &capture.data {
                        log(&format!(
                            "drop data [{mime}]: {}",
                            &text[..text.len().min(80)]
                        ));
                    }
                    for file in capture.files {
                        let name = file.name();
                        spawn_local(async move {
                            match read_file_text(file).await {
                                Ok(text) => {
                                    log(&format!("dropped file {name}: {} bytes", text.len()));
                                }
                                Err(e) => log(&format!("file read for {name}: {e}")),
                            }
                        });
                    }
                }
            }
        });
        self._dnd = Some(listener);
    }

    /// Hidden `<input>` IME overlay — logs composition events so the
    /// smoke test can verify CJK/IME input reaches the app.
    fn init_ime(&mut self, document: &web_sys::Document) {
        let Some(body) = document.body() else {
            return;
        };
        match win_web::HiddenImeInput::new(&body) {
            Ok(ime) => {
                ime.set_on_ime(|event| log(&format!("ime: {event:?}")));
                ime.set_position(16.0, 16.0, 20.0);
                self._ime = Some(ime);
            }
            Err(e) => log(&format!("ime overlay unavailable: {e}")),
        }
    }

    /// Runtime font fetch. Bundled fonts would go through
    /// `text_web::bundled_font_manager(&[include_bytes!(...)])` — this
    /// example fetches instead so no font file needs to live in the
    /// repo; drop any `.ttf` at `assets/fonts/font.ttf` next to
    /// `index.html` to exercise the success path.
    fn init_fonts(&mut self) {
        let manager = Rc::clone(&self._fonts);
        spawn_local(async move {
            match text_web::fetch_font("assets/fonts/font.ttf").await {
                Ok(bytes) => {
                    let ids = manager.borrow_mut().load_font_data(bytes);
                    log(&format!("font loaded: {} face(s)", ids.len()));
                }
                Err(e) => log(&format!(
                    "font fetch (expected without assets/fonts/font.ttf): {e}"
                )),
            }
        });
    }

    /// WebGPU → WebGL2 → CPU-raster adapter probe.
    fn init_gpu(&mut self, canvas: HtmlCanvasElement) {
        let slot = Rc::clone(&self.gpu);
        spawn_local(async move {
            let instance = gpu::create_instance().await;
            let surface = match instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas)) {
                Ok(surface) => surface,
                Err(e) => {
                    log(&format!("surface creation failed ({e}); CPU raster only"));
                    return;
                }
            };
            match gpu::gpu_context_for_web(&instance, Some(&surface)).await {
                Ok((context, backend)) => {
                    log(&format!("web backend selected: {backend:?}"));
                    if backend.requires_cpu_raster() {
                        // Production wiring: RenderOrchestrator's TinySkia
                        // path rasterizes into a pixel buffer uploaded via
                        // `queue.write_texture`. Vello is compute-only —
                        // there is no WebGL2 Vello path to attempt.
                        log("Vello unavailable — TinySkia CPU raster path");
                    }
                    let caps = surface.get_capabilities(&context.adapter);
                    let Some(&format) = caps.formats.first() else {
                        log("surface exposes no formats; CPU raster only");
                        return;
                    };
                    let scale = web_sys::window()
                        .map(|w| w.device_pixel_ratio())
                        .unwrap_or(1.0);
                    let config = wgpu::SurfaceConfiguration {
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        format,
                        color_space: wgpu::SurfaceColorSpace::Auto,
                        width: (f64::from(LOGICAL_WIDTH) * scale).round().max(1.0) as u32,
                        height: (f64::from(LOGICAL_HEIGHT) * scale).round().max(1.0) as u32,
                        present_mode: wgpu::PresentMode::AutoVsync,
                        desired_maximum_frame_latency: 2,
                        alpha_mode: wgpu::CompositeAlphaMode::Auto,
                        view_formats: vec![],
                    };
                    surface.configure(&context.device, &config);
                    *slot.borrow_mut() = Some(GpuState {
                        context,
                        surface,
                        config,
                        backend,
                    });
                    log("surface configured — rendering");
                }
                Err(e) => {
                    // WebBackend::CpuRaster: no adapter at all. The
                    // TinySkia rasterizer (RenderOrchestrator) is the
                    // committed fallback; this skeleton reports it.
                    log(&format!("no GPU adapter ({e}) — TinySkia CPU raster only"));
                }
            }
        });
    }

    /// Renders one animated clear-color frame — proof that the selected
    /// backend's device, queue, and surface actually present.
    fn render(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        let guard = self.gpu.borrow();
        let Some(state) = &*guard else {
            return;
        };
        let frame = match state.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            // Outdated/Lost → reconfigure on the next resize; the rest
            // are transient skip-frame statuses.
            _ => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            state
                .context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("martensite-web"),
                });
        let t = f64::from(self.frame % 240) / 240.0;
        // Tint by backend so the smoke test is visually distinguishable:
        // indigo sweep on WebGPU, teal on the WebGL2 downlevel path.
        let clear = match state.backend {
            gpu::WebBackend::WebGpu => wgpu::Color {
                r: 0.05 + 0.45 * t,
                g: 0.12,
                b: 0.55 - 0.35 * t,
                a: 1.0,
            },
            _ => wgpu::Color {
                r: 0.05,
                g: 0.25 + 0.45 * t,
                b: 0.35,
                a: 1.0,
            },
        };
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("martensite-web-clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        state.context.queue.submit([encoder.finish()]);
        // wgpu 30 presents the surface texture on drop.
        drop(frame);
    }
}

/// wasm-bindgen entry point (`wasm_bindgen(start)`): configures the web
/// event loop and spawns the [`App`].
///
/// Returns once the handler is registered — on the web `run_app` is the
/// spawn call; the browser's scheduler drives the loop from there.
///
/// # Errors
///
/// Returns a JS error if the winit event loop cannot be created or the
/// application handler cannot be registered.
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    log("martensite web example starting");
    let event_loop = EventLoop::new().map_err(|e| JsValue::from_str(&e.to_string()))?;
    win_web::configure_web_event_loop(&event_loop);
    win_web::spawn_app(event_loop, App::default()).map_err(|e| JsValue::from_str(&e.to_string()))
}
