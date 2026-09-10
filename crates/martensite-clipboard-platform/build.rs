//! Build script for `martensite-clipboard-platform`.
//!
//! Links the Objective-C runtime (`libobjc`) and the AppKit framework on
//! macOS, and `libX11` on Linux. On Windows (or any other platform) this
//! build script is a no-op.

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("darwin") || target.contains("apple") {
        println!("cargo:rustc-link-lib=dylib=objc");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=Foundation");
    } else if target.contains("linux") {
        println!("cargo:rustc-link-lib=dylib=X11");
    }
}
