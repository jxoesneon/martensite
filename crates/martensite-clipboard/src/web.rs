//! Web (`wasm32-unknown-unknown`) clipboard backend via
//! `navigator.clipboard`.
//!
//! The [Async Clipboard API](https://www.w3.org/TR/clipboard-apis/) is
//! promise-based and doubly gated:
//!
//! * **Secure context** — `navigator.clipboard` only exists on HTTPS /
//!   `localhost` pages ([`Window::is_secure_context`]). In insecure
//!   contexts [`WebClipboard`] behaves like [`StubClipboard`]: writes are
//!   remembered in-process but nothing reaches the browser clipboard.
//! * **User gesture / permissions** — `readText` requires the
//!   `clipboard-read` permission, which browsers grant only inside a
//!   transient user-gesture (click, keypress). Calls outside a gesture
//!   reject; the error surfaces as [`WebClipboardError::Rejected`].
//!
//! # Sync/async impedance
//!
//! [`ClipboardService`] is synchronous but the browser API is not. The
//! bridge:
//!
//! * `set_contents` records the item in an in-process cache **and**
//!   schedules a `writeText` for the `text/plain` representation
//!   (browsers only accept text/HTML writes from a gesture anyway — the
//!   write is attempted, failure is traced, never fatal). Writes are
//!   funneled through a FIFO queue drained by a single task so two
//!   fire-and-forget `writeText` promises can never resolve out of order
//!   and leave stale text on the OS clipboard.
//! * `get_contents`/`available_types` serve from the cache — the browser
//!   does not allow enumerating clipboard contents synchronously.
//! * Real reads go through the inherent async methods
//!   [`WebClipboard::read_text`] / [`WebClipboard::write_text`], which the
//!   caller should drive inside a user-gesture handler.
//!
//! # Examples
//!
//! ```no_run
//! use martensite_clipboard::{ClipboardService, PlatformClipboard};
//! use martensite_clipboard::web::WebClipboard;
//!
//! let mut cb = WebClipboard::new();
//! // The name advertises the backend regardless of availability.
//! assert_eq!(cb.platform_name(), "web-navigator-clipboard");
//! cb.set_contents(&martensite_clipboard::ClipboardItem::new().offer_text("hi"));
//! ```

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::clipboard::{ClipboardItem, ClipboardService, MIME_TEXT_PLAIN};
use crate::platform::PlatformClipboard;

/// Error type for [`WebClipboard`]'s async methods.
///
/// # Examples
///
/// ```
/// use martensite_clipboard::web::WebClipboardError;
///
/// let err = WebClipboardError::Rejected("denied".to_string());
/// assert!(err.to_string().contains("denied"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebClipboardError {
    /// `navigator.clipboard` is unavailable (insecure context or
    /// unsupported browser).
    Unavailable,
    /// The clipboard promise rejected — typically a missing user gesture
    /// or denied `clipboard-read`/`clipboard-write` permission.
    Rejected(String),
}

impl std::fmt::Display for WebClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => {
                write!(f, "navigator.clipboard is unavailable (insecure context)")
            }
            Self::Rejected(msg) => write!(f, "clipboard operation rejected: {msg}"),
        }
    }
}

impl std::error::Error for WebClipboardError {}

/// Returns the page's `navigator.clipboard` handle when the API is
/// actually present: secure context *and* the property defined on
/// `navigator`.
fn browser_clipboard() -> Option<web_sys::Clipboard> {
    let window = web_sys::window()?;
    if !window.is_secure_context() {
        return None;
    }
    let navigator = window.navigator();
    // `navigator.clipboard` is undefined in insecure contexts and in
    // browsers without the API; Reflect checks presence without throwing.
    let has_clipboard = js_sys::Reflect::get(&navigator, &"clipboard".into())
        .map(|v| !v.is_undefined() && !v.is_null())
        .unwrap_or(false);
    has_clipboard.then(|| navigator.clipboard())
}

/// Clipboard backend bridging [`ClipboardService`] to
/// `navigator.clipboard`.
///
/// See the [module docs](self) for the sync/async bridging model and the
/// secure-context/user-gesture gates.
#[derive(Debug)]
pub struct WebClipboard {
    /// The browser clipboard handle, or `None` in insecure contexts.
    clipboard: Option<web_sys::Clipboard>,
    /// In-process record of the last `set_contents`: the sync
    /// `ClipboardService` read path serves from here because the browser
    /// cannot be queried synchronously.
    cache: HashMap<String, Vec<u8>>,
    /// FIFO of `writeText` payloads awaiting the browser. Drained
    /// sequentially by one task so `set_contents`/`clear` writes cannot
    /// race and resolve out of order.
    write_queue: Rc<RefCell<VecDeque<String>>>,
    /// `true` while a drain task is awaiting a `writeText` promise.
    write_draining: Rc<Cell<bool>>,
}

impl Default for WebClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl WebClipboard {
    /// Creates a [`WebClipboard`], probing for `navigator.clipboard`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_clipboard::web::WebClipboard;
    ///
    /// let cb = WebClipboard::new();
    /// let _ = cb.is_available();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            clipboard: browser_clipboard(),
            cache: HashMap::new(),
            write_queue: Rc::new(RefCell::new(VecDeque::new())),
            write_draining: Rc::new(Cell::new(false)),
        }
    }

    /// Returns `true` when `navigator.clipboard` is present (secure
    /// context + API support). Reads/writes may still be rejected without
    /// a user gesture or permission grant.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use martensite_clipboard::web::WebClipboard;
    ///
    /// let cb = WebClipboard::new();
    /// let _ = cb.is_available();
    /// ```
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.clipboard.is_some()
    }

    /// Reads `text/plain` from the browser clipboard.
    ///
    /// Must be called from within a transient user-gesture handler (or
    /// after `clipboard-read` permission was granted); otherwise the
    /// promise rejects with `NotAllowedError`.
    ///
    /// # Errors
    ///
    /// [`WebClipboardError::Unavailable`] when the API is absent,
    /// [`WebClipboardError::Rejected`] when the promise rejects.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// async fn paste() {
    ///     let cb = martensite_clipboard::web::WebClipboard::new();
    ///     let text = cb.read_text().await;
    ///     let _ = text;
    /// }
    /// ```
    pub async fn read_text(&self) -> Result<String, WebClipboardError> {
        let clipboard = self
            .clipboard
            .as_ref()
            .ok_or(WebClipboardError::Unavailable)?;
        let value = JsFuture::from(clipboard.read_text())
            .await
            .map_err(|e| WebClipboardError::Rejected(js_err(&e)))?;
        Ok(value.as_string().unwrap_or_default())
    }

    /// Writes `text` to the browser clipboard as `text/plain`.
    ///
    /// The write is allowed inside transient user gestures (and, on most
    /// browsers, requires the document to be focused).
    ///
    /// This caller-driven write intentionally bypasses the FIFO queue
    /// that serializes the implicit `set_contents`/`clear` writes: it is
    /// awaited inside the caller's gesture, so no ordering guarantee
    /// relative to queued fire-and-forget writes is provided or needed.
    ///
    /// # Errors
    ///
    /// [`WebClipboardError::Unavailable`] when the API is absent,
    /// [`WebClipboardError::Rejected`] when the promise rejects.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// async fn copy() {
    ///     let cb = martensite_clipboard::web::WebClipboard::new();
    ///     let result = cb.write_text("hello").await;
    ///     let _ = result;
    /// }
    /// ```
    pub async fn write_text(&self, text: &str) -> Result<(), WebClipboardError> {
        let clipboard = self
            .clipboard
            .as_ref()
            .ok_or(WebClipboardError::Unavailable)?;
        JsFuture::from(clipboard.write_text(text))
            .await
            .map_err(|e| WebClipboardError::Rejected(js_err(&e)))?;
        Ok(())
    }

    /// Enqueues `text` for a fire-and-forget `writeText`.
    ///
    /// A single spawned task drains the queue in order; without this,
    /// two overlapping `writeText` promises could resolve out of order
    /// and leave an older payload on the OS clipboard. Rejections are
    /// traced, never fatal.
    fn enqueue_write(&self, text: String) {
        let Some(clipboard) = self.clipboard.clone() else {
            return;
        };
        self.write_queue.borrow_mut().push_back(text);
        if self.write_draining.replace(true) {
            // A drain task is already running and will pick the entry up.
            return;
        }
        let queue = Rc::clone(&self.write_queue);
        let draining = Rc::clone(&self.write_draining);
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                // The borrow ends before `.await` — wasm is
                // single-threaded, so no other drain runs concurrently.
                let next = queue.borrow_mut().pop_front();
                let Some(text) = next else { break };
                if let Err(e) = JsFuture::from(clipboard.write_text(&text)).await {
                    tracing::debug!("navigator.clipboard.write_text rejected: {e:?}");
                }
            }
            draining.set(false);
        });
    }
}

impl ClipboardService for WebClipboard {
    fn set_contents(&mut self, item: &ClipboardItem) {
        self.cache.clear();
        let mut text: Option<String> = None;
        for (mime, payload) in item.iter() {
            if let Some(bytes) = payload.with_deadline(crate::clipboard::DEFAULT_LAZY_DEADLINE) {
                if mime.as_str() == MIME_TEXT_PLAIN {
                    text = String::from_utf8(bytes.clone()).ok();
                }
                self.cache.insert(mime.to_string(), bytes);
            }
        }
        // Fire-and-forget the browser write for the text representation.
        // `writeText` outside a gesture rejects — that is expected (the
        // cache still records the contents) and only traced, not fatal.
        if let Some(text) = text {
            self.enqueue_write(text);
        }
    }

    fn get_contents(&self, mime: &str) -> Option<Vec<u8>> {
        self.cache.get(mime).cloned()
    }

    fn available_types(&self) -> Vec<String> {
        self.cache.keys().cloned().collect()
    }

    fn clear(&mut self) {
        self.cache.clear();
        // The Async Clipboard API has no real "clear"; writing the empty
        // string is the closest equivalent and matches the desktop
        // `clear()` contract. It typically rejects outside a transient
        // user gesture — the guaranteed effect is the cache clear. The
        // write goes through the same serialized queue so a pending
        // `set_contents` write cannot resolve *after* the clear.
        self.enqueue_write(String::new());
    }
}

impl PlatformClipboard for WebClipboard {
    #[inline]
    fn platform_name(&self) -> &str {
        "web-navigator-clipboard"
    }
}

fn js_err(value: &wasm_bindgen::JsValue) -> String {
    value
        .as_string()
        .or_else(|| value.dyn_ref::<js_sys::Error>().map(|e| e.message().into()))
        .unwrap_or_else(|| format!("{value:?}"))
}
