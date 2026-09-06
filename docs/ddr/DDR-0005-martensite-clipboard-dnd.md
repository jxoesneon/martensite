# Detailed Design Record: DDR-0005
## Title: `martensite-clipboard` & `martensite-dnd` Delayed Rendering Protocols

### 1. Architectural Role & Invariants
`martensite-clipboard` and `martensite-dnd` manage inter-application data exchange across Windows, macOS, and Linux.
* **Invariant 1.1 (Multi-MIME Simultaneous Registration)**: Copy operations must simultaneously register multiple formats (`text/plain`, `text/html`, `image/png`, and custom binary MIME).
* **Invariant 1.2 (Zero-Allocation Delayed Rendering)**: Heavy payloads (e.g., 4K bitmap captures, large 3D scene graphs) are registered as **lazy promises**. Data is synthesized only when requested by the destination application.

---

### 2. Platform Lazy Clipboard Engine Implementations

```rust
pub enum ClipboardDataPromise {
    Immediate(Vec<u8>),
    Lazy(Box<dyn Fn() -> Vec<u8> + Send + Sync>),
}

pub struct ClipboardItem {
    pub mime_type: String,
    pub payload: ClipboardDataPromise,
}

pub trait PlatformClipboard {
    fn set_contents(&mut self, items: Vec<ClipboardItem>) -> Result<(), Box<dyn std::error::Error>>;
    fn get_available_mimes(&self) -> Vec<String>;
    fn read_mime(&mut self, mime: &str) -> Option<Vec<u8>>;
}
```

* **Windows OLE**: Implements the COM `IDataObject` vtable with delayed rendering in `IDataObject::GetData`.
* **macOS Cocoa**: Registers types on `NSPasteboard` using `NSPasteboardItemDataProvider` callbacks.
* **Linux Wayland**: Responds to `wl_data_source.send` events by writing on demand to the provided file descriptor.
