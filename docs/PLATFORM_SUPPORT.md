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

### WebAssembly (wasm32-unknown-unknown) — shipped in v0.17.0; compile-verified + headless-browser smoke gate passed
* **GPU Backend:** WebGPU (Primary — required for Vello compute), WebGL2 (downlevel, TinySkia raster fallback only)
* **Accessibility:** **No upstream AccessKit web adapter exists.** v0.17.0 ships `WebA11yBridge`, a hidden-DOM/ARIA mirror; full DOM mirroring is post-1.0 hardening.
* **IME Support:** Hidden `<input>` overlay (canvas has no native IME).
* **Fonts:** Bundled via `fontdb::Source::Binary` + `fetch`; no system fonts.
* **Limitations:** Multi-threading requires `SharedArrayBuffer` (COOP/COEP headers). Clipboard is async + user-gesture gated. File system access is emulated or restricted.
* **Browser floor:** Chrome/Edge 113+ (WebGPU), Firefox 141+ (Windows), Safari 26 (partial). Non-WebGPU browsers render via TinySkia.
* **CI Status:** `cargo check --target wasm32-unknown-unknown` runs on every push (`target-checks` job). **Browser-runtime gate executed 2026-09-15 (macOS 26.5.2, Apple Silicon):** `MARTENSITE_WEB_BROWSER=1` playwright gate (`examples/web/tests/browser_gate.rs`) passed under headless Chromium 140 — trunk-built wasm loaded, GPU backend decision logged, `data-martensite-a11y-mirror` present, `aria-live` load announcement landed. Scope: startup, GPU-backend selection, and the a11y mirror only — clipboard, IME, and drag-and-drop paths are still covered only by the manual checklist in `examples/web/README.md`.

### iOS (aarch64) — shipped in v0.17.0; compile-verified + simulator a11y-adapter smoke passed
* **GPU Backend:** Metal via `wgpu`; `wgpu::Surface` created in `can_create_surfaces`.
* **Accessibility:** `accesskit_ios` `SubclassingAdapter` — **upstream Phase-1 maturity** (basic traits/properties; editable text incomplete).
* **Input:** Unified winit 0.31 `Pointer*` events; `Window::safe_area()` implemented.
* **Packaging:** `staticlib`/`cdylib` + Xcode project (`cargo-mobile2`).
* **CI Status:** `cargo check --target aarch64-apple-ios-sim` runs on every push (`target-checks` job). **Simulator runtime gate executed 2026-09-15:** `MARTENSITE_IOS_SIM_TESTS=1` adapter smoke test (`crates/martensite-access-platform/tests/ios_adapter.rs`) run via `xcrun simctl spawn` on an iPhone 17 Pro simulator (iOS 26.4) — `IosAdapter` subclassed a real `UIView`, `accessibilityElements` exported the materialized AccessKit node, PASS. Scope: AccessKit adapter injection and element enumeration on the simulator — no VoiceOver, no rendering, no physical-device coverage.

### Android (aarch64 / x86_64) — shipped in v0.17.0, compile-verified
* **GPU Backend:** Vulkan (primary), GLES (downlevel fallback); surface destroy/recreate across `destroy_surfaces`/`can_create_surfaces` + `resumed`/`suspended`.
* **Accessibility:** `accesskit_android` `InjectingAdapter` (`embedded-dex`). **Requires `GameActivity`** — `NativeActivity` breaks IME and AccessKit.
* **IME/Input:** `GameActivity` GameText path; `Window::safe_area()` returns zeros on Android — `WindowInsets` platform code until winit lands it.
* **Packaging:** `cdylib` + `cargo-apk2`/`xbuild` — see [android-packaging.md](android-packaging.md).
* **CI Status:** `cargo check --target aarch64-linux-android` runs on every push (`target-checks` job). **On-device/emulator runtime verification (`MARTENSITE_ANDROID_DEVICE` gate) attempted 2026-09-15 and descoped:** the gate must run inside a GameActivity APK process (`cargo apk test`/xbuild + `[package.metadata.android]` manifest metadata), which is not provisioned for `martensite-access-platform`, and the host could not boot an emulator anyway (AVD `Medium_Phone_API_36.1` launch failed — insufficient disk space; an earlier boot reached adb `unauthorized` before shutdown). This platform remains compile-verified only.

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
