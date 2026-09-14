# martensite-access-platform

Platform-specific accessibility FFI glue for
[`martensite-access`](../martensite-access).

This crate is the workspace's audited `unsafe` exception for mobile
accessibility boundaries, mirroring the `martensite-media-platform`
convention:

- **Android** (`android` module) — JNI glue that resolves the
  GameActivity `InputEnabledSurfaceView` and constructs
  `accesskit_android`'s `InjectingAdapter` (with the `embedded-dex`
  feature, so the Java delegate class ships inside the crate).
  **GameActivity is required**; `NativeActivity` is unsupported because
  IME events are unreliable there and the AccessKit delegate cannot be
  injected.
- **iOS** (`ios` module) — reserved for the `accesskit_ios`
  `SubclassingAdapter` Objective-C boundary (separate workstream).

All other Martensite crates keep `unsafe_code = "deny"`; the unsafe code
lives here so it can be audited in isolation.
