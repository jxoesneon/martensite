//! Web (`wasm32-unknown-unknown`) canvas backend.
//!
//! On the web a Martensite "window" is an [`HtmlCanvasElement`] owned by
//! winit's web backend (`winit-web`). This module provides the small
//! platform glue the desktop backends get for free from the OS:
//!
//! * [`WebWindowAttributes`] — attaches a canvas to
//!   [`WindowAttributes`] via winit's `WindowAttributesWeb` platform
//!   attributes (`with_canvas`, `with_append`, `with_prevent_default`,
//!   `with_focusable`).
//! * [`window_canvas`] — retrieves the [`HtmlCanvasElement`] backing an
//!   existing winit window (`WindowExtWeb::canvas`).
//! * [`configure_web_event_loop`] / [`spawn_app`] — the web event-loop
//!   model: `ControlFlow::Poll` driven by winit's `PollStrategy`
//!   (`scheduler.postTask` with `setTimeout` fallback), and *spawn*
//!   semantics — `EventLoop::run_app` on the web registers the
//!   [`ApplicationHandler`] and returns immediately rather than blocking
//!   (there is no `EventLoopExtWeb::spawn_app` in winit 0.31; `run_app`
//!   *is* the spawn call). Redraws are driven by the browser's
//!   `requestAnimationFrame` via winit's `request_redraw`.
//! * [`sync_canvas_backing_store`] — manual backing-store DPI scaling:
//!   the canvas's `width`/`height` attributes (physical pixels) are set to
//!   `css size * devicePixelRatio` while its CSS size stays in logical
//!   pixels.
//! * [`HiddenImeInput`] — the hidden-`<input>` IME overlay: a canvas has
//!   no native IME, so composition events are captured on a visually
//!   hidden text input positioned over the insertion point (the approach
//!   egui and Makepad use).
//!
//! # COOP/COEP
//!
//! `Cross-Origin-Opener-Policy: same-origin` and
//! `Cross-Origin-Embedder-Policy: require-corp` are **not required** for
//! this backend: they only gate `crossOriginIsolated` (SharedArrayBuffer /
//! wasm threads), which Martensite does not use on the web — everything
//! here is single-threaded. They are documented because a future
//! multithreaded wasm build would need the serving page to send both
//! headers, and because COEP `require-corp` affects which cross-origin
//! resources (e.g. fetched font files) the page may load.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CompositionEvent, HtmlCanvasElement, HtmlInputElement};
use winit::window::WindowAttributes;

/// Error type for web window operations.
///
/// Wraps a DOM/`wasm-bindgen` failure as a plain message so it stays
/// `Send`-free and cheap on the single-threaded wasm target.
///
/// # Examples
///
/// ```
/// use martensite_window::web::WebError;
///
/// let err = WebError::new("no DOM");
/// assert!(err.to_string().contains("no DOM"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebError(String);

impl WebError {
    /// Creates a [`WebError`] from a message.
    #[must_use]
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }

    /// Creates a [`WebError`] from a `JsValue` produced by a failed DOM
    /// call, preserving the JS error message when present.
    #[must_use]
    pub fn from_js(value: &JsValue) -> Self {
        if let Some(message) = value.as_string() {
            return Self(message);
        }
        if let Some(error) = value.dyn_ref::<js_sys::Error>() {
            return Self(error.message().into());
        }
        Self(format!("{value:?}"))
    }
}

impl std::fmt::Display for WebError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "web window error: {}", self.0)
    }
}

impl std::error::Error for WebError {}

/// Returns the global `window.document`, or a [`WebError`] when no DOM
/// context exists (e.g. running inside a worker without `document`).
fn document() -> Result<web_sys::Document, WebError> {
    let window = web_sys::window().ok_or_else(|| WebError::new("no `window` object"))?;
    window
        .document()
        .ok_or_else(|| WebError::new("no `document` object"))
}

/// Web-specific window attributes: canvas binding plus winit-web's
/// prevent-default / focusable / append flags.
///
/// Apply to a [`WindowAttributes`] via [`WebWindowAttributes::apply`],
/// which boxes them into winit's `platform` attribute slot. On the web,
/// winit uses exactly one canvas per window; if no canvas is supplied,
/// winit creates one and (with [`with_append`](Self::with_append))
/// appends it to the document.
///
/// # Examples
///
/// ```no_run
/// use martensite_window::web::WebWindowAttributes;
/// use winit::window::WindowAttributes;
///
/// let attrs = WebWindowAttributes::new()
///     .with_append(true)
///     .apply(WindowAttributes::default().with_title("Martensite"));
/// ```
#[derive(Clone, Debug, Default)]
pub struct WebWindowAttributes {
    canvas: Option<HtmlCanvasElement>,
    prevent_default: Option<bool>,
    focusable: Option<bool>,
    append: Option<bool>,
}

impl WebWindowAttributes {
    /// Creates default web window attributes (no canvas bound; winit
    /// creates one at window creation).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_window::web::WebWindowAttributes;
    ///
    /// let attrs = WebWindowAttributes::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds an existing [`HtmlCanvasElement`] to the window.
    ///
    /// When `None` (the default), winit creates a fresh canvas — it is
    /// then the caller's responsibility to insert it into the page, or to
    /// use [`with_append`](Self::with_append).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_window::web::WebWindowAttributes;
    /// # fn get_canvas() -> Option<web_sys::HtmlCanvasElement> { None }
    /// let attrs = WebWindowAttributes::new().with_canvas(get_canvas());
    /// ```
    #[must_use]
    pub fn with_canvas(mut self, canvas: Option<HtmlCanvasElement>) -> Self {
        self.canvas = canvas;
        self
    }

    /// Sets whether winit calls `event.preventDefault()` on canvas events
    /// that have side effects (e.g. mouse-wheel scrolling the page).
    /// Enabled by default in winit.
    #[must_use]
    pub fn with_prevent_default(mut self, prevent_default: bool) -> Self {
        self.prevent_default = Some(prevent_default);
        self
    }

    /// Sets whether the canvas is keyboard-focusable (`tabindex`). Needed
    /// for canvas keyboard input; enabled by default in winit.
    #[must_use]
    pub fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = Some(focusable);
        self
    }

    /// Sets whether winit appends the canvas to the document body at
    /// window creation when it is not already in the page.
    /// Disabled by default in winit.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_window::web::WebWindowAttributes;
    ///
    /// let attrs = WebWindowAttributes::new().with_append(true);
    /// ```
    #[must_use]
    pub fn with_append(mut self, append: bool) -> Self {
        self.append = Some(append);
        self
    }

    /// Applies these web attributes to `attrs`, returning the updated
    /// [`WindowAttributes`] ready for
    /// [`WindowManager::create_window`](crate::WindowManager::create_window).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_window::web::WebWindowAttributes;
    /// use winit::window::WindowAttributes;
    ///
    /// let attrs = WebWindowAttributes::new()
    ///     .with_append(true)
    ///     .apply(WindowAttributes::default());
    /// ```
    #[must_use]
    pub fn apply(self, attrs: WindowAttributes) -> WindowAttributes {
        let mut web = winit::platform::web::WindowAttributesWeb::default();
        if let Some(canvas) = self.canvas {
            web = web.with_canvas(Some(canvas));
        }
        if let Some(prevent_default) = self.prevent_default {
            web = web.with_prevent_default(prevent_default);
        }
        if let Some(focusable) = self.focusable {
            web = web.with_focusable(focusable);
        }
        if let Some(append) = self.append {
            web = web.with_append(append);
        }
        attrs.with_platform_attributes(Box::new(web))
    }
}

/// Returns the [`HtmlCanvasElement`] backing `window`, if called on the
/// window's thread (the main thread).
///
/// Thin wrapper over `winit::platform::web::WindowExtWeb::canvas`; the
/// element is cloned out of winit's `Ref` guard so callers can hold it.
///
/// # Examples
///
/// ```no_run
/// # fn example(window: &dyn winit::window::Window) {
/// if let Some(canvas) = martensite_window::web::window_canvas(window) {
///     let _ = canvas.width();
/// }
/// # }
/// ```
#[must_use]
pub fn window_canvas(window: &dyn winit::window::Window) -> Option<HtmlCanvasElement> {
    use winit::platform::web::WindowExtWeb;
    // `canvas()` returns `Ref<'_, HtmlCanvasElement>`; `canvas.clone()`
    // would resolve to `Ref::clone` (the inherent method on the guard),
    // so deref first to clone the DOM element itself.
    window.canvas().map(|canvas| (*canvas).clone())
}

/// Configures `event_loop` for the web render model.
///
/// Sets [`ControlFlow::Poll`] so the loop reschedules continuously and
/// selects winit's [`PollStrategy::Scheduler`]
/// (`scheduler.postTask`, falling back to `setTimeout`) — the lowest-
/// latency strategy that still yields to browser input. Frame production
/// itself stays vsync-aligned because `Window::request_redraw` maps to
/// `requestAnimationFrame` on the web backend.
///
/// Call once before [`spawn_app`].
///
/// # Examples
///
/// ```no_run
/// # fn example(event_loop: &winit::event_loop::EventLoop) {
/// martensite_window::web::configure_web_event_loop(event_loop);
/// # }
/// ```
pub fn configure_web_event_loop(event_loop: &winit::event_loop::EventLoop) {
    use winit::event_loop::ControlFlow;
    use winit::platform::web::{EventLoopExtWeb, PollStrategy};

    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.set_poll_strategy(PollStrategy::Scheduler);
}

/// Spawns `app` on `event_loop`, consuming both.
///
/// On the web this does **not** block: winit's `run_app` registers the
/// [`ApplicationHandler`] with the browser's scheduler and returns
/// `Ok(())` immediately — the event loop keeps running after `main`
/// returns. This is the winit 0.31 equivalent of the older
/// `EventLoopExtWeb::spawn_app`.
///
/// # Errors
///
/// Propagates [`winit::error::EventLoopError`] if the handler cannot be
/// registered.
///
/// # Examples
///
/// ```no_run
/// use martensite_window::web::spawn_app;
/// use winit::application::ApplicationHandler;
/// use winit::event_loop::EventLoop;
///
/// struct App;
/// impl ApplicationHandler for App {
///     fn can_create_surfaces(&mut self, _event_loop: &dyn winit::event_loop::ActiveEventLoop) {}
/// }
///
/// let event_loop = EventLoop::new().expect("event loop");
/// spawn_app(event_loop, App).expect("app spawned");
/// // `run_app` has already returned; the browser drives the loop now.
/// ```
pub fn spawn_app(
    event_loop: winit::event_loop::EventLoop,
    app: impl winit::application::ApplicationHandler + 'static,
) -> Result<(), winit::error::EventLoopError> {
    event_loop.run_app(app)
}

/// Sizes a canvas's backing store to `logical * scale_factor` physical
/// pixels while keeping the CSS size in logical pixels.
///
/// The browser does not scale a `<canvas>`'s backing store
/// automatically: `canvas.width`/`height` are device pixels while
/// `style.width`/`height` are CSS pixels. winit reports sizes in logical
/// units and `devicePixelRatio` as the scale factor; this helper performs
/// the manual conversion so rendered content stays sharp on HiDPI
/// displays.
///
/// Values are rounded to the nearest physical pixel and clamped to ≥ 1.
///
/// # Examples
///
/// ```no_run
/// # fn example(canvas: &web_sys::HtmlCanvasElement) {
/// // A 400×300 CSS-pixel canvas on a 2× display gets an 800×600
/// // backing store.
/// martensite_window::web::sync_canvas_backing_store(canvas, 400, 300, 2.0);
/// assert_eq!(canvas.width(), 800);
/// assert_eq!(canvas.height(), 600);
/// # }
/// ```
pub fn sync_canvas_backing_store(
    canvas: &HtmlCanvasElement,
    logical_width: u32,
    logical_height: u32,
    scale_factor: f64,
) {
    let physical_width = (f64::from(logical_width) * scale_factor).round().max(1.0) as u32;
    let physical_height = (f64::from(logical_height) * scale_factor).round().max(1.0) as u32;
    canvas.set_width(physical_width);
    canvas.set_height(physical_height);
    let style = canvas.style();
    // CSS size stays in logical units so the browser compositor performs
    // the physical→CSS downscale.
    let _ = style.set_property("width", &format!("{logical_width}px"));
    let _ = style.set_property("height", &format!("{logical_height}px"));
}

/// An IME composition event captured by [`HiddenImeInput`].
///
/// Mirrors the DOM `compositionstart`/`compositionupdate`/`compositionend`
/// sequence; `Text` is the non-composition `input` event's inserted data
/// (single-character input on browsers that don't run a composition).
///
/// # Examples
///
/// ```
/// use martensite_window::web::ImeEvent;
///
/// let event = ImeEvent::CompositionEnd("日本語".to_string());
/// assert!(matches!(event, ImeEvent::CompositionEnd(_)));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImeEvent {
    /// A composition session began (`compositionstart`).
    CompositionStart,
    /// The in-flight composition string changed (`compositionupdate`).
    CompositionUpdate(String),
    /// The composition committed or was cancelled (`compositionend`); the
    /// string is the final composed text.
    CompositionEnd(String),
    /// A non-composition text insertion (`input` event data), e.g.
    /// autocorrect/keyboard text arriving outside a composition.
    InsertText(String),
}

/// Shared [`ImeEvent`] handler slot: DOM closures dispatch into it, and
/// [`HiddenImeInput::set_on_ime`] swaps the boxed callable.
///
/// `Option` so dispatch can take the handler out before invoking it —
/// the callback then runs with no borrow held and may call
/// `set_on_ime` (or trigger DOM events re-entering a sibling listener)
/// without a `RefCell` double-borrow panic.
type ImeHandler = std::rc::Rc<std::cell::RefCell<Option<Box<dyn FnMut(ImeEvent)>>>>;

/// The retained DOM listener set: `Closure`s must be owned for as long
/// as their listeners are registered.
type EventClosures = Vec<Closure<dyn FnMut(web_sys::Event)>>;

/// A hidden `<input>` element that captures IME composition for a canvas.
///
/// A `<canvas>` cannot host an IME, so web front-ends (egui, Makepad)
/// overlay a visually hidden but focusable text input on the insertion
/// point and read `composition*` events from it. This type manages that
/// overlay:
///
/// * [`HiddenImeInput::new`] creates the `<input>` styled
///   `position: absolute; opacity: 0; pointer-events: none` — invisible
///   and click-through but still focusable and IME-eligible — and appends
///   it to the given parent element.
/// * [`set_position`](Self::set_position) moves the input over the text
///   insertion point so the candidate window tracks the caret.
/// * [`focus`](Self::focus) / [`blur`](Self::blur) transfer keyboard focus
///   between the canvas and the overlay when a text field gains/loses
///   logical focus.
/// * [`set_on_ime`](Self::set_on_ime) installs the handler receiving
///   [`ImeEvent`]s.
///
/// The type intentionally does not attempt to convert DOM events into
/// `winit::event::Ime`: the web event model delivers composition on the
/// DOM element, not through winit's window event stream. Callers forward
/// [`ImeEvent`]s into their own text-input pipeline (e.g.
/// `martensite_text::ime`).
///
/// # Known caveats
///
/// * While the overlay holds DOM focus, raw `keydown`/`keyup` events
///   land on the `<input>` and are **not** forwarded — non-composition
///   keys (arrows, shortcuts) are invisible to the app until
///   [`blur`](Self::blur) returns focus to the canvas.
/// * `input` events whose `data` is null — paste/drop insertion,
///   deletions, undo/redo — are suppressed rather than reported; see the
///   `input` listener in [`new`](Self::new).
///
/// # Examples
///
/// ```no_run
/// use martensite_window::web::HiddenImeInput;
///
/// # fn example() -> Result<(), martensite_window::web::WebError> {
/// let document = web_sys::window().unwrap().document().unwrap();
/// let body = document.body().unwrap().unchecked_into::<web_sys::Element>();
/// let ime = HiddenImeInput::new(&body)?;
/// ime.set_position(100.0, 50.0, 20.0);
/// ime.set_on_ime(|event| {
///     // forward `ImeEvent` into the text pipeline
///     let _ = event;
/// });
/// ime.focus();
/// # Ok(())
/// # }
/// ```
pub struct HiddenImeInput {
    input: HtmlInputElement,
    handler: ImeHandler,
    // Held for the lifetime of `self` so the DOM listeners stay alive.
    _closures: EventClosures,
}

impl std::fmt::Debug for HiddenImeInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HiddenImeInput").finish_non_exhaustive()
    }
}

impl HiddenImeInput {
    /// Creates a hidden IME `<input>` appended to `parent`.
    ///
    /// # Errors
    ///
    /// Returns [`WebError`] if there is no DOM document or element
    /// creation/listener registration fails.
    pub fn new(parent: &web_sys::Element) -> Result<Self, WebError> {
        let document = document()?;
        let input: HtmlInputElement = document
            .create_element("input")
            .map_err(|e| WebError::from_js(&e))?
            .unchecked_into();

        // Visually hidden but focusable: `display: none` or `visibility:
        // hidden` would prevent IME attachment entirely; a fully
        // transparent absolutely-positioned input is the standard trick.
        let style = input.style();
        let _ = style.set_property("position", "absolute");
        let _ = style.set_property("opacity", "0");
        let _ = style.set_property("pointer-events", "none");
        let _ = style.set_property("z-index", "-1");
        let _ = style.set_property("padding", "0");
        let _ = style.set_property("border", "none");
        let _ = style.set_property("background", "transparent");
        // Keep the input out of sequential tab order; focus is managed
        // programmatically via `focus()`.
        input.set_tab_index(-1);
        input
            .set_attribute("autocapitalize", "off")
            .map_err(|e| WebError::from_js(&e))?;
        input
            .set_attribute("autocomplete", "off")
            .map_err(|e| WebError::from_js(&e))?;
        input
            .set_attribute("autocorrect", "off")
            .map_err(|e| WebError::from_js(&e))?;
        input
            .set_attribute("spellcheck", "false")
            .map_err(|e| WebError::from_js(&e))?;
        // `aria-hidden` is deliberate: the accessibility mirror lives in
        // martensite-access's web bridge, not in this raw input.
        input
            .set_attribute("aria-hidden", "true")
            .map_err(|e| WebError::from_js(&e))?;

        let handler: ImeHandler = std::rc::Rc::new(std::cell::RefCell::new(None));

        let mut closures: EventClosures = Vec::new();

        let listen = |input: &HtmlInputElement,
                      kind: &'static str,
                      handler: &ImeHandler,
                      map: fn(&web_sys::Event) -> Option<ImeEvent>,
                      closures: &mut EventClosures|
         -> Result<(), WebError> {
            let handler = std::rc::Rc::clone(handler);
            let closure = Closure::wrap(Box::new(move |event: web_sys::Event| {
                if let Some(mapped) = map(&event) {
                    // Take-and-reinstall: the handler runs with no borrow
                    // held, so it may call `set_on_ime` without a
                    // double-borrow panic. An event arriving while the
                    // slot is empty (mid-dispatch reentrancy) is dropped.
                    let mut cb = handler.borrow_mut().take();
                    if let Some(f) = cb.as_mut() {
                        f(mapped);
                    }
                    let mut slot = handler.borrow_mut();
                    if slot.is_none() {
                        *slot = cb;
                    }
                }
            }) as Box<dyn FnMut(web_sys::Event)>);
            input
                .add_event_listener_with_callback(kind, closure.as_ref().unchecked_ref())
                .map_err(|e| WebError::from_js(&e))?;
            closures.push(closure);
            Ok(())
        };

        listen(
            &input,
            "compositionstart",
            &handler,
            |_| Some(ImeEvent::CompositionStart),
            &mut closures,
        )?;
        listen(
            &input,
            "compositionupdate",
            &handler,
            |event| {
                // An empty `data` here is a real update — the composition
                // string was deleted back to empty — so it is forwarded
                // (unlike the suppressed composition-phase `input`
                // events below).
                Some(ImeEvent::CompositionUpdate(
                    event
                        .dyn_ref::<CompositionEvent>()
                        .and_then(|e| e.data())
                        .unwrap_or_default(),
                ))
            },
            &mut closures,
        )?;
        listen(
            &input,
            "compositionend",
            &handler,
            |event| {
                Some(ImeEvent::CompositionEnd(
                    event
                        .dyn_ref::<CompositionEvent>()
                        .and_then(|e| e.data())
                        .unwrap_or_default(),
                ))
            },
            &mut closures,
        )?;
        listen(
            &input,
            "input",
            &handler,
            |event| {
                let input_event = event.dyn_ref::<web_sys::InputEvent>()?;
                // `input` fires during composition too; those reports are
                // already covered by compositionupdate. Suppress them
                // entirely rather than emitting a synthetic empty
                // `CompositionUpdate`, which would flicker/clear the
                // composition preview in consumers.
                if input_event.is_composing() {
                    return None;
                }
                // `data` is null for non-insert inputs
                // (`insertFromPaste`/`insertFromDrop`, `deleteContent*`,
                // `historyUndo`/`historyRedo`, …). Paste into the hidden
                // input is deliberately not forwarded — canvas apps take
                // paste through `navigator.clipboard` — and deletes on
                // the always-empty input carry no payload, so both are
                // dropped instead of surfacing a spurious empty
                // `InsertText`.
                match input_event.data() {
                    Some(data) if !data.is_empty() => Some(ImeEvent::InsertText(data)),
                    _ => None,
                }
            },
            &mut closures,
        )?;

        parent
            .append_child(&input)
            .map_err(|e| WebError::from_js(&e))?;

        Ok(Self {
            input,
            handler,
            _closures: closures,
        })
    }

    /// Installs the handler invoked for every [`ImeEvent`].
    ///
    /// Events dispatched before the first `set_on_ime` are dropped. The
    /// handler runs with no internal borrow held, so it may call
    /// `set_on_ime` again safely.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(ime: &martensite_window::web::HiddenImeInput) {
    /// ime.set_on_ime(|event| { let _ = event; });
    /// # }
    /// ```
    pub fn set_on_ime(&self, handler: impl FnMut(ImeEvent) + 'static) {
        *self.handler.borrow_mut() = Some(Box::new(handler));
    }

    /// Moves the hidden input over the insertion point.
    ///
    /// `x`/`y` are CSS-pixel coordinates relative to the input's
    /// containing block (usually the page); `height` sets the input's
    /// line height so the browser's IME candidate window anchors to the
    /// caret line.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(ime: &martensite_window::web::HiddenImeInput) {
    /// ime.set_position(120.0, 340.0, 18.0);
    /// # }
    /// ```
    pub fn set_position(&self, x: f64, y: f64, height: f64) {
        let style = self.input.style();
        let _ = style.set_property("left", &format!("{x}px"));
        let _ = style.set_property("top", &format!("{y}px"));
        let _ = style.set_property("height", &format!("{}px", height.max(1.0)));
        let _ = style.set_property("line-height", &format!("{}px", height.max(1.0)));
    }

    /// Moves keyboard focus to the hidden input so the IME targets it.
    ///
    /// Call when a text field gains logical focus. The canvas regains
    /// focus via [`blur`](Self::blur) when editing ends.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(ime: &martensite_window::web::HiddenImeInput) {
    /// ime.focus();
    /// # }
    /// ```
    pub fn focus(&self) {
        self.input.focus().ok();
    }

    /// Removes focus from the hidden input (e.g. on `blur`/cancel), so
    /// the IME detaches and the canvas can retake keyboard focus.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(ime: &martensite_window::web::HiddenImeInput) {
    /// ime.blur();
    /// # }
    /// ```
    pub fn blur(&self) {
        self.input.blur().ok();
    }

    /// Returns the underlying `<input>` element for callers that need
    /// direct DOM access (e.g. setting `value` to seed an in-progress
    /// edit).
    #[must_use]
    pub fn element(&self) -> &HtmlInputElement {
        &self.input
    }
}

impl Drop for HiddenImeInput {
    fn drop(&mut self) {
        self.input.remove();
    }
}
