//! `engine_embed` — the v0.14.0 external-surface milestone, end to end.
//!
//! One `MockEngine` producer renders a deterministic frame into a
//! same-device `wgpu::Texture` every tick and publishes it through the
//! [`BridgeRegistry`] two-slot mailbox ring (`mark_ready_full`). The
//! [`ExternalEngine`] widget laid out inside the window emits a
//! `PaintCommand::External` marker, and [`RenderOrchestrator`]
//! composites the ring's front frame zero-copy between the surrounding
//! Vello paint segments — then the released token flows back to the
//! engine for texture recycling:
//!
//! ```text
//! drive_frame ──► MockEngine::render ──► acquire → render → mark_ready_full
//!                                                    │ ready waker
//!                                                    ▼
//!                            widget.poll_frame + window.request_redraw
//!                                                    │
//! record_paint ─► PaintCommand::External ─► orchestrator.render
//!                                                    ▼
//!              render_to_surface: take_front → composite_front →
//!              queue.submit → release → pre_present_notify → present
//!                                                    │
//!              engines.drain_released ◄──────────────┘
//! ```

use std::sync::{Arc, Mutex};

use martensite::core::{HotNode, LayoutContext, Rect, Widget};
use martensite::widgets::external::{ExternalEngine, ExternalEngines};
use martensite_engine_bridge::testing::MockEngine;
use martensite_engine_bridge::{BridgeHandle, EngineContext, FrameToken, SurfaceId};
use martensite_render::{PaintList, Rect as PaintRect};
use martensite_wgpu::{
    BackdropMode, GpuContext, OrchestratorConfig, PresentModePreference, RecoveryMachine,
    RenderOrchestrator, SurfaceWrapper,
};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

/// Window inset around the embedded viewport, in physical pixels.
const MARGIN: f32 = 32.0;

/// Lays `widget` out inside `size` and pushes the physical-pixel
/// viewport to the bridge — the producer's size + DPI contract for the
/// next `drive_frame` call.
fn layout_into(widget: &mut ExternalEngine, size: PhysicalSize<u32>) {
    let w = (size.width as f32 - MARGIN * 2.0).max(1.0);
    let h = (size.height as f32 - MARGIN * 2.0).max(1.0);
    let mut hot = HotNode::default();
    widget.layout(
        &mut LayoutContext { hot: &mut hot },
        Rect::new(MARGIN, MARGIN, w, h),
    );
}

/// The demo app. Everything GPU/window-bound is created lazily inside
/// `can_create_surfaces`, per the winit 0.31 lifecycle.
struct App {
    /// One bridge registry shared by producer, widget, and compositor.
    bridge: BridgeHandle,
    /// The registered ring this demo's single engine/widget pair uses.
    surface_id: SurfaceId,
    /// Bound producers. Behind a mutex because the orchestrator's
    /// CPU-frame resolver also calls `cpu_frame_for` from inside
    /// `render`.
    engines: Arc<Mutex<ExternalEngines>>,

    window: Option<Arc<dyn Window>>,
    gpu: Option<GpuContext>,
    surface: Option<SurfaceWrapper<'static>>,
    orchestrator: Option<RenderOrchestrator>,
    widget: Option<ExternalEngine>,

    /// Device-loss FSM driving the GPU → TinySkia fallback decision.
    recovery: RecoveryMachine,
    /// Set on init/resize/scale change; cleared once layout runs.
    needs_layout: bool,
}

impl App {
    fn new() -> Self {
        // One bridge, one surface: every stage references the same
        // `SurfaceId` on this registry.
        let bridge = BridgeHandle::new();
        let surface_id = bridge.lock().register();

        // The deterministic producer: fills each ring-slot texture
        // with a color whose red channel increments per frame, and
        // ships a small CpuFrame for the TinySkia fallback path.
        let engine = MockEngine::new(bridge.clone(), surface_id, [200, 80, 40, 255]);
        let mut engines = ExternalEngines::new();
        engines
            .bind(bridge.clone(), surface_id, Box::new(engine))
            .expect("first binding on this surface");

        Self {
            bridge,
            surface_id,
            engines: Arc::new(Mutex::new(engines)),
            window: None,
            gpu: None,
            surface: None,
            orchestrator: None,
            widget: None,
            recovery: RecoveryMachine::new(),
            needs_layout: true,
        }
    }

    /// One frame of the producer → ring → composite loop.
    fn redraw(&mut self) {
        let Self {
            window: Some(window),
            gpu: Some(gpu),
            surface: Some(surface),
            orchestrator: Some(orchestrator),
            widget: Some(widget),
            engines,
            recovery,
            needs_layout,
            surface_id,
            ..
        } = self
        else {
            return;
        };

        // 1. Layout — forwards the widget's physical bounds + DPI as the
        //    bridge viewport so the engine renders at its real size.
        if *needs_layout {
            layout_into(widget, window.surface_size());
            *needs_layout = false;
        }

        // 2. Drive producers: each bound engine renders one frame into a
        //    ring slot and publishes it (`mark_ready_full`). The shared
        //    queue makes the finished texture visible to the composite
        //    pass with no extra synchronization. Surfaces that produced
        //    a frame are returned; the ready-waker installed below has
        //    already queued the next repaint for streaming cadence.
        let ready = {
            let mut ctx = EngineContext {
                device: &gpu.device,
                queue: &gpu.queue,
            };
            engines.lock().expect("engines mutex").drive_frame(&mut ctx)
        };

        // 3. Consume the FramePoll: a size change re-runs layout, a new
        //    frame warrants the repaint the loop already schedules.
        if ready.contains(surface_id) {
            let poll = widget.poll_frame();
            if poll.needs_layout() {
                // First frame carries the intrinsic size — relayout so
                // the fit rect is computed against real dimensions.
                layout_into(widget, window.surface_size());
            }
            if poll.needs_redraw() {
                window.request_redraw();
            }
        }

        // 4. Record paint: a backdrop and a border under the marker,
        //    the `PaintCommand::External` at the widget rect, and a HUD
        //    strip painted after it — showing ordinary paint both under
        //    and over the external frame in exact paint order.
        let size = window.surface_size();
        let (w, h) = (f64::from(size.width), f64::from(size.height));
        let m = f64::from(MARGIN);
        let mut list = PaintList::new();
        list.push_fill_rect(PaintRect::new(0.0, 0.0, w, h), [18, 20, 26, 255]);
        list.push_fill_rect(
            PaintRect::new(m - 4.0, m - 4.0, (w - m + 4.0).max(m), (h - m + 4.0).max(m)),
            [60, 120, 200, 255],
        );
        widget.record_paint(&mut list);
        list.push_fill_rect(PaintRect::new(0.0, 0.0, w, 6.0), [40, 200, 120, 255]);

        // 5. Composite + present. `render` splits the list at the
        //    External marker into Vello segments; `render_to_surface`
        //    takes the ring's front frame, composites it zero-copy in
        //    paint order, submits, releases the slot, fires
        //    `pre_present_notify`, and presents.
        orchestrator.render(&list, recovery);
        if let Err(err) = orchestrator.render_to_surface(&gpu.device, &gpu.queue, surface) {
            // Feed the failure into the recovery FSM and reconfigure so
            // the next frame has a live swapchain.
            recovery.handle_surface_error(err);
            let _ = surface.resize(&gpu.device, size.width.max(1), size.height.max(1));
        }

        // 6. Producer recycling: the composite released the ring slots
        //    after `queue.submit`; hand the tokens back to the engines.
        engines.lock().expect("engines mutex").drain_released();

        // 7. The mock produces every tick — keep the loop animating.
        window.request_redraw();
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        // Window — held as `Arc<dyn Window>` so the wgpu surface and the
        // app share ownership (wgpu 30 / winit 0.31 idiom).
        let window: Arc<dyn Window> = event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title("Martensite — engine_embed")
                    .with_surface_size(PhysicalSize::new(960, 640)),
            )
            .expect("create window")
            .into();

        // GPU: surface first, then a compatible adapter — the adapter
        // is guaranteed to be able to present to this surface.
        let instance = wgpu::Instance::default();
        let raw_surface = instance
            .create_surface(Arc::clone(&window))
            .expect("create surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&raw_surface),
            force_fallback_adapter: false,
            apply_limit_buckets: true,
        }))
        .expect("request adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("request device");
        let gpu = GpuContext {
            adapter_info: adapter.get_info(),
            instance,
            adapter,
            device,
            queue,
        };

        // Swapchain — low-latency pacing for streaming content.
        let size = window.surface_size();
        let mut surface = SurfaceWrapper::new(raw_surface);
        surface.set_pacing(PresentModePreference::LowLatency);
        surface
            .configure(
                &gpu.device,
                &gpu.adapter,
                size.width.max(1),
                size.height.max(1),
                BackdropMode::Opaque,
            )
            .expect("configure surface");

        // Orchestrator — Vello GPU path with TinySkia fallback allowed
        // (the cpu_frame_resolver below feeds it `Engine::to_pixmap`).
        let mut orchestrator = RenderOrchestrator::new(
            size.width.max(1),
            size.height.max(1),
            OrchestratorConfig::new(true, false),
        )
        .expect("orchestrator");

        // Bridge install: `PaintCommand::External` markers now resolve
        // through the ring — `composite_front` takes the freshest frame
        // zero-copy (the `WgpuHost` is created lazily at dispatch time
        // for the configured surface format).
        orchestrator.set_bridge(self.bridge.clone());

        // winit pacing contract — notify the compositor before present.
        let notify_window = Arc::clone(&window);
        orchestrator.set_pre_present_notify(Some(Box::new(move || {
            notify_window.pre_present_notify();
        })));

        // CPU-fallback raster source: the bound engine's `to_pixmap`.
        let resolver_engines = Arc::clone(&self.engines);
        orchestrator.set_cpu_frame_resolver(Some(Box::new(move |surface, token| {
            resolver_engines
                .lock()
                .expect("engines mutex")
                .cpu_frame_for(surface, FrameToken(token))
        })));

        // Producer → host wake: every published frame schedules a
        // repaint. The waker runs under the registry lock — it must
        // only signal, never re-lock the bridge.
        let waker_window = Arc::clone(&window);
        self.bridge.set_ready_waker(Some(Box::new(move |_surface| {
            waker_window.request_redraw();
        })));

        // The widget — a retained leaf whose paint output is the
        // External marker at its laid-out rect.
        self.widget = Some(
            ExternalEngine::new(self.bridge.clone(), self.surface_id)
                .with_scale_factor(window.scale_factor())
                .with_aspect_ratio(16.0 / 9.0)
                .with_label("Mock engine viewport"),
        );

        self.window = Some(window);
        self.gpu = Some(gpu);
        self.surface = Some(surface);
        self.orchestrator = Some(orchestrator);
        self.needs_layout = true;

        // Kick the first frame; the redraw loop is self-sustaining
        // from here (ready-waker + trailing request_redraw).
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::SurfaceResized(size) => {
                if size.width > 0 && size.height > 0 {
                    if let (Some(surface), Some(gpu)) = (&mut self.surface, &self.gpu) {
                        // Reconfigure the swapchain; the relayout flag
                        // pushes the new viewport on the next redraw.
                        let _ = surface.resize(&gpu.device, size.width, size.height);
                    }
                    self.needs_layout = true;
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                // Push the new DPI to the widget — the next layout
                // re-pushes the bridge viewport so producers render at
                // the new scale.
                if let (Some(window), Some(widget)) = (&self.window, &mut self.widget) {
                    widget.set_scale_factor(window.scale_factor());
                }
                self.needs_layout = true;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    // Poll keeps the producer cadence continuous; each RedrawRequested
    // re-arms the next, and Fifo-present acquisition paces the loop.
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(App::new())?;
    Ok(())
}
