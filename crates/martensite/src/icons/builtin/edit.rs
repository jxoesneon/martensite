//! `edit` namespace — document and editing verbs.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`edit.*`).
pub mod names {
    /// `"edit.pen"` — compose / annotate.
    pub const PEN: &str = "edit.pen";
    /// `"edit.trash"` — delete.
    pub const TRASH: &str = "edit.trash";
    /// `"edit.save"` — persist / floppy.
    pub const SAVE: &str = "edit.save";
    /// `"edit.download"` — pull to disk.
    pub const DOWNLOAD: &str = "edit.download";
    /// `"edit.upload"` — push from disk.
    pub const UPLOAD: &str = "edit.upload";
    /// `"edit.refresh"` — reload / sync.
    pub const REFRESH: &str = "edit.refresh";
    /// `"edit.pencil"` — pencil with band.
    pub const EDIT_PENCIL: &str = "edit.pencil";
    /// `"edit.pencil-line"` — pencil over a baseline.
    pub const EDIT_PENCIL_LINE: &str = "edit.pencil-line";
    /// `"edit.pen-line"` — nib over a baseline.
    pub const EDIT_PEN_LINE: &str = "edit.pen-line";
    /// `"edit.eraser"` — erase.
    pub const EDIT_ERASER: &str = "edit.eraser";
    /// `"edit.highlighter"` — highlight marker.
    pub const EDIT_HIGHLIGHTER: &str = "edit.highlighter";
    /// `"edit.paintbrush"` — paint tool.
    pub const EDIT_PAINTBRUSH: &str = "edit.paintbrush";
    /// `"edit.palette"` — color palette.
    pub const EDIT_PALETTE: &str = "edit.palette";
    /// `"edit.pipette"` — color picker / eyedropper.
    pub const EDIT_PIPETTE: &str = "edit.pipette";
    /// `"edit.crop"` — crop frame.
    pub const EDIT_CROP: &str = "edit.crop";
    /// `"edit.scissors"` — cut.
    pub const EDIT_SCISSORS: &str = "edit.scissors";
    /// `"edit.wand"` — magic wand (auto-select / enhance).
    pub const EDIT_WAND: &str = "edit.wand";
    /// `"edit.layers"` — stacked layers.
    pub const EDIT_LAYERS: &str = "edit.layers";
    /// `"edit.stamp"` — rubber stamp / approve.
    pub const EDIT_STAMP: &str = "edit.stamp";
    /// `"edit.hand"` — open hand / pan tool.
    pub const EDIT_HAND: &str = "edit.hand";
    /// `"edit.grab"` — closed hand / dragging.
    pub const EDIT_GRAB: &str = "edit.grab";
    /// `"edit.mouse-pointer"` — cursor select.
    pub const EDIT_MOUSE_POINTER: &str = "edit.mouse-pointer";
    /// `"edit.mouse-pointer-click"` — cursor click.
    pub const EDIT_MOUSE_POINTER_CLICK: &str = "edit.mouse-pointer-click";
    /// `"edit.select"` — marquee selection box.
    pub const EDIT_SELECT: &str = "edit.select";
    /// `"edit.flip-horizontal"` — mirror horizontally.
    pub const EDIT_FLIP_HORIZONTAL: &str = "edit.flip-horizontal";
    /// `"edit.flip-vertical"` — mirror vertically.
    pub const EDIT_FLIP_VERTICAL: &str = "edit.flip-vertical";
}

/// `"edit.pen"` — diagonal pencil with band.
pub const EDIT_PEN: &str = "M17 3a2.83 2.83 0 114 4L7.5 20.5 2 22l1.5-5.5L17 3z";
/// `"edit.trash"` — lid, can, twin grooves.
pub const EDIT_TRASH: &str = "M3 6h18M8 6V4a2 2 0 012-2h4a2 2 0 012 2v2M19 6l-1 14a2 2 0 01-2 2H8a2 2 0 01-2-2L5 6M10 11v6M14 11v6";
/// `"edit.save"` — floppy body, shutter slot, label.
pub const EDIT_SAVE: &str =
    "M19 21H5a2 2 0 01-2-2V5a2 2 0 012-2h11l5 5v11a2 2 0 01-2 2zM17 21v-8H7v8M7 3v5h8";
/// `"edit.download"` — tray plus down arrow.
pub const EDIT_DOWNLOAD: &str = "M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4M7 10l5 5 5-5M12 15V3";
/// `"edit.upload"` — tray plus up arrow.
pub const EDIT_UPLOAD: &str = "M21 15v4a2 2 0 01-2-2v-4M17 8l-5-5-5 5M12 3v12";
/// `"edit.refresh"` — twin chasing arcs.
pub const EDIT_REFRESH: &str =
    "M21 4v6h-6M3 20v-6h6M3.5 9a9 9 0 0114.9-3.4L21 10M3 14l4.6 4.4A9 9 0 0020.5 15";
/// `"edit.pencil"` — pen body plus a ferrule band.
pub const EDIT_PENCIL: &str = "M17 3a2.83 2.83 0 114 4L7.5 20.5 2 22l1.5-5.5L17 3zM15 5l4 4";
/// `"edit.pencil-line"` — pencil plus a writing baseline.
pub const EDIT_PENCIL_LINE: &str =
    "M17 3a2.83 2.83 0 114 4L7.5 20.5 2 22l1.5-5.5L17 3zM15 5l4 4M9 21h13";
/// `"edit.pen-line"` — bare nib plus a writing baseline.
pub const EDIT_PEN_LINE: &str = "M16.5 3.5a2.12 2.12 0 013 3L7 19l-4 1 1-4L16.5 3.5zM9 21h13";
/// `"edit.eraser"` — tilted eraser block on a baseline.
pub const EDIT_ERASER: &str =
    "M7 21l-4.3-4.3a2.4 2.4 0 010-3.4l9.6-9.6a2.4 2.4 0 013.4 0l5.6 5.6a2.4 2.4 0 010 3.4L13 21M22 21H7M5 11l9 9";
/// `"edit.highlighter"` — chisel marker plus nib.
pub const EDIT_HIGHLIGHTER: &str =
    "M9 11l-6 6v3h9l3-3M22 12l-4.6 4.6a2 2 0 01-2.8 0l-5.2-5.2a2 2 0 010-2.8L14 4";
/// `"edit.paintbrush"` — diagonal brush with a paint blob.
pub const EDIT_PAINTBRUSH: &str =
    "M9.06 11.9l8.07-8.06a2.85 2.85 0 114.03 4.03l-8.06 8.08M7.07 14.94c-1.66 0-3 1.35-3 3.02 0 1.33-2.5 1.52-2 2.02 1.08 1.1 2.49 2.02 4 2.02 2.2 0 4-1.8 4-4.04a3.01 3.01 0 00-3-3.02z";
/// `"edit.palette"` — artist palette with four dabs.
pub const EDIT_PALETTE: &str =
    "M12 2a10 10 0 00-10 10c0 5.5 4.5 10 10 10a2.5 2.5 0 002.5-2.5c0-.6-.25-1.2-.66-1.65a2.5 2.5 0 01-.67-1.65A2.5 2.5 0 0115.5 14h2.5a4 4 0 004-4C22 5.4 17.5 2 12 2zM13.5 6.5h0.01M17.5 10.5h0.01M8.5 7.5h0.01M6.5 12.5h0.01";
/// `"edit.pipette"` — eyedropper tube with a collar.
pub const EDIT_PIPETTE: &str = "M5 19l2-6L17 3a2.83 2.83 0 014 4L11 17l-6 2zM13.5 6.5l4 4";
/// `"edit.crop"` — two overlapping crop rails.
pub const EDIT_CROP: &str = "M6 2v14a2 2 0 002 2h14M18 22V8a2 2 0 00-2-2H2";
/// `"edit.scissors"` — twin loops plus crossing blades.
pub const EDIT_SCISSORS: &str =
    "M9 6a3 3 0 11-6 0 3 3 0 016 0zM8.12 8.12L12 12M20 4L8.12 15.88M9 18a3 3 0 11-6 0 3 3 0 016 0zM14.8 14.8L20 20";
/// `"edit.wand"` — magic wand surrounded by sparks.
pub const EDIT_WAND: &str =
    "M21.64 3.64l-1.28-1.28a1.21 1.21 0 00-1.72 0L2.36 18.64a1.21 1.21 0 000 1.72l1.28 1.28a1.2 1.2 0 001.72 0L21.64 5.36a1.2 1.2 0 000-1.72zM14 7l3 3M5 6v4M19 14v4M10 2v2M7 8H3M21 16h-4M11 3H9";
/// `"edit.layers"` — top sheet over two stacked layers.
pub const EDIT_LAYERS: &str =
    "M12.83 2.18a2 2 0 00-1.66 0L2.6 6.08a1 1 0 000 1.83l8.58 3.91a2 2 0 001.66 0l8.58-3.9a1 1 0 000-1.83l-8.58-3.91M22 17.65l-9.17 4.16a2 2 0 01-1.66 0L2 17.65M22 12.65l-9.17 4.16a2 2 0 01-1.66 0L2 12.65";
/// `"edit.stamp"` — knob, flared neck, plate, impression.
pub const EDIT_STAMP: &str =
    "M14 4.5a2 2 0 11-4 0 2 2 0 014 0zM10 6.5c-.5 3.5-2 5-4.5 5.5h13c-2.5-.5-4-2-4.5-5.5M4 12h16v5H4zM5 20.5h14";
/// `"edit.hand"` — open palm with thumb.
pub const EDIT_HAND: &str =
    "M18 11V6.5a1.5 1.5 0 00-3 0V11M14 10V4.5a1.5 1.5 0 00-3 0v6M10 10.5V6.5a1.5 1.5 0 00-3 0v8M18 8a2 2 0 114 0v6a8 8 0 01-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 012.83-2.82L7 15";
/// `"edit.grab"` — closed fist with tucked thumb.
pub const EDIT_GRAB: &str =
    "M18 11.5V9a2 2 0 00-2-2 2 2 0 00-2 2v1.4M14 10V8a2 2 0 00-2-2 2 2 0 00-2 2v2M10 9.9V9a2 2 0 00-2-2 2 2 0 00-2 2v5M6 14a2 2 0 00-2-2M18 11a2 2 0 114 0v3a8 8 0 01-8 8h-4a8 8 0 01-8-8 2 2 0 114 0";
/// `"edit.mouse-pointer"` — arrow cursor plus tail.
pub const EDIT_MOUSE_POINTER: &str = "M3 3l7.07 16.97 2.51-7.39 7.39-2.51L3 3zM13 13l6 6";
/// `"edit.mouse-pointer-click"` — cursor plus click rays.
pub const EDIT_MOUSE_POINTER_CLICK: &str =
    "M9 9l5.07 11.97 1.72-5.05L20.94 14L9 9zM14.5 4.5L12 7M5.5 6.5L8 9M4.5 14.5L7 12";
/// `"edit.select"` — dashed marquee rectangle.
pub const EDIT_SELECT: &str =
    "M7 3H5a2 2 0 00-2 2v2M17 3h2a2 2 0 012 2v2M21 17v2a2 2 0 01-2 2h-2M7 21H5a2 2 0 01-2-2v-2M10 3h4M10 21h4M3 10v4M21 10v4";
/// `"edit.flip-horizontal"` — panels mirrored on a dashed axis.
pub const EDIT_FLIP_HORIZONTAL: &str =
    "M8 3H5a2 2 0 00-2 2v14a2 2 0 002 2h3M16 3h3a2 2 0 012 2v14a2 2 0 01-2 2h-3M12 20v2M12 14v2M12 8v2M12 2v2";
/// `"edit.flip-vertical"` — panels mirrored on a dashed axis.
pub const EDIT_FLIP_VERTICAL: &str =
    "M3 8V5a2 2 0 012-2h14a2 2 0 012 2v3M3 16v3a2 2 0 002 2h14a2 2 0 002-2v-3M20 12h2M14 12h2M8 12h2M2 12h2";

/// `edit` entries.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::PEN, EDIT_PEN),
    IconEntry::new(names::TRASH, EDIT_TRASH),
    IconEntry::new(names::SAVE, EDIT_SAVE),
    IconEntry::new(names::DOWNLOAD, EDIT_DOWNLOAD),
    IconEntry::new(names::UPLOAD, EDIT_UPLOAD),
    IconEntry::new(names::REFRESH, EDIT_REFRESH),
    IconEntry::new(names::EDIT_PENCIL, EDIT_PENCIL),
    IconEntry::new(names::EDIT_PENCIL_LINE, EDIT_PENCIL_LINE),
    IconEntry::new(names::EDIT_PEN_LINE, EDIT_PEN_LINE),
    IconEntry::new(names::EDIT_ERASER, EDIT_ERASER),
    IconEntry::new(names::EDIT_HIGHLIGHTER, EDIT_HIGHLIGHTER),
    IconEntry::new(names::EDIT_PAINTBRUSH, EDIT_PAINTBRUSH),
    IconEntry::new(names::EDIT_PALETTE, EDIT_PALETTE),
    IconEntry::new(names::EDIT_PIPETTE, EDIT_PIPETTE),
    IconEntry::new(names::EDIT_CROP, EDIT_CROP),
    IconEntry::new(names::EDIT_SCISSORS, EDIT_SCISSORS),
    IconEntry::new(names::EDIT_WAND, EDIT_WAND),
    IconEntry::new(names::EDIT_LAYERS, EDIT_LAYERS),
    IconEntry::new(names::EDIT_STAMP, EDIT_STAMP),
    IconEntry::new(names::EDIT_HAND, EDIT_HAND),
    IconEntry::new(names::EDIT_GRAB, EDIT_GRAB),
    IconEntry::new(names::EDIT_MOUSE_POINTER, EDIT_MOUSE_POINTER),
    IconEntry::new(names::EDIT_MOUSE_POINTER_CLICK, EDIT_MOUSE_POINTER_CLICK),
    IconEntry::new(names::EDIT_SELECT, EDIT_SELECT),
    IconEntry::new(names::EDIT_FLIP_HORIZONTAL, EDIT_FLIP_HORIZONTAL),
    IconEntry::new(names::EDIT_FLIP_VERTICAL, EDIT_FLIP_VERTICAL),
];

/// `edit` morph pairs.
pub const PAIRS: &[IconPair] = &[
    IconPair::new(names::DOWNLOAD, names::UPLOAD),
    IconPair::new(names::EDIT_HAND, names::EDIT_GRAB),
];
