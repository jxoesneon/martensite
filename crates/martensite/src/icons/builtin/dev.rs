//! `dev` namespace — code, git, terminals, debugging.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`dev.*`).
///
/// Every name constant in this module is prefixed `DEV_`
/// (`DEV_FOO` → `"dev.foo"`) so the flattened
/// `builtin::names` re-export cannot collide.
pub mod names {
    /// `"dev.code"` — angle brackets, code.
    pub const CODE: &str = "dev.code";
    /// `"dev.braces"` — curly braces.
    pub const BRACES: &str = "dev.braces";
    /// `"dev.brackets"` — square brackets.
    pub const BRACKETS: &str = "dev.brackets";
    /// `"dev.binary"` — 0/1 digits, binary.
    pub const BINARY: &str = "dev.binary";
    /// `"dev.regex"` — asterisk plus capture square.
    pub const REGEX: &str = "dev.regex";
    /// `"dev.bug"` — bug, defect.
    pub const BUG: &str = "dev.bug";
    /// `"dev.bug-off"` — bug struck through, debug suppressed.
    pub const BUG_OFF: &str = "dev.bug-off";
    /// `"dev.git-branch"` — branch node and merge curve.
    pub const GIT_BRANCH: &str = "dev.git-branch";
    /// `"dev.git-commit"` — node on a line.
    pub const GIT_COMMIT: &str = "dev.git-commit";
    /// `"dev.git-merge"` — two nodes merging.
    pub const GIT_MERGE: &str = "dev.git-merge";
    /// `"dev.git-pull-request"` — pull request.
    pub const GIT_PULL_REQUEST: &str = "dev.git-pull-request";
    /// `"dev.diff"` — plus/minus in a circle.
    pub const DIFF: &str = "dev.diff";
    /// `"dev.bot"` — robot head.
    pub const BOT: &str = "dev.bot";
    /// `"dev.boxes"` — three stacked cubes.
    pub const BOXES: &str = "dev.boxes";
}

/// `"dev.code"` — opposing angle brackets.
pub const DEV_CODE: &str = "M16 18l6-6-6-6M8 6l-6 6 6 6";
/// `"dev.braces"` — left and right curly braces.
pub const DEV_BRACES: &str =
    "M8 3H7a2 2 0 00-2 2v4a2 2 0 01-2 2 2 2 0 012 2v4a2 2 0 002 2h1M16 21h-1a2 2 0 01-2-2v-4a2 2 0 00-2-2 2 2 0 002-2V5a2 2 0 012-2h1";
/// `"dev.brackets"` — left and right square brackets.
pub const DEV_BRACKETS: &str =
    "M8 3H5a2 2 0 00-2 2v14a2 2 0 002 2h3M16 3h3a2 2 0 012 2v14a2 2 0 01-2 2h-3";
/// `"dev.binary"` — alternating rounded 0s and 1s.
pub const DEV_BINARY: &str =
    "M8 4a2 2 0 012 2v2a2 2 0 01-2 2 2 2 0 01-2-2V6a2 2 0 012-2zM16 14a2 2 0 012 2v2a2 2 0 01-2 2 2 2 0 01-2-2v-2a2 2 0 012-2zM14 4h2v6M14 10h4M6 14h2v6M6 20h4";
/// `"dev.regex"` — asterisk plus match square.
pub const DEV_REGEX: &str =
    "M17 3v10M12.7 5.5l8.7 5M12.7 10.5l8.7-5M5 13h2a2 2 0 012 2v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4a2 2 0 012-2z";
/// `"dev.bug"` — body, head, antennae, six legs.
pub const DEV_BUG: &str =
    "M8 2l1.9 1.9M16 2l-1.9 1.9M9 7.1v-1a3 3 0 116 0v1M12 20c-3.3 0-6-2.7-6-6v-3a4 4 0 014-4h4a4 4 0 014 4v3c0 3.3-2.7 6-6 6zM12 20v-9M6.5 9C4.6 8.8 3 7.1 3 5M6 13H2M3 21c0-2.1 1.7-3.9 3.8-4M21 5c0 2.1-1.6 3.8-3.5 4M22 13h-4M20.8 17c-2.1.1-3.8 1.9-3.8 4";
/// `"dev.bug-off"` — bug plus strike slash.
pub const DEV_BUG_OFF: &str =
    "M8 2l1.9 1.9M16 2l-1.9 1.9M9 7.1v-1a3 3 0 116 0v1M12 20c-3.3 0-6-2.7-6-6v-3a4 4 0 014-4h4a4 4 0 014 4v3c0 3.3-2.7 6-6 6zM12 20v-9M6.5 9C4.6 8.8 3 7.1 3 5M6 13H2M3 21c0-2.1 1.7-3.9 3.8-4M21 5c0 2.1-1.6 3.8-3.5 4M22 13h-4M20.8 17c-2.1.1-3.8 1.9-3.8 4M2 2l20 20";
/// `"dev.git-branch"` — stem plus branch curve and two nodes.
pub const DEV_GIT_BRANCH: &str =
    "M6 3v12M9 18a3 3 0 11-6 0 3 3 0 016 0zM21 6a3 3 0 11-6 0 3 3 0 016 0zM18 9a9 9 0 01-9 9";
/// `"dev.git-commit"` — node centered on a line.
pub const DEV_GIT_COMMIT: &str = "M16 12a4 4 0 11-8 0 4 4 0 018 0zM2 12h6M16 12h6";
/// `"dev.git-merge"` — trunk node plus merged branch node.
pub const DEV_GIT_MERGE: &str =
    "M9 6a3 3 0 11-6 0 3 3 0 016 0zM21 18a3 3 0 11-6 0 3 3 0 016 0zM6 21V9a9 9 0 009 9";
/// `"dev.git-pull-request"` — trunk node plus incoming branch.
pub const DEV_GIT_PULL_REQUEST: &str =
    "M9 6a3 3 0 11-6 0 3 3 0 016 0zM21 18a3 3 0 11-6 0 3 3 0 016 0zM6 21V9M13 6h3a2 2 0 012 2v7";
/// `"dev.diff"` — circle with plus over minus.
pub const DEV_DIFF: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM10 7h4M12 5v4M10 16h4";
/// `"dev.bot"` — robot head with antenna and ear stubs.
pub const DEV_BOT: &str =
    "M12 8V4H8M6 8h12a2 2 0 012 2v8a2 2 0 01-2 2H6a2 2 0 01-2-2v-8a2 2 0 012-2zM2 14h2M20 14h2M9 13v2M15 13v2";
/// `"dev.boxes"` — three isometric cubes, two low one high.
pub const DEV_BOXES: &str =
    "M12 3l4.5 2.5v5L12 13l-4.5-2.5v-5zM12 8l4.5-2.5M12 8l-4.5-2.5M12 8v5M6.5 12l4.5 2.5v5L6.5 22L2 19.5v-5zM6.5 17l4.5-2.5M6.5 17l-4.5-2.5M6.5 17v5M17.5 12L22 14.5V19.5L17.5 22L13 19.5v-5zM17.5 17l4.5-2.5M17.5 17L13 14.5M17.5 17v5";

/// `dev` entries — registered in the pack in this order.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::CODE, DEV_CODE),
    IconEntry::new(names::BRACES, DEV_BRACES),
    IconEntry::new(names::BRACKETS, DEV_BRACKETS),
    IconEntry::new(names::BINARY, DEV_BINARY),
    IconEntry::new(names::REGEX, DEV_REGEX),
    IconEntry::new(names::BUG, DEV_BUG),
    IconEntry::new(names::BUG_OFF, DEV_BUG_OFF),
    IconEntry::new(names::GIT_BRANCH, DEV_GIT_BRANCH),
    IconEntry::new(names::GIT_COMMIT, DEV_GIT_COMMIT),
    IconEntry::new(names::GIT_MERGE, DEV_GIT_MERGE),
    IconEntry::new(names::GIT_PULL_REQUEST, DEV_GIT_PULL_REQUEST),
    IconEntry::new(names::DIFF, DEV_DIFF),
    IconEntry::new(names::BOT, DEV_BOT),
    IconEntry::new(names::BOXES, DEV_BOXES),
];

/// `dev` morph pairs.
pub const PAIRS: &[IconPair] = &[IconPair::new(names::BUG, names::BUG_OFF)];
