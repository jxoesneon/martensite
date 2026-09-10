//! Cross-platform `MockBackend` tests for `ClipboardBackend`.
//!
//! `martensite-clipboard-platform` has real clipboard round-trips on macOS,
//! but the Windows and X11 backends only have name smoke tests. This file
//! provides a `MockBackend` that implements `ClipboardBackend` using an
//! in-memory `HashMap`, so the full read/write/clear/types contract can be
//! exercised on every platform without touching the real OS clipboard.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use martensite_clipboard_platform::ClipboardBackend;

/// A pure-Rust `ClipboardBackend` backed by a `HashMap<String, Vec<u8>>`.
///
/// This mirrors the `ClipboardBackend` contract without any OS clipboard
/// interaction, enabling cross-platform round-trip tests.
struct MockBackend {
    contents: HashMap<String, Vec<u8>>,
}

impl MockBackend {
    fn new() -> Self {
        Self {
            contents: HashMap::new(),
        }
    }
}

impl ClipboardBackend for MockBackend {
    fn write(&mut self, mime: &str, bytes: &[u8]) {
        self.contents.insert(mime.to_owned(), bytes.to_vec());
    }

    fn read(&self, mime: &str) -> Option<Vec<u8>> {
        self.contents.get(mime).cloned()
    }

    fn available_types(&self) -> Vec<String> {
        self.contents.keys().cloned().collect()
    }

    fn clear(&mut self) {
        self.contents.clear();
    }

    fn platform_name(&self) -> &str {
        "mock"
    }
}

const TEXT_PLAIN: &str = "text/plain;charset=utf-8";

#[test]
fn mock_text_round_trip() {
    let mut cb = MockBackend::new();
    cb.write(TEXT_PLAIN, b"hello-mock");
    let read = cb.read(TEXT_PLAIN);
    assert_eq!(read, Some(b"hello-mock".to_vec()));
}

#[test]
fn mock_multi_mime_round_trip() {
    let mut cb = MockBackend::new();
    cb.write("text/plain;charset=utf-8", b"plain");
    cb.write("text/html", b"<b>html</b>");
    cb.write("application/x-custom", &[1, 2, 3]);
    assert_eq!(cb.read("text/plain;charset=utf-8"), Some(b"plain".to_vec()));
    assert_eq!(cb.read("text/html"), Some(b"<b>html</b>".to_vec()));
    assert_eq!(cb.read("application/x-custom"), Some(vec![1, 2, 3]));
}

#[test]
fn mock_clear_empties() {
    let mut cb = MockBackend::new();
    cb.write(TEXT_PLAIN, b"to-be-cleared");
    assert!(cb.read(TEXT_PLAIN).is_some());
    cb.clear();
    assert!(cb.read(TEXT_PLAIN).is_none());
    assert!(cb.available_types().is_empty());
}

#[test]
fn mock_available_types_after_write() {
    let mut cb = MockBackend::new();
    cb.write("text/plain;charset=utf-8", b"a");
    cb.write("text/html", b"b");
    let mut types = cb.available_types();
    types.sort();
    assert_eq!(
        types,
        vec![
            "text/html".to_owned(),
            "text/plain;charset=utf-8".to_owned(),
        ]
    );
}

#[test]
fn mock_unknown_mime_returns_none() {
    let cb = MockBackend::new();
    assert!(cb.read("application/does-not-exist").is_none());
}

#[test]
fn mock_platform_name() {
    let cb = MockBackend::new();
    assert_eq!(cb.platform_name(), "mock");
}

#[test]
fn mock_multiple_writes_overwrite() {
    let mut cb = MockBackend::new();
    cb.write(TEXT_PLAIN, b"first");
    assert_eq!(cb.read(TEXT_PLAIN), Some(b"first".to_vec()));
    cb.write(TEXT_PLAIN, b"second");
    assert_eq!(cb.read(TEXT_PLAIN), Some(b"second".to_vec()));
}

#[test]
fn mock_large_text_round_trip() {
    let mut cb = MockBackend::new();
    let text = "x".repeat(256 * 1024);
    cb.write(TEXT_PLAIN, text.as_bytes());
    let read = cb.read(TEXT_PLAIN);
    assert_eq!(read, Some(text.into_bytes()));
}

#[test]
fn mock_empty_string_round_trip() {
    let mut cb = MockBackend::new();
    cb.write(TEXT_PLAIN, b"");
    let read = cb.read(TEXT_PLAIN);
    assert_eq!(read, Some(b"".to_vec()));
}

#[test]
fn mock_unicode_cjk_round_trip() {
    let mut cb = MockBackend::new();
    let text = "你好，世界！こんにちは世界안녕하세요";
    cb.write(TEXT_PLAIN, text.as_bytes());
    let read = cb.read(TEXT_PLAIN);
    assert_eq!(read, Some(text.as_bytes().to_vec()));
}

#[test]
fn mock_unicode_emoji_round_trip() {
    let mut cb = MockBackend::new();
    let text = "🦀🎉✨🚀❤️👨‍👩‍👧‍👦";
    cb.write(TEXT_PLAIN, text.as_bytes());
    let read = cb.read(TEXT_PLAIN);
    assert_eq!(read, Some(text.as_bytes().to_vec()));
}

#[test]
fn mock_unicode_rtl_round_trip() {
    let mut cb = MockBackend::new();
    // Arabic and Hebrew (RTL) text with combining marks.
    let text = "مرحبا بالعالم שלום עולם";
    cb.write(TEXT_PLAIN, text.as_bytes());
    let read = cb.read(TEXT_PLAIN);
    assert_eq!(read, Some(text.as_bytes().to_vec()));
}

#[test]
fn mock_write_read_clear_round_trip() {
    let mut cb = MockBackend::new();
    cb.write(TEXT_PLAIN, b"round-trip");
    assert_eq!(cb.read(TEXT_PLAIN), Some(b"round-trip".to_vec()));
    cb.clear();
    assert!(cb.read(TEXT_PLAIN).is_none());
    // Writing again after clear works.
    cb.write(TEXT_PLAIN, b"after-clear");
    assert_eq!(cb.read(TEXT_PLAIN), Some(b"after-clear".to_vec()));
}

#[test]
fn mock_available_types_after_clear_is_empty() {
    let mut cb = MockBackend::new();
    cb.write("text/plain;charset=utf-8", b"a");
    cb.write("text/html", b"b");
    assert_eq!(cb.available_types().len(), 2);
    cb.clear();
    assert!(cb.available_types().is_empty());
}

#[test]
fn mock_binary_payload_round_trip() {
    let mut cb = MockBackend::new();
    let payload = vec![0x00, 0x01, 0xFF, 0xFE, 0xDE, 0xAD, 0xBE, 0xEF];
    cb.write("application/octet-stream", &payload);
    assert_eq!(cb.read("application/octet-stream"), Some(payload.clone()));
    let types = cb.available_types();
    assert_eq!(types, vec!["application/octet-stream".to_string()]);
}
