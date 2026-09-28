//! A real secondary OS window — the "Console" surface the toolbar's
//! **Console** button opens. This is the multi-window dogfood: its own
//! `WidgetArena`, `EventRouter`, `FocusManager`, `SurfaceWrapper`, and
//! `RenderOrchestrator`, sharing the primary window's `GpuContext`
//! (`instance`/`device`/`queue` are `Arc`s by design — surfaces must be
//! created from the same `wgpu::Instance` to present).
//!
//! Content is a `Deck` — a `Segmented` strip over one-of-N pages:
//! the original console column (heading, `Banner`, `Disclosure`,
//! telemetry `Switch`+`ProgressBar`) plus one gallery page per main-
//! window zone mounting the `showcase` card sections for real. The
//! second window therefore exercises the same widget stack, theme
//! dictionary, shaped text pipeline, and overlay layer as the main
//! surface — and the whole 274-widget catalog.

use std::sync::Arc;
use std::time::{Duration, Instant};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite::core::{
    ColdNode, EventContext, EventResponse, HotNode, LayoutConstraints, LayoutContext, NodeFlags,
    PaintContext, Rect, Widget, WidgetArena, WidgetId,
};
use martensite::focus::FocusManager;
use martensite::prelude::Signal;
use martensite::render::PaintList;
use martensite::theme::{Theme, TokenKey};
use martensite::wgpu::{
    BackdropMode, GpuContext, OrchestratorConfig, RecoveryMachine, RenderOrchestrator,
    SurfaceWrapper,
};
use martensite::widgets::{
    Banner, Container, Disclosure, Flex, ProgressBar, Segmented, Severity, Switch, Text,
};
use martensite::window::event::{
    ime_event_for_winit, EventRouter, ModifierKeys, MouseButton as MButton, PointerEvent,
    PointerId, PointerKind, PointerState,
};
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId};

/// A `Segmented` strip over a single-page deck — the console window's
/// surface switcher. `Tabs` was considered and rejected: its `event`
/// override only forwards `PointerPressed` into the panel stack
/// (moves, releases, and wheel deltas die at the tab set), which would
/// starve the gallery `ScrollView`s of every gesture that scrolls
/// them. Here the strip and the pages are siblings, so the default
/// bounds-gated forwarding delivers every positional event to
/// whichever region it lands in, and non-positional events reach the
/// shown page first, then the strip.
///
/// Child protocol: `child(0)` is the strip, `child(1..=n)` the pages.
/// Only the selected page reports `child_bounds` — the same
/// "not presented" signal `TabPanelChild` uses — so the arena's
/// recursive tick, the paint walk, and hit-testing all skip hidden
/// pages: 274 showcase widgets cost nothing while their page is
/// parked. Every page still lays out against the shared region each
/// pass, so a page is correctly sized the frame it is selected.
struct Deck {
    /// The option strip (child 0).
    strip: Segmented,
    /// Page widgets (children 1..=n).
    pages: Vec<Box<dyn Widget>>,
    /// Shown page index — mirrors `strip.selected_index()`, synced in
    /// `tick` (the same parked-state idiom `take_selected` wraps).
    selected: usize,
    /// Strip bounds from the last layout pass.
    strip_bounds: Option<Rect>,
    /// Page region bounds from the last layout pass.
    page_bounds: Option<Rect>,
}

impl Widget for Deck {
    fn debug_name(&self) -> &'static str {
        "Console Deck"
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let strip = self.strip.measure(cx, constraints);
        let page_constraints = LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(
                constraints.max_size.x,
                (constraints.max_size.y - strip.y).max(0.0),
            ),
        };
        let mut pages = Vec2::ZERO;
        for page in &mut self.pages {
            pages = pages.max(page.measure(cx, page_constraints));
        }
        Vec2::new(
            strip
                .x
                .max(pages.x)
                .clamp(0.0, constraints.max_size.x.max(0.0)),
            (strip.y + pages.y).clamp(0.0, constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        // The strip's measured height leads — `Segmented` owns its row
        // metrics (segment padding + label), so the deck asks rather
        // than hard-coding a constant.
        let strip_h = self
            .strip
            .measure(
                cx,
                LayoutConstraints {
                    min_size: Vec2::ZERO,
                    max_size: bounds.size,
                },
            )
            .y
            .min(bounds.height());
        let strip = Rect::new(bounds.min_x(), bounds.min_y(), bounds.width(), strip_h);
        let region = Rect::new(
            bounds.min_x(),
            bounds.min_y() + strip_h,
            bounds.width(),
            (bounds.height() - strip_h).max(0.0),
        );
        self.strip_bounds = Some(strip);
        self.page_bounds = Some(region);
        cx.layout_child(&mut self.strip, strip);
        for page in &mut self.pages {
            cx.layout_child(page.as_mut(), region);
        }
    }

    fn tick(&mut self, _dt: Duration) -> bool {
        // `set_selected` (from the strip's pointer/keyboard dispatch)
        // moves `selected_index` synchronously; mirroring it once per
        // frame swaps the page the same frame the input landed without
        // a callback seam. `take_selected` is drained too so the
        // documented change seam never goes stale.
        let _ = self.strip.take_selected();
        let selected = self.strip.selected_index();
        let changed = selected != self.selected;
        self.selected = selected;
        changed
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
        node.set_label("Console surface selector");
    }

    fn clips_children(&self) -> bool {
        // Pages fill the deck; a mis-measured demo must not paint up
        // into the strip or out into the container padding.
        true
    }

    fn child_count(&self) -> usize {
        1 + self.pages.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        if index == 0 {
            Some(&self.strip)
        } else {
            self.pages.get(index - 1).map(|p| &**p as &dyn Widget)
        }
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        if index == 0 {
            Some(&mut self.strip)
        } else {
            self.pages
                .get_mut(index - 1)
                .map(|p| &mut **p as &mut dyn Widget)
        }
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        if index == 0 {
            self.strip_bounds
        } else if index - 1 == self.selected {
            self.page_bounds
        } else {
            None
        }
    }
}

/// The facade `Switch` owns its `on` flag and offers no signal-out
/// seam, so this probe mirrors the flag into `gate` after every
/// dispatch (and each tick as a catch-all for programmatic toggles) —
/// the window reads the toggle without reaching into the widget tree.
struct MirrorSwitch {
    switch: Switch,
    gate: Signal<bool>,
}

impl Widget for MirrorSwitch {
    fn debug_name(&self) -> &'static str {
        "Mirror Switch"
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        self.switch.measure(cx, constraints)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.switch.layout(cx, bounds);
    }

    fn paint(&self, cx: &mut PaintContext) {
        self.switch.paint(cx);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        let response = self.switch.event(cx);
        let _ = self.gate.set_if_changed(self.switch.on);
        response
    }

    fn tick(&mut self, dt: Duration) -> bool {
        let _ = self.gate.set_if_changed(self.switch.on);
        self.switch.tick(dt)
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        self.switch.accessibility(node);
    }
}

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
    /// The primary window's shared CPU telemetry signal — the mirror
    /// source.
    cpu: Signal<f64>,
    /// The console-local signal the `ProgressBar` actually reads;
    /// written from `cpu` each frame only while `mirror_gate` holds.
    mirror: Signal<f64>,
    /// Whether the telemetry mirror is live — driven by the console
    /// page's `Switch` through `MirrorSwitch`.
    mirror_gate: Signal<bool>,
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
                    .with_title("Martensite — Console & Gallery")
                    .with_surface_size(PhysicalSize::new(680, 560)),
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
        // The telemetry mirror: the console bar binds `mirror`, not
        // `cpu` — `render` copies `cpu` into it only while the gate
        // holds, so the switch freezes the feed instead of resetting
        // it. The gate starts `true`, matching the switch's `.on(true)`.
        let mirror = Signal::new(cpu.get());
        let mirror_gate = Signal::new(true);
        // The console page — the fixed `Flex` column this window has
        // always hosted, now one page of the deck.
        let console_page = Flex::column().gap(10.0).children([
            // Title-tier heading (spec B1): 15 pt semibold — the
            // console window's one heading gets the same tier as
            // the shell's title chrome.
            Box::new(
                Text::new("Console — secondary OS window")
                    .font_size(15.0)
                    .font_weight(martensite::core::FontWeight::SEMIBOLD),
            ) as Box<dyn Widget>,
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
            Box::new(MirrorSwitch {
                switch: Switch::new("mirror telemetry")
                    .on(true)
                    .with_text_painter(painter.clone()),
                gate: mirror_gate.clone(),
            }),
            // `bind` dogfoods reactive progress: the bar polls the
            // gated mirror signal in `tick` and dirties itself.
            Box::new(ProgressBar::new().bind(mirror.clone())),
        ]);
        // The gallery pages reuse the showcase's own section split so
        // the deck reads in the same zone order as the main window —
        // GRID/TELEMETRY/EDITOR/MEDIA map onto `grid_sections` …
        // `media_sections`. Only the shown page reports bounds, so the
        // 274-card catalog costs nothing while parked.
        let deck = Deck {
            strip: Segmented::new()
                .options(["CONSOLE", "GRID", "TELEMETRY", "EDITOR", "MEDIA"])
                .with_text_painter(painter),
            pages: vec![
                Box::new(console_page),
                Box::new(crate::showcase::gallery_page(
                    crate::showcase::grid_sections(),
                )),
                Box::new(crate::showcase::gallery_page(
                    crate::showcase::telemetry_sections(),
                )),
                Box::new(crate::showcase::gallery_page(
                    crate::showcase::editor_sections(),
                )),
                Box::new(crate::showcase::gallery_page(
                    crate::showcase::media_sections(),
                )),
            ],
            selected: 0,
            strip_bounds: None,
            page_bounds: None,
        };
        let content = Container::new().padding_uniform(16.0).child(deck);
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
            cpu,
            mirror,
            mirror_gate,
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
        // Telemetry mirror: while the console's switch holds the gate
        // open, copy the shared CPU signal into the console-local
        // `mirror` the ProgressBar polls — closed, `mirror` simply
        // stops being written and the bar freezes on its last sample
        // (a drop to zero would read as a telemetry fault, not a
        // paused mirror).
        if self.mirror_gate.get() {
            self.mirror.set(self.cpu.get());
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn deck() -> Deck {
        Deck {
            strip: Segmented::new().options(["A", "B", "C"]),
            pages: vec![
                Box::new(Text::new("page a")),
                Box::new(Text::new("page b")),
                Box::new(Text::new("page c")),
            ],
            selected: 0,
            strip_bounds: None,
            page_bounds: None,
        }
    }

    /// Before any layout pass nothing reports bounds — no child is
    /// presented, matching the `TabPanelChild` contract the arena's
    /// tick/paint/hit walks key off.
    #[test]
    fn deck_unlaid_out_presents_nothing() {
        let deck = deck();
        assert_eq!(deck.child_count(), 4);
        for i in 0..4 {
            assert!(deck.child_bounds(i).is_none());
        }
    }

    /// Only the selected page reports bounds after layout — hidden
    /// pages are skipped by the recursive tick/paint/hit walks, which
    /// is what makes parking a 274-card gallery page free.
    #[test]
    fn deck_presents_only_selected_page() {
        let mut deck = deck();
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        deck.layout(&mut cx, Rect::new(0.0, 0.0, 640.0, 480.0));
        assert!(deck.child_bounds(0).is_some(), "strip always presented");
        assert!(deck.child_bounds(1).is_some(), "page 0 selected");
        assert!(deck.child_bounds(2).is_none(), "page 1 parked");
        assert!(deck.child_bounds(3).is_none(), "page 2 parked");
    }

    /// `set_selected` on the strip lands in `selected` on the next
    /// `tick` — the same frame the dispatch ran — and reports dirty
    /// so the swap repaints.
    #[test]
    fn deck_selection_follows_strip_on_tick() {
        let mut deck = deck();
        deck.strip.set_selected(2);
        assert!(deck.tick(Duration::from_millis(16)));
        assert_eq!(deck.selected, 2);
        assert!(!deck.tick(Duration::from_millis(16)));
    }

    /// The switch probe mirrors `on` into the gate on dispatch —
    /// toggling the wrapped `Switch` flips the gate.
    #[test]
    fn mirror_switch_writes_gate_on_event() {
        let gate = Signal::new(true);
        let mut probe = MirrorSwitch {
            switch: Switch::new("mirror telemetry").on(true),
            gate: gate.clone(),
        };
        let ev = martensite::core::WidgetEvent::KeyPressed {
            key: "Enter".to_string(),
            repeat: false,
        };
        let mut cx = EventContext {
            event: &ev,
            bounds: Rect::new(0.0, 0.0, 200.0, 24.0),
            scale: 1.0,
        };
        let _ = probe.event(&mut cx);
        assert!(!gate.get());
    }
}
