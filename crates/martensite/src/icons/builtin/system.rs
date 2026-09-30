//! `system` namespace — power, compute, databases, infrastructure.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`system.*`).
///
/// Every name constant in this module is prefixed `SYSTEM_`
/// (`SYSTEM_FOO` → `"system.foo"`) so the flattened
/// `builtin::names` re-export cannot collide.
pub mod names {
    /// `"system.power"` — power on/off control.
    pub const SYSTEM_POWER: &str = "system.power";
    /// `"system.power-off"` — powered down / power disabled.
    pub const SYSTEM_POWER_OFF: &str = "system.power-off";
    /// `"system.server"` — rack server / host.
    pub const SYSTEM_SERVER: &str = "system.server";
    /// `"system.database"` — database cylinder.
    pub const SYSTEM_DATABASE: &str = "system.database";
    /// `"system.database-zap"` — energized / cached database.
    pub const SYSTEM_DATABASE_ZAP: &str = "system.database-zap";
    /// `"system.network"` — hub with two leaves.
    pub const SYSTEM_NETWORK: &str = "system.network";
    /// `"system.ethernet"` — wired port.
    pub const SYSTEM_ETHERNET: &str = "system.ethernet";
    /// `"system.blocks"` — modular blocks.
    pub const SYSTEM_BLOCKS: &str = "system.blocks";
    /// `"system.workflow"` — two nodes in a loop.
    pub const SYSTEM_WORKFLOW: &str = "system.workflow";
    /// `"system.component"` — four-way diamond composite.
    pub const SYSTEM_COMPONENT: &str = "system.component";
    /// `"system.container"` — ribbed container box.
    pub const SYSTEM_CONTAINER: &str = "system.container";
    /// `"system.circuit-board"` — PCB with trace and vias.
    pub const SYSTEM_CIRCUIT_BOARD: &str = "system.circuit-board";
}

/// `"system.power"` — stem plus broken ring.
pub const SYSTEM_POWER: &str = "M18.25 6.5A8.5 8.5 0 115.75 6.5M12 2V10";
/// `"system.power-off"` — stem, broken ring, strike slash.
pub const SYSTEM_POWER_OFF: &str = "M18.25 6.5A8.5 8.5 0 115.75 6.5M12 2V10M2 2L22 22";
/// `"system.server"` — two rack units plus indicator dots.
pub const SYSTEM_SERVER: &str = "M4 4H20A1 1 0 0121 5V9A1 1 0 0120 10H4A1 1 0 013 9V5A1 1 0 014 4ZM4 14H20A1 1 0 0121 15V19A1 1 0 0120 20H4A1 1 0 013 19V15A1 1 0 014 14ZM7 7h.01M7 17h.01";
/// `"system.database"` — cylinder lid, sides, mid seam.
pub const SYSTEM_DATABASE: &str = "M21 5C21 6.75 17 8 12 8C7 8 3 6.75 3 5C3 3.25 7 2 12 2C17 2 21 3.25 21 5ZM3 5V17C3 18.75 7 20 12 20C17 20 21 18.75 21 17V5M3 12C3 13.75 7 15 12 15C17 15 21 13.75 21 12";
/// `"system.database-zap"` — cylinder plus inner bolt.
pub const SYSTEM_DATABASE_ZAP: &str = "M21 5C21 6.75 17 8 12 8C7 8 3 6.75 3 5C3 3.25 7 2 12 2C17 2 21 3.25 21 5ZM3 5V17C3 18.75 7 20 12 20C17 20 21 18.75 21 17V5M3 12C3 13.75 7 15 12 15C17 15 21 13.75 21 12M14 9L10.5 15H14L11.5 20L18 12.5H14.5L16 9Z";
/// `"system.network"` — parent node bussed to two leaves.
pub const SYSTEM_NETWORK: &str = "M10 2H14A1 1 0 0115 3V7A1 1 0 0114 8H10A1 1 0 019 7V3A1 1 0 0110 2ZM3 16H7A1 1 0 018 17V21A1 1 0 017 22H3A1 1 0 012 21V17A1 1 0 013 16ZM17 16H21A1 1 0 0122 17V21A1 1 0 0121 22H17A1 1 0 0116 21V17A1 1 0 0117 16ZM12 8V12M5 16V14A2 2 0 017 12H17A2 2 0 0119 14V16";
/// `"system.ethernet"` — stepped jack plus contacts.
pub const SYSTEM_ETHERNET: &str =
    "M7 6H17V9H21V20A1 1 0 0120 21H4A1 1 0 013 20V9H7ZM8 12V16M12 12V16M16 12V16";
/// `"system.blocks"` — two blocks over one.
pub const SYSTEM_BLOCKS: &str = "M5 3H9A2 2 0 0111 5V9A2 2 0 019 11H5A2 2 0 013 9V5A2 2 0 015 3ZM15 3H19A2 2 0 0121 5V9A2 2 0 0119 11H15A2 2 0 0113 9V5A2 2 0 0115 3ZM10 13H14A2 2 0 0116 15V19A2 2 0 0114 21H10A2 2 0 018 19V15A2 2 0 0110 13Z";
/// `"system.workflow"` — two nodes, two elbow pipes.
pub const SYSTEM_WORKFLOW: &str = "M4 5H7A1 1 0 018 6V9A1 1 0 017 10H4A1 1 0 013 9V6A1 1 0 014 5ZM17 14H20A1 1 0 0121 15V18A1 1 0 0120 19H17A1 1 0 0116 18V15A1 1 0 0117 14ZM8 6.5H15.5A2 2 0 0117.5 8.5V14M16 16.5H9.5A2 2 0 017.5 14.5V10";
/// `"system.component"` — four diamonds around a center.
pub const SYSTEM_COMPONENT: &str = "M12 2L15.5 5.5L12 9L8.5 5.5ZM22 12L18.5 15.5L15 12L18.5 8.5ZM12 15L15.5 18.5L12 22L8.5 18.5ZM9 12L5.5 15.5L2 12L5.5 8.5Z";
/// `"system.container"` — ribbed box.
pub const SYSTEM_CONTAINER: &str =
    "M3 8H21V19A1 1 0 0120 20H4A1 1 0 013 19ZM7 8V20M11 8V20M15 8V20";
/// `"system.circuit-board"` — board, vias, U-trace.
pub const SYSTEM_CIRCUIT_BOARD: &str = "M5 4H19A1 1 0 0120 5V19A1 1 0 0119 20H5A1 1 0 014 19V5A1 1 0 015 4ZM8 8h.01M16 8h.01M8 16h.01M16 16h.01M8 10V14H16V10";

/// `system` entries.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::SYSTEM_POWER, SYSTEM_POWER),
    IconEntry::new(names::SYSTEM_POWER_OFF, SYSTEM_POWER_OFF),
    IconEntry::new(names::SYSTEM_SERVER, SYSTEM_SERVER),
    IconEntry::new(names::SYSTEM_DATABASE, SYSTEM_DATABASE),
    IconEntry::new(names::SYSTEM_DATABASE_ZAP, SYSTEM_DATABASE_ZAP),
    IconEntry::new(names::SYSTEM_NETWORK, SYSTEM_NETWORK),
    IconEntry::new(names::SYSTEM_ETHERNET, SYSTEM_ETHERNET),
    IconEntry::new(names::SYSTEM_BLOCKS, SYSTEM_BLOCKS),
    IconEntry::new(names::SYSTEM_WORKFLOW, SYSTEM_WORKFLOW),
    IconEntry::new(names::SYSTEM_COMPONENT, SYSTEM_COMPONENT),
    IconEntry::new(names::SYSTEM_CONTAINER, SYSTEM_CONTAINER),
    IconEntry::new(names::SYSTEM_CIRCUIT_BOARD, SYSTEM_CIRCUIT_BOARD),
];

/// `system` morph pairs.
pub const PAIRS: &[IconPair] = &[
    IconPair::new(names::SYSTEM_POWER, names::SYSTEM_POWER_OFF),
    IconPair::new(names::SYSTEM_DATABASE, names::SYSTEM_DATABASE_ZAP),
];
