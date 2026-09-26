# Android Packaging Guide

**Status:** Implemented (v0.17.0, workstream 4)
**Applies to:** `aarch64-linux-android`, `x86_64-linux-android`,
`armv7-linux-androideabi`, `i686-linux-android`

Martensite apps ship on Android as a `cdylib` loaded by **GameActivity**.
This document covers the crate manifest, the NDK toolchain environment,
and the two supported packagers: `cargo-apk2` and `xbuild`.

## 1. Library crate type

The application crate — the crate that owns `android_main` — must declare
a `cdylib` so GameActivity can `dlopen` it:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
martensite = "0.20.1"
martensite-window = "0.20.1"
```

Export the entry point and hand the `AndroidApp` to winit via
`martensite_window::android`:

```rust
use martensite_window::android;

#[no_mangle]
fn android_main(app: android::AndroidApp) {
    let event_loop = android::new_event_loop(app).expect("event loop");
    // event_loop.run_app(&mut MyApp) ...
}
```

`android_main` is invoked by GameActivity on the main thread; the
`AndroidApp` handle is not global state — it must be threaded into the
event loop at build time (`new_event_loop` does this).

## 2. NDK toolchain environment

Compilation uses the NDK's LLVM toolchain. Point Cargo's linker and the
`cc`-family environment variables at the API-suffixed NDK tools (the NDK
no longer ships unsuffixed `aarch64-linux-android-clang`, so `cc-rs`
crates such as `android-activity` need `CC`/`CXX`/`AR` explicitly):

```sh
export NDK="$HOME/Library/Android/sdk/ndk/28.2.13676358"  # macOS example
export NDK_BIN="$NDK/toolchains/llvm/prebuilt/darwin-x86_64/bin"

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_BIN/aarch64-linux-android24-clang"
export CC_aarch64_linux_android="$NDK_BIN/aarch64-linux-android24-clang"
export CXX_aarch64_linux_android="$NDK_BIN/aarch64-linux-android24-clang++"
export AR_aarch64_linux_android="$NDK_BIN/llvm-ar"
export PATH="$NDK_BIN:$PATH"
```

Then check or build:

```sh
rustup target add aarch64-linux-android
cargo check --target aarch64-linux-android -p martensite-window \
    -p martensite-access -p martensite-wgpu
cargo build --target aarch64-linux-android --release -p my_app
```

API level 24 is a good floor (Vulkan requires API 24+; GLES fallback
works at API 21+). NDK 27 and 28 are both known to work; `darwin-x86_64`
in the path above becomes `linux-x86_64` or `windows-x86_64` on other
hosts.

## 3. GameActivity (required, not NativeActivity)

Martensite selects winit's `android-game-activity` feature for
`cfg(target_os = "android")` (see `martensite-window`). GameActivity is
**required**:

- `NativeActivity` delivers IME events unreliably; GameActivity's
  `GameTextInput` bridge is the working soft-keyboard path
  (`martensite_window::android::show_soft_input` /
  `hide_soft_input`).
- `accesskit_android`'s `InjectingAdapter` installs its accessibility
  delegate on `GameActivity.mSurfaceView`
  (`InputEnabledSurfaceView`); the field does not exist on
  `NativeActivity` and the injection panics. This is the same reason
  other Rust UI frameworks disable accessibility on `NativeActivity`.
- The `embedded-dex` feature of `accesskit_android` (enabled by
  `martensite-access-platform`) bundles the compiled
  `dev.accesskit.android.Delegate` class inside the crate — no Java
  sources are added to the APK.

The application manifest must declare
`android.app.lib_name` matching the cdylib name and use GameActivity:

```xml
<activity
    android:name="com.google.androidgamesdk.GameActivity"
    android:configChanges="orientation|screenSize|screenLayout|smallestScreenSize|uiMode|density|keyboard|keyboardHidden|navigation|touchscreen"
    android:exported="true">
    <meta-data android:name="android.app.lib_name" android:value="my_app" />
    <intent-filter>
        <action android:name="android.intent.action.MAIN" />
        <category android:name="android.intent.category.LAUNCHER" />
    </intent-filter>
</activity>
```

The `configChanges` set is the one GameActivity documents for games:
it keeps the activity — and, within the surface lifecycle contract of
§6, its `ANativeWindow` — alive across rotations, fold/unfold,
dark-mode flips, density changes, and keyboard attach/detach, each of
which otherwise recreates the activity.

## 4. Packaging with `cargo-apk2`

[cargo-apk2](https://crates.io/crates/cargo-apk2) builds the cdylib and
assembles a signed APK:

```sh
cargo install cargo-apk2

cargo apk build --target aarch64-linux-android        # debug APK
cargo apk build --target aarch64-linux-android --release
cargo apk run   --target aarch64-linux-android        # install + launch
```

The crate manifest carries the APK metadata:

```toml
[package.metadata.android]
package = "dev.martensite.my_app"
label = "My App"
version_name = "0.1.0"
min_sdk_version = 24
target_sdk_version = 35

[[package.metadata.android.uses_feature]]
name = "android.hardware.vulkan.version"
required = false  # GLES fallback — do not require Vulkan
```

Mark the Vulkan feature `required = false`: Martensite's wgpu instance
requests `VULKAN | GL` and picks Vulkan first, falling back to the GLES
downlevel backend on devices without Vulkan.

## 5. Packaging with `xbuild`

[xbuild](https://github.com/rust-mobile/xbuild) is the newer
cross-packaging tool:

```sh
cargo install xbuild

x build --platform android --arch arm64          # debug APK
x build --platform android --arch arm64 --release
x run  --device adb:DEVICE_ID                    # deploy over adb
```

`xbuild` reads a `manifest.yaml` next to `Cargo.toml` (or generates one
via `x new`). Its Android manifest has the same GameActivity
requirements as §3.

## 6. Surface lifecycle contract

Android destroys the `ANativeWindow` backing the surface while the
process keeps running (backgrounding, fold/unfold, some rotations). The
application's `ApplicationHandler` must:

- **`can_create_surfaces`** — create the `winit` window(s), create the
  `wgpu::Surface` via `GpuContext::create_surface`, and construct the
  AccessKit adapter (`martensite_access::android::create_adapter`).
  The `ANativeWindow` does not exist before this callback.
- **`destroy_surfaces`** — drop every `wgpu::Surface` *and* every
  `winit::window::Window`.
  `WindowManager::destroy_all_windows` is the manager-level teardown;
  GPU per-surface resources (configured surfaces, pipelines holding
  surface references) must be released first.
- **`resumed` / `suspended`** — bracket the activity's visible
  lifetime; start/stop frame production here.

The `accesskit_winit::Adapter` survives surface destruction and can be
kept; only the window/surface objects are tied to the `ANativeWindow`.

## 7. Known limitations

- `Window::safe_area()` returns zero insets on Android. Reading
  `WindowInsets` requires platform code until winit exposes it
  upstream (winit's own Android `safe_area` returns `(0, 0, 0, 0)`).
- `cargo-apk2` and `xbuild` are host tools — install them separately;
  they are not workspace dependencies.
- **Device/emulator gate (tracking):** the v0.17.0 §5 acceptance
  artifact is
  `crates/martensite-access-platform/tests/android_adapter_gate.rs` —
  `#[ignore]`-gated and env-gated on `MARTENSITE_ANDROID_DEVICE=1`,
  exercising the real `GameActivity.mSurfaceView` → `InjectingAdapter`
  injection path. It only does meaningful work when run on-device
  (`cargo apk test` / `x test` under a GameActivity process); host runs
  print a skip line, matching the 4K120 gate pattern. `cargo apk test`
  requires `[package.metadata.android]` manifest metadata on the tested
  crate and runs libtest inside the activity on unattached JVM worker
  threads — the gate attaches via `attach_current_thread` and prefers
  `-- --test-threads=1`. When `ndk-context` exposes no VM/activity
  (not a GameActivity process) the gate skips rather than fails. An
  APK-level end-to-end run (window + surface + IME) remains a manual
  step pending a packaged example app.
