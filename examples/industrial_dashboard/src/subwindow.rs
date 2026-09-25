//! A real secondary OS window — the "Console" surface the toolbar's
//! **Console** button opens. This is the multi-window dogfood: its own
//! `WidgetArena`, `EventRouter`, `FocusManager`, `SurfaceWrapper`, and
//! `RenderOrchestrator`, sharing the primary window's `GpuContext`
//! (`instance`/`device`/`queue` are `Arc`s by design — surfaces must be
//! created from the same `wgpu::Instance` to present).
//!
//! Content is a `Flex` column of facade widgets — `Banner`,
//! `Disclosure`+`Text`, `ProgressBar`, `Switch` — so the second window
//! exercises the same widget stack, theme dictionary, and shaped text
//! pipeline as the main surface, including its own overlay layer.

use std::sync::Arc;
use std::time::Instant;

use glam::Vec2;
use martensite::core::{ColdNode, HotNode, LayoutContext, NodeFlags, Rect, WidgetArena, WidgetId};
use martensite::focus::FocusManager;
use martensite::prelude::Signal;
use martensite::render::PaintList;
use martensite::theme::{Theme, TokenKey};
use martensite::wgpu::{
    BackdropMode, GpuContext, OrchestratorConfig, RecoveryMachine, RenderOrchestrator,
    SurfaceWrapper,
};
use martensite::widgets::{
    Banner, Container, Disclosure, Flex, ProgressBar, Severity, Switch, Text,
};
use martensite::window::event::{
    ime_event_for_winit, EventRouter, ModifierKeys, MouseButton as MButton, PointerEvent,
    PointerId, PointerKind, PointerState,
};
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId};

/// An open secondary window: window + surface + orchestrator + arena +
/// router — everything needed to route events and present frames
/// independently of the primary surface.
pub struct SubWindow {
    window: Arc<dyn Window>,
    surface: SurfaceWrapper<'static>,
    orchestrator: RenderOrchestrator,
    recovery: RecoveryMachine,
    arena: WidgetArena,
    root: WidgetId,
    router: EventRouter,
    focus: FocusManager,
    last_frame: Instant,
}

impl SubWindow {
    /// The winit id used to route `window_event`s here.
    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    /// Opens the console window sharing `gpu`'s device and instance.
    pub fn open(
        event_loop: &dyn ActiveEventLoop,
        gpu: &GpuContext,
        theme: &Theme,
        cpu: Signal<f64>,
    ) -> Option<Self> {
        let window: Arc<dyn Window> = event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title("Martensite — Console")
                    .with_surface_size(PhysicalSize::new(520, 420)),
            )
            .ok()?
            .into();
        let scale = window.scale_factor() as f32;

        let raw = gpu.instance.create_surface(Arc::clone(&window)).ok()?;
        let mut surface = SurfaceWrapper::new(raw);
        let size = window.surface_size();
        surface
            .configure(
                &gpu.device,
                &gpu.adapter,
                size.width.max(1),
                size.height.max(1),
                BackdropMode::Opaque,
            )
            .ok()?;
        let orchestrator = RenderOrchestrator::new(
            size.width.max(1),
            size.height.max(1),
            OrchestratorConfig::new(true, false),
        )
        .ok()?;

        // A second arena — the widget stack is identical to the main
        // surface's: theme dictionary, shared text painter, overlay
        // layer with the window's real viewport.
        let mut arena = WidgetArena::new();
        arena.set_theme(theme.clone());
        arena.set_scale_factor(scale);
        arena.set_text_painter(martensite::text_paint::shared_painter());
        arena.overlay_mut().set_viewport(Rect::new(
            0.0,
            0.0,
            size.width as f32,
            size.height as f32,
        ));

        let mut root_hot = HotNode::default();
        root_hot.flags |= NodeFlags::VISIBLE;
        let root = arena.insert_with_widget(root_hot, Box::new(Container::new()));

        let painter = martensite::text_paint::shared_painter();
        let content = Container::new().padding_uniform(16.0).child(
            Flex::column().gap(10.0).children([
                // Title-tier heading (spec B1): 15 pt semibold — the
                // console window's one heading gets the same tier as
                // the shell's title chrome.
                Box::new(
                    Text::new("Console — secondary OS window")
                        .font_size(15.0)
                        .font_weight(martensite::core::FontWeight::SEMIBOLD),
                ) as Box<dyn martensite::core::Widget>,
                Box::new(
                    Banner::new(Severity::Info, "Same arena, separate surface")
                        .with_text_painter(painter.clone()),
                ),
                Box::new(
                    Disclosure::new("Facilities")
                        .child(Text::new(
                            "This window has its own arena, router, focus manager, and overlay layer.",
                        ))
                        .with_text_painter(painter.clone()),
                ),
                Box::new(
                    Switch::new("mirror telemetry")
                        .on(true)
                        .with_text_painter(painter.clone()),
                ),
                // `bind` dogfoods reactive progress: the bar polls the
                // shared CPU signal in `tick` and dirties itself.
                Box::new(ProgressBar::new().bind(cpu)),
            ]),
        );
        let mut hot = HotNode::default();
        hot.flags |= NodeFlags::FOCUSABLE | NodeFlags::VISIBLE | NodeFlags::HIT_TEST_ENABLED;
        let mut cold = ColdNode::new(Box::new(content));
        cold.debug_name = Some("ConsoleContent");
        let id = arena.insert(hot, cold);
        arena.append_child(root, id).expect("console child");

        let mut focus = FocusManager::new();
        focus.set_root(root);

        let mut w = Self {
            window,
            surface,
            orchestrator,
            recovery: RecoveryMachine::new(),
            arena,
            root,
            router: EventRouter::new(),
            focus,
            last_frame: Instant::now(),
        };
        w.relayout();
        Some(w)
    }

    /// Lays out the arena to the window's current size.
    fn relayout(&mut self) {
        let size = self.window.surface_size();
        let b = Rect::new(0.0, 0.0, size.width as f32, size.height as f32);
        self.arena
            .set_scale_factor(self.window.scale_factor() as f32);
        self.arena
            .overlay_mut()
            .set_viewport(Rect::new(0.0, 0.0, b.width(), b.height()));
        if let Some(hot) = self.arena.get_hot_mut(self.root) {
            hot.bounds = b;
        }
        // The root's single child fills the window.
        let scale = self.arena.scale_factor();
        let child = self.arena.children(self.root).next();
        if let Some(child) = child {
            if let Some((hot, cold)) = self.arena.get_both_mut(child) {
                hot.bounds = b;
                cold.widget.layout(&mut LayoutContext { hot, scale }, b);
            }
        }
    }

    /// Handles a `WindowEvent` addressed to this window. Returns `true`
    /// when the window asked to close (caller drops the `SubWindow`).
    pub fn handle_event(&mut self, gpu: &GpuContext, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::CloseRequested => return true,
            WindowEvent::SurfaceResized(size) => {
                if size.width > 0 && size.height > 0 {
                    let _ = self.surface.resize(&gpu.device, size.width, size.height);
                    self.orchestrator.set_frame_size(size.width, size.height);
                    self.relayout();
                    self.window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                self.relayout();
                self.window.request_redraw();
            }
            WindowEvent::PointerMoved {
                position, primary, ..
            } => {
                if *primary {
                    self.pointer(
                        Vec2::new(position.x as f32, position.y as f32),
                        PointerState::Moved,
                        None,
                    );
                }
            }
            WindowEvent::PointerButton {
                state,
                position,
                button,
                primary,
                ..
            } => {
                if *primary {
                    let btn = match button {
                        winit::event::ButtonSource::Mouse(winit::event::MouseButton::Left) => {
                            Some(MButton::Left)
                        }
                        winit::event::ButtonSource::Mouse(winit::event::MouseButton::Right) => {
                            Some(MButton::Right)
                        }
                        _ => None,
                    };
                    self.pointer(
                        Vec2::new(position.x as f32, position.y as f32),
                        if *state == ElementState::Pressed {
                            PointerState::Pressed
                        } else {
                            PointerState::Released
                        },
                        btn,
                    );
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let key_name = match &event.logical_key {
                    winit::keyboard::Key::Named(n) => format!("{n:?}"),
                    winit::keyboard::Key::Character(c) => c.to_string(),
                    _ => return false,
                };
                let focused = self.focus.current_focus();
                self.router.dispatch_keyboard_event(
                    &mut self.arena,
                    focused,
                    &key_name,
                    event.state == ElementState::Pressed,
                    event.repeat,
                );
                self.sync_focus();
            }
            WindowEvent::Ime(ime) => {
                // Preedit/Commit route to the focused widget; host
                // lifecycle variants drop out as `None`.
                if let Some(ev) = ime_event_for_winit(ime) {
                    let focused = self.focus.current_focus();
                    self.router
                        .dispatch_ime_event(&mut self.arena, focused, &ev);
                    self.sync_focus();
                }
            }
            WindowEvent::RedrawRequested => self.render(gpu),
            _ => {}
        }
        false
    }

    /// `CaptureFocus` responses land in the router's pending slot; feed
    /// them to the FocusManager.
    fn sync_focus(&mut self) {
        if let Some(id) = self.router.take_focus_request() {
            self.focus.apply_focus_request(&mut self.arena, id);
        }
    }

    fn pointer(&mut self, pos: Vec2, state: PointerState, button: Option<MButton>) {
        let ev = PointerEvent {
            pointer_id: PointerId::PRIMARY,
            kind: PointerKind::Mouse,
            position: pos,
            state,
            button,
            modifiers: ModifierKeys::empty(),
        };
        self.router
            .dispatch_pointer_event(&mut self.arena, self.root, self.window.id(), &ev);
        self.sync_focus();
        self.window.request_redraw();
    }

    /// Ticks + paints + presents one frame on this window's surface.
    pub fn render(&mut self, gpu: &GpuContext) {
        let now = Instant::now();
        let dt = now - self.last_frame;
        self.last_frame = now;
        self.arena.tick(dt);

        let size = self.window.surface_size();
        let (w, h) = (f64::from(size.width), f64::from(size.height));
        let mut list = PaintList::new();
        list.push_fill_rect(
            martensite::render::Rect::new(0.0, 0.0, w, h),
            self.arena
                .theme()
                .color(TokenKey::BackgroundColor)
                .map(|c| c.to_srgba8())
                .unwrap_or([24, 26, 30, 255]),
        );
        self.arena.build_paint_list(self.root, &mut list);
        self.orchestrator.render(&list, &self.recovery);
        if let Err(err) =
            self.orchestrator
                .render_to_surface(&gpu.device, &gpu.queue, &mut self.surface)
        {
            self.recovery.handle_surface_error(err);
            let _ = self
                .surface
                .resize(&gpu.device, size.width.max(1), size.height.max(1));
        }
        // Telemetry-bound widgets animate — keep the loop alive, same
        // as the primary window's redraw tail.
        self.request_redraw();
    }

    /// Queues a redraw on the OS window.
    fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// Installs a theme — called when the primary window's theme
    /// changes so both surfaces track the same dictionary entry.
    pub fn set_theme(&mut self, theme: &Theme) {
        self.arena.set_theme(theme.clone());
    }
}
