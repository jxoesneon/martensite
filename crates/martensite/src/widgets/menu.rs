//! `Menu` widget: the item model and popup surface for menus.
//!
//! A menu is a vertical list of [`MenuItem`]s — actions, checkable and
//! radio items, separators, section headings, and nested submenus —
//! rendered in the
//! [`OverlayLayer`](martensite_core::overlay::OverlayLayer) as a
//! `Role::Menu` popup. The framework owners [`MenuBar`](crate::widgets::menu_bar::MenuBar)
//! and [`ContextMenu`](crate::widgets::context_menu::ContextMenu) drive
//! the same [`Menu`] surface: they reconcile overlay entries through a
//! shared [`MenuState`] (`Arc<Mutex<_>>`, the same seam `Dropdown`
//! uses), so hover, clicks, and AT activations inside a popup reach
//! the owner without the adapter preparing popup widgets.
//!
//! - **Row layout**: check/choice gutter | label | `▸` submenu glyph |
//!   right-aligned muted shortcut.
//! - **Keyboard**: `Up`/`Down`/`Home`/`End` move the highlight
//!   (separators, headings, and disabled actions are skipped), `Right`
//!   opens a submenu, `Left` backs out of one, `Enter`/`Space`
//!   activates, printable characters cycle typeahead by first letter,
//!   and `Escape` dismisses the topmost popup via the overlay layer.
//! - **Submenus**: each submenu is its own [`Menu`] overlay entry
//!   anchored to the parent row's bounds. Only one open chain exists
//!   at a time — moving the pointer to another parent row closes the
//!   deeper menus through the shared `open_path`.
//! - **Activation**: an activation writes a [`MenuPath`] (the index
//!   path from the root menu into the item tree) into the shared
//!   state; the owner drains it via `take_activated()` and the whole
//!   menu stack closes.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::{Menu, MenuItem};
//!
//! let menu = Menu::new([
//!     MenuItem::heading("File"),
//!     MenuItem::action("Open").with_shortcut("Ctrl+O"),
//!     MenuItem::separator(),
//!     MenuItem::checkable("Word wrap", true),
//!     MenuItem::submenu("Recent", vec![MenuItem::action("a.txt")]),
//! ]);
//! assert_eq!(menu.item_count(), 5);
//! ```

use std::sync::{Arc, Mutex};

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::overlay::{OverlayAnchor, OverlayLayer};
use martensite_core::shape::Shape;
use martensite_core::widget::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    SemanticAction, Widget, WidgetEvent,
};
use martensite_core::{Rect, TokenKey};

use crate::widgets::scrollview::ScrollView;

/// Action/checkable row height in logical pixels.
const ITEM_H: f32 = 26.0;
/// Separator row height.
const SEPARATOR_H: f32 = 9.0;
/// Section-heading row height.
const HEADING_H: f32 = 22.0;
/// Vertical padding inside the popup chrome.
const POPUP_PAD_Y: f32 = 4.0;
/// Maximum popup height before the list scrolls.
const MAX_POPUP_H: f32 = 340.0;
/// Minimum popup width.
const MIN_POPUP_W: f32 = 150.0;
/// Check/radio glyph gutter width.
const GUTTER_W: f32 = 24.0;
/// Reserved strip on the right for the `▸` submenu glyph.
const SUBMENU_W: f32 = 20.0;
/// Gap between the label and a right-aligned shortcut.
const SHORTCUT_GAP: f32 = 16.0;
/// Item label font size.
const FONT_PT: f32 = 13.0;
/// Shortcut/heading font size.
const MUTED_PT: f32 = 11.0;
/// Inset of each row inside the popup edge.
const ROW_INSET: f32 = 4.0;
/// The overlay layer's pointer-anchor offset — a `Pointer` anchor is
/// placed this many logical points below-right of the point, so a
/// submenu anchor is synthesized by backing the offset out of the
/// desired popup corner (the parent's row top-right).
const POINTER_OFFSET_PT: f32 = 12.0;

/// Popup background.
const POPUP_BG: [u8; 4] = [252, 252, 254, 255];
/// Popup border.
const POPUP_BORDER: [u8; 4] = [140, 145, 155, 255];
/// Item label ink.
const INK: [u8; 4] = [30, 30, 36, 255];
/// Muted ink (shortcuts, headings).
const INK_MUTED: [u8; 4] = [120, 122, 132, 255];
/// Disabled ink.
const INK_DISABLED: [u8; 4] = [160, 160, 168, 255];
/// Highlighted row background.
const HIGHLIGHT_BG: [u8; 4] = [60, 110, 220, 255];
/// Highlighted row ink.
const HIGHLIGHT_INK: [u8; 4] = [255, 255, 255, 255];
/// Check/radio glyph colour on an unhighlighted row.
const CHECK: [u8; 4] = [60, 110, 220, 255];
/// Separator hairline.
const SEPARATOR_INK: [u8; 4] = [200, 202, 210, 255];

/// One entry in a [`Menu`]'s item tree.
///
/// Items form a tree through [`MenuItem::Submenu`]; everything else is
/// a leaf row. `Separator` and `Heading` are non-interactive and are
/// skipped by keyboard navigation.
///
/// # Examples
///
/// ```
/// use martensite::widgets::MenuItem;
///
/// let item = MenuItem::action("Save").with_shortcut("Ctrl+S");
/// assert_eq!(item.label(), "Save");
/// assert!(item.is_selectable());
/// assert!(!MenuItem::separator().is_selectable());
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum MenuItem {
    /// A plain clickable action.
    Action {
        /// Row label.
        label: String,
        /// Right-aligned shortcut hint text (display only).
        shortcut: Option<String>,
        /// Whether the item can be activated. Disabled actions render
        /// muted and are skipped by keyboard navigation.
        enabled: bool,
    },
    /// A checkable item toggling independently (menu checkbox).
    Checkable {
        /// Row label.
        label: String,
        /// Whether the item is currently checked.
        checked: bool,
    },
    /// A radio item — one checked item per `group` within the same
    /// menu level.
    Radio {
        /// Row label.
        label: String,
        /// Whether the item is currently selected.
        checked: bool,
        /// Radio group key: activating this item unchecks siblings in
        /// the same group at the same menu level.
        group: String,
    },
    /// A non-interactive hairline between sections.
    Separator,
    /// An item that opens a nested [`Menu`].
    Submenu {
        /// Row label.
        label: String,
        /// The submenu's items.
        items: Vec<MenuItem>,
    },
    /// A non-interactive section label (Qt menu section headers).
    Heading {
        /// Row label.
        label: String,
    },
}

impl MenuItem {
    /// Creates a plain action item.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::MenuItem;
    ///
    /// let item = MenuItem::action("Copy");
    /// assert!(item.is_selectable());
    /// ```
    pub fn action(label: impl Into<String>) -> Self {
        Self::Action {
            label: label.into(),
            shortcut: None,
            enabled: true,
        }
    }

    /// Creates a checkable item.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::MenuItem;
    ///
    /// let item = MenuItem::checkable("Status bar", false);
    /// assert!(item.is_selectable());
    /// ```
    pub fn checkable(label: impl Into<String>, checked: bool) -> Self {
        Self::Checkable {
            label: label.into(),
            checked,
        }
    }

    /// Creates a radio item in `group`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::MenuItem;
    ///
    /// let item = MenuItem::radio("Left", "align", true);
    /// assert!(item.is_selectable());
    /// ```
    pub fn radio(label: impl Into<String>, group: impl Into<String>, checked: bool) -> Self {
        Self::Radio {
            label: label.into(),
            checked,
            group: group.into(),
        }
    }

    /// Creates a separator row.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::MenuItem;
    ///
    /// assert!(!MenuItem::separator().is_selectable());
    /// ```
    pub fn separator() -> Self {
        Self::Separator
    }

    /// Creates a submenu item.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::MenuItem;
    ///
    /// let item = MenuItem::submenu("Export", vec![MenuItem::action("PNG")]);
    /// assert!(item.is_submenu());
    /// ```
    pub fn submenu(label: impl Into<String>, items: Vec<MenuItem>) -> Self {
        Self::Submenu {
            label: label.into(),
            items,
        }
    }

    /// Creates a non-interactive section heading.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::MenuItem;
    ///
    /// assert!(!MenuItem::heading("Edit").is_selectable());
    /// ```
    pub fn heading(label: impl Into<String>) -> Self {
        Self::Heading {
            label: label.into(),
        }
    }

    /// Sets the shortcut hint on an [`MenuItem::Action`]; a no-op on
    /// other variants.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::MenuItem;
    ///
    /// let item = MenuItem::action("Paste").with_shortcut("Ctrl+V");
    /// assert_eq!(item.shortcut(), Some("Ctrl+V"));
    /// ```
    #[inline]
    #[must_use]
    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        if let Self::Action { shortcut: s, .. } = &mut self {
            *s = Some(shortcut.into());
        }
        self
    }

    /// Sets `enabled` on an [`MenuItem::Action`]; a no-op on other
    /// variants.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::MenuItem;
    ///
    /// let item = MenuItem::action("Undo").enabled(false);
    /// assert!(!item.is_selectable());
    /// ```
    #[inline]
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        if let Self::Action { enabled: e, .. } = &mut self {
            *e = enabled;
        }
        self
    }

    /// The row's display label (`""` for separators).
    pub fn label(&self) -> &str {
        match self {
            Self::Action { label, .. }
            | Self::Checkable { label, .. }
            | Self::Radio { label, .. }
            | Self::Submenu { label, .. }
            | Self::Heading { label } => label,
            Self::Separator => "",
        }
    }

    /// The action's shortcut hint, if any.
    pub fn shortcut(&self) -> Option<&str> {
        match self {
            Self::Action { shortcut, .. } => shortcut.as_deref(),
            _ => None,
        }
    }

    /// Whether the row participates in highlight/activation —
    /// actions (when enabled), checkables, radios, and submenus.
    /// Separators, headings, and disabled actions are skipped by
    /// keyboard navigation and hover.
    pub fn is_selectable(&self) -> bool {
        match self {
            Self::Action { enabled, .. } => *enabled,
            Self::Checkable { .. } | Self::Radio { .. } | Self::Submenu { .. } => true,
            Self::Separator | Self::Heading { .. } => false,
        }
    }

    /// Whether the row opens a nested menu.
    pub fn is_submenu(&self) -> bool {
        matches!(self, Self::Submenu { .. })
    }
}

/// The index path of an activated item, from the root menu into the
/// [`MenuItem`] tree — `[2]` is the root's third row; `[2, 0]` is the
/// first row of the submenu opened at root row 2.
///
/// # Examples
///
/// ```
/// use martensite::widgets::MenuPath;
///
/// let path: MenuPath = vec![1, 0];
/// assert_eq!(path.len(), 2);
/// ```
pub type MenuPath = Vec<usize>;

/// State shared between a menu's owner ([`MenuBar`](crate::widgets::menu_bar::MenuBar),
/// [`ContextMenu`](crate::widgets::context_menu::ContextMenu), or a
/// custom widget) and the [`Menu`] popups it drives.
///
/// Popups are stateless views: rows read `items`/`highlight`/
/// `open_path` to paint and emit accessibility, and write back hover,
/// submenu opens, and `activated` for the owner to drain each
/// `sync_overlay`. `open_path[d]` is the row in the depth-`d` menu
/// whose submenu is open at depth `d + 1`; the owner keeps exactly
/// `1 + open_path.len()` overlay entries alive to match.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{MenuItem, MenuState};
///
/// let mut state = MenuState::new(vec![MenuItem::action("A")]);
/// assert_eq!(state.items().len(), 1);
/// assert_eq!(state.take_activated(), None);
/// ```
#[derive(Debug)]
pub struct MenuState {
    /// The root menu's item tree.
    items: Vec<MenuItem>,
    /// Highlighted row per open menu depth (`highlight[0]` is the
    /// root menu); `None` means no row is highlighted.
    highlight: Vec<Option<usize>>,
    /// Open submenu chain — `open_path[d]` is the row in the depth-`d`
    /// menu whose `Submenu` is open at depth `d + 1`.
    open_path: Vec<usize>,
    /// Written by a popup row on activation (click, Enter, AT Click);
    /// the owner drains it and closes the stack.
    activated: Option<MenuPath>,
    /// Per-depth window-space submenu anchors recorded by each open
    /// `Menu` during layout — `row_anchors[d][i]` places the submenu
    /// of row `i` in the depth-`d` menu.
    row_anchors: Vec<Vec<OverlayAnchor>>,
    /// Typeahead buffer for the deepest menu level.
    typeahead: String,
    /// The depth `typeahead` was accumulated against — a move to a
    /// different menu level resets the buffer.
    typeahead_depth: usize,
}

impl MenuState {
    /// Creates shared state over `items`.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{MenuItem, MenuState};
    ///
    /// let state = MenuState::new(vec![MenuItem::action("A"), MenuItem::separator()]);
    /// assert_eq!(state.items().len(), 2);
    /// ```
    pub fn new(items: Vec<MenuItem>) -> Self {
        Self {
            items,
            highlight: vec![None],
            open_path: Vec::new(),
            activated: None,
            row_anchors: Vec::new(),
            typeahead: String::new(),
            typeahead_depth: 0,
        }
    }

    /// The root menu's items.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{MenuItem, MenuState};
    ///
    /// let state = MenuState::new(vec![MenuItem::heading("H")]);
    /// assert_eq!(state.items()[0].label(), "H");
    /// ```
    pub fn items(&self) -> &[MenuItem] {
        &self.items
    }

    /// Replaces the root item tree and resets all navigation state —
    /// how a `MenuBar` switches the open popup to another menu.
    pub fn set_items(&mut self, items: Vec<MenuItem>) {
        self.items = items;
        self.open_path.clear();
        self.highlight = vec![None];
        self.row_anchors.clear();
        self.typeahead.clear();
        self.typeahead_depth = 0;
    }

    /// Resets navigation state for a fresh open (no highlight yet,
    /// no stale activation).
    fn reset_navigation(&mut self) {
        self.open_path.clear();
        self.highlight = vec![None];
        self.row_anchors.clear();
        self.typeahead.clear();
        self.typeahead_depth = 0;
        self.activated = None;
    }

    /// The items of the menu at `depth`, resolved through
    /// `open_path` — `None` when the path no longer resolves (the
    /// owner is about to close that entry).
    fn items_at(&self, depth: usize) -> Option<&[MenuItem]> {
        if depth == 0 {
            return Some(&self.items);
        }
        if depth > self.open_path.len() {
            return None;
        }
        let mut items = self.items.as_slice();
        for &row in &self.open_path[..depth] {
            match items.get(row) {
                Some(MenuItem::Submenu { items: sub, .. }) => items = sub,
                _ => return None,
            }
        }
        Some(items)
    }

    /// Indices of the selectable rows at `depth`.
    fn selectable(&self, depth: usize) -> Vec<usize> {
        self.items_at(depth)
            .map(|items| {
                items
                    .iter()
                    .enumerate()
                    .filter_map(|(i, item)| item.is_selectable().then_some(i))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The highlighted row at `depth`, if any.
    pub fn highlighted(&self, depth: usize) -> Option<usize> {
        self.highlight.get(depth).copied().flatten()
    }

    /// Sets the highlight at `depth`, growing the per-depth table.
    fn set_highlight(&mut self, depth: usize, row: Option<usize>) {
        if self.highlight.len() <= depth {
            self.highlight.resize(depth + 1, None);
        }
        self.highlight[depth] = row;
    }

    /// The open submenu chain (see the type documentation).
    pub fn open_path(&self) -> &[usize] {
        &self.open_path
    }

    /// The deepest open menu depth — `0` while only the root shows.
    fn deepest(&self) -> usize {
        self.open_path.len()
    }

    /// Opens the submenu at `row` in the depth-`d` menu (closing any
    /// deeper chain). `via_keyboard` seeds the new level's highlight
    /// at the first selectable row, matching the menu-bar keyboard
    /// convention; pointer opens start unhighlighted.
    fn open_submenu(&mut self, depth: usize, row: usize, via_keyboard: bool) {
        self.open_path.truncate(depth);
        self.open_path.push(row);
        let first = if via_keyboard {
            self.selectable(depth + 1).first().copied()
        } else {
            None
        };
        self.set_highlight(depth + 1, first);
    }

    /// Closes every menu deeper than `depth` (pointer hovered a
    /// different parent row).
    fn close_deeper(&mut self, depth: usize) {
        self.open_path.truncate(depth);
        self.highlight.truncate(depth + 1);
    }

    /// Backs out of the depth-`d` menu (`ArrowLeft`): closes it and
    /// lands the parent highlight on the row that spawned it.
    fn back_out(&mut self, depth: usize) {
        if depth == 0 || depth > self.open_path.len() {
            return;
        }
        let parent_row = self.open_path[depth - 1];
        self.open_path.truncate(depth - 1);
        self.highlight.truncate(depth);
        self.set_highlight(depth - 1, Some(parent_row));
    }

    /// Moves the highlight at `depth` among selectable rows, clamped
    /// (APG keeps the highlight inside the menu). From no highlight,
    /// `delta >= 0` lands on the first row, negative on the last.
    fn move_highlight(&mut self, depth: usize, delta: i64) {
        let sel = self.selectable(depth);
        if sel.is_empty() {
            return;
        }
        let next = match self.highlighted(depth) {
            None => {
                if delta >= 0 {
                    sel[0]
                } else {
                    sel[sel.len() - 1]
                }
            }
            Some(cur) => {
                let pos = sel.iter().position(|&i| i == cur).unwrap_or(0) as i64;
                sel[(pos + delta).clamp(0, sel.len() as i64 - 1) as usize]
            }
        };
        self.set_highlight(depth, Some(next));
    }

    /// Jumps the highlight to the first (`Home`) or last (`End`)
    /// selectable row at `depth`.
    fn highlight_edge(&mut self, depth: usize, first: bool) {
        let sel = self.selectable(depth);
        if sel.is_empty() {
            return;
        }
        let row = if first {
            sel[0]
        } else {
            sel[sel.len() - 1]
        };
        self.set_highlight(depth, Some(row));
    }

    /// First-letter typeahead at `depth`: cycles the highlight through
    /// selectable rows whose label starts with the typed prefix
    /// (falling back to the single last character so a repeated letter
    /// walks every match — APG typeahead semantics).
    fn typeahead_select(&mut self, depth: usize, c: char) -> bool {
        if self.typeahead_depth != depth {
            self.typeahead.clear();
            self.typeahead_depth = depth;
        }
        self.typeahead.push(c.to_ascii_lowercase());
        let sel = self.selectable(depth);
        if sel.is_empty() {
            return false;
        }
        let buffer = self.typeahead.clone();
        let current = self.highlighted(depth);
        for (probe, skip) in [
            (buffer.as_str(), 0usize),
            (&buffer[buffer.len() - 1..], 1usize),
        ] {
            let n = sel.len();
            for offset in skip..skip + n {
                let pos = match current {
                    Some(cur) => (sel.iter().position(|&i| i == cur).unwrap_or(0) + offset) % n,
                    None => offset % n,
                };
                let row = sel[pos];
                let matches = self
                    .items_at(depth)
                    .and_then(|items| items.get(row))
                    .is_some_and(|item| item.label().to_ascii_lowercase().starts_with(probe));
                if matches {
                    self.set_highlight(depth, Some(row));
                    return true;
                }
            }
        }
        false
    }

    /// Records an activation: toggles checkable/radio state in the
    /// model and writes the full [`MenuPath`] for the owner to drain.
    fn activate(&mut self, depth: usize, row: usize) {
        let selectable = self
            .items_at(depth)
            .and_then(|items| items.get(row))
            .is_some_and(MenuItem::is_selectable);
        if !selectable {
            return;
        }
        let mut path = self.open_path[..depth.min(self.open_path.len())].to_vec();
        path.push(row);
        self.apply_activation(&path);
        self.activated = Some(path);
    }

    /// Applies the model side of an activation: flips a `Checkable`,
    /// or checks a `Radio` while unchecking its group siblings.
    fn apply_activation(&mut self, path: &[usize]) {
        let Some((&last, ancestors)) = path.split_last() else {
            return;
        };
        let Some(parent) = parent_items_mut(&mut self.items, ancestors) else {
            return;
        };
        match parent.get_mut(last) {
            Some(MenuItem::Checkable { checked, .. }) => *checked = !*checked,
            Some(MenuItem::Radio { group, .. }) => {
                let group = group.clone();
                for item in parent.iter_mut() {
                    if let MenuItem::Radio {
                        group: g, checked, ..
                    } = item
                    {
                        if *g == group {
                            *checked = false;
                        }
                    }
                }
                if let Some(MenuItem::Radio { checked, .. }) = parent.get_mut(last) {
                    *checked = true;
                }
            }
            _ => {}
        }
    }

    /// Drains the pending activation path (one-shot).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{MenuItem, MenuState};
    ///
    /// let mut state = MenuState::new(vec![MenuItem::action("A")]);
    /// state.activate_for_test(0);
    /// assert_eq!(state.take_activated(), Some(vec![0]));
    /// assert_eq!(state.take_activated(), None);
    /// ```
    pub fn take_activated(&mut self) -> Option<MenuPath> {
        self.activated.take()
    }

    /// Whether an activation is pending (undrained). The owner uses
    /// this to close the stack while leaving the path for the app.
    pub fn has_activated(&self) -> bool {
        self.activated.is_some()
    }

    /// Test/support helper: activates row `row` of the root menu.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{MenuItem, MenuState};
    ///
    /// let mut state = MenuState::new(vec![MenuItem::checkable("C", false)]);
    /// state.activate_for_test(0);
    /// assert!(matches!(state.items()[0], MenuItem::Checkable { checked: true, .. }));
    /// ```
    pub fn activate_for_test(&mut self, row: usize) {
        self.activate(0, row);
    }

    /// Stores a `Menu`'s per-row submenu anchors (window space) for
    /// its depth — the owner opens deeper entries off these.
    fn record_row_anchors(&mut self, depth: usize, anchors: Vec<OverlayAnchor>) {
        if self.row_anchors.len() <= depth {
            self.row_anchors.resize_with(depth + 1, Vec::new);
        }
        self.row_anchors[depth] = anchors;
    }

    /// The anchor that places row `row`'s submenu, if the depth-`d`
    /// menu has laid out.
    fn row_anchor(&self, depth: usize, row: usize) -> Option<OverlayAnchor> {
        self.row_anchors
            .get(depth)
            .and_then(|rows| rows.get(row))
            .cloned()
    }

    /// Shared keyboard handling for a menu level — called by the
    /// topmost `Menu` popup (the overlay offers it keys first) and by
    /// owners as a fallback for embedded use without overlay routing.
    /// `depth` is the menu level the key targets; `ArrowLeft` is only
    /// consumed above the root so the root can fall through to a
    /// `MenuBar`'s menu switching.
    fn key(&mut self, key: &str, depth: usize) -> EventResponse {
        match key {
            "ArrowDown" => {
                self.move_highlight(depth, 1);
                EventResponse::RequestRepaint
            }
            "ArrowUp" => {
                self.move_highlight(depth, -1);
                EventResponse::RequestRepaint
            }
            "Home" => {
                self.highlight_edge(depth, true);
                EventResponse::RequestRepaint
            }
            "End" => {
                self.highlight_edge(depth, false);
                EventResponse::RequestRepaint
            }
            "ArrowRight" => {
                let submenu = self.highlighted(depth).is_some_and(|row| {
                    self.items_at(depth)
                        .and_then(|items| items.get(row))
                        .is_some_and(MenuItem::is_submenu)
                });
                if submenu {
                    let row = self.highlighted(depth).unwrap_or(0);
                    self.open_submenu(depth, row, true);
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            "ArrowLeft" => {
                if depth > 0 && depth <= self.open_path.len() {
                    self.back_out(depth);
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            "Enter" | " " | "Space" => {
                match self.highlighted(depth) {
                    Some(row) => {
                        let is_sub = self
                            .items_at(depth)
                            .and_then(|items| items.get(row))
                            .is_some_and(MenuItem::is_submenu);
                        if is_sub {
                            self.open_submenu(depth, row, true);
                            EventResponse::RequestRepaint
                        } else {
                            self.activate(depth, row);
                            EventResponse::Handled
                        }
                    }
                    None => EventResponse::Ignored,
                }
            }
            k if k.chars().count() == 1 => {
                let c = k.chars().next().unwrap_or_default();
                if c.is_ascii_alphanumeric() && self.typeahead_select(depth, c) {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }
}

/// Walks `path` ancestors into the item tree, returning the item list
/// containing the final index.
fn parent_items_mut<'a>(
    mut items: &'a mut Vec<MenuItem>,
    path: &[usize],
) -> Option<&'a mut Vec<MenuItem>> {
    for &row in path {
        items = match items.get_mut(row) {
            Some(MenuItem::Submenu { items: sub, .. }) => sub,
            _ => return None,
        };
    }
    Some(items)
}

/// The logical height of an item row in points.
fn row_height_pt(item: &MenuItem) -> f32 {
    match item {
        MenuItem::Separator => SEPARATOR_H,
        MenuItem::Heading { .. } => HEADING_H,
        _ => ITEM_H,
    }
}

/// One row inside a [`Menu`] — a stateless view over the shared
/// [`MenuState`], emitted as the appropriate `MenuItem*` role.
struct MenuRow {
    /// Index into the depth-`d` item list.
    index: usize,
    /// The menu level this row belongs to (`0` = root).
    depth: usize,
    /// Shared state with the owning widget and sibling popups.
    shared: Arc<Mutex<MenuState>>,
    /// Shared shaped-text painter.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl MenuRow {
    /// The item this row views (resolved through `open_path`).
    fn item(&self) -> Option<MenuItem> {
        self.shared
            .lock()
            .expect("menu state poisoned")
            .items_at(self.depth)
            .and_then(|items| items.get(self.index))
            .cloned()
    }

    /// Logical row height at the current item kind.
    fn height_pt(&self) -> f32 {
        self.item().as_ref().map_or(ITEM_H, row_height_pt)
    }

    /// Activates this row or opens its submenu through shared state.
    fn press(&self) -> EventResponse {
        let mut state = self.shared.lock().expect("menu state poisoned");
        let is_sub = state
            .items_at(self.depth)
            .and_then(|items| items.get(self.index))
            .is_some_and(MenuItem::is_submenu);
        if is_sub {
            state.open_submenu(self.depth, self.index, false);
            EventResponse::RequestRepaint
        } else {
            state.activate(self.depth, self.index);
            EventResponse::Handled
        }
    }
}

impl Widget for MenuRow {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        Vec2::new(
            constraints.max_size.x.max(0.0),
            cx.pt(self.height_pt()).min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, _cx: &mut LayoutContext, _bounds: Rect) {}

    fn accessibility(&self, node: &mut AccessKitNode) {
        let state = self.shared.lock().expect("menu state poisoned");
        let Some(item) = state
            .items_at(self.depth)
            .and_then(|items| items.get(self.index))
        else {
            node.set_role(accesskit::Role::GenericContainer);
            return;
        };
        let len = state.items_at(self.depth).map_or(0, |items| items.len());
        match item {
            MenuItem::Action { label, enabled, .. } => {
                node.set_role(accesskit::Role::MenuItem);
                node.set_label(label.as_str());
                if !enabled {
                    node.set_disabled();
                } else {
                    node.add_action(accesskit::Action::Click);
                }
            }
            MenuItem::Checkable { label, checked } => {
                node.set_role(accesskit::Role::MenuItemCheckBox);
                node.set_label(label.as_str());
                node.set_toggled(if *checked {
                    accesskit::Toggled::True
                } else {
                    accesskit::Toggled::False
                });
                node.add_action(accesskit::Action::Click);
            }
            MenuItem::Radio { label, checked, .. } => {
                node.set_role(accesskit::Role::MenuItemRadio);
                node.set_label(label.as_str());
                node.set_toggled(if *checked {
                    accesskit::Toggled::True
                } else {
                    accesskit::Toggled::False
                });
                node.add_action(accesskit::Action::Click);
            }
            MenuItem::Separator => {
                // accesskit has no separator role — the row is
                // presentational chrome between item groups.
                node.set_role(accesskit::Role::GenericContainer);
            }
            MenuItem::Submenu { label, .. } => {
                node.set_role(accesskit::Role::MenuItem);
                node.set_label(label.as_str());
                node.set_has_popup(accesskit::HasPopup::Menu);
                node.set_expanded(
                    state.open_path.get(self.depth) == Some(&self.index),
                );
                node.add_action(accesskit::Action::Click);
                node.add_action(accesskit::Action::Expand);
                node.add_action(accesskit::Action::Collapse);
            }
            MenuItem::Heading { label } => {
                node.set_role(accesskit::Role::Label);
                node.set_label(label.as_str());
            }
        }
        if item.is_selectable() {
            node.set_position_in_set(self.index + 1);
            node.set_size_of_set(len);
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { .. } => {
                let mut state = self.shared.lock().expect("menu state poisoned");
                let item = state
                    .items_at(self.depth)
                    .and_then(|items| items.get(self.index))
                    .cloned();
                let selectable = item.as_ref().is_some_and(MenuItem::is_selectable);
                let mut dirty = false;
                if selectable {
                    if state.highlighted(self.depth) != Some(self.index) {
                        state.set_highlight(self.depth, Some(self.index));
                        dirty = true;
                    }
                    let is_sub = item.as_ref().is_some_and(MenuItem::is_submenu);
                    if is_sub {
                        // Hovering a submenu row opens it; hovering any
                        // other row closes whatever was deeper.
                        if state.open_path.get(self.depth) != Some(&self.index) {
                            state.open_submenu(self.depth, self.index, false);
                            dirty = true;
                        }
                    } else if state.open_path.len() > self.depth {
                        state.close_deeper(self.depth);
                        dirty = true;
                    }
                } else {
                    // Non-selectable rows clear the highlight and
                    // collapse deeper menus, like native menus.
                    if state.highlighted(self.depth).is_some() {
                        state.set_highlight(self.depth, None);
                        dirty = true;
                    }
                    if state.open_path.len() > self.depth {
                        state.close_deeper(self.depth);
                        dirty = true;
                    }
                }
                if dirty {
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Handled
                }
            }
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                ..
            } => {
                let selectable = self
                    .item()
                    .as_ref()
                    .is_some_and(MenuItem::is_selectable);
                if selectable {
                    self.press()
                } else {
                    EventResponse::Handled
                }
            }
            WidgetEvent::SemanticAction(SemanticAction::Click) => {
                let selectable = self
                    .item()
                    .as_ref()
                    .is_some_and(MenuItem::is_selectable);
                if selectable {
                    self.press()
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::SemanticAction(SemanticAction::Expand) => {
                let is_sub = self.item().as_ref().is_some_and(MenuItem::is_submenu);
                if is_sub {
                    self.shared
                        .lock()
                        .expect("menu state poisoned")
                        .open_submenu(self.depth, self.index, true);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::SemanticAction(SemanticAction::Collapse) => {
                let mut state = self.shared.lock().expect("menu state poisoned");
                if state.open_path.get(self.depth) == Some(&self.index) {
                    state.close_deeper(self.depth);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            WidgetEvent::SemanticAction(SemanticAction::Focus) => {
                self.shared
                    .lock()
                    .expect("menu state poisoned")
                    .set_highlight(self.depth, Some(self.index));
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let state = self.shared.lock().expect("menu state poisoned");
        let Some(item) = state
            .items_at(self.depth)
            .and_then(|items| items.get(self.index))
            .cloned()
        else {
            return;
        };
        let highlighted = state.highlighted(self.depth) == Some(self.index);
        drop(state);
        let b = cx.bounds;
        match &item {
            MenuItem::Separator => {
                let y = f64::from(b.min_y() + b.height() / 2.0);
                cx.list.push_fill_rect(
                    kurbo::Rect::new(
                        f64::from(b.min_x() + cx.pt(GUTTER_W)),
                        y - cx.ptf(0.5),
                        f64::from(b.max_x() - cx.pt(8.0)),
                        y + cx.ptf(0.5),
                    ),
                    cx.color(TokenKey::DividerColor, SEPARATOR_INK),
                );
                return;
            }
            MenuItem::Heading { label } => {
                let font_px = cx.pt(MUTED_PT);
                let x = b.min_x() + cx.pt(8.0);
                crate::text_paint::paint_label_clipped(
                    crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                    cx.list,
                    kurbo::Rect::new(
                        f64::from(x),
                        f64::from(b.min_y()),
                        f64::from(b.max_x() - cx.pt(6.0)),
                        f64::from(b.max_y()),
                    ),
                    kurbo::Point::new(
                        f64::from(x),
                        f64::from(b.min_y() + (b.height() - font_px) / 2.0),
                    ),
                    label.as_str(),
                    font_px,
                    cx.color(TokenKey::TextMutedColor, INK_MUTED),
                );
                return;
            }
            _ => {}
        }
        if highlighted {
            let row = kurbo::Rect::new(
                f64::from(b.min_x() + cx.pt(ROW_INSET)),
                f64::from(b.min_y() + cx.pt(1.0)),
                f64::from(b.max_x() - cx.pt(ROW_INSET)),
                f64::from(b.max_y() - cx.pt(1.0)),
            );
            cx.list.push_fill_shape(
                row,
                &Shape::rounded(cx.dim(TokenKey::BorderRadiusSmall, 3.0)),
                cx.color(TokenKey::AccentColor, HIGHLIGHT_BG),
            );
        }
        let font_px = cx.pt(FONT_PT);
        let text_y = b.min_y() + (b.height() - font_px) / 2.0;
        let enabled = item.is_selectable();
        let ink = if highlighted {
            cx.color(TokenKey::TextInverseColor, HIGHLIGHT_INK)
        } else if enabled {
            cx.color(TokenKey::TextColor, INK)
        } else {
            cx.color(TokenKey::TextMutedColor, INK_DISABLED)
        };
        // Gutter glyphs: ✓ for a checked item, • for a selected radio.
        let glyph = match &item {
            MenuItem::Checkable { checked: true, .. } => Some("✓"),
            MenuItem::Radio { checked: true, .. } => Some("•"),
            _ => None,
        };
        if let Some(glyph) = glyph {
            let glyph_ink = if highlighted {
                ink
            } else {
                cx.color(TokenKey::AccentColor, CHECK)
            };
            crate::text_paint::paint_label(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Point::new(
                    f64::from(b.min_x() + cx.pt(8.0)),
                    f64::from(text_y),
                ),
                glyph,
                font_px,
                glyph_ink,
            );
        }
        // Label, clipped before the suffix zone (shortcut + ▸).
        let has_sub = item.is_submenu();
        let label_x = b.min_x() + cx.pt(GUTTER_W);
        let suffix_w = if has_sub {
            cx.pt(SUBMENU_W)
        } else {
            cx.pt(10.0)
        };
        let shortcut_w = item
            .shortcut()
            .map(|s| s.chars().count() as f32 * cx.pt(6.2) + cx.pt(SHORTCUT_GAP))
            .unwrap_or(0.0);
        let label_right = b.max_x() - suffix_w - shortcut_w;
        crate::text_paint::paint_label_clipped(
            crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
            cx.list,
            kurbo::Rect::new(
                f64::from(label_x),
                f64::from(b.min_y()),
                f64::from(label_right.max(label_x)),
                f64::from(b.max_y()),
            ),
            kurbo::Point::new(f64::from(label_x), f64::from(text_y)),
            item.label(),
            font_px,
            ink,
        );
        // Right-aligned muted shortcut.
        if let Some(shortcut) = item.shortcut() {
            let sc_px = cx.pt(MUTED_PT);
            let sc_w = shortcut.chars().count() as f32 * cx.pt(6.2);
            let sc_x = (b.max_x() - suffix_w - sc_w).max(label_x);
            let sc_ink = if highlighted {
                ink
            } else {
                cx.color(TokenKey::TextMutedColor, INK_MUTED)
            };
            crate::text_paint::paint_label_clipped(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Rect::new(
                    f64::from(sc_x),
                    f64::from(b.min_y()),
                    f64::from(b.max_x() - suffix_w),
                    f64::from(b.max_y()),
                ),
                kurbo::Point::new(
                    f64::from(sc_x),
                    f64::from(b.min_y() + (b.height() - sc_px) / 2.0),
                ),
                shortcut,
                sc_px,
                sc_ink,
            );
        }
        // ▸ submenu glyph in the reserved suffix strip.
        if has_sub {
            let sub_px = cx.pt(MUTED_PT);
            let x = b.max_x() - cx.pt(SUBMENU_W - 6.0);
            crate::text_paint::paint_label(
                crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter),
                cx.list,
                kurbo::Point::new(
                    f64::from(x),
                    f64::from(b.min_y() + (b.height() - sub_px) / 2.0),
                ),
                "▸",
                sub_px,
                ink,
            );
        }
    }
}

/// Vertical column of menu rows — the [`ScrollView`] content inside a
/// [`Menu`] popup. Records per-row submenu anchors into shared state
/// during layout so the owner can place deeper popups.
struct MenuColumn {
    /// Rows in order.
    items: Vec<MenuRow>,
    /// The menu level (`0` = root).
    depth: usize,
    /// Shared state — receives `row_anchors` at layout.
    shared: Arc<Mutex<MenuState>>,
    /// Shared shaped-text painter handed to rebuilt rows.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
    /// Row bounds from the last layout pass.
    row_bounds: Vec<Rect>,
}

impl MenuColumn {
    /// Rebuilds `items` to match the shared item list at this depth —
    /// called from `measure`/`layout` so a swapped item tree (a
    /// `MenuBar` switching menus in a live popup) shows the new rows.
    fn sync_rows(&mut self) {
        let count = self
            .shared
            .lock()
            .expect("menu state poisoned")
            .items_at(self.depth)
            .map_or(0, |items| items.len());
        if count == self.items.len() {
            return;
        }
        self.items = (0..count)
            .map(|index| MenuRow {
                index,
                depth: self.depth,
                shared: Arc::clone(&self.shared),
                text_painter: self.text_painter.clone(),
            })
            .collect();
        self.row_bounds.clear();
    }
}

impl Widget for MenuColumn {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        self.sync_rows();
        let mut h = cx.pt(POPUP_PAD_Y);
        let mut w = 0.0f32;
        for row in &mut self.items {
            let s = row.measure(cx, constraints);
            h += s.y;
            w = w.max(s.x);
        }
        h += cx.pt(POPUP_PAD_Y);
        Vec2::new(
            w.min(constraints.max_size.x.max(0.0)),
            h.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.sync_rows();
        self.row_bounds.clear();
        let mut anchors = Vec::with_capacity(self.items.len());
        let mut y = bounds.min_y() + cx.pt(POPUP_PAD_Y);
        for row in &mut self.items {
            let h = cx.pt(row.height_pt());
            let rect = Rect::new(bounds.min_x(), y, bounds.width(), h);
            self.row_bounds.push(rect);
            cx.layout_child(row, rect);
            // A `Pointer` anchor lands `POINTER_OFFSET` pt below-right
            // of its point — back the offset out so the submenu's
            // top-left lands on the row's top-right corner.
            anchors.push(OverlayAnchor::Pointer(Vec2::new(
                rect.max_x() - cx.pt(POINTER_OFFSET_PT),
                rect.min_y() - cx.pt(POINTER_OFFSET_PT),
            )));
            y += h;
        }
        self.shared
            .lock()
            .expect("menu state poisoned")
            .record_row_anchors(self.depth, anchors);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::GenericContainer);
    }

    fn child_count(&self) -> usize {
        self.items.len()
    }

    fn child(&self, index: usize) -> Option<&dyn Widget> {
        self.items.get(index).map(|i| i as &dyn Widget)
    }

    fn child_mut(&mut self, index: usize) -> Option<&mut dyn Widget> {
        self.items.get_mut(index).map(|i| i as &mut dyn Widget)
    }

    fn child_bounds(&self, index: usize) -> Option<Rect> {
        self.row_bounds.get(index).copied()
    }
}

/// A vertical menu of [`MenuItem`]s — the popup surface opened by
/// [`MenuBar`](crate::widgets::menu_bar::MenuBar) and
/// [`ContextMenu`](crate::widgets::context_menu::ContextMenu), usable
/// standalone as an inline item list.
///
/// Emitted as `Role::Menu` of `MenuItem`/`MenuItemCheckBox`/
/// `MenuItemRadio` rows. All state lives in the shared [`MenuState`]
/// so popups stay stateless views and the owner can drive keyboard
/// navigation, submenu stacking, and activation through
/// `sync_overlay`.
///
/// # Examples
///
/// ```
/// use martensite::widgets::{Menu, MenuItem};
///
/// let mut menu = Menu::new([MenuItem::action("A"), MenuItem::action("B")]);
/// assert_eq!(menu.item_count(), 2);
/// ```
pub struct Menu {
    /// Scrolling row list (internal child 0) — owns the `MenuColumn`.
    scroll: ScrollView,
    /// Shared state with the owning widget.
    shared: Arc<Mutex<MenuState>>,
    /// The menu level this surface renders (`0` = root).
    depth: usize,
    /// Popup bounds from the last layout pass.
    bounds: Option<Rect>,
    /// The silhouette painted last frame — the single source of truth
    /// for `clip_shape`/`hit_shape`.
    painted_shape: Mutex<Shape>,
}

impl Menu {
    /// Creates a menu over `items` (the root level of a new shared
    /// state). Owners that drive several stacked menus construct the
    /// deeper levels through [`Menu::with_shared`].
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Menu, MenuItem};
    ///
    /// let menu = Menu::new([MenuItem::action("Open"), MenuItem::separator()]);
    /// assert_eq!(menu.item_count(), 2);
    /// ```
    pub fn new(items: impl Into<Vec<MenuItem>>) -> Self {
        Self::with_shared(
            Arc::new(Mutex::new(MenuState::new(items.into()))),
            0,
            None,
        )
    }

    /// Creates a menu level over an existing shared state — `depth`
    /// selects which item list the surface renders through
    /// `open_path` (`0` is the root menu).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::{Arc, Mutex};
    /// use martensite::widgets::{Menu, MenuItem, MenuState};
    ///
    /// let shared = Arc::new(Mutex::new(MenuState::new(vec![MenuItem::action("A")])));
    /// let menu = Menu::with_shared(shared, 0, None);
    /// assert_eq!(menu.item_count(), 1);
    /// ```
    pub fn with_shared(
        shared: Arc<Mutex<MenuState>>,
        depth: usize,
        text_painter: Option<crate::text_paint::SharedTextPainter>,
    ) -> Self {
        let column = MenuColumn {
            items: Vec::new(),
            depth,
            shared: Arc::clone(&shared),
            text_painter,
            row_bounds: Vec::new(),
        };
        Self {
            scroll: ScrollView::new(column),
            shared,
            depth,
            bounds: None,
            painted_shape: Mutex::new(Shape::RECT),
        }
    }

    /// The shared state this menu views.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Menu, MenuItem};
    ///
    /// let menu = Menu::new([MenuItem::action("A")]);
    /// assert_eq!(menu.shared().lock().unwrap().items().len(), 1);
    /// ```
    pub fn shared(&self) -> Arc<Mutex<MenuState>> {
        Arc::clone(&self.shared)
    }

    /// The number of rows at this menu's depth.
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Menu, MenuItem};
    ///
    /// assert_eq!(Menu::new([MenuItem::action("A")]).item_count(), 1);
    /// ```
    pub fn item_count(&self) -> usize {
        self.shared
            .lock()
            .expect("menu state poisoned")
            .items_at(self.depth)
            .map_or(0, |items| items.len())
    }

    /// Drains the pending activation path written by a row
    /// (click, `Enter`, or AT `Click`).
    ///
    /// # Examples
    ///
    /// ```
    /// use martensite::widgets::{Menu, MenuItem};
    ///
    /// let mut menu = Menu::new([MenuItem::action("A")]);
    /// assert_eq!(menu.take_activated(), None);
    /// ```
    pub fn take_activated(&mut self) -> Option<MenuPath> {
        self.shared
            .lock()
            .expect("menu state poisoned")
            .take_activated()
    }
}

impl Widget for Menu {
    fn debug_name(&self) -> &'static str {
        "Menu"
    }

    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let mut s = self.scroll.measure(cx, constraints);
        s.y = s.y.min(cx.pt(MAX_POPUP_H) + cx.pt(2.0));
        // Width: the widest label plus its shortcut and the row chrome.
        let state = self.shared.lock().expect("menu state poisoned");
        let widest = state
            .items_at(self.depth)
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        let label = item.label().chars().count() as f32;
                        let shortcut =
                            item.shortcut().map_or(0.0, |s| s.chars().count() as f32 * 0.9);
                        label + shortcut
                    })
                    .fold(0.0f32, f32::max)
            })
            .unwrap_or(0.0);
        drop(state);
        s.x = (widest * cx.pt(7.0)
            + cx.pt(GUTTER_W + SUBMENU_W + SHORTCUT_GAP + 2.0 * ROW_INSET))
        .clamp(
            cx.pt(MIN_POPUP_W).min(constraints.max_size.x.max(0.0)),
            constraints.max_size.x.max(0.0),
        );
        s
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = Some(bounds);
        cx.layout_child(&mut self.scroll, bounds);
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Menu);
        node.set_orientation(accesskit::Orientation::Vertical);
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        // Keyboard on the popup itself: the overlay offers non-Escape
        // keys to the topmost entry first, so the deepest open menu
        // gets navigation — matching focus-in-owner semantics without
        // the owner knowing popup internals.
        if let WidgetEvent::KeyPressed { key, repeat } = cx.event {
            // Held-down Enter must not retrigger activation.
            if key.as_str() == "Enter" && *repeat {
                return EventResponse::Handled;
            }
            let response = self
                .shared
                .lock()
                .expect("menu state poisoned")
                .key(key, self.depth);
            if response != EventResponse::Ignored {
                return response;
            }
        }
        // Forward to the scroll view / rows.
        self.scroll.event(cx)
    }

    fn paint(&self, cx: &mut PaintContext) {
        let b = cx.bounds;
        let rect = kurbo::Rect::new(
            f64::from(b.min_x()),
            f64::from(b.min_y()),
            f64::from(b.max_x()),
            f64::from(b.max_y()),
        );
        let popup_shape = Shape::rounded(cx.dim(TokenKey::BorderRadius, 6.0));
        *self
            .painted_shape
            .lock()
            .expect("popup shape poisoned") = popup_shape.clone();
        cx.list.push_fill_shape(
            rect,
            &popup_shape,
            cx.color(TokenKey::SurfaceColor, POPUP_BG),
        );
        cx.list.push_stroke_shape(
            rect,
            &popup_shape,
            cx.pt(1.0),
            cx.color(TokenKey::BorderColor, POPUP_BORDER),
        );
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn clip_shape(&self) -> Option<Shape> {
        Some(
            self.painted_shape
                .lock()
                .expect("popup shape poisoned")
                .clone(),
        )
    }

    fn hit_shape(&self) -> Option<Shape> {
        Some(
            self.painted_shape
                .lock()
                .expect("popup shape poisoned")
                .clone(),
        )
    }

    fn child_count(&self) -> usize {
        1
    }

    fn child(&self, _index: usize) -> Option<&dyn Widget> {
        Some(&self.scroll)
    }

    fn child_mut(&mut self, _index: usize) -> Option<&mut dyn Widget> {
        Some(&mut self.scroll)
    }

    fn child_bounds(&self, _index: usize) -> Option<Rect> {
        self.bounds
    }
}

/// Owner-side controller for a stack of [`Menu`] popups — one root
/// entry plus one entry per open submenu level, reconciled against
/// the shared [`MenuState`]'s `open_path` each `sync_overlay`.
///
/// `pub(crate)`: [`MenuBar`](crate::widgets::menu_bar::MenuBar) and
/// [`ContextMenu`](crate::widgets::context_menu::ContextMenu) embed
/// this to get the whole submenu/dismissal/activation contract.
pub(crate) struct MenuStack {
    /// State shared with every popup level.
    shared: Arc<Mutex<MenuState>>,
    /// Overlay entry ids per depth — `popup_ids[0]` is the root.
    popup_ids: Vec<u64>,
    /// The anchors each live entry was opened/re-anchored with
    /// (parallel to `popup_ids`), so a settled stack doesn't re-mark
    /// layout every tick.
    last_anchors: Vec<OverlayAnchor>,
    /// Whether the menu stack is logically open.
    open: bool,
    /// The anchor the root popup opens at (button bounds, pointer).
    root_anchor: Option<OverlayAnchor>,
    /// Shared shaped-text painter for popup content.
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl MenuStack {
    /// Creates a stack over `shared`.
    pub(crate) fn new(
        shared: Arc<Mutex<MenuState>>,
        text_painter: Option<crate::text_paint::SharedTextPainter>,
    ) -> Self {
        Self {
            shared,
            popup_ids: Vec::new(),
            last_anchors: Vec::new(),
            open: false,
            root_anchor: None,
            text_painter,
        }
    }

    /// The shared state (for owners that drain/inspect it).
    pub(crate) fn shared(&self) -> Arc<Mutex<MenuState>> {
        Arc::clone(&self.shared)
    }

    /// Whether the menu stack is logically open.
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// The root entry's overlay id, if open.
    pub(crate) fn root_id(&self) -> Option<u64> {
        self.popup_ids.first().copied()
    }

    /// Opens the stack at `anchor` — `via_keyboard` seeds the root
    /// highlight at the first selectable row.
    pub(crate) fn open_at(&mut self, anchor: OverlayAnchor, via_keyboard: bool) {
        {
            let mut state = self.shared.lock().expect("menu state poisoned");
            state.reset_navigation();
            if via_keyboard {
                state.highlight_edge(0, true);
            }
        }
        self.root_anchor = Some(anchor);
        self.open = true;
    }

    /// Replaces the live item tree and re-anchors the root — a
    /// `MenuBar` switching between its menus while open.
    /// `via_keyboard` seeds the new root highlight.
    pub(crate) fn switch_items(
        &mut self,
        items: Vec<MenuItem>,
        anchor: OverlayAnchor,
        via_keyboard: bool,
    ) {
        {
            let mut state = self.shared.lock().expect("menu state poisoned");
            state.set_items(items);
            if via_keyboard {
                state.highlight_edge(0, true);
            }
        }
        self.root_anchor = Some(anchor);
        self.open = true;
    }

    /// Closes the whole stack logically; `sync` drops the entries.
    pub(crate) fn close(&mut self) {
        self.open = false;
    }

    /// Drains a pending activation path (closing the stack).
    pub(crate) fn take_activated(&mut self) -> Option<MenuPath> {
        self.shared
            .lock()
            .expect("menu state poisoned")
            .take_activated()
    }

    /// Forwards a key to the deepest open menu level — the fallback
    /// path for owners embedded without arena overlay routing.
    pub(crate) fn key(&mut self, key: &str) -> EventResponse {
        let depth = self.shared.lock().expect("menu state poisoned").deepest();
        self.shared
            .lock()
            .expect("menu state poisoned")
            .key(key, depth)
    }

    /// Reconciles overlay entries with the logical state: opens the
    /// root while open, grows/shrinks submenu entries to match
    /// `open_path`, re-anchors entries whose rows moved, notices
    /// layer-initiated dismissal (outside press, `Escape`), and closes
    /// the stack when an activation is pending.
    pub(crate) fn sync(&mut self, overlay: &mut OverlayLayer) {
        // A row activated (pointer or AT) — the stack closes; the
        // path stays in shared state for the app to drain.
        if self
            .shared
            .lock()
            .expect("menu state poisoned")
            .has_activated()
        {
            self.open = false;
        }
        // Entries the layer killed (Escape pops the topmost = deepest
        // menu; an outside press kills them all).
        let had_root = !self.popup_ids.is_empty();
        while let Some(&id) = self.popup_ids.last() {
            if overlay.is_open(id) {
                break;
            }
            self.popup_ids.pop();
            self.last_anchors.pop();
            let mut state = self.shared.lock().expect("menu state poisoned");
            state.open_path.truncate(self.popup_ids.len().saturating_sub(1));
            state.highlight.truncate(self.popup_ids.len() + 1);
        }
        // The root itself is gone — the layer dismissed the whole
        // stack (outside press, or Escape with no submenu open).
        if had_root && self.popup_ids.is_empty() {
            self.open = false;
            self.root_anchor = None;
            let mut state = self.shared.lock().expect("menu state poisoned");
            state.open_path.clear();
            state.highlight.truncate(1);
        }
        if self.popup_ids.is_empty() {
            if !self.open {
                self.root_anchor = None;
                return;
            }
            // Root entry.
            let Some(anchor) = self.root_anchor.clone() else {
                self.open = false;
                return;
            };
            let menu = Menu::with_shared(
                Arc::clone(&self.shared),
                0,
                self.text_painter.clone(),
            );
            self.popup_ids
                .push(overlay.open(Box::new(menu), anchor.clone()));
            self.last_anchors.push(anchor);
        }
        if !self.open {
            for id in self.popup_ids.drain(..) {
                overlay.close(id);
            }
            self.last_anchors.clear();
            self.shared
                .lock()
                .expect("menu state poisoned")
                .open_path
                .clear();
            return;
        }
        // Grow/shrink deeper entries to match `open_path`.
        let desired = 1 + self
            .shared
            .lock()
            .expect("menu state poisoned")
            .open_path
            .len();
        while self.popup_ids.len() > desired {
            if let Some(id) = self.popup_ids.pop() {
                overlay.close(id);
            }
            self.last_anchors.pop();
        }
        while self.popup_ids.len() < desired {
            let d = self.popup_ids.len();
            let anchor = {
                let state = self.shared.lock().expect("menu state poisoned");
                state
                    .open_path
                    .get(d - 1)
                    .and_then(|&row| state.row_anchor(d - 1, row))
            }
            .unwrap_or(OverlayAnchor::Pointer(Vec2::ZERO));
            let menu = Menu::with_shared(
                Arc::clone(&self.shared),
                d,
                self.text_painter.clone(),
            );
            self.popup_ids
                .push(overlay.open(Box::new(menu), anchor.clone()));
            self.last_anchors.push(anchor);
        }
        // Re-anchor entries whose target moved (bar switch, parent
        // scroll, first-layout fallback).
        for (d, &id) in self.popup_ids.iter().enumerate() {
            let anchor = if d == 0 {
                self.root_anchor.clone()
            } else {
                let state = self.shared.lock().expect("menu state poisoned");
                state
                    .open_path
                    .get(d - 1)
                    .and_then(|&row| state.row_anchor(d - 1, row))
            };
            if let Some(anchor) = anchor {
                if self.last_anchors.get(d) != Some(&anchor) {
                    overlay.set_anchor(id, anchor.clone());
                    self.last_anchors[d] = anchor;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::HotNode;

    fn items() -> Vec<MenuItem> {
        vec![
            MenuItem::heading("File"),
            MenuItem::action("New").with_shortcut("Ctrl+N"),
            MenuItem::action("Open"),
            MenuItem::separator(),
            MenuItem::checkable("Wrap", false),
            MenuItem::radio("Left", "align", true),
            MenuItem::radio("Right", "align", false),
            MenuItem::action("Disabled").enabled(false),
            MenuItem::submenu(
                "Recent",
                vec![MenuItem::action("a.txt"), MenuItem::action("b.txt")],
            ),
        ]
    }

    fn state() -> Arc<Mutex<MenuState>> {
        Arc::new(Mutex::new(MenuState::new(items())))
    }

    fn overlay() -> OverlayLayer {
        let mut o = OverlayLayer::new();
        o.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0));
        o
    }

    #[test]
    fn item_model() {
        let item = MenuItem::action("Save").with_shortcut("Ctrl+S");
        assert_eq!(item.label(), "Save");
        assert_eq!(item.shortcut(), Some("Ctrl+S"));
        assert!(item.is_selectable());
        assert!(!MenuItem::action("X").enabled(false).is_selectable());
        assert!(MenuItem::submenu("S", vec![]).is_submenu());
        assert!(!MenuItem::heading("H").is_selectable());
    }

    #[test]
    fn nav_skips_inert_rows() {
        let mut st = MenuState::new(items());
        // Down from nothing → first selectable (row 1, "New").
        st.move_highlight(0, 1);
        assert_eq!(st.highlighted(0), Some(1));
        st.move_highlight(0, 1); // Open
        assert_eq!(st.highlighted(0), Some(2));
        st.move_highlight(0, 1); // skips separator → Wrap
        assert_eq!(st.highlighted(0), Some(4));
        st.move_highlight(0, 4); // clamps at last selectable (submenu, 8)
        assert_eq!(st.highlighted(0), Some(8));
        st.move_highlight(0, -1); // back over disabled → Right radio (6)
        assert_eq!(st.highlighted(0), Some(6));
        st.highlight_edge(0, true);
        assert_eq!(st.highlighted(0), Some(1));
        st.highlight_edge(0, false);
        assert_eq!(st.highlighted(0), Some(8));
    }

    #[test]
    fn activation_path_and_toggle() {
        let mut st = MenuState::new(items());
        st.activate(0, 4); // checkable → toggles
        assert_eq!(st.take_activated(), Some(vec![4]));
        assert!(matches!(
            st.items()[4],
            MenuItem::Checkable { checked: true, .. }
        ));
        // Radio: checking "Right" unchecks "Left".
        st.activate(0, 6);
        assert!(matches!(
            st.items()[6],
            MenuItem::Radio { checked: true, .. }
        ));
        assert!(matches!(
            st.items()[5],
            MenuItem::Radio { checked: false, .. }
        ));
        // Disabled and inert rows never activate.
        st.activate(0, 7);
        st.activate(0, 0);
        assert_eq!(st.take_activated(), Some(vec![6]));
        assert_eq!(st.take_activated(), None);
    }

    #[test]
    fn submenu_open_and_back_out() {
        let mut st = MenuState::new(items());
        st.set_highlight(0, Some(8));
        assert_eq!(st.key("ArrowRight", 0), EventResponse::RequestRepaint);
        assert_eq!(st.open_path(), &[8]);
        // Keyboard open seeds the submenu highlight.
        assert_eq!(st.highlighted(1), Some(0));
        assert_eq!(st.key("ArrowDown", 1), EventResponse::RequestRepaint);
        assert_eq!(st.highlighted(1), Some(1));
        // Enter activates the nested path.
        assert_eq!(st.key("Enter", 1), EventResponse::Handled);
        assert_eq!(st.take_activated(), Some(vec![8, 1]));
        // Re-open and back out with Left.
        st.open_submenu(0, 8, true);
        assert_eq!(st.key("ArrowLeft", 1), EventResponse::RequestRepaint);
        assert!(st.open_path().is_empty());
        assert_eq!(st.highlighted(0), Some(8));
        // Left at root falls through (a MenuBar would switch menus).
        assert_eq!(st.key("ArrowLeft", 0), EventResponse::Ignored);
    }

    #[test]
    fn typeahead_cycles() {
        let mut st = MenuState::new(items());
        assert!(st.typeahead_select(0, 'n'));
        assert_eq!(st.highlighted(0), Some(1)); // "New"
        assert!(st.typeahead_select(0, 'r'));
        // "Recent" after "Right" radio? order: Right(6), Recent(8) → first 'r' after start scans from highlight.
        assert_eq!(st.highlighted(0), Some(6));
        assert!(st.typeahead_select(0, 'r'));
        assert_eq!(st.highlighted(0), Some(8));
    }

    #[test]
    fn stack_opens_and_reconciles() {
        let shared = state();
        let mut stack = MenuStack::new(Arc::clone(&shared), None);
        let mut o = overlay();
        stack.open_at(OverlayAnchor::Bounds(Rect::new(10.0, 10.0, 60.0, 30.0)), false);
        stack.sync(&mut o);
        o.layout_pass();
        assert_eq!(o.len(), 1);
        let b = o.entry_bounds(stack.root_id().unwrap()).unwrap();
        assert!(b.min_y() >= 40.0);
        // Outside press dismisses; sync reconciles.
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(700.0, 500.0),
            button: PointerButton::Primary,
            count: 1,
        };
        assert_eq!(o.dispatch_event(&press), EventResponse::Ignored);
        stack.sync(&mut o);
        assert!(!stack.is_open() || stack.root_id().is_none());
    }

    #[test]
    fn stack_activation_closes() {
        let shared = state();
        let mut stack = MenuStack::new(Arc::clone(&shared), None);
        let mut o = overlay();
        stack.open_at(OverlayAnchor::Pointer(Vec2::new(50.0, 50.0)), false);
        stack.sync(&mut o);
        o.layout_pass();
        // Activate through shared state (as a row would).
        shared.lock().unwrap().activate(0, 1);
        stack.sync(&mut o);
        assert_eq!(stack.take_activated(), Some(vec![1]));
        assert_eq!(o.len(), 0);
    }

    #[test]
    fn menu_accessibility() {
        let menu = Menu::new(items());
        let mut node = AccessKitNode::new(accesskit::Role::Unknown);
        menu.accessibility(&mut node);
        assert_eq!(node.role(), accesskit::Role::Menu);
    }

    #[test]
    fn popup_row_click_activates() {
        let shared = state();
        let mut stack = MenuStack::new(Arc::clone(&shared), None);
        let mut o = overlay();
        stack.open_at(OverlayAnchor::Bounds(Rect::new(10.0, 10.0, 60.0, 30.0)), false);
        stack.sync(&mut o);
        o.layout_pass();
        let id = stack.root_id().unwrap();
        // Path [0, 0, 1]: Menu → ScrollView → MenuColumn → row 1.
        let row = o.widget_at_mut(id, &[0, 0, 1]).expect("menu row");
        let press = WidgetEvent::PointerPressed {
            position: Vec2::new(5.0, 5.0),
            button: PointerButton::Primary,
            count: 1,
        };
        let mut cx = EventContext {
            event: &press,
            bounds: Rect::default(),
            scale: 1.0,
        };
        assert_eq!(row.event(&mut cx), EventResponse::Handled);
        stack.sync(&mut o);
        assert_eq!(stack.take_activated(), Some(vec![1]));
        assert_eq!(o.len(), 0);
    }

    #[test]
    fn submenu_entry_opens_on_row_anchor() {
        let shared = state();
        let mut stack = MenuStack::new(Arc::clone(&shared), None);
        let mut o = overlay();
        stack.open_at(OverlayAnchor::Bounds(Rect::new(10.0, 10.0, 60.0, 30.0)), false);
        stack.sync(&mut o);
        o.layout_pass();
        // Keyboard: highlight the submenu row and open it.
        shared.lock().unwrap().set_highlight(0, Some(8));
        shared.lock().unwrap().open_submenu(0, 8, true);
        stack.sync(&mut o);
        o.layout_pass();
        assert_eq!(o.len(), 2);
        // The submenu sits to the right of the root popup.
        let root_b = o.entry_bounds(stack.popup_ids[0]).unwrap();
        let sub_b = o.entry_bounds(stack.popup_ids[1]).unwrap();
        assert!(sub_b.min_x() >= root_b.max_x() - 1.0);
        assert!(sub_b.min_y() <= root_b.max_y());
    }

    #[test]
    fn menu_layout_records_anchors() {
        let mut menu = Menu::new(items());
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        menu.layout(&mut cx, Rect::new(0.0, 0.0, 200.0, 300.0));
        let state = menu.shared.lock().unwrap();
        assert_eq!(state.row_anchors.len(), 1);
        assert_eq!(state.row_anchors[0].len(), 9);
    }
}
