//! macOS `NSPasteboard` clipboard backend.
//!
//! Uses the Objective-C runtime (`libobjc`) to interact with
//! `NSPasteboard`'s `generalPasteboard`, `clearContents`,
//! `setData:forType:`, `dataForType:`, and `types` APIs. This is the same
//! pasteboard that the system clipboard (Copy/Paste) uses.
//!
//! # Safety
//!
//! All `unsafe` blocks in this module call Objective-C runtime functions
//! (`objc_msgSend`, `objc_getClass`, `sel_registerName`) and AppKit methods
//! that are documented to be safe per Apple's documentation:
//! - `NSPasteboard`: <https://developer.apple.com/documentation/appkit/nspasteboard>
//! - `NSString`: <https://developer.apple.com/documentation/foundation/nsstring>
//! - `NSData`: <https://developer.apple.com/documentation/foundation/nsdata>
//!
//! The Objective-C runtime is reference-counted (ARC/manual retain-release).
//! We follow the "create rule": objects returned by methods whose names do
//! not begin with "alloc", "new", "copy", or "mutableCopy" are autoreleased
//! and we do not need to release them. Objects we create via
//! `dataWithBytes:length:` and `stringWithUTF8String:` are autoreleased
//! convenience constructors, so we also do not release those.

use std::ffi::{c_char, c_void, CStr};
use std::os::raw::c_ulong;

use crate::ClipboardBackend;

// ---------------------------------------------------------------------------
// Objective-C runtime FFI
// ---------------------------------------------------------------------------

/// Opaque pointer to an Objective-C class or instance (`id`).
type Id = *mut c_void;

/// Opaque pointer to a selector (`SEL`).
type Sel = *mut c_void;

/// Objective-C `BOOL` is a signed char (`YES` = 1, `NO` = 0).
type ObjcBool = c_char;

/// Objective-C `NSUInteger`.
type NSUInteger = c_ulong;

extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;
    // `objc_msgSend` is NOT declared as variadic (`...`). On ARM64 (Apple
    // Silicon) the variadic calling convention passes extra arguments on
    // the stack, which would corrupt the Objective-C message dispatch.
    // Instead we declare the base 2-argument form and `transmute` the
    // function pointer to the exact signature needed for each call site.
    fn objc_msgSend(obj: Id, sel: Sel) -> Id;
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Returns the Objective-C class for the given name, or null if not found.
fn class(name: &str) -> Id {
    let c_name = std::ffi::CString::new(name).expect("class name has no nul bytes");
    // SAFETY: `c_name` is a live, NUL-terminated `CString` for the duration
    // of the call; `objc_getClass` only reads it and returns a valid class
    // `id` or null, which callers check before use.
    unsafe { objc_getClass(c_name.as_ptr()) }
}

/// Returns (and registers if needed) the selector for the given name.
fn sel(name: &str) -> Sel {
    let c_name = std::ffi::CString::new(name).expect("selector name has no nul bytes");
    // SAFETY: `c_name` is a live, NUL-terminated `CString` for the duration
    // of the call; `sel_registerName` only reads it and always returns a
    // valid `SEL`.
    unsafe { sel_registerName(c_name.as_ptr()) }
}

/// Sends a message that returns an object (`id`).
///
/// # Safety
///
/// `obj` must be a valid Objective-C receiver (or null, in which case the
/// runtime returns nil/0 without dereferencing), and `selector` must name
/// a method whose signature matches `fn(Id, Sel) -> Id`.
unsafe fn send(obj: Id, selector: Sel) -> Id {
    // SAFETY: the caller's contract guarantees `obj` is a valid receiver
    // and `selector` matches this signature; the declared base signature
    // is used directly with no transmute.
    objc_msgSend(obj, selector)
}

/// Sends a message with one object argument that returns an object.
///
/// # Safety
///
/// `obj` must be a valid Objective-C receiver and `selector` must name a
/// method taking one `id` parameter and returning `id`.
unsafe fn send1(obj: Id, selector: Sel, arg: Id) -> Id {
    // SAFETY: `objc_msgSend` is a signature-agnostic trampoline that reads
    // arguments per the platform C ABI, so casting its base 2-argument
    // function pointer to the exact 3-argument signature is the standard
    // idiom. All parameter types are pointer-width and match the method's
    // declared parameters.
    let f: unsafe extern "C" fn(Id, Sel, Id) -> Id =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel) -> Id);
    // SAFETY: `f` is the `objc_msgSend` entry point cast to the matching
    // arity; the caller's contract guarantees `obj`, `selector`, and `arg`
    // match the target method.
    f(obj, selector, arg)
}

/// Sends a message with one pointer argument that returns an object.
///
/// # Safety
///
/// `obj` must be a valid Objective-C receiver, `selector` must name a
/// method taking one pointer parameter and returning `id`, and `arg`
/// must satisfy that parameter's validity requirements.
unsafe fn send_ptr(obj: Id, selector: Sel, arg: *const c_void) -> Id {
    // SAFETY: `objc_msgSend` is a signature-agnostic trampoline; the cast
    // only reinterprets the function pointer (same bit pattern, same
    // calling convention) to add one pointer-width parameter.
    let f: unsafe extern "C" fn(Id, Sel, *const c_void) -> Id =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel) -> Id);
    // SAFETY: `f` is the `objc_msgSend` entry point cast to the matching
    // arity; the caller's contract guarantees the receiver, selector, and
    // pointer argument match the target method.
    f(obj, selector, arg)
}

/// Sends a message with one pointer and one `NSUInteger` argument that
/// returns an object.
///
/// # Safety
///
/// `obj` must be a valid Objective-C receiver, `selector` must name a
/// method taking a pointer and an `NSUInteger` and returning `id`, and
/// `arg1` must satisfy the pointer parameter's validity requirements.
unsafe fn send_ptr_usize(obj: Id, selector: Sel, arg1: *const c_void, arg2: NSUInteger) -> Id {
    // SAFETY: `objc_msgSend` is a signature-agnostic trampoline; the cast
    // reinterprets the function pointer (same bit pattern, same calling
    // convention) to add a pointer and an `NSUInteger` parameter, both
    // pointer-width.
    let f: unsafe extern "C" fn(Id, Sel, *const c_void, NSUInteger) -> Id =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel) -> Id);
    // SAFETY: `f` is the `objc_msgSend` entry point cast to the matching
    // arity; the caller's contract guarantees the arguments match the
    // target method.
    f(obj, selector, arg1, arg2)
}

/// Sends a message that returns a `BOOL`.
///
/// # Safety
///
/// `obj` must be a valid Objective-C receiver and `selector` must name a
/// method taking no parameters and returning `BOOL`.
unsafe fn send_bool(obj: Id, selector: Sel) -> ObjcBool {
    // SAFETY: `objc_msgSend` is a signature-agnostic trampoline; the cast
    // reinterprets the function pointer to a signature returning `BOOL`
    // (a `signed char` returned in the same integer return register as
    // `id`, per the platform ABI).
    let f: unsafe extern "C" fn(Id, Sel) -> ObjcBool =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel) -> Id);
    // SAFETY: `f` is the `objc_msgSend` entry point cast to the matching
    // signature; the caller's contract guarantees the receiver and
    // selector match a `BOOL`-returning method.
    f(obj, selector)
}

/// Sends a message with two object arguments that returns a `BOOL`.
///
/// # Safety
///
/// `obj` must be a valid Objective-C receiver and `selector` must name a
/// method taking two `id` parameters and returning `BOOL`.
unsafe fn send2_bool(obj: Id, selector: Sel, arg1: Id, arg2: Id) -> ObjcBool {
    // SAFETY: `objc_msgSend` is a signature-agnostic trampoline; the cast
    // reinterprets the function pointer (same bit pattern, same calling
    // convention) to add two pointer-width parameters and a `BOOL`
    // (integer-register) return.
    let f: unsafe extern "C" fn(Id, Sel, Id, Id) -> ObjcBool =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel) -> Id);
    // SAFETY: `f` is the `objc_msgSend` entry point cast to the matching
    // arity; the caller's contract guarantees the arguments match the
    // target method.
    f(obj, selector, arg1, arg2)
}

/// Sends a message that returns an `NSUInteger`.
///
/// # Safety
///
/// `obj` must be a valid Objective-C receiver and `selector` must name a
/// method taking no parameters and returning `NSUInteger`.
unsafe fn send_usize(obj: Id, selector: Sel) -> NSUInteger {
    // SAFETY: `objc_msgSend` is a signature-agnostic trampoline; the cast
    // reinterprets the function pointer to a signature returning
    // `NSUInteger`, which shares the integer return register and width
    // with `id` on 64-bit platforms.
    let f: unsafe extern "C" fn(Id, Sel) -> NSUInteger =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel) -> Id);
    // SAFETY: `f` is the `objc_msgSend` entry point cast to the matching
    // signature; the caller's contract guarantees the receiver and
    // selector match an `NSUInteger`-returning method.
    f(obj, selector)
}

/// Sends a message with one `NSUInteger` argument that returns an object.
///
/// # Safety
///
/// `obj` must be a valid Objective-C receiver and `selector` must name a
/// method taking one `NSUInteger` parameter and returning `id`.
unsafe fn send1_index(obj: Id, selector: Sel, arg: NSUInteger) -> Id {
    // SAFETY: `objc_msgSend` is a signature-agnostic trampoline; the cast
    // reinterprets the function pointer (same bit pattern, same calling
    // convention) to add one pointer-width `NSUInteger` parameter.
    let f: unsafe extern "C" fn(Id, Sel, NSUInteger) -> Id =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel) -> Id);
    // SAFETY: `f` is the `objc_msgSend` entry point cast to the matching
    // arity; the caller's contract guarantees the receiver, selector, and
    // index match the target method.
    f(obj, selector, arg)
}

/// Creates an autoreleased `NSString` from a UTF-8 byte slice.
///
/// Returns null if the input is empty or contains invalid UTF-8.
fn nsstring_from_bytes(bytes: &[u8]) -> Id {
    if bytes.is_empty() {
        return std::ptr::null_mut();
    }
    let Ok(s) = std::str::from_utf8(bytes) else {
        return std::ptr::null_mut();
    };
    let c_s = std::ffi::CString::new(s).unwrap_or_default();
    let nsstring = class("NSString");
    // SAFETY: `nsstring` is the `NSString` class object (or null, which
    // `objc_msgSend` tolerates), the selector matches
    // `+stringWithUTF8String:` which takes a `const char*`, and `c_s` is a
    // live, NUL-terminated buffer read only for the duration of the call.
    // The result is an autoreleased `NSString*` we do not own.
    unsafe {
        send_ptr(
            nsstring,
            sel("stringWithUTF8String:"),
            c_s.as_ptr() as *const c_void,
        )
    }
}

/// Creates an autoreleased `NSData` from a byte slice.
fn nsdata_from_bytes(bytes: &[u8]) -> Id {
    let nsdata = class("NSData");
    // SAFETY: `nsdata` is the `NSData` class object (or null, which
    // `objc_msgSend` tolerates), the selector matches
    // `+dataWithBytes:length:` which copies `arg2` bytes from `arg1`
    // during the call, and `bytes` is a live slice of that length.
    // The result is an autoreleased `NSData*` we do not own.
    unsafe {
        send_ptr_usize(
            nsdata,
            sel("dataWithBytes:length:"),
            bytes.as_ptr() as *const c_void,
            bytes.len() as NSUInteger,
        )
    }
}

/// Returns the bytes of an `NSData` object, or `None` if it is null.
fn nsdata_to_bytes(data: Id) -> Option<Vec<u8>> {
    if data.is_null() {
        return None;
    }
    // SAFETY: `data` is non-null (checked above) and `-length` returns an
    // `NSUInteger`, matching the `send_usize` contract.
    let length = unsafe { send_usize(data, sel("length")) } as usize;
    if length == 0 {
        return Some(Vec::new());
    }
    // SAFETY: `data` is a valid `NSData*` and `-bytes` returns a pointer
    // to its contents, which remain valid while the object lives (it is
    // autoreleased and still alive here).
    let bytes_ptr = unsafe { send(data, sel("bytes")) } as *const u8;
    if bytes_ptr.is_null() {
        return Some(Vec::new());
    }
    // SAFETY: `bytes_ptr` is non-null (checked above) and points to
    // `length` readable bytes owned by the live `NSData`; the slice only
    // borrows them until copied into the `Vec` below.
    let slice = unsafe { std::slice::from_raw_parts(bytes_ptr, length) };
    Some(slice.to_vec())
}

/// Returns the UTF-8 string contents of an `NSString`, or `None` if null.
fn nsstring_to_bytes(string: Id) -> Option<Vec<u8>> {
    if string.is_null() {
        return None;
    }
    // SAFETY: `string` is a non-null `NSString*` (checked above) and
    // `-UTF8String` returns a `const char*` valid for the object's
    // autorelease lifetime.
    let c_str_ptr = unsafe { send(string, sel("UTF8String")) } as *const c_char;
    if c_str_ptr.is_null() {
        return None;
    }
    // SAFETY: `c_str_ptr` is non-null (checked above) and points to a
    // NUL-terminated UTF-8 string owned by the still-alive `NSString`;
    // the borrow ends when the bytes are copied below.
    let c_str = unsafe { CStr::from_ptr(c_str_ptr) };
    Some(c_str.to_bytes().to_vec())
}

/// Returns the `NSArray` of type strings (`NSString`) on the pasteboard.
fn pasteboard_types(pasteboard: Id) -> Vec<String> {
    // SAFETY: `pasteboard` is non-null (checked by callers) and `-types`
    // returns an autoreleased `NSArray*` (or nil, checked below).
    let types_array = unsafe { send(pasteboard, sel("types")) };
    if types_array.is_null() {
        return Vec::new();
    }
    // SAFETY: `types_array` is a non-null `NSArray*` and `-count` returns
    // an `NSUInteger`, matching the `send_usize` contract.
    let count = unsafe { send_usize(types_array, sel("count")) } as usize;
    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        // SAFETY: `types_array` is a valid `NSArray*` and `i` is a valid
        // index (`i < count`), so `-objectAtIndex:` returns a valid
        // `NSString*` element.
        let type_str = unsafe { send1_index(types_array, sel("objectAtIndex:"), i as NSUInteger) };
        if let Some(bytes) = nsstring_to_bytes(type_str) {
            if let Ok(s) = std::str::from_utf8(&bytes) {
                result.push(s.to_string());
            }
        }
    }
    result
}

/// Maps a Martensite MIME type to the NSPasteboard type string used for text.
///
/// `NSPasteboardTypeString` (`public.utf8-plain-text`) is the canonical type
/// for UTF-8 text on macOS. All other MIME types are passed through as-is.
fn mime_to_pasteboard_type(mime: &str) -> String {
    if mime == "text/plain;charset=utf-8" || mime == "text/plain" {
        "public.utf8-plain-text".to_string()
    } else {
        mime.to_string()
    }
}

/// Maps an NSPasteboard type string back to a Martensite MIME type.
fn pasteboard_type_to_mime(pasteboard_type: &str) -> String {
    if pasteboard_type == "public.utf8-plain-text" || pasteboard_type == "public.text" {
        "text/plain;charset=utf-8".to_string()
    } else {
        pasteboard_type.to_string()
    }
}

// ---------------------------------------------------------------------------
// MacosBackend
// ---------------------------------------------------------------------------

/// A macOS clipboard backend backed by `NSPasteboard`.
///
/// Reads and writes go through the system pasteboard (`generalPasteboard`),
/// so text copied in another application is visible here and vice versa.
///
/// # Thread safety
///
/// `NSPasteboard` is documented as thread-safe by Apple. The Objective-C
/// runtime uses an autorelease pool per thread; on non-main threads the
/// autoreleased objects returned by convenience constructors are drained
/// when the thread's autorelease pool is popped. This backend does not
/// retain any objects across calls, so there is no reference-counting
/// hazard.
///
/// # Examples
///
/// ```no_run
/// use martensite_clipboard_platform::ClipboardBackend;
/// use martensite_clipboard_platform::macos::MacosBackend;
///
/// let mut cb = MacosBackend::new();
/// cb.write("text/plain;charset=utf-8", b"hello");
/// let read = cb.read("text/plain;charset=utf-8");
/// assert_eq!(read, Some(b"hello".to_vec()));
/// ```
pub struct MacosBackend;

impl MacosBackend {
    /// Creates a new [`MacosBackend`] that reads and writes the system
    /// `generalPasteboard`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite_clipboard_platform::macos::MacosBackend;
    ///
    /// let cb = MacosBackend::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Returns the system `generalPasteboard` (`NSPasteboard` instance).
    fn general_pasteboard() -> Id {
        let cls = class("NSPasteboard");
        // SAFETY: `cls` is the `NSPasteboard` class object (or null, which
        // `objc_msgSend` tolerates by returning nil) and `+generalPasteboard`
        // returns an autoreleased singleton we do not own.
        unsafe { send(cls, sel("generalPasteboard")) }
    }
}

impl Default for MacosBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardBackend for MacosBackend {
    fn write(&mut self, mime: &str, bytes: &[u8]) {
        let pasteboard = Self::general_pasteboard();
        if pasteboard.is_null() {
            return;
        }
        // Clear the pasteboard first.
        // SAFETY: `pasteboard` is non-null (checked above) and
        // `-clearContents` takes no arguments and returns `BOOL`,
        // matching the `send_bool` contract.
        unsafe {
            send_bool(pasteboard, sel("clearContents"));
        }
        let pasteboard_type = mime_to_pasteboard_type(mime);
        let type_string = nsstring_from_bytes(pasteboard_type.as_bytes());
        if type_string.is_null() {
            return;
        }
        let data = nsdata_from_bytes(bytes);
        if data.is_null() {
            return;
        }
        // SAFETY: `pasteboard`, `data`, and `type_string` are all non-null
        // (checked above) and `-setData:forType:` takes two `id` arguments
        // and returns `BOOL`, matching the `send2_bool` contract. The
        // pasteboard retains the data, so no lifetime hazard remains after
        // the call.
        unsafe {
            send2_bool(pasteboard, sel("setData:forType:"), data, type_string);
        }
    }

    fn read(&self, mime: &str) -> Option<Vec<u8>> {
        let pasteboard = Self::general_pasteboard();
        if pasteboard.is_null() {
            return None;
        }
        let pasteboard_type = mime_to_pasteboard_type(mime);
        let type_string = nsstring_from_bytes(pasteboard_type.as_bytes());
        if type_string.is_null() {
            return None;
        }
        // SAFETY: `pasteboard` and `type_string` are non-null (checked
        // above) and `-dataForType:` takes one `id` and returns `id`
        // (an autoreleased `NSData*` or nil), matching the `send1`
        // contract. Null is handled inside `nsdata_to_bytes`.
        let data = unsafe { send1(pasteboard, sel("dataForType:"), type_string) };
        nsdata_to_bytes(data)
    }

    fn available_types(&self) -> Vec<String> {
        let pasteboard = Self::general_pasteboard();
        if pasteboard.is_null() {
            return Vec::new();
        }
        pasteboard_types(pasteboard)
            .into_iter()
            .map(|t| pasteboard_type_to_mime(&t))
            .collect()
    }

    fn clear(&mut self) {
        let pasteboard = Self::general_pasteboard();
        if pasteboard.is_null() {
            return;
        }
        // SAFETY: `pasteboard` is non-null (checked above) and
        // `-clearContents` takes no arguments and returns `BOOL`,
        // matching the `send_bool` contract.
        unsafe {
            send_bool(pasteboard, sel("clearContents"));
        }
    }

    fn platform_name(&self) -> &str {
        "macos-nspasteboard"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes tests that access the system clipboard to prevent
    /// concurrent read/write races that cause SIGSEGV on macOS.
    static CLIPBOARD_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Helper that acquires the clipboard lock for the duration of a test.
    fn clipboard_lock() -> std::sync::MutexGuard<'static, ()> {
        CLIPBOARD_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn platform_name_is_macos() {
        let _lock = clipboard_lock();
        let cb = MacosBackend::new();
        assert_eq!(cb.platform_name(), "macos-nspasteboard");
    }

    #[test]
    fn text_round_trip_through_system_clipboard() {
        let _lock = clipboard_lock();
        // This test reads and writes the real system clipboard. It may
        // clobber whatever the user has copied; that is acceptable for a
        // unit test on a development machine.
        let mut cb = MacosBackend::new();
        cb.write("text/plain;charset=utf-8", b"martensite-test-123");
        let read = cb.read("text/plain;charset=utf-8");
        assert_eq!(read, Some(b"martensite-test-123".to_vec()));
    }

    #[test]
    fn clear_empties_text() {
        let _lock = clipboard_lock();
        let mut cb = MacosBackend::new();
        cb.write("text/plain;charset=utf-8", b"to-be-cleared");
        cb.clear();
        // After clearing, a text read returns None (no data for the type).
        assert!(cb.read("text/plain;charset=utf-8").is_none());
    }

    #[test]
    fn available_types_includes_text_after_write() {
        let _lock = clipboard_lock();
        let mut cb = MacosBackend::new();
        cb.write("text/plain;charset=utf-8", b"type-check");
        let types = cb.available_types();
        // The canonical text type should appear among the available types.
        assert!(
            types
                .iter()
                .any(|t| t.contains("utf8-plain-text") || t == "text/plain;charset=utf-8"),
            "expected a text type in {types:?}"
        );
    }

    #[test]
    fn empty_text_write_is_safe() {
        let _lock = clipboard_lock();
        let mut cb = MacosBackend::new();
        cb.write("text/plain;charset=utf-8", b"");
        // Should not panic; the read may be None or empty.
        let _ = cb.read("text/plain;charset=utf-8");
    }
}
