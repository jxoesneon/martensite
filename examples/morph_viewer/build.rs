//! Folds the fetched icon data into the build.
//!
//! `cargo run -p morph_viewer --bin fetch_icons` downloads the free
//! stroke-icon packs and writes `src/icons_gen.rs`. When that file is
//! absent (fresh checkout, no network) the viewer builds against an
//! empty catalog and paints its own "run fetch_icons" empty state —
//! the example never fails to compile for lack of icon data.

use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let src = manifest.join("src/icons_gen.rs");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("icons_gen.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    if src.exists() {
        fs::copy(&src, &out).expect("copy icons_gen.rs");
    } else {
        fs::write(&out, "pub static PACKS: &[PackDef] = &[];\n").expect("write stub icons_gen.rs");
    }
}
