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
#[derive(Clone, Copy)]
pub struct IconDef {
    /// Icon name (upstream file stem, e.g. `"arrow-left"`).
    pub name: &'static str,
    /// Combined SVG path data — one `d` carrying every subpath.
    pub d: &'static str,
}

/// A fetched icon pack.
#[derive(Clone, Copy)]
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

use std::sync::LazyLock;

include!(concat!(env!("OUT_DIR"), "/icons_gen.rs"));

static NATIVE_ICONS: LazyLock<Vec<IconDef>> = LazyLock::new(|| {
    let mut icons: Vec<IconDef> = martensite::icons::builtin()
        .entries()
        .iter()
        .map(|e| IconDef {
            name: e.name.as_ref(),
            d: e.d.as_ref(),
        })
        .collect();
    icons.sort_by(|a, b| a.name.cmp(b.name));
    icons
});

static ALL_PACKS: LazyLock<Vec<PackDef>> = LazyLock::new(|| {
    let mut packs = PACKS.to_vec();
    packs.push(PackDef {
        name: "martensite",
        source: "native",
        license: "MIT",
        icons: NATIVE_ICONS.as_slice(),
    });
    packs
});

/// Every browsable pack: the fetched external sets plus Martensite's
/// native [`icons`](martensite::icons) pack (appended last so external
/// indices — and the lucide-first default — are unchanged).
pub fn all_packs() -> &'static [PackDef] {
    &ALL_PACKS
}
