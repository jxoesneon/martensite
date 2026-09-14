# martensite-access-platform

Platform-specific accessibility FFI glue for
[martensite](../martensite).

This crate is the audited unsafe boundary between Martensite's AccessKit
integration and the mobile operating systems' native accessibility APIs:

- **iOS**: `ios::IosAdapter` wraps `accesskit_ios`'s `SubclassingAdapter`,
  which dynamically subclasses the winit-provided `UIView` to implement
  `UIAccessibilityContainer` and `UIAccessibilityHitTest`.
- **Android**: `accesskit_android`'s `InjectingAdapter` + `embedded-dex`
  JNI glue (stub — populated by the parallel Android workstream).

## Upstream maturity (iOS)

`accesskit_ios` 0.2.x is a Phase-1 implementation: basic traits and
properties (roles, labels, bounds, focus, tap/focus actions) are exported
to VoiceOver, but editable-text support is incomplete upstream. VoiceOver
navigation and activation work; full text-field editing is degraded.

## Safety

This crate uses `#![allow(unsafe_code)]` because it contains
platform-specific FFI to the Objective-C runtime (iOS) and the JNI
boundary (Android). It is one of the workspace's explicitly whitelisted
unsafe crates; the workspace-level `unsafe_code = "deny"` policy is
preserved for all other crates, including `martensite-access` itself.
