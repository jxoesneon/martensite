//! The native icon pack data — hand-authored stroke geometry on the
//! 24px grid in the lucide/feather idiom (`M`/`L`/`C`/`Z`/`H`/`V`/`A`
//! commands only, stroke rendering, 2px-at-24px stroke convention).
//!
//! Names are qualified `namespace.kebab-name` — see [`names`].
//! Each namespace lives in its own module; [`BUILTIN`](super::BUILTIN)
//! concatenates every module's `ENTRIES`/`PAIRS` in the order the
//! modules are declared here.
//!
//! | namespace  | covers |
//! |------------|--------|
//! | `nav`      | app/page navigation, disclosure, directional moves |
//! | `arrow`    | directional movement, expand/compress, rotate |
//! | `layout`   | grids, columns, panels, splits, frames |
//! | `media`    | transport and audio/video state |
//! | `device`   | monitors, phones, peripherals, batteries, radios |
//! | `status`   | state feedback, toggles, affordances |
//! | `security` | locks, keys, shields, credentials |
//! | `system`   | power, compute, databases, infrastructure |
//! | `data`     | density, charts, and data shaping |
//! | `file`     | files, folders, documents, clipboard, archives |
//! | `edit`     | document/editing verbs and tools |
//! | `text`     | typography, formatting, alignment, lists, links |
//! | `comms`    | mail, messaging, phone, broadcast |
//! | `people`   | users, groups, identity, accessibility |
//! | `time`     | clocks, calendars, alarms, timers |
//! | `weather`  | sky conditions, precipitation, temperature |
//! | `map`      | location, navigation, landmarks, buildings |
//! | `tool`     | instruments, sliders, repair, measurement |
//! | `dev`      | code, git, terminals, debugging |
//! | `misc`     | theme, identity, and system affordances |
//!
//! The `NAMESPACE_NAME` path constants double as direct `d` sources
//! for code that does not want name resolution — the same role
//! `morph_icon::demo` plays for the widget's own fixtures. Authoring
//! rules live in `../DESIGN.md` and are enforced by `icons::tests`.

use super::IconPack;
use std::sync::LazyLock;

pub mod arrow;
pub mod comms;
pub mod data;
pub mod dev;
pub mod device;
pub mod edit;
pub mod file;
pub mod layout;
pub mod map;
pub mod media;
pub mod misc;
pub mod nav;
pub mod people;
pub mod security;
pub mod status;
pub mod system;
pub mod text;
pub mod time;
pub mod tool;
pub mod weather;

/// Qualified icon names — `namespace.kebab-name` — as constants so
/// app code and pack entries share one spelling. Flattened across all
/// namespace modules; the per-namespace modules
/// (`builtin::nav::names`, …) remain available too.
///
/// # Examples
///
/// ```
/// use martensite::icons::builtin::names;
///
/// assert_eq!(names::LOCK, "status.lock");
/// assert_eq!(names::PLAY, "media.play");
/// ```
pub mod names {
    pub use super::arrow::names::*;
    pub use super::comms::names::*;
    pub use super::data::names::*;
    pub use super::dev::names::*;
    pub use super::device::names::*;
    pub use super::edit::names::*;
    pub use super::file::names::*;
    pub use super::layout::names::*;
    pub use super::map::names::*;
    pub use super::media::names::*;
    pub use super::misc::names::*;
    pub use super::nav::names::*;
    pub use super::people::names::*;
    pub use super::security::names::*;
    pub use super::status::names::*;
    pub use super::system::names::*;
    pub use super::text::names::*;
    pub use super::time::names::*;
    pub use super::tool::names::*;
    pub use super::weather::names::*;
}

/// The builtin pack's ordered entry and pair tables — backs
/// [`BUILTIN`](super::BUILTIN). Namespace modules contribute their
/// `ENTRIES`/`PAIRS`; assembly happens once on first access.
///
/// Kept as `LazyLock` rather than a `const`: `&[T]` slices cannot be
/// concatenated in const context, and the pack is assembled from
/// twenty namespace tables.
static BUILTIN_PACK: LazyLock<IconPack> = LazyLock::new(|| {
    let mut entries = Vec::new();
    let mut pairs = Vec::new();
    for (e, p) in NS_TABLES {
        entries.extend_from_slice(e);
        pairs.extend_from_slice(p);
    }
    IconPack::from_entries("martensite", entries).with_pairs(pairs)
});

const NS_TABLES: &[(&[super::IconEntry], &[super::IconPair])] = &[
    (nav::ENTRIES, nav::PAIRS),
    (arrow::ENTRIES, arrow::PAIRS),
    (layout::ENTRIES, layout::PAIRS),
    (media::ENTRIES, media::PAIRS),
    (device::ENTRIES, device::PAIRS),
    (status::ENTRIES, status::PAIRS),
    (security::ENTRIES, security::PAIRS),
    (system::ENTRIES, system::PAIRS),
    (data::ENTRIES, data::PAIRS),
    (file::ENTRIES, file::PAIRS),
    (edit::ENTRIES, edit::PAIRS),
    (text::ENTRIES, text::PAIRS),
    (comms::ENTRIES, comms::PAIRS),
    (people::ENTRIES, people::PAIRS),
    (time::ENTRIES, time::PAIRS),
    (weather::ENTRIES, weather::PAIRS),
    (map::ENTRIES, map::PAIRS),
    (tool::ENTRIES, tool::PAIRS),
    (dev::ENTRIES, dev::PAIRS),
    (misc::ENTRIES, misc::PAIRS),
];

/// The native icon pack — built once on first access.
///
/// # Examples
///
/// ```
/// use martensite::icons::builtin;
///
/// assert_eq!(builtin().name(), "martensite");
/// assert!(builtin().contains("nav.search"));
/// ```
pub fn builtin() -> &'static IconPack {
    &BUILTIN_PACK
}

// ---------------------------------------------------------------------------
// Selected affordance — the baseline-pip convention
// ---------------------------------------------------------------------------

/// The selected-affordance subpath: a short baseline under the glyph
/// (`M8 21h8` on the 24px grid). Appended to any icon's `d` it marks
/// the "selected destination" state — see [`selected_pip`].
pub const SELECTED_PIP: &str = "M8 21h8";

/// The selected-state variant of any icon `d`: `d` plus the
/// [`SELECTED_PIP`] baseline subpath.
///
/// Rails, tabs, and toolbars mark the active destination by morphing
/// between a base icon and this variant instead of shipping a
/// `*-selected` entry per icon in the pack. The pip is one extra
/// subpath — the [`MorphIcon`](crate::widgets::MorphIcon) engine
/// dissolves it in/out against the base shape, so `selected_pip`
/// output is valid morph input for any `d`.
///
/// # Examples
///
/// ```
/// use martensite::icons::builtin;
///
/// let sel = builtin::selected_pip(builtin::data::DATA_GRID);
/// assert_eq!(sel, format!("{}{}", builtin::data::DATA_GRID, builtin::SELECTED_PIP));
/// ```
pub fn selected_pip(d: &str) -> String {
    let mut out = String::with_capacity(d.len() + SELECTED_PIP.len());
    out.push_str(d);
    out.push_str(SELECTED_PIP);
    out
}
