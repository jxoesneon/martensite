# Platform Support Matrix

**Document Identifier:** DOC-0001-PLATFORM
**Status:** Maintained
**Target:** v1.0.0

Martensite is a retained-mode GUI framework with a commitment to pure-Rust cross-platform compilation. Tier 1 targets are tested and verified in automated CI.

## Tier 1: Fully Supported, CI-Tested, Guaranteed

Tier 1 platforms are guaranteed to compile, link, and render with 100% feature parity. The core team provides immediate fixes for regressions.

### Windows 10+ (x86_64)
* **GPU Backend:** DirectX 12 (Primary), Vulkan (Secondary), Software Rasterization (`tiny-skia` fallback)
* **Accessibility:** UI Automation (UIA) via AccessKit (100% coverage)
* **IME Support:** Full native candidate window placement
* **Drag and Drop:** Full OLE D&D (files, text, image buffers)
* **Clipboard:** Delayed multi-MIME rendering
* **CI Status:** Fully verified (Build + Lavapipe headless rendering)

### macOS 12+ (aarch64 / x86_64)
* **GPU Backend:** Metal (Primary via `wgpu`), Software Fallback
* **Accessibility:** NSAccessibility via AccessKit (100% coverage)
* **IME Support:** Full CoreText input method integration
* **Drag and Drop:** NSDraggingInfo complete
* **Clipboard:** NSPasteboard multi-format
* **CI Status:** Fully verified (Build)

### Linux (x86_64)
* **GPU Backend:** Vulkan (Primary via `wgpu`), Software Rasterization (Mesa Lavapipe CI fallback)
* **Accessibility:** AT-SPI2 via AccessKit over `zbus`
* **IME Support:** Wayland (zwp_text_input_v3), X11 (XIM fallback)
* **Drag and Drop:** Wayland / X11 generic
* **Clipboard:** Wayland / X11 primary/clipboard buffers
* **Known Limitations:** Proprietary NVIDIA drivers on some Wayland compositors may require fallback to software.
* **CI Status:** Fully verified (Build + Lavapipe headless rendering)

## Tier 2: Best-Effort & Community Tested

Tier 2 platforms compile cleanly via pure-Rust toolchains. They receive CI build checks but may not have fully verified hardware rendering pipelines in continuous integration.

### WebAssembly (wasm32-unknown-unknown)
* **GPU Backend:** WebGPU (Primary), WebGL2 (Fallback)
* **Accessibility:** HTML DOM proxying via AccessKit
* **IME Support:** Browser native `<input>` overlay
* **Limitations:** Multi-threading requires `SharedArrayBuffer` (COOP/COEP headers). File system access is emulated or restricted.

### Linux (aarch64) & FreeBSD (x86_64)
* **GPU Backend:** Vulkan / Software
* **CI Status:** Build verified (`aarch64-unknown-linux-musl`), community tested.

## Tier 3: Experimental

Tier 3 targets represent research domains or incubating hardware architectures.

### Embedded Linux (Framebuffer / DRM/KMS)
* **GPU Backend:** EGL / Software (`tiny-skia`)
* **Limitations:** Direct scanout to DRM/KMS; no window manager. AccessKit is bypassed.
* **CI Status:** Not tested.

### Windows ARM64
* **GPU Backend:** DX12 / Software
* **CI Status:** No automated test coverage.
