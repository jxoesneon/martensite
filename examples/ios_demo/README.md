# ios_demo — Martensite iOS shell

Minimal iOS demo exercising the v0.17.0 iOS surface: the winit 0.31 UIKit
lifecycle (`can_create_surfaces` / `resumed` / `suspended` /
`destroy_surfaces`), `Window::safe_area()`, touch events routed with
`PointerKind`/`PointerId` preservation, and the `WindowEvent::Ime` stream.

The crate compiles as a **`staticlib`** (`libios_demo.a`) for
`aarch64-apple-ios` (device) and `aarch64-apple-ios-sim` (simulator) so it
can be linked into an Xcode application target. An `rlib` crate type is
also emitted so host-side `cargo check`/`cargo test` keep working on
desktop.

## Building the static library

```sh
# Device
cargo build -p ios_demo --target aarch64-apple-ios --release
# → target/aarch64-apple-ios/release/libios_demo.a

# Simulator (Apple Silicon)
cargo build -p ios_demo --target aarch64-apple-ios-sim --release
# → target/aarch64-apple-ios-sim/release/libios_demo.a
```

The library exports one symbol:

```c
void martensite_ios_demo_main(void);
```

winit's UIKit backend calls `UIApplicationMain` internally inside
`EventLoop::run_app`, so the native launcher only has to call this symbol
on the main thread — **do not** call `UIApplicationMain` from Swift/ObjC.

## Packaging with cargo-mobile2

[cargo-mobile2](https://github.com/rust-mobile/cargo-mobile2) generates
the Xcode project around a Rust staticlib.

```sh
cargo install cargo-mobile2   # once
cargo mobile init             # generates gen/apple/ in a standalone app
```

In a generated project (or a consuming app's `Cargo.toml`):

1. Keep `crate-type = ["staticlib"]` for the iOS target, as this crate
   does.
2. Point the app's library dependency at the crate (path or workspace
   dependency).
3. In `gen/apple/<app>.xcodeproj` — or `project.yml` when using
   [XcodeGen], which cargo-mobile2 templates use — the app target links
   `libios_demo.a` from the Cargo `target/` dir. A build phase runs
   `cargo build --target aarch64-apple-ios(-sim)` before Xcode links.
4. In the generated `main.swift`, call the exported entry point:

   ```swift
   martensite_ios_demo_main()
   ```

   instead of the default `@main`/`UIApplicationMain` app delegate.

5. `Info.plist` needs no Martensite-specific keys; standard keys
   (`UILaunchScreen`, orientation, bundle id/signing) suffice. Add
   `NSBluetooth…`/camera/etc. usage descriptions only if the app uses
   those APIs — Martensite itself requires none for this demo.

## Packaging with plain Xcode (no cargo-mobile2)

1. `File → New → Project → iOS App`, SwiftUI or UIKit lifecycle either
   works — the app delegate is only a launcher.
2. Add a **Run Script** build phase (before "Compile Sources"):

   ```sh
   cd "$SRCROOT/path/to/martensite"
   cargo build -p ios_demo --target aarch64-apple-ios-sim --release  # or -ios for device
   ```

3. Add `libios_demo.a` to **Link Binary With Libraries**, plus the system
   frameworks winit/wgpu need: `UIKit`, `QuartzCore`/`Metal`,
   `MetalKit`, `Foundation`, `CoreGraphics`, `objc`.
4. Add a bridging header exposing `martensite_ios_demo_main`, call it
   from `main.swift` (create one with `martensite_ios_demo_main()` and
   remove `@main` from the App struct / AppDelegate).
5. Set the **Library Search Paths** to
   `$(SRCROOT)/path/to/martensite/target/$(RUST_TARGET)/release`, where
   `RUST_TARGET` is `aarch64-apple-ios` for device builds and
   `aarch64-apple-ios-sim` for simulator builds (key it off
   `ARCHS`/`SDKROOT` in a `xcconfig` if both are needed).
6. Configure signing (team, bundle id) as usual and run.

## Runtime notes

- **Safe area:** after `can_create_surfaces` creates the window,
  `Window::safe_area()` returns UIKit `safeAreaInsets` in *physical*
  pixels — top inset covers the Dynamic Island/status region, bottom the
  home-indicator area. Divide by `window.scale_factor()` for logical
  points. `martensite_window::WindowEntry::safe_area()` is the
  framework-level accessor.
- **Lifecycle:** iOS uses `SurfaceLifecycle::Persistent` — the
  `CAMetalLayer`-backed surface survives `suspended`/`resumed`, so the
  demo keeps its window; `destroy_surfaces` is never emitted by
  `winit-uikit`. Pause frame production on `suspended` anyway: the system
  may kill backgrounded apps that keep rendering.
- **Touch:** `UITouch` events arrive as `PointerMoved`/`PointerButton`
  with `PointerSource::Touch`; `convert_window_event` maps them to
  `PointerKind::Touch` with a distinct 64-bit `PointerId` per finger.
- **IME:** call `martensite_window::ime::enable_ime(&*window, caps, data)`
  to summon the software keyboard (`becomeFirstResponder`); commits and
  preedit arrive as `WindowEvent::Ime`. `cursor_area` is unsupported on
  iOS — the system positions the keyboard itself.
- **Accessibility:** when the full `martensite` crate is wired in, the
  AccessKit path subclasses the winit `UIView` via
  `martensite-access-platform`. Phase-1: VoiceOver sees roles, names,
  and actions; editable text is incomplete upstream in
  `accesskit_ios` 0.2.0.
