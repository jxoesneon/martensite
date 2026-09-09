//! Multi-MIME delayed-rendering clipboard engine.
//!
//! `martensite-clipboard` provides a platform-agnostic clipboard data model
//! with support for multiple MIME representations of a single logical
//! payload and lazy (delayed) rendering of large payloads guarded by a
//! configurable deadline.
//!
//! # Architecture
//!
//! * [`ClipboardItem`] holds an arbitrary number of representations keyed by
//!   MIME type. Builder methods ([`ClipboardItem::offer_text`],
//!   [`ClipboardItem::offer_html`], [`ClipboardItem::offer_rtf`],
//!   [`ClipboardItem::offer_png`], [`ClipboardItem::offer_custom`]) populate
//!   the common formats.
//! * [`ClipboardPayload`] is either eager ([`ClipboardPayload::Text`] /
//!   [`ClipboardPayload::Bytes`]) or lazy
//!   ([`ClipboardPayload::Lazy`]). Lazy payloads wrap a `Send` `FnOnce`
//!   closure that is invoked at most once, with a deadline to protect against
//!   unresponsive producers (see [`ClipboardPayload::with_deadline`]).
//! * [`ClipboardService`] is the read/write contract implemented by backends.
//!   [`InMemoryClipboard`] is a pure-Rust implementation for tests and
//!   headless environments.
//! * [`PlatformClipboard`] extends [`ClipboardService`] with a backend name.
//!   [`default_platform_clipboard`] selects the best available backend for
//!   the current target. Because this crate is `#![forbid(unsafe_code)]`,
//!   every platform backend is currently a safe stub that documents the
//!   intended FFI integration point; real `unsafe` FFI is deferred to a
//!   future milestone (see the [`platform`] module docs).
//!
//! # Examples
//!
//! ```
//! use martensite_clipboard::{
//!     ClipboardItem, ClipboardPayload, ClipboardService, InMemoryClipboard,
//! };
//!
//! let mut cb = InMemoryClipboard::new();
//! let item = ClipboardItem::new()
//!     .offer_text("hello")
//!     .offer_html("<b>hello</b>")
//!     .offer_custom("application/x-lazy", ClipboardPayload::lazy(|| {
//!         // Expensive work deferred until paste.
//!         b"deferred-bytes".to_vec()
//!     }));
//! cb.set_contents(&item);
//! assert_eq!(
//!     cb.get_contents("text/plain;charset=utf-8"),
//!     Some(b"hello".to_vec())
//! );
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod clipboard;
pub mod platform;

pub use clipboard::{
    canonicalize_mime, ClipboardItem, ClipboardPayload, ClipboardService, InMemoryClipboard,
    LazyPayload, Mime, DEFAULT_LAZY_DEADLINE,
};
pub use platform::{default_platform_clipboard, PlatformClipboard, StubClipboard};

#[cfg(test)]
mod tests {
    use super::*;
    use clipboard::MIME_TEXT_HTML;
    use clipboard::MIME_TEXT_PLAIN;
    use std::time::Duration;

    #[test]
    fn re_exported_item_builder_round_trip() {
        let mut cb = InMemoryClipboard::new();
        cb.set_contents(
            &ClipboardItem::new()
                .offer_text("hi")
                .offer_html("<b>hi</b>"),
        );
        assert_eq!(cb.get_contents(MIME_TEXT_PLAIN), Some(b"hi".to_vec()));
        assert_eq!(cb.get_contents(MIME_TEXT_HTML), Some(b"<b>hi</b>".to_vec()));
    }

    #[test]
    fn re_exported_lazy_payload_round_trip() {
        let mut cb = InMemoryClipboard::new();
        cb.set_contents(&ClipboardItem::new().offer_custom(
            "application/x-lazy",
            ClipboardPayload::lazy(|| b"lazy".to_vec()),
        ));
        assert_eq!(
            cb.get_contents("application/x-lazy"),
            Some(b"lazy".to_vec())
        );
    }

    #[test]
    fn re_exported_default_platform_clipboard_is_stub_like() {
        let cb = default_platform_clipboard();
        assert!(!cb.platform_name().is_empty());
        // Without the `platform` feature, all backends are stubs and
        // always report empty types/contents. With the `platform` feature,
        // the real OS clipboard may contain arbitrary data, so we only
        // assert the stub-like behavior when the feature is disabled.
        #[cfg(not(feature = "platform"))]
        assert!(cb.available_types().is_empty());
    }

    #[test]
    fn re_exported_stub_returns_empty() {
        let mut cb = StubClipboard::new();
        cb.set_contents(&ClipboardItem::new().offer_text("x"));
        assert!(cb.available_types().is_empty());
        assert!(cb.get_contents(MIME_TEXT_PLAIN).is_none());
    }

    #[test]
    fn deadline_constant_re_exported() {
        assert_eq!(DEFAULT_LAZY_DEADLINE, Duration::from_millis(500));
    }

    #[test]
    fn mime_re_exported() {
        let m = Mime::new("text/plain;charset=utf-8");
        assert_eq!(m.as_str(), MIME_TEXT_PLAIN);
    }
}
