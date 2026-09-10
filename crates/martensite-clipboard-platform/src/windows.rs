//! Windows Win32 clipboard backend.
//!
//! Uses the Win32 clipboard API (`OpenClipboard`, `EmptyClipboard`,
//! `SetClipboardData`, `GetClipboardData`, `RegisterClipboardFormat`) to
//! read and write the system clipboard.
//!
//! # Safety
//!
//! All `unsafe` blocks in this module call Win32 API functions that are
//! documented to be safe per Microsoft's documentation:
//! - Clipboard: <https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard>
//! - Memory: <https://learn.microsoft.com/en-us/windows/win32/memory/global-memory-functions>

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use crate::ClipboardBackend;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Ole::CF_UNICODETEXT;

/// A Windows clipboard backend backed by the Win32 clipboard API.
///
/// # Examples
///
/// ```no_run
/// use martensite_clipboard_platform::ClipboardBackend;
/// use martensite_clipboard_platform::windows::Win32Backend;
///
/// let mut cb = Win32Backend::new();
/// cb.write("text/plain;charset=utf-8", b"hello");
/// let read = cb.read("text/plain;charset=utf-8");
/// assert_eq!(read, Some(b"hello".to_vec()));
/// ```
pub struct Win32Backend;

impl Win32Backend {
    /// Creates a new [`Win32Backend`].
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Converts a MIME type to a registered clipboard format (`UINT`), or
    /// falls back to `CF_UNICODETEXT` for plain text.
    fn mime_to_format(mime: &str) -> u32 {
        if mime == "text/plain;charset=utf-8" || mime == "text/plain" {
            return CF_UNICODETEXT.0 as u32;
        }
        // Register a custom format for non-text MIME types.
        let wide: Vec<u16> = OsStr::new(mime)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe { RegisterClipboardFormatW(PCWSTR::from_raw(wide.as_ptr())) }
    }
}

impl Default for Win32Backend {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardBackend for Win32Backend {
    fn write(&mut self, mime: &str, bytes: &[u8]) {
        unsafe {
            if OpenClipboard(None).is_err() {
                return;
            }
            let _ = EmptyClipboard();
            let format = Self::mime_to_format(mime);
            if format == 0 {
                let _ = CloseClipboard();
                return;
            }
            // For CF_UNICODETEXT, encode as UTF-16 with a nul terminator.
            let data: Vec<u8> = if format == CF_UNICODETEXT.0 as u32 {
                let Ok(s) = std::str::from_utf8(bytes) else {
                    let _ = CloseClipboard();
                    return;
                };
                let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
                let raw: &[u8] =
                    std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
                raw.to_vec()
            } else {
                // Append a nul terminator for binary formats.
                let mut data = bytes.to_vec();
                data.push(0);
                data
            };
            let size = data.len();
            let handle = match GlobalAlloc(GMEM_MOVEABLE, size) {
                Ok(h) => h,
                Err(_) => {
                    let _ = CloseClipboard();
                    return;
                }
            };
            let ptr = GlobalLock(handle);
            if ptr.is_null() {
                let _ = GlobalFree(Some(handle));
                let _ = CloseClipboard();
                return;
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, size);
            let _ = GlobalUnlock(handle);
            if SetClipboardData(format, Some(HANDLE(handle.0))).is_err() {
                let _ = GlobalFree(Some(handle));
            }
            let _ = CloseClipboard();
        }
    }

    fn read(&self, mime: &str) -> Option<Vec<u8>> {
        let format = Self::mime_to_format(mime);
        if format == 0 {
            return None;
        }
        unsafe {
            if OpenClipboard(None).is_err() {
                return None;
            }
            let handle = match GetClipboardData(format) {
                Ok(h) => h,
                Err(_) => {
                    let _ = CloseClipboard();
                    return None;
                }
            };
            if handle.is_invalid() {
                let _ = CloseClipboard();
                return None;
            }
            let h = HGLOBAL(handle.0);
            let result = {
                let ptr = GlobalLock(h);
                if ptr.is_null() {
                    None
                } else {
                    // Query the actual allocation size so the slice we scan for
                    // a NUL terminator never extends past the GlobalAlloc'd
                    // object. Constructing a slice with `usize::MAX` length is
                    // undefined behavior; `GlobalSize` bounds it to the real
                    // allocation.
                    let size = GlobalSize(h);
                    if size == 0 {
                        None
                    } else if format == CF_UNICODETEXT.0 as u32 {
                        let wide = std::slice::from_raw_parts(ptr as *const u16, size / 2);
                        let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
                        let s = String::from_utf16_lossy(&wide[..len]);
                        Some(s.into_bytes())
                    } else {
                        let raw = std::slice::from_raw_parts(ptr as *const u8, size);
                        let len = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                        Some(raw[..len].to_vec())
                    }
                }
            };
            let _ = GlobalUnlock(h);
            let _ = CloseClipboard();
            result
        }
    }

    fn available_types(&self) -> Vec<String> {
        // Enumerating registered formats requires EnumClipboardFormats; we
        // report the text type as available if CF_UNICODETEXT is present.
        unsafe {
            if OpenClipboard(None).is_err() {
                return Vec::new();
            }
            let mut types = Vec::new();
            let handle = match GetClipboardData(CF_UNICODETEXT.0 as u32) {
                Ok(h) => h,
                Err(_) => {
                    let _ = CloseClipboard();
                    return types;
                }
            };
            if !handle.is_invalid() {
                types.push("text/plain;charset=utf-8".to_string());
            }
            let _ = CloseClipboard();
            types
        }
    }

    fn clear(&mut self) {
        unsafe {
            if OpenClipboard(None).is_ok() {
                let _ = EmptyClipboard();
                let _ = CloseClipboard();
            }
        }
    }

    fn platform_name(&self) -> &str {
        "windows-ole"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes tests that access the system clipboard to prevent
    /// concurrent read/write races on Windows.
    static CLIPBOARD_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Helper that acquires the clipboard lock for the duration of a test.
    fn clipboard_lock() -> std::sync::MutexGuard<'static, ()> {
        CLIPBOARD_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    const TEXT_PLAIN: &str = "text/plain;charset=utf-8";

    #[test]
    fn platform_name_is_windows() {
        let _lock = clipboard_lock();
        let cb = Win32Backend::new();
        assert_eq!(cb.platform_name(), "windows-ole");
    }

    #[test]
    fn text_round_trip_through_system_clipboard() {
        let _lock = clipboard_lock();
        // This test reads and writes the real system clipboard. It may
        // clobber whatever the user has copied; that is acceptable for a
        // unit test on a CI runner.
        let mut cb = Win32Backend::new();
        cb.write(TEXT_PLAIN, b"martensite-test-123");
        let read = cb.read(TEXT_PLAIN);
        assert_eq!(read, Some(b"martensite-test-123".to_vec()));
    }

    #[test]
    fn clear_empties_text() {
        let _lock = clipboard_lock();
        let mut cb = Win32Backend::new();
        cb.write(TEXT_PLAIN, b"to-be-cleared");
        cb.clear();
        // After clearing, a text read returns None (no data for the type).
        assert!(cb.read(TEXT_PLAIN).is_none());
    }

    #[test]
    fn available_types_includes_text_after_write() {
        let _lock = clipboard_lock();
        let mut cb = Win32Backend::new();
        cb.write(TEXT_PLAIN, b"type-check");
        let types = cb.available_types();
        // The canonical text type should appear among the available types.
        assert!(
            types.iter().any(|t| t == TEXT_PLAIN),
            "expected a text type in {types:?}"
        );
    }

    #[test]
    fn empty_text_write_is_safe() {
        let _lock = clipboard_lock();
        let mut cb = Win32Backend::new();
        cb.write(TEXT_PLAIN, b"");
        // Should not panic; the read may be None or empty.
        let _ = cb.read(TEXT_PLAIN);
    }

    #[test]
    fn unicode_text_round_trip() {
        let _lock = clipboard_lock();
        let mut cb = Win32Backend::new();
        let text = "héllo 世界 🦀";
        cb.write(TEXT_PLAIN, text.as_bytes());
        let read = cb.read(TEXT_PLAIN);
        assert_eq!(read, Some(text.as_bytes().to_vec()));
    }

    #[test]
    fn multiple_writes_overwrite() {
        let _lock = clipboard_lock();
        let mut cb = Win32Backend::new();
        cb.write(TEXT_PLAIN, b"first");
        cb.write(TEXT_PLAIN, b"second");
        let read = cb.read(TEXT_PLAIN);
        assert_eq!(read, Some(b"second".to_vec()));
    }

    #[test]
    fn large_text_round_trip() {
        let _lock = clipboard_lock();
        let mut cb = Win32Backend::new();
        let text = "x".repeat(64 * 1024);
        cb.write(TEXT_PLAIN, text.as_bytes());
        let read = cb.read(TEXT_PLAIN);
        assert_eq!(read, Some(text.into_bytes()));
    }
}
