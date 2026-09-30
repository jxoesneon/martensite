//! `arrow` namespace — directional movement, arrows, chevrons, expand/compress, rotate.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`arrow.*`).
///
/// Every name constant in this module is prefixed `ARROW_`
/// (`ARROW_FOO` → `"arrow.foo"`) so the flattened
/// `builtin::names` re-export cannot collide.
pub mod names {
    /// `"arrow.up"` — shaft up plus head.
    pub const ARROW_UP: &str = "arrow.up";
    /// `"arrow.down"` — shaft down plus head.
    pub const ARROW_DOWN: &str = "arrow.down";
    /// `"arrow.left"` — shaft left plus head.
    pub const ARROW_LEFT: &str = "arrow.left";
    /// `"arrow.right"` — shaft right plus head.
    pub const ARROW_RIGHT: &str = "arrow.right";
    /// `"arrow.up-left"` — diagonal to top-left.
    pub const ARROW_UP_LEFT: &str = "arrow.up-left";
    /// `"arrow.up-right"` — diagonal to top-right.
    pub const ARROW_UP_RIGHT: &str = "arrow.up-right";
    /// `"arrow.down-left"` — diagonal to bottom-left.
    pub const ARROW_DOWN_LEFT: &str = "arrow.down-left";
    /// `"arrow.down-right"` — diagonal to bottom-right.
    pub const ARROW_DOWN_RIGHT: &str = "arrow.down-right";
    /// `"arrow.up-down"` — opposing vertical arrows (sort/swap).
    pub const ARROW_UP_DOWN: &str = "arrow.up-down";
    /// `"arrow.left-right"` — opposing horizontal arrows (sort/swap).
    pub const ARROW_LEFT_RIGHT: &str = "arrow.left-right";
    /// `"arrow.corner-down-left"` — hooked arrow turning down-left.
    pub const ARROW_CORNER_DOWN_LEFT: &str = "arrow.corner-down-left";
    /// `"arrow.corner-down-right"` — hooked arrow turning down-right.
    pub const ARROW_CORNER_DOWN_RIGHT: &str = "arrow.corner-down-right";
    /// `"arrow.corner-up-left"` — hooked arrow turning up-left.
    pub const ARROW_CORNER_UP_LEFT: &str = "arrow.corner-up-left";
    /// `"arrow.corner-up-right"` — hooked arrow turning up-right.
    pub const ARROW_CORNER_UP_RIGHT: &str = "arrow.corner-up-right";
    /// `"arrow.move"` — four-way move arrows.
    pub const ARROW_MOVE: &str = "arrow.move";
    /// `"arrow.move-horizontal"` — double-headed horizontal arrow.
    pub const ARROW_MOVE_HORIZONTAL: &str = "arrow.move-horizontal";
    /// `"arrow.move-vertical"` — double-headed vertical arrow.
    pub const ARROW_MOVE_VERTICAL: &str = "arrow.move-vertical";
    /// `"arrow.expand"` — four corner arrows pointing outward.
    pub const ARROW_EXPAND: &str = "arrow.expand";
    /// `"arrow.shrink"` — four corner arrows pointing inward.
    pub const ARROW_SHRINK: &str = "arrow.shrink";
    /// `"arrow.maximize"` — diagonal arrows to opposite corners.
    pub const ARROW_MAXIMIZE: &str = "arrow.maximize";
    /// `"arrow.minimize"` — diagonal arrows collapsing to center.
    pub const ARROW_MINIMIZE: &str = "arrow.minimize";
    /// `"arrow.undo"` — hooked arrow turning back left.
    pub const ARROW_UNDO: &str = "arrow.undo";
    /// `"arrow.redo"` — hooked arrow turning back right.
    pub const ARROW_REDO: &str = "arrow.redo";
    /// `"arrow.reply"` — hooked arrow answering back left.
    pub const ARROW_REPLY: &str = "arrow.reply";
    /// `"arrow.forward"` — hooked arrow passing right.
    pub const ARROW_FORWARD: &str = "arrow.forward";
    /// `"arrow.share"` — arrow rising out of a tray.
    pub const ARROW_SHARE: &str = "arrow.share";
    /// `"arrow.external-link"` — arrow escaping a box top-right.
    pub const ARROW_EXTERNAL_LINK: &str = "arrow.external-link";
    /// `"arrow.log-in"` — arrow entering a door frame.
    pub const ARROW_LOG_IN: &str = "arrow.log-in";
    /// `"arrow.log-out"` — arrow leaving a door frame.
    pub const ARROW_LOG_OUT: &str = "arrow.log-out";
    /// `"arrow.rotate-cw"` — clockwise circular arrow.
    pub const ARROW_ROTATE_CW: &str = "arrow.rotate-cw";
    /// `"arrow.rotate-ccw"` — counter-clockwise circular arrow.
    pub const ARROW_ROTATE_CCW: &str = "arrow.rotate-ccw";
    /// `"arrow.history"` — counter-clockwise circular arrow with hands.
    pub const ARROW_HISTORY: &str = "arrow.history";
    /// `"arrow.refresh-cw"` — two chasing clockwise arcs.
    pub const ARROW_REFRESH_CW: &str = "arrow.refresh-cw";
    /// `"arrow.refresh-ccw"` — two chasing counter-clockwise arcs.
    pub const ARROW_REFRESH_CCW: &str = "arrow.refresh-ccw";
    /// `"arrow.up-circle"` — up arrow inside a circle.
    pub const ARROW_UP_CIRCLE: &str = "arrow.up-circle";
    /// `"arrow.down-circle"` — down arrow inside a circle.
    pub const ARROW_DOWN_CIRCLE: &str = "arrow.down-circle";
    /// `"arrow.left-circle"` — left arrow inside a circle.
    pub const ARROW_LEFT_CIRCLE: &str = "arrow.left-circle";
    /// `"arrow.right-circle"` — right arrow inside a circle.
    pub const ARROW_RIGHT_CIRCLE: &str = "arrow.right-circle";
}

/// `"arrow.up"` — shaft up plus head.
pub const ARROW_UP: &str = "M12 19V5M5 12l7-7 7 7";
/// `"arrow.down"` — shaft down plus head.
pub const ARROW_DOWN: &str = "M12 5v14M5 12l7 7 7-7";
/// `"arrow.left"` — shaft left plus head.
pub const ARROW_LEFT: &str = "M19 12H5M12 19l-7-7 7-7";
/// `"arrow.right"` — shaft right plus head.
pub const ARROW_RIGHT: &str = "M5 12h14M12 5l7 7-7 7";
/// `"arrow.up-left"` — diagonal shaft plus corner head.
pub const ARROW_UP_LEFT: &str = "M17 17L7 7M15 7H7v8";
/// `"arrow.up-right"` — diagonal shaft plus corner head.
pub const ARROW_UP_RIGHT: &str = "M7 17L17 7M9 7h8v8";
/// `"arrow.down-left"` — diagonal shaft plus corner head.
pub const ARROW_DOWN_LEFT: &str = "M17 7L7 17M15 17H7V9";
/// `"arrow.down-right"` — diagonal shaft plus corner head.
pub const ARROW_DOWN_RIGHT: &str = "M7 7l10 10M9 17h8V9";
/// `"arrow.up-down"` — left shaft up, right shaft down.
pub const ARROW_UP_DOWN: &str = "M3 8l4-4 4 4M7 3v18M13 16l4 4 4-4M17 21V3";
/// `"arrow.left-right"` — top shaft left, bottom shaft right.
pub const ARROW_LEFT_RIGHT: &str = "M8 3L4 7l4 4M4 7h16M16 21l4-4-4-4M20 17H4";
/// `"arrow.corner-down-left"` — hook from top-right into a left head.
pub const ARROW_CORNER_DOWN_LEFT: &str = "M9 10l-5 5 5 5M20 4v7a4 4 0 01-4 4H4";
/// `"arrow.corner-down-right"` — hook from top-left into a right head.
pub const ARROW_CORNER_DOWN_RIGHT: &str = "M15 10l5 5-5 5M4 4v7a4 4 0 004 4h12";
/// `"arrow.corner-up-left"` — hook from bottom-right into a left head.
pub const ARROW_CORNER_UP_LEFT: &str = "M9 14L4 9l5-5M20 20v-7a4 4 0 00-4-4H4";
/// `"arrow.corner-up-right"` — hook from bottom-left into a right head.
pub const ARROW_CORNER_UP_RIGHT: &str = "M15 14l5-5-5-5M4 20v-7a4 4 0 014-4h12";
/// `"arrow.move"` — cross shafts with four heads.
pub const ARROW_MOVE: &str =
    "M12 3v18M3 12h18M10 6l3-3 3 3M14 18l-3 3-3-3M6 10l-3 3 3 3M18 10l3 3-3 3";
/// `"arrow.move-horizontal"` — shaft with heads at both ends.
pub const ARROW_MOVE_HORIZONTAL: &str = "M3 12h18M18 9l3 3-3 3M6 9l-3 3 3 3";
/// `"arrow.move-vertical"` — shaft with heads at both ends.
pub const ARROW_MOVE_VERTICAL: &str = "M12 3v18M9 6l3-3 3 3M9 18l3 3-3-3";
/// `"arrow.expand"` — corner heads pointing outward, short shafts.
pub const ARROW_EXPAND: &str =
    "M9 9L3 3M8 3H3v5M15 9l6-6M16 3h5v5M9 15l-6 6M8 21H3v-5M15 15l6 6M16 21h5v-5";
/// `"arrow.shrink"` — corner shafts ending in inward heads.
pub const ARROW_SHRINK: &str =
    "M3 3l6 6M4 9h5V4M21 3l-6 6M20 9h-5V4M3 21l6-6M4 15h5v5M21 21l-6-6M20 15h-5v5";
/// `"arrow.maximize"` — two diagonal shafts opening to the corners.
pub const ARROW_MAXIMIZE: &str = "M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7";
/// `"arrow.minimize"` — two diagonal shafts closing to center.
pub const ARROW_MINIMIZE: &str = "M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7";
/// `"arrow.undo"` — left chevron on a hooked return path.
pub const ARROW_UNDO: &str = "M9 14L4 9l5-5M4 9h10.5a5.5 5.5 0 015.5 5.5 5.5 5.5 0 01-5.5 5.5H11";
/// `"arrow.redo"` — right chevron on a hooked return path.
pub const ARROW_REDO: &str = "M15 14l5-5-5-5M20 9H9.5a5.5 5.5 0 00-5.5 5.5 5.5 5.5 0 005.5 5.5H13";
/// `"arrow.reply"` — left chevron on a hooked drop path.
pub const ARROW_REPLY: &str = "M9 17l-5-5 5-5M20 18v-2a4 4 0 00-4-4H4";
/// `"arrow.forward"` — right chevron on a hooked drop path.
pub const ARROW_FORWARD: &str = "M15 17l5-5-5-5M4 18v-2a4 4 0 014-4h12";
/// `"arrow.share"` — up arrow escaping an open tray.
pub const ARROW_SHARE: &str = "M4 12v8a2 2 0 002 2h12a2 2 0 002-2v-8M16 6l-4-4-4 4M12 2v13";
/// `"arrow.external-link"` — corner arrow above an open box.
pub const ARROW_EXTERNAL_LINK: &str =
    "M15 3h6v6M10 14L21 3M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6";
/// `"arrow.log-in"` — right arrow into a left-open frame.
pub const ARROW_LOG_IN: &str = "M15 3h4a2 2 0 012 2v14a2 2 0 01-2 2h-4M10 17l5-5-5-5M15 12H3";
/// `"arrow.log-out"` — right arrow out of a right-open frame.
pub const ARROW_LOG_OUT: &str = "M9 21H5a2 2 0 01-2-2V5a2 2 0 012-2h4M16 17l5-5-5-5M21 12H9";
/// `"arrow.rotate-cw"` — near-full circle with a head at top-right.
pub const ARROW_ROTATE_CW: &str = "M21 12a9 9 0 11-9-9C14.5 3 17 4 18.75 5.75L21 8M21 3v5h-5";
/// `"arrow.rotate-ccw"` — near-full circle with a head at top-left.
pub const ARROW_ROTATE_CCW: &str = "M3 12a9 9 0 109-9C9.5 3 7 4 5.25 5.75L3 8M3 3v5h5";
/// `"arrow.history"` — `rotate-ccw` circle plus clock hands.
pub const ARROW_HISTORY: &str = "M3 12a9 9 0 109-9C9.5 3 7 4 5.25 5.75L3 8M3 3v5h5M12 7v5l4 2";
/// `"arrow.refresh-cw"` — two arcs chasing heads, clockwise.
pub const ARROW_REFRESH_CW: &str = "M3 12a9 9 0 019-9C14.5 3 17 4 18.75 5.75L21 8M21 3v5h-5M21 12a9 9 0 01-9 9C9.5 21 7 20 5.25 18.25L3 16M8 16H3v5";
/// `"arrow.refresh-ccw"` — two arcs chasing heads, counter-clockwise.
pub const ARROW_REFRESH_CCW: &str = "M21 12a9 9 0 00-9-9C9.5 3 7 4 5.25 5.75L3 8M3 3v5h5M3 12a9 9 0 009 9c2.5 0 5-1 6.75-2.75L21 16M16 16h5v5";
/// `"arrow.up-circle"` — up arrow inside a circle.
pub const ARROW_UP_CIRCLE: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM12 16V8M16 12l-4-4-4 4";
/// `"arrow.down-circle"` — down arrow inside a circle.
pub const ARROW_DOWN_CIRCLE: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM12 8v8M8 12l4 4 4-4";
/// `"arrow.left-circle"` — left arrow inside a circle.
pub const ARROW_LEFT_CIRCLE: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM16 12H8M12 16l-4-4 4-4";
/// `"arrow.right-circle"` — right arrow inside a circle.
pub const ARROW_RIGHT_CIRCLE: &str = "M21 12a9 9 0 11-18 0 9 9 0 0118 0zM8 12h8M12 8l4 4-4 4";

/// `arrow` entries — registered in the pack in this order.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::ARROW_UP, ARROW_UP),
    IconEntry::new(names::ARROW_DOWN, ARROW_DOWN),
    IconEntry::new(names::ARROW_LEFT, ARROW_LEFT),
    IconEntry::new(names::ARROW_RIGHT, ARROW_RIGHT),
    IconEntry::new(names::ARROW_UP_LEFT, ARROW_UP_LEFT),
    IconEntry::new(names::ARROW_UP_RIGHT, ARROW_UP_RIGHT),
    IconEntry::new(names::ARROW_DOWN_LEFT, ARROW_DOWN_LEFT),
    IconEntry::new(names::ARROW_DOWN_RIGHT, ARROW_DOWN_RIGHT),
    IconEntry::new(names::ARROW_UP_DOWN, ARROW_UP_DOWN),
    IconEntry::new(names::ARROW_LEFT_RIGHT, ARROW_LEFT_RIGHT),
    IconEntry::new(names::ARROW_CORNER_DOWN_LEFT, ARROW_CORNER_DOWN_LEFT),
    IconEntry::new(names::ARROW_CORNER_DOWN_RIGHT, ARROW_CORNER_DOWN_RIGHT),
    IconEntry::new(names::ARROW_CORNER_UP_LEFT, ARROW_CORNER_UP_LEFT),
    IconEntry::new(names::ARROW_CORNER_UP_RIGHT, ARROW_CORNER_UP_RIGHT),
    IconEntry::new(names::ARROW_MOVE, ARROW_MOVE),
    IconEntry::new(names::ARROW_MOVE_HORIZONTAL, ARROW_MOVE_HORIZONTAL),
    IconEntry::new(names::ARROW_MOVE_VERTICAL, ARROW_MOVE_VERTICAL),
    IconEntry::new(names::ARROW_EXPAND, ARROW_EXPAND),
    IconEntry::new(names::ARROW_SHRINK, ARROW_SHRINK),
    IconEntry::new(names::ARROW_MAXIMIZE, ARROW_MAXIMIZE),
    IconEntry::new(names::ARROW_MINIMIZE, ARROW_MINIMIZE),
    IconEntry::new(names::ARROW_UNDO, ARROW_UNDO),
    IconEntry::new(names::ARROW_REDO, ARROW_REDO),
    IconEntry::new(names::ARROW_REPLY, ARROW_REPLY),
    IconEntry::new(names::ARROW_FORWARD, ARROW_FORWARD),
    IconEntry::new(names::ARROW_SHARE, ARROW_SHARE),
    IconEntry::new(names::ARROW_EXTERNAL_LINK, ARROW_EXTERNAL_LINK),
    IconEntry::new(names::ARROW_LOG_IN, ARROW_LOG_IN),
    IconEntry::new(names::ARROW_LOG_OUT, ARROW_LOG_OUT),
    IconEntry::new(names::ARROW_ROTATE_CW, ARROW_ROTATE_CW),
    IconEntry::new(names::ARROW_ROTATE_CCW, ARROW_ROTATE_CCW),
    IconEntry::new(names::ARROW_HISTORY, ARROW_HISTORY),
    IconEntry::new(names::ARROW_REFRESH_CW, ARROW_REFRESH_CW),
    IconEntry::new(names::ARROW_REFRESH_CCW, ARROW_REFRESH_CCW),
    IconEntry::new(names::ARROW_UP_CIRCLE, ARROW_UP_CIRCLE),
    IconEntry::new(names::ARROW_DOWN_CIRCLE, ARROW_DOWN_CIRCLE),
    IconEntry::new(names::ARROW_LEFT_CIRCLE, ARROW_LEFT_CIRCLE),
    IconEntry::new(names::ARROW_RIGHT_CIRCLE, ARROW_RIGHT_CIRCLE),
];

/// `arrow` morph pairs.
pub const PAIRS: &[IconPair] = &[
    IconPair::new(names::ARROW_UNDO, names::ARROW_REDO),
    IconPair::new(names::ARROW_EXPAND, names::ARROW_SHRINK),
    IconPair::new(names::ARROW_LOG_IN, names::ARROW_LOG_OUT),
    IconPair::new(names::ARROW_MAXIMIZE, names::ARROW_MINIMIZE),
    IconPair::new(names::ARROW_ROTATE_CW, names::ARROW_ROTATE_CCW),
    IconPair::new(names::ARROW_REFRESH_CW, names::ARROW_REFRESH_CCW),
];
