//! Icon-pack data types + the fetched catalog.
//!
//! `fetch_icons` (`cargo run -p morph_viewer --bin fetch_icons`)
//! downloads the stroke sets morphicons itself targets — lucide (ISC),
//! tabler (MIT), feather (MIT), heroicons (MIT), iconoir (MIT) — from
//! their upstream repositories, extracts every glyph as a single
//! multi-subpath SVG `d` string, and writes `src/icons_gen.rs`. The
//! build script falls back to an empty `PACKS` when that file is
//! absent, so the viewer compiles with or without a fetch.

/// One icon: an upstream file stem plus its combined `d` string.
///
/// All `d`s are multi-subpath stroke geometry on a 24×24 grid — the
/// exact contract [`martensite::widgets::MorphIcon`] consumes.
pub struct IconDef {
    /// Icon name (upstream file stem, e.g. `"arrow-left"`).
    pub name: &'static str,
    /// Combined SVG path data — one `d` carrying every subpath.
    pub d: &'static str,
}

/// A fetched icon pack.
pub struct PackDef {
    /// Display name (`"lucide"`, …).
    pub name: &'static str,
    /// Upstream repository URL.
    pub source: &'static str,
    /// SPDX id of the pack's license — the `d` data stays under it.
    pub license: &'static str,
    /// Icons sorted by name.
    pub icons: &'static [IconDef],
}

include!(concat!(env!("OUT_DIR"), "/icons_gen.rs"));
