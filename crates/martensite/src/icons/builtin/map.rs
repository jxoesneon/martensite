//! `map` namespace — location, navigation, landmarks, buildings.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`map.*`).
///
/// Every name constant in this module is prefixed `MAP_`
/// (`MAP_FOO` → `"map.foo"`) so the flattened
/// `builtin::names` re-export cannot collide.
pub mod names {
    /// `"map.map"` — folded map.
    pub const MAP: &str = "map.map";
    /// `"map.map-pin"` — location pin.
    pub const MAP_PIN: &str = "map.map-pin";
    /// `"map.compass"` — compass.
    pub const COMPASS: &str = "map.compass";
    /// `"map.navigation"` — navigation pointer.
    pub const NAVIGATION: &str = "map.navigation";
    /// `"map.route"` — route between two points.
    pub const ROUTE: &str = "map.route";
    /// `"map.locate"` — crosshair rings.
    pub const LOCATE: &str = "map.locate";
    /// `"map.locate-fixed"` — crosshair rings with center dot.
    pub const LOCATE_FIXED: &str = "map.locate-fixed";
    /// `"map.globe"` — meridian globe.
    pub const GLOBE: &str = "map.globe";
    /// `"map.landmark"` — columned landmark / institution.
    pub const LANDMARK: &str = "map.landmark";
    /// `"map.building"` — building with side wings.
    pub const BUILDING: &str = "map.building";
    /// `"map.factory"` — sawtooth-roof factory.
    pub const FACTORY: &str = "map.factory";
    /// `"map.store"` — storefront with awning.
    pub const STORE: &str = "map.store";
}

/// `"map.map"` — three-panel folded map.
pub const MAP_MAP: &str = "M9 4L3 6.5V19.5L9 17l6 3 6-2.5V5L15 7 9 4zM9 4v13M15 7v13";
/// `"map.map-pin"` — teardrop pin with center dot.
pub const MAP_MAP_PIN: &str =
    "M20 10C20 16 12 22 12 22C12 22 4 16 4 10A8 8 0 1120 10zM15 10a3 3 0 11-6 0 3 3 0 016 0z";
/// `"map.compass"` — ring plus angled needle.
pub const MAP_COMPASS: &str =
    "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM16.24 7.76L14.12 14.12L7.76 16.24L9.88 9.88Z";
/// `"map.navigation"` — kite pointer.
pub const MAP_NAVIGATION: &str = "M3 11L22 2L13 21L11 13Z";
/// `"map.route"` — two dots joined by an S-bend.
pub const MAP_ROUTE: &str =
    "M9 6a3 3 0 11-6 0 3 3 0 016 0zM21 18a3 3 0 11-6 0 3 3 0 016 0zM6 9v3a2 2 0 002 2h8a2 2 0 012 2v2";
/// `"map.locate"` — ring plus four ticks.
pub const MAP_LOCATE: &str = "M19 12a7 7 0 11-14 0 7 7 0 0114 0zM2 12h3M19 12h3M12 2v3M12 19v3";
/// `"map.locate-fixed"` — locate plus center dot.
pub const MAP_LOCATE_FIXED: &str =
    "M19 12a7 7 0 11-14 0 7 7 0 0114 0zM2 12h3M19 12h3M12 2v3M12 19v3M13 12a1 1 0 11-2 0 1 1 0 012 0z";
/// `"map.globe"` — circle, equator, meridian ellipse.
pub const MAP_GLOBE: &str =
    "M22 12a10 10 0 11-20 0 10 10 0 0120 0zM2 12h20M12 2a15.3 15.3 0 014 10 15.3 15.3 0 01-4 10 15.3 15.3 0 01-4-10 15.3 15.3 0 014-10z";
/// `"map.landmark"` — pediment over four columns on a base.
pub const MAP_LANDMARK: &str = "M3 22h18M12 2L4 7h16zM6 18V9M10 18V9M14 18V9M18 18V9";
/// `"map.building"` — tower with side wings and window dashes.
pub const MAP_BUILDING: &str =
    "M6 22V4a2 2 0 012-2h8a2 2 0 012 2v18M6 12H4a2 2 0 00-2 2v6a2 2 0 002 2h2M18 9h2a2 2 0 012 2v9a2 2 0 01-2 2h-2M10 6h4M10 10h4M10 14h4M10 18h4";
/// `"map.factory"` — chimney block with sawtooth roof.
pub const MAP_FACTORY: &str =
    "M2 20a2 2 0 002 2h16a2 2 0 002-2V9l-7 5V9l-7 5V5a3 3 0 00-3-3H2zM7 18h1M12 18h1M17 18h1";
/// `"map.store"` — canopy, scalloped awning, shopfront, door.
pub const MAP_STORE: &str =
    "M2 7l4.4-4.4A2 2 0 017.8 2h8.4a2 2 0 011.4.6L22 7M2 7h20M4 7a2 2 0 014 0 2 2 0 004 0 2 2 0 004 0 2 2 0 004 0M4 12v8a2 2 0 002 2h12a2 2 0 002-2v-8M15 22v-4a2 2 0 00-2-2h-2a2 2 0 00-2 2v4";

/// `map` entries — registered in the pack in this order.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::MAP, MAP_MAP),
    IconEntry::new(names::MAP_PIN, MAP_MAP_PIN),
    IconEntry::new(names::COMPASS, MAP_COMPASS),
    IconEntry::new(names::NAVIGATION, MAP_NAVIGATION),
    IconEntry::new(names::ROUTE, MAP_ROUTE),
    IconEntry::new(names::LOCATE, MAP_LOCATE),
    IconEntry::new(names::LOCATE_FIXED, MAP_LOCATE_FIXED),
    IconEntry::new(names::GLOBE, MAP_GLOBE),
    IconEntry::new(names::LANDMARK, MAP_LANDMARK),
    IconEntry::new(names::BUILDING, MAP_BUILDING),
    IconEntry::new(names::FACTORY, MAP_FACTORY),
    IconEntry::new(names::STORE, MAP_STORE),
];

/// `map` morph pairs.
pub const PAIRS: &[IconPair] = &[IconPair::new(names::LOCATE, names::LOCATE_FIXED)];
