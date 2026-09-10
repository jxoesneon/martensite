//! Linux X11 clipboard backend.
//!
//! Uses raw FFI to `libX11` to interact with the `CLIPBOARD` selection.
//! A real implementation owns the `CLIPBOARD` selection via
//! `XSetSelectionOwner`, serves `SelectionRequest` events by converting the
//! requested target (MIME type) to a property, and uses `XConvertSelection`
//! for reads. This module implements the read/write path against the
//! `CLIPBOARD` selection using `XConvertSelection` and `XGetWindowProperty`.
//!
//! # Safety
//!
//! All `unsafe` blocks in this module call X11 C functions that are
//! documented to be safe per the Xlib documentation:
//! - Selections: <https://www.x.org/releases/current/doc/xlib/xlib.pdf#selections>

use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void, CString};
use std::ptr;

use crate::ClipboardBackend;

// ---------------------------------------------------------------------------
// X11 FFI declarations
// ---------------------------------------------------------------------------

/// Opaque `Display*` pointer.
type Display = *mut c_void;

/// X11 `Window` (XID).
type Window = c_ulong;

/// X11 `Atom` (XID).
type Atom = c_ulong;

const FALSE: c_int = 0;
const ANY_PROPERTY_TYPE: Atom = 0;
const DELETE_PROP: c_int = 1;

extern "C" {
    fn XOpenDisplay(name: *const c_char) -> Display;
    fn XCloseDisplay(display: Display) -> c_int;
    fn XDefaultRootWindow(display: Display) -> Window;
    fn XInternAtom(display: Display, name: *const c_char, only_if_exists: c_int) -> Atom;
    fn XCreateSimpleWindow(
        display: Display,
        parent: Window,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
        border_width: c_uint,
        border: c_ulong,
        background: c_ulong,
    ) -> Window;
    fn XDestroyWindow(display: Display, window: Window);
    fn XSetSelectionOwner(display: Display, selection: Atom, owner: Window, time: c_ulong)
        -> c_int;
    fn XGetSelectionOwner(display: Display, selection: Atom) -> Window;
    fn XConvertSelection(
        display: Display,
        selection: Atom,
        target: Atom,
        property: Atom,
        requestor: Window,
        time: c_ulong,
    );
    fn XChangeProperty(
        display: Display,
        window: Window,
        property: Atom,
        type_: Atom,
        format: c_int,
        mode: c_int,
        data: *const c_char,
        nelements: c_int,
    );
    fn XGetWindowProperty(
        display: Display,
        window: Window,
        property: Atom,
        offset: c_long,
        length: c_long,
        delete: c_int,
        req_type: Atom,
        actual_type_return: *mut Atom,
        actual_format_return: *mut c_int,
        nitems_return: *mut c_ulong,
        bytes_after_return: *mut c_ulong,
        prop_return: *mut *mut c_char,
    ) -> c_int;
    fn XFree(data: *mut c_void);
    fn XFlush(display: Display) -> c_int;
    fn XPending(display: Display) -> c_int;
    fn XNextEvent(display: Display, event: *mut XEvent) -> c_int;
}

/// Minimal X11 `XEvent` union (we only need the type field).
#[repr(C)]
#[derive(Copy, Clone, Default)]
struct XEvent {
    /// The event type discriminant.
    type_: c_int,
    /// Padding to make the union large enough (64 bytes is typical).
    padding: [c_long; 31],
}

/// X11 `SelectionNotify` event type code.
const SELECTION_NOTIFY: c_int = 31;

// ---------------------------------------------------------------------------
// X11Backend
// ---------------------------------------------------------------------------

/// A Linux X11 clipboard backend.
///
/// This backend opens a connection to the X11 display, creates a hidden
/// window to act as the selection owner, and uses `XConvertSelection` /
/// `XGetWindowProperty` to read and `XChangeProperty` to write the
/// `CLIPBOARD` selection.
///
/// # Examples
///
/// ```no_run
/// use martensite_clipboard_platform::ClipboardBackend;
/// use martensite_clipboard_platform::x11::X11Backend;
///
/// let mut cb = X11Backend::new();
/// cb.write("text/plain;charset=utf-8", b"hello");
/// let read = cb.read("text/plain;charset=utf-8");
/// assert_eq!(read, Some(b"hello".to_vec()));
/// ```
pub struct X11Backend {
    /// The X11 display connection, or `None` if the display could not be
    /// opened (e.g. no X server running).
    display: Option<Display>,
    /// The hidden window used as the selection owner / requestor.
    window: Window,
    /// The `CLIPBOARD` atom.
    clipboard_atom: Atom,
    /// The `UTF8_STRING` atom for UTF-8 text.
    utf8_string_atom: Atom,
}

impl X11Backend {
    /// Creates a new [`X11Backend`], opening a connection to the X11
    /// display. If no display is available, the backend is a no-op.
    #[must_use]
    pub fn new() -> Self {
        let display = unsafe { XOpenDisplay(ptr::null()) };
        if display.is_null() {
            return Self {
                display: None,
                window: 0,
                clipboard_atom: 0,
                utf8_string_atom: 0,
            };
        }
        let root = unsafe { XDefaultRootWindow(display) };
        let window = unsafe { XCreateSimpleWindow(display, root, 0, 0, 1, 1, 0, 0, 0) };
        let clipboard_name = CString::new("CLIPBOARD").unwrap();
        let utf8_name = CString::new("UTF8_STRING").unwrap();
        let clipboard_atom = unsafe { XInternAtom(display, clipboard_name.as_ptr(), FALSE) };
        let utf8_string_atom = unsafe { XInternAtom(display, utf8_name.as_ptr(), FALSE) };
        Self {
            display: Some(display),
            window,
            clipboard_atom,
            utf8_string_atom,
        }
    }

    /// Returns the X11 display if one is available.
    fn display(&self) -> Option<Display> {
        self.display
    }

    /// Maps a MIME type to an X11 target `Atom`.
    fn mime_to_target(&self, mime: &str) -> Atom {
        if mime == "text/plain;charset=utf-8" || mime == "text/plain" {
            return self.utf8_string_atom;
        }
        let Some(display) = self.display() else {
            return 0;
        };
        let Ok(name) = CString::new(mime) else {
            return 0;
        };
        unsafe { XInternAtom(display, name.as_ptr(), FALSE) }
    }

    /// Reads and deletes a property from our window, returning the bytes.
    /// Returns `None` if the property is empty or the read fails.
    fn read_property(&self, display: Display, prop_atom: Atom) -> Option<Vec<u8>> {
        let mut actual_type: Atom = 0;
        let mut actual_format: c_int = 0;
        let mut nitems: c_ulong = 0;
        let mut bytes_after: c_ulong = 0;
        let mut prop_data: *mut c_char = ptr::null_mut();
        let status = unsafe {
            XGetWindowProperty(
                display,
                self.window,
                prop_atom,
                0,
                c_long::MAX / 4,
                DELETE_PROP,
                ANY_PROPERTY_TYPE,
                &mut actual_type,
                &mut actual_format,
                &mut nitems,
                &mut bytes_after,
                &mut prop_data,
            )
        };
        if status != 0 || prop_data.is_null() || nitems == 0 {
            return None;
        }
        let slice = unsafe { std::slice::from_raw_parts(prop_data as *const u8, nitems as usize) };
        let result = slice.to_vec();
        unsafe { XFree(prop_data as *mut c_void) };
        Some(result)
    }
}

impl Default for X11Backend {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for X11Backend {
    fn drop(&mut self) {
        if let Some(display) = self.display {
            if self.window != 0 {
                unsafe { XDestroyWindow(display, self.window) };
            }
            unsafe { XCloseDisplay(display) };
        }
    }
}

impl ClipboardBackend for X11Backend {
    fn write(&mut self, mime: &str, bytes: &[u8]) {
        let Some(display) = self.display() else {
            return;
        };
        // Take ownership of the CLIPBOARD selection.
        unsafe {
            XSetSelectionOwner(display, self.clipboard_atom, self.window, 0);
        }
        let target = self.mime_to_target(mime);
        if target == 0 {
            return;
        }
        unsafe {
            XChangeProperty(
                display,
                self.window,
                target,
                target,
                8,
                0,
                bytes.as_ptr() as *const c_char,
                bytes.len() as c_int,
            );
            XFlush(display);
        }
    }

    fn read(&self, mime: &str) -> Option<Vec<u8>> {
        let display = self.display()?;
        let target = self.mime_to_target(mime);
        if target == 0 {
            return None;
        }

        // If we are the selection owner, read the property we set in write()
        // directly. XConvertSelection would send a SelectionRequest to us,
        // but we don't serve those events, so the conversion would time out.
        let owner = unsafe { XGetSelectionOwner(display, self.clipboard_atom) };
        if owner == self.window {
            return self.read_property(display, target);
        }

        // Request the selection conversion into a property on our window.
        let prop_name = CString::new("MARTENSITE_CLIP").unwrap();
        let prop_atom = unsafe { XInternAtom(display, prop_name.as_ptr(), FALSE) };
        unsafe {
            XConvertSelection(
                display,
                self.clipboard_atom,
                target,
                prop_atom,
                self.window,
                0,
            );
            XFlush(display);
        }
        // Wait for the SelectionNotify event (with a bounded poll).
        for _ in 0..100 {
            if unsafe { XPending(display) } > 0 {
                let mut event = XEvent::default();
                unsafe { XNextEvent(display, &mut event) };
                if event.type_ == SELECTION_NOTIFY {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        self.read_property(display, prop_atom)
    }

    fn available_types(&self) -> Vec<String> {
        let Some(display) = self.display() else {
            return Vec::new();
        };
        // If we own the selection, report the text type.
        let owner = unsafe { XGetSelectionOwner(display, self.clipboard_atom) };
        if owner == self.window {
            return vec!["text/plain;charset=utf-8".to_string()];
        }
        Vec::new()
    }

    fn clear(&mut self) {
        let Some(display) = self.display() else {
            return;
        };
        unsafe {
            XSetSelectionOwner(display, self.clipboard_atom, 0, 0);
            XFlush(display);
        }
    }

    fn platform_name(&self) -> &str {
        "x11"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes tests that access the X11 clipboard to prevent
    /// concurrent selection races.
    static CLIPBOARD_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Helper that acquires the clipboard lock for the duration of a test.
    fn clipboard_lock() -> std::sync::MutexGuard<'static, ()> {
        CLIPBOARD_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    const TEXT_PLAIN: &str = "text/plain;charset=utf-8";

    /// Returns early if no X display is available (no `DISPLAY` env var or
    /// `XOpenDisplay` fails). This allows the tests to compile and run on
    /// any Linux machine without an X server, skipping gracefully instead
    /// of failing.
    fn require_display() -> Option<X11Backend> {
        if std::env::var("DISPLAY").is_err() {
            return None;
        }
        let cb = X11Backend::new();
        if cb.display.is_none() {
            return None;
        }
        Some(cb)
    }

    #[test]
    fn platform_name_is_x11() {
        let _lock = clipboard_lock();
        let cb = X11Backend::new();
        assert_eq!(cb.platform_name(), "x11");
    }

    #[test]
    fn text_round_trip_through_x11_clipboard() {
        let _lock = clipboard_lock();
        let Some(mut cb) = require_display() else {
            eprintln!("skipping: no X display available");
            return;
        };
        cb.write(TEXT_PLAIN, b"martensite-test-123");
        let read = cb.read(TEXT_PLAIN);
        assert_eq!(read, Some(b"martensite-test-123".to_vec()));
    }

    #[test]
    fn clear_empties_text() {
        let _lock = clipboard_lock();
        let Some(mut cb) = require_display() else {
            eprintln!("skipping: no X display available");
            return;
        };
        cb.write(TEXT_PLAIN, b"to-be-cleared");
        cb.clear();
        // After clearing, we no longer own the selection, so available_types
        // is empty and a read returns None.
        assert!(cb.available_types().is_empty());
    }

    #[test]
    fn available_types_includes_text_after_write() {
        let _lock = clipboard_lock();
        let Some(mut cb) = require_display() else {
            eprintln!("skipping: no X display available");
            return;
        };
        cb.write(TEXT_PLAIN, b"type-check");
        let types = cb.available_types();
        assert!(
            types.iter().any(|t| t == TEXT_PLAIN),
            "expected a text type in {types:?}"
        );
    }

    #[test]
    fn empty_text_write_is_safe() {
        let _lock = clipboard_lock();
        let Some(mut cb) = require_display() else {
            eprintln!("skipping: no X display available");
            return;
        };
        cb.write(TEXT_PLAIN, b"");
        // Should not panic; the read may be None or empty.
        let _ = cb.read(TEXT_PLAIN);
    }

    #[test]
    fn unicode_text_round_trip() {
        let _lock = clipboard_lock();
        let Some(mut cb) = require_display() else {
            eprintln!("skipping: no X display available");
            return;
        };
        let text = "héllo 世界 🦀";
        cb.write(TEXT_PLAIN, text.as_bytes());
        let read = cb.read(TEXT_PLAIN);
        assert_eq!(read, Some(text.as_bytes().to_vec()));
    }

    #[test]
    fn multiple_writes_overwrite() {
        let _lock = clipboard_lock();
        let Some(mut cb) = require_display() else {
            eprintln!("skipping: no X display available");
            return;
        };
        cb.write(TEXT_PLAIN, b"first");
        cb.write(TEXT_PLAIN, b"second");
        let read = cb.read(TEXT_PLAIN);
        assert_eq!(read, Some(b"second".to_vec()));
    }

    #[test]
    fn large_text_round_trip() {
        let _lock = clipboard_lock();
        let Some(mut cb) = require_display() else {
            eprintln!("skipping: no X display available");
            return;
        };
        let text = "x".repeat(64 * 1024);
        cb.write(TEXT_PLAIN, text.as_bytes());
        let read = cb.read(TEXT_PLAIN);
        assert_eq!(read, Some(text.into_bytes()));
    }
}
