//! Platform-specific accessibility FFI glue for `martensite-access`.
//!
//! This crate is the workspace's audited `unsafe` exception for mobile
//! accessibility boundaries, mirroring the `martensite-media-platform`
//! convention: every other Martensite crate keeps `unsafe_code = "deny"`,
//! while the small amount of unavoidable FFI is isolated here.
//!
//! - [`android`] — JNI glue resolving the GameActivity
//!   `InputEnabledSurfaceView` and constructing `accesskit_android`'s
//!   `InjectingAdapter` (with the `embedded-dex` feature so the Java
//!   delegate class ships inside this crate).
//! - [`ios`] — reserved for the `accesskit_ios` `SubclassingAdapter`
//!   Objective-C boundary (implemented by a parallel workstream).
#![allow(unsafe_code)]

#[cfg(target_os = "android")]
pub mod android;
#[cfg(target_os = "ios")]
pub mod ios;
