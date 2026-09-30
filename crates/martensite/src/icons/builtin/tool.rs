//! `tool` namespace — instruments, sliders, repair, measurement.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`tool.*`).
///
/// Every name constant in this module is prefixed `TOOL_`
/// (`TOOL_FOO` → `"tool.foo"`) so the flattened
/// `builtin::names` re-export cannot collide.
pub mod names {
    /// `"tool.wrench"` — open-end wrench.
    pub const WRENCH: &str = "tool.wrench";
    /// `"tool.hammer"` — hammer.
    pub const HAMMER: &str = "tool.hammer";
    /// `"tool.screwdriver"` — screwdriver.
    pub const SCREWDRIVER: &str = "tool.screwdriver";
    /// `"tool.ruler"` — diagonal ruler with ticks.
    pub const RULER: &str = "tool.ruler";
    /// `"tool.pencil-ruler"` — pencil and ruler.
    pub const PENCIL_RULER: &str = "tool.pencil-ruler";
    /// `"tool.sliders-horizontal"` — horizontal sliders.
    pub const SLIDERS_HORIZONTAL: &str = "tool.sliders-horizontal";
    /// `"tool.construction"` — striped barrier.
    pub const CONSTRUCTION: &str = "tool.construction";
    /// `"tool.traffic-cone"` — traffic cone.
    pub const TRAFFIC_CONE: &str = "tool.traffic-cone";
    /// `"tool.life-buoy"` — life ring, help/support.
    pub const LIFE_BUOY: &str = "tool.life-buoy";
    /// `"tool.magnet"` — horseshoe magnet.
    pub const MAGNET: &str = "tool.magnet";
    /// `"tool.puzzle"` — puzzle piece.
    pub const PUZZLE: &str = "tool.puzzle";
    /// `"tool.lightbulb"` — idea bulb.
    pub const LIGHTBULB: &str = "tool.lightbulb";
    /// `"tool.flashlight"` — flashlight.
    pub const FLASHLIGHT: &str = "tool.flashlight";
    /// `"tool.flashlight-off"` — flashlight struck through.
    pub const FLASHLIGHT_OFF: &str = "tool.flashlight-off";
    /// `"tool.telescope"` — telescope on tripod.
    pub const TELESCOPE: &str = "tool.telescope";
    /// `"tool.microscope"` — microscope.
    pub const MICROSCOPE: &str = "tool.microscope";
}

/// `"tool.wrench"` — open jaw plus long handle.
pub const TOOL_WRENCH: &str =
    "M14.7 6.3a1 1 0 000 1.4l1.6 1.6a1 1 0 001.4 0l3.77-3.77a6 6 0 01-7.94 7.94l-6.91 6.91a2.12 2.12 0 01-3-3l6.91-6.91a6 6 0 017.94-7.94l-3.76 3.76z";
/// `"tool.hammer"` — angled head plus diagonal handle.
pub const TOOL_HAMMER: &str = "M13 4l7 7-3 3-7-7zM13 11L4 20";
/// `"tool.screwdriver"` — tip, shaft, rounded handle.
pub const TOOL_SCREWDRIVER: &str =
    "M14 10l3.5-3.5a2.12 2.12 0 013 3L17 13zM15.5 11.5L4.5 19.5M4.5 19.5L2.5 21.5";
/// `"tool.ruler"` — diagonal ruler with tick marks.
pub const TOOL_RULER: &str =
    "M21.3 15.3a2.4 2.4 0 010 3.4l-2.6 2.6a2.4 2.4 0 01-3.4 0L2.7 8.7a2.4 2.4 0 010-3.4l2.6-2.6a2.4 2.4 0 013.4 0zM14.5 12.5l2-2M11.5 9.5l2-2M8.5 6.5l2-2M17.5 15.5l2-2";
/// `"tool.pencil-ruler"` — pencil over ruler, diagonal pair.
pub const TOOL_PENCIL_RULER: &str =
    "M13 7L8.7 2.7a2.4 2.4 0 00-3.4 0L2.7 5.3a2.4 2.4 0 000 3.4L7 13M8 6l2-2M17 11l4.3 4.3a2.4 2.4 0 010 3.4l-2.6 2.6a2.4 2.4 0 01-3.4 0L11 17M14.5 14.5l1.5 1.5";
/// `"tool.sliders-horizontal"` — three rails with knobs.
pub const TOOL_SLIDERS_HORIZONTAL: &str =
    "M21 4h-7M10 4H3M21 12h-9M8 12H3M21 20h-5M12 20H3M14 2v4M8 10v4M16 18v4";
/// `"tool.construction"` — striped barrier on posts.
pub const TOOL_CONSTRUCTION: &str =
    "M4 6h16a1 1 0 011 1v6a1 1 0 01-1 1H4a1 1 0 01-1-1V7a1 1 0 011-1zM7 3v3M17 3v3M7 14v7M17 14v7M4.5 14L10 6M10.5 14L16 6M16.5 14L21 8.5";
/// `"tool.traffic-cone"` — banded cone on a base.
pub const TOOL_TRAFFIC_CONE: &str = "M10 3h4L19.5 18H4.5L10 3zM6.7 12h10.6M5.6 15h12.8M2 21h20";
/// `"tool.life-buoy"` — ring, hub, four spokes.
pub const TOOL_LIFE_BUOY: &str =
    "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM16 12a4 4 0 11-8 0 4 4 0 018 0zM5.7 5.7l3.5 3.5M18.3 18.3l-3.5-3.5M5.7 18.3l3.5-3.5M18.3 5.7l-3.5 3.5";
/// `"tool.magnet"` — horseshoe magnet with poles.
pub const TOOL_MAGNET: &str =
    "M6 15L2 11L8.75 4.23A7.79 7.79 0 0119.75 15.23L13 22L9 18L15.39 11.64A2.14 2.14 0 0012.36 8.61L6 15M5 8l4 4M12 15l4 4";
/// `"tool.puzzle"` — piece with tab and slot edges.
pub const TOOL_PUZZLE: &str =
    "M4 7h4a2 2 0 104 0h4v4a2 2 0 100 4v4h-4a2 2 0 10-4 0H4v-4a2 2 0 100-4z";
/// `"tool.lightbulb"` — bulb globe plus base.
pub const TOOL_LIGHTBULB: &str =
    "M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 006 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5M9 18h6M10 22h4";
/// `"tool.flashlight"` — head taper plus body.
pub const TOOL_FLASHLIGHT: &str =
    "M18 6c0 2-2 2-2 4v10a2 2 0 01-2 2h-4a2 2 0 01-2-2V10c0-2-2-2-2-4V2h12zM6 6h12M12 12h.01";
/// `"tool.flashlight-off"` — flashlight plus strike slash.
pub const TOOL_FLASHLIGHT_OFF: &str =
    "M18 6c0 2-2 2-2 4v10a2 2 0 01-2 2h-4a2 2 0 01-2-2V10c0-2-2-2-2-4V2h12zM6 6h12M12 12h.01M2 2l20 20";
/// `"tool.telescope"` — tilted tube on tripod legs.
pub const TOOL_TELESCOPE: &str =
    "M4.1 11.4L16.1 2.4L19.9 7.6L7.9 16.6zM6 14L3.5 17.5M11.5 14L8 21M11.5 14L15.5 21";
/// `"tool.microscope"` — tube, stage, C-arm, base.
pub const TOOL_MICROSCOPE: &str =
    "M6 18h8M3 22h18M14 22a7 7 0 100-14M9 14h2M9 12a2 2 0 01-2-2V6h6v4a2 2 0 01-2 2zM12 6V3a1 1 0 00-1-1H9a1 1 0 00-1 1v3";

/// `tool` entries — registered in the pack in this order.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::WRENCH, TOOL_WRENCH),
    IconEntry::new(names::HAMMER, TOOL_HAMMER),
    IconEntry::new(names::SCREWDRIVER, TOOL_SCREWDRIVER),
    IconEntry::new(names::RULER, TOOL_RULER),
    IconEntry::new(names::PENCIL_RULER, TOOL_PENCIL_RULER),
    IconEntry::new(names::SLIDERS_HORIZONTAL, TOOL_SLIDERS_HORIZONTAL),
    IconEntry::new(names::CONSTRUCTION, TOOL_CONSTRUCTION),
    IconEntry::new(names::TRAFFIC_CONE, TOOL_TRAFFIC_CONE),
    IconEntry::new(names::LIFE_BUOY, TOOL_LIFE_BUOY),
    IconEntry::new(names::MAGNET, TOOL_MAGNET),
    IconEntry::new(names::PUZZLE, TOOL_PUZZLE),
    IconEntry::new(names::LIGHTBULB, TOOL_LIGHTBULB),
    IconEntry::new(names::FLASHLIGHT, TOOL_FLASHLIGHT),
    IconEntry::new(names::FLASHLIGHT_OFF, TOOL_FLASHLIGHT_OFF),
    IconEntry::new(names::TELESCOPE, TOOL_TELESCOPE),
    IconEntry::new(names::MICROSCOPE, TOOL_MICROSCOPE),
];

/// `tool` morph pairs.
pub const PAIRS: &[IconPair] = &[IconPair::new(names::FLASHLIGHT, names::FLASHLIGHT_OFF)];
