# Martensite Milestone Specification Suite

This directory contains the formal, granular engineering specifications for each development milestone leading to Martensite v1.0.0. Each document defines explicit architectural deliverables, data structures, entry and exit criteria, verified invariants, performance gates, and failure recovery protocols.

---

## Milestone Execution Sequence

```mermaid
graph TD
    M1["v0.1.0: Foundation<br/>(Core Arena & Reactive DAG)"] --> M2["v0.2.0: Render Pipeline<br/>(WGPU, Vello, TinySkia)"]
    M2 --> M3["v0.3.0: Text & Layout<br/>(Cosmic-Text, Taffy Two-Pass)"]
    M3 --> M4["v0.4.0: Accessibility & Focus<br/>(AccessKit, 2D Spatial Focus)"]
    M4 --> M5["v0.5.0: Input & Platform<br/>(Clipboard, DnD, IME)"]
    M5 --> M6["v0.6.0: Motion & Theme<br/>(Spring Physics, Oklab)"]
    M6 --> M7["v0.7.0: Subsystems<br/>(Undo/Redo LCA, VFS, Fluent)"]
    M7 --> M8["v0.8.0: Media & HDR<br/>(NV12/P010 Zero-Copy, Display EOTF)"]
    M8 --> M9["v0.9.0: Developer Experience<br/>(Tracy, Hot-Reload, Headless CI)"]
    M9 --> M10["v0.10.0: Plugins & Ecosystem<br/>(Wasmtime ABI, Blessed Widgets, Fuzzing)"]
    M10 --> M11["v0.11.0: Typography & A11y<br/>(BiDi UAX #9, Vertical Text, Font Cascades, WCAG 2.2)"]
    M11 --> M12["v0.12.0: Blessed Widgets & Kinematics<br/>(1M-Row DataGrid, BSP Docking, 0.55 Rubber-Band)"]
    M12 --> M13["v0.13.0: Modern Shell & Platform<br/>(Win11 Mica, macOS Liquid Glass, Wayland CSD)"]
    M13 --> M14["v0.14.0: Engine Embedding & Media<br/>(Bevy/Godot Bridge, Zero-Copy Surfaces, BT.2408 HDR)"]
    M14 --> M15["v1.0.0: Production Stability & Distribution<br/>(API Freeze, WiX/DMG/Flatpak, Ed25519 Updates, Crates.io)"]
```

---

## Master Milestone Matrix

| Milestone | Specification Document | Primary Target Crates | Core Architectural Focus | Exit Verification Gate |
| :--- | :--- | :--- | :--- | :--- |
| **v0.1.0** | [v0.1.0-foundation.md](v0.1.0-foundation.md) | `martensite-core`, `martensite-reactive` | Generational arena, 64-byte HotNode, push-pull reactive scheduler | 10k DAG propagation <1.0ms; zero generational collisions |
| **v0.2.0** | [v0.2.0-render-pipeline.md](v0.2.0-render-pipeline.md) | `martensite-wgpu`, `martensite-render`, `martensite-window` | WGPU surface lifecycle, Vello Scene translation, TinySkia CPU fallback | Device loss restored <16.6ms; >=99.9% DSSIM fallback parity |
| **v0.3.0** | [v0.3.0-text-layout.md](v0.3.0-text-layout.md) | `martensite-text`, `martensite-layout` | Two-tier text cache, Taffy two-pass layout bridge, zero 1-frame lag | 1,000 flex nodes laid out in <0.5ms; text hit rate >98% |
| **v0.4.0** | [v0.4.0-accessibility-focus.md](v0.4.0-accessibility-focus.md) | `martensite-access`, `martensite-focus` | AccessKit semantic tree sync, 2D projected-beam focus navigation | 100% WCAG 2.1 AA tree compliance; deterministic 2D tab order |
| **v0.5.0** | [v0.5.0-input-platform.md](v0.5.0-input-platform.md) | `martensite-clipboard`, `martensite-dnd`, `martensite-window` | Detached DnD session, multi-MIME clipboard, velocity-damped IME | Multi-window drag survival; IME position alignment during kinetic scroll |
| **v0.6.0** | [v0.6.0-motion-theme.md](v0.6.0-motion-theme.md) | `martensite-motion`, `martensite-theme` | Closed-form analytical spring solver, Oklab perceptual color blending | Analytical accuracy within 1e-6; zero allocation in animation loop |
| **v0.7.0** | [v0.7.0-subsystems.md](v0.7.0-subsystems.md) | `martensite-history`, `martensite-assets`, `martensite-l10n` | LCA tree undo/redo ledger, dual-mode VFS, Project Fluent l10n | 10k transaction rollback integrity; <10µs memory VFS resolution |
| **v0.8.0** | [v0.8.0-media-hdr.md](v0.8.0-media-hdr.md) | `martensite-media`, `martensite-wgpu` | Zero-copy NV12/P010 surfaces, BT.2020 PQ EOTF, adaptive SDR luminance | 4K 60fps at ~0.0ms CPU dispatch; Delta E < 1.0 color accuracy |
| **v0.9.0** | [v0.9.0-developer-experience.md](v0.9.0-developer-experience.md) | `martensite-devtools`, `cargo-martensite`, `martensite-macros`, `martensite-test` | In-app Tracy HUD, cdylib hot-reload CLI, VirtualClock headless test harness | Hot reload code swap <350ms; deterministic headless golden frames |
| **v0.10.0** | [v0.10.0-plugins-ecosystem.md](v0.10.0-plugins-ecosystem.md) | `martensite-plugin`, `martensite-blessed` | Wasmtime ring-buffer ABI, blessed widget suite, 48h soak fuzzing | Plugin execution <2.0ms/frame; 48h continuous fuzz with 0 panics |
| **v0.11.0** | [v0.11.0-typography-a11y.md](v0.11.0-typography-a11y.md) | `martensite-text`, `martensite-access`, `martensite-layout` | Swash/HarfBuzz BiDi (UAX #9), vertical-rl (UAX #50), system font cascades, WCAG 2.2 AA/AAA, Section 508 VPAT, caret tracking | 100% BiDi/vertical layout parity; 0 missing glyphs; WCAG 2.2 AAA pass; caret sync <16.6ms |
| **v0.12.0** | [v0.12.0-blessed-kinematics.md](v0.12.0-blessed-kinematics.md) | `martensite-blessed`, `martensite-motion`, `martensite-window` | 1,000,000-Row virtualized DataGrid, BSP docking tree with multi-swapchains, 0.55 rubber-banding, 6-DoF Kalman stylus | 1M-row scroll steady 120fps; zero allocation during docking split; Kalman latency <2.0ms |
| **v0.13.0** | [v0.13.0-modern-shell.md](v0.13.0-modern-shell.md) | `martensite-window`, `martensite-shell`, `martensite-theme` | Windows 11 Mica / Mica Alt / Acrylic & Snap Layouts, macOS Liquid Glass vibrancy, Wayland wp_fractional_scale_v1 & StatusNotifierItem | DWM backdrop switch <8ms; zero blur under fractional Wayland DPI; native CSD hit-testing |
| **v0.14.0** | `v0.14.0-embedding-media.md` | `martensite-engine-bridge`, `martensite-media`, `martensite-wgpu` | Bevy ECS plugin, Godot 4 GDExtension RenderingDevice, DXGI / IOSurface / dma-buf zero-copy surfaces, ITU-R BT.2408 SDR white scaling | In-pass Bevy/Godot UI compositing at zero GPU copy; 4K 120fps video playback with <0.1% frame drops |
| **v1.0.0** | [v1.0.0-production-release.md](v1.0.0-production-release.md) | Complete Workspace (22+ crates) | Public API freeze, 100% docs.rs coverage, security audit sign-off | Zero compiler warnings; cargo audit clean; Crates.io release |

---

## Milestone Verification Governance

Every milestone implementation must fulfill three verification stages before merging:
1. **Compilation & Static Checks:** `cargo check --workspace` and `cargo clippy --workspace -- -D warnings` must exit cleanly with zero warnings.
2. **Automated Test Suite:** `cargo test --workspace` must achieve 100% pass rate with zero flaky tests.
3. **Milestone Exit Gate Audit:** The criteria explicitly defined in the corresponding milestone specification must be validated on CI reference runners across Windows, macOS, and Linux.
