//! `file` namespace — files, folders, documents, clipboard, archives.

use super::super::{IconEntry, IconPair};

/// Qualified icon names (`file.*`).
///
/// Every name constant in this module is prefixed `FILE_`
/// (`FILE_FOO` → `"file.foo"`) so the flattened
/// `builtin::names` re-export cannot collide.
pub mod names {
    /// `"file.file"` — generic document.
    pub const FILE: &str = "file.file";
    /// `"file.file-text"` — document with content.
    pub const FILE_TEXT: &str = "file.file-text";
    /// `"file.file-plus"` — new document.
    pub const FILE_PLUS: &str = "file.file-plus";
    /// `"file.file-minus"` — remove document.
    pub const FILE_MINUS: &str = "file.file-minus";
    /// `"file.file-x"` — delete document.
    pub const FILE_X: &str = "file.file-x";
    /// `"file.file-check"` — verified document.
    pub const FILE_CHECK: &str = "file.file-check";
    /// `"file.file-search"` — find in document.
    pub const FILE_SEARCH: &str = "file.file-search";
    /// `"file.file-code"` — source file.
    pub const FILE_CODE: &str = "file.file-code";
    /// `"file.file-image"` — image file.
    pub const FILE_IMAGE: &str = "file.file-image";
    /// `"file.file-audio"` — audio file.
    pub const FILE_AUDIO: &str = "file.file-audio";
    /// `"file.file-video"` — video file.
    pub const FILE_VIDEO: &str = "file.file-video";
    /// `"file.file-warning"` — document alert.
    pub const FILE_WARNING: &str = "file.file-warning";
    /// `"file.folder"` — closed directory.
    pub const FOLDER: &str = "file.folder";
    /// `"file.folder-open"` — open directory.
    pub const FOLDER_OPEN: &str = "file.folder-open";
    /// `"file.folder-plus"` — new directory.
    pub const FOLDER_PLUS: &str = "file.folder-plus";
    /// `"file.folder-minus"` — remove directory.
    pub const FOLDER_MINUS: &str = "file.folder-minus";
    /// `"file.folder-x"` — delete directory.
    pub const FOLDER_X: &str = "file.folder-x";
    /// `"file.folder-check"` — verified directory.
    pub const FOLDER_CHECK: &str = "file.folder-check";
    /// `"file.folder-search"` — find in directory.
    pub const FOLDER_SEARCH: &str = "file.folder-search";
    /// `"file.folder-tree"` — directory hierarchy.
    pub const FOLDER_TREE: &str = "file.folder-tree";
    /// `"file.folder-archive"` — archived directory.
    pub const FOLDER_ARCHIVE: &str = "file.folder-archive";
    /// `"file.clipboard"` — clipboard / pasteboard.
    pub const CLIPBOARD: &str = "file.clipboard";
    /// `"file.clipboard-check"` — clipboard confirmed.
    pub const CLIPBOARD_CHECK: &str = "file.clipboard-check";
    /// `"file.clipboard-x"` — clipboard failed / clear.
    pub const CLIPBOARD_X: &str = "file.clipboard-x";
    /// `"file.clipboard-copy"` — copy to clipboard.
    pub const CLIPBOARD_COPY: &str = "file.clipboard-copy";
    /// `"file.clipboard-list"` — clipboard contents.
    pub const CLIPBOARD_LIST: &str = "file.clipboard-list";
    /// `"file.clipboard-paste"` — paste from clipboard.
    pub const CLIPBOARD_PASTE: &str = "file.clipboard-paste";
    /// `"file.copy"` — duplicate.
    pub const COPY: &str = "file.copy";
    /// `"file.copy-check"` — duplicate confirmed / copied.
    pub const COPY_CHECK: &str = "file.copy-check";
    /// `"file.copy-x"` — duplicate failed.
    pub const COPY_X: &str = "file.copy-x";
    /// `"file.archive"` — archive box.
    pub const ARCHIVE: &str = "file.archive";
    /// `"file.archive-restore"` — unarchive / restore.
    pub const ARCHIVE_RESTORE: &str = "file.archive-restore";
    /// `"file.archive-x"` — remove archive.
    pub const ARCHIVE_X: &str = "file.archive-x";
    /// `"file.package"` — sealed package / bundle.
    pub const PACKAGE: &str = "file.package";
    /// `"file.package-open"` — opened package.
    pub const PACKAGE_OPEN: &str = "file.package-open";
    /// `"file.package-check"` — package verified / delivered.
    pub const PACKAGE_CHECK: &str = "file.package-check";
    /// `"file.inbox"` — incoming tray.
    pub const INBOX: &str = "file.inbox";
    /// `"file.paperclip"` — attachment.
    pub const PAPERCLIP: &str = "file.paperclip";
}

/// `"file.file"` — the shared file silhouette: page with folded
/// top-right corner. Every `file-*` variant reuses this body
/// unchanged and places its modifier in the lower area.
pub const FILE_FILE: &str = "M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8zM14 2v6h6";
/// `"file.file-text"` — file body plus three content lines.
pub const FILE_FILE_TEXT: &str =
    "M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8zM14 2v6h6M16 13H8M16 17H8M10 9H8";
/// `"file.file-plus"` — file body plus an add cross.
pub const FILE_FILE_PLUS: &str =
    "M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8zM14 2v6h6M9 13h6M12 10v6";
/// `"file.file-minus"` — file body plus a remove bar.
pub const FILE_FILE_MINUS: &str =
    "M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8zM14 2v6h6M9 13h6";
/// `"file.file-x"` — file body plus a delete cross.
pub const FILE_FILE_X: &str =
    "M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8zM14 2v6h6M10 11l4 4M14 11l-4 4";
/// `"file.file-check"` — file body plus a verify tick.
pub const FILE_FILE_CHECK: &str =
    "M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8zM14 2v6h6M9 13l2.25 2.25L15.5 11";
/// `"file.file-search"` — file body plus a magnifier.
pub const FILE_FILE_SEARCH: &str =
    "M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8zM14 2v6h6M13.5 12a2.5 2.5 0 11-5 0 2.5 2.5 0 015 0zM13 14l3.5 3.5";
/// `"file.file-code"` — file body plus angle brackets.
pub const FILE_FILE_CODE: &str =
    "M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8zM14 2v6h6M10.5 11l-2.5 2.5 2.5 2.5M13.5 11l2.5 2.5-2.5 2.5";
/// `"file.file-image"` — file body plus a framed landscape.
pub const FILE_FILE_IMAGE: &str =
    "M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8zM14 2v6h6M8 10h8v7H8zM8 16l3.75-3.75 2 2 1.25-1.25L16 14.25M10 11.5h0.01";
/// `"file.file-audio"` — file body plus a beamed note.
pub const FILE_FILE_AUDIO: &str =
    "M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8zM14 2v6h6M9 17.5V11.5l5.5-1.5V15.5M9 17.5a1.5 1.5 0 11-3 0 1.5 1.5 0 013 0zM14.5 15.5a1.5 1.5 0 11-3 0 1.5 1.5 0 013 0z";
/// `"file.file-video"` — file body plus a play triangle.
pub const FILE_FILE_VIDEO: &str =
    "M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8zM14 2v6h6M10 10.5l5.5 3-5.5 3z";
/// `"file.file-warning"` — file body plus an alert mark.
pub const FILE_FILE_WARNING: &str =
    "M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8zM14 2v6h6M12 10v4M12 17h0.01";
/// `"file.folder"` — the shared folder silhouette: tabbed directory.
/// Every `folder-*` variant reuses this body unchanged.
pub const FILE_FOLDER: &str =
    "M20 20a2 2 0 002-2V8a2 2 0 00-2-2h-7.9a2 2 0 01-1.69-.9L9.6 3.9A2 2 0 007.93 3H4a2 2 0 00-2 2v13a2 2 0 002 2z";
/// `"file.folder-open"` — tilted-open folder with retained tab.
pub const FILE_FOLDER_OPEN: &str =
    "M6 14l1.45-2.85A2 2 0 019.21 10H20a2 2 0 011.94 2.5l-1.55 6a2 2 0 01-1.94 1.5H4a2 2 0 01-2-2V5a2 2 0 012-2h3.9a2 2 0 011.69.9l.81 1.2a2 2 0 001.67.9H18a2 2 0 012 2v2";
/// `"file.folder-plus"` — folder body plus an add cross.
pub const FILE_FOLDER_PLUS: &str =
    "M20 20a2 2 0 002-2V8a2 2 0 00-2-2h-7.9a2 2 0 01-1.69-.9L9.6 3.9A2 2 0 007.93 3H4a2 2 0 00-2 2v13a2 2 0 002 2zM9.5 14.5h5M12 12v5";
/// `"file.folder-minus"` — folder body plus a remove bar.
pub const FILE_FOLDER_MINUS: &str =
    "M20 20a2 2 0 002-2V8a2 2 0 00-2-2h-7.9a2 2 0 01-1.69-.9L9.6 3.9A2 2 0 007.93 3H4a2 2 0 00-2 2v13a2 2 0 002 2zM9.5 14.5h5";
/// `"file.folder-x"` — folder body plus a delete cross.
pub const FILE_FOLDER_X: &str =
    "M20 20a2 2 0 002-2V8a2 2 0 00-2-2h-7.9a2 2 0 01-1.69-.9L9.6 3.9A2 2 0 007.93 3H4a2 2 0 00-2 2v13a2 2 0 002 2zM10.5 13l3 3M13.5 13l-3 3";
/// `"file.folder-check"` — folder body plus a verify tick.
pub const FILE_FOLDER_CHECK: &str =
    "M20 20a2 2 0 002-2V8a2 2 0 00-2-2h-7.9a2 2 0 01-1.69-.9L9.6 3.9A2 2 0 007.93 3H4a2 2 0 00-2 2v13a2 2 0 002 2zM10 14.5l1.5 1.5 3-3";
/// `"file.folder-search"` — folder body plus a magnifier.
pub const FILE_FOLDER_SEARCH: &str =
    "M20 20a2 2 0 002-2V8a2 2 0 00-2-2h-7.9a2 2 0 01-1.69-.9L9.6 3.9A2 2 0 007.93 3H4a2 2 0 00-2 2v13a2 2 0 002 2zM13.5 13a2 2 0 11-4 0 2 2 0 014 0zM12.75 14.75l2.75 2.75";
/// `"file.folder-tree"` — folder body plus a hierarchy trace.
pub const FILE_FOLDER_TREE: &str =
    "M20 20a2 2 0 002-2V8a2 2 0 00-2-2h-7.9a2 2 0 01-1.69-.9L9.6 3.9A2 2 0 007.93 3H4a2 2 0 00-2 2v13a2 2 0 002 2zM8.5 10v8M8.5 12.5h4M8.5 16.5h4M12.5 11h4v3h-4zM12.5 15h4v3h-4z";
/// `"file.folder-archive"` — folder body plus a store arrow.
pub const FILE_FOLDER_ARCHIVE: &str =
    "M20 20a2 2 0 002-2V8a2 2 0 00-2-2h-7.9a2 2 0 01-1.69-.9L9.6 3.9A2 2 0 007.93 3H4a2 2 0 00-2 2v13a2 2 0 002 2zM12 10.5v5M9.75 13.25l2.25 2.25 2.25-2.25M9.5 18.5h5";
/// `"file.clipboard"` — clipboard body plus clip tab.
/// Every `clipboard-*` variant reuses this body unchanged.
pub const FILE_CLIPBOARD: &str =
    "M16 4h2a2 2 0 012 2v14a2 2 0 01-2 2H6a2 2 0 01-2-2V6a2 2 0 012-2h2M10 2h4a2 2 0 012 2 2 2 0 01-2 2h-4a2 2 0 01-2-2 2 2 0 012-2z";
/// `"file.clipboard-check"` — clipboard plus a verify tick.
pub const FILE_CLIPBOARD_CHECK: &str =
    "M16 4h2a2 2 0 012 2v14a2 2 0 01-2 2H6a2 2 0 01-2-2V6a2 2 0 012-2h2M10 2h4a2 2 0 012 2 2 2 0 01-2 2h-4a2 2 0 01-2-2 2 2 0 012-2zM9 14.5l2 2 4.5-4.5";
/// `"file.clipboard-x"` — clipboard plus a failure cross.
pub const FILE_CLIPBOARD_X: &str =
    "M16 4h2a2 2 0 012 2v14a2 2 0 01-2 2H6a2 2 0 01-2-2V6a2 2 0 012-2h2M10 2h4a2 2 0 012 2 2 2 0 01-2 2h-4a2 2 0 01-2-2 2 2 0 012-2zM9.5 12.5l5 5M14.5 12.5l-5 5";
/// `"file.clipboard-copy"` — clipboard plus stacked sheets.
pub const FILE_CLIPBOARD_COPY: &str =
    "M16 4h2a2 2 0 012 2v14a2 2 0 01-2 2H6a2 2 0 01-2-2V6a2 2 0 012-2h2M10 2h4a2 2 0 012 2 2 2 0 01-2 2h-4a2 2 0 01-2-2 2 2 0 012-2zM9 12.5h5v5H9zM11.5 10.5H16.5V15.5";
/// `"file.clipboard-list"` — clipboard plus content lines.
pub const FILE_CLIPBOARD_LIST: &str =
    "M16 4h2a2 2 0 012 2v14a2 2 0 01-2 2H6a2 2 0 01-2-2V6a2 2 0 012-2h2M10 2h4a2 2 0 012 2 2 2 0 01-2 2h-4a2 2 0 01-2-2 2 2 0 012-2zM9 11h6M9 14.5h6M9 18h4";
/// `"file.clipboard-paste"` — clipboard plus an incoming arrow.
pub const FILE_CLIPBOARD_PASTE: &str =
    "M16 4h2a2 2 0 012 2v14a2 2 0 01-2 2H6a2 2 0 01-2-2V6a2 2 0 012-2h2M10 2h4a2 2 0 012 2 2 2 0 01-2 2h-4a2 2 0 01-2-2 2 2 0 012-2zM12 9.5V14M9.5 11.75L12 14.25l2.5-2.5M8.5 18.5h7";
/// `"file.copy"` — two overlapping sheets.
/// Every `copy-*` variant reuses this body unchanged.
pub const FILE_COPY: &str =
    "M11 9h9a2 2 0 012 2v9a2 2 0 01-2 2h-9a2 2 0 01-2-2v-9a2 2 0 012-2zM5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1";
/// `"file.copy-check"` — copy body plus a verify tick.
pub const FILE_COPY_CHECK: &str =
    "M11 9h9a2 2 0 012 2v9a2 2 0 01-2 2h-9a2 2 0 01-2-2v-9a2 2 0 012-2zM5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1M12.5 15l1.75 1.75 3.5-3.5";
/// `"file.copy-x"` — copy body plus a failure cross.
pub const FILE_COPY_X: &str =
    "M11 9h9a2 2 0 012 2v9a2 2 0 01-2 2h-9a2 2 0 01-2-2v-9a2 2 0 012-2zM5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1M13.5 13.5l4 4M17.5 13.5l-4 4";
/// `"file.archive"` — lidded archive box plus handle slot.
/// Every `archive-*` variant reuses this body unchanged.
pub const FILE_ARCHIVE: &str =
    "M4 3h16a2 2 0 012 2v1a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2zM4 8v11a2 2 0 002 2h12a2 2 0 002-2V8M10 13h4";
/// `"file.archive-restore"` — archive body plus a lift-out arrow.
pub const FILE_ARCHIVE_RESTORE: &str =
    "M4 3h16a2 2 0 012 2v1a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2zM4 8v11a2 2 0 002 2h12a2 2 0 002-2V8M12 17.5V12M9.5 14.5l2.5-2.5 2.5 2.5";
/// `"file.archive-x"` — archive body plus a delete cross.
pub const FILE_ARCHIVE_X: &str =
    "M4 3h16a2 2 0 012 2v1a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2zM4 8v11a2 2 0 002 2h12a2 2 0 002-2V8M9.75 12.75l4.5 4.5M14.25 12.75l-4.5 4.5";
/// `"file.package"` — sealed shipping box: shell, flap ridge,
/// center drop, and tape tag.
pub const FILE_PACKAGE: &str =
    "M7.5 4.27l9 5.15M21 8a2 2 0 00-1-1.73l-7-4a2 2 0 00-2 0l-7 4A2 2 0 003 8v8a2 2 0 001 1.73l7 4a2 2 0 002 0l7-4A2 2 0 0021 16V8zM3.3 7l8.7 5 8.7-5M12 22V12";
/// `"file.package-open"` — open box: rim rhombus over the body.
pub const FILE_PACKAGE_OPEN: &str =
    "M5 9l7-3.5 7 3.5-7 3.5zM5 9v8a2 2 0 001 1.73l5 2.77a2 2 0 002 0l5-2.77A2 2 0 0019 17V9M12 12.5V21.5";
/// `"file.package-check"` — package plus a verify tick on the face.
pub const FILE_PACKAGE_CHECK: &str =
    "M7.5 4.27l9 5.15M21 8a2 2 0 00-1-1.73l-7-4a2 2 0 00-2 0l-7 4A2 2 0 003 8v8a2 2 0 001 1.73l7 4a2 2 0 002 0l7-4A2 2 0 0021 16V8zM3.3 7l8.7 5 8.7-5M12 22V12M6 15.5l1.25 1.25 2.25-2.25";
/// `"file.inbox"` — inbox tray with a scoop notch.
pub const FILE_INBOX: &str =
    "M22 12h-6l-2 3h-4l-2-3H2M5.45 5.11L2 12v6a2 2 0 002 2h16a2 2 0 002-2v-6l-3.45-6.89A2 2 0 0016.76 4H7.24a2 2 0 00-1.79 1.11z";
/// `"file.paperclip"` — diagonal attachment clip.
pub const FILE_PAPERCLIP: &str =
    "M21.44 11.05l-9.19 9.19a6 6 0 01-8.49-8.49l8.57-8.57A4 4 0 1118 8.84l-8.59 8.57a2 2 0 01-2.83-2.83l8.49-8.48";

/// `file` entries — registered in the pack in this order.
pub const ENTRIES: &[IconEntry] = &[
    IconEntry::new(names::FILE, FILE_FILE),
    IconEntry::new(names::FILE_TEXT, FILE_FILE_TEXT),
    IconEntry::new(names::FILE_PLUS, FILE_FILE_PLUS),
    IconEntry::new(names::FILE_MINUS, FILE_FILE_MINUS),
    IconEntry::new(names::FILE_X, FILE_FILE_X),
    IconEntry::new(names::FILE_CHECK, FILE_FILE_CHECK),
    IconEntry::new(names::FILE_SEARCH, FILE_FILE_SEARCH),
    IconEntry::new(names::FILE_CODE, FILE_FILE_CODE),
    IconEntry::new(names::FILE_IMAGE, FILE_FILE_IMAGE),
    IconEntry::new(names::FILE_AUDIO, FILE_FILE_AUDIO),
    IconEntry::new(names::FILE_VIDEO, FILE_FILE_VIDEO),
    IconEntry::new(names::FILE_WARNING, FILE_FILE_WARNING),
    IconEntry::new(names::FOLDER, FILE_FOLDER),
    IconEntry::new(names::FOLDER_OPEN, FILE_FOLDER_OPEN),
    IconEntry::new(names::FOLDER_PLUS, FILE_FOLDER_PLUS),
    IconEntry::new(names::FOLDER_MINUS, FILE_FOLDER_MINUS),
    IconEntry::new(names::FOLDER_X, FILE_FOLDER_X),
    IconEntry::new(names::FOLDER_CHECK, FILE_FOLDER_CHECK),
    IconEntry::new(names::FOLDER_SEARCH, FILE_FOLDER_SEARCH),
    IconEntry::new(names::FOLDER_TREE, FILE_FOLDER_TREE),
    IconEntry::new(names::FOLDER_ARCHIVE, FILE_FOLDER_ARCHIVE),
    IconEntry::new(names::CLIPBOARD, FILE_CLIPBOARD),
    IconEntry::new(names::CLIPBOARD_CHECK, FILE_CLIPBOARD_CHECK),
    IconEntry::new(names::CLIPBOARD_X, FILE_CLIPBOARD_X),
    IconEntry::new(names::CLIPBOARD_COPY, FILE_CLIPBOARD_COPY),
    IconEntry::new(names::CLIPBOARD_LIST, FILE_CLIPBOARD_LIST),
    IconEntry::new(names::CLIPBOARD_PASTE, FILE_CLIPBOARD_PASTE),
    IconEntry::new(names::COPY, FILE_COPY),
    IconEntry::new(names::COPY_CHECK, FILE_COPY_CHECK),
    IconEntry::new(names::COPY_X, FILE_COPY_X),
    IconEntry::new(names::ARCHIVE, FILE_ARCHIVE),
    IconEntry::new(names::ARCHIVE_RESTORE, FILE_ARCHIVE_RESTORE),
    IconEntry::new(names::ARCHIVE_X, FILE_ARCHIVE_X),
    IconEntry::new(names::PACKAGE, FILE_PACKAGE),
    IconEntry::new(names::PACKAGE_OPEN, FILE_PACKAGE_OPEN),
    IconEntry::new(names::PACKAGE_CHECK, FILE_PACKAGE_CHECK),
    IconEntry::new(names::INBOX, FILE_INBOX),
    IconEntry::new(names::PAPERCLIP, FILE_PAPERCLIP),
];

/// `file` morph pairs.
pub const PAIRS: &[IconPair] = &[
    IconPair::new(names::FOLDER, names::FOLDER_OPEN),
    IconPair::new(names::PACKAGE, names::PACKAGE_OPEN),
    IconPair::new(names::COPY, names::COPY_CHECK),
    IconPair::new(names::FILE_PLUS, names::FILE_MINUS),
    IconPair::new(names::FOLDER_PLUS, names::FOLDER_MINUS),
];
