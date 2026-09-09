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

use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalFree, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Ole::{CF_TEXT, CF_UNICODETEXT};

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
            return CF_UNICODETEXT.0;
        }
        // Register a custom format for non-text MIME types.
        let wide: Vec<u16> = OsStr::new(mime)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe { RegisterClipboardFormatW(wide.as_ptr()) }
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
            let data: Vec<u8> = if format == CF_UNICODETEXT.0 {
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
            let handle = GlobalAlloc(GMEM_MOVEABLE, size);
            if handle.is_err() {
                let _ = CloseClipboard();
                return;
            }
            let handle = handle.ok();
            let ptr = GlobalLock(handle);
            if ptr.is_null() {
                let _ = GlobalFree(handle);
                let _ = CloseClipboard();
                return;
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, size);
            let _ = GlobalUnlock(handle);
            if SetClipboardData(format, handle).is_err() {
                let _ = GlobalFree(handle);
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
            let handle = GetClipboardData(format);
            let result = if handle.is_invalid() {
                None
            } else {
                let ptr = GlobalLock(handle);
                if ptr.is_null() {
                    None
                } else {
                    // Query the actual allocation size so the slice we scan for
                    // a NUL terminator never extends past the GlobalAlloc'd
                    // object. Constructing a slice with `usize::MAX` length is
                    // undefined behavior; `GlobalSize` bounds it to the real
                    // allocation.
                    let size = GlobalSize(handle) as usize;
                    if size == 0 {
                        None
                    } else if format == CF_UNICODETEXT.0 {
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
            if !handle.is_invalid() {
                let _ = GlobalUnlock(handle);
            }
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
            let handle = GetClipboardData(CF_UNICODETEXT.0);
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

    #[test]
    fn platform_name_is_windows() {
        let cb = Win32Backend::new();
        assert_eq!(cb.platform_name(), "windows-ole");
    }
}
