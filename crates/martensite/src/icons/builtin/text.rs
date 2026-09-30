//! `text` namespace — typography, formatting, alignment, lists, links.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`text.*`).
///
/// Every name constant in this module is prefixed `TEXT_`
/// (`TEXT_FOO` → `"text.foo"`) so the flattened
/// `builtin::names` re-export cannot collide.
pub mod names {
    /// `"text.bold"` — boldface.
    pub const BOLD: &str = "text.bold";
    /// `"text.italic"` — italic slant.
    pub const ITALIC: &str = "text.italic";
    /// `"text.underline"` — underlined text.
    pub const UNDERLINE: &str = "text.underline";
    /// `"text.strikethrough"` — struck text.
    pub const STRIKETHROUGH: &str = "text.strikethrough";
    /// `"text.align-left"` — flush-left alignment.
    pub const ALIGN_LEFT: &str = "text.align-left";
    /// `"text.align-center"` — centered alignment.
    pub const ALIGN_CENTER: &str = "text.align-center";
    /// `"text.align-right"` — flush-right alignment.
    pub const ALIGN_RIGHT: &str = "text.align-right";
    /// `"text.align-justify"` — justified alignment.
    pub const ALIGN_JUSTIFY: &str = "text.align-justify";
    /// `"text.indent"` — increase indent.
    pub const INDENT: &str = "text.indent";
    /// `"text.outdent"` — decrease indent.
    pub const OUTDENT: &str = "text.outdent";
    /// `"text.list"` — bulleted list.
    pub const LIST: &str = "text.list";
    /// `"text.list-ordered"` — numbered list.
    pub const LIST_ORDERED: &str = "text.list-ordered";
    /// `"text.list-checks"` — checklist.
    pub const LIST_CHECKS: &str = "text.list-checks";
    /// `"text.list-x"` — dismissed list.
    pub const LIST_X: &str = "text.list-x";
    /// `"text.list-plus"` — add list item.
    pub const LIST_PLUS: &str = "text.list-plus";
    /// `"text.list-tree"` — nested list / outline.
    pub const LIST_TREE: &str = "text.list-tree";
    /// `"text.list-todo"` — to-do list.
    pub const LIST_TODO: &str = "text.list-todo";
    /// `"text.quote"` — block quote marks.
    pub const QUOTE: &str = "text.quote";
    /// `"text.heading"` — heading style.
    pub const HEADING: &str = "text.heading";
    /// `"text.pilcrow"` — paragraph mark.
    pub const PILCROW: &str = "text.pilcrow";
    /// `"text.subscript"` — lowered script.
    pub const SUBSCRIPT: &str = "text.subscript";
    /// `"text.superscript"` — raised script.
    pub const SUPERSCRIPT: &str = "text.superscript";
    /// `"text.remove-formatting"` — clear formatting.
    pub const REMOVE_FORMATTING: &str = "text.remove-formatting";
    /// `"text.link"` — hyperlink.
    pub const LINK: &str = "text.link";
    /// `"text.unlink"` — remove hyperlink.
    pub const UNLINK: &str = "text.unlink";
    /// `"text.book"` — closed book.
    pub const BOOK: &str = "text.book";
    /// `"text.book-open"` — open book / reading.
    pub const BOOK_OPEN: &str = "text.book-open";
    /// `"text.library"` — book shelf / collection.
    pub const LIBRARY: &str = "text.library";
    /// `"text.text-cursor"` — I-beam caret.
    pub const TEXT_CURSOR: &str = "text.text-cursor";
}

/// `"text.bold"` — double-lobed B.
pub const TEXT_BOLD: &str = "M6 12h9a4 4 0 010 8H7a1 1 0 01-1-1V5a1 1 0 011-1h7a4 4 0 010 8z";
/// `"text.italic"` — slanted stem between serifs.
pub const TEXT_ITALIC: &str = "M19 4h-9M14 20H5M15 4L9 20";
/// `"text.underline"` — U curve over a baseline.
pub const TEXT_UNDERLINE: &str = "M6 4v6a6 6 0 1012 0V4M4 21h16";
/// `"text.strikethrough"` — partial S cut by a midline.
pub const TEXT_STRIKETHROUGH: &str = "M16 4H9a3 3 0 00-2.83 4M14 12a4 4 0 010 8H6M4 12h16";
/// `"text.align-left"` — left rail plus uneven bars.
pub const TEXT_ALIGN_LEFT: &str = "M21 6H3M15 12H3M17 18H3M2 3v18";
/// `"text.align-center"` — center rail plus symmetric bars.
pub const TEXT_ALIGN_CENTER: &str = "M21 6H3M17 12H7M19 18H5M12 2v20";
/// `"text.align-right"` — right rail plus uneven bars.
pub const TEXT_ALIGN_RIGHT: &str = "M21 6H3M21 12H9M21 18H7M22 3v18";
/// `"text.align-justify"` — three full-width bars.
pub const TEXT_ALIGN_JUSTIFY: &str = "M3 6h18M3 12h18M3 18h18";
/// `"text.indent"` — rightward chevron before the lines.
pub const TEXT_INDENT: &str = "M21 12H11M21 18H11M21 6H11M3 8l4 4-4 4";
/// `"text.outdent"` — leftward chevron before the lines.
pub const TEXT_OUTDENT: &str = "M21 12H11M21 18H11M21 6H11M7 8l-4 4 4 4";
/// `"text.list"` — dot bullets plus lines.
pub const TEXT_LIST: &str = "M8 6h13M8 12h13M8 18h13M3 6h0.01M3 12h0.01M3 18h0.01";
/// `"text.list-ordered"` — digit bullets plus lines.
pub const TEXT_LIST_ORDERED: &str =
    "M10 6h11M10 12h11M10 18h11M4 6h1v4M4 10h2M4 14.5a1.5 1.5 0 013-.25c.6 1.25-1.5 2.25-3 3.75h3";
/// `"text.list-checks"` — tick bullets plus lines.
pub const TEXT_LIST_CHECKS: &str =
    "M11 6h10M11 12h10M11 18h10M3 7l1.5 1.5L7.5 5M3 17l1.5 1.5L7.5 15";
/// `"text.list-x"` — cross bullets plus lines.
pub const TEXT_LIST_X: &str =
    "M11 6h10M11 12h10M11 18h10M3.5 4.5l3 3M6.5 4.5l-3 3M3.5 16.5l3 3M6.5 16.5l-3 3";
/// `"text.list-plus"` — two rows plus an add cross row.
pub const TEXT_LIST_PLUS: &str = "M8 6h13M8 12h13M3 6h0.01M3 12h0.01M3 18h5M5.5 15.5v5M11 18h10";
/// `"text.list-tree"` — branching spine plus item lines.
pub const TEXT_LIST_TREE: &str =
    "M21 12h-8M21 6H8M21 18h-8M3 6v4c0 1.1.9 2 2 2h3M3 10v6c0 1.1.9 2 2 2h3";
/// `"text.list-todo"` — checkbox, tick, plus lines.
pub const TEXT_LIST_TODO: &str =
    "M13 6h8M13 12h8M13 18h8M4 5h4a1 1 0 011 1v4a1 1 0 01-1 1H4a1 1 0 01-1-1V6a1 1 0 011-1zM3.5 17.5l1.5 1.5L8.5 15.5";
/// `"text.quote"` — paired quotation marks.
pub const TEXT_QUOTE: &str =
    "M3 21c3 0 7-1 7-8V5a2 2 0 00-2-2H4a2 2 0 00-2 2v6a2 2 0 002 2h1a1 1 0 011 1v1a2 2 0 01-2 2 1 1 0 00-1 1.03V20a1 1 0 001 1zM15 21c3 0 7-1 7-8V5a2 2 0 00-2-2h-4a2 2 0 00-2 2v6a2 2 0 002 2h.75c0 2.25.25 4-2.75 4v3a1 1 0 001 1z";
/// `"text.heading"` — seriffed H.
pub const TEXT_HEADING: &str = "M6 12h12M6 20V4M18 20V4";
/// `"text.pilcrow"` — paragraph mark.
pub const TEXT_PILCROW: &str = "M13 4v16M17 4v16M19 4H9.5a4.5 4.5 0 000 9H13";
/// `"text.subscript"` — x with a lowered 2.
pub const TEXT_SUBSCRIPT: &str =
    "M4 5l8 8M12 5l-8 8M20 19h-4c0-1.5.5-2 1.5-2.5S20 15.3 20 14c0-.5-.2-.9-.5-1.3a2.1 2.1 0 00-1.5-.7c-.8 0-1.6.5-1.9 1.2";
/// `"text.superscript"` — x with a raised 2.
pub const TEXT_SUPERSCRIPT: &str =
    "M4 10l8 8M12 10l-8 8M20 9h-4c0-1.5.5-2 1.5-2.5S20 5.3 20 4c0-.5-.2-.9-.5-1.3a2.1 2.1 0 00-1.5-.7c-.8 0-1.6.5-1.9 1.2";
/// `"text.remove-formatting"` — italic T plus a clear cross.
pub const TEXT_REMOVE_FORMATTING: &str = "M4 7V4h16v3M5 20h6M13 4L8 20M15 15l5 5M20 15l-5 5";
/// `"text.link"` — two interlocked chain links.
pub const TEXT_LINK: &str =
    "M10 13a5 5 0 007.54.54l3-3a5 5 0 00-7.07-7.07l-1.72 1.71M14 11a5 5 0 00-7.54-.54l-3 3a5 5 0 007.07 7.07l1.71-1.71";
/// `"text.unlink"` — broken links with separation sparks.
pub const TEXT_UNLINK: &str =
    "M18.84 12.25l1.72-1.71a5 5 0 00-7.07-7.07l-1.72 1.71M5.17 11.75l-1.72 1.71a5 5 0 007.07 7.07l1.72-1.71M8 2L6 4M2 8l2-2M16 15l2 2M15 22l2-2";
/// `"text.book"` — spine plus page block.
pub const TEXT_BOOK: &str = "M4 19.5v-15A2.5 2.5 0 016.5 2H20v20H6.5a2.5 2.5 0 010-5H20";
/// `"text.book-open"` — spread pages.
pub const TEXT_BOOK_OPEN: &str =
    "M2 3h6a4 4 0 014 4v14a3 3 0 00-3-3H2zM22 3h-6a4 4 0 00-4 4v14a3 3 0 013-3h7z";
/// `"text.library"` — book spines, one tilted.
pub const TEXT_LIBRARY: &str = "M16 6l4 14M12 6v14M8 8v12M4 4v16";
/// `"text.text-cursor"` — I-beam caret.
pub const TEXT_TEXT_CURSOR: &str = "M12 4v16M8.5 4h7M8.5 20h7";

/// `text` entries.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::BOLD, TEXT_BOLD),
    IconEntry::new(names::ITALIC, TEXT_ITALIC),
    IconEntry::new(names::UNDERLINE, TEXT_UNDERLINE),
    IconEntry::new(names::STRIKETHROUGH, TEXT_STRIKETHROUGH),
    IconEntry::new(names::ALIGN_LEFT, TEXT_ALIGN_LEFT),
    IconEntry::new(names::ALIGN_CENTER, TEXT_ALIGN_CENTER),
    IconEntry::new(names::ALIGN_RIGHT, TEXT_ALIGN_RIGHT),
    IconEntry::new(names::ALIGN_JUSTIFY, TEXT_ALIGN_JUSTIFY),
    IconEntry::new(names::INDENT, TEXT_INDENT),
    IconEntry::new(names::OUTDENT, TEXT_OUTDENT),
    IconEntry::new(names::LIST, TEXT_LIST),
    IconEntry::new(names::LIST_ORDERED, TEXT_LIST_ORDERED),
    IconEntry::new(names::LIST_CHECKS, TEXT_LIST_CHECKS),
    IconEntry::new(names::LIST_X, TEXT_LIST_X),
    IconEntry::new(names::LIST_PLUS, TEXT_LIST_PLUS),
    IconEntry::new(names::LIST_TREE, TEXT_LIST_TREE),
    IconEntry::new(names::LIST_TODO, TEXT_LIST_TODO),
    IconEntry::new(names::QUOTE, TEXT_QUOTE),
    IconEntry::new(names::HEADING, TEXT_HEADING),
    IconEntry::new(names::PILCROW, TEXT_PILCROW),
    IconEntry::new(names::SUBSCRIPT, TEXT_SUBSCRIPT),
    IconEntry::new(names::SUPERSCRIPT, TEXT_SUPERSCRIPT),
    IconEntry::new(names::REMOVE_FORMATTING, TEXT_REMOVE_FORMATTING),
    IconEntry::new(names::LINK, TEXT_LINK),
    IconEntry::new(names::UNLINK, TEXT_UNLINK),
    IconEntry::new(names::BOOK, TEXT_BOOK),
    IconEntry::new(names::BOOK_OPEN, TEXT_BOOK_OPEN),
    IconEntry::new(names::LIBRARY, TEXT_LIBRARY),
    IconEntry::new(names::TEXT_CURSOR, TEXT_TEXT_CURSOR),
];

/// `text` morph pairs.
pub const PAIRS: &[IconPair] = &[
    IconPair::new(names::LINK, names::UNLINK),
    IconPair::new(names::BOOK_OPEN, names::BOOK),
];
