//! `ClipboardService` trait-contract tests for `InMemoryClipboard`.
//!
//! These tests wrap the existing `InMemoryClipboard` and verify the
//! `ClipboardService` trait contract (text round-trip, multi-MIME,
//! clear, available types, and lazy-payload deadline timeout) without
//! touching the real OS clipboard. The lazy timeout test uses a
//! blocking `std::thread::park()` call that never returns, so the test
//! only waits for the deadline to elapse rather than a fixed sleep.

#![forbid(unsafe_code)]

use martensite_clipboard::clipboard::{MIME_TEXT_HTML, MIME_TEXT_PLAIN};
use martensite_clipboard::{ClipboardItem, ClipboardPayload, ClipboardService, InMemoryClipboard};

#[test]
fn in_memory_text_round_trip() {
    let mut cb = InMemoryClipboard::new();
    cb.set_contents(&ClipboardItem::new().offer_text("hello"));
    assert_eq!(cb.get_contents(MIME_TEXT_PLAIN), Some(b"hello".to_vec()));
}

#[test]
fn in_memory_multi_mime_round_trip() {
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
fn in_memory_clear_empties() {
    let mut cb = InMemoryClipboard::new();
    cb.set_contents(&ClipboardItem::new().offer_text("x").offer_html("y"));
    assert!(!cb.is_empty());
    cb.clear();
    assert!(cb.is_empty());
    assert!(cb.available_types().is_empty());
    assert!(cb.get_contents(MIME_TEXT_PLAIN).is_none());
    assert!(cb.get_contents(MIME_TEXT_HTML).is_none());
}

#[test]
fn in_memory_available_types() {
    let mut cb = InMemoryClipboard::new();
    cb.set_contents(
        &ClipboardItem::new()
            .offer_text("a")
            .offer_html("b")
            .offer_custom("application/x-custom", vec![0]),
    );
    let mut types = cb.available_types();
    types.sort();
    assert_eq!(
        types,
        vec![
            "application/x-custom".to_owned(),
            "text/html".to_owned(),
            "text/plain;charset=utf-8".to_owned(),
        ]
    );
}

#[test]
fn in_memory_lazy_payload_times_out() {
    let mut cb = InMemoryClipboard::new();
    // The producer blocks forever via `thread::park()` (no unparker), so
    // `get_contents` returns `None` once the default deadline elapses
    // instead of waiting for the producer to finish.
    let item = ClipboardItem::new().offer_custom(
        "application/x-blocking",
        ClipboardPayload::lazy(|| {
            std::thread::park();
            Vec::new()
        }),
    );
    cb.set_contents(&item);
    assert_eq!(cb.get_contents("application/x-blocking"), None);
}

#[test]
fn in_memory_multiple_writes_overwrite() {
    let mut cb = InMemoryClipboard::new();
    cb.set_contents(&ClipboardItem::new().offer_text("first"));
    assert_eq!(cb.get_contents(MIME_TEXT_PLAIN), Some(b"first".to_vec()));
    cb.set_contents(&ClipboardItem::new().offer_text("second"));
    assert_eq!(cb.get_contents(MIME_TEXT_PLAIN), Some(b"second".to_vec()));
    assert_eq!(cb.len(), 1);
}

#[test]
fn in_memory_large_text_round_trip() {
    let mut cb = InMemoryClipboard::new();
    let text = "x".repeat(256 * 1024);
    cb.set_contents(&ClipboardItem::new().offer_text(text.clone()));
    assert_eq!(cb.get_contents(MIME_TEXT_PLAIN), Some(text.into_bytes()));
}

#[test]
fn in_memory_empty_string_round_trip() {
    let mut cb = InMemoryClipboard::new();
    cb.set_contents(&ClipboardItem::new().offer_text(""));
    assert_eq!(cb.get_contents(MIME_TEXT_PLAIN), Some(b"".to_vec()));
}

#[test]
fn in_memory_unicode_cjk_round_trip() {
    let mut cb = InMemoryClipboard::new();
    let text = "你好，世界！こんにちは世界안녕하세요";
    cb.set_contents(&ClipboardItem::new().offer_text(text));
    assert_eq!(
        cb.get_contents(MIME_TEXT_PLAIN),
        Some(text.as_bytes().to_vec())
    );
}

#[test]
fn in_memory_unicode_emoji_round_trip() {
    let mut cb = InMemoryClipboard::new();
    let text = "🦀🎉✨🚀❤️👨‍👩‍👧‍👦";
    cb.set_contents(&ClipboardItem::new().offer_text(text));
    assert_eq!(
        cb.get_contents(MIME_TEXT_PLAIN),
        Some(text.as_bytes().to_vec())
    );
}

#[test]
fn in_memory_unicode_rtl_round_trip() {
    let mut cb = InMemoryClipboard::new();
    // Arabic and Hebrew (RTL) text with combining marks.
    let text = "مرحبا بالعالم שלום עולם";
    cb.set_contents(&ClipboardItem::new().offer_text(text));
    assert_eq!(
        cb.get_contents(MIME_TEXT_PLAIN),
        Some(text.as_bytes().to_vec())
    );
}

#[test]
fn in_memory_write_read_clear_round_trip() {
    let mut cb = InMemoryClipboard::new();
    cb.set_contents(&ClipboardItem::new().offer_text("round-trip"));
    assert_eq!(
        cb.get_contents(MIME_TEXT_PLAIN),
        Some(b"round-trip".to_vec())
    );
    cb.clear();
    assert!(cb.get_contents(MIME_TEXT_PLAIN).is_none());
    // Writing again after clear works.
    cb.set_contents(&ClipboardItem::new().offer_text("after-clear"));
    assert_eq!(
        cb.get_contents(MIME_TEXT_PLAIN),
        Some(b"after-clear".to_vec())
    );
}
