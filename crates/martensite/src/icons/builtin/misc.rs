//! `misc` namespace — theme, identity, and system affordances.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`misc.*`).
pub mod names {
    /// `"misc.sun"` — light theme / brightness.
    pub const SUN: &str = "misc.sun";
    /// `"misc.moon"` — dark theme / night.
    pub const MOON: &str = "misc.moon";
    /// `"misc.user"` — account / identity.
    pub const USER: &str = "misc.user";
    /// `"misc.terminal"` — console / prompt.
    pub const TERMINAL: &str = "misc.terminal";
    /// `"misc.cpu"` — processor / compute.
    pub const CPU: &str = "misc.cpu";
    /// `"misc.gauge"` — speed / utilization dial.
    pub const GAUGE: &str = "misc.gauge";
    /// `"misc.type"` — typography / text style.
    pub const TYPE: &str = "misc.type";
    /// `"misc.contrast"` — half-shaded disc; accent / swatch.
    pub const CONTRAST: &str = "misc.contrast";
    /// `"misc.award"` — rosette ribbon, achievement.
    pub const AWARD: &str = "misc.award";
    /// `"misc.trophy"` — trophy cup.
    pub const TROPHY: &str = "misc.trophy";
    /// `"misc.crown"` — crown, premium.
    pub const CROWN: &str = "misc.crown";
    /// `"misc.gift"` — gift box with bow.
    pub const GIFT: &str = "misc.gift";
    /// `"misc.tag"` — label tag.
    pub const TAG: &str = "misc.tag";
    /// `"misc.ticket"` — ticket with perforation.
    pub const TICKET: &str = "misc.ticket";
    /// `"misc.rocket"` — rocket, launch.
    pub const ROCKET: &str = "misc.rocket";
    /// `"misc.sparkles"` — four-point star plus glints.
    pub const SPARKLES: &str = "misc.sparkles";
    /// `"misc.graduation-cap"` — mortarboard, education.
    pub const GRADUATION_CAP: &str = "misc.graduation-cap";
    /// `"misc.scale"` — balance scale, justice/compare.
    pub const SCALE: &str = "misc.scale";
}

/// `"misc.sun"` — disc plus eight rays.
pub const MISC_SUN: &str = "M17 12a5 5 0 11-10 0 5 5 0 0110 0zM12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M19.1 4.9l-1.4 1.4M6.3 17.7l-1.4 1.4";
/// `"misc.moon"` — crescent.
pub const MISC_MOON: &str = "M21 12.8A9 9 0 1111.2 3a7 7 0 009.8 9.8z";
/// `"misc.user"` — head circle plus shoulders.
pub const MISC_USER: &str =
    "M16 7a4 4 0 11-8 0 4 4 0 018 0zM20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2";
/// `"misc.terminal"` — prompt chevron plus underscore.
pub const MISC_TERMINAL: &str = "M4 17l6-6-6-6M12 19h8";
/// `"misc.cpu"` — package, die, eight pins.
pub const MISC_CPU: &str =
    "M4 4h16v16H4zM9 9h6v6H9zM9 1v3M15 1v3M9 20v3M15 20v3M1 9h3M1 15h3M20 9h3M20 15h3";
/// `"misc.gauge"` — dial arc plus needle.
pub const MISC_GAUGE: &str = "M3.34 19a10 10 0 1117.32 0M12 14l4-4";
/// `"misc.type"` — cap bar, stem, and baseline serif.
pub const MISC_TYPE: &str = "M4 7V4h16v3M9 20h6M12 4v16";
/// `"misc.contrast"` — disc plus shaded half.
pub const MISC_CONTRAST: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM12 18a6 6 0 000-12v12z";
/// `"misc.award"` — rosette circle plus ribbon tails.
pub const MISC_AWARD: &str =
    "M18 8a6 6 0 11-12 0 6 6 0 0112 0zM15.5 12.9l1.5 8.5a.5.5 0 01-.8.5l-3.6-2.7a1 1 0 00-1.2 0l-3.6 2.7a.5.5 0 01-.8-.5l1.5-8.5";
/// `"misc.trophy"` — cup, handles, stem, base.
pub const MISC_TROPHY: &str =
    "M18 2H6v7a6 6 0 0012 0V2zM6 9H4.5a2.5 2.5 0 010-5H6M18 9h1.5a2.5 2.5 0 000-5H18M10 14.7V17c0 .6-.5 1-1 1.2-1.1.5-2 2-2 3.8M14 14.7V17c0 .6.5 1 1 1.2 1.1.5 2 2 2 3.8M4 22h16";
/// `"misc.crown"` — triple-peak crown band.
pub const MISC_CROWN: &str = "M4 18L3 7l6 5 3-8 3 8 6-5-1 11z";
/// `"misc.gift"` — lid, box, ribbon, bow.
pub const MISC_GIFT: &str =
    "M20 12v9a1 1 0 01-1 1H5a1 1 0 01-1-1v-9M3 7h18v5H3zM12 22V7M12 7H7.5a2.5 2.5 0 010-5C11 2 12 7 12 7zM12 7h4.5a2.5 2.5 0 000-5C13 2 12 7 12 7z";
/// `"misc.tag"` — angled label with hole.
pub const MISC_TAG: &str =
    "M12.586 2.586A2 2 0 0011.172 2H4a2 2 0 00-2 2v7.172a2 2 0 00.586 1.414l8.704 8.704a2.426 2.426 0 003.42 0l6.58-6.58a2.426 2.426 0 000-3.42zM7.5 7.5h.01";
/// `"misc.ticket"` — stub with edge notches and perforation.
pub const MISC_TICKET: &str =
    "M2 9a3 3 0 010 6v3a2 2 0 002 2h16a2 2 0 002-2v-3a3 3 0 010-6V6a2 2 0 00-2-2H4a2 2 0 00-2 2zM13 5v2M13 11v2M13 17v2";
/// `"misc.rocket"` — body, fins, flame.
pub const MISC_ROCKET: &str =
    "M4.5 16.5C3 17.8 2.5 21.5 2.5 21.5C2.5 21.5 6.2 21 7.5 19.5C8.2 18.7 8.2 17.4 7.4 16.6A2.18 2.18 0 004.5 16.5zM12 15l-3-3a22 22 0 012-3.95A12.88 12.88 0 0122 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 01-4 2zM9 12H4C4 12 4.55 8.97 6 8C7.62 6.92 11 8 11 8M12 15v5C12 20 15.03 19.45 16 18C17.08 16.38 16 13 16 13";
/// `"misc.sparkles"` — big four-point star plus two glints.
pub const MISC_SPARKLES: &str =
    "M12 3l1.9 5.8a2 2 0 001.3 1.3L21 12l-5.8 1.9a2 2 0 00-1.3 1.3L12 21l-1.9-5.8a2 2 0 00-1.3-1.3L3 12l5.8-1.9a2 2 0 001.3-1.3zM5 3v4M3 5h4M19 17v4M17 19h4";
/// `"misc.graduation-cap"` — mortarboard, band, tassel.
pub const MISC_GRADUATION_CAP: &str = "M2 10l10-5 10 5-10 5zM6 12v5c3 3 9 3 12 0v-5M22 10v6";
/// `"misc.scale"` — beam balance on a post.
pub const MISC_SCALE: &str =
    "M12 3v18M7 21h10M5 7h14M2 15l3-8 3 8c-.9.65-1.9 1-3 1s-2.1-.35-3-1zM16 15l3-8 3 8c-.9.65-1.9 1-3 1s-2.1-.35-3-1z";

/// `misc` entries.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::SUN, MISC_SUN),
    IconEntry::new(names::MOON, MISC_MOON),
    IconEntry::new(names::USER, MISC_USER),
    IconEntry::new(names::TERMINAL, MISC_TERMINAL),
    IconEntry::new(names::CPU, MISC_CPU),
    IconEntry::new(names::GAUGE, MISC_GAUGE),
    IconEntry::new(names::TYPE, MISC_TYPE),
    IconEntry::new(names::CONTRAST, MISC_CONTRAST),
    IconEntry::new(names::AWARD, MISC_AWARD),
    IconEntry::new(names::TROPHY, MISC_TROPHY),
    IconEntry::new(names::CROWN, MISC_CROWN),
    IconEntry::new(names::GIFT, MISC_GIFT),
    IconEntry::new(names::TAG, MISC_TAG),
    IconEntry::new(names::TICKET, MISC_TICKET),
    IconEntry::new(names::ROCKET, MISC_ROCKET),
    IconEntry::new(names::SPARKLES, MISC_SPARKLES),
    IconEntry::new(names::GRADUATION_CAP, MISC_GRADUATION_CAP),
    IconEntry::new(names::SCALE, MISC_SCALE),
];

/// `misc` morph pairs.
pub const PAIRS: &[IconPair] = &[IconPair::new(names::SUN, names::MOON)];
