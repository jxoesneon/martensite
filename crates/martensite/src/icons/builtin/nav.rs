//! `nav` namespace — app/page navigation, disclosure, directional moves.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`nav.*`).
pub mod names {
    /// `"nav.menu"` — hamburger menu.
    pub const MENU: &str = "nav.menu";
    /// `"nav.home"` — house / root destination.
    pub const HOME: &str = "nav.home";
    /// `"nav.settings"` — gear / preferences.
    pub const SETTINGS: &str = "nav.settings";
    /// `"nav.search"` — magnifier.
    pub const SEARCH: &str = "nav.search";
    /// `"nav.chevron-up"` — disclosure/up chevron.
    pub const CHEVRON_UP: &str = "nav.chevron-up";
    /// `"nav.chevron-down"` — disclosure/down chevron.
    pub const CHEVRON_DOWN: &str = "nav.chevron-down";
    /// `"nav.chevron-left"` — disclosure/left chevron.
    pub const CHEVRON_LEFT: &str = "nav.chevron-left";
    /// `"nav.chevron-right"` — disclosure/right chevron.
    pub const CHEVRON_RIGHT: &str = "nav.chevron-right";
    /// `"nav.arrow-left"` — back/previous move.
    pub const NAV_ARROW_LEFT: &str = "nav.arrow-left";
    /// `"nav.arrow-right"` — forward/next move.
    pub const NAV_ARROW_RIGHT: &str = "nav.arrow-right";
    /// `"nav.chevrons-up"` — stacked up chevrons (fast scroll/collapse).
    pub const NAV_CHEVRONS_UP: &str = "nav.chevrons-up";
    /// `"nav.chevrons-down"` — stacked down chevrons (fast scroll/expand).
    pub const NAV_CHEVRONS_DOWN: &str = "nav.chevrons-down";
    /// `"nav.chevrons-left"` — stacked left chevrons (rewind/back).
    pub const NAV_CHEVRONS_LEFT: &str = "nav.chevrons-left";
    /// `"nav.chevrons-right"` — stacked right chevrons (fast-forward/next).
    pub const NAV_CHEVRONS_RIGHT: &str = "nav.chevrons-right";
    /// `"nav.chevron-up-down"` — opposing chevrons (select/sort affordance).
    pub const NAV_CHEVRON_UP_DOWN: &str = "nav.chevron-up-down";
    /// `"nav.chevron-left-right"` — opposing chevrons (horizontal affordance).
    pub const NAV_CHEVRON_LEFT_RIGHT: &str = "nav.chevron-left-right";
    /// `"nav.more-horizontal"` — three-dot overflow row.
    pub const NAV_MORE_HORIZONTAL: &str = "nav.more-horizontal";
    /// `"nav.more-vertical"` — three-dot overflow column.
    pub const NAV_MORE_VERTICAL: &str = "nav.more-vertical";
    /// `"nav.grip"` — 3×3 dot drag handle.
    pub const NAV_GRIP: &str = "nav.grip";
    /// `"nav.grip-vertical"` — 2×3 dot vertical drag handle.
    pub const NAV_GRIP_VERTICAL: &str = "nav.grip-vertical";
    /// `"nav.grip-horizontal"` — 3×2 dot horizontal drag handle.
    pub const NAV_GRIP_HORIZONTAL: &str = "nav.grip-horizontal";
}

/// `"nav.menu"` — three-stroke hamburger.
pub const NAV_MENU: &str = "M4 7h16M4 12h16M4 17h16";
/// `"nav.home"` — roofline over a body with a door.
pub const NAV_HOME: &str = "M4 11l8-7 8 7M6 9.5V20h12V9.5M10 20v-5h4v5";
/// `"nav.settings"` — hub circle with eight teeth.
pub const NAV_SETTINGS: &str = "M16 12a4 4 0 11-8 0 4 4 0 018 0zM12 2v2.5M12 19.5V22M2 12h2.5M19.5 12H22M4.9 4.9l1.8 1.8M17.3 17.3l1.8 1.8M19.1 4.9l-1.8 1.8M6.7 17.3l-1.8 1.8";
/// `"nav.search"` — lens circle plus handle.
pub const NAV_SEARCH: &str = "M18 11a7 7 0 11-14 0 7 7 0 0114 0zM21 21l-4.9-4.9";
/// `"nav.chevron-up"`.
pub const NAV_CHEVRON_UP: &str = "M6 15l6-6 6 6";
/// `"nav.chevron-down"`.
pub const NAV_CHEVRON_DOWN: &str = "M6 9l6 6 6-6";
/// `"nav.chevron-left"`.
pub const NAV_CHEVRON_LEFT: &str = "M15 6l-6 6 6 6";
/// `"nav.chevron-right"`.
pub const NAV_CHEVRON_RIGHT: &str = "M9 6l6 6-6 6";
/// `"nav.arrow-left"` — shaft plus arrowhead.
pub const NAV_ARROW_LEFT: &str = "M19 12H5M12 19l-7-7 7-7";
/// `"nav.arrow-right"` — shaft plus arrowhead.
pub const NAV_ARROW_RIGHT: &str = "M5 12h14M12 5l7 7-7 7";
/// `"nav.chevrons-up"` — two stacked `chevron-up` shapes.
pub const NAV_CHEVRONS_UP: &str = "M17 11l-6-6-6 6M17 18l-6-6-6 6";
/// `"nav.chevrons-down"` — two stacked `chevron-down` shapes.
pub const NAV_CHEVRONS_DOWN: &str = "M7 6l6 6 6-6M7 13l6 6 6-6";
/// `"nav.chevrons-left"` — two stacked `chevron-left` shapes.
pub const NAV_CHEVRONS_LEFT: &str = "M11 17l-6-6 6-6M18 17l-6-6 6-6";
/// `"nav.chevrons-right"` — two stacked `chevron-right` shapes.
pub const NAV_CHEVRONS_RIGHT: &str = "M6 17l6-6-6-6M13 17l6-6-6-6";
/// `"nav.chevron-up-down"` — up chevron over down chevron.
pub const NAV_CHEVRON_UP_DOWN: &str = "M7 9l5-5 5 5M7 15l5 5 5-5";
/// `"nav.chevron-left-right"` — left chevron beside right chevron.
pub const NAV_CHEVRON_LEFT_RIGHT: &str = "M9 7l-5 5 5 5M15 7l5 5-5 5";
/// `"nav.more-horizontal"` — three dots in a row.
pub const NAV_MORE_HORIZONTAL: &str = "M5 12H5.25M12 12H12.25M19 12H19.25";
/// `"nav.more-vertical"` — three dots in a column.
pub const NAV_MORE_VERTICAL: &str = "M12 5V5.25M12 12V12.25M12 19V19.25";
/// `"nav.grip"` — 3×3 dot field drag handle.
pub const NAV_GRIP: &str = "M5 5H5.25M12 5H12.25M19 5H19.25M5 12H5.25M12 12H12.25M19 12H19.25M5 19H5.25M12 19H12.25M19 19H19.25";
/// `"nav.grip-vertical"` — two columns of three dots.
pub const NAV_GRIP_VERTICAL: &str =
    "M9 5H9.25M15 5H15.25M9 12H9.25M15 12H15.25M9 19H9.25M15 19H15.25";
/// `"nav.grip-horizontal"` — three columns of two dots.
pub const NAV_GRIP_HORIZONTAL: &str =
    "M5 9H5.25M12 9H12.25M19 9H19.25M5 15H5.25M12 15H12.25M19 15H19.25";

/// `nav` entries — registered in the pack in this order.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::MENU, NAV_MENU),
    IconEntry::new(names::HOME, NAV_HOME),
    IconEntry::new(names::SETTINGS, NAV_SETTINGS),
    IconEntry::new(names::SEARCH, NAV_SEARCH),
    IconEntry::new(names::CHEVRON_UP, NAV_CHEVRON_UP),
    IconEntry::new(names::CHEVRON_DOWN, NAV_CHEVRON_DOWN),
    IconEntry::new(names::CHEVRON_LEFT, NAV_CHEVRON_LEFT),
    IconEntry::new(names::CHEVRON_RIGHT, NAV_CHEVRON_RIGHT),
    IconEntry::new(names::NAV_ARROW_LEFT, NAV_ARROW_LEFT),
    IconEntry::new(names::NAV_ARROW_RIGHT, NAV_ARROW_RIGHT),
    IconEntry::new(names::NAV_CHEVRONS_UP, NAV_CHEVRONS_UP),
    IconEntry::new(names::NAV_CHEVRONS_DOWN, NAV_CHEVRONS_DOWN),
    IconEntry::new(names::NAV_CHEVRONS_LEFT, NAV_CHEVRONS_LEFT),
    IconEntry::new(names::NAV_CHEVRONS_RIGHT, NAV_CHEVRONS_RIGHT),
    IconEntry::new(names::NAV_CHEVRON_UP_DOWN, NAV_CHEVRON_UP_DOWN),
    IconEntry::new(names::NAV_CHEVRON_LEFT_RIGHT, NAV_CHEVRON_LEFT_RIGHT),
    IconEntry::new(names::NAV_MORE_HORIZONTAL, NAV_MORE_HORIZONTAL),
    IconEntry::new(names::NAV_MORE_VERTICAL, NAV_MORE_VERTICAL),
    IconEntry::new(names::NAV_GRIP, NAV_GRIP),
    IconEntry::new(names::NAV_GRIP_VERTICAL, NAV_GRIP_VERTICAL),
    IconEntry::new(names::NAV_GRIP_HORIZONTAL, NAV_GRIP_HORIZONTAL),
];

/// `nav` morph pairs.
pub const PAIRS: &[IconPair] = &[
    IconPair::new(names::MENU, super::status::names::CLOSE),
    IconPair::new(names::CHEVRON_RIGHT, names::CHEVRON_DOWN),
];
