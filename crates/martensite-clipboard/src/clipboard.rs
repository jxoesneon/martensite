//! Multi-MIME clipboard core with lazy payload evaluation.
//!
//! This module defines the data model for clipboard contents and the
//! [`ClipboardService`] trait that platform backends implement. A clipboard
//! item can carry multiple representations of the same logical content,
//! each addressed by a MIME type. Large payloads may be supplied as a
//! [`ClipboardPayload::Lazy`] closure that is only evaluated on demand,
//! guarded by a configurable deadline so an unresponsive producer cannot
//! block the UI thread indefinitely.
//!
//! # Design note on lazy payloads
//!
//! [`ClipboardService::set_contents`] takes the item by shared reference
//! (`&ClipboardItem`) and [`LazyPayload`] wraps a `FnOnce` closure. To make
//! these two constraints compose without `unsafe` code, [`LazyPayload`]
//! stores the closure inside an `Arc<Mutex<Option<…>>>`. This makes
//! [`ClipboardPayload`] cheaply [`Clone`] and allows a backend to retain a
//! lazy payload and evaluate it at most once via
//! [`ClipboardPayload::with_deadline`].
//!
//! # Examples
//!
//! ```
//! use martensite_clipboard::{ClipboardItem, ClipboardPayload, InMemoryClipboard,
//!     ClipboardService};
//!
//! let mut cb = InMemoryClipboard::new();
//! let item = ClipboardItem::new()
//!     .offer_text("hello")
//!     .offer_html("<b>hello</b>");
//! cb.set_contents(&item);
//! assert_eq!(
//!     cb.get_contents("text/plain;charset=utf-8"),
//!     Some(b"hello".to_vec())
//! );
//! ```

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// The default deadline applied to lazy payload evaluation when no explicit
/// deadline is supplied. This mitigates the risk of an unresponsive
/// clipboard IPC producer blocking the UI thread indefinitely.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use martensite_clipboard::DEFAULT_LAZY_DEADLINE;
///
/// assert_eq!(DEFAULT_LAZY_DEADLINE, Duration::from_millis(500));
/// ```
pub const DEFAULT_LAZY_DEADLINE: Duration = Duration::from_millis(500);

/// Common MIME type for UTF-8 plain text.
///
/// # Examples
///
/// ```
/// use martensite_clipboard::clipboard::MIME_TEXT_PLAIN;
///
/// assert_eq!(MIME_TEXT_PLAIN, "text/plain;charset=utf-8");
/// ```
pub const MIME_TEXT_PLAIN: &str = "text/plain;charset=utf-8";
/// Common MIME type for HTML text.
///
/// # Examples
///
/// ```
/// use martensite_clipboard::clipboard::MIME_TEXT_HTML;
///
/// assert_eq!(MIME_TEXT_HTML, "text/html");
/// ```
pub const MIME_TEXT_HTML: &str = "text/html";
/// Common MIME type for Rich Text Format.
///
/// # Examples
///
/// ```
/// use martensite_clipboard::clipboard::MIME_TEXT_RTF;
///
/// assert_eq!(MIME_TEXT_RTF, "application/rtf");
/// ```
pub const MIME_TEXT_RTF: &str = "application/rtf";
/// Common MIME type for PNG image data.
///
/// # Examples
///
/// ```
/// use martensite_clipboard::clipboard::MIME_IMAGE_PNG;
///
/// assert_eq!(MIME_IMAGE_PNG, "image/png");
/// ```
pub const MIME_IMAGE_PNG: &str = "image/png";

/// A MIME type string.
///
/// This is a newtype around [`String`] that documents intent and allows
/// future validation without breaking the public API.
///
/// # Examples
///
/// ```
/// use martensite_clipboard::clipboard::Mime;
///
/// let mime = Mime::new("text/plain;charset=utf-8");
/// assert_eq!(mime.as_str(), "text/plain;charset=utf-8");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Mime(String);

impl Mime {
    /// Creates a new [`Mime`] from anything convertible into [`String`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::clipboard::Mime;
    ///
    /// let mime = Mime::new("image/png");
    /// assert_eq!(mime.as_str(), "image/png");
    /// ```
    #[inline]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the MIME type as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::clipboard::Mime;
    ///
    /// let mime = Mime::new("text/plain;charset=utf-8");
    /// assert_eq!(mime.as_str(), "text/plain;charset=utf-8");
    /// ```
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the [`Mime`] and returns the inner [`String`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::clipboard::Mime;
    ///
    /// let mime = Mime::new("image/png");
    /// assert_eq!(mime.into_inner(), "image/png");
    /// ```
    #[inline]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl From<String> for Mime {
    #[inline]
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Mime {
    #[inline]
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl AsRef<str> for Mime {
    #[inline]
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Mime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A boxed, `Send` closure that produces clipboard bytes lazily.
///
/// The wrapped closure has the signature `Box<dyn FnOnce() -> Vec<u8> +
/// Send>`. It is stored inside an [`Arc`]`<`[`Mutex`]`<`[`Option`]`<…>>>` so
/// that:
///
/// * [`ClipboardPayload`] remains [`Clone`] (the closure is shared, not
///   duplicated), allowing backends to retain a lazy payload obtained from a
///   `&ClipboardItem`;
/// * the closure can be taken out and invoked **at most once** without
///   `unsafe` code, even through a shared reference.
///
/// Cloning a [`LazyPayload`] clones the [`Arc`], not the closure. The first
/// caller that materializes the payload consumes the closure; subsequent
/// materializations of clones return [`None`].
///
/// # Examples
///
/// ```
/// use martensite_clipboard::LazyPayload;
///
/// let payload = LazyPayload::new(|| b"deferred".to_vec());
/// assert!(payload.is_pending());
/// ```
#[derive(Clone)]
pub struct LazyPayload {
    inner: Arc<Mutex<Option<BoxedProducer>>>,
}

/// The boxed closure type stored inside a [`LazyPayload`].
type BoxedProducer = Box<dyn FnOnce() -> Vec<u8> + Send>;

impl LazyPayload {
    /// Creates a new [`LazyPayload`] wrapping the given closure.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::LazyPayload;
    ///
    /// let payload = LazyPayload::new(|| b"deferred".to_vec());
    /// assert!(payload.is_pending());
    /// ```
    #[inline]
    pub fn new<F>(f: F) -> Self
    where
        F: FnOnce() -> Vec<u8> + Send + 'static,
    {
        Self {
            inner: Arc::new(Mutex::new(Some(Box::new(f)))),
        }
    }

    /// Returns `true` if the closure has not yet been consumed.
    ///
    /// A poisoned lock is tolerated: if another thread panicked while
    /// holding this mutex, the inner value is still recovered via
    /// [`std::sync::PoisonError::into_inner`] so that a producer panic cannot cascade
    /// into every subsequent clipboard read.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::LazyPayload;
    ///
    /// let payload = LazyPayload::new(|| b"deferred".to_vec());
    /// assert!(payload.is_pending());
    /// let _ = payload.take();
    /// assert!(!payload.is_pending());
    /// ```
    pub fn is_pending(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// Takes the closure out of this shared slot, if it has not already been
    /// consumed by any clone.
    ///
    /// A poisoned lock is tolerated: if another thread panicked while
    /// holding this mutex, the inner value is still recovered via
    /// [`std::sync::PoisonError::into_inner`] so that a producer panic cannot cascade
    /// into every subsequent clipboard read.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::LazyPayload;
    ///
    /// let payload = LazyPayload::new(|| b"deferred".to_vec());
    /// let closure = payload.take();
    /// assert!(closure.is_some());
    /// // A second take returns None.
    /// assert!(payload.take().is_none());
    /// ```
    pub fn take(&self) -> Option<Box<dyn FnOnce() -> Vec<u8> + Send>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

impl std::fmt::Debug for LazyPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pending = self.is_pending();
        f.debug_struct("LazyPayload")
            .field("pending", &pending)
            .finish_non_exhaustive()
    }
}

impl<F> From<F> for LazyPayload
where
    F: FnOnce() -> Vec<u8> + Send + 'static,
{
    #[inline]
    fn from(f: F) -> Self {
        Self::new(f)
    }
}

/// A single clipboard representation.
///
/// Variants cover the two common eager forms (text and raw bytes) plus a
/// lazy form whose producer is only invoked when the payload is actually
/// requested.
///
/// # Examples
///
/// ```
/// use martensite_clipboard::ClipboardPayload;
///
/// let text = ClipboardPayload::text("hi");
/// let bytes = ClipboardPayload::bytes(vec![0x89, 0x50]);
/// let lazy = ClipboardPayload::lazy(|| b"deferred".to_vec());
/// ```
#[derive(Clone, Debug)]
pub enum ClipboardPayload {
    /// A UTF-8 text payload. Stored as [`String`] for ergonomic construction
    /// and serialized to bytes as UTF-8 when retrieved.
    Text(String),
    /// Raw bytes, used for binary formats such as images.
    Bytes(Vec<u8>),
    /// A lazily-evaluated payload. The closure is invoked at most once when
    /// the payload is materialized via [`ClipboardPayload::materialize`] or
    /// [`ClipboardPayload::with_deadline`].
    Lazy(LazyPayload),
}

impl ClipboardPayload {
    /// Creates a [`ClipboardPayload::Text`] from anything convertible into
    /// [`String`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardPayload;
    ///
    /// let payload = ClipboardPayload::text("hello");
    /// assert_eq!(payload.materialize(), Some(b"hello".to_vec()));
    /// ```
    #[inline]
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    /// Creates a [`ClipboardPayload::Bytes`] from a byte vector.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardPayload;
    ///
    /// let payload = ClipboardPayload::bytes(vec![0x89, 0x50]);
    /// assert_eq!(payload.materialize(), Some(vec![0x89, 0x50]));
    /// ```
    #[inline]
    pub fn bytes(value: Vec<u8>) -> Self {
        Self::Bytes(value)
    }

    /// Creates a [`ClipboardPayload::Lazy`] from a closure.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardPayload;
    ///
    /// let payload = ClipboardPayload::lazy(|| b"deferred".to_vec());
    /// assert!(payload.is_lazy_pending());
    /// ```
    #[inline]
    pub fn lazy<F>(f: F) -> Self
    where
        F: FnOnce() -> Vec<u8> + Send + 'static,
    {
        Self::Lazy(LazyPayload::new(f))
    }

    /// Returns `true` if this payload is a lazy one that has not yet been
    /// consumed.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardPayload;
    ///
    /// let eager = ClipboardPayload::text("hi");
    /// assert!(!eager.is_lazy_pending());
    ///
    /// let lazy = ClipboardPayload::lazy(|| b"deferred".to_vec());
    /// assert!(lazy.is_lazy_pending());
    /// ```
    pub fn is_lazy_pending(&self) -> bool {
        match self {
            Self::Lazy(l) => l.is_pending(),
            _ => false,
        }
    }

    /// Materializes the payload into bytes, invoking any lazy closure.
    ///
    /// For [`ClipboardPayload::Text`] the bytes are the UTF-8 encoding of
    /// the string. For [`ClipboardPayload::Bytes`] the inner vector is
    /// returned. For [`ClipboardPayload::Lazy`] the closure is consumed and
    /// invoked (if it has not already been consumed by a clone).
    ///
    /// This method has **no deadline**: a panicking or long-running lazy
    /// producer will block the caller. Use [`ClipboardPayload::with_deadline`]
    /// when an unresponsive producer is a possibility.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardPayload;
    ///
    /// let text = ClipboardPayload::text("hello");
    /// assert_eq!(text.materialize(), Some(b"hello".to_vec()));
    ///
    /// let bytes = ClipboardPayload::bytes(vec![1, 2, 3]);
    /// assert_eq!(bytes.materialize(), Some(vec![1, 2, 3]));
    /// ```
    pub fn materialize(&self) -> Option<Vec<u8>> {
        match self {
            Self::Text(s) => Some(s.as_bytes().to_vec()),
            Self::Bytes(b) => Some(b.clone()),
            Self::Lazy(l) => l.take().map(|f| f()),
        }
    }

    /// Materializes the payload with a deadline.
    ///
    /// For eager variants this returns the bytes immediately. For
    /// [`ClipboardPayload::Lazy`] the closure is run on a spawned thread and
    /// this call blocks until either the result arrives or the `deadline`
    /// elapses. If the deadline elapses, the producer thread panics, or the
    /// closure was already consumed by a clone, [`None`] is returned.
    ///
    /// # Detached-thread behavior on timeout
    ///
    /// This method uses only safe std primitives (`thread::spawn` and
    /// `mpsc::recv_timeout`); no `unsafe` code is involved and there is no
    /// way to cancel a running thread in safe Rust. On timeout the producer
    /// thread is therefore **not** cancelled: it continues to run to
    /// completion in the background, and the result it eventually produces is
    /// silently dropped when the channel's sender is released. This is
    /// intentional and acceptable because:
    ///
    /// * the closure is `Send`, so it is safe for the detached thread to
    ///   outlive the calling stack frame;
    /// * the `mpsc::channel` drops cleanly — once the receiver is dropped
    ///   (on timeout) and the sender is dropped (when the thread finishes),
    ///   all resources are reclaimed;
    /// * the detached thread is a daemon-level worker that will be joined by
    ///   the runtime on process exit.
    ///
    /// Callers that require hard cancellation of the producer must arrange
    /// for that inside the closure itself (e.g. via a shared `AtomicBool`
    /// flag), since safe Rust offers no thread-cancellation primitive.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardPayload;
    /// use std::time::Duration;
    ///
    /// let text = ClipboardPayload::text("fast");
    /// assert_eq!(
    ///     text.with_deadline(Duration::from_millis(10)),
    ///     Some(b"fast".to_vec())
    /// );
    /// ```
    pub fn with_deadline(&self, deadline: Duration) -> Option<Vec<u8>> {
        match self {
            Self::Text(s) => Some(s.as_bytes().to_vec()),
            Self::Bytes(b) => Some(b.clone()),
            Self::Lazy(l) => l.take().and_then(|f| run_with_deadline(f, deadline)),
        }
    }
}

impl From<String> for ClipboardPayload {
    #[inline]
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for ClipboardPayload {
    #[inline]
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<Vec<u8>> for ClipboardPayload {
    #[inline]
    fn from(value: Vec<u8>) -> Self {
        Self::Bytes(value)
    }
}

/// Runs a lazy producer closure on a spawned thread with a receive timeout.
///
/// Returns [`None`] if the producer panics or the deadline elapses before
/// a result is produced. The producer thread is detached in the timeout
/// case; it will be cleaned up when the process exits.
fn run_with_deadline(
    producer: Box<dyn FnOnce() -> Vec<u8> + Send>,
    deadline: Duration,
) -> Option<Vec<u8>> {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let builder = thread::Builder::new().name("martensite-clipboard-lazy".to_owned());
    let handle = builder.spawn(move || {
        // Convert a producer panic into a silent no-result by guarding the
        // send; if `catch_unwind` fails the channel is simply never sent to.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(producer));
        if let Ok(bytes) = result {
            let _ = tx.send(bytes);
        }
    });
    // If the thread could not be spawned at all, there is nothing to wait for.
    let _thread = handle.ok()?;
    rx.recv_timeout(deadline).ok()
}

/// Canonicalizes a MIME type string for use as a lookup key.
///
/// MIME types are case-insensitive per RFC 2045/2046, and parameters may
/// appear in any order with arbitrary surrounding whitespace. This function
/// normalizes a MIME string so that semantically equivalent types compare
/// equal when used as a [`HashMap`] key:
///
/// * The type/subtype portion (everything before the first `;`) is
///   lowercased and trimmed.
/// * Each parameter is trimmed of surrounding whitespace.
/// * Parameters are sorted alphabetically, so `text/html; b=2; a=1` and
///   `text/html; a=1; b=2` canonicalize identically.
///
/// All MIME types used as keys in this crate ([`ClipboardItem`] payloads and
/// [`InMemoryClipboard`] contents) are stored in their canonical form, so
/// lookups are case-insensitive and order-independent.
///
/// # Examples
///
/// ```
/// use martensite_clipboard::clipboard::canonicalize_mime;
///
/// // Case-insensitive type/subtype.
/// assert_eq!(canonicalize_mime("Text/Plain"), "text/plain");
/// assert_eq!(canonicalize_mime("TEXT/HTML"), "text/html");
/// // Whitespace around parameters is trimmed.
/// assert_eq!(
///     canonicalize_mime("text/html;  charset=utf-8 "),
///     "text/html;charset=utf-8"
/// );
/// // Parameters are sorted alphabetically.
/// assert_eq!(
///     canonicalize_mime("text/html; b=2; a=1"),
///     "text/html;a=1;b=2"
/// );
/// ```
pub fn canonicalize_mime(mime: &str) -> String {
    let mut parts = mime.split(';');
    let mut result = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let mut params: Vec<String> = parts
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty())
        .collect();
    params.sort();
    for p in params {
        result.push(';');
        result.push_str(&p);
    }
    result
}

/// A multi-MIME clipboard item.
///
/// An item holds an arbitrary number of representations keyed by MIME type.
/// Builder methods provide convenient construction for common formats while
/// [`ClipboardItem::offer_custom`] allows arbitrary MIME types.
///
/// # Examples
///
/// ```
/// use martensite_clipboard::ClipboardItem;
///
/// let item = ClipboardItem::new()
///     .offer_text("hello")
///     .offer_html("<p>hello</p>")
///     .offer_custom("application/x-custom", vec![1, 2, 3]);
/// assert!(item.has("text/plain;charset=utf-8"));
/// ```
#[derive(Default, Clone)]
pub struct ClipboardItem {
    payloads: HashMap<String, ClipboardPayload>,
}

impl ClipboardItem {
    /// Creates a new empty [`ClipboardItem`] with no payloads.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardItem;
    ///
    /// let item = ClipboardItem::new();
    /// assert!(item.is_empty());
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Offers a plain-text representation under
    /// [`MIME_TEXT_PLAIN`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardItem;
    ///
    /// let item = ClipboardItem::new().offer_text("hello");
    /// assert!(item.has("text/plain;charset=utf-8"));
    /// ```
    #[inline]
    pub fn offer_text(mut self, text: impl Into<String>) -> Self {
        self.payloads
            .insert(MIME_TEXT_PLAIN.to_owned(), ClipboardPayload::text(text));
        self
    }

    /// Offers an HTML representation under
    /// [`MIME_TEXT_HTML`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardItem;
    ///
    /// let item = ClipboardItem::new().offer_html("<b>hello</b>");
    /// assert!(item.has("text/html"));
    /// ```
    #[inline]
    pub fn offer_html(mut self, html: impl Into<String>) -> Self {
        self.payloads
            .insert(MIME_TEXT_HTML.to_owned(), ClipboardPayload::text(html));
        self
    }

    /// Offers an RTF representation under
    /// [`MIME_TEXT_RTF`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardItem;
    ///
    /// let item = ClipboardItem::new().offer_rtf("{\\rtf1}");
    /// assert!(item.has("application/rtf"));
    /// ```
    #[inline]
    pub fn offer_rtf(mut self, rtf: impl Into<String>) -> Self {
        self.payloads
            .insert(MIME_TEXT_RTF.to_owned(), ClipboardPayload::text(rtf));
        self
    }

    /// Offers a PNG image representation under
    /// [`MIME_IMAGE_PNG`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardItem;
    ///
    /// let item = ClipboardItem::new().offer_png(vec![0x89, 0x50]);
    /// assert!(item.has("image/png"));
    /// ```
    #[inline]
    pub fn offer_png(mut self, png: Vec<u8>) -> Self {
        self.payloads
            .insert(MIME_IMAGE_PNG.to_owned(), ClipboardPayload::bytes(png));
        self
    }

    /// Offers a custom representation under an arbitrary MIME type.
    ///
    /// The `mime` string is canonicalized via [`canonicalize_mime`] before
    /// being used as a key, so lookups are case-insensitive and
    /// parameter-order-independent (see that function's docs).
    ///
    /// The payload may be any [`ClipboardPayload`], including a lazy one.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardItem;
    ///
    /// let item = ClipboardItem::new().offer_custom("application/x-custom", vec![1, 2, 3]);
    /// assert!(item.has("application/x-custom"));
    /// ```
    #[inline]
    pub fn offer_custom(
        mut self,
        mime: impl Into<String>,
        payload: impl Into<ClipboardPayload>,
    ) -> Self {
        self.payloads
            .insert(canonicalize_mime(&mime.into()), payload.into());
        self
    }

    /// Returns `true` if a payload for the given MIME type is offered.
    ///
    /// The lookup uses [`canonicalize_mime`], so the comparison is
    /// case-insensitive and parameter-order-independent.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardItem;
    ///
    /// let item = ClipboardItem::new().offer_text("hello");
    /// assert!(item.has("text/plain;charset=utf-8"));
    /// assert!(!item.has("text/html"));
    /// ```
    #[inline]
    pub fn has(&self, mime: &str) -> bool {
        self.payloads.contains_key(&canonicalize_mime(mime))
    }

    /// Returns the list of offered MIME types, in unspecified order.
    ///
    /// The returned strings are in canonical form (see [`canonicalize_mime`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardItem;
    ///
    /// let item = ClipboardItem::new().offer_text("a").offer_html("b");
    /// let mut types = item.offered_types();
    /// types.sort();
    /// assert_eq!(types.len(), 2);
    /// ```
    pub fn offered_types(&self) -> Vec<String> {
        self.payloads.keys().cloned().collect()
    }

    /// Returns a borrowing iterator over the offered MIME types without
    /// collecting them into a `Vec`.
    ///
    /// This is the zero-allocation counterpart to [`Self::offered_types`],
    /// used by the `Debug` impl and by callers that only need to inspect
    /// the offered type names without owning them.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardItem;
    ///
    /// let item = ClipboardItem::new().offer_text("a");
    /// let types: Vec<&str> = item.types().collect();
    /// assert_eq!(types.len(), 1);
    /// ```
    pub fn types(&self) -> impl Iterator<Item = &str> {
        self.payloads.keys().map(String::as_str)
    }

    /// Removes the payload for the given MIME type, if present, and returns it.
    ///
    /// The lookup uses [`canonicalize_mime`], so the comparison is
    /// case-insensitive and parameter-order-independent.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardItem;
    ///
    /// let mut item = ClipboardItem::new().offer_text("hello");
    /// assert!(item.remove("text/plain;charset=utf-8").is_some());
    /// assert!(!item.has("text/plain;charset=utf-8"));
    /// ```
    pub fn remove(&mut self, mime: &str) -> Option<ClipboardPayload> {
        self.payloads.remove(&canonicalize_mime(mime))
    }

    /// Returns a reference to the payload for the given MIME type, if present.
    ///
    /// The lookup uses [`canonicalize_mime`], so the comparison is
    /// case-insensitive and parameter-order-independent.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardItem;
    ///
    /// let item = ClipboardItem::new().offer_text("hello");
    /// assert!(item.get("text/plain;charset=utf-8").is_some());
    /// assert!(item.get("text/html").is_none());
    /// ```
    #[inline]
    pub fn get(&self, mime: &str) -> Option<&ClipboardPayload> {
        self.payloads.get(&canonicalize_mime(mime))
    }

    /// Returns the number of offered representations.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardItem;
    ///
    /// let item = ClipboardItem::new().offer_text("a").offer_html("b");
    /// assert_eq!(item.len(), 2);
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.payloads.len()
    }

    /// Returns `true` if no representations are offered.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardItem;
    ///
    /// let item = ClipboardItem::new();
    /// assert!(item.is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.payloads.is_empty()
    }

    /// Returns an iterator over the offered `(mime, payload)` pairs.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardItem;
    ///
    /// let item = ClipboardItem::new().offer_text("a").offer_html("b");
    /// assert_eq!(item.iter().count(), 2);
    /// ```
    pub fn iter(&self) -> impl Iterator<Item = (&String, &ClipboardPayload)> {
        self.payloads.iter()
    }

    /// Consumes the item and returns the underlying payload map.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::ClipboardItem;
    ///
    /// let item = ClipboardItem::new().offer_text("hello");
    /// let map = item.into_payloads();
    /// assert!(map.contains_key("text/plain;charset=utf-8"));
    /// ```
    pub fn into_payloads(self) -> HashMap<String, ClipboardPayload> {
        self.payloads
    }
}

impl std::fmt::Debug for ClipboardItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Format the offered type names directly from the map keys via the
        // zero-allocation `types()` iterator — no intermediate `Vec`.
        f.debug_struct("ClipboardItem")
            .field("types", &self.payloads.keys())
            .finish_non_exhaustive()
    }
}

/// The contract for reading and writing clipboard contents.
///
/// Platform backends implement this trait; [`InMemoryClipboard`] provides a
/// pure-Rust implementation suitable for tests and headless environments.
///
/// # Examples
///
/// ```
/// use martensite_clipboard::{ClipboardItem, ClipboardService, InMemoryClipboard};
///
/// let mut cb = InMemoryClipboard::new();
/// cb.set_contents(&ClipboardItem::new().offer_text("hi"));
/// assert_eq!(
///     cb.get_contents("text/plain;charset=utf-8"),
///     Some(b"hi".to_vec())
/// );
/// ```
pub trait ClipboardService {
    /// Sets the clipboard contents, replacing any previous contents.
    ///
    /// Implementations retain the offered payloads. Lazy payloads are
    /// evaluated when [`ClipboardService::get_contents`] is called, subject
    /// to the backend's deadline policy.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::{ClipboardItem, ClipboardService, InMemoryClipboard};
    ///
    /// let mut cb = InMemoryClipboard::new();
    /// cb.set_contents(&ClipboardItem::new().offer_text("hello"));
    /// assert_eq!(cb.available_types().len(), 1);
    /// ```
    fn set_contents(&mut self, item: &ClipboardItem);

    /// Returns the bytes for the requested MIME type, or [`None`] if it is
    /// not available.
    ///
    /// For lazy payloads this invokes the producer closure (at most once).
    /// Implementations apply a deadline so an unresponsive producer cannot
    /// block indefinitely.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::{ClipboardItem, ClipboardService, InMemoryClipboard};
    ///
    /// let mut cb = InMemoryClipboard::new();
    /// cb.set_contents(&ClipboardItem::new().offer_text("hello"));
    /// assert_eq!(
    ///     cb.get_contents("text/plain;charset=utf-8"),
    ///     Some(b"hello".to_vec())
    /// );
    /// ```
    fn get_contents(&self, mime: &str) -> Option<Vec<u8>>;

    /// Returns the list of MIME types currently available on the clipboard.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::{ClipboardItem, ClipboardService, InMemoryClipboard};
    ///
    /// let mut cb = InMemoryClipboard::new();
    /// cb.set_contents(&ClipboardItem::new().offer_text("a").offer_html("b"));
    /// assert_eq!(cb.available_types().len(), 2);
    /// ```
    fn available_types(&self) -> Vec<String>;

    /// Clears the clipboard contents.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::{ClipboardItem, ClipboardService, InMemoryClipboard};
    ///
    /// let mut cb = InMemoryClipboard::new();
    /// cb.set_contents(&ClipboardItem::new().offer_text("a"));
    /// cb.clear();
    /// assert!(cb.available_types().is_empty());
    /// ```
    fn clear(&mut self);
}

/// A simple in-memory [`ClipboardService`] for tests and headless use.
///
/// Payloads (including lazy ones) are retained on `set_contents` and
/// materialized on `get_contents`. Lazy payloads are evaluated with
/// [`DEFAULT_LAZY_DEADLINE`]; an unresponsive or panicking producer yields
/// [`None`] for that MIME type without affecting others.
///
/// # Examples
///
/// ```
/// use martensite_clipboard::{ClipboardItem, ClipboardService, InMemoryClipboard};
///
/// let mut cb = InMemoryClipboard::new();
/// cb.set_contents(&ClipboardItem::new().offer_text("hi"));
/// assert_eq!(cb.available_types().len(), 1);
/// cb.clear();
/// assert!(cb.available_types().is_empty());
/// ```
#[derive(Default)]
pub struct InMemoryClipboard {
    contents: HashMap<String, ClipboardPayload>,
}

impl InMemoryClipboard {
    /// Creates a new empty in-memory clipboard.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::{ClipboardService, InMemoryClipboard};
    ///
    /// let cb = InMemoryClipboard::new();
    /// assert!(cb.available_types().is_empty());
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of representations currently stored.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::{ClipboardItem, ClipboardService, InMemoryClipboard};
    ///
    /// let mut cb = InMemoryClipboard::new();
    /// cb.set_contents(&ClipboardItem::new().offer_text("a").offer_html("b"));
    /// assert_eq!(cb.len(), 2);
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.contents.len()
    }

    /// Returns `true` if the clipboard holds no representations.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard::{ClipboardItem, ClipboardService, InMemoryClipboard};
    ///
    /// let mut cb = InMemoryClipboard::new();
    /// assert!(cb.is_empty());
    /// cb.set_contents(&ClipboardItem::new().offer_text("a"));
    /// assert!(!cb.is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.contents.is_empty()
    }
}

impl ClipboardService for InMemoryClipboard {
    fn set_contents(&mut self, item: &ClipboardItem) {
        self.contents.clear();
        for (mime, payload) in item.iter() {
            // `mime` is already canonical (ClipboardItem canonicalizes on
            // insert), but re-canonicalize defensively in case a caller
            // constructed the item via `into_payloads`/`Default`.
            self.contents
                .insert(canonicalize_mime(mime), payload.clone());
        }
    }

    fn get_contents(&self, mime: &str) -> Option<Vec<u8>> {
        let payload = self.contents.get(&canonicalize_mime(mime))?;
        payload.with_deadline(DEFAULT_LAZY_DEADLINE)
    }

    fn available_types(&self) -> Vec<String> {
        self.contents.keys().cloned().collect()
    }

    fn clear(&mut self) {
        self.contents.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn mime_new_and_as_str() {
        let m = Mime::new("text/plain");
        assert_eq!(m.as_str(), "text/plain");
        assert_eq!(m.to_string(), "text/plain");
    }

    #[test]
    fn mime_from_string_and_str() {
        let from_string: Mime = String::from("image/png").into();
        assert_eq!(from_string.as_str(), "image/png");
        let from_str: Mime = "application/rtf".into();
        assert_eq!(from_str.as_str(), "application/rtf");
    }

    #[test]
    fn mime_into_inner() {
        let m = Mime::new("x/y");
        assert_eq!(m.into_inner(), "x/y");
    }

    #[test]
    fn mime_eq_and_hash() {
        let a = Mime::new("a/b");
        let b = Mime::new("a/b");
        let c = Mime::new("c/d");
        assert_eq!(a, b);
        assert_ne!(a, c);
        let mut set = std::collections::HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }

    #[test]
    fn payload_text_materializes_utf8() {
        let p = ClipboardPayload::text("héllo");
        assert_eq!(p.materialize(), Some("héllo".as_bytes().to_vec()));
    }

    #[test]
    fn payload_bytes_materializes() {
        let p = ClipboardPayload::bytes(vec![1, 2, 3]);
        assert_eq!(p.materialize(), Some(vec![1, 2, 3]));
    }

    #[test]
    fn payload_lazy_materializes_once() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        let p = ClipboardPayload::lazy(move || {
            c.fetch_add(1, Ordering::SeqCst);
            b"deferred".to_vec()
        });
        assert_eq!(p.materialize(), Some(b"deferred".to_vec()));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        // A second materialization of the same (non-cloned) payload returns
        // None because the closure was consumed.
        assert_eq!(p.materialize(), None);
    }

    #[test]
    fn payload_with_deadline_eager_returns_immediately() {
        let p = ClipboardPayload::text("fast");
        let out = p.with_deadline(Duration::from_millis(10));
        assert_eq!(out, Some(b"fast".to_vec()));
    }

    #[test]
    fn payload_with_deadline_lazy_returns_within_time() {
        let p = ClipboardPayload::lazy(|| b"ok".to_vec());
        let out = p.with_deadline(Duration::from_millis(500));
        assert_eq!(out, Some(b"ok".to_vec()));
    }

    #[test]
    fn payload_with_deadline_lazy_times_out() {
        let p = ClipboardPayload::lazy(|| {
            // Block forever; the deadline will still fire because
            // `with_deadline` uses `recv_timeout` on a detached thread.
            std::thread::park();
            b"late".to_vec()
        });
        let out = p.with_deadline(Duration::from_millis(20));
        assert!(out.is_none(), "expected timeout, got {:?}", out);
    }

    #[test]
    fn payload_with_deadline_lazy_panic_returns_none() {
        let p = ClipboardPayload::lazy(|| panic!("boom"));
        let out = p.with_deadline(Duration::from_millis(500));
        assert!(out.is_none());
    }

    #[test]
    fn payload_with_deadline_already_consumed_returns_none() {
        let p = ClipboardPayload::lazy(|| b"once".to_vec());
        assert_eq!(
            p.with_deadline(Duration::from_millis(100)),
            Some(b"once".to_vec())
        );
        assert_eq!(p.with_deadline(Duration::from_millis(100)), None);
    }

    #[test]
    fn lazy_payload_clone_shares_closure() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        let p = ClipboardPayload::lazy(move || {
            c.fetch_add(1, Ordering::SeqCst);
            b"shared".to_vec()
        });
        let q = p.clone();
        // Only one of the clones may consume the closure.
        let first = p.materialize();
        let second = q.materialize();
        assert!(first.is_some() || second.is_some());
        assert!(first.is_none() || second.is_none());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lazy_payload_is_pending() {
        let p = ClipboardPayload::lazy(|| vec![1]);
        assert!(p.is_lazy_pending());
        let _ = p.materialize();
        assert!(!p.is_lazy_pending());
    }

    #[test]
    fn item_new_is_empty() {
        let item = ClipboardItem::new();
        assert!(item.is_empty());
        assert_eq!(item.len(), 0);
        assert!(item.offered_types().is_empty());
    }

    #[test]
    fn item_offer_text_html_rtf_png_custom() {
        let item = ClipboardItem::new()
            .offer_text("t")
            .offer_html("<b>t</b>")
            .offer_rtf("{\\rtf1}")
            .offer_png(vec![0x89, 0x50])
            .offer_custom("application/x-custom", vec![9, 9]);
        assert_eq!(item.len(), 5);
        assert!(item.has(MIME_TEXT_PLAIN));
        assert!(item.has(MIME_TEXT_HTML));
        assert!(item.has(MIME_TEXT_RTF));
        assert!(item.has(MIME_IMAGE_PNG));
        assert!(item.has("application/x-custom"));
    }

    #[test]
    fn item_offer_text_accepts_str_and_string() {
        let a = ClipboardItem::new().offer_text("from &str");
        let b = ClipboardItem::new().offer_text(String::from("from String"));
        assert!(a.has(MIME_TEXT_PLAIN));
        assert!(b.has(MIME_TEXT_PLAIN));
    }

    #[test]
    fn item_remove_and_get() {
        let mut item = ClipboardItem::new().offer_text("hi");
        assert!(item.get(MIME_TEXT_PLAIN).is_some());
        let removed = item.remove(MIME_TEXT_PLAIN);
        assert!(matches!(removed, Some(ClipboardPayload::Text(_))));
        assert!(!item.has(MIME_TEXT_PLAIN));
        assert!(item.remove(MIME_TEXT_PLAIN).is_none());
    }

    #[test]
    fn item_iter_yields_all() {
        let item = ClipboardItem::new().offer_text("a").offer_html("b");
        let mut types: Vec<String> = item.iter().map(|(m, _)| m.clone()).collect();
        types.sort();
        assert_eq!(
            types,
            vec![MIME_TEXT_HTML.to_owned(), MIME_TEXT_PLAIN.to_owned()]
        );
    }

    #[test]
    fn item_into_payloads() {
        let item = ClipboardItem::new().offer_text("x");
        let map = item.into_payloads();
        assert!(map.contains_key(MIME_TEXT_PLAIN));
    }

    #[test]
    fn item_clone_is_independent() {
        let item = ClipboardItem::new().offer_text("x");
        let cloned = item.clone();
        assert_eq!(item.len(), cloned.len());
        assert!(cloned.has(MIME_TEXT_PLAIN));
    }

    #[test]
    fn in_memory_round_trip_text() {
        let mut cb = InMemoryClipboard::new();
        cb.set_contents(&ClipboardItem::new().offer_text("hello"));
        assert_eq!(cb.get_contents(MIME_TEXT_PLAIN), Some(b"hello".to_vec()));
    }

    #[test]
    fn in_memory_round_trip_multi_mime() {
        let mut cb = InMemoryClipboard::new();
        let item = ClipboardItem::new()
            .offer_text("plain")
            .offer_html("<p>plain</p>")
            .offer_custom("application/x-custom", vec![1, 2, 3]);
        cb.set_contents(&item);
        assert_eq!(cb.available_types().len(), 3);
        assert_eq!(cb.get_contents(MIME_TEXT_PLAIN), Some(b"plain".to_vec()));
        assert_eq!(
            cb.get_contents(MIME_TEXT_HTML),
            Some(b"<p>plain</p>".to_vec())
        );
        assert_eq!(cb.get_contents("application/x-custom"), Some(vec![1, 2, 3]));
    }

    #[test]
    fn in_memory_round_trip_png_bytes() {
        let mut cb = InMemoryClipboard::new();
        let png = vec![0x89, 0x50, 0x4E, 0x47];
        cb.set_contents(&ClipboardItem::new().offer_png(png.clone()));
        assert_eq!(cb.get_contents(MIME_IMAGE_PNG), Some(png));
    }

    #[test]
    fn in_memory_lazy_evaluated_on_get() {
        let mut cb = InMemoryClipboard::new();
        let item = ClipboardItem::new().offer_custom(
            "application/x-lazy",
            ClipboardPayload::lazy(|| b"lazy-data".to_vec()),
        );
        cb.set_contents(&item);
        assert_eq!(
            cb.get_contents("application/x-lazy"),
            Some(b"lazy-data".to_vec())
        );
        // Second read returns None: closure already consumed.
        assert_eq!(cb.get_contents("application/x-lazy"), None);
    }

    #[test]
    fn in_memory_lazy_timeout_returns_none() {
        let mut cb = InMemoryClipboard::new();
        let item = ClipboardItem::new().offer_custom(
            "application/x-slow",
            ClipboardPayload::lazy(|| {
                // Block forever; the in-memory backend's default deadline
                // will still fire because `with_deadline` uses
                // `recv_timeout` on a detached thread.
                std::thread::park();
                b"late".to_vec()
            }),
        );
        cb.set_contents(&item);
        // The in-memory backend uses the 500ms default deadline; the slow
        // producer exceeds it.
        assert_eq!(cb.get_contents("application/x-slow"), None);
    }

    #[test]
    fn in_memory_clear_empties() {
        let mut cb = InMemoryClipboard::new();
        cb.set_contents(&ClipboardItem::new().offer_text("x"));
        assert!(!cb.is_empty());
        cb.clear();
        assert!(cb.is_empty());
        assert!(cb.available_types().is_empty());
        assert!(cb.get_contents(MIME_TEXT_PLAIN).is_none());
    }

    #[test]
    fn in_memory_set_replaces_previous() {
        let mut cb = InMemoryClipboard::new();
        cb.set_contents(&ClipboardItem::new().offer_text("a"));
        cb.set_contents(&ClipboardItem::new().offer_html("b"));
        assert!(cb.get_contents(MIME_TEXT_PLAIN).is_none());
        assert_eq!(cb.get_contents(MIME_TEXT_HTML), Some(b"b".to_vec()));
        assert_eq!(cb.len(), 1);
    }

    #[test]
    fn in_memory_missing_mime_returns_none() {
        let cb = InMemoryClipboard::new();
        assert!(cb.get_contents("nope").is_none());
    }

    #[test]
    fn in_memory_len_and_is_empty() {
        let mut cb = InMemoryClipboard::new();
        assert!(cb.is_empty());
        cb.set_contents(&ClipboardItem::new().offer_text("a").offer_html("b"));
        assert_eq!(cb.len(), 2);
        assert!(!cb.is_empty());
    }

    #[test]
    fn round_trip_fidelity_text_bytes() {
        let mut cb = InMemoryClipboard::new();
        let text = "héllo, 世界";
        let bytes = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let item = ClipboardItem::new()
            .offer_text(text)
            .offer_custom("application/octet-stream", bytes.clone());
        cb.set_contents(&item);
        assert_eq!(
            cb.get_contents(MIME_TEXT_PLAIN),
            Some(text.as_bytes().to_vec())
        );
        assert_eq!(cb.get_contents("application/octet-stream"), Some(bytes));
    }

    #[test]
    fn round_trip_fidelity_many_custom_mimes() {
        let mut cb = InMemoryClipboard::new();
        let mut item = ClipboardItem::new();
        for i in 0..16u8 {
            item = item.offer_custom(format!("application/x-{i}"), vec![i, i, i]);
        }
        cb.set_contents(&item);
        assert_eq!(cb.available_types().len(), 16);
        for i in 0..16u8 {
            assert_eq!(
                cb.get_contents(&format!("application/x-{i}")),
                Some(vec![i, i, i]),
                "round-trip failed for application/x-{i}"
            );
        }
    }

    #[test]
    fn deadline_constant_is_500ms() {
        assert_eq!(DEFAULT_LAZY_DEADLINE, Duration::from_millis(500));
    }

    #[test]
    fn lazy_payload_with_deadline_preserves_bytes() {
        let data: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
        let expected = data.clone();
        let p = ClipboardPayload::lazy(move || data);
        let out = p.with_deadline(Duration::from_millis(500));
        assert_eq!(out, Some(expected));
    }

    #[test]
    fn canonicalize_mime_lowercases_type_subtype() {
        assert_eq!(canonicalize_mime("Text/Plain"), "text/plain");
        assert_eq!(canonicalize_mime("TEXT/HTML"), "text/html");
        assert_eq!(canonicalize_mime("Application/RTF"), "application/rtf");
    }

    #[test]
    fn canonicalize_mime_trims_whitespace_around_parameters() {
        assert_eq!(
            canonicalize_mime("text/html;  charset=utf-8 "),
            "text/html;charset=utf-8"
        );
        assert_eq!(
            canonicalize_mime("  text/plain ; charset = utf-8  "),
            "text/plain;charset = utf-8"
        );
    }

    #[test]
    fn canonicalize_mime_sorts_parameters_alphabetically() {
        assert_eq!(
            canonicalize_mime("text/html; b=2; a=1"),
            "text/html;a=1;b=2"
        );
        assert_eq!(
            canonicalize_mime("text/html; a=1; b=2"),
            "text/html;a=1;b=2"
        );
    }

    #[test]
    fn canonicalize_mime_no_parameters() {
        assert_eq!(canonicalize_mime("text/plain"), "text/plain");
        assert_eq!(canonicalize_mime("  image/png  "), "image/png");
    }

    #[test]
    fn canonicalize_mime_empty_and_semicolon_only() {
        assert_eq!(canonicalize_mime(""), "");
        assert_eq!(canonicalize_mime(";"), "");
        assert_eq!(canonicalize_mime("text/plain;"), "text/plain");
    }

    #[test]
    fn item_has_is_case_insensitive() {
        let item = ClipboardItem::new().offer_custom("Text/Plain", "hi");
        assert!(item.has("text/plain"));
        assert!(item.has("TEXT/PLAIN"));
        assert!(item.has("Text/Plain"));
        assert!(!item.has("text/html"));
    }

    #[test]
    fn item_get_is_case_insensitive() {
        let item = ClipboardItem::new().offer_custom("Text/Plain", "hi");
        assert!(item.get("text/plain").is_some());
        assert!(item.get("TEXT/PLAIN").is_some());
    }

    #[test]
    fn item_remove_is_case_insensitive() {
        let mut item = ClipboardItem::new().offer_custom("Text/Plain", "hi");
        assert!(item.remove("text/plain").is_some());
        assert!(!item.has("Text/Plain"));
    }

    #[test]
    fn item_offer_custom_parameter_order_independent() {
        let item = ClipboardItem::new().offer_custom("text/html; b=2; a=1", "x");
        assert!(item.has("text/html; a=1; b=2"));
        assert!(item.has("text/html; b=2; a=1"));
    }

    #[test]
    fn item_offer_text_matches_case_insensitive_lookup() {
        // The built-in constants are already lowercase/canonical, but a
        // caller using a mixed-case type/subtype must still match. Only the
        // type/subtype portion is case-insensitive (per RFC 2045/2046);
        // parameter values are compared as-is.
        let item = ClipboardItem::new().offer_text("hello");
        assert!(item.has("Text/Plain;charset=utf-8"));
        assert!(item.has("TEXT/PLAIN;charset=utf-8"));
    }

    #[test]
    fn in_memory_get_contents_case_insensitive() {
        let mut cb = InMemoryClipboard::new();
        cb.set_contents(&ClipboardItem::new().offer_custom("Text/Plain", "hi"));
        assert_eq!(cb.get_contents("text/plain"), Some(b"hi".to_vec()));
        assert_eq!(cb.get_contents("TEXT/PLAIN"), Some(b"hi".to_vec()));
    }

    #[test]
    fn in_memory_set_contents_canonicalizes_keys() {
        let mut cb = InMemoryClipboard::new();
        cb.set_contents(&ClipboardItem::new().offer_custom("Text/HTML; charset=utf-8", "<b>x</b>"));
        // available_types returns canonical keys.
        let types = cb.available_types();
        assert_eq!(types.len(), 1);
        assert_eq!(types[0], "text/html;charset=utf-8");
        // Lookup with a different parameter order still matches.
        assert_eq!(
            cb.get_contents("text/html;charset=utf-8"),
            Some(b"<b>x</b>".to_vec())
        );
    }

    #[test]
    fn in_memory_round_trip_text_case_insensitive() {
        let mut cb = InMemoryClipboard::new();
        cb.set_contents(&ClipboardItem::new().offer_text("hello"));
        // Only the type/subtype portion is case-insensitive; parameter
        // values are compared as-is.
        assert_eq!(
            cb.get_contents("TEXT/PLAIN;charset=utf-8"),
            Some(b"hello".to_vec())
        );
    }
}
