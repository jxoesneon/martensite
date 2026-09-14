//! Platform-specific accessibility FFI glue for `martensite-access`.
//!
//! This crate is the audited unsafe boundary between Martensite's AccessKit
//! integration and the mobile operating systems' native accessibility APIs:
//!
//! - **iOS**: [`ios::IosAdapter`] wraps `accesskit_ios`'s
//!   `SubclassingAdapter`, which dynamically subclasses the winit-provided
//!   `UIView` to implement `UIAccessibilityContainer`,
//!   `UIAccessibilityHitTest`, and view-visibility notifications.
//! - **Android**: [`android::AndroidAdapter`] resolves the GameActivity
//!   `InputEnabledSurfaceView` and wraps `accesskit_android`'s
//!   `InjectingAdapter` (with the `embedded-dex` feature so the Java
//!   delegate class ships inside this crate).
//!
//! # Safety policy
//!
//! This crate uses `#![allow(unsafe_code)]` at the crate level because it
//! contains platform-specific FFI to the Objective-C runtime (iOS) and the
//! JNI boundary (Android). It is one of the workspace's explicitly
//! whitelisted unsafe crates — the workspace-level `unsafe_code = "deny"`
//! policy is preserved for all other crates, including `martensite-access`
//! itself, following the same pattern as `martensite-media-platform` and
//! `martensite-clipboard-platform`.
//!
//! All `unsafe` blocks are confined to the platform modules (`ios`,
//! `android`) and are audited against the upstream `accesskit_ios` /
//! `accesskit_android` API contracts.

#![allow(unsafe_code)]
#![forbid(missing_docs)]
// Platform modules are cfg-gated per OS, so intra-doc links to them may not
// resolve on all platforms. This matches the pattern used by
// `martensite-media-platform` and `martensite-font-fallback`.
#![allow(rustdoc::broken_intra_doc_links)]

#[cfg(target_os = "android")]
pub mod android;

#[cfg(target_os = "ios")]
pub mod ios;
