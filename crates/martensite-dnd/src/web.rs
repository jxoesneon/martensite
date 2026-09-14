//! Web (`wasm32-unknown-unknown`) drag-and-drop backend via the HTML5
//! `DataTransfer` API.
//!
//! winit's web backend does not wire DOM drag events into its
//! data-transfer events, so this module attaches `dragenter`/`dragover`/
//! `dragleave`/`drop` listeners directly to a DOM element (typically the
//! window canvas) and translates them into the crate's normalized
//! [`DropInput`] stream driving a [`DropBridge`].
//!
//! # Mapping
//!
//! * `dragenter` → [`DropInput::Entered`] with `dataTransfer.types` as
//!   `available_types`.
//! * `dragover` → [`DropInput::Moved`]. The handler calls
//!   `preventDefault()` (required by HTML5 for the element to be a valid
//!   drop target) and writes the negotiated [`DropEffect`] back to
//!   `dataTransfer.dropEffect` so the browser shows the right cursor.
//! * `drop` → [`DropInput::Dropped`]. Because `getData`/`files` are only
//!   readable inside the drop event, payloads are **captured eagerly**
//!   into a [`DropCapture`] keyed by a [`PlatformTransferId`] serial;
//!   retrieve them with [`WebDropListener::take_drop`].
//! * `dragleave` → [`DropInput::Left`].
//!
//! Drag *source* (outgoing drags) is not implemented for v0.17.0: HTML5
//! drag sources require `draggable` DOM elements and `dragstart` events,
//! which do not map onto a canvas-rendered UI. In-app drags continue to
//! work through [`DndSession`](crate::DndSession) directly.
//!
//! # Examples
//!
//! ```no_run
//! use martensite_dnd::web::WebDropListener;
//!
//! # fn example(canvas: &web_sys::HtmlElement) -> Result<(), martensite_dnd::web::WebDndError> {
//! let listener = WebDropListener::attach(canvas)?;
//! listener.set_on_outcome(|outcome| {
//!     let _ = outcome;
//! });
//! # Ok(())
//! # }
//! ```

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use glam::Vec2;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{DragEvent, EventTarget};

use crate::bridge::{DropBridge, DropInput, DropOutcome};
use crate::platform::PlatformTransferId;
use crate::{DropEffect, ProposedAction};

/// Error type for [`WebDropListener`] operations.
///
/// # Examples
///
/// ```
/// use martensite_dnd::web::WebDndError;
///
/// let err = WebDndError::new("no listener");
/// assert!(err.to_string().contains("no listener"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebDndError(String);

impl WebDndError {
    /// Creates a [`WebDndError`] from a message.
    #[must_use]
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }

    /// Creates a [`WebDndError`] from a rejected DOM call.
    #[must_use]
    pub fn from_js(value: &JsValue) -> Self {
        Self(
            value
                .as_string()
                .or_else(|| value.dyn_ref::<js_sys::Error>().map(|e| e.message().into()))
                .unwrap_or_else(|| format!("{value:?}")),
        )
    }
}

impl std::fmt::Display for WebDndError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "web drag-and-drop error: {}", self.0)
    }
}

impl std::error::Error for WebDndError {}

/// The payload captured synchronously inside a `drop` event.
///
/// HTML5 only exposes `DataTransfer.getData`/`files` while the `drop`
/// handler runs, so [`WebDropListener`] snapshots them eagerly.
///
/// # Examples
///
/// ```
/// use martensite_dnd::web::DropCapture;
///
/// let capture = DropCapture::default();
/// assert!(capture.data.is_empty());
/// ```
#[derive(Default)]
pub struct DropCapture {
    /// `(mime, text)` pairs read via `DataTransfer.getData` for every
    /// string-typed format advertised by the drag.
    pub data: Vec<(String, String)>,
    /// Dropped files as `web_sys::File` handles for async reads
    /// (`file.text()`, `file.array_buffer()`).
    pub files: Vec<web_sys::File>,
}

impl std::fmt::Debug for DropCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DropCapture")
            .field("data", &self.data)
            .field("files", &self.files.len())
            .finish()
    }
}

/// The retained DOM listener set: `Closure`s must be owned for as long
/// as their listeners are registered.
type EventClosures = Vec<Closure<dyn FnMut(web_sys::Event)>>;

/// Internal shared state for the DOM listener closures.
struct Shared {
    bridge: DropBridge,
    /// Captured drop payloads keyed by transfer serial.
    captures: HashMap<i64, DropCapture>,
    /// Serial assigned to the most recent `drop`.
    last_serial: Option<i64>,
    /// Monotonic transfer-id counter.
    next_serial: Cell<i64>,
    /// User callback receiving every [`DropOutcome`].
    on_outcome: Box<dyn FnMut(DropOutcome)>,
}

/// Attaches HTML5 drag-and-drop listeners to a DOM element and drives a
/// [`DropBridge`].
///
/// Clone-cheap: all clones share the same listener set and bridge.
///
/// # Examples
///
/// ```no_run
/// use martensite_dnd::web::WebDropListener;
///
/// # fn example(canvas: &web_sys::HtmlElement) -> Result<(), martensite_dnd::web::WebDndError> {
/// let listener = WebDropListener::attach(canvas)?;
/// listener.set_on_outcome(|outcome| {
///     if outcome.accepted {
///         let _ = outcome.effect;
///     }
/// });
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct WebDropListener {
    shared: Rc<RefCell<Shared>>,
    /// The element the listeners are attached to (kept for `detach`).
    element: EventTarget,
    /// Kept alive for the lifetime of `self` so the DOM listeners stay
    /// registered.
    closures: Rc<EventClosures>,
}

impl std::fmt::Debug for WebDropListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebDropListener").finish_non_exhaustive()
    }
}

/// Maps `dataTransfer.effectAllowed` to a [`ProposedAction`].
fn proposed_action(effect_allowed: &str) -> ProposedAction {
    match effect_allowed {
        "copy" | "copyLink" | "copyMove" | "all" => ProposedAction::Copy,
        "move" | "linkMove" => ProposedAction::Move,
        "link" => ProposedAction::Link,
        _ => ProposedAction::None,
    }
}

/// Maps a negotiated [`DropEffect`] back to a `dataTransfer.dropEffect`
/// string so the browser renders the matching cursor.
fn drop_effect_str(effect: Option<DropEffect>) -> &'static str {
    match effect {
        Some(DropEffect::Copy) => "copy",
        Some(DropEffect::Move) => "move",
        Some(DropEffect::Link) => "link",
        Some(DropEffect::None) | None => "none",
    }
}

/// Extracts the advertised MIME types from a `DataTransfer`.
fn transfer_types(transfer: &web_sys::DataTransfer) -> Vec<String> {
    transfer
        .types()
        .iter()
        .filter_map(|v| v.as_string())
        .collect()
}

/// Event position in logical (CSS-pixel) coordinates.
fn event_position(event: &DragEvent) -> Vec2 {
    Vec2::new(event.client_x() as f32, event.client_y() as f32)
}

/// Captures the drop payload from `dataTransfer` inside the `drop`
/// handler — the only point where `getData`/`files` are readable.
fn capture_drop(transfer: &web_sys::DataTransfer) -> DropCapture {
    let data = transfer_types(transfer)
        .into_iter()
        .filter_map(|mime| transfer.get_data(&mime).ok().map(|text| (mime, text)))
        .collect();
    let files = transfer
        .files()
        .map(|list| (0..list.length()).filter_map(|i| list.get(i)).collect())
        .unwrap_or_default();
    DropCapture { data, files }
}

impl WebDropListener {
    /// Attaches `dragenter`/`dragover`/`dragleave`/`drop` listeners to
    /// `element` and returns the handle owning them.
    ///
    /// # Errors
    ///
    /// Returns [`WebDndError`] if listener registration fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_dnd::web::WebDropListener;
    ///
    /// # fn example(canvas: &web_sys::HtmlElement) -> Result<(), martensite_dnd::web::WebDndError> {
    /// let _listener = WebDropListener::attach(canvas)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn attach(element: &EventTarget) -> Result<Self, WebDndError> {
        let shared = Rc::new(RefCell::new(Shared {
            bridge: DropBridge::new(),
            captures: HashMap::new(),
            last_serial: None,
            next_serial: Cell::new(1),
            on_outcome: Box::new(|_| {}),
        }));

        let mut closures: EventClosures = Vec::new();

        let register = |kind: &'static str,
                        handler: Box<dyn FnMut(web_sys::Event)>,
                        closures: &mut EventClosures|
         -> Result<(), WebDndError> {
            let closure = Closure::wrap(handler);
            element
                .add_event_listener_with_callback(kind, closure.as_ref().unchecked_ref())
                .map_err(|e| WebDndError::from_js(&e))?;
            closures.push(closure);
            Ok(())
        };

        // dragenter: advertise types + initial position.
        {
            let shared = Rc::clone(&shared);
            register(
                "dragenter",
                Box::new(move |event: web_sys::Event| {
                    let Some(drag) = event.dyn_ref::<DragEvent>() else {
                        return;
                    };
                    let Some(transfer) = drag.data_transfer() else {
                        return;
                    };
                    let outcome = shared.borrow_mut().bridge.handle(DropInput::Entered {
                        available_types: transfer_types(&transfer),
                        position: Some(event_position(drag)),
                        action: proposed_action(&transfer.effect_allowed()),
                    });
                    (shared.borrow_mut().on_outcome)(outcome);
                }),
                &mut closures,
            )?;
        }

        // dragover: must preventDefault to be a valid drop target; report
        // the negotiated effect via dropEffect.
        {
            let shared = Rc::clone(&shared);
            register(
                "dragover",
                Box::new(move |event: web_sys::Event| {
                    let Some(drag) = event.dyn_ref::<DragEvent>() else {
                        return;
                    };
                    let Some(transfer) = drag.data_transfer() else {
                        return;
                    };
                    let outcome = shared.borrow_mut().bridge.handle(DropInput::Moved {
                        position: event_position(drag),
                        action: proposed_action(&transfer.effect_allowed()),
                    });
                    if outcome.accepted {
                        event.prevent_default();
                        transfer.set_drop_effect(drop_effect_str(outcome.effect));
                    } else {
                        transfer.set_drop_effect("none");
                    }
                    (shared.borrow_mut().on_outcome)(outcome);
                }),
                &mut closures,
            )?;
        }

        // drop: preventDefault + eager payload capture.
        {
            let shared = Rc::clone(&shared);
            register(
                "drop",
                Box::new(move |event: web_sys::Event| {
                    event.prevent_default();
                    let Some(drag) = event.dyn_ref::<DragEvent>() else {
                        return;
                    };
                    let Some(transfer) = drag.data_transfer() else {
                        return;
                    };
                    let mut shared_mut = shared.borrow_mut();
                    let serial = shared_mut.next_serial.get();
                    shared_mut.next_serial.set(serial + 1);
                    shared_mut.captures.insert(serial, capture_drop(&transfer));
                    shared_mut.last_serial = Some(serial);
                    let outcome = shared_mut.bridge.handle(DropInput::Dropped {
                        action: proposed_action(&transfer.effect_allowed()),
                    });
                    (shared_mut.on_outcome)(outcome);
                }),
                &mut closures,
            )?;
        }

        // dragleave: cancel.
        {
            let shared = Rc::clone(&shared);
            register(
                "dragleave",
                Box::new(move |event: web_sys::Event| {
                    let _ = event;
                    let mut shared_mut = shared.borrow_mut();
                    let outcome = shared_mut.bridge.handle(DropInput::Left);
                    (shared_mut.on_outcome)(outcome);
                }),
                &mut closures,
            )?;
        }

        Ok(Self {
            shared,
            element: element.clone(),
            closures: Rc::new(closures),
        })
    }

    /// Installs the callback invoked with each [`DropOutcome`] produced
    /// by DOM drag events.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(listener: &martensite_dnd::web::WebDropListener) {
    /// listener.set_on_outcome(|outcome| { let _ = outcome.accepted; });
    /// # }
    /// ```
    pub fn set_on_outcome(&self, callback: impl FnMut(DropOutcome) + 'static) {
        self.shared.borrow_mut().on_outcome = Box::new(callback);
    }

    /// Returns a shared handle to the internal [`DropBridge`] so callers
    /// can register [`crate::DropTarget`]s and inspect sessions.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(listener: &martensite_dnd::web::WebDropListener) {
    /// listener.with_bridge(|bridge| {
    ///     let _ = bridge.registry().len();
    /// });
    /// # }
    /// ```
    pub fn with_bridge<R>(&self, f: impl FnOnce(&mut DropBridge) -> R) -> R {
        f(&mut self.shared.borrow_mut().bridge)
    }

    /// Returns the [`PlatformTransferId`] serial of the most recent
    /// `drop`, for correlating with [`take_drop`](Self::take_drop).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(listener: &martensite_dnd::web::WebDropListener) {
    /// if let Some(id) = listener.last_transfer_id() {
    ///     let capture = listener.take_drop(id);
    ///     let _ = capture;
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn last_transfer_id(&self) -> Option<PlatformTransferId> {
        self.shared
            .borrow()
            .last_serial
            .map(PlatformTransferId::new)
    }

    /// Removes and returns the [`DropCapture`] stored under `id`, if any.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(listener: &martensite_dnd::web::WebDropListener) {
    /// let capture = listener.take_drop(martensite_dnd::PlatformTransferId::new(1));
    /// let _ = capture;
    /// # }
    /// ```
    #[must_use]
    pub fn take_drop(&self, id: PlatformTransferId) -> Option<DropCapture> {
        self.shared.borrow_mut().captures.remove(&id.into_raw())
    }

    /// Detaches all registered listeners from the element.
    ///
    /// Called automatically when the last handle is dropped; exposed for
    /// explicit teardown and idempotent.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn example(listener: &martensite_dnd::web::WebDropListener) {
    /// listener.detach();
    /// # }
    /// ```
    pub fn detach(&self) {
        for (kind, closure) in Self::KINDS.iter().zip(self.closures.iter()) {
            let _ = self
                .element
                .remove_event_listener_with_callback(kind, closure.as_ref().unchecked_ref());
        }
    }

    const KINDS: [&'static str; 4] = ["dragenter", "dragover", "drop", "dragleave"];
}

impl Drop for WebDropListener {
    fn drop(&mut self) {
        // Only the last handle removes listeners; `Rc` keeps clones
        // sharing one registration set.
        if Rc::strong_count(&self.closures) == 1 {
            for (kind, closure) in Self::KINDS.iter().zip(self.closures.iter()) {
                let _ = self
                    .element
                    .remove_event_listener_with_callback(kind, closure.as_ref().unchecked_ref());
            }
        }
    }
}

/// Reads a dropped file's full text contents.
///
/// Thin wrapper over `File.text()` → `JsFuture`, for use with
/// [`DropCapture::files`].
///
/// # Errors
///
/// Returns the JS rejection as a [`WebDndError`].
///
/// # Examples
///
/// ```no_run
/// async fn example(file: web_sys::File) {
///     let text = martensite_dnd::web::read_file_text(file).await;
///     let _ = text;
/// }
/// ```
pub async fn read_file_text(file: web_sys::File) -> Result<String, WebDndError> {
    let value = wasm_bindgen_futures::JsFuture::from(file.text())
        .await
        .map_err(|e| WebDndError::from_js(&e))?;
    Ok(value.as_string().unwrap_or_default())
}
