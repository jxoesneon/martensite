//! Build script for `martensite-clipboard-platform`.
//!
//! On macOS, links the Objective-C runtime (`libobjc`) and the AppKit
//! framework so that `NSPasteboard` is available. On other platforms this
//! build script is a no-op.

fn main() {
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-lib=dylib=objc");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=Foundation");
    }
    #[cfg(target_os = "linux")]
    {
        println!("cargo:rustc-link-lib=dylib=X11");
    }
}
